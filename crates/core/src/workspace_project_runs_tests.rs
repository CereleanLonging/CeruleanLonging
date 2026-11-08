//! Original local run-control records through the bounded read path; no workers or providers.
use super::project_runs::original_basis;
use super::*;
use cerulean_domain::management::ManagementDecision;
use cerulean_domain::project_setup::{ProjectRun, RunState};
use sha2::{Digest, Sha256};

fn list(offset: u32, revision: Option<String>) -> WorkspaceQuery {
    WorkspaceQuery::ProjectRuns {
        project_id: "project".into(),
        offset,
        revision,
    }
}
fn detail(run_id: &str, revision: &str) -> WorkspaceQuery {
    WorkspaceQuery::ProjectRun {
        project_id: "project".into(),
        run_id: run_id.into(),
        revision: revision.into(),
    }
}
fn first(host: &RuntimeHost) -> WorkspaceProjectRunPage {
    let WorkspaceResult::ProjectRuns(page) = host.workspace_query(&list(0, None)).unwrap() else {
        panic!("run page")
    };
    page
}
fn record(host: &RuntimeHost, id: &str, revision: &str) -> ProjectRun {
    let WorkspaceResult::ProjectRun(found) = host.workspace_query(&detail(id, revision)).unwrap()
    else {
        panic!("run detail")
    };
    assert_eq!(found.project_id, "project");
    assert_eq!(found.revision, revision);
    found.record
}
fn original_hashes(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    crate::reader_generation::tests::snapshot(root)
        .into_iter()
        .map(|(path, bytes)| (path, Sha256::digest(bytes).to_vec()))
        .collect()
}

// Each terminal historical run has its own approved revision and authenticated
// revision request. No fabricated completed outcomes or bypassed ledger validation.
fn fixture(root: &Path, count: usize) -> RuntimeHost {
    let (mut host, draft) = project_planning::tests::fixture(root);
    host.configure_project(draft).unwrap();
    for n in 0..count.max(1) {
        let goal = format!("Original local run revision {n}");
        let mut decision = project_planning::tests::proposal("tests");
        if let ManagementDecision::Plan { tasks, .. } = &mut decision {
            tasks[0].goal = goal;
        }
        project_planning::tests::complete_at(
            &mut host,
            decision,
            &format!("ui-plan-bounded-run-plan-{n:04}"),
        );
        let revision = host.projects["project"]
            .revision()
            .unwrap()
            .approval_digest()
            .unwrap();
        host.approve("project", &revision).unwrap();
        if count == 0 {
            break;
        }
        let run = ProjectRun {
            id: format!("bounded-run-{n:04}"),
            project_id: "project".into(),
            revision_digest: revision,
            setup_digest: host.project_setups["project"].digest.clone(),
            state: RunState::Active,
            message: "Original durable authorization ✓ ".repeat(60),
            acknowledged_decisions: n as u32,
        };
        host.runs.save(run, "user").unwrap();
        if n + 1 < count {
            let review = host
                .project_revision_candidates()
                .into_iter()
                .find(|review| review.project_id == "project")
                .unwrap();
            host.request_project_revision(
                "project",
                &review.evidence_digest,
                "Original local historical revision",
            )
            .unwrap();
            assert_eq!(host.runs.all().last().unwrap().state, RunState::Superseded);
        }
    }
    host.validate_runs().unwrap();
    host
}

