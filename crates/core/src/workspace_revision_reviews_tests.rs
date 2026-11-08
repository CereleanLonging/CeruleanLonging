//! Original selected-project revision reviews; no dispatched workers or providers.
use super::revision_reviews::original_basis;
use super::*;
use crate::workflow_access::WorkflowAccess;
use cerulean_domain::management::{ManagementDecision, Purpose};
use cerulean_domain::project_setup::{ProjectRevisionReview, ProjectRun, RunState};
use cerulean_domain::workspace::{WorkspaceRevisionReviewDetail, WorkspaceRevisionReviewPage};
use sha2::{Digest, Sha256};

fn list(offset: u32, revision: Option<String>) -> WorkspaceQuery {
    WorkspaceQuery::RevisionReviews {
        project_id: "project".into(),
        offset,
        revision,
    }
}
fn detail(revision: &str) -> WorkspaceQuery {
    WorkspaceQuery::RevisionReview {
        project_id: "project".into(),
        revision: revision.into(),
    }
}
fn first(host: &RuntimeHost) -> WorkspaceRevisionReviewPage {
    let WorkspaceResult::RevisionReviews(page) = host.workspace_query(&list(0, None)).unwrap()
    else {
        panic!("revision review page")
    };
    page
}
fn record(host: &RuntimeHost, revision: &str) -> ProjectRevisionReview {
    let WorkspaceResult::RevisionReview(found) = host.workspace_query(&detail(revision)).unwrap()
    else {
        panic!("revision review detail")
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
fn same_review(actual: &ProjectRevisionReview, expected: &ProjectRevisionReview) {
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
}
fn unavailable(host: &RuntimeHost, expected_reason: &str) -> WorkspaceRevisionReviewPage {
    let page = first(host);
    assert_eq!(page.project_id, "project");
    assert_eq!(page.offset, 0);
    assert_eq!(page.total, 0);
    assert!(page.entries.is_empty());
    assert!(page.next_offset.is_none());
    assert_eq!(page.unavailable_reason.as_deref(), Some(expected_reason));
    assert!(host.workspace_query(&detail(&page.revision)).is_err());
    page
}

// Each replacement is an authenticated local-user request against the original
// approved plan. No completed outcomes or historical authority are fabricated.
fn fixture(root: &Path, historical_revisions: usize) -> RuntimeHost {
    let (mut host, draft) = project_planning::tests::fixture(root);
    host.configure_project(draft).unwrap();
    for n in 0..=historical_revisions {
        let mut decision = project_planning::tests::proposal("tests");
        if let ManagementDecision::Plan { tasks, .. } = &mut decision {
            tasks[0].goal = format!("Original selected-project revision {n}");
        }
        project_planning::tests::complete_at(
            &mut host,
            decision,
            &format!("ui-plan-review-fixture-{n:04}"),
        );
        let revision = host.projects["project"]
            .revision()
            .unwrap()
            .approval_digest()
            .unwrap();
        host.approve("project", &revision).unwrap();
        host.runs
            .save(
                ProjectRun {
                    id: format!("review-run-{n:04}"),
                    project_id: "project".into(),
                    revision_digest: revision,
                    setup_digest: host.project_setups["project"].digest.clone(),
                    state: RunState::Active,
                    message: "Original local approved authorization".into(),
                    acknowledged_decisions: 0,
                },
                "user",
            )
            .unwrap();
        if n < historical_revisions {
            let review = host
                .project_revision_candidates()
                .into_iter()
                .find(|review| review.project_id == "project")
                .unwrap();
            host.request_project_revision("project", &review.evidence_digest, &"r".repeat(7900))
                .unwrap();
            assert_eq!(host.runs.all().last().unwrap().state, RunState::Superseded);
        }
    }
    host.validate_runs().unwrap();
    host
}

#[test]
fn workspace_revision_reviews_original_candidate_equality_bounds_and_read_only_sha() {
    let temp = tempfile::tempdir().unwrap();
    let host = fixture(temp.path(), 2);
    let originals = host.project_revision_candidates();
    let original = originals
        .iter()
        .find(|review| review.project_id == "project")
        .unwrap();
    let before = original_hashes(&host.root);
    let page = first(&host);
    assert_eq!(page.project_id, "project");
    assert_eq!(page.offset, 0);
    assert_eq!(page.total, 1);
    assert_eq!(page.entries.len(), 1);
    assert!(page.next_offset.is_none());
    assert!(page.unavailable_reason.is_none());
    same_review(&page.entries[0], original);
    same_review(&record(&host, &page.revision), original);
    assert_eq!(page.entries[0].evidence_digest, original.evidence_digest);
    for query in [list(0, Some(page.revision.clone())), detail(&page.revision)] {
        let result = host.workspace_query(&query).unwrap();
        assert!(serde_json::to_vec(&result).unwrap().len() <= crate::workspace_read::RESULT_BYTES);
    }
    let WorkspaceResult::RevisionReviews(end) = host
        .workspace_query(&list(1, Some(page.revision.clone())))
        .unwrap()
    else {
        panic!("end page")
    };
    assert_eq!(end.offset, 1);
    assert_eq!(end.total, 1);
    assert!(end.entries.is_empty());
    assert!(end.next_offset.is_none());
    assert!(end.unavailable_reason.is_none());
    assert_eq!(before, original_hashes(&host.root));
}

#[test]
fn workspace_revision_reviews_unknown_invalid_stale_cursor_and_offsets_fail_closed() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), 0);
    let page = first(&host);
    let before = original_hashes(&host.root);
    for query in [
        list(1, None),
        list(2, Some(page.revision.clone())),
        list(u32::MAX, Some(page.revision.clone())),
        list(0, Some("not-a-digest".into())),
        list(0, Some("0".repeat(64))),
        detail("not-a-digest"),
        detail(&"0".repeat(64)),
        WorkspaceQuery::RevisionReviews {
            project_id: "unknown".into(),
            offset: 0,
            revision: None,
        },
        WorkspaceQuery::RevisionReview {
            project_id: "unknown".into(),
            revision: page.revision.clone(),
        },
        WorkspaceQuery::RevisionReviews {
            project_id: "../invalid".into(),
            offset: 0,
            revision: None,
        },
        WorkspaceQuery::RevisionReview {
            project_id: "../invalid".into(),
            revision: page.revision.clone(),
        },
    ] {
        assert!(host.workspace_query(&query).is_err(), "{query:?}");
    }
    assert_eq!(before, original_hashes(&host.root));
    let original = record(&host, &page.revision);
    host.request_project_revision(
        "project",
        &original.evidence_digest,
        "Original user replacement",
    )
    .unwrap();
    let changed = unavailable(&host, "请先完成或处置已派发任务，再修订完整方案");
    assert_ne!(changed.revision, page.revision);
    assert!(host
        .workspace_query(&list(0, Some(page.revision.clone())))
        .is_err());
    assert!(host.workspace_query(&detail(&page.revision)).is_err());
    assert!(host
        .workspace_query(&list(1, Some(changed.revision)))
        .is_err());
}

