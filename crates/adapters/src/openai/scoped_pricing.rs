//! Scoped review and a separate single-use counted transport. Review objects
//! alone cannot send; transport requires a fresh live count of the exact body.
//! Neither grants user approval or authority to change a ledger.
mod transport;
use super::*;
use crate::api_generation::ApiUsage;
use cerulean_domain::{
    api_worker::ApiTarget,
    credentials::CredentialProvider,
    currency_budget::scoped::{
        InputRange, PricingObservation, PricingRequest, RequestedService, ScopedPriceQuote,
    },
    execution::digest,
};
#[cfg(feature = "test-fixtures")]
pub use transport::{
    fixture_counted_scoped_decision, fixture_expired_scoped_count, fixture_scoped_result,
};

/// Shared official Responses billing normalization for management and workers.
/// Parsing public billing fields does not grant tool or execution permission.
pub(crate) fn decode_billing(value: &Value) -> Result<ScopedBillingObservation, String> {
    transport::billing(value)
}
pub use transport::{
    CountedScopedDecision, PreparedScopedDecision, ScopedBillingObservation, ScopedDecisionResult,
    ScopedTokenCount,
};

/// Public identity only; it cannot recreate context, a count proof, or a request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopedDecisionFingerprint {
    pub schema: u32,
    pub url: String,
    pub purpose: Purpose,
    pub requested_model: String,
    pub requested_service: RequestedService,
    pub state_digest: String,
    pub body_sha256: String,
    pub count_url: String,
    pub count_body_sha256: String,
    pub count_body_bytes: usize,
    pub max_output_tokens: u32,
}
impl ScopedDecisionFingerprint {
    /// Archived identity validation never reconstructs the private request.
    pub fn validate(&self) -> Result<(), String> {
        sha256(&self.state_digest)?;
        sha256(&self.body_sha256)?;
        sha256(&self.count_body_sha256)?;
        model_id(&self.requested_model)?;
        require(
            self.schema == 1
                && self.url == format!("{ENDPOINT}/responses")
                && self.count_url == format!("{ENDPOINT}/responses/input_tokens")
                && self.requested_service
                    == RequestedService::Explicit {
                        tier: "default".into(),
                    }
                && (1..=256 * 1024).contains(&self.count_body_bytes)
                && self.max_output_tokens == 4096,
            "invalid scoped management identity",
        )
    }
}

/// Holds the exact new body in memory. No Debug/Serialize/Clone or transport.
pub struct ScopedDecisionDraft {
    body: Value,
    fingerprint: ScopedDecisionFingerprint,
}
impl ScopedDecisionDraft {
    /// First adapter contract is official Responses, explicit standard service,
    /// tool-free management. No project-default, remote tool, or session pricing.
    pub fn standard(
        purpose: Purpose,
        model: &str,
        state: &str,
        input: &Value,
    ) -> Result<Self, String> {
        let mut body = body(purpose, model, state, input)?;
        let object = body
            .as_object_mut()
            .ok_or("invalid scoped management body")?;
        require(
            object.len() == 10 && !object.contains_key("service_tier"),
            "management template changed; scoped projection requires review",
        )?;
        object.insert("service_tier".into(), json!("default"));
        let projected = count_projection(&body)?;
        let fingerprint = ScopedDecisionFingerprint {
            schema: 1,
            url: format!("{ENDPOINT}/responses"),
            purpose,
            requested_model: model.into(),
            requested_service: RequestedService::Explicit {
                tier: "default".into(),
            },
            state_digest: state.into(),
            body_sha256: digest(&body)?,
            count_url: format!("{ENDPOINT}/responses/input_tokens"),
            count_body_sha256: digest(&projected)?,
            count_body_bytes: serde_json::to_vec(&projected)
                .map_err(|_| "invalid scoped count body")?
                .len(),
            max_output_tokens: 4096,
        };
        fingerprint.validate()?;
        Ok(Self { body, fingerprint })
    }

    pub fn fingerprint(&self) -> &ScopedDecisionFingerprint {
        &self.fingerprint
    }

    fn checked_count_body(&self) -> Result<Value, String> {
        self.fingerprint.validate()?;
        let projected = count_projection(&self.body)?;
        require(
            digest(&self.body)? == self.fingerprint.body_sha256
                && digest(&projected)? == self.fingerprint.count_body_sha256
                && serde_json::to_vec(&projected)
                    .map_err(|_| "invalid count body")?
                    .len()
                    == self.fingerprint.count_body_bytes,
            "scoped management request changed",
        )?;
        Ok(projected)
    }

