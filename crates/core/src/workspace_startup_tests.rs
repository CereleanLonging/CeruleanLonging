//! Controlled original startup query contracts; no executor, model or Git calls.
use super::*;
use cerulean_domain::workspace::*;

fn fixture(root: &Path) -> RuntimeHost {
    startup_recovery::tests::fixture(root, true, true)
}
fn list(offset: u32, revision: Option<String>) -> WorkspaceQuery {
    WorkspaceQuery::CodexStartups {
        project_id: "project".into(),
        offset,
        revision,
    }
}
fn first(host: &RuntimeHost) -> WorkspaceCodexStartupPage {
    match host.workspace_query(&list(0, None)).unwrap() {
        WorkspaceResult::CodexStartups(page) => page,
        _ => panic!("original startup page"),
    }
}
fn detail(revision: &str) -> WorkspaceQuery {
    WorkspaceQuery::CodexStartup {
        project_id: "project".into(),
        attempt_id: "startup-native".into(),
        revision: revision.into(),
    }
}

#[test]
fn workspace_startup_readonly_original_unknown_proof_and_cursor_contract() {
    let temp = tempfile::tempdir().unwrap();
    let host = fixture(temp.path());
    let before = crate::reader_generation::tests::snapshot(&host.root);
    let page = first(&host);
    assert_eq!(page.total, 1);
    assert_eq!(page.entries[0].attempt_id, "startup-native");
    assert_eq!(page.entries[0].state, "prepared");
    let WorkspaceResult::CodexStartup(found) =
        host.workspace_query(&detail(&page.revision)).unwrap()
    else {
        panic!("detail")
    };
    assert_eq!(
        serde_json::to_value(&found.detail.summary).unwrap(),
        serde_json::to_value(&page.entries[0]).unwrap()
    );
    assert_eq!(
        found.detail.protocol.as_deref(),
        Some("codex-durable-before-send-v1")
    );
    assert_eq!(
        host.codex_jobs[&("project".into(), "startup-native".into())].state,
        CodexJobState::Unknown
    );
    assert!(host.workspace_query(&list(1, None)).is_err());
    assert!(host.workspace_query(&detail(&"0".repeat(64))).is_err());
    let absent = WorkspaceQuery::CodexStartup {
        project_id: "project".into(),
        attempt_id: "missing".into(),
        revision: page.revision.clone(),
    };
    assert!(host.workspace_query(&absent).is_err());
    let expected =
        serde_json::to_value(host.workspace_query(&detail(&page.revision)).unwrap()).unwrap();
    let root = host.root.clone();
    drop(host);
    let host = RuntimeHost::open(&root).unwrap();
    assert_eq!(first(&host).revision, page.revision);
    assert_eq!(
        serde_json::to_value(host.workspace_query(&detail(&page.revision)).unwrap()).unwrap(),
        expected
    );
    assert_eq!(before, crate::reader_generation::tests::snapshot(&root));
    assert_eq!(
        before,
        crate::reader_generation::tests::snapshot(&host.root)
    );
}

#[test]
fn startup_reopen_preserves_original_unknown_message_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path());
    let key = ("project".to_owned(), "startup-native".to_owned());
    let original = host.codex_jobs[&key].message.clone();
    assert_eq!(host.codex_jobs[&key].state, CodexJobState::Unknown);
    let store = host.projects.remove("project").unwrap();
    let writer =
        crate::async_api::ApiWorkflow::start(store, host.root.join("workflows/project")).unwrap();
    host.api_writers.insert("project".into(), writer);
    let mut writer = host.api_writers.remove("project").unwrap();
    let store = writer.owner.finish(false).unwrap();
    host.projects.insert("project".into(), store);
    let reopened =
        crate::async_codex::load(&host.root, "project", &host.projects["project"]).unwrap();
    let loaded = reopened
        .into_iter()
        .find(|job| job.attempt_id == "startup-native")
        .unwrap();
    assert_eq!(loaded.state, CodexJobState::Unknown);
    assert_eq!(loaded.message, original);
}