#[test]
fn workspace_revision_reviews_unapproved_and_pending_management_have_explicit_original_reasons() {
    let temp = tempfile::tempdir().unwrap();
    let (mut host, draft) = project_planning::tests::fixture(temp.path());
    host.configure_project(draft).unwrap();
    let before = original_hashes(&host.root);
    unavailable(&host, "请先完成或处置已派发任务，再修订完整方案");
    assert_eq!(before, original_hashes(&host.root));
    project_planning::tests::complete(&mut host, project_planning::tests::proposal("tests"));
    unavailable(&host, "请先完成或处置已派发任务，再修订完整方案");
    let digest = host.projects["project"]
        .revision()
        .unwrap()
        .approval_digest()
        .unwrap();
    host.approve("project", &digest).unwrap();
    assert_eq!(first(&host).total, 1);
    let context = host
        .management_context("project", &host.project_setups["project"].setup.requirement)
        .unwrap();
    let state_digest = cerulean_domain::execution::digest(&context).unwrap();
    host.management
        .begin(crate::management_execution::DecisionIntent {
            operation_id: "review-pending-management".into(),
            project_id: "project".into(),
            purpose: Purpose::Plan,
            model: "fixture-chief".into(),
            state_digest,
            context,
            created_at: "2026-10-15T00:00:00Z".into(),
        })
        .unwrap();
    assert!(host.project_revision_candidates().is_empty());
    let before = original_hashes(&host.root);
    unavailable(&host, "项目有待处理指挥请求，请先处置");
    assert_eq!(before, original_hashes(&host.root));
}

