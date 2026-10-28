//! Fixed public categories only. Never retain provider text or parser errors.
use super::ApiFailure;
use serde::{Deserialize, Serialize};

// Keep the established journal wire representation. The live caller receives
// the bounded reason and includes it in its existing terminal error string;
// older readers must still decode the immutable failure journal. If a crash
// occurs before the terminal is saved, only the generic rejection is durable.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(super) enum PersistedFailure {
    NotSent {},
    LocalCancellation {},
    Transport {},
    Http { status: u16 },
    InvalidResponse {},
    Oversized {},
}
impl From<ApiFailure> for PersistedFailure {
    fn from(value: ApiFailure) -> Self {
        match value {
            ApiFailure::NotSent {} => Self::NotSent {},
            ApiFailure::LocalCancellation {} => Self::LocalCancellation {},
            ApiFailure::Transport {} => Self::Transport {},
            ApiFailure::Http { status } => Self::Http { status },
            ApiFailure::InvalidResponse {} | ApiFailure::ResponseRejected { .. } => {
                Self::InvalidResponse {}
            }
            ApiFailure::Oversized {} => Self::Oversized {},
        }
    }
}
impl From<PersistedFailure> for ApiFailure {
    fn from(value: PersistedFailure) -> Self {
        match value {
            PersistedFailure::NotSent {} => Self::NotSent {},
            PersistedFailure::LocalCancellation {} => Self::LocalCancellation {},
            PersistedFailure::Transport {} => Self::Transport {},
            PersistedFailure::Http { status } => Self::Http { status },
            PersistedFailure::InvalidResponse {} => Self::InvalidResponse {},
            PersistedFailure::Oversized {} => Self::Oversized {},
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResponseRejection {
    InvalidJson,
    CredentialEcho,
    UrlMismatch,
    OutputLimit,
    IncompleteOutput,
    InvalidToolArguments,
    UnregisteredTool,
    InvalidUsage,
    InvalidIdentity,
    InvalidText,
    InvalidCalls,
    NoActionableOutput,
    UnsupportedSchema,
}

impl ApiFailure {
    pub(super) fn response_rejected(error: String) -> Self {
        use ResponseRejection::*;
        // Only exact internal literals map to a category; arbitrary errors are
        // discarded, including any future error that embeds provider content.
        let reason = match error.as_str() {
            "API output token limit reached" => OutputLimit,
            "API response incomplete"
            | "API output item incomplete"
            | "API finish reason incomplete or inconsistent"
            | "API stop reason incomplete or inconsistent" => IncompleteOutput,
            "API tool arguments are incomplete or invalid"
            | "API tool arguments must be an object" => InvalidToolArguments,
            "API requested an unregistered tool" => UnregisteredTool,
            "API usage missing or invalid" => InvalidUsage,
            "invalid API identity" => InvalidIdentity,
            "API text exceeds bounds or is invalid" | "API response text too large" => InvalidText,
            "API calls repeated or arguments invalid" => InvalidCalls,
            "API reply has no actionable output or too many calls" => NoActionableOutput,
            _ => UnsupportedSchema,
        };
        Self::ResponseRejected { reason }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_generation::{
        tests::{reply, request},
        wire, ApiTarget, ProviderEndpoint,
    };
    use serde_json::json;

    #[test]
    fn legacy_failure_stays_byte_compatible_and_unknown_text_is_discarded() {
        let old = r#"{"kind":"invalidResponse"}"#;
        let failure: ApiFailure = serde_json::from_str(old).unwrap();
        assert_eq!(serde_json::to_string(&failure).unwrap(), old);
        let failure =
            ApiFailure::response_rejected("sensitive-provider-text sk-test-secret".into());
        assert_eq!(serde_json::to_string(&failure).unwrap(), old);
        assert!(!format!("{failure:?}").contains("sensitive"));
        assert!(serde_json::from_str::<ApiFailure>(
            r#"{"kind":"responseRejected","reason":"provider-secret"}"#
        )
        .is_err());
    }

    #[test]
    fn rejected_chat_responses_have_bounded_distinct_causes() {
        let r = request(ApiTarget::Provider {
            endpoint: ProviderEndpoint::DeepSeek {},
        });
        for (case, expected) in [
            ("length", ResponseRejection::OutputLimit),
            ("arguments", ResponseRejection::InvalidToolArguments),
            ("tool", ResponseRejection::UnregisteredTool),
            ("usage", ResponseRejection::InvalidUsage),
            ("identity", ResponseRejection::InvalidIdentity),
        ] {
            let mut value = reply(&r, true);
            match case {
                "length" => {
                    value["choices"][0]["finish_reason"] = json!("length");
                    value["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"] =
                        json!("{truncated");
                }
                "arguments" => {
                    value["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"] =
                        json!("{bad")
                }
                "tool" => {
                    value["choices"][0]["message"]["tool_calls"][0]["function"]["name"] =
                        json!("secret-unregistered")
                }
                "usage" => value["usage"]["prompt_tokens"] = json!(-1),
                "identity" => value["id"] = json!("invalid identity"),
                _ => unreachable!(),
            }
            let failure = ApiFailure::response_rejected(wire::parse(&r, value).unwrap_err());
            assert_eq!(failure, ApiFailure::ResponseRejected { reason: expected });
        }
    }
}
