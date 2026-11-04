use super::*;
use crate::omp_protocol::PromptStatus;
use tokio::io::{
    duplex, split, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf,
    WriteHalf,
};
type Client = WorkerTransport<BufReader<ReadHalf<DuplexStream>>, WriteHalf<DuplexStream>>;
type Reader = BufReader<ReadHalf<DuplexStream>>;
type Writer = WriteHalf<DuplexStream>;
pub(super) const PROMPT: &str = "Implement the approved local fixture task.";

#[tokio::test]
async fn terminal_before_ack_suppresses_late_abort_but_still_requires_ack() {
    let (mut client, peer) = pair();
    let (cancel, mut rx) = oneshot::channel();
    let peer = tokio::spawn(async move {
        let (mut reader, mut writer) = setup(peer, true).await;
        let prompt = read(&mut reader).await;
        send(&mut writer, result("completed")).await;
        cancel.send(()).unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        respond(&mut writer, &prompt, Value::Null).await;
        let request = read(&mut reader).await;
        assert_eq!(request["type"], "get_state");
        respond(&mut writer, &request, state()).await;
        eof(&mut reader).await;
    });
    client.connect(Duration::from_secs(2)).await.unwrap();
    assert!(matches!(
        client
            .execute(
                intent(),
                PROMPT,
                Duration::from_secs(2),
                &mut rx,
                |_| Ok(())
            )
            .await
            .unwrap(),
        PromptOutcome::Finished(PromptReceipt {
            status: PromptStatus::Completed,
            ..
        })
    ));
    assert!(!client.command_attempts().contains_key("abort"));
    client.close_input().await.unwrap();
    peer.await.unwrap();
}

#[tokio::test]
async fn cancellation_before_prompt_ack_and_under_observation_flood_is_single_shot() {
    for early in [true, false] {
        let (mut client, peer) = pair();
        let (cancel, mut rx) = oneshot::channel();
        let peer = tokio::spawn(async move {
            let (mut reader, mut writer) = setup(peer, true).await;
            let prompt = read(&mut reader).await;
            if !early {
                respond(&mut writer, &prompt, Value::Null).await;
            }
            cancel.send(()).unwrap();
            if !early {
                for _ in 0..96 {
                    send(&mut writer, json!({"type":"message_update"})).await;
                }
            }
            let abort = read(&mut reader).await;
            assert_eq!(abort["type"], "abort");
            respond(&mut writer, &abort, Value::Null).await;
            send(&mut writer, result("aborted")).await;
            if early {
                respond(&mut writer, &prompt, Value::Null).await;
            }
            let request = read(&mut reader).await;
            assert_eq!(request["type"], "get_state");
            respond(&mut writer, &request, state()).await;
            eof(&mut reader).await;
        });
        client.connect(Duration::from_secs(2)).await.unwrap();
        assert!(matches!(
            client
                .execute(
                    intent(),
                    PROMPT,
                    Duration::from_secs(2),
                    &mut rx,
                    |_| Ok(())
                )
                .await
                .unwrap(),
            PromptOutcome::Finished(PromptReceipt {
                status: PromptStatus::Aborted,
                ..
            })
        ));
        assert_eq!(client.command_attempts().get("abort"), Some(&1));
        assert_eq!(client.command_attempts().get("prompt"), Some(&1));
        client.close_input().await.unwrap();
        peer.await.unwrap();
    }
}

