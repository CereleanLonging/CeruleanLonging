//! Original count liability settlement, independent of generation accounting.
use super::*;
use crate::count_resolution::ManagementCountResolution;
use cerulean_domain::count_resolution::CountResolutionAction;

impl CountBooking {
    pub fn source_digest(&self) -> Result<String, String> {
        digest(&("original-management-count-v1", &self.attempt, &self.outcome))
    }
    pub fn original_outcome(&self) -> &'static str {
        match self.outcome {
            CountOutcome::Pending => "pending",
            CountOutcome::Counted { .. } | CountOutcome::ScopedCounted { .. } => "counted",
            CountOutcome::Unknown { .. } => "unknown",
        }
    }
    pub fn resolution(&self, action: &CountResolutionAction) -> Option<&ManagementCountResolution> {
        match action {
            CountResolutionAction::SettleFee { .. } => self.fee_resolution.as_deref(),
            CountResolutionAction::RetireContinuation { .. } => self.retirement.as_deref(),
        }
    }
}
impl BudgetState {
    pub(super) fn resolve_count(
        &mut self,
        policy: &BudgetPolicy,
        resolution: &ManagementCountResolution,
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
            "count resolution has no original current approved account/head",
        )?;
        let count = self
            .management_counts
            .get_mut(&r.input.operation_id)
            .ok_or("original count missing")?;
        require(
            count.resolution(&r.input.action).is_none(),
            "count aspect already resolved",
        )?;
        require(
            count.source_digest()? == r.source_digest
                && count.original_outcome() == r.original_outcome
                && count.attempt.maximum_usd_nanos == r.maximum_usd_nanos,
            "count resolution differs from original attempt/outcome",
        )?;
        match r.input.action {
            CountResolutionAction::SettleFee {
                amount_usd_nanos, ..
            } => {
                let reserved = self
                    .reserved_usd_nanos
                    .checked_sub(count.attempt.maximum_usd_nanos)
                    .ok_or("count liability underflow")?;
                let charged = self
                    .charged_usd_nanos
                    .checked_add(amount_usd_nanos)
                    .filter(|n| *n <= MAX_USD_NANOS)
                    .ok_or("count charge exceeds exact monetary range")?;
                require(
                    charged
                        .checked_add(reserved)
                        .is_some_and(|n| n <= MAX_USD_NANOS),
                    "count charge and reservations exceed exact monetary range",
                )?;
                // A confirmed actual bill may exceed its original ceiling. Keep
                // the amount; insufficient capacity blocks subsequent spending.
                self.reserved_usd_nanos = reserved;
                self.charged_usd_nanos = charged;
                count.fee_resolution = Some(Box::new(resolution.clone()));
            }
            CountResolutionAction::RetireContinuation { .. } => {
                // Retirement cannot discharge unresolved generation liability.
                require(
                    self.bookings.get(&r.input.operation_id).is_none_or(|b| {
                        matches!(b.outcome, Outcome::Settled { .. } | Outcome::NotSent { .. })
                    }),
                    "resolve original generation liability before count retirement",
                )?;
                count.retirement = Some(Box::new(resolution.clone()));
            }
        }
        Ok(())
    }
}
impl BudgetLedger {
    pub(super) fn require_count_resolution_guard(&self) -> Result<(), String> {
        let data = self
            .root
            .parent()
            .filter(|p| p.file_name().is_some_and(|n| n == "currency-budgets"))
            .and_then(Path::parent)
            .ok_or("count resolution requires guarded host directory")?;
        crate::currency_count_resolution_guard::require_protected(data, true)
    }
    pub fn check_management_count_resolution(
        &self,
        resolution: &ManagementCountResolution,
    ) -> Result<(), String> {
        require(self.healthy(), "currency ledger requires reopen")?;
        self.require_count_resolution_guard()?;
        resolution.validate()?;
        let r = &resolution.review;
        let count = self
            .state
            .management_counts
            .get(&r.input.operation_id)
            .ok_or("original count missing")?;
        if let Some(saved) = count.resolution(&r.input.action) {
            return require(
                saved == resolution,
                "count resolution changed after publication",
            );
        }
        self.state
            .clone()
            .resolve_count(&self.policy, resolution, &self.head)
    }
    pub fn record_management_count_resolution(
        &mut self,
        resolution: ManagementCountResolution,
    ) -> Result<(), String> {
        self.check_management_count_resolution(&resolution)?;
        if self.state.management_counts[&resolution.review.input.operation_id]
            .resolution(&resolution.review.input.action)
            .is_some()
        {
            return Ok(());
        }
        self.publish(Event::ResolveManagementCount {
            resolution: Box::new(resolution),
        })
    }
}
