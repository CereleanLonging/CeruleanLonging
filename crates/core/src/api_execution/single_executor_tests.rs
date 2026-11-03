//! Same production API/file/verifier loop, with local identities and no
//! fabricated commander decisions. Provider responses remain synthetic.
use super::*;

fn config(plan: &PlanRevision) -> SchedulerConfig {
    SchedulerConfig::single_executor(ExecutionScope {
        project: plan.project.clone(),
        policy_version: plan.policy.version,
    })
}
fn proposed(root: &Path, p: &PlanRevision) -> DurableWorkflow {
    let mut store = DurableWorkflow::open(&root.join("ledger"), config(p)).unwrap();
    record(
        &mut store,
        "user",
        WorkflowCommand::Propose {
            plan: p.workflow.clone(),
        },
        CommandContext {
            execution: Some(ExecutionEvidence::Revision {
                revision: p.clone(),
            }),
            ..Default::default()
        },
    )
    .unwrap();
    store
}
fn approve(store: &mut DurableWorkflow, p: &PlanRevision) {
    let digest = store.state().digest.clone().unwrap();
    record(
        store,
        "user",
        WorkflowCommand::Approve { digest },
        CommandContext {
            approval_source: Some("synthetic baseline user approval".into()),
            execution: Some(ExecutionEvidence::Approval {
                approval: Approval {
                    worker_account_setup: None,
                    plan_id: p.workflow.id.clone(),
                    revision: p.workflow.version,
                    policy_version: p.policy.version,
                    revision_digest: p.approval_digest().unwrap(),
                    user_id: "user".into(),
                    approved_at: chrono::Utc::now().to_rfc3339(),
                },
            }),
            ..Default::default()
        },
    )
    .unwrap();
}
fn assignment(p: &PlanRevision) -> (WorkflowCommand, CommandContext) {
    let attempt = Attempt {
        workspace_proof: None,
        id: "attempt".into(),
        task_id: "task".into(),
        worker_id: "worker".into(),
        revision_digest: p.approval_digest().unwrap(),
        fence: 1,
        binding: SessionBinding {
            adapter_id: p.execution[0].adapter_id.clone(),
            adapter_version: "api-worker-v1".into(),
            session_id: "single-session".into(),
            workspace: p.project.root.clone(),
            identity_strength: IdentityStrength::ControllerRequest(ControllerRequestIdentity {
                connection_id: "single-connection".into(),
                request_id: "single-request".into(),
            }),
        },
    };
    (
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
}
fn assign(store: &mut DurableWorkflow, p: &PlanRevision) {
    let (command, context) = assignment(p);
    record(store, "core", command, context).unwrap();
}
fn accept(store: &mut DurableWorkflow, principal: &str) -> Result<(), String> {
    record(
        store,
        principal,
        WorkflowCommand::Accept {
            task: "task".into(),
            attempt: "attempt".into(),
        },
        CommandContext::default(),
    )
}

#[test]
fn baseline_user_approval_is_required_and_other_roles_cannot_assign() {
    let dir = tempfile::tempdir().unwrap();
    let p = plan(dir.path(), selection(targets().remove(2)));
    let mut store = proposed(dir.path(), &p);
    let (command, context) = assignment(&p);
    assert!(record(&mut store, "core", command.clone(), context.clone()).is_err());
    assert_eq!(store.history().len(), 1);
    approve(&mut store, &p);
    for principal in ["user", "worker", "chief", "deputy"] {
        assert!(record(&mut store, principal, command.clone(), context.clone()).is_err());
    }
    assign(&mut store, &p);
    assert!(record(&mut store, "core", command, context).is_err());
    assert_eq!(store.history().len(), 3);
    assert!(store.handoffs().is_ok());
}

#[test]
fn baseline_configuration_cannot_convert_team_ledgers_or_claim_management() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = plan(dir.path(), selection(targets().remove(2)));
    let store = proposed(dir.path(), &p);
    let mut old = config(&p);
    old.schema_version = 1;
    drop(store);
    assert!(DurableWorkflow::open(&dir.path().join("ledger"), old).is_err());
    let mut invalid = config(&p);
    invalid.principals.push(Principal {
        id: "chief".into(),
        role: Role::Chief,
        provider: Some("openai".into()),
    });
    assert!(DurableWorkflow::open(&dir.path().join("invalid"), invalid).is_err());
    assert!(!dir.path().join("invalid").exists());
    p.management_setup_digest = Some("a".repeat(64));
    let mut s = DurableWorkflow::open(&dir.path().join("manager"), config(&p)).unwrap();
    assert!(record(
        &mut s,
        "user",
        WorkflowCommand::Propose {
            plan: p.workflow.clone()
        },
        CommandContext {
            execution: Some(ExecutionEvidence::Revision { revision: p }),
            ..Default::default()
        }
    )
    .is_err());
    assert!(s.history().is_empty());
}

