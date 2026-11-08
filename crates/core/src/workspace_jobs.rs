//! Per-project original executor records. Never builds a complete Snapshot.
use super::*;
use sha2::Digest;

#[derive(serde::Serialize)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
enum JobRef<'a> {
    Codex(&'a CodexJob),
    Omp(&'a cerulean_domain::omp_jobs::OmpJob),
    #[cfg(windows)]
    Api(&'a cerulean_domain::api_jobs::ApiJob),
    Verification(&'a VerificationJob),
}
impl JobRef<'_> {
    fn summary(&self) -> WorkspaceJobSummary {
        let (id, project, task, attempt, status, started, updated, message) = match self {
            Self::Codex(j) => (
                &j.attempt_id,
                &j.project_id,
                &j.task_id,
                &j.attempt_id,
                WorkspaceJobStatus::Codex(j.state.clone()),
                &j.started_at,
                Some(&j.updated_at),
                &j.message,
            ),
            Self::Omp(j) => (
                &j.attempt_id,
                &j.project_id,
                &j.task_id,
                &j.attempt_id,
                WorkspaceJobStatus::Omp(j.state.clone()),
                &j.started_at,
                None,
                &j.message,
            ),
            #[cfg(windows)]
            Self::Api(j) => (
                &j.attempt_id,
                &j.project_id,
                &j.task_id,
                &j.attempt_id,
                WorkspaceJobStatus::Api(j.state.clone()),
                &j.started_at,
                None,
                &j.message,
            ),
            Self::Verification(j) => (
                &j.operation_id,
                &j.project_id,
                &j.task_id,
                &j.attempt_id,
                WorkspaceJobStatus::Verification(j.state.clone()),
                &j.started_at,
                Some(&j.updated_at),
                &j.message,
            ),
        };
        WorkspaceJobSummary {
            id: id.clone(),
            project_id: project.clone(),
            task_id: task.clone(),
            attempt_id: attempt.clone(),
            status,
            started_at: started.clone(),
            updated_at: updated.cloned(),
            has_message: message.is_some(),
        }
    }
    fn identity(&self) -> (WorkspaceJobKind, &str) {
        match self {
            Self::Codex(j) => (WorkspaceJobKind::Codex, &j.attempt_id),
            Self::Omp(j) => (WorkspaceJobKind::Omp, &j.attempt_id),
            #[cfg(windows)]
            Self::Api(j) => (WorkspaceJobKind::Api, &j.attempt_id),
            Self::Verification(j) => (WorkspaceJobKind::Verification, &j.operation_id),
        }
    }
    fn validate_key(&self, key: &(String, String), project_id: &str) -> Result<(), String> {
        let (project, task, attempt, revision) = match self {
            Self::Codex(j) => (
                &j.project_id,
                &j.task_id,
                &j.attempt_id,
                Some(&j.revision_digest),
            ),
            Self::Omp(j) => (
                &j.project_id,
                &j.task_id,
                &j.attempt_id,
                Some(&j.revision_digest),
            ),
            #[cfg(windows)]
            Self::Api(j) => (
                &j.project_id,
                &j.task_id,
                &j.attempt_id,
                Some(&j.revision_digest),
            ),
            Self::Verification(j) => (&j.project_id, &j.task_id, &j.attempt_id, None),
        };
        require(
            project == project_id && key.0 == *project && key.1 == self.identity().1,
            "原作业项目或记录身份不一致，请保留原记录核对",
        )?;
        for id in [
            project.as_str(),
            task.as_str(),
            attempt.as_str(),
            self.identity().1,
        ] {
            cerulean_domain::execution::identifier(id)?;
        }
        if let Some(revision) = revision {
            cerulean_domain::execution::sha256(revision)?;
        }
        // Historical jobs may refer to an earlier task/attempt or revision.
        // Their identity must not be replaced with the current task run.
        Ok(())
    }
    fn record(&self) -> WorkspaceJobRecord {
        // Return precisely the original record bound by the query version.
        // Recovery projections depend on live ownership and belong to recovery reads.
        match self {
            Self::Codex(j) => WorkspaceJobRecord::Codex((*j).clone()),
            Self::Omp(j) => WorkspaceJobRecord::Omp((*j).clone()),
            #[cfg(windows)]
            Self::Api(j) => WorkspaceJobRecord::Api((*j).clone()),
            Self::Verification(j) => WorkspaceJobRecord::Verification((*j).clone()),
        }
    }
}
impl RuntimeHost {
    pub(super) fn workspace_jobs(
        &self,
        query: &WorkspaceQuery,
        project_version: &str,
    ) -> Result<WorkspaceResult, String> {
        let project_id = match query {
            WorkspaceQuery::Jobs { project_id, .. } | WorkspaceQuery::Job { project_id, .. } => {
                project_id
            }
            _ => return Err("原作业查询类型不符".into()),
        };
        require(!self.base.poisoned, "原作业写入尚未核对，请保留记录并重开")?;
        #[cfg(windows)]
        require(
            self.api_fault.is_none(),
            "API原写入需要重开核对，作业记录暂不可用",
        )?;
        // Borrow original records; large messages are hashed, not copied into
        // page summaries. Stable order is executor kind then original map key.
        let mut jobs = Vec::new();
        for (key, job) in &self.codex_jobs {
            if &key.0 == project_id || &job.project_id == project_id {
                let job = JobRef::Codex(job);
                job.validate_key(key, project_id)?;
                jobs.push(job);
            }
        }
        for (key, job) in &self.omp_jobs {
            if &key.0 == project_id || &job.project_id == project_id {
                let job = JobRef::Omp(job);
                job.validate_key(key, project_id)?;
                jobs.push(job);
            }
        }
        #[cfg(windows)]
        for (key, job) in &self.api_jobs {
            if &key.0 == project_id || &job.project_id == project_id {
                let job = JobRef::Api(job);
                job.validate_key(key, project_id)?;
                jobs.push(job);
            }
        }
        for (key, job) in &self.jobs {
            if &key.0 == project_id || &job.project_id == project_id {
                let job = JobRef::Verification(job);
                job.validate_key(key, project_id)?;
                jobs.push(job);
            }
        }
        let mut hash = QueryHash(sha2::Sha256::new());
        serde_json::to_writer(
            &mut hash,
            &("workspace-jobs-v1", project_id, project_version, &jobs),
        )
        .map_err(|e| e.to_string())?;
        let version = format!("{:x}", hash.0.finalize());
        let result = match query {
            WorkspaceQuery::Jobs {
                offset, revision, ..
            } => {
                let (entries, next_offset) = crate::workspace_read::page(
                    jobs.iter().map(JobRef::summary),
                    jobs.len(),
                    *offset,
                    revision.as_deref(),
                    &version,
                )?;
                WorkspaceResult::Jobs(WorkspaceJobPage {
                    project_id: project_id.clone(),
                    revision: version,
                    offset: *offset,
                    total: jobs.len() as u32,
                    next_offset,
                    entries,
                })
            }
            WorkspaceQuery::Job {
                kind, id, revision, ..
            } => {
                cerulean_domain::execution::identifier(id)?;
                cerulean_domain::execution::sha256(revision)?;
                require(revision == &version, "原项目作业已变化，请刷新作业首页")?;
                let job = jobs
                    .iter()
                    .find(|j| j.identity() == (*kind, id.as_str()))
                    .ok_or("原作业不存在，未创建替代记录")?;
                WorkspaceResult::Job(Box::new(WorkspaceJobDetail {
                    project_id: project_id.clone(),
                    revision: version,
                    record: job.record(),
                }))
            }
            _ => return Err("原作业查询类型不符".into()),
        };
        crate::workspace_read::bounded(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn workspace_jobs_same_id_across_executors_uses_original_views_and_scope() {
        use cerulean_domain::{
            api_jobs::{ApiJob, ApiJobState},
            api_worker::{ApiTarget, ApiWorkerSelection},
            execution::{ValidationFile, WorkerSelection},
            omp_jobs::{OmpJob, OmpJobState},
        };
        let temp = tempfile::tempdir().unwrap();
        let (mut host, draft) = project_planning::tests::fixture(temp.path());
        host.configure_project(draft).unwrap();
        let worker = WorkerSelection {
            reasoning_effort: None,
            omp_runtime_parent: None,
            provider: "fixture".into(),
            model: "fixture".into(),
            executable: ValidationFile {
                path: "fixture.exe".into(),
                sha256: "0".repeat(64),
            },
        };
        let key = ("project".into(), "same-id".into());
        let codex = CodexJob {
            management_operation: None,
            project_id: "project".into(),
            task_id: "task".into(),
            attempt_id: "same-id".into(),
            revision_digest: "a".repeat(64),
            worker: worker.clone(),
            state: cerulean_domain::CodexJobState::CancelRequested,
            started_at: "2026-10-14T00:00:00Z".into(),
            updated_at: "2026-10-14T00:00:00Z".into(),
            message: Some("codex-original".into()),
        };
        host.codex_jobs.insert(key.clone(), codex.clone());
        let mut other = codex.clone();
        other.project_id = "other".into();
        host.codex_jobs
            .insert(("other".into(), "same-id".into()), other);
        host.omp_jobs.insert(
            key.clone(),
            OmpJob {
                management_operation: None,
                project_id: "project".into(),
                task_id: "task".into(),
                attempt_id: "same-id".into(),
                revision_digest: "a".repeat(64),
                worker,
                state: OmpJobState::Unknown,
                started_at: codex.started_at.clone(),
                message: Some("omp-original".into()),
                closure_digest: Some("stale".into()),
            },
        );
        host.api_jobs.insert(
            key.clone(),
            ApiJob {
                project_id: "project".into(),
                task_id: "task".into(),
                attempt_id: "same-id".into(),
                revision_digest: "a".repeat(64),
                worker: ApiWorkerSelection {
                    reasoning_effort: None,
                    write_scope: None,
                    target: ApiTarget::OpenAi {},
                    model: "fixture".into(),
                    file_tools: vec![ToolPermission::ReadFiles],
                    max_rounds: 1,
                    max_tool_calls: 1,
                    max_output_tokens: 100,
                    max_total_tokens: 1000,
                    request_timeout_seconds: 2,
                },
                management_operation: None,
                state: ApiJobState::Retired,
                started_at: codex.started_at.clone(),
                message: Some("api-original".into()),
                closure_digest: Some("stale".into()),
                retirement_digest: Some("stale".into()),
            },
        );
        host.jobs.insert(
            key,
            VerificationJob {
                project_id: "project".into(),
                operation_id: "same-id".into(),
                task_id: "task".into(),
                attempt_id: "different-attempt".into(),
                state: VerificationState::Unknown,
                started_at: codex.started_at.clone(),
                updated_at: codex.updated_at.clone(),
                message: Some("verification-original".into()),
                reconciliation_digest: Some("stale".into()),
            },
        );
        let before = crate::reader_generation::tests::snapshot(&host.root);
        let p = page(&host, 0, None).unwrap();
        assert_eq!(p.total, 4);
        for (kind, expected) in [
            (
                WorkspaceJobKind::Codex,
                WorkspaceJobRecord::Codex(
                    host.codex_jobs[&("project".into(), "same-id".into())].clone(),
                ),
            ),
            (
                WorkspaceJobKind::Omp,
                WorkspaceJobRecord::Omp(
                    host.omp_jobs[&("project".into(), "same-id".into())].clone(),
                ),
            ),
            (
                WorkspaceJobKind::Api,
                WorkspaceJobRecord::Api(
                    host.api_jobs[&("project".into(), "same-id".into())].clone(),
                ),
            ),
            (
                WorkspaceJobKind::Verification,
                WorkspaceJobRecord::Verification(
                    host.jobs[&("project".into(), "same-id".into())].clone(),
                ),
            ),
        ] {
            let WorkspaceResult::Job(detail) = host
                .workspace_query(&WorkspaceQuery::Job {
                    project_id: "project".into(),
                    kind,
                    id: "same-id".into(),
                    revision: p.revision.clone(),
                })
                .unwrap()
            else {
                panic!("job")
            };
            assert_eq!(
                serde_json::to_value(detail.record).unwrap(),
                serde_json::to_value(expected).unwrap()
            );
        }
        assert_eq!(
            crate::reader_generation::tests::snapshot(&host.root),
            before
        );
    }
    fn page(
        host: &RuntimeHost,
        offset: u32,
        revision: Option<String>,
    ) -> Result<WorkspaceJobPage, String> {
        match host.workspace_query(&WorkspaceQuery::Jobs {
            project_id: "project".into(),
            offset,
            revision,
        })? {
            WorkspaceResult::Jobs(p) => Ok(p),
            _ => panic!("jobs"),
        }
    }
    #[test]
    fn workspace_jobs_pages_keep_original_identity_state_and_messages_out_of_list() {
        let temp = tempfile::tempdir().unwrap();
        let (mut host, draft) = project_planning::tests::fixture(temp.path());
        host.configure_project(draft).unwrap();
        for n in 0..43 {
            let job = VerificationJob {
                project_id: "project".into(),
                operation_id: format!("check-{n:03}"),
                task_id: "task".into(),
                attempt_id: "attempt".into(),
                state: VerificationState::Unknown,
                started_at: "2026-10-14T00:00:00Z".into(),
                updated_at: "2026-10-14T00:00:00Z".into(),
                message: Some("original message ".repeat(8000)),
                reconciliation_digest: Some("stale-must-not-be-used".into()),
            };
            host.jobs
                .insert((job.project_id.clone(), job.operation_id.clone()), job);
        }
        let before = crate::reader_generation::tests::snapshot(&host.root);
        assert!(serde_json::to_vec(&host.snapshot()).unwrap().len() > 4 * 1024 * 1024);
        let mut p = page(&host, 0, None).unwrap();
        let version = p.revision.clone();
        let mut ids = Vec::new();
        loop {
            assert!(serde_json::to_vec(&p).unwrap().len() < 64 * 1024);
            for job in &p.entries {
                assert!(job.has_message);
                assert!(matches!(
                    job.status,
                    WorkspaceJobStatus::Verification(VerificationState::Unknown)
                ));
                ids.push(job.id.clone());
            }
            let Some(offset) = p.next_offset else { break };
            p = page(&host, offset, Some(version.clone())).unwrap();
        }
        assert_eq!(
            ids,
            (0..43).map(|n| format!("check-{n:03}")).collect::<Vec<_>>()
        );
        let query = WorkspaceQuery::Job {
            project_id: "project".into(),
            kind: WorkspaceJobKind::Verification,
            id: "check-042".into(),
            revision: version.clone(),
        };
        let WorkspaceResult::Job(detail) = host.workspace_query(&query).unwrap() else {
            panic!("detail")
        };
        let WorkspaceJobRecord::Verification(job) = detail.record else {
            panic!("verification")
        };
        assert_eq!(
            job.message,
            host.jobs[&("project".into(), "check-042".into())].message
        );
        #[cfg(windows)]
        assert_eq!(
            job.reconciliation_digest.as_deref(),
            Some("stale-must-not-be-used")
        );
        assert!(page(&host, 20, None).is_err());
        assert!(page(&host, 44, Some(version)).is_err());
        host.jobs
            .get_mut(&("project".into(), "check-042".into()))
            .unwrap()
            .message = Some("changed without changing project sequence".into());
        assert!(host.workspace_query(&query).is_err());
        assert_eq!(
            crate::reader_generation::tests::snapshot(&host.root),
            before
        );
    }
    fn original_verification() -> VerificationJob {
        VerificationJob {
            project_id: "project".into(),
            operation_id: "original-check".into(),
            task_id: "historical-task".into(),
            attempt_id: "historical-attempt".into(),
            state: VerificationState::Unknown,
            started_at: "2026-10-14T00:00:00Z".into(),
            updated_at: "2026-10-14T00:00:00Z".into(),
            message: None,
            reconciliation_digest: None,
        }
    }
    #[test]
    fn workspace_jobs_rejects_tampered_original_identity_and_preserves_historical_scope() {
        let temp = tempfile::tempdir().unwrap();
        let (mut host, draft) = project_planning::tests::fixture(temp.path());
        host.configure_project(draft).unwrap();
        let original = original_verification();
        let key = (original.project_id.clone(), original.operation_id.clone());
        host.jobs.insert(key.clone(), original.clone());
        let initial = page(&host, 0, None).unwrap();
        assert_eq!(initial.entries[0].task_id, "historical-task");
        assert_eq!(initial.entries[0].attempt_id, "historical-attempt");
        let before = crate::reader_generation::tests::snapshot(&host.root);
        for (bad_key, mut job) in [
            (("project".into(), "wrong-key".into()), original.clone()),
            (("other".into(), "original-check".into()), original.clone()),
            (key.clone(), original.clone()),
        ] {
            host.jobs.clear();
            if bad_key.0 == "project" && bad_key.1 == "original-check" {
                job.project_id = "other".into();
            }
            host.jobs.insert(bad_key, job);
            assert!(page(&host, 0, None).is_err());
        }
        for field in ["task", "attempt", "operation"] {
            host.jobs.clear();
            let mut job = original.clone();
            match field {
                "task" => job.task_id = "invalid/id".into(),
                "attempt" => job.attempt_id = "invalid/id".into(),
                _ => job.operation_id = "invalid/id".into(),
            }
            host.jobs
                .insert((job.project_id.clone(), job.operation_id.clone()), job);
            assert!(page(&host, 0, None).is_err());
        }
        host.jobs.clear();
        host.jobs.insert(key, original);
        let query = WorkspaceQuery::Job {
            project_id: "project".into(),
            kind: WorkspaceJobKind::Verification,
            id: "original-check".into(),
            revision: initial.revision,
        };
        assert!(host.workspace_query(&query).is_ok());
        host.base.poisoned = true;
        assert!(page(&host, 0, None).is_err());
        assert!(host.workspace_query(&query).is_err());
        assert_eq!(
            before,
            crate::reader_generation::tests::snapshot(&host.root)
        );
    }
    #[test]
    fn workspace_jobs_enforce_encoded_detail_limit_without_truncation() {
        let temp = tempfile::tempdir().unwrap();
        let (mut host, draft) = project_planning::tests::fixture(temp.path());
        host.configure_project(draft).unwrap();
        let job = original_verification();
        let key = (job.project_id.clone(), job.operation_id.clone());
        host.jobs.insert(key.clone(), job);
        let query = |revision: String| WorkspaceQuery::Job {
            project_id: "project".into(),
            kind: WorkspaceJobKind::Verification,
            id: "original-check".into(),
            revision,
        };
        let size = |host: &RuntimeHost| -> Option<usize> {
            let revision = page(host, 0, None).ok()?.revision;
            let result = host.workspace_query(&query(revision)).ok()?;
            Some(serde_json::to_vec(&result).ok()?.len())
        };
        let mut low = 0usize;
        let mut high = crate::workspace_read::RESULT_BYTES + 1;
        while low + 1 < high {
            let mid = (low + high) / 2;
            host.jobs.get_mut(&key).unwrap().message = Some("a".repeat(mid));
            if size(&host).is_some_and(|bytes| bytes <= crate::workspace_read::RESULT_BYTES) {
                low = mid;
            } else {
                high = mid;
            }
        }
        host.jobs.get_mut(&key).unwrap().message = Some("a".repeat(low));
        let revision = page(&host, 0, None).unwrap().revision;
        let result = host.workspace_query(&query(revision.clone())).unwrap();
        assert!(serde_json::to_vec(&result).unwrap().len() <= crate::workspace_read::RESULT_BYTES);
        let WorkspaceResult::Job(detail) = result else {
            panic!("job")
        };
        let WorkspaceJobRecord::Verification(record) = detail.record else {
            panic!("verification")
        };
        assert_eq!(record.message.as_deref().unwrap().len(), low);
        host.jobs.get_mut(&key).unwrap().message = Some("a".repeat(low + 1));
        let revision = page(&host, 0, None).unwrap().revision;
        assert!(host.workspace_query(&query(revision)).is_err());
    }
    #[test]
    fn workspace_jobs_page_cursor_is_byte_driven_and_rejects_one_large_summary() {
        let temp = tempfile::tempdir().unwrap();
        let (mut host, draft) = project_planning::tests::fixture(temp.path());
        host.configure_project(draft).unwrap();
        for n in 0..23 {
            let mut job = original_verification();
            job.operation_id = format!("byte-check-{n:02}");
            job.started_at = "\u{1}".repeat(3000);
            host.jobs
                .insert((job.project_id.clone(), job.operation_id.clone()), job);
        }
        let mut current = page(&host, 0, None).unwrap();
        assert!(current.entries.len() < 20);
        let revision = current.revision.clone();
        let mut count = current.entries.len();
        while let Some(offset) = current.next_offset {
            current = page(&host, offset, Some(revision.clone())).unwrap();
            count += current.entries.len();
        }
        assert_eq!(count, 23);
        host.jobs
            .get_mut(&("project".into(), "byte-check-00".into()))
            .unwrap()
            .started_at = "\u{1}".repeat(20_000);
        assert!(page(&host, 0, None).is_err());
    }
    #[test]
    fn workspace_jobs_empty_missing_project_and_fault_are_distinct() {
        let temp = tempfile::tempdir().unwrap();
        let (mut host, draft) = project_planning::tests::fixture(temp.path());
        host.configure_project(draft).unwrap();
        let empty = page(&host, 0, None).unwrap();
        assert_eq!(empty.total, 0);
        assert!(empty.entries.is_empty());
        assert!(host
            .workspace_query(&WorkspaceQuery::Jobs {
                project_id: "absent".into(),
                offset: 0,
                revision: None
            })
            .is_err());
        assert!(host
            .workspace_query(&WorkspaceQuery::Job {
                project_id: "project".into(),
                kind: WorkspaceJobKind::Codex,
                id: "absent".into(),
                revision: empty.revision
            })
            .is_err());
        #[cfg(windows)]
        {
            host.api_fault = Some("original writer interrupted".into());
            assert!(page(&host, 0, None).is_err());
        }
    }
}
