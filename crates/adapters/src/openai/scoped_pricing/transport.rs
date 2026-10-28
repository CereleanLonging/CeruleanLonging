//! Exactly one official count and then one explicit-tier generation. The Host
//! must persist count liability BEFORE count, generation reservation/intent
//! BEFORE decide, and billing evidence BEFORE applying the decision. These
//! transports do not perform those Host duties or establish supplier guarantees.
use super::*;
use cerulean_domain::currency_budget::{Usage, MAX_TOKENS};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const MAX_AGE: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopedTokenCount {
    pub schema: u32,
    pub request: ScopedDecisionFingerprint,
    pub quote_sha256: String,
    pub response_sha256: String,
    pub observed_unix_ms: u64,
    pub input_tokens: u64,
}
impl ScopedTokenCount {
    /// An audit receipt is never a deserializable transport capability.
    pub fn validate(&self, request: &ScopedDecisionFingerprint, quote: &str) -> Result<(), String> {
        request.validate()?;
        sha256(quote)?;
        sha256(&self.response_sha256)?;
        require(
            self.schema == 1
                && &self.request == request
                && self.quote_sha256 == quote
                && self.observed_unix_ms > 0
                && (1..=MAX_TOKENS).contains(&self.input_tokens)
                && self.response_sha256
                    == digest(
                        &json!({"object":"response.input_tokens","input_tokens":self.input_tokens}),
                    )?,
            "invalid archived scoped count",
        )
    }
}

/// No Clone/Deserialize/public constructor: owns the exact request, original
/// quote, and authenticated client that received the count. Archive cannot send.
pub struct CountedScopedDecision {
    client: OpenAiClient,
    draft: ScopedDecisionDraft,
    quote: ScopedPriceQuote,
    evidence: ScopedTokenCount,
    observed: Instant,
}
impl CountedScopedDecision {
    pub fn evidence(&self) -> &ScopedTokenCount {
        &self.evidence
    }
    pub fn fingerprint(&self) -> &ScopedDecisionFingerprint {
        self.draft.fingerprint()
    }

    fn review_now(&self) -> Result<ScopedPricingReview, String> {
        require(
            self.observed.elapsed() < MAX_AGE,
            "scoped count expired; no generation sent",
        )?;
        self.evidence
            .validate(self.draft.fingerprint(), &digest(&self.quote)?)?;
        // Actual billable input may be smaller than the observed count. Do not
        // remove low-input price bands even if those bands cost more.
        self.draft.review(
            &self.quote,
            InputRange {
                from: 0,
                through: self.evidence.input_tokens,
            },
            &chrono::Utc::now().to_rfc3339(),
        )
    }

    /// Caller records the successful count even if this pricing step fails.
    /// Success does not release the independently reserved count fee.
    pub fn prepare(self) -> Result<PreparedScopedDecision, String> {
        let review = self.review_now()?;
        Ok(PreparedScopedDecision {
            counted: self,
            review,
        })
    }
}

pub struct PreparedScopedDecision {
    counted: CountedScopedDecision,
    review: ScopedPricingReview,
}
impl PreparedScopedDecision {
    pub fn evidence(&self) -> &ScopedTokenCount {
        self.counted.evidence()
    }
    pub fn audit(&self) -> &ScopedPricingAudit {
        self.review.audit()
    }
    pub fn pricing_request(&self) -> &PricingRequest {
        &self.review.request
    }
    pub fn quote(&self) -> &ScopedPriceQuote {
        &self.review.quote
    }

    /// Check immediately before durable preparation and again before transport.
    /// The Host must separately authenticate the current approval and account.
    pub fn check_ready(&self) -> Result<(), String> {
        let current = self.counted.review_now()?;
        require(
            current.request == self.review.request
                && current.quote == self.review.quote
                && current.audit.maximum_usd_nanos == self.review.audit.maximum_usd_nanos
                && current.audit.request == self.review.audit.request,
            "scoped count or original pricing changed",
        )
    }

