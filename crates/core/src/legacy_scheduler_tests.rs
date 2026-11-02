use super::*;

fn legacy_fixture() -> (
    tempfile::TempDir,
    RuntimeHost,
    project_setup::ProjectSetupDraft,
) {
    let root = tempfile::tempdir().unwrap();
    let (host, draft) = project_planning::tests::fixture(root.path());
    let project = host.projects["project"]
        .configuration()
        .execution_scope
        .as_ref()
        .unwrap()
        .project
        .clone();
    drop(host);
    let workflow = root.path().join("data/workflows/project");
    // The fixture has no events yet. Replace only its disposable bindings
    // with the exact pre-A6API generation before any workflow command exists.
    fs::remove_dir_all(&workflow).unwrap();
    drop(DurableWorkflow::open(&workflow, legacy_config(project)).unwrap());
    let host = RuntimeHost::open(&root.path().join("data")).unwrap();
    (root, host, draft)
}

#[test]
fn old_role_bindings_reopen_with_original_plan_and_stay_byte_identical() {
    let (root, mut host, draft) = legacy_fixture();
    host.configure_project(draft).unwrap();
    project_planning::tests::complete(&mut host, project_planning::tests::proposal("tests"));
    let workflow = &host.projects["project"];
    assert!(workflow.revision().is_some());
    assert!(!workflow.history().is_empty());
    assert!(require_bound_worker(workflow.configuration(), "codex").is_ok());
    assert!(require_bound_worker(workflow.configuration(), "a6api").is_err());
    let context = host
        .management_context("project", "Read original plan")
        .unwrap();
    assert_eq!(
        context["workers"],
        serde_json::to_value(
            cerulean_adapters::catalog()
                .into_iter()
                .filter(|a| a.id != "workbuddy")
                .collect::<Vec<_>>()
        )
        .unwrap()
    );
    let history_len = workflow.history().len();
    let binding = root.path().join("data/workflows/project/bindings.json");
    let before = fs::read(&binding).unwrap();
    drop(host);
    let reopened = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(reopened.projects["project"].history().len(), history_len);
    assert_eq!(fs::read(binding).unwrap(), before);
}

#[test]
fn only_exact_legacy_or_current_roles_match_and_old_project_refuses_new_worker() {
    let (root, mut host, mut draft) = legacy_fixture();
    let original = host.projects["project"].configuration().clone();
    let project = original.execution_scope.as_ref().unwrap().project.clone();
    assert!(matches_config(&original, &project));
    assert!(matches_config(&config(project.clone()), &project));
    assert!(require_bound_worker(&config(project), "a6api").is_ok());

    draft.worker_adapter = project_setup::ProjectWorkerAdapter::A6api;
    let error = host.configure_project(draft).unwrap_err();
    assert!(error.contains("原角色表"), "{error}");
    assert!(!root
        .path()
        .join("data/project-setups")
        .read_dir()
        .unwrap()
        .any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".json")
        }));

    let binding = root.path().join("data/workflows/project/bindings.json");
    let bytes = fs::read(&binding).unwrap();
    drop(host);
    let mut altered = Vec::new();
    let mut unknown = original.clone();
    unknown.principals.push(Principal {
        id: "worker-unregistered".into(),
        role: Role::Worker,
        provider: None,
    });
    altered.push(unknown);
    let mut changed = original.clone();
    changed.principals[4].provider = Some("changed".into());
    altered.push(changed);
    let mut reordered = original;
    reordered.principals.swap(4, 5);
    altered.push(reordered);
    for candidate in altered {
        assert!(!matches_config(
            &candidate,
            &candidate.execution_scope.as_ref().unwrap().project
        ));
        fs::write(&binding, serde_json::to_vec(&candidate).unwrap()).unwrap();
        let error = RuntimeHost::open(&root.path().join("data"))
            .err()
            .expect("altered role bindings must be refused");
        assert!(
            error.contains("host role or scope configuration mismatch"),
            "{error}"
        );
    }
    fs::write(&binding, &bytes).unwrap();
    let reopened = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert!(require_bound_worker(reopened.projects["project"].configuration(), "codex").is_ok());
    assert_eq!(fs::read(binding).unwrap(), bytes);
}
