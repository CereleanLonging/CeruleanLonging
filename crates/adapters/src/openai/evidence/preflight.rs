//! Exact provider counting of the existing, tool-free management request.
//! A count is not approval, a price quote, or proof that counting itself is free.
use super::*;
use cerulean_domain::currency_budget::{TokenCeiling, MAX_TOKENS};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const COUNT_PATH: &str = "/responses/input_tokens";
const COUNT_MAX_AGE: Duration = Duration::from_secs(60);

/// Audit data only. It cannot be deserialized into authority to send a request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManagementTokenCount {
    pub schema: u32,
    pub endpoint: String,
    pub request: ManagementRequestFingerprint,
    pub count_body_sha256: String,
    pub response_sha256: String,
    pub observed_unix_ms: u64,
    pub input_tokens: u64,
}
impl ManagementTokenCount {
    /// Validate an archived receipt, not authority to send or recreate a live count.
    pub fn validate(&self, request: &ManagementRequestFingerprint) -> Result<(), String> {
        request.validate_for(
            request.purpose,
            &request.requested_model,
            &request.state_digest,
        )?;
        sha256(&self.count_body_sha256)?;
        sha256(&self.response_sha256)?;
        require(
            self.schema == 1
                && self.endpoint == format!("{ENDPOINT}{COUNT_PATH}")
                && &self.request == request
                && self.observed_unix_ms > 0
                && (1..=MAX_TOKENS).contains(&self.input_tokens)
                && self.response_sha256
                    == digest(
                        &json!({"object":"response.input_tokens","input_tokens":self.input_tokens}),
                    )?,
            "invalid archived management token count",
        )
    }
}

/// Owns both the unchanged request and the client which counted it. No Clone,
/// Deserialize, public constructor, or caller-supplied count can grant a proof.
/// Host authorization, durable reservation, and an intent still precede send.
pub struct CountedDecision {
    client: OpenAiClient,
    prepared: PreparedDecision,
    evidence: ManagementTokenCount,
    observed: Instant,
}
impl CountedDecision {
    pub fn evidence(&self) -> &ManagementTokenCount {
        &self.evidence
    }
    pub fn prepared(&self) -> &PreparedDecision {
        &self.prepared
    }
    pub fn ceiling(&self) -> Result<TokenCeiling, String> {
        require(
            self.observed.elapsed() < COUNT_MAX_AGE,
            "management token count expired; no generation sent",
        )?;
        let ceiling = TokenCeiling {
            total_input: self.evidence.input_tokens,
            output: u64::from(self.prepared.fingerprint.max_output_tokens),
            // Do not assume discounts, absent cache counters, or free writes.
            // The approved quote must cover every possible input category.
            cache_read_allowed: true,
            cache_write_allowed: true,
        };
        ceiling.validate()?;
        Ok(ceiling)
    }
    /// Consumes the counted body once, using the same credential/client.
    /// This method does not reserve money or authenticate user approval.
    pub async fn decide(self) -> Result<DecisionResult, String> {
        self.decide_url(&format!("{ENDPOINT}/responses")).await
    }
    async fn decide_url(self, url: &str) -> Result<DecisionResult, String> {
        self.ceiling()?;
        let response = self
            .client
            .send_url(reqwest::Method::POST, url, Some(self.prepared.body))
            .await?;
        decode(&self.prepared.fingerprint, response)
    }
}

