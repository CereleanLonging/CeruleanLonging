use super::super::omp_host::tests::{fixture, startup};
use super::*;
use crate::{
    async_omp::{ActiveOmp, Update},
    managed_assignment::Update as Assignment,
    management_execution::DecisionIntent,
};
use cerulean_domain::{management::DecisionEnvelope, omp_jobs::OmpJobState};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};

#[path = "assignment_crash_tests.rs"]
mod assignment_crash_tests;

#[path = "publication_tests.rs"]
mod publication_tests;

#[path = "review_tests.rs"]
mod review_tests;

pub(crate) fn decision(host: &mut RuntimeHost) {
    let context = host
        .management_context("project", "Implement the approved task")
        .unwrap();
    let state_digest = digest(&context).unwrap();
    host.management
        .begin(DecisionIntent {
            operation_id: "deputy-omp".into(),
            project_id: "project".into(),
            purpose: Purpose::Assign,
            model: "fixture-model".into(),
            state_digest: state_digest.clone(),
            context,
            created_at: chrono::Utc::now().to_rfc3339(),
        })
        .unwrap();
    host.management
        .finish(
            "deputy-omp",
            Ok(cerulean_adapters::openai::DecisionObservation {
                response_id: "resp_fixture".into(),
                requested_model: "fixture-model".into(),
                actual_model: "fixture-model".into(),
                input_tokens: 10,
                output_tokens: 20,
                envelope: DecisionEnvelope {
                    state_digest,
                    decision: ManagementDecision::Assign {
                        task_id: "task".into(),
                        worker_id: "worker-omp".into(),
                        rationale: "approved bounded task".into(),
                    },
                },
            }),
        )
        .unwrap();
}
pub(crate) fn reserve(host: &mut RuntimeHost) {
    let mut i = startup(host, "attempt");
    i.management_operation = Some("deputy-omp".into());
    i.management_hash = Some(host.management.response_head("deputy-omp").unwrap().into());
    host.omp_startups.begin(i).unwrap();
    host.load_omp_jobs().unwrap();
}
fn simulated(host: &mut RuntimeHost) -> (std::thread::JoinHandle<()>, Arc<AtomicUsize>) {
    reserve(host);
    let store = host.projects.remove("project").unwrap();
    let job = host.omp_jobs.values().next().unwrap().clone();
    let snapshot = crate::async_verification::snapshot(&store);
    let attempt = Attempt {
        workspace_proof: None,
        id: "attempt".into(),
        task_id: "task".into(),
        worker_id: "worker-omp".into(),
        revision_digest: job.revision_digest.clone(),
        fence: 1,
        binding: SessionBinding {
            adapter_id: "omp".into(),
            adapter_version: "fixture".into(),
            session_id: "session".into(),
            identity_strength: IdentityStrength::ControllerRequest(ControllerRequestIdentity {
                connection_id: "connection".into(),
                request_id: "prompt-request".into(),
            }),
            workspace: snapshot.project.root.clone(),
        },
    };
    let writer =
        crate::async_api::ApiWorkflow::start(store, host.root.join("workflows/project")).unwrap();
    let mut store = writer
        .owner
        .omp_client(crate::workflow_coordinator::Scope {
            project: "project".into(),
            task: "task".into(),
            attempt: "attempt".into(),
            revision: job.revision_digest.clone(),
        })
        .unwrap();
    host.api_writers.insert("project".into(), writer);
    let (sender, receiver) = mpsc::channel();
    host.omp_active.insert(
        ("project".into(), "attempt".into()),
        ActiveOmp {
            snapshot,
            job,
            receiver,
            cancel: None,
            thread: None,
            disconnected: false,
            tool_checkpoint: None,
            advice_waiter: None,
            advice_gate: None,
            preparation_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        },
    );
    let count = Arc::new(AtomicUsize::new(0));
    let after = count.clone();
    let thread = std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let outcome = rt.block_on(crate::managed_assignment::assign(
            &mut store,
            "deputy-omp",
            attempt,
            |u| {
                sender
                    .send(Update::Assignment(u))
                    .map_err(|_| "closed".into())
            },
        ));
        if outcome.is_ok() {
            after.fetch_add(1, Ordering::SeqCst);
        }
        drop(store);
        sender
            .send(Update::Done {
                interrupted: false,
                outcome,
                cleaned: true,
            })
            .unwrap();
    });
    (thread, count)
}
fn next(host: &RuntimeHost) -> Assignment {
    match host.omp_active[&("project".into(), "attempt".into())]
        .receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap()
    {
        Update::Assignment(u) => u,
        _ => panic!("assignment expected"),
    }
}
fn drain(host: &mut RuntimeHost, thread: std::thread::JoinHandle<()>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !host.omp_active.is_empty() && Instant::now() < deadline {
        host.poll_verifications();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(host.omp_active.is_empty());
    thread.join().unwrap();
}
#[test]
fn managed_omp_ack_loss_recovery_and_no_resend() {
    for stage in [
        "before-prepare",
        "prepared",
        "assigned",
        "applied",
        "acknowledged",
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut host = fixture(root.path(), None);
        decision(&mut host);
        let (thread, after) = simulated(&mut host);
        let Assignment::Prepare {
            operation,
            attempt,
            snapshot,
            reply,
        } = next(&host)
        else {
            panic!("prepare")
        };
        if stage == "before-prepare" {
            drop(reply);
        } else {
            host.prepare_managed_assignment("project", &operation, *attempt, *snapshot, "omp")
                .unwrap();
            assert!(host.application_recovery_review("deputy-omp").is_err());
            if stage == "prepared" {
                drop(reply);
            } else {
                reply.send(Ok(())).unwrap();
                let Assignment::Commit {
                    operation,
                    entry,
                    snapshot,
                    reply,
                } = next(&host)
                else {
                    panic!("commit")
                };
                assert_eq!(after.load(Ordering::SeqCst), 0);
                if stage == "assigned" {
                    drop(reply);
                } else {
                    host.commit_managed_assignment("project", &operation, *entry, *snapshot, "omp")
                        .unwrap();
                    if stage == "applied" {
                        drop(reply);
                    } else {
                        reply.send(Ok(())).unwrap();
                    }
                }
            }
        }
        drain(&mut host, thread);
        assert_eq!(
            after.load(Ordering::SeqCst),
            usize::from(stage == "acknowledged")
        );
        assert!(!host.projects["project"]
            .history()
            .iter()
            .any(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. })));
        drop(host);
        let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert!(host
            .start_managed_omp("deputy-omp", "another-attempt")
            .is_err());
        if stage == "before-prepare" {
            let review = host.snapshot().omp_jobs[0].closure_digest.clone().unwrap();
            host.abandon_omp_startup("project", "attempt", &review, "close unused startup")
                .unwrap();
            assert!(host.management.startup_closure("deputy-omp").is_some());
        } else if stage == "prepared" {
            assert!(host.snapshot().omp_jobs[0].closure_digest.is_none());
            let review = host.application_recovery_review("deputy-omp").unwrap();
            assert!(host
                .abandon_management_application("deputy-omp", &"0".repeat(64), "stale")
                .is_err());
            host.abandon_management_application(
                "deputy-omp",
                &review.evidence_digest,
                "close original prepared intent",
            )
            .unwrap();
        } else {
            let (_, applied, receipt) = host.management.application("deputy-omp").unwrap();
            assert!(applied && receipt.is_some());
            assert_eq!(
                host.projects["project"].state().tasks["task"].state,
                TaskState::Assigned
            );
            assert!(host.application_recovery_review("deputy-omp").is_err());
        }
        let count = host.projects["project"].history().len();
        drop(host);
        let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(host.projects["project"].history().len(), count);
        assert_eq!(
            host.snapshot().omp_jobs[0].management_operation.as_deref(),
            Some("deputy-omp")
        );
        if matches!(stage, "before-prepare" | "prepared") {
            assert_eq!(host.snapshot().omp_jobs[0].state, OmpJobState::Abandoned);
        }
        assert!(host
            .start_managed_omp("deputy-omp", "another-attempt")
            .is_err());
    }
}
#[test]
fn managed_omp_rejects_foreign_preparation_and_unpublished_commit() {
    let root = tempfile::tempdir().unwrap();
    let mut host = fixture(root.path(), None);
    decision(&mut host);
    let (thread, after) = simulated(&mut host);
    let Assignment::Prepare {
        operation,
        attempt,
        snapshot,
        reply,
    } = next(&host)
    else {
        panic!("prepare")
    };
    for variant in 0..7 {
        let mut bad = (*attempt).clone();
        match variant {
            0 => bad.worker_id = "worker-codex".into(),
            1 => bad.binding.identity_strength = IdentityStrength::ProviderId,
            2 => bad.id = "foreign".into(),
            3 => bad.revision_digest = "0".repeat(64),
            4 => bad.fence = 2,
            5 => bad.binding.session_id.clear(),
            _ => {
                bad.binding.identity_strength =
                    IdentityStrength::ControllerRequest(ControllerRequestIdentity {
                        connection_id: "connection".into(),
                        request_id: "cerulean-omp-forged".into(),
                    })
            }
        };
        assert!(host
            .prepare_managed_assignment("project", &operation, bad, (*snapshot).clone(), "omp")
            .is_err());
        assert!(host.management.application(&operation).is_none());
    }
    host.prepare_managed_assignment("project", &operation, *attempt, *snapshot, "omp")
        .unwrap();
    reply.send(Ok(())).unwrap();
    let Assignment::Commit {
        operation,
        entry,
        snapshot,
        reply,
    } = next(&host)
    else {
        panic!("commit")
    };
    let path = host.root.join("workflows/project/events").join(format!(
        "{:012}_{}.json",
        entry.body.event.sequence, entry.body.operation_id
    ));
    let held = path.with_extension("held");
    fs::rename(&path, &held).unwrap();
    assert!(host
        .commit_managed_assignment(
            "project",
            &operation,
            (*entry).clone(),
            (*snapshot).clone(),
            "omp"
        )
        .is_err());
    assert!(!host.management.application(&operation).unwrap().1);
    fs::rename(held, path).unwrap();
    host.commit_managed_assignment("project", &operation, *entry, *snapshot, "omp")
        .unwrap();
    reply.send(Ok(())).unwrap();
    drain(&mut host, thread);
    assert_eq!(after.load(Ordering::SeqCst), 1);
}
#[test]
fn omp_startup_without_management_link_cannot_claim_managed_assignment() {
    let root = tempfile::tempdir().unwrap();
    let mut host = fixture(root.path(), None);
    decision(&mut host);
    let i = startup(&host, "attempt");
    host.omp_startups.begin(i).unwrap();
    let plan = host.projects["project"].revision().unwrap();
    let attempt = Attempt {
        workspace_proof: None,
        id: "attempt".into(),
        task_id: "task".into(),
        worker_id: "worker-omp".into(),
        revision_digest: plan.approval_digest().unwrap(),
        fence: 1,
        binding: SessionBinding {
            adapter_id: "omp".into(),
            adapter_version: "fixture".into(),
            session_id: "session".into(),
            identity_strength: IdentityStrength::ControllerRequest(ControllerRequestIdentity {
                connection_id: "connection".into(),
                request_id: "request".into(),
            }),
            workspace: plan.project.root.clone(),
        },
    };
    let state = host
        .management
        .intent("deputy-omp")
        .unwrap()
        .state_digest
        .clone();
    host.management
        .prepare_application(
            "deputy-omp",
            ApplicationTarget::Workflow {
                principal: "deputy".into(),
                command: WorkflowCommand::Assign {
                    task: "task".into(),
                    worker: "worker-omp".into(),
                    attempt: "attempt".into(),
                },
                context: Box::new(CommandContext {
                    execution: Some(ExecutionEvidence::Attempt { attempt }),
                    ..Default::default()
                }),
            },
            &state,
        )
        .unwrap();
    drop(host);
    assert!(RuntimeHost::open(&root.path().join("data")).is_err());
}

