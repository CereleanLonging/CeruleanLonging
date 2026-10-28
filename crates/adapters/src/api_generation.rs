//! Worker-only HTTP transport. Caller owns approval, durable intent and tools.
//! A reply is a proposal, never proof that a task or a tool succeeded.
pub mod gateway;
pub mod scoped;
pub type GatewayObserver<'a> = dyn FnMut(&gateway::GatewayHeaders) + Send + 'a;
mod response_diagnostic;
mod transport_diagnostic;
use response_diagnostic::PersistedFailure;
pub use response_diagnostic::ResponseRejection;
mod wire;
use cerulean_domain::{
    credentials::CredentialProvider,
    execution::{digest, identifier, require},
    secret::SecretInput,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeSet, time::Duration};
use tokio::sync::oneshot;
pub(crate) use transport_diagnostic::TransportStage;
pub const MAX_TEXT: usize = 128 * 1024;
const MAX_BODY: usize = 1024 * 1024;
const MAX_RESPONSE: usize = 2 * 1024 * 1024;

pub use cerulean_domain::api_worker::ApiTarget;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiTool {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}
impl ApiTool {
    fn validate(&self) -> Result<(), String> {
        identifier(&self.name)?;
        require(
            self.name.starts_with("cerulean_") && self.name.len() <= 64,
            "API tool must use the workbench namespace",
        )?;
        text(&self.description, 4096, false)?;
        require(
            self.parameters["type"] == "object"
                && self.parameters["properties"].is_object()
                && self.parameters["additionalProperties"] == false,
            "API tool schema must be a closed object",
        )?;
        require(
            serde_json::to_vec(&self.parameters)
                .map_err(|_| "invalid API tool schema")?
                .len()
                <= 32768,
            "API tool schema too large",
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
}
impl ApiUsage {
    /// Strict normalization for money accounting. Token-count reporting remains
    /// available when cache details are absent, but absence is not a zero-price
    /// or zero-cache observation. The caller retains its reservation on error.
    pub fn billable_usage(
        &self,
        provider: CredentialProvider,
    ) -> Result<cerulean_domain::currency_budget::Usage, String> {
        require(
            provider != CredentialProvider::A6Api,
            "A6API monetary usage contract is not approved",
        )?;
        self.total_tokens(provider)?;
        let read = self
            .cache_read_tokens
            .ok_or("billable cache-read usage unknown")?;
        let usage = if provider == CredentialProvider::Anthropic {
            cerulean_domain::currency_budget::Usage {
                input: self.input_tokens,
                output: self.output_tokens,
                cache_read: read,
                cache_write: self
                    .cache_write_tokens
                    .ok_or("billable cache-write usage unknown")?,
            }
        } else if provider == CredentialProvider::OpenAi {
            let write = self
                .cache_write_tokens
                .ok_or("billable OpenAI cache-write usage unknown")?;
            cerulean_domain::currency_budget::Usage {
                input: self
                    .input_tokens
                    .checked_sub(read)
                    .and_then(|n| n.checked_sub(write))
                    .ok_or("cached input exceeds total")?,
                output: self.output_tokens,
                cache_read: read,
                cache_write: write,
            }
        } else {
            cerulean_domain::currency_budget::Usage {
                input: self
                    .input_tokens
                    .checked_sub(read)
                    .ok_or("cached input exceeds total")?,
                output: self.output_tokens,
                cache_read: read,
                cache_write: 0,
            }
        };
        usage.validate()?;
        Ok(usage)
    }
    pub fn total_tokens(&self, provider: CredentialProvider) -> Result<u64, String> {
        let mut input = self.input_tokens;
        if provider == CredentialProvider::Anthropic {
            input = input
                .checked_add(self.cache_read_tokens.unwrap_or(0))
                .and_then(|n| n.checked_add(self.cache_write_tokens.unwrap_or(0)))
                .ok_or("API input usage overflow")?;
        } else if provider == CredentialProvider::OpenAi {
            require(
                self.cache_read_tokens
                    .unwrap_or(0)
                    .checked_add(self.cache_write_tokens.unwrap_or(0))
                    .is_some_and(|n| n <= input),
                "OpenAI cached input breakdown invalid",
            )?;
        } else {
            require(
                self.cache_read_tokens.is_none_or(|n| n <= input)
                    && self.cache_write_tokens.is_none(),
                "API cached input breakdown invalid",
            )?;
        }
        input
            .checked_add(self.output_tokens)
            .ok_or("API usage overflow".into())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiObservation {
    pub response_id: String,
    pub actual_model: String,
    pub text: String,
    pub tool_calls: Vec<ToolCall>,
    pub usage: ApiUsage,
}
impl ApiObservation {
    pub fn validate(&self) -> Result<(), String> {
        token(&self.response_id)?;
        token(&self.actual_model)?;
        text(&self.text, MAX_TEXT, true)?;
        require(
            self.tool_calls.len() <= 16 && (!self.text.is_empty() || !self.tool_calls.is_empty()),
            "API reply has no actionable output or too many calls",
        )?;
        let mut ids = BTreeSet::new();
        for call in &self.tool_calls {
            token(&call.id)?;
            identifier(&call.name)?;
            require(
                ids.insert(&call.id)
                    && call.arguments.is_object()
                    && serde_json::to_vec(&call.arguments)
                        .map_err(|_| "invalid API arguments")?
                        .len()
                        <= MAX_TEXT,
                "API calls repeated or arguments invalid",
            )?;
        }
        Ok(())
    }
}
/// Only the decoder can create a reply usable as conversation history. Private
/// reasoning/protocol continuation lives in memory and is excluded from Debug,
/// serialization and journal observations. No replay token is exposed over IPC.
#[derive(Clone)]
pub struct ApiReply {
    observation: ApiObservation,
    target: ApiTarget,
    requested_model: String,
    continuation: Vec<Value>,
}
impl std::fmt::Debug for ApiReply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ApiReply")
            .field("observation", &self.observation)
            .field("continuation", &"[PRIVATE]")
            .finish()
    }
}
impl ApiReply {
    pub fn observation(&self) -> &ApiObservation {
        &self.observation
    }
}
#[derive(Clone, Debug)]
pub struct ToolResult {
    pub call_id: String,
    pub text: String,
    pub is_error: bool,
}
#[derive(Clone, Debug)]
pub enum ApiMessage {
    User(String),
    Assistant(ApiReply),
    Tools(Vec<ToolResult>),
}
#[derive(Clone, Debug)]
pub struct ApiRequest {
    pub target: ApiTarget,
    pub model: String,
    pub instructions: String,
    pub messages: Vec<ApiMessage>,
    pub tools: Vec<ApiTool>,
    pub max_output_tokens: u32,
    pub timeout_seconds: u32,
}
fn text(s: &str, limit: usize, empty: bool) -> Result<(), String> {
    require(
        (empty || !s.trim().is_empty()) && s.len() <= limit && !s.contains('\0'),
        "API text exceeds bounds or is invalid",
    )
}
fn token(s: &str) -> Result<(), String> {
    require(
        !s.is_empty()
            && s.len() <= 200
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b)),
        "invalid API identity",
    )
}
fn contains_secret(v: &Value, key: &str) -> bool {
    match v {
        Value::String(s) => s.contains(key),
        Value::Array(a) => a.iter().any(|v| contains_secret(v, key)),
        Value::Object(o) => o
            .iter()
            .any(|(k, v)| k.contains(key) || contains_secret(v, key)),
        _ => false,
    }
}
impl ApiRequest {
    fn validate(&self) -> Result<(), String> {
        self.target.url()?;
        token(&self.model)?;
        text(&self.instructions, 32768, false)?;
        require(
            (1..=16384).contains(&self.max_output_tokens)
                && (1..=600).contains(&self.timeout_seconds),
            "API output or time limit invalid",
        )?;
        require(
            !self.messages.is_empty()
                && self.messages.len() <= 256
                && matches!(self.messages[0], ApiMessage::User(_))
                && self.tools.len() <= 8,
            "API conversation or tools out of bounds",
        )?;
        let mut names = BTreeSet::new();
        for tool in &self.tools {
            tool.validate()?;
            require(names.insert(&tool.name), "API tool name repeated")?;
        }
        let mut pending = BTreeSet::new();
        let mut seen = BTreeSet::new();
        let mut last_assistant = false;
        for message in &self.messages {
            match message {
                ApiMessage::User(s) => {
                    require(
                        pending.is_empty(),
                        "API tool results missing before user message",
                    )?;
                    text(s, MAX_TEXT, false)?;
                    last_assistant = false;
                }
                ApiMessage::Assistant(reply) => {
                    require(
                        !last_assistant
                            && pending.is_empty()
                            && reply.target == self.target
                            && reply.requested_model == self.model,
                        "API reply cannot cross conversation target or unfinished calls",
                    )?;
                    reply.observation.validate()?;
                    for call in &reply.observation.tool_calls {
                        require(
                            names.contains(&call.name) && seen.insert(call.id.clone()),
                            "unregistered or repeated API tool call",
                        )?;
                        pending.insert(call.id.clone());
                    }
                    last_assistant = true;
                }
                ApiMessage::Tools(results) => {
                    require(
                        last_assistant && !results.is_empty() && results.len() <= 16,
                        "unexpected API tool results",
                    )?;
                    for result in results {
                        text(&result.text, MAX_TEXT, true)?;
                        require(
                            pending.remove(&result.call_id),
                            "API tool result does not match a pending call",
                        )?;
                    }
                    if pending.is_empty() {
                        last_assistant = false;
                    }
                }
            }
        }
        require(
            pending.is_empty() && !last_assistant,
            "API request must end with user input or all tool results",
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestFingerprint {
    pub target: ApiTarget,
    pub requested_model: String,
    pub body_sha256: String,
    pub tool_catalog_sha256: String,
    pub max_output_tokens: u32,
    pub timeout_seconds: u32,
}
pub struct PreparedRequest {
    request: ApiRequest,
    body: Value,
    fingerprint: RequestFingerprint,
}
pub(crate) fn request_fingerprint(request: &ApiRequest) -> Result<RequestFingerprint, String> {
    request.validate()?;
    let body = wire::body(request)?;
    require(
        serde_json::to_vec(&body)
            .map_err(|_| "invalid API body")?
            .len()
            <= MAX_BODY,
        "API request too large",
    )?;
    Ok(RequestFingerprint {
        target: request.target.clone(),
        requested_model: request.model.clone(),
        body_sha256: digest(&body)?,
        tool_catalog_sha256: digest(&request.tools)?,
        max_output_tokens: request.max_output_tokens,
        timeout_seconds: request.timeout_seconds,
    })
}
/// Offline test decoder; excluded from normal product builds. This still uses
/// the production parser, target and registered-tool checks, and never sends.
#[cfg(feature = "test-fixtures")]
pub fn fixture_reply(prepared: &PreparedRequest, value: Value) -> Result<ApiReply, String> {
    require(
        serde_json::to_vec(&value)
            .map_err(|_| "invalid fixture")?
            .len()
            <= MAX_RESPONSE,
        "fixture too large",
    )?;
    wire::parse(&prepared.request, value)
}
/// Test-only inspection of actual prepared conversation data; no secret or HTTP transport.
#[cfg(feature = "test-fixtures")]
pub fn fixture_messages(prepared: &PreparedRequest) -> &[ApiMessage] {
    &prepared.request.messages
}
impl PreparedRequest {
    pub fn fingerprint(&self) -> &RequestFingerprint {
        &self.fingerprint
    }
    pub(crate) fn decode(&self, value: Value) -> Result<ApiReply, String> {
        wire::parse(&self.request, value)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "PersistedFailure", from = "PersistedFailure")]
pub enum ApiFailure {
    NotSent {},
    LocalCancellation {},
    Transport {},
    Http { status: u16 },
    InvalidResponse {},
    ResponseRejected { reason: ResponseRejection },
    Oversized {},
}
pub struct ApiClient {
    http: reqwest::Client,
    key: SecretInput,
}
impl ApiClient {
    /// Internal evidence-preserving path. The raw reply stays in memory; callers
    /// must extract bounded public billing fields before validating a decision.
    pub(crate) async fn send_evidence_json(
        &self,
        prepared: &PreparedRequest,
        cancel: &mut oneshot::Receiver<()>,
    ) -> (Result<Value, ApiFailure>, Option<TransportStage>) {
        self.send_evidence_json_with_gateway(prepared, cancel, &mut |_| {})
            .await
    }
    pub(crate) async fn send_evidence_json_with_gateway(
        &self,
        prepared: &PreparedRequest,
        cancel: &mut oneshot::Receiver<()>,
        gateway: &mut GatewayObserver<'_>,
    ) -> (Result<Value, ApiFailure>, Option<TransportStage>) {
        let Ok(url) = prepared.request.target.url() else {
            return (Err(ApiFailure::NotSent {}), None);
        };
        let mut stage = None;
        let result = self
            .send_json_captured(
                &prepared.request.target,
                &prepared.body,
                prepared.request.timeout_seconds,
                cancel,
                &url,
                &mut stage,
                gateway,
            )
            .await;
        if !matches!(result, Err(ApiFailure::Transport {})) {
            stage = None;
        }
        (result, stage)
    }
    pub fn new(key: SecretInput) -> Result<Self, String> {
        key.validate()?;
        Ok(Self {
            http: Self::http(true)?,
            key,
        })
    }
    fn http(https: bool) -> Result<reqwest::Client, String> {
        reqwest::Client::builder()
            .https_only(https)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(15))
            .build()
            .map_err(|_| "API TLS client unavailable".into())
    }
    pub fn prepare(&self, request: ApiRequest) -> Result<PreparedRequest, String> {
        request.validate()?;
        let body = wire::body(&request)?;
        let bytes = serde_json::to_vec(&body).map_err(|_| "invalid API request")?;
        require(
            bytes.len() <= MAX_BODY && !contains_secret(&body, self.key.expose()),
            "API request too large or contains credential",
        )?;
        let fingerprint = RequestFingerprint {
            target: request.target.clone(),
            requested_model: request.model.clone(),
            body_sha256: digest(&body)?,
            tool_catalog_sha256: digest(&request.tools)?,
            max_output_tokens: request.max_output_tokens,
            timeout_seconds: request.timeout_seconds,
        };
        Ok(PreparedRequest {
            request,
            body,
            fingerprint,
        })
    }
    /// One owned prepared request, one HTTP POST, no retry. The caller MUST
    /// persist its intent before calling this method. Cancellation after send
    /// says nothing about remote cancellation or billing.
    pub async fn send(
        &self,
        prepared: PreparedRequest,
        cancel: &mut oneshot::Receiver<()>,
    ) -> Result<ApiReply, ApiFailure> {
        let url = prepared
            .request
            .target
            .url()
            .map_err(|_| ApiFailure::NotSent {})?;
        self.send_url(prepared, cancel, &url).await
    }
    async fn send_url(
        &self,
        prepared: PreparedRequest,
        cancel: &mut oneshot::Receiver<()>,
        url: &str,
    ) -> Result<ApiReply, ApiFailure> {
        self.send_url_with_gateway(prepared, cancel, url, &mut |_| {})
            .await
    }
    /// Observational callback runs once after response headers, before body/HTTP
    /// rejection. It grants no retry, billing, tool, or workflow authority.
    pub async fn send_with_gateway(
        &self,
        prepared: PreparedRequest,
        cancel: &mut oneshot::Receiver<()>,
        gateway: &mut GatewayObserver<'_>,
    ) -> Result<ApiReply, ApiFailure> {
        let url = prepared
            .request
            .target
            .url()
            .map_err(|_| ApiFailure::NotSent {})?;
        self.send_url_with_gateway(prepared, cancel, &url, gateway)
            .await
    }
    async fn send_url_with_gateway(
        &self,
        prepared: PreparedRequest,
        cancel: &mut oneshot::Receiver<()>,
        url: &str,
        gateway: &mut GatewayObserver<'_>,
    ) -> Result<ApiReply, ApiFailure> {
        let value = self
            .send_json_captured(
                &prepared.request.target,
                &prepared.body,
                prepared.request.timeout_seconds,
                cancel,
                url,
                &mut None,
                gateway,
            )
            .await?;
        wire::parse(&prepared.request, value).map_err(ApiFailure::response_rejected)
    }
    async fn send_json(
        &self,
        target: &ApiTarget,
        body: &Value,
        timeout_seconds: u32,
        cancel: &mut oneshot::Receiver<()>,
        url: &str,
    ) -> Result<Value, ApiFailure> {
        self.send_json_observed(target, body, timeout_seconds, cancel, url, &mut None)
            .await
    }
    async fn send_json_observed(
        &self,
        target: &ApiTarget,
        body: &Value,
        timeout_seconds: u32,
        cancel: &mut oneshot::Receiver<()>,
        url: &str,
        stage: &mut Option<TransportStage>,
    ) -> Result<Value, ApiFailure> {
        self.send_json_captured(
            target,
            body,
            timeout_seconds,
            cancel,
            url,
            stage,
            &mut |_| {},
        )
        .await
    }
    #[allow(clippy::too_many_arguments)]
    async fn send_json_captured(
        &self,
        target: &ApiTarget,
        body: &Value,
        timeout_seconds: u32,
        cancel: &mut oneshot::Receiver<()>,
        url: &str,
        stage: &mut Option<TransportStage>,
        gateway: &mut GatewayObserver<'_>,
    ) -> Result<Value, ApiFailure> {
        if contains_secret(body, self.key.expose()) {
            return Err(ApiFailure::NotSent {});
        }
        if !matches!(cancel.try_recv(), Err(oneshot::error::TryRecvError::Empty)) {
            return Err(ApiFailure::NotSent {});
        }
        let mut req = self
            .http
            .post(url)
            .timeout(Duration::from_secs(timeout_seconds.into()))
            .json(body);
        if target.provider() == CredentialProvider::Anthropic {
            let mut key = reqwest::header::HeaderValue::from_str(self.key.expose())
                .map_err(|_| ApiFailure::NotSent {})?;
            key.set_sensitive(true);
            req = req
                .header("x-api-key", key)
                .header("anthropic-version", "2023-06-01");
        } else {
            req = req.bearer_auth(self.key.expose());
        }
        let response = async {
            let mut response = req.send().await.map_err(|error| {
                *stage = Some(TransportStage::request_error(&error));
                ApiFailure::Transport {}
            })?;
            // Synchronous capture precedes every status/body rejection and the
            // next await; cancellation cannot erase an already delivered header.
            if response.url().as_str() == url {
                if let Some(observation) = gateway::capture(
                    target,
                    response.status().as_u16(),
                    response.headers(),
                    self.key.expose(),
                ) {
                    gateway(&observation);
                }
            }
            if !response.status().is_success() {
                return Err(ApiFailure::Http {
                    status: response.status().as_u16(),
                });
            }
            if response.url().as_str() != url {
                return Err(ApiFailure::ResponseRejected {
                    reason: ResponseRejection::UrlMismatch,
                });
            }
            if response
                .content_length()
                .is_some_and(|n| n > MAX_RESPONSE as u64)
            {
                return Err(ApiFailure::Oversized {});
            }
            let mut raw = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|error| {
                *stage = Some(if error.is_timeout() {
                    TransportStage::BodyTimeout
                } else {
                    TransportStage::BodyRead
                });
                ApiFailure::Transport {}
            })? {
                if raw.len() + chunk.len() > MAX_RESPONSE {
                    return Err(ApiFailure::Oversized {});
                }
                raw.extend_from_slice(&chunk);
            }
            if raw
                .windows(self.key.expose().len())
                .any(|w| w == self.key.expose().as_bytes())
            {
                return Err(ApiFailure::ResponseRejected {
                    reason: ResponseRejection::CredentialEcho,
                });
            }
            let value = serde_json::from_slice(&raw).map_err(|_| ApiFailure::ResponseRejected {
                reason: ResponseRejection::InvalidJson,
            })?;
            if contains_secret(&value, self.key.expose()) {
                return Err(ApiFailure::ResponseRejected {
                    reason: ResponseRejection::CredentialEcho,
                });
            }
            Ok(value)
        };
        tokio::select! {biased;_=&mut *cancel=>Err(ApiFailure::LocalCancellation{}),result=response=>result}
    }
}
#[cfg(test)]
mod tests;
#[cfg(test)]
use cerulean_domain::credentials::ProviderEndpoint;

#[cfg(test)]
#[path = "api_generation/gateway_transport_tests.rs"]
mod gateway_transport_tests;