#[test]
fn workspace_project_runs_original_45_pages_and_full_messages_without_writes() {
    let temp = tempfile::tempdir().unwrap();
    let host = fixture(temp.path(), 45);
    let before = original_hashes(&host.root);
    let page = first(&host);
    assert_eq!(page.project_id, "project");
    assert_eq!(page.total, 45);
    assert_eq!(page.active_count, 1);
    assert_eq!(page.entries.len(), 20);
    let mut entries = page.entries.clone();
    let mut next = page.next_offset;
    while let Some(offset) = next {
        let result = host
            .workspace_query(&list(offset, Some(page.revision.clone())))
            .unwrap();
        assert!(serde_json::to_vec(&result).unwrap().len() <= crate::workspace_read::RESULT_BYTES);
        let WorkspaceResult::ProjectRuns(part) = result else {
            panic!("page")
        };
        assert_eq!(part.revision, page.revision);
        assert_eq!(part.total, 45);
        assert_eq!(part.active_count, 1);
        assert_eq!(part.offset, offset);
        assert!(!part.entries.is_empty() && part.entries.len() <= 20);
        next = part.next_offset;
        entries.extend(part.entries);
    }
    let originals = host.runs.all();
    assert_eq!(entries.len(), originals.len());
    for (entry, original) in entries.iter().zip(&originals) {
        assert_eq!(entry.id, original.id);
        assert_eq!(entry.project_id, original.project_id);
        assert_eq!(entry.revision_digest, original.revision_digest);
        assert_eq!(entry.setup_digest, original.setup_digest);
        assert_eq!(entry.state, original.state);
        assert_eq!(
            entry.acknowledged_decisions,
            original.acknowledged_decisions
        );
        assert!(entry.has_message);
        let result = host
            .workspace_query(&detail(&entry.id, &page.revision))
            .unwrap();
        assert!(serde_json::to_vec(&result).unwrap().len() <= crate::workspace_read::RESULT_BYTES);
        assert_eq!(
            serde_json::to_value(record(&host, &entry.id, &page.revision)).unwrap(),
            serde_json::to_value(original).unwrap()
        );
    }
    assert!(
        record(&host, "bounded-run-0044", &page.revision)
            .message
            .chars()
            .count()
            > 160
    );
    let WorkspaceResult::ProjectRuns(end) = host
        .workspace_query(&list(45, Some(page.revision.clone())))
        .unwrap()
    else {
        panic!("end page")
    };
    assert!(end.entries.is_empty());
    assert!(end.next_offset.is_none());
    assert!(host
        .workspace_query(&list(46, Some(page.revision.clone())))
        .is_err());
    assert!(host.workspace_query(&list(20, None)).is_err());
    assert_eq!(before, original_hashes(&host.root));
}

#[test]
fn workspace_project_runs_empty_and_cursor_identity_stale_missing_fail_closed() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), 0);
    let empty = first(&host);
    assert_eq!(empty.total, 0);
    assert_eq!(empty.active_count, 0);
    assert!(empty.entries.is_empty());
    assert!(empty.next_offset.is_none());
    let before = original_hashes(&host.root);
    for query in [
        list(1, None),
        list(1, Some(empty.revision.clone())),
        list(0, Some("not-a-digest".into())),
        list(0, Some("0".repeat(64))),
        detail("missing-run", &empty.revision),
        detail("../invalid", &empty.revision),
        detail("missing-run", "not-a-digest"),
        WorkspaceQuery::ProjectRuns {
            project_id: "unknown".into(),
            offset: 0,
            revision: None,
        },
        WorkspaceQuery::ProjectRun {
            project_id: "unknown".into(),
            run_id: "missing-run".into(),
            revision: empty.revision.clone(),
        },
        WorkspaceQuery::ProjectRuns {
            project_id: "../invalid".into(),
            offset: 0,
            revision: None,
        },
    ] {
        assert!(host.workspace_query(&query).is_err(), "{query:?}");
    }
    assert_eq!(before, original_hashes(&host.root));
    host.runner_error = Some("original runtime failure".into());
    assert_ne!(first(&host).revision, empty.revision);
    assert!(host
        .workspace_query(&list(0, Some(empty.revision)))
        .is_err());
}

