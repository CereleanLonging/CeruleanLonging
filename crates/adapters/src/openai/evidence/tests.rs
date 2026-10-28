use super::*;

fn prepared() -> PreparedDecision {
    PreparedDecision::new(
        Purpose::Assign,
        "fixture-model",
        &"a".repeat(64),
        &json!({"scope":"original"}),
    )
    .unwrap()
}
#[test]
fn management_wire_fingerprint_binds_body_and_does_not_expose_context() {
    let original = prepared();
    let altered = PreparedDecision::new(
        Purpose::Assign,
        "fixture-model",
        &"a".repeat(64),
        &json!({"scope":"changed"}),
    )
    .unwrap();
    assert_ne!(original.fingerprint(), altered.fingerprint());
    assert_eq!(
        original.fingerprint.body_sha256,
        digest(&original.body).unwrap()
    );
    assert!(!serde_json::to_string(original.fingerprint())
        .unwrap()
        .contains("original"));
    let mut wrong = original.fingerprint.clone();
    wrong.url = "https://example.invalid/responses".into();
    assert!(wrong
        .validate_for(Purpose::Assign, "fixture-model", &"a".repeat(64))
        .is_err());
}
#[test]
fn management_wire_preserves_public_cache_counts_but_never_private_reasoning() {
    let request = prepared();
    let mut value = crate::openai::tests::response();
    value["usage"]["input_tokens_details"] = json!({"cached_tokens":3,"cache_write_tokens":4});
    let result = decode(request.fingerprint(), value.clone()).unwrap();
    let transport = result.transport.unwrap();
    let usage = transport
        .usage
        .billable_usage(CredentialProvider::OpenAi)
        .unwrap();
    assert_eq!(
        (
            usage.input,
            usage.output,
            usage.cache_read,
            usage.cache_write
        ),
        (3, 20, 3, 4)
    );
    assert!(!serde_json::to_string(&transport)
        .unwrap()
        .contains("private-marker"));
    for invalid in [json!(-1), json!(1.5), json!("0"), json!(8)] {
        value["usage"]["input_tokens_details"]["cache_write_tokens"] = invalid;
        assert!(decode(request.fingerprint(), value.clone()).is_err());
    }
}
#[test]
fn management_wire_missing_counts_are_unknown_not_zero_even_with_valid_decision() {
    let request = prepared();
    for details in [
        Value::Null,
        json!({"cached_tokens":0}),
        json!({"cache_write_tokens":0}),
    ] {
        let mut value = crate::openai::tests::response();
        value["usage"]["input_tokens_details"] = details;
        let result = decode(request.fingerprint(), value).unwrap();
        assert!(result
            .transport
            .unwrap()
            .usage
            .billable_usage(CredentialProvider::OpenAi)
            .is_err());
    }
}

#[tokio::test]
async fn management_wire_loopback_body_matches_durable_fingerprint_once() {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    let prepared = prepared();
    let expected = prepared.fingerprint().clone();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/responses", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        let mut buf = [0u8; 4096];
        let start = loop {
            let n = socket.read(&mut buf).await.unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buf[..n]);
            if let Some(i) = bytes.windows(4).position(|s| s == b"\r\n\r\n") {
                break i + 4;
            }
        };
        let headers = String::from_utf8_lossy(&bytes[..start]);
        let size: usize = headers
            .lines()
            .find_map(|s| {
                s.to_lowercase()
                    .strip_prefix("content-length:")
                    .map(|n| n.trim().parse().unwrap())
            })
            .unwrap();
        while bytes.len() < start + size {
            let n = socket.read(&mut buf).await.unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&buf[..n]);
        }
        use sha2::{Digest, Sha256};
        assert_eq!(
            format!("{:x}", Sha256::digest(&bytes[start..start + size])),
            expected.body_sha256
        );
        let mut value = crate::openai::tests::response();
        value["usage"]["input_tokens_details"] = json!({"cached_tokens":0,"cache_write_tokens":0});
        let body = serde_json::to_string(&value).unwrap();
        socket
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .unwrap();
        drop(socket);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), listener.accept())
                .await
                .is_err()
        );
    });
    let client = OpenAiClient {
        http: OpenAiClient::http(false).unwrap(),
        key: SecretInput::new("sk-synthetic-management-wire".into()).unwrap(),
    };
    let value = client
        .send_url(reqwest::Method::POST, &url, Some(prepared.body))
        .await
        .unwrap();
    let result = decode(&prepared.fingerprint, value).unwrap();
    assert_eq!(result.transport.unwrap().usage.cache_write_tokens, Some(0));
    server.await.unwrap();
}
