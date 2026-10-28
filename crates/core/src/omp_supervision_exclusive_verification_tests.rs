//! Process-free regression for a ledger borrowed by exclusive verification.
use super::*;
use crate::async_verification::{ActiveVerification, Completion};
use cerulean_adapters::omp_transport::worker::advice::AdviceEvent;
use cerulean_domain::project_setup::RunState;

fn observed_fixture(root: &Path) -> (Fixture, String) {
    let mut f = fixture_delivery(root, 8, true);
    f.failure("a");
    f.failure("b");
    let (reply, mut receiver) = tokio::sync::oneshot::channel();
    f.host
        .handle_omp_advice_boundary("project", "attempt", Some(reply))
        .unwrap();
    f.host.poll_project_runs();
    let op = f.wait();
    f.host.poll_project_runs();
    assert!(f.host.management.application(&op).unwrap().1);
    let mut advice = receiver.try_recv().unwrap().unwrap().unwrap();
    assert_eq!(advice.operation_id, op);
    assert!(std::mem::replace(&mut advice.may_send, Box::new(|| false))());
    (advice.observe)(&AdviceEvent::SendIntent {
        command_id: "exclusive-command".into(),
        message_sha256: crate::omp_advice_journal::text_hash(&advice.message),
    })
    .unwrap();
    (advice.observe)(&AdviceEvent::Accepted {
        command_id: "exclusive-command".into(),
    })
    .unwrap();
    (advice.observe)(&AdviceEvent::Queued {
        state_id: "exclusive-state".into(),
    })
    .unwrap();
    let original = f.host.management.intent(&op).unwrap().clone();
    (advice.observe)(&AdviceEvent::ToolResultReleased {
        result_sha256: evidence(&original, &binding(&original).unwrap().unwrap())
            .unwrap()
            .held_result_sha256()
            .into(),
    })
    .unwrap();
    (advice.observe)(&AdviceEvent::MessageObserved {
        message_id: "exclusive-message".into(),
    })
    .unwrap();
    assert!(
        f.host
            .supervision_advice_view(&op)
            .unwrap()
            .unwrap()
            .delivered
    );
    drop(advice);
    (f, op)
}

