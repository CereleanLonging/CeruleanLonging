use super::*;
use serde_json::json;
pub(super) fn key() -> SecretInput {
    SecretInput::new("sk-synthetic-api-generation-only".into()).unwrap()
}

#[test]
fn selected_reasoning_reaches_provider_wire_and_request_identity() {
    use cerulean_domain::reasoning::ReasoningEffort;
    let client = ApiClient::new(key()).unwrap();
    for (target, model, field, expected) in [
        (
            ApiTarget::OpenAi {},
            "gpt-6-astra",
            "reasoning",
            json!({"effort":"high"}),
        ),
        (
            ApiTarget::Provider {
                endpoint: ProviderEndpoint::DeepSeek {},
            },
            "deepseek-flash",
            "reasoning_effort",
            json!("high"),
        ),
        (
            ApiTarget::Provider {
                endpoint: ProviderEndpoint::Anthropic {},
            },
            "claude-sonnet-4-6",
            "output_config",
            json!({"effort":"high"}),
        ),
        (
            ApiTarget::Provider {
                endpoint: ProviderEndpoint::QwenSingapore {},
            },
            "qwen3.8-flash",
            "reasoning_effort",
            json!("low"),
        ),
    ] {
        let mut request = request(target);
        request.model = model.into();
        let original = client.prepare(request.clone()).unwrap();
        assert!(original.body.get(field).is_none());
        request.reasoning_effort = Some(if model == "qwen3.8-flash" {
            ReasoningEffort::Low
        } else {
            ReasoningEffort::High
        });
        let selected = client.prepare(request.clone()).unwrap();
        assert_eq!(selected.body[field], expected);
        assert_ne!(
            selected.fingerprint().body_sha256,
            original.fingerprint().body_sha256
        );
        if field == "output_config" {
            assert_eq!(selected.body["thinking"], json!({"type":"adaptive"}));
        }
        request.model = "unreviewed-model".into();
        assert!(client.prepare(request).is_err());
    }
}

#[test]
fn provider_output_limits_reach_wire_and_fingerprint_without_silent_clamping() {
    let client = ApiClient::new(key()).unwrap();
    for target in targets() {
        let mut r = request(target);
        let legacy = client.prepare(r.clone()).unwrap();
        for limit in [16385, 393216, u32::MAX] {
            r.max_output_tokens = limit;
            let prepared = client.prepare(r.clone()).unwrap();
            let field = if matches!(r.target, ApiTarget::OpenAi {}) {
                "max_output_tokens"
            } else {
                "max_tokens"
            };
            assert_eq!(prepared.body[field], limit);
            assert_eq!(prepared.fingerprint().max_output_tokens, limit);
            assert_ne!(
                prepared.fingerprint().body_sha256,
                legacy.fingerprint().body_sha256
            );
            let mut expected = legacy.body.clone();
            expected[field] = json!(limit);
            assert_eq!(prepared.body, expected);
        }
        r.max_output_tokens = 0;
        assert!(client.prepare(r).is_err());
    }
}

#[test]
fn relay_vip_binds_exact_origin_and_rejects_com_continuation() {
    let client = ApiClient::new(key()).unwrap();
    let com = request(ApiTarget::Provider {
        endpoint: ProviderEndpoint::A6Api {},
    });
    let mut vip = request(ApiTarget::Provider {
        endpoint: ProviderEndpoint::A6ApiVip {},
    });
    assert_eq!(
        com.target.url().unwrap(),
        "https://api.a6api.com/v1/chat/completions"
    );
    assert_eq!(
        vip.target.url().unwrap(),
        "https://api.a6api.vip/v1/chat/completions"
    );
    let original = client.prepare(com.clone()).unwrap();
    let prepared = client.prepare(vip.clone()).unwrap();
    assert_ne!(original.fingerprint(), prepared.fingerprint());
    assert!(prepared.body.get("messages").is_some());
    assert!(prepared.body.get("input").is_none());
    let parsed = wire::parse(&vip, reply(&vip, false)).unwrap();
    assert!(parsed
        .observation()
        .usage
        .billable_usage(CredentialProvider::A6Api)
        .is_err());
    vip.messages.push(ApiMessage::Assistant(
        wire::parse(&com, reply(&com, false)).unwrap(),
    ));
    assert!(
        client.prepare(vip).is_err(),
        "reply from .com cannot authorize continuation at .vip"
    );
    assert_eq!(
        serde_json::to_value(&com.target).unwrap(),
        json!({"kind":"provider","endpoint":{"kind":"a6Api"}})
    );
}