#[tokio::test]
async fn baseline_file_execution_verification_accept_and_reopen_without_management() {
    let dir = tempfile::tempdir().unwrap();
    // Keep one auditable full case when explicitly selected by the local harness.
    let kept = std::env::var_os("CERULEAN_BASELINE_246_DIR").map(std::path::PathBuf::from);
    let root = kept.as_deref().unwrap_or(dir.path());
    if kept.is_some() {
        fs::create_dir(root).unwrap();
    }
    let p = plan(root, selection(targets().remove(2)));
    let mut store = proposed(root, &p);
    approve(&mut store, &p);
    assign(&mut store, &p);
    let (_tx, mut rx) = oneshot::channel();
    let (result, sends) = run(&mut store, "ok", &mut rx, &mut |_| Ok(())).await;
    result.unwrap();
    assert_eq!(sends, 2);
    assert_eq!(
        fs::read_to_string(root.join("project/result.txt")).unwrap(),
        "fixture output"
    );
    assert_eq!(prior_requests(&store).unwrap(), 2);
    assert!(accept(&mut store, "core").is_err());
    assert!(accept(&mut store, "worker").is_err());
    let pin = PinnedCheck::from_approved(&p.execution[0].checks[0]).unwrap();
    store
        .run_verification(
            "baseline-check",
            "task",
            "attempt",
            &[pin],
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap();
    accept(&mut store, "core").unwrap();
    let count = store.history().len();
    for _ in 0..2 {
        store = reopen(store, root);
        assert_eq!(store.state().tasks["task"].state, TaskState::Accepted);
        assert_eq!(store.history().len(), count);
        let (result, calls) = run(&mut store, "ok", &mut rx, &mut |_| Ok(())).await;
        assert!(result.is_err());
        assert_eq!(calls, 0);
    }
    assert!(store
        .configuration()
        .principals
        .iter()
        .all(|p| p.provider.is_none()));
    assert!(store
        .history()
        .iter()
        .all(|e| ["user", "core", "worker"].contains(&e.body.event.principal.as_str())));
    assert!(store.handoffs().is_ok());
    drop(store);
    DurableWorkflow::inspect_existing(&root.join("ledger"), |s| {
        assert_eq!(s.state().tasks["task"].state, TaskState::Accepted);
        assert!(s.requires_reopen());
        Ok(())
    })
    .unwrap();
}

#[tokio::test]
async fn baseline_transport_failure_reopens_unknown_and_never_resends() {
    let dir = tempfile::tempdir().unwrap();
    let p = plan(dir.path(), selection(targets().remove(2)));
    let mut store = proposed(dir.path(), &p);
    approve(&mut store, &p);
    assign(&mut store, &p);
    let (_tx, mut rx) = oneshot::channel();
    let (result, sends) = run(&mut store, "provider-error", &mut rx, &mut |_| Ok(())).await;
    assert!(result.is_err());
    assert_eq!(sends, 1);
    let mut store = reopen(store, dir.path());
    assert_eq!(store.state().tasks["task"].state, TaskState::Unknown);
    assert!(accept(&mut store, "core").is_err());
    let (result, sends) = run(&mut store, "ok", &mut rx, &mut |_| Ok(())).await;
    assert!(result.is_err());
    assert_eq!(sends, 0);
}

#[tokio::test]
async fn baseline_request_budget_stops_before_second_request() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = plan(dir.path(), selection(targets().remove(2)));
    p.policy.max_requests = 1;
    let mut store = proposed(dir.path(), &p);
    approve(&mut store, &p);
    assign(&mut store, &p);
    let (_tx, mut rx) = oneshot::channel();
    let (result, sends) = run(&mut store, "ok", &mut rx, &mut |_| Ok(())).await;
    assert!(result.is_err());
    assert_eq!(sends, 1);
    // An unsealed failed attempt retains its entire approved reservation;
    // observed send count must not be confused with released capacity.
    assert_eq!(
        prior_requests(&store).unwrap(),
        p.execution[0].api_worker.as_ref().unwrap().max_rounds
    );
    assert!(accept(&mut store, "core").is_err());
}
