use super::*;
use cerulean_domain::management::ManagementDecision;
use cerulean_domain::project_setup::{
    AcceptanceDraft, AcceptanceStageDraft, ProjectSetupDraft, ProjectWorkerAdapter,
    SavedProjectSetup, WorkerProfileDraft,
};
use project_planning::tests::{complete, complete_at, fixture, proposal};

fn profile(d: &ProjectSetupDraft) -> WorkerProfileDraft {
    WorkerProfileDraft {
        supervision_override: None,
        resource_keys: vec!["shared-db".into(), "gpu-0".into()],
        id: "special".into(),
        description: "Specialized worker".into(),
        worker_adapter: d.worker_adapter,
        codex_executable: d.codex_executable.clone(),
        worker_model: d.worker_model.clone(),
        omp_runtime_parent: None,
        api_worker: None,
        desktop_worker: None,
        artifact_paths: d.artifact_paths.clone(),
        acceptance: AcceptanceDraft {
            schema_version: 1,
            stages: vec![AcceptanceStageDraft {
                id: "check".into(),
                name: "Check".into(),
                validator_id: d.validator_id.clone(),
                args: d.check_args.clone(),
                timeout_seconds: 30,
            }],
        },
    }
}

#[test]
fn task_supervision_overrides_bind_approval_and_preserve_legacy_profiles() {
    use cerulean_domain::supervision::{SupervisionOverride as Override, SupervisionPolicy};
    let root = tempfile::tempdir().unwrap();
    let (mut host, mut draft) = fixture(root.path());
    draft.worker_max_requests = 4;
    let policy = SupervisionPolicy {
        schema_version: 1,
        repeated_failure_threshold: 2,
        max_advice_per_attempt: 2,
        delivery_enabled: true,
    };
    draft.supervision = Some(policy.clone());
    let mut inherited = profile(&draft);
    inherited.id = "inherited".into();
    draft.profiles.push(inherited.clone());
    host.configure_project(draft.clone()).unwrap();
    let old_saved = host.project_setups["project"].clone();
    let old_bytes = serde_json::to_vec(&old_saved).unwrap();
    assert!(!String::from_utf8_lossy(&old_bytes).contains("supervisionOverride"));
    let roundtrip: SavedProjectSetup = serde_json::from_slice(&old_bytes).unwrap();
    assert_eq!(serde_json::to_vec(&roundtrip).unwrap(), old_bytes);
    assert_eq!(roundtrip.digest, digest(&roundtrip.setup).unwrap());
    assert_eq!(
        roundtrip.setup.supervision_for(Some("inherited")).unwrap(),
        Some(&policy)
    );
    let old_path = root
        .path()
        .join("data/project-setups/project.00000001.json");
    let old_file = fs::read(&old_path).unwrap();

    let mut disabled = inherited.clone();
    disabled.id = "disabled".into();
    disabled.supervision_override = Some(Override::Disabled {});
    let mut scoped = inherited.clone();
    scoped.id = "scoped".into();
    let mut scoped_policy = policy.clone();
    scoped_policy.repeated_failure_threshold = 3;
    scoped_policy.max_advice_per_attempt = 1;
    scoped.supervision_override = Some(Override::Enabled {
        policy: scoped_policy.clone(),
    });
    let api = super::api_project_tests::draft_for(
        draft.clone(),
        ProjectWorkerAdapter::Deepseek,
        cerulean_domain::api_worker::ApiTarget::Provider {
            endpoint: cerulean_domain::credentials::ProviderEndpoint::DeepSeek {},
        },
    );
    let mut api_profile = profile(&api);
    api_profile.id = "api".into();
    api_profile.api_worker = api.api_worker;
    api_profile.supervision_override = Some(Override::Disabled {});
    draft.profiles = vec![inherited, disabled, scoped, api_profile];
    let before = crate::reader_generation::tests::snapshot(&root.path().join("data"));
    for selected in [1, 2] {
        let mut unprotected = draft.clone();
        unprotected.profiles = vec![draft.profiles[selected].clone()];
        assert!(host
            .configure_project(unprotected)
            .unwrap_err()
            .contains("reader generation 3"));
        assert_eq!(
            crate::reader_generation::tests::snapshot(&root.path().join("data")),
            before
        );
    }
    drop(host);
    crate::reader_generation::tests::promote_fixture(&root.path().join("data"));
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    // Both inherited and explicit API delivery now preserve the approved
    // policy. Neither may silently become Disabled or rewrite old setups.
    for selected in [
        None,
        Some(Override::Enabled {
            policy: policy.clone(),
        }),
    ] {
        let mut allowed = draft.clone();
        allowed.profiles[3].supervision_override = selected.clone();
        host.configure_project(allowed).unwrap();
        let current = &host.project_setups["project"].setup;
        assert_eq!(current.profiles[3].draft().supervision_override, selected);
        assert_eq!(current.supervision_for(Some("api")).unwrap(), Some(&policy));
        assert_eq!(fs::read(&old_path).unwrap(), old_file);
    }
    host.configure_project(draft.clone()).unwrap();
    let saved = host.project_setups["project"].clone();
    assert_eq!(
        saved.setup.profiles[1].draft().supervision_override,
        Some(Override::Disabled {})
    );
    assert_eq!(
        saved.setup.supervision_for(Some("scoped")).unwrap(),
        Some(&scoped_policy)
    );
    let mut decision = proposal("tests");
    if let ManagementDecision::Plan { tasks, .. } = &mut decision {
        let base = tasks[0].clone();
        *tasks = draft
            .profiles
            .iter()
            .map(|profile| {
                let mut task = base.clone();
                task.id = profile.id.clone();
                task.profile_id = Some(profile.id.clone());
                task.worker_id = profile.worker_adapter.worker_id();
                task
            })
            .collect();
    }
    complete(&mut host, decision);
    assert!(
        host.planning_errors.is_empty(),
        "{:?}",
        host.planning_errors
    );
    let revision = host.projects["project"].revision().unwrap().clone();
    assert_eq!(
        revision.supervision_for("inherited").unwrap(),
        Some(&policy)
    );
    assert_eq!(
        revision.supervision_for("scoped").unwrap(),
        Some(&scoped_policy)
    );
    assert!(revision.supervision_for("disabled").unwrap().is_none());
    assert!(revision.supervision_for("api").unwrap().is_none());
    assert!(revision.supervision_for("missing").is_err());
    let approved = revision.approval_digest().unwrap();
    host.approve("project", &approved).unwrap();
    for override_value in [
        None,
        Some(Override::Disabled {}),
        Some(Override::Enabled { policy }),
    ] {
        let mut changed = revision.clone();
        changed
            .execution
            .iter_mut()
            .find(|e| e.task_id == "scoped")
            .unwrap()
            .supervision_override = override_value;
        assert_ne!(changed.approval_digest().unwrap(), approved);
        assert!(host.projects["project"]
            .approval()
            .unwrap()
            .validate(&changed, "user", changed.policy.version)
            .is_err());
    }
    let history = host.projects["project"].history().to_vec();
    drop(host);
    let reopened = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(reopened.project_setups["project"].digest, saved.digest);
    assert_eq!(reopened.projects["project"].history(), history);
    assert_eq!(fs::read(old_path).unwrap(), old_file);
    assert!(reopened.codex_jobs.is_empty() && reopened.api_jobs.is_empty());
}

