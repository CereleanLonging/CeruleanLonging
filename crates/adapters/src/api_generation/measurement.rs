//! Public token observation independent of whether response content is usable.
//! No continuation, tool arguments, billing authority, or retry permission.
use super::*;
use cerulean_domain::execution::sha256;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RejectedUsage {
    pub schema: u32,
    pub request: RequestFingerprint,
    pub response_id: String,
    pub actual_model: String,
    pub usage: ApiUsage,
    /// Digest of the bounded decoded JSON, never a retained response body.
    pub response_json_sha256: String,
}
impl RejectedUsage {
    pub fn validate(&self, request: &RequestFingerprint) -> Result<(), String> {
        require(
            self.schema == 1 && &self.request == request,
            "rejected usage request binding differs",
        )?;
        request.target.url()?;
        sha256(&request.body_sha256)?;
        sha256(&request.tool_catalog_sha256)?;
        token(&request.requested_model)?;
        token(&self.response_id)?;
        token(&self.actual_model)?;
        sha256(&self.response_json_sha256)?;
        self.usage.total_tokens(request.target.provider())?;
        Ok(())
    }
}

#[derive(Debug)]
pub struct ApiAttempt {
    pub result: Result<ApiReply, ApiFailure>,
    pub rejected_usage: Option<RejectedUsage>,
}
/// Journal preflight is required for senders that can retain the new record.
pub trait ApiAttemptResult: Into<ApiAttempt> {
    const REJECTED_USAGE: bool;
}
impl ApiAttemptResult for ApiAttempt {
    const REJECTED_USAGE: bool = true;
}
impl ApiAttemptResult for Result<ApiReply, ApiFailure> {
    const REJECTED_USAGE: bool = false;
}
impl From<Result<ApiReply, ApiFailure>> for ApiAttempt {
    fn from(result: Result<ApiReply, ApiFailure>) -> Self {
        Self {
            result,
            rejected_usage: None,
        }
    }
}

fn number(v: &Value) -> Result<u64, String> {
    v.as_u64()
        .ok_or_else(|| "API usage missing or invalid".into())
}
fn optional(object: &Value, name: &str) -> Result<Option<u64>, String> {
    match object.get(name) {
        None => Ok(None),
        Some(value) => number(value).map(Some),
    }
}
fn details(value: &Value, name: &str) -> Result<Value, String> {
    match value.get(name) {
        None => Ok(serde_json::json!({})),
        Some(value) if value.is_object() => Ok(value.clone()),
        _ => Err("API usage details invalid".into()),
    }
}

fn capture(prepared: &PreparedRequest, value: &Value) -> Result<RejectedUsage, String> {
    require(
        value.is_object() && value["error"].is_null(),
        "API response contains an error",
    )?;
    let target = &prepared.fingerprint.target;
    let raw = &value["usage"];
    require(raw.is_object(), "API usage missing or invalid")?;
    let (input, output, read, write) = if matches!(target, ApiTarget::OpenAi {}) {
        require(value["object"] == "response", "API response type invalid")?;
        let details = details(raw, "input_tokens_details")?;
        (
            number(&raw["input_tokens"])?,
            number(&raw["output_tokens"])?,
            optional(&details, "cached_tokens")?,
            optional(&details, "cache_write_tokens")?,
        )
    } else if target.provider() == CredentialProvider::Anthropic {
        require(
            value["type"] == "message" && value["role"] == "assistant",
            "API message type invalid",
        )?;
        (
            number(&raw["input_tokens"])?,
            number(&raw["output_tokens"])?,
            optional(raw, "cache_read_input_tokens")?,
            optional(raw, "cache_creation_input_tokens")?,
        )
    } else {
        require(
            value["object"] == "chat.completion",
            "API response type invalid",
        )?;
        let details = details(raw, "prompt_tokens_details")?;
        let direct = optional(raw, "prompt_cache_hit_tokens")?;
        let cached = optional(&details, "cached_tokens")?;
        require(
            direct.zip(cached).is_none_or(|(a, b)| a == b),
            "API cache fields disagree",
        )?;
        let input = number(&raw["prompt_tokens"])?;
        if let Some(miss) = optional(raw, "prompt_cache_miss_tokens")? {
            require(
                direct.and_then(|hit| hit.checked_add(miss)) == Some(input),
                "API cache breakdown disagrees",
            )?;
        }
        (
            input,
            number(&raw["completion_tokens"])?,
            direct.or(cached),
            None,
        )
    };
    let usage = ApiUsage {
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: read,
        cache_write_tokens: write,
    };
    let total = usage.total_tokens(target.provider())?;
    if let Some(declared) = optional(raw, "total_tokens")? {
        require(declared == total, "API total usage disagrees")?;
    }
    let result = RejectedUsage {
        schema: 1,
        request: prepared.fingerprint.clone(),
        response_id: value["id"].as_str().ok_or("invalid API identity")?.into(),
        actual_model: value["model"]
            .as_str()
            .ok_or("invalid API identity")?
            .into(),
        usage,
        response_json_sha256: digest(value)?,
    };
    result.validate(&prepared.fingerprint)?;
    Ok(result)
}

pub(super) fn decode(prepared: &PreparedRequest, value: Value) -> ApiAttempt {
    let usage = capture(prepared, &value).ok();
    let result = wire::parse(&prepared.request, value).map_err(ApiFailure::response_rejected);
    ApiAttempt {
        rejected_usage: if result.is_err() { usage } else { None },
        result,
    }
}

impl ApiClient {
    pub async fn send_measured_with_gateway(
        &self,
        prepared: PreparedRequest,
        cancel: &mut oneshot::Receiver<()>,
        gateway: &mut GatewayObserver<'_>,
    ) -> ApiAttempt {
        let (result, _) = self
            .send_evidence_json_with_gateway(&prepared, cancel, gateway)
            .await;
        match result {
            Ok(value) => decode(&prepared, value),
            Err(failure) => Err(failure).into(),
        }
    }
    /// Loopback-only fixture uses the same bounded HTTP decoder as production.
    #[cfg(feature = "test-fixtures")]
    pub async fn fixture_send_measured(
        &self,
        prepared: PreparedRequest,
        cancel: &mut oneshot::Receiver<()>,
        url: &str,
    ) -> ApiAttempt {
        let Ok(parsed) = reqwest::Url::parse(url) else {
            return Err(ApiFailure::NotSent {}).into();
        };
        if parsed.scheme() != "http"
            || parsed.host_str() != Some("127.0.0.1")
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(ApiFailure::NotSent {}).into();
        }
        let local = Self {
            http: Self::http(false).expect("fixture HTTP client"),
            key: SecretInput::new(self.key.expose().to_owned()).expect("fixture key"),
        };
        match local
            .send_json(
                &prepared.request.target,
                &prepared.body,
                prepared.request.timeout_seconds,
                cancel,
                url,
            )
            .await
        {
            Ok(value) => decode(&prepared, value),
            Err(failure) => Err(failure).into(),
        }
    }
}
