//! Original Host queues, scoped transport, and durable fee confirmation.
//! The economic gate is intentionally unchanged; fixtures cannot exist in a
//! production binary. No persisted receipt recreates live send capability.
use super::*;
use crate::management_execution::DecisionIntent;
use cerulean_adapters::openai::{scoped_pricing::ScopedDecisionResult, DecisionResult};

pub(super) enum ManagementCompletion {
    Deputy {
        response: Box<cerulean_adapters::deputy_management::DeputyResult>,
        gateway: Option<crate::gateway_response_evidence::GatewayAttempt>,
    },
    Legacy(Box<DecisionResult>),
    Scoped(Box<ScopedDecisionResult>),
}
#[cfg(test)]
type FixtureCount = std::sync::Arc<
    dyn Fn(
            cerulean_adapters::openai::scoped_pricing::ScopedDecisionDraft,
            cerulean_domain::currency_budget::scoped::ScopedPriceQuote,
        )
            -> Result<cerulean_adapters::openai::scoped_pricing::CountedScopedDecision, String>
        + Send
        + Sync,
>;
#[cfg(test)]
type FixtureSend = std::sync::Arc<
    dyn Fn(
            DecisionIntent,
            cerulean_adapters::openai::scoped_pricing::PreparedScopedDecision,
        ) -> Result<ScopedDecisionResult, String>
        + Send
        + Sync,
>;
#[cfg(test)]
pub(super) struct ScopedFixture {
    pub count: FixtureCount,
    pub send: FixtureSend,
    pub fail_count_spawn: bool,
    pub fail_generation_spawn: bool,
}

impl RuntimeHost {
    pub(super) fn management_original_intents(
        &self,
        project: &str,
    ) -> BTreeMap<String, &DecisionIntent> {
        let mut intents = BTreeMap::new();
        for s in self
            .management
            .statuses()
            .into_iter()
            .filter(|s| s.project_id == project)
        {
            if let Ok(i) = self.management.intent(&s.operation_id) {
                intents.insert(s.operation_id, i);
            }
        }
        if let Some(b) = self.currency_accounts.get(project) {
            for (id, c) in &b.state().management_counts {
                intents.entry(id.clone()).or_insert(&c.attempt.intent);
            }
        }
        intents
    }
    pub(super) fn management_operation_total(&self) -> usize {
        let mut ids: std::collections::BTreeSet<_> = self
            .management
            .statuses()
            .into_iter()
            .map(|s| s.operation_id)
            .collect();
        for book in self.currency_accounts.values() {
            ids.extend(book.state().management_counts.keys().cloned());
        }
        ids.len()
    }
    pub(super) fn management_operation_ids(
        &self,
        project: &str,
    ) -> std::collections::BTreeSet<String> {
        let mut ids: std::collections::BTreeSet<_> = self
            .management
            .statuses()
            .into_iter()
            .filter(|s| s.project_id == project)
            .map(|s| s.operation_id)
            .collect();
        if let Some(book) = self.currency_accounts.get(project) {
            ids.extend(book.state().management_counts.keys().cloned());
        }
        ids
    }
    pub(super) fn scoped_management_configured(&self, project: &str) -> bool {
        self.project_setups
            .get(project)
            .and_then(|s| s.setup.policy.currency_budget.as_ref())
            .is_some_and(|p| p.schema == 2)
    }
    pub(super) fn require_management_dispatch_available(
        &self,
        project: &str,
        purpose: management::Purpose,
    ) -> Result<(), String> {
        if purpose == management::Purpose::Plan {
            return self.require_management_planning_available(project);
        }
        #[cfg(test)]
        if self.management_scoped_fixture.is_some() {
            return Ok(());
        }
        self.require_currency_execution_available(project)
    }
    pub(super) fn record_management_completion(
        &mut self,
        id: &str,
        outcome: Result<ManagementCompletion, String>,
    ) -> Result<(), String> {
        if self.management.is_deputy(id) {
            let (response, gateway) = match outcome {
                Ok(ManagementCompletion::Deputy { response, gateway }) => (*response, gateway),
                Err(_) => (
                    cerulean_adapters::deputy_management::DeputyResult {
                        request: self.management.deputy_evidence(id)?.0,
                        decode_stage: None,
                        outcome: cerulean_adapters::deputy_management::DeputyOutcome::Failed {
                            failure: cerulean_adapters::api_generation::ApiFailure::Transport {},
                        },
                    },
                    None,
                ),
                _ => {
                    return Err(
                        "deputy completion provider mismatch; preserve original evidence".into(),
                    )
                }
            };
            self.management.finish_deputy(id, response)?;
            if let Some(capture) = gateway {
                self.management.finish_gateway(id, capture);
            }
            return Ok(());
        }
        match (self.management.is_scoped(id), outcome) {
            (true, Ok(ManagementCompletion::Scoped(r))) => {
                self.management.finish_scoped_result(id, Ok(*r))
            }
            (false, Ok(ManagementCompletion::Legacy(r))) => {
                self.management.finish_result(id, Ok(*r))
            }
            (true, Err(e)) => self.management.finish_scoped_result(id, Err(e)),
            (false, Err(e)) => self.management.finish_result(id, Err(e)),
            _ => Err(
                "management completion protocol differs from original intent; preserve evidence"
                    .into(),
            ),
        }
    }
}

