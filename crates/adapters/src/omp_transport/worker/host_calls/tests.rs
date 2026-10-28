use super::*;
use crate::omp_transport::worker::tests::{
    eof, intent, pair, read, respond, result, send, setup, state, PROMPT,
};
use std::sync::{Arc, Mutex};

fn tool() -> host_tools::HostToolDefinition {
    host_tools::HostToolDefinition {
        name: "cerulean_fixture".into(),
        description: "Test only".into(),
        parameters: json!({"type":"object","properties":{},"additionalProperties":false}),
    }
}
fn call(id: &str) -> Value {
    json!({"type":"host_tool_call","id":id,"toolCallId":format!("model-{id}"),"toolName":tool().name,"arguments":{}})
}
fn cancel(id: &str, target: &str) -> Value {
    json!({"type":"host_tool_cancel","id":id,"targetId":target})
}
#[derive(Default)]
struct Service {
    events: Arc<Mutex<Vec<HostEvent>>>,
    starts: usize,
    wait_cancel: bool,
    fail: Option<&'static str>,
}
impl HostService for Service {
    fn record(&mut self, scope: &DispatchIntent, event: &HostEvent) -> Result<(), String> {
        assert_eq!(scope, &intent());
        if matches!(
            (self.fail, event),
            (Some("dispatch"), HostEvent::Dispatch { .. })
                | (Some("result"), HostEvent::Result { .. })
                | (Some("cancel"), HostEvent::Cancel { .. })
        ) {
            return Err("journal unavailable".into());
        }
        self.events.lock().unwrap().push(event.clone());
        Ok(())
    }
    fn start(&mut self, _: HostRequest, mut cancel: watch::Receiver<bool>) -> HostFuture {
        self.starts += 1;
        let waiting = self.wait_cancel;
        let events = self.events.clone();
        assert!(matches!(
            events.lock().unwrap().last(),
            Some(HostEvent::Dispatch { .. })
        ));
        Box::pin(async move {
            if waiting {
                while !*cancel.borrow_and_update() {
                    cancel.changed().await.map_err(|_| "owner dropped")?;
                }
                assert!(events
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|e| matches!(e, HostEvent::Cancel { .. })));
                // Represent an operation that acknowledges only after cleanup.
                tokio::time::sleep(Duration::from_millis(15)).await;
            }
            Ok(HostOutput {
                text: "fixture-output".into(),
                is_error: waiting,
            })
        })
    }
}
#[tokio::test]
async fn calls_are_unique_bounded_and_journaled_before_start_and_result() {
    let mut service = Service::default();
    let events = service.events.clone();
    let mut calls = Calls::new(&mut service, intent(), &[tool()]);
    for id in ["1", "2", "3", "4"] {
        calls.accept(call(id), true).unwrap();
    }
    assert!(calls.accept(call("5"), true).is_err());
    assert!(calls.accept(call("1"), true).is_err());
    let output = calls.next().await.unwrap();
    assert_eq!(output["id"], "1");
    assert_eq!(output["result"]["content"][0]["text"], "fixture-output");
    assert!(matches!(events.lock().unwrap().last(),Some(HostEvent::Result{id,..}) if id=="1"));
    assert!(calls.accept(call("1"), true).is_err());
    calls.accept(cancel("cancel-1", "1"), true).unwrap(); // crossed result/abort
    assert!(calls.accept(cancel("cancel-1", "2"), true).is_err());
    assert!(calls.accept(cancel("foreign", "missing"), true).is_err());
    let mut foreign = call("6");
    foreign["toolName"] = json!("bash");
    assert!(calls.accept(foreign, true).is_err());
    let mut malformed = call("6");
    malformed["extra"] = json!(true);
    assert!(calls.accept(malformed, true).is_err());
    assert!(calls.accept(call("6"), false).is_err());
    calls.accept(call("5"), true).unwrap();
    while !calls.idle() {
        calls.next().await.unwrap();
    }
    drop(calls);
    assert_eq!(service.starts, 5);
}
#[tokio::test]
async fn cancellation_waits_for_quiescence_and_failed_journal_never_grants_action() {
    let mut service = Service {
        wait_cancel: true,
        ..Default::default()
    };
    let mut calls = Calls::new(&mut service, intent(), &[tool()]);
    calls.accept(call("1"), true).unwrap();
    calls.accept(cancel("2", "1"), true).unwrap();
    assert!(!calls.idle());
    calls.cancel_all().unwrap();
    assert!(calls.accept(call("3"), true).is_err());
    assert_eq!(calls.next().await.unwrap()["isError"], true);
    assert!(calls.idle());
    for phase in ["dispatch", "result", "cancel"] {
        let mut service = Service {
            fail: Some(phase),
            ..Default::default()
        };
        let mut calls = Calls::new(&mut service, intent(), &[tool()]);
        let r = calls.accept(call("1"), true);
        if phase == "dispatch" {
            assert!(r.is_err());
            drop(calls);
            assert_eq!(service.starts, 0);
        } else {
            r.unwrap();
            if phase == "result" {
                assert!(calls.next().await.is_err());
            } else {
                assert!(calls.cancel_all().is_err());
            }
        }
    }
}

