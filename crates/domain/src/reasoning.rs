//! Explicit model effort is part of the saved/approved configuration. Omitted
//! values preserve historical requests. Capabilities are protocol-specific.
use crate::{api_worker::ApiTarget, credentials::CredentialProvider, execution::require};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    None,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
    Ultra,
}

impl ReasoningEffort {
    pub fn id(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
            Self::Max => "max",
            Self::Ultra => "ultra",
        }
    }
    pub fn omp(self) -> &'static str {
        if self == Self::None {
            "off"
        } else {
            self.id()
        }
    }
}

// Reviewed provider contracts. Unknown models retain the provider default.
// Relay capability cannot be inferred from the model's advertised name.
pub fn api_options(target: &ApiTarget, model: &str) -> Vec<ReasoningEffort> {
    use ReasoningEffort::*;
    let m = model.to_ascii_lowercase();
    match target.provider() {
        CredentialProvider::OpenAi if matches!(m.as_str(), "gpt-6-astra" | "gpt-6.1-sol") => {
            vec![Low, Medium, High, Xhigh, Max]
        }
        CredentialProvider::OpenAi
            if matches!(m.as_str(), "gpt-5" | "gpt-5-mini" | "gpt-5-nano") =>
        {
            vec![Minimal, Low, Medium, High]
        }
        CredentialProvider::OpenAi if matches!(m.as_str(), "o3" | "o3-mini" | "o4-mini") => {
            vec![Low, Medium, High]
        }
        CredentialProvider::DeepSeek
            if matches!(
                m.as_str(),
                "deepseek-flash" | "deepseek-v4-pro" | "deepseek-v4-flash"
            ) =>
        {
            vec![None, Low, High, Max]
        }
        CredentialProvider::Qwen
            if matches!(
                m.as_str(),
                "qwen3.8-max"
                    | "qwen3.8-max-0902"
                    | "qwen3.8-flash"
                    | "qwen3.8-2.4t-a95b"
                    | "qwen3.8-27b"
            ) =>
        {
            vec![None, Low, Medium, Xhigh]
        }
        CredentialProvider::Anthropic
            if matches!(m.as_str(), "claude-opus-4-6" | "claude-sonnet-4-6") =>
        {
            vec![Low, Medium, High, Max]
        }
        CredentialProvider::Anthropic
            if matches!(
                m.as_str(),
                "claude-opus-4-7"
                    | "claude-opus-4-8"
                    | "claude-opus-5"
                    | "claude-opus-5-5"
                    | "claude-sonnet-5"
                    | "claude-sonnet-5-5"
                    | "claude-haiku-5-5"
                    | "claude-fable-5"
                    | "claude-fable-5-1"
                    | "claude-mythos-5"
                    | "claude-mythos-5-1"
            ) =>
        {
            vec![Low, Medium, High, Xhigh, Max]
        }
        CredentialProvider::Grok if matches!(m.as_str(), "grok-4.6" | "grok-4.7") => {
            vec![Low, Medium, High, Xhigh]
        }
        CredentialProvider::Grok if m == "grok-4.5" => vec![Low, Medium, High],
        _ => vec![],
    }
}

pub fn validate_api(
    target: &ApiTarget,
    model: &str,
    effort: Option<ReasoningEffort>,
) -> Result<(), String> {
    require(
        effort.is_none_or(|value| api_options(target, model).contains(&value)),
        "当前接入与模型尚未确认支持所选思考强度，请重新选择模型与思考强度",
    )
}

pub fn apply_api(
    body: &mut Value,
    target: &ApiTarget,
    model: &str,
    effort: Option<ReasoningEffort>,
) -> Result<(), String> {
    validate_api(target, model, effort)?;
    let Some(effort) = effort else {
        return Ok(());
    };
    match target.provider() {
        CredentialProvider::OpenAi => body["reasoning"] = json!({"effort": effort}),
        CredentialProvider::Anthropic => {
            body["thinking"] = json!({"type": "adaptive"});
            body["output_config"] = json!({"effort": effort});
        }
        _ => body["reasoning_effort"] = json!(effort),
    }
    Ok(())
}