#[test]
fn reader_generation_blocks_all_proposal_writers_before_any_record() {
    use cerulean_domain::supervision::{SupervisionOverride, SupervisionPolicy};
    let root = tempfile::tempdir().unwrap();
    let (mut host, draft) = fixture(root.path());
    host.configure_project(draft).unwrap();
    complete(&mut host, proposal("tests"));
    let legacy = host.projects["project"].revision().unwrap().clone();
    let before = crate::reader_generation::tests::snapshot(&root.path().join("data"));
    for value in [
        SupervisionOverride::Disabled {},
        SupervisionOverride::Enabled {
            policy: SupervisionPolicy {
                schema_version: 1,
                repeated_failure_threshold: 2,
                max_advice_per_attempt: 1,
                delivery_enabled: true,
            },
        },
    ] {
        let mut revision = legacy.clone();
        revision.execution[0].supervision_override = Some(value);
        let command = WorkflowCommand::Propose {
            plan: revision.workflow.clone(),
        };
        let context = CommandContext {
            execution: Some(ExecutionEvidence::Revision {
                revision: revision.clone(),
            }),
            ..Default::default()
        };
        let target = crate::management_execution::ApplicationTarget::Workflow {
            principal: "chief".into(),
            command: command.clone(),
            context: Box::new(context.clone()),
        };
        assert!(host
            .propose("project", "blocked-proposal", revision)
            .unwrap_err()
            .contains("reader generation 3"));
        assert!(host
            .apply_management_decision_inner("absent-operation", target.clone())
            .unwrap_err()
            .contains("reader generation 3"));
        assert!(host
            .management
            .prepare_application("absent-operation", target, "absent-digest")
            .unwrap_err()
            .contains("reader generation 3"));
        assert!(host
            .projects
            .get_mut("project")
            .unwrap()
            .command("blocked-proposal", "chief", command, context, false)
            .unwrap_err()
            .contains("reader generation 3"));
        assert_eq!(
            crate::reader_generation::tests::snapshot(&root.path().join("data")),
            before
        );
    }
}

