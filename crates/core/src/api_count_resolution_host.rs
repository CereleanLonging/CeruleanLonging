//! Original user workflow confirmation precedes exact API count-fee publication.
use super::*;
use crate::api_count_resolution::{ApiCountResolution, ApiCountResolutionReview, APPROVAL_SOURCE};
use cerulean_domain::api_count_resolution::ApiCountResolutionInput;

impl RuntimeHost {
    fn validate_api_count_source(&self, r: &ApiCountResolutionReview) -> Result<(), String> {
        let book = self
            .currency_accounts
            .get(&r.input.project_id)
            .ok_or("API count account missing")?;
        require(book.healthy(), "currency ledger requires reopen")?;
        let count = book
            .state()
            .api_counts
            .get(&r.input.request_id)
            .ok_or("original API count missing")?;
        require(
            count.source_digest()? == r.source_digest
                && count.original_outcome() == r.original_outcome
                && count.attempt.maximum_usd_nanos == r.maximum_usd_nanos
                && book
                    .state()
                    .api_count_generation_digest(&r.input.request_id)?
                    == r.generation_digest,
            "original API count or generation source changed; preserve confirmation",
        )
    }
    pub(super) fn api_count_resolution_confirmations(
        &self,
    ) -> Result<BTreeMap<String, ApiCountResolution>, String> {
        let mut confirmed = BTreeMap::new();
        for catalogue in self.currency_catalogues()?.into_values() {
            for (review, event) in catalogue.api_count_resolutions {
                self.validate_api_count_source(&review)?;
                let key = digest(&(&review.input.project_id, &review.input.request_id))?;
                require(
                    confirmed.insert(key, review.resolution(&event)).is_none(),
                    "duplicate API count confirmation",
                )?;
            }
        }
        for (project, book) in &self.currency_accounts {
            for (request, count) in &book.state().api_counts {
                if let Some(saved) = &count.fee_resolution {
                    require(
                        confirmed.get(&digest(&(project, request))?) == Some(saved.as_ref()),
                        "API count fee has no exact original user confirmation",
                    )?;
                }
            }
        }
        let mut missing = 0;
        for saved in confirmed.values() {
            let book = &self.currency_accounts[&saved.review.input.project_id];
            book.check_api_count_resolution(saved)?;
            if book.state().api_counts[&saved.review.input.request_id]
                .fee_resolution
                .is_none()
            {
                missing += 1;
            }
        }
        require(
            missing <= 1,
            "multiple API count publications missing; preserve user events",
        )?;
        Ok(confirmed)
    }
    pub fn review_api_count_resolution(
        &self,
        input: &ApiCountResolutionInput,
    ) -> Result<ApiCountResolutionReview, String> {
        input.validate()?;
        crate::currency_api_resolution_guard::require_protected(&self.root, true)?;
        let confirmed = self.api_count_resolution_confirmations()?;
        require(
            confirmed.values().all(|s| {
                self.currency_accounts[&s.review.input.project_id]
                    .state()
                    .api_counts[&s.review.input.request_id]
                    .fee_resolution
                    .is_some()
            }),
            "complete original API fee publication before new review",
        )?;
        #[cfg(windows)]
        require(
            !self.api_active.keys().any(|(p, _)| p == &input.project_id),
            "wait for original API worker to stop before fee review",
        )?;
        let store = self
            .projects
            .get(&input.project_id)
            .ok_or("wait for original workflow owner before fee review")?;
        let book = self
            .currency_accounts
            .get(&input.project_id)
            .ok_or("original API account missing")?;
        crate::api_execution::CallInventory::read_with_account(store, Some(book))?
            .check(Some(book))?;
        let count = book
            .state()
            .api_counts
            .get(&input.request_id)
            .ok_or("original API count missing")?;
        require(
            count.fee_resolution.is_none(),
            "API count already confirmed; read original event",
        )?;
        let r = ApiCountResolutionReview {
            input: input.clone(),
            source_digest: count.source_digest()?,
            policy_digest: digest(book.policy())?,
            previous_ledger_head: book.head().into(),
            maximum_usd_nanos: count.attempt.maximum_usd_nanos,
            original_outcome: count.original_outcome().into(),
            generation_digest: book
                .state()
                .api_count_generation_digest(&input.request_id)?,
        };
        r.validate()?;
        book.check_api_count_resolution(&r.resolution(&"0".repeat(64)))?;
        Ok(r)
    }
    pub fn confirm_api_count_resolution(
        &mut self,
        review: &ApiCountResolutionReview,
        expected: &str,
    ) -> Result<(), String> {
        review.validate_for(&review.input.project_id, expected)?;
        crate::currency_api_resolution_guard::require_protected(&self.root, true)?;
        self.reconcile_currency_account(&review.input.project_id)?;
        let key = digest(&(&review.input.project_id, &review.input.request_id))?;
        if let Some(saved) = self.api_count_resolution_confirmations()?.get(&key) {
            return require(
                *saved == review.resolution(&saved.user_event_digest),
                "API count confirmed with another review",
            );
        }
        require(
            self.review_api_count_resolution(&review.input)? == *review,
            "API count review stale; review original account again",
        )?;
        self.projects
            .get_mut(&review.input.project_id)
            .ok_or("original API workflow unavailable")?
            .command(
                &format!("user-api-count-{expected}"),
                "user",
                WorkflowCommand::ConfirmApiCountResolution {
                    review_digest: expected.into(),
                },
                CommandContext {
                    api_count_resolution: Some(review.clone()),
                    approval_source: Some(APPROVAL_SOURCE.into()),
                    ..Default::default()
                },
                false,
            )?;
        self.reconcile_currency_account(&review.input.project_id)
    }
}
