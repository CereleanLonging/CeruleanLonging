//! Historical amounts are explicit user-reviewed evidence, never inferred prices.
//! These primitives do not authenticate a user or validate an external invoice.
//! The host must bind the original immutable source and user workflow event.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoricalCharge {
    pub project_id: String,
    pub scope: HistoricalScope,
    /// Digest of the complete immutable original evidence reviewed by the user.
    pub source_digest: String,
    pub policy_digest: String,
    pub previous_ledger_head: String,
    pub amount_usd_nanos: u64,
    /// Explicit invoice/statement reference; the ledger does not fetch it.
    pub billing_reference: String,
    pub explanation: String,
    /// Host-validated authenticated user event, not a model-supplied role label.
    pub user_event_digest: String,
}
impl HistoricalCharge {
    pub fn validate(&self) -> Result<(), String> {
        identifier(&self.project_id)?;
        self.scope.validate(&self.project_id)?;
        for h in [
            &self.source_digest,
            &self.policy_digest,
            &self.previous_ledger_head,
            &self.user_event_digest,
        ] {
            sha256(h)?;
        }
        require(
            self.amount_usd_nanos <= MAX_USD_NANOS,
            "historical amount is outside the exact monetary range",
        )?;
        for value in [&self.billing_reference, &self.explanation] {
            require(
                !value.trim().is_empty() && value.len() <= 8000 && !value.contains('\0'),
                "historical amount requires an explicit billing reference and explanation",
            )?;
        }
        Ok(())
    }
}

impl BudgetState {
    pub(super) fn advance_historical_charge(
        &mut self,
        policy: &BudgetPolicy,
        charge: &HistoricalCharge,
        head: &str,
    ) -> Result<(), String> {
        charge.validate()?;
        require(
            charge.project_id == policy.project_id
                && charge.policy_digest == self.current_policy_digest
                && charge.policy_digest == digest(policy)?
                && charge.previous_ledger_head == head,
            "historical review belongs to a different account, approval or ledger head",
        )?;
        require(
            self.approval_links.contains_key(&charge.policy_digest),
            "historical charge requires an original linked user approval",
        )?;
        let key = charge.scope.key()?;
        require(
            !self.historical_charges.contains_key(&key),
            "historical scope already accounted",
        )?;
        require(
            !self
                .bookings
                .values()
                .any(|b| b.reservation.operation_id == charge.scope.operation_id()),
            "historical scope already has reservations; use its original settlement",
        )?;
        let total = self
            .charged_usd_nanos
            .checked_add(charge.amount_usd_nanos)
            .filter(|n| *n <= MAX_USD_NANOS)
            .ok_or("historical total exceeds exact monetary range")?;
        require(
            total
                .checked_add(self.reserved_usd_nanos)
                .is_some_and(|n| n <= MAX_USD_NANOS),
            "historical amount plus outstanding reservations exceeds exact monetary range",
        )?;
        // An observed historical charge may exceed a prior limit. Keep the true
        // amount; available_usd_nanos then refuses new spending until explicitly
        // authorized capacity covers all charges AND outstanding reservations.
        self.charged_usd_nanos = total;
        self.historical_charges.insert(key, charge.clone());
        Ok(())
    }
}

impl BudgetLedger {
    /// Arithmetic and original-account preflight only. The host must separately
    /// prove the immutable source scope and authenticated confirmation event.
    pub fn check_historical_charge(&self, charge: &HistoricalCharge) -> Result<(), String> {
        require(self.healthy(), "currency ledger requires reopen")?;
        charge.validate()?;
        if let Some(saved) = self.state.historical_charges.get(&charge.scope.key()?) {
            return require(
                saved == charge,
                "historical confirmation changed after accounting",
            );
        }
        self.state
            .clone()
            .advance_historical_charge(&self.policy, charge, &self.head)
    }
    /// Append once; no automatic pricing, refund, invoice verification or send.
    pub fn record_historical_charge(&mut self, charge: HistoricalCharge) -> Result<(), String> {
        self.check_historical_charge(&charge)?;
        if self
            .state
            .historical_charges
            .contains_key(&charge.scope.key()?)
        {
            return Ok(());
        }
        self.publish(Event::HistoricalCharge {
            charge: Box::new(charge),
        })
    }
}
