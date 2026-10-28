//! Bounded selected-project revision review, read from the original durable owner.
use super::*;
use cerulean_domain::project_setup::ProjectRevisionReview;
use serde::{ser::SerializeSeq, Serialize};
use sha2::{Digest, Sha256};

pub(crate) struct OriginalBasis {
    pub(crate) version: String,
    pub(crate) last_hash: Option<String>,
    pub(crate) candidate: Result<ProjectRevisionReview, String>,
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
fn streamed<T: Serialize>(value: &T) -> Result<String, String> {
    let mut hash = HashWriter(Sha256::new());
    serde_json::to_writer(&mut hash, value).map_err(|error| error.to_string())?;
    Ok(format!("{:x}", hash.0.finalize()))
}

/// No Snapshot, encoded workflow buffer, or cached owner hash is authoritative.
pub(crate) fn original_basis(
    store: &crate::scheduler::DurableWorkflow,
    project: &str,
) -> Result<OriginalBasis, String> {
    identifier(project)?;
    require(
        !store.requires_reopen(),
        "原项目写入尚未核对，请保留记录并重开",
    )?;
    require(
        store
            .configuration()
            .execution_scope
            .as_ref()
            .is_some_and(|scope| scope.project.id == project),
        "原项目账本身份与修订审阅项目不符",
    )?;
    let version = streamed(&(
        "workspace-revision-reviews-original-basis-v1",
        project,
        store.configuration(),
        store.state(),
        store.revision(),
        store.approval(),
        store.history(),
    ))?;
    let state = store.state();
    let eligible = state.revision_requested.is_none()
        && state.approved
        && !state.tasks.is_empty()
        && state.tasks.values().all(|task| {
            matches!(
                task.state,
                TaskState::Pending
                    | TaskState::Accepted
                    | TaskState::Cancelled
                    | TaskState::Retired
                    | TaskState::Submitted
            )
        });
    let candidate = if eligible {
        let revision_digest = store.revision().ok_or("方案缺失")?.approval_digest()?;
        let mut review = ProjectRevisionReview {
            project_id: project.into(),
            revision_digest,
            evidence_digest: String::new(),
            accepted_tasks: 0,
            cancelled_tasks: 0,
            retired_tasks: 0,
            pending_tasks: 0,
            submitted_tasks: 0,
            verified_submissions: 0,
        };
        for task in state.tasks.values() {
            match task.state {
                TaskState::Accepted => review.accepted_tasks += 1,
                TaskState::Cancelled => review.cancelled_tasks += 1,
                TaskState::Retired => review.retired_tasks += 1,
                TaskState::Pending => review.pending_tasks += 1,
                TaskState::Submitted => {
                    review.submitted_tasks += 1;
                    if task.verified {
                        review.verified_submissions += 1;
                    }
                }
                _ => {}
            }
        }
        Ok(review)
    } else {
        Err("请先完成或处置已派发任务，再修订完整方案".into())
    };
    Ok(OriginalBasis {
        version,
        last_hash: store.history().last().map(|entry| entry.hash.clone()),
        candidate,
    })
}

// Preserve the original Vec<ProjectRun> JSON sequence/order without cloning it.
struct ProjectRuns<'a>(&'a crate::run_control::RunLedger, &'a str);
impl Serialize for ProjectRuns<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(None)?;
        for run in self.0.project_runs(self.1) {
            sequence.serialize_element(run)?;
        }
        sequence.end()
    }
}

