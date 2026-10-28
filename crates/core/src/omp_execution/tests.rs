use super::*;
use crate::scheduler::SchedulerConfig;
use cerulean_adapters::omp_protocol::PromptReceipt;
use cerulean_domain::workflow::{Principal, WorkflowPlan, WorkflowTask};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tokio::io::{
    AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader, DuplexStream, ReadHalf, WriteHalf,
};

pub(crate) fn assigned(root: &Path, money: bool) -> (DurableWorkflow, PlanRevision) {
    let project = root.join("project");
    fs::create_dir(&project).unwrap();
    let tool = crate::verifier::PinnedCheck::describe(
        &std::env::current_exe().unwrap(),
        &[],
        Default::default(),
    )
    .unwrap();
    let plan = PlanRevision {
        management_setup_digest: None,
        worktree: None,
        schema_version: 1,
        snapshot_policy: Some(SnapshotPolicy {
            exclude: vec![],
            max_files: 100,
            max_bytes: 1024 * 1024,
        }),
        project: Project {
            id: "project".into(),
            root: project.to_string_lossy().into(),
            timezone: "Asia/Shanghai".into(),
        },
        workflow: WorkflowPlan {
            id: "plan".into(),
            version: 1,
            tasks: vec![WorkflowTask {
                id: "task".into(),
                workspace_key: "project".into(),
                goal: "Produce a fixture output".into(),
                dependencies: vec![],
                resource_keys: None,
            }],
        },
        policy: ExecutionPolicy {
            version: 1,
            tools: vec![
                ToolPermission::ReadFiles,
                ToolPermission::WriteFiles,
                ToolPermission::RunCommands,
            ],
            max_requests: 3,
            max_seconds: 5,
            currency_budget: None,
            budget_usd_cents: money.then_some(25),
            supervision: None,
        },
        execution: vec![TaskExecution {
            supervision_override: None,
            profile_id: None,
            desktop_worker: None,
            api_worker: None,
            task_id: "task".into(),
            adapter_id: "omp".into(),
            artifact_paths: vec!["result.txt".into()],
            worker: Some(WorkerSelection {
                omp_runtime_parent: None,
                provider: "deepseek".into(),
                model: "fixture".into(),
                executable: tool.executable.clone(),
            }),
            checks: vec![AcceptanceCheck {
                id: "check".into(),
                name: None,
                program: "fixture-check".into(),
                args: vec![
                    "--ignored".into(),
                    "--exact".into(),
                    "omp_execution::tests::artifact_validator".into(),
                ],
                timeout_seconds: 2,
                tool: Some(tool),
            }],
        }],
    };
    let config = SchedulerConfig {
        schema_version: 1,
        core_id: "core".into(),
        execution_scope: Some(ExecutionScope {
            project: plan.project.clone(),
            policy_version: 1,
        }),
        principals: vec![
            Principal {
                id: "user".into(),
                role: Role::User,
                provider: None,
            },
            Principal {
                id: "chief".into(),
                role: Role::Chief,
                provider: Some("openai".into()),
            },
            Principal {
                id: "deputy".into(),
                role: Role::Deputy,
                provider: Some("openai".into()),
            },
            Principal {
                id: "core".into(),
                role: Role::Core,
                provider: None,
            },
            Principal {
                id: "worker".into(),
                role: Role::Worker,
                provider: None,
            },
        ],
    };
    let mut store = DurableWorkflow::open(&root.join("ledger"), config).unwrap();
    record(
        &mut store,
        "chief",
        WorkflowCommand::Propose {
            plan: plan.workflow.clone(),
        },
        CommandContext {
            execution: Some(ExecutionEvidence::Revision {
                revision: plan.clone(),
            }),
            ..Default::default()
        },
    )
    .unwrap();
    let workflow_digest = store.state().digest.clone().unwrap();
    record(
        &mut store,
        "user",
        WorkflowCommand::Approve {
            digest: workflow_digest,
        },
        CommandContext {
            approval_source: Some("offline-test".into()),
            execution: Some(ExecutionEvidence::Approval {
                approval: Approval {
                    plan_id: "plan".into(),
                    revision: 1,
                    policy_version: 1,
                    revision_digest: plan.approval_digest().unwrap(),
                    user_id: "user".into(),
                    approved_at: chrono::Utc::now().to_rfc3339(),
                },
            }),
            ..Default::default()
        },
    )
    .unwrap();
    let attempt = Attempt {
        workspace_proof: None,
        id: "attempt".into(),
        task_id: "task".into(),
        worker_id: "worker".into(),
        fence: 1,
        revision_digest: plan.approval_digest().unwrap(),
        binding: SessionBinding {
            adapter_id: "omp".into(),
            adapter_version: "18.4.4".into(),
            session_id: "session".into(),
            workspace: plan.project.root.clone(),
            identity_strength: IdentityStrength::ControllerRequest(ControllerRequestIdentity {
                connection_id: "connection".into(),
                request_id: "prompt-request".into(),
            }),
        },
    };
    record(
        &mut store,
        "deputy",
        WorkflowCommand::Assign {
            task: "task".into(),
            worker: "worker".into(),
            attempt: "attempt".into(),
        },
        CommandContext {
            execution: Some(ExecutionEvidence::Attempt { attempt }),
            ..Default::default()
        },
    )
    .unwrap();
    (store, plan)
}
fn reopen(store: DurableWorkflow, root: &Path) -> DurableWorkflow {
    let config = store.configuration().clone();
    drop(store);
    DurableWorkflow::open(&root.join("ledger"), config).unwrap()
}
fn idle() -> Value {
    json!({"sessionId":"session","model":{"provider":"deepseek","id":"fixture"},"isStreaming":false,"isCompacting":false,"hasPendingAsyncWork":false,"isSettled":true,"queuedMessageCount":0,"dumpTools":[]})
}
async fn send(writer: &mut WriteHalf<DuplexStream>, frame: Value) {
    let mut bytes = serde_json::to_vec(&frame).unwrap();
    bytes.push(b'\n');
    writer.write_all(&bytes).await.unwrap();
}
async fn read(reader: &mut BufReader<ReadHalf<DuplexStream>>) -> Value {
    let mut line = String::new();
    assert!(reader.read_line(&mut line).await.unwrap() > 0);
    serde_json::from_str(&line).unwrap()
}
async fn reply(writer: &mut WriteHalf<DuplexStream>, request: &Value, data: Value) {
    send(writer,json!({"type":"response","id":request["id"],"command":request["type"],"success":true,"data":data})).await;
}
async fn peer(
    stream: DuplexStream,
    root: PathBuf,
    mode: &'static str,
    cancel: oneshot::Sender<()>,
) {
    peer_gated(stream, root, mode, cancel, None).await;
}
async fn peer_gated(
    stream: DuplexStream,
    root: PathBuf,
    mode: &'static str,
    cancel: oneshot::Sender<()>,
    gate: Option<(oneshot::Sender<()>, oneshot::Receiver<()>)>,
) {
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);
    send(&mut writer,json!({"type":"ready","protocolVersion":1,"supportedProtocolVersions":[1,2],"maxFrameBytes":1024*1024,"maxReassembledFrameBytes":64*1024*1024})).await;
    let request = read(&mut reader).await;
    reply(&mut writer, &request, json!({"protocolVersion":2})).await;
    for _ in 0..2 {
        let request = read(&mut reader).await;
        assert_eq!(request["type"], "get_state");
        reply(&mut writer, &request, idle()).await;
    }
    let prompt = read(&mut reader).await;
    assert_eq!(prompt["type"], "prompt");
    // Both durable records must exist before the peer sees any prompt.
    let journal = root
        .join("ledger/omp")
        .join(format!("attempt-{}", digest(&"attempt").unwrap()));
    let event: Value = serde_json::from_slice(&fs::read(journal.join("00.json")).unwrap()).unwrap();
    assert_eq!(event["body"]["event"]["kind"], "dispatch-intent");
    let workflow_intents = fs::read_dir(root.join("ledger/events"))
        .unwrap()
        .filter_map(|e| {
            let e = e.unwrap();
            if e.path().extension()?.to_str()? != "json" {
                return None;
            }
            let entry: crate::scheduler::Entry =
                serde_json::from_slice(&fs::read(e.path()).unwrap()).unwrap();
            matches!(
                entry.body.event.command,
                WorkflowCommand::DispatchIntent { .. }
            )
            .then_some(entry)
        })
        .count();
    assert_eq!(workflow_intents, 1);
    if let Some((entered, resume)) = gate {
        entered.send(()).unwrap();
        resume.await.unwrap();
    }
    if mode == "lost-ack" {
        return;
    }
    if mode == "early-cancel" {
        cancel.send(()).unwrap();
        let abort = read(&mut reader).await;
        assert_eq!(abort["type"], "abort");
        reply(&mut writer, &abort, Value::Null).await;
    }
    reply(&mut writer, &prompt, Value::Null).await;
    if mode == "completed" {
        fs::write(root.join("project/result.txt"), "fixture output").unwrap();
    }
    let status = match mode {
        "early-cancel" => "aborted",
        "provider-error" => "error",
        _ => "completed",
    };
    send(&mut writer,json!({"type":"prompt_result","id":prompt["id"],"agentInvoked":true,"sessionSettled":true,"status":status})).await;
    let state = read(&mut reader).await;
    assert_eq!(state["type"], "get_state");
    reply(&mut writer, &state, idle()).await;
    let mut tail = Vec::new();
    reader.read_to_end(&mut tail).await.unwrap();
    assert!(tail.is_empty());
}

