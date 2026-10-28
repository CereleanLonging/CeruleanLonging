//! Original user workflow confirmation precedes exact API generation-fee publication.
use super::*;
use crate::api_generation_resolution::{
    ApiGenerationResolution, ApiGenerationResolutionReview, APPROVAL_SOURCE,
};
use cerulean_domain::api_generation_resolution::ApiGenerationResolutionInput;

impl RuntimeHost {
    fn validate_api_generation_source(
        &self,
        r: &ApiGenerationResolutionReview,
    ) -> Result<(), String> {
        let book = self
            .currency_accounts
            .get(&r.input.project_id)
            .ok_or("API generation account missing")?;
        require(book.healthy(), "currency ledger requires reopen")?;
        let count = book
            .state()
            .bookings
            .get(&r.input.request_id)
            .ok_or("original API generation missing")?;
        require(
            count.generation_source_digest()? == r.source_digest
                && count.generation_outcome() == r.original_outcome
                && count.reserved_usd_nanos == r.maximum_usd_nanos
                && book
                    .state()
                    .api_generation_count_digest(&r.input.request_id)?
                    == r.count_digest,
            "original API generation or generation source changed; preserve confirmation",
        )
    }
    pub(super) fn api_generation_resolution_confirmations(
        &self,
    ) -> Result<BTreeMap<String, ApiGenerationResolution>, String> {
        let mut confirmed = BTreeMap::new();
        for catalogue in self.currency_catalogues()?.into_values() {
            for (review, event) in catalogue.api_generation_resolutions {
                self.validate_api_generation_source(&review)?;
                let key = digest(&(&review.input.project_id, &review.input.request_id))?;
                require(
                    confirmed.insert(key, review.resolution(&event)).is_none(),
                    "duplicate API generation confirmation",
                )?;
            }
        }
        for (project, book) in &self.currency_accounts {
            for (request, count) in &book.state().bookings {
                if let Some(saved) = &count.fee_resolution {
                    require(
                        confirmed.get(&digest(&(project, request))?) == Some(saved.as_ref()),
                        "API generation fee has no exact original user confirmation",
                    )?;
                }
            }
        }
        let mut missing = 0;
        for saved in confirmed.values() {
            let book = &self.currency_accounts[&saved.review.input.project_id];
            book.check_api_generation_resolution(saved)?;
            if book.state().bookings[&saved.review.input.request_id]
                .fee_resolution
                .is_none()
            {
                missing += 1;
            }
        }
        require(
            missing <= 1,
            "multiple API generation publications missing; preserve user events",
        )?;
        Ok(confirmed)
    }
    pub fn review_api_generation_resolution(
        &self,
        input: &ApiGenerationResolutionInput,
    ) -> Result<ApiGenerationResolutionReview, String> {
        input.validate()?;
        crate::currency_api_resolution_guard::require_protected(&self.root, true)?;
        let confirmed = self.api_generation_resolution_confirmations()?;
        require(
            confirmed.values().all(|s| {
                self.currency_accounts[&s.review.input.project_id]
                    .state()
                    .bookings[&s.review.input.request_id]
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
            .bookings
            .get(&input.request_id)
            .ok_or("original API generation missing")?;
        require(
            count.fee_resolution.is_none(),
            "API generation already confirmed; read original event",
        )?;
        let r = ApiGenerationResolutionReview {
            input: input.clone(),
            source_digest: count.generation_source_digest()?,
            policy_digest: digest(book.policy())?,
            previous_ledger_head: book.head().into(),
            maximum_usd_nanos: count.reserved_usd_nanos,
            original_outcome: count.generation_outcome().into(),
            count_digest: book
                .state()
                .api_generation_count_digest(&input.request_id)?,
        };
        r.validate()?;
        book.check_api_generation_resolution(&r.resolution(&"0".repeat(64)))?;
        Ok(r)
    }
    pub fn confirm_api_generation_resolution(
        &mut self,
        review: &ApiGenerationResolutionReview,
        expected: &str,
    ) -> Result<(), String> {
        review.validate_for(&review.input.project_id, expected)?;
        crate::currency_api_resolution_guard::require_protected(&self.root, true)?;
        self.reconcile_currency_account(&review.input.project_id)?;
        let key = digest(&(&review.input.project_id, &review.input.request_id))?;
        if let Some(saved) = self.api_generation_resolution_confirmations()?.get(&key) {
            return require(
                *saved == review.resolution(&saved.user_event_digest),
                "API generation confirmed with another review",
            );
        }
        require(
            self.review_api_generation_resolution(&review.input)? == *review,
            "API generation review stale; review original account again",
        )?;
        self.projects
            .get_mut(&review.input.project_id)
            .ok_or("original API workflow unavailable")?
            .command(
                &format!("user-api-generation-{expected}"),
                "user",
                WorkflowCommand::ConfirmApiGenerationResolution {
                    review_digest: expected.into(),
                },
                CommandContext {
                    api_generation_resolution: Some(review.clone()),
                    approval_source: Some(APPROVAL_SOURCE.into()),
                    ..Default::default()
                },
                false,
            )?;
        self.reconcile_currency_account(&review.input.project_id)
    }
}
