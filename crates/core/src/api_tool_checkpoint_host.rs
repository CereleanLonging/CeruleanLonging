//! Validate worker-owned tool checkpoints against original API/run authority.
//! Observations are memory projections of existing journals, never new grants.
use super::*;
use crate::{
    api_call_journal::ApiCallJournal,
    api_tool_checkpoint::{Checkpoint, Request},
};

#[cfg(test)]
#[path = "api_tool_checkpoint_tests.rs"]
pub(super) mod tests;

impl RuntimeHost {
    /// Read the last original-owner checkpoint. This does not pause the worker
    /// or authorize a management request; historical verification is separate.
    pub fn api_checkpoint_evidence(
        &self,
        project: &str,
        attempt: &str,
    ) -> Result<Option<crate::ApiToolEvidence>, String> {
        let Some(active) = self.api_active.get(&(project.into(), attempt.into())) else {
            return Ok(None);
        };
        let Some(checkpoint) = &active.tool_checkpoint else {
            return Ok(None);
        };
        self.api_live_scope(project, attempt, &checkpoint.run, false)?;
        Ok(Some(checkpoint.evidence()))
    }
    pub(super) fn accept_api_tool_checkpoint(
        &mut self,
        project: &str,
        attempt: &str,
        checkpoint: Checkpoint,
    ) -> Result<(), String> {
        let run = &checkpoint.run;
        let plan = self.api_live_scope(project, attempt, run, false)?;
        let root = self.api_original_scope(project, attempt, run)?;
        crate::api_execution::require_original_identity(&root, run)?;
        let active = self
            .api_active
            .get(&(project.into(), attempt.into()))
            .ok_or("Original API owner missing")?;
        require(
            active.job.state == cerulean_domain::api_jobs::ApiJobState::Executing
                && checkpoint.tools.observation().calls.len() <= run.worker.max_tool_calls as usize,
            "API tool checkpoint exceeds its active attempt or tool budget",
        )?;
        let allowed = self
            .api_writers
            .get(project)
            .ok_or("Original API writer missing")?
            .owner
            .api_request_limit(crate::workflow_coordinator::Scope {
                project: project.into(),
                task: run.task_id.clone(),
                attempt: attempt.into(),
                revision: run.revision_digest.clone(),
            })?;
        require(
            (1..=allowed).contains(&checkpoint.call.round),
            "API checkpoint exceeds original round reservation",
        )?;
        let journal = ApiCallJournal::read_existing(
            &root
                .join("rounds")
                .join(format!("{:03}", checkpoint.call.round)),
        )?;
        require(
            journal.state().scoped_request.is_some()
                == plan
                    .policy
                    .currency_budget
                    .as_ref()
                    .is_some_and(|p| p.schema == 2),
            "API checkpoint pricing mode differs from original approval",
        )?;
        if journal.state().scoped_request.is_some() {
            self.currency_accounts
                .get(project)
                .ok_or("Original API account missing")?
                .check_api(&journal)?;
        }
        checkpoint.validate(active.tool_checkpoint.as_ref(), &journal)?;
        self.api_active
            .get_mut(&(project.into(), attempt.into()))
            .unwrap()
            .tool_checkpoint = Some(checkpoint);
        Ok(())
    }
    pub(super) fn handle_api_tool_checkpoint(
        &mut self,
        project: &str,
        attempt: &str,
        request: Request,
    ) {
        let result = self
            .accept_api_tool_checkpoint(project, attempt, request.checkpoint)
            .and_then(|_| self.api_advice_boundary_needed(project, attempt));
        match result {
            Ok(true) => {
                let active = self
                    .api_active
                    .get_mut(&(project.into(), attempt.into()))
                    .unwrap();
                if active.advice_waiter.is_some() {
                    let _ = request
                        .reply
                        .send(Err("Original API boundary already held".into()));
                    return;
                }
                active.advice_waiter = Some(request.reply);
                if let Err(error) = self.request_api_supervision(project, attempt) {
                    if let Some(reply) = self
                        .api_active
                        .get_mut(&(project.into(), attempt.into()))
                        .and_then(|a| a.advice_waiter.take())
                    {
                        let _ = reply.send(Err(error));
                    }
                }
            }
            Ok(false) => {
                let result = self.request_api_supervision(project, attempt).map(|_| None);
                let _ = request.reply.send(result);
            }
            Err(error) => {
                let _ = request.reply.send(Err(error));
            }
        }
    }
}
