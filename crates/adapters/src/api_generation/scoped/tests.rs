use super::super::tests as legacy;
use super::*;
use cerulean_domain::currency_budget::{
    scoped::{BillingBasis, ContextMetric, PriceBand},
    ManagementCountPrice, Rates,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

fn client() -> ApiClient {
    ApiClient {
        http: ApiClient::http(false).unwrap(),
        key: legacy::key(),
    }
}
fn request() -> ApiRequest {
    legacy::request(ApiTarget::OpenAi {})
}

#[test]
fn provider_output_limits_preserve_scoped_count_projection_and_exact_reservation() {
    let c = client();
    let legacy = c.prepare_scoped(request()).unwrap();
    let mut r = request();
    r.max_output_tokens = 393216;
    let draft = c.prepare_scoped(r.clone()).unwrap();
    draft.fingerprint.validate().unwrap();
    assert_eq!(draft.prepared.body["max_output_tokens"], 393216);
    assert_ne!(
        draft.fingerprint.request.body_sha256,
        legacy.fingerprint.request.body_sha256
    );
    // Counting covers input; the separate output reservation must bind the new ceiling.
    assert_eq!(
        draft.checked_count_body().unwrap(),
        legacy.checked_count_body().unwrap()
    );
    let counted = counted_request(r, 100);
    assert_eq!(counted.draft.fingerprint.request.max_output_tokens, 393216);
    let prepared = counted.prepare().unwrap();
    assert_eq!(prepared.review.request.output_ceiling, 393216);
    assert!(
        prepared.review.audit.maximum_usd_nanos
            > counted_request(request(), 100)
                .prepare()
                .unwrap()
                .review
                .audit
                .maximum_usd_nanos
    );
}
fn quote() -> ScopedPriceQuote {
    let now = chrono::Utc::now();
    ScopedPriceQuote {
        schema: 1,
        target: ApiTarget::OpenAi {},
        requested_model: "fixture-model".into(),
        requested_service: RequestedService::Explicit {
            tier: "default".into(),
        },
        reported_models: vec!["fixture-model-snapshot".into()],
        reported_services: vec!["default".into(), "priority".into()],
        billing_basis: BillingBasis::PerRequest,
        context_metric: ContextMetric::TotalInputIncludingCache,
        bands: [
            ("default", 0, 10, 9000),
            ("default", 11, 1000, 1000),
            ("priority", 0, 1000, 3000),
        ]
        .into_iter()
        .map(|(service, from, through, output)| PriceBand {
            actual_model: "fixture-model-snapshot".into(),
            actual_service: service.into(),
            input: InputRange { from, through },
            rates: Rates {
                input: 2000,
                output,
                cache_read: Some(100),
                cache_write: Some(3000),
            },
        })
        .collect(),
        count_price: Some(ManagementCountPrice {
            schema: 1,
            maximum_usd_nanos: 17,
            maximum_body_bytes: 256 * 1024,
            source: "Synthetic count fee; no supplier guarantee".into(),
        }),
        valid_from: (now - chrono::Duration::hours(1)).to_rfc3339(),
        valid_until: (now + chrono::Duration::hours(1)).to_rfc3339(),
        source: "synthetic round prices".into(),
    }
}
fn counted_request(r: ApiRequest, n: u64) -> CountedScopedApi {
    let c = client();
    let d = c.prepare_scoped(r).unwrap();
    counted(
        c,
        d,
        quote(),
        json!({"object":"response.input_tokens","input_tokens":n}),
    )
    .unwrap()
}
fn response(tool: bool, id: &str) -> Value {
    let mut v = legacy::reply(&request(), tool);
    v["id"] = json!(id);
    v["service_tier"] = json!("default");
    v["usage"]["input_tokens_details"] = json!({"cached_tokens":3,"cache_write_tokens":2});
    v
}
fn next_request(reply: ApiReply) -> ApiRequest {
    let mut r = request();
    r.messages.push(ApiMessage::Assistant(reply));
    r.messages.push(ApiMessage::Tools(vec![ToolResult {
        call_id: "call-1".into(),
        text: "Original file contents, added only to the second round.".into(),
        is_error: false,
    }]));
    r
}

#[test]
fn scoped_api_round_count_includes_tools_results_and_private_continuation_without_archiving_them() {
    let c = client();
    let old = c.prepare(request()).unwrap();
    let first = c.prepare_scoped(request()).unwrap();
    assert_ne!(
        old.fingerprint.body_sha256,
        first.fingerprint.request.body_sha256
    );
    assert_eq!(first.prepared.body["service_tier"], "default");
    let reply = wire::parse(&request(), response(true, "resp_first")).unwrap();
    let second = c.prepare_scoped(next_request(reply)).unwrap();
    let body = second.checked_count_body().unwrap();
    let text = body.to_string();
    assert!(text.contains("private-continuation") && text.contains("Original file contents"));
    assert_eq!(body["tools"], second.prepared.body["tools"]);
    assert_eq!(body["input"], second.prepared.body["input"]);
    assert_ne!(
        first.fingerprint.count_body_sha256,
        second.fingerprint.count_body_sha256
    );
    assert!(body.get("include").is_none() && body.get("service_tier").is_none());
    let proof = counted(
        c,
        second,
        quote(),
        json!({"object":"response.input_tokens","input_tokens":300}),
    )
    .unwrap();
    assert!(proof
        .evidence
        .validate(first.fingerprint(), &digest(&quote()).unwrap())
        .is_err());
    let saved = serde_json::to_string(proof.evidence()).unwrap();
    assert!(
        !saved.contains("private-continuation")
            && !saved.contains("Original file contents")
            && !saved.contains("sk-synthetic")
    );
    assert!(
        serde_json::from_str::<crate::openai::scoped_pricing::ScopedTokenCount>(&saved).is_err()
    );
    let p = proof.prepare().unwrap();
    assert_eq!(
        p.pricing_request().input,
        InputRange {
            from: 0,
            through: 300
        }
    );
    let mut point = p.pricing_request().clone();
    point.input.from = 300;
    assert!(
        p.audit().maximum_usd_nanos > p.quote().reserve(&point, &p.audit().reviewed_at).unwrap()
    );
}

#[test]
fn scoped_api_count_receipt_cannot_be_rebound_and_expired_counts_cannot_send() {
    let p = counted_request(request(), 100);
    let q = digest(&p.quote).unwrap();
    for mode in 0..6 {
        let mut e = p.evidence.clone();
        match mode {
            0 => e.input_tokens += 1,
            1 => e.request.request.body_sha256 = "b".repeat(64),
            2 => e.quote_sha256 = "b".repeat(64),
            3 => e.observed_unix_ms = 0,
            4 => e.request.requested_service = RequestedService::Omitted,
            _ => e.request.count_body_sha256 = "b".repeat(64),
        }
        assert!(e.validate(p.fingerprint(), &q).is_err());
    }
    let mut p = p.prepare().unwrap();
    p.check_ready().unwrap();
    p.counted.observed = Instant::now() - MAX_AGE;
    assert!(p.check_ready().is_err());
    let mut p = counted_request(request(), 100).prepare().unwrap();
    p.counted.draft.prepared.body["input"] = json!([]);
    assert!(p.check_ready().is_err());
    let mut p = counted_request(request(), 100).prepare().unwrap();
    p.counted.quote.valid_until = (chrono::Utc::now() - chrono::Duration::seconds(1)).to_rfc3339();
    assert!(p.check_ready().is_err());
}

#[test]
fn scoped_api_count_rejects_malformed_counts_and_retains_receipt_when_price_has_a_gap() {
    for value in [
        json!(0),
        json!(-1),
        json!("100"),
        json!(1.2),
        json!(MAX_TOKENS + 1),
        Value::Null,
    ] {
        let c = client();
        let d = c.prepare_scoped(request()).unwrap();
        assert!(counted(
            c,
            d,
            quote(),
            json!({"object":"response.input_tokens","input_tokens":value})
        )
        .is_err());
    }
    for value in [
        json!({"object":"other","input_tokens":100}),
        json!({"object":"response.input_tokens","input_tokens":100,"extra":"private"}),
    ] {
        let c = client();
        let d = c.prepare_scoped(request()).unwrap();
        assert!(counted(c, d, quote(), value).is_err());
    }
    let p = counted_request(request(), 1001);
    assert_eq!(p.evidence().input_tokens, 1001);
    assert!(p.prepare().is_err());
}

#[test]
fn scoped_api_rejected_tools_and_partial_replies_keep_independent_billing() {
    let p = counted_request(request(), 100).prepare().unwrap();
    for mode in 0..6 {
        let mut v = response(true, "resp_billable");
        match mode {
            0 => v["status"] = json!("incomplete"),
            1 => v["output"][1]["name"] = json!("shell"),
            2 => v["output"][1]["arguments"] = json!("{"),
            3 => v["error"] = json!({"message":"private-error"}),
            4 => {
                let call = v["output"][1].clone();
                v["output"].as_array_mut().unwrap().push(call);
            }
            _ => v["output"] = json!([]),
        }
        let result = decode(&p, v.clone()).unwrap();
        assert!(result.reply.is_err());
        assert!(result.billing.is_ok());
        assert!(matches!(result.pricing, ScopedPricingResult::Priced { .. }));
        assert_eq!(result.response_sha256, digest(&v).unwrap());
        assert!(!format!("{:?}", result.reply.err().unwrap()).contains("private"));
    }
}

#[test]
fn scoped_api_missing_billing_or_unpriced_usage_never_invents_default_or_zero() {
    let p = counted_request(request(), 100).prepare().unwrap();
    for mode in 0..7 {
        let mut v = response(false, "resp_unknown");
        match mode {
            0 => v["service_tier"] = Value::Null,
            1 => v["service_tier"] = json!("unquoted"),
            2 => v["usage"]["input_tokens"] = json!(101),
            3 => v["usage"]["input_tokens_details"]["cache_write_tokens"] = Value::Null,
            4 => v["usage"]["output_tokens"] = json!(1001),
            5 => v["usage"] = Value::Null,
            _ => {
                v["usage"]["input_tokens_details"] =
                    json!({"cached_tokens":9,"cache_write_tokens":9})
            }
        }
        let result = decode(&p, v).unwrap();
        assert!(matches!(
            result.pricing,
            ScopedPricingResult::Unpriced { .. }
        ));
        if mode == 0 {
            assert_eq!(result.billing.as_ref().unwrap().actual_service, None);
            assert!(result.reply.is_ok());
        }
        if mode == 3 || mode >= 5 {
            assert!(result.billing.is_err());
        }
    }
    let mut p = p;
    // Response is priced by its original quote even after expiry; sending is not.
    p.review.quote.valid_until = (chrono::Utc::now() - chrono::Duration::seconds(1)).to_rfc3339();
    assert!(matches!(
        decode(&p, response(false, "resp_late")).unwrap().pricing,
        ScopedPricingResult::Priced { .. }
    ));
}

async fn read(socket: &mut TcpStream) -> (String, Value) {
    let mut bytes = vec![];
    let mut buf = [0; 4096];
    let start = loop {
        let n = socket.read(&mut buf).await.unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&buf[..n]);
        if let Some(n) = bytes.windows(4).position(|x| x == b"\r\n\r\n") {
            break n + 4;
        }
    };
    let header = String::from_utf8(bytes[..start].to_vec()).unwrap();
    let length: usize = header
        .lines()
        .find_map(|s| {
            s.to_lowercase()
                .strip_prefix("content-length:")
                .map(|s| s.trim().parse().unwrap())
        })
        .unwrap();
    while bytes.len() < start + length {
        let n = socket.read(&mut buf).await.unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&buf[..n]);
    }
    (
        header,
        serde_json::from_slice(&bytes[start..start + length]).unwrap(),
    )
}
async fn respond(socket: &mut TcpStream, status: &str, body: &str) {
    socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nLocation: http://127.0.0.1:9/forbidden\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
}

