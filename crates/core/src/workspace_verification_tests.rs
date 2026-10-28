use super::*;
fn original() -> VerificationDetail {
    VerificationDetail {
        project_id: "project".into(),
        operation_id: "check".into(),
        task_id: "task".into(),
        attempt_id: "attempt".into(),
        state: VerificationState::Unknown,
        revision_digest: "a".repeat(64),
        manifest_digest: "b".repeat(64),
        snapshot_digest: None,
        ledger_recorded: false,
        evidence_digest: "c".repeat(64),
        observed_at: "2026-10-15T00:00:00Z".into(),
        message: None,
        artifacts: (0..45)
            .map(|i| Artifact {
                path: format!("artifact-{i}"),
                bytes: i,
                sha256: "d".repeat(64),
            })
            .collect(),
        stages: vec![],
        recovery: None,
        recovery_error: Some("original refusal".into()),
    }
}
#[test]
fn workspace_verification_version_and_metadata_preserve_original_fields() {
    let detail = original();
    let before = serde_json::to_value(&detail).unwrap();
    let revision = query_revision("project", &detail).unwrap();
    let mut changed = detail.clone();
    changed.observed_at.push('x');
    assert_eq!(revision, query_revision("project", &changed).unwrap());
    assert_ne!(
        revision,
        query_revision("same-sequence-different-state", &detail).unwrap()
    );
    changed = detail.clone();
    changed.artifacts[44].bytes += 1;
    assert_ne!(revision, query_revision("project", &changed).unwrap());
    changed = detail.clone();
    changed.recovery_error = None;
    assert_ne!(revision, query_revision("project", &changed).unwrap());
    let mut expected = before.clone();
    let fields = expected.as_object_mut().unwrap();
    fields.remove("artifacts");
    fields.remove("stages");
    fields.insert("revision".into(), serde_json::json!(revision));
    fields.insert("artifactCount".into(), serde_json::json!(45));
    fields.insert("stageCount".into(), serde_json::json!(0));
    assert_eq!(
        serde_json::to_value(metadata(detail.clone(), revision).unwrap()).unwrap(),
        expected
    );
    assert_eq!(serde_json::to_value(detail).unwrap(), before);
}
#[test]
fn workspace_verification_pages_preserve_original_array_and_refuse_cursor() {
    let detail = original();
    let revision = query_revision("project", &detail).unwrap();
    let mut collected = vec![];
    let mut offset = 0;
    loop {
        let (entries, next) = crate::workspace_read::page(
            detail.artifacts.iter(),
            45,
            offset,
            Some(&revision),
            &revision,
        )
        .unwrap();
        assert!(entries.len() <= 20);
        assert!(serde_json::to_vec(&entries).unwrap().len() < 64 * 1024);
        collected.extend(entries.into_iter().cloned());
        let Some(next) = next else { break };
        offset = next;
    }
    assert_eq!(collected, detail.artifacts);
    for (offset, expected) in [(46, Some(revision.as_str())), (1, None), (0, Some("wrong"))] {
        assert!(crate::workspace_read::page(
            detail.artifacts.iter(),
            45,
            offset,
            expected,
            &revision
        )
        .is_err());
    }
    let wrong = "0".repeat(64);
    assert!(
        crate::workspace_read::page(detail.artifacts.iter(), 45, 0, Some(&wrong), &revision)
            .is_err()
    );
    let huge = Artifact {
        path: "x".repeat(64 * 1024),
        bytes: 0,
        sha256: "a".repeat(64),
    };
    assert!(crate::workspace_read::page([huge], 1, 0, Some(&revision), &revision).is_err());
}
#[test]
fn workspace_verification_metadata_exact_512kib_and_overflow() {
    let mut detail = original();
    let base = WorkspaceResult::VerificationResult(Box::new(
        metadata(detail.clone(), "a".repeat(64)).unwrap(),
    ));
    let fill = crate::workspace_read::RESULT_BYTES - serde_json::to_vec(&base).unwrap().len() + 2;
    detail.message = Some("x".repeat(fill));
    let exact = WorkspaceResult::VerificationResult(Box::new(
        metadata(detail.clone(), "a".repeat(64)).unwrap(),
    ));
    assert_eq!(
        serde_json::to_vec(&exact).unwrap().len(),
        crate::workspace_read::RESULT_BYTES
    );
    assert!(crate::workspace_read::bounded(exact).is_ok());
    detail.message.as_mut().unwrap().push('x');
    assert!(
        crate::workspace_read::bounded(WorkspaceResult::VerificationResult(Box::new(
            metadata(detail, "a".repeat(64)).unwrap()
        )))
        .is_err()
    );
}

