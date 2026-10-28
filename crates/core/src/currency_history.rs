//! Read-only historical uncertainty. Never invent prices or allocate charges.
use super::*;
use cerulean_domain::currency_budget::{CurrencyHistoryGap, CurrencyHistoryReason};

impl RuntimeHost {
    /// Trusted read-only review preparation; no public IPC, accounting or send.
    pub fn historical_source_for_review(
        &self,
        scope: &crate::currency_budget::HistoricalScope,
    ) -> Result<crate::currency_budget::HistoricalSource, String> {
        self.historical_source_inner(scope, true)
    }
    pub(super) fn historical_source_inner(
        &self,
        scope: &crate::currency_budget::HistoricalScope,
        published: bool,
    ) -> Result<crate::currency_budget::HistoricalSource, String> {
        use crate::currency_budget::HistoricalScope;
        let source = match scope {
            HistoricalScope::Management { operation_id, .. } => {
                require(
                    !self.management_active.contains_key(operation_id),
                    "original management request is still active",
                )?;
                self.management.historical_source(operation_id, published)?
            }
            HistoricalScope::Api {
                project_id,
                attempt_id,
            } => {
                #[cfg(windows)]
                {
                    if let Some(store) = self.projects.get(project_id) {
                        require(
                            !self.api_writers.contains_key(project_id)
                                && !self.api_active.keys().any(|(p, _)| p == project_id),
                            "historical workflow has conflicting owners",
                        )?;
                        let inventory = crate::api_execution::CallInventory::read(store)?;
                        inventory.historical_source(store, attempt_id)?
                    } else {
                        require(
                            !self
                                .api_active
                                .contains_key(&(project_id.clone(), attempt_id.clone())),
                            "original API request is still active",
                        )?;
                        let writer = self
                            .api_writers
                            .get(project_id)
                            .ok_or("historical project missing")?;
                        require(
                            !writer.interrupted,
                            "historical coordinator requires reopen",
                        )?;
                        let snapshot = writer.owner.snapshot()?;
                        let task = snapshot
                            .state
                            .tasks
                            .keys()
                            .next()
                            .ok_or("historical coordinator has no approved task")?;
                        let reader = self.api_read(project_id, task, "historical-read")?;
                        crate::api_execution::CallInventory::historical_source_from_active(
                            &*reader, attempt_id,
                        )?
                    }
                }
                #[cfg(not(windows))]
                {
                    let _ = (project_id, attempt_id);
                    return Err("API history review requires the Windows execution host".into());
                }
            }
        };
        require(
            &source.scope == scope,
            "historical source differs from the requested original scope",
        )?;
        Ok(source)
    }
    pub(super) fn currency_history_gaps(&self) -> Option<Vec<CurrencyHistoryGap>> {
        if !self.management.healthy() || self.projects.values().any(|p| p.requires_reopen()) {
            return None;
        }
        let mut gaps = self.management.currency_history_gaps();
        for (project, entries) in self.currency_workflow_entries().ok()? {
            // Use validated immutable workflow history, not current plan settings
            // or a filesystem scan on every UI poll. One entry is an execution
            // scope; it is deliberately not labelled as one provider request.
            let mut revisions = BTreeMap::new();
            let mut unbudgeted = BTreeMap::new();
            let mut dispatched = std::collections::BTreeSet::new();
            for entry in entries.iter() {
                if let Some(ExecutionEvidence::Revision { revision }) =
                    &entry.body.context.execution
                {
                    revisions.insert(
                        revision.approval_digest().ok()?,
                        revision.policy.currency_budget.is_some(),
                    );
                }
                if let WorkflowCommand::Assign { attempt, .. } = &entry.body.event.command {
                    if let Some(ExecutionEvidence::Attempt { attempt: bound }) =
                        &entry.body.context.execution
                    {
                        if cerulean_domain::api_worker::is_api_adapter(&bound.binding.adapter_id) {
                            let budgeted = *revisions.get(&bound.revision_digest)?;
                            if !budgeted {
                                unbudgeted.insert(attempt.clone(), bound.task_id.clone());
                            }
                        }
                    }
                }
                if let WorkflowCommand::DispatchIntent { task, attempt } = &entry.body.event.command
                {
                    if unbudgeted.get(attempt) == Some(task) && dispatched.insert(attempt.clone()) {
                        gaps.push(CurrencyHistoryGap {
                            project_id: Some(project.clone()),
                            source_id: attempt.clone(),
                            reason: CurrencyHistoryReason::ApiWithoutBudget,
                            reference: format!("workflow:{project}:{}", entry.hash),
                        });
                    }
                }
            }
        }
        gaps.sort_by(|a, b| {
            (&a.project_id, &a.source_id, &a.reference).cmp(&(
                &b.project_id,
                &b.source_id,
                &b.reference,
            ))
        });
        Some(gaps)
    }
}