    /// Consumed once. Caller must have durably recorded reservation and intent;
    /// this method does not grant execution permission or retry on failure.
    pub async fn decide(self) -> Result<ScopedDecisionResult, String> {
        self.decide_url(&format!("{ENDPOINT}/responses")).await
    }
    async fn decide_url(self, url: &str) -> Result<ScopedDecisionResult, String> {
        self.check_ready()?;
        let response = self
            .counted
            .client
            .send_url(reqwest::Method::POST, url, Some(self.counted.draft.body))
            .await?;
        decode(&self.review, response)
    }
}

impl OpenAiClient {
    /// One count POST, no generation. Explicit count terms and the original
    /// quote must already be approved/reserved by the Host; they are not proof
    /// that a supplier guarantees this fee or its returned token bound.
    pub async fn count_scoped_decision(
        self,
        draft: ScopedDecisionDraft,
        quote: ScopedPriceQuote,
    ) -> Result<CountedScopedDecision, String> {
        self.count_scoped_url(draft, quote, &format!("{ENDPOINT}/responses/input_tokens"))
            .await
    }
    async fn count_scoped_url(
        self,
        draft: ScopedDecisionDraft,
        quote: ScopedPriceQuote,
        url: &str,
    ) -> Result<CountedScopedDecision, String> {
        let body = draft.checked_count_body()?;
        validate_count_terms(&draft, &quote)?;
        let response = self
            .send_url(reqwest::Method::POST, url, Some(body))
            .await?;
        counted(self, draft, quote, response)
    }
}

fn validate_count_terms(
    draft: &ScopedDecisionDraft,
    quote: &ScopedPriceQuote,
) -> Result<(), String> {
    quote.valid_at(&chrono::Utc::now().to_rfc3339())?;
    let price = quote
        .count_price
        .as_ref()
        .ok_or("scoped count fee is unknown; no count sent")?;
    require(
        quote.requested_model == draft.fingerprint.requested_model
            && quote.requested_service == draft.fingerprint.requested_service
            && matches!(quote.target, ApiTarget::OpenAi {})
            && draft.fingerprint.count_body_bytes <= price.maximum_body_bytes as usize,
        "scoped count differs from approved quote or request size",
    )
}

