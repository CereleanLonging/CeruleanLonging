//! Controlled JSONL peer, real file tools, coordinator and management application.
//! No process/provider call: these are product-boundary tests, not live OMP proof.
use super::*;
use crate::workflow_coordinator::{Scope, WorkflowClient};
use cerulean_adapters::{
    omp_protocol::PromptStatus,
    omp_transport::{
        worker::{PromptOutcome, WorkerTransport},
        FRAME_LIMIT, REASSEMBLED_LIMIT,
    },
};
use serde_json::Value;
use std::sync::atomic::Ordering;
use tokio::io::{
    duplex, split, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf,
    WriteHalf,
};
use tokio::sync::oneshot;

static HOLD_RESPONSE: AtomicBool = AtomicBool::new(false);
static MANAGER_ENTERED: AtomicBool = AtomicBool::new(false);
struct ReleaseManager;
impl Drop for ReleaseManager {
    fn drop(&mut self) {
        HOLD_RESPONSE.store(false, Ordering::Release);
    }
}
fn controlled_manager(intent: DecisionIntent) -> Result<DecisionObservation, String> {
    let result = manager(intent);
    MANAGER_ENTERED.store(true, Ordering::Release);
    let until = Instant::now() + Duration::from_secs(15);
    while HOLD_RESPONSE.load(Ordering::Acquire) {
        if Instant::now() >= until {
            return Err("Controlled manager release timed out".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    result
}

struct Fresh {
    host: RuntimeHost,
    worker: WorkflowClient,
    intent: DispatchIntent,
    updates: mpsc::Sender<crate::async_omp::Update>,
    cancel: oneshot::Receiver<()>,
    policy: SupervisionPolicy,
}
fn fresh(root: &Path) -> Fresh {
    let (mut host, mut draft) = crate::host::project_planning::tests::fixture(root);
    let policy = SupervisionPolicy {
        schema_version: 1,
        repeated_failure_threshold: 2,
        max_advice_per_attempt: 1,
        delivery_enabled: true,
    };
    draft.worker_adapter = project_setup::ProjectWorkerAdapter::Omp;
    draft.management_max_requests = 4;
    draft.supervision = Some(policy.clone());
    crate::host::project_planning::tests::isolate_draft(root, &mut draft);
    host.configure_project(draft).unwrap();
    let mut proposed = crate::host::project_planning::tests::proposal("tests");
    if let ManagementDecision::Plan { tasks, .. } = &mut proposed {
        tasks[0].worker_id = "worker-omp".into();
    }
    crate::host::project_planning::tests::complete(&mut host, proposed);
    let plan = host.projects["project"].revision().unwrap().clone();
    let revision = plan.approval_digest().unwrap();
    host.approve("project", &revision).unwrap();
    host.start_project_run(
        "project",
        &revision,
        &host.project_setups["project"].digest.clone(),
    )
    .unwrap();
    let startup = crate::host::omp_host::tests::startup(&host, "attempt");
    host.omp_startups.begin(startup).unwrap();
    let mut store = host.projects.remove("project").unwrap();
    let attempt = Attempt {
        workspace_proof: None,
        id: "attempt".into(),
        task_id: "task".into(),
        worker_id: "worker-omp".into(),
        fence: 1,
        revision_digest: revision.clone(),
        binding: SessionBinding {
            adapter_id: "omp".into(),
            adapter_version: "18.4.4".into(),
            session_id: "session".into(),
            workspace: plan.execution_root("task", "attempt").unwrap(),
            identity_strength: IdentityStrength::ControllerRequest(ControllerRequestIdentity {
                connection_id: "connection".into(),
                request_id: "prompt-request".into(),
            }),
        },
    };
    store
        .command(
            "omp-attempt-assign",
            "deputy",
            WorkflowCommand::Assign {
                task: "task".into(),
                worker: "worker-omp".into(),
                attempt: "attempt".into(),
            },
            CommandContext {
                execution: Some(ExecutionEvidence::Attempt { attempt }),
                ..Default::default()
            },
            false,
        )
        .unwrap();
    let (_, _, intent, _) = crate::omp_execution::scope(&store, "task", "attempt").unwrap();
    let snapshot = crate::async_verification::snapshot(&store);
    let writer =
        crate::async_api::ApiWorkflow::start(store, host.root.join("workflows/project")).unwrap();
    let worker = writer
        .owner
        .omp_client(Scope {
            project: "project".into(),
            task: "task".into(),
            attempt: "attempt".into(),
            revision: revision.clone(),
        })
        .unwrap();
    host.api_writers.insert("project".into(), writer);
    let (updates, receiver) = mpsc::channel();
    let (cancel, cancel_rx) = oneshot::channel();
    host.omp_active.insert(
        ("project".into(), "attempt".into()),
        ActiveOmp {
            snapshot,
            job: OmpJob {
                management_operation: None,
                project_id: "project".into(),
                task_id: "task".into(),
                attempt_id: "attempt".into(),
                revision_digest: revision,
                worker: plan.execution[0].worker.clone().unwrap(),
                state: OmpJobState::Executing,
                started_at: chrono::Utc::now().to_rfc3339(),
                message: None,
                closure_digest: None,
            },
            receiver,
            cancel: Some(cancel),
            thread: None,
            disconnected: false,
            preparation_cancel: Arc::new(AtomicBool::new(false)),
            tool_checkpoint: None,
            advice_waiter: None,
            advice_gate: None,
        },
    );
    host.openai_connection.key_configured = true;
    host.management_fixture = Some(controlled_manager);
    Fresh {
        host,
        worker,
        intent,
        updates,
        cancel: cancel_rx,
        policy,
    }
}

type Reader = BufReader<ReadHalf<DuplexStream>>;
type Writer = WriteHalf<DuplexStream>;
async fn read(reader: &mut Reader) -> Value {
    let mut line = String::new();
    assert!(reader.read_line(&mut line).await.unwrap() > 0);
    serde_json::from_str(&line).unwrap()
}
async fn send(writer: &mut Writer, value: Value) {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    writer.write_all(&bytes).await.unwrap();
    writer.flush().await.unwrap();
}
async fn respond(writer: &mut Writer, request: &Value, data: Value) {
    send(
        writer,
        json!({"type":"response","id":request["id"],"command":request["type"],"success":true,"data":data}),
    )
    .await;
}
fn idle(intent: &DispatchIntent, tools: &Value) -> Value {
    json!({"sessionId":intent.binding.session_id,"model":{"provider":intent.provider,"id":intent.model},"isStreaming":false,"isCompacting":false,"hasPendingAsyncWork":false,"isSettled":true,"queuedMessageCount":0,"dumpTools":tools})
}
async fn peer(
    channel: DuplexStream,
    intent: DispatchIntent,
    mode: &str,
    released: mpsc::Sender<()>,
    proceed: oneshot::Receiver<()>,
) -> Vec<Value> {
    let mut proceed = Some(proceed);
    let (reader, mut writer) = split(channel);
    let mut reader = BufReader::new(reader);
    send(
        &mut writer,
        json!({"type":"ready","protocolVersion":1,"supportedProtocolVersions":[1,2],"maxFrameBytes":FRAME_LIMIT,"maxReassembledFrameBytes":REASSEMBLED_LIMIT}),
    )
    .await;
    let negotiation = read(&mut reader).await;
    assert_eq!(negotiation["type"], "negotiate_protocol");
    respond(&mut writer, &negotiation, json!({"protocolVersion":2})).await;
    let mut tools = json!([]);
    loop {
        let command = read(&mut reader).await;
        match command["type"].as_str().unwrap() {
            "get_state" => respond(&mut writer, &command, idle(&intent, &tools)).await,
            "set_host_tools" => {
                assert_eq!(tools, json!([]));
                tools = command["tools"].clone();
                let names: Vec<_> = tools
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|t| &t["name"])
                    .collect();
                respond(&mut writer, &command, json!({"toolNames":names})).await;
            }
            "prompt" => {
                assert_eq!(command["id"], "prompt-request");
                assert_eq!(
                    crate::omp_advice_journal::text_hash(command["message"].as_str().unwrap()),
                    intent.prompt_sha256
                );
                respond(&mut writer, &command, Value::Null).await;
                break;
            }
            other => panic!("unexpected setup command {other}"),
        }
    }
    let mut results = Vec::new();
    for id in ["a", "b"] {
        send(
            &mut writer,
            json!({"type":"host_tool_call","id":id,"toolCallId":format!("model-{id}"),"toolName":"cerulean_read_file","arguments":{"path":"missing.txt","offset":0}}),
        )
        .await;
        let mut output = read(&mut reader).await;
        if id == "b" && mode == "success" {
            assert_eq!(output["type"], "steer");
            let message = output["message"].as_str().unwrap().to_owned();
            assert!(message.starts_with("[cerulean-advice:advice-"));
            respond(&mut writer, &output, Value::Null).await;
            let query = read(&mut reader).await;
            assert_eq!(query["type"], "get_state");
            let mut queued = idle(&intent, &tools);
            queued["isStreaming"] = json!(true);
            queued["isSettled"] = json!(false);
            queued["queuedMessageCount"] = json!(1);
            queued["queuedMessages"] = json!({"steering":[message],"followUp":[]});
            respond(&mut writer, &query, queued).await;
            output = read(&mut reader).await;
            assert_eq!(output["type"], "host_tool_result");
            released.send(()).unwrap();
            // Let the real Host inspect ACK/queue/local-flush evidence before
            // the native user-message event can exist.
            proceed.take().unwrap().await.unwrap();
            for kind in ["message_start", "message_end"] {
                send(&mut writer, json!({"type":kind,"messageId":"native-advice-message","message":{"role":"user","content":[{"type":"text","text":message}]}})).await;
            }
        }
        assert_eq!(output["type"], "host_tool_result");
        assert_eq!(output["id"], id);
        assert_eq!(output["isError"], true);
        results.push(output);
    }
    send(&mut writer, json!({"type":"host_tool_call","id":"c","toolCallId":"model-c","toolName":"cerulean_write_file","arguments":{"path":"sum.mjs","content":"export const sum = (a, b) => a + b;\n","expectedSha256":"absent"}})).await;
    let output = read(&mut reader).await;
    assert_eq!(output["type"], "host_tool_result");
    assert_eq!(output["id"], "c");
    assert_eq!(output["isError"], false);
    results.push(output);
    send(&mut writer, json!({"type":"prompt_result","id":"prompt-request","agentInvoked":true,"sessionSettled":true,"status":"completed"})).await;
    let query = read(&mut reader).await;
    assert_eq!(query["type"], "get_state");
    respond(&mut writer, &query, idle(&intent, &tools)).await;
    let mut tail = Vec::new();
    reader.read_to_end(&mut tail).await.unwrap();
    assert!(
        tail.is_empty(),
        "no resend, replacement prompt or late steer"
    );
    results
}

#[test]
fn omp_original_files_management_and_native_transport_cross_the_same_boundary() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    for mode in [
        "success",
        "pause-before-response",
        "pause-before-reservation",
    ] {
        let _release = ReleaseManager;
        MANAGER_ENTERED.store(false, Ordering::Release);
        HOLD_RESPONSE.store(mode == "pause-before-response", Ordering::Release);
        let temp = tempfile::tempdir().unwrap();
        let Fresh {
            mut host,
            mut worker,
            intent,
            updates,
            mut cancel,
            policy,
        } = fresh(temp.path());
        let (client, remote) = duplex(32768);
        let (read_half, write_half) = split(client);
        let mut transport = WorkerTransport::new(BufReader::new(read_half), write_half);
        let finished = Arc::new(AtomicBool::new(false));
        let worker_finished = finished.clone();
        let tools_updates = updates.clone();
        let progress_updates = updates.clone();
        let mut seen = std::collections::BTreeSet::new();
        let observer: crate::omp_tool_checkpoint::Observer = Box::new(move |checkpoint| {
            let wait = checkpoint
                .tools
                .supervision_incidents(&policy)?
                .iter()
                .any(|i| seen.insert(i.id.clone()));
            let (reply, boundary) = if wait {
                let (tx, rx) = oneshot::channel();
                (Some(tx), Some(rx))
            } else {
                (None, None)
            };
            tools_updates
                .send(crate::async_omp::Update::ToolCheckpoint {
                    checkpoint: Box::new(checkpoint),
                    reply,
                })
                .map_err(|_| "controlled owner closed".to_owned())?;
            Ok(boundary)
        });
        let (released, released_rx) = mpsc::channel();
        let (proceed, continue_peer) = oneshot::channel();
        let mut proceed = Some(proceed);
        let mut paused = false;
        let (outcome, results, ()) = runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(30), async {
                tokio::join!(
                    async {
                        transport.connect(Duration::from_secs(5)).await.unwrap();
                        let result = crate::omp_execution::dispatch_files_progress(
                            &mut worker,
                            &mut transport,
                            "task",
                            "attempt",
                            &mut cancel,
                            observer,
                            move |store| {
                                progress_updates
                                    .send(crate::async_omp::Update::Progress(Box::new(
                                        crate::async_verification::snapshot(store),
                                    )))
                                    .unwrap();
                            },
                        )
                        .await;
                        transport.close_input().await.unwrap();
                        worker_finished.store(true, Ordering::Release);
                        result
                    },
                    peer(remote, intent.clone(), mode, released, continue_peer),
                    async {
                        while !finished.load(Ordering::Acquire) {
                            host.poll_omp();
                            if mode == "pause-before-response"
                                && !paused
                                && MANAGER_ENTERED.load(Ordering::Acquire)
                            {
                                assert!(host
                                    .omp_active
                                    .values()
                                    .any(|a| a.advice_waiter.is_some()));
                                assert!(host
                                    .management
                                    .statuses()
                                    .iter()
                                    .filter(|s| s.purpose == Purpose::Advise)
                                    .all(|s| host
                                        .management
                                        .application(&s.operation_id)
                                        .is_none()));
                                host.pause_project_run(&host.runs.all()[0].id.clone())
                                    .unwrap();
                                paused = true;
                                HOLD_RESPONSE.store(false, Ordering::Release);
                            }
                            host.poll_management();
                            host.poll_project_runs();
                            if mode == "pause-before-reservation" && !paused {
                                if let Some(gate) = host
                                    .omp_active
                                    .values()
                                    .find_map(|a| a.advice_gate.as_ref())
                                {
                                    assert_eq!(gate.load(Ordering::SeqCst), 0);
                                    // Same executor poll: the worker cannot reserve
                                    // or send between application and this pause.
                                    host.pause_project_run(&host.runs.all()[0].id.clone())
                                        .unwrap();
                                    paused = true;
                                }
                            }
                            if released_rx.try_recv().is_ok() {
                                let operation = host
                                    .management
                                    .statuses()
                                    .into_iter()
                                    .find(|s| s.purpose == Purpose::Advise)
                                    .unwrap()
                                    .operation_id;
                                let view =
                                    host.supervision_advice_view(&operation).unwrap().unwrap();
                                assert!(!view.delivered);
                                assert_eq!(
                                    view.delivery_state.as_deref(),
                                    Some("toolResultReleased")
                                );
                                proceed.take().unwrap().send(()).unwrap();
                            }
                            tokio::time::sleep(Duration::from_millis(1)).await;
                        }
                    },
                )
            })
            .await
            .expect("controlled boundary stalled")
        });
        let PromptOutcome::Finished(receipt) = outcome.unwrap() else {
            panic!("{mode}: approved original task did not finish")
        };
        assert_eq!(receipt.status, PromptStatus::Completed);
        assert_eq!(receipt.request_id, "prompt-request");
        assert_eq!(transport.command_attempts().get("prompt"), Some(&1));
        assert_eq!(
            transport
                .command_attempts()
                .get("steer")
                .copied()
                .unwrap_or(0),
            u64::from(mode == "success")
        );
        assert!(!transport.command_attempts().contains_key("abort"));
        assert_eq!(paused, mode != "success");
        host.poll_omp();
        host.api_writers
            .get_mut("project")
            .unwrap()
            .refresh()
            .unwrap();
        assert_eq!(
            host.api_writers["project"].snapshot.state.tasks["task"].state,
            TaskState::Submitted
        );
        let deadline = Instant::now() + Duration::from_secs(5);
        while !host.management_active.is_empty() {
            assert!(Instant::now() < deadline);
            host.poll_management();
            std::thread::sleep(Duration::from_millis(1));
        }
        let operations: Vec<_> = host
            .management
            .statuses()
            .into_iter()
            .filter(|s| s.purpose == Purpose::Advise)
            .collect();
        assert_eq!(operations.len(), 1);
        let operation = &operations[0].operation_id;
        let view = host.supervision_advice_view(operation).unwrap().unwrap();
        assert_eq!(view.delivered, mode == "success");
        if mode == "pause-before-response" {
            assert!(host.management.application(operation).is_none());
        } else {
            assert!(host.management.application(operation).unwrap().1);
            assert_eq!(
                view.delivery_state.as_deref(),
                Some(if mode == "success" {
                    "messageObserved"
                } else {
                    "notSent"
                })
            );
            assert!(host.require_resolved_omp_advice("project").is_ok());
        }
        let journal = crate::omp_tool_journal::OmpToolJournal::open(
            &worker
                .omp_file_service_path("attempt")
                .unwrap()
                .join("events"),
            &intent,
        )
        .unwrap();
        assert_eq!(journal.observation().calls.len(), 3);
        for result in results {
            let original = &journal.observation().calls[result["id"].as_str().unwrap()];
            assert_eq!(
                original.result_sha256.as_deref(),
                Some(digest(&result).unwrap().as_str()),
                "original wire result must retain its durable hash"
            );
        }
        drop(journal);
        assert_eq!(
            fs::read_to_string(
                crate::host::project_planning::tests::worker_root(temp.path()).join("sum.mjs")
            )
            .unwrap(),
            "export const sum = (a, b) => a + b;\n"
        );
        drop(worker);
        drop(updates);
        drop(host);
        // Every completion is inspected twice without reconstructing a worker
        // or management sender, including both forms of scheduling pause.
        for _ in 0..2 {
            let reopened = RuntimeHost::open(&temp.path().join("data")).unwrap();
            assert!(reopened.omp_active.is_empty() && reopened.management_active.is_empty());
            assert_eq!(
                reopened
                    .supervision_advice_view(operation)
                    .unwrap()
                    .unwrap()
                    .delivered,
                mode == "success"
            );
        }
    }
}
