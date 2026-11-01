//! Trusted preparation and separate durable user approval for corrected checks.
//! Approval only records authority; no process or model request is launched here.
use super::*;
use cerulean_domain::verification_recheck::{
    RecheckApproval, RecheckBasis, RecheckRequest, RecheckReview, RecheckStatus,
};

impl RuntimeHost {
    pub(super) fn read_recheck_disposition_review(
        &self,
        project: &str,
        operation: &str,
        reason: &str,
    ) -> Result<cerulean_domain::verification_recheck::RecheckDispositionReview, String> {
        #[cfg(windows)]
        {
            self.review_verification_recheck_disposition(project, operation, reason)
        }
        #[cfg(not(windows))]
        {
            let _ = (project, operation, reason);
            Err("复验处置需要Windows运行时".into())
        }
    }

    /// Reads retained approval authority without polling any project or worker.
    pub(super) fn read_verification_recheck(
        &self,
        project: &str,
        operation: &str,
    ) -> Result<RecheckStatus, String> {
        identifier(project)?;
        identifier(operation)?;
        let (approval, approval_event) = if let Some(store) = self.projects.get(project) {
            require(!store.requires_reopen(), "项目写入未确认，请先重开核对")?;
            let mut approvals = store.history().iter().filter_map(|entry| {
                entry
                    .body
                    .context
                    .verification_recheck
                    .as_ref()
                    .filter(|a| a.operation_id == operation)
                    .map(|a| {
                        (
                            a.as_ref().clone(),
                            EventAnchor {
                                sequence: entry.body.event.sequence,
                                hash: entry.hash.clone(),
                            },
                        )
                    })
            });
            let authority = approvals.next().ok_or("原复验批准不存在")?;
            require(approvals.next().is_none(), "复验批准编号冲突")?;
            authority
        } else {
            #[cfg(windows)]
            {
                let active = self
                    .active
                    .get(project)
                    .filter(|a| a.job.operation_id == operation)
                    .ok_or("原复验正在由其他操作持有，请稍后核对")?;
                crate::verification_detail::recheck_authority(&active.detail_basis)
                    .ok_or("该操作不是独立复验")?
            }
            #[cfg(not(windows))]
            {
                return Err("复验需要Windows运行时".into());
            }
        };
        require(approval.review.project_id == project, "复验批准项目不匹配")?;
        let job = self.jobs.get(&(project.into(), operation.into())).cloned();
        if let Some(job) = &job {
            require(
                job.task_id == approval.review.task_id
                    && job.attempt_id == approval.review.attempt_id,
                "复验状态与批准身份不匹配",
            )?;
        }
        let disposition = if let Some(store) = self.projects.get(project) {
            let entry = store
                .history()
                .iter()
                .find(|e| e.hash == approval_event.hash)
                .ok_or("原批准事件缺失")?;
            store.recheck_disposition(entry).map(|e| {
                let review = e.body.context.recheck_disposition.as_deref().ok_or("原处置审阅缺失")?;
                #[cfg(windows)]
                let observed = crate::recheck_disposition_evidence::fingerprint(&self.root, &approval);
                #[cfg(not(windows))]
                let observed: Result<String, String> = Err("复验处置证据核查需要Windows运行时".into());
                let evidence_unchanged = observed.as_ref().is_ok_and(|d| *d == review.evidence_digest);
                Ok::<_, String>(cerulean_domain::verification_recheck::RecheckDispositionReceipt {
                    event_operation_id: e.body.operation_id.clone(),
                    event: EventAnchor { sequence: e.body.event.sequence, hash: e.hash.clone() },
                    review: review.clone(),
                    evidence_unchanged,
                    message: (!evidence_unchanged).then(|| "原处置已记录，但当前证据变化或不可读取；请保留并核对，不得重新执行".into()),
                })
            }).transpose()?
        } else {
            None
        };
        Ok(RecheckStatus {
            review_digest: digest(&approval.review)?,
            approval,
            approval_event,
            job,
            disposition,
        })
    }

