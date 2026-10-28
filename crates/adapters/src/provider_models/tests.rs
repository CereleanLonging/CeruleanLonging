use super::*;
use serde_json::json;
fn key() -> SecretInput {
    SecretInput::new("sk-synthetic-provider-contract-only".into()).unwrap()
}
fn targets() -> Vec<ProviderEndpoint> {
    vec![
        ProviderEndpoint::DeepSeek {},
        ProviderEndpoint::Anthropic {},
        ProviderEndpoint::KimiChina {},
        ProviderEndpoint::Grok {},
        ProviderEndpoint::A6Api {},
        ProviderEndpoint::QwenBeijing {
            workspace_id: "workspace-123".into(),
        },
        ProviderEndpoint::QwenSingapore {},
        ProviderEndpoint::QwenHongKong {},
    ]
}
fn body(target: &ProviderEndpoint) -> Value {
    if qwen(target) {
        json!({"success":true,"code":null,"output":{"page_no":1,"total":3,"models":[{"model":"fixture-b"},{"model":"fixture-a"}]}})
    } else {
        json!({"data":[{"id":"fixture-b"},{"id":"fixture-a"}],"has_more":true})
    }
}
#[test]
fn closed_endpoints_and_provider_specific_shapes() {
    for target in targets() {
        let url = reqwest::Url::parse(&endpoint(&target).unwrap()).unwrap();
        assert_eq!(url.scheme(), "https");
        assert_eq!(url.username(), "");
        assert!(url.password().is_none());
        let list = parse(&target, body(&target), &key()).unwrap();
        assert_eq!(list.models, ["fixture-a", "fixture-b"]);
        assert!(list.limited);
        let mut wrong = body(&target);
        wrong["error"] = json!({"message":"secret-error"});
        assert!(parse(&target, wrong, &key()).is_err());
        assert!(parse(&target, json!({}), &key()).is_err());
    }
    for id in [
        "",
        "../evil",
        "x@evil.test",
        "x.evil.test",
        "x:443",
        "-x",
        "x-",
        "a/b",
        "a?x",
        "a\n",
    ] {
        assert!(endpoint(&ProviderEndpoint::QwenBeijing {
            workspace_id: id.into()
        })
        .is_err());
    }
    assert!(serde_json::from_value::<ProviderEndpoint>(
        json!({"kind":"grok","url":"https://evil.test"})
    )
    .is_err());
    assert!(serde_json::from_value::<ProviderEndpoint>(json!({"kind":"openAi"})).is_err());
}
#[test]
fn bounded_list_never_promotes_invalid_or_echoed_models() {
    let target = ProviderEndpoint::DeepSeek {};
    assert!(parse(&target, json!({"data":[],"has_more":"yes"}), &key()).is_err());
    assert!(endpoint(&ProviderEndpoint::QwenBeijing {
        workspace_id: "WORKSPACE-123".into()
    })
    .unwrap()
    .starts_with("https://workspace-123."));
    let many = Value::Array(
        (0..MODEL_LIMIT + 3)
            .map(|n| json!({"id":format!("model-{n}")}))
            .collect(),
    );
    let list = parse(&target, json!({"data":many}), &key()).unwrap();
    assert_eq!(list.models.len(), MODEL_LIMIT);
    assert!(list.limited);
    for id in [
        "",
        "line\nbreak",
        "<script>",
        key().expose(),
        &"x".repeat(201),
    ] {
        assert!(parse(&target, json!({"data":[{"id":id}]}), &key()).is_err());
    }
    assert!(parse(&ProviderEndpoint::Anthropic {}, json!({"data":[]}), &key()).is_err());
    assert!(parse(
        &ProviderEndpoint::QwenSingapore {},
        json!({"success":true,"output":{"page_no":2,"models":[],"total":0}}),
        &key()
    )
    .is_err());
    assert!(parse(
        &ProviderEndpoint::QwenSingapore {},
        json!({"success":false,"output":{"page_no":1,"models":[],"total":0}}),
        &key()
    )
    .is_err());
    assert!(parse(&target, json!({"data":[]}), &key())
        .unwrap()
        .models
        .is_empty());
}
#[tokio::test]
async fn actual_http_uses_provider_auth_and_only_one_get() {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    for target in targets() {
        for status in [
            "ok",
            "redirect",
            "unauthorized",
            "forbidden",
            "limited",
            "oversized",
            "bad-json",
            "chunked-large",
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let prod = endpoint(&target).unwrap();
            let prod = reqwest::Url::parse(&prod).unwrap();
            let suffix = format!(
                "{}{}",
                prod.path(),
                prod.query().map(|q| format!("?{q}")).unwrap_or_default()
            );
            let url = format!("http://{addr}{suffix}");
            let t = target.clone();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut raw = Vec::new();
                let mut buffer = [0; 4096];
                while !raw.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = socket.read(&mut buffer).await.unwrap();
                    assert!(n > 0);
                    raw.extend_from_slice(&buffer[..n]);
                    assert!(raw.len() < 8192);
                }
                let req = String::from_utf8(raw).unwrap().to_lowercase();
                assert!(req.starts_with(&format!("get {suffix} http/1.1\r\n")));
                assert!(!req.contains("content-length:"));
                if matches!(t, ProviderEndpoint::Anthropic {}) {
                    assert!(req.contains(&format!("x-api-key: {}", key().expose())));
                    assert!(req.contains("anthropic-version: 2023-06-01"));
                    assert!(!req.contains("authorization:"));
                } else {
                    assert!(req.contains(&format!("authorization: bearer {}", key().expose())));
                    assert!(!req.contains("x-api-key:"));
                }
                let payload = if status == "ok" {
                    body(&t).to_string()
                } else {
                    "secret-error-body-not-for-client".into()
                };
                if status == "chunked-large" {
                    socket.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n").await.unwrap();
                    let big = "x".repeat(RESPONSE_LIMIT + 1);
                    let _ = socket
                        .write_all(format!("{:x}\r\n{big}\r\n0\r\n\r\n", big.len()).as_bytes())
                        .await;
                } else {
                    let code = match status {
                        "redirect" => "302 Found",
                        "unauthorized" => "401 Unauthorized",
                        "forbidden" => "403 Forbidden",
                        "limited" => "429 Too Many Requests",
                        _ => "200 OK",
                    };
                    let size = if status == "oversized" {
                        RESPONSE_LIMIT + 1
                    } else {
                        payload.len()
                    };
                    socket.write_all(format!("HTTP/1.1 {code}\r\nContent-Length: {size}\r\nLocation: http://{addr}/credential-leak\r\nConnection: close\r\n\r\n{payload}").as_bytes()).await.unwrap();
                }
                drop(socket);
                assert!(
                    tokio::time::timeout(Duration::from_millis(30), listener.accept())
                        .await
                        .is_err(),
                    "retry or redirect"
                );
            });
            let result = fetch(&http(false).unwrap(), &target, &key(), &url).await;
            if status == "ok" {
                assert_eq!(result.unwrap().models, ["fixture-a", "fixture-b"]);
            } else {
                let err = result.unwrap_err();
                assert!(!err.contains("secret-error") && !err.contains(key().expose()));
            }
            server.await.unwrap();
        }
    }
}
