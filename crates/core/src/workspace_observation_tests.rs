//! Controlled in-memory DTO boundaries, not valid persisted digest records or model output.
use cerulean_domain::{worker_observation::*, workspace::*};

fn metadata(thread_id: String) -> WorkspaceResult {
    WorkspaceResult::WorkerObservation(Box::new(WorkspaceObservationDetail {
        project_id: "project".into(),
        task_id: "task".into(),
        attempt_id: "attempt".into(),
        revision: "revision".into(),
        revision_digest: "a".repeat(64),
        evidence_digest: "b".repeat(64),
        storage_error: None,
        supervision: None,
        thread_id: Some(thread_id),
        turn_id: Some("turn".into()),
        dropped: 0,
        rejected: 0,
        duplicates: 0,
        finished: true,
        event_count: 45,
        incident_count: 0,
    }))
}

#[test]
fn workspace_observation_boundary_metadata_exact_encoded_limit() {
    // The helper must enforce actual JSON bytes, including six-byte escapes.
    let mut low = 0;
    let mut high = crate::workspace_read::RESULT_BYTES;
    while low < high {
        let middle = low + (high - low).div_ceil(2);
        if serde_json::to_vec(&metadata("\0".repeat(middle)))
            .unwrap()
            .len()
            <= crate::workspace_read::RESULT_BYTES
        {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    let base = metadata("\0".repeat(low));
    let remaining = crate::workspace_read::RESULT_BYTES - serde_json::to_vec(&base).unwrap().len();
    let exact = metadata(format!("{}{}", "\0".repeat(low), "x".repeat(remaining)));
    assert_eq!(
        serde_json::to_vec(&exact).unwrap().len(),
        crate::workspace_read::RESULT_BYTES
    );
    assert!(crate::workspace_read::bounded(exact).is_ok());
    let oversized = metadata(format!("{}{}", "\0".repeat(low), "x".repeat(remaining + 1)));
    assert_eq!(
        serde_json::to_vec(&oversized).unwrap().len(),
        crate::workspace_read::RESULT_BYTES + 1
    );
    assert!(crate::workspace_read::bounded(oversized).is_err());
}

#[test]
fn workspace_observation_boundary_actual_escaped_pages_complete_and_readonly() {
    // Oversized synthetic digest strings exercise serialization budgets only;
    // these are intentionally NOT records accepted by worker_observation::read.
    let entries: Vec<_> = (1..=45)
        .map(|sequence| ToolObservation {
            sequence,
            received_at_ms: sequence as u64,
            item_digest: format!("{}-{sequence}", "\0".repeat(1400)),
            kind: ObservedToolKind::Command,
            status: ObservedToolStatus::Failed,
            exit_code: Some(7),
            action_digest: Some("a".repeat(64)),
        })
        .collect();
    let before = serde_json::to_vec(&entries).unwrap();
    let mut offset = 0;
    let mut collected = vec![];
    let mut page_count = 0;
    let revision = "a".repeat(64);
    let stale_revision = "b".repeat(64);
    loop {
        let (items, next_offset) = crate::workspace_read::page(
            entries.iter().cloned(),
            entries.len(),
            offset,
            Some(&revision),
            &revision,
        )
        .unwrap();
        assert!(!items.is_empty() && items.len() < 20);
        let page = WorkspaceObservationEventPage {
            project_id: "project".into(),
            attempt_id: "attempt".into(),
            revision: revision.clone(),
            offset,
            total: 45,
            next_offset,
            entries: items,
        };
        assert!(serde_json::to_vec(&page).unwrap().len() <= 64 * 1024);
        assert!(
            crate::workspace_read::bounded(WorkspaceResult::WorkerObservationEvents(page.clone()))
                .is_ok()
        );
        collected.extend(page.entries);
        page_count += 1;
        match next_offset {
            Some(next) => {
                assert_eq!(next as usize, collected.len());
                assert!(next > offset);
                offset = next;
            }
            None => break,
        }
    }
    assert!(page_count > 3);
    assert_eq!(collected, entries);
    assert_eq!(serde_json::to_vec(&entries).unwrap(), before);
    assert!(crate::workspace_read::page(
        entries.iter().cloned(),
        entries.len(),
        1,
        None,
        &revision
    )
    .is_err());
    assert!(crate::workspace_read::page(
        entries.iter().cloned(),
        entries.len(),
        0,
        Some(&stale_revision),
        &revision
    )
    .is_err());
    assert!(crate::workspace_read::page(
        entries.iter().cloned(),
        entries.len(),
        46,
        Some(&revision),
        &revision
    )
    .is_err());
    let huge = ToolObservation {
        item_digest: "\0".repeat(64 * 1024),
        ..entries[0].clone()
    };
    assert!(crate::workspace_read::page([huge], 1, 0, Some(&revision), &revision).is_err());
    let (empty, next) = crate::workspace_read::page(
        entries.iter().cloned(),
        entries.len(),
        45,
        Some(&revision),
        &revision,
    )
    .unwrap();
    assert!(empty.is_empty() && next.is_none());
}

#[cfg(windows)]
#[test]
fn workspace_observation_original_shared_owner_transfer_and_same_sequence_revision() {
    use crate::host::project_planning;
    use cerulean_domain::{
        execution::{ValidationFile, WorkerSelection},
        CodexJob, CodexJobState,
    };
    let temp = tempfile::tempdir().unwrap();
    let (mut host, draft) = project_planning::tests::fixture(temp.path());
    host.configure_project(draft).unwrap();
    project_planning::tests::complete(&mut host, project_planning::tests::proposal("tests"));
    let revision_digest = host.projects["project"]
        .revision()
        .unwrap()
        .approval_digest()
        .unwrap();
    host.approve("project", &revision_digest).unwrap();
    // Controlled historical status/view with real original revision evidence,
    // not a dispatched child or actual model product. Unknown holds stay unknown.
    let job = CodexJob {
        management_operation: None,
        project_id: "project".into(),
        task_id: "task".into(),
        attempt_id: "controlled-observation".into(),
        revision_digest,
        worker: WorkerSelection {
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
        message: Some("Controlled historical basis; unknown hold preserved".into()),
    };
    host.codex_jobs.insert(
        (job.project_id.clone(), job.attempt_id.clone()),
        job.clone(),
    );
    let trace = WorkerObservationTrace {
        finished: true,
        ..Default::default()
    };
    let view = crate::worker_observation::view(&job, trace.clone(), None, None).unwrap();
    let directory = host.root.join("worker-observations/project");
    std::fs::create_dir_all(&directory).unwrap();
    let file = directory.join(format!("{}.json", job.attempt_id));
    let original_bytes =
        serde_json::to_vec(&serde_json::json!({"schemaVersion": 1, "view": view})).unwrap();
    std::fs::write(&file, &original_bytes).unwrap();
    let query = |revision| WorkspaceQuery::WorkerObservation {
        project_id: job.project_id.clone(),
        attempt_id: job.attempt_id.clone(),
        revision,
    };
    let original = host.workspace_query(&query(None)).unwrap();
    let WorkspaceResult::WorkerObservation(detail) = &original else {
        panic!("detail")
    };
    let revision = detail.revision.clone();
    let events_query = WorkspaceQuery::WorkerObservationEvents {
        project_id: job.project_id.clone(),
        attempt_id: job.attempt_id.clone(),
        offset: 0,
        revision: revision.clone(),
    };
    let original_events =
        serde_json::to_value(host.workspace_query(&events_query).unwrap()).unwrap();
    let sequence = host.projects["project"].history().len();
    let before = crate::reader_generation::tests::snapshot(&host.root);
    let store = host.projects.remove("project").unwrap();
    let writer =
        crate::async_api::ApiWorkflow::start(store, host.root.join("workflows/project")).unwrap();
    host.api_writers.insert("project".into(), writer);
    assert_eq!(
        serde_json::to_value(
            host.workspace_query(&query(Some(revision.clone())))
                .unwrap()
        )
        .unwrap(),
        serde_json::to_value(&original).unwrap()
    );
    assert_eq!(
        serde_json::to_value(host.workspace_query(&events_query).unwrap()).unwrap(),
        original_events
    );
    let mut writer = host.api_writers.remove("project").unwrap();
    let store = writer.owner.finish(false).unwrap();
    host.projects.insert("project".into(), store);
    assert_eq!(
        serde_json::to_value(
            host.workspace_query(&query(Some(revision.clone())))
                .unwrap()
        )
        .unwrap(),
        serde_json::to_value(&original).unwrap()
    );
    assert_eq!(
        serde_json::to_value(host.workspace_query(&events_query).unwrap()).unwrap(),
        original_events
    );
    assert_eq!(host.projects["project"].history().len(), sequence);
    assert_eq!(
        crate::reader_generation::tests::snapshot(&host.root),
        before
    );
    assert!(matches!(
        host.codex_jobs[&(job.project_id.clone(), job.attempt_id.clone())].state,
        CodexJobState::Unknown
    ));
    let mut changed_trace = trace;
    changed_trace.duplicates = 1;
    let changed = crate::worker_observation::view(&job, changed_trace, None, None).unwrap();
    std::fs::write(
        &file,
        serde_json::to_vec(&serde_json::json!({"schemaVersion": 1, "view": changed})).unwrap(),
    )
    .unwrap();
    assert_eq!(host.projects["project"].history().len(), sequence);
    assert!(host
        .workspace_query(&query(Some(revision.clone())))
        .is_err());
    assert!(host.workspace_query(&events_query).is_err());
    let WorkspaceResult::WorkerObservation(updated) = host.workspace_query(&query(None)).unwrap()
    else {
        panic!("updated")
    };
    assert_ne!(updated.revision, revision);
    assert_eq!(updated.duplicates, 1);
    assert_eq!(updated.event_count, 0);
    std::fs::write(&file, original_bytes).unwrap();
    assert_eq!(
        crate::reader_generation::tests::snapshot(&host.root),
        before
    );
    drop(host);
}
