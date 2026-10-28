//! Bounded, read-only selected project-run records.
//!
//! Run-control records are authoritative user/core records. This module never
//! obtains a Snapshot: it hashes the durable project basis and borrows ledger
//! records while constructing only the bounded page or one requested detail.
use super::*;
use cerulean_domain::{
    execution::{identifier, require, sha256},
    project_setup::{ProjectRun, RunState},
    workspace::{
        WorkspaceProjectRunDetail, WorkspaceProjectRunPage, WorkspaceProjectRunSummary,
        WorkspaceQuery, WorkspaceResult,
    },
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::Write;

pub(crate) struct OriginalBasis {
    pub(crate) version: String,
}

struct HashWriter(Sha256);
impl Write for HashWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Hash the complete authoritative project workflow without constructing an
/// ExecutionSnapshot. The coordinator calls this while it owns the workflow
/// writer, so a shared active writer is read from its actual durable owner.
pub(crate) fn original_basis(
    store: &crate::scheduler::DurableWorkflow,
    project_id: &str,
) -> Result<OriginalBasis, String> {
    identifier(project_id)?;
    require(
        !store.requires_reopen(),
        "原项目写入尚未核对，请保留记录并重开",
    )?;
    require(
        store
            .configuration()
            .execution_scope
            .as_ref()
            .is_some_and(|scope| scope.project.id == project_id),
        "原项目账本身份与指挥请求项目不符",
    )?;
    let mut hash = HashWriter(Sha256::new());
    serde_json::to_writer(
        &mut hash,
        &(
            "workspace-project-runs-original-basis-v1",
            project_id,
            store.configuration(),
            store.state(),
            store.revision(),
            store.approval(),
            store.history(),
        ),
    )
    .map_err(|error| error.to_string())?;
    Ok(OriginalBasis {
        version: format!("{:x}", hash.0.finalize()),
    })
}

struct RunVersion<'a> {
    original: &'a ProjectRun,
    state: RunState,
    message: &'a str,
}
impl Serialize for RunVersion<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        (&self.original, &self.state, self.message).serialize(serializer)
    }
}

fn projected<'a>(run: &'a ProjectRun, runner_error: Option<&'a String>) -> (RunState, &'a str) {
    if run.state == RunState::Active {
        if let Some(error) = runner_error {
            return (RunState::Attention, error.as_str());
        }
    }
    (run.state.clone(), run.message.as_str())
}

fn summary(run: &ProjectRun, runner_error: Option<&String>) -> WorkspaceProjectRunSummary {
    let (state, message) = projected(run, runner_error);
    WorkspaceProjectRunSummary {
        id: run.id.clone(),
        project_id: run.project_id.clone(),
        revision_digest: run.revision_digest.clone(),
        setup_digest: run.setup_digest.clone(),
        state,
        has_message: !message.is_empty(),
        acknowledged_decisions: run.acknowledged_decisions,
    }
}