#[test]
fn workspace_startup_strict_proof_refuses_inconsistent_poisoned_and_stale_records() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path());
    let page = first(&host);
    let before = crate::reader_generation::tests::snapshot(&host.root);
    host.codex_jobs
        .get_mut(&("project".into(), "startup-native".into()))
        .unwrap()
        .task_id = "wrong-task".into();
    assert!(host.workspace_query(&list(0, None)).is_err());
    assert!(host.workspace_query(&detail(&page.revision)).is_err());
    host.codex_jobs
        .get_mut(&("project".into(), "startup-native".into()))
        .unwrap()
        .task_id = "task".into();
    host.base.poisoned = true;
    assert!(host.workspace_query(&list(0, None)).is_err());
    assert!(host.workspace_query(&detail(&page.revision)).is_err());
    host.base.poisoned = false;
    host.codex_jobs
        .get_mut(&("project".into(), "startup-native".into()))
        .unwrap()
        .message = Some("changed original projection".into());
    assert!(host.workspace_query(&detail(&page.revision)).is_err());
    assert_eq!(
        host.codex_jobs[&("project".into(), "startup-native".into())].state,
        CodexJobState::Unknown
    );
    assert_eq!(
        before,
        crate::reader_generation::tests::snapshot(&host.root)
    );
}

#[test]
fn workspace_startup_revision_survives_actual_shared_writer_transfer() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path());
    let page = first(&host);
    let original =
        serde_json::to_value(host.workspace_query(&detail(&page.revision)).unwrap()).unwrap();
    let before = crate::reader_generation::tests::snapshot(&host.root);
    let store = host.projects.remove("project").unwrap();
    let path = host.root.join("workflows/project");
    let writer = crate::async_api::ApiWorkflow::start(store, path).unwrap();
    host.api_writers.insert("project".into(), writer);
    assert_eq!(first(&host).revision, page.revision);
    assert_eq!(
        serde_json::to_value(host.workspace_query(&detail(&page.revision)).unwrap()).unwrap(),
        original
    );
    host.api_writers
        .get_mut("project")
        .unwrap()
        .snapshot
        .sequence += 999;
    assert_eq!(first(&host).revision, page.revision);
    assert_eq!(
        serde_json::to_value(host.workspace_query(&detail(&page.revision)).unwrap()).unwrap(),
        original
    );
    host.api_writers.get_mut("project").unwrap().interrupted = true;
    assert!(host.workspace_query(&list(0, None)).is_err());
    assert!(host.workspace_query(&detail(&page.revision)).is_err());
    host.api_writers.get_mut("project").unwrap().interrupted = false;
    let mut writer = host.api_writers.remove("project").unwrap();
    let store = writer.owner.finish(false).unwrap();
    host.projects.insert("project".into(), store);
    assert_eq!(first(&host).revision, page.revision);
    assert_eq!(
        serde_json::to_value(host.workspace_query(&detail(&page.revision)).unwrap()).unwrap(),
        original
    );
    assert_eq!(
        before,
        crate::reader_generation::tests::snapshot(&host.root)
    );
}

