use super::*;
use crate::scheduler::Entry;
use crate::{
    workflow_access::WorkflowAccess,
    workflow_coordinator::{Scope, WorkflowClient, WorkflowCoordinator},
};
use std::sync::{Arc, Barrier};
#[path = "coordinated_codex_tests.rs"]
mod codex;

fn fixture(root: &Path) -> (RuntimeHost, WorkflowCoordinator, Scope, Attempt) {
    let (mut host, draft) = project_planning::tests::fixture(root);
    host.configure_project(draft).unwrap();
    project_planning::tests::complete(&mut host, project_planning::tests::proposal("tests"));
    let plan = host.projects["project"].revision().unwrap().clone();
    let revision = plan.approval_digest().unwrap();
    host.approve("project", &revision).unwrap();
    let attempt = Attempt {
        workspace_proof: None,
        id: "writer-attempt".into(),
        task_id: "task".into(),
        worker_id: "worker-codex".into(),
        revision_digest: revision.clone(),
        fence: 1,
        binding: SessionBinding {
            adapter_id: "codex".into(),
            adapter_version: "fixture".into(),
            session_id: "fixture-session".into(),
            identity_strength: IdentityStrength::ProviderId,
            workspace: plan.project.root,
        },
    };
    let scope = Scope {
        project: "project".into(),
        task: "task".into(),
        attempt: attempt.id.clone(),
        revision,
    };
    let store = host.projects.remove("project").unwrap();
    let owner = WorkflowCoordinator::start(store, root.join("data/workflows/project")).unwrap();
    (host, owner, scope, attempt)
}
fn assign(client: &mut WorkflowClient, attempt: Attempt) -> Result<Entry, String> {
    client.command(
        "coordinator-assignment",
        "deputy",
        WorkflowCommand::Assign {
            task: attempt.task_id.clone(),
            worker: attempt.worker_id.clone(),
            attempt: attempt.id.clone(),
        },
        CommandContext {
            execution: Some(ExecutionEvidence::Attempt { attempt }),
            ..Default::default()
        },
        true,
    )
}

#[test]
fn simultaneous_clients_share_one_writer_and_identical_operation_has_one_event() {
    let root = tempfile::tempdir().unwrap();
    let (mut host, mut owner, scope, attempt) = fixture(root.path());
    let before = owner.snapshot().unwrap();
    let first = owner.client(scope.clone()).unwrap();
    let second = owner.client(scope).unwrap();
    assert!(owner.finish(false).err().unwrap().contains("仍有尝试"));
    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = [first, second]
        .into_iter()
        .map(|mut client| {
            let barrier = barrier.clone();
            let attempt = attempt.clone();
            std::thread::spawn(move || {
                barrier.wait();
                let entry = assign(&mut client, attempt);
                (entry, client.history().to_vec())
            })
        })
        .collect();
    barrier.wait();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.0.is_ok()).count(), 1);
    assert!(results
        .iter()
        .find_map(|r| r.0.as_ref().err())
        .unwrap()
        .contains("intent already recorded"));
    let accepted = &results.iter().find(|r| r.0.is_ok()).unwrap().1;
    assert_eq!(owner.snapshot().unwrap().sequence, before.sequence + 1);
    let store = owner.finish(false).unwrap();
    assert_eq!(store.history(), accepted);
    assert_eq!(store.state().tasks["task"].state, TaskState::Assigned);
    host.projects.insert("project".into(), store);
    drop(host);
    let host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(host.projects["project"].history(), accepted);
}

#[test]
fn scoped_client_rejects_wrong_revision_and_other_attempt_without_publication() {
    let root = tempfile::tempdir().unwrap();
    let (_host, mut owner, scope, mut attempt) = fixture(root.path());
    let before = owner.snapshot().unwrap().sequence;
    let mut wrong = scope.clone();
    wrong.revision = "f".repeat(64);
    assert!(owner.client(wrong).is_err());
    let mut client = owner.client(scope).unwrap();
    attempt.id = "foreign-attempt".into();
    assert!(assign(&mut client, attempt)
        .unwrap_err()
        .contains("跨任务或跨尝试"));
    assert!(client.refresh().unwrap_err().contains("失去确认"));
    assert_eq!(owner.snapshot().unwrap().sequence, before);
    drop(client);
    let store = owner.finish(false).unwrap();
    assert_eq!(store.history().len(), before as usize);
}

