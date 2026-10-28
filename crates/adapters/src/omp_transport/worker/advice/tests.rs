use super::*;
use crate::omp_transport::worker::{
    host_calls::{HostEvent, HostFuture, HostOutput, HostRequest, HostService},
    host_tools::HostToolDefinition,
    tests::{eof, intent, pair, read, respond, result, send, setup, state, PROMPT},
};
use std::sync::{Arc, Mutex};
use tokio::sync::watch;

fn message(operation: &str) -> String {
    format!("[cerulean-advice:{operation}]\nInspect the approved path before retrying.")
}
fn permit(events: Arc<Mutex<Vec<AdviceEvent>>>, fail: Option<&'static str>) -> SteerAdvice {
    SteerAdvice {
        operation_id: "advice-fixture".into(),
        message: message("advice-fixture"),
        may_send: Box::new(|| true),
        observe: Box::new(move |event| {
            if matches!(
                (fail, event),
                (Some("intent"), AdviceEvent::SendIntent { .. })
                    | (Some("released"), AdviceEvent::ToolResultReleased { .. })
                    | (Some("revoked"), AdviceEvent::RevokedBeforeSend)
            ) {
                return Err("fixture journal unavailable".into());
            }
            events.lock().unwrap().push(event.clone());
            Ok(())
        }),
    }
}
fn started(events: Arc<Mutex<Vec<AdviceEvent>>>) -> ActiveAdvice {
    match ActiveAdvice::begin(
        permit(events, None),
        "cerulean-omp-9".into(),
        &intent().validate_metadata().unwrap().0,
    )
    .unwrap()
    {
        PreparedAdvice::Send(advice) => advice,
        PreparedAdvice::Revoked(_) => panic!("fixture was unexpectedly revoked"),
    }
}
fn ack() -> Value {
    json!({"type":"response","id":"cerulean-omp-9","command":"steer","success":true})
}
fn queued() -> Value {
    let mut data = state();
    data["isStreaming"] = json!(true);
    data["isSettled"] = json!(false);
    data["queuedMessageCount"] = json!(1);
    data["queuedMessages"] = json!({"steering":[message("advice-fixture")],"followUp":[]});
    json!({"type":"response","id":"cerulean-omp-10","command":"get_state","success":true,"data":data})
}
fn user_event(kind: &str, id: &str) -> Value {
    json!({"type":kind,"messageId":id,"message":{"role":"user","content":[{"type":"text","text":message("advice-fixture")}]}})
}
#[test]
fn native_advice_requires_exact_active_scope_queue_and_message_pair() {
    for mutation in [
        "session",
        "provider",
        "model",
        "idle",
        "compacting",
        "duplicate",
        "foreign",
        "count",
        "followup",
    ] {
        let mut advice = started(Default::default());
        advice.accept(&ack()).unwrap();
        advice.state_id = Some("cerulean-omp-10".into());
        let mut q = queued();
        match mutation {
            "session" => q["data"]["sessionId"] = json!("other"),
            "provider" => q["data"]["model"]["provider"] = json!("other"),
            "model" => q["data"]["model"]["id"] = json!("other"),
            "idle" => q["data"]["isStreaming"] = json!(false),
            "compacting" => q["data"]["isCompacting"] = json!(true),
            "duplicate" => {
                q["data"]["queuedMessages"]["steering"] =
                    json!([message("advice-fixture"), message("advice-fixture")])
            }
            "foreign" => q["data"]["queuedMessages"]["steering"] = json!(["quoted suggestion"]),
            "count" => q["data"]["queuedMessageCount"] = json!(2),
            "followup" => q["data"]["queuedMessages"]["followUp"] = json!(["foreign"]),
            _ => unreachable!(),
        }
        assert!(advice.queue(&q).is_err(), "{mutation}");
    }
    let events: Arc<Mutex<Vec<AdviceEvent>>> = Arc::default();
    let mut advice = started(events.clone());
    assert!(!advice.complete());
    advice.accept(&ack()).unwrap();
    assert!(!advice.complete());
    advice.state_id = Some("cerulean-omp-10".into());
    advice.queue(&queued()).unwrap();
    assert!(!advice.complete());
    advice
        .released(&json!({"type":"host_tool_result","id":"1"}))
        .unwrap();
    let mut quote = user_event("message_start", "m1");
    quote["message"]["role"] = json!("assistant");
    advice.message(&quote).unwrap();
    assert!(!advice.complete());
    advice.message(&user_event("message_start", "m1")).unwrap();
    assert!(advice
        .message(&user_event("message_end", "different"))
        .is_err());
    advice.message(&user_event("message_end", "m1")).unwrap();
    assert!(advice.complete());
    assert!(advice.message(&user_event("message_end", "m1")).is_err());
    assert_eq!(events.lock().unwrap().len(), 5);
}
#[test]
fn advice_permission_is_bounded_and_persistence_precedes_wire() {
    let identity = intent().validate_metadata().unwrap().0;
    assert!(ActiveAdvice::begin(
        permit(Default::default(), Some("intent")),
        "cerulean-omp-9".into(),
        &identity
    )
    .is_err());
    for bad in [
        "/shell do things".into(),
        "no operation marker".into(),
        format!("{}{}", message("advice-fixture"), "x".repeat(21 * 1024)),
    ] {
        let mut p = permit(Default::default(), None);
        p.message = bad;
        assert!(ActiveAdvice::begin(p, "cerulean-omp-9".into(), &identity).is_err());
    }
    let mut advice = started(Default::default());
    assert!(advice.released(&json!({})).is_err());
    let mut wrong = ack();
    wrong["id"] = json!("other");
    assert!(advice.accept(&wrong).is_err());
    advice.accept(&ack()).unwrap();
    assert!(advice.accept(&ack()).is_err());
    assert!(advice.message(&user_event("message_end", "m1")).is_err());
}

