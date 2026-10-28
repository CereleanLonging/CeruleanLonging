use super::*;
use cerulean_domain::currency_budget::{
    scoped::{BillingBasis, ContextMetric, PriceBand},
    ManagementCountPrice, Rates,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

fn draft(purpose: Purpose) -> ScopedDecisionDraft {
    ScopedDecisionDraft::standard(
        purpose,
        "fixture-model",
        &"a".repeat(64),
        &json!({"context":"private-count-context"}),
    )
    .unwrap()
}
fn client() -> OpenAiClient {
    OpenAiClient {
        http: OpenAiClient::http(false).unwrap(),
        key: SecretInput::new("sk-synthetic-scoped-only".into()).unwrap(),
    }
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
        reported_models: vec!["fixture-model".into()],
        reported_services: vec!["default".into(), "priority".into()],
        billing_basis: BillingBasis::PerRequest,
        context_metric: ContextMetric::TotalInputIncludingCache,
        bands: [
            ("default", 0, 10, 9000),
            ("default", 11, 100, 1000),
            ("priority", 0, 100, 5000),
        ]
        .into_iter()
        .map(|(service, from, through, output)| PriceBand {
            actual_model: "fixture-model".into(),
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
        valid_from: (now - chrono::Duration::hours(1)).to_rfc3339(),
        valid_until: (now + chrono::Duration::hours(1)).to_rfc3339(),
        source: "synthetic scoped quote".into(),
        count_price: Some(ManagementCountPrice {
            schema: 1,
            maximum_usd_nanos: 17,
            maximum_body_bytes: 65536,
            source: "synthetic count terms, not supplier guarantee".into(),
        }),
    }
}
fn proof() -> CountedScopedDecision {
    counted(
        client(),
        draft(Purpose::Assign),
        quote(),
        json!({"object":"response.input_tokens","input_tokens":100}),
    )
    .unwrap()
}
fn response(purpose: Purpose) -> Value {
    let mut value = crate::openai::tests::response();
    let decision = match purpose {
        Purpose::Assign => {
            json!({"action":"assign","taskId":"task","workerId":"worker-codex","rationale":"approved task"})
        }
        Purpose::Accept => {
            json!({"action":"accept","taskId":"task","attemptId":"attempt","rationale":"verified"})
        }
        Purpose::Advise => {
            json!({"action":"advise","taskId":"task","suggestion":"bounded suggestion","rationale":"observed failure"})
        }
        Purpose::Plan => {
            json!({"action":"plan","rationale":"feasible","tasks":[{"id":"task","profileId":null,"goal":"code","dependencies":[],"workerId":"worker-codex","validatorId":"tests"}]})
        }
    };
    value["output"][1]["content"][0]["text"] = json!(serde_json::to_string(
        &json!({"stateDigest":"a".repeat(64),"decision":decision})
    )
    .unwrap());
    value["service_tier"] = json!("priority");
    value["usage"]["input_tokens_details"] = json!({"cached_tokens":3,"cache_write_tokens":4});
    value
}

#[test]
fn scoped_transport_count_receipt_binds_original_quote_and_generation_and_is_not_authority() {
    let p = proof();
    let original = p.evidence().clone();
    original
        .validate(p.fingerprint(), &digest(&p.quote).unwrap())
        .unwrap();
    let serialized = serde_json::to_string(&original).unwrap();
    assert!(!serialized.contains("private-count-context") && !serialized.contains("sk-synthetic"));
    assert!(serde_json::from_str::<ManagementTokenCount>(&serialized).is_err());
    for mode in 0..7 {
        let mut evidence = original.clone();
        match mode {
            0 => evidence.input_tokens += 1,
            1 => evidence.quote_sha256 = "b".repeat(64),
            2 => evidence.request.body_sha256 = "b".repeat(64),
            3 => evidence.request.count_body_sha256 = "b".repeat(64),
            4 => evidence.request.requested_service = RequestedService::Omitted,
            5 => evidence.observed_unix_ms = 0,
            _ => evidence.schema = 2,
        }
        assert!(evidence
            .validate(p.fingerprint(), &digest(&p.quote).unwrap())
            .is_err());
    }
    let prepared = p.prepare().unwrap();
    assert_eq!(
        prepared.pricing_request().input,
        InputRange {
            from: 0,
            through: 100
        }
    );
    // Lower bands cost more, so a point interval at count100 would underreserve.
    assert_eq!(prepared.audit().maximum_usd_nanos, 36894);
    assert_eq!(
        prepared
            .quote()
            .count_price
            .as_ref()
            .unwrap()
            .maximum_usd_nanos,
        17
    );
    prepared.check_ready().unwrap();
}

#[test]
fn scoped_transport_count_rejects_invalid_or_extended_responses_without_estimates() {
    for n in [
        Value::Null,
        json!(0),
        json!(-1),
        json!(1.5),
        json!("10"),
        json!(MAX_TOKENS + 1),
    ] {
        assert!(counted(
            client(),
            draft(Purpose::Assign),
            quote(),
            json!({"object":"response.input_tokens","input_tokens":n})
        )
        .is_err());
    }
    for value in [
        json!({"object":"response","input_tokens":10}),
        json!({"object":"response.input_tokens","input_tokens":10,"extra":"secret"}),
    ] {
        assert!(counted(client(), draft(Purpose::Assign), quote(), value).is_err());
    }
}

#[test]
fn scoped_transport_preserves_billable_rejected_decisions_and_unknown_service() {
    let p = proof().prepare().unwrap();
    let valid = response(Purpose::Assign);
    for mode in 0..7 {
        let mut v = valid.clone();
        match mode {
            0 => v["status"] = json!("incomplete"),
            1 => v["output"][1]["content"][0]["text"] = json!("invalid-secret-output"),
            2 => v["output"]
                .as_array_mut()
                .unwrap()
                .push(json!({"type":"function_call","name":"forbidden"})),
            3 => {
                v["output"][1]["content"][0]["text"] = json!(serde_json::to_string(
                &json!({"stateDigest":"b".repeat(64),"decision":{"action":"wait","reason":"stale"}})
            )
            .unwrap())
            }
            4 => v["error"] = json!({"private":"provider-secret"}),
            5 => v["output"] = json!([]),
            _ => v["incomplete_details"] = json!({"reason":"max_output_tokens"}),
        }
        let result = decode(&p.review, v.clone()).unwrap();
        assert!(result.decision.is_err());
        assert!(result.billing.is_ok());
        assert_eq!(result.response_sha256, digest(&v).unwrap());
        assert!(matches!(
            result.pricing,
            ScopedPricingResult::Priced {
                amount_usd_nanos: 119
            }
        ));
        assert!(!result.decision.unwrap_err().contains("secret"));
    }
    for service in [Value::Null, json!("fast"), json!("auto"), json!("other")] {
        let mut v = valid.clone();
        v["service_tier"] = service;
        let result = decode(&p.review, v).unwrap();
        assert!(result.decision.is_ok());
        assert!(result.billing.is_ok());
        assert!(matches!(
            result.pricing,
            ScopedPricingResult::Unpriced { .. }
        ));
    }
    for mode in 0..4 {
        let mut v = valid.clone();
        match mode {
            0 => v["usage"]["input_tokens_details"]["cached_tokens"] = Value::Null,
            1 => v["usage"]["input_tokens_details"]["cache_write_tokens"] = json!(-1),
            2 => v["service_tier"] = json!(12),
            _ => {
                v["usage"]["input_tokens_details"] =
                    json!({"cached_tokens":9,"cache_write_tokens":9})
            }
        }
        let result = decode(&p.review, v).unwrap();
        assert!(result.billing.is_err());
        assert!(matches!(
            result.pricing,
            ScopedPricingResult::Unpriced { .. }
        ));
    }
    for mode in 0..3 {
        let mut v = valid.clone();
        match mode {
            0 => v["usage"]["input_tokens"] = json!(101),
            1 => v["usage"]["output_tokens"] = json!(4097),
            _ => v["model"] = json!("unquoted-model"),
        }
        let result = decode(&p.review, v).unwrap();
        assert!(result.billing.is_ok());
        assert!(matches!(
            result.pricing,
            ScopedPricingResult::Unpriced { .. }
        ));
    }
}

async fn read_request(socket: &mut TcpStream) -> (String, Value) {
    let mut bytes = vec![];
    let mut buf = [0; 4096];
    let start = loop {
        let n = socket.read(&mut buf).await.unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&buf[..n]);
        if let Some(i) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        assert!(bytes.len() < 128 * 1024);
    };
    let headers = String::from_utf8(bytes[..start].to_vec()).unwrap();
    let length: usize = headers
        .lines()
        .find_map(|line| {
            line.to_lowercase()
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
        headers,
        serde_json::from_slice(&bytes[start..start + length]).unwrap(),
    )
}
async fn reply(socket: &mut TcpStream, status: &str, body: &str) {
    socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nLocation: http://127.0.0.1:9/forbidden\r\nConnection: close\r\n\r\n{body}",body.len()).as_bytes()).await.unwrap();
}

#[tokio::test]
async fn scoped_transport_loopback_counts_and_sends_each_purpose_once_with_same_client_and_body() {
    for purpose in [
        Purpose::Plan,
        Purpose::Assign,
        Purpose::Accept,
        Purpose::Advise,
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let d = draft(purpose);
        let expected = d.body.clone();
        let count = d.checked_count_body().unwrap();
        let response = response(purpose);
        let server = tokio::spawn(async move {
            for first in [true, false] {
                let (mut socket, _) =
                    tokio::time::timeout(Duration::from_secs(5), listener.accept())
                        .await
                        .unwrap()
                        .unwrap();
                let (headers, body) = read_request(&mut socket).await;
                assert!(headers
                    .to_lowercase()
                    .contains("authorization: bearer sk-synthetic-scoped-only"));
                if first {
                    assert!(headers.starts_with("POST /responses/input_tokens "));
                    assert_eq!(body, count);
                    assert!(body.get("service_tier").is_none());
                    reply(
                        &mut socket,
                        "200 OK",
                        r#"{"object":"response.input_tokens","input_tokens":100}"#,
                    )
                    .await;
                } else {
                    assert!(headers.starts_with("POST /responses "));
                    assert_eq!(body, expected);
                    assert_eq!(body["service_tier"], "default");
                    reply(
                        &mut socket,
                        "200 OK",
                        &serde_json::to_string(&response).unwrap(),
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
        let counted = client()
            .count_scoped_url(d, quote(), &format!("{base}/responses/input_tokens"))
            .await
            .unwrap();
        let prepared = counted.prepare().unwrap();
        assert_eq!(prepared.audit().maximum_usd_nanos, 36894);
        let result = prepared
            .decide_url(&format!("{base}/responses"))
            .await
            .unwrap();
        assert!(result.decision.is_ok(), "{:?}", result.decision.err());
        assert_eq!(
            result.billing.unwrap().actual_service,
            Some("priority".into())
        );
        assert!(matches!(
            result.pricing,
            ScopedPricingResult::Priced {
                amount_usd_nanos: 119
            }
        ));
        server.await.unwrap();
    }
}

#[tokio::test]
async fn scoped_transport_loopback_count_and_generation_failures_never_retry_or_leak() {
    for generation in [false, true] {
        for (status, body) in [
            ("302 Found", "provider-secret"),
            ("429 Too Many Requests", "provider-secret"),
            ("200 OK", "invalid-json-secret"),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}/responses", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                read_request(&mut socket).await;
                reply(&mut socket, status, body).await;
                assert!(
                    tokio::time::timeout(Duration::from_millis(80), listener.accept())
                        .await
                        .is_err()
                );
            });
            let err = if generation {
                proof()
                    .prepare()
                    .unwrap()
                    .decide_url(&url)
                    .await
                    .err()
                    .unwrap()
            } else {
                client()
                    .count_scoped_url(draft(Purpose::Assign), quote(), &url)
                    .await
                    .err()
                    .unwrap()
            };
            assert!(
                !err.contains("provider-secret")
                    && !err.contains("json-secret")
                    && !err.contains("sk-synthetic")
            );
            server.await.unwrap();
        }
    }
}

#[tokio::test]
async fn scoped_transport_expired_changed_or_unquoted_requests_produce_no_http() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/responses", listener.local_addr().unwrap());
    for mode in 0..5 {
        let mut q = quote();
        match mode {
            0 => q.count_price = None,
            1 => q.count_price.as_mut().unwrap().maximum_body_bytes = 1,
            2 => q.requested_service = RequestedService::Omitted,
            3 => q.valid_until = (chrono::Utc::now() - chrono::Duration::seconds(1)).to_rfc3339(),
            _ => q.requested_model = "other".into(),
        }
        assert!(client()
            .count_scoped_url(draft(Purpose::Assign), q, &url)
            .await
            .is_err());
    }
    for mode in 0..4 {
        let mut p = proof().prepare().unwrap();
        match mode {
            0 => p.counted.observed = Instant::now() - Duration::from_secs(61),
            1 => p.counted.draft.body["input"] = json!("changed"),
            2 => p.counted.evidence.input_tokens += 1,
            _ => {
                p.counted.quote.valid_until =
                    (chrono::Utc::now() - chrono::Duration::seconds(1)).to_rfc3339()
            }
        }
        assert!(p.decide_url(&url).await.is_err());
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(80), listener.accept())
            .await
            .is_err()
    );
}

#[test]
fn scoped_transport_successful_count_is_available_when_quote_cannot_cover_generation() {
    let mut q = quote();
    q.bands
        .iter_mut()
        .for_each(|b| b.input.through = b.input.through.min(50));
    let counted = counted(
        client(),
        draft(Purpose::Assign),
        q,
        json!({"object":"response.input_tokens","input_tokens":100}),
    )
    .unwrap();
    let audit = counted.evidence().clone();
    assert_eq!(audit.input_tokens, 100);
    assert!(counted.prepare().is_err());
    assert_eq!(
        audit.response_sha256,
        digest(&json!({"object":"response.input_tokens","input_tokens":100})).unwrap()
    );
}

#[test]
fn scoped_transport_late_response_uses_original_quote_and_agrees_with_review() {
    let mut prepared = proof().prepare().unwrap();
    let mut copied_quote = prepared.quote().clone();
    copied_quote
        .bands
        .iter_mut()
        .for_each(|b| b.rates.output *= 4);
    assert_ne!(&copied_quote, prepared.quote());
    // A reply may arrive after quote/count expiry. New send is forbidden but
    // response billing must still use the original approved pricing bands.
    prepared.counted.observed = Instant::now() - Duration::from_secs(61);
    assert!(prepared.check_ready().is_err());
    // Review a historical quote at its original valid time, then evaluate the
    // late response today without modifying either the original quote or audit.
    let now = chrono::Utc::now();
    let mut original_quote = quote();
    original_quote.valid_from = (now - chrono::Duration::hours(2)).to_rfc3339();
    original_quote.valid_until = (now - chrono::Duration::hours(1)).to_rfc3339();
    let review = draft(Purpose::Assign)
        .review(
            &original_quote,
            InputRange {
                from: 0,
                through: 100,
            },
            &(now - chrono::Duration::minutes(90)).to_rfc3339(),
        )
        .unwrap();
    let original_digest = digest(&original_quote).unwrap();
    assert!(original_quote.valid_at(&now.to_rfc3339()).is_err());
    assert_eq!(review.audit.quote_sha256, original_digest);
    for mode in 0..6 {
        let mut value = response(Purpose::Assign);
        match mode {
            0 => {}
            1 => value["service_tier"] = Value::Null,
            2 => value["usage"]["input_tokens"] = json!(101),
            3 => value["usage"]["input_tokens_details"]["cached_tokens"] = Value::Null,
            4 => value["status"] = json!("incomplete"),
            _ => value["service_tier"] = json!("default"),
        }
        let reviewed = review.evaluate_response(&value).unwrap();
        let transported = decode(&review, value).unwrap();
        assert_eq!(transported.pricing, reviewed.result);
        assert_eq!(transported.decision.is_ok(), reviewed.decision_usable);
        assert_eq!(transported.response_sha256, reviewed.response_sha256);
    }
    assert_eq!(digest(&review.quote).unwrap(), original_digest);
}