fn scope_and_attempt(host: &RuntimeHost) -> (crate::workflow_coordinator::Scope, Attempt) {
    let revision = host.projects["project"].revision().unwrap();
    let digest = revision.approval_digest().unwrap();
    let attempt = Attempt {
        workspace_proof: None,
        id: "review-writer-attempt".into(),
        task_id: "task".into(),
        worker_id: "worker-codex".into(),
        revision_digest: digest.clone(),
        fence: 1,
        binding: SessionBinding {
            adapter_id: "codex".into(),
            adapter_version: "fixture".into(),
            session_id: "review-writer-session".into(),
            identity_strength: IdentityStrength::ProviderId,
            workspace: revision.project.root.clone(),
        },
    };
    (
        crate::workflow_coordinator::Scope {
            project: "project".into(),
            task: "task".into(),
            attempt: attempt.id.clone(),
            revision: digest,
        },
        attempt,
    )
}
fn assign(
    client: &mut crate::workflow_coordinator::WorkflowClient,
    attempt: Attempt,
) -> Result<crate::scheduler::Entry, String> {
    client.command(
        "review-writer-assignment",
        "deputy",
        WorkflowCommand::Assign {
            task: attempt.task_id.clone(),
            worker: attempt.worker_id.clone(),
            attempt: attempt.id.clone(),
        },
        CommandContext {
            execution: Some(ExecutionEvidence::Attempt { attempt }),
            ..Default::default()
        },
        true,
    )
}

#[test]
fn workspace_revision_reviews_shared_owner_before_during_after_ignores_cached_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), 0);
    let page = first(&host);
    let original = record(&host, &page.revision);
    let basis = original_basis(&host.projects["project"], "project")
        .unwrap()
        .version;
    let before = original_hashes(&host.root);
    let store = host.projects.remove("project").unwrap();
    let writer =
        crate::async_api::ApiWorkflow::start(store, host.root.join("workflows/project")).unwrap();
    host.api_writers.insert("project".into(), writer);
    assert_eq!(
        host.api_writers["project"]
            .owner
            .workspace_revision_reviews_basis("project")
            .unwrap()
            .version,
        basis
    );
    // The original control refuses while busy, but the bounded reader still
    // reads the actual owner to bind its explicit unavailable result.
    assert!(host.project_revision_candidates().is_empty());
    let busy = unavailable(&host, "API尝试执行中或需要核对；禁止重派");
    host.api_writers
        .get_mut("project")
        .unwrap()
        .snapshot
        .sequence += 999;
    let cached = unavailable(&host, "API尝试执行中或需要核对；禁止重派");
    assert_eq!(cached.revision, busy.revision);
    host.api_writers.get_mut("project").unwrap().interrupted = true;
    assert!(host.workspace_query(&list(0, None)).is_err());
    assert!(host.workspace_query(&detail(&busy.revision)).is_err());
    host.api_writers.get_mut("project").unwrap().interrupted = false;
    let mut writer = host.api_writers.remove("project").unwrap();
    host.projects
        .insert("project".into(), writer.owner.finish(false).unwrap());
    assert_eq!(first(&host).revision, page.revision);
    same_review(&record(&host, &page.revision), &original);
    assert_eq!(before, original_hashes(&host.root));
}

#[test]
fn workspace_revision_reviews_shared_owner_progress_changes_version_despite_busy_guard() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), 0);
    let initial = first(&host);
    let (scope, attempt) = scope_and_attempt(&host);
    let store = host.projects.remove("project").unwrap();
    let writer =
        crate::async_api::ApiWorkflow::start(store, host.root.join("workflows/project")).unwrap();
    host.api_writers.insert("project".into(), writer);
    let busy = unavailable(&host, "API尝试执行中或需要核对；禁止重派");
    let mut client = host.api_writers["project"].owner.client(scope).unwrap();
    assign(&mut client, attempt).unwrap();
    let before = original_hashes(&host.root);
    let progressed = unavailable(&host, "API尝试执行中或需要核对；禁止重派");
    assert_ne!(progressed.revision, busy.revision);
    assert!(host.workspace_query(&list(0, Some(busy.revision))).is_err());
    assert!(host.workspace_query(&detail(&initial.revision)).is_err());
    host.api_writers
        .get_mut("project")
        .unwrap()
        .snapshot
        .sequence += 999;
    assert_eq!(first(&host).revision, progressed.revision);
    assert_eq!(before, original_hashes(&host.root));
    drop(client);
    let mut writer = host.api_writers.remove("project").unwrap();
    host.projects
        .insert("project".into(), writer.owner.finish(false).unwrap());
    unavailable(&host, "请先完成或处置已派发任务，再修订完整方案");
}

