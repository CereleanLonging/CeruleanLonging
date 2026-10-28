use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Closed list of credential slots implemented by the local vault. Provider
/// selection is never interpreted as a filesystem path or a custom API endpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum CredentialProvider {
    OpenAi,
    DeepSeek,
    Anthropic,
    Qwen,
    Kimi,
    Grok,
    A6Api,
}
impl CredentialProvider {
    pub fn id(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::DeepSeek => "deepseek",
            Self::Anthropic => "anthropic",
            Self::Qwen => "qwen",
            Self::Kimi => "kimi",
            Self::Grok => "grok",
            Self::A6Api => "a6api",
        }
    }
    pub const WORKERS: [Self; 6] = [
        Self::DeepSeek,
        Self::Anthropic,
        Self::Qwen,
        Self::Kimi,
        Self::Grok,
        Self::A6Api,
    ];
}

/// A closed endpoint selection: never accept a URL from the frontend or a model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ProviderEndpoint {
    DeepSeek {},
    Anthropic {},
    KimiChina {},
    Grok {},
    A6Api {},
    QwenBeijing {
        #[serde(rename = "workspaceId")]
        workspace_id: String,
    },
    QwenSingapore {},
    QwenHongKong {},
}
impl ProviderEndpoint {
    pub fn provider(&self) -> CredentialProvider {
        match self {
            Self::DeepSeek {} => CredentialProvider::DeepSeek,
            Self::Anthropic {} => CredentialProvider::Anthropic,
            Self::KimiChina {} => CredentialProvider::Kimi,
            Self::Grok {} => CredentialProvider::Grok,
            Self::A6Api {} => CredentialProvider::A6Api,
            Self::QwenBeijing { .. } | Self::QwenSingapore {} | Self::QwenHongKong {} => {
                CredentialProvider::Qwen
            }
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProviderConnection {
    pub provider: CredentialProvider,
    pub key_configured: bool,
    pub state: String,
    pub target: Option<ProviderEndpoint>,
    pub endpoint: Option<String>,
    pub checked_at: Option<String>,
    pub models: Vec<String>,
    pub list_limited: bool,
    pub error: Option<String>,
}
impl ProviderConnection {
    pub fn unchecked(provider: CredentialProvider, key_configured: bool) -> Self {
        Self {
            provider,
            key_configured,
            state: if key_configured {
                "unchecked"
            } else {
                "notConfigured"
            }
            .into(),
            target: None,
            endpoint: None,
            checked_at: None,
            models: vec![],
            list_limited: false,
            error: None,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderCredentialStatus {
    pub provider: CredentialProvider,
    /// Presence of a bounded encrypted envelope, not API authentication.
    pub key_configured: bool,
}
