//! A scoped round keeps public billing independently of usable worker output.
//! Reply/continuation remains private until the original ledger supplies a fee permit.
use super::*;
use crate::currency_budget::{ApiFeePermit, ApiPreparationPermit};
use cerulean_adapters::{
    api_generation::scoped::{PreparedScopedApi, ScopedApiFingerprint, ScopedApiResult},
    openai::scoped_pricing::ScopedBillingObservation,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiScopedBinding {
    pub request: ScopedApiFingerprint,
    pub preparation_sha256: String,
    pub pricing_review_sha256: String,
}
impl ApiScopedBinding {
    pub(crate) fn validate(&self, call: &CallIdentity) -> Result<(), String> {
        call.validate()?;
        self.request.validate()?;
        sha256(&self.preparation_sha256)?;
        sha256(&self.pricing_review_sha256)?;
        require(
            self.request.request == call.fingerprint,
            "scoped API intent differs from original request",
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiScopedResponse {
    pub schema: u32,
    pub binding: ApiScopedBinding,
    pub response_sha256: String,
    pub observation: Option<ApiObservation>,
    pub billing: Option<ScopedBillingObservation>,
}
impl ApiScopedResponse {
    fn validate(&self, call: &CallIdentity, binding: &ApiScopedBinding) -> Result<(), String> {
        sha256(&self.response_sha256)?;
        self.binding.validate(call)?;
        require(
            self.schema == 1 && &self.binding == binding,
            "scoped API response binding changed",
        )?;
        if let Some(bill) = &self.billing {
            bill.validate()?;
        }
        if let Some(observation) = &self.observation {
            observation.validate()?;
            if let Some(bill) = &self.billing {
                require(
                    observation.response_id == bill.response_id
                        && observation.actual_model == bill.actual_model
                        && observation
                            .usage
                            .billable_usage(call.fingerprint.target.provider())?
                            == bill.usage,
                    "API reply and independent billing disagree",
                )?;
            }
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(super) enum ScopedCallOutcome {
    Response { record: Box<ApiScopedResponse> },
    Failure { failure: ApiFailure },
}
/// Archived proof, not a capability. Only the ledger can construct ApiFeePermit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiFeeAck {
    pub schema: u32,
    pub request_id: String,
    pub response_head: String,
    pub settlement_head: String,
    pub settlement_sha256: String,
}
impl ApiFeeAck {
    pub(crate) fn validate(&self) -> Result<(), String> {
        require(self.schema == 1, "unsupported API fee ACK")?;
        identifier(&self.request_id)?;
        for h in [
            &self.response_head,
            &self.settlement_head,
            &self.settlement_sha256,
        ] {
            sha256(h)?;
        }
        Ok(())
    }
}
/// No Serialize/Deserialize/Clone/Debug; re-opening cannot reconstruct a reply
/// with private continuation or obtain permission to repeat a tool.
pub struct PendingScopedReply {
    identity_digest: String,
    response_head: String,
    reply: Result<ApiReply, ApiFailure>,
}
fn data_root(root: &Path) -> Result<&Path, String> {
    let rounds = root
        .parent()
        .filter(|p| p.file_name().is_some_and(|n| n == "rounds"))
        .ok_or("scoped API journal requires its original rounds directory")?;
    let attempt = rounds.parent().ok_or("API attempt missing")?;
    require(
        attempt
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("api-attempt-")),
        "API attempt path invalid",
    )?;
    let workflows = attempt
        .parent()
        .and_then(Path::parent)
        .filter(|p| p.file_name().is_some_and(|n| n == "workflows"))
        .ok_or("scoped API journal requires original workflow")?;
    workflows.parent().ok_or("API data root missing".into())
}
pub(super) fn require_guard(root: &Path) -> Result<(), String> {
    crate::currency_api_guard::require_protected(data_root(root)?, true)
}
pub(super) fn require_event(root: &Path, event: &Event) -> Result<(), String> {
    if matches!(
        event,
        Event::ScopedIntent { .. } | Event::ScopedOutcome { .. } | Event::FeeAck { .. }
    ) {
        require_guard(root)?;
    }
    Ok(())
}
pub(super) fn advance(
    state: &mut CallState,
    expected: &CallIdentity,
    event: &Event,
) -> Result<(), String> {
    match event {
        Event::ScopedIntent { identity, binding } => {
            require(
                !state.intent_recorded && identity == expected,
                "scoped API intent repeated or changed",
            )?;
            binding.validate(expected)?;
            state.intent_recorded = true;
            state.scoped_request = Some(binding.as_ref().clone());
        }
        Event::ScopedOutcome { outcome } => {
            let binding = state
                .scoped_request
                .as_ref()
                .ok_or("original scoped API intent missing")?;
            require(
                state.intent_recorded && state.outcome.is_none(),
                "scoped API outcome repeated or out of order",
            )?;
            match outcome.as_ref() {
                ScopedCallOutcome::Response { record } => {
                    record.validate(expected, binding)?;
                    state.outcome = Some(match &record.observation {
                        Some(observation) => CallOutcome::Reply {
                            observation: observation.clone(),
                        },
                        None => CallOutcome::Failure {
                            failure: ApiFailure::InvalidResponse {},
                        },
                    });
                    state.scoped_response = Some(record.clone());
                }
                ScopedCallOutcome::Failure { failure } => {
                    if let ApiFailure::Http { status } = failure {
                        require(
                            (100..=599).contains(status) && !(200..=299).contains(status),
                            "API HTTP failure invalid",
                        )?;
                    }
                    state.outcome = Some(CallOutcome::Failure {
                        failure: failure.clone(),
                    });
                }
            }
        }
        Event::FeeAck { ack } => {
            ack.validate()?;
            require(
                state.scoped_request.is_some()
                    && state.scoped_response.is_some()
                    && state.fee_ack.is_none()
                    && ack.request_id == expected.request_id
                    && state.response_head.as_deref() == Some(ack.response_head.as_str()),
                "API fee ACK differs from original response or is repeated",
            )?;
            state.fee_ack = Some(ack.clone());
        }
        _ => return Err("not a scoped API event".into()),
    }
    Ok(())
}
impl ApiCallJournal {
    pub(crate) fn require_original_path(&self, path: &Path) -> Result<(), String> {
        require(
            fs::canonicalize(&self.root).map_err(|e| e.to_string())?
                == fs::canonicalize(path).map_err(|e| e.to_string())?,
            "API journal is outside its original round path",
        )
    }
    pub(crate) fn require_account_root(&self, root: &Path) -> Result<(), String> {
        require(
            fs::canonicalize(data_root(&self.root)?).map_err(|e| e.to_string())?
                == fs::canonicalize(root).map_err(|e| e.to_string())?,
            "API journal and monetary account belong to different data roots",
        )
    }
    pub async fn execute_scoped(
        &mut self,
        prepared: PreparedScopedApi,
        permit: ApiPreparationPermit,
        cancel: &mut tokio::sync::oneshot::Receiver<()>,
    ) -> Result<PendingScopedReply, String> {
        self.execute_scoped_with(prepared, permit, |p| async { p.send(cancel).await })
            .await
    }
    pub(crate) async fn execute_scoped_with<
        F: Future<Output = Result<ScopedApiResult, ApiFailure>>,
    >(
        &mut self,
        prepared: PreparedScopedApi,
        permit: ApiPreparationPermit,
        send: impl FnOnce(PreparedScopedApi) -> F,
    ) -> Result<PendingScopedReply, String> {
        require(
            !self.read_only && !self.poisoned && !self.state.intent_recorded,
            "API round already attempted; never resend",
        )?;
        prepared.check_ready()?;
        self.require_account_root(permit.data_root())?;
        let binding = permit.binding().clone();
        require(
            permit.call() == &self.expected
                && binding.request == prepared.audit().request
                && binding.pricing_review_sha256 == digest(prepared.audit())?,
            "API generation differs from original monetary preparation",
        )?;
        self.publish(
            &Event::ScopedIntent {
                identity: self.expected.clone(),
                binding: Box::new(binding.clone()),
            },
            |_| Ok(()),
        )?;
        let result = send(prepared).await;
        let outcome = match &result {
            Ok(result) => {
                require(
                    result.request == binding.request
                        && result.review_sha256 == binding.pricing_review_sha256,
                    "API result differs from original pricing review",
                )?;
                ScopedCallOutcome::Response {
                    record: Box::new(ApiScopedResponse {
                        schema: 1,
                        binding,
                        response_sha256: result.response_sha256.clone(),
                        observation: result.reply.as_ref().ok().map(|v| v.observation().clone()),
                        billing: result.billing.as_ref().ok().cloned(),
                    }),
                }
            }
            Err(failure) => ScopedCallOutcome::Failure {
                failure: failure.clone(),
            },
        };
        self.publish(
            &Event::ScopedOutcome {
                outcome: Box::new(outcome),
            },
            |_| Ok(()),
        )?;
        Ok(PendingScopedReply {
            identity_digest: self.identity_digest.clone(),
            response_head: self
                .state
                .response_head
                .clone()
                .ok_or("API response publication missing")?,
            reply: result.and_then(|r| r.reply),
        })
    }
    /// Persist the original settlement ACK before exposing any usable reply.
    pub fn release_scoped_reply(
        &mut self,
        pending: PendingScopedReply,
        permit: ApiFeePermit,
    ) -> Result<Result<ApiReply, ApiFailure>, String> {
        self.require_account_root(permit.data_root())?;
        require(
            pending.identity_digest == self.identity_digest
                && self.state.response_head.as_deref() == Some(pending.response_head.as_str()),
            "pending API reply belongs to another round",
        )?;
        self.publish(
            &Event::FeeAck {
                ack: permit.proof().clone(),
            },
            |_| Ok(()),
        )?;
        Ok(pending.reply)
    }
    /// Reopened evidence can verify a persisted ACK, but cannot reconstruct the
    /// in-memory pending reply or run tools that did not execute before exit.
    pub(crate) fn require_fee_ack(&self) -> Result<(), String> {
        require(
            self.healthy() && (self.state.scoped_request.is_none() || self.state.fee_ack.is_some()),
            "scoped API result requires its original durable fee ACK",
        )
    }
}