#[test]
fn workspace_project_runs_original_state_transitions_and_full_failure_stale_revision() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), 1);
    let page = first(&host);
    let id = "bounded-run-0000";
    let sequence = host.projects["project"].history().len();
    let before = original_hashes(&host.root);
    host.runner_error = Some(format!("{}first failure", "✓".repeat(200)));
    let failed = first(&host);
    assert_eq!(failed.active_count, 0);
    assert_eq!(failed.entries[0].state, RunState::Attention);
    assert_eq!(
        record(&host, id, &failed.revision).message,
        host.runner_error.clone().unwrap()
    );
    host.runner_error = Some(format!("{}second failure", "✓".repeat(200)));
    let changed = first(&host);
    assert_eq!(
        serde_json::to_value(&failed.entries).unwrap(),
        serde_json::to_value(&changed.entries).unwrap()
    );
    assert_ne!(changed.revision, failed.revision);
    assert!(host.workspace_query(&detail(id, &failed.revision)).is_err());
    assert!(host.workspace_query(&list(0, Some(page.revision))).is_err());
    assert_eq!(host.projects["project"].history().len(), sequence);
    assert_eq!(before, original_hashes(&host.root));
    host.runner_error = None;
    host.pause_project_run(id).unwrap();
    let paused = first(&host);
    assert_eq!(paused.active_count, 0);
    assert_eq!(record(&host, id, &paused.revision).state, RunState::Paused);
    assert!(host
        .workspace_query(&detail(id, &changed.revision))
        .is_err());
}

#[test]
fn workspace_project_runs_synthetic_page_byte_cap_is_not_durable_history() {
    // Synthetic serialization pressure, deliberately not claimed as ledger records.
    let entries = (0..21).map(|_| "✓".repeat(12_000));
    let revision = "a".repeat(64);
    let (page, next) = crate::workspace_read::page(entries, 21, 0, None, &revision).unwrap();
    assert!(!page.is_empty());
    assert!(page.len() < 20);
    assert_eq!(next, Some(page.len() as u32));
    assert!(serde_json::to_vec(&page).unwrap().len() < 64 * 1024);
    assert!(crate::workspace_read::page(["x".repeat(70 * 1024)], 1, 0, None, &revision).is_err());
}

#[test]
fn workspace_project_runs_oversize_projected_detail_refuses_without_truncation_or_writes() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), 1);
    // Runtime error projection is not a synthetic persisted run message: ledger
    // messages remain valid under their own independent 4096-byte limit.
    host.runner_error = Some("x".repeat(crate::workspace_read::RESULT_BYTES + 1));
    let before = original_hashes(&host.root);
    let page = first(&host);
    assert_eq!(page.entries.len(), 1);
    assert!(host
        .workspace_query(&detail("bounded-run-0000", &page.revision))
        .is_err());
    assert_eq!(before, original_hashes(&host.root));
}

#[test]
fn workspace_project_runs_shared_writer_before_during_after_ignores_cached_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), 1);
    let page = first(&host);
    let query = detail("bounded-run-0000", &page.revision);
    let original = serde_json::to_value(host.workspace_query(&query).unwrap()).unwrap();
    let before = original_hashes(&host.root);
    let basis = original_basis(&host.projects["project"], "project")
        .unwrap()
        .version;
    let store = host.projects.remove("project").unwrap();
    let writer =
        crate::async_api::ApiWorkflow::start(store, host.root.join("workflows/project")).unwrap();
    host.api_writers.insert("project".into(), writer);
    assert_eq!(
        host.api_writers["project"]
            .owner
            .workspace_project_runs_basis("project")
            .unwrap()
            .version,
        basis
    );
    assert_eq!(first(&host).revision, page.revision);
    host.api_writers
        .get_mut("project")
        .unwrap()
        .snapshot
        .sequence += 999;
    assert_eq!(first(&host).revision, page.revision);
    assert_eq!(
        serde_json::to_value(host.workspace_query(&query).unwrap()).unwrap(),
        original
    );
    host.api_writers.get_mut("project").unwrap().interrupted = true;
    assert!(host.workspace_query(&list(0, None)).is_err());
    assert!(host.workspace_query(&query).is_err());
    host.api_writers.get_mut("project").unwrap().interrupted = false;
    let mut writer = host.api_writers.remove("project").unwrap();
    host.projects
        .insert("project".into(), writer.owner.finish(false).unwrap());
    assert_eq!(first(&host).revision, page.revision);
    assert_eq!(
        serde_json::to_value(host.workspace_query(&query).unwrap()).unwrap(),
        original
    );
    assert_eq!(before, original_hashes(&host.root));
}