#[test]
fn original_advice_messages_can_cross_command_ack_and_queue_snapshot() {
    for empty_queue in [false, true] {
        let events: Arc<Mutex<Vec<AdviceEvent>>> = Arc::default();
        let mut advice = started(events.clone());
        advice
            .message(&user_event("message_start", "early"))
            .unwrap();
        advice.message(&user_event("message_end", "early")).unwrap();
        assert_eq!(events.lock().unwrap().len(), 1); // no acknowledgement yet
        advice.accept(&ack()).unwrap();
        assert!(
            matches!(events.lock().unwrap().last(), Some(AdviceEvent::MessageObserved { message_id }) if message_id=="early")
        );
        advice.state_id = Some("cerulean-omp-10".into());
        let mut q = queued();
        if empty_queue {
            q["data"]["queuedMessages"]["steering"] = json!([]);
            q["data"]["queuedMessageCount"] = json!(0);
        }
        advice.queue(&q).unwrap();
        assert!(!advice.complete());
        advice
            .released(&json!({"type":"host_tool_result","id":"1"}))
            .unwrap();
        assert!(advice.complete());
    }
}

#[test]
fn revoked_reservation_can_only_record_unchanged_result_release() {
    let events: Arc<Mutex<Vec<AdviceEvent>>> = Arc::default();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed_calls = calls.clone();
    let mut permit = permit(events.clone(), None);
    permit.may_send = Box::new(move || {
        observed_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        false
    });
    let PreparedAdvice::Revoked(revoked) = ActiveAdvice::begin(
        permit,
        "cerulean-omp-9".into(),
        &intent().validate_metadata().unwrap().0,
    )
    .unwrap() else {
        panic!("revoked reservation produced a send");
    };
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        *events.lock().unwrap(),
        vec![AdviceEvent::RevokedBeforeSend]
    );
    let result = json!({"type":"host_tool_result","id":"original","result":{"content":[{"type":"text","text":"original output"}]},"isError":false});
    revoked.released(&result).unwrap();
    assert_eq!(
        *events.lock().unwrap(),
        vec![
            AdviceEvent::RevokedBeforeSend,
            AdviceEvent::ToolResultReleased {
                result_sha256: result_hash(&result).unwrap()
            }
        ]
    );
}