fn counted(
    client: OpenAiClient,
    draft: ScopedDecisionDraft,
    quote: ScopedPriceQuote,
    response: Value,
) -> Result<CountedScopedDecision, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct CountResponse {
        object: String,
        input_tokens: u64,
    }
    let decoded: CountResponse =
        serde_json::from_value(response.clone()).map_err(|_| "invalid scoped input token count")?;
    require(
        decoded.object == "response.input_tokens"
            && (1..=MAX_TOKENS).contains(&decoded.input_tokens),
        "invalid scoped input token count",
    )?;
    draft.checked_count_body()?;
    let evidence = ScopedTokenCount {
        schema: 1,
        request: draft.fingerprint.clone(),
        quote_sha256: digest(&quote)?,
        response_sha256: digest(&response)?,
        observed_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "invalid count time")?
            .as_millis()
            .try_into()
            .map_err(|_| "count time overflow")?,
        input_tokens: decoded.input_tokens,
    };
    evidence.validate(draft.fingerprint(), &digest(&quote)?)?;
    Ok(CountedScopedDecision {
        client,
        draft,
        quote,
        evidence,
        observed: Instant::now(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopedBillingObservation {
    pub response_id: String,
    pub actual_model: String,
    /// Missing is retained as unknown, never copied from the requested tier.
    pub actual_service: Option<String>,
    pub usage: Usage,
}
impl ScopedBillingObservation {
    /// Validate public billing data without treating missing service as default.
    pub fn validate(&self) -> Result<(), String> {
        require(
            self.response_id.starts_with("resp_")
                && self.response_id.len() <= 200
                && self
                    .response_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_'),
            "invalid scoped billing response ID",
        )?;
        model_id(&self.actual_model)?;
        if let Some(service) = &self.actual_service {
            require(
                !service.is_empty()
                    && service.len() <= 64
                    && service
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)),
                "invalid scoped billing service",
            )?;
        }
        self.usage.validate()
    }
}

/// No raw response/private reasoning retained. Billing and decision validity
/// remain independent; rejected/unfinished decisions may still incur charges.
pub struct ScopedDecisionResult {
    pub request: ScopedDecisionFingerprint,
    pub response_sha256: String,
    pub review_sha256: String,
    pub decision: Result<DecisionObservation, String>,
    pub billing: Result<ScopedBillingObservation, String>,
    pub pricing: ScopedPricingResult,
}
pub(super) fn billing(value: &Value) -> Result<ScopedBillingObservation, String> {
    let id = value["id"].as_str().ok_or("billing response ID missing")?;
    require(
        id.starts_with("resp_")
            && id.len() <= 200
            && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
        "invalid billing response ID",
    )?;
    let model = value["model"].as_str().ok_or("billing model missing")?;
    model_id(model)?;
    let actual_service = match &value["service_tier"] {
        Value::Null => None,
        Value::String(s)
            if !s.is_empty()
                && s.len() <= 64
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)) =>
        {
            Some(s.clone())
        }
        _ => return Err("invalid actual service tier".into()),
    };
    let usage = ApiUsage {
        input_tokens: counter(&value["usage"]["input_tokens"])?
            .ok_or("billing input usage missing")?,
        output_tokens: counter(&value["usage"]["output_tokens"])?
            .ok_or("billing output usage missing")?,
        cache_read_tokens: counter(&value["usage"]["input_tokens_details"]["cached_tokens"])?,
        cache_write_tokens: counter(&value["usage"]["input_tokens_details"]["cache_write_tokens"])?,
    }
    .billable_usage(CredentialProvider::OpenAi)?;
    Ok(ScopedBillingObservation {
        response_id: id.into(),
        actual_model: model.into(),
        actual_service,
        usage,
    })
}
fn decode(review: &ScopedPricingReview, value: Value) -> Result<ScopedDecisionResult, String> {
    require(
        serde_json::to_vec(&value)
            .map_err(|_| "invalid scoped response")?
            .len()
            <= RESPONSE_LIMIT,
        "scoped response exceeds limit",
    )?;
    let response_sha256 = digest(&value)?;
    let billing = billing(&value);
    let price = billing.as_ref().map_err(Clone::clone).and_then(|bill| {
        review.quote.charge(
            &review.request,
            &PricingObservation {
                actual_model: bill.actual_model.clone(),
                actual_service: bill.actual_service.clone(),
                usage: bill.usage.clone(),
            },
        )
    });
    let pricing = match price {
        Ok(amount_usd_nanos) => ScopedPricingResult::Priced { amount_usd_nanos },
        Err(reason) => ScopedPricingResult::Unpriced { reason },
    };
    let request = review.audit.request.clone();
    let decision = parse_decision(
        value,
        request.purpose,
        &request.requested_model,
        &request.state_digest,
    );
    Ok(ScopedDecisionResult {
        request,
        response_sha256,
        review_sha256: digest(&review.audit)?,
        decision,
        billing,
        pricing,
    })
}

#[cfg(test)]
mod tests;

/// Controlled cross-crate response, decoded by the original transport parser.
#[cfg(feature = "test-fixtures")]
pub fn fixture_scoped_result(
    prepared: &PreparedScopedDecision,
    value: Value,
) -> Result<ScopedDecisionResult, String> {
    decode(&prepared.review, value)
}

#[cfg(feature = "test-fixtures")]
pub fn fixture_expired_scoped_count(
    draft: ScopedDecisionDraft,
    quote: ScopedPriceQuote,
    tokens: u64,
) -> Result<CountedScopedDecision, String> {
    let mut counted = fixture_counted_scoped_decision(draft, quote, tokens)?;
    counted.observed = Instant::now() - MAX_AGE;
    Ok(counted)
}

/// Synthetic cross-crate receipt only, absent from normal builds. No credential
/// lookup or HTTP; same decoder and original quote binding as the adapter.
#[cfg(feature = "test-fixtures")]
pub fn fixture_counted_scoped_decision(
    draft: ScopedDecisionDraft,
    quote: ScopedPriceQuote,
    input_tokens: u64,
) -> Result<CountedScopedDecision, String> {
    validate_count_terms(&draft, &quote)?;
    counted(
        OpenAiClient::new(SecretInput::new(
            "sk-synthetic-scoped-count-fixture".into(),
        )?)?,
        draft,
        quote,
        json!({"object":"response.input_tokens","input_tokens":input_tokens}),
    )
}