#[tokio::test]
async fn workflow_wire_submission_cancellation_and_failure_replay() {
    for mode in [
        "completed",
        "early-cancel",
        "lost-ack",
        "provider-error",
        "missing-artifact",
    ] {
        let root = tempfile::tempdir().unwrap();
        let (mut store, plan) = assigned(root.path(), false);
        let (client, server) = tokio::io::duplex(8192);
        let (r, w) = tokio::io::split(client);
        let mut client = WorkerTransport::new(BufReader::new(r), w);
        let (cancel, mut rx) = oneshot::channel();
        let job = tokio::spawn(peer(server, root.path().into(), mode, cancel));
        client.connect(Duration::from_secs(3)).await.unwrap();
        let result = dispatch(&mut store, &mut client, "task", "attempt", &mut rx).await;
        let expected = match mode {
            "completed" => TaskState::Submitted,
            "early-cancel" => TaskState::Cancelled,
            _ => TaskState::Unknown,
        };
        assert_eq!(
            result.is_ok(),
            matches!(mode, "completed" | "early-cancel"),
            "{mode}: {result:?}"
        );
        assert_eq!(store.state().tasks["task"].state, expected, "{mode}");
        assert_eq!(client.command_attempts().get("prompt"), Some(&1));
        client.close_input().await.unwrap();
        job.await.unwrap();
        if mode == "early-cancel" {
            assert_eq!(client.command_attempts().get("abort"), Some(&1));
            assert!(!store
                .history()
                .iter()
                .any(|e| matches!(e.body.event.command, WorkflowCommand::Started { .. })));
        }
        if mode == "completed" {
            assert!(record(
                &mut store,
                "chief",
                WorkflowCommand::Accept {
                    task: "task".into(),
                    attempt: "attempt".into()
                },
                CommandContext::default()
            )
            .is_err());
        }
        let mut store = reopen(store, root.path());
        assert_eq!(store.state().tasks["task"].state, expected);
        let before = store.history().len();
        if expected == TaskState::Unknown {
            assert!(recover_terminal(&mut store, "task", "attempt").is_err());
            assert_eq!(store.history().len(), before);
        }
        let (next, mut untouched) = tokio::io::duplex(4096);
        let (r, w) = tokio::io::split(next);
        let mut next = WorkerTransport::new(BufReader::new(r), w);
        assert!(dispatch(&mut store, &mut next, "task", "attempt", &mut rx)
            .await
            .is_err());
        assert!(next.command_attempts().is_empty());
        next.close_input().await.unwrap();
        let mut wire = Vec::new();
        untouched.read_to_end(&mut wire).await.unwrap();
        assert!(wire.is_empty());
        if mode == "completed" {
            let pin =
                crate::verifier::PinnedCheck::from_approved(&plan.execution[0].checks[0]).unwrap();
            store
                .run_verification(
                    "omp-independent-check",
                    "task",
                    "attempt",
                    &[pin],
                    &std::sync::atomic::AtomicBool::new(false),
                )
                .unwrap();
            assert!(store.state().tasks["task"].verified);
            fs::write(
                root.path().join("project/result.txt"),
                "modified after verification",
            )
            .unwrap();
            assert!(record(
                &mut store,
                "chief",
                WorkflowCommand::Accept {
                    task: "task".into(),
                    attempt: "attempt".into()
                },
                CommandContext::default()
            )
            .is_err());
            fs::write(root.path().join("project/result.txt"), "fixture output").unwrap();
            record(
                &mut store,
                "chief",
                WorkflowCommand::Accept {
                    task: "task".into(),
                    attempt: "attempt".into(),
                },
                CommandContext::default(),
            )
            .unwrap();
            let store = reopen(store, root.path());
            assert_eq!(store.state().tasks["task"].state, TaskState::Accepted);
        }
    }
}

