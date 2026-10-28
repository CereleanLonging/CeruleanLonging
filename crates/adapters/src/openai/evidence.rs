//! Public billing counters and a fingerprint of the exact tool-free request.
//! Neither a fingerprint nor usage establishes approval or a billing ceiling.
use super::*;
use crate::api_generation::ApiUsage;
use cerulean_domain::{credentials::CredentialProvider, execution::digest};
mod preflight;
#[cfg(feature = "test-fixtures")]
pub use preflight::{fixture_counted_decision, fixture_expired_counted_decision};
pub use preflight::{CountedDecision, ManagementTokenCount};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManagementRequestFingerprint {
    pub schema: u32,
    pub url: String,
    pub purpose: Purpose,
    pub requested_model: String,
    pub state_digest: String,
    pub body_sha256: String,
    pub max_output_tokens: u32,
}
impl ManagementRequestFingerprint {
    /// Replay checks the recorded wire contract, not a future prompt template.
    pub fn validate_for(&self, purpose: Purpose, model: &str, state: &str) -> Result<(), String> {
        sha256(&self.body_sha256)?;
        sha256(&self.state_digest)?;
        model_id(&self.requested_model)?;
        require(
            self.schema == 1
                && self.url == format!("{ENDPOINT}/responses")
                && self.purpose == purpose
                && self.requested_model == model
                && self.state_digest == state
                && self.max_output_tokens == 4096,
            "management wire identity differs from original intent",
        )
    }
}

/// Not serializable or Debug: context and any private response content stay out
/// of diagnostic output. Only the fingerprint is persisted with the intent.
pub struct PreparedDecision {
    body: Value,
    fingerprint: ManagementRequestFingerprint,
}
impl PreparedDecision {
    pub fn new(purpose: Purpose, model: &str, state: &str, input: &Value) -> Result<Self, String> {
        let body = body(purpose, model, state, input)?;
        let fingerprint = ManagementRequestFingerprint {
            schema: 1,
            url: format!("{ENDPOINT}/responses"),
            purpose,
            requested_model: model.into(),
            state_digest: state.into(),
            body_sha256: digest(&body)?,
            max_output_tokens: 4096,
        };
        Ok(Self { body, fingerprint })
    }
    pub fn fingerprint(&self) -> &ManagementRequestFingerprint {
        &self.fingerprint
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecisionTransport {
    pub request: ManagementRequestFingerprint,
    pub usage: ApiUsage,
}
impl DecisionTransport {
    pub fn validate(&self, observation: &DecisionObservation) -> Result<(), String> {
        self.request.validate_for(
            self.request.purpose,
            &observation.requested_model,
            &observation.envelope.state_digest,
        )?;
        self.usage.total_tokens(CredentialProvider::OpenAi)?;
        require(
            self.usage.input_tokens == observation.input_tokens
                && self.usage.output_tokens == observation.output_tokens,
            "management billing counters differ from response",
        )
    }
}

#[derive(Debug, Clone)]
pub struct DecisionResult {
    pub observation: DecisionObservation,
    pub transport: Option<DecisionTransport>,
}
impl From<DecisionObservation> for DecisionResult {
    fn from(observation: DecisionObservation) -> Self {
        Self {
            observation,
            transport: None,
        }
    }
}

fn optional_counter(value: &Value) -> Result<Option<u64>, String> {
    if value.is_null() {
        return Ok(None);
    }
    value
        .as_u64()
        .map(Some)
        .ok_or_else(|| "invalid OpenAI cache usage".into())
}

fn decode(request: &ManagementRequestFingerprint, value: Value) -> Result<DecisionResult, String> {
    let read = optional_counter(&value["usage"]["input_tokens_details"]["cached_tokens"])?;
    let write = optional_counter(&value["usage"]["input_tokens_details"]["cache_write_tokens"])?;
    let observation = parse_decision(
        value,
        request.purpose,
        &request.requested_model,
        &request.state_digest,
    )?;
    let transport = DecisionTransport {
        request: request.clone(),
        usage: ApiUsage {
            input_tokens: observation.input_tokens,
            output_tokens: observation.output_tokens,
            cache_read_tokens: read,
            cache_write_tokens: write,
        },
    };
    transport.validate(&observation)?;
    Ok(DecisionResult {
        observation,
        transport: Some(transport),
    })
}

impl OpenAiClient {
    /// Consumes exactly the prepared body after its caller persists the intent.
    /// There are no retries, tools, or provider-controlled follow-up requests.
    pub async fn decide_prepared(
        &self,
        request: PreparedDecision,
    ) -> Result<DecisionResult, String> {
        let response = self
            .send(reqwest::Method::POST, "/responses", Some(request.body))
            .await?;
        decode(&request.fingerprint, response)
    }
}

#[cfg(test)]
mod tests;
