use super::*;

fn fixture() -> (tempfile::TempDir, RuntimeHost, Value) {
    let root = tempfile::tempdir().unwrap();
    let (mut host, draft) = super::super::project_planning::tests::fixture(root.path());
    host.configure_project(draft).unwrap();
    let context = host.management_context("project", "fixed task").unwrap();
    (root, host, context)
}

fn intent(context: Value) -> DecisionIntent {
    DecisionIntent {
        operation_id: "planning-bindings".into(),
        project_id: "project".into(),
        purpose: management::Purpose::Plan,
        model: "fixture-chief".into(),
        state_digest: digest(&context).unwrap(),
        context,
        created_at: chrono::Utc::now().to_rfc3339(),
    }
}

#[test]
fn planning_bindings_preserve_legacy_and_reject_changed_identity_version_or_purpose() {
    let (_root, _host, original) = fixture();
    let old = intent(original.clone());
    assert_eq!(rebuild(&old, original.clone()).unwrap(), original);
    let updated = add_planning_bindings(original.clone()).unwrap();
    assert_eq!(
        updated["planningBindings"]["default"]["workerId"],
        "worker-codex"
    );
    let current = intent(updated.clone());
    assert_eq!(rebuild(&current, original.clone()).unwrap(), updated);
    for (field, value) in [
        ("workerId", json!("codex")),
        ("validatorId", json!("other")),
    ] {
        let mut changed = current.clone();
        changed.context["planningBindings"]["default"][field] = value;
        assert!(rebuild(&changed, original.clone()).is_err());
    }
    let mut changed = current.clone();
    changed.context["planningBindings"]["schemaVersion"] = json!(2);
    assert!(rebuild(&changed, original.clone()).is_err());
    changed = current.clone();
    changed.purpose = management::Purpose::Assign;
    assert!(rebuild(&changed, original.clone()).is_err());
    let mut changed_setup = original.clone();
    changed_setup["executionSetup"]["setup"]["workerAdapter"] = json!("deepseek");
    assert!(rebuild(&current, changed_setup).is_err());
    let mut changed_requirement = original;
    changed_requirement["requirement"] = json!("different goal");
    assert_ne!(
        digest(&rebuild(&current, changed_requirement).unwrap()).unwrap(),
        current.state_digest
    );
}

#[test]
fn planning_bindings_select_profile_and_integration_without_confusing_adapter_identity() {
    let (_root, _host, mut context) = fixture();
    let mut saved: project_setup::SavedProjectSetup =
        serde_json::from_value(context["executionSetup"].clone()).unwrap();
    let setup = &mut saved.setup;
    let acceptance = project_setup::AcceptanceSetup {
        schema_version: 1,
        stages: vec![project_setup::AcceptanceStage {
            validator_id: "profile-check".into(),
            check: setup.check.clone(),
        }],
    };
    setup.profiles.push(project_setup::WorkerProfile {
        id: "api-profile".into(),
        description: "profile".into(),
        resource_keys: vec![],
        supervision_override: None,
        worker_adapter: project_setup::ProjectWorkerAdapter::Deepseek,
        worker: None,
        api_worker: None,
        desktop_worker: None,
        artifact_paths: vec!["main.mjs".into()],
        acceptance,
    });
    setup.worktree = Some(GitWorktreePolicy {
        schema_version: 1,
        base_commit: "a".repeat(40),
        pool_root: "C:\\fixture-pool".into(),
        git: setup.check.tool.as_ref().unwrap().executable.clone(),
        input: GitWorktreeInput::CommittedRawBlobsWithAcceptedDependencies,
        integration: Some(Box::new(ProjectIntegration {
            task_id: "integrate".into(),
            artifact_paths: vec!["main.mjs".into()],
            validator_id: "integration-check".into(),
            check: setup.check.clone(),
        })),
    });
    context["executionSetup"] = serde_json::to_value(saved).unwrap();
    let bound = add_planning_bindings(context).unwrap();
    assert_eq!(
        bound["planningBindings"]["profiles"][0],
        json!({
            "profileId":"api-profile","adapterId":"deepseek","workerId":"worker-deepseek","validatorId":"profile-check"
        })
    );
    assert_eq!(
        bound["planningBindings"]["integration"],
        json!({"taskId":"integrate","validatorId":"integration-check"})
    );
}

#[test]
fn planning_bindings_are_only_added_to_new_provider_plan_contexts() {
    let (_root, mut host, original) = fixture();
    assert_eq!(
        host.management_dispatch_context("project", management::Purpose::Plan, "fixed task")
            .unwrap(),
        original
    );
    host.project_setups
        .get_mut("project")
        .unwrap()
        .setup
        .management_sources = Some(project_setup::ManagementSources {
        chief: project_setup::ChiefSource::Provider {
            endpoint: cerulean_domain::credentials::ProviderEndpoint::DeepSeek {},
        },
        deputy: project_setup::DeputySource::DeepSeek {},
    });
    let plan = host
        .management_dispatch_context("project", management::Purpose::Plan, "fixed task")
        .unwrap();
    assert!(plan["planningBindings"].is_object());
    assert!(host
        .management_dispatch_context("project", management::Purpose::Accept, "fixed task")
        .unwrap()
        .get("planningBindings")
        .is_none());
    let base = host.management_context("project", "fixed task").unwrap();
    assert!(base.get("planningBindings").is_none());
    assert_eq!(
        host.rebuild_management_context(&intent(plan.clone()), base)
            .unwrap(),
        plan
    );
}

#[test]
#[ignore = "requires explicit immutable original provider ledger directories"]
fn original_provider_ledgers_keep_wire_fingerprints_and_context_bytes() {
    let paths =
        std::env::var_os("CERULEAN_PLANNING_LEDGERS_511").expect("explicit ledger directories");
    let mut count = 0;
    for directory in std::env::split_paths(&paths) {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if !path.to_string_lossy().ends_with(".0.json") {
                continue;
            }
            let before = std::fs::read(&path).unwrap();
            let record: Value = serde_json::from_slice(&before).unwrap();
            let evidence = &record["body"]["evidence"];
            let Some(request) = evidence.get("request") else {
                continue;
            };
            let request: cerulean_adapters::deputy_management::DeputyRequest =
                serde_json::from_value(request.clone()).unwrap();
            request.validate().unwrap();
            let old: DecisionIntent = serde_json::from_value(evidence["intent"].clone()).unwrap();
            assert!(old.context.get("planningBindings").is_none());
            let mut base = old.context.clone();
            base.as_object_mut().unwrap().remove("assignmentBindings");
            assert_eq!(rebuild(&old, base).unwrap(), old.context);
            assert_eq!(digest(&old.context).unwrap(), old.state_digest);
            assert_eq!(std::fs::read(path).unwrap(), before);
            count += 1;
        }
    }
    assert!(
        count >= 4,
        "include original Plan/Assign/Accept and the rejected Plan"
    );
}
