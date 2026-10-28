//! Controlled HTTP only; no supplier calls or real credentials.
use super::*;
use cerulean_adapters::api_generation::{
    ApiAttempt, ApiMessage, ApiRequest, ApiTarget, ResponseRejection,
};
use cerulean_domain::secret::SecretInput;
use serde_json::{json, Value};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

pub(crate) async fn server(body: Value) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/fixture", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        tokio::time::timeout(std::time::Duration::from_secs(8), async {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut part = [0; 4096];
                let n = socket.read(&mut part).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&part[..n]);
                assert!(bytes.len() < 65536);
                if let Some(end) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&bytes[..end]).to_ascii_lowercase();
                    assert!(header.starts_with("post /fixture http/1.1"));
                    let length: usize = header.lines().find_map(|l| l.strip_prefix("content-length: ")).unwrap().parse().unwrap();
                    if bytes.len() == end + 4 + length { break; }
                }
            }
            let body = serde_json::to_vec(&body).unwrap();
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).await.unwrap();
            socket.write_all(&body).await.unwrap();
            socket.shutdown().await.unwrap();
            drop(socket);
        }).await.unwrap();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), listener.accept())
                .await
                .is_err(),
            "automatic HTTP retry"
        );
    });
    (url, task)
}
pub(crate) fn rejected_body() -> Value {
    json!({"id":"rejected-543", "model":"fixture", "object":"response", "status":"incomplete", "incomplete_details":{"reason":"max_output_tokens"},
        "output":[{"type":"function_call","call_id":"bad","name":"cerulean_write","arguments":"{truncated"}],
        "usage":{"input_tokens":13,"output_tokens":7,"total_tokens":20,"input_tokens_details":{"cached_tokens":2}}})
}
fn client_request() -> (ApiClient, PreparedRequest, CallIdentity) {
    client_request_for(ApiTarget::OpenAi {})
}
fn client_request_for(target: ApiTarget) -> (ApiClient, PreparedRequest, CallIdentity) {
    let client =
        ApiClient::new(SecretInput::new("synthetic-rejected-usage-543".into()).unwrap()).unwrap();
    let prepared = client
        .prepare(ApiRequest {
            target,
            model: "fixture".into(),
            instructions: "Local controlled test".into(),
            messages: vec![ApiMessage::User("test".into())],
            tools: vec![],
            max_output_tokens: 64,
            timeout_seconds: 5,
        })
        .unwrap();
    let mut id = super::tests::identity();
    id.fingerprint = prepared.fingerprint().clone();
    (client, prepared, id)
}
async fn attempt(value: Value) -> (ApiAttempt, CallIdentity) {
    let (client, prepared, id) = client_request();
    let (url, server) = server(value).await;
    let (_tx, mut rx) = tokio::sync::oneshot::channel();
    let attempt = client.fixture_send_measured(prepared, &mut rx, &url).await;
    server.await.unwrap();
    (attempt, id)
}

#[tokio::test]
async fn rejected_usage_http_journal_reopen_is_read_only_and_never_resends() {
    let (attempt, id) = attempt(rejected_body()).await;
    assert!(matches!(
        attempt.result,
        Err(ApiFailure::ResponseRejected {
            reason: ResponseRejection::IncompleteOutput
        })
    ));
    assert_eq!(
        attempt.rejected_usage.as_ref().unwrap().usage.output_tokens,
        7
    );
    let dir = protected_round();
    let mut journal = ApiCallJournal::open(dir.path(), &id).unwrap();
    assert!(journal
        .execute_reply_with_gateway(&id.fingerprint, dir.path(), async |_| attempt)
        .await
        .unwrap()
        .is_err());
    let head = journal.head().to_owned();
    assert!(journal.state().remote_outcome_unknown()); // remote business/billing status is still unknown
    drop(journal);
    let before = fs::read(dir.path().join("01.json")).unwrap();
    let value: Value = serde_json::from_slice(&before).unwrap();
    assert_eq!(
        value["body"]["event"]["outcome"]["failure"],
        json!({"kind":"invalidResponse"})
    );
    for _ in 0..2 {
        let mut journal = ApiCallJournal::read_existing(dir.path()).unwrap();
        assert_eq!(journal.head(), head);
        assert_eq!(
            journal
                .state()
                .rejected_usage
                .as_ref()
                .unwrap()
                .usage
                .total_tokens(ApiTarget::OpenAi {}.provider())
                .unwrap(),
            20
        );
        assert!(journal
            .execute_reply_with_gateway(&id.fingerprint, dir.path(), async |_| {
                panic!("reopen must not send");
                #[allow(unreachable_code)]
                Err::<ApiReply, _>(ApiFailure::NotSent {})
            })
            .await
            .is_err());
    }
    assert_eq!(fs::read(dir.path().join("01.json")).unwrap(), before);
}