#[test]
fn workspace_verification_missing_identity_basis_and_poisoned_are_explicit() {
    let temp = tempfile::tempdir().unwrap();
    let (mut host, draft) = project_planning::tests::fixture(temp.path());
    host.configure_project(draft).unwrap();
    let query = WorkspaceQuery::VerificationResult {
        project_id: "project".into(),
        operation_id: "check".into(),
        revision: None,
    };
    let before = crate::reader_generation::tests::snapshot(&host.root);
    assert!(host
        .workspace_query(&query)
        .unwrap_err()
        .contains("原验收记录不存在"));
    let job = VerificationJob {
        project_id: "project".into(),
        operation_id: "check".into(),
        task_id: "task".into(),
        attempt_id: "attempt".into(),
        state: VerificationState::Unknown,
        started_at: "2026-10-15T00:00:00Z".into(),
        updated_at: "2026-10-15T00:00:00Z".into(),
        message: None,
        reconciliation_digest: None,
    };
    let key = ("project".into(), "check".into());
    host.jobs.insert(key.clone(), job.clone());
    assert!(host
        .workspace_query(&query)
        .unwrap_err()
        .contains("缺少提交产物证据"));
    host.jobs.get_mut(&key).unwrap().project_id = "other".into();
    assert!(host
        .workspace_query(&query)
        .unwrap_err()
        .contains("身份不符"));
    host.jobs.insert(key.clone(), job);
    host.base.poisoned = true;
    assert!(host.workspace_query(&query).unwrap_err().contains("重开"));
    assert_eq!(host.jobs[&key].state, VerificationState::Unknown);
    assert_eq!(
        before,
        crate::reader_generation::tests::snapshot(&host.root)
    );
}

#[test]
fn workspace_verification_stage_pages_preserve_tail_and_bind_changes() {
    use cerulean_domain::verification_detail::{VerificationStageState, VerificationStageView};
    let mut detail = original();
    detail.stages = (0..45)
        .map(|i| VerificationStageView {
            id: format!("stage-{i}"),
            name: format!("original name {i}"),
            command_digest: "a".repeat(64),
            timeout_seconds: 1,
            state: VerificationStageState::Unknown,
            exit_code: None,
            timed_out: false,
            stdout: None,
            stderr: None,
        })
        .collect();
    let revision = query_revision("project", &detail).unwrap();
    let before = serde_json::to_value(&detail.stages).unwrap();
    let mut found = Vec::new();
    let mut offset = 0;
    loop {
        let (entries, next) = crate::workspace_read::page(
            detail.stages.iter(),
            45,
            offset,
            Some(&revision),
            &revision,
        )
        .unwrap();
        assert!(entries.len() <= 20);
        found.extend(entries.into_iter().cloned());
        let Some(next) = next else { break };
        offset = next;
    }
    assert_eq!(serde_json::to_value(found).unwrap(), before);
    detail.stages[44].name.push_str(" changed");
    assert_ne!(revision, query_revision("project", &detail).unwrap());
}

#[test]
fn workspace_verification_stage_pages_bound_actual_escaped_json_bytes() {
    use cerulean_domain::verification_detail::{VerificationStageState, VerificationStageView};
    let stages: Vec<_> = (0..45)
        .map(|i| VerificationStageView {
            id: format!("stage-{i}"),
            name: format!("original-{i}:{}", "\0\"\\\n".repeat(900)),
            command_digest: "a".repeat(64),
            timeout_seconds: 12,
            state: VerificationStageState::Unknown,
            exit_code: None,
            timed_out: false,
            stdout: None,
            stderr: None,
        })
        .collect();
    let before = serde_json::to_vec(&stages).unwrap();
    let revision = crate::workspace_history::version(&stages).unwrap();
    let mut collected = Vec::new();
    let mut offset = 0;
    loop {
        let (entries, next_offset) = crate::workspace_read::page(
            stages.iter(),
            stages.len(),
            offset,
            Some(&revision),
            &revision,
        )
        .unwrap();
        assert!(!entries.is_empty() && entries.len() < 20);
        let page = WorkspaceResult::VerificationResultStages(WorkspaceVerificationStagePage {
            project_id: "project".into(),
            operation_id: "check".into(),
            revision: revision.clone(),
            offset,
            total: stages.len() as u32,
            next_offset,
            entries: entries.into_iter().cloned().collect(),
        });
        assert!(serde_json::to_vec(&page).unwrap().len() <= 64 * 1024);
        let WorkspaceResult::VerificationResultStages(page) = page else {
            unreachable!()
        };
        collected.extend(page.entries);
        let Some(next) = next_offset else { break };
        offset = next;
    }
    assert_eq!(serde_json::to_vec(&collected).unwrap(), before);
    assert_eq!(serde_json::to_vec(&stages).unwrap(), before);
    let mut oversized = stages[0].clone();
    oversized.name = "\0".repeat(64 * 1024);
    assert!(crate::workspace_read::page([oversized], 1, 0, Some(&revision), &revision).is_err());
}

