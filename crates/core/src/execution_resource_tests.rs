use super::*;
use cerulean_domain::{
    desktop_sessions::*, management::ManagementDecision, project_setup::WorktreeSetupDraft,
};

fn directory(path: &str, write: bool) -> ResourceKey {
    ResourceKey::Directory {
        path: path.into(),
        write,
    }
}

#[test]
fn directories_use_case_folded_components_and_read_write_compatibility() {
    let write = directory(r"D:\Repo", true);
    let read = directory(r"d:\repo", false);
    assert!(write.conflicts(&read) && read.conflicts(&write));
    assert!(write.conflicts(&directory(r"d:\REPO\nested", false)));
    assert!(write.conflicts(&directory(r"d:\", true)));
    assert!(!write.conflicts(&directory(r"d:\repository", true)));
    assert!(!read.conflicts(&directory(r"D:\REPO\nested", false)));
    assert!(!write.conflicts(&ResourceKey::Named(r"D:\Repo".into())));
    assert!(!ResourceKey::Named("GPU".into()).conflicts(&ResourceKey::Named("gpu".into())));
}

#[test]
fn approved_worktree_reservations_allow_independent_tasks_but_retain_attempt_ownership() {
    for isolated in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (mut host, mut draft) = project_planning::tests::fixture(root.path());
        let pool = root.path().join("pool");
        fs::create_dir(&pool).unwrap();
        if isolated {
            // Configuration only: this pinned file is never launched as Git.
            let git = root.path().join("git.exe");
            fs::write(&git, b"resource-configuration fixture; not executable").unwrap();
            draft.worktree = Some(WorktreeSetupDraft {
                base_commit: "a".repeat(40),
                pool_root: pool.to_string_lossy().into(),
                git_executable: git.to_string_lossy().into(),
                inherit_dependencies: false,
                integration: None,
            });
        }
        host.configure_project(draft).unwrap();
        let mut decision = project_planning::tests::proposal("tests");
        if let ManagementDecision::Plan { tasks, .. } = &mut decision {
            let mut second = tasks[0].clone();
            second.id = "second".into();
            tasks.push(second);
        }
        project_planning::tests::complete(&mut host, decision);
        let generated = host.projects["project"].revision().unwrap().clone();
        for task in &generated.workflow.tasks {
            assert_eq!(
                task.workspace_key,
                if isolated {
                    isolated_task_key("project", &task.id).unwrap()
                } else {
                    "project".into()
                }
            );
        }
        let mut forged = generated.clone();
        forged.workflow.version += 1;
        forged.workflow.tasks[0].workspace_key = if isolated {
            isolated_task_key("project", "foreign-task").unwrap()
        } else {
            isolated_task_key("project", "task").unwrap()
        };
        let before = host.projects["project"].history().to_vec();
        assert!(host.propose("project", "forged-isolation", forged).is_err());
        assert_eq!(host.projects["project"].history(), before);
        assert_eq!(host.projects["project"].revision(), Some(&generated));
        let approved = host.projects["project"]
            .revision()
            .unwrap()
            .approval_digest()
            .unwrap();
        host.approve("project", &approved).unwrap();
        super::tests::reserve(&mut host);
        let leases = host.resource_leases().unwrap();
        assert_eq!(leases.len(), if isolated { 2 } else { 1 });
        assert!(leases.iter().all(|l| l.starting));
        assert!(host
            .require_task_resources("project", "task", "resource-owner")
            .is_ok());
        assert!(host
            .require_task_resources("project", "task", "replacement")
            .is_err());
        assert_eq!(
            host.require_task_resources("project", "second", "second-attempt")
                .is_ok(),
            isolated
        );
        assert_eq!(fs::read_dir(&pool).unwrap().count(), 0);
        let history = host.projects["project"].history().to_vec();
        drop(host);
        let host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(host.projects["project"].history(), history);
        assert_eq!(host.resource_leases().unwrap(), leases);
        assert_eq!(
            host.require_task_resources("project", "second", "second-attempt")
                .is_ok(),
            isolated
        );
        assert!(host
            .require_task_resources("project", "task", "replacement")
            .is_err());
        assert_eq!(fs::read_dir(&pool).unwrap().count(), 0);
    }
}

#[test]
fn implicit_directory_is_not_released_when_named_resources_are_absent() {
    let root = tempfile::tempdir().unwrap();
    let mut host = super::tests::fixture(root.path());
    let mut plan = host.projects["project"].revision().unwrap().clone();
    plan.workflow.version += 1;
    plan.workflow.tasks[0].resource_keys = None;
    host.propose("project", "implicit-plan", plan.clone())
        .unwrap();
    host.approve("project", &plan.approval_digest().unwrap())
        .unwrap();
    super::tests::reserve(&mut host);
    let leases = host.resource_leases().unwrap();
    assert_eq!(leases.len(), 1);
    assert!(matches!(
        leases[0].key,
        ResourceKey::Directory { write: true, .. }
    ));
    assert!(host
        .require_task_resources("other", "task", "candidate")
        .is_ok());
    assert!(host
        .require_task_resources("project", "task", "candidate")
        .is_err());
    drop(host);
    let host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(host.resource_leases().unwrap(), leases);
}

fn selection(root: String, app: DesktopApp) -> DesktopWorkerSelection {
    let observation = DesktopObservation {
        window: DesktopWindow {
            app: app.clone(),
            hwnd: "123456".into(),
            pid: 123,
            start_ticks: "123456789".into(),
            exe: if app == DesktopApp::Qoder {
                r"D:\apps\qoder.exe"
            } else {
                r"D:\apps\workbuddy.exe"
            }
            .into(),
            title: "controlled-resource-fixture".into(),
            version: "fixture".into(),
        },
        captured_at: chrono::Utc::now().to_rfc3339(),
        document_runtime_id: "42.1.2".into(),
        session_id: (app == DesktopApp::Qoder)
            .then(|| "12345678-1234-1234-1234-123456789abc".into()),
        content_digest: "a".repeat(64),
        layout_digest: "b".repeat(64),
        node_count: 10,
        draft_empty: true,
        busy: false,
        model_label: Some("fixture-model".into()),
    };
    let mut binding = DesktopBinding {
        id: String::new(),
        project_id: "project".into(),
        label: "resource fixture".into(),
        observation,
        confirmed_at: chrono::Utc::now().to_rfc3339(),
        bridge_sha256: "c".repeat(64),
    };
    binding.id = digest(&(
        &binding.project_id,
        &binding.label,
        &binding.observation,
        &binding.confirmed_at,
        &binding.bridge_sha256,
    ))
    .unwrap();
    let selected = DesktopWorkerSelection {
        binding,
        project_root: root,
        model_label: "fixture-model".into(),
        workspace_confirmed: true,
        external_tools_acknowledged: true,
    };
    selected.validate().unwrap();
    selected
}

#[test]
fn desktop_locks_follow_window_and_provider_session_instead_of_binding_labels() {
    let root = tempfile::tempdir().unwrap();
    let host = super::tests::fixture(root.path());
    let original = host.projects["project"].revision().unwrap();
    for app in [DesktopApp::Qoder, DesktopApp::Workbuddy] {
        let mut plan = original.clone();
        plan.workflow.tasks[0].resource_keys = None;
        plan.execution[0].desktop_worker = Some(selection(plan.project.root.clone(), app.clone()));
        let first = task_resources(&plan, "task", "first").unwrap();
        let channels: Vec<_> = first
            .iter()
            .filter(|k| {
                matches!(
                    k,
                    ResourceKey::DesktopWindow { .. } | ResourceKey::DesktopSession { .. }
                )
            })
            .collect();
        assert_eq!(channels.len(), if app == DesktopApp::Qoder { 2 } else { 1 });
        // A new local binding label and UI runtime anchor are not a new channel.
        let selected = plan.execution[0].desktop_worker.as_mut().unwrap();
        selected.binding.label = "new local label".into();
        selected.binding.id = "d".repeat(64);
        selected.binding.observation.document_runtime_id = "different-anchor".into();
        selected.binding.observation.window.hwnd = "000123456".into();
        selected.binding.observation.window.start_ticks = "000123456789".into();
        if let Some(id) = &mut selected.binding.observation.session_id {
            *id = id.to_ascii_uppercase();
        }
        let second = task_resources(&plan, "task", "second").unwrap();
        assert!(channels
            .iter()
            .all(|a| second.iter().any(|b| a.conflicts(b))));
        // Same Qoder chat in another window still conflicts via provider ID.
        let window = &mut plan.execution[0]
            .desktop_worker
            .as_mut()
            .unwrap()
            .binding
            .observation
            .window;
        window.hwnd = "456789".into();
        window.start_ticks = "987654321".into();
        let third = task_resources(&plan, "task", "third").unwrap();
        assert_eq!(
            channels
                .iter()
                .any(|a| third.iter().any(|b| a.conflicts(b))),
            app == DesktopApp::Qoder
        );
        plan.execution[0]
            .desktop_worker
            .as_mut()
            .unwrap()
            .binding
            .observation
            .session_id = None;
        let fourth = task_resources(&plan, "task", "fourth").unwrap();
        assert!(!channels
            .iter()
            .any(|a| fourth.iter().any(|b| a.conflicts(b))));
    }
}
