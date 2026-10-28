use super::*;
use crate::management_execution::{ApplicationTarget, DecisionState};
use cerulean_domain::management::{ManagementDecision, ProposedTask};

fn setup(root: &Path) -> RuntimeHost {
    let (mut host, mut draft) = super::super::project_planning::tests::fixture(root);
    draft.management_max_requests = 4;
    host.configure_project(draft).unwrap();
    drop(host);
    crate::management_source_guard::tests::promote_fixture(&root.join("data"));
    RuntimeHost::open(&root.join("data")).unwrap()
}
fn prepare(host: &mut RuntimeHost) -> ManagementHandoff {
    let digest = host.project_setups["project"].digest.clone();
    host.prepare_external_chief_plan("project", &digest, "current-session")
        .unwrap()
}
fn response(request: &ManagementHandoff) -> ExternalChiefDecision {
    ExternalChiefDecision {
        request_digest: digest(request).unwrap(),
        session_id: "current-session".into(),
        envelope: DecisionEnvelope {
            state_digest: request.state_digest.clone(),
            decision: ManagementDecision::Plan {
                tasks: vec![ProposedTask {
                    profile_id: None,
                    id: "sum".into(),
                    goal: "Implement a sum function".into(),
                    dependencies: vec![],
                    worker_id: "worker-codex".into(),
                    validator_id: "tests".into(),
                }],
                rationale: "one bounded task".into(),
            },
        },
    }
}
#[test]
fn external_chief_wait_survives_reopen_and_plan_never_approves_itself() {
    let root = tempfile::tempdir().unwrap();
    let mut host = setup(root.path());
    assert!(!host.openai_connection.key_configured);
    let request = prepare(&mut host);
    assert_eq!(
        host.management_decisions()[0].state,
        DecisionState::AwaitingExternal
    );
    assert!(host.management_active.is_empty());
    drop(host);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(
        host.management_decisions()[0].state,
        DecisionState::AwaitingExternal
    );
    assert_eq!(
        digest(
            &host
                .read_external_chief_request(&request.operation_id)
                .unwrap()
        )
        .unwrap(),
        digest(&request).unwrap()
    );
    let config = host.project_setups["project"].digest.clone();
    assert!(host
        .prepare_external_chief_plan("project", &config, "current-session")
        .is_err());
    host.submit_external_chief_decision(
        &request.operation_id,
        "current-session",
        response(&request),
    )
    .unwrap();
    assert_eq!(host.management_decisions()[0].state, DecisionState::Applied);
    assert!(host.projects["project"].revision().is_some());
    assert!(host.projects["project"].approval().is_none());
    assert!(host.management.observation(&request.operation_id).is_err());
    assert!(host
        .management
        .billing_evidence(&request.operation_id)
        .is_err());
    assert!(host.management.currency_history_gaps().is_empty());
    assert!(host.management_active.is_empty());
    assert!(host
        .submit_external_chief_decision(
            &request.operation_id,
            "current-session",
            response(&request)
        )
        .is_err());
    let history_len = host.projects["project"].history().len();
    drop(host);
    let host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(host.management_decisions()[0].state, DecisionState::Applied);
    assert_eq!(host.projects["project"].history().len(), history_len);
    assert!(host.projects["project"].approval().is_none());
}
#[test]
fn external_chief_rejects_session_request_role_and_changed_state_before_publication() {
    let root = tempfile::tempdir().unwrap();
    let mut host = setup(root.path());
    let request = prepare(&mut host);
    assert!(host
        .submit_external_chief_decision(&request.operation_id, "other-session", response(&request))
        .is_err());
    let mut wrong = response(&request);
    wrong.request_digest = "a".repeat(64);
    assert!(host
        .submit_external_chief_decision(&request.operation_id, "current-session", wrong)
        .is_err());
    let mut wrong = response(&request);
    wrong.envelope.decision = ManagementDecision::Assign {
        task_id: "sum".into(),
        worker_id: "worker-codex".into(),
        rationale: "wrong role".into(),
    };
    assert!(host
        .submit_external_chief_decision(&request.operation_id, "current-session", wrong)
        .is_err());
    host.register_validator(
        "extra".into(),
        "changes context".into(),
        std::env::current_exe().unwrap().to_string_lossy().into(),
        vec![],
        Default::default(),
    )
    .unwrap();
    assert!(host
        .submit_external_chief_decision(
            &request.operation_id,
            "current-session",
            response(&request)
        )
        .is_err());
    assert!(!root
        .path()
        .join("data/management-decisions")
        .join(format!("{}.1.json", request.operation_id))
        .exists());
    assert_eq!(
        host.management_decisions()[0].state,
        DecisionState::AwaitingExternal
    );
}
#[test]
fn external_chief_wait_reply_and_discard_use_existing_resolution_transaction() {
    let root = tempfile::tempdir().unwrap();
    let mut host = setup(root.path());
    let request = prepare(&mut host);
    let mut answer = response(&request);
    answer.envelope.decision = ManagementDecision::Wait {
        reason: "which scope?".into(),
    };
    host.submit_external_chief_decision(&request.operation_id, "current-session", answer)
        .unwrap();
    assert_eq!(
        host.management
            .review(&request.operation_id)
            .unwrap()
            .1
            .as_deref(),
        Some("which scope?")
    );
    let head = host
        .management
        .evidence_digest(&request.operation_id)
        .unwrap()
        .to_string();
    host.resolve_management(
        &request.operation_id,
        &head,
        crate::management_execution::UserResolution::Reply {
            answer: "local-only".into(),
        },
    )
    .unwrap();
    assert_eq!(host.management.user_replies("project", 32768).len(), 1);
    let second = prepare(&mut host);
    let head = host
        .management
        .evidence_digest(&second.operation_id)
        .unwrap()
        .to_string();
    host.resolve_management(
        &second.operation_id,
        &head,
        crate::management_execution::UserResolution::Discard {
            reason: "user closes pending request".into(),
        },
    )
    .unwrap();
    assert!(host
        .submit_external_chief_decision(&second.operation_id, "current-session", response(&second))
        .is_err());
    drop(host);
    let host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(host.management.user_replies("project", 32768).len(), 1);
}
#[test]
fn external_chief_synced_pending_response_recovers_once_without_dispatch() {
    let root = tempfile::tempdir().unwrap();
    let mut host = setup(root.path());
    let request = prepare(&mut host);
    // Persist a valid response without applying it, then emulate sync-before-publish.
    host.management
        .finish_external(
            &request.operation_id,
            response(&request),
            &request.context,
            "current-session",
        )
        .unwrap();
    drop(host);
    let path = root
        .path()
        .join("data/management-decisions")
        .join(format!("{}.1.json", request.operation_id));
    let pending = path.with_extension("pending");
    fs::copy(&path, &pending).unwrap();
    fs::remove_file(&path).unwrap();
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(host.management_decisions()[0].state, DecisionState::Ready);
    assert!(host.projects["project"].revision().is_none());
    assert!(path.exists() && !pending.exists());
    host.poll_planning_results();
    assert_eq!(host.management_decisions()[0].state, DecisionState::Applied);
    assert!(host.projects["project"].approval().is_none());
    assert!(host.management_active.is_empty());
}
#[test]
fn external_chief_has_versioned_evidence_and_cannot_use_api_or_approval_receipts() {
    let root = tempfile::tempdir().unwrap();
    let mut host = setup(root.path());
    let request = prepare(&mut host);
    assert!(host
        .management
        .finish(&request.operation_id, Err("fake API failure".into()))
        .is_err());
    let response = response(&request);
    host.management
        .finish_external(
            &request.operation_id,
            response,
            &request.context,
            "current-session",
        )
        .unwrap();
    let target = ApplicationTarget::Workflow {
        principal: "user".into(),
        command: WorkflowCommand::Approve {
            digest: request.state_digest.clone(),
        },
        context: Box::default(),
    };
    assert!(host
        .apply_management_decision(&request.operation_id, target)
        .is_err());
    let path = root
        .path()
        .join("data/management-decisions")
        .join(format!("{}.0.json", request.operation_id));
    let value: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(value["body"]["version"], 2);
    assert_eq!(value["body"]["evidence"]["kind"], "externalIntent");
    assert!(value.to_string().find("inputTokens").is_none());
}

#[test]
fn external_chief_partial_response_requires_review_and_never_becomes_api_debt_or_retry() {
    let root = tempfile::tempdir().unwrap();
    let mut host = setup(root.path());
    let request = prepare(&mut host);
    drop(host);
    let data = root.path().join("data");
    let relative = format!("management-decisions/{}.1.pending", request.operation_id);
    fs::write(data.join(&relative), b"{").unwrap();
    assert!(RuntimeHost::open(&data).is_err());
    let (head, reviewed) = crate::management_review::inspect(&data, &relative).unwrap();
    assert!(!reviewed);
    crate::management_review::acknowledge(
        &data,
        &relative,
        &head,
        "retain incomplete external reply",
        |_| Ok(()),
    )
    .unwrap();
    let mut host = RuntimeHost::open(&data).unwrap();
    assert_eq!(host.management_decisions()[0].state, DecisionState::Unknown);
    assert!(host.management.currency_history_gaps().is_empty());
    assert!(host
        .submit_external_chief_decision(
            &request.operation_id,
            "current-session",
            response(&request)
        )
        .is_err());
    assert!(data.join(relative).exists());
    assert!(host.management_active.is_empty());
}
