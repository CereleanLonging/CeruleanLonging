use super::*;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

fn request(purpose: Purpose) -> PreparedDecision {
    PreparedDecision::new(
        purpose,
        "fixture-model",
        &"a".repeat(64),
        &json!({"privateContext":"本地测试"}),
    )
    .unwrap()
}
fn client() -> OpenAiClient {
    OpenAiClient {
        http: OpenAiClient::http(false).unwrap(),
        key: SecretInput::new("sk-synthetic-preflight-only".into()).unwrap(),
    }
}
fn proof() -> CountedDecision {
    let p = request(Purpose::Assign);
    let body = p.count_body().unwrap();
    counted(
        client(),
        p,
        &body,
        json!({"object":"response.input_tokens","input_tokens":10}),
    )
    .unwrap()
}

#[test]
fn preflight_preserves_all_input_fields_for_every_management_role() {
    for purpose in [
        Purpose::Plan,
        Purpose::Assign,
        Purpose::Accept,
        Purpose::Advise,
    ] {
        let prepared = request(purpose);
        let body = prepared.count_body().unwrap();
        for name in [
            "model",
            "instructions",
            "input",
            "text",
            "tools",
            "tool_choice",
            "parallel_tool_calls",
        ] {
            assert_eq!(body[name], prepared.body[name]);
        }
        assert_eq!(body.as_object().unwrap().len(), 7);
        assert_eq!(body["text"]["format"]["type"], "json_schema");
        for field in [
            "conversation",
            "previous_response_id",
            "reasoning",
            "new_unknown_field",
        ] {
            let mut changed = request(purpose);
            changed.body[field] = json!("unreviewed");
            // Even a freshly computed fingerprint cannot extend this contract.
            changed.fingerprint.body_sha256 = digest(&changed.body).unwrap();
            assert!(changed.count_body().is_err());
        }
        let mut changed = request(purpose);
        changed.body["input"] = json!("tampered");
        assert!(changed.count_body().is_err());
    }
}

#[test]
fn reasoning_is_preserved_in_count_and_invalid_options_are_rejected() {
    for purpose in [
        Purpose::Plan,
        Purpose::Assign,
        Purpose::Accept,
        Purpose::Advise,
        Purpose::Refine,
    ] {
        let context = json!({"chiefReasoningEffort":"high","executionSetup":{"setup":{"managementSources":{"chiefReasoningEffort":"high","deputyReasoningEffort":"high"}}}});
        let prepared =
            PreparedDecision::new(purpose, "gpt-6-astra", &digest(&context).unwrap(), &context)
                .unwrap();
        let count = prepared.count_body().unwrap();
        assert_eq!(count["reasoning"], json!({"effort":"high"}));
        assert_eq!(count["reasoning"], prepared.body["reasoning"]);
        for invalid in [
            json!(null),
            json!({}),
            json!({"effort":"ultra"}),
            json!({"effort":"high","summary":"auto"}),
        ] {
            let mut changed =
                PreparedDecision::new(purpose, "gpt-6-astra", &digest(&context).unwrap(), &context)
                    .unwrap();
            changed.body["reasoning"] = invalid;
            changed.fingerprint.body_sha256 = digest(&changed.body).unwrap();
            assert!(changed.count_body().is_err());
        }
    }
}

#[test]
fn preflight_rejects_invalid_counts_and_never_substitutes_character_estimates() {
    let invalid = [
        json!(null),
        json!(0),
        json!(-1),
        json!(1.5),
        json!("10"),
        json!(MAX_TOKENS + 1),
    ];
    for n in invalid {
        let p = request(Purpose::Assign);
        let body = p.count_body().unwrap();
        assert!(counted(
            client(),
            p,
            &body,
            json!({"object":"response.input_tokens","input_tokens":n})
        )
        .is_err());
    }
    for value in [
        json!({"object":"response","input_tokens":10}),
        json!({"object":"response.input_tokens"}),
        json!({"object":"response.input_tokens","input_tokens":10,"unexpected":"secret"}),
    ] {
        let p = request(Purpose::Assign);
        let body = p.count_body().unwrap();
        assert!(counted(client(), p, &body, value).is_err());
    }
    let p = proof();
    let ceiling = p.ceiling().unwrap();
    assert_eq!((ceiling.total_input, ceiling.output), (10, 4096));
    assert!(ceiling.cache_read_allowed && ceiling.cache_write_allowed);
    let receipt = serde_json::to_string(p.evidence()).unwrap();
    assert!(!receipt.contains("本地测试") && !receipt.contains("synthetic"));
    assert_eq!(p.evidence.request, *p.prepared().fingerprint());
}

