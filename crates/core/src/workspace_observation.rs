//! Revision-bound worker-observation reads without allocating a full Snapshot.
use super::*;
use cerulean_domain::{
    execution::{identifier, require, sha256},
    supervision::SupervisionPolicy,
    worker_observation::{WorkerObservationTrace, WorkerObservationView},
    workspace::{
        WorkspaceObservationDetail, WorkspaceObservationEventPage,
        WorkspaceObservationIncidentPage, WorkspaceQuery, WorkspaceResult,
    },
};

pub(crate) struct OriginalBasis {
    pub project_version: String,
    pub policy: Option<SupervisionPolicy>,
}

pub(crate) fn original_policy(
    store: &DurableWorkflow,
    job: &CodexJob,
    trace: &WorkerObservationTrace,
) -> Result<Option<SupervisionPolicy>, String> {
    let revision = store
        .history()
        .iter()
        .find_map(|entry| match &entry.body.context.execution {
            Some(ExecutionEvidence::Revision { revision })
                if revision
                    .approval_digest()
                    .is_ok_and(|digest| digest == job.revision_digest) =>
            {
                Some(revision)
            }
            _ => None,
        })
        .ok_or("观察缺少原方案证据")?;
    let policy = revision.supervision_for(&job.task_id)?.cloned();
    require(
        trace.thread_id.is_some() == trace.turn_id.is_some()
            && (trace.events.is_empty() || trace.turn_id.is_some()),
        "观察轮次关联无效",
    )?;
    if let (Some(thread), Some(turn)) = (&trace.thread_id, &trace.turn_id) {
        require(
            store.history().iter().any(|entry| {
                matches!(&entry.body.event.command, WorkflowCommand::Started { task, attempt }
                    if task == &job.task_id && attempt == &job.attempt_id)
                    && entry.body.context.thread_id.as_ref() == Some(thread)
                    && entry.body.context.turn_id.as_ref() == Some(turn)
            }),
            "观察缺少原执行轮次证据",
        )?;
    }
    Ok(policy)
}

pub(crate) fn original_basis(
    store: &DurableWorkflow,
    job: &CodexJob,
    trace: &WorkerObservationTrace,
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
    require(project.id == job.project_id, "观察原项目身份不符")?;
    let policy = original_policy(store, job, trace)?;
    let project_version = crate::workspace_history::version(&(
        "workspace-worker-observation-project-v1",
        project,
        store.configuration(),
        store.state(),
        store.revision(),
        store.approval(),
        store.history(),
    ))?;
    Ok(OriginalBasis {
        project_version,
        policy,
    })
}

fn query_revision(project_version: &str, view: &WorkerObservationView) -> Result<String, String> {
    crate::workspace_history::version(&("workspace-worker-observation-v1", project_version, view))
}
fn detail(
    project_version: &str,
    view: &WorkerObservationView,
) -> Result<WorkspaceObservationDetail, String> {
    Ok(WorkspaceObservationDetail {
        project_id: view.project_id.clone(),
        task_id: view.task_id.clone(),
        attempt_id: view.attempt_id.clone(),
        revision: query_revision(project_version, view)?,
        revision_digest: view.revision_digest.clone(),
        evidence_digest: view.evidence_digest.clone(),
        storage_error: view.storage_error.clone(),
        supervision: view.supervision.clone(),
        thread_id: view.trace.thread_id.clone(),
        turn_id: view.trace.turn_id.clone(),
        dropped: view.trace.dropped,
        rejected: view.trace.rejected,
        duplicates: view.trace.duplicates,
        finished: view.trace.finished,
        event_count: u32::try_from(view.trace.events.len()).map_err(|_| "观察事件数量超出范围")?,
        incident_count: u32::try_from(view.incidents.len()).map_err(|_| "观察异常数量超出范围")?,
    })
}