fn tool() -> HostToolDefinition {
    HostToolDefinition {
        name: "cerulean_fixture".into(),
        description: "Advice fixture".into(),
        parameters: json!({"type":"object","properties":{},"additionalProperties":false}),
    }
}
struct Service {
    boundary: Option<AdviceBoundary>,
    sender: Option<oneshot::Sender<Result<Option<SteerAdvice>, String>>>,
    events: Arc<Mutex<Vec<AdviceEvent>>>,
    tools: Arc<Mutex<Vec<HostEvent>>>,
    mode: &'static str,
}
impl HostService for Service {
    fn record(&mut self, _: &DispatchIntent, event: &HostEvent) -> Result<(), String> {
        self.tools.lock().unwrap().push(event.clone());
        if matches!(event, HostEvent::Result { .. }) {
            let (tx, rx) = oneshot::channel();
            self.boundary = Some(rx);
            if matches!(self.mode, "cancel-wait" | "terminal-wait" | "timeout-wait") {
                self.sender = Some(tx);
            } else if self.mode == "no-advice" {
                assert!(tx.send(Ok(None)).is_ok());
            } else if self.mode != "closed-owner" {
                let fail = match self.mode {
                    "failed-intent" => Some("intent"),
                    "failed-release" => Some("released"),
                    "failed-revocation" => Some("revoked"),
                    _ => None,
                };
                let mut advice = permit(self.events.clone(), fail);
                if matches!(self.mode, "revoked" | "failed-revocation") {
                    advice.may_send = Box::new(|| false);
                }
                assert!(tx.send(Ok(Some(advice))).is_ok());
            }
        }
        Ok(())
    }
    fn start(&mut self, _: HostRequest, _: watch::Receiver<bool>) -> HostFuture {
        Box::pin(async {
            Ok(HostOutput {
                text: "unchanged-output".into(),
                is_error: true,
            })
        })
    }
    fn take_advice_boundary(
        &mut self,
        _: &DispatchIntent,
    ) -> Result<Option<AdviceBoundary>, String> {
        if self.mode == "boundary-error" {
            return Err("fixture boundary publication failed".into());
        }
        Ok(self.boundary.take())
    }
}