async fn read_request(stream: &mut TcpStream) -> (String, Vec<u8>) {
    let mut bytes = Vec::new();
    let mut buf = [0; 4096];
    let start = loop {
        let n = stream.read(&mut buf).await.unwrap();
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
        .find_map(|s| {
            s.to_lowercase()
                .strip_prefix("content-length:")
                .map(|s| s.trim().parse().unwrap())
        })
        .unwrap();
    while bytes.len() < start + length {
        let n = stream.read(&mut buf).await.unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&buf[..n]);
    }
    (headers, bytes[start..start + length].to_vec())
}
async fn reply(stream: &mut TcpStream, status: &str, body: &str) {
    stream.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nLocation: http://127.0.0.1:9/forbidden\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
}

#[tokio::test]
async fn preflight_loopback_counts_then_sends_the_identical_generation_once() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let prepared = request(Purpose::Assign);
    let expected = prepared.body.clone();
    let expected_count = prepared.count_body().unwrap();
    let server = tokio::spawn(async move {
        for count in [true, false] {
            let (mut socket, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
                .await
                .unwrap()
                .unwrap();
            let (headers, bytes) = read_request(&mut socket).await;
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            if count {
                assert!(headers.starts_with("POST /responses/input_tokens HTTP/1.1"));
                assert_eq!(value, expected_count);
                reply(
                    &mut socket,
                    "200 OK",
                    r#"{"object":"response.input_tokens","input_tokens":10}"#,
                )
                .await;
            } else {
                assert!(headers.starts_with("POST /responses HTTP/1.1"));
                assert_eq!(value, expected);
                use sha2::{Digest, Sha256};
                assert_eq!(
                    format!("{:x}", Sha256::digest(&bytes)),
                    digest(&expected).unwrap()
                );
                let mut response = crate::openai::tests::response();
                response["usage"]["input_tokens_details"] =
                    json!({"cached_tokens":0,"cache_write_tokens":0});
                reply(
                    &mut socket,
                    "200 OK",
                    &serde_json::to_string(&response).unwrap(),
                )
                .await;
            }
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(100), listener.accept())
                .await
                .is_err()
        );
    });
    let counted = client()
        .count_decision_url(prepared, &format!("{base}/responses/input_tokens"))
        .await
        .unwrap();
    assert_eq!(counted.ceiling().unwrap().total_input, 10);
    let result = counted
        .decide_url(&format!("{base}/responses"))
        .await
        .unwrap();
    assert_eq!(result.observation.input_tokens, 10);
    server.await.unwrap();
}

#[tokio::test]
async fn preflight_loopback_failure_never_generates_retries_or_leaks_error_body() {
    for (status, body) in [
        ("302 Found", "provider-secret"),
        ("429 Too Many Requests", "provider-secret"),
        ("200 OK", "not-json-provider-secret"),
        (
            "200 OK",
            r#"{"object":"response.input_tokens","input_tokens":"12"}"#,
        ),
    ] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "http://{}/responses/input_tokens",
            listener.local_addr().unwrap()
        );
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let (headers, _) = read_request(&mut socket).await;
            assert!(headers.starts_with("POST /responses/input_tokens HTTP/1.1"));
            reply(&mut socket, status, body).await;
            drop(socket);
            assert!(
                tokio::time::timeout(Duration::from_millis(100), listener.accept())
                    .await
                    .is_err()
            );
        });
        let result = client()
            .count_decision_url(request(Purpose::Assign), &url)
            .await;
        let Err(error) = result else {
            panic!("invalid count granted proof")
        };
        assert!(!error.contains("provider-secret"));
        server.await.unwrap();
    }
}

#[tokio::test]
async fn preflight_expired_proof_sends_nothing() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/responses", listener.local_addr().unwrap());
    let mut counted = proof();
    counted.observed = Instant::now() - COUNT_MAX_AGE;
    assert!(counted.ceiling().is_err());
    assert!(counted
        .decide_url(&url)
        .await
        .unwrap_err()
        .contains("expired"));
    assert!(
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err()
    );
}