#[test]
fn workspace_startup_pages_all_45_original_journal_records_without_writing() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path());
    let mut intent = host
        .startups
        .get("project", "startup-native")
        .unwrap()
        .intent
        .clone();
    intent.management_operation = None;
    intent.management_hash = None;
    intent.schema_version = 1;
    for n in 0..44 {
        intent.attempt_id = format!("bounded-startup-{n:02}");
        host.startups.begin(intent.clone()).unwrap();
    }
    let before = crate::reader_generation::tests::snapshot(&host.root);
    let page = first(&host);
    assert_eq!(page.total, 45);
    assert_eq!(page.entries.len(), 20);
    assert!(
        serde_json::to_vec(&WorkspaceResult::CodexStartups(page.clone()))
            .unwrap()
            .len()
            <= 64 * 1024
    );
    let mut entries = page.entries.clone();
    let mut offset = page.next_offset;
    while let Some(next) = offset {
        let WorkspaceResult::CodexStartups(next_page) = host
            .workspace_query(&list(next, Some(page.revision.clone())))
            .unwrap()
        else {
            panic!("page")
        };
        assert_eq!(next_page.revision, page.revision);
        assert!(next_page.entries.len() <= 20);
        assert!(
            serde_json::to_vec(&WorkspaceResult::CodexStartups(next_page.clone()))
                .unwrap()
                .len()
                <= 64 * 1024
        );
        offset = next_page.next_offset;
        entries.extend(next_page.entries);
    }
    assert_eq!(entries.len(), 45);
    assert!(entries
        .windows(2)
        .all(|p| p[0].attempt_id < p[1].attempt_id));
    assert!(host
        .workspace_query(&list(46, Some(page.revision.clone())))
        .is_err());
    assert!(host
        .workspace_query(&list(20, Some("0".repeat(64))))
        .is_err());
    assert_eq!(
        before,
        crate::reader_generation::tests::snapshot(&host.root)
    );
}

#[test]
fn workspace_startup_json_page_and_detail_limits_preserve_original_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let host = fixture(temp.path());
    let page = first(&host);
    let WorkspaceResult::CodexStartup(mut original) =
        host.workspace_query(&detail(&page.revision)).unwrap()
    else {
        panic!("detail")
    };
    let mut low = 0usize;
    let mut high = crate::workspace_read::RESULT_BYTES;
    while low + 1 < high {
        let n = low + (high - low) / 2;
        original.detail.blocked_reason = Some("x".repeat(n));
        if serde_json::to_vec(&WorkspaceResult::CodexStartup(original.clone()))
            .unwrap()
            .len()
            <= crate::workspace_read::RESULT_BYTES
        {
            low = n;
        } else {
            high = n;
        }
    }
    original.detail.blocked_reason = Some("x".repeat(low));
    let result = WorkspaceResult::CodexStartup(original.clone());
    assert_eq!(
        serde_json::to_vec(&result).unwrap().len(),
        crate::workspace_read::RESULT_BYTES
    );
    assert!(crate::workspace_read::bounded(result).is_ok());
    original.detail.blocked_reason = Some("x".repeat(low + 1));
    assert!(crate::workspace_read::bounded(WorkspaceResult::CodexStartup(original)).is_err());
    let mut summary = page.entries[0].clone();
    summary.management_operation = Some("x".repeat(40 * 1024));
    let (entries, next) = crate::workspace_read::page(
        vec![summary.clone(), summary.clone()],
        2,
        0,
        None,
        &page.revision,
    )
    .unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(next, Some(1));
    summary.management_operation = Some("x".repeat(64 * 1024));
    assert!(crate::workspace_read::page(vec![summary], 1, 0, None, &page.revision).is_err());
}

#[test]
fn workspace_startup_missing_legacy_and_faulted_owner_are_not_empty_success() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path());
    let page = first(&host);
    let missing = WorkspaceQuery::CodexStartups {
        project_id: "missing-project".into(),
        offset: 0,
        revision: None,
    };
    assert!(host.workspace_query(&missing).is_err());
    let mut legacy = host.codex_jobs[&("project".into(), "startup-native".into())].clone();
    legacy.attempt_id = "legacy-no-journal".into();
    host.codex_jobs
        .insert(("project".into(), legacy.attempt_id.clone()), legacy);
    assert!(host.workspace_query(&list(0, None)).is_err());
    assert!(host.workspace_query(&detail(&page.revision)).is_err());
    host.codex_jobs
        .remove(&("project".into(), "legacy-no-journal".into()));
    host.api_fault = Some("controlled original publication acknowledgement loss".into());
    assert!(host.workspace_query(&list(0, None)).is_err());
    assert!(host.workspace_query(&detail(&page.revision)).is_err());
    assert_eq!(
        host.codex_jobs[&("project".into(), "startup-native".into())].state,
        CodexJobState::Unknown
    );
}