fn submitted_store(f: &mut Fixture) -> DurableWorkflow {
    f.host
        .omp_active
        .remove(&("project".into(), "attempt".into()));
    let mut writer = f.host.api_writers.remove("project").unwrap();
    let mut store = writer.owner.finish(false).unwrap();
    let (plan, attempt, _, _) = crate::omp_execution::scope(&store, "task", "attempt").unwrap();
    fs::write(
        Path::new(&plan.project.root).join("sum.mjs"),
        b"export const sum = (a,b) => a+b;\n",
    )
    .unwrap();
    let receipt = ExecutionReceipt {
        schema_version: 1,
        task_id: "task".into(),
        attempt_id: "attempt".into(),
        fence: attempt.fence,
        binding: attempt.binding.clone(),
        turn_id: None,
        observed_at: chrono::Utc::now().to_rfc3339(),
        status: ReceiptStatus::Completed,
        evidence_sha256: "a".repeat(64),
    };
    let approval = store.approval().unwrap();
    let manifest = crate::artifacts::ArtifactObserver::new(
        &plan,
        approval,
        &approval.user_id,
        plan.policy.version,
        &attempt,
        &receipt,
        None,
    )
    .unwrap()
    .capture()
    .unwrap();
    store
        .record_omp(
            "worker-omp",
            WorkflowCommand::Submit {
                task: "task".into(),
                attempt: "attempt".into(),
                summary: "Synthetic completed worker; no process or model call".into(),
                evidence: Some(vec![digest(&manifest).unwrap()]),
            },
            CommandContext {
                thread_id: Some(receipt.binding.session_id.clone()),
                terminal_status: Some("completed".into()),
                execution: Some(ExecutionEvidence::Receipt {
                    receipt,
                    manifest: Some(manifest),
                }),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(store.state().tasks["task"].state, TaskState::Submitted);
    store
}

fn hold_verification(f: &mut Fixture, store: &DurableWorkflow) -> mpsc::Sender<Completion> {
    let now = chrono::Utc::now().to_rfc3339();
    let job = VerificationJob {
        project_id: "project".into(),
        operation_id: "exclusive-check".into(),
        task_id: "task".into(),
        attempt_id: "attempt".into(),
        state: VerificationState::Running,
        started_at: now.clone(),
        updated_at: now,
        message: None,
        reconciliation_digest: None,
    };
    let (sender, receiver) = mpsc::channel();
    let active = ActiveVerification {
        derived_diagnostics: Some(store.derived_diagnostics()),
        shared: false,
        snapshot: crate::async_verification::snapshot(store),
        detail_basis: crate::verification_detail::basis(store, &job).unwrap(),
        job: job.clone(),
        cancel: Arc::new(AtomicBool::new(false)),
        receiver,
    };
    crate::async_verification::save(&f.host.root, &job).unwrap();
    f.host
        .jobs
        .insert(("project".into(), job.operation_id.clone()), job);
    f.host.active.insert("project".into(), active);
    sender
}

fn assert_held(f: &mut Fixture, advice: &str, requests: usize) {
    assert!(!f.host.projects.contains_key("project"));
    assert!(!f.host.api_writers.contains_key("project"));
    assert!(!f.host.active["project"].shared);
    for _ in 0..8 {
        f.host.poll_project_runs();
        let run = &f.host.runs.all()[0];
        assert_eq!(run.state, RunState::Active, "{:?}", run);
        assert!(!serde_json::to_string(run)
            .unwrap()
            .contains("OMP项目不可用"));
        assert_eq!(f.host.management.statuses().len(), requests);
        assert!(f.host.management_active.is_empty());
        assert!(f.host.management.application(advice).unwrap().1);
    }
}

fn manager_accept(intent: DecisionIntent) -> Result<DecisionObservation, String> {
    assert_eq!(intent.purpose, Purpose::Accept);
    assert_eq!(
        intent.context["workflow"]["state"]["tasks"]["task"]["verified"],
        true
    );
    Ok(DecisionObservation {
        response_id: "resp_exclusive_chief".into(),
        requested_model: intent.model.clone(),
        actual_model: intent.model,
        input_tokens: 1,
        output_tokens: 1,
        envelope: DecisionEnvelope {
            state_digest: intent.state_digest,
            decision: ManagementDecision::Accept {
                task_id: "task".into(),
                attempt_id: "attempt".into(),
                rationale: "Original independent proof passed".into(),
            },
        },
    })
}

fn accept_count(host: &RuntimeHost) -> usize {
    host.projects.get("project").map_or(0, |store| {
        store
            .history()
            .iter()
            .filter(|entry| matches!(entry.body.event.command, WorkflowCommand::Accept { .. }))
            .count()
    })
}

#[test]
fn exclusive_verification_waits_before_observed_advice_then_accepts_once() {
    let temp = tempfile::tempdir().unwrap();
    let (mut f, advice) = observed_fixture(temp.path());
    let mut store = submitted_store(&mut f);
    let requests = f.host.management.statuses().len();
    // Sender ownership is the completion barrier: assertion failure cannot strand a thread.
    let completion = hold_verification(&mut f, &store);
    assert_held(&mut f, &advice, requests);
    let prepared = store
        .prepare_verification("exclusive-check", "task", "attempt")
        .unwrap();
    let plan = store.revision().unwrap().clone();
    let (_, manifest) = store.completed_artifacts("task", "attempt").unwrap();
    let manifest = manifest.clone();
    let dir = f
        .host
        .root
        .join("workflows/project/verification/exclusive-check");
    fs::create_dir_all(&dir).unwrap();
    let lease =
        crate::source_snapshot::SnapshotLease::create(&plan, &manifest, &dir, "exclusive-check")
            .unwrap();
    // Trusted synthetic runner observations, bound to the actual sealed source tree.
    // No validator executable is started.
    let report = VerificationReport {
        schema_version: 1,
        verifier_id: store.configuration().core_id.clone(),
        observed_at: chrono::Utc::now().to_rfc3339(),
        manifest_digest: digest(&manifest).unwrap(),
        checks: plan
            .task("task")
            .unwrap()
            .checks
            .iter()
            .map(|check| CheckObservation {
                check_id: check.id.clone(),
                command_digest: digest(check).unwrap(),
                exit_code: Some(0),
                timed_out: false,
                stdout_sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
                    .into(),
                stderr_sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
                    .into(),
            })
            .collect(),
        source_snapshot: Some(lease.seal.clone()),
    };
    report
        .require_pass(&plan, &manifest, &store.configuration().core_id)
        .unwrap();
    store
        .finish_prepared_verification(&prepared, report)
        .unwrap();
    assert!(store.state().tasks["task"].verified);
    drop(lease);
    f.host.management_fixture = Some(manager_accept);
    assert!(completion
        .send(Completion {
            store: Some(Ok(store)),
            outcome: Ok(true),
            uncertain: false
        })
        .is_ok());
    f.host.poll_verifications();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !f.host.management_active.is_empty() {
        assert!(Instant::now() < deadline);
        f.host.poll_management();
        std::thread::yield_now();
    }
    for _ in 0..8 {
        f.host.poll_project_runs();
    }
    assert_eq!(
        accept_count(&f.host),
        1,
        "runs={:?}; management={:?}; jobs={:?}",
        f.host.runs.all(),
        f.host.management.statuses(),
        f.host.omp_jobs
    );
    assert_eq!(
        f.host.projects["project"].state().tasks["task"].state,
        TaskState::Accepted
    );
    assert_eq!(f.host.runs.all()[0].state, RunState::Completed);
    assert_eq!(f.host.management.statuses().len(), requests + 1);
    assert_eq!(
        f.host
            .management
            .statuses()
            .iter()
            .filter(|s| s.purpose == Purpose::Accept)
            .count(),
        1
    );
    assert!(
        f.host
            .supervision_advice_view(&advice)
            .unwrap()
            .unwrap()
            .delivered
    );
}

fn unsuccessful_completion(unknown: bool) {
    let temp = tempfile::tempdir().unwrap();
    let (mut f, advice) = observed_fixture(temp.path());
    let store = submitted_store(&mut f);
    let requests = f.host.management.statuses().len();
    let completion = hold_verification(&mut f, &store);
    assert_held(&mut f, &advice, requests);
    assert!(completion
        .send(Completion {
            store: Some(Ok(store)),
            outcome: if unknown {
                Err("synthetic result unavailable; reconciliation required".into())
            } else {
                Ok(false)
            },
            uncertain: unknown,
        })
        .is_ok());
    f.host.poll_verifications();
    for _ in 0..8 {
        f.host.poll_project_runs();
    }
    assert_eq!(
        f.host.jobs[&("project".into(), "exclusive-check".into())].state,
        if unknown {
            VerificationState::Unknown
        } else {
            VerificationState::Failed
        }
    );
    assert_eq!(f.host.runs.all()[0].state, RunState::Attention);
    assert_eq!(accept_count(&f.host), 0);
    assert!(!f.host.projects["project"].state().tasks["task"].verified);
    assert_eq!(f.host.management.statuses().len(), requests);
    assert!(f.host.management_active.is_empty());
    assert!(f.host.active.is_empty());
    assert!(
        f.host
            .supervision_advice_view(&advice)
            .unwrap()
            .unwrap()
            .delivered
    );
}

#[test]
fn exclusive_verification_failed_completion_never_accepts_or_replays() {
    unsuccessful_completion(false);
}

#[test]
fn exclusive_verification_unknown_completion_never_accepts_or_replays() {
    unsuccessful_completion(true);
}

#[test]
fn missing_project_without_exclusive_verification_still_rejects() {
    let temp = tempfile::tempdir().unwrap();
    let (mut f, _) = observed_fixture(temp.path());
    let store = submitted_store(&mut f);
    let requests = f.host.management.statuses().len();
    assert!(!f.host.active.contains_key("project"));
    f.host.poll_project_runs();
    assert_eq!(f.host.runs.all()[0].state, RunState::Attention);
    assert_eq!(f.host.management.statuses().len(), requests);
    assert_eq!(store.state().tasks["task"].state, TaskState::Submitted);
}

#[test]
fn projection_failure_remains_visible_during_exclusive_verification() {
    use cerulean_domain::workspace::{WorkspaceQuery, WorkspaceResult};
    let temp = tempfile::tempdir().unwrap();
    let (mut f, _) = observed_fixture(temp.path());
    let store = submitted_store(&mut f);
    let config = store.configuration().clone();
    let before = store.history().to_vec();
    let root = f.host.root.join("workflows/project");
    drop(store);
    fs::remove_file(root.join("state.json")).unwrap();
    fs::create_dir(root.join("state.json")).unwrap();
    let store = DurableWorkflow::open(&root, config).unwrap();
    assert!(store
        .wait_for_projections(std::time::Duration::from_secs(20))
        .is_err());
    let completion = hold_verification(&mut f, &store);
    let WorkspaceResult::Overview(value) =
        f.host.workspace_query(&WorkspaceQuery::Overview).unwrap()
    else {
        panic!("overview")
    };
    assert!(value
        .message
        .unwrap()
        .contains("日报、查询索引或恢复缓存更新失败"));
    assert_eq!(store.history(), before);
    drop(completion);
}
