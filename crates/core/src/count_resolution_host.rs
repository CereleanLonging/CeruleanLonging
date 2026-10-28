//! Authenticated review, original workflow event, then exact ledger publication.
use super::*;
use crate::count_resolution::{CountResolutionReview, ManagementCountResolution, APPROVAL_SOURCE};
use cerulean_domain::count_resolution::{CountResolutionAction, CountResolutionInput};

impl RuntimeHost {
    fn count_generation_digest(&self, project: &str, operation: &str) -> Result<String, String> {
        self.management.require_billing_published()?;
        let book = self
            .currency_accounts
            .get(project)
            .ok_or("original count account missing")?;
        let generation = book.state().bookings.get(operation).map(|b| {
            (
                &b.policy_digest,
                &b.reservation,
                &b.management_identity,
                &b.management_preparation,
            )
        });
        let original = if self.management.absent_prepared_intent(operation)? {
            None
        } else {
            let evidence = self.management.billing_evidence(operation)?;
            Some((
                evidence.intent,
                evidence.request,
                evidence.transport,
                evidence.observation,
                evidence.head,
            ))
        };
        let legacy = digest(&("count-generation-original-v1", generation, original))?;
        let preparation = book
            .state()
            .bookings
            .get(operation)
            .and_then(|b| b.scoped_management_preparation.as_ref());
        let scoped = if self.management.is_scoped(operation) {
            let evidence = self.management.billing_evidence(operation)?;
            Some((evidence.scoped_request, evidence.scoped_response))
        } else {
            None
        };
        if preparation.is_some() || scoped.is_some() {
            digest(&("count-generation-scoped-v1", legacy, preparation, scoped))
        } else {
            Ok(legacy)
        }
    }
    fn validate_count_resolution_source(&self, r: &CountResolutionReview) -> Result<(), String> {
        let book = self
            .currency_accounts
            .get(&r.input.project_id)
            .ok_or("original count account missing")?;
        require(book.healthy(), "currency ledger requires reopen")?;
        let count = book
            .state()
            .management_counts
            .get(&r.input.operation_id)
            .ok_or("original count missing")?;
        require(
            count.source_digest()? == r.source_digest
                && count.original_outcome() == r.original_outcome
                && count.attempt.maximum_usd_nanos == r.maximum_usd_nanos
                && self.count_generation_digest(&r.input.project_id, &r.input.operation_id)?
                    == r.generation_digest,
            "original count or generation source changed; preserve resolution",
        )
    }
    pub(super) fn count_resolution_confirmations(
        &self,
    ) -> Result<BTreeMap<String, ManagementCountResolution>, String> {
        let mut confirmed = BTreeMap::new();
        for catalogue in self.currency_catalogues()?.into_values() {
            for (review, event) in catalogue.count_resolutions {
                self.validate_count_resolution_source(&review)?;
                require(
                    confirmed
                        .insert(review.key()?, review.resolution(&event))
                        .is_none(),
                    "duplicate count resolution",
                )?;
            }
        }
        for book in self.currency_accounts.values() {
            for count in book.state().management_counts.values() {
                for saved in [count.fee_resolution.as_deref(), count.retirement.as_deref()]
                    .into_iter()
                    .flatten()
                {
                    require(
                        confirmed.get(&saved.review.key()?) == Some(saved),
                        "count resolution has no exact original user confirmation",
                    )?;
                }
            }
        }
        let mut missing = 0;
        for resolution in confirmed.values() {
            let r = &resolution.review;
            let book = &self.currency_accounts[&r.input.project_id];
            book.check_management_count_resolution(resolution)?;
            if book.state().management_counts[&r.input.operation_id]
                .resolution(&r.input.action)
                .is_none()
            {
                missing += 1;
            }
        }
        require(
            missing <= 1,
            "multiple count publications missing; preserve original events",
        )?;
        Ok(confirmed)
    }
    pub fn review_management_count_resolution(
        &self,
        input: &CountResolutionInput,
    ) -> Result<CountResolutionReview, String> {
        input.validate()?;
        crate::currency_count_resolution_guard::require_protected(&self.root, true)?;
        let confirmed = self.count_resolution_confirmations()?;
        require(
            confirmed.values().all(|s| {
                self.currency_accounts[&s.review.input.project_id]
                    .state()
                    .management_counts[&s.review.input.operation_id]
                    .resolution(&s.review.input.action)
                    .is_some()
            }),
            "complete original count publication before new review",
        )?;
        require(
            !self.counting_project(&input.project_id)
                && !self.management_active.keys().any(|id| {
                    self.management
                        .intent(id)
                        .is_ok_and(|a| a.project_id == input.project_id)
                }),
            "wait for original count/generation worker to stop before review",
        )?;
        let book = self
            .currency_accounts
            .get(&input.project_id)
            .ok_or("original count account missing")?;
        let count = book
            .state()
            .management_counts
            .get(&input.operation_id)
            .ok_or("original count missing")?;
        require(
            count.resolution(&input.action).is_none(),
            "count aspect already resolved; read original confirmation",
        )?;
        if matches!(
            input.action,
            CountResolutionAction::RetireContinuation { .. }
        ) {
            if book.state().bookings.contains_key(&input.operation_id) {
                require(
                    self.management
                        .statuses()
                        .iter()
                        .any(|s| s.operation_id == input.operation_id && s.state.resolved()),
                    "resolve original generation responsibility before retirement",
                )?;
            } else {
                require(
                    self.management
                        .absent_prepared_intent(&input.operation_id)?,
                    "generation intent without reservation requires original reconciliation",
                )?;
            }
        }
        let review = CountResolutionReview {
            input: input.clone(),
            source_digest: count.source_digest()?,
            policy_digest: digest(book.policy())?,
            previous_ledger_head: book.head().into(),
            maximum_usd_nanos: count.attempt.maximum_usd_nanos,
            original_outcome: count.original_outcome().into(),
            generation_digest: self
                .count_generation_digest(&input.project_id, &input.operation_id)?,
        };
        review.validate(&input.project_id, &review.digest()?)?;
        book.check_management_count_resolution(&review.resolution(&"0".repeat(64)))?;
        Ok(review)
    }
    pub fn confirm_management_count_resolution(
        &mut self,
        review: &CountResolutionReview,
        expected: &str,
    ) -> Result<(), String> {
        review.validate(&review.input.project_id, expected)?;
        crate::currency_count_resolution_guard::require_protected(&self.root, true)?;
        self.reconcile_currency_account(&review.input.project_id)?;
        if let Some(saved) = self.count_resolution_confirmations()?.get(&review.key()?) {
            return require(
                *saved == review.resolution(&saved.user_event_digest),
                "count aspect confirmed with a different review",
            );
        }
        require(
            self.review_management_count_resolution(&review.input)? == *review,
            "count review stale; review current original account again",
        )?;
        self.projects
            .get_mut(&review.input.project_id)
            .ok_or("count project missing")?
            .command(
                &format!("user-count-{}", expected),
                "user",
                WorkflowCommand::ConfirmManagementCountResolution {
                    review_digest: expected.into(),
                },
                CommandContext {
                    count_resolution: Some(review.clone()),
                    approval_source: Some(APPROVAL_SOURCE.into()),
                    ..Default::default()
                },
                false,
            )?;
        self.reconcile_currency_account(&review.input.project_id)?;
        self.refresh_management_count_attention(&review.input.project_id);
        Ok(())
    }
    pub(super) fn refresh_management_count_attention(&mut self, project: &str) {
        let Some(book) = self.currency_accounts.get(project) else {
            return;
        };
        let unresolved = book.state().management_counts.iter().any(|(id, c)| {
            c.retirement.is_none()
                && (!matches!(
                    c.outcome,
                    crate::currency_budget::CountOutcome::Counted { .. }
                        | crate::currency_budget::CountOutcome::ScopedCounted { .. }
                ) || !book.state().bookings.contains_key(id)
                    || self.management.intent(id).is_err())
        });
        if unresolved {
            self.management_count_errors.insert(
                project.into(),
                "持久计数尚未完成生成交接；请审阅原计数并明确终止续发，费用需单独核对".into(),
            );
        } else if !book.state().management_counts.is_empty() {
            self.management_count_errors.remove(project);
        }
    }
}
