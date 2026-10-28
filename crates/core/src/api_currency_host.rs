//! RuntimeHost remains the only monetary writer during API background work.
use super::*;
use crate::{
    api_call_journal::{ApiCallJournal, CallIdentity, CallOutcome},
    api_currency::{Authority, CountApproval, DispatchPermit, PreparedApproval, Request},
    api_execution::ApiRunIdentity,
};
use cerulean_adapters::api_generation::scoped::{CountedScopedApi, ScopedApiDraft};
use cerulean_domain::{api_jobs::ApiJobState, project_setup::RunState};

impl RuntimeHost {
    pub(super) fn require_api_currency_available(&self, project: &str) -> Result<(), String> {
        #[cfg(test)]
        if self.api_scoped_fixture.is_some() {
            return Ok(());
        }
        self.require_currency_execution_available(project)
    }
    pub(super) fn api_original_scope(
        &self,
        project: &str,
        attempt: &str,
        run: &ApiRunIdentity,
    ) -> Result<PathBuf, String> {
        require(
            run.project_id == project && run.attempt_id == attempt,
            "API money channel owner mismatch",
        )?;
        let active = self
            .api_active
            .get(&(project.into(), attempt.into()))
            .ok_or("original API worker missing")?;
        let startup = self
            .api_startups
            .get(&(project.into(), attempt.into()))
            .ok_or("original API startup missing")?;
        require(
            active.job.task_id == run.task_id
                && active.job.revision_digest == run.revision_digest
                && startup.attempt.binding == run.binding
                && startup.attempt.fence == run.fence
                && startup.worker == run.worker,
            "API money scope differs from original worker",
        )?;
        // Late outcomes belong to the saved startup/count, even if current
        // authorization has changed. Only new work consults the live plan.
        Ok(self
            .root
            .join("workflows")
            .join(project)
            .join(format!("api-attempt-{}", digest(&attempt)?)))
    }
    pub(super) fn api_live_scope(
        &self,
        project: &str,
        attempt: &str,
        run: &ApiRunIdentity,
        dispatch: bool,
    ) -> Result<PlanRevision, String> {
        self.api_original_scope(project, attempt, run)?;
        require(
            self.api_fault.is_none() && self.runner_error.is_none() && self.runs.healthy(),
            "API or project control needs recovery",
        )?;
        let active = &self.api_active[&(project.into(), attempt.into())];
        require(
            !active.disconnected
                && active.cancel.is_some()
                && active.job.state != ApiJobState::CancelRequested
                && !active
                    .preparation_cancel
                    .load(std::sync::atomic::Ordering::Acquire),
            "API worker cancelled or disconnected",
        )?;
        for control in self
            .runs
            .project_runs(project)
            .filter(|r| !r.state.terminal())
        {
            require(
                control.state == RunState::Active,
                "API project run paused or needs attention",
            )?;
            self.run_scope(control)?;
        }
        let store = self.api_read(project, &run.task_id, attempt)?;
        let (_, _, current, _) =
            crate::api_execution::evidence_scope(&*store, &run.task_id, attempt)?;
        require(&current == run, "API current approval or run changed")?;
        require(
            store.state().approved && store.state().revision_requested.is_none(),
            "API approval changed or revision requested",
        )?;
        let task = store
            .state()
            .tasks
            .get(&run.task_id)
            .ok_or("original API task missing")?;
        require(
            task.attempt.as_deref() == Some(attempt)
                && task.cancellation_requested != Some(true)
                && if dispatch {
                    task.state == TaskState::Assigned
                } else {
                    matches!(task.state, TaskState::Dispatching | TaskState::Running)
                },
            "API task no longer authorizes this step",
        )?;
        store
            .revision()
            .cloned()
            .ok_or_else(|| "API plan missing".into())
    }
    fn api_live_authority(
        &self,
        project: &str,
        attempt: &str,
        run: &ApiRunIdentity,
        dispatch: bool,
    ) -> Result<Authority, String> {
        self.require_api_currency_available(project)?;
        crate::currency_api_guard::require_protected(&self.root, true)?;
        let plan = self.api_live_scope(project, attempt, run, dispatch)?;
        let policy = plan
            .policy
            .currency_budget
            .as_ref()
            .ok_or("API monetary policy missing")?
            .bind(project, &run.revision_digest)?;
        let book = self
            .currency_accounts
            .get(project)
            .ok_or("original API account missing")?;
        require(
            book.healthy()
                && book.policy() == &policy
                && policy.schema == 2
                && book
                    .state()
                    .approval_links
                    .contains_key(&book.state().current_policy_digest),
            "original API monetary authorization changed",
        )?;
        let setup = self
            .project_setups
            .get(project)
            .ok_or("original API setup missing")?;
        crate::project_setup::validate(setup)?;
        Ok(Authority {
            policy: digest(&policy)?,
            setup: setup.digest.clone(),
            run_control: self.runs.project_evidence_digest(project).into(),
        })
    }
    fn api_original_count(
        &self,
        project: &str,
        attempt: &str,
        run: &ApiRunIdentity,
        call: &CallIdentity,
    ) -> Result<(), String> {
        let root = self.api_original_scope(project, attempt, run)?;
        crate::api_execution::require_original_identity(&root, run)?;
        crate::api_execution::check_round_scope(run, call, call.round)?;
        let book = self
            .currency_accounts
            .get(project)
            .ok_or("original API account missing")?;
        let count = book
            .state()
            .api_counts
            .get(&call.request_id)
            .ok_or("original API count missing")?;
        require(
            count.attempt.run == *run && count.attempt.call == *call,
            "API result belongs to another original count",
        )
    }
    fn api_count_approval(
        &mut self,
        project: &str,
        attempt: &str,
        run: &ApiRunIdentity,
        call: &CallIdentity,
        draft: ScopedApiDraft,
    ) -> Result<CountApproval, String> {
        let authority = self.api_live_authority(project, attempt, run, false)?;
        crate::api_execution::check_round_scope(run, call, call.round)?;
        require(
            draft.fingerprint().request == call.fingerprint,
            "API count draft differs from original call",
        )?;
        let root = self.api_original_scope(project, attempt, run)?;
        let store = self.api_read(project, &run.task_id, attempt)?;
        crate::api_execution::require_original_identity(&root, run)?;
        let dispatch = store.history().iter().find(|e| matches!(&e.body.event.command,
            WorkflowCommand::DispatchIntent { task, attempt: id } if task == &run.task_id && id == attempt))
            .ok_or("original API dispatch missing")?.hash.clone();
        // The coordinator owns the request budget. Count only rounds within
        // the original attempt reservation, and never skip an unsettled round.
        let allowed = self
            .api_writers
            .get(project)
            .ok_or("original API coordinator missing")?
            .owner
            .api_request_limit(crate::workflow_coordinator::Scope {
                project: project.into(),
                task: run.task_id.clone(),
                attempt: attempt.into(),
                revision: run.revision_digest.clone(),
            })?;
        require(
            (1..=allowed).contains(&call.round),
            "API count exceeds original request reservation",
        )?;
        drop(store);
        let book = self
            .currency_accounts
            .get(project)
            .ok_or("original API account missing")?;
        for n in 1..call.round {
            let journal =
                ApiCallJournal::read_existing(&root.join("rounds").join(format!("{n:03}")))?;
            crate::api_execution::check_round_scope(run, journal.identity(), n)?;
            book.check_api(&journal)?;
            journal.require_fee_ack()?;
            require(
                matches!(&journal.state().outcome, Some(CallOutcome::Reply { observation }) if !observation.tool_calls.is_empty()),
                "API next count follows an unfinished or final round",
            )?;
        }
        let quote = book
            .policy()
            .scoped_quotes
            .as_ref()
            .ok_or("API scoped quotes missing")?
            .iter()
            .find(|q| {
                q.target == run.worker.target
                    && q.requested_model == run.worker.model
                    && q.requested_service == draft.fingerprint().requested_service
            })
            .ok_or("API requested service is not quoted")?
            .clone();
        quote.valid_at(&chrono::Utc::now().to_rfc3339())?;
        let quote_digest = digest(&quote)?;
        self.currency_accounts
            .get_mut(project)
            .unwrap()
            .begin_api_count(run, call, &draft, &dispatch, &quote_digest)?;
        Ok(CountApproval {
            draft,
            quote,
            authority,
        })
    }
    fn api_count_returned(
        &mut self,
        project: &str,
        attempt: &str,
        run: &ApiRunIdentity,
        call: &CallIdentity,
        authority: Authority,
        counted: CountedScopedApi,
    ) -> Result<PreparedApproval, String> {
        self.api_original_count(project, attempt, run, call)?;
        self.currency_accounts
            .get_mut(project)
            .unwrap()
            .record_api_count(&call.request_id, &counted)?;
        // Preserve the paid receipt before checking pause/cancel/new approval.
        require(
            self.api_live_authority(project, attempt, run, false)? == authority,
            "API count authority changed; receipt retained without generation",
        )?;
        let prepared = counted.prepare()?;
        let book = self.currency_accounts.get_mut(project).unwrap();
        book.prepare_scoped_api(&call.request_id, &prepared)?;
        let permit = book.api_preparation_permit(&call.request_id, &prepared)?;
        Ok(PreparedApproval { prepared, permit })
    }
    pub(super) fn handle_api_currency(&mut self, project: &str, attempt: &str, request: Request) {
        match request {
            Request::Dispatch { run, reply } => {
                let result = self
                    .api_live_authority(project, attempt, &run, true)
                    .map(|_| DispatchPermit::new(*run));
                let _ = reply.send(result);
            }
            Request::Count {
                run,
                call,
                draft,
                reply,
            } => {
                let result = self.api_count_approval(project, attempt, &run, &call, *draft);
                let _ = reply.send(result);
            }
            Request::Counted {
                run,
                call,
                authority,
                result,
                reply,
            } => {
                let result = match result {
                    Ok(counted) => {
                        self.api_count_returned(project, attempt, &run, &call, authority, *counted)
                    }
                    Err(failure) => (|| {
                        self.api_original_count(project, attempt, &run, &call)?;
                        let hash = digest(&("original-api-count-failure-v1", &call, &failure))?;
                        self.currency_accounts
                            .get_mut(project)
                            .unwrap()
                            .mark_api_count_unknown(&call.request_id, &hash)?;
                        Err(format!(
                            "API count failed: {failure:?}; retain count fee, never retry"
                        ))
                    })(),
                };
                let _ = reply.send(result);
            }
            Request::Settle {
                run,
                authority,
                journal,
                reply,
            } => {
                let result = (|| {
                    self.api_original_count(project, attempt, &run, journal.identity())?;
                    let root = self.api_original_scope(project, attempt, &run)?;
                    journal.require_original_path(
                        &root
                            .join("rounds")
                            .join(format!("{:03}", journal.identity().round)),
                    )?;
                    self.currency_accounts
                        .get_mut(project)
                        .unwrap()
                        .observe_api(&journal)?;
                    require(
                        self.api_live_authority(project, attempt, &run, false)? == authority,
                        "API execution authority changed; original fee retained",
                    )?;
                    let permit = self.currency_accounts[project].api_fee_permit(&journal)?;
                    Ok((*journal, permit))
                })();
                let _ = reply.send(result);
            }
            Request::Continue {
                run,
                authority,
                call,
                reply,
            } => {
                let result = (|| {
                    require(
                        self.api_live_authority(project, attempt, &run, false)? == authority,
                        "API authority changed before tool or completion",
                    )?;
                    self.api_original_count(project, attempt, &run, &call)?;
                    let root = self.api_original_scope(project, attempt, &run)?;
                    let journal = ApiCallJournal::read_existing(
                        &root.join("rounds").join(format!("{:03}", call.round)),
                    )?;
                    require(
                        journal.identity() == call.as_ref(),
                        "API continuation call changed",
                    )?;
                    self.currency_accounts[project].check_api(&journal)?;
                    journal.require_fee_ack()
                })();
                let _ = reply.send(result);
            }
        }
    }
}
