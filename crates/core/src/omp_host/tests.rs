use super::*;
use std::time::{Duration, Instant};

#[path = "tests/worktree.rs"]
mod worktree;

pub(crate) fn fixture(root: &Path, worker: Option<(&Path, &str)>) -> RuntimeHost {
    fixture_with_revision(root, worker, |_, _| {})
}
fn fixture_with_revision(
    root: &Path,
    worker: Option<(&Path, &str)>,
    revise: impl FnOnce(&Path, &mut PlanRevision),
) -> RuntimeHost {
    fs::create_dir(root.join("seed")).unwrap();
    let (store, mut plan) = crate::omp_execution::tests::assigned(&root.join("seed"), false);
    drop(store);
    if let Some((exe, model)) = worker {
        plan.execution[0].worker = Some(WorkerSelection {
            omp_runtime_parent: None,
            provider: "deepseek".into(),
            model: model.into(),
            executable: crate::verifier::PinnedCheck::describe(exe, &[], Default::default())
                .unwrap()
                .executable,
        });
    }
    let mut host = RuntimeHost::open(&root.join("data")).unwrap();
    host.register(plan.project.clone()).unwrap();
    plan.project = host.projects["project"]
        .configuration()
        .execution_scope
        .as_ref()
        .unwrap()
        .project
        .clone();
    revise(root, &mut plan);
    host.propose("project", "proposal", plan.clone()).unwrap();
    host.approve("project", &plan.approval_digest().unwrap())
        .unwrap();
    host
}
pub(crate) fn startup(host: &RuntimeHost, id: &str) -> StartupIntent {
    let store = &host.projects["project"];
    let plan = store.revision().unwrap();
    StartupIntent {
        schema_version: 1,
        protocol: OMP_PROTOCOL.into(),
        project_id: "project".into(),
        task_id: "task".into(),
        attempt_id: id.into(),
        revision_digest: plan.approval_digest().unwrap(),
        worker: plan.execution[0].worker.clone().unwrap(),
        management_operation: None,
        management_hash: None,
        workflow_sequence: store.history().len() as u32,
        workflow_hash: store.history().last().map(|e| e.hash.clone()),
        started_at: chrono::Utc::now().to_rfc3339(),
    }
}
fn control(host: &mut RuntimeHost, command: Command) -> Response {
    host.handle(Request {
        id: "control".into(),
        protocol_version: PROTOCOL_VERSION,
        command,
    })
}

#[test]
fn startup_executor_separation_scope_replay_and_reviewed_unsent_closure() {
    let root = tempfile::tempdir().unwrap();
    let mut host = fixture(root.path(), None);
    let intent = startup(&host, "attempt");
    assert!(host.startups.begin(intent.clone()).is_err());
    let mut wrong = intent.clone();
    wrong.worker.provider = "openai".into();
    assert!(host.omp_startups.begin(wrong).is_err());
    host.omp_startups.begin(intent.clone()).unwrap();
    drop(host);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(host.snapshot().omp_jobs[0].state, OmpJobState::Unknown);
    assert!(host.snapshot().codex_startups.is_empty());
    assert!(host.require_available("project").is_err());
    let review = host.snapshot().omp_jobs[0].closure_digest.clone().unwrap();
    assert!(control(
        &mut host,
        Command::AbandonOmpStartup {
            project_id: "project".into(),
            attempt_id: "attempt".into(),
            evidence_digest: "0".repeat(64),
            reason: "retry with new approved attempt".into()
        }
    )
    .error
    .is_some());
    assert!(control(
        &mut host,
        Command::AbandonOmpStartup {
            project_id: "project".into(),
            attempt_id: "attempt".into(),
            evidence_digest: review,
            reason: "close proven unassigned startup".into()
        }
    )
    .error
    .is_none());
    assert!(host.require_available("project").is_ok());
    assert!(host.omp_startups.begin(intent).is_err());
    drop(host);
    let host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(host.snapshot().omp_jobs[0].state, OmpJobState::Abandoned);
    assert!(host.snapshot().omp_jobs[0].closure_digest.is_none());
}

#[test]
fn missing_credential_and_stale_approval_never_reserve_or_launch() {
    let root = tempfile::tempdir().unwrap();
    let mut host = fixture(root.path(), None);
    let wanted = host.projects["project"]
        .revision()
        .unwrap()
        .approval_digest()
        .unwrap();
    assert!(host
        .start_omp("project", "task", "attempt", &"0".repeat(64))
        .is_err());
    assert!(host
        .start_omp("project", "task", "attempt", &wanted)
        .is_err());
    assert!(host.omp_startups.all().is_empty());
    assert!(host.omp_jobs.is_empty());
    assert!(host.omp_active.is_empty());
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Pending
    );
}