#[test]
fn lost_writer_ack_preserves_published_assignment_and_never_retries() {
    let root = tempfile::tempdir().unwrap();
    let (_host, mut owner, scope, attempt) = fixture(root.path());
    let before = owner.snapshot().unwrap().sequence;
    let mut client = owner.client(scope).unwrap();
    owner.lose_next_acknowledgement();
    assert!(assign(&mut client, attempt.clone())
        .unwrap_err()
        .contains("回执丢失"));
    assert!(assign(&mut client, attempt)
        .unwrap_err()
        .contains("失去确认"));
    assert_eq!(owner.snapshot().unwrap().sequence, before + 1);
    assert_eq!(
        owner.snapshot().unwrap().state.tasks["task"].state,
        TaskState::Assigned
    );
    drop(client);
    let store = owner.finish(true).unwrap();
    assert_eq!(store.history().len(), before as usize + 1);
    assert_eq!(store.state().tasks["task"].state, TaskState::Assigned);
}

#[test]
fn interrupted_writer_refuses_further_commands_and_reopens_original_event() {
    let root = tempfile::tempdir().unwrap();
    let (_host, mut owner, scope, attempt) = fixture(root.path());
    let before = owner.snapshot().unwrap().sequence;
    let mut client = owner.client(scope).unwrap();
    owner.interrupt_next_publication();
    assert!(assign(&mut client, attempt.clone())
        .unwrap_err()
        .contains("写入中断"));
    assert!(owner.snapshot().is_err());
    assert!(assign(&mut client, attempt).is_err());
    drop(client);
    let store = owner.finish(false).unwrap();
    assert_eq!(store.history().len(), before as usize + 1);
    assert_eq!(store.state().tasks["task"].state, TaskState::Assigned);
}

type ReplyGate = (std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>);
static REPLY_GATE: std::sync::Mutex<Option<ReplyGate>> = std::sync::Mutex::new(None);
fn paused_reply(
    prepared: &cerulean_adapters::api_generation::PreparedRequest,
    round: u32,
) -> Result<
    cerulean_adapters::api_generation::ApiReply,
    cerulean_adapters::api_generation::ApiFailure,
> {
    let gate = REPLY_GATE.lock().unwrap().take();
    if let Some((entered, release)) = gate {
        entered.send(()).unwrap();
        release
            .recv_timeout(std::time::Duration::from_secs(10))
            .unwrap();
    }
    api_host_tests::reply(prepared, round)
}

#[test]
fn api_waiting_for_provider_does_not_hold_the_journal_writer() {
    use cerulean_domain::{
        api_worker::ApiTarget, credentials::ProviderEndpoint, project_setup::ProjectWorkerAdapter,
    };
    let root = tempfile::tempdir().unwrap();
    let mut host = api_host_tests::fixture(
        root.path(),
        ProjectWorkerAdapter::Deepseek,
        ApiTarget::Provider {
            endpoint: ProviderEndpoint::DeepSeek {},
        },
        true,
    );
    let (entered, waiting) = std::sync::mpsc::channel();
    let (release, resume) = std::sync::mpsc::channel();
    *REPLY_GATE.lock().unwrap() = Some((entered, resume));
    host.api_fixture = Some(paused_reply);
    host.start_managed_api("deputy-api", "attempt").unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        host.poll_verifications();
        if waiting.try_recv().is_ok() {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    // The model response is still withheld. The real writer can answer independently.
    let state = host.api_writers["project"].owner.snapshot().unwrap();
    assert_eq!(state.state.tasks["task"].state, TaskState::Dispatching);
    let worker_root = project_planning::tests::worker_root(root.path());
    assert!(!worker_root.join("result.txt").exists());
    assert_eq!(
        host.snapshot().workflows[0].state.tasks["task"].state,
        TaskState::Dispatching
    );
    release.send(()).unwrap();
    api_host_tests::drain(&mut host);
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Submitted
    );
    assert_eq!(
        fs::read_to_string(worker_root.join("result.txt")).unwrap(),
        "background fixture result"
    );
    assert!(!root.path().join("project/result.txt").exists());
    let history = host.projects["project"].history().to_vec();
    drop(host);
    let host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(host.projects["project"].history(), history);
}
