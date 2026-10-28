//! Independent bounded unsent review. Approval evidence remains the original host proof.
use super::*;
use serde::Serialize;
use sha2::{Digest, Sha256};

pub(crate) struct OriginalBasis {
    pub(crate) version: String,
    // Only identities and the original no-dispatch fact leave the durable owner.
    pub(crate) assigned: Vec<(String, String, bool)>,
}
struct HashWriter(Sha256);
impl std::io::Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn feed<T: Serialize>(hash: &mut HashWriter, value: &T) -> Result<(), String> {
    serde_json::to_writer(hash, value).map_err(|error| error.to_string())
}

pub(crate) fn original_basis(
    store: &crate::scheduler::DurableWorkflow,
    project: &str,
) -> Result<OriginalBasis, String> {
    identifier(project)?;
    require(!store.requires_reopen(), "项目账本需要重开核心恢复")?;
    require(
        store
            .configuration()
            .execution_scope
            .as_ref()
            .is_some_and(|scope| scope.project.id == project),
        "原项目账本身份与未派发审阅项目不符",
    )?;
    let mut hash = HashWriter(Sha256::new());
    feed(
        &mut hash,
        &(
            "workspace-unsent-assignments-original-basis-v1",
            project,
            store.configuration(),
            store.state(),
            store.revision(),
            store.approval(),
            store.history(),
        ),
    )?;
    let mut assigned = Vec::new();
    for (task, run) in &store.state().tasks {
        if !matches!(
            run.state,
            TaskState::Assigned
                | TaskState::Dispatching
                | TaskState::Running
                | TaskState::Cancelling
                | TaskState::Unknown
        ) {
            continue;
        }
        let attempt = run.attempt.as_ref().ok_or("原分配缺少尝试身份")?;
        identifier(task)?;
        identifier(attempt)?;
        let unsent = store.attempt_record(task, attempt).is_some()
            && !store.history().iter().any(|entry| {
                matches!(
                    &entry.body.event.command,
                    WorkflowCommand::DispatchIntent { attempt: original, .. } if original == attempt
                )
            });
        assigned.push((task.clone(), attempt.clone(), unsent));
    }
    Ok(OriginalBasis {
        version: format!("{:x}", hash.0.finalize()),
        assigned,
    })
}

