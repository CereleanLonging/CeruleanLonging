//! Narrow journal access shared by direct recovery and coordinated API workers.
use crate::scheduler::{CommandContext, DurableWorkflow, Entry, SchedulerConfig};
use cerulean_domain::{
    execution::*,
    workflow::{WorkflowCommand, WorkflowState},
};
use std::path::PathBuf;

pub(crate) trait WorkflowAccess {
    fn configuration(&self) -> &SchedulerConfig;
    fn state(&self) -> &WorkflowState;
    fn history(&self) -> &[Entry];
    fn revision(&self) -> Option<&PlanRevision>;
    fn approval(&self) -> Option<&Approval>;
    fn api_attempt_path(&self, attempt: &str) -> Result<PathBuf, String>;
    fn refresh(&mut self) -> Result<(), String> {
        Ok(())
    }
    fn attempt_record(&self, task: &str, id: &str) -> Option<&Attempt> {
        self.history()
            .iter()
            .rev()
            .find_map(|e| match &e.body.context.execution {
                Some(ExecutionEvidence::Attempt { attempt })
                    if attempt.task_id == task && attempt.id == id =>
                {
                    Some(attempt)
                }
                _ => None,
            })
    }
    fn command(
        &mut self,
        operation: &str,
        principal: &str,
        command: WorkflowCommand,
        context: CommandContext,
        require_new: bool,
    ) -> Result<Entry, String>;
    /// Reserve requests and publish the original dispatch event under one writer.
    #[cfg(windows)]
    fn begin_api_dispatch(&mut self, task: &str, attempt: &str) -> Result<u32, String>;
    #[cfg(windows)]
    fn begin_scoped_api_dispatch(
        &mut self,
        _task: &str,
        _attempt: &str,
        _permit: crate::api_currency::DispatchPermit,
    ) -> Result<u32, String> {
        Err("scoped API dispatch requires the original Host coordinator".into())
    }
    fn record_api(
        &mut self,
        principal: &str,
        command: WorkflowCommand,
        context: CommandContext,
    ) -> Result<(), String> {
        let operation = format!("api-event-{}", self.history().len() + 1);
        self.command(&operation, principal, command, context, true)
            .map(|_| ())
    }
}
impl WorkflowAccess for DurableWorkflow {
    fn configuration(&self) -> &SchedulerConfig {
        self.configuration()
    }
    fn state(&self) -> &WorkflowState {
        self.state()
    }
    fn history(&self) -> &[Entry] {
        self.history()
    }
    fn revision(&self) -> Option<&PlanRevision> {
        self.revision()
    }
    fn approval(&self) -> Option<&Approval> {
        self.approval()
    }
    fn api_attempt_path(&self, attempt: &str) -> Result<PathBuf, String> {
        self.api_attempt_path(attempt)
    }
    #[cfg(windows)]
    fn begin_api_dispatch(&mut self, task: &str, attempt: &str) -> Result<u32, String> {
        let limit = crate::api_execution::dispatch_reservation(self, task, attempt)?;
        let (principal, command, context) =
            crate::api_execution::dispatch_command(self, task, attempt)?;
        self.record_api(&principal, command, context)?;
        Ok(limit)
    }
    fn command(
        &mut self,
        operation: &str,
        principal: &str,
        command: WorkflowCommand,
        context: CommandContext,
        require_new: bool,
    ) -> Result<Entry, String> {
        self.command(operation, principal, command, context, require_new)
    }
}
