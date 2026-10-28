//! User disposition of local tracking, never evidence of remote cancellation.
use super::*;
use crate::desktop_execution as execution;

impl RuntimeHost {
    pub(super) fn desktop_retirement_digest(
        &self,
        project: &str,
        attempt: &str,
    ) -> Result<String, String> {
        self.require_available(project)?;
        require(
            self.desktop_active.is_none() && self.desktop_probe.is_none(),
            "请等待当前桌面操作结束",
        )?;
        require(
            self.management.healthy() && self.runs.healthy() && self.runner_error.is_none(),
            "指挥或调度账本需要重开",
        )?;
        require(
            !self.management_active.keys().any(|op| {
                self.management
                    .intent(op)
                    .is_ok_and(|i| i.project_id == project)
            }),
            "项目指挥请求仍在运行",
        )?;
        let management: Vec<_> = self
            .management
            .statuses()
            .into_iter()
            .filter(|s| s.project_id == project)
            .collect();
        require(
            management.iter().all(|s| s.state.resolved()),
            "请先处置未完成的指挥应用",
        )?;
        let runs: Vec<_> = self
            .runs
            .all()
            .into_iter()
            .filter(|r| r.project_id == project)
            .collect();
        require(
            runs.iter()
                .all(|r| r.state != project_setup::RunState::Active),
            "请先暂停项目调度",
        )?;
        let store = self.projects.get(project).ok_or("原项目账本不可用")?;
        require(!store.requires_reopen(), "项目账本需要重开")?;
        let (task, identity) = execution::retirement_identity(&self.root, store, project, attempt)?;
        require(
            store.state().tasks.iter().all(|(id, t)| {
                id == &task
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
        let heads = management
            .iter()
            .map(|s| {
                self.management
                    .evidence_digest(&s.operation_id)
                    .map(|h| (&s.operation_id, h))
            })
            .collect::<Result<Vec<_>, _>>()?;
        digest(
            &serde_json::json!({"purpose":"retire-unknown-desktop-v1", "project":project, "attempt":attempt, "identity":identity, "workflowHead":store.history().last().map(|e|&e.hash), "management":heads, "runs":runs}),
        )
    }
    pub(super) fn retire_unknown_desktop(
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
            self.desktop_retirement_digest(project, attempt)? == expected,
            "桌面处置证据已变化，请重新审阅",
        )?;
        let (task, _) =
            execution::retirement_identity(&self.root, &self.projects[project], project, attempt)?;
        let store = self.projects.get_mut(project).ok_or("原项目账本不可用")?;
        store.command(
            &format!("user-desktop-retired-{}", store.history().len() + 1),
            "user",
            WorkflowCommand::RetireUnknownDesktop {
                task,
                attempt: attempt.into(),
                evidence_digest: expected.into(),
                reason: reason.into(),
            },
            CommandContext::default(),
            false,
        )?;
        self.desktop_access.task_review = None;
        self.desktop_access.error = None;
        self.refresh_desktop_tasks()
    }
}