#[tokio::test]
async fn scoped_api_two_round_loopback_recounts_complete_original_wire_and_never_retries() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let mut count_body = Value::Null;
        for step in 0..4 {
            let (mut socket, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let (head, body) = read(&mut socket).await;
            assert!(head
                .to_lowercase()
                .contains(&format!("authorization: bearer {}", legacy::key().expose())));
            if step % 2 == 0 {
                assert!(head.starts_with("POST /responses/input_tokens "));
                assert_eq!(body["tools"].as_array().unwrap().len(), 1);
                assert_eq!(body.to_string().contains("private-continuation"), step == 2);
                assert_eq!(
                    body.to_string().contains("Original file contents"),
                    step == 2
                );
                count_body = body;
                respond(&mut socket,"200 OK",&json!({"object":"response.input_tokens","input_tokens":if step==0 {100}else{300}}).to_string()).await;
            } else {
                assert!(head.starts_with("POST /responses "));
                assert_eq!(body["service_tier"], "default");
                assert_eq!(count_projection(&body).unwrap(), count_body);
                respond(
                    &mut socket,
                    "200 OK",
                    &response(
                        step == 1,
                        if step == 1 {
                            "resp_first"
                        } else {
                            "resp_second"
                        },
                    )
                    .to_string(),
                )
                .await;
            }
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(80), listener.accept())
                .await
                .is_err()
        );
    });
    let (_tx, mut cancel) = oneshot::channel();
    let c = client();
    let d = c.prepare_scoped(request()).unwrap();
    let counted = c
        .count_scoped_url(
            d,
            quote(),
            &mut cancel,
            &format!("{base}/responses/input_tokens"),
        )
        .await
        .unwrap();
    let first = counted.evidence().clone();
    let first_reply = counted
        .prepare()
        .unwrap()
        .send_url(&mut cancel, &format!("{base}/responses"))
        .await
        .unwrap();
    assert!(matches!(
        first_reply.pricing,
        ScopedPricingResult::Priced { .. }
    ));
    let c = client();
    let d = c
        .prepare_scoped(next_request(first_reply.reply.unwrap()))
        .unwrap();
    assert_ne!(first.request, d.fingerprint);
    let counted = c
        .count_scoped_url(
            d,
            quote(),
            &mut cancel,
            &format!("{base}/responses/input_tokens"),
        )
        .await
        .unwrap();
    assert_eq!(counted.evidence().input_tokens, 300);
    let second = counted
        .prepare()
        .unwrap()
        .send_url(&mut cancel, &format!("{base}/responses"))
        .await
        .unwrap();
    assert!(second.reply.unwrap().observation().tool_calls.is_empty());
    assert!(second.billing.is_ok());
    server.await.unwrap();
}

