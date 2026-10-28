//! Explicit role selection; credentials never belong in project configuration.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ChiefSource {
    OpenAi {},
    Provider {
        endpoint: crate::credentials::ProviderEndpoint,
    },
    CodexSession {
        session_id: String,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum DeputySource {
    OpenAi {},
    DeepSeek {},
    Provider {
        endpoint: crate::credentials::ProviderEndpoint,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManagementSources {
    pub chief: ChiefSource,
    pub deputy: DeputySource,
}
impl ManagementSources {
    pub fn uses_provider_selection(&self) -> bool {
        matches!(self.chief, ChiefSource::Provider { .. })
            || matches!(self.deputy, DeputySource::Provider { .. })
    }
    pub fn uses_external_provider(&self) -> bool {
        !matches!(self.chief, ChiefSource::OpenAi {})
            || !matches!(self.deputy, DeputySource::OpenAi {})
    }
    pub fn validate(&self, chief_model: &str, monetary: bool) -> Result<(), String> {
        for endpoint in [
            match &self.chief {
                ChiefSource::Provider { endpoint } => Some(endpoint),
                _ => None,
            },
            match &self.deputy {
                DeputySource::Provider { endpoint } => Some(endpoint),
                _ => None,
            },
        ]
        .into_iter()
        .flatten()
        {
            crate::api_worker::ApiTarget::Provider {
                endpoint: endpoint.clone(),
            }
            .url()?;
        }
        if let ChiefSource::CodexSession { session_id } = &self.chief {
            identifier(session_id)?;
            require(
                chief_model == "external-codex-session",
                "当前Codex聊天使用external-codex-session标识，不伪造API模型身份",
            )?;
        }
        require(
            !monetary || !self.uses_external_provider(),
            "当前外部聊天及供应商管理来源尚不支持项目金额预算；请保留请求次数和时限约束",
        )
    }
}