#[tokio::test]
async fn dropped_worker_future_and_failed_ack_journal_never_replay() {
    for drop_wait in [true, false] {
        let (mut client, peer) = pair();
        let (_cancel, mut rx) = oneshot::channel();
        let (observed, when_observed) = oneshot::channel();
        let peer = tokio::spawn(async move {
            let (mut reader, mut writer) = setup(peer, true).await;
            let prompt = read(&mut reader).await;
            if drop_wait {
                writer.write_all(b"{\"type\":").await.unwrap();
            } else {
                respond(&mut writer, &prompt, Value::Null).await;
            }
            observed.send(()).unwrap();
            eof(&mut reader).await;
        });
        client.connect(Duration::from_secs(2)).await.unwrap();
        if drop_wait {
            tokio::select! {
                response=client.execute(intent(),PROMPT,Duration::from_secs(2),&mut rx,|_|Ok(()))=>panic!("unexpected result {response:?}"),
                _=when_observed=>{},
            }
        } else {
            assert!(client
                .execute(intent(), PROMPT, Duration::from_secs(2), &mut rx, |e| {
                    if matches!(e, WorkerEvent::PromptAcknowledged { .. }) {
                        Err("fixture journal failure".into())
                    } else {
                        Ok(())
                    }
                })
                .await
                .is_err());
        }
        assert!(client
            .execute(
                intent(),
                PROMPT,
                Duration::from_secs(1),
                &mut rx,
                |_| Ok(())
            )
            .await
            .is_err());
        assert_eq!(client.command_attempts().get("prompt"), Some(&1));
        client.close_input().await.unwrap();
        peer.await.unwrap();
    }
}
pub(super) fn intent() -> DispatchIntent {
    DispatchIntent {
        version: 1,
        task_id: "task".into(),
        attempt_id: "attempt".into(),
        revision_digest: "a".repeat(64),
        fence: 1,
        binding: SessionBinding {
            adapter_id: "omp".into(),
            adapter_version: "18.4.4".into(),
            session_id: "session".into(),
            workspace: std::env::current_dir().unwrap().to_string_lossy().into(),
            identity_strength: IdentityStrength::ControllerRequest(ControllerRequestIdentity {
                connection_id: "connection".into(),
                request_id: "prompt-fixture".into(),
            }),
        },
        provider: "deepseek".into(),
        model: "fixture".into(),
        prompt_sha256: format!("{:x}", Sha256::digest(PROMPT.as_bytes())),
    }
}
pub(super) fn state() -> Value {
    json!({"sessionId":"session","model":{"provider":"deepseek","id":"fixture"},"isStreaming":false,"isCompacting":false,"hasPendingAsyncWork":false,"isSettled":true,"queuedMessageCount":0,"dumpTools":[]})
}
pub(super) fn result(status: &str) -> Value {
    json!({"type":"prompt_result","id":"prompt-fixture","agentInvoked":true,"sessionSettled":true,"status":status})
}
pub(super) async fn read(reader: &mut Reader) -> Value {
    let mut line = String::new();
    assert!(reader.read_line(&mut line).await.unwrap() > 0);
    serde_json::from_str(&line).unwrap()
}
pub(super) async fn send(writer: &mut Writer, value: Value) {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    writer.write_all(&bytes).await.unwrap();
}
pub(super) async fn respond(writer: &mut Writer, request: &Value, data: Value) {
    send(writer,json!({"type":"response","id":request["id"],"command":request["type"],"success":true,"data":data})).await;
}
pub(super) async fn setup(peer: DuplexStream, preflight: bool) -> (Reader, Writer) {
    let (read_half, mut writer) = split(peer);
    let mut reader = BufReader::new(read_half);
    send(&mut writer,json!({"type":"ready","protocolVersion":1,"supportedProtocolVersions":[1,2],"maxFrameBytes":super::super::FRAME_LIMIT,"maxReassembledFrameBytes":super::super::REASSEMBLED_LIMIT})).await;
    let request = read(&mut reader).await;
    assert_eq!(request["type"], "negotiate_protocol");
    respond(&mut writer, &request, json!({"protocolVersion":2})).await;
    for _ in 0..if preflight { 2 } else { 1 } {
        let request = read(&mut reader).await;
        assert_eq!(request["type"], "get_state");
        respond(&mut writer, &request, state()).await;
    }
    (reader, writer)
}
pub(super) fn pair() -> (Client, DuplexStream) {
    let (client, peer) = duplex(8192);
    let (reader, writer) = split(client);
    (WorkerTransport::new(BufReader::new(reader), writer), peer)
}
pub(super) async fn eof(reader: &mut Reader) {
    let mut tail = Vec::new();
    reader.read_to_end(&mut tail).await.unwrap();
    assert!(tail.is_empty(), "unexpected extra command");
}