#[tokio::test]
async fn rejected_usage_invalid_fields_and_credential_echo_stay_unknown() {
    for case in [
        "missing",
        "negative",
        "overflow",
        "cache",
        "total",
        "identity",
        "echo",
        "null-details",
    ] {
        let mut value = rejected_body();
        match case {
            "missing" => {
                value.as_object_mut().unwrap().remove("usage");
            }
            "negative" => value["usage"]["input_tokens"] = json!(-1),
            "overflow" => value["usage"]["input_tokens"] = json!(u64::MAX),
            "cache" => value["usage"]["input_tokens_details"]["cached_tokens"] = json!(14),
            "total" => value["usage"]["total_tokens"] = json!(21),
            "identity" => value["id"] = json!("bad id"),
            "echo" => value["extra"] = json!("synthetic-rejected-usage-543"),
            "null-details" => value["usage"]["input_tokens_details"] = Value::Null,
            _ => unreachable!(),
        }
        let (attempt, _) = attempt(value).await;
        assert!(attempt.result.is_err(), "{case}");
        assert!(attempt.rejected_usage.is_none(), "{case}");
    }
}

#[tokio::test]
async fn rejected_usage_binding_order_and_partial_publication_fail_closed() {
    let (attempt, id) = attempt(rejected_body()).await;
    let evidence = attempt.rejected_usage.unwrap();
    for case in ["request", "digest", "success", "not-sent", "transport"] {
        let dir = protected_round();
        let mut journal = ApiCallJournal::open(dir.path(), &id).unwrap();
        journal
            .publish(
                &Event::Intent {
                    identity: id.clone(),
                },
                |_| Ok(()),
            )
            .unwrap();
        let mut evidence = evidence.clone();
        let outcome = match case {
            "success" => CallOutcome::Reply {
                observation: super::tests::observation(),
            },
            "not-sent" => CallOutcome::Failure {
                failure: ApiFailure::NotSent {},
            },
            "transport" => CallOutcome::Failure {
                failure: ApiFailure::Transport {},
            },
            _ => CallOutcome::Failure {
                failure: ApiFailure::InvalidResponse {},
            },
        };
        if case == "request" {
            evidence.request.body_sha256 = "f".repeat(64);
        }
        if case == "digest" {
            evidence.response_json_sha256 = "invalid".into();
        }
        assert!(journal
            .publish(
                &Event::Outcome {
                    outcome,
                    rejected_usage: Some(evidence)
                },
                |_| Ok(())
            )
            .is_err());
        assert!(!dir.path().join("01.json").exists());
    }
    for stage in ["created", "synced", "published", "committed"] {
        let dir = protected_round();
        let mut journal = ApiCallJournal::open(dir.path(), &id).unwrap();
        journal
            .publish(
                &Event::Intent {
                    identity: id.clone(),
                },
                |_| Ok(()),
            )
            .unwrap();
        let event = Event::Outcome {
            outcome: CallOutcome::Failure {
                failure: ApiFailure::InvalidResponse {},
            },
            rejected_usage: Some(evidence.clone()),
        };
        assert!(journal
            .publish(&event, |at| if at == stage {
                Err("controlled write interruption".into())
            } else {
                Ok(())
            })
            .is_err());
        assert!(!journal.healthy());
        drop(journal);
        let reopened = ApiCallJournal::read_existing(dir.path());
        if stage == "committed" {
            assert_eq!(
                reopened.unwrap().state().rejected_usage.as_ref(),
                Some(&evidence)
            );
        } else {
            assert!(reopened.is_err());
        }
    }
}

#[tokio::test]
async fn rejected_usage_unmodified_old_failure_has_no_measurement() {
    let (_, _, id) = client_request();
    let dir = protected_round();
    let mut journal = ApiCallJournal::open(dir.path(), &id).unwrap();
    journal
        .execute_reply(&id.fingerprint, || async {
            Err(ApiFailure::InvalidResponse {})
        })
        .await
        .unwrap()
        .unwrap_err();
    drop(journal);
    let value: Value =
        serde_json::from_slice(&fs::read(dir.path().join("01.json")).unwrap()).unwrap();
    assert!(value["body"]["event"].get("rejected_usage").is_none());
    assert!(ApiCallJournal::read_existing(dir.path())
        .unwrap()
        .state()
        .rejected_usage
        .is_none());
}

