//! Public parallel admission. Two real API worker threads,
//! actual approved Git worktrees and durable management ACKs; synthetic replies.
//! Original management context remains authoritative across other-task progress.
use super::*;
use std::sync::{mpsc, Mutex};

type Gate = (mpsc::Sender<()>, mpsc::Receiver<()>);
static GATES: Mutex<BTreeMap<String, Gate>> = Mutex::new(BTreeMap::new());
fn gated(prepared: &PreparedRequest, round: u32) -> Result<ApiReply, ApiFailure> {
    let gate = GATES
        .lock()
        .unwrap()
        .remove(&prepared.fingerprint().requested_model);
    if let Some((entered, release)) = gate {
        entered.send(()).unwrap();
        release.recv_timeout(Duration::from_secs(40)).unwrap();
    }
    reply(prepared, round)
}
fn gate(model: &str) -> (mpsc::Receiver<()>, mpsc::Sender<()>) {
    let (entered, waiting) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    assert!(GATES
        .lock()
        .unwrap()
        .insert(model.into(), (entered, resume))
        .is_none());
    (waiting, release)
}
fn wait_for(host: &mut RuntimeHost, condition: impl Fn(&RuntimeHost) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        host.poll_verifications();
        if condition(host) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "waiting for registry fixture: {:?}",
            host.snapshot().api_jobs
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

// Reuse controlled management responses, but obtain context and start both
// workers through production public admission; no direct registry insertion.
fn prepare_second(host: &mut RuntimeHost) {
    let context = host
        .management_context("project", "Execute the approved second task")
        .unwrap();
    let state_digest = digest(&context).unwrap();
    host.management
        .begin(crate::management_execution::DecisionIntent {
            operation_id: "deputy-second".into(),
            project_id: "project".into(),
            purpose: Purpose::Assign,
            model: "fixture".into(),
            state_digest: state_digest.clone(),
            context,
            created_at: chrono::Utc::now().to_rfc3339(),
        })
        .unwrap();
    host.management
        .finish(
            "deputy-second",
            Ok(DecisionObservation {
                response_id: "resp_second".into(),
                requested_model: "fixture".into(),
                actual_model: "fixture".into(),
                input_tokens: 1,
                output_tokens: 1,
                envelope: DecisionEnvelope {
                    state_digest,
                    decision: ManagementDecision::Assign {
                        task_id: "second".into(),
                        worker_id: "worker-deepseek".into(),
                        rationale: "approved isolated task".into(),
                    },
                },
            }),
        )
        .unwrap();
}

fn parallel_fixture(root: &Path) -> RuntimeHost {
    fixture_with_revision(
        root,
        project_setup::ProjectWorkerAdapter::Deepseek,
        ApiTarget::Provider {
            endpoint: credentials::ProviderEndpoint::DeepSeek {},
        },
        true,
        |root, plan| {
            worktree::revise(root, plan);
            plan.policy.max_requests = 12;
            plan.workflow.tasks[0].workspace_key = isolated_task_key("project", "task").unwrap();
            let mut task = plan.workflow.tasks[0].clone();
            task.id = "second".into();
            task.workspace_key = isolated_task_key("project", "second").unwrap();
            plan.workflow.tasks.push(task);
            let mut execution = plan.execution[0].clone();
            execution.task_id = "second".into();
            execution.api_worker.as_mut().unwrap().model = "second-model".into();
            plan.execution.push(execution);
        },
    )
}

#[test]
#[ignore = "explicit real Git; shared registry with two controlled production workers; public parallel admission"]
fn shared_writer_survives_first_completion_or_cancel_and_stale_worker_progress() {
    for cancel_first in [false, true] {
        let parent =
            fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports")).unwrap();
        let root = tempfile::Builder::new()
            .prefix("api-registry-")
            .tempdir_in(parent)
            .unwrap();
        let mut host = parallel_fixture(root.path());
        let first_model = host.projects["project"]
            .revision()
            .unwrap()
            .task("task")
            .unwrap()
            .api_worker
            .as_ref()
            .unwrap()
            .model
            .clone();
        let (first_waiting, first_release) = gate(&first_model);
        let (second_waiting, second_release) = gate("second-model");
        host.api_fixture = Some(gated);
        host.start_managed_api("deputy-api", "attempt").unwrap();
        let first_key = ("project".into(), "attempt".into());
        let second_key = ("project".into(), "second-attempt".into());
        let stale = host.api_active[&first_key].snapshot.clone();
        wait_for(&mut host, |_| first_waiting.try_recv().is_ok());
        prepare_second(&mut host);
        // A changed manager context must not be excused as unrelated progress.
        let validators = host.validators.clone();
        host.validators.clear();
        assert!(!validators.is_empty());
        {
            assert!(host
                .start_managed_api("deputy-second", "rejected-context")
                .is_err());
            assert!(!host
                .api_startups
                .contains_key(&("project".into(), "rejected-context".into())));
        }
        host.validators = validators;
        host.start_managed_api("deputy-second", "second-attempt")
            .unwrap();
        wait_for(&mut host, |_| second_waiting.try_recv().is_ok());
        assert_eq!(host.api_active.len(), 2);
        assert!(host
            .start_managed_api("deputy-second", "third-attempt")
            .unwrap_err()
            .contains("并发上限为2"));
        assert!(!host
            .api_startups
            .contains_key(&("project".into(), "third-attempt".into())));

        assert_eq!(host.api_writers.len(), 1);
        assert!(!host.projects.contains_key("project"));
        let expected_sequence = host.api_writers["project"]
            .owner
            .snapshot()
            .unwrap()
            .sequence;
        let (fake_sender, fake_receiver) = mpsc::channel();
        let original = std::mem::replace(
            &mut host.api_active.get_mut(&first_key).unwrap().receiver,
            fake_receiver,
        );
        fake_sender
            .send(crate::async_api::Update::Progress(Box::new(stale)))
            .unwrap();
        host.poll_api();
        host.api_active.get_mut(&first_key).unwrap().receiver = original;
        drop(fake_sender);
        let snapshot = host.snapshot();
        let workflows: Vec<_> = snapshot
            .workflows
            .iter()
            .filter(|w| w.project.id == "project")
            .collect();
        assert_eq!(workflows.len(), 1);
        assert_eq!(workflows[0].sequence, expected_sequence);
        assert_eq!(
            workflows[0].state.tasks["task"].state,
            TaskState::Dispatching
        );
        assert_eq!(
            workflows[0].state.tasks["second"].state,
            TaskState::Dispatching
        );
        assert!(host
            .cancel_api("project", "missing-attempt", "wrong attempt")
            .is_err());
        assert!(host.api_active[&second_key].cancel.is_some());
        if cancel_first {
            host.cancel_api("project", "attempt", "cancel only first")
                .unwrap();
        }
        first_release.send(()).unwrap();
        wait_for(&mut host, |h| !h.api_active.contains_key(&first_key));
        assert_eq!(host.api_writers.len(), 1);
        assert!(!host.projects.contains_key("project"));
        assert_eq!(
            host.api_jobs[&first_key].state,
            if cancel_first {
                ApiJobState::Unknown
            } else {
                ApiJobState::Submitted
            }
        );
        assert_eq!(
            host.api_active[&second_key].job.state,
            ApiJobState::Executing
        );
        assert!(host
            .require_provider_idle(CredentialProvider::DeepSeek)
            .is_err());
        second_release.send(()).unwrap();
        drain(&mut host);
        assert!(host.api_writers.is_empty());
        assert!(host.api_fault.is_none(), "{:?}", host.api_fault);
        assert_eq!(host.api_jobs[&second_key].state, ApiJobState::Submitted);
        let store = &host.projects["project"];
        assert_eq!(store.state().tasks["second"].state, TaskState::Submitted);
        let plan = store.revision().unwrap();
        let second_path =
            Path::new(&plan.execution_root("second", "second-attempt").unwrap()).join("result.txt");
        assert_eq!(
            fs::read_to_string(second_path).unwrap(),
            "background fixture result"
        );
        let first_path =
            Path::new(&plan.execution_root("task", "attempt").unwrap()).join("result.txt");
        assert_eq!(first_path.exists(), !cancel_first);
        let history = store.history().to_vec();
        assert_eq!(
            history
                .iter()
                .filter(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. }))
                .count(),
            2
        );
        drop(host);
        let host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(host.projects["project"].history(), history);
        assert!(host.api_active.is_empty() && host.api_writers.is_empty());
        println!("shared registry cancelFirst={cancel_first}: two worker threads, one writer, two dispatches, independent completion, stale progress ignored, reopen events 0; public API admission passed; runner remains serial");
    }
}

