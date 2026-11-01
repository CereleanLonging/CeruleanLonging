//! Narrow proof that unresolved work has no live local owner.
use super::*;
use crate::management_execution::DecisionState;

#[cfg(test)]
#[path = "preserve_shutdown_tests.rs"]
pub(crate) mod tests;

impl RuntimeHost {
    pub(crate) fn preserved_shutdown_evidence(&self) -> Result<(Snapshot, String), String> {
        require(
            !self.base.poisoned
                && self.runs.healthy()
                && self.management.healthy()
                && self.startups.healthy()
                && self.omp_startups.healthy()
                && self.advice_closures.healthy()
                && self.runner_error.is_none()
                && self.currency_accounts.values().all(|a| a.healthy())
                && self.projects.values().all(|p| !p.requires_reopen()),
            "账本尚未可靠保存，请保留原记录进行诊断",
        )?;
        require(
            !self.orchestrating
                && self.management_work_count() == 0
                && self.codex_probe.is_none()
                && self.openai_probe.is_none()
                && self.provider_probes.is_empty()
                && self
                    .runs
                    .all()
                    .iter()
                    .all(|r| r.state != project_setup::RunState::Active),
            "调度或后台请求仍在运行，请先暂停并等待原操作结束",
        )?;
        #[cfg(windows)]
        require(
            !self.baselines.busy()
                && self.api_fault.is_none()
                && self.desktop_probe.is_none()
                && self.desktop_active.is_none()
                && self.api_active.is_empty()
                && self.codex_active.is_empty()
                && self.omp_active.is_empty()
                && self.active.is_empty()
                && self.api_writers.is_empty(),
            "本地执行、清理或账本所有者尚未结束，不能关闭",
        )?;
        let snapshot = self.snapshot();
        require(
            snapshot
                .team_import
                .as_ref()
                .is_none_or(|v| v.state == "completed")
                && snapshot.api_recovery.is_none()
                && snapshot.core_diagnostic.is_none()
                && snapshot.management_record_reviews.is_empty()
                && snapshot.startup_closure_reviews.is_empty()
                && snapshot.startup_reservations.is_empty()
                && snapshot.api_publication_reviews.is_empty()
                && snapshot.omp_stop_issues.is_empty(),
            "存在未完成发布、启动或恢复审阅，请先处理原记录",
        )?;
        require(
            snapshot.desktop_access.as_ref().is_none_or(|d| {
                !d.checking
                    && d.trials
                        .iter()
                        .all(|t| t.proof.as_ref().is_some_and(|p| p.completed))
                    && d.tasks.iter().all(|t| {
                        !t.active
                            && !t.reading
                            && !t.stopping
                            && (t.submitted
                                || t.retired
                                || t.cancellation.as_ref().is_some_and(|c| c.confirmed))
                    })
            }),
            "桌面操作仍需原停止或核对流程",
        )?;
        let mut recheck_cancellations = Vec::new();
        for job in self
            .jobs
            .values()
            .filter(|j| j.state == VerificationState::Unknown)
        {
            require(
                self.is_verification_recheck(&job.project_id, &job.operation_id),
                "其他验收仍需对应恢复入口",
            )?;
            let store = self
                .projects
                .get(&job.project_id)
                .ok_or("原复验账本尚不可用")?;
            crate::async_verification_recheck::require_claim(&self.root, store, job)?;
            let basis = crate::verification_detail::basis(store, job)?;
            let mut proof = crate::verification_detail::preserved_recheck_cancellation(
                &self.root, job, &basis,
            )?;
            if let Some(processes) =
                crate::async_verification_recheck::process_exit_proof(&self.root, store, job)?
            {
                proof = digest(&("preserved-recheck-processes-v1", proof, processes))?;
            }
            recheck_cancellations.push((&job.project_id, &job.operation_id, proof));
        }
        require(
            snapshot.api_jobs.iter().all(|j| {
                matches!(
                    j.state,
                    api_jobs::ApiJobState::Submitted
                        | api_jobs::ApiJobState::Abandoned
                        | api_jobs::ApiJobState::Retired
                )
            }) && self.omp_jobs.values().all(|j| {
                matches!(
                    j.state,
                    omp_jobs::OmpJobState::Submitted
                        | omp_jobs::OmpJobState::Cancelled
                        | omp_jobs::OmpJobState::Abandoned
                )
            }) && self.jobs.values().all(|j| {
                !matches!(
                    j.state,
                    VerificationState::Running | VerificationState::Cancelling
                )
            }),
            "其他执行仍未结束，请使用对应恢复入口",
        )?;
        let decisions = self.management.statuses();
        require(
            decisions
                .iter()
                .all(|d| d.state.resolved() || d.state == DecisionState::AwaitingExternal),
            "存在未决指挥应用，不能关闭",
        )?;
        let mut observations = Vec::new();
        for (project, store) in &self.projects {
            for (task_id, task) in &store.state().tasks {
                require(
                    matches!(
                        task.state,
                        TaskState::Pending
                            | TaskState::Accepted
                            | TaskState::Cancelled
                            | TaskState::Retired
                            | TaskState::Submitted
                            | TaskState::Unknown
                    ),
                    "任务仍持有执行状态，不能关闭",
                )?;
                if task.state != TaskState::Unknown {
                    continue;
                }
                let attempt = task.attempt.as_deref().ok_or("未知任务缺少原尝试")?;
                let job = self
                    .codex_jobs
                    .get(&(project.clone(), attempt.into()))
                    .ok_or("仅支持已保存的Codex未知执行，其余任务须先核对")?;
                require(
                    job.task_id == *task_id && job.state == CodexJobState::Unknown,
                    "Codex任务和原尝试投影不符",
                )?;
                let bound = store
                    .attempt_record(task_id, attempt)
                    .ok_or("原Codex尝试缺失")?;
                require(
                    bound.binding.adapter_id == "codex"
                        && bound.revision_digest == job.revision_digest,
                    "原Codex关联不符",
                )?;
                let observation = crate::worker_observation::read(&self.root, job)?;
                require(observation.trace.finished && observation.storage_error.is_none()
                    && observation.trace.thread_id.as_ref() == Some(&bound.binding.session_id)
                    && observation.trace.turn_id.as_ref().is_some_and(|turn| store.history().iter().any(|e| {
                        matches!(&e.body.event.command, WorkflowCommand::Started { task, attempt: a }
                            if task == task_id && a == attempt)
                            && e.body.context.turn_id.as_ref() == Some(turn)
                    })), "原执行观察尚未完成或轮次关联缺失")?;
                observations.push(observation);
            }
        }
        require(
            self.codex_jobs.values().all(|j| {
                matches!(
                    j.state,
                    CodexJobState::Unknown
                        | CodexJobState::Submitted
                        | CodexJobState::Cancelled
                        | CodexJobState::Failed
                        | CodexJobState::Abandoned
                )
            }),
            "Codex执行尚未结束",
        )?;
        require(
            self.codex_jobs
                .values()
                .filter(|j| j.state == CodexJobState::Unknown)
                .all(|j| {
                    self.projects
                        .get(&j.project_id)
                        .and_then(|s| s.state().tasks.get(&j.task_id))
                        .is_some_and(|t| {
                            t.state == TaskState::Unknown
                                && t.attempt.as_ref() == Some(&j.attempt_id)
                        })
                }),
            "未知Codex投影缺少当前原任务关联",
        )?;
        let management_heads = decisions
            .iter()
            .map(|d| {
                self.management
                    .evidence_digest(&d.operation_id)
                    .map(|h| (&d.operation_id, h))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let project_heads = self
            .projects
            .iter()
            .map(|(id, store)| (id, store.history().last().map(|e| &e.hash)))
            .collect::<Vec<_>>();
        let original_evidence = digest(&(
            "preserved-shutdown-v1",
            &self.base.last_hash,
            project_heads,
            management_heads,
            self.startups.all(),
            self.omp_startups.all(),
            observations,
        ))?;
        // Preserve the existing evidence identity when no recheck is involved.
        let evidence = if recheck_cancellations.is_empty() {
            original_evidence
        } else {
            digest(&(
                "preserved-shutdown-recheck-v1",
                original_evidence,
                recheck_cancellations,
            ))?
        };
        Ok((snapshot, evidence))
    }
}
