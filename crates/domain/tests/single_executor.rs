use cerulean_domain::workflow::*;

fn bindings() -> Vec<Principal> {
    [
        ("user", Role::User),
        ("core", Role::Core),
        ("worker", Role::Worker),
    ]
    .into_iter()
    .map(|(id, role)| Principal {
        id: id.into(),
        role,
        provider: None,
    })
    .collect()
}
fn plan() -> WorkflowPlan {
    WorkflowPlan {
        id: "baseline".into(),
        version: 1,
        tasks: vec![WorkflowTask {
            id: "task".into(),
            dependencies: vec![],
            workspace_key: "project".into(),
            goal: "single approved task".into(),
            resource_keys: None,
        }],
    }
}
#[test]
fn baseline_constructor_rejects_missing_duplicate_and_model_roles() {
    let mut missing = bindings();
    missing.pop();
    assert!(Workflow::new_single_executor(missing).is_err());
    let mut duplicate = bindings();
    duplicate[2] = duplicate[0].clone();
    assert!(Workflow::new_single_executor(duplicate).is_err());
    let mut provider = bindings();
    provider[0].provider = Some("openai".into());
    assert!(Workflow::new_single_executor(provider).is_err());
    assert!(Workflow::new(vec![Principal {
        id: "chief".into(),
        role: Role::Chief,
        provider: Some("local".into())
    }])
    .is_err());
}
#[test]
fn baseline_has_no_implicit_approval_or_team_permissions() {
    let mut core = Workflow::new_single_executor(bindings()).unwrap();
    let propose = WorkflowCommand::Propose { plan: plan() };
    for who in ["core", "worker"] {
        assert!(core.apply(who, propose.clone()).is_err());
    }
    core.apply("user", propose.clone()).unwrap();
    assert!(!core.state().approved);
    let assign = WorkflowCommand::Assign {
        task: "task".into(),
        worker: "worker".into(),
        attempt: "a".into(),
    };
    assert!(core.apply("core", assign.clone()).is_err());
    core.apply(
        "user",
        WorkflowCommand::Approve {
            digest: core.state().digest.clone().unwrap(),
        },
    )
    .unwrap();
    assert!(core.apply("worker", assign.clone()).is_err());
    core.apply("core", assign).unwrap();
    assert!(core.apply("user", propose).is_err());
    assert!(core
        .apply(
            "core",
            WorkflowCommand::Advise {
                task: "task".into(),
                suggestion: "change".into()
            }
        )
        .is_err());
    assert!(core
        .apply(
            "core",
            WorkflowCommand::Accept {
                task: "task".into(),
                attempt: "a".into()
            }
        )
        .is_err());
    assert!(Workflow::replay(bindings(), core.journal()).is_err());
}
#[test]
fn baseline_rejects_multiple_tasks_and_replanning() {
    let mut core = Workflow::new_single_executor(bindings()).unwrap();
    let mut two = plan();
    let mut second = two.tasks[0].clone();
    second.id = "second".into();
    two.tasks.push(second);
    assert!(core
        .apply("user", WorkflowCommand::Propose { plan: two })
        .is_err());
    core.apply("user", WorkflowCommand::Propose { plan: plan() })
        .unwrap();
    let mut next = plan();
    next.version = 2;
    assert!(core
        .apply("user", WorkflowCommand::Propose { plan: next })
        .is_err());
}
