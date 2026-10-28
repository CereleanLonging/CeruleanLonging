//! New wire protocol, same original approval, count map and fee writer.
use super::counts::CountIdentity;
use super::*;
use crate::management_execution::DecisionIntent;
use cerulean_adapters::openai::scoped_pricing::{
    CountedScopedDecision, ScopedDecisionDraft, ScopedTokenCount,
};

impl ManagementCountAttempt {
    pub(super) fn validate_scoped(&self, policy: &BudgetPolicy) -> Result<(), String> {
        require(
            self.schema == 2 && policy.schema == 2,
            "scoped count requires original scoped policy",
        )?;
        let draft = ScopedDecisionDraft::standard(
            self.intent.purpose,
            &self.intent.model,
            &self.intent.state_digest,
            &self.intent.context,
        )?;
        let identity = ManagementChargeIdentity::from_scoped_original(
            &self.intent,
            draft.fingerprint(),
            policy,
        )?;
        require(
            self.identity == CountIdentity::Scoped(identity),
            "scoped count differs from original management request",
        )?;
        let quote = policy.scoped_quote(&self.quote_digest)?;
        let price = quote
            .count_price
            .as_ref()
            .ok_or("explicit scoped count fee ceiling required; unknown is not free")?;
        let request = draft.fingerprint();
        require(
            quote.target == cerulean_domain::api_worker::ApiTarget::OpenAi {}
                && quote.requested_model == request.requested_model
                && quote.requested_service == request.requested_service
                && self.endpoint == request.count_url
                && self.body_sha256 == request.count_body_sha256
                && self.body_bytes == request.count_body_bytes
                && self.body_bytes <= price.maximum_body_bytes as usize
                && self.maximum_usd_nanos == price.maximum_usd_nanos,
            "scoped count request or fee ceiling differs from approved scope",
        )
    }
}
impl BudgetState {
    pub(super) fn scoped_count_receipt(
        &mut self,
        operation: &str,
        receipt: &ScopedTokenCount,
    ) -> Result<(), String> {
        let booking = self
            .management_counts
            .get_mut(operation)
            .ok_or("original scoped count attempt missing")?;
        require(
            matches!(booking.outcome, CountOutcome::Pending),
            "count outcome already recorded; do not replace",
        )?;
        let CountIdentity::Scoped(identity) = &booking.attempt.identity else {
            return Err("scoped count identity required".into());
        };
        receipt.validate(&identity.request, &booking.attempt.quote_digest)?;
        booking.outcome = CountOutcome::ScopedCounted {
            receipt: Box::new(receipt.clone()),
        };
        Ok(())
    }
}
pub(super) fn require_guard(root: &Path) -> Result<(), String> {
    let data = root
        .parent()
        .filter(|p| p.file_name().is_some_and(|n| n == "currency-budgets"))
        .and_then(Path::parent)
        .ok_or("scoped count requires guarded Host account")?;
    crate::currency_scoped_count_guard::require_protected(data, true)
}
pub(super) fn require_history(root: &Path, state: &BudgetState) -> Result<(), String> {
    if state.management_counts.values().any(|b| {
        b.attempt.schema == 2
            || matches!(b.attempt.identity, CountIdentity::Scoped(_))
            || matches!(b.outcome, CountOutcome::ScopedCounted { .. })
    }) {
        require_guard(root)?;
    }
    Ok(())
}
pub(super) fn require_event(root: &Path, event: &Event) -> Result<(), String> {
    match event {
        Event::BeginManagementCount { attempt }
            if attempt.schema == 2 || matches!(attempt.identity, CountIdentity::Scoped(_)) =>
        {
            require_guard(root)
        }
        Event::RecordScopedManagementCount { .. } => require_guard(root),
        _ => Ok(()),
    }
}
impl BudgetLedger {
    /// Durable count responsibility only; authenticating current Host scope and
    /// launching exactly once remain caller duties. Does not send or retry.
    pub fn begin_scoped_management_count(
        &mut self,
        intent: &DecisionIntent,
        draft: &ScopedDecisionDraft,
        quote_digest: &str,
    ) -> Result<u64, String> {
        let original = ScopedDecisionDraft::standard(
            intent.purpose,
            &intent.model,
            &intent.state_digest,
            &intent.context,
        )?;
        require(
            original.fingerprint() == draft.fingerprint(),
            "scoped count body differs from original intent",
        )?;
        let price = self
            .policy
            .scoped_quote(quote_digest)?
            .count_price
            .as_ref()
            .ok_or("explicit scoped count fee ceiling required; unknown is not free")?;
        let request = draft.fingerprint();
        let attempt = ManagementCountAttempt {
            schema: 2,
            intent: intent.clone(),
            identity: CountIdentity::Scoped(ManagementChargeIdentity::from_scoped_original(
                intent,
                request,
                &self.policy,
            )?),
            quote_digest: quote_digest.into(),
            endpoint: request.count_url.clone(),
            body_sha256: request.count_body_sha256.clone(),
            body_bytes: request.count_body_bytes,
            maximum_usd_nanos: price.maximum_usd_nanos,
        };
        let maximum = attempt.maximum_usd_nanos;
        self.publish(Event::BeginManagementCount {
            attempt: Box::new(attempt),
        })?;
        Ok(maximum)
    }
    /// Only accepts the adapter's live opaque count, never an audit JSON as send
    /// authority. Preserve its receipt even if live freshness has expired.
    pub fn record_scoped_management_count(
        &mut self,
        operation: &str,
        counted: &CountedScopedDecision,
    ) -> Result<(), String> {
        require(self.healthy(), "currency ledger requires reopen")?;
        require_history(&self.root, &self.state)?;
        let booking = self
            .state
            .management_counts
            .get(operation)
            .ok_or("original scoped count attempt missing")?;
        if matches!(&booking.outcome, CountOutcome::ScopedCounted { receipt } if receipt.as_ref() == counted.evidence())
        {
            return Ok(());
        }
        self.publish(Event::RecordScopedManagementCount {
            operation: operation.into(),
            receipt: counted.evidence().clone(),
        })
    }
}
