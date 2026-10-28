//! User billing resolves money only; original unknown/unpriced evidence stays.
use super::*;
use crate::api_generation_resolution::ApiGenerationResolution;

impl Booking {
    pub fn generation_source_digest(&self) -> Result<String, String> {
        digest(&(
            "original-api-generation-v1",
            &self.policy_digest,
            &self.api_identity,
            &self.management_identity,
            &self.management_preparation,
            &self.scoped_management_preparation,
            &self.scoped_api_preparation,
            &self.reservation,
            self.reserved_usd_nanos,
            &self.outcome,
        ))
    }
    pub fn generation_outcome(&self) -> &'static str {
        match self.outcome {
            Outcome::Reserved => "reserved",
            Outcome::Unknown { .. } => "unknown",
            Outcome::Unpriced { .. } => "unpriced",
            Outcome::Settled { .. } => "settled",
            Outcome::NotSent { .. } => "notSent",
        }
    }
}
impl BudgetState {
    /// Count billing is independent; its later reconciliation is not a new source.
    pub fn api_generation_count_digest(&self, request: &str) -> Result<String, String> {
        self.api_counts
            .get(request)
            .ok_or("original API count missing")?
            .source_digest()
    }
    pub(super) fn resolve_api_generation(
        &mut self,
        policy: &BudgetPolicy,
        resolution: &ApiGenerationResolution,
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
            "API generation resolution has no exact current approved account/head",
        )?;
        require(
            self.api_generation_count_digest(&r.input.request_id)? == r.count_digest,
            "original API count source changed",
        )?;
        let booking = self
            .bookings
            .get(&r.input.request_id)
            .ok_or("original API generation missing")?;
        let prep = booking
            .scoped_api_preparation
            .as_ref()
            .ok_or("original scoped API preparation missing")?;
        let original_policy = self
            .policies
            .get(&booking.policy_digest)
            .ok_or("original API policy missing")?;
        prep.validate(original_policy)?;
        require(
            booking.api_identity.as_ref() == Some(&prep.attempt.call)
                && prep.attempt.call.project_id == r.input.project_id
                && prep.attempt.call.request_id == r.input.request_id
                && booking.policy_digest == prep.attempt.policy_digest
                && booking.reservation == prep.reservation
                && booking.reserved_usd_nanos == prep.audit.maximum_usd_nanos
                && booking.management_identity.is_none()
                && booking.management_preparation.is_none()
                && booking.scoped_management_preparation.is_none()
                && self.approval_links.contains_key(&booking.policy_digest),
            "generation resolution requires original approved API worker booking",
        )?;
        require(
            booking.fee_resolution.is_none()
                && booking.generation_source_digest()? == r.source_digest
                && booking.generation_outcome() == r.original_outcome
                && booking.reserved_usd_nanos == r.maximum_usd_nanos,
            "API generation resolution differs from original unresolved source",
        )?;
        let reserved = self
            .reserved_usd_nanos
            .checked_sub(booking.reserved_usd_nanos)
            .ok_or("API generation liability underflow")?;
        let charged = self
            .charged_usd_nanos
            .checked_add(r.input.amount_usd_nanos)
            .filter(|n| *n <= MAX_USD_NANOS)
            .ok_or("API generation fee exceeds exact range")?;
        require(
            charged
                .checked_add(reserved)
                .is_some_and(|n| n <= MAX_USD_NANOS),
            "API generation fee plus reservations exceeds exact range",
        )?;
        self.bookings
            .get_mut(&r.input.request_id)
            .unwrap()
            .fee_resolution = Some(Box::new(resolution.clone()));
        self.reserved_usd_nanos = reserved;
        self.charged_usd_nanos = charged;
        // Never clear another unpriced API or management request's block.
        self.blocked = self
            .bookings
            .values()
            .any(|b| matches!(b.outcome, Outcome::Unpriced { .. }) && b.fee_resolution.is_none());
        Ok(())
    }
}
impl BudgetLedger {
    /// Host must authenticate the original user event and stopped worker first.
    pub fn check_api_generation_resolution(
        &self,
        resolution: &ApiGenerationResolution,
    ) -> Result<(), String> {
        require(self.healthy(), "currency ledger requires reopen")?;
        super::api_count_resolution::require_guard(&self.root)?;
        resolution.validate()?;
        let request = &resolution.review.input.request_id;
        self.checked_api_preparation(request)?;
        let booking = self
            .state
            .bookings
            .get(request)
            .ok_or("original API booking missing")?;
        if let Some(saved) = &booking.fee_resolution {
            return require(
                saved.as_ref() == resolution,
                "API generation confirmation changed after publication",
            );
        }
        require(
            self.count < MAX_RECORDS,
            "currency ledger cannot hold generation confirmation",
        )?;
        self.state
            .clone()
            .resolve_api_generation(&self.policy, resolution, &self.head)?;
        let at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, false);
        encode_record(
            self.count,
            &self.head,
            &self.policy_digest,
            at,
            Event::ResolveApiGeneration {
                resolution: Box::new(resolution.clone()),
            },
        )?;
        Ok(())
    }
    pub fn record_api_generation_resolution(
        &mut self,
        resolution: ApiGenerationResolution,
    ) -> Result<(), String> {
        self.check_api_generation_resolution(&resolution)?;
        if self.state.bookings[&resolution.review.input.request_id]
            .fee_resolution
            .is_some()
        {
            return Ok(());
        }
        self.publish(Event::ResolveApiGeneration {
            resolution: Box::new(resolution),
        })
    }
}