#[tokio::test]
async fn review_requires_complete_reply_and_rejects_tool_requests_in_stream() {
    for mode in [
        "clean",
        "toolcall_start",
        "unknown",
        "host-tool",
        "missing-reply",
    ] {
        let (mut client, peer) = pair();
        let (_cancel, mut cancellation) = oneshot::channel();
        let peer = tokio::spawn(async move {
            let (mut reader, mut writer) = setup(peer, true).await;
            let prompt = read(&mut reader).await;
            respond(&mut writer, &prompt, Value::Null).await;
            if mode == "host-tool" {
                send(&mut writer, json!({"type":"host_tool_call","id":"action"})).await;
            } else if matches!(mode, "toolcall_start" | "unknown") {
                send(
                    &mut writer,
                    json!({"type":"message_update","assistantMessageEvent":{"type":mode}}),
                )
                .await;
            } else {
                if mode == "clean" {
                    send(&mut writer, json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"PASS"}})).await;
                    send(&mut writer, json!({"type":"message_end","message":{"role":"assistant","model":"fixture","content":[{"type":"text","text":"PASS"}]}})).await;
                }
                send(&mut writer, result("completed")).await;
                let request = read(&mut reader).await;
                assert_eq!(request["type"], "get_state");
                respond(&mut writer, &request, state()).await;
            }
            eof(&mut reader).await;
        });
        client.connect(Duration::from_secs(2)).await.unwrap();
        let outcome = client
            .execute_review(
                intent(),
                PROMPT,
                Duration::from_secs(2),
                &mut cancellation,
                |_| Ok(()),
            )
            .await;
        match mode {
            "clean" => assert_eq!(outcome.unwrap().1, "PASS"),
            "host-tool" | "toolcall_start" => {
                assert!(outcome.unwrap_err().starts_with("review_tool_observed:"))
            }
            _ => assert!(outcome.is_err()),
        }
        assert_eq!(client.command_attempts().get("prompt"), Some(&1));
        client.close_input().await.unwrap();
        peer.await.unwrap();
    }
}

#[tokio::test]
async fn one_prompt_waits_for_ack_bound_result_and_fresh_idle_state() {
    let (mut client, peer) = pair();
    let (_cancel, mut rx) = oneshot::channel();
    let peer = tokio::spawn(async move {
        let (mut reader, mut writer) = setup(peer, true).await;
        let prompt = read(&mut reader).await;
        assert_eq!(prompt["type"], "prompt");
        assert_eq!(prompt["message"], PROMPT);
        send(
            &mut writer,
            json!({"type":"message_update","privateThinking":"never-record-this"}),
        )
        .await;
        send(&mut writer, result("completed")).await;
        respond(&mut writer, &prompt, Value::Null).await;
        let request = read(&mut reader).await;
        assert_eq!(request["type"], "get_state");
        let mut busy = state();
        busy["hasPendingAsyncWork"] = json!(true);
        respond(&mut writer, &request, busy).await;
        let request = read(&mut reader).await;
        respond(&mut writer, &request, state()).await;
        eof(&mut reader).await;
    });
    client.connect(Duration::from_secs(2)).await.unwrap();
    let mut events = Vec::new();
    let outcome = client
        .execute(intent(), PROMPT, Duration::from_secs(2), &mut rx, |event| {
            events.push(serde_json::to_value(event).unwrap());
            Ok(())
        })
        .await
        .unwrap();
    let PromptOutcome::Finished(receipt) = outcome else {
        panic!("no completion")
    };
    assert_eq!(receipt.status, PromptStatus::Completed);
    assert_eq!(receipt.request_id, "prompt-fixture");
    assert_eq!(
        events
            .iter()
            .map(|v| v["kind"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["dispatch-intent", "prompt-acknowledged", "terminal"]
    );
    assert!(!serde_json::to_string(&events)
        .unwrap()
        .contains("never-record-this"));
    assert!(client
        .execute(
            intent(),
            PROMPT,
            Duration::from_secs(1),
            &mut rx,
            |_| Ok(())
        )
        .await
        .is_err());
    assert_eq!(client.command_attempts().get("prompt"), Some(&1));
    client.close_input().await.unwrap();
    peer.await.unwrap();
}

#[tokio::test]
async fn cancellation_preserves_partial_frame_and_waits_for_terminal_after_abort_ack() {
    let (mut client, peer) = pair();
    let (cancel, mut rx) = oneshot::channel();
    let peer = tokio::spawn(async move {
        let (mut reader, mut writer) = setup(peer, true).await;
        let prompt = read(&mut reader).await;
        respond(&mut writer, &prompt, Value::Null).await;
        writer
            .write_all(b"{\"type\":\"message_update\"")
            .await
            .unwrap();
        cancel.send(()).unwrap();
        let abort = read(&mut reader).await;
        assert_eq!(abort["type"], "abort");
        writer.write_all(b"}\n").await.unwrap();
        respond(&mut writer, &abort, Value::Null).await;
        send(&mut writer, json!({"type":"agent_end"})).await;
        send(&mut writer, result("aborted")).await;
        let request = read(&mut reader).await;
        assert_eq!(request["type"], "get_state");
        respond(&mut writer, &request, state()).await;
        eof(&mut reader).await;
    });
    client.connect(Duration::from_secs(2)).await.unwrap();
    let mut events = Vec::new();
    let outcome = client
        .execute(intent(), PROMPT, Duration::from_secs(2), &mut rx, |e| {
            events.push(serde_json::to_value(e).unwrap());
            Ok(())
        })
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        PromptOutcome::Finished(PromptReceipt {
            status: PromptStatus::Aborted,
            ..
        })
    ));
    assert_eq!(client.command_attempts().get("abort"), Some(&1));
    assert_eq!(events.last().unwrap()["abort_acknowledged"], true);
    client.close_input().await.unwrap();
    peer.await.unwrap();
}

