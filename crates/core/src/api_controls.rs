//! User review of API startups and unknown local attempts; never a remote cancellation receipt.
use super::*;
use crate::{
    api_startup::ApiStartup,
    management_execution::{DecisionState, UserResolution},
};
use cerulean_domain::api_jobs::{ApiJob, ApiJobState};
impl RuntimeHost {
    pub(crate) fn can_restart_api_fault(&self) -> bool {
        self.api_fault.is_some()
            && !self.baselines.busy()
            && self.desktop_probe.is_none()
            && self.desktop_active.is_none()
            && self.api_active.is_empty()
            && self.omp_active.is_empty()
            && self.codex_active.is_empty()
            && self.active.is_empty()
            && self.management_work_count() == 0
            && self.codex_probe.is_none()
            && self.openai_probe.is_none()
            && self.provider_probes.is_empty()
            && self.runs.healthy()
            && !self
                .runs
                .all()
                .iter()
                .any(|r| r.state == project_setup::RunState::Active)
    }
    pub(super) fn validate_api_startup_closure(&self, startup: &ApiStartup) -> Result<(), String> {
        if let Some(closed) = self
            .management
            .startup_closure(&startup.management_operation)
        {
            require(
                closed.attempt_id == startup.attempt.id
                    && closed.startup_hash == digest(startup)?
                    && self
                        .management
                        .application(&startup.management_operation)
                        .is_none(),
                "API启动关闭与原意图不符",
            )?;
            let store = self
                .projects
                .get(&startup.project_id)
                .ok_or("API关闭项目账本缺失")?;
            require(
                !store.history().iter().any(|e| {
                    e.body.event.command.attempt() == Some(startup.attempt.id.as_str())
                        || e.body.operation_id == format!("mgmt-{}", startup.management_operation)
                }),
                "已关闭API启动出现工作流分配",
            )?;
        }
        Ok(())
    }
    pub(super) fn api_summaries(&self) -> Vec<ApiJob> {
        self.api_jobs
            .values()
            .map(|job| self.api_job_view(job))
            .collect()
    }
    pub(super) fn api_job_view(&self, original: &ApiJob) -> ApiJob {
        let mut job = original.clone();
        job.closure_digest = self
            .api_closure_digest(&job.project_id, &job.attempt_id)
            .ok();
        job.retirement_digest = self
            .api_retirement_digest(&job.project_id, &job.attempt_id)
            .ok();
        job
    }
    fn api_retirement_digest(&self, project: &str, attempt: &str) -> Result<String, String> {
        require(
            self.api_fault.is_none()
                && self.management.healthy()
                && self.runs.healthy()
                && self.runner_error.is_none(),
            "API或管理账本需要重开",
        )?;
        require(
            !self.has_api_activity(project)
                && !self.has_omp_activity(project)
                && !self.has_codex_activity(project)
                && !self.active.contains_key(project),
            "项目后台尚未结束",
        )?;
        require(
            !self.management_active.keys().any(|op| {
                self.management
                    .intent(op)
                    .is_ok_and(|i| i.project_id == project)
            }),
            "项目指挥仍在运行",
        )?;
        let statuses = self
            .management
            .statuses()
            .into_iter()
            .filter(|s| s.project_id == project)
            .collect::<Vec<_>>();
        require(
            statuses.iter().all(|s| s.state.resolved()),
            "请先核对未决指挥应用",
        )?;
        let runs = self
            .runs
            .all()
            .into_iter()
            .filter(|r| r.project_id == project)
            .collect::<Vec<_>>();
        require(
            !runs
                .iter()
                .any(|r| r.state == project_setup::RunState::Active),
            "请先暂停项目调度",
        )?;
        let startup = self
            .api_startups
            .get(&(project.into(), attempt.into()))
            .ok_or("API原启动缺失")?;
        let store = self.projects.get(project).ok_or("API项目不可用")?;
        require(!store.requires_reopen(), "项目账本需要重开")?;
        let task = &startup.attempt.task_id;
        let run = store.state().tasks.get(task).ok_or("原任务缺失")?;
        require(
            run.state == TaskState::Unknown
                && run.attempt.as_deref() == Some(attempt)
                && store.attempt_record(task, attempt) == Some(&startup.attempt),
            "当前不是该API未知尝试",
        )?;
        require(store.history().iter().any(|e| matches!(&e.body.event.command, WorkflowCommand::DispatchIntent{task:t,attempt:a} if t==task && a==attempt)), "原派发记录缺失")?;
        require(
            store.state().tasks.iter().all(|(id, t)| {
                id == task
                    || matches!(
                        t.state,
                        TaskState::Pending
                            | TaskState::Accepted
                            | TaskState::Cancelled
                            | TaskState::Retired
                    )
            }),
            "项目存在其他未完成执行",
        )?;
        require(
            !self
                .jobs
                .values()
                .any(|j| j.project_id == project && j.state == VerificationState::Unknown)
                && !self
                    .codex_jobs
                    .values()
                    .any(|j| j.project_id == project && j.state == CodexJobState::Unknown)
                && !self.omp_jobs.values().any(|j| {
                    j.project_id == project
                        && matches!(
                            j.state,
                            omp_jobs::OmpJobState::Unknown | omp_jobs::OmpJobState::Failed
                        )
                })
                && !self.api_jobs.values().any(|j| {
                    j.project_id == project
                        && j.attempt_id != attempt
                        && j.state == ApiJobState::Unknown
                }),
            "项目存在其他待核对执行",
        )?;
        let identity = crate::api_execution::retirement_identity(store, task, attempt)?;
        let reservation = crate::api_execution::prior_requests(store)?;
        let currency_evidence = if self.currency_accounts.contains_key(project)
            || store
                .revision()
                .is_some_and(|p| p.policy.currency_budget.is_some())
        {
            Some(
                crate::api_execution::CallInventory::read_with_account(
                    store,
                    self.currency_accounts.get(project),
                )?
                .retirement_evidence(self.currency_accounts.get(project), attempt)?,
            )
        } else {
            None
        };
        let heads = statuses
            .iter()
            .map(|s| {
                self.management
                    .evidence_digest(&s.operation_id)
                    .map(|h| (&s.operation_id, h))
            })
            .collect::<Result<Vec<_>, _>>()?;
        digest(
            &serde_json::json!({"purpose":"retire-unknown-api-v1","startup":startup,"identity":identity,"reservedRequests":reservation,"workflowHead":store.history().last().map(|e|&e.hash),"management":heads,"runs":runs,"currencyEvidence":currency_evidence}),
        )
    }
    pub(super) fn retire_unknown_api(
        &mut self,
        project: &str,
        attempt: &str,
        expected: &str,
        reason: &str,
    ) -> Result<(), String> {
        sha256(expected)?;
        require(
            !reason.trim().is_empty() && reason.len() <= 8000 && !reason.contains('\0'),
            "请输入有效处置原因",
        )?;
        require(
            self.api_retirement_digest(project, attempt)? == expected,
            "API处置证据已变化，请重新审阅",
        )?;
        let task = self.api_startups[&(project.into(), attempt.into())]
            .attempt
            .task_id
            .clone();
        let store = self.projects.get_mut(project).ok_or("项目账本缺失")?;
        store.command(
            &format!("user-api-retired-{}", store.history().len() + 1),
            "user",
            WorkflowCommand::RetireUnknownApi {
                task,
                attempt: attempt.into(),
                reason: reason.into(),
            },
            CommandContext::default(),
            false,
        )?;
        let job = self
            .api_jobs
            .get_mut(&(project.into(), attempt.into()))
            .ok_or("API投影缺失")?;
        job.state = ApiJobState::Retired;
        job.retirement_digest = None;
        job.message = Some(
            "本地尝试已结束；原请求结果和费用仍未确认，已有修改保留。须修订并重新批准方案。".into(),
        );
        Ok(())
    }
    fn api_closure_digest(&self, project: &str, attempt: &str) -> Result<String, String> {
        require(
            self.api_fault.is_none()
                && self.management.healthy()
                && self.runs.healthy()
                && self.runner_error.is_none(),
            "API或管理账本需要重开",
        )?;
        require(
            !self.has_api_activity(project)
                && !self.has_omp_activity(project)
                && !self.has_codex_activity(project)
                && !self.active.contains_key(project),
            "项目后台尚未结束",
        )?;
        let startup = self
            .api_startups
            .get(&(project.into(), attempt.into()))
            .ok_or("API启动记录缺失")?;
        let operation = &startup.management_operation;
        require(
            self.management.application(operation).is_none()
                && self.management.startup_closure(operation).is_none(),
            "API启动已有应用准备或已关闭",
        )?;
        let statuses = self.management.statuses();
        require(
            statuses
                .iter()
                .any(|s| s.operation_id == *operation && s.state == DecisionState::Ready),
            "API分配响应不再待处理",
        )?;
        require(
            !statuses.iter().any(|s| {
                s.project_id == project && s.operation_id != *operation && !s.state.resolved()
            }),
            "项目存在其他未决指挥",
        )?;
        require(
            !self.management_active.keys().any(|op| {
                self.management
                    .intent(op)
                    .is_ok_and(|i| i.project_id == project)
            }),
            "项目指挥仍在运行",
        )?;
        let runs = self
            .runs
            .all()
            .into_iter()
            .filter(|r| r.project_id == project)
            .collect::<Vec<_>>();
        require(
            !runs
                .iter()
                .any(|r| r.state == project_setup::RunState::Active),
            "请先暂停项目调度",
        )?;
        let store = self.projects.get(project).ok_or("API项目账本不可用")?;
        require(!store.requires_reopen(), "API项目需要重开")?;
        require(
            !store.history().iter().any(|e| {
                e.body.event.command.attempt() == Some(attempt)
                    || e.body.operation_id == format!("mgmt-{operation}")
            }),
            "已分配尝试不能按未分配启动关闭",
        )?;
        require(
            store.state().tasks.values().all(|t| {
                matches!(
                    t.state,
                    TaskState::Pending
                        | TaskState::Accepted
                        | TaskState::Cancelled
                        | TaskState::Retired
                )
            }),
            "项目存在未完成执行",
        )?;
        require(
            !self.api_jobs.values().any(|j| {
                j.project_id == project
                    && j.attempt_id != attempt
                    && j.state == ApiJobState::Unknown
            }),
            "项目存在另一待核对API尝试",
        )?;
        let heads = statuses
            .iter()
            .filter(|s| s.project_id == project)
            .map(|s| {
                Ok((
                    &s.operation_id,
                    self.management.evidence_digest(&s.operation_id)?,
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        digest(
            &serde_json::json!({"purpose":"close-unassigned-api-startup-v1","startup":startup,"workflowHead":store.history().last().map(|e|&e.hash),"management":heads,"runs":runs}),
        )
    }
    pub(super) fn abandon_api_startup(
        &mut self,
        project: &str,
        attempt: &str,
        expected: &str,
        reason: &str,
    ) -> Result<(), String> {
        sha256(expected)?;
        require(
            !reason.trim().is_empty() && reason.len() <= 8000 && !reason.contains('\0'),
            "请输入有效关闭原因",
        )?;
        require(
            self.api_closure_digest(project, attempt)? == expected,
            "API启动证据已变化，请重新审阅",
        )?;
        let startup = &self.api_startups[&(project.into(), attempt.into())];
        let head = self
            .management
            .evidence_digest(&startup.management_operation)?
            .to_owned();
        self.management.resolve(
            &startup.management_operation,
            &head,
            UserResolution::AbandonStartup {
                reason: reason.into(),
                attempt_id: attempt.into(),
                startup_hash: digest(startup)?,
            },
        )?;
        let job = self
            .api_jobs
            .get_mut(&(project.into(), attempt.into()))
            .ok_or("API任务投影缺失")?;
        job.state = ApiJobState::Abandoned;
        job.closure_digest = None;
        job.message = Some("已关闭未分配启动，原记录保留；没有发送模型请求".into());
        Ok(())
    }
}
