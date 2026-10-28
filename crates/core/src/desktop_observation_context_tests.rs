//! Approval-to-execution drift. The native immediate send guard is unchanged.
use super::*;

#[test]
fn qoder_initial_task_uses_fresh_layout_without_changing_approved_scope() {
    let root = tempfile::tempdir().unwrap();
    let mut host = configured(root.path(), "CONTEXT_LAYOUT");
    plan_and_approve(&mut host);
    start(&mut host);
    let attempt = assigned(&mut host);
    let store = &host.projects["project"];
    let revision = store.revision().unwrap();
    let approved = revision.approval_digest().unwrap();
    let mut current = revision
        .task("task")
        .unwrap()
        .desktop_worker
        .as_ref()
        .unwrap()
        .binding
        .observation
        .clone();
    current.captured_at = chrono::Utc::now().to_rfc3339();
    current.layout_digest = "f".repeat(64);
    current.node_count += 2;
    let review = crate::desktop_execution::prepare(
        store,
        "task",
        &attempt,
        current.clone(),
        vec!["a".repeat(64), "b".repeat(64)],
    )
    .unwrap();
    assert_eq!(review.observation, current);
    assert_eq!(
        store.revision().unwrap().approval_digest().unwrap(),
        approved
    );
    assert!(host.desktop_access.tasks.is_empty());
}

#[test]
fn qoder_initial_task_rejects_semantic_identity_and_invalid_layout_changes() {
    let root = tempfile::tempdir().unwrap();
    let mut host = configured(root.path(), "CONTEXT_NEGATIVE");
    plan_and_approve(&mut host);
    start(&mut host);
    let attempt = assigned(&mut host);
    let store = &host.projects["project"];
    let original = store
        .revision()
        .unwrap()
        .task("task")
        .unwrap()
        .desktop_worker
        .as_ref()
        .unwrap()
        .binding
        .observation
        .clone();
    for case in 0..12 {
        let mut changed = original.clone();
        changed.captured_at = chrono::Utc::now().to_rfc3339();
        changed.layout_digest = "f".repeat(64);
        changed.node_count += 2;
        match case {
            0 => changed.window.pid += 1,
            1 => changed.window.start_ticks = "99".into(),
            2 => changed.document_runtime_id.push_str(".extra"),
            3 => changed.session_id = Some("22222222-1234-1234-1234-123456789abc".into()),
            4 => changed.content_digest = "c".repeat(64),
            5 => changed.model_label = Some("other-model".into()),
            6 => changed.draft_empty = false,
            7 => changed.busy = true,
            8 => changed.node_count = 0,
            9 => changed.node_count = 1001,
            10 => changed.layout_digest = "invalid".into(),
            _ => changed.captured_at = "2000-01-01T00:00:00Z".into(),
        }
        assert!(
            crate::desktop_execution::prepare(
                store,
                "task",
                &attempt,
                changed,
                vec!["a".repeat(64), "b".repeat(64)],
            )
            .is_err(),
            "case {case}"
        );
    }
    assert!(host.desktop_access.tasks.is_empty());
}

#[test]
fn workbuddy_without_provider_session_keeps_strict_initial_layout_check() {
    let root = tempfile::tempdir().unwrap();
    let mut host = configured_for(root.path(), "CONTEXT_WORKBUDDY", DesktopApp::Workbuddy);
    plan_and_approve(&mut host);
    start(&mut host);
    let attempt = assigned(&mut host);
    let store = &host.projects["project"];
    let mut changed = store
        .revision()
        .unwrap()
        .task("task")
        .unwrap()
        .desktop_worker
        .as_ref()
        .unwrap()
        .binding
        .observation
        .clone();
    changed.captured_at = chrono::Utc::now().to_rfc3339();
    changed.layout_digest = "f".repeat(64);
    changed.node_count += 2;
    assert!(crate::desktop_execution::prepare(
        store,
        "task",
        &attempt,
        changed,
        vec!["a".repeat(64), "b".repeat(64)],
    )
    .is_err());
    assert!(host.desktop_access.tasks.is_empty());
}
