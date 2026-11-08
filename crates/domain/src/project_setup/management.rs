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
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum DeputySource {
    OpenAi {},
    DeepSeek {},
    Provider {
        endpoint: crate::credentials::ProviderEndpoint,
    },
    Omp {
        executable: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        executable_sha256: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        runtime_parent: Option<String>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManagementSources {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chief_reasoning_effort: Option<crate::reasoning::ReasoningEffort>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deputy_reasoning_effort: Option<crate::reasoning::ReasoningEffort>,
    pub chief: ChiefSource,
    pub deputy: DeputySource,
}
impl ManagementSources {
    pub fn deputy_omp_worker(
        &self,
        model: &str,
    ) -> Result<crate::execution::WorkerSelection, String> {
        let DeputySource::Omp {
            executable,
            executable_sha256,
            runtime_parent,
        } = &self.deputy
        else {
            return Err("副指挥当前未选择 OMP".into());
        };
        let worker = crate::execution::WorkerSelection {
            executable: crate::execution::ValidationFile {
                path: executable.clone(),
                sha256: executable_sha256.clone().ok_or("副指挥 OMP 程序尚未固定")?,
            },
            provider: "deepseek".into(),
            model: model.into(),
            reasoning_effort: self.deputy_reasoning_effort,
            omp_runtime_parent: runtime_parent.clone(),
        };
        worker.validate()?;
        Ok(worker)
    }
    pub fn validate_reasoning(&self, chief_model: &str, deputy_model: &str) -> Result<(), String> {
        use crate::api_worker::ApiTarget;
        let chief_target = match &self.chief {
            ChiefSource::OpenAi {} => Some(ApiTarget::OpenAi {}),
            ChiefSource::Provider { endpoint } => Some(ApiTarget::Provider {
                endpoint: endpoint.clone(),
            }),
            ChiefSource::CodexSession { .. } => None,
        };
        if let Some(target) = chief_target {
            crate::reasoning::validate_api(&target, chief_model, self.chief_reasoning_effort)?;
        } else {
            require(
                self.chief_reasoning_effort.is_none(),
                "主指挥后台接入沿用绑定会话的思考配置",
            )?;
        }
        let deputy_target = match &self.deputy {
            DeputySource::OpenAi {} => ApiTarget::OpenAi {},
            DeputySource::DeepSeek {} => ApiTarget::Provider {
                endpoint: crate::credentials::ProviderEndpoint::DeepSeek {},
            },
            DeputySource::Provider { endpoint } => ApiTarget::Provider {
                endpoint: endpoint.clone(),
            },
            DeputySource::Omp { .. } => ApiTarget::Provider {
                endpoint: crate::credentials::ProviderEndpoint::DeepSeek {},
            },
        };
        crate::reasoning::validate_api(&deputy_target, deputy_model, self.deputy_reasoning_effort)
    }
    pub fn uses_provider_selection(&self) -> bool {
        matches!(self.chief, ChiefSource::Provider { .. })
            || matches!(self.deputy, DeputySource::Provider { .. })
    }
    pub fn uses_external_provider(&self) -> bool {
        !matches!(self.chief, ChiefSource::OpenAi {})
            || !matches!(self.deputy, DeputySource::OpenAi {})
    }
    pub fn validate(&self, chief_model: &str, monetary: bool) -> Result<(), String> {
        if let DeputySource::Omp {
            executable,
            executable_sha256,
            runtime_parent,
        } = &self.deputy
        {
            require(
                std::path::Path::new(executable).is_absolute(),
                "请填写副指挥 OMP 程序绝对路径",
            )?;
            if let Some(hash) = executable_sha256 {
                crate::execution::sha256(hash)?;
            }
            if let Some(parent) = runtime_parent {
                require(
                    std::path::Path::new(parent).is_absolute(),
                    "副指挥 OMP 运行目录必须为绝对路径",
                )?;
            }
            require(
                self.deputy_reasoning_effort.is_some(),
                "请确认副指挥 OMP 的思考强度",
            )?;
        }
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
