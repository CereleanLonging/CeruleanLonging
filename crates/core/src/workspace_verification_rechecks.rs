//! Bounded discovery of original recheck approvals, without ticking scheduling.
use super::*;

impl RuntimeHost {
    pub(super) fn workspace_verification_rechecks(
        &self,
        query: &WorkspaceQuery,
    ) -> Result<WorkspaceResult, String> {
        require(!self.base.poisoned, "原写入尚未核对，请保留记录并重开")?;
        let result = match query {
            WorkspaceQuery::CurrentVerificationRecheck { project_id } => {
                WorkspaceResult::CurrentVerificationRecheck(
                    self.current_verification_recheck(project_id)?.map(Box::new),
                )
            }
            WorkspaceQuery::RegisteredValidator {
                id,
                digest: expected,
            } => {
                identifier(id)?;
                cerulean_domain::execution::sha256(expected)?;
                let tool = self.registered_validator(id)?;
                require(
                    digest(tool)? == *expected,
                    "原工具登记内容已变化，请重新读取",
                )?;
                WorkspaceResult::RegisteredValidator(Box::new(tool.clone()))
            }
            WorkspaceQuery::VerificationRecheckInputs {
                project_id,
                original_operation_id,
            } => {
                identifier(project_id)?;
                identifier(original_operation_id)?;
                self.require_available(project_id)?;
                let store = self.projects.get(project_id).ok_or("原项目尚未恢复")?;
                require(!store.requires_reopen(), "项目写入未确认，请先重开核对")?;
                let job = self
                    .jobs
                    .get(&(project_id.clone(), original_operation_id.clone()))
                    .ok_or("原验收操作不存在")?;
                let task = store
                    .state()
                    .tasks
                    .get(&job.task_id)
                    .ok_or("原任务不存在")?;
                require(
                    job.state == VerificationState::Failed
                        && task.state == TaskState::Submitted
                        && !task.verified
                        && task.attempt.as_deref() == Some(job.attempt_id.as_str())
                        && store.state().approved
                        && store.state().revision_requested.is_none(),
                    "仅当前已提交且未通过的原失败验收可准备复验",
                )?;
                let plan = store.revision().ok_or("原方案缺失")?;
                require(
                    plan.project.id == *project_id
                        && job.project_id == *project_id
                        && job.operation_id == *original_operation_id,
                    "原失败验收身份不一致",
                )?;
                let checks = &plan.task(&job.task_id)?.checks;
                require(
                    !checks.is_empty() && checks.len() <= 64 && self.validators.len() <= 32,
                    "原检查器或登记工具数量超出复验范围",
                )?;
                let validators = self
                    .validators
                    .values()
                    .map(|tool| {
                        Ok(WorkspaceValidatorSummary {
                            id: tool.id.clone(),
                            name: tool.name.clone(),
                            digest: digest(tool)?,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                WorkspaceResult::VerificationRecheckInputs(Box::new(WorkspaceRecheckInputs {
                    original_job: job.clone(),
                    revision_digest: plan.approval_digest()?,
                    original_checks_digest: digest(checks)?,
                    checks: checks.clone(),
                    max_timeout_seconds: plan.policy.max_seconds,
                    validators,
                }))
            }
            WorkspaceQuery::VerificationRecheckDispositionReview {
                project_id,
                operation_id,
                reason,
            } => WorkspaceResult::VerificationRecheckDispositionReview(Box::new(
                self.read_recheck_disposition_review(project_id, operation_id, reason)?,
            )),
            WorkspaceQuery::VerificationRecheckReview { input } => {
                WorkspaceResult::VerificationRecheckReview(Box::new(
                    self.review_verification_recheck(input)?,
                ))
            }
            WorkspaceQuery::VerificationRecheck {
                project_id,
                operation_id,
            } => WorkspaceResult::VerificationRecheck(Box::new(
                self.read_verification_recheck(project_id, operation_id)?,
            )),
            WorkspaceQuery::VerificationRechecks {
                project_id,
                offset,
                revision,
            } => {
                identifier(project_id)?;
                // A writer may own the complete event history. An incomplete
                // live snapshot must never be advertised as an empty history.
                #[cfg(windows)]
                require(
                    self.api_fault.is_none()
                        && !self.api_writers.contains_key(project_id)
                        && !self
                            .active
                            .values()
                            .any(|a| a.snapshot.project.id == *project_id)
                        && !self
                            .desktop_active
                            .iter()
                            .any(|a| a.snapshot.project.id == *project_id),
                    "原批准历史由活动写者持有，请稍后读取；可继续核对已选复验",
                )?;
                let store = self.projects.get(project_id).ok_or("原项目尚未恢复")?;
                require(!store.requires_reopen(), "项目写入未确认，请先重开核对")?;
                let scope = store
                    .configuration()
                    .execution_scope
                    .as_ref()
                    .ok_or("原项目范围不存在")?;
                require(scope.project.id == *project_id, "原项目身份不一致")?;
                let version = crate::workspace_history::version(&(
                    "workspace-verification-rechecks-v1",
                    project_id,
                    store.history().len(),
                    store.history().last().map(|e| &e.hash),
                ))?;
                let approvals = store.history().iter().filter_map(|entry| {
                    entry
                        .body
                        .context
                        .verification_recheck
                        .as_deref()
                        .map(|a| (entry, a))
                });
                let total = approvals.clone().count();
                let mut ids = std::collections::BTreeSet::new();
                for (entry, a) in approvals.clone() {
                    require(
                        a.review.project_id == *project_id
                            && entry.body.event.principal == "user"
                            && ids.insert(&a.operation_id),
                        "原复验批准身份或编号不一致",
                    )?;
                }
                // Only the selected page is projected. Large review envelopes
                // stay in the original history and are read individually.
                let (selected, next_offset) = crate::workspace_read::page(
                    approvals.clone().map(|(entry, a)| WorkspaceRecheckSummary {
                        operation_id: a.operation_id.clone(),
                        original_operation_id: a.review.original_operation_id.clone(),
                        task_id: a.review.task_id.clone(),
                        attempt_id: a.review.attempt_id.clone(),
                        approval_event: EventAnchor {
                            sequence: entry.body.event.sequence,
                            hash: entry.hash.clone(),
                        },
                        // Reserve the exact serialized digest width for page
                        // sizing, then hash only the selected original reviews.
                        review_digest: "0".repeat(64),
                    }),
                    total,
                    *offset,
                    revision.as_deref(),
                    &version,
                )?;
                let mut entries = selected;
                for (summary, (_, a)) in entries.iter_mut().zip(approvals.skip(*offset as usize)) {
                    summary.review_digest = digest(&a.review)?;
                }
                WorkspaceResult::VerificationRechecks(WorkspaceRecheckPage {
                    project_id: project_id.clone(),
                    revision: version,
                    offset: *offset,
                    total: total as u32,
                    next_offset,
                    entries,
                })
            }
            _ => return Err("复验查询类型不符".into()),
        };
        crate::workspace_read::bounded(result)
    }

    /// Discover one current operation without claiming to have read the complete
    /// history from a live writer. A new window can then select its exact status.
    fn current_verification_recheck(
        &self,
        project: &str,
    ) -> Result<Option<cerulean_domain::verification_recheck::RecheckStatus>, String> {
        identifier(project)?;
        #[cfg(windows)]
        require(
            self.api_fault.is_none()
                && !self.api_writers.contains_key(project)
                && !self
                    .desktop_active
                    .iter()
                    .any(|a| a.snapshot.project.id == project),
            "原项目协调器或写者需要核对，请稍后读取当前复验",
        )?;
        #[cfg(windows)]
        if let Some(active) = self.active.get(project) {
            return match crate::verification_detail::recheck_authority(&active.detail_basis) {
                Some((approval, _)) => self
                    .read_verification_recheck(project, &approval.operation_id)
                    .map(Some),
                None => Ok(None),
            };
        }
        let store = self
            .projects
            .get(project)
            .ok_or("原项目由其他写者持有或尚未恢复，请稍后读取")?;
        require(!store.requires_reopen(), "项目写入未确认，请先重开核对")?;
        match store.unresolved_verification_recheck() {
            Some(entry) => {
                let approval = entry
                    .body
                    .context
                    .verification_recheck
                    .as_ref()
                    .ok_or("原复验批准内容缺失")?;
                self.read_verification_recheck(project, &approval.operation_id)
                    .map(Some)
            }
            None => Ok(None),
        }
    }
}