#[test]
fn relay_uses_chat_protocol_and_does_not_inherit_official_prices_or_continuation() {
    let target = ApiTarget::Provider {
        endpoint: ProviderEndpoint::A6Api {},
    };
    let mut relay = request(target);
    relay.model = "gpt-6.1-sol".into();
    let client = ApiClient::new(key()).unwrap();
    let prepared = client.prepare(relay.clone()).unwrap();
    assert_eq!(prepared.body["model"], "gpt-6.1-sol");
    assert!(prepared.body.get("messages").is_some());
    assert!(prepared.body.get("input").is_none());
    assert_eq!(
        prepared.body["tools"][0]["function"]["name"],
        "cerulean_read"
    );
    let parsed = wire::parse(&relay, reply(&relay, false)).unwrap();
    assert!(parsed
        .observation()
        .usage
        .billable_usage(CredentialProvider::A6Api)
        .is_err());
    let mut official = request(ApiTarget::OpenAi {});
    official.messages.push(ApiMessage::Assistant(parsed));
    assert!(client.prepare(official).is_err());
    assert!(serde_json::from_value::<ApiTarget>(
        json!({"kind":"provider","endpoint":{"kind":"a6Api","baseUrl":"https://different.invalid"}})
    )
    .is_err());
}
pub(super) fn targets() -> Vec<ApiTarget> {
    vec![
        ApiTarget::OpenAi {},
        ApiTarget::Provider {
            endpoint: ProviderEndpoint::Anthropic {},
        },
        ApiTarget::Provider {
            endpoint: ProviderEndpoint::DeepSeek {},
        },
        ApiTarget::Provider {
            endpoint: ProviderEndpoint::KimiChina {},
        },
        ApiTarget::Provider {
            endpoint: ProviderEndpoint::Grok {},
        },
        ApiTarget::Provider {
            endpoint: ProviderEndpoint::QwenBeijing {
                workspace_id: "space-123".into(),
            },
        },
        ApiTarget::Provider {
            endpoint: ProviderEndpoint::QwenSingapore {},
        },
        ApiTarget::Provider {
            endpoint: ProviderEndpoint::QwenHongKong {},
        },
        ApiTarget::Provider {
            endpoint: ProviderEndpoint::A6Api {},
        },
        ApiTarget::Provider {
            endpoint: ProviderEndpoint::A6ApiVip {},
        },
    ]
}
pub(super) fn request(target: ApiTarget) -> ApiRequest {
    ApiRequest {
        reasoning_effort: None,
        target,
        model: "fixture-model".into(),
        instructions: "Perform the approved task within its scope.".into(),
        messages: vec![ApiMessage::User("Read approved file.".into())],
        tools: vec![ApiTool {
            name: "cerulean_read".into(),
            description: "Read approved text.".into(),
            parameters: json!({"type":"object","properties":{"path":{"type":"string"}},"required":["path"],"additionalProperties":false}),
        }],
        max_output_tokens: 1000,
        timeout_seconds: 2,
    }
}
pub(super) fn reply(r: &ApiRequest, tool: bool) -> Value {
    let call = json!({"path":"src/main.rs"});
    if matches!(r.target, ApiTarget::OpenAi {}) {
        json!({"id":"resp-1","model":"fixture-model-snapshot","status":"completed","output":if tool {json!([{"type":"reasoning","id":"rs-1","encrypted_content":"private-continuation","summary":[]},{"type":"function_call","status":"completed","call_id":"call-1","name":"cerulean_read","arguments":call.to_string()}])}else{json!([{"type":"message","id":"msg-1","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Done."}]}])},"usage":{"input_tokens":10,"output_tokens":20}})
    } else if r.target.provider() == CredentialProvider::Anthropic {
        json!({"id":"msg-1","model":"fixture-model-snapshot","type":"message","role":"assistant","stop_reason":if tool{"tool_use"}else{"end_turn"},"content":if tool{json!([{"type":"thinking","thinking":"private-continuation","signature":"signed-token"},{"type":"tool_use","id":"call-1","name":"cerulean_read","input":call}])}else{json!([{"type":"text","text":"Done."}])},"usage":{"input_tokens":10,"output_tokens":20}})
    } else {
        json!({"id":"chat-1","model":"fixture-model-snapshot","choices":[{"index":0,"finish_reason":if tool{"tool_calls"}else{"stop"},"message":if tool{json!({"role":"assistant","content":null,"reasoning_content":"private-continuation","tool_calls":[{"id":"call-1","type":"function","function":{"name":"cerulean_read","arguments":call.to_string()}}]})}else{json!({"role":"assistant","content":"Done."})}}],"usage":{"prompt_tokens":10,"completion_tokens":20}})
    }
}
#[test]
fn provider_turns_preserve_private_continuation_and_correlate_results() {
    let client = ApiClient::new(key()).unwrap();
    for target in targets() {
        let mut r = request(target);
        let first = client.prepare(r.clone()).unwrap();
        assert_eq!(first.body["stream"], false);
        let parsed = wire::parse(&r, reply(&r, true)).unwrap();
        assert_eq!(
            parsed.observation.tool_calls[0].arguments["path"],
            "src/main.rs"
        );
        assert!(!format!("{parsed:?}").contains("private-continuation"));
        assert!(!serde_json::to_string(parsed.observation())
            .unwrap()
            .contains("private-continuation"));
        r.messages.push(ApiMessage::Assistant(parsed));
        assert!(client.prepare(r.clone()).is_err());
        r.messages.push(ApiMessage::Tools(vec![ToolResult {
            call_id: "wrong-call".into(),
            text: "result".into(),
            is_error: false,
        }]));
        assert!(client.prepare(r.clone()).is_err());
        r.messages.pop();
        r.messages.push(ApiMessage::Tools(vec![ToolResult {
            call_id: "call-1".into(),
            text: "file not found".into(),
            is_error: true,
        }]));
        let second = client.prepare(r.clone()).unwrap();
        assert_ne!(
            first.fingerprint.body_sha256,
            second.fingerprint.body_sha256
        );
        assert!(second.body.to_string().contains("private-continuation"));
        assert!(second.body.to_string().contains("file not found"));
        let final_reply = wire::parse(&r, reply(&r, false)).unwrap();
        assert_eq!(final_reply.observation.text, "Done.");
        assert!(final_reply.observation.tool_calls.is_empty());
        r.messages.push(ApiMessage::Assistant(final_reply));
        r.messages
            .push(ApiMessage::User("Explain the result.".into()));
        let followup = client.prepare(r.clone()).unwrap();
        if matches!(r.target, ApiTarget::OpenAi {}) {
            assert_eq!(followup.body["input"][4]["id"], "msg-1");
        }
    }
}
#[test]
fn fixed_generation_endpoints_match_supported_regions() {
    let urls = [
        "https://api.openai.com/v1/responses",
        "https://api.anthropic.com/v1/messages",
        "https://api.deepseek.com/chat/completions",
        "https://api.moonshot.cn/v1/chat/completions",
        "https://api.x.ai/v1/chat/completions",
        "https://space-123.cn-beijing.maas.aliyuncs.com/compatible-mode/v1/chat/completions",
        "https://dashscope-intl.aliyuncs.com/compatible-mode/v1/chat/completions",
        "https://cn-hongkong.dashscope.aliyuncs.com/compatible-mode/v1/chat/completions",
        "https://api.a6api.com/v1/chat/completions",
    ];
    for (target, url) in targets().iter().zip(urls) {
        assert_eq!(target.url().unwrap(), url);
    }
}
#[test]
fn usage_respects_inclusive_and_separate_cache_counters() {
    let mut usage = ApiUsage {
        input_tokens: 10,
        output_tokens: 3,
        cache_read_tokens: Some(8),
        cache_write_tokens: None,
    };
    assert_eq!(usage.total_tokens(CredentialProvider::OpenAi).unwrap(), 13);
    assert_eq!(
        usage.total_tokens(CredentialProvider::Anthropic).unwrap(),
        21
    );
    usage.cache_write_tokens = Some(2);
    assert_eq!(
        usage.total_tokens(CredentialProvider::Anthropic).unwrap(),
        23
    );
    assert!(usage.total_tokens(CredentialProvider::DeepSeek).is_err());
    usage.input_tokens = u64::MAX;
    assert!(usage.total_tokens(CredentialProvider::Anthropic).is_err());
}
#[test]
fn billable_usage_requires_complete_cache_details_and_never_double_counts() {
    let mut usage = ApiUsage {
        input_tokens: 10,
        output_tokens: 3,
        cache_read_tokens: Some(8),
        cache_write_tokens: None,
    };
    assert!(usage.billable_usage(CredentialProvider::OpenAi).is_err());
    usage.cache_write_tokens = Some(0);
    let normalized = usage.billable_usage(CredentialProvider::OpenAi).unwrap();
    assert_eq!(
        normalized,
        cerulean_domain::currency_budget::Usage {
            input: 2,
            output: 3,
            cache_read: 8,
            cache_write: 0,
        }
    );
    usage.cache_write_tokens = None;
    assert!(usage.billable_usage(CredentialProvider::Anthropic).is_err());
    usage.cache_write_tokens = Some(2);
    assert_eq!(
        usage.billable_usage(CredentialProvider::Anthropic).unwrap(),
        cerulean_domain::currency_budget::Usage {
            input: 10,
            output: 3,
            cache_read: 8,
            cache_write: 2
        }
    );
    let inclusive = usage.billable_usage(CredentialProvider::OpenAi).unwrap();
    assert_eq!(
        (inclusive.input, inclusive.cache_read, inclusive.cache_write),
        (0, 8, 2)
    );
    usage.cache_write_tokens = Some(3);
    assert!(usage.billable_usage(CredentialProvider::OpenAi).is_err());
    usage.cache_write_tokens = None;
    usage.cache_read_tokens = None;
    assert!(usage.billable_usage(CredentialProvider::DeepSeek).is_err());
    assert_eq!(
        usage.total_tokens(CredentialProvider::DeepSeek).unwrap(),
        13
    );
    usage.cache_read_tokens = Some(0);
    assert_eq!(
        usage
            .billable_usage(CredentialProvider::DeepSeek)
            .unwrap()
            .input,
        10
    );
}