#[cfg(windows)]
pub(super) fn production_shared_owner_transfer(host: &mut RuntimeHost, operation: &str) {
    fn stable(result: WorkspaceResult) -> serde_json::Value {
        let mut value = serde_json::to_value(result).unwrap();
        if value["type"] == "verificationResult" {
            value["value"].as_object_mut().unwrap().remove("observedAt");
        }
        value
    }
    let key = ("project".into(), operation.to_string());
    let original_job = host.jobs[&key].clone();
    assert_eq!(original_job.state, VerificationState::Succeeded);
    let query = |revision| WorkspaceQuery::VerificationResult {
        project_id: "project".into(),
        operation_id: operation.into(),
        revision,
    };
    let initial = host.workspace_query(&query(None)).unwrap();
    let WorkspaceResult::VerificationResult(detail) = &initial else {
        panic!("metadata")
    };
    let revision = detail.revision.clone();
    let queries = [
        query(Some(revision.clone())),
        WorkspaceQuery::VerificationResultArtifacts {
            project_id: "project".into(),
            operation_id: operation.into(),
            offset: 0,
            revision: revision.clone(),
        },
        WorkspaceQuery::VerificationResultStages {
            project_id: "project".into(),
            operation_id: operation.into(),
            offset: 0,
            revision: revision.clone(),
        },
    ];
    let baseline: Vec<_> = queries
        .iter()
        .map(|q| stable(host.workspace_query(q).unwrap()))
        .collect();
    let before = crate::reader_generation::tests::snapshot(&host.root);
    let sequence = host.projects["project"].history().len();
    let store = host.projects.remove("project").unwrap();
    let writer =
        crate::async_api::ApiWorkflow::start(store, host.root.join("workflows/project")).unwrap();
    host.api_writers.insert("project".into(), writer);
    for (q, expected) in queries.iter().zip(&baseline) {
        assert_eq!(&stable(host.workspace_query(q).unwrap()), expected);
    }
    // Controlled live status exercises the real shared owner without inventing
    // any ledger/report. Unknown must remain unknown; recovery availability is
    // intentionally not asserted identical when ownership changes.
    host.jobs.get_mut(&key).unwrap().state = VerificationState::Unknown;
    let WorkspaceResult::VerificationResult(unknown) = host.workspace_query(&query(None)).unwrap()
    else {
        panic!("unknown")
    };
    assert_eq!(unknown.state, VerificationState::Unknown);
    assert!(unknown.ledger_recorded);
    assert_eq!(host.jobs[&key].state, VerificationState::Unknown);
    host.jobs.insert(key.clone(), original_job.clone());
    host.api_writers.get_mut("project").unwrap().interrupted = true;
    for q in &queries {
        assert!(host.workspace_query(q).is_err());
    }
    host.api_writers.get_mut("project").unwrap().interrupted = false;
    host.base.poisoned = true;
    for q in &queries {
        assert!(host.workspace_query(q).is_err());
    }
    host.base.poisoned = false;
    let mut writer = host.api_writers.remove("project").unwrap();
    let store = writer.owner.finish(false).unwrap();
    host.projects.insert("project".into(), store);
    for (q, expected) in queries.iter().zip(&baseline) {
        assert_eq!(&stable(host.workspace_query(q).unwrap()), expected);
    }
    assert_eq!(host.projects["project"].history().len(), sequence);
    assert_eq!(host.jobs[&key].state, original_job.state);
    assert_eq!(
        crate::reader_generation::tests::snapshot(&host.root),
        before
    );
}