    pub(super) fn verification_recheck_command(&mut self, command: Command) -> Result<(), String> {
        #[cfg(windows)]
        {
            match command {
                Command::RetireVerificationRecheck {
                    review,
                    event_operation_id,
                } => self
                    .retire_verification_recheck(&review, &event_operation_id)
                    .map(|_| ()),
                Command::StartVerificationRecheck {
                    project_id,
                    operation_id,
                } => self.start_verification_recheck(&project_id, &operation_id),
                Command::RecoverVerificationRecheck {
                    project_id,
                    operation_id,
                } => self.recover_verification_recheck(&project_id, &operation_id),
                Command::CancelVerificationRecheck {
                    project_id,
                    operation_id,
                } => {
                    require(
                        self.is_verification_recheck(&project_id, &operation_id),
                        "原复验启动记录不存在",
                    )?;
                    self.cancel_verification(&project_id, &operation_id)
                }
                _ => Err("不支持的复验操作".into()),
            }
        }
        #[cfg(not(windows))]
        {
            let _ = command;
            Err("复验需要Windows运行时".into())
        }
    }

    /// Authenticated local-user entry. A repeated approval only returns its
    /// existing event. Neither this method nor recovery starts an execution.
    pub fn approve_verification_recheck(
        &mut self,
        review: &RecheckReview,
        approval_operation: &str,
        verification_operation: &str,
    ) -> Result<crate::scheduler::Entry, String> {
        identifier(approval_operation)?;
        identifier(verification_operation)?;
        identifier(&review.project_id)?;
        require(
            approval_operation != verification_operation,
            "批准与复验必须使用不同操作编号",
        )?;
        crate::verification_recheck_guard::require_protected(&self.root, true)?;
        let command = WorkflowCommand::ApproveVerificationRecheck {
            task: review.task_id.clone(),
            attempt: review.attempt_id.clone(),
            operation_id: verification_operation.into(),
            review_digest: digest(review)?,
        };
        let store = self
            .projects
            .get(&review.project_id)
            .ok_or("原项目尚未恢复")?;
        require(!store.requires_reopen(), "项目写入未确认，请先重开核对")?;
        if let Some(saved) = store
            .history()
            .iter()
            .find(|e| e.body.operation_id == approval_operation)
        {
            require(
                saved.body.event.principal == "user"
                    && saved.body.event.command == command
                    && saved
                        .body
                        .context
                        .verification_recheck
                        .as_ref()
                        .is_some_and(|a| {
                            a.review == *review && a.operation_id == verification_operation
                        }),
                "原批准编号已用于其他内容，不得替换或重发",
            )?;
            return Ok(saved.clone());
        }
        self.require_current_verification_recheck(review)?;
        require(
            !self
                .jobs
                .contains_key(&(review.project_id.clone(), verification_operation.into())),
            "复验操作编号已有记录，请核对原操作",
        )?;
        for path in [
            self.root
                .join("workflows")
                .join(&review.project_id)
                .join("verification")
                .join(verification_operation),
            self.root
                .join("verification-recheck-jobs")
                .join(&review.project_id)
                .join(verification_operation),
        ] {
            match fs::symlink_metadata(path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(e.to_string()),
                Ok(_) => return Err("复验操作已有文件或未决意图，不得重新执行".into()),
            }
        }
        // Pause existing scheduling before recording fresh authority. This is
        // not a resume, retry, or transition of the original failed task.
        let runs: Vec<_> = self
            .runs
            .all()
            .into_iter()
            .filter(|r| r.project_id == review.project_id && !r.state.terminal())
            .map(|r| r.id)
            .collect();
        for id in runs {
            self.pause_project_run(&id)?;
        }
        self.require_current_verification_recheck(review)?;
        let original_job = self
            .jobs
            .get(&(
                review.project_id.clone(),
                review.original_operation_id.clone(),
            ))
            .ok_or("原验收操作不存在")?
            .clone();
        self.projects
            .get_mut(&review.project_id)
            .ok_or("原项目尚未恢复")?
            .command(
                approval_operation,
                "user",
                command,
                CommandContext {
                    verification_recheck: Some(Box::new(RecheckApproval {
                        schema_version: 1,
                        operation_id: verification_operation.into(),
                        review: review.clone(),
                        original_job,
                    })),
                    approval_source: Some(crate::scheduler::RECHECK_APPROVAL_SOURCE.into()),
                    ..Default::default()
                },
                false,
            )
    }