/// The context is constructed and digest-bound by the core before dispatch.
pub fn management_effort(
    context: &Value,
    purpose: crate::management::Purpose,
) -> Result<Option<ReasoningEffort>, String> {
    let key = match purpose {
        crate::management::Purpose::Plan
        | crate::management::Purpose::Refine
        | crate::management::Purpose::Accept => "chiefReasoningEffort",
        _ => "deputyReasoningEffort",
    };
    let value = if purpose == crate::management::Purpose::Refine {
        &context[key]
    } else {
        &context["executionSetup"]["setup"]["managementSources"][key]
    };
    serde_json::from_value(value.clone()).map_err(|_| "管理模型思考强度配置无效".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        credentials::ProviderEndpoint, execution::digest, project_setup::ManagementSources,
    };

    #[test]
    fn legacy_settings_keep_original_bytes_and_new_settings_change_approval_identity() {
        let original = json!({"chief":{"kind":"openAi"},"deputy":{"kind":"deepSeek"}});
        let mut sources: ManagementSources = serde_json::from_value(original.clone()).unwrap();
        assert_eq!(serde_json::to_value(&sources).unwrap(), original);
        let before = digest(&sources).unwrap();
        sources.deputy_reasoning_effort = Some(ReasoningEffort::High);
        assert_ne!(digest(&sources).unwrap(), before);
        sources
            .validate_reasoning("gpt-6-astra", "deepseek-flash")
            .unwrap();
        sources.deputy_reasoning_effort = Some(ReasoningEffort::Medium);
        assert!(sources
            .validate_reasoning("gpt-6-astra", "deepseek-flash")
            .is_err());
    }

    #[test]
    fn protocol_specific_fields_and_unsupported_choices_fail_closed() {
        for (target, model, expected) in [
            (
                ApiTarget::OpenAi {},
                "gpt-6-astra",
                json!({"reasoning":{"effort":"high"}}),
            ),
            (
                ApiTarget::Provider {
                    endpoint: ProviderEndpoint::DeepSeek {},
                },
                "deepseek-flash",
                json!({"reasoning_effort":"high"}),
            ),
            (
                ApiTarget::Provider {
                    endpoint: ProviderEndpoint::Anthropic {},
                },
                "claude-sonnet-4-6",
                json!({"thinking":{"type":"adaptive"},"output_config":{"effort":"high"}}),
            ),
        ] {
            let mut body = json!({});
            apply_api(&mut body, &target, model, None).unwrap();
            assert_eq!(body, json!({}));
            apply_api(&mut body, &target, model, Some(ReasoningEffort::High)).unwrap();
            assert_eq!(body, expected);
            assert!(apply_api(
                &mut body,
                &target,
                "unknown-model",
                Some(ReasoningEffort::High)
            )
            .is_err());
        }
        let relay = ApiTarget::Provider {
            endpoint: ProviderEndpoint::A6ApiVip {},
        };
        assert!(api_options(&relay, "deepseek-flash").is_empty());
    }

    #[test]
    fn role_and_refinement_efforts_are_read_from_digest_bound_context() {
        use crate::management::Purpose;
        let context = json!({"executionSetup":{"setup":{"managementSources":{"chiefReasoningEffort":"high","deputyReasoningEffort":"low"}}}});
        assert_eq!(
            management_effort(&context, Purpose::Plan).unwrap(),
            Some(ReasoningEffort::High)
        );
        assert_eq!(
            management_effort(&context, Purpose::Advise).unwrap(),
            Some(ReasoningEffort::Low)
        );
        assert_eq!(management_effort(&json!({}), Purpose::Plan).unwrap(), None);
        assert_eq!(
            management_effort(&json!({"chiefReasoningEffort":"medium"}), Purpose::Refine).unwrap(),
            Some(ReasoningEffort::Medium)
        );
        assert!(
            management_effort(&json!({"chiefReasoningEffort":"invented"}), Purpose::Refine)
                .is_err()
        );
    }
}