#[tokio::test]
async fn lost_abort_ack_cannot_turn_agent_end_into_a_completed_request() {
    let (mut client, peer) = pair();
    let (cancel, mut rx) = oneshot::channel();
    let peer = tokio::spawn(async move {
        let (mut reader, mut writer) = setup(peer, true).await;
        let prompt = read(&mut reader).await;
        respond(&mut writer, &prompt, Value::Null).await;
        cancel.send(()).unwrap();
        assert_eq!(read(&mut reader).await["type"], "abort");
        send(&mut writer, json!({"type":"agent_end"})).await;
        eof(&mut reader).await;
    });
    client.connect(Duration::from_secs(2)).await.unwrap();
    let mut terminals = 0;
    assert!(client
        .execute(intent(), PROMPT, Duration::from_millis(100), &mut rx, |e| {
            if matches!(e, WorkerEvent::Terminal { .. }) {
                terminals += 1;
            }
            Ok(())
        })
        .await
        .is_err());
    assert_eq!(terminals, 0);
    assert_eq!(client.command_attempts().get("prompt"), Some(&1));
    assert_eq!(client.command_attempts().get("abort"), Some(&1));
    assert!(client
        .execute(
            intent(),
            PROMPT,
            Duration::from_secs(1),
            &mut rx,
            |_| Ok(())
        )
        .await
        .is_err());
    client.close_input().await.unwrap();
    peer.await.unwrap();
}

#[tokio::test]
async fn journal_failures_cancel_before_send_and_invalid_inputs_never_send_prompt() {
    for mode in [
        "journal",
        "cancel-before",
        "cancel-in-journal",
        "bad-digest",
        "slash",
        "provider-id",
    ] {
        let invalid = matches!(mode, "bad-digest" | "slash" | "provider-id");
        let (mut client, peer) = pair();
        let (cancel, mut rx) = oneshot::channel();
        let mut cancel = Some(cancel);
        let peer = tokio::spawn(async move {
            let (mut reader, _writer) = setup(peer, !invalid).await;
            eof(&mut reader).await;
        });
        client.connect(Duration::from_secs(2)).await.unwrap();
        let mut bound = intent();
        let mut prompt = PROMPT;
        match mode {
            "cancel-before" => cancel.take().unwrap().send(()).unwrap(),
            "bad-digest" => bound.prompt_sha256 = "b".repeat(64),
            "slash" => {
                prompt = "/retry";
                bound.prompt_sha256 = format!("{:x}", Sha256::digest(prompt.as_bytes()));
            }
            "provider-id" => bound.binding.identity_strength = IdentityStrength::ProviderId,
            _ => {}
        }
        let outcome = client
            .execute(bound, prompt, Duration::from_secs(2), &mut rx, |e| {
                if mode == "journal" {
                    return Err("fixture-journal-unavailable".into());
                }
                if mode == "cancel-in-journal" && matches!(e, WorkerEvent::DispatchIntent { .. }) {
                    cancel.take().unwrap().send(()).unwrap();
                }
                Ok(())
            })
            .await;
        if mode.starts_with("cancel") {
            assert!(matches!(outcome, Ok(PromptOutcome::NotStarted)));
        } else {
            assert!(outcome.is_err());
        }
        assert!(!client.command_attempts().contains_key("prompt"));
        client.close_input().await.unwrap();
        peer.await.unwrap();
    }
}

