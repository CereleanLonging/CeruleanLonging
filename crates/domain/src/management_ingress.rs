//! Provider-neutral management handoff. A local source assertion is not remote
//! model attestation, a workflow application, or user approval.
use crate::{
    execution::{digest, identifier, require},
    management::{DecisionEnvelope, Purpose},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ManagementSource {
    CodexSession {
        session_id: String,
    },
    DeepSeek {
        model: String,
    },
    Provider {
        endpoint: crate::credentials::ProviderEndpoint,
        model: String,
        role: ManagementRole,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ManagementRole {
    Chief,
    Deputy,
}
impl ManagementRole {
    pub fn for_purpose(purpose: Purpose) -> Self {
        match purpose {
            Purpose::Plan | Purpose::Refine | Purpose::Accept => Self::Chief,
            _ => Self::Deputy,
        }
    }
}
impl ManagementSource {
    pub fn api_selection(&self) -> Result<(crate::credentials::ProviderEndpoint, &str), String> {
        match self {
            Self::DeepSeek { model } => {
                Ok((crate::credentials::ProviderEndpoint::DeepSeek {}, model))
            }
            Self::Provider {
                endpoint, model, ..
            } => Ok((endpoint.clone(), model)),
            Self::CodexSession { .. } => Err("external session is not an API provider".into()),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManagementHandoff {
    pub operation_id: String,
    pub project_id: String,
    pub purpose: Purpose,
    pub source: ManagementSource,
    pub state_digest: String,
    pub context: Value,
}
impl ManagementHandoff {
    pub fn validate(&self) -> Result<(), String> {
        identifier(&self.operation_id)?;
        identifier(&self.project_id)?;
        require(
            self.context.is_object()
                && serde_json::to_vec(&self.context)
                    .map_err(|_| "invalid context")?
                    .len()
                    <= self.purpose.context_limit_bytes()
                && digest(&self.context)? == self.state_digest,
            "management context is invalid or changed",
        )?;
        match &self.source {
            ManagementSource::Provider {
                endpoint,
                model,
                role,
            } => {
                crate::api_worker::ApiTarget::Provider {
                    endpoint: endpoint.clone(),
                }
                .url()?;
                require(
                    !model.is_empty()
                        && model.len() <= 200
                        && model
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b)),
                    "invalid management model",
                )?;
                require(
                    *role == ManagementRole::for_purpose(self.purpose),
                    "management source role differs from requested purpose",
                )
            }
            ManagementSource::CodexSession { session_id } => {
                identifier(session_id)?;
                require(
                    matches!(self.purpose, Purpose::Plan | Purpose::Accept),
                    "external chief may only plan or accept",
                )
            }
            ManagementSource::DeepSeek { model } => {
                require(
                    !model.is_empty()
                        && model.len() <= 200
                        && model
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"-_.:".contains(&b)),
                    "invalid deputy model",
                )?;
                require(
                    matches!(self.purpose, Purpose::Assign | Purpose::Advise),
                    "DeepSeek deputy may only assign or advise",
                )
            }
        }
    }
    pub fn check_current(&self, current_context: &Value) -> Result<(), String> {
        self.validate()?;
        require(
            digest(current_context)? == self.state_digest,
            "management context is stale",
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExternalChiefDecision {
    pub request_digest: String,
    pub session_id: String,
    pub envelope: DecisionEnvelope,
}

/// Authenticated local-user IPC only. These operations never grant approval.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "action",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ExternalChiefOperation {
    PreparePlan {
        project_id: String,
        setup_digest: String,
        session_id: String,
    },
    Read {
        operation_id: String,
    },
    Submit {
        operation_id: String,
        session_id: String,
        decision: ExternalChiefDecision,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExternalChiefResult {
    pub operation_id: String,
    pub request_digest: String,
    pub state: String,
    pub handoff: ManagementHandoff,
    pub message: Option<String>,
}
impl ExternalChiefDecision {
    /// expected_session is supplied by the local caller, never by model output.
    /// Environment/session metadata provides correlation, not cryptographic proof.
    pub fn validate_for(
        &self,
        request: &ManagementHandoff,
        current_context: &Value,
        expected_session: &str,
    ) -> Result<(), String> {
        request.check_current(current_context)?;
        require(
            matches!(&request.source, ManagementSource::CodexSession { session_id } if session_id == expected_session && session_id == &self.session_id),
            "external chief session mismatch",
        )?;
        require(
            self.request_digest == digest(request)?,
            "external chief request mismatch",
        )?;
        self.envelope
            .validate(request.purpose, &request.state_digest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::management::ManagementDecision;
    use serde_json::json;
    #[test]
    fn external_chief_binds_session_operation_state_and_role_without_approval_fields() {
        let context = json!({"revision":1});
        let mut request = ManagementHandoff {
            operation_id: "plan-208".into(),
            project_id: "project".into(),
            purpose: Purpose::Plan,
            source: ManagementSource::CodexSession {
                session_id: "current-session".into(),
            },
            state_digest: digest(&context).unwrap(),
            context: context.clone(),
        };
        let reply = ExternalChiefDecision {
            request_digest: digest(&request).unwrap(),
            session_id: "current-session".into(),
            envelope: DecisionEnvelope {
                state_digest: request.state_digest.clone(),
                decision: ManagementDecision::Wait {
                    reason: "needs user input".into(),
                },
            },
        };
        assert!(reply
            .validate_for(&request, &context, "current-session")
            .is_ok());
        assert!(reply
            .validate_for(&request, &context, "other-session")
            .is_err());
        assert!(reply
            .validate_for(&request, &json!({"revision":2}), "current-session")
            .is_err());
        request.operation_id = "other-operation".into();
        assert!(reply
            .validate_for(&request, &context, "current-session")
            .is_err());
        request.purpose = Purpose::Assign;
        assert!(request.validate().is_err());
        let mut value = serde_json::to_value(&reply).unwrap();
        value["approved"] = json!(true);
        assert!(serde_json::from_value::<ExternalChiefDecision>(value).is_err());
    }
    #[test]
    fn deputy_cannot_claim_chief_purposes_or_change_context() {
        let context = json!({"task":"review"});
        let mut request = ManagementHandoff {
            operation_id: "advice-208".into(),
            project_id: "project".into(),
            purpose: Purpose::Advise,
            source: ManagementSource::DeepSeek {
                model: "deepseek-flash".into(),
            },
            state_digest: digest(&context).unwrap(),
            context,
        };
        assert!(request.validate().is_ok());
        request.purpose = Purpose::Accept;
        assert!(request.validate().is_err());
        request.purpose = Purpose::Advise;
        request.context["task"] = json!("changed");
        assert!(request.validate().is_err());
    }
}
