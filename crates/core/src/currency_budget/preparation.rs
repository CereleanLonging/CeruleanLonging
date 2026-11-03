//! One durable entry contains the reservation, original intent and count receipt.
//! Recovery never reconstructs a send intent or interprets absence as a refund.
use super::*;
use crate::management_execution::{DecisionIntent, DecisionJournal};
use cerulean_adapters::openai::{CountedDecision, ManagementTokenCount, PreparedDecision};
use cerulean_domain::management::Purpose;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManagementPreparation {
    pub schema: u32,
    pub intent: DecisionIntent,
    pub identity: ManagementChargeIdentity,
    pub count: ManagementTokenCount,
    pub reservation: Reservation,
}
impl ManagementPreparation {
    pub(super) fn validate(&self, policy: &BudgetPolicy) -> Result<(), String> {
        require(self.schema == 1, "unsupported management preparation")?;
        let identity =
            ManagementChargeIdentity::from_original(&self.intent, &self.identity.request, policy)?;
        require(
            identity == self.identity,
            "prepared original management identity changed",
        )?;
        self.count.validate(&identity.request)?;
        self.reservation.validate()?;
        management::validate_binding(policy, &self.reservation, &identity)?;
        require(
            self.reservation.ceiling
                == TokenCeiling {
                    total_input: self.count.input_tokens,
                    output: u64::from(identity.request.max_output_tokens),
                    cache_read_allowed: true,
                    cache_write_allowed: true,
                }
                && self.reservation.ceiling_evidence_sha256 == digest(&self.count)?,
            "prepared count and reservation disagree",
        )
    }
}
impl BudgetLedger {
    pub(super) fn require_preparation_guard(&self) -> Result<(), String> {
        let data = self
            .root
            .parent()
            .filter(|p| p.file_name().is_some_and(|n| n == "currency-budgets"))
            .and_then(Path::parent)
            .ok_or("prepared money calls require a guarded host directory")?;
        crate::currency_execution_guard::require_protected(data, true)
    }
    /// Atomically publishes all preparation evidence before the original send
    /// intent. The authenticated Host still reconciles approval before calling.
    pub fn prepare_counted_management(
        &mut self,
        intent: &DecisionIntent,
        counted: &CountedDecision,
        quote: &str,
    ) -> Result<u64, String> {
        self.require_preparation_guard()?;
        require(
            PreparedDecision::new(
                intent.purpose,
                &intent.model,
                &intent.state_digest,
                &intent.context,
            )?
            .fingerprint()
                == counted.prepared().fingerprint(),
            "management preparation differs from original request",
        )?;
        require(
            self.state.approval_links.contains_key(&self.policy_digest),
            "management preparation needs original linked approval",
        )?;
        let preparation = ManagementPreparation {
            schema: 1,
            intent: intent.clone(),
            identity: ManagementChargeIdentity::from_original(
                intent,
                counted.prepared().fingerprint(),
                &self.policy,
            )?,
            count: counted.evidence().clone(),
            reservation: Reservation {
                pricing: None,
                request_id: intent.operation_id.clone(),
                operation_id: intent.operation_id.clone(),
                role: match intent.purpose {
                    Purpose::Plan | Purpose::Refine | Purpose::Accept => ChargeRole::Chief,
                    _ => ChargeRole::Deputy,
                },
                quote_digest: quote.into(),
                request_sha256: counted.prepared().fingerprint().body_sha256.clone(),
                ceiling: counted.ceiling()?,
                ceiling_evidence_sha256: digest(counted.evidence())?,
            },
        };
        self.publish(Event::ReservePreparedManagement {
            preparation: Box::new(preparation),
        })?;
        Ok(self.state.bookings[&intent.operation_id].reserved_usd_nanos)
    }
    /// Outer None means an original journal must be checked. Some(None) means
    /// the already-recorded unknown preparation is unchanged and stays reserved.
    pub(super) fn prepared_without_intent(
        &self,
        journal: &DecisionJournal,
        id: &str,
    ) -> Result<Option<Option<Event>>, String> {
        let booking = self
            .state
            .bookings
            .get(id)
            .ok_or("management reservation missing")?;
        let Some(prepared) = &booking.management_preparation else {
            return Ok(None);
        };
        if !journal.absent_prepared_intent(id)? {
            return Ok(None);
        }
        prepared.validate(
            self.state
                .policies
                .get(&booking.policy_digest)
                .ok_or("prepared policy missing")?,
        )?;
        require(
            booking.management_identity.as_ref() == Some(&prepared.identity)
                && booking.reservation == prepared.reservation,
            "prepared booking changed",
        )?;
        let hash = digest(prepared)?;
        match &booking.outcome {
            Outcome::Reserved => Ok(Some(Some(Event::Unknown { request_id: id.into(), evidence_sha256: hash }))),
            Outcome::Unknown { evidence_sha256 } if *evidence_sha256 == hash => Ok(Some(None)),
            _ => Err("original management evidence disappeared after observation; preserve the reservation".into()),
        }
    }
}
