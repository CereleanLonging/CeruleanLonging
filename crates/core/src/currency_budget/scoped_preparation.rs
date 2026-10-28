//! One original monetary reservation carries scoped preparation and count proof.
use super::*;
use crate::management_execution::{DecisionIntent, DecisionJournal};
use cerulean_adapters::openai::scoped_pricing::{
    PreparedScopedDecision, ScopedDecisionDraft, ScopedDecisionFingerprint, ScopedPricingAudit,
    ScopedTokenCount,
};
use cerulean_domain::{currency_budget::scoped::InputRange, management::Purpose};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopedManagementPreparation {
    pub schema: u32,
    pub intent: DecisionIntent,
    pub identity: ManagementChargeIdentity<ScopedDecisionFingerprint>,
    pub count: ScopedTokenCount,
    pub audit: ScopedPricingAudit,
    pub reservation: Reservation,
}
fn role(purpose: Purpose) -> ChargeRole {
    match purpose {
        Purpose::Plan | Purpose::Accept => ChargeRole::Chief,
        _ => ChargeRole::Deputy,
    }
}
impl ScopedManagementPreparation {
    pub(super) fn validate(&self, policy: &BudgetPolicy) -> Result<(), String> {
        require(
            self.schema == 1 && policy.schema == 2,
            "unsupported scoped management preparation",
        )?;
        let draft = ScopedDecisionDraft::standard(
            self.intent.purpose,
            &self.intent.model,
            &self.intent.state_digest,
            &self.intent.context,
        )?;
        require(
            self.identity
                == ManagementChargeIdentity::from_scoped_original(
                    &self.intent,
                    draft.fingerprint(),
                    policy,
                )?,
            "scoped preparation differs from original role/configuration/request",
        )?;
        let quote = policy.scoped_quote(&self.reservation.quote_digest)?;
        self.count.validate(draft.fingerprint(), &digest(quote)?)?;
        self.reservation.validate()?;
        let review = draft.review(
            quote,
            InputRange {
                from: 0,
                through: self.count.input_tokens,
            },
            &self.audit.reviewed_at,
        )?;
        let r = &self.reservation;
        require(
            review.audit() == &self.audit
                && r.request_id == self.intent.operation_id
                && r.operation_id == self.intent.operation_id
                && r.role == role(self.intent.purpose)
                && r.request_sha256 == self.identity.request.body_sha256
                && r.ceiling_evidence_sha256 == digest(&self.count)?
                && r.pricing.as_ref().is_some_and(|p| {
                    digest(p).ok().as_deref() == Some(self.audit.pricing_request_sha256.as_str())
                }),
            "scoped preparation, audit and reservation disagree",
        )
    }
    pub(crate) fn journal_binding(
        &self,
    ) -> Result<crate::management_execution::ScopedIntentBinding, String> {
        Ok(crate::management_execution::ScopedIntentBinding {
            request: self.identity.request.clone(),
            pricing_review_sha256: digest(&self.audit)?,
        })
    }
}
impl Booking {
    pub fn is_management(&self) -> bool {
        self.management_identity.is_some() || self.scoped_management_preparation.is_some()
    }
}
impl BudgetState {
    pub(super) fn scoped_counted_preparation(
        &self,
        policy: &BudgetPolicy,
        prep: &ScopedManagementPreparation,
    ) -> Result<(), String> {
        prep.validate(policy)?;
        require(
            self.approval_links
                .contains_key(&self.current_policy_digest),
            "scoped preparation needs linked original approval",
        )?;
        let count = self
            .management_counts
            .get(&prep.intent.operation_id)
            .ok_or("original scoped count missing")?;
        require(
            count.retirement.is_none()
                && count.attempt.intent == prep.intent
                && count.attempt.identity == CountIdentity::Scoped(prep.identity.clone())
                && count.attempt.quote_digest == prep.reservation.quote_digest
                && matches!(&count.outcome, CountOutcome::ScopedCounted {receipt} if receipt.as_ref() == &prep.count),
            "scoped preparation differs from its original count or continuation is retired",
        )
    }
}
pub(super) fn require_guard(root: &Path) -> Result<(), String> {
    let data = root
        .parent()
        .filter(|p| p.file_name().is_some_and(|n| n == "currency-budgets"))
        .and_then(Path::parent)
        .ok_or("scoped generation requires original guarded account")?;
    crate::currency_scoped_execution_guard::require_protected(data, true)
}
pub(super) fn require_history(root: &Path, state: &BudgetState) -> Result<(), String> {
    if state
        .bookings
        .values()
        .any(|b| b.scoped_management_preparation.is_some())
    {
        require_guard(root)?;
    }
    Ok(())
}
pub(super) fn require_event(root: &Path, event: &Event) -> Result<(), String> {
    if matches!(
        event,
        Event::Reserve {
            scoped_preparation: Some(_),
            ..
        }
    ) {
        require_guard(root)?;
    }
    Ok(())
}
impl BudgetLedger {
    pub(crate) fn scoped_fee_ack(
        &self,
        journal: &DecisionJournal,
        id: &str,
    ) -> Result<crate::management_execution::ScopedFeeAck, String> {
        journal.require_billing_published()?;
        self.scoped_fee_ack_readback(journal, id)
    }
    /// Startup validates the original settled record before journal pending
    /// publications are recovered. This readback never creates a charge.
    pub(crate) fn scoped_fee_ack_readback(
        &self,
        journal: &DecisionJournal,
        id: &str,
    ) -> Result<crate::management_execution::ScopedFeeAck, String> {
        require(self.healthy(), "original currency ledger requires reopen")?;
        require(
            self.scoped_management_observation(journal, id)?.is_none(),
            "original scoped fee must be durably reconciled before application",
        )?;
        let booking = self
            .state
            .bookings
            .get(id)
            .ok_or("original scoped booking missing")?;
        let Outcome::Settled { settlement, .. } = &booking.outcome else {
            return Err("scoped fee is unknown or unpriced; application remains blocked".into());
        };
        let ack = crate::management_execution::ScopedFeeAck {
            schema: 1,
            operation_id: id.into(),
            response_head: journal.billing_evidence(id)?.head.into(),
            settlement_head: self
                .settlement_heads
                .get(id)
                .ok_or("original settlement publication missing")?
                .clone(),
            settlement_sha256: digest(settlement)?,
        };
        ack.validate()?;
        Ok(ack)
    }
    pub fn prepare_scoped_management(
        &mut self,
        intent: &DecisionIntent,
        prepared: &PreparedScopedDecision,
    ) -> Result<u64, String> {
        prepared.check_ready()?;
        let request = &prepared.audit().request;
        let pricing = prepared.pricing_request();
        let prep = ScopedManagementPreparation {
            schema: 1,
            intent: intent.clone(),
            identity: ManagementChargeIdentity::from_scoped_original(
                intent,
                request,
                &self.policy,
            )?,
            count: prepared.evidence().clone(),
            audit: prepared.audit().clone(),
            reservation: Reservation {
                pricing: Some(pricing.clone()),
                request_id: intent.operation_id.clone(),
                operation_id: intent.operation_id.clone(),
                role: role(intent.purpose),
                quote_digest: digest(prepared.quote())?,
                request_sha256: request.body_sha256.clone(),
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
            api_identity: None,
            management_identity: None,
            scoped_preparation: Some(Box::new(prep)),
            scoped_api_preparation: None,
        })?;
        Ok(self.state.bookings[&intent.operation_id].reserved_usd_nanos)
    }
    pub(super) fn scoped_management_observation(
        &self,
        journal: &DecisionJournal,
        id: &str,
    ) -> Result<Option<Event>, String> {
        let booking = self
            .state
            .bookings
            .get(id)
            .ok_or("scoped reservation missing")?;
        let prep = booking
            .scoped_management_preparation
            .as_ref()
            .ok_or("original scoped preparation missing")?;
        let policy = self
            .state
            .policies
            .get(&booking.policy_digest)
            .ok_or("original scoped policy missing")?;
        prep.validate(policy)?;
        require(
            booking.reservation == prep.reservation
                && booking.management_identity.is_none()
                && booking.api_identity.is_none(),
            "scoped reservation changed",
        )?;
        if journal.absent_prepared_intent(id)? {
            let hash = digest(prep)?;
            return match &booking.outcome {
                Outcome::Reserved => Ok(Some(Event::Unknown {
                    request_id: id.into(),
                    evidence_sha256: hash,
                })),
                Outcome::Unknown { evidence_sha256 } if *evidence_sha256 == hash => Ok(None),
                _ => Err(
                    "original scoped intent disappeared after observation; preserve reservation"
                        .into(),
                ),
            };
        }
        let evidence = journal.billing_evidence(id)?;
        require(
            evidence.intent == &prep.intent
                && evidence.request.is_none()
                && evidence.scoped_request == Some(&prep.journal_binding()?),
            "scoped journal differs from original preparation",
        )?;
        if let Some(response) = evidence.scoped_response {
            if let Some(bill) = &response.billing {
                let settlement = Settlement {
                    request_id: id.into(),
                    response_id: bill.response_id.clone(),
                    request_sha256: prep.identity.request.body_sha256.clone(),
                    actual_model: bill.actual_model.clone(),
                    actual_service: bill.actual_service.clone(),
                    usage: bill.usage.clone(),
                    evidence_sha256: evidence.head.into(),
                };
                if matches!(&booking.outcome,Outcome::Settled{settlement:s,..}|Outcome::Unpriced{settlement:s,..} if s==&settlement)
                {
                    return Ok(None);
                }
                return Ok(Some(Event::Settle { settlement }));
            }
        }
        if matches!(booking.outcome, Outcome::Unknown { .. }) {
            return Ok(None);
        }
        Ok(Some(Event::Unknown {
            request_id: id.into(),
            evidence_sha256: evidence.head.into(),
        }))
    }
}