#[test]
#[ignore = "explicit real Git; interleaved original context and delayed Prepare ACK"]
fn public_assignment_preserves_original_prefix_across_other_task_completion() {
    let parent =
        fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports")).unwrap();
    let root = tempfile::Builder::new()
        .prefix("api-parallel-gap-")
        .tempdir_in(parent)
        .unwrap();
    let mut host = parallel_fixture(root.path());
    let first_model = host.projects["project"]
        .revision()
        .unwrap()
        .task("task")
        .unwrap()
        .api_worker
        .as_ref()
        .unwrap()
        .model
        .clone();
    let (waiting, release) = gate(&first_model);
    host.api_fixture = Some(gated);
    host.start_managed_api("deputy-api", "attempt").unwrap();
    wait_for(&mut host, |_| waiting.try_recv().is_ok());
    prepare_second(&mut host);
    let original: ExecutionSnapshot = serde_json::from_value(
        host.management.intent("deputy-second").unwrap().context["workflow"].clone(),
    )
    .unwrap();
    host.start_managed_api("deputy-second", "second-attempt")
        .unwrap();
    let key = ("project".into(), "second-attempt".into());
    // Allow workspace proof ACK only; capture Prepare before the other task finishes.
    wait_for(&mut host, |h| {
        h.api_startups[&key].attempt.workspace_proof.is_some()
    });
    let update = host.api_active[&key]
        .receiver
        .recv_timeout(Duration::from_secs(15))
        .unwrap();
    let crate::async_api::Update::Assignment(crate::managed_assignment::Update::Prepare {
        operation,
        attempt,
        snapshot,
        reply,
    }) = update
    else {
        panic!("expected Prepare");
    };
    assert_eq!(snapshot.sequence, original.sequence);
    release.send(()).unwrap();
    let first_key = ("project".into(), "attempt".into());
    wait_for(&mut host, |h| !h.api_active.contains_key(&first_key));
    assert_eq!(host.api_jobs[&first_key].state, ApiJobState::Submitted);
    assert!(
        host.api_writers["project"]
            .owner
            .snapshot()
            .unwrap()
            .sequence
            > original.sequence
    );
    {
        let reader = host
            .api_read("project", "second", "second-attempt")
            .unwrap();
        let mut forged = original.clone();
        forged.sequence += 1;
        assert!(api_assignment_scope::unchanged_target(
            &*reader,
            &forged,
            "second",
            reader.history().len() as u32
        )
        .is_err());
        assert!(host
            .validate_api_assignment_scope(
                "deputy-second",
                &*reader,
                "task",
                reader.history().len() as u32,
                true
            )
            .is_err());
    }
    let result =
        host.prepare_managed_assignment("project", &operation, *attempt, *snapshot, "deepseek");
    assert!(result.is_ok(), "{result:?}");
    reply.send(result).unwrap();
    drain(&mut host);
    assert_eq!(host.api_jobs[&key].state, ApiJobState::Submitted);
    let startup = &host.api_startups[&key];
    assert_eq!(startup.version, 2);
    assert_eq!(startup.workflow_sequence, original.sequence);
    let store = &host.projects["project"];
    let entry = store
        .history()
        .iter()
        .find(|e| e.body.operation_id == "mgmt-deputy-second")
        .unwrap();
    assert!(entry.body.event.sequence > startup.workflow_sequence + 1);
    assert!(host
        .validate_api_assignment_scope(
            "deputy-second",
            store,
            "second",
            store.history().len() as u32,
            true
        )
        .is_err());
    let mut nonisolated = store.revision().unwrap().clone();
    nonisolated
        .workflow
        .tasks
        .iter_mut()
        .find(|t| t.id == "second")
        .unwrap()
        .workspace_key = "project".into();
    assert!(startup.validate_workspace(&nonisolated).is_err());
    let history = store.history().to_vec();
    drop(host);
    let host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(host.projects["project"].history(), history);
    println!("public prefix gap: original management retained, delayed Prepare accepted, forged/changed target rejected, version2 independent-worktree enforced, reopen events 0");
}
