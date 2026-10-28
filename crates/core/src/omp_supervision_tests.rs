//! Real approval, workflow owner and file tools; synthetic deputy, no model/process calls.
use super::*;
use crate::{
    async_omp::ActiveOmp, omp_file_tools::OmpFileTools, omp_workflow_access::OmpWorkflowAccess,
};
use cerulean_adapters::{
    omp_transport::worker::{
        host_calls::{HostEvent, HostService},
        DispatchIntent, WorkerEvent,
    },
    openai::DecisionObservation,
};
use cerulean_domain::{
    management::DecisionEnvelope,
    omp_jobs::{OmpJob, OmpJobState},
    supervision::SupervisionPolicy,
};
use serde_json::json;
use std::{
    sync::{atomic::AtomicBool, mpsc, Arc},
    time::{Duration, Instant},
};

struct Fixture {
    host: RuntimeHost,
    files: OmpFileTools,
    intent: DispatchIntent,
    checkpoints: mpsc::Receiver<crate::omp_tool_checkpoint::OmpToolCheckpoint>,
    _updates: mpsc::Sender<crate::async_omp::Update>,
    _cancel: tokio::sync::oneshot::Receiver<()>,
}
fn manager(intent: DecisionIntent) -> Result<DecisionObservation, String> {
    let b = binding(&intent)?.ok_or("OMP binding missing")?;
    let e = evidence(&intent, &b)?;
    assert_eq!(e.intent().validate_metadata()?.1, "prompt-request");
    Ok(DecisionObservation {
        response_id: "resp_omp_advice".into(),
        requested_model: intent.model.clone(),
        actual_model: intent.model,
        input_tokens: 1,
        output_tokens: 1,
        envelope: DecisionEnvelope {
            state_digest: intent.state_digest,
            decision: ManagementDecision::Advise {
                task_id: b.task_id,
                suggestion:
                    "Stop repeating the failed file operation and inspect the approved task.".into(),
                rationale: "Original file tool evidence contains two equal consecutive failures."
                    .into(),
            },
        },
    })
}
fn fixture(root: &Path, limit: u32) -> Fixture {
    fixture_delivery(root, limit, false)
}
fn fixture_delivery(root: &Path, limit: u32, delivery_enabled: bool) -> Fixture {
    fixture_source(root, limit, delivery_enabled, false)
}
fn fixture_source(root: &Path, limit: u32, delivery_enabled: bool, deepseek: bool) -> Fixture {
    if deepseek {
        drop(crate::Store::open(&root.join("data")).unwrap());
        crate::management_source_guard::tests::promote_fixture(&root.join("data"));
    }
    let (mut host, mut draft) = super::super::project_planning::tests::fixture(root);
    if deepseek {
        draft.management_sources = Some(project_setup::ManagementSources {
            chief: project_setup::ChiefSource::OpenAi {},
            deputy: project_setup::DeputySource::DeepSeek {},
        });
        draft.deputy_model = "deepseek-flash".into();
        host.set_openai_key(Some(
            secret::SecretInput::new("sk-synthetic-route-chief".into()).unwrap(),
        ))
        .unwrap();
        host.set_provider_key(
            credentials::CredentialProvider::DeepSeek,
            Some(secret::SecretInput::new("sk-synthetic-route-deputy".into()).unwrap()),
        )
        .unwrap();
    }
    draft.worker_adapter = project_setup::ProjectWorkerAdapter::Omp;
    draft.management_max_requests = limit;
    draft.supervision = Some(SupervisionPolicy {
        schema_version: 1,
        repeated_failure_threshold: 2,
        max_advice_per_attempt: 1,
        delivery_enabled,
    });
    host.configure_project(draft).unwrap();
    let mut proposed = super::super::project_planning::tests::proposal("tests");
    if let ManagementDecision::Plan { tasks, .. } = &mut proposed {
        tasks[0].worker_id = "worker-omp".into();
    }
    super::super::project_planning::tests::complete(&mut host, proposed);
    let plan = host.projects["project"].revision().unwrap().clone();
    let revision = plan.approval_digest().unwrap();
    host.approve("project", &revision).unwrap();
    host.start_project_run(
        "project",
        &revision,
        &host.project_setups["project"].digest.clone(),
    )
    .unwrap();
    let startup = super::super::omp_host::tests::startup(&host, "attempt");
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
            workspace: plan.project.root.clone(),
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
                execution: Some(ExecutionEvidence::Attempt {
                    attempt: attempt.clone(),
                }),
                ..Default::default()
            },
            false,
        )
        .unwrap();
    let (_, _, intent, _) = crate::omp_execution::scope(&store, "task", "attempt").unwrap();
    let mut journal =
        crate::omp_journal::OmpJournal::open(&store.omp_attempt_path("attempt").unwrap(), &intent)
            .unwrap();
    store
        .record_omp(
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
    store
        .record_omp(
            "core",
            WorkflowCommand::Started {
                task: "task".into(),
                attempt: "attempt".into(),
            },
            CommandContext {
                thread_id: Some("session".into()),
                execution: Some(ExecutionEvidence::Receipt {
                    receipt: ExecutionReceipt {
                        schema_version: 1,
                        task_id: "task".into(),
                        attempt_id: "attempt".into(),
                        fence: 1,
                        binding: attempt.binding,
                        turn_id: None,
                        observed_at: chrono::Utc::now().to_rfc3339(),
                        status: ReceiptStatus::Running,
                        evidence_sha256: journal.head().into(),
                    },
                    manifest: None,
                }),
                ..Default::default()
            },
        )
        .unwrap();
    drop(journal);
    let approval = store.approval().unwrap();
    let mut files = OmpFileTools::create(
        &plan,
        approval,
        &approval.user_id,
        plan.policy.version,
        &intent,
        &store.omp_file_service_path("attempt").unwrap(),
    )
    .unwrap();
    let (tx, checkpoints) = mpsc::channel();
    files.observe_completed_tools(Box::new(move |c| {
        tx.send(c)
            .map(|_| None)
            .map_err(|_| "checkpoint closed".into())
    }));
    let snapshot = crate::async_verification::snapshot(&store);
    let writer =
        crate::async_api::ApiWorkflow::start(store, host.root.join("workflows/project")).unwrap();
    host.api_writers.insert("project".into(), writer);
    let (updates, receiver) = mpsc::channel();
    let (cancel, cancel_rx) = tokio::sync::oneshot::channel();
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
    host.management_fixture = Some(manager);
    if deepseek {
        host.openai_connection.key_configured = false;
        host.management_fixture = Some(|_| panic!("DS advice must not call OpenAI"));
        host.deputy_management_fixture =
            Some(super::super::integration_tests::deepseek_routing::deputy);
    }
    Fixture {
        host,
        files,
        intent,
        checkpoints,
        _updates: updates,
        _cancel: cancel_rx,
    }
}
impl Fixture {
    fn failure(&mut self, id: &str) {
        let arguments = json!({"path":"missing.txt", "offset":0});
        let request = crate::omp_file_tools::tests::request(id, "read_file", arguments.clone());
        self.files
            .record(
                &self.intent,
                &HostEvent::Dispatch {
                    id: id.into(),
                    tool_call_id: format!("model-{id}"),
                    tool_name: "cerulean_read_file".into(),
                    arguments_sha256: digest(&arguments).unwrap(),
                },
            )
            .unwrap();
        let (_cancel, rx) = tokio::sync::watch::channel(false);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let output = runtime.block_on(self.files.start(request, rx)).unwrap();
        assert!(output.is_error);
        let result = json!({"type":"host_tool_result", "id":id,"result":{"content":[{"type":"text","text":output.text}]},"isError":true});
        self.files
            .record(
                &self.intent,
                &HostEvent::Result {
                    id: id.into(),
                    result_sha256: digest(&result).unwrap(),
                    is_error: true,
                },
            )
            .unwrap();
        self.host
            .accept_omp_tool_checkpoint("project", "attempt", self.checkpoints.recv().unwrap())
            .unwrap();
    }
    fn wait(&mut self) -> String {
        let op = self
            .host
            .management
            .statuses()
            .into_iter()
            .find(|s| s.purpose == Purpose::Advise)
            .unwrap()
            .operation_id;
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.host.management_active.contains_key(&op) {
            assert!(Instant::now() < deadline);
            self.host.poll_management();
            std::thread::sleep(Duration::from_millis(2));
        }
        op
    }
}
#[test]
fn original_file_incident_records_once_and_reopens_without_delivery() {
    for deepseek in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let mut f = fixture_source(temp.path(), 4, false, deepseek);
        f.failure("a");
        assert!(!f
            .host
            .request_omp_supervision("project", "attempt")
            .unwrap());
        f.failure("b");
        // Scheduling, not a bespoke sender, launches the existing deputy path.
        f.host.poll_project_runs();
        let op = f.wait();
        assert_eq!(f.host.management.is_deputy(&op), deepseek);
        assert!(!f
            .host
            .request_omp_supervision("project", "attempt")
            .unwrap());
        let view = f.host.supervision_advice_view(&op).unwrap().unwrap();
        assert!(view.current && !view.delivered);
        assert!(view.api.is_none() && view.omp.is_some());
        assert_eq!(view.omp.unwrap().request_id, "prompt-request");
        assert_eq!(view.delivery_state.as_deref(), Some("recordOnly"));
        assert!(f.host.management.application(&op).is_none());
        let intent = f.host.management.intent(&op).unwrap().clone();
        f.host.revalidate_counted_supervision(&intent).unwrap();
        drop(f);
        let mut host = RuntimeHost::open(&temp.path().join("data")).unwrap();
        host.validate_omp_supervision_requests().unwrap();
        let view = host.supervision_advice_view(&op).unwrap().unwrap();
        assert!(!view.current && !view.delivered && view.suggestion.is_some());
        assert!(host.management_active.is_empty());
        let count = host.management.statuses().len();
        host.poll_project_runs();
        assert_eq!(host.management.statuses().len(), count);
        assert!(host.management.application(&op).is_none());
        drop(host);
        RuntimeHost::open(&temp.path().join("data")).unwrap();
    }
}
#[test]
fn original_run_and_live_owner_required_before_request_or_counted_send() {
    for deepseek in [false, true] {
        for mode in [
            "pause",
            "cancel",
            "disconnected",
            "expiry",
            "changed-observation",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let mut f = fixture_source(temp.path(), 4, false, deepseek);
            f.failure("a");
            f.failure("b");
            assert!(f
                .host
                .request_omp_supervision("project", "attempt")
                .unwrap());
            let op = f.wait();
            assert_eq!(f.host.management.is_deputy(&op), deepseek);
            let intent = f.host.management.intent(&op).unwrap().clone();
            let key = ("project".into(), "attempt".into());
            match mode {
                "pause" => {
                    let id = f.host.runs.all()[0].id.clone();
                    f.host.pause_project_run(&id).unwrap();
                }
                "cancel" => f
                    .host
                    .omp_active
                    .get_mut(&key)
                    .unwrap()
                    .preparation_cancel
                    .store(true, std::sync::atomic::Ordering::Release),
                "disconnected" => f.host.omp_active.get_mut(&key).unwrap().disconnected = true,
                "expiry" => {
                    f.host.omp_active.get_mut(&key).unwrap().job.started_at =
                        "2000-01-01T00:00:00Z".into()
                }
                _ => f.failure("c"),
            }
            assert!(
                f.host.revalidate_counted_supervision(&intent).is_err(),
                "{mode}"
            );
            assert!(f.host.management.application(&op).is_none(), "{mode}");
        }
    }
}
#[test]
fn management_request_budget_prevents_new_advice() {
    let temp = tempfile::tempdir().unwrap();
    let mut f = fixture(temp.path(), 1); // original planning already used one
    f.failure("a");
    f.failure("b");
    assert!(f
        .host
        .request_omp_supervision("project", "attempt")
        .unwrap_err()
        .contains("budget"));
    assert_eq!(f.host.management.statuses().len(), 1);
    assert!(f.host.management_active.is_empty());
}