#[test]
fn cancellation_is_durable_bound_and_not_duplicated() {
    let root = tempfile::tempdir().unwrap();
    let mut host = fixture(root.path(), None);
    let i = startup(&host, "attempt");
    host.omp_startups.begin(i.clone()).unwrap();
    let head = host
        .omp_startups
        .get("project", "attempt")
        .unwrap()
        .intent_hash
        .clone();
    crate::async_omp::record_cancel(&host.root, &i, &head, "user stop").unwrap();
    crate::async_omp::record_cancel(&host.root, &i, &head, "second click").unwrap();
    assert_eq!(
        crate::async_omp::cancellation(&host.root, &i, &head)
            .unwrap()
            .as_deref(),
        Some("user stop")
    );
    assert!(crate::async_omp::cancellation(&host.root, &i, &"0".repeat(64)).is_err());
    drop(host);
    let host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(host.snapshot().omp_jobs[0].state, OmpJobState::Unknown);
}

#[test]
fn complete_cancellation_publication_recovers_without_cancelling_or_resending_task() {
    for point in ["temp-synced", "published", "pending-removed"] {
        let root = tempfile::tempdir().unwrap();
        let mut host = fixture(root.path(), None);
        let intent = startup(&host, "attempt");
        host.omp_startups.begin(intent.clone()).unwrap();
        let head = host
            .omp_startups
            .get("project", "attempt")
            .unwrap()
            .intent_hash
            .clone();
        assert!(crate::async_omp::record_cancel_observed(
            &host.root,
            &intent,
            &head,
            "original user reason",
            |p| if p == point {
                Err("simulated lost confirmation".into())
            } else {
                Ok(())
            }
        )
        .is_err());
        let path = host
            .root
            .join("omp-cancellations")
            .join(format!("{}.json", digest(&("project", "attempt")).unwrap()));
        let original = fs::read(if path.exists() {
            path.clone()
        } else {
            path.with_extension("pending")
        })
        .unwrap();
        let events = host.projects["project"].history().len();
        drop(host);
        let host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(host.projects["project"].history().len(), events);
        assert_eq!(host.snapshot().omp_jobs[0].state, OmpJobState::Unknown);
        assert!(host.omp_active.is_empty());
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(!path.with_extension("pending").exists());
        crate::async_omp::record_cancel(&host.root, &intent, &head, "duplicate click").unwrap();
        assert_eq!(
            crate::async_omp::cancellation(&host.root, &intent, &head)
                .unwrap()
                .as_deref(),
            Some("original user reason")
        );
        assert_eq!(fs::read(&path).unwrap(), original);
    }
}

#[test]
fn partial_or_conflicting_cancel_evidence_is_never_promoted_or_removed() {
    for mode in ["temp-created", "partial-written", "wrong-head", "conflict"] {
        let root = tempfile::tempdir().unwrap();
        let mut host = fixture(root.path(), None);
        let intent = startup(&host, "attempt");
        host.omp_startups.begin(intent.clone()).unwrap();
        let head = host
            .omp_startups
            .get("project", "attempt")
            .unwrap()
            .intent_hash
            .clone();
        let point = if matches!(mode, "temp-created" | "partial-written") {
            mode
        } else {
            "temp-synced"
        };
        assert!(crate::async_omp::record_cancel_observed(
            &host.root,
            &intent,
            &head,
            "original reason",
            |p| if p == point {
                Err("interrupted".into())
            } else {
                Ok(())
            }
        )
        .is_err());
        let path = host
            .root
            .join("omp-cancellations")
            .join(format!("{}.json", digest(&("project", "attempt")).unwrap()));
        let pending = path.with_extension("pending");
        let bytes = fs::read(&pending).unwrap();
        if mode == "conflict" {
            let other = tempfile::tempdir().unwrap();
            crate::async_omp::record_cancel(other.path(), &intent, &head, "different valid reason")
                .unwrap();
            fs::copy(
                other
                    .path()
                    .join("omp-cancellations")
                    .join(path.file_name().unwrap()),
                &path,
            )
            .unwrap();
        }
        let published = fs::read(&path).ok();
        let wanted = if mode == "wrong-head" {
            "0".repeat(64)
        } else {
            head.clone()
        };
        assert!(crate::async_omp::cancellation(&host.root, &intent, &wanted).is_err());
        assert_eq!(fs::read(pending).unwrap(), bytes);
        assert_eq!(fs::read(path).ok(), published);
    }
}

