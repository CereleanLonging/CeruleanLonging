//! Bounded read-only views of one original verification, never a reconciliation.
use super::*;
use cerulean_domain::{verification_detail::VerificationDetail, workspace::*};

pub(crate) struct OriginalBasis {
    pub project_version: String,
    pub detail_basis: crate::verification_detail::Basis,
}
pub(crate) fn original_basis(
    store: &DurableWorkflow,
    job: &VerificationJob,
) -> Result<OriginalBasis, String> {
    require(
        !store.requires_reopen(),
        "原项目写入尚未核对，请保留记录并重开",
    )?;
    let project = &store
        .configuration()
        .execution_scope
        .as_ref()
        .ok_or("原项目范围不存在")?
        .project;
    require(project.id == job.project_id, "验收原项目身份不符")?;
    let detail_basis = crate::verification_detail::basis(store, job)?;
    let project_version = crate::workspace_history::version(&(
        "workspace-verification-project-v1",
        project,
        store.configuration(),
        store.state(),
        store.revision(),
        store.approval(),
        store.history(),
    ))?;
    Ok(OriginalBasis {
        project_version,
        detail_basis,
    })
}
fn query_revision(project_version: &str, detail: &VerificationDetail) -> Result<String, String> {
    // observed_at is the original reader's wall clock, not persisted evidence.
    // All other original fields, including complete arrays, bind the cursor.
    crate::workspace_history::version(&(
        "workspace-verification-result-v1",
        project_version,
        (
            &detail.project_id,
            &detail.operation_id,
            &detail.task_id,
            &detail.attempt_id,
            &detail.state,
            &detail.revision_digest,
            &detail.manifest_digest,
            &detail.snapshot_digest,
        ),
        (
            detail.ledger_recorded,
            &detail.evidence_digest,
            &detail.message,
            &detail.artifacts,
            &detail.stages,
            &detail.recovery,
            &detail.recovery_error,
            detail.can_retry_start,
        ),
    ))
}
fn metadata(
    detail: VerificationDetail,
    revision: String,
) -> Result<WorkspaceVerificationResultDetail, String> {
    Ok(WorkspaceVerificationResultDetail {
        artifact_count: u32::try_from(detail.artifacts.len())
            .map_err(|_| "验收产物数量超出范围")?,
        stage_count: u32::try_from(detail.stages.len()).map_err(|_| "验收阶段数量超出范围")?,
        project_id: detail.project_id,
        operation_id: detail.operation_id,
        task_id: detail.task_id,
        attempt_id: detail.attempt_id,
        state: detail.state,
        revision_digest: detail.revision_digest,
        manifest_digest: detail.manifest_digest,
        snapshot_digest: detail.snapshot_digest,
        ledger_recorded: detail.ledger_recorded,
        evidence_digest: detail.evidence_digest,
        observed_at: detail.observed_at,
        message: detail.message,
        recovery: detail.recovery,
        recovery_error: detail.recovery_error,
        can_retry_start: detail.can_retry_start,
        revision,
    })
}
impl RuntimeHost {
    pub(super) fn workspace_verification(
        &self,
        query: &WorkspaceQuery,
    ) -> Result<WorkspaceResult, String> {
        let (project_id, operation_id) = match query {
            WorkspaceQuery::VerificationResult {
                project_id,
                operation_id,
                ..
            }
            | WorkspaceQuery::VerificationResultArtifacts {
                project_id,
                operation_id,
                ..
            }
            | WorkspaceQuery::VerificationResultStages {
                project_id,
                operation_id,
                ..
            } => (project_id, operation_id),
            _ => return Err("验收结果查询类型不符".into()),
        };
        identifier(project_id)?;
        identifier(operation_id)?;
        require(!self.base.poisoned, "原项目写入尚未核对，请保留记录并重开")?;
        #[cfg(windows)]
        require(self.api_fault.is_none(), "原项目协调器需要重开核对")?;
        let job = self
            .jobs
            .get(&(project_id.clone(), operation_id.clone()))
            .ok_or("原验收记录不存在，未创建替代记录")?;
        require(
            job.project_id == *project_id && job.operation_id == *operation_id,
            "验收记录身份不符",
        )?;
        let basis = {
            #[cfg(windows)]
            {
                if let Some(owner) = self.api_writers.get(project_id) {
                    require(!owner.interrupted, "原项目协调器需要重开核对")?;
                    owner.owner.workspace_verification_basis(job)?
                } else {
                    require(
                        !self
                            .active
                            .values()
                            .any(|a| a.snapshot.project.id == *project_id)
                            && !self
                                .desktop_active
                                .iter()
                                .any(|a| a.snapshot.project.id == *project_id)
                            && !self
                                .codex_active
                                .values()
                                .any(|a| a.job.project_id == *project_id),
                        "原项目由非共享活动写者持有，验收结果暂不可用；请保留原记录",
                    )?;
                    original_basis(
                        self.projects
                            .get(project_id)
                            .ok_or("原项目账本不可用，未创建替代记录")?,
                        job,
                    )?
                }
            }
            #[cfg(not(windows))]
            {
                original_basis(
                    self.projects
                        .get(project_id)
                        .ok_or("原项目账本不可用，未创建替代记录")?,
                    job,
                )?
            }
        };
        let mut detail = crate::verification_detail::read(&self.root, job, &basis.detail_basis)?;
        // An entirely missing verification directory used to produce unknown
        // stages; the new result surface must explicitly refuse absent evidence.
        let mut evidence = self.root.clone();
        for part in [
            "workflows",
            project_id.as_str(),
            "verification",
            operation_id.as_str(),
        ] {
            evidence.push(part);
            ordinary(&evidence, true).map_err(|e| format!("原验收证据不可用：{e}"))?;
        }
        ordinary(&evidence.join("intent.json"), false)
            .map_err(|e| format!("原验收意图证据不可用：{e}"))?;
        if job.state == VerificationState::Unknown {
            match self.verification_recovery_review(job) {
                Ok(review) => detail.recovery = Some(review),
                Err(error) => detail.recovery_error = Some(error.chars().take(1000).collect()),
            }
        }
        #[cfg(windows)]
        {
            detail.can_retry_start = self
                .verification_start_retry_available(project_id, operation_id)
                .is_ok();
        }
        let revision = query_revision(&basis.project_version, &detail)?;
        let result = match query {
            WorkspaceQuery::VerificationResult {
                revision: expected, ..
            } => {
                if let Some(expected) = expected {
                    sha256(expected)?;
                    require(
                        expected == &revision,
                        "原验收结果或项目状态已变化，请刷新首页",
                    )?;
                }
                WorkspaceResult::VerificationResult(Box::new(metadata(detail, revision)?))
            }
            WorkspaceQuery::VerificationResultArtifacts {
                offset,
                revision: expected,
                ..
            } => {
                let (entries, next_offset) = crate::workspace_read::page(
                    detail.artifacts.iter(),
                    detail.artifacts.len(),
                    *offset,
                    Some(expected),
                    &revision,
                )?;
                WorkspaceResult::VerificationResultArtifacts(WorkspaceVerificationArtifactPage {
                    project_id: project_id.clone(),
                    operation_id: operation_id.clone(),
                    revision,
                    offset: *offset,
                    total: u32::try_from(detail.artifacts.len())
                        .map_err(|_| "验收产物数量超出范围")?,
                    next_offset,
                    entries: entries.into_iter().cloned().collect(),
                })
            }
            WorkspaceQuery::VerificationResultStages {
                offset,
                revision: expected,
                ..
            } => {
                let (entries, next_offset) = crate::workspace_read::page(
                    detail.stages.iter(),
                    detail.stages.len(),
                    *offset,
                    Some(expected),
                    &revision,
                )?;
                WorkspaceResult::VerificationResultStages(WorkspaceVerificationStagePage {
                    project_id: project_id.clone(),
                    operation_id: operation_id.clone(),
                    revision,
                    offset: *offset,
                    total: u32::try_from(detail.stages.len())
                        .map_err(|_| "验收阶段数量超出范围")?,
                    next_offset,
                    entries: entries.into_iter().cloned().collect(),
                })
            }
            _ => return Err("验收结果查询类型不符".into()),
        };
        crate::workspace_read::bounded(result)
    }
}

#[cfg(test)]
#[path = "workspace_verification_tests.rs"]
mod tests;

#[cfg(all(test, windows))]
pub(crate) fn production_shared_owner_transfer(host: &mut RuntimeHost, operation: &str) {
    tests::production_shared_owner_transfer(host, operation);
}