impl PreparedDecision {
    /// Public audit identity of the exact counting projection, never send authority.
    pub fn count_request_identity(&self) -> Result<(String, String, usize), String> {
        let body = self.count_body()?;
        Ok((
            format!("{ENDPOINT}{COUNT_PATH}"),
            digest(&body)?,
            serde_json::to_vec(&body)
                .map_err(|_| "invalid count body")?
                .len(),
        ))
    }
    fn count_body(&self) -> Result<Value, String> {
        let mut body = self
            .body
            .as_object()
            .ok_or("invalid management body")?
            .clone();
        // Remove only the three generation-only fields not accepted by count.
        // Reject newly added fields until their token semantics are reviewed.
        require(
            body.len() == 10
                && body.get("store") == Some(&json!(false))
                && body.get("stream") == Some(&json!(false))
                && body.get("max_output_tokens") == Some(&json!(4096))
                && body.get("tools") == Some(&json!([]))
                && body.get("tool_choice") == Some(&json!("none"))
                && body.get("parallel_tool_calls") == Some(&json!(false))
                && digest(&self.body)? == self.fingerprint.body_sha256,
            "management request is outside reviewed counting contract",
        )?;
        for field in ["store", "stream", "max_output_tokens"] {
            body.remove(field);
        }
        let allowed = [
            "model",
            "instructions",
            "input",
            "text",
            "tools",
            "tool_choice",
            "parallel_tool_calls",
        ];
        require(
            body.len() == allowed.len() && allowed.iter().all(|key| body.contains_key(*key)),
            "unreviewed management token-count fields",
        )?;
        Ok(Value::Object(body))
    }
}

impl OpenAiClient {
    /// Sends exactly one counting request, never a generation or a fallback.
    /// Caller must authorize disclosure of context before invoking this method.
    pub async fn count_decision(
        self,
        prepared: PreparedDecision,
    ) -> Result<CountedDecision, String> {
        self.count_decision_url(prepared, &format!("{ENDPOINT}{COUNT_PATH}"))
            .await
    }
    // Private loopback seam, like send_url; production callers cannot pick a URL.
    async fn count_decision_url(
        self,
        prepared: PreparedDecision,
        url: &str,
    ) -> Result<CountedDecision, String> {
        let body = prepared.count_body()?;
        let response = self
            .send_url(reqwest::Method::POST, url, Some(body.clone()))
            .await?;
        counted(self, prepared, &body, response)
    }
}

fn counted(
    client: OpenAiClient,
    prepared: PreparedDecision,
    body: &Value,
    response: Value,
) -> Result<CountedDecision, String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct CountResponse {
        object: String,
        input_tokens: u64,
    }
    let decoded: CountResponse =
        serde_json::from_value(response.clone()).map_err(|_| "invalid OpenAI input token count")?;
    require(
        decoded.object == "response.input_tokens"
            && (1..=MAX_TOKENS).contains(&decoded.input_tokens),
        "invalid OpenAI input token count",
    )?;
    let evidence = ManagementTokenCount {
        schema: 1,
        endpoint: format!("{ENDPOINT}{COUNT_PATH}"),
        request: prepared.fingerprint.clone(),
        count_body_sha256: digest(body)?,
        response_sha256: digest(&response)?,
        observed_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "invalid token-count observation time")?
            .as_millis()
            .try_into()
            .map_err(|_| "token-count observation time overflow")?,
        input_tokens: decoded.input_tokens,
    };
    Ok(CountedDecision {
        client,
        prepared,
        evidence,
        observed: Instant::now(),
    })
}

/// Offline, synthetic proof for cross-crate tests only; excluded from normal
/// product builds. No key lookup, HTTP request, or caller-controlled transport.
#[cfg(feature = "test-fixtures")]
pub fn fixture_counted_decision(
    prepared: PreparedDecision,
    input_tokens: u64,
) -> Result<CountedDecision, String> {
    let body = prepared.count_body()?;
    counted(
        OpenAiClient::new(SecretInput::new("sk-synthetic-count-fixture".into())?)?,
        prepared,
        &body,
        json!({"object":"response.input_tokens","input_tokens":input_tokens}),
    )
}

#[cfg(test)]
mod tests;

/// Expired synthetic proof, only for cross-crate recovery tests.
#[cfg(feature = "test-fixtures")]
pub fn fixture_expired_counted_decision(
    prepared: PreparedDecision,
    input_tokens: u64,
) -> Result<CountedDecision, String> {
    let mut value = fixture_counted_decision(prepared, input_tokens)?;
    value.observed = Instant::now() - COUNT_MAX_AGE;
    Ok(value)
}