impl RuntimeHost {
    pub(super) fn workspace_unsent_assignments(
        &self,
        query: &WorkspaceQuery,
    ) -> Result<WorkspaceResult, String> {
        let project = match query {
            WorkspaceQuery::UnsentAssignments { project_id, .. }
            | WorkspaceQuery::UnsentAssignment { project_id, .. } => project_id,
            _ => return Err("未派发审阅查询类型不符".into()),
        };
        identifier(project)?;
        require(
            !self.base.poisoned && self.runs.healthy() && self.management.healthy(),
            "原未派发证据写入尚未核对，请保留记录并重开",
        )?;
        #[cfg(windows)]
        require(self.api_fault.is_none(), "原项目协调器需要重开核对")?;
        #[cfg(windows)]
        let basis = {
            require(
                !self
                    .active
                    .iter()
                    .any(|(id, active)| id == project && !active.shared)
                    && !self
                        .desktop_active
                        .iter()
                        .any(|active| active.snapshot.project.id == *project),
                "原项目由非共享活动写者持有，未派发审阅暂不可用",
            )?;
            if let Some(owner) = self.api_writers.get(project) {
                require(!owner.interrupted, "原项目协调器已中断，请重开核对")?;
                owner.owner.workspace_unsent_assignments_basis(project)?
            } else {
                require(
                    !self.active.contains_key(project)
                        && !self.api_active.keys().any(|(id, _)| id == project)
                        && !self.codex_active.keys().any(|(id, _)| id == project)
                        && !self.omp_active.keys().any(|(id, _)| id == project),
                    "原项目由非共享活动写者持有，未派发审阅暂不可用",
                )?;
                original_basis(
                    self.projects
                        .get(project)
                        .ok_or("原项目不存在，未创建替代记录")?,
                    project,
                )?
            }
        };
        #[cfg(not(windows))]
        let basis = original_basis(
            self.projects
                .get(project)
                .ok_or("原项目不存在，未创建替代记录")?,
            project,
        )?;

        let mut entries = Vec::new();
        let mut unavailable_reason = None;
        // A shared owner is authoritative for the revision, but this RuntimeHost
        // has no original store with which to run the unsent_review guard.
        // Preserve its exact busy reason instead of consulting cached snapshots.
        #[cfg(windows)]
        let shared_owner = self.api_writers.contains_key(project);
        #[cfg(not(windows))]
        let shared_owner = false;
        if shared_owner {
            unavailable_reason = Some("执行或验收线程仍持有项目，不能关闭尝试".into());
        } else {
            for (task, attempt, unsent) in &basis.assigned {
                if !unsent {
                    if unavailable_reason.is_none() {
                        unavailable_reason = Some("原尝试已有派发意图，必须核对原执行结果".into());
                    }
                    continue;
                }
                match self.unsent_review(project, task, attempt) {
                    Ok(evidence_digest) => {
                        require(*unsent, "原分配派发证明不一致，请重开核对")?;
                        entries.push(UnsentAssignmentReview {
                            project_id: project.clone(),
                            task_id: task.clone(),
                            attempt_id: attempt.clone(),
                            evidence_digest,
                        });
                    }
                    Err(reason) => {
                        if unavailable_reason.is_none() {
                            unavailable_reason = Some(reason);
                        }
                    }
                }
            }
        }
        let mut hash = HashWriter(Sha256::new());
        feed(
            &mut hash,
            &(
                "workspace-unsent-assignments-v1",
                project,
                &basis.version,
                &basis.assigned,
                self.runs.sequence(),
                self.runs.evidence_digest(),
                self.management.workspace_basis(project)?,
                &entries,
                &unavailable_reason,
            ),
        )?;
        for run in self
            .runs
            .all()
            .iter()
            .filter(|run| run.project_id == *project)
        {
            feed(&mut hash, run)?;
        }
        for operation in self.management_active.keys() {
            if self
                .management
                .intent(operation)
                .is_ok_and(|intent| intent.project_id == *project)
            {
                feed(&mut hash, &("management-active", operation))?;
            }
        }
        for job in self.jobs.values().filter(|job| job.project_id == *project) {
            feed(&mut hash, job)?;
        }
        for job in self
            .codex_jobs
            .values()
            .filter(|job| job.project_id == *project)
        {
            feed(&mut hash, job)?;
        }
        for job in self
            .omp_jobs
            .values()
            .filter(|job| job.project_id == *project)
        {
            feed(&mut hash, job)?;
        }
        #[cfg(windows)]
        {
            feed(
                &mut hash,
                &(
                    self.has_api_activity(project),
                    self.has_codex_activity(project),
                    self.has_omp_activity(project),
                    self.active.contains_key(project),
                    self.desktop_active
                        .as_ref()
                        .is_some_and(|active| active.snapshot.project.id == *project),
                ),
            )?;
            for key in self.api_active.keys().filter(|(id, _)| id == project) {
                feed(&mut hash, &("api-active", key))?;
            }
            for key in self.codex_active.keys().filter(|(id, _)| id == project) {
                feed(&mut hash, &("codex-active", key))?;
            }
            for key in self.omp_active.keys().filter(|(id, _)| id == project) {
                feed(&mut hash, &("omp-active", key))?;
            }
            for job in self
                .api_jobs
                .values()
                .filter(|job| job.project_id == *project)
            {
                feed(&mut hash, job)?;
            }
        }
        let revision = format!("{:x}", hash.0.finalize());
        let result = match query {
            WorkspaceQuery::UnsentAssignments {
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
                WorkspaceResult::UnsentAssignments(WorkspaceUnsentAssignmentPage {
                    project_id: project.clone(),
                    revision,
                    offset: *offset,
                    total: total as u32,
                    next_offset,
                    entries,
                    unavailable_reason,
                })
            }
            WorkspaceQuery::UnsentAssignment {
                task_id,
                attempt_id,
                revision: expected,
                ..
            } => {
                identifier(task_id)?;
                identifier(attempt_id)?;
                sha256(expected)?;
                require(expected == &revision, "原未派发审阅已变化，请刷新首页")?;
                let record = entries
                    .into_iter()
                    .find(|entry| entry.task_id == *task_id && entry.attempt_id == *attempt_id)
                    .ok_or_else(|| {
                        unavailable_reason.unwrap_or_else(|| "原未派发审阅不存在".into())
                    })?;
                WorkspaceResult::UnsentAssignment(Box::new(WorkspaceUnsentAssignmentDetail {
                    project_id: project.clone(),
                    revision,
                    record,
                }))
            }
            _ => return Err("未派发审阅查询类型不符".into()),
        };
        crate::workspace_read::bounded(result)
    }
}