#[tokio::test]
async fn sole_reader_routes_results_and_waits_for_cancelled_tool_before_terminal() {
    for mode in [
        "success",
        "peer-cancel",
        "user-cancel",
        "duplicate",
        "failed-result-journal",
    ] {
        let (mut client, peer) = pair();
        let (cancel_tx, mut rx) = oneshot::channel();
        let events = Arc::new(Mutex::new(Vec::new()));
        let peer_events = events.clone();
        let peer = tokio::spawn(async move {
            let (mut r, mut w) = setup(peer, true).await;
            let q = read(&mut r).await;
            assert_eq!(q["type"], "set_host_tools");
            respond(&mut w, &q, json!({"toolNames":[tool().name]})).await;
            let mut active = state();
            active["dumpTools"] = json!([tool()]);
            let q = read(&mut r).await;
            respond(&mut w, &q, active.clone()).await;
            let q = read(&mut r).await;
            assert_eq!(q["type"], "get_state");
            respond(&mut w, &q, active.clone()).await;
            let prompt = read(&mut r).await;
            assert_eq!(prompt["type"], "prompt");
            respond(&mut w, &prompt, Value::Null).await;
            send(
                &mut w,
                json!({"type":"tool_execution_start","toolCallId":"model-1"}),
            )
            .await;
            send(&mut w, call("1")).await;
            if mode == "duplicate" {
                send(&mut w, call("1")).await;
                eof(&mut r).await;
                return;
            }
            if mode == "failed-result-journal" {
                eof(&mut r).await;
                return;
            }
            if mode == "peer-cancel" {
                send(&mut w, cancel("2", "1")).await;
                send(&mut w, result("aborted")).await;
            }
            if mode == "user-cancel" {
                // Wait until the host has accepted the call before user cancellation.
                loop {
                    if !peer_events.lock().unwrap().is_empty() {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
                cancel_tx.send(()).unwrap();
                let abort = read(&mut r).await;
                assert_eq!(abort["type"], "abort");
                respond(&mut w, &abort, Value::Null).await;
                send(&mut w, result("aborted")).await;
            }
            let output = read(&mut r).await;
            assert_eq!(output["type"], "host_tool_result");
            assert_eq!(output["id"], "1");
            assert!(matches!(
                peer_events.lock().unwrap().last(),
                Some(HostEvent::Result { .. })
            ));
            if mode == "success" {
                send(&mut w, result("completed")).await;
            }
            let q = read(&mut r).await;
            assert_eq!(q["type"], "get_state");
            respond(&mut w, &q, active).await;
            eof(&mut r).await;
        });
        client.connect(Duration::from_secs(2)).await.unwrap();
        client
            .register_host_tools(&intent().validate_metadata().unwrap().0, &[tool()])
            .await
            .unwrap();
        let mut service = Service {
            events,
            wait_cancel: matches!(mode, "peer-cancel" | "user-cancel" | "duplicate"),
            fail: if mode == "failed-result-journal" {
                Some("result")
            } else {
                None
            },
            ..Default::default()
        };
        let mut terminal = false;
        let outcome = client
            .execute_with_tools(
                intent(),
                PROMPT,
                Duration::from_secs(3),
                &mut rx,
                |e| {
                    if matches!(e, WorkerEvent::Terminal { .. }) {
                        terminal = true;
                    }
                    Ok(())
                },
                &mut service,
            )
            .await;
        if matches!(mode, "duplicate" | "failed-result-journal") {
            assert!(outcome.is_err(), "{mode}");
            assert!(!terminal);
            assert!(!client.command_attempts().contains_key("host_tool_result"));
        } else {
            assert!(
                matches!(outcome, Ok(PromptOutcome::Finished(_))),
                "{mode}: {outcome:?}"
            );
            assert!(terminal);
            assert_eq!(client.command_attempts().get("host_tool_result"), Some(&1));
        }
        assert_eq!(service.starts, 1);
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