#[tokio::test]
async fn host_actions_retry_wrong_result_and_changed_final_state_are_rejected() {
    for mode in [
        "tool",
        "retry",
        "foreign-result",
        "changed-model",
        "duplicate-ack",
        "local-command",
    ] {
        let (mut client, peer) = pair();
        let (_cancel, mut rx) = oneshot::channel();
        let peer = tokio::spawn(async move {
            let (mut reader, mut writer) = setup(peer, true).await;
            let prompt = read(&mut reader).await;
            if mode == "local-command" {
                respond(&mut writer, &prompt, json!({"agentInvoked":false})).await;
            } else {
                respond(&mut writer, &prompt, Value::Null).await;
                match mode {
                    "tool"=>send(&mut writer,json!({"type":"host_tool_call","id":"action","arguments":{"secret":"never-retain"}})).await,
                    "retry"=>send(&mut writer,json!({"type":"auto_retry_start"})).await,
                    "foreign-result"=>{let mut value=result("completed");value["id"]=json!("other");send(&mut writer,value).await;},
                    "duplicate-ack"=>respond(&mut writer,&prompt,Value::Null).await,
                    _=>{send(&mut writer,result("completed")).await;let request=read(&mut reader).await;let mut value=state();value["model"]["id"]=json!("other");respond(&mut writer,&request,value).await;},
                }
            }
            eof(&mut reader).await;
        });
        client.connect(Duration::from_secs(2)).await.unwrap();
        let mut terminal = false;
        let error = client
            .execute(intent(), PROMPT, Duration::from_secs(2), &mut rx, |e| {
                terminal |= matches!(e, WorkerEvent::Terminal { .. });
                Ok(())
            })
            .await
            .unwrap_err();
        assert!(!terminal && !error.contains("never-retain"));
        client.close_input().await.unwrap();
        peer.await.unwrap();
    }
}

#[tokio::test]
async fn completion_already_readable_wins_cancel_and_closed_sender_is_not_cancellation() {
    for cancel_now in [true, false] {
        let (mut client, peer) = pair();
        let (cancel, mut rx) = oneshot::channel();
        let peer = tokio::spawn(async move {
            let (mut reader, mut writer) = setup(peer, true).await;
            let prompt = read(&mut reader).await;
            respond(&mut writer, &prompt, Value::Null).await;
            send(&mut writer, result("completed")).await;
            if cancel_now {
                cancel.send(()).unwrap();
            } else {
                drop(cancel);
            }
            let request = read(&mut reader).await;
            assert_eq!(request["type"], "get_state");
            respond(&mut writer, &request, state()).await;
            eof(&mut reader).await;
        });
        client.connect(Duration::from_secs(2)).await.unwrap();
        assert!(matches!(
            client
                .execute(
                    intent(),
                    PROMPT,
                    Duration::from_secs(2),
                    &mut rx,
                    |_| Ok(())
                )
                .await
                .unwrap(),
            PromptOutcome::Finished(PromptReceipt {
                status: PromptStatus::Completed,
                ..
            })
        ));
        assert!(!client.command_attempts().contains_key("abort"));
        client.close_input().await.unwrap();
        peer.await.unwrap();
    }
}