impl RuntimeHost {
    pub(super) fn workspace_project_runs(
        &self,
        query: &WorkspaceQuery,
    ) -> Result<WorkspaceResult, String> {
        let project_id = match query {
            WorkspaceQuery::ProjectRuns { project_id, .. }
            | WorkspaceQuery::ProjectRun { project_id, .. } => project_id,
            _ => return Err("项目运行查询类型不符".into()),
        };
        identifier(project_id)?;
        require(
            self.runs.healthy(),
            "运行控制写入尚未核对，请保留记录并重开",
        )?;
        require(!self.base.poisoned, "原项目写入尚未核对，请保留记录并重开")?;
        #[cfg(windows)]
        require(self.api_fault.is_none(), "原项目协调器需要重开核对")?;

        let basis = {
            #[cfg(windows)]
            {
                if let Some(owner) = self.api_writers.get(project_id) {
                    require(!owner.interrupted, "原项目协调器已中断，请重开核对")?;
                    owner.owner.workspace_project_runs_basis(project_id)?
                } else {
                    require(
                        !self.active.values().any(|active| {
                            active.snapshot.project.id == *project_id
                                && (!active.shared || !self.api_writers.contains_key(project_id))
                        }) && !self.api_active.values().any(|active| {
                            active.snapshot.project.id == *project_id
                                && !self.api_writers.contains_key(project_id)
                        }) && !self
                            .desktop_active
                            .iter()
                            .any(|active| active.snapshot.project.id == *project_id)
                            && !self.omp_active.values().any(|active| {
                                active.job.project_id == *project_id
                                    && !self.api_writers.contains_key(project_id)
                            })
                            && !self.codex_active.values().any(|active| {
                                active.job.project_id == *project_id
                                    && !self.api_writers.contains_key(project_id)
                            }),
                        "原项目由非共享活动写者持有，运行记录暂不可用",
                    )?;
                    original_basis(
                        self.projects
                            .get(project_id)
                            .ok_or("原项目不存在，未创建替代记录")?,
                        project_id,
                    )?
                }
            }
            #[cfg(not(windows))]
            {
                original_basis(
                    self.projects
                        .get(project_id)
                        .ok_or("原项目不存在，未创建替代记录")?,
                    project_id,
                )?
            }
        };

        let runner_error = self.runner_error.as_ref();
        let mut hash = HashWriter(Sha256::new());
        serde_json::to_writer(
            &mut hash,
            &(
                "workspace-project-runs-v1",
                project_id,
                &basis.version,
                self.runs.sequence(),
                self.runs.evidence_digest(),
                runner_error,
            ),
        )
        .map_err(|error| error.to_string())?;
        let mut total = 0usize;
        let mut active_count = 0usize;
        for run in self.runs.project_runs(project_id) {
            require(run.project_id == *project_id, "运行记录原项目身份不一致")?;
            identifier(&run.id)?;
            sha256(&run.revision_digest)?;
            sha256(&run.setup_digest)?;
            let (state, message) = projected(run, runner_error);
            if state == RunState::Active {
                active_count += 1;
            }
            serde_json::to_writer(
                &mut hash,
                &RunVersion {
                    original: run,
                    state,
                    message,
                },
            )
            .map_err(|error| error.to_string())?;
            total += 1;
        }
        let revision = format!("{:x}", hash.0.finalize());
        let total_u32 = u32::try_from(total).map_err(|_| "运行记录数量超出范围")?;
        let active_u32 = u32::try_from(active_count).map_err(|_| "活动运行数量超出范围")?;

        let result = match query {
            WorkspaceQuery::ProjectRuns {
                offset,
                revision: expected,
                ..
            } => {
                let entries = self
                    .runs
                    .project_runs(project_id)
                    .map(|run| summary(run, runner_error));
                let (entries, next_offset) = crate::workspace_read::page(
                    entries,
                    total,
                    *offset,
                    expected.as_deref(),
                    &revision,
                )?;
                WorkspaceResult::ProjectRuns(WorkspaceProjectRunPage {
                    project_id: project_id.clone(),
                    revision,
                    offset: *offset,
                    total: total_u32,
                    next_offset,
                    active_count: active_u32,
                    entries,
                })
            }
            WorkspaceQuery::ProjectRun {
                run_id,
                revision: expected,
                ..
            } => {
                identifier(run_id)?;
                sha256(expected)?;
                require(expected == &revision, "原项目运行记录已变化，请刷新首页")?;
                let run = self
                    .runs
                    .project_runs(project_id)
                    .find(|run| run.id == *run_id)
                    .ok_or("原项目运行记录不存在，未创建替代记录")?;
                let (state, message) = projected(run, runner_error);
                let mut record = run.clone();
                record.state = state;
                record.message = message.to_owned();
                WorkspaceResult::ProjectRun(Box::new(WorkspaceProjectRunDetail {
                    project_id: project_id.clone(),
                    revision,
                    record,
                }))
            }
            _ => return Err("项目运行查询类型不符".into()),
        };
        crate::workspace_read::bounded(result)
    }
}
