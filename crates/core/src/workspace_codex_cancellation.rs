//! Bounded cancellation evidence read from the original durable workflow owner.
use super::*;
use serde::Serialize;
use sha2::Digest;

pub(crate) struct OriginalBasis {
    pub(crate) version: String,
    pub(crate) entries: Vec<CodexCancellationReview>,
}

// Field order and names intentionally match ExecutionSnapshot's serialized proof.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CancellationSnapshot<'a> {
    project: &'a Project,
    revision: Option<&'a PlanRevision>,
    revision_digest: Option<String>,
    approval: Option<&'a Approval>,
    state: &'a WorkflowState,
    sequence: u32,
}

fn hash(value: &impl Serialize) -> Result<String, String> {
    let mut writer = QueryHash(sha2::Sha256::new());
    serde_json::to_writer(&mut writer, value).map_err(|error| error.to_string())?;
    Ok(format!("{:x}", writer.0.finalize()))
}

pub(crate) fn original_basis(
    store: &crate::scheduler::DurableWorkflow,
    project: &str,
    jobs: &[CodexJob],
) -> Result<OriginalBasis, String> {
    identifier(project)?;
    require(!store.requires_reopen(), "项目账本需要重开核心恢复")?;
    let scope = store
        .configuration()
        .execution_scope
        .as_ref()
        .ok_or("原项目缺少执行范围")?;
    require(
        scope.project.id == project,
        "原项目账本身份与停止审阅项目不符",
    )?;
    let sequence = u32::try_from(store.history().len()).map_err(|_| "原项目历史超出范围")?;
    let snapshot = CancellationSnapshot {
        project: &scope.project,
        revision: store.revision(),
        revision_digest: store
            .revision()
            .map(|revision| revision.approval_digest())
            .transpose()?,
        approval: store.approval(),
        state: store.state(),
        sequence,
    };
    let mut entries = Vec::new();
    for job in jobs {
        require(job.project_id == project, "原编程执行项目身份不符")?;
        identifier(&job.task_id)?;
        identifier(&job.attempt_id)?;
        let task = store
            .state()
            .tasks
            .get(&job.task_id)
            .ok_or("执行任务缺失")?;
        if (job.state == CodexJobState::Starting
            && task.state == TaskState::Pending
            && task.attempt.is_none())
            || (matches!(
                job.state,
                CodexJobState::Starting | CodexJobState::Executing
            ) && task.state == TaskState::Running
                && task.attempt.as_deref() == Some(job.attempt_id.as_str())
                && task.cancellation_requested != Some(true))
        {
            entries.push(CodexCancellationReview {
                project_id: project.into(),
                task_id: job.task_id.clone(),
                attempt_id: job.attempt_id.clone(),
                evidence_digest: hash(&(
                    "codex-cancel-v1",
                    &snapshot,
                    &job.worker,
                    &job.attempt_id,
                ))?,
            });
        }
    }
    Ok(OriginalBasis {
        version: hash(&(
            "workspace-codex-cancellation-original-v1",
            project,
            store.configuration(),
            store.state(),
            store.revision(),
            store.approval(),
            store.history(),
        ))?,
        entries,
    })
}

fn result(
    query: &WorkspaceQuery,
    project: &str,
    revision: String,
    entries: Vec<CodexCancellationReview>,
) -> Result<WorkspaceResult, String> {
    let result = match query {
        WorkspaceQuery::CodexCancellations {
            offset,
            revision: expected,
            ..
        } => {
            let total = entries.len();
            let (entries, next_offset) = crate::workspace_read::page(
                entries,
                total,
                *offset,
                expected.as_deref(),
                &revision,
            )?;
            WorkspaceResult::CodexCancellations(WorkspaceCodexCancellationPage {
                project_id: project.into(),
                revision,
                offset: *offset,
                total: total as u32,
                next_offset,
                entries,
            })
        }
        WorkspaceQuery::CodexCancellation {
            attempt_id,
            revision: expected,
            ..
        } => {
            identifier(attempt_id)?;
            sha256(expected)?;
            require(expected == &revision, "原停止审阅已变化，请刷新首页")?;
            let review = entries
                .into_iter()
                .find(|entry| entry.attempt_id == *attempt_id)
                .ok_or("原可停止执行不存在，请刷新核对")?;
            WorkspaceResult::CodexCancellation(Box::new(WorkspaceCodexCancellationDetail {
                project_id: project.into(),
                revision,
                review,
            }))
        }
        _ => return Err("停止审阅查询类型不符".into()),
    };
    crate::workspace_read::bounded(result)
}