#[tokio::test]
async fn rejected_usage_deepseek_length_and_invalid_tools_do_not_release_reply() {
    use cerulean_domain::credentials::ProviderEndpoint;
    for case in [
        "length",
        "arguments",
        "cache-conflict",
        "cache-miss",
        "success",
    ] {
        let target = ApiTarget::Provider {
            endpoint: ProviderEndpoint::DeepSeek {},
        };
        let (client, prepared, id) = client_request_for(target);
        let mut body = json!({"object":"chat.completion","id":"chat-543","model":"fixture",
            "choices":[{"index":0,"finish_reason":"length","message":{"role":"assistant","content":null,
                "tool_calls":[{"id":"tool-1","type":"function","function":{"name":"cerulean_write","arguments":"{truncated"}}]}}],
            "usage":{"prompt_tokens":20,"completion_tokens":10,"total_tokens":30,
                "prompt_cache_hit_tokens":5,"prompt_cache_miss_tokens":15,
                "prompt_tokens_details":{"cached_tokens":5}}});
        match case {
            "arguments" => body["choices"][0]["finish_reason"] = json!("tool_calls"),
            "cache-conflict" => body["usage"]["prompt_tokens_details"]["cached_tokens"] = json!(4),
            "cache-miss" => body["usage"]["prompt_cache_miss_tokens"] = json!(14),
            "success" => {
                body["choices"][0]["finish_reason"] = json!("stop");
                body["choices"][0]["message"] = json!({"role":"assistant","content":"Done"});
            }
            _ => {}
        }
        let (url, server) = server(body).await;
        let dir = protected_round();
        let mut journal = ApiCallJournal::open(dir.path(), &id).unwrap();
        let (_tx, mut rx) = tokio::sync::oneshot::channel();
        let result = journal
            .execute_reply_with_gateway(
                &id.fingerprint,
                dir.path(),
                async |_: &mut cerulean_adapters::api_generation::GatewayObserver<'_>| {
                    assert!(dir.path().join("00.json").exists());
                    client.fixture_send_measured(prepared, &mut rx, &url).await
                },
            )
            .await
            .unwrap();
        server.await.unwrap();
        if case == "success" {
            assert!(result.is_ok());
            assert!(
                journal.state().rejected_usage.is_none(),
                "success is not counted twice"
            );
        } else {
            assert!(result.is_err());
            if case == "length" || case == "arguments" {
                assert!(
                    matches!(result, Err(ApiFailure::ResponseRejected {reason}) if reason == if case == "length" {ResponseRejection::OutputLimit} else {ResponseRejection::InvalidToolArguments})
                );
                assert_eq!(
                    journal
                        .state()
                        .rejected_usage
                        .as_ref()
                        .unwrap()
                        .usage
                        .input_tokens,
                    20
                );
            } else {
                assert!(journal.state().rejected_usage.is_none());
            }
        }
    }
}

#[tokio::test]
async fn rejected_usage_not_sent_and_transport_do_not_invent_measurement() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/fixture", listener.local_addr().unwrap());
    let (client, prepared, _) = client_request();
    let (tx, mut rx) = tokio::sync::oneshot::channel();
    tx.send(()).unwrap();
    let attempt = client.fixture_send_measured(prepared, &mut rx, &url).await;
    assert!(matches!(attempt.result, Err(ApiFailure::NotSent {})));
    assert!(attempt.rejected_usage.is_none());
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), listener.accept())
            .await
            .is_err()
    );
    drop(listener);
    let (client, prepared, _) = client_request();
    let (_tx, mut rx) = tokio::sync::oneshot::channel();
    let attempt = client.fixture_send_measured(prepared, &mut rx, &url).await;
    assert!(matches!(attempt.result, Err(ApiFailure::Transport {})));
    assert!(attempt.rejected_usage.is_none());
}

struct ProtectedRound {
    _temp: tempfile::TempDir,
    round: std::path::PathBuf,
}
impl ProtectedRound {
    fn path(&self) -> &Path {
        &self.round
    }
}
fn protected_round() -> ProtectedRound {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    drop(crate::Store::open(&data).unwrap());
    crate::rejected_usage_guard::tests::promote_fixture(&data, &temp.path().join("cold"));
    let round = data.join("workflows/project/api-attempt-fixture/rounds/001");
    ProtectedRound { _temp: temp, round }
}

#[tokio::test]
async fn rejected_usage_guard_stops_before_intent_and_sender_on_unprotected_root() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    drop(crate::Store::open(&data).unwrap());
    let round = data.join("workflows/project/api-attempt-fixture/rounds/001");
    let (_, _, id) = client_request();
    let mut journal = ApiCallJournal::open(&round, &id).unwrap();
    let mut sent = false;
    let failure = journal
        .execute_reply_with_gateway(&id.fingerprint, &round, async |_| {
            sent = true;
            ApiAttempt::from(Err(ApiFailure::Transport {}))
        })
        .await
        .unwrap_err();
    assert!(failure.contains("reader17"), "{failure}");
    assert!(!sent);
    assert!(!journal.state().intent_recorded);
    assert!(!round.join("00.json").exists());
    assert!(journal.healthy());
}
