//! Original OMP paths and commands, without giving a worker the journal owner.
use crate::{
    scheduler::{CommandContext, DurableWorkflow},
    workflow_access::WorkflowAccess,
};
use cerulean_domain::{execution::*, workflow::WorkflowCommand};
use std::path::PathBuf;

pub(crate) trait OmpWorkflowAccess: WorkflowAccess {
    fn omp_attempt_path(&self, attempt: &str) -> Result<PathBuf, String>;
    fn omp_file_service_path(&self, attempt: &str) -> Result<PathBuf, String>;
    fn requires_reopen(&self) -> bool;
    fn record_omp(
        &mut self,
        who: &str,
        command: WorkflowCommand,
        context: CommandContext,
    ) -> Result<(), String> {
        let id = format!("omp-event-{}", self.history().len() + 1);
        self.command(&id, who, command, context, false).map(|_| ())
    }
    #[cfg(windows)]
    fn prepare_omp_workspace(
        &mut self,
        task: &str,
        attempt: &str,
        cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<(String, WorkspaceProof), String>;
}
impl OmpWorkflowAccess for DurableWorkflow {
    fn omp_attempt_path(&self, attempt: &str) -> Result<PathBuf, String> {
        self.omp_attempt_path(attempt)
    }
    fn omp_file_service_path(&self, attempt: &str) -> Result<PathBuf, String> {
        self.omp_file_service_path(attempt)
    }
    fn requires_reopen(&self) -> bool {
        self.requires_reopen()
    }
    #[cfg(windows)]
    fn prepare_omp_workspace(
        &mut self,
        task: &str,
        attempt: &str,
        cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<(String, WorkspaceProof), String> {
        self.prepare_task_workspace_cancellable(task, attempt, cancel)
    }
}

/// Reuse the API accountant: CLI prompts count once; unresolved API rounds keep
/// their full original reservation. No local cancellation refunds either one.
pub(crate) fn require_budget(store: &(impl WorkflowAccess + ?Sized)) -> Result<(), String> {
    let plan = store.revision().ok_or("OMP approved plan missing")?;
    #[cfg(windows)]
    let used = crate::api_execution::prior_requests(store)?;
    #[cfg(not(windows))]
    let used = store
        .history()
        .iter()
        .filter(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. }))
        .count() as u32;
    require(
        used < plan.policy.max_requests,
        "OMP project request budget exhausted",
    )
}