#[tokio::test]
async fn scoped_api_loopback_http_failures_and_echoes_never_retry_or_expose_raw_response() {
    for generation in [false, true] {
        for (status, body) in [
            ("302 Found", "private-error"),
            ("429 Too Many Requests", "private-error"),
            ("200 OK", "invalid-private-json"),
            ("200 OK", r#"{"secret":"sk-synthetic-api-generation-only"}"#),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/responses", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                read(&mut socket).await;
                respond(&mut socket, status, body).await;
                assert!(
                    tokio::time::timeout(Duration::from_millis(60), listener.accept())
                        .await
                        .is_err()
                );
            });
            let (_tx, mut cancel) = oneshot::channel();
            let error = if generation {
                counted_request(request(), 100)
                    .prepare()
                    .unwrap()
                    .send_url(&mut cancel, &url)
                    .await
                    .err()
                    .unwrap()
            } else {
                let c = client();
                let d = c.prepare_scoped(request()).unwrap();
                c.count_scoped_url(d, quote(), &mut cancel, &url)
                    .await
                    .err()
                    .unwrap()
            };
            assert!(!matches!(error, ApiFailure::NotSent {}));
            assert!(
                !format!("{error:?}").contains("private")
                    && !format!("{error:?}").contains("sk-synthetic")
            );
            server.await.unwrap();
        }
    }
}

