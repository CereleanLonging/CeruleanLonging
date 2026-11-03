use super::*;
use cerulean_domain::{
    api_jobs::ApiJobState, api_worker::ApiTarget, project_setup::ProjectWorkerAdapter,
};
use std::time::Duration;
fn fixture(root: &Path) -> RuntimeHost {
    api_host_tests::fixture(
        root,
        ProjectWorkerAdapter::Deepseek,
        ApiTarget::Provider {
            endpoint: credentials::ProviderEndpoint::DeepSeek {},
        },
        true,
    )
}
fn control(host: &mut RuntimeHost, command: Command) -> Response {
    host.handle(Request {
        id: "api-control".into(),
        protocol_version: PROTOCOL_VERSION,
        command,
    })
}
fn retire(host: &mut RuntimeHost, hash: &str) -> Response {
    control(
        host,
        Command::RetireUnknownApi {
            project_id: "project".into(),
            attempt_id: "attempt".into(),
            evidence_digest: hash.into(),
            reason: "保留原修改，修订后继续；远端结果与费用未确认".into(),
        },
    )
}
fn unknown_after_write(root: &Path) -> RuntimeHost {
    let mut host = fixture(root);
    host.api_fixture = Some(|p, r| {
        if r == 1 {
            api_host_tests::reply(p, r)
        } else {
            Err(cerulean_adapters::api_generation::ApiFailure::Http { status: 503 })
        }
    });
    host.start_managed_api("deputy-api", "attempt").unwrap();
    api_host_tests::drain(&mut host);
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Unknown
    );
    host
}
#[test]
fn api_unknown_retirement_preserves_files_budget_identity_and_survives_replanning() {
    let root = tempfile::tempdir().unwrap();
    let mut host = unknown_after_write(root.path());
    let content =
        fs::read(project_planning::tests::worker_root(root.path()).join("result.txt")).unwrap();
    let reserved = crate::api_execution::prior_requests(&host.projects["project"]).unwrap();
    let original = host.projects["project"]
        .history()
        .iter()
        .map(|e| e.hash.clone())
        .collect::<Vec<_>>();
    let binding = host.projects["project"]
        .attempt_record("task", "attempt")
        .unwrap()
        .clone();
    let hash = host.snapshot().api_jobs[0]
        .retirement_digest
        .clone()
        .unwrap();
    assert!(retire(&mut host, &"0".repeat(64)).error.is_some());
    let cmd = WorkflowCommand::RetireUnknownApi {
        task: "task".into(),
        attempt: "attempt".into(),
        reason: "test".into(),
    };
    for principal in ["core", "deputy", "chief", "worker-deepseek"] {
        assert!(host
            .projects
            .get_mut("project")
            .unwrap()
            .command(
                "forbidden",
                principal,
                cmd.clone(),
                CommandContext::default(),
                false
            )
            .is_err());
    }
    assert_eq!(host.projects["project"].history().len(), original.len());
    let response = retire(&mut host, &hash);
    assert!(response.error.is_none(), "{:?}", response.error);
    assert!(retire(&mut host, &hash).error.is_some());
    let store = &host.projects["project"];
    assert_eq!(store.state().tasks["task"].state, TaskState::Retired);
    assert!(!store.state().tasks["task"].verified);
    assert_eq!(store.attempt_record("task", "attempt"), Some(&binding));
    assert_eq!(
        store.history()[..original.len()]
            .iter()
            .map(|e| e.hash.clone())
            .collect::<Vec<_>>(),
        original
    );
    assert_eq!(
        crate::api_execution::prior_requests(store).unwrap(),
        reserved
    );
    assert!(host.recover_api("project", "attempt").is_err());
    drop(host);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(host.snapshot().api_jobs[0].state, ApiJobState::Retired);
    assert!(host.require_available("project").is_ok());
    let review = host
        .snapshot()
        .project_revisions
        .into_iter()
        .find(|r| r.project_id == "project")
        .unwrap();
    assert_eq!(review.retired_tasks, 1);
    assert_eq!(review.cancelled_tasks, 0);
    host.request_project_revision("project", &review.evidence_digest, "重新规划未完成任务")
        .unwrap();
    let mut proposal = project_planning::tests::proposal("tests");
    if let cerulean_domain::management::ManagementDecision::Plan { tasks, .. } = &mut proposal {
        tasks[0].worker_id = "worker-deepseek".into();
    }
    project_planning::tests::complete_at(&mut host, proposal, "ui-plan-retired-new-plan");
    let store = &host.projects["project"];
    assert_eq!(store.revision().unwrap().workflow.version, 2);
    assert!(!store.state().approved);
    assert_eq!(store.state().tasks["task"].state, TaskState::Pending);
    assert_eq!(store.state().tasks["task"].fence, binding.fence);
    assert_eq!(
        crate::api_execution::prior_requests(store).unwrap(),
        reserved
    );
    assert_eq!(
        fs::read(project_planning::tests::worker_root(root.path()).join("result.txt")).unwrap(),
        content
    );
    assert!(host.api_active.is_empty());
    drop(host);
    let host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(host.snapshot().api_jobs[0].state, ApiJobState::Retired);
    assert_eq!(
        crate::api_execution::prior_requests(&host.projects["project"]).unwrap(),
        reserved
    );
}
#[test]
fn api_unknown_retirement_rejects_stale_identity_completed_evidence_and_late_resurrection() {
    let root = tempfile::tempdir().unwrap();
    let mut host = unknown_after_write(root.path());
    let old = host.snapshot().api_jobs[0]
        .retirement_digest
        .clone()
        .unwrap();
    host.projects
        .get_mut("project")
        .unwrap()
        .command(
            "new-advice",
            "deputy",
            WorkflowCommand::Advise {
                task: "task".into(),
                suggestion: "review changed".into(),
            },
            CommandContext::default(),
            false,
        )
        .unwrap();
    assert!(retire(&mut host, &old).error.is_some());
    let hash = host.snapshot().api_jobs[0]
        .retirement_digest
        .clone()
        .unwrap();
    let path = host.projects["project"]
        .api_attempt_path("attempt")
        .unwrap();
    let original = fs::read(path.join("identity.json")).unwrap();
    fs::write(path.join("identity.json"), b"{}").unwrap();
    assert!(retire(&mut host, &hash).error.is_some());
    fs::write(path.join("identity.json"), original).unwrap();
    fs::write(path.join("completion.json"), b"{}").unwrap();
    assert!(retire(&mut host, &hash).error.is_some());
    fs::remove_file(path.join("completion.json")).unwrap();
    assert!(retire(&mut host, &hash).error.is_none());
    let reserved = crate::api_execution::prior_requests(&host.projects["project"]).unwrap();
    fs::write(path.join("completion.json"), b"late untrusted completion").unwrap();
    assert!(host.recover_api("project", "attempt").is_err());
    assert_eq!(
        crate::api_execution::prior_requests(&host.projects["project"]).unwrap(),
        reserved
    );
    drop(host);
    let host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(host.snapshot().api_jobs[0].state, ApiJobState::Retired);
}
#[test]
fn api_retirement_rejects_active_and_successfully_submitted_attempts() {
    let root = tempfile::tempdir().unwrap();
    let mut host = fixture(root.path());
    host.start_managed_api("deputy-api", "attempt").unwrap();
    assert!(host.snapshot().api_jobs[0].retirement_digest.is_none());
    assert!(retire(&mut host, &"a".repeat(64)).error.is_some());
    assert!(host.has_api_activity("project"));
    api_host_tests::drain(&mut host);
    assert_eq!(host.snapshot().api_jobs[0].state, ApiJobState::Submitted);
    assert!(host.snapshot().api_jobs[0].retirement_digest.is_none());
    assert!(retire(&mut host, &"a".repeat(64)).error.is_some());
}
#[test]
fn api_user_stop_then_abandon_unsent_preserves_attempt_and_reopens_pending() {
    let root = tempfile::tempdir().unwrap();
    let mut host = fixture(root.path());
    host.start_managed_api("deputy-api", "attempt").unwrap();
    let response = control(
        &mut host,
        Command::CancelApi {
            project_id: "project".into(),
            attempt_id: "wrong-attempt".into(),
            reason: "stop".into(),
        },
    );
    assert!(response.error.is_some());
    let response = control(
        &mut host,
        Command::CancelApi {
            project_id: "project".into(),
            attempt_id: "attempt".into(),
            reason: "stop before send".into(),
        },
    );
    assert!(response.error.is_none(), "{:?}", response.error);
    api_host_tests::drain(&mut host);
    let review = host
        .snapshot()
        .unsent_assignments
        .into_iter()
        .find(|r| r.attempt_id == "attempt")
        .unwrap();
    let history = host.projects["project"].history().len();
    assert!(control(
        &mut host,
        Command::AbandonUnsentAttempt {
            project_id: "project".into(),
            task_id: "task".into(),
            attempt_id: "attempt".into(),
            evidence_digest: "0".repeat(64),
            reason: "release".into()
        }
    )
    .error
    .is_some());
    assert_eq!(host.projects["project"].history().len(), history);
    let response = control(
        &mut host,
        Command::AbandonUnsentAttempt {
            project_id: review.project_id,
            task_id: review.task_id,
            attempt_id: review.attempt_id,
            evidence_digest: review.evidence_digest,
            reason: "retry only after explicit new dispatch".into(),
        },
    );
    assert!(response.error.is_none(), "{:?}", response.error);
    assert_eq!(host.snapshot().api_jobs[0].state, ApiJobState::Abandoned);
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Pending
    );
    assert!(host.require_available("project").is_ok());
    drop(host);
    let host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(host.snapshot().api_jobs[0].state, ApiJobState::Abandoned);
    assert!(host.projects["project"]
        .attempt_record("task", "attempt")
        .is_some());
    assert!(host.api_active.is_empty());
}
#[test]
fn api_unassigned_startup_user_closure_binds_review_and_management_response() {
    let root = tempfile::tempdir().unwrap();
    let mut host = fixture(root.path());
    host.start_managed_api("deputy-api", "attempt").unwrap();
    let update = host.api_active[&("project".into(), "attempt".into())]
        .receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    let crate::async_api::Update::Assignment(crate::managed_assignment::Update::Prepare {
        reply,
        ..
    }) = update
    else {
        panic!("expected prepare")
    };
    reply.send(Err("preparation not recorded".into())).unwrap();
    api_host_tests::drain(&mut host);
    let review = host.snapshot().api_jobs[0].closure_digest.clone().unwrap();
    let wrong = control(
        &mut host,
        Command::AbandonApiStartup {
            project_id: "project".into(),
            attempt_id: "attempt".into(),
            evidence_digest: "0".repeat(64),
            reason: "close".into(),
        },
    );
    assert!(wrong.error.is_some());
    let response = control(
        &mut host,
        Command::AbandonApiStartup {
            project_id: "project".into(),
            attempt_id: "attempt".into(),
            evidence_digest: review,
            reason: "close unassigned startup".into(),
        },
    );
    assert!(response.error.is_none(), "{:?}", response.error);
    assert_eq!(host.snapshot().api_jobs[0].state, ApiJobState::Abandoned);
    assert!(host.management.startup_closure("deputy-api").is_some());
    assert!(host.start_managed_api("deputy-api", "another").is_err());
    drop(host);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    host.poll_verifications();
    assert_eq!(host.snapshot().api_jobs[0].state, ApiJobState::Abandoned);
    assert!(host.api_active.is_empty());
    assert!(host.require_available("project").is_ok());
    assert!(host.projects["project"]
        .attempt_record("task", "attempt")
        .is_none());
    assert!(!project_planning::tests::worker_root(root.path())
        .join("result.txt")
        .exists());
}
#[test]
fn api_prepared_application_can_close_without_assigning_or_calling_provider() {
    let root = tempfile::tempdir().unwrap();
    let mut host = fixture(root.path());
    host.start_managed_api("deputy-api", "attempt").unwrap();
    let update = host.api_active[&("project".into(), "attempt".into())]
        .receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    let crate::async_api::Update::Assignment(crate::managed_assignment::Update::Prepare {
        operation,
        attempt,
        snapshot,
        reply,
    }) = update
    else {
        panic!("expected prepare")
    };
    host.prepare_managed_assignment("project", &operation, *attempt, *snapshot, "deepseek")
        .unwrap();
    reply
        .send(Err("simulated acknowledgement loss".into()))
        .unwrap();
    api_host_tests::drain(&mut host);
    assert!(host.snapshot().api_jobs[0].closure_digest.is_none());
    let review = host.application_recovery_review("deputy-api").unwrap();
    let response = control(
        &mut host,
        Command::AbandonManagementApplication {
            operation_id: "deputy-api".into(),
            evidence_digest: review.evidence_digest,
            reason: "close unapplied preparation".into(),
        },
    );
    assert!(response.error.is_none(), "{:?}", response.error);
    assert_eq!(host.snapshot().api_jobs[0].state, ApiJobState::Abandoned);
    assert!(host.require_available("project").is_ok());
    drop(host);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    host.poll_verifications();
    assert_eq!(host.snapshot().api_jobs[0].state, ApiJobState::Abandoned);
    assert!(host.projects["project"]
        .attempt_record("task", "attempt")
        .is_none());
}
#[test]
fn api_original_result_check_never_reissues_unknown_provider_request() {
    let root = tempfile::tempdir().unwrap();
    let mut host = fixture(root.path());
    host.api_fixture =
        Some(|_, _| Err(cerulean_adapters::api_generation::ApiFailure::Http { status: 401 }));
    host.start_managed_api("deputy-api", "attempt").unwrap();
    api_host_tests::drain(&mut host);
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Unknown
    );
    let history = host.projects["project"].history().len();
    let response = control(
        &mut host,
        Command::RecoverApi {
            project_id: "project".into(),
            attempt_id: "attempt".into(),
        },
    );
    assert!(response.error.is_some());
    assert_eq!(host.projects["project"].history().len(), history);
    assert!(host.api_active.is_empty());
    assert!(host.snapshot().api_jobs[0].closure_digest.is_none());
    assert!(host.snapshot().unsent_assignments.is_empty());
    assert!(!project_planning::tests::worker_root(root.path())
        .join("result.txt")
        .exists());
}
