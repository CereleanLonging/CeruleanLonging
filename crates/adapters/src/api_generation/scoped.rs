//! Exact worker-round count and single-use transport. Host must durably reserve
//! count fees, then generation fees and the original call intent, and settle
//! the original response before tools/next round. This layer grants none of
//! those permissions and cannot establish provider economic guarantees.
use super::*;
use crate::openai::scoped_pricing::{
    decode_billing, ScopedBillingObservation, ScopedPricingResult,
};
use cerulean_domain::{
    currency_budget::{
        scoped::{
            InputRange, PricingObservation, PricingRequest, RequestedService, ScopedPriceQuote,
        },
        MAX_TOKENS,
    },
    execution::sha256,
};
use serde_json::json;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const COUNT_URL: &str = "https://api.openai.com/v1/responses/input_tokens";
const MAX_AGE: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopedApiFingerprint {
    pub schema: u32,
    pub request: RequestFingerprint,
    pub requested_service: RequestedService,
    pub count_url: String,
    pub count_body_sha256: String,
    pub count_body_bytes: usize,
}
impl ScopedApiFingerprint {
    pub fn validate(&self) -> Result<(), String> {
        sha256(&self.request.body_sha256)?;
        sha256(&self.request.tool_catalog_sha256)?;
        sha256(&self.count_body_sha256)?;
        token(&self.request.requested_model)?;
        require(
            self.schema == 1
                && self
                    .request
                    .requested_model
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b))
                && self.request.target == (ApiTarget::OpenAi {})
                && self.requested_service
                    == (RequestedService::Explicit {
                        tier: "default".into(),
                    })
                && self.count_url == COUNT_URL
                && (1..=MAX_BODY).contains(&self.count_body_bytes)
                && (1..=16384).contains(&self.request.max_output_tokens)
                && (1..=600).contains(&self.request.timeout_seconds),
            "invalid scoped API identity",
        )
    }
}

