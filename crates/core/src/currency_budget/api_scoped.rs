//! Original API round liability and preparation share the project account.
//! Public archived evidence never reconstructs an adapter send capability.
use super::*;
use crate::{api_execution::ApiRunIdentity, omp_tool_journal::ToolScope};
use cerulean_adapters::api_generation::scoped::{
    CountedScopedApi, PreparedScopedApi, ScopedApiDraft, ScopedApiFingerprint,
    ScopedApiPricingAudit, ScopedApiTokenCount,
};
use cerulean_domain::currency_budget::scoped::{InputRange, PricingRequest};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiCountAttempt {
    pub schema: u32,
    pub run: ApiRunIdentity,
    pub call: CallIdentity,
    pub request: ScopedApiFingerprint,
    pub policy_digest: String,
    pub dispatch_head: String,
    pub quote_digest: String,
    pub maximum_usd_nanos: u64,
}
impl ApiCountAttempt {
    pub(crate) fn validate(&self, policy: &BudgetPolicy) -> Result<(), String> {
        require(
            self.schema == 1 && policy.schema == 2 && policy.planning.is_none(),
            "API count requires original scoped execution approval",
        )?;
        self.run.validate_tool_scope()?;
        self.call.validate()?;
        self.request.validate()?;
        sha256(&self.dispatch_head)?;
        crate::api_execution::check_round_scope(&self.run, &self.call, self.call.round)?;
        let quote = policy.scoped_quote(&self.quote_digest)?;
        let fee = quote
            .count_price
            .as_ref()
            .ok_or("explicit API count fee required")?;
        require(
            self.policy_digest == digest(policy)?
                && self.call.project_id == policy.project_id
                && self.call.revision_digest == policy.revision_digest
                && self.call.round <= self.run.worker.max_rounds
                && self.call.fingerprint == self.request.request
                && quote.target == self.request.request.target
                && quote.requested_model == self.request.request.requested_model
                && quote.requested_service == self.request.requested_service
                && self.request.count_body_bytes <= fee.maximum_body_bytes as usize
                && self.maximum_usd_nanos == fee.maximum_usd_nanos,
            "API count differs from original round, approval or fee scope",
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ApiCountOutcome {
    Pending,
    Counted { receipt: Box<ScopedApiTokenCount> },
    Unknown { evidence_sha256: String },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiCountBooking {
    pub attempt: ApiCountAttempt,
    pub outcome: ApiCountOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fee_resolution: Option<Box<crate::api_count_resolution::ApiCountResolution>>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopedApiPreparation {
    pub schema: u32,
    pub attempt: ApiCountAttempt,
    pub count: ScopedApiTokenCount,
    pub audit: ScopedApiPricingAudit,
    pub reservation: Reservation,
}
impl ScopedApiPreparation {
    pub(crate) fn validate(&self, policy: &BudgetPolicy) -> Result<(), String> {
        require(self.schema == 1, "unsupported API preparation")?;
        self.attempt.validate(policy)?;
        let a = &self.attempt;
        self.count.validate(&a.request, &a.quote_digest)?;
        self.reservation.validate()?;
        let request = PricingRequest {
            target: a.request.request.target.clone(),
            requested_model: a.request.request.requested_model.clone(),
            requested_service: a.request.requested_service.clone(),
            input: InputRange {
                from: 0,
                through: self.count.input_tokens,
            },
            output_ceiling: a.request.request.max_output_tokens.into(),
            cache_read_allowed: true,
            cache_write_allowed: true,
        };
        let quote = policy.scoped_quote(&a.quote_digest)?;
        let amount = quote.reserve(&request, &self.audit.reviewed_at)?;
        let r = &self.reservation;
        require(
            self.audit.request == a.request
                && self.audit.quote_sha256 == a.quote_digest
                && self.audit.pricing_request_sha256 == digest(&request)?
                && self.audit.maximum_usd_nanos == amount
                && r.pricing.as_ref() == Some(&request)
                && r.quote_digest == a.quote_digest
                && r.request_id == a.call.request_id
                && r.operation_id == a.call.attempt_id
                && r.role == ChargeRole::Worker
                && r.request_sha256 == a.call.fingerprint.body_sha256
                && r.ceiling_evidence_sha256 == digest(&self.count)?,
            "API preparation audit, original count and reservation disagree",
        )?;
        validate_api_binding(policy, r, &a.call)
    }
}

pub(super) fn require_guard(root: &Path) -> Result<(), String> {
    let data = root
        .parent()
        .filter(|p| p.file_name().is_some_and(|v| v == "currency-budgets"))
        .and_then(Path::parent)
        .ok_or("API liability requires original guarded account")?;
    crate::currency_api_guard::require_protected(data, true)
}
pub(super) fn require_history(root: &Path, state: &BudgetState) -> Result<(), String> {
    if !state.api_counts.is_empty()
        || state
            .bookings
            .values()
            .any(|b| b.scoped_api_preparation.is_some())
    {
        require_guard(root)?;
    }
    Ok(())
}
pub(super) fn require_event(root: &Path, event: &Event) -> Result<(), String> {
    if matches!(
        event,
        Event::BeginApiCount { .. }
            | Event::RecordApiCount { .. }
            | Event::ApiCountUnknown { .. }
            | Event::Reserve {
                scoped_api_preparation: Some(_),
                ..
            }
    ) {
        require_guard(root)?;
    }
    Ok(())
}
impl BudgetState {
    pub(super) fn begin_api_count(
        &mut self,
        policy: &BudgetPolicy,
        attempt: &ApiCountAttempt,
        at: &str,
    ) -> Result<(), String> {
        attempt.validate(policy)?;
        require(
            self.approval_links
                .contains_key(&self.current_policy_digest),
            "API count needs linked original user approval",
        )?;
        let id = &attempt.call.request_id;
        require(
            self.api_counts
                .values()
                .filter(|b| b.attempt.call.attempt_id == attempt.call.attempt_id)
                .all(|b| {
                    b.attempt.run == attempt.run && b.attempt.dispatch_head == attempt.dispatch_head
                }),
            "API rounds cannot change the original run or dispatch authority",
        )?;
        require(
            !self.api_counts.contains_key(id)
                && !self.management_counts.contains_key(id)
                && !self.bookings.contains_key(id)
                && !self
                    .historical_charges
                    .values()
                    .any(|h| h.scope.operation_id() == attempt.call.attempt_id),
            "API round already counted or accounted; never recount",
        )?;
        policy.scoped_quote(&attempt.quote_digest)?.valid_at(at)?;
        require(
            attempt.maximum_usd_nanos <= self.available_usd_nanos(policy)?,
            "shared currency budget exhausted before API count",
        )?;
        self.reserved_usd_nanos = self
            .reserved_usd_nanos
            .checked_add(attempt.maximum_usd_nanos)
            .ok_or("API count liability overflow")?;
        self.api_counts.insert(
            id.clone(),
            ApiCountBooking {
                attempt: attempt.clone(),
                outcome: ApiCountOutcome::Pending,
                fee_resolution: None,
            },
        );
        Ok(())
    }
    pub(super) fn api_count_receipt(
        &mut self,
        request: &str,
        receipt: &ScopedApiTokenCount,
    ) -> Result<(), String> {
        let booking = self
            .api_counts
            .get_mut(request)
            .ok_or("original API count missing")?;
        require(
            matches!(booking.outcome, ApiCountOutcome::Pending),
            "API count already resolved",
        )?;
        receipt.validate(&booking.attempt.request, &booking.attempt.quote_digest)?;
        booking.outcome = ApiCountOutcome::Counted {
            receipt: Box::new(receipt.clone()),
        };
        Ok(())
    }
    pub(super) fn api_count_unknown(&mut self, request: &str, hash: &str) -> Result<(), String> {
        sha256(hash)?;
        let booking = self
            .api_counts
            .get_mut(request)
            .ok_or("original API count missing")?;
        require(
            matches!(booking.outcome, ApiCountOutcome::Pending),
            "API count already resolved",
        )?;
        booking.outcome = ApiCountOutcome::Unknown {
            evidence_sha256: hash.into(),
        };
        Ok(())
    }
    pub(super) fn api_counted_preparation(
        &self,
        policy: &BudgetPolicy,
        prep: &ScopedApiPreparation,
    ) -> Result<(), String> {
        prep.validate(policy)?;
        require(
            self.approval_links
                .contains_key(&self.current_policy_digest),
            "API preparation needs original user approval",
        )?;
        let count = self
            .api_counts
            .get(&prep.attempt.call.request_id)
            .ok_or("original paid API count missing")?;
        require(
            count.fee_resolution.is_none()
                && count.attempt == prep.attempt
                && matches!(&count.outcome, ApiCountOutcome::Counted { receipt } if receipt.as_ref() == &prep.count),
            "API preparation differs from original count",
        )
    }
}
impl BudgetLedger {
    /// Host authenticates the original assignment/dispatch and live run before
    /// this atomic publication; only then may the worker issue its count POST.
    pub fn begin_api_count(
        &mut self,
        run: &ApiRunIdentity,
        call: &CallIdentity,
        draft: &ScopedApiDraft,
        dispatch_head: &str,
        quote: &str,
    ) -> Result<u64, String> {
        let price = self
            .policy
            .scoped_quote(quote)?
            .count_price
            .as_ref()
            .ok_or("API count fee unknown")?;
        let attempt = ApiCountAttempt {
            schema: 1,
            run: run.clone(),
            call: call.clone(),
            request: draft.fingerprint().clone(),
            policy_digest: digest(&self.policy)?,
            dispatch_head: dispatch_head.into(),
            quote_digest: quote.into(),
            maximum_usd_nanos: price.maximum_usd_nanos,
        };
        let fee = attempt.maximum_usd_nanos;
        self.publish(Event::BeginApiCount {
            attempt: Box::new(attempt),
        })?;
        Ok(fee)
    }
    /// This is deliberately before fresh approval/expiry checks: a late valid
    /// receipt still belongs to the already recorded count-fee liability.
    pub fn record_api_count(
        &mut self,
        request: &str,
        counted: &CountedScopedApi,
    ) -> Result<(), String> {
        require(self.healthy(), "currency ledger requires reopen")?;
        let original = self
            .state
            .api_counts
            .get(request)
            .ok_or("original API count missing")?;
        if matches!(&original.outcome, ApiCountOutcome::Counted { receipt } if receipt.as_ref() == counted.evidence())
        {
            return Ok(());
        }
        self.publish(Event::RecordApiCount {
            request_id: request.into(),
            receipt: Box::new(counted.evidence().clone()),
        })
    }
    pub fn mark_api_count_unknown(&mut self, request: &str, hash: &str) -> Result<(), String> {
        require(self.healthy(), "currency ledger requires reopen")?;
        let original = self
            .state
            .api_counts
            .get(request)
            .ok_or("original API count missing")?;
        if matches!(&original.outcome, ApiCountOutcome::Unknown { evidence_sha256 } if evidence_sha256 == hash)
        {
            return Ok(());
        }
        self.publish(Event::ApiCountUnknown {
            request_id: request.into(),
            evidence_sha256: hash.into(),
        })
    }
    pub(crate) fn recover_api_counts(&mut self) -> Result<(), String> {
        let pending: Vec<_> = self
            .state
            .api_counts
            .iter()
            .filter(|(_, b)| matches!(b.outcome, ApiCountOutcome::Pending))
            .map(|(id, b)| {
                Ok((
                    id.clone(),
                    digest(&("api-count-interrupted-v1", &b.attempt))?,
                ))
            })
            .collect::<Result<_, String>>()?;
        for (id, hash) in pending {
            self.mark_api_count_unknown(&id, &hash)?;
        }
        Ok(())
    }
    pub fn prepare_scoped_api(
        &mut self,
        request: &str,
        prepared: &PreparedScopedApi,
    ) -> Result<u64, String> {
        prepared.check_ready()?;
        let count = self
            .state
            .api_counts
            .get(request)
            .ok_or("original API count missing")?;
        let pricing = prepared.pricing_request();
        let prep = ScopedApiPreparation {
            schema: 1,
            attempt: count.attempt.clone(),
            count: prepared.evidence().clone(),
            audit: prepared.audit().clone(),
            reservation: Reservation {
                pricing: Some(pricing.clone()),
                request_id: request.into(),
                operation_id: count.attempt.call.attempt_id.clone(),
                role: ChargeRole::Worker,
                quote_digest: digest(prepared.quote())?,
                request_sha256: prepared.audit().request.request.body_sha256.clone(),
                ceiling: TokenCeiling {
                    total_input: pricing.input.through,
                    output: pricing.output_ceiling,
                    cache_read_allowed: true,
                    cache_write_allowed: true,
                },
                ceiling_evidence_sha256: digest(prepared.evidence())?,
            },
        };
        self.publish(Event::Reserve {
            reservation: prep.reservation.clone(),
            api_identity: Some(Box::new(prep.attempt.call.clone())),
            management_identity: None,
            scoped_preparation: None,
            scoped_api_preparation: Some(Box::new(prep)),
        })?;
        Ok(self.state.bookings[request].reserved_usd_nanos)
    }
}

#[cfg(all(test, windows))]
mod tests;