#[test]
fn workspace_project_runs_poisoned_durable_writer_and_shared_owner_refuse() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), 1);
    let page = first(&host);
    let query = detail("bounded-run-0000", &page.revision);
    let state = host.projects["project"].state().clone();
    let history = host.projects["project"].history().to_vec();
    let before = original_hashes(&host.root);
    let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        host.projects.get_mut("project").unwrap().command_observed(
            "run-reader-controlled-interruption",
            "user",
            WorkflowCommand::RequestRevision {
                digest: state.digest.clone().unwrap(),
                reason: "controlled interruption".into(),
            },
            CommandContext {
                approval_source: Some("authenticated local-user revision request".into()),
                ..Default::default()
            },
            true,
            |point| {
                if point == crate::scheduler::CommitPoint::BeforeEventWrite {
                    panic!("controlled interruption");
                }
            },
        )
    }));
    assert!(interrupted.is_err());
    assert!(host.projects["project"].requires_reopen());
    assert_eq!(host.projects["project"].state(), &state);
    assert_eq!(host.projects["project"].history(), history);
    assert!(host.workspace_query(&list(0, None)).is_err());
    assert!(host.workspace_query(&query).is_err());
    let store = host.projects.remove("project").unwrap();
    let writer =
        crate::async_api::ApiWorkflow::start(store, host.root.join("workflows/project")).unwrap();
    host.api_writers.insert("project".into(), writer);
    assert!(host.workspace_query(&list(0, None)).is_err());
    assert!(host.workspace_query(&query).is_err());
    assert_eq!(before, original_hashes(&host.root));
}

#[test]
fn workspace_project_runs_service_normal_reopen_preserves_original_sha() {
    let temp = tempfile::tempdir().unwrap();
    let host = fixture(temp.path(), 23);
    let page = first(&host);
    let query = detail("bounded-run-0022", &page.revision);
    let original = serde_json::to_value(host.workspace_query(&query).unwrap()).unwrap();
    let root = host.root.clone();
    let before = original_hashes(&root);
    drop(host);
    let service = crate::diagnostic_service::ServiceHost::open(&root).unwrap();
    assert!(!service.is_diagnostic());
    let WorkspaceResult::ProjectRuns(reopened) = service.workspace_query(&list(0, None)).unwrap()
    else {
        panic!("page")
    };
    assert_eq!(reopened.revision, page.revision);
    assert_eq!(reopened.total, 23);
    assert_eq!(
        serde_json::to_value(service.workspace_query(&query).unwrap()).unwrap(),
        original
    );
    assert_eq!(before, original_hashes(&root));
}

#[test]
fn workspace_project_runs_diagnostic_service_refuses_without_writes() {
    let temp = tempfile::tempdir().unwrap();
    let host = fixture(temp.path(), 1);
    let page = first(&host);
    let root = host.root.clone();
    drop(host);
    let damaged = root.join("desktop-task-dispatch-v1").join("a".repeat(64));
    fs::create_dir_all(&damaged).unwrap();
    fs::write(
        damaged.join("intent.pending"),
        br#"{"private-marker":"never expose","unfinished":"#,
    )
    .unwrap();
    let before = original_hashes(&root);
    let service = crate::diagnostic_service::ServiceHost::open(&root).unwrap();
    assert!(service.is_diagnostic());
    for query in [list(0, None), detail("bounded-run-0000", &page.revision)] {
        assert_eq!(
            service.workspace_query(&query).unwrap_err(),
            "项目记录处于诊断状态，清单暂不可用；不代表没有项目"
        );
    }
    assert_eq!(before, original_hashes(&root));
}