#[test]
fn native_advice_requires_original_application_and_message_receipt_without_replay() {
    for deepseek in [false, true] {
        use cerulean_adapters::omp_transport::worker::advice::AdviceEvent;
        for mode in [
            "confirmed",
            "lost-message",
            "revoked",
            "publication-gap",
            "published-pending",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let mut f = fixture_source(temp.path(), 4, true, deepseek);
            f.failure("a");
            f.failure("b");
            let (reply, mut rx) = tokio::sync::oneshot::channel();
            f.host
                .handle_omp_advice_boundary("project", "attempt", Some(reply))
                .unwrap();
            f.host.poll_project_runs();
            let op = f.wait();
            assert_eq!(f.host.management.is_deputy(&op), deepseek);
            f.host.poll_project_runs();
            assert!(f.host.management.application(&op).unwrap().1);
            let mut advice = rx.try_recv().unwrap().unwrap().unwrap();
            assert_eq!(advice.operation_id, op);
            let original = f.host.management.intent(&op).unwrap().clone();
            assert_eq!(
                f.host.omp_advice_delivery_state(&original).unwrap(),
                "appliedLocally"
            );
            let send = AdviceEvent::SendIntent {
                command_id: "cerulean-omp-steer-1".into(),
                message_sha256: crate::omp_advice_journal::text_hash(&advice.message),
            };
            if mode == "revoked" {
                let run = f.host.runs.all()[0].id.clone();
                f.host.pause_project_run(&run).unwrap();
                assert!(!std::mem::replace(&mut advice.may_send, Box::new(|| false))());
                (advice.observe)(&AdviceEvent::RevokedBeforeSend).unwrap();
                assert!((advice.observe)(&send).is_err());
                (advice.observe)(&AdviceEvent::ToolResultReleased {
                    result_sha256: evidence(&original, &binding(&original).unwrap().unwrap())
                        .unwrap()
                        .held_result_sha256()
                        .into(),
                })
                .unwrap();
                assert_eq!(
                    f.host.omp_advice_delivery_state(&original).unwrap(),
                    "notSent"
                );
            } else {
                assert!(std::mem::replace(&mut advice.may_send, Box::new(|| false))());
                (advice.observe)(&send).unwrap();
                assert!((advice.observe)(&AdviceEvent::MessageObserved {
                    message_id: "before-original-ack".into(),
                })
                .is_err());
                (advice.observe)(&AdviceEvent::Accepted {
                    command_id: "cerulean-omp-steer-1".into(),
                })
                .unwrap();
                assert!(
                    !f.host
                        .supervision_advice_view(&op)
                        .unwrap()
                        .unwrap()
                        .delivered
                );
                (advice.observe)(&AdviceEvent::Queued {
                    state_id: "cerulean-omp-state-1".into(),
                })
                .unwrap();
                (advice.observe)(&AdviceEvent::ToolResultReleased {
                    result_sha256: evidence(&original, &binding(&original).unwrap().unwrap())
                        .unwrap()
                        .held_result_sha256()
                        .into(),
                })
                .unwrap();
                assert!(
                    !f.host
                        .supervision_advice_view(&op)
                        .unwrap()
                        .unwrap()
                        .delivered
                );
                if mode == "confirmed" {
                    (advice.observe)(&AdviceEvent::MessageObserved {
                        message_id: "original-user-message-2".into(),
                    })
                    .unwrap();
                    assert!(
                        f.host
                            .supervision_advice_view(&op)
                            .unwrap()
                            .unwrap()
                            .delivered
                    );
                }
                if matches!(mode, "publication-gap" | "published-pending") {
                    let store = f.host.omp_read("project", "task", "attempt").unwrap();
                    let folder = store
                        .omp_attempt_path("attempt")
                        .unwrap()
                        .join(format!("advice-{op}"));
                    if mode == "published-pending" {
                        fs::hard_link(folder.join("04.json"), folder.join("04.json.pending"))
                            .unwrap();
                    } else {
                        fs::write(folder.join("05.json.pending"), b"partial").unwrap();
                    }
                    assert_eq!(
                        f.host.omp_advice_delivery_state(&original).unwrap(),
                        "adviceOutcomeUnknown"
                    );
                    let path = folder.join("reference.json");
                    let bytes = fs::read(&path).unwrap();
                    let mut changed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                    changed["toolHead"] = json!("f".repeat(64));
                    fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
                    assert!(
                        f.host.omp_advice_delivery_state(&original).is_err(),
                        "pending cannot conceal changed committed reference"
                    );
                    fs::write(&path, bytes).unwrap();
                }
            }
            assert!(!f
                .host
                .request_omp_supervision("project", "attempt")
                .unwrap());
            drop(advice);
            drop(f);
            let mut host = RuntimeHost::open(&temp.path().join("data")).unwrap();
            assert_eq!(
                host.supervision_advice_view(&op)
                    .unwrap()
                    .unwrap()
                    .delivered,
                mode == "confirmed"
            );
            assert_eq!(
                host.require_resolved_omp_advice("project").is_ok(),
                matches!(mode, "confirmed" | "revoked")
            );
            let before = host.management.statuses().len();
            host.poll_project_runs();
            assert_eq!(host.management.statuses().len(), before);
            assert!(host.omp_active.is_empty() && host.management_active.is_empty());
            drop(host);
            RuntimeHost::open(&temp.path().join("data")).unwrap();
        }
    }
}

#[path = "omp_advice_recovery_tests.rs"]
mod recovery_tests;

#[path = "omp_supervision_boundary_tests.rs"]
mod boundary;

#[path = "omp_supervision_exclusive_verification_tests.rs"]
mod exclusive_verification_tests;

#[path = "omp_supervision_live.rs"]
mod live;
