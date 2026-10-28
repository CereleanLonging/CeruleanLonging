//! One account and original-policy lookup for legacy and scoped pricing.
//! Guard checks precede all new records. These helpers grant no transport.
use super::*;
use cerulean_domain::{api_worker::ApiTarget, currency_budget::scoped::PricingObservation};

pub(super) fn require_policy(root: &Path, policy: &BudgetPolicy) -> Result<(), String> {
    if policy.schema == 2 || policy.scoped_quotes.is_some() {
        require_guard(root)?;
    }
    Ok(())
}
fn require_guard(root: &Path) -> Result<(), String> {
    let data = root
        .parent()
        .filter(|p| p.file_name().is_some_and(|n| n == "currency-budgets"))
        .and_then(Path::parent)
        .ok_or("scoped currency records require their guarded Host account directory")?;
    crate::currency_scoped_guard::require_protected(data, true)
}
pub(super) fn require_history(root: &Path, state: &BudgetState) -> Result<(), String> {
    if state.bookings.values().any(|b| b.fee_resolution.is_some()) {
        super::api_count_resolution::require_guard(root)?;
    }
    if state
        .api_counts
        .values()
        .any(|c| c.fee_resolution.is_some())
    {
        super::api_count_resolution::require_guard(root)?;
    }
    super::api_scoped::require_history(root, state)?;
    super::scoped_counts::require_history(root, state)?;
    super::scoped_preparation::require_history(root, state)?;
    if state
        .policies
        .values()
        .any(|p| p.schema == 2 || p.scoped_quotes.is_some())
        || state.bookings.values().any(|b| {
            b.reservation.pricing.is_some()
                || match &b.outcome {
                    Outcome::Settled { settlement, .. } | Outcome::Unpriced { settlement, .. } => {
                        settlement.actual_service.is_some()
                    }
                    _ => false,
                }
        })
    {
        require_guard(root)?;
    }
    Ok(())
}
pub(super) fn require_event(
    root: &Path,
    policy: &BudgetPolicy,
    event: &Event,
) -> Result<(), String> {
    if matches!(
        event,
        Event::ResolveApiCount { .. } | Event::ResolveApiGeneration { .. }
    ) {
        super::api_count_resolution::require_guard(root)?;
    }
    super::api_scoped::require_event(root, event)?;
    super::scoped_counts::require_event(root, event)?;
    super::scoped_preparation::require_event(root, event)?;
    require_policy(root, policy)?;
    match event {
        Event::Open { policy } => require_policy(root, policy),
        Event::Authorize { authorization } => require_policy(root, &authorization.policy),
        Event::Reserve { reservation, .. } if reservation.pricing.is_some() => require_guard(root),
        Event::ReservePreparedManagement { preparation }
            if preparation.reservation.pricing.is_some() =>
        {
            require_guard(root)
        }
        Event::Settle { settlement } if settlement.actual_service.is_some() => require_guard(root),
        _ => Ok(()),
    }
}

pub(super) fn model_target<'a>(
    policy: &'a BudgetPolicy,
    reservation: &Reservation,
) -> Result<(&'a str, &'a ApiTarget), String> {
    if reservation.pricing.is_some() {
        let quote = policy.scoped_quote(&reservation.quote_digest)?;
        Ok((&quote.requested_model, &quote.target))
    } else {
        let quote = policy.quote(&reservation.quote_digest)?;
        Ok((&quote.requested_model, &quote.target))
    }
}

pub(super) fn reserve_amount(
    policy: &BudgetPolicy,
    reservation: &Reservation,
    at: &str,
) -> Result<u64, String> {
    match &reservation.pricing {
        Some(request) => {
            require(
                policy.schema == 2,
                "scoped reservation requires its original scoped approval",
            )?;
            policy
                .scoped_quote(&reservation.quote_digest)?
                .reserve(request, at)
        }
        None => {
            require(
                policy.schema == 1,
                "scoped approval requires complete pricing identity",
            )?;
            let quote = policy.quote(&reservation.quote_digest)?;
            quote.valid_at(at)?;
            quote.rates.reserve(&reservation.ceiling)
        }
    }
}

pub(super) fn charge_amount(
    policy: &BudgetPolicy,
    reservation: &Reservation,
    settlement: &Settlement,
) -> Result<u64, String> {
    match &reservation.pricing {
        Some(request) => {
            require(
                policy.schema == 2,
                "scoped response requires its original scoped approval",
            )?;
            policy.scoped_quote(&reservation.quote_digest)?.charge(
                request,
                &PricingObservation {
                    actual_model: settlement.actual_model.clone(),
                    actual_service: settlement.actual_service.clone(),
                    usage: settlement.usage.clone(),
                },
            )
        }
        None => {
            require(
                policy.schema == 1 && settlement.actual_service.is_none(),
                "legacy reservation cannot acquire scoped response terms",
            )?;
            let quote = policy.quote(&reservation.quote_digest)?;
            require(
                quote.reported_models.contains(&settlement.actual_model),
                "actual model absent from approved quote",
            )?;
            require(
                reservation.ceiling.contains(&settlement.usage)?,
                "observed billable usage exceeds reserved ceiling",
            )?;
            quote.rates.charge(&settlement.usage)
        }
    }
}

#[cfg(all(test, windows))]
mod tests;