impl RuntimeHost {
    pub(super) fn workspace_observation(
        &self,
        query: &WorkspaceQuery,
    ) -> Result<WorkspaceResult, String> {
        let (project_id, attempt_id) = match query {
            WorkspaceQuery::WorkerObservation {
                project_id,
                attempt_id,
                ..
            }
            | WorkspaceQuery::WorkerObservationEvents {
                project_id,
                attempt_id,
                ..
            }
            | WorkspaceQuery::WorkerObservationIncidents {
                project_id,
                attempt_id,
                ..
            } => (project_id, attempt_id),
            _ => return Err("观察查询类型不符".into()),
        };
        identifier(project_id)?;
        identifier(attempt_id)?;
        require(!self.base.poisoned, "原项目写入尚未核对，请保留记录并重开")?;
        #[cfg(windows)]
        require(self.api_fault.is_none(), "原项目协调器需要重开核对")?;
        let job = self
            .codex_jobs
            .get(&(project_id.clone(), attempt_id.clone()))
            .ok_or("执行记录不存在")?;
        let (view, project_version) = {
            #[cfg(windows)]
            if let Some(active) = self
                .codex_active
                .get(&(project_id.clone(), attempt_id.clone()))
            {
                let capture = active.observation.as_ref().ok_or("原执行观察尚不可用")?;
                require(
                    active.observation_error.is_none(),
                    "执行观察存储错误，原记录可能不完整",
                )?;
                let trace = capture.snapshot();
                let owner = self.api_writers.get(project_id).ok_or("原项目协调器缺失")?;
                require(!owner.interrupted, "原项目协调器需要重开核对")?;
                let basis = owner
                    .owner
                    .workspace_observation_basis(job, trace.clone())?;
                let view = crate::worker_observation::view(job, trace, None, basis.policy)?;
                (view, basis.project_version)
            } else {
                let view = crate::worker_observation::read(&self.root, job)
                    .map_err(|error| format!("原执行观察不可用：{error}"))?;
                require(
                    view.storage_error.is_none(),
                    "执行观察存储错误，原记录可能不完整",
                )?;
                let project_version = if let Some(owner) = self.api_writers.get(project_id) {
                    require(!owner.interrupted, "原项目协调器需要重开核对")?;
                    let basis = owner
                        .owner
                        .workspace_observation_basis(job, view.trace.clone())?;
                    require(
                        basis.policy.as_ref() == view.supervision.as_ref(),
                        "观察策略与原方案不符",
                    )?;
                    basis.project_version
                } else {
                    require(
                        !self
                            .active
                            .values()
                            .any(|a| a.snapshot.project.id == *project_id && !a.shared)
                            && !self
                                .desktop_active
                                .iter()
                                .any(|a| a.snapshot.project.id == *project_id)
                            && !self
                                .codex_active
                                .values()
                                .any(|a| a.job.project_id == *project_id),
                        "原项目由活动写者持有，观察暂不可用；请保留原记录",
                    )?;
                    let store = self
                        .projects
                        .get(project_id)
                        .ok_or("原项目账本不可用，未创建替代记录")?;
                    let basis = original_basis(store, job, &view.trace)?;
                    require(
                        basis.policy.as_ref() == view.supervision.as_ref(),
                        "观察策略与原方案不符",
                    )?;
                    basis.project_version
                };
                (view, project_version)
            }
            #[cfg(not(windows))]
            {
                let view = crate::worker_observation::read(&self.root, job)
                    .map_err(|error| format!("原执行观察不可用：{error}"))?;
                require(
                    view.storage_error.is_none(),
                    "执行观察存储错误，原记录可能不完整",
                )?;
                let store = self
                    .projects
                    .get(project_id)
                    .ok_or("原项目账本不可用，未创建替代记录")?;
                let basis = original_basis(store, job, &view.trace)?;
                require(
                    basis.policy.as_ref() == view.supervision.as_ref(),
                    "观察策略与原方案不符",
                )?;
                (view, basis.project_version)
            }
        };
        require(
            view.project_id == *project_id && view.attempt_id == *attempt_id,
            "观察记录身份不符",
        )?;
        let metadata = detail(&project_version, &view)?;
        match query {
            WorkspaceQuery::WorkerObservation {
                revision: expected, ..
            } => {
                if let Some(expected) = expected {
                    sha256(expected)?;
                    require(
                        expected == &metadata.revision,
                        "原执行观察或项目状态已变化，请刷新首页",
                    )?;
                }
                crate::workspace_read::bounded(WorkspaceResult::WorkerObservation(Box::new(
                    metadata,
                )))
            }
            WorkspaceQuery::WorkerObservationEvents {
                offset,
                revision: expected,
                ..
            } => {
                sha256(expected)?;
                require(
                    expected == &metadata.revision,
                    "原执行观察或项目状态已变化，请刷新首页",
                )?;
                let (entries, next_offset) = crate::workspace_read::page(
                    view.trace.events.iter().cloned(),
                    view.trace.events.len(),
                    *offset,
                    Some(expected),
                    &metadata.revision,
                )?;
                crate::workspace_read::bounded(WorkspaceResult::WorkerObservationEvents(
                    WorkspaceObservationEventPage {
                        project_id: project_id.clone(),
                        attempt_id: attempt_id.clone(),
                        revision: metadata.revision,
                        offset: *offset,
                        total: u32::try_from(view.trace.events.len())
                            .map_err(|_| "观察事件数量超出范围")?,
                        next_offset,
                        entries,
                    },
                ))
            }
            WorkspaceQuery::WorkerObservationIncidents {
                offset,
                revision: expected,
                ..
            } => {
                sha256(expected)?;
                require(
                    expected == &metadata.revision,
                    "原执行观察或项目状态已变化，请刷新首页",
                )?;
                let (entries, next_offset) = crate::workspace_read::page(
                    view.incidents.iter().cloned(),
                    view.incidents.len(),
                    *offset,
                    Some(expected),
                    &metadata.revision,
                )?;
                crate::workspace_read::bounded(WorkspaceResult::WorkerObservationIncidents(
                    WorkspaceObservationIncidentPage {
                        project_id: project_id.clone(),
                        attempt_id: attempt_id.clone(),
                        revision: metadata.revision,
                        offset: *offset,
                        total: u32::try_from(view.incidents.len())
                            .map_err(|_| "观察异常数量超出范围")?,
                        next_offset,
                        entries,
                    },
                ))
            }
            _ => Err("观察查询类型不符".into()),
        }
    }
}

#[cfg(test)]
#[path = "workspace_observation_tests.rs"]
mod tests;
