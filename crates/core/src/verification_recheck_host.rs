//! Host scheduling for the exact already-approved corrected-check operation.
use super::*;

impl RuntimeHost {
    pub fn review_verification_recheck_disposition(
        &self,
        project: &str,
        operation: &str,
        reason: &str,
    ) -> Result<cerulean_domain::verification_recheck::RecheckDispositionReview, String> {
        identifier(project)?;
        identifier(operation)?;
        self.require_available_for_recheck(project, false, Some(operation))?;
        require(
            !self
                .management
                .statuses()
                .iter()
                .any(|s| s.project_id == project && !s.state.resolved()),
            "项目仍有未决指挥请求",
        )?;
        require(
            self.runs
                .all()
                .iter()
                .all(|r| r.project_id != project || r.state != project_setup::RunState::Active),
            "请先暂停原项目调度",
        )?;
        self.projects
            .get(project)
            .ok_or("原项目尚未恢复")?
            .review_verification_recheck_disposition(operation, reason)
    }
    pub fn retire_verification_recheck(
        &mut self,
        review: &cerulean_domain::verification_recheck::RecheckDispositionReview,
        event_operation: &str,
    ) -> Result<crate::scheduler::Entry, String> {
        identifier(event_operation)?;
        let command = WorkflowCommand::RetireVerificationRecheck {
            task: review.task_id.clone(),
            attempt: review.attempt_id.clone(),
            operation_id: review.operation_id.clone(),
            review_digest: digest(review)?,
        };
        let context = crate::scheduler::CommandContext {
            recheck_disposition: Some(Box::new(review.clone())),
            approval_source: Some(
                "authenticated local-user verification recheck disposition".into(),
            ),
            ..Default::default()
        };
        let store = self
            .projects
            .get(&review.project_id)
            .ok_or("原项目尚未恢复")?;
        require(
            !self.base.poisoned && !store.requires_reopen(),
            "原账本需要重开核对",
        )?;
        let saved = store
            .history()
            .iter()
            .find(|e| e.body.operation_id == event_operation)
            .cloned();
        let entry = if let Some(saved) = saved {
            require(
                saved.body.event.principal == "user"
                    && saved.body.event.command == command
                    && saved.body.context == context,
                "原处置编号已用于其他内容",
            )?;
            // Verify original files before repairing a lagging projection.
            require(
                store
                    .retired_recheck_operations()?
                    .contains(&review.operation_id),
                "原处置证据已变化",
            )?;
            saved
        } else {
            require(
                self.review_verification_recheck_disposition(
                    &review.project_id,
                    &review.operation_id,
                    &review.reason,
                )? == *review,
                "原处置审阅已变化，请重新核对",
            )?;
            self.projects
                .get_mut(&review.project_id)
                .ok_or("原项目尚未恢复")?
                .command(event_operation, "user", command, context, true)?
        };
        let key = (review.project_id.clone(), review.operation_id.clone());
        if let Some(mut job) = self.jobs.get(&key).cloned() {
            job.state = VerificationState::Abandoned;
            job.updated_at = entry.body.received_at.clone();
            job.message = Some(review.reason.clone());
            job.reconciliation_digest = None;
            self.save_verification_job(&job)?;
            self.jobs.insert(key, job);
        }
        Ok(entry)
    }
    pub(super) fn is_verification_recheck(&self, project: &str, operation: &str) -> bool {
        self.recheck_jobs
            .contains(&(project.into(), operation.into()))
    }
    pub(super) fn save_verification_job(&self, job: &VerificationJob) -> Result<(), String> {
        if self.is_verification_recheck(&job.project_id, &job.operation_id) {
            crate::async_verification_recheck::save(&self.root, job)
        } else {
            crate::async_verification::save(&self.root, job)
        }
    }
    /// No new pins or source are accepted here: the original user approval owns them.
    pub fn start_verification_recheck(
        &mut self,
        project: &str,
        operation: &str,
    ) -> Result<(), String> {
        identifier(project)?;
        identifier(operation)?;
        // This explicit local-check entry must not tick unrelated project runs.
        self.require_available_for_recheck(project, false, Some(operation))?;
        require(!self.base.poisoned, "核心写入状态需要重开核对")?;
        require(
            !self
                .management
                .statuses()
                .iter()
                .any(|s| s.project_id == project && !s.state.resolved()),
            "项目仍有未决指挥请求",
        )?;
        require(
            self.runs
                .all()
                .iter()
                .all(|r| r.project_id != project || r.state != project_setup::RunState::Active),
            "复验前必须暂停原项目运行",
        )?;
        require(
            self.active.len()
                + self.codex_active.len()
                + self.omp_active.len()
                + self.api_active.len()
                + usize::from(self.desktop_active.is_some())
                < 4,
            "最多同时运行4个后台项目操作",
        )?;
        require(
            self.jobs
                .values()
                .filter(|j| j.project_id == project)
                .count()
                < 1000,
            "验收记录达到当前版本上限",
        )?;
        let key = (project.to_string(), operation.to_string());
        require(
            !self.jobs.contains_key(&key),
            "复验启动已有记录；仅核对原结果，不得重跑",
        )?;
        let store = self.projects.get(project).ok_or("原项目尚未恢复")?;
        let _prepared = store.prepare_verification_recheck(operation)?;
        let approved = store
            .unresolved_verification_recheck()
            .ok_or("原复验批准缺失")?;
        let approval = approved
            .body
            .context
            .verification_recheck
            .as_ref()
            .ok_or("原复验批准缺失")?;
        let now = chrono::Utc::now().to_rfc3339();
        let mut job = VerificationJob {
            project_id: project.into(),
            operation_id: operation.into(),
            task_id: approval.review.task_id.clone(),
            attempt_id: approval.review.attempt_id.clone(),
            state: VerificationState::Running,
            started_at: now.clone(),
            updated_at: now,
            message: None,
            reconciliation_digest: None,
        };
        if let Err(error) = crate::async_verification_recheck::claim(&self.root, store, &job) {
            if crate::async_verification_recheck::directory(&self.root, project, operation).exists()
            {
                job.state = VerificationState::Unknown;
                job.message = Some(error.clone());
                self.recheck_jobs.insert(key.clone());
                self.jobs.insert(key, job);
            }
            return Err(error);
        }
        self.recheck_jobs.insert(key.clone());
        self.jobs.insert(key.clone(), job.clone());
        let store = self
            .projects
            .remove(project)
            .expect("checked original project");
        let config = store.configuration().clone();
        match crate::async_verification_recheck::launch(&self.root, store, job.clone()) {
            Ok(active) => {
                self.active.insert(project.into(), active);
                Ok(())
            }
            Err(error) => {
                job.state = VerificationState::Unknown;
                job.message = Some(error.clone());
                let _ = self.save_verification_job(&job);
                self.jobs.insert(key, job);
                let store =
                    DurableWorkflow::open(&self.root.join("workflows").join(project), config)?;
                self.projects.insert(project.into(), store);
                Err(error)
            }
        }
    }
    /// Reconcile only an existing Host launch; no verifier/model process starts.
    pub fn recover_verification_recheck(
        &mut self,
        project: &str,
        operation: &str,
    ) -> Result<(), String> {
        identifier(project)?;
        identifier(operation)?;
        // Recovery never schedules a checker, worker, or management request.
        let key = (project.to_string(), operation.to_string());
        require(self.recheck_jobs.contains(&key), "原复验启动记录不存在")?;
        require(!self.active.contains_key(project), "复验线程尚未结束")?;
        let mut job = self.jobs.get(&key).ok_or("原复验状态不存在")?.clone();
        // Validate immutable launch authority before recovery can publish any
        // ledger event, even when the derived status file is damaged.
        crate::async_verification_recheck::require_claim(
            &self.root,
            self.projects.get(project).ok_or("原项目尚未恢复")?,
            &job,
        )?;
        // New launches carry OS identities. Failure to query them cannot be
        // replaced by a mutable Unknown status or a complete-looking log.
        let _process_proof = crate::async_verification_recheck::process_exit_proof(
            &self.root,
            self.projects.get(project).ok_or("原项目尚未恢复")?,
            &job,
        )?;
        let recorded = self
            .projects
            .get(project)
            .is_some_and(|s| s.history().iter().any(|e| e.body.operation_id == operation));
        if !recorded {
            require(
                job.state == VerificationState::Unknown,
                "仅待核对复验可补记",
            )?;
            self.require_available_for_recheck(project, false, Some(operation))?;
            require(
                !self
                    .management
                    .statuses()
                    .iter()
                    .any(|s| s.project_id == project && !s.state.resolved()),
                "项目仍有未决指挥请求",
            )?;
        }
        let entry = self
            .projects
            .get_mut(project)
            .ok_or("原项目尚未恢复")?
            .recover_verification_recheck(operation)?;
        let passed = match &entry.body.event.command {
            WorkflowCommand::Verify {
                task,
                attempt,
                passed,
                ..
            } if task == &job.task_id && attempt == &job.attempt_id => *passed,
            _ => return Err("复验结果身份不匹配".into()),
        };
        job.state = if passed {
            VerificationState::Succeeded
        } else {
            VerificationState::Failed
        };
        job.updated_at = chrono::Utc::now().to_rfc3339();
        job.message = None;
        job.reconciliation_digest = None;
        self.save_verification_job(&job)?;
        self.jobs.insert(key, job);
        Ok(())
    }
}
