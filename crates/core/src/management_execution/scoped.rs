//! Scoped replies preserve billing independently of decision usability.
use super::*;
use cerulean_adapters::openai::scoped_pricing::{
    PreparedScopedDecision, ScopedBillingObservation, ScopedDecisionDraft,
    ScopedDecisionFingerprint, ScopedDecisionResult,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopedIntentBinding {
    pub request: ScopedDecisionFingerprint,
    pub pricing_review_sha256: String,
}
impl ScopedIntentBinding {
    pub(super) fn validate(&self, intent: &DecisionIntent) -> Result<(), String> {
        sha256(&self.pricing_review_sha256)?;
        self.request.validate()?;
        require(
            &self.request
                == ScopedDecisionDraft::standard(
                    intent.purpose,
                    &intent.model,
                    &intent.state_digest,
                    &intent.context,
                )?
                .fingerprint(),
            "scoped intent differs from original request",
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopedResponseRecord {
    pub schema: u32,
    pub request: ScopedIntentBinding,
    pub response_sha256: String,
    pub observation: Option<DecisionObservation>,
    pub billing: Option<ScopedBillingObservation>,
}
impl ScopedResponseRecord {
    pub(super) fn validate(&self, op: &Operation) -> Result<(), String> {
        sha256(&self.response_sha256)?;
        require(
            self.schema == 1
                && op.request.is_none()
                && op.scoped_request.as_ref() == Some(&self.request),
            "scoped response differs from original intent",
        )?;
        self.request.validate(&op.intent)?;
        if let Some(bill) = &self.billing {
            bill.validate()?;
        }
        if let Some(observation) = &self.observation {
            DecisionJournal::validate_observation(&op.intent, observation)?;
            if let Some(bill) = &self.billing {
                require(
                    observation.response_id == bill.response_id
                        && observation.actual_model == bill.actual_model
                        && observation.input_tokens
                            == bill.usage.input + bill.usage.cache_read + bill.usage.cache_write
                        && observation.output_tokens == bill.usage.output,
                    "scoped decision and billing disagree",
                )?;
            }
        }
        Ok(())
    }
}

/// Public proof of a completed original settlement, not permission to send.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopedFeeAck {
    pub schema: u32,
    pub operation_id: String,
    pub response_head: String,
    pub settlement_head: String,
    pub settlement_sha256: String,
}
impl ScopedFeeAck {
    pub(crate) fn validate(&self) -> Result<(), String> {
        require(self.schema == 1, "unsupported scoped fee ACK")?;
        identifier(&self.operation_id)?;
        for hash in [
            &self.response_head,
            &self.settlement_head,
            &self.settlement_sha256,
        ] {
            sha256(hash)?;
        }
        Ok(())
    }
}
pub(super) fn require_guard(root: &Path) -> Result<(), String> {
    crate::currency_scoped_execution_guard::require_protected(
        root.parent().ok_or("management root missing")?,
        true,
    )
}
pub(super) fn validate_operation(op: &Operation) -> Result<(), String> {
    require(
        op.request.is_none() && op.transport.is_none(),
        "scoped journal mixed legacy wire evidence",
    )?;
    op.scoped_request
        .as_ref()
        .ok_or("scoped intent missing")?
        .validate(&op.intent)?;
    if let Some(response) = &op.scoped_response {
        response.validate(op)?;
        require(
            digest(&response.observation)? == digest(&op.observation)?,
            "scoped observation differs from original response",
        )?;
    } else {
        require(op.observation.is_none(), "scoped response missing")?;
    }
    Ok(())
}
pub(super) fn validate_ack(op: &Operation, ack: Option<&ScopedFeeAck>) -> Result<(), String> {
    if op.scoped_request.is_some() {
        let ack = ack.ok_or("scoped decision requires original fee ACK before application")?;
        ack.validate()?;
        require(
            ack.operation_id == op.intent.operation_id
                && op.response_hash.as_ref() == Some(&ack.response_head),
            "scoped fee ACK belongs to another response",
        )?;
        validate_operation(op)?;
    } else {
        require(
            ack.is_none(),
            "legacy decision cannot acquire a scoped fee ACK",
        )?;
    }
    Ok(())
}
impl DecisionJournal {
    pub(crate) fn begin_scoped(
        &mut self,
        intent: DecisionIntent,
        prepared: &PreparedScopedDecision,
    ) -> Result<(), String> {
        prepared.check_ready()?;
        let binding = ScopedIntentBinding {
            request: prepared.audit().request.clone(),
            pricing_review_sha256: digest(prepared.audit())?,
        };
        self.begin_with_binding(intent, None, Some(binding))
    }
    pub(crate) fn finish_scoped_result(
        &mut self,
        id: &str,
        outcome: Result<ScopedDecisionResult, String>,
    ) -> Result<(), String> {
        let op = self
            .operations
            .get(id)
            .ok_or("original scoped intent missing")?;
        require(
            op.scoped_request.is_some() && !op.finished && op.resolution.is_none(),
            "scoped outcome already recorded or not expected",
        )?;
        require_guard(&self.root)?;
        let response = match outcome {
            Err(_) => None,
            Ok(result) => {
                let mut response = ScopedResponseRecord {
                    schema: 1,
                    request: ScopedIntentBinding {
                        request: result.request,
                        pricing_review_sha256: result.review_sha256,
                    },
                    response_sha256: result.response_sha256,
                    observation: result
                        .decision
                        .ok()
                        .filter(|o| Self::validate_observation(&op.intent, o).is_ok()),
                    billing: result.billing.ok().filter(|b| b.validate().is_ok()),
                };
                // Keep billable usage even if a malformed decision disagrees.
                let observation = response.observation.take();
                response.validate(op)?;
                response.observation = observation;
                if response.validate(op).is_err() {
                    response.observation = None;
                }
                response.validate(op)?;
                Some(response)
            }
        };
        let evidence = response
            .clone()
            .map_or(Evidence::Unknown, |r| Evidence::ScopedResponse {
                response: Box::new(r),
            });
        let hash = self.publish(id, 1, op.chain_hash.clone(), evidence)?;
        let op = self.operations.get_mut(id).unwrap();
        op.observation = response.as_ref().and_then(|r| r.observation.clone());
        op.scoped_response = response;
        op.finished = true;
        op.response_hash = Some(hash.clone());
        op.chain_hash = hash;
        Ok(())
    }
    pub(crate) fn is_scoped(&self, id: &str) -> bool {
        self.operations
            .get(id)
            .is_some_and(|op| op.scoped_request.is_some())
    }
    pub(crate) fn scoped_application_ack(&self, id: &str) -> Option<&ScopedFeeAck> {
        self.operations.get(id).and_then(|op| op.fee_ack.as_ref())
    }
}