fn control(host: &mut RuntimeHost, command: Command) -> Response {
    host.handle(Request {
        id: "control".into(),
        protocol_version: PROTOCOL_VERSION,
        command,
    })
}
#[test]
#[ignore = "explicit offline worker executable and configuration-free parent required; no provider calls"]
fn actual_managed_omp_worker_success_cancel_failure_and_restart() {
    let parent =
        PathBuf::from(std::env::var_os("CERULEAN_OMP_TEST_PARENT").expect("isolated parent"));
    let exe = PathBuf::from(
        std::env::var_os("CERULEAN_OMP_FIXTURE_EXE").expect("offline fixture executable"),
    );
    assert_eq!(exe.file_name().unwrap(), "omp_offline_worker_fixture.exe");
    for model in [
        "fixture-file-completed",
        "fixture-file-wait",
        "fixture-file-lost-terminal",
        "fixture-wrong-model",
    ] {
        let root = tempfile::tempdir_in(&parent).unwrap();
        let mut host = fixture(root.path(), Some((&exe, model)));
        crate::credential_vault::save_for(
            &host.root,
            credentials::CredentialProvider::DeepSeek,
            &secret::SecretInput::new("cerulean-offline-placeholder-not-a-real-key".into())
                .unwrap(),
        )
        .unwrap();
        host.deepseek_key_configured = true;
        let wanted = host.projects["project"]
            .revision()
            .unwrap()
            .approval_digest()
            .unwrap();
        decision(&mut host);
        host.start_managed_omp("deputy-omp", "attempt").unwrap();
        assert_eq!(host.snapshot().omp_jobs[0].state, OmpJobState::Starting);
        assert!(!host.can_shutdown());
        assert!(host
            .start_omp("project", "task", "second-attempt", &wanted)
            .is_err());
        assert_eq!(host.snapshot().workflows.len(), 1);
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut cancelled = false;
        while !host.omp_active.is_empty() && Instant::now() < deadline {
            host.poll_verifications();
            if model == "fixture-file-wait"
                && !cancelled
                && root.path().join("seed/project/result.txt").exists()
            {
                let response = control(
                    &mut host,
                    Command::CancelOmp {
                        project_id: "project".into(),
                        attempt_id: "attempt".into(),
                        reason: "offline user cancellation".into(),
                    },
                );
                assert!(response.error.is_none(), "{:?}", response.error);
                cancelled = true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            host.omp_active.is_empty(),
            "{model}: {:?}",
            host.snapshot().omp_jobs
        );
        let expected = match model {
            "fixture-file-completed" => OmpJobState::Submitted,
            "fixture-file-wait" => OmpJobState::Cancelled,
            "fixture-wrong-model" => OmpJobState::Failed,
            _ => OmpJobState::Unknown,
        };
        assert_eq!(
            host.snapshot().omp_jobs[0].state,
            expected,
            "{model}: {:?}",
            host.snapshot().omp_jobs
        );
        assert_eq!(
            host.snapshot().omp_jobs[0].management_operation.as_deref(),
            Some("deputy-omp")
        );
        if model != "fixture-wrong-model" {
            let (_, applied, receipt) = host.management.application("deputy-omp").unwrap();
            assert!(applied && receipt.is_some());
            assert_eq!(
                host.projects["project"]
                    .history()
                    .iter()
                    .filter(|e| e.body.operation_id == "mgmt-deputy-omp")
                    .count(),
                1
            );
        }
        assert!(host
            .start_managed_omp("deputy-omp", "second-attempt")
            .is_err());
        let history = host.projects["project"].history().len();
        assert!(host
            .start_omp("project", "task", "attempt", &wanted)
            .is_err());
        assert_eq!(host.projects["project"].history().len(), history);
        if model == "fixture-wrong-model" {
            let review = host.snapshot().omp_jobs[0].closure_digest.clone().unwrap();
            assert!(control(
                &mut host,
                Command::AbandonOmpStartup {
                    project_id: "project".into(),
                    attempt_id: "attempt".into(),
                    evidence_digest: review,
                    reason: "close failed handshake".into()
                }
            )
            .error
            .is_none());
        } else {
            assert_eq!(
                fs::read_to_string(root.path().join("seed/project/result.txt")).unwrap(),
                "fixture output"
            );
        }
        drop(host);
        let host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(
            host.snapshot().omp_jobs[0].state,
            if model == "fixture-wrong-model" {
                OmpJobState::Abandoned
            } else {
                expected
            }
        );
        assert!(host.omp_active.is_empty());
        assert_eq!(host.snapshot().workflows.len(), 1);
        if model != "fixture-wrong-model" {
            assert_eq!(host.projects["project"].history().len(), history);
        }
    }
}