    /// Calculates a conditional liability under the supplied interval. The
    /// interval is NOT a trusted count or proof of a provider's upper bound.
    pub fn review(
        &self,
        quote: &ScopedPriceQuote,
        input: InputRange,
        at: &str,
    ) -> Result<ScopedPricingReview, String> {
        self.checked_count_body()?;
        let request = PricingRequest {
            target: ApiTarget::OpenAi {},
            requested_model: self.fingerprint.requested_model.clone(),
            requested_service: self.fingerprint.requested_service.clone(),
            input,
            output_ceiling: u64::from(self.fingerprint.max_output_tokens),
            cache_read_allowed: true,
            cache_write_allowed: true,
        };
        let maximum = quote.reserve(&request, at)?;
        Ok(ScopedPricingReview {
            audit: ScopedPricingAudit {
                request: self.fingerprint.clone(),
                quote_sha256: digest(quote)?,
                pricing_request_sha256: digest(&request)?,
                reviewed_at: at.into(),
                maximum_usd_nanos: maximum,
            },
            quote: quote.clone(),
            request,
        })
    }
}

fn count_projection(body: &Value) -> Result<Value, String> {
    let mut object = body
        .as_object()
        .ok_or("invalid scoped management body")?
        .clone();
    require(
        object.len() == 11
            && object.get("service_tier") == Some(&json!("default"))
            && object.get("store") == Some(&json!(false))
            && object.get("stream") == Some(&json!(false))
            && object.get("max_output_tokens") == Some(&json!(4096))
            && object.get("tools") == Some(&json!([]))
            && object.get("tool_choice") == Some(&json!("none"))
            && object.get("parallel_tool_calls") == Some(&json!(false)),
        "scoped management body exceeds reviewed count projection",
    )?;
    // Service tier selects processing, not context. It is not forwarded to the
    // count endpoint. The new generation digest still binds its explicit value.
    for field in ["service_tier", "store", "stream", "max_output_tokens"] {
        object.remove(field);
    }
    let fields = [
        "model",
        "instructions",
        "input",
        "text",
        "tools",
        "tool_choice",
        "parallel_tool_calls",
    ];
    require(
        object.len() == fields.len() && fields.iter().all(|field| object.contains_key(*field)),
        "unreviewed scoped count fields",
    )?;
    Ok(Value::Object(object))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopedPricingAudit {
    pub request: ScopedDecisionFingerprint,
    pub quote_sha256: String,
    pub pricing_request_sha256: String,
    pub reviewed_at: String,
    pub maximum_usd_nanos: u64,
}

/// Owns the original quote and interval so later evaluation cannot swap rates.
/// This is an analysis object, not a booked reservation or a send capability.
pub struct ScopedPricingReview {
    audit: ScopedPricingAudit,
    quote: ScopedPriceQuote,
    request: PricingRequest,
}
impl ScopedPricingReview {
    pub fn audit(&self) -> &ScopedPricingAudit {
        &self.audit
    }

    /// Review supplied response JSON; no transport provenance is implied.
    /// Billable counters and management decision validity are independent: a
    /// rejected decision may still have incurred a charge. Nothing is applied.
    pub fn evaluate_response(&self, value: &Value) -> Result<ScopedPricingEvaluation, String> {
        require(
            serde_json::to_vec(value)
                .map_err(|_| "invalid pricing response")?
                .len()
                <= RESPONSE_LIMIT,
            "pricing response exceeds limit",
        )?;
        let decision_usable = parse_decision(
            value.clone(),
            self.audit.request.purpose,
            &self.audit.request.requested_model,
            &self.audit.request.state_digest,
        )
        .is_ok();
        let amount = self.price_response(value);
        Ok(ScopedPricingEvaluation {
            review_sha256: digest(&self.audit)?,
            response_sha256: digest(value)?,
            decision_usable,
            result: match amount {
                Ok(amount_usd_nanos) => ScopedPricingResult::Priced { amount_usd_nanos },
                Err(reason) => ScopedPricingResult::Unpriced { reason },
            },
        })
    }

    fn price_response(&self, value: &Value) -> Result<u64, String> {
        let observed = transport::billing(value)?;
        self.quote.charge(
            &self.request,
            &PricingObservation {
                actual_model: observed.actual_model,
                actual_service: observed.actual_service,
                usage: observed.usage,
            },
        )
    }
}

fn counter(value: &Value) -> Result<Option<u64>, String> {
    if value.is_null() {
        Ok(None)
    } else {
        value
            .as_u64()
            .map(Some)
            .ok_or("invalid pricing usage counter".into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ScopedPricingResult {
    Priced { amount_usd_nanos: u64 },
    Unpriced { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopedPricingEvaluation {
    pub review_sha256: String,
    pub response_sha256: String,
    pub decision_usable: bool,
    pub result: ScopedPricingResult,
}

#[cfg(test)]
mod tests;
