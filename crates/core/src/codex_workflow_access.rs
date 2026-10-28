//! Native Codex evidence through the original writer, without lending its lock.
use crate::{scheduler::DurableWorkflow, workflow_access::WorkflowAccess};
use cerulean_domain::execution::*;

pub(crate) trait CodexWorkflowAccess: WorkflowAccess {
    fn requires_reopen(&self) -> bool;
    fn direct(&mut self) -> Option<&mut DurableWorkflow> {
        None
    }
    fn started_cancellation_anchors(
        &self,
        task: &str,
        attempt: &str,
    ) -> Result<StartedCancellationAnchors, String>;
    #[cfg(windows)]
    fn prepare_codex_workspace(
        &mut self,
        task: &str,
        attempt: &str,
        cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<(String, WorkspaceProof), String>;
}
impl CodexWorkflowAccess for DurableWorkflow {
    fn requires_reopen(&self) -> bool {
        self.requires_reopen()
    }
    fn direct(&mut self) -> Option<&mut DurableWorkflow> {
        Some(self)
    }
    fn started_cancellation_anchors(
        &self,
        task: &str,
        attempt: &str,
    ) -> Result<StartedCancellationAnchors, String> {
        self.started_cancellation_anchors(task, attempt)
    }
    #[cfg(windows)]
    fn prepare_codex_workspace(
        &mut self,
        task: &str,
        attempt: &str,
        cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<(String, WorkspaceProof), String> {
        self.prepare_task_workspace_cancellable(task, attempt, cancel)
    }
}

pub(crate) fn require_budget(store: &(impl WorkflowAccess + ?Sized)) -> Result<(), String> {
    let plan = store.revision().ok_or("Codex approved plan missing")?;
    #[cfg(windows)]
    let used = crate::api_execution::prior_requests(store)?;
    #[cfg(not(windows))]
    let used = store
        .history()
        .iter()
        .filter(|e| {
            matches!(
                e.body.event.command,
                cerulean_domain::workflow::WorkflowCommand::DispatchIntent { .. }
            )
        })
        .count() as u32;
    require(
        used < plan.policy.max_requests,
        "Codex project request budget exhausted",
    )
}