#[test]
fn workspace_revision_reviews_interrupted_coordinator_refuses_and_reopens_published_original() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), 0);
    let (scope, attempt) = scope_and_attempt(&host);
    let store = host.projects.remove("project").unwrap();
    let writer =
        crate::async_api::ApiWorkflow::start(store, host.root.join("workflows/project")).unwrap();
    host.api_writers.insert("project".into(), writer);
    let busy = first(&host);
    let mut client = host.api_writers["project"].owner.client(scope).unwrap();
    host.api_writers["project"]
        .owner
        .interrupt_next_publication();
    assert!(assign(&mut client, attempt).is_err());
    let before = original_hashes(&host.root);
    assert!(host.workspace_query(&list(0, None)).is_err());
    assert!(host.workspace_query(&detail(&busy.revision)).is_err());
    assert_eq!(before, original_hashes(&host.root));
    drop(client);
    let mut writer = host.api_writers.remove("project").unwrap();
    host.projects
        .insert("project".into(), writer.owner.finish(false).unwrap());
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Assigned
    );
    let root = host.root.clone();
    drop(host);
    let host = RuntimeHost::open(&root).unwrap();
    let before = original_hashes(&root);
    unavailable(&host, "请先完成或处置已派发任务，再修订完整方案");
    assert_eq!(before, original_hashes(&root));
}

#[test]
fn workspace_revision_reviews_poisoned_durable_and_shared_owner_refuse_without_writes() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), 0);
    let page = first(&host);
    let state = host.projects["project"].state().clone();
    let before = original_hashes(&host.root);
    let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        host.projects.get_mut("project").unwrap().command_observed(
            "review-controlled-interruption",
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
    assert!(host.workspace_query(&list(0, None)).is_err());
    assert!(host.workspace_query(&detail(&page.revision)).is_err());
    let store = host.projects.remove("project").unwrap();
    let writer =
        crate::async_api::ApiWorkflow::start(store, host.root.join("workflows/project")).unwrap();
    host.api_writers.insert("project".into(), writer);
    assert!(host.workspace_query(&list(0, None)).is_err());
    assert!(host.workspace_query(&detail(&page.revision)).is_err());
    assert_eq!(before, original_hashes(&host.root));
}

#[test]
fn workspace_revision_reviews_poisoned_run_ledger_is_fault_not_empty_candidate() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), 0);
    let page = first(&host);
    let before = original_hashes(&host.root);
    host.runs.poison_for_test();
    assert!(host.workspace_query(&list(0, None)).is_err());
    assert!(host.workspace_query(&detail(&page.revision)).is_err());
    assert_eq!(before, original_hashes(&host.root));
}

#[test]
fn workspace_revision_reviews_nonshared_writer_refuses_instead_of_cached_snapshot() {
    use cerulean_domain::{
        execution::{ValidationFile, WorkerSelection},
        CodexJob, CodexJobState,
    };
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), 0);
    let page = first(&host);
    let before = original_hashes(&host.root);
    let snapshot = crate::async_verification::snapshot(&host.projects["project"]);
    let (sender, receiver) = std::sync::mpsc::channel();
    let (advice_wake, _advice_receiver) = tokio::sync::watch::channel(());
    let key = ("project".to_owned(), "review-inert-owner".to_owned());
    // Ownership-only fixture: no child process, dispatched task, or model call.
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
    assert!(host.workspace_query(&detail(&page.revision)).is_err());
    host.codex_active.get_mut(&key).unwrap().snapshot.sequence += 999;
    assert!(host.workspace_query(&list(0, None)).is_err());
    assert!(host.workspace_query(&detail(&page.revision)).is_err());
    host.codex_active.remove(&key).unwrap();
    drop(sender);
    assert_eq!(first(&host).revision, page.revision);
    assert_eq!(before, original_hashes(&host.root));
}

