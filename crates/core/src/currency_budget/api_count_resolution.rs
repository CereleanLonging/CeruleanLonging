//! Release only the original count liability, retaining original API evidence.
use super::*;
use crate::api_count_resolution::ApiCountResolution;

pub(super) fn require_guard(root: &Path) -> Result<(), String> {
    let data = root
        .parent()
        .filter(|p| p.file_name().is_some_and(|n| n == "currency-budgets"))
        .and_then(Path::parent)
        .ok_or("API resolution requires original guarded account")?;
    crate::currency_api_resolution_guard::require_protected(data, true)
}
impl ApiCountBooking {
    pub fn source_digest(&self) -> Result<String, String> {
        digest(&("original-api-count-v1", &self.attempt, &self.outcome))
    }
    pub fn original_outcome(&self) -> &'static str {
        match self.outcome {
            ApiCountOutcome::Pending => "pending",
            ApiCountOutcome::Counted { .. } => "counted",
            ApiCountOutcome::Unknown { .. } => "unknown",
        }
    }
}
impl BudgetState {
    /// Excludes later billing outcomes; original preparation can never be replaced.
    pub fn api_count_generation_digest(&self, request: &str) -> Result<String, String> {
        let source = self.bookings.get(request).map(|b| {
            (
                &b.policy_digest,
                &b.api_identity,
                &b.reservation,
                &b.scoped_api_preparation,
            )
        });
        digest(&("api-count-original-generation-v1", source))
    }
    pub(super) fn resolve_api_count(
        &mut self,
        policy: &BudgetPolicy,
        resolution: &ApiCountResolution,
        head: &str,
    ) -> Result<(), String> {
        resolution.validate()?;
        let r = &resolution.review;
        require(
            r.input.project_id == policy.project_id
                && r.policy_digest == digest(policy)?
                && r.policy_digest == self.current_policy_digest
                && r.previous_ledger_head == head
                && self.approval_links.contains_key(&r.policy_digest),
            "API count resolution has no exact current approved account/head",
        )?;
        require(
            self.api_count_generation_digest(&r.input.request_id)? == r.generation_digest,
            "original API generation preparation changed",
        )?;
        let count = self
            .api_counts
            .get_mut(&r.input.request_id)
            .ok_or("original API count missing")?;
        require(
            count.fee_resolution.is_none(),
            "API count already reconciled",
        )?;
        require(
            count.source_digest()? == r.source_digest
                && count.original_outcome() == r.original_outcome
                && count.attempt.maximum_usd_nanos == r.maximum_usd_nanos,
            "API count resolution differs from original request/receipt",
        )?;
        let reserved = self
            .reserved_usd_nanos
            .checked_sub(count.attempt.maximum_usd_nanos)
            .ok_or("API count liability underflow")?;
        let charged = self
            .charged_usd_nanos
            .checked_add(r.input.amount_usd_nanos)
            .filter(|n| *n <= MAX_USD_NANOS)
            .ok_or("API count charge exceeds exact range")?;
        require(
            charged
                .checked_add(reserved)
                .is_some_and(|n| n <= MAX_USD_NANOS),
            "API count charge plus reservations exceeds exact range",
        )?;
        // Actual user-confirmed fees may exceed the quoted maximum. Preserve the
        // real amount; existing available-balance checks refuse further spending.
        self.reserved_usd_nanos = reserved;
        self.charged_usd_nanos = charged;
        count.fee_resolution = Some(Box::new(resolution.clone()));
        Ok(())
    }
}
impl BudgetLedger {
    /// Trusted Host callers must authenticate the exact original user event and
    /// verify that the API worker has stopped before publishing this record.
    pub fn check_api_count_resolution(
        &self,
        resolution: &ApiCountResolution,
    ) -> Result<(), String> {
        require(self.healthy(), "currency ledger requires reopen")?;
        require_guard(&self.root)?;
        resolution.validate()?;
        let count = self
            .state
            .api_counts
            .get(&resolution.review.input.request_id)
            .ok_or("original API count missing")?;
        if let Some(saved) = &count.fee_resolution {
            return require(
                saved.as_ref() == resolution,
                "API fee confirmation changed after publication",
            );
        }
        require(
            self.count < MAX_RECORDS,
            "currency ledger cannot hold an API confirmation",
        )?;
        self.state
            .clone()
            .resolve_api_count(&self.policy, resolution, &self.head)?;
        let at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, false);
        encode_record(
            self.count,
            &self.head,
            &self.policy_digest,
            at,
            Event::ResolveApiCount {
                resolution: Box::new(resolution.clone()),
            },
        )?;
        Ok(())
    }
    /// Idempotent exact publication; never creates a count or generation permit.
    pub fn record_api_count_resolution(
        &mut self,
        resolution: ApiCountResolution,
    ) -> Result<(), String> {
        self.check_api_count_resolution(&resolution)?;
        if self.state.api_counts[&resolution.review.input.request_id]
            .fee_resolution
            .is_some()
        {
            return Ok(());
        }
        self.publish(Event::ResolveApiCount {
            resolution: Box::new(resolution),
        })
    }
}
