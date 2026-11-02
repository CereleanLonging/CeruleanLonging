//! Count off the host thread, then revalidate before durable preparation/send.
//! Live monetary counting remains disabled until its own cost and pricing-tier
//! contract is implemented. The only bypass is an in-process cfg(test) fixture.
use super::*;
use crate::management_execution::DecisionIntent;
use cerulean_adapters::openai::{CountedDecision, PreparedDecision};

pub(super) struct PendingCount {
    pub(super) intent: DecisionIntent,
    pub(super) basis: CountBasis,
    pub(super) scoped: bool,
    pub(super) receiver: std::sync::mpsc::Receiver<Result<CountCompletion, String>>,
}
pub(super) struct CountCompletion {
    pub(super) runtime: tokio::runtime::Runtime,
    pub(super) counted: CountProof,
}
pub(super) enum CountProof {
    Legacy(Box<CountedDecision>),
    Scoped(Box<cerulean_adapters::openai::scoped_pricing::CountedScopedDecision>),
}
#[derive(PartialEq, Eq)]
pub(super) struct CountBasis {
    pub(super) policy: String,
    pub(super) ledger_head: String,
    pub(super) quote: String,
    pub(super) authority: Option<String>,
}
#[cfg(test)]
type FixtureCount =
    std::sync::Arc<dyn Fn(PreparedDecision) -> Result<CountedDecision, String> + Send + Sync>;
#[cfg(test)]
type FixtureSend = std::sync::Arc<
    dyn Fn(DecisionIntent) -> Result<cerulean_adapters::openai::DecisionResult, String>
        + Send
        + Sync,
>;
#[cfg(test)]
pub(super) struct CountFixture {
    pub count: FixtureCount,
    pub send: FixtureSend,
    pub fail_spawn: bool,
}