    /// Re-observe authority, logs, source bytes and pins before an eventual
    /// approval. A deserialized review alone is not current authorization.
    pub fn require_current_verification_recheck(
        &self,
        review: &RecheckReview,
    ) -> Result<(), String> {
        let current = self.review_verification_recheck(&RecheckRequest {
            project_id: review.project_id.clone(),
            original_operation_id: review.original_operation_id.clone(),
            replacement_checks: review.replacement_checks.clone(),
            reason: review.reason.clone(),
        })?;
        require(*review == current, "复验依据已变化，请重新审阅")
    }

    /// Trusted host entry; the returned envelope is only a review. Execution
    /// requires a separate authenticated approval and a durable single-use intent.
    pub fn review_verification_recheck(
        &self,
        request: &RecheckRequest,
    ) -> Result<RecheckReview, String> {
        #[cfg(windows)]
        {
            identifier(&request.project_id)?;
            identifier(&request.original_operation_id)?;
            self.require_available(&request.project_id)?;
            require(
                !self
                    .management
                    .statuses()
                    .iter()
                    .any(|s| s.project_id == request.project_id && !s.state.resolved()),
                "项目仍有未决指挥请求，不能准备复验",
            )?;
            let job = self
                .jobs
                .get(&(
                    request.project_id.clone(),
                    request.original_operation_id.clone(),
                ))
                .ok_or("原验收操作不存在")?;
            require(
                job.state == VerificationState::Failed,
                "仅完整失败的验收可修订检查器",
            )?;
            let store = self
                .projects
                .get(&request.project_id)
                .ok_or("原项目尚未恢复")?;
            require(!store.requires_reopen(), "项目写入未确认，请先重开核对")?;
            let task = store
                .state()
                .tasks
                .get(&job.task_id)
                .ok_or("原任务不在当前方案")?;
            require(
                store.state().approved
                    && store.state().revision_requested.is_none()
                    && task.state == TaskState::Submitted
                    && !task.verified
                    && task.attempt.as_deref() == Some(job.attempt_id.as_str()),
                "复验需要当前批准方案内已提交且未通过的原尝试",
            )?;
            let plan = store.revision().ok_or("原方案缺失")?;
            let (receipt, manifest) = store
                .completed_artifacts(&job.task_id, &job.attempt_id)
                .ok_or("原提交证据缺失")?;
            let basis = crate::verification_detail::basis(store, job)?;
            let (detail, report) = crate::verification_detail::inspect(&self.root, job, &basis)?;
            require(detail.ledger_recorded, "原失败尚未记入账本，请先核对原操作")?;
            let report = report.ok_or("原验收没有完整报告")?;
            let head = &store.history().last().ok_or("原账本为空")?.hash;
            let review = request.review(&RecheckBasis {
                plan,
                manifest,
                original_job: job,
                original_report: &report,
                ledger_head: head,
                trusted_verifier: &store.configuration().core_id,
            })?;
            let approval = store.approval().ok_or("原批准缺失")?;
            let attempt = store
                .attempt_record(&job.task_id, &job.attempt_id)
                .ok_or("原尝试缺失")?;
            crate::artifacts::ArtifactObserver::new(
                plan,
                approval,
                &approval.user_id,
                plan.policy.version,
                attempt,
                receipt,
                receipt.turn_id.as_deref(),
            )?
            .require_unchanged(manifest)?;
            crate::source_snapshot::require_sealed(
                plan,
                manifest,
                &review.original_source_snapshot,
                &self
                    .root
                    .join("workflows")
                    .join(&job.project_id)
                    .join("verification"),
            )?;
            // Describe and hold the new sources together while inspecting them.
            // The eventual runner must independently lock and revalidate them.
            let root =
                crate::artifacts::root_path(&plan.execution_root(&job.task_id, &job.attempt_id)?)?;
            let mut locks = vec![];
            for check in &review.replacement_checks {
                let tool = check.tool.as_ref().ok_or("新检查器缺少工具摘要")?;
                for file in std::iter::once(&tool.executable).chain(&tool.inputs) {
                    let path = fs::canonicalize(&file.path).map_err(|e| e.to_string())?;
                    require(
                        !crate::artifacts::contains_path(&root, &path),
                        "独立检查器不能来自被检查的工作目录",
                    )?;
                }
                locks.extend(crate::verifier::PinnedCheck::from_approved(check)?.lock(check)?);
            }
            Ok(review)
        }
        #[cfg(not(windows))]
        {
            let _ = request;
            Err("检查器复验需要Windows运行时".into())
        }
    }
}
