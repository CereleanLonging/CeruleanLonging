//! Reconcile an already completed check without launching any verifier or model.
use super::*;
use cerulean_domain::verification_detail::VerificationRecoveryReview;

struct Prepared {
    review: VerificationRecoveryReview,
    report: VerificationReport,
    _tool_locks: Vec<fs::File>,
}
impl RuntimeHost {
    #[cfg(windows)]
    fn prepare_verification_recovery(&self, job: &VerificationJob) -> Result<Prepared, String> {
        require(
            job.state == VerificationState::Unknown,
            "仅待对账验收可补记",
        )?;
        require(
            !self.active.contains_key(&job.project_id) && !self.has_codex_activity(&job.project_id),
            "项目仍有活动执行器，不能补记",
        )?;
        require(
            !self
                .management
                .statuses()
                .iter()
                .any(|s| s.project_id == job.project_id && !s.state.resolved()),
            "项目仍有未决指挥请求",
        )?;
        require(
            !self
                .codex_jobs
                .values()
                .any(|j| j.project_id == job.project_id && j.state == CodexJobState::Unknown),
            "编程执行仍待对账",
        )?;
        let store = self
            .projects
            .get(&job.project_id)
            .ok_or("项目账本尚未恢复")?;
        let task = store
            .state()
            .tasks
            .get(&job.task_id)
            .ok_or("原任务不在当前方案")?;
        require(
            store.state().revision_requested.is_none()
                && task.state == TaskState::Submitted
                && !task.verified
                && task.attempt.as_deref() == Some(&job.attempt_id),
            "仅当前已提交但未确认的原尝试可补记",
        )?;
        require(
            !store
                .history()
                .iter()
                .any(|e| e.body.operation_id == job.operation_id),
            "验收已入账或操作ID冲突",
        )?;
        require(
            !crate::verification_reconciliation::applies(&self.root, job)?,
            "本次验收已被放弃",
        )?;
        let basis = crate::verification_detail::basis(store, job)?;
        let (detail, report) = crate::verification_detail::inspect(&self.root, job, &basis)?;
        let report = report.ok_or("没有完整的原验收报告，不能从部分阶段补造结果")?;
        let plan = store.revision().ok_or("原方案缺失")?;
        require(
            plan.approval_digest()? == detail.revision_digest,
            "原验收不属于当前方案",
        )?;
        let checks = &plan.task(&job.task_id)?.checks;
        require(
            !report.checks.is_empty()
                && (report.checks.len() == checks.len()
                    || report.checks.last().is_some_and(|c| c.timed_out)),
            "验收阶段不完整",
        )?;
        require(
            report.checks.iter().enumerate().all(|(i, c)| {
                c.exit_code.is_some() && (!c.timed_out || i + 1 == report.checks.len())
            }),
            "验收缺退出观察或超时顺序冲突",
        )?;
        let started = chrono::DateTime::parse_from_rfc3339(&job.started_at)
            .map_err(|_| "验收开始时间无效")?;
        let observed = chrono::DateTime::parse_from_rfc3339(&report.observed_at)
            .map_err(|_| "验收观察时间无效")?;
        require(
            observed >= started && observed <= chrono::Utc::now(),
            "验收报告时间不在原操作范围",
        )?;
        let (_, manifest) = store
            .completed_artifacts(&job.task_id, &job.attempt_id)
            .ok_or("原提交缺失")?;
        let mut locks = vec![];
        for check in checks {
            locks.extend(crate::verifier::PinnedCheck::from_approved(check)?.lock(check)?);
        }
        crate::source_snapshot::require_sealed(
            plan,
            manifest,
            report.source_snapshot.as_ref().ok_or("受测副本缺失")?,
            &self
                .root
                .join("workflows")
                .join(&job.project_id)
                .join("verification"),
        )?;
        let passed = report.require_pass(plan, manifest, "core").is_ok();
        require(
            passed
                || report
                    .checks
                    .iter()
                    .any(|c| c.timed_out || c.exit_code.is_some_and(|code| code != 0)),
            "报告未证明完整通过或真实失败",
        )?;
        let evidence_digest = digest(&(
            "verification-recovery-v1",
            detail.evidence_digest,
            store.history().last().map(|e| &e.hash),
            digest(&report)?,
        ))?;
        Ok(Prepared {
            review: VerificationRecoveryReview {
                evidence_digest,
                passed,
            },
            report,
            _tool_locks: locks,
        })
    }
    pub(super) fn verification_recovery_review(
        &self,
        job: &VerificationJob,
    ) -> Result<VerificationRecoveryReview, String> {
        #[cfg(windows)]
        {
            Ok(self.prepare_verification_recovery(job)?.review)
        }
        #[cfg(not(windows))]
        {
            let _ = job;
            Err("验收补记需要Windows运行时".into())
        }
    }
    pub(super) fn recover_verification(
        &mut self,
        project: &str,
        operation: &str,
        wanted: &str,
    ) -> Result<(), String> {
        #[cfg(windows)]
        {
            identifier(project)?;
            identifier(operation)?;
            sha256(wanted)?;
            let key = (project.to_string(), operation.to_string());
            let mut job = self.jobs.get(&key).ok_or("验收操作不存在")?.clone();
            let audit = format!("authenticated local-user verification recovery:{wanted}");
            // A Verify commit is authoritative even when its job projection write failed.
            let prior = self.projects.get(project).and_then(|s| {
                s.history()
                    .iter()
                    .find(|e| e.body.operation_id == operation)
            });
            let passed = if let Some(entry) = prior {
                require(
                    entry.body.context.approval_source.as_deref() == Some(audit.as_str()),
                    "该操作不是同一次补记请求",
                )?;
                match &entry.body.event.command {
                    WorkflowCommand::Verify {
                        task,
                        attempt,
                        passed,
                        ..
                    } if task == &job.task_id && attempt == &job.attempt_id => *passed,
                    _ => return Err("补记与已有操作冲突".into()),
                }
            } else {
                let prepared = self.prepare_verification_recovery(&job)?;
                require(
                    prepared.review.evidence_digest == wanted,
                    "证据或任务状态已变化，请重新读取并审阅",
                )?;
                // Reconciliation cannot immediately resume chief/worker calls.
                for run in self
                    .runs
                    .all()
                    .into_iter()
                    .filter(|r| r.project_id == project && !r.state.terminal())
                {
                    self.pause_project_run(&run.id)?;
                }
                let passed = prepared.review.passed;
                self.projects
                    .get_mut(project)
                    .ok_or("项目账本缺失")?
                    .command(
                        operation,
                        "core",
                        WorkflowCommand::Verify {
                            task: job.task_id.clone(),
                            attempt: job.attempt_id.clone(),
                            passed,
                            evidence: vec![digest(&prepared.report)?],
                        },
                        CommandContext {
                            approval_source: Some(audit),
                            execution: Some(ExecutionEvidence::Verification {
                                report: prepared.report,
                            }),
                            ..Default::default()
                        },
                        true,
                    )?;
                passed
            };
            job.state = if passed {
                VerificationState::Succeeded
            } else {
                VerificationState::Failed
            };
            job.updated_at = chrono::Utc::now().to_rfc3339();
            job.reconciliation_digest = None;
            job.message = None;
            self.jobs.insert(key, job.clone());
            crate::async_verification::save(&self.root, &job)
        }
        #[cfg(not(windows))]
        {
            let _ = (project, operation, wanted);
            Err("验收补记需要Windows运行时".into())
        }
    }
}
