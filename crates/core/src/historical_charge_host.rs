//! Same-owner original user confirmation, global source uniqueness and exact recovery.
use super::*;
use crate::{
    currency_budget::{HistoricalCharge, HistoricalScope},
    historical_confirmation::{HistoricalReview, APPROVAL_SOURCE},
};

impl RuntimeHost {
    fn require_unreserved_historical_scope(&self, scope: &HistoricalScope) -> Result<(), String> {
        for (project, book) in &self.currency_accounts {
            let id = match scope {
                HistoricalScope::Management { operation_id, .. } => operation_id,
                HistoricalScope::Api {
                    project_id,
                    attempt_id,
                } if project_id == project => attempt_id,
                HistoricalScope::Api { .. } => continue,
            };
            require(
                !book
                    .state()
                    .bookings
                    .values()
                    .any(|b| &b.reservation.operation_id == id),
                "historical source has an original reservation; preserve its original account",
            )?;
        }
        Ok(())
    }
    pub(super) fn historical_confirmations(
        &self,
        published: bool,
    ) -> Result<BTreeMap<String, HistoricalCharge>, String> {
        let mut original = Vec::new();
        for catalogue in self.currency_catalogues()?.into_values() {
            original.extend(catalogue.historical);
        }
        let mut management_ids = Vec::new();
        for (review, _) in &original {
            self.require_unreserved_historical_scope(&review.source.scope)?;
            if let HistoricalScope::Management { operation_id, .. } = &review.source.scope {
                require(
                    !self.management_active.contains_key(operation_id),
                    "original management request is still active",
                )?;
                management_ids.push(operation_id.as_str());
            }
        }
        let management = self
            .management
            .historical_sources(&management_ids, published)?;
        let mut confirmed = BTreeMap::new();
        for (review, event) in original {
            let source = match &review.source.scope {
                HistoricalScope::Management { operation_id, .. } => management
                    .get(operation_id)
                    .ok_or("original historical management source missing")?
                    .clone(),
                HistoricalScope::Api { .. } => {
                    self.historical_source_inner(&review.source.scope, published)?
                }
            };
            require(
                source == review.source,
                "original historical source changed; preserve confirmation and account",
            )?;
            let key = source.scope.key()?;
            require(
                confirmed.insert(key, review.charge(&event)).is_none(),
                "original historical scope confirmed in more than one project",
            )?;
        }
        let mut missing = 0;
        for book in self.currency_accounts.values() {
            for (key, charge) in &book.state().historical_charges {
                require(confirmed.get(key) == Some(charge),
                    "historical charges require original user-event reconciliation; preserve the account")?;
            }
        }
        for (key, charge) in &confirmed {
            let book = self
                .currency_accounts
                .get(&charge.project_id)
                .ok_or("confirmed historical account missing; do not recreate")?;
            book.check_historical_charge(charge)?;
            if !book.state().historical_charges.contains_key(key) {
                missing += 1;
            }
        }
        require(
            missing <= 1,
            "multiple original historical publications are missing; preserve evidence",
        )?;
        Ok(confirmed)
    }

    /// Trusted local-user service preparation. Merely reading this does not confirm or charge.
    pub fn review_historical_charge(
        &self,
        project: &str,
        scope: &HistoricalScope,
        amount_usd_nanos: u64,
        billing_reference: &str,
        explanation: &str,
    ) -> Result<HistoricalReview, String> {
        crate::currency_guard::require_protected(&self.root, true)?;
        let confirmed = self.historical_confirmations(true)?;
        require(
            !confirmed.contains_key(&scope.key()?),
            "historical scope already confirmed",
        )?;
        require(
            confirmed.iter().all(|(k, c)| {
                self.currency_accounts[&c.project_id]
                    .state()
                    .historical_charges
                    .contains_key(k)
            }),
            "complete original historical publication before new review",
        )?;
        let source = self.historical_source_for_review(scope)?;
        self.require_unreserved_historical_scope(scope)?;
        let book = self
            .currency_accounts
            .get(project)
            .ok_or("approve the target project budget before historical review")?;
        let review = HistoricalReview {
            schema: 1,
            project_id: project.into(),
            source,
            policy_digest: digest(book.policy())?,
            previous_ledger_head: book.head().into(),
            amount_usd_nanos,
            billing_reference: billing_reference.into(),
            explanation: explanation.into(),
        };
        review.validate(project, &review.digest()?)?;
        book.check_historical_charge(&review.charge(&"0".repeat(64)))?;
        Ok(review)
    }

    /// Trusted local-user service only. UI/IPC must show the full packet and send its exact digest.
    pub fn confirm_historical_charge(
        &mut self,
        review: &HistoricalReview,
        expected: &str,
    ) -> Result<(), String> {
        review.validate(&review.project_id, expected)?;
        crate::currency_guard::require_protected(&self.root, true)?;
        self.reconcile_currency_account(&review.project_id)?;
        let confirmed = self.historical_confirmations(true)?;
        if let Some(saved) = confirmed.get(&review.source.scope.key()?) {
            return require(
                *saved == review.charge(&saved.user_event_digest)
                    && self
                        .currency_workflow_entries()?
                        .get(&saved.project_id)
                        .ok_or("historical project missing")?
                        .iter()
                        .any(|e| {
                            e.hash == saved.user_event_digest
                                && e.body.context.historical_charge.as_ref() == Some(review)
                        }),
                "historical scope already confirmed with different review",
            );
        }
        let fresh = self.review_historical_charge(
            &review.project_id,
            &review.source.scope,
            review.amount_usd_nanos,
            &review.billing_reference,
            &review.explanation,
        )?;
        require(
            fresh == *review,
            "historical review changed; review the original source and account again",
        )?;
        let store = self
            .projects
            .get_mut(&review.project_id)
            .ok_or("historical project missing")?;
        store.command(
            &format!("user-history-{}", expected),
            "user",
            WorkflowCommand::ConfirmHistoricalCharge {
                review_digest: expected.into(),
            },
            CommandContext {
                historical_charge: Some(review.clone()),
                approval_source: Some(APPROVAL_SOURCE.into()),
                ..Default::default()
            },
            false,
        )?;
        self.reconcile_currency_account(&review.project_id)
    }
}