#[tokio::test]
async fn scoped_api_local_prerequisites_cancel_and_expiry_refuse_before_connect() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/responses", listener.local_addr().unwrap());
    for mode in 0..7 {
        let (tx, mut cancel) = oneshot::channel();
        let c = client();
        let d = c.prepare_scoped(request()).unwrap();
        let mut q = quote();
        let _keep_sender = if mode == 0 {
            tx.send(()).unwrap();
            None
        } else {
            Some(tx)
        };
        match mode {
            1 => q.count_price = None,
            2 => q.count_price.as_mut().unwrap().maximum_body_bytes = 1,
            3 => q.requested_model = "other".into(),
            4 => q.valid_until = (chrono::Utc::now() - chrono::Duration::seconds(1)).to_rfc3339(),
            _ => {}
        }
        let error = if mode >= 5 {
            let mut p = counted_request(request(), 100).prepare().unwrap();
            if mode == 5 {
                p.counted.observed = Instant::now() - MAX_AGE;
            } else {
                drop(cancel);
                let (tx, rx) = oneshot::channel();
                drop(tx);
                cancel = rx;
            }
            p.send_url(&mut cancel, &url).await.err().unwrap()
        } else {
            c.count_scoped_url(d, q, &mut cancel, &url)
                .await
                .err()
                .unwrap()
        };
        assert_eq!(error, ApiFailure::NotSent {});
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(60), listener.accept())
            .await
            .is_err()
    );
    for target in legacy::targets().into_iter().skip(1) {
        let r = legacy::request(target);
        assert!(client().prepare_scoped(r.clone()).is_err());
        assert!(client().prepare(r).is_ok());
    }
}

#[tokio::test]
async fn scoped_api_cancel_after_post_remains_uncertain_without_generation_or_retry() {
    for generation in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/responses", listener.local_addr().unwrap());
        let (seen, wait) = oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            read(&mut socket).await;
            seen.send(()).unwrap();
            let mut byte = [0];
            let _ = tokio::time::timeout(Duration::from_secs(3), socket.read(&mut byte)).await;
            assert!(
                tokio::time::timeout(Duration::from_millis(60), listener.accept())
                    .await
                    .is_err()
            );
        });
        let (tx, mut cancel) = oneshot::channel();
        let cancelling = tokio::spawn(async move {
            wait.await.unwrap();
            tx.send(()).unwrap();
        });
        let error = if generation {
            counted_request(request(), 100)
                .prepare()
                .unwrap()
                .send_url(&mut cancel, &url)
                .await
                .err()
                .unwrap()
        } else {
            let c = client();
            let d = c.prepare_scoped(request()).unwrap();
            c.count_scoped_url(d, quote(), &mut cancel, &url)
                .await
                .err()
                .unwrap()
        };
        assert_eq!(error, ApiFailure::LocalCancellation {});
        cancelling.await.unwrap();
        server.await.unwrap();
    }
}