#[test]
fn workspace_revision_reviews_service_reopen_preserves_original_candidate_and_sha() {
    let temp = tempfile::tempdir().unwrap();
    let host = fixture(temp.path(), 2);
    let page = first(&host);
    let original =
        serde_json::to_value(host.workspace_query(&detail(&page.revision)).unwrap()).unwrap();
    let root = host.root.clone();
    let before = original_hashes(&root);
    drop(host);
    let service = crate::diagnostic_service::ServiceHost::open(&root).unwrap();
    assert!(!service.is_diagnostic());
    let WorkspaceResult::RevisionReviews(reopened) =
        service.workspace_query(&list(0, None)).unwrap()
    else {
        panic!("revision review page")
    };
    assert_eq!(reopened.revision, page.revision);
    assert_eq!(reopened.total, 1);
    same_review(&reopened.entries[0], &page.entries[0]);
    assert_eq!(
        serde_json::to_value(service.workspace_query(&detail(&page.revision)).unwrap()).unwrap(),
        original
    );
    assert_eq!(before, original_hashes(&root));
}

#[test]
fn workspace_revision_reviews_diagnostic_service_refuses_without_writes() {
    let temp = tempfile::tempdir().unwrap();
    let host = fixture(temp.path(), 0);
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
    for query in [list(0, None), detail(&page.revision)] {
        assert_eq!(
            service.workspace_query(&query).unwrap_err(),
            "项目记录处于诊断状态，清单暂不可用；不代表没有项目"
        );
    }
    assert_eq!(before, original_hashes(&root));
}

#[test]
fn workspace_revision_reviews_full_streamed_basis_and_same_sequence_state_are_bound() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), 0);
    let store = &host.projects["project"];
    let expected = cerulean_domain::execution::digest(&(
        "workspace-revision-reviews-original-basis-v1",
        "project",
        store.configuration(),
        store.state(),
        store.revision(),
        store.approval(),
        store.history(),
    ))
    .unwrap();
    let basis = original_basis(store, "project").unwrap();
    assert_eq!(basis.version, expected);
    assert_eq!(
        basis.last_hash.as_deref(),
        store.history().last().map(|entry| entry.hash.as_str())
    );
    let mut candidate = basis.candidate.unwrap();
    let page = first(&host);
    candidate.evidence_digest = page.entries[0].evidence_digest.clone();
    same_review(&candidate, &page.entries[0]);
    assert!(original_basis(store, "other-project").is_err());
    let sequence = store.history().len();
    let before = original_hashes(&host.root);
    // Real domain state transition without a fabricated durable event. The full
    // state, not merely sequence/tail or a cached snapshot, must bind the cursor.
    host.projects
        .get_mut("project")
        .unwrap()
        .request_revision_state_for_workspace_test(
            "project",
            "same-sequence original revision review state",
        )
        .unwrap();
    assert_eq!(host.projects["project"].history().len(), sequence);
    let changed = unavailable(&host, "请先完成或处置已派发任务，再修订完整方案");
    assert_ne!(changed.revision, page.revision);
    assert_ne!(
        original_basis(&host.projects["project"], "project")
            .unwrap()
            .version,
        expected
    );
    assert!(host
        .workspace_query(&list(0, Some(page.revision.clone())))
        .is_err());
    assert!(host.workspace_query(&detail(&page.revision)).is_err());
    assert_eq!(before, original_hashes(&host.root));
}

#[test]
fn workspace_revision_reviews_large_original_durable_basis_keeps_single_fixed_candidate() {
    let temp = tempfile::tempdir().unwrap();
    let host = fixture(temp.path(), 70);
    let store = &host.projects["project"];
    let encoded = serde_json::to_vec(&(
        "workspace-revision-reviews-original-basis-v1",
        "project",
        store.configuration(),
        store.state(),
        store.revision(),
        store.approval(),
        store.history(),
    ))
    .unwrap();
    assert!(encoded.len() > crate::workspace_read::RESULT_BYTES);
    let expected = format!("{:x}", Sha256::digest(&encoded));
    assert_eq!(original_basis(store, "project").unwrap().version, expected);
    let original = host
        .project_revision_candidates()
        .into_iter()
        .find(|review| review.project_id == "project")
        .unwrap();
    let before = original_hashes(&host.root);
    let page = first(&host);
    assert_eq!(page.total, 1);
    assert_eq!(page.entries.len(), 1);
    same_review(&page.entries[0], &original);
    same_review(&record(&host, &page.revision), &original);
    for query in [list(0, None), detail(&page.revision)] {
        assert!(
            serde_json::to_vec(&host.workspace_query(&query).unwrap())
                .unwrap()
                .len()
                < 4096
        );
    }
    assert_eq!(before, original_hashes(&host.root));
}

