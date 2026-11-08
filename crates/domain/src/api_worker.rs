//! Approved API worker selection. No credential or arbitrary endpoint is data.
use crate::{
    credentials::{CredentialProvider, ProviderEndpoint},
    execution::{identifier, require},
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ApiTarget {
    OpenAi {},
    Provider { endpoint: ProviderEndpoint },
}
impl ApiTarget {
    pub fn provider(&self) -> CredentialProvider {
        match self {
            Self::OpenAi {} => CredentialProvider::OpenAi,
            Self::Provider { endpoint } => endpoint.provider(),
        }
    }
    pub fn url(&self) -> Result<String, String> {
        Ok(match self {
            Self::OpenAi {} => "https://api.openai.com/v1/responses".into(),
            Self::Provider { endpoint } => match endpoint {
                ProviderEndpoint::Anthropic {} => "https://api.anthropic.com/v1/messages".into(),
                ProviderEndpoint::DeepSeek {} => "https://api.deepseek.com/chat/completions".into(),
                ProviderEndpoint::KimiChina {} => {
                    "https://api.moonshot.cn/v1/chat/completions".into()
                }
                ProviderEndpoint::A6Api {} => "https://api.a6api.com/v1/chat/completions".into(),
                ProviderEndpoint::A6ApiVip {} => "https://api.a6api.vip/v1/chat/completions".into(),
                ProviderEndpoint::Grok {} => "https://api.x.ai/v1/chat/completions".into(),
                ProviderEndpoint::QwenSingapore {} => {
                    "https://dashscope-intl.aliyuncs.com/compatible-mode/v1/chat/completions".into()
                }
                ProviderEndpoint::QwenHongKong {} => {
                    "https://cn-hongkong.dashscope.aliyuncs.com/compatible-mode/v1/chat/completions"
                        .into()
                }
                ProviderEndpoint::QwenBeijing { workspace_id } => {
                    require(
                        !workspace_id.is_empty()
                            && workspace_id.len() <= 63
                            && workspace_id
                                .bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                            && !workspace_id.starts_with('-')
                            && !workspace_id.ends_with('-'),
                        "invalid Qwen workspace",
                    )?;
                    format!("https://{}.cn-beijing.maas.aliyuncs.com/compatible-mode/v1/chat/completions",workspace_id.to_ascii_lowercase())
                }
            },
        })
    }
}
pub fn is_api_adapter(id: &str) -> bool {
    matches!(
        id,
        "openai" | "anthropic" | "deepseek" | "qwen" | "kimi" | "grok" | "a6api"
    )
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ApiWriteScope {
    /// Exact relative paths from the task's approved artifact_paths. Reading
    /// dependencies remains permitted by the separate ReadFiles capability.
    DeclaredArtifacts,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiWorkerSelection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<crate::reasoning::ReasoningEffort>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub write_scope: Option<ApiWriteScope>,
    pub target: ApiTarget,
    pub model: String,
    pub file_tools: Vec<crate::execution::ToolPermission>,
    pub max_rounds: u32,
    pub max_tool_calls: u32,
    /// Explicit provider request ceiling. The provider enforces its model limit;
    /// do not impose a second, model-independent 16K ceiling here.
    pub max_output_tokens: u32,
    #[ts(type = "number")]
    pub max_total_tokens: u64,
    pub request_timeout_seconds: u32,
}
impl ApiWorkerSelection {
    pub fn validate(&self) -> Result<(), String> {
        self.target.url()?;
        crate::reasoning::validate_api(&self.target, &self.model, self.reasoning_effort)?;
        require(
            self.write_scope.is_none()
                || self
                    .file_tools
                    .contains(&crate::execution::ToolPermission::WriteFiles),
            "API write scope requires the write-files capability",
        )?;
        require(
            !self.file_tools.is_empty()
                && self.file_tools.len() <= 2
                && self.file_tools.iter().all(|p| {
                    matches!(
                        p,
                        crate::execution::ToolPermission::ReadFiles
                            | crate::execution::ToolPermission::WriteFiles
                    )
                })
                && (self.file_tools.len() != 2 || self.file_tools[0] != self.file_tools[1]),
            "API worker file permissions invalid",
        )?;
        identifier(self.target.provider().id())?;
        require(
            !self.model.is_empty()
                && self.model.len() <= 200
                && self
                    .model
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b)),
            "invalid API worker model",
        )?;
        require(
            (1..=128).contains(&self.max_rounds)
                && (1..=128).contains(&self.max_tool_calls)
                && self.max_output_tokens > 0
                && (1..=10_000_000).contains(&self.max_total_tokens)
                && (1..=600).contains(&self.request_timeout_seconds),
            "invalid API worker limits",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn provider_output_limits_preserve_numeric_contract_and_finite_task_limits() {
        let mut value = json!({
            "target": {"kind":"provider","endpoint":{"kind":"deepSeek"}},
            "model":"deepseek-flash","fileTools":["read-files","write-files"],
            "maxRounds":16,"maxToolCalls":24,"maxOutputTokens":8192,
            "maxTotalTokens":10000000,"requestTimeoutSeconds":120
        });
        for limit in [8192, 16384, 393216, u32::MAX] {
            value["maxOutputTokens"] = json!(limit);
            let worker: ApiWorkerSelection = serde_json::from_value(value.clone()).unwrap();
            worker.validate().unwrap();
            assert_eq!(serde_json::to_value(&worker).unwrap(), value);
            let mut invalid = worker.clone();
            invalid.max_output_tokens = 0;
            assert!(invalid.validate().is_err());
            invalid = worker.clone();
            invalid.max_total_tokens = 10_000_001;
            assert!(invalid.validate().is_err());
            invalid = worker;
            invalid.request_timeout_seconds = 601;
            assert!(invalid.validate().is_err());
        }
        for invalid in [json!(null), json!(-1), json!(4294967296_u64)] {
            value["maxOutputTokens"] = invalid;
            assert!(serde_json::from_value::<ApiWorkerSelection>(value.clone()).is_err());
        }
    }
}