impl RuntimeHost {
    pub(super) fn workspace_revision_reviews(
        &self,
        query: &WorkspaceQuery,
    ) -> Result<WorkspaceResult, String> {
        let project = match query {
            WorkspaceQuery::RevisionReviews { project_id, .. }
            | WorkspaceQuery::RevisionReview { project_id, .. } => project_id,
            _ => return Err("修订审阅查询类型不符".into()),
        };
        identifier(project)?;
        require(
            !self.base.poisoned
                && self.runs.healthy()
                && self.management.healthy()
                && self.advice_closures.healthy()
                && self.startups.healthy()
                && self.omp_startups.healthy(),
            "原修订证据写入尚未核对，请保留记录并重开",
        )?;
        #[cfg(windows)]
        require(self.api_fault.is_none(), "原项目协调器需要重开核对")?;
        #[cfg(windows)]
        let basis = {
            // An independent writer must never be replaced by a cached Snapshot,
            // even if a different shared owner is also registered.
            require(
                !self
                    .active
                    .values()
                    .any(|active| active.snapshot.project.id == *project && !active.shared)
                    && !self
                        .desktop_active
                        .iter()
                        .any(|active| active.snapshot.project.id == *project),
                "原项目由非共享活动写者持有，修订审阅暂不可用",
            )?;
            if let Some(owner) = self.api_writers.get(project) {
                require(!owner.interrupted, "原项目协调器已中断，请重开核对")?;
                owner.owner.workspace_revision_reviews_basis(project)?
            } else {
                require(
                    !self
                        .active
                        .values()
                        .any(|active| active.snapshot.project.id == *project)
                        && !self
                            .api_active
                            .values()
                            .any(|active| active.snapshot.project.id == *project)
                        && !self
                            .omp_active
                            .values()
                            .any(|active| active.job.project_id == *project)
                        && !self
                            .codex_active
                            .values()
                            .any(|active| active.job.project_id == *project),
                    "原项目由非共享活动写者持有，修订审阅暂不可用",
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

        // Keep the original guard precedence. Fault checks above are errors,
        // whereas a healthy known project's eligibility is explicit page data.
        let available = self.require_available(project).and_then(|()| {
            require(
                !self
                    .management
                    .statuses()
                    .iter()
                    .any(|status| status.project_id == *project && !status.state.resolved()),
                "项目有待处理指挥请求，请先处置",
            )
        });
        let candidate = available.and(basis.candidate);
        let (candidate, unavailable_reason) = match candidate {
            Ok(mut review) => {
                review.evidence_digest = streamed(&(
                    project,
                    &review.revision_digest,
                    &basis.last_hash,
                    ProjectRuns(&self.runs, project),
                    self.project_setups.get(project),
                ))?;
                (Some(review), None)
            }
            Err(reason) => (None, Some(reason)),
        };
        let mut hash = HashWriter(Sha256::new());
        serde_json::to_writer(
            &mut hash,
            &(
                "workspace-revision-reviews-v1",
                project,
                &basis.version,
                self.runs.sequence(),
                self.runs.evidence_digest(),
                ProjectRuns(&self.runs, project),
                self.project_setups.get(project),
                self.management.workspace_basis(project)?,
                &self.runner_error,
                &candidate,
                &unavailable_reason,
            ),
        )
        .map_err(|error| error.to_string())?;
        // Bind all durable records that determine the original availability
        // capabilities, including changes that leave the same guard message.
        for job in self.jobs.values().filter(|job| job.project_id == *project) {
            serde_json::to_writer(&mut hash, job).map_err(|error| error.to_string())?;
        }
        for job in self
            .codex_jobs
            .values()
            .filter(|job| job.project_id == *project)
        {
            serde_json::to_writer(&mut hash, job).map_err(|error| error.to_string())?;
        }
        for job in self
            .omp_jobs
            .values()
            .filter(|job| job.project_id == *project)
        {
            serde_json::to_writer(&mut hash, job).map_err(|error| error.to_string())?;
        }
        for record in self
            .startups
            .all()
            .iter()
            .filter(|record| record.intent.project_id == *project)
        {
            serde_json::to_writer(&mut hash, record).map_err(|error| error.to_string())?;
        }
        for record in self
            .omp_startups
            .all()
            .iter()
            .filter(|record| record.intent.project_id == *project)
        {
            serde_json::to_writer(&mut hash, record).map_err(|error| error.to_string())?;
        }
        for closure in self
            .advice_closures
            .all()
            .filter(|closure| closure.proof.project == *project)
        {
            serde_json::to_writer(&mut hash, closure).map_err(|error| error.to_string())?;
        }
        #[cfg(windows)]
        serde_json::to_writer(
            &mut hash,
            &(
                self.api_writers.contains_key(project),
                self.has_api_activity(project),
                self.has_codex_activity(project),
                self.has_omp_activity(project),
            ),
        )
        .map_err(|error| error.to_string())?;
        #[cfg(windows)]
        for key in self.api_active.keys().filter(|(id, _)| id == project) {
            serde_json::to_writer(&mut hash, &("api-active", key))
                .map_err(|error| error.to_string())?;
        }
        #[cfg(windows)]
        for key in self.codex_active.keys().filter(|(id, _)| id == project) {
            serde_json::to_writer(&mut hash, &("codex-active", key))
                .map_err(|error| error.to_string())?;
        }
        #[cfg(windows)]
        for key in self.omp_active.keys().filter(|(id, _)| id == project) {
            serde_json::to_writer(&mut hash, &("omp-active", key))
                .map_err(|error| error.to_string())?;
        }
        #[cfg(windows)]
        for job in self
            .api_jobs
            .values()
            .filter(|job| job.project_id == *project)
        {
            serde_json::to_writer(&mut hash, job).map_err(|error| error.to_string())?;
        }
        let revision = format!("{:x}", hash.0.finalize());
        let total = usize::from(candidate.is_some());
        let result = match query {
            WorkspaceQuery::RevisionReviews {
                offset,
                revision: expected,
                ..
            } => {
                let (entries, next_offset) = crate::workspace_read::page(
                    candidate,
                    total,
                    *offset,
                    expected.as_deref(),
                    &revision,
                )?;
                WorkspaceResult::RevisionReviews(WorkspaceRevisionReviewPage {
                    project_id: project.clone(),
                    revision,
                    offset: *offset,
                    total: total as u32,
                    next_offset,
                    entries,
                    unavailable_reason,
                })
            }
            WorkspaceQuery::RevisionReview {
                revision: expected, ..
            } => {
                sha256(expected)?;
                require(expected == &revision, "原修订审阅已变化，请刷新首页")?;
                let record = candidate.ok_or_else(|| {
                    unavailable_reason.unwrap_or_else(|| "原修订审阅不可用".into())
                })?;
                WorkspaceResult::RevisionReview(Box::new(WorkspaceRevisionReviewDetail {
                    project_id: project.clone(),
                    revision,
                    record,
                }))
            }
            _ => return Err("修订审阅查询类型不符".into()),
        };
        crate::workspace_read::bounded(result)
    }
}