impl RuntimeHost {
    pub(super) fn management_work_count(&self) -> usize {
        self.management_active.len() + self.management_counts.len()
    }
    pub(super) fn counting_project(&self, project: &str) -> bool {
        self.management_counts
            .values()
            .any(|p| p.intent.project_id == project)
    }
    pub(super) fn require_management_planning_available(
        &self,
        project: &str,
    ) -> Result<(), String> {
        if let Some(saved) = self.project_setups.get(project) {
            saved.setup.require_available_adapters()?;
        }
        if self.require_currency_execution_available(project).is_ok() {
            return Ok(());
        }
        // Never infer authority from a quote, environment variable or API key.
        // These hooks cannot exist in the product binary.
        #[cfg(test)]
        if self.management_count_fixture.is_some() || self.management_scoped_fixture.is_some() {
            return Ok(());
        }
        Err(
            "此项目的金额预算尚不可执行：计数费用及服务/上下文报价覆盖尚未确定；未发送模型请求"
                .into(),
        )
    }
    #[cfg(windows)]
    fn count_basis(
        &mut self,
        intent: &DecisionIntent,
        returning: bool,
    ) -> Result<CountBasis, String> {
        intent.validate()?;
        self.management
            .require_intent_capacity(self.management_counts.len())?;
        require(
            intent.purpose == management::Purpose::Plan,
            "counted Host integration currently covers chief planning only",
        )?;
        self.planning_idle(&intent.project_id)?;
        crate::currency_count_guard::require_protected(&self.root, true)?;
        require(
            self.management
                .absent_prepared_intent(&intent.operation_id)?,
            "management operation already recorded; never resend",
        )?;
        let requirement = intent.context["requirement"]
            .as_str()
            .ok_or("management requirement missing")?;
        let current = self.match_original_catalog(
            &intent.context,
            self.management_context(&intent.project_id, requirement)?,
        )?;
        require(
            digest(&current)? == intent.state_digest,
            "management context changed during preflight",
        )?;
        let saved = self
            .project_setups
            .get(&intent.project_id)
            .ok_or("project setup missing")?;
        require(
            saved.setup.chief_model == intent.model && saved.setup.requirement == requirement,
            "counted planning must use the original configured requirement and chief",
        )?;
        let maximum_requests = saved.setup.management_max_requests as usize;
        let policy = saved
            .setup
            .policy
            .currency_budget
            .as_ref()
            .ok_or("planning money settings missing")?
            .bind_planning(&intent.project_id, &saved.digest, &intent.model)?;
        self.reconcile_currency_account(&intent.project_id)?;
        let history = self.currency_history_gaps();
        let view = self
            .currency_account_views(history.as_deref())
            .into_iter()
            .find(|v| v.project_id == intent.project_id)
            .ok_or("planning budget not authorized")?;
        require(
            view.available_usd_nanos.is_some(),
            "currency history or approval is unresolved",
        )?;
        let book = self
            .currency_accounts
            .get(&intent.project_id)
            .ok_or("planning account missing")?;
        let mut attempts: std::collections::BTreeSet<String> = self
            .management
            .statuses()
            .iter()
            .filter(|s| s.project_id == intent.project_id)
            .map(|s| s.operation_id.clone())
            .collect();
        attempts.extend(book.state().management_counts.keys().cloned());
        if returning {
            let original = book
                .state()
                .management_counts
                .get(&intent.operation_id)
                .ok_or("original durable count missing")?;
            require(
                original.attempt.intent == *intent
                    && matches!(
                        original.outcome,
                        crate::currency_budget::CountOutcome::Counted { .. }
                    ),
                "original durable count differs or is unresolved",
            )?;
            attempts.remove(&intent.operation_id);
        } else {
            require(
                !attempts.contains(&intent.operation_id),
                "count operation already accounted; never recount",
            )?;
        }
        require(
            attempts.len() < maximum_requests,
            "项目主副指挥请求上限已用完",
        )?;
        require(
            book.policy() == &policy,
            "original planning approval changed",
        )?;
        require(
            !book.state().bookings.contains_key(&intent.operation_id),
            "management operation already reserved; never resend",
        )?;
        let quotes: Vec<_> = policy
            .quotes
            .iter()
            .filter(|q| {
                q.target == api_worker::ApiTarget::OpenAi {} && q.requested_model == intent.model
            })
            .collect();
        require(
            quotes.len() == 1,
            "planning requires one unambiguous approved quote",
        )?;
        let quote = quotes[0];
        quote.valid_at(&chrono::Utc::now().to_rfc3339())?;
        let fee = quote
            .count_price
            .as_ref()
            .ok_or("explicit count fee ceiling required; unknown is not free")?
            .maximum_usd_nanos;
        // Refuse known insufficiency even before count. Exact input ceiling is
        // checked again by the ledger once the provider count is available.
        let minimum = quote
            .rates
            .reserve(&cerulean_domain::currency_budget::TokenCeiling {
                total_input: 1,
                output: 4096,
                cache_read_allowed: true,
                cache_write_allowed: true,
            })?;
        let required = minimum
            .checked_add(if returning { 0 } else { fee })
            .ok_or("count and generation fee overflow")?;
        require(
            required <= view.available_usd_nanos.unwrap(),
            "currency budget exhausted before counting",
        )?;
        Ok(CountBasis {
            authority: None,
            policy: digest(&policy)?,
            ledger_head: book.head().into(),
            quote: digest(quote)?,
        })
    }
    #[cfg(windows)]
    pub(super) fn start_counted_planning(&mut self, intent: DecisionIntent) -> Result<(), String> {
        self.require_management_planning_available(&intent.project_id)?;
        require(
            self.management_work_count() < 4,
            "management concurrency limit reached",
        )?;
        require(
            !self.management_counts.contains_key(&intent.operation_id)
                && !self.management_active.contains_key(&intent.operation_id),
            "management operation still active",
        )?;
        require(
            self.openai_connection.key_configured && self.openai_probe.is_none(),
            "OpenAI authentication unavailable or being checked",
        )?;
        let mut basis = self.count_basis(&intent, false)?;
        let prepared = PreparedDecision::new(
            intent.purpose,
            &intent.model,
            &intent.state_digest,
            &intent.context,
        )?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "management counting runtime unavailable")?;
        let count: Box<dyn FnOnce(PreparedDecision) -> Result<CountCompletion, String> + Send>;
        #[cfg(test)]
        if let Some(fixture) = &self.management_count_fixture {
            let fixture = fixture.count.clone();
            count = Box::new(move |prepared| {
                Ok(CountCompletion {
                    runtime,
                    counted: CountProof::Legacy(Box::new(fixture(prepared)?)),
                })
            });
        } else {
            let client = cerulean_adapters::openai::OpenAiClient::new(
                crate::credential_vault::load(&self.root)?,
            )?;
            count = Box::new(move |prepared| {
                let counted = runtime.block_on(client.count_decision(prepared))?;
                Ok(CountCompletion {
                    runtime,
                    counted: CountProof::Legacy(Box::new(counted)),
                })
            });
        }
        #[cfg(not(test))]
        {
            let client = cerulean_adapters::openai::OpenAiClient::new(
                crate::credential_vault::load(&self.root)?,
            )?;
            count = Box::new(move |prepared| {
                let counted = runtime.block_on(client.count_decision(prepared))?;
                Ok(CountCompletion {
                    runtime,
                    counted: CountProof::Legacy(Box::new(counted)),
                })
            });
        }
        let book = self
            .currency_accounts
            .get_mut(&intent.project_id)
            .ok_or("planning account missing")?;
        let quote = book.policy().quote(&basis.quote)?.clone();
        book.begin_management_count(&intent, &prepared, &basis.quote)?;
        basis.ledger_head = book.head().into();
        let (sender, receiver) = std::sync::mpsc::channel();
        let fail_spawn = false;
        #[cfg(test)]
        let fail_spawn = fail_spawn
            || self
                .management_count_fixture
                .as_ref()
                .is_some_and(|f| f.fail_spawn);
        let spawned = if fail_spawn {
            Err(std::io::Error::other("synthetic count spawn failure"))
        } else {
            std::thread::Builder::new()
                .name("openai-management-count".into())
                .spawn(move || {
                    let result = quote
                        .valid_at(&chrono::Utc::now().to_rfc3339())
                        .and_then(|()| count(prepared));
                    let _ = sender.send(result);
                })
        };
        if spawned.is_err() {
            let error = self.fail_count_attempt(
                &intent,
                "management counting worker not started; no retry".into(),
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
                scoped: false,
                receiver,
            },
        );
        Ok(())
    }
    #[cfg(windows)]
    fn finish_counted_planning(
        &mut self,
        mut pending: PendingCount,
        completion: CountCompletion,
    ) -> Result<(), String> {
        let CountCompletion { runtime, counted } = completion;
        let CountProof::Legacy(counted) = counted else {
            return Err("legacy count received scoped proof".into());
        };
        // Record the original receipt even if scope changed. Only our own
        // publication may advance the expected head, never unrelated writes.
        let book = self
            .currency_accounts
            .get_mut(&pending.intent.project_id)
            .ok_or("planning account missing")?;
        let unchanged = book.head() == pending.basis.ledger_head;
        book.record_management_count(&pending.intent.operation_id, &counted)?;
        pending.basis.ledger_head = book.head().into();
        require(unchanged, "currency ledger changed during preflight")?;
        require(
            self.count_basis(&pending.intent, true)? == pending.basis,
            "currency approval, quote or ledger changed during preflight",
        )?;
        counted.ceiling()?;
        let fingerprint = counted.prepared().fingerprint().clone();
        let quote = self.currency_accounts[&pending.intent.project_id]
            .policy()
            .quote(&pending.basis.quote)?
            .clone();
        self.currency_accounts
            .get_mut(&pending.intent.project_id)
            .ok_or("planning account missing")?
            .prepare_counted_management(&pending.intent, &counted, &pending.basis.quote)?;
        #[cfg(test)]
        let fixture = self
            .management_count_fixture
            .as_ref()
            .map(|f| f.send.clone());
        self.launch_management_evidenced(pending.intent, Some(fingerprint), move |intent| {
            quote.valid_at(&chrono::Utc::now().to_rfc3339())?;
            counted.ceiling()?;
            #[cfg(test)]
            if let Some(send) = fixture {
                return send(intent);
            }
            let _ = intent;
            runtime.block_on(counted.decide())
        })
    }
    #[cfg(windows)]
    pub(super) fn fail_count_attempt(&mut self, intent: &DecisionIntent, error: String) -> String {
        let result = (|| {
            let book = self
                .currency_accounts
                .get_mut(&intent.project_id)
                .ok_or("planning account missing")?;
            if book
                .state()
                .management_counts
                .get(&intent.operation_id)
                .is_some_and(|b| matches!(b.outcome, crate::currency_budget::CountOutcome::Pending))
            {
                book.mark_management_count_unknown(
                    &intent.operation_id,
                    &digest(&("count-host-failure-v1", &intent.operation_id, &error))?,
                )?;
            }
            Ok::<(), String>(())
        })();
        match result {
            Ok(()) => error,
            Err(e) => format!("{error}; count accounting: {e}"),
        }
    }
    #[cfg(windows)]
    pub(super) fn poll_management_counts(&mut self) {
        let ready: Vec<_> = self
            .management_counts
            .iter()
            .filter_map(|(id, pending)| match pending.receiver.try_recv() {
                Ok(outcome) => Some((id.clone(), outcome)),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Some((
                    id.clone(),
                    Err("management count worker disconnected; no retry".into()),
                )),
            })
            .collect();
        for (id, outcome) in ready {
            let pending = self.management_counts.remove(&id).expect("completed count");
            let project = pending.intent.project_id.clone();
            let intent = pending.intent.clone();
            if let Err(error) = outcome.and_then(|done| {
                if pending.scoped {
                    self.finish_scoped_count(pending, done)
                } else {
                    self.finish_counted_planning(pending, done)
                }
            }) {
                let error = self.fail_count_attempt(&intent, error);
                // Preserve any fully-published preparation through the existing
                // recovery path, including original-intent publication failures.
                let recovery = self.reconcile_currency_account(&project);
                let error = match recovery {
                    Ok(()) => error,
                    Err(e) => format!("{error}; accounting: {e}"),
                };
                self.management_count_errors.insert(project, error);
            }
        }
    }
}
