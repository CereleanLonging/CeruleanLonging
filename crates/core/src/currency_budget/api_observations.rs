//! Original API observations and immutable settlement ACK; no second balance.
use super::*;
use crate::api_call_journal::{ApiFeeAck, ApiScopedBinding};
use cerulean_adapters::api_generation::scoped::PreparedScopedApi;

pub struct ApiPreparationPermit {
    call: CallIdentity,
    binding: ApiScopedBinding,
    data_root: PathBuf,
}
impl ApiPreparationPermit {
    pub(crate) fn call(&self) -> &CallIdentity {
        &self.call
    }
    pub(crate) fn binding(&self) -> &ApiScopedBinding {
        &self.binding
    }
    pub(crate) fn data_root(&self) -> &Path {
        &self.data_root
    }
}
/// Only the original account creates this in-memory proof. Archived ACK JSON
/// cannot be deserialized into a permit or a private worker reply.
pub struct ApiFeePermit {
    proof: ApiFeeAck,
    data_root: PathBuf,
}
impl ApiFeePermit {
    pub(crate) fn proof(&self) -> &ApiFeeAck {
        &self.proof
    }
    pub(crate) fn data_root(&self) -> &Path {
        &self.data_root
    }
}
impl ScopedApiPreparation {
    pub(crate) fn journal_binding(&self) -> Result<ApiScopedBinding, String> {
        Ok(ApiScopedBinding {
            request: self.attempt.request.clone(),
            preparation_sha256: digest(self)?,
            pricing_review_sha256: digest(&self.audit)?,
        })
    }
}
impl BudgetLedger {
    fn api_data_root(&self) -> Result<PathBuf, String> {
        api_scoped::require_guard(&self.root)?;
        self.root
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| "API account data root missing".to_string())
            .and_then(|p| fs::canonicalize(p).map_err(|e| e.to_string()))
    }
    pub(crate) fn checked_api_preparation(
        &self,
        request: &str,
    ) -> Result<&ScopedApiPreparation, String> {
        require(self.healthy(), "currency ledger requires reopen")?;
        let booking = self
            .state
            .bookings
            .get(request)
            .ok_or("original API booking missing")?;
        let prep = booking
            .scoped_api_preparation
            .as_deref()
            .ok_or("original scoped API preparation missing")?;
        let policy = self
            .state
            .policies
            .get(&booking.policy_digest)
            .ok_or("original API policy missing")?;
        prep.validate(policy)?;
        let count = self
            .state
            .api_counts
            .get(request)
            .ok_or("original API count missing")?;
        require(
            count.attempt == prep.attempt
                && matches!(&count.outcome,ApiCountOutcome::Counted{receipt} if receipt.as_ref()==&prep.count)
                && prep.attempt.policy_digest == booking.policy_digest
                && self
                    .state
                    .approval_links
                    .contains_key(&booking.policy_digest)
                && booking.api_identity.as_ref() == Some(&prep.attempt.call)
                && booking.management_identity.is_none()
                && booking.management_preparation.is_none()
                && booking.scoped_management_preparation.is_none()
                && booking.reservation == prep.reservation
                && booking.reserved_usd_nanos == prep.audit.maximum_usd_nanos,
            "API preparation differs from original count, approval or reservation",
        )?;
        Ok(prep)
    }
    /// Host must revalidate live assignment/run authority immediately before
    /// requesting this permit. It neither starts a request nor changes a quote.
    pub fn api_preparation_permit(
        &self,
        request: &str,
        prepared: &PreparedScopedApi,
    ) -> Result<ApiPreparationPermit, String> {
        prepared.check_ready()?;
        require(
            self.state
                .api_counts
                .get(request)
                .is_some_and(|c| c.fee_resolution.is_none()),
            "reconciled API count cannot restore a generation permit",
        )?;
        let prep = self.checked_api_preparation(request)?;
        require(
            matches!(self.state.bookings[request].outcome, Outcome::Reserved)
                && prep.attempt.policy_digest == self.state.current_policy_digest
                && prep.audit == *prepared.audit()
                && prep.count == *prepared.evidence()
                && self.policy.scoped_quote(&prep.attempt.quote_digest)? == prepared.quote(),
            "API preparation expired, resolved or differs from current original approval",
        )?;
        Ok(ApiPreparationPermit {
            call: prep.attempt.call.clone(),
            binding: prep.journal_binding()?,
            data_root: self.api_data_root()?,
        })
    }
    pub(super) fn check_scoped_api(&self, journal: &ApiCallJournal) -> Result<(), String> {
        require(journal.healthy(), "API journal requires recovery")?;
        journal.require_account_root(&self.api_data_root()?)?;
        let prep = self.checked_api_preparation(&journal.identity().request_id)?;
        require(
            journal.identity() == &prep.attempt.call,
            "API preparation belongs to another original call",
        )?;
        if journal.state().intent_recorded {
            require(
                journal.state().scoped_request.as_ref() == Some(&prep.journal_binding()?),
                "API original scoped intent missing or changed",
            )?;
        } else {
            require(
                journal.state().outcome.is_none() && journal.state().scoped_request.is_none(),
                "API empty preparation journal changed",
            )?;
        }
        if let Some(ack) = &journal.state().fee_ack {
            require(
                ack == &self.expected_api_fee_ack(journal)?,
                "API fee ACK differs from original settlement",
            )?;
        }
        Ok(())
    }
    pub(super) fn api_absent_preparation_observation(
        &self,
        request: &str,
    ) -> Result<Option<Event>, String> {
        let prep = self.checked_api_preparation(request)?;
        let hash = digest(prep)?;
        match &self.state.bookings[request].outcome {
            Outcome::Reserved => Ok(Some(Event::Unknown {
                request_id: request.into(),
                evidence_sha256: hash,
            })),
            Outcome::Unknown { evidence_sha256 } if *evidence_sha256 == hash => Ok(None),
            _ => Err(
                "API intent disappeared after a different observation; preserve evidence".into(),
            ),
        }
    }
    fn api_settlement(&self, journal: &ApiCallJournal) -> Result<Option<Settlement>, String> {
        let Some(response) = &journal.state().scoped_response else {
            return Ok(None);
        };
        let Some(bill) = &response.billing else {
            return Ok(None);
        };
        Ok(Some(Settlement {
            request_id: journal.identity().request_id.clone(),
            response_id: bill.response_id.clone(),
            request_sha256: journal.identity().fingerprint.body_sha256.clone(),
            actual_model: bill.actual_model.clone(),
            actual_service: bill.actual_service.clone(),
            usage: bill.usage.clone(),
            evidence_sha256: journal
                .state()
                .response_head
                .clone()
                .ok_or("API original response head missing")?,
        }))
    }
    pub(super) fn scoped_api_observation(
        &self,
        journal: &ApiCallJournal,
    ) -> Result<Option<Event>, String> {
        self.check_scoped_api(journal)?;
        let id = &journal.identity().request_id;
        if !journal.state().intent_recorded {
            return self.api_absent_preparation_observation(id);
        }
        let prior = &self.state.bookings[id].outcome;
        if let Some(settlement) = self.api_settlement(journal)? {
            if matches!(prior,Outcome::Settled{settlement:s,..}|Outcome::Unpriced{settlement:s,..} if s==&settlement)
            {
                return Ok(None);
            }
            return Ok(Some(Event::Settle { settlement }));
        }
        let head = journal
            .state()
            .response_head
            .as_deref()
            .unwrap_or(journal.head());
        if matches!(
            &journal.state().outcome,
            Some(CallOutcome::Failure {
                failure: cerulean_adapters::api_generation::ApiFailure::NotSent {}
            })
        ) {
            if matches!(prior,Outcome::NotSent{evidence_sha256} if evidence_sha256==head) {
                return Ok(None);
            }
            return Ok(Some(Event::NotSent {
                request_id: id.clone(),
                evidence_sha256: head.into(),
            }));
        }
        if matches!(prior, Outcome::Unknown { .. }) {
            return Ok(None);
        }
        Ok(Some(Event::Unknown {
            request_id: id.clone(),
            evidence_sha256: head.into(),
        }))
    }
    fn expected_api_fee_ack(&self, journal: &ApiCallJournal) -> Result<ApiFeeAck, String> {
        let id = &journal.identity().request_id;
        let booking = self
            .state
            .bookings
            .get(id)
            .ok_or("API original booking missing")?;
        let Outcome::Settled { settlement, .. } = &booking.outcome else {
            return Err(
                "API fee is unknown or unpriced; tools and next round remain blocked".into(),
            );
        };
        require(
            self.api_settlement(journal)?.as_ref() == Some(settlement),
            "API fee settlement differs from original response",
        )?;
        let ack = ApiFeeAck {
            schema: 1,
            request_id: id.clone(),
            response_head: settlement.evidence_sha256.clone(),
            settlement_head: self
                .settlement_heads
                .get(id)
                .ok_or("original API settlement publication missing")?
                .clone(),
            settlement_sha256: digest(settlement)?,
        };
        ack.validate()?;
        Ok(ack)
    }
    pub fn api_fee_permit(&self, journal: &ApiCallJournal) -> Result<ApiFeePermit, String> {
        self.check_scoped_api(journal)?;
        require(
            self.scoped_api_observation(journal)?.is_none(),
            "API fee must be durably observed before releasing reply",
        )?;
        Ok(ApiFeePermit {
            proof: self.expected_api_fee_ack(journal)?,
            data_root: self.api_data_root()?,
        })
    }
}
