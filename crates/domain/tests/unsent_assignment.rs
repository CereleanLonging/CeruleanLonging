use cerulean_domain::workflow::*;

fn principals() -> Vec<Principal> {
    [
        ("user", Role::User),
        ("chief", Role::Chief),
        ("deputy", Role::Deputy),
        ("core", Role::Core),
        ("worker", Role::Worker),
    ]
    .into_iter()
    .map(|(id, role)| Principal {
        id: id.into(),
        role,
        provider: matches!(role, Role::Chief | Role::Deputy).then(|| "openai".into()),
    })
    .collect()
}
fn assigned() -> Workflow {
    let mut core = Workflow::new(principals()).unwrap();
    let plan = WorkflowPlan {
        id: "plan".into(),
        version: 1,
        tasks: ["task", "other"]
            .into_iter()
            .map(|id| WorkflowTask {
                id: id.into(),
                dependencies: vec![],
                workspace_key: id.into(),
                goal: "synthetic unsent assignment".into(),
                resource_keys: None,
            })
            .collect(),
    };
    core.apply("chief", WorkflowCommand::Propose { plan })
        .unwrap();
    core.apply(
        "user",
        WorkflowCommand::Approve {
            digest: core.state().digest.clone().unwrap(),
        },
    )
    .unwrap();
    core.apply("deputy", assign("task", "old")).unwrap();
    core
}
fn assign(task: &str, attempt: &str) -> WorkflowCommand {
    WorkflowCommand::Assign {
        task: task.into(),
        worker: "worker".into(),
        attempt: attempt.into(),
    }
}
fn abandon(attempt: &str) -> WorkflowCommand {
    WorkflowCommand::AbandonUnsent {
        task: "task".into(),
        attempt: attempt.into(),
        reason: "明确放弃本地未发送尝试".into(),
    }
}

#[test]
fn retired_unknown_blocks_all_old_plan_assignments_until_explicit_revision_and_approval() {
    let mut core = assigned();
    let retirement = WorkflowCommand::RetireUnknownApi {
        task: "task".into(),
        attempt: "old".into(),
        reason: "local end only".into(),
    };
    assert!(core.apply("user", retirement.clone()).is_err());
    core.apply(
        "core",
        WorkflowCommand::DispatchIntent {
            task: "task".into(),
            attempt: "old".into(),
        },
    )
    .unwrap();
    core.apply(
        "core",
        WorkflowCommand::RecoveryUnknown {
            task: "task".into(),
            attempt: "old".into(),
        },
    )
    .unwrap();
    core.apply("user", retirement).unwrap();
    for task in ["task", "other"] {
        assert!(core.apply("deputy", assign(task, "new")).is_err());
    }
    let mut next = core.state().plan.clone().unwrap();
    next.version += 1;
    assert!(core
        .apply("chief", WorkflowCommand::Propose { plan: next.clone() })
        .is_err());
    core.apply(
        "user",
        WorkflowCommand::RequestRevision {
            digest: core.state().digest.clone().unwrap(),
            reason: "revise".into(),
        },
    )
    .unwrap();
    core.apply("chief", WorkflowCommand::Propose { plan: next })
        .unwrap();
    assert!(core.apply("deputy", assign("other", "new")).is_err());
    core.apply(
        "user",
        WorkflowCommand::Approve {
            digest: core.state().digest.clone().unwrap(),
        },
    )
    .unwrap();
    assert!(core.apply("deputy", assign("task", "old")).is_err());
    core.apply("deputy", assign("task", "new")).unwrap();
    assert_eq!(core.state().tasks["task"].fence, 2);
    assert_eq!(
        Workflow::replay(principals(), core.journal())
            .unwrap()
            .state(),
        core.state()
    );
}

#[test]
fn abandon_unsent_requires_user_and_retires_attempt_across_tasks_and_replay() {
    let mut core = assigned();
    let before = core.state().clone();
    for role in ["chief", "deputy", "core", "worker"] {
        assert!(core.apply(role, abandon("old")).is_err());
        assert_eq!(core.state(), &before);
    }
    assert!(core.apply("user", abandon("stale")).is_err());
    core.apply("user", abandon("old")).unwrap();
    let pending = &core.state().tasks["task"];
    assert_eq!(pending.state, TaskState::Pending);
    assert_eq!(pending.fence, 1);
    assert!(pending.attempt.is_none() && pending.worker.is_none());
    assert!(
        pending.evidence.is_empty()
            && !pending.verified
            && pending.cancellation_requested.is_none()
    );
    assert!(core.apply("user", abandon("old")).is_err());
    for task in ["task", "other"] {
        assert!(core.apply("deputy", assign(task, "old")).is_err());
    }
    let mut replayed = Workflow::replay(principals(), core.journal()).unwrap();
    assert_eq!(replayed.state(), core.state());
    assert!(replayed.apply("deputy", assign("task", "old")).is_err());
    replayed.apply("deputy", assign("task", "new")).unwrap();
    assert_eq!(replayed.state().tasks["task"].fence, 2);
    assert!(replayed
        .apply(
            "core",
            WorkflowCommand::DispatchIntent {
                task: "task".into(),
                attempt: "old".into()
            }
        )
        .is_err());
    replayed.apply("user", abandon("new")).unwrap();
    let mut revised = replayed.state().plan.clone().unwrap();
    revised.version = 2;
    replayed
        .apply("chief", WorkflowCommand::Propose { plan: revised })
        .unwrap();
    replayed
        .apply(
            "user",
            WorkflowCommand::Approve {
                digest: replayed.state().digest.clone().unwrap(),
            },
        )
        .unwrap();
    assert!(
        replayed.apply("deputy", assign("task", "old")).is_err(),
        "plan replacement does not recycle historical attempt IDs"
    );
}

#[test]
fn abandon_unsent_rejects_dispatch_history_even_after_not_started_reconciliation() {
    let mut core = assigned();
    core.apply(
        "core",
        WorkflowCommand::DispatchIntent {
            task: "task".into(),
            attempt: "old".into(),
        },
    )
    .unwrap();
    assert!(core.apply("user", abandon("old")).is_err());
    core.apply(
        "core",
        WorkflowCommand::RecoveryUnknown {
            task: "task".into(),
            attempt: "old".into(),
        },
    )
    .unwrap();
    assert!(core.apply("user", abandon("old")).is_err());
    core.apply(
        "core",
        WorkflowCommand::Reconcile {
            task: "task".into(),
            attempt: "old".into(),
            outcome: ReconcileOutcome::NotStarted,
            evidence: vec!["structured provider observation".into()],
        },
    )
    .unwrap();
    assert_eq!(core.state().tasks["task"].state, TaskState::Assigned);
    let journal = core.journal().to_vec();
    assert!(core
        .apply("user", abandon("old"))
        .unwrap_err()
        .contains("dispatch history"));
    assert_eq!(core.journal(), &journal);
    let mut reopened = Workflow::replay(principals(), &journal).unwrap();
    assert!(reopened.apply("user", abandon("old")).is_err());
}