#[test]
fn accepted_stop_reason_limit_survives_json_escaping_and_reopen() {
    let root = tempfile::tempdir().unwrap();
    let mut host = fixture(root.path(), None);
    let intent = startup(&host, "attempt");
    host.omp_startups.begin(intent.clone()).unwrap();
    let head = host
        .omp_startups
        .get("project", "attempt")
        .unwrap()
        .intent_hash
        .clone();
    let text = "\u{1}".repeat(8000);
    crate::async_omp::record_cancel(&host.root, &intent, &head, &text).unwrap();
    let path = host.root.clone();
    drop(host);
    let host = RuntimeHost::open(&path).unwrap();
    assert_eq!(
        crate::async_omp::cancellation(&path, &intent, &head).unwrap(),
        Some(text)
    );
    assert_eq!(host.snapshot().omp_jobs[0].state, OmpJobState::Unknown);
    let other = tempfile::tempdir().unwrap();
    assert!(
        crate::async_omp::record_cancel(other.path(), &intent, &head, &"x".repeat(8001)).is_err()
    );
    assert!(!other.path().join("omp-cancellations").exists());
}

#[test]
fn restart_rejects_foreign_startup_workflow_anchor() {
    let root = tempfile::tempdir().unwrap();
    let mut host = fixture(root.path(), None);
    let mut i = startup(&host, "attempt");
    i.workflow_hash = Some("0".repeat(64));
    host.omp_startups.begin(i).unwrap();
    drop(host);
    assert!(RuntimeHost::open(&root.path().join("data")).is_err());
}

#[test]
fn explicit_no_send_evidence_recovers_as_cancelled_without_replay() {
    use cerulean_adapters::omp_transport::worker::WorkerEvent;
    let root = tempfile::tempdir().unwrap();
    let mut host = fixture(root.path(), None);
    host.omp_startups.begin(startup(&host, "attempt")).unwrap();
    let store = host.projects.get_mut("project").unwrap();
    let plan = store.revision().unwrap().clone();
    let attempt = Attempt {
        workspace_proof: None,
        id: "attempt".into(),
        task_id: "task".into(),
        worker_id: "worker-omp".into(),
        revision_digest: plan.approval_digest().unwrap(),
        fence: 1,
        binding: SessionBinding {
            adapter_id: "omp".into(),
            adapter_version: "host-rpc-v2".into(),
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
                execution: Some(ExecutionEvidence::Attempt { attempt }),
                ..Default::default()
            },
            false,
        )
        .unwrap();
    let (_, _, intent, _) = crate::omp_execution::scope(store, "task", "attempt").unwrap();
    store
        .command(
            "dispatch",
            "core",
            WorkflowCommand::DispatchIntent {
                task: "task".into(),
                attempt: "attempt".into(),
            },
            CommandContext {
                thread_id: Some("session".into()),
                cwd: Some(plan.project.root),
                ..Default::default()
            },
            false,
        )
        .unwrap();
    let mut journal =
        crate::omp_journal::OmpJournal::open(&store.omp_attempt_path("attempt").unwrap(), &intent)
            .unwrap();
    journal
        .record(&WorkerEvent::DispatchIntent { intent })
        .unwrap();
    journal
        .record(&WorkerEvent::NotStarted {
            request_id: "prompt-request".into(),
        })
        .unwrap();
    drop(journal);
    drop(host);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(host.snapshot().omp_jobs[0].state, OmpJobState::Unknown);
    let response = control(
        &mut host,
        Command::RecoverOmp {
            project_id: "project".into(),
            attempt_id: "attempt".into(),
        },
    );
    assert!(response.error.is_none(), "{:?}", response.error);
    assert_eq!(host.snapshot().omp_jobs[0].state, OmpJobState::Cancelled);
    let count = host.projects["project"].history().len();
    assert!(control(
        &mut host,
        Command::RecoverOmp {
            project_id: "project".into(),
            attempt_id: "attempt".into()
        }
    )
    .error
    .is_some());
    assert_eq!(host.projects["project"].history().len(), count);
}

#[test]
#[ignore = "explicit offline worker executable and configuration-free parent required; no provider calls"]
fn actual_resident_omp_worker_success_cancel_failure_and_restart() {
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
        host.start_omp("project", "task", "attempt", &wanted)
            .unwrap();
        assert_eq!(host.snapshot().omp_jobs[0].state, OmpJobState::Starting);
        let original_key = fs::read(host.root.join("credentials/deepseek.dpapi")).unwrap();
        assert!(host
            .set_provider_key(credentials::CredentialProvider::DeepSeek, None)
            .is_err());
        assert!(host
            .set_provider_key(
                credentials::CredentialProvider::DeepSeek,
                Some(secret::SecretInput::new("sk-replacement-must-not-save".into()).unwrap())
            )
            .is_err());
        assert_eq!(
            fs::read(host.root.join("credentials/deepseek.dpapi")).unwrap(),
            original_key
        );
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