#[test]
#[ignore = "isolated independent artifact validator, invoked only by the approved check"]
fn artifact_validator() {
    assert_eq!(fs::read_to_string("result.txt").unwrap(), "fixture output");
}

#[tokio::test]
async fn unsupported_money_budget_rejects_before_any_wire_or_dispatch() {
    let root = tempfile::tempdir().unwrap();
    let (mut store, _) = assigned(root.path(), true);
    let (client, mut peer) = tokio::io::duplex(4096);
    let (r, w) = tokio::io::split(client);
    let mut client = WorkerTransport::new(BufReader::new(r), w);
    let (_cancel, mut rx) = oneshot::channel();
    let before = store.history().len();
    assert!(
        dispatch(&mut store, &mut client, "task", "attempt", &mut rx)
            .await
            .unwrap_err()
            .contains("money enforcement")
    );
    assert_eq!(store.history().len(), before);
    assert!(client.command_attempts().is_empty());
    client.close_input().await.unwrap();
    let mut wire = Vec::new();
    peer.read_to_end(&mut wire).await.unwrap();
    assert!(wire.is_empty());
}

#[test]
fn durable_completion_recovers_original_artifacts_only() {
    for mode in [
        "unchanged",
        "changed",
        "missing-seal",
        "pending-seal",
        "foreign-seal",
    ] {
        let root = tempfile::tempdir().unwrap();
        let (mut store, plan) = assigned(root.path(), false);
        let (_, attempt, intent, _) = scope(&store, "task", "attempt").unwrap();
        record(
            &mut store,
            "core",
            WorkflowCommand::DispatchIntent {
                task: "task".into(),
                attempt: "attempt".into(),
            },
            CommandContext {
                thread_id: Some("session".into()),
                cwd: Some(plan.project.root.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        let path = store.omp_attempt_path("attempt").unwrap();
        let mut journal = OmpJournal::open(&path, &intent).unwrap();
        journal
            .record(&WorkerEvent::DispatchIntent {
                intent: intent.clone(),
            })
            .unwrap();
        journal
            .record(&WorkerEvent::PromptAcknowledged {
                request_id: "prompt-request".into(),
            })
            .unwrap();
        record(
            &mut store,
            "core",
            WorkflowCommand::Started {
                task: "task".into(),
                attempt: "attempt".into(),
            },
            context(
                receipt(&attempt, ReceiptStatus::Running, journal.head()),
                None,
            ),
        )
        .unwrap();
        let (identity, request_id) = intent.validate_metadata().unwrap();
        journal
            .record(&WorkerEvent::Terminal {
                receipt: PromptReceipt {
                    identity,
                    request_id,
                    correlation_origin: "controller-rpc-request-id".into(),
                    status: PromptStatus::Completed,
                },
                abort_requested: false,
                abort_acknowledged: false,
            })
            .unwrap();
        fs::write(root.path().join("project/result.txt"), "original").unwrap();
        if mode != "missing-seal" {
            seal_artifacts(&store, &plan, &attempt, &mut journal).unwrap();
        }
        let original = journal.completion().cloned();
        drop(journal);
        if mode == "changed" {
            fs::write(root.path().join("project/result.txt"), "replacement").unwrap();
        }
        if mode == "pending-seal" {
            fs::copy(
                path.join("completion.json"),
                path.join("completion.pending"),
            )
            .unwrap();
        }
        if mode == "foreign-seal" {
            let mut seal: Value =
                serde_json::from_slice(&fs::read(path.join("completion.json")).unwrap()).unwrap();
            seal["body"]["evidence"]["receipt"]["attemptId"] = json!("foreign");
            fs::write(
                path.join("completion.json"),
                serde_json::to_vec(&seal).unwrap(),
            )
            .unwrap();
        }
        // Model the owner ending after durable terminal/seal and before Submit.
        let mut store = reopen(store, root.path());
        assert_eq!(store.state().tasks["task"].state, TaskState::Unknown);
        let before = store.history().len();
        let result = recover_terminal(&mut store, "task", "attempt");
        if mode == "unchanged" {
            result.unwrap();
            assert_eq!(store.state().tasks["task"].state, TaskState::Submitted);
            let entry = store.history().last().unwrap();
            assert_eq!(
                entry.body.context.execution,
                Some(ExecutionEvidence::Receipt {
                    receipt: original.as_ref().unwrap().receipt.clone(),
                    manifest: Some(original.unwrap().manifest)
                })
            );
            let count = store.history().len();
            assert!(recover_terminal(&mut store, "task", "attempt").is_err());
            assert_eq!(count, store.history().len());
        } else {
            assert!(result.is_err(), "{mode}");
            assert_eq!(store.history().len(), before);
            assert_eq!(store.state().tasks["task"].state, TaskState::Unknown);
        }
    }
}

#[tokio::test]
async fn only_explicit_durable_no_send_cancellation_can_close_unknown_attempt() {
    for mode in ["intent-only", "ack", "cancel-before-wire"] {
        let root = tempfile::tempdir().unwrap();
        let (mut store, plan) = assigned(root.path(), false);
        let (_, _, intent, _) = scope(&store, "task", "attempt").unwrap();
        record(
            &mut store,
            "core",
            WorkflowCommand::DispatchIntent {
                task: "task".into(),
                attempt: "attempt".into(),
            },
            CommandContext {
                thread_id: Some("session".into()),
                cwd: Some(plan.project.root.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        let mut journal =
            OmpJournal::open(&store.omp_attempt_path("attempt").unwrap(), &intent).unwrap();
        journal
            .record(&WorkerEvent::DispatchIntent { intent })
            .unwrap();
        if mode == "ack" {
            journal
                .record(&WorkerEvent::PromptAcknowledged {
                    request_id: "prompt-request".into(),
                })
                .unwrap();
        }
        if mode == "cancel-before-wire" {
            journal
                .record(&WorkerEvent::NotStarted {
                    request_id: "prompt-request".into(),
                })
                .unwrap();
        }
        drop(journal);
        let mut store = reopen(store, root.path());
        assert_eq!(store.state().tasks["task"].state, TaskState::Unknown);
        let before = store.history().len();
        let result = resolve_unsent_cancellation(&mut store, "task", "attempt");
        if mode == "cancel-before-wire" {
            result.unwrap();
            assert_eq!(store.state().tasks["task"].state, TaskState::Cancelled);
            assert_eq!(store.history().len(), before + 1);
        } else {
            assert!(result.is_err());
            assert_eq!(store.history().len(), before);
        }
        let count = store.history().len();
        assert!(resolve_unsent_cancellation(&mut store, "task", "attempt").is_err());
        assert_eq!(count, store.history().len());
        let (stream, mut peer) = tokio::io::duplex(4096);
        let (r, w) = tokio::io::split(stream);
        let mut channel = WorkerTransport::new(BufReader::new(r), w);
        let (_cancel, mut rx) = oneshot::channel();
        assert!(
            dispatch(&mut store, &mut channel, "task", "attempt", &mut rx)
                .await
                .is_err()
        );
        assert!(channel.command_attempts().is_empty());
        channel.close_input().await.unwrap();
        let mut wire = Vec::new();
        peer.read_to_end(&mut wire).await.unwrap();
        assert!(wire.is_empty());
        let restored = reopen(store, root.path());
        assert_eq!(
            restored.state().tasks["task"].state,
            if mode == "cancel-before-wire" {
                TaskState::Cancelled
            } else {
                TaskState::Unknown
            }
        );
    }
}

#[cfg(windows)]
mod coordinated;
#[cfg(windows)]
mod files;