/// Exact request/continuation remains private, never Debug/Serialize/Clone.
pub struct ScopedApiDraft {
    prepared: PreparedRequest,
    fingerprint: ScopedApiFingerprint,
}
impl ApiClient {
    /// Each round consumes its own draft and count/generation capability while
    /// retaining the session credential and HTTP configuration for later rounds.
    pub async fn count_scoped_round(
        &self,
        draft: ScopedApiDraft,
        quote: ScopedPriceQuote,
        cancel: &mut oneshot::Receiver<()>,
    ) -> Result<CountedScopedApi, ApiFailure> {
        let round_client = ApiClient {
            http: self.http.clone(),
            key: SecretInput::new(self.key.expose().to_owned())
                .map_err(|_| ApiFailure::NotSent {})?,
        };
        round_client.count_scoped_api(draft, quote, cancel).await
    }
    /// Scoped pricing currently has an official OpenAI contract only. Other
    /// providers keep their original nonmonetary path; no inferred tier mapping.
    pub fn prepare_scoped(&self, request: ApiRequest) -> Result<ScopedApiDraft, String> {
        require(
            request.target == (ApiTarget::OpenAi {}),
            "scoped API count is unsupported for this provider",
        )?;
        let mut prepared = self.prepare(request)?;
        let object = prepared.body.as_object_mut().ok_or("invalid API body")?;
        require(
            object.len() == 10 && !object.contains_key("service_tier"),
            "API template changed; review count projection",
        )?;
        object.insert("service_tier".into(), json!("default"));
        require(
            serde_json::to_vec(&prepared.body)
                .map_err(|_| "invalid API body")?
                .len()
                <= MAX_BODY,
            "scoped API body too large",
        )?;
        prepared.fingerprint.body_sha256 = digest(&prepared.body)?;
        let projected = count_projection(&prepared.body)?;
        let fingerprint = ScopedApiFingerprint {
            schema: 1,
            request: prepared.fingerprint.clone(),
            requested_service: RequestedService::Explicit {
                tier: "default".into(),
            },
            count_url: COUNT_URL.into(),
            count_body_sha256: digest(&projected)?,
            count_body_bytes: serde_json::to_vec(&projected)
                .map_err(|_| "invalid API count body")?
                .len(),
        };
        fingerprint.validate()?;
        Ok(ScopedApiDraft {
            prepared,
            fingerprint,
        })
    }
    /// Host records explicit count-fee liability first. Exactly one POST; the
    /// owned client is retained for generation so credentials cannot be swapped.
    pub async fn count_scoped_api(
        self,
        draft: ScopedApiDraft,
        quote: ScopedPriceQuote,
        cancel: &mut oneshot::Receiver<()>,
    ) -> Result<CountedScopedApi, ApiFailure> {
        self.count_scoped_url(draft, quote, cancel, COUNT_URL).await
    }
    async fn count_scoped_url(
        self,
        draft: ScopedApiDraft,
        quote: ScopedPriceQuote,
        cancel: &mut oneshot::Receiver<()>,
        url: &str,
    ) -> Result<CountedScopedApi, ApiFailure> {
        let body = draft
            .checked_count_body()
            .map_err(|_| ApiFailure::NotSent {})?;
        draft
            .validate_count_terms(&quote)
            .map_err(|_| ApiFailure::NotSent {})?;
        let response = self
            .send_json(
                &draft.fingerprint.request.target,
                &body,
                draft.fingerprint.request.timeout_seconds,
                cancel,
                url,
            )
            .await?;
        counted(self, draft, quote, response).map_err(|_| ApiFailure::InvalidResponse {})
    }
}
impl ScopedApiDraft {
    pub fn fingerprint(&self) -> &ScopedApiFingerprint {
        &self.fingerprint
    }
    fn checked_count_body(&self) -> Result<Value, String> {
        self.fingerprint.validate()?;
        self.prepared.request.validate()?;
        let projected = count_projection(&self.prepared.body)?;
        require(
            self.prepared.fingerprint == self.fingerprint.request
                && digest(&self.prepared.body)? == self.fingerprint.request.body_sha256
                && digest(&self.prepared.request.tools)?
                    == self.fingerprint.request.tool_catalog_sha256
                && digest(&projected)? == self.fingerprint.count_body_sha256
                && serde_json::to_vec(&projected)
                    .map_err(|_| "invalid API count body")?
                    .len()
                    == self.fingerprint.count_body_bytes,
            "scoped API request changed",
        )?;
        Ok(projected)
    }
    fn validate_count_terms(&self, quote: &ScopedPriceQuote) -> Result<(), String> {
        quote.valid_at(&chrono::Utc::now().to_rfc3339())?;
        let terms = quote.count_price.as_ref().ok_or("API count fee unknown")?;
        require(
            quote.target == self.fingerprint.request.target
                && quote.requested_model == self.fingerprint.request.requested_model
                && quote.requested_service == self.fingerprint.requested_service
                && self.fingerprint.count_body_bytes <= terms.maximum_body_bytes as usize,
            "API count differs from original quote or approved size",
        )
    }
    fn review(&self, quote: &ScopedPriceQuote, tokens: u64) -> Result<Review, String> {
        self.checked_count_body()?;
        let request = PricingRequest {
            target: self.fingerprint.request.target.clone(),
            requested_model: self.fingerprint.request.requested_model.clone(),
            requested_service: self.fingerprint.requested_service.clone(),
            input: InputRange {
                from: 0,
                through: tokens,
            },
            output_ceiling: self.fingerprint.request.max_output_tokens.into(),
            cache_read_allowed: true,
            cache_write_allowed: true,
        };
        let at = chrono::Utc::now().to_rfc3339();
        let amount = quote.reserve(&request, &at)?;
        Ok(Review {
            quote: quote.clone(),
            request: request.clone(),
            audit: ScopedApiPricingAudit {
                request: self.fingerprint.clone(),
                quote_sha256: digest(quote)?,
                pricing_request_sha256: digest(&request)?,
                reviewed_at: at,
                maximum_usd_nanos: amount,
            },
        })
    }
}
fn count_projection(body: &Value) -> Result<Value, String> {
    let mut object = body.as_object().ok_or("invalid scoped API body")?.clone();
    require(
        object.len() == 11
            && object.get("service_tier") == Some(&json!("default"))
            && object.get("stream") == Some(&json!(false))
            && object.get("store") == Some(&json!(false))
            && object.get("include") == Some(&json!(["reasoning.encrypted_content"]))
            && object.get("parallel_tool_calls") == Some(&json!(false)),
        "scoped API body exceeds reviewed count projection",
    )?;
    // Output/processing controls are excluded; input contains the ENTIRE private
    // continuation and tool outputs. Public evidence records hashes only.
    for name in [
        "service_tier",
        "stream",
        "store",
        "include",
        "max_output_tokens",
    ] {
        object.remove(name);
    }
    let fields = [
        "model",
        "instructions",
        "input",
        "tools",
        "tool_choice",
        "parallel_tool_calls",
    ];
    require(
        object.len() == fields.len() && fields.iter().all(|f| object.contains_key(*f)),
        "unknown API count fields",
    )?;
    Ok(Value::Object(object))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopedApiTokenCount {
    pub schema: u32,
    pub request: ScopedApiFingerprint,
    pub quote_sha256: String,
    pub response_sha256: String,
    pub observed_unix_ms: u64,
    pub input_tokens: u64,
}
impl ScopedApiTokenCount {
    pub fn validate(&self, request: &ScopedApiFingerprint, quote: &str) -> Result<(), String> {
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
            "invalid archived API count",
        )
    }
}
pub struct CountedScopedApi {
    client: ApiClient,
    draft: ScopedApiDraft,
    quote: ScopedPriceQuote,
    evidence: ScopedApiTokenCount,
    observed: Instant,
}
impl CountedScopedApi {
    pub fn evidence(&self) -> &ScopedApiTokenCount {
        &self.evidence
    }
    pub fn fingerprint(&self) -> &ScopedApiFingerprint {
        self.draft.fingerprint()
    }
    fn review_now(&self) -> Result<Review, String> {
        require(
            self.observed.elapsed() < MAX_AGE,
            "API count expired; generation not sent",
        )?;
        self.evidence
            .validate(self.fingerprint(), &digest(&self.quote)?)?;
        self.draft.review(&self.quote, self.evidence.input_tokens)
    }
    /// Preserve the successful count receipt even when pricing cannot prepare.
    pub fn prepare(self) -> Result<PreparedScopedApi, String> {
        let review = self.review_now()?;
        Ok(PreparedScopedApi {
            counted: self,
            review,
        })
    }
}
fn counted(
    client: ApiClient,
    draft: ScopedApiDraft,
    quote: ScopedPriceQuote,
    response: Value,
) -> Result<CountedScopedApi, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct CountResponse {
        object: String,
        input_tokens: u64,
    }
    let decoded: CountResponse =
        serde_json::from_value(response.clone()).map_err(|_| "invalid API input count")?;
    require(
        decoded.object == "response.input_tokens"
            && (1..=MAX_TOKENS).contains(&decoded.input_tokens),
        "invalid API count",
    )?;
    draft.checked_count_body()?;
    let evidence = ScopedApiTokenCount {
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
    Ok(CountedScopedApi {
        client,
        draft,
        quote,
        evidence,
        observed: Instant::now(),
    })
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopedApiPricingAudit {
    pub request: ScopedApiFingerprint,
    pub quote_sha256: String,
    pub pricing_request_sha256: String,
    pub reviewed_at: String,
    pub maximum_usd_nanos: u64,
}
struct Review {
    audit: ScopedApiPricingAudit,
    quote: ScopedPriceQuote,
    request: PricingRequest,
}
pub struct PreparedScopedApi {
    counted: CountedScopedApi,
    review: Review,
}
impl PreparedScopedApi {
    pub fn evidence(&self) -> &ScopedApiTokenCount {
        self.counted.evidence()
    }
    pub fn audit(&self) -> &ScopedApiPricingAudit {
        &self.review.audit
    }
    pub fn pricing_request(&self) -> &PricingRequest {
        &self.review.request
    }
    pub fn quote(&self) -> &ScopedPriceQuote {
        &self.review.quote
    }
    pub fn check_ready(&self) -> Result<(), String> {
        let fresh = self.counted.review_now()?;
        require(
            fresh.request == self.review.request
                && fresh.quote == self.review.quote
                && fresh.audit.request == self.review.audit.request
                && fresh.audit.maximum_usd_nanos == self.review.audit.maximum_usd_nanos,
            "original API count or pricing changed",
        )
    }
    /// Consumes the only live request capability. No retry, redirect or tier
    /// fallback. Host persists preparation and original intent before calling.
    pub async fn send(
        self,
        cancel: &mut oneshot::Receiver<()>,
    ) -> Result<ScopedApiResult, ApiFailure> {
        let url = self
            .counted
            .draft
            .fingerprint
            .request
            .target
            .url()
            .map_err(|_| ApiFailure::NotSent {})?;
        self.send_url(cancel, &url).await
    }
    async fn send_url(
        self,
        cancel: &mut oneshot::Receiver<()>,
        url: &str,
    ) -> Result<ScopedApiResult, ApiFailure> {
        self.check_ready().map_err(|_| ApiFailure::NotSent {})?;
        let prepared = &self.counted.draft.prepared;
        let value = self
            .counted
            .client
            .send_json(
                &prepared.request.target,
                &prepared.body,
                prepared.request.timeout_seconds,
                cancel,
                url,
            )
            .await?;
        decode(&self, value).map_err(|_| ApiFailure::InvalidResponse {})
    }
}
/// Reply may contain private in-memory continuation; this wrapper deliberately
/// has no Debug/Serialize. Billing remains observable when reply parsing fails.
pub struct ScopedApiResult {
    pub request: ScopedApiFingerprint,
    pub response_sha256: String,
    pub review_sha256: String,
    pub reply: Result<ApiReply, ApiFailure>,
    pub billing: Result<ScopedBillingObservation, String>,
    pub pricing: ScopedPricingResult,
}
fn decode(prepared: &PreparedScopedApi, value: Value) -> Result<ScopedApiResult, String> {
    require(
        !contains_secret(&value, prepared.counted.client.key.expose()),
        "API response contains credential",
    )?;
    require(
        serde_json::to_vec(&value)
            .map_err(|_| "invalid API response")?
            .len()
            <= MAX_RESPONSE,
        "API response too large",
    )?;
    let response_sha256 = digest(&value)?;
    let billing = decode_billing(&value);
    let amount = billing.as_ref().map_err(Clone::clone).and_then(|b| {
        prepared.review.quote.charge(
            &prepared.review.request,
            &PricingObservation {
                actual_model: b.actual_model.clone(),
                actual_service: b.actual_service.clone(),
                usage: b.usage.clone(),
            },
        )
    });
    let pricing = match amount {
        Ok(amount_usd_nanos) => ScopedPricingResult::Priced { amount_usd_nanos },
        Err(reason) => ScopedPricingResult::Unpriced { reason },
    };
    let reply = wire::parse(&prepared.counted.draft.prepared.request, value)
        .map_err(ApiFailure::response_rejected);
    Ok(ScopedApiResult {
        request: prepared.audit().request.clone(),
        response_sha256,
        review_sha256: digest(prepared.audit())?,
        reply,
        billing,
        pricing,
    })
}

#[cfg(test)]
mod tests;

#[cfg(feature = "test-fixtures")]
pub fn fixture_counted_api(
    draft: ScopedApiDraft,
    quote: ScopedPriceQuote,
    tokens: u64,
) -> Result<CountedScopedApi, String> {
    let client = ApiClient::new(SecretInput::new("sk-synthetic-scoped-api-only".into())?)?;
    draft.validate_count_terms(&quote)?;
    counted(
        client,
        draft,
        quote,
        json!({"object":"response.input_tokens","input_tokens":tokens}),
    )
}
#[cfg(feature = "test-fixtures")]
pub fn fixture_scoped_api_result(
    prepared: &PreparedScopedApi,
    value: Value,
) -> Result<ScopedApiResult, String> {
    decode(prepared, value)
}

#[cfg(feature = "test-fixtures")]
pub fn fixture_scoped_api_messages(prepared: &PreparedScopedApi) -> &[ApiMessage] {
    fixture_messages(&prepared.counted.draft.prepared)
}