#[tokio::test]
async fn original_prompt_native_advice_keeps_tool_bytes_and_never_resends() {
    for mode in [
        "success",
        "queue-missing",
        "missing-message",
        "message-id",
        "failed-intent",
        "failed-release",
        "closed-owner",
        "cancel-wait",
        "cancel-queue",
        "terminal-wait",
        "boundary-error",
        "timeout-wait",
        "no-advice",
        "revoked",
        "failed-revocation",
    ] {
        let (mut client, peer) = pair();
        let (cancel_tx, mut rx) = oneshot::channel();
        let events: Arc<Mutex<Vec<AdviceEvent>>> = Arc::default();
        let tools: Arc<Mutex<Vec<HostEvent>>> = Arc::default();
        let peer_events = events.clone();
        let peer_tools = tools.clone();
        let peer = tokio::spawn(async move {
            let (mut r, mut w) = setup(peer, true).await;
            let q = read(&mut r).await;
            assert_eq!(q["type"], "set_host_tools");
            respond(&mut w, &q, json!({"toolNames":[tool().name]})).await;
            let mut idle = state();
            idle["dumpTools"] = json!([tool()]);
            for _ in 0..2 {
                let q = read(&mut r).await;
                assert_eq!(q["type"], "get_state");
                respond(&mut w, &q, idle.clone()).await;
            }
            let prompt = read(&mut r).await;
            respond(&mut w, &prompt, Value::Null).await;
            send(&mut w,json!({"type":"host_tool_call","id":"1","toolCallId":"model-1","toolName":tool().name,"arguments":{}})).await;
            if matches!(
                mode,
                "closed-owner"
                    | "failed-intent"
                    | "boundary-error"
                    | "timeout-wait"
                    | "failed-revocation"
            ) {
                eof(&mut r).await;
                return;
            }
            if mode == "terminal-wait" {
                send(&mut w, result("completed")).await;
                eof(&mut r).await;
                return;
            }
            if mode == "cancel-wait" {
                while !peer_tools
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|e| matches!(e, HostEvent::Result { .. }))
                {
                    tokio::task::yield_now().await;
                }
                cancel_tx.send(()).unwrap();
                let abort = read(&mut r).await;
                assert_eq!(abort["type"], "abort");
                respond(&mut w, &abort, Value::Null).await;
                send(&mut w, result("aborted")).await;
                let q = read(&mut r).await;
                respond(&mut w, &q, idle).await;
                eof(&mut r).await;
                return;
            }
            if !matches!(mode, "no-advice" | "revoked") {
                let steer = read(&mut r).await;
                assert_eq!(steer["type"], "steer");
                assert_eq!(steer["message"], message("advice-fixture"));
                assert!(matches!(
                    peer_events.lock().unwrap().first(),
                    Some(AdviceEvent::SendIntent { .. })
                ));
                send(&mut w, json!({"type":"queue_update","steering":[message("advice-fixture")],"followUp":[]})).await;
                respond(&mut w, &steer, Value::Null).await;
                // Notifications can race the command ACK. Even this conflicting
                // display payload must not grant release or replace get_state.
                send(&mut w, json!({"type":"queue_update","steering":["untrusted display text"],"followUp":[]})).await;
                let q = read(&mut r).await;
                assert_eq!(q["type"], "get_state");
                if mode == "cancel-queue" {
                    cancel_tx.send(()).unwrap();
                    let abort = read(&mut r).await;
                    assert_eq!(abort["type"], "abort");
                    respond(&mut w, &abort, Value::Null).await;
                    send(&mut w, result("aborted")).await;
                    let q = read(&mut r).await;
                    respond(&mut w, &q, idle).await;
                    eof(&mut r).await;
                    return;
                }
                let mut active = queued()["data"].clone();
                if mode == "queue-missing" {
                    active["queuedMessages"]["steering"] = json!([]);
                }
                respond(&mut w, &q, active).await;
                if mode == "queue-missing" {
                    eof(&mut r).await;
                    return;
                }
            }
            let output = read(&mut r).await;
            assert_eq!(
                output,
                json!({"type":"host_tool_result","id":"1","result":{"content":[{"type":"text","text":"unchanged-output"}]},"isError":true})
            );
            let sha = format!("{:x}", Sha256::digest(serde_json::to_vec(&output).unwrap()));
            assert!(peer_tools
                .lock()
                .unwrap()
                .iter()
                .any(|e| matches!(e,HostEvent::Result{result_sha256,..} if *result_sha256==sha)));
            if mode == "failed-release" {
                eof(&mut r).await;
                return;
            }
            if !matches!(mode, "missing-message" | "no-advice" | "revoked") {
                send(&mut w, user_event("message_start", "m1")).await;
                send(
                    &mut w,
                    user_event(
                        "message_end",
                        if mode == "message-id" { "m2" } else { "m1" },
                    ),
                )
                .await;
            }
            if mode == "message-id" {
                eof(&mut r).await;
                return;
            }
            send(&mut w, result("completed")).await;
            if mode == "missing-message" {
                eof(&mut r).await;
                return;
            }
            let q = read(&mut r).await;
            assert_eq!(q["type"], "get_state");
            respond(&mut w, &q, idle).await;
            eof(&mut r).await;
        });
        client.connect(Duration::from_secs(2)).await.unwrap();
        client
            .register_host_tools(&intent().validate_metadata().unwrap().0, &[tool()])
            .await
            .unwrap();
        let mut service = Service {
            boundary: None,
            sender: None,
            events: events.clone(),
            tools,
            mode,
        };
        let mut terminal = false;
        let result = client
            .execute_with_tools(
                intent(),
                PROMPT,
                if mode == "timeout-wait" {
                    Duration::from_millis(100)
                } else {
                    Duration::from_secs(3)
                },
                &mut rx,
                |event| {
                    terminal |= matches!(event, WorkerEvent::Terminal { .. });
                    Ok(())
                },
                &mut service,
            )
            .await;
        let finished = matches!(
            mode,
            "success" | "cancel-wait" | "cancel-queue" | "no-advice" | "revoked"
        );
        assert_eq!(result.is_ok(), finished, "{mode}: {result:?}");
        assert_eq!(terminal, finished, "{mode}");
        assert!(
            client
                .command_attempts()
                .get("steer")
                .copied()
                .unwrap_or_default()
                <= 1
        );
        if mode == "success" {
            assert_eq!(events.lock().unwrap().len(), 5);
        }
        if matches!(mode, "no-advice" | "revoked" | "failed-revocation") {
            assert!(!client.command_attempts().contains_key("steer"));
            let recorded = events.lock().unwrap();
            if mode == "revoked" {
                assert!(matches!(
                    recorded.as_slice(),
                    [
                        AdviceEvent::RevokedBeforeSend,
                        AdviceEvent::ToolResultReleased { .. }
                    ]
                ));
            } else {
                assert!(recorded.is_empty());
            }
        }
        if mode.starts_with("cancel") {
            assert!(!events
                .lock()
                .unwrap()
                .iter()
                .any(|e| matches!(e, AdviceEvent::MessageObserved { .. })));
        }
        assert!(client
            .execute_with_tools(
                intent(),
                PROMPT,
                Duration::from_secs(1),
                &mut rx,
                |_| Ok(()),
                &mut service
            )
            .await
            .is_err());
        client.close_input().await.unwrap();
        peer.await.unwrap();
    }
}