#[test]
fn openai_wire_preserves_cache_write_counts_and_rejects_invalid_breakdowns() {
    let r = request(ApiTarget::OpenAi {});
    let mut v = reply(&r, true);
    v["usage"] = json!({"input_tokens":100,"output_tokens":3,
        "input_tokens_details":{"cached_tokens":40,"cache_write_tokens":50}});
    let parsed = wire::parse(&r, v.clone()).unwrap();
    let billed = parsed
        .observation()
        .usage
        .billable_usage(CredentialProvider::OpenAi)
        .unwrap();
    assert_eq!(
        (billed.input, billed.cache_read, billed.cache_write),
        (10, 40, 50)
    );
    for invalid in [json!(-1), json!(1.5), json!("0"), json!(61)] {
        v["usage"]["input_tokens_details"]["cache_write_tokens"] = invalid;
        assert!(wire::parse(&r, v.clone()).is_err());
    }
}

#[test]
fn malformed_partial_unregistered_and_duplicate_calls_never_escape_decoder() {
    for target in targets() {
        let r = request(target);
        for case in [
            "partial",
            "unknown-tool",
            "bad-args",
            "duplicate",
            "usage",
            "empty",
            "error",
        ] {
            let mut v = reply(&r, true);
            if case == "usage" {
                v["usage"] = Value::Null;
            } else if case == "error" {
                v["error"] = json!({"message":"private-error"});
            } else if matches!(r.target, ApiTarget::OpenAi {}) {
                match case {
                    "partial" => v["status"] = json!("incomplete"),
                    "unknown-tool" => v["output"][1]["name"] = json!("shell"),
                    "bad-args" => v["output"][1]["arguments"] = json!("{"),
                    "duplicate" => {
                        let c = v["output"][1].clone();
                        v["output"].as_array_mut().unwrap().push(c);
                    }
                    "empty" => v["output"] = json!([]),
                    _ => unreachable!(),
                }
            } else if r.target.provider() == CredentialProvider::Anthropic {
                match case {
                    "partial" => v["stop_reason"] = json!("max_tokens"),
                    "unknown-tool" => v["content"][1]["name"] = json!("shell"),
                    "bad-args" => v["content"][1]["input"] = json!([]),
                    "duplicate" => {
                        let c = v["content"][1].clone();
                        v["content"].as_array_mut().unwrap().push(c);
                    }
                    "empty" => v["content"] = json!([]),
                    _ => unreachable!(),
                }
            } else {
                match case {
                    "partial" => v["choices"][0]["finish_reason"] = json!("length"),
                    "unknown-tool" => {
                        v["choices"][0]["message"]["tool_calls"][0]["function"]["name"] =
                            json!("shell")
                    }
                    "bad-args" => {
                        v["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"] =
                            json!("{")
                    }
                    "duplicate" => {
                        let c = v["choices"][0]["message"]["tool_calls"][0].clone();
                        v["choices"][0]["message"]["tool_calls"]
                            .as_array_mut()
                            .unwrap()
                            .push(c);
                    }
                    "empty" => v["choices"] = json!([]),
                    _ => unreachable!(),
                }
            }
            assert!(wire::parse(&r, v).is_err(), "{case}");
        }
    }
}
#[test]
fn request_limits_target_history_and_credential_echo_guard() {
    let client = ApiClient::new(key()).unwrap();
    let mut r = request(targets().remove(0));
    assert!(serde_json::from_value::<ApiTarget>(
        json!({"kind":"openAi","url":"https://evil.invalid"})
    )
    .is_err());
    r.instructions = key().expose().into();
    assert!(client.prepare(r.clone()).is_err());
    r.instructions = "scope".into();
    r.max_output_tokens = 0;
    assert!(client.prepare(r.clone()).is_err());
    r.max_output_tokens = 100;
    let parsed = wire::parse(&r, reply(&r, true)).unwrap();
    r.target = targets().remove(1);
    r.messages.push(ApiMessage::Assistant(parsed));
    r.messages.push(ApiMessage::Tools(vec![ToolResult {
        call_id: "call-1".into(),
        text: "ok".into(),
        is_error: false,
    }]));
    assert!(client.prepare(r).is_err());
    let escaped = format!(
        "\"{}\"",
        key()
            .expose()
            .chars()
            .map(|c| format!("\\u{:04x}", c as u32))
            .collect::<String>()
    );
    assert!(contains_secret(
        &serde_json::from_str(&escaped).unwrap(),
        key().expose()
    ));
}
#[tokio::test]
async fn http_auth_no_retries_redaction_bounds_and_cancellation() {
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    for target in targets() {
        for case in [
            "ok",
            "redirect",
            "unauthorized",
            "limited",
            "bad-json",
            "echo",
            "escaped-echo",
            "oversized",
            "chunked-large",
            "cancel",
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let r = request(target.clone());
            let raw = reply(&r, false).to_string();
            let anthropic = r.target.provider() == CredentialProvider::Anthropic;
            let (accepted, seen) = oneshot::channel();
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                let mut buffer = [0; 4096];
                loop {
                    let n = socket.read(&mut buffer).await.unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buffer[..n]);
                    if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let h = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                        let length: usize = h
                            .lines()
                            .find_map(|line| line.strip_prefix("content-length: "))
                            .unwrap()
                            .parse()
                            .unwrap();
                        if bytes.len() >= end + 4 + length {
                            assert!(h.starts_with("post /generate "));
                            assert!(h.contains(if anthropic {
                                "x-api-key: sk-synthetic-api-generation-only"
                            } else {
                                "authorization: bearer sk-synthetic-api-generation-only"
                            }));
                            assert_eq!(h.contains("anthropic-version: 2023-06-01"), anthropic);
                            assert!(!String::from_utf8_lossy(&bytes[end + 4..])
                                .contains(key().expose()));
                            break;
                        }
                    }
                }
                accepted.send(()).unwrap();
                let body = match case {
                    "bad-json" => "{".into(),
                    "echo" => json!({"error":key().expose()}).to_string(),
                    "escaped-echo" => format!(
                        "{{\"error\":\"{}\"}}",
                        key()
                            .expose()
                            .chars()
                            .map(|c| format!("\\u{:04x}", c as u32))
                            .collect::<String>()
                    ),
                    _ => raw,
                };
                if case == "cancel" {
                    let mut one = [0];
                    let _ =
                        tokio::time::timeout(Duration::from_secs(2), socket.read(&mut one)).await;
                } else if case == "chunked-large" {
                    let big = "x".repeat(MAX_RESPONSE + 1);
                    let header="HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n";
                    let _ = socket.write_all(header.as_bytes()).await;
                    let _ = socket
                        .write_all(format!("{:x}\r\n{big}\r\n0\r\n\r\n", big.len()).as_bytes())
                        .await;
                } else {
                    let status = match case {
                        "redirect" => "302 Found",
                        "unauthorized" => "401 Unauthorized",
                        "limited" => "429 Too Many Requests",
                        _ => "200 OK",
                    };
                    let size = if case == "oversized" {
                        MAX_RESPONSE + 1
                    } else {
                        body.len()
                    };
                    let _=socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: {size}\r\nLocation: http://{addr}/redirected\r\nConnection: close\r\n\r\n{body}").as_bytes()).await;
                }
                drop(socket);
                assert!(
                    tokio::time::timeout(Duration::from_millis(30), listener.accept())
                        .await
                        .is_err(),
                    "unexpected retry"
                );
            });
            let client = ApiClient {
                http: ApiClient::http(false).unwrap(),
                key: key(),
            };
            let prepared = client.prepare(r).unwrap();
            let (cancel_tx, mut cancel) = oneshot::channel();
            let control = tokio::spawn(async move {
                seen.await.unwrap();
                if case == "cancel" {
                    cancel_tx.send(()).unwrap();
                    None
                } else {
                    Some(cancel_tx)
                }
            });
            let result = client
                .send_url(prepared, &mut cancel, &format!("http://{addr}/generate"))
                .await;
            match case {
                "ok" => assert_eq!(result.unwrap().observation.text, "Done."),
                "redirect" => assert_eq!(result.unwrap_err(), ApiFailure::Http { status: 302 }),
                "unauthorized" => assert_eq!(result.unwrap_err(), ApiFailure::Http { status: 401 }),
                "limited" => assert_eq!(result.unwrap_err(), ApiFailure::Http { status: 429 }),
                "oversized" | "chunked-large" => {
                    assert_eq!(result.unwrap_err(), ApiFailure::Oversized {})
                }
                "cancel" => assert_eq!(result.unwrap_err(), ApiFailure::LocalCancellation {}),
                "bad-json" => assert_eq!(
                    result.unwrap_err(),
                    ApiFailure::ResponseRejected {
                        reason: ResponseRejection::InvalidJson
                    }
                ),
                "echo" | "escaped-echo" => assert_eq!(
                    result.unwrap_err(),
                    ApiFailure::ResponseRejected {
                        reason: ResponseRejection::CredentialEcho
                    }
                ),
                _ => unreachable!(),
            }
            server.await.unwrap();
            let _cancel_sender = control.await.unwrap();
        }
    }
}
#[tokio::test]
async fn cancellation_before_dispatch_does_not_connect() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/generate", listener.local_addr().unwrap());
    let client = ApiClient {
        http: ApiClient::http(false).unwrap(),
        key: key(),
    };
    let prepared = client.prepare(request(targets().remove(0))).unwrap();
    let (tx, mut rx) = oneshot::channel();
    tx.send(()).unwrap();
    assert_eq!(
        client.send_url(prepared, &mut rx, &url).await.unwrap_err(),
        ApiFailure::NotSent {}
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(30), listener.accept())
            .await
            .is_err()
    );
}