#[test]
fn invalid_and_overflow_resources_never_persist_configuration() {
    let root = tempfile::tempdir().unwrap();
    let (mut host, original) = fixture(root.path());
    for keys in [
        vec!["bad:key".into()],
        vec!["x".into(), "x".into()],
        vec!["a".repeat(81)],
        (0..33).map(|n| format!("r{n}")).collect(),
    ] {
        let mut draft = original.clone();
        draft.resource_keys = keys;
        assert!(host.configure_project(draft).is_err());
        assert!(host.project_setups.is_empty());
    }
    let mut draft = original;
    let mut extra = profile(&draft);
    draft.resource_keys = (0..32).map(|n| format!("r{n}")).collect();
    extra.resource_keys = vec!["additional".into()];
    draft.profiles = vec![extra];
    assert!(host.configure_project(draft.clone()).is_err());
    assert!(host.project_setups.is_empty());
    draft.profiles[0].resource_keys = vec!["r0".into()];
    host.configure_project(draft).unwrap();
    assert_eq!(
        host.project_setups["project"]
            .setup
            .resources_for(Some("special"))
            .unwrap()
            .unwrap()
            .len(),
        32
    );
}

#[test]
fn resources_derive_from_saved_profile_enter_approval_and_replay() {
    let root = tempfile::tempdir().unwrap();
    let (mut host, mut draft) = fixture(root.path());
    host.configure_project(draft.clone()).unwrap();
    complete(&mut host, proposal("tests"));
    let old = host.projects["project"].revision().unwrap().clone();
    assert!(old.workflow.tasks[0].resource_keys.is_none());
    let file = root
        .path()
        .join("data/project-setups/project.00000001.json");
    let legacy = fs::read(&file).unwrap();
    assert!(!String::from_utf8_lossy(&legacy).contains("resourceKeys"));
    draft.resource_keys = vec!["shared-db".into(), "Device".into(), "device".into()];
    draft.profiles = vec![profile(&draft)];
    host.configure_project(draft).unwrap();
    let mut decision = proposal("tests");
    if let ManagementDecision::Plan { tasks, .. } = &mut decision {
        tasks[0].profile_id = Some("special".into());
    }
    complete_at(&mut host, decision, "ui-plan-resources");
    let plan = host.projects["project"].revision().unwrap().clone();
    assert_eq!(
        plan.workflow.version,
        old.workflow.version + 1,
        "{:?}",
        host.planning_errors
    );
    let expected = vec!["shared-db", "Device", "device", "gpu-0"];
    assert_eq!(
        plan.workflow.tasks[0].resource_keys.as_ref().unwrap(),
        &expected
    );
    assert_eq!(
        host.project_setups["project"].setup.profiles[0].resource_keys,
        vec!["shared-db", "gpu-0"]
    );
    assert_eq!(
        host.project_setups["project"]
            .setup
            .resources_for(None)
            .unwrap()
            .unwrap(),
        vec!["shared-db", "Device", "device"]
    );
    assert!(host.project_setups["project"]
        .setup
        .resources_for(Some("missing"))
        .is_err());
    assert_ne!(
        plan.approval_digest().unwrap(),
        old.approval_digest().unwrap()
    );
    let mut changed = plan.clone();
    changed.workflow.tasks[0]
        .resource_keys
        .as_mut()
        .unwrap()
        .pop();
    assert_ne!(
        changed.approval_digest().unwrap(),
        plan.approval_digest().unwrap()
    );
    assert!(host.snapshot().workflows[0].approval.is_none());
    host.approve("project", &plan.approval_digest().unwrap())
        .unwrap();
    let history = host.projects["project"].history().len();
    drop(host);
    let host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(
        host.projects["project"]
            .revision()
            .unwrap()
            .approval_digest()
            .unwrap(),
        plan.approval_digest().unwrap()
    );
    assert_eq!(host.projects["project"].history().len(), history);
    assert_eq!(fs::read(file).unwrap(), legacy);
    assert!(host.startups.all().is_empty());
}

#[test]
fn changed_configuration_resources_cannot_start_approved_plan() {
    let root = tempfile::tempdir().unwrap();
    let (mut host, mut draft) = fixture(root.path());
    draft.resource_keys = vec!["shared-db".into()];
    host.configure_project(draft).unwrap();
    complete(&mut host, proposal("tests"));
    let plan = host.projects["project"].revision().unwrap().clone();
    let approved = plan.approval_digest().unwrap();
    host.approve("project", &approved).unwrap();
    let mut changed = host.project_setups["project"].clone();
    changed.setup.resource_keys.clear();
    changed.digest = digest(&changed.setup).unwrap();
    host.project_setups
        .insert("project".into(), changed.clone());
    assert!(host
        .start_project_run("project", &approved, &changed.digest)
        .is_err());
    assert!(host.runs.all().is_empty());
    assert!(host.codex_jobs.is_empty());
    assert!(host.management.statuses().len() == 1);
}