#[cfg(windows)]
mod dispatch {
    use super::super::management_preflight_host::{
        CountBasis, CountCompletion, CountProof, PendingCount,
    };
    use super::*;
    use cerulean_adapters::openai::{
        scoped_pricing::{PreparedScopedDecision, ScopedDecisionDraft},
        OpenAiClient,
    };
    use cerulean_domain::currency_budget::scoped::InputRange;
    use cerulean_domain::project_setup::RunState;
    use cerulean_domain::{currency_budget::BudgetPolicy, management::Purpose};

    impl RuntimeHost {
        fn scoped_run_authority(&self, intent: &DecisionIntent) -> Result<String, String> {
            require(
                self.runs.healthy() && self.runner_error.is_none(),
                "run control requires recovery",
            )?;
            if intent.purpose != Purpose::Plan {
                for run in self
                    .runs
                    .project_runs(&intent.project_id)
                    .filter(|r| !r.state.terminal())
                {
                    require(
                        run.state == RunState::Active,
                        "project run was paused or needs attention",
                    )?;
                    self.run_scope(run)?;
                }
            }
            Ok(self.runs.project_evidence_digest(&intent.project_id).into())
        }
        fn current_scoped_policy(&self, intent: &DecisionIntent) -> Result<BudgetPolicy, String> {
            let saved = self
                .project_setups
                .get(&intent.project_id)
                .ok_or("original scoped configuration missing")?;
            crate::project_setup::validate(saved)?;
            let requirement = intent.context["requirement"]
                .as_str()
                .ok_or("original scoped requirement missing")?;
            if requirement != saved.setup.requirement {
                // Parallel assignment uses the scheduler's narrow candidate
                // instruction. The saved project requirement remains inside
                // executionSetup; revalidate_scoped_context has authenticated
                // that complete configuration and the original workflow prefix.
                #[derive(serde::Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct ParallelAssignment {
                    parallel_assignment: bool,
                    task_candidates: Vec<String>,
                    instruction: String,
                }
                let scope: ParallelAssignment = serde_json::from_str(requirement)
                    .map_err(|_| "management requirement differs from current configured scope")?;
                require(
                    intent.purpose == Purpose::Assign
                        && scope.parallel_assignment
                        && scope.instruction == "仅分配候选中的一个任务；原始需求见executionSetup.setup.requirement，遵循已批准方案。其他任务仍在执行；不要重复分配、验收或修改方案。"
                        && !scope.task_candidates.is_empty(),
                    "invalid original parallel assignment instruction",
                )?;
                let original: ExecutionSnapshot =
                    serde_json::from_value(intent.context["workflow"].clone())
                        .map_err(|_| "original parallel workflow missing")?;
                let revision = original
                    .revision
                    .as_ref()
                    .ok_or("original parallel plan missing")?;
                let mut seen = std::collections::BTreeSet::new();
                for task in &scope.task_candidates {
                    let state = original
                        .state
                        .tasks
                        .get(task)
                        .ok_or("parallel candidate missing")?;
                    let planned = revision
                        .workflow
                        .tasks
                        .iter()
                        .find(|t| &t.id == task)
                        .ok_or("parallel candidate not approved")?;
                    require(
                        seen.insert(task)
                            && state.state == TaskState::Pending
                            && state.attempt.is_none()
                            && api_assignment_scope::isolated(revision, task)?
                            && planned.dependencies.iter().all(|d| {
                                original
                                    .state
                                    .tasks
                                    .get(d)
                                    .is_some_and(|s| s.state == TaskState::Accepted)
                            }),
                        "parallel candidates differ from original ready tasks",
                    )?;
                }
            }
            if intent.purpose == Purpose::Plan {
                require(
                    saved.setup.chief_model == intent.model,
                    "chief planning model changed",
                )?;
                return saved
                    .setup
                    .policy
                    .currency_budget
                    .as_ref()
                    .ok_or("planning budget missing")?
                    .bind_planning(&intent.project_id, &saved.digest, &intent.model);
            }
            let original: ExecutionSnapshot =
                serde_json::from_value(intent.context["workflow"].clone())
                    .map_err(|_| "original management workflow missing")?;
            let revision = original
                .revision
                .as_ref()
                .ok_or("original execution plan missing")?;
            let approved = original
                .approval
                .as_ref()
                .ok_or("original execution approval missing")?;
            approved.validate(revision, &approved.user_id, revision.policy.version)?;
            require(
                original.project.id == intent.project_id
                    && original.state.approved
                    && original.state.revision_requested.is_none()
                    && saved.setup.policy == revision.policy,
                "original management approval or configuration changed",
            )?;
            let model = if intent.purpose == Purpose::Accept {
                &saved.setup.chief_model
            } else {
                &saved.setup.deputy_model
            };
            require(model == &intent.model, "original role model changed")?;
            revision
                .policy
                .currency_budget
                .as_ref()
                .ok_or("execution money budget missing")?
                .bind(&intent.project_id, &revision.approval_digest()?)
        }
        fn revalidate_scoped_context(&self, intent: &DecisionIntent) -> Result<(), String> {
            intent.validate()?;
            if intent.context.get("supervision").is_some() {
                return self.revalidate_counted_supervision(intent);
            }
            let requirement = intent.context["requirement"]
                .as_str()
                .ok_or("original management requirement missing")?;
            let original: ExecutionSnapshot =
                serde_json::from_value(intent.context["workflow"].clone())
                    .map_err(|_| "original management workflow missing")?;
            if matches!(intent.purpose, Purpose::Assign | Purpose::Accept)
                && original
                    .revision
                    .as_ref()
                    .is_some_and(|r| r.worktree.is_some())
            {
                // Preserve the original authenticated workflow prefix while other
                // isolated tasks finish. Applying the reply still validates its
                // exact selected task through the original assignment/acceptance gate.
                self.require_available_inner(&intent.project_id, true)?;
                let task = original
                    .state
                    .tasks
                    .keys()
                    .next()
                    .ok_or("original worktree task missing")?;
                let store = self.api_read(&intent.project_id, task, "count-preflight")?;
                let prefix = crate::workflow_prefix::prefix(&*store, original.sequence)?;
                require(
                    digest(&prefix)? == digest(&original)?,
                    "count context is not an original workflow prefix",
                )?;
                let current =
                    crate::workflow_prefix::prefix(&*store, store.history().len() as u32)?;
                require(
                    current.project == original.project
                        && current.revision_digest == original.revision_digest
                        && current.approval == original.approval
                        && current.state.approved
                        && current.state.revision_requested.is_none(),
                    "counted execution approval changed",
                )?;
                for e in &store.history()[original.sequence as usize..] {
                    require(
                        e.body.event.command.task().is_some()
                            && matches!(
                                e.body.event.command,
                                WorkflowCommand::Assign { .. }
                                    | WorkflowCommand::DispatchIntent { .. }
                                    | WorkflowCommand::Started { .. }
                                    | WorkflowCommand::Submit { .. }
                                    | WorkflowCommand::RecoveryUnknown { .. }
                                    | WorkflowCommand::Reconcile { .. }
                                    | WorkflowCommand::Verify { .. }
                                    | WorkflowCommand::Accept { .. }
                                    | WorkflowCommand::CancelIntent { .. }
                                    | WorkflowCommand::Advise { .. }
                            ),
                        "global workflow authority changed during count",
                    )?;
                }
                require(
                    digest(&self.match_original_catalog(
                        &intent.context,
                        self.context_for_snapshot(requirement, &original),
                    )?)? == intent.state_digest,
                    "management configuration or user answers changed during count",
                )
            } else {
                if intent.purpose == Purpose::Plan {
                    self.planning_idle(&intent.project_id)?;
                }
                require(
                    digest(&self.match_original_catalog(
                        &intent.context,
                        self.management_context(&intent.project_id, requirement)?,
                    )?)? == intent.state_digest,
                    "original management context changed during count",
                )
            }
        }
        fn scoped_count_basis(
            &mut self,
            intent: &DecisionIntent,
            returning: bool,
        ) -> Result<CountBasis, String> {
            self.require_management_dispatch_available(&intent.project_id, intent.purpose)?;
            self.management
                .require_intent_capacity(self.management_counts.len())?;
            let authority = Some(self.scoped_run_authority(intent)?);
            crate::currency_scoped_execution_guard::require_protected(&self.root, true)?;
            require(
                self.management.healthy(),
                "management journal requires recovery",
            )?;
            require(
                self.management
                    .absent_prepared_intent(&intent.operation_id)?,
                "management operation already recorded; never resend",
            )?;
            require(
                !self
                    .management
                    .statuses()
                    .iter()
                    .any(|s| s.project_id == intent.project_id && !s.state.resolved()),
                "original project management decision needs disposition",
            )?;
            self.revalidate_scoped_context(intent)?;
            let policy = self.current_scoped_policy(intent)?;
            require(
                policy.schema == 2,
                "scoped dispatch requires original scoped policy",
            )?;
            self.reconcile_currency_account(&intent.project_id)?;
            let history = self.currency_history_gaps();
            let available = self
                .currency_account_views(history.as_deref())
                .into_iter()
                .find(|v| v.project_id == intent.project_id)
                .and_then(|v| v.available_usd_nanos)
                .ok_or("currency history or approval is unresolved")?;
            let book = self
                .currency_accounts
                .get(&intent.project_id)
                .ok_or("approved scoped account missing")?;
            require(
                book.policy() == &policy
                    && book.state().approval_links.contains_key(&digest(&policy)?),
                "original scoped approval changed",
            )?;
            let mut ids = self.management_operation_ids(&intent.project_id);
            if returning {
                let count = book
                    .state()
                    .management_counts
                    .get(&intent.operation_id)
                    .ok_or("original scoped count missing")?;
                require(
                    count.attempt.intent == *intent
                        && count.retirement.is_none()
                        && matches!(
                            count.outcome,
                            crate::currency_budget::CountOutcome::ScopedCounted { .. }
                        ),
                    "original scoped count is changed, retired or unresolved",
                )?;
                ids.remove(&intent.operation_id);
            } else {
                require(
                    !ids.contains(&intent.operation_id),
                    "count operation already accounted; never recount",
                )?;
            }
            require(
                ids.len()
                    < self.project_setups[&intent.project_id]
                        .setup
                        .management_max_requests as usize,
                "项目主副指挥请求上限已用完",
            )?;
            require(
                !book.state().bookings.contains_key(&intent.operation_id),
                "original generation already reserved; never resend",
            )?;
            let draft = ScopedDecisionDraft::standard(
                intent.purpose,
                &intent.model,
                &intent.state_digest,
                &intent.context,
            )?;
            let quotes: Vec<_> = policy
                .scoped_quotes
                .as_ref()
                .ok_or("scoped quotes missing")?
                .iter()
                .filter(|q| {
                    q.target == api_worker::ApiTarget::OpenAi {}
                        && q.requested_model == intent.model
                        && q.requested_service == draft.fingerprint().requested_service
                })
                .collect();
            require(
                quotes.len() == 1,
                "one original unambiguous scoped quote required",
            )?;
            let quote = quotes[0];
            let fee = quote
                .count_price
                .as_ref()
                .ok_or("explicit count fee required; unknown is not free")?
                .maximum_usd_nanos;
            let minimum = draft
                .review(
                    quote,
                    InputRange {
                        from: 0,
                        through: 0,
                    },
                    &chrono::Utc::now().to_rfc3339(),
                )?
                .audit()
                .maximum_usd_nanos;
            require(
                minimum
                    .checked_add(if returning { 0 } else { fee })
                    .is_some_and(|n| n <= available),
                "currency budget exhausted before scoped counting",
            )?;
            Ok(CountBasis {
                authority,
                policy: digest(&policy)?,
                ledger_head: book.management_head().into(),
                quote: digest(quote)?,
            })
        }
        pub(in crate::host) fn start_scoped_management(
            &mut self,
            intent: DecisionIntent,
        ) -> Result<(), String> {
            // No key access, runtime, count publication or network before this gate.
            self.require_management_dispatch_available(&intent.project_id, intent.purpose)?;
            require(
                self.management_work_count() < 4,
                "management concurrency limit reached",
            )?;
            require(
                !self.counting_project(&intent.project_id)
                    && !self.management_active.contains_key(&intent.operation_id),
                "original management work is still active",
            )?;
            require(
                self.openai_connection.key_configured && self.openai_probe.is_none(),
                "OpenAI authentication unavailable or being checked",
            )?;
            let mut basis = self.scoped_count_basis(&intent, false)?;
            let draft = ScopedDecisionDraft::standard(
                intent.purpose,
                &intent.model,
                &intent.state_digest,
                &intent.context,
            )?;
            let quote = self.currency_accounts[&intent.project_id]
                .policy()
                .scoped_quote(&basis.quote)?
                .clone();
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| "management counting runtime unavailable")?;
            let count: Box<dyn FnOnce() -> Result<CountCompletion, String> + Send>;
            #[cfg(test)]
            if let Some(fixture) = &self.management_scoped_fixture {
                let callback = fixture.count.clone();
                let q = quote.clone();
                count = Box::new(move || {
                    Ok(CountCompletion {
                        runtime,
                        counted: CountProof::Scoped(Box::new(callback(draft, q)?)),
                    })
                });
            } else {
                let client = OpenAiClient::new(crate::credential_vault::load(&self.root)?)?;
                count = Box::new(move || {
                    let counted = runtime.block_on(client.count_scoped_decision(draft, quote))?;
                    Ok(CountCompletion {
                        runtime,
                        counted: CountProof::Scoped(Box::new(counted)),
                    })
                });
            }
            #[cfg(not(test))]
            {
                let client = OpenAiClient::new(crate::credential_vault::load(&self.root)?)?;
                count = Box::new(move || {
                    let counted = runtime.block_on(client.count_scoped_decision(draft, quote))?;
                    Ok(CountCompletion {
                        runtime,
                        counted: CountProof::Scoped(Box::new(counted)),
                    })
                });
            }
            let book = self
                .currency_accounts
                .get_mut(&intent.project_id)
                .ok_or("original scoped account missing")?;
            let original = ScopedDecisionDraft::standard(
                intent.purpose,
                &intent.model,
                &intent.state_digest,
                &intent.context,
            )?;
            book.begin_scoped_management_count(&intent, &original, &basis.quote)?;
            basis.ledger_head = book.management_head().into();
            let (sender, receiver) = std::sync::mpsc::channel();
            let fail = false;
            #[cfg(test)]
            let fail = fail
                || self
                    .management_scoped_fixture
                    .as_ref()
                    .is_some_and(|f| f.fail_count_spawn);
            let spawned = if fail {
                Err(std::io::Error::other(
                    "synthetic scoped count spawn failure",
                ))
            } else {
                std::thread::Builder::new()
                    .name("openai-scoped-count".into())
                    .spawn(move || {
                        let _ = sender.send(count());
                    })
            };
            if spawned.is_err() {
                let error = self.fail_count_attempt(
                    &intent,
                    "scoped count worker not started; no retry".into(),
                );
                self.management_count_errors
                    .insert(intent.project_id.clone(), error.clone());
                return Err(error);
            }
            self.management_counts.insert(
                intent.operation_id.clone(),
                PendingCount {
                    intent,
                    basis,
                    scoped: true,
                    receiver,
                },
            );
            Ok(())
        }
        pub(in crate::host) fn finish_scoped_count(
            &mut self,
            mut pending: PendingCount,
            completion: CountCompletion,
        ) -> Result<(), String> {
            let CountCompletion { runtime, counted } = completion;
            let CountProof::Scoped(counted) = counted else {
                return Err("scoped count received legacy proof".into());
            };
            let book = self
                .currency_accounts
                .get_mut(&pending.intent.project_id)
                .ok_or("original scoped account missing")?;
            let unchanged = book.management_head() == pending.basis.ledger_head;
            // Received count fees never disappear on later scope, quote or budget failure.
            book.record_scoped_management_count(&pending.intent.operation_id, &counted)?;
            pending.basis.ledger_head = book.management_head().into();
            require(unchanged, "currency ledger changed during scoped preflight")?;
            require(
                self.scoped_count_basis(&pending.intent, true)? == pending.basis,
                "original scoped authority changed during count",
            )?;
            let prepared = counted.prepare()?;
            self.currency_accounts
                .get_mut(&pending.intent.project_id)
                .ok_or("original scoped account missing")?
                .prepare_scoped_management(&pending.intent, &prepared)?;
            self.launch_scoped_generation(pending.intent, prepared, runtime)
        }
        fn launch_scoped_generation(
            &mut self,
            intent: DecisionIntent,
            prepared: PreparedScopedDecision,
            runtime: tokio::runtime::Runtime,
        ) -> Result<(), String> {
            // Full preparation is already durable. Journal publication is the last
            // persistent operation before starting this one-use generation object.
            let operation = intent.operation_id.clone();
            self.management.begin_scoped(intent.clone(), &prepared)?;
            let (sender, receiver) = std::sync::mpsc::channel();
            #[cfg(test)]
            let fixture = self
                .management_scoped_fixture
                .as_ref()
                .map(|f| f.send.clone());
            let fail = false;
            #[cfg(test)]
            let fail = fail
                || self
                    .management_scoped_fixture
                    .as_ref()
                    .is_some_and(|f| f.fail_generation_spawn);
            let spawned = if fail {
                Err(std::io::Error::other(
                    "synthetic scoped generation spawn failure",
                ))
            } else {
                std::thread::Builder::new()
                    .name("openai-scoped-management".into())
                    .spawn(move || {
                        let outcome = (|| {
                            prepared.check_ready()?;
                            #[cfg(test)]
                            if let Some(send) = fixture {
                                return send(intent, prepared);
                            }
                            let _ = intent;
                            runtime.block_on(prepared.decide())
                        })()
                        .map(|r| ManagementCompletion::Scoped(Box::new(r)));
                        let _ = sender.send(outcome);
                    })
            };
            if spawned.is_err() {
                self.management.finish_scoped_result(
                    &operation,
                    Err("scoped generation worker not started".into()),
                )?;
                return Err(
                    "scoped generation not started; original preparation and intent retained"
                        .into(),
                );
            }
            self.management_active.insert(operation, receiver);
            Ok(())
        }
    }
}