#[test]
fn workspace_project_runs_streamed_basis_matches_original_and_full_same_sequence_state() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), 1);
    let store = &host.projects["project"];
    let expected = cerulean_domain::execution::digest(&(
        "workspace-project-runs-original-basis-v1",
        "project",
        store.configuration(),
        store.state(),
        store.revision(),
        store.approval(),
        store.history(),
    ))
    .unwrap();
    let basis = original_basis(store, "project").unwrap().version;
    assert_eq!(basis, expected);
    assert!(original_basis(store, "other-project").is_err());
    let sequence = store.history().len();
    let page = first(&host);
    let before = original_hashes(&host.root);
    // Deliberate in-memory pressure via a real domain command: this is not a
    // persisted user revision request or a fabricated historical run record.
    host.projects
        .get_mut("project")
        .unwrap()
        .request_revision_state_for_workspace_test(
            "project",
            "same-sequence original state pressure",
        )
        .unwrap();
    let changed = first(&host);
    assert_eq!(host.projects["project"].history().len(), sequence);
    assert_eq!(
        serde_json::to_value(&page.entries).unwrap(),
        serde_json::to_value(&changed.entries).unwrap()
    );
    assert_ne!(
        original_basis(&host.projects["project"], "project")
            .unwrap()
            .version,
        basis
    );
    assert_ne!(changed.revision, page.revision);
    assert!(host
        .workspace_query(&list(0, Some(page.revision.clone())))
        .is_err());
    assert!(host
        .workspace_query(&detail("bounded-run-0000", &page.revision))
        .is_err());
    assert_eq!(
        record(&host, "bounded-run-0000", &changed.revision).state,
        RunState::Active
    );
    assert_eq!(before, original_hashes(&host.root));
}

#[test]
fn workspace_project_runs_poisoned_run_ledger_refuses_page_and_detail_without_writes() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), 1);
    let page = first(&host);
    let before = original_hashes(&host.root);
    host.runs.poison_for_test();
    assert!(!host.runs.healthy());
    assert!(host.workspace_query(&list(0, None)).is_err());
    assert!(host
        .workspace_query(&detail("bounded-run-0000", &page.revision))
        .is_err());
    assert_eq!(before, original_hashes(&host.root));
}

#[test]
fn workspace_project_runs_nonshared_writer_refuses_instead_of_using_cached_snapshot() {
    use cerulean_domain::{
        execution::{ValidationFile, WorkerSelection},
        CodexJob, CodexJobState,
    };
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), 1);
    let page = first(&host);
    let query = detail("bounded-run-0000", &page.revision);
    let before = original_hashes(&host.root);
    let snapshot = crate::async_verification::snapshot(&host.projects["project"]);
    let (sender, receiver) = std::sync::mpsc::channel();
    let (advice_wake, _advice_receiver) = tokio::sync::watch::channel(());
    let key = ("project".to_owned(), "inert-nonshared-owner".to_owned());
    // An inert ownership record, not a dispatched child, native process, or model
    // call. Its cached snapshot must never stand in for a readable durable owner.
    host.codex_active.insert(
        key.clone(),
        crate::async_codex::ActiveCodex {
            preparation_cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            advice_wake,
            advice_gate: std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0)),
            observation: None,
            observation_saved: None,
            observation_error: None,
            snapshot,
            job: CodexJob {
                management_operation: None,
                project_id: "project".into(),
                task_id: "task".into(),
                attempt_id: key.1.clone(),
                revision_digest: page.entries[0].revision_digest.clone(),
                worker: WorkerSelection {
                    reasoning_effort: None,
                    omp_runtime_parent: None,
                    provider: "fixture".into(),
                    model: "fixture".into(),
                    executable: ValidationFile {
                        path: "fixture.exe".into(),
                        sha256: "a".repeat(64),
                    },
                },
                state: CodexJobState::Unknown,
                started_at: "2026-10-15T00:00:00Z".into(),
                updated_at: "2026-10-15T00:00:00Z".into(),
                message: Some("Controlled inert nonshared ownership".into()),
            },
            receiver,
            disconnected: false,
            cancel: None,
        },
    );
    assert!(host.workspace_query(&list(0, None)).is_err());
    assert!(host.workspace_query(&query).is_err());
    host.codex_active.get_mut(&key).unwrap().snapshot.sequence += 999;
    assert!(host.workspace_query(&list(0, None)).is_err());
    assert!(host.workspace_query(&query).is_err());
    host.codex_active.remove(&key).unwrap();
    drop(sender);
    assert_eq!(first(&host).revision, page.revision);
    assert_eq!(
        record(&host, "bounded-run-0000", &page.revision).state,
        RunState::Active
    );
    assert_eq!(before, original_hashes(&host.root));
}