#[test]
fn workspace_revision_reviews_generic_encoded_cap_refuses_without_claiming_oversize_candidate() {
    // A genuine revision candidate contains fixed-size digests and counters;
    // generic encoded-result pressure is not claimed as durable candidate data.
    let review = ProjectRevisionReview {
        project_id: "project".into(),
        revision_digest: "a".repeat(64),
        evidence_digest: "b".repeat(64),
        accepted_tasks: 0,
        cancelled_tasks: 0,
        retired_tasks: 0,
        pending_tasks: 1,
        submitted_tasks: 0,
        verified_submissions: 0,
    };
    let bounded = WorkspaceResult::RevisionReview(Box::new(WorkspaceRevisionReviewDetail {
        project_id: "project".into(),
        revision: "c".repeat(64),
        record: review.clone(),
    }));
    assert!(crate::workspace_read::bounded(bounded).is_ok());
    let encoded_pressure = WorkspaceResult::RevisionReviews(WorkspaceRevisionReviewPage {
        project_id: "project".into(),
        revision: "c".repeat(64),
        offset: 0,
        total: 0,
        next_offset: None,
        entries: vec![],
        unavailable_reason: Some("\u{0001}".repeat(crate::workspace_read::RESULT_BYTES / 2)),
    });
    assert!(
        serde_json::to_vec(&encoded_pressure).unwrap().len() > crate::workspace_read::RESULT_BYTES
    );
    assert!(crate::workspace_read::bounded(encoded_pressure).is_err());
}

#[test]
fn workspace_revision_reviews_selected_project_does_not_merge_other_original_projects() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), 0);
    let original = first(&host);
    fs::create_dir(temp.path().join("project-two")).unwrap();
    host.register(Project {
        id: "project-two".into(),
        root: temp.path().join("project-two").to_string_lossy().into(),
        timezone: "Asia/Shanghai".into(),
    })
    .unwrap();
    let before = original_hashes(&host.root);
    let selected = first(&host);
    assert_eq!(selected.total, 1);
    assert_eq!(selected.entries.len(), 1);
    same_review(&selected.entries[0], &original.entries[0]);
    assert!(selected
        .entries
        .iter()
        .all(|entry| entry.project_id == "project"));
    let WorkspaceResult::RevisionReviews(other) = host
        .workspace_query(&WorkspaceQuery::RevisionReviews {
            project_id: "project-two".into(),
            offset: 0,
            revision: None,
        })
        .unwrap()
    else {
        panic!("other selected project page")
    };
    assert_eq!(other.project_id, "project-two");
    assert_eq!(other.total, 0);
    assert!(other.entries.is_empty());
    assert!(other.next_offset.is_none());
    assert!(other
        .unavailable_reason
        .as_ref()
        .is_some_and(|reason| !reason.is_empty()));
    assert_ne!(other.revision, selected.revision);
    assert!(host
        .workspace_query(&WorkspaceQuery::RevisionReview {
            project_id: "project-two".into(),
            revision: selected.revision.clone(),
        })
        .is_err());
    assert_eq!(before, original_hashes(&host.root));
}

#[test]
fn workspace_revision_reviews_full_run_message_and_setup_mutations_stale_original_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), 0);
    let initial = first(&host);
    let mut run = host
        .runs
        .all()
        .into_iter()
        .find(|run| run.project_id == "project")
        .unwrap();
    run.message = format!("{}first full original message", "m".repeat(300));
    host.runs.save(run.clone(), "user").unwrap();
    let first_message = first(&host);
    run.message = format!("{}second full original message", "m".repeat(300));
    host.runs.save(run, "user").unwrap();
    let before = original_hashes(&host.root);
    let second_message = first(&host);
    assert_ne!(first_message.revision, second_message.revision);
    assert_ne!(
        first_message.entries[0].evidence_digest,
        second_message.entries[0].evidence_digest
    );
    assert!(host.workspace_query(&detail(&initial.revision)).is_err());
    assert!(host
        .workspace_query(&detail(&first_message.revision))
        .is_err());
    same_review(
        &second_message.entries[0],
        &host.project_revision_candidates()[0],
    );
    assert_eq!(before, original_hashes(&host.root));
    // In-memory setup pressure is not claimed as a persisted user setup.
    let saved = host.project_setups.get_mut("project").unwrap();
    saved
        .setup
        .requirement
        .push_str(" Additional original setup requirement");
    saved.digest = cerulean_domain::execution::digest(&saved.setup).unwrap();
    let changed_setup = first(&host);
    assert_ne!(changed_setup.revision, second_message.revision);
    assert_ne!(
        changed_setup.entries[0].evidence_digest,
        second_message.entries[0].evidence_digest
    );
    same_review(
        &changed_setup.entries[0],
        &host.project_revision_candidates()[0],
    );
    assert!(host
        .workspace_query(&list(0, Some(second_message.revision.clone())))
        .is_err());
    assert!(host
        .workspace_query(&detail(&second_message.revision))
        .is_err());
    assert_eq!(before, original_hashes(&host.root));
}