impl RuntimeHost {
    pub(super) fn workspace_codex_cancellation(
        &self,
        query: &WorkspaceQuery,
    ) -> Result<WorkspaceResult, String> {
        let project = match query {
            WorkspaceQuery::CodexCancellations { project_id, .. }
            | WorkspaceQuery::CodexCancellation { project_id, .. } => project_id,
            _ => return Err("停止审阅查询类型不符".into()),
        };
        identifier(project)?;
        require(
            !self.base.poisoned && self.runs.healthy(),
            "原停止证据写入尚未核对，请保留记录并重开",
        )?;
        #[cfg(windows)]
        let jobs = {
            require(self.api_fault.is_none(), "原项目协调器需要重开核对")?;
            require(
                !self
                    .active
                    .iter()
                    .any(|(id, active)| id == project && !active.shared)
                    && !self
                        .desktop_active
                        .iter()
                        .any(|active| active.snapshot.project.id == *project),
                "原项目由非共享活动写者持有，停止审阅暂不可用",
            )?;
            let mut jobs = Vec::new();
            for ((id, attempt), active) in self
                .codex_active
                .iter()
                .filter(|((id, _), _)| id == project)
            {
                require(
                    active.job.project_id == *id && active.job.attempt_id == *attempt,
                    "原编程执行身份不符",
                )?;
                if !active.disconnected
                    && active.cancel.is_some()
                    && matches!(
                        active.job.state,
                        CodexJobState::Starting | CodexJobState::Executing
                    )
                {
                    jobs.push(active.job.clone());
                }
            }
            jobs
        };
        #[cfg(not(windows))]
        let jobs: Vec<CodexJob> = Vec::new();
        let jobs_version = hash(&jobs)?;
        #[cfg(windows)]
        let basis = if let Some(owner) = self.api_writers.get(project) {
            require(!owner.interrupted, "原项目协调器已中断，请重开核对")?;
            owner
                .owner
                .workspace_codex_cancellation_basis(project, jobs)?
        } else {
            require(
                !self.active.contains_key(project)
                    && !self.api_active.keys().any(|(id, _)| id == project)
                    && !self.codex_active.keys().any(|(id, _)| id == project)
                    && !self.omp_active.keys().any(|(id, _)| id == project),
                "原项目由非共享活动写者持有，停止审阅暂不可用",
            )?;
            original_basis(
                self.projects
                    .get(project)
                    .ok_or("原项目不存在，未创建替代记录")?,
                project,
                &jobs,
            )?
        };
        #[cfg(not(windows))]
        let basis = original_basis(
            self.projects
                .get(project)
                .ok_or("原项目不存在，未创建替代记录")?,
            project,
            &jobs,
        )?;
        let revision = hash(&(
            "workspace-codex-cancellation-v1",
            project,
            &basis.version,
            &jobs_version,
            self.runs.sequence(),
            self.runs.evidence_digest(),
            &basis.entries,
        ))?;
        result(query, project, revision, basis.entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(index: usize) -> CodexCancellationReview {
        CodexCancellationReview {
            project_id: "project".into(),
            task_id: format!("task-{index}"),
            attempt_id: format!("attempt-{index}"),
            evidence_digest: "a".repeat(64),
        }
    }

    #[test]
    fn workspace_codex_cancellation_synthetic_page_count_and_byte_boundaries() {
        let revision = "a".repeat(64);
        let entries: Vec<_> = (0..21).map(entry).collect();
        let query = WorkspaceQuery::CodexCancellations {
            project_id: "project".into(),
            offset: 0,
            revision: None,
        };
        let WorkspaceResult::CodexCancellations(page) =
            result(&query, "project", revision.clone(), entries.clone()).unwrap()
        else {
            panic!("page")
        };
        assert_eq!(page.entries.len(), 20);
        assert_eq!(page.next_offset, Some(20));
        assert_eq!(page.total, 21);
        let WorkspaceResult::CodexCancellations(last) = result(
            &WorkspaceQuery::CodexCancellations {
                project_id: "project".into(),
                offset: 20,
                revision: Some(revision.clone()),
            },
            "project",
            revision.clone(),
            entries,
        )
        .unwrap() else {
            panic!("last page")
        };
        assert_eq!(last.entries.len(), 1);
        assert_eq!(last.entries[0].attempt_id, "attempt-20");
        assert_eq!(last.next_offset, None);
        let mut exact = entry(0);
        let overhead = serde_json::to_vec(&exact).unwrap().len() - exact.evidence_digest.len();
        exact.evidence_digest = "x".repeat(64 * 1024 - 1024 - overhead - 1);
        let WorkspaceResult::CodexCancellations(page) = result(
            &query,
            "project",
            revision.clone(),
            vec![exact.clone(), entry(1)],
        )
        .unwrap() else {
            panic!("byte boundary")
        };
        assert_eq!(page.entries.len(), 1);
        assert_eq!(page.next_offset, Some(1));
        assert!(
            serde_json::to_vec(&WorkspaceResult::CodexCancellations(page))
                .unwrap()
                .len()
                <= 64 * 1024
        );
        exact.evidence_digest.push('x');
        assert!(result(&query, "project", revision.clone(), vec![exact]).is_err());
        let detail_query = WorkspaceQuery::CodexCancellation {
            project_id: "project".into(),
            attempt_id: "attempt-0".into(),
            revision: revision.clone(),
        };
        let base = WorkspaceResult::CodexCancellation(Box::new(WorkspaceCodexCancellationDetail {
            project_id: "project".into(),
            revision: revision.clone(),
            review: entry(0),
        }));
        let base_size = serde_json::to_vec(&base).unwrap().len();
        let mut exact = entry(0);
        exact
            .evidence_digest
            .push_str(&"x".repeat(crate::workspace_read::RESULT_BYTES - base_size));
        let boundary = result(
            &detail_query,
            "project",
            revision.clone(),
            vec![exact.clone()],
        )
        .unwrap();
        assert_eq!(
            serde_json::to_vec(&boundary).unwrap().len(),
            crate::workspace_read::RESULT_BYTES
        );
        exact.evidence_digest.push('x');
        assert!(result(&detail_query, "project", revision, vec![exact]).is_err());
    }
}