#[test]
fn workspace_revision_reviews_poisoned_base_is_fault_not_unavailable_page() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), 0);
    let page = first(&host);
    let before = original_hashes(&host.root);
    host.base.poisoned = true;
    assert!(host.workspace_query(&list(0, None)).is_err());
    assert!(host.workspace_query(&detail(&page.revision)).is_err());
    assert_eq!(before, original_hashes(&host.root));
}

#[test]
fn workspace_revision_reviews_management_resolution_stales_same_sequence_original_review() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), 0);
    let initial = first(&host);
    let sequence = host.projects["project"].history().len();
    let context = host
        .management_context("project", &host.project_setups["project"].setup.requirement)
        .unwrap();
    let state_digest = cerulean_domain::execution::digest(&context).unwrap();
    let id = "review-resolution-fixture";
    host.management
        .begin(crate::management_execution::DecisionIntent {
            operation_id: id.into(),
            project_id: "project".into(),
            purpose: Purpose::Plan,
            model: "fixture-chief".into(),
            state_digest,
            context,
            created_at: "2026-10-15T00:00:00Z".into(),
        })
        .unwrap();
    let pending = unavailable(&host, "项目有待处理指挥请求，请先处置");
    let reviewed = host.management.evidence_digest(id).unwrap().to_owned();
    host.management
        .resolve(
            id,
            &reviewed,
            crate::management_execution::UserResolution::Discard {
                reason: "Explicit local disposition of original unresolved call".into(),
            },
        )
        .unwrap();
    let before = original_hashes(&host.root);
    let resolved = first(&host);
    assert_eq!(host.projects["project"].history().len(), sequence);
    assert_eq!(resolved.total, 1);
    assert!(resolved.unavailable_reason.is_none());
    same_review(&resolved.entries[0], &initial.entries[0]);
    assert_ne!(resolved.revision, initial.revision);
    assert_ne!(resolved.revision, pending.revision);
    for old in [&initial.revision, &pending.revision] {
        assert!(host.workspace_query(&list(0, Some(old.clone()))).is_err());
        assert!(host.workspace_query(&detail(old)).is_err());
    }
    same_review(&record(&host, &resolved.revision), &initial.entries[0]);
    assert_eq!(before, original_hashes(&host.root));
}

#[test]
fn workspace_revision_reviews_capability_job_changes_stale_same_unavailable_reason() {
    use cerulean_domain::{
        execution::{ValidationFile, WorkerSelection},
        CodexJob, CodexJobState,
    };
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), 0);
    let initial = first(&host);
    let key = ("project".to_owned(), "review-capability-job".to_owned());
    // Controlled unresolved capability record, not a fabricated execution
    // outcome or a dispatched process. Its full payload must bind the cursor.
    host.codex_jobs.insert(
        key.clone(),
        CodexJob {
            management_operation: None,
            project_id: "project".into(),
            task_id: "task".into(),
            attempt_id: key.1.clone(),
            revision_digest: initial.entries[0].revision_digest.clone(),
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
            message: Some(format!("{}first unresolved observation", "x".repeat(300))),
        },
    );
    let reason = "项目执行结果待对账，禁止重派";
    let pending = unavailable(&host, reason);
    host.codex_jobs.get_mut(&key).unwrap().message =
        Some(format!("{}second unresolved observation", "x".repeat(300)));
    let before = original_hashes(&host.root);
    let changed = unavailable(&host, reason);
    assert_ne!(pending.revision, changed.revision);
    assert!(host
        .workspace_query(&list(0, Some(pending.revision.clone())))
        .is_err());
    assert!(host.workspace_query(&detail(&pending.revision)).is_err());
    assert_eq!(before, original_hashes(&host.root));
}
