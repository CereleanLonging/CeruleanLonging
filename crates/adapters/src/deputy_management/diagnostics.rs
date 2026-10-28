//! Fixed, non-sensitive decode stages. Never persist parser text, raw model
//! output, or private reasoning as a diagnostic.
use super::*;
use cerulean_domain::management::ManagementDecision;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DecodeStage {
    TransportConnect,
    TransportConnectTimeout,
    TransportRequestSend,
    TransportRequestTimeout,
    TransportBodyRead,
    TransportBodyTimeout,
    Accepted,
    Billing,
    Wire,
    ToolCall,
    DecisionJson,
    DecisionSchema,
    SchemaEnvelopeObject,
    SchemaEnvelopeExtraFields,
    SchemaFieldNaming,
    SchemaStateDigestField,
    SchemaDecisionField,
    SchemaAction,
    SchemaTaskId,
    SchemaAttemptId,
    SchemaWorkerId,
    SchemaRationale,
    SchemaSuggestion,
    SchemaReason,
    SchemaTasks,
    SchemaDecisionExtraFields,
    StateDigest,
    Role,
    DecisionValidation,
    SupervisionTarget,
}

pub(super) fn envelope(
    h: &ManagementHandoff,
    observation: &ApiObservation,
) -> Result<DecisionEnvelope, DecodeStage> {
    if !observation.tool_calls.is_empty() {
        return Err(DecodeStage::ToolCall);
    }
    let value: serde_json::Value =
        serde_json::from_str(&observation.text).map_err(|_| DecodeStage::DecisionJson)?;
    let envelope: DecisionEnvelope =
        serde_json::from_value(value.clone()).map_err(|_| schema_stage(&value))?;
    if envelope.state_digest != h.state_digest {
        return Err(DecodeStage::StateDigest);
    }
    let role_ok = match &envelope.decision {
        ManagementDecision::Plan { .. } => h.purpose == Purpose::Plan,
        ManagementDecision::Assign { .. } => h.purpose == Purpose::Assign,
        ManagementDecision::Accept { .. } => h.purpose == Purpose::Accept,
        ManagementDecision::Advise { .. } => h.purpose == Purpose::Advise,
        ManagementDecision::Wait { .. } => true,
    };
    if !role_ok {
        return Err(DecodeStage::Role);
    }
    envelope
        .validate(h.purpose, &h.state_digest)
        .map_err(|_| DecodeStage::DecisionValidation)?;
    evidence::validate_target(h, &envelope).map_err(|_| DecodeStage::SupervisionTarget)?;
    Ok(envelope)
}

// Called only after the authoritative serde schema rejects the answer. These
// fixed facts never repair/accept input or retain arbitrary property names.
fn schema_stage(value: &serde_json::Value) -> DecodeStage {
    use DecodeStage::*;
    let Some(root) = value.as_object() else {
        return SchemaEnvelopeObject;
    };
    let decision = &value["decision"];
    if root.contains_key("state_digest")
        || ["task_id", "attempt_id", "worker_id"]
            .iter()
            .any(|key| decision.get(key).is_some())
    {
        return SchemaFieldNaming;
    }
    if !value["stateDigest"].is_string() {
        return SchemaStateDigestField;
    }
    let Some(object) = decision.as_object() else {
        return SchemaDecisionField;
    };
    if root
        .keys()
        .any(|k| !["stateDigest", "decision"].contains(&k.as_str()))
    {
        return SchemaEnvelopeExtraFields;
    }
    let fields: &[(&str, DecodeStage)] = match decision["action"].as_str() {
        Some("accept") => &[
            ("taskId", SchemaTaskId),
            ("attemptId", SchemaAttemptId),
            ("rationale", SchemaRationale),
        ],
        Some("assign") => &[
            ("taskId", SchemaTaskId),
            ("workerId", SchemaWorkerId),
            ("rationale", SchemaRationale),
        ],
        Some("advise") => &[
            ("taskId", SchemaTaskId),
            ("suggestion", SchemaSuggestion),
            ("rationale", SchemaRationale),
        ],
        Some("wait") => &[("reason", SchemaReason)],
        Some("plan") => &[("tasks", SchemaTasks), ("rationale", SchemaRationale)],
        _ => return SchemaAction,
    };
    for (field, stage) in fields {
        if if *field == "tasks" {
            !decision[field].is_array()
        } else {
            !decision[field].is_string()
        } {
            return *stage;
        }
    }
    if object
        .keys()
        .any(|k| k != "action" && !fields.iter().any(|(name, _)| name == k))
    {
        return SchemaDecisionExtraFields;
    }
    DecisionSchema
}

impl DecodeStage {
    pub fn is_transport(&self) -> bool {
        matches!(
            self,
            Self::TransportConnect
                | Self::TransportConnectTimeout
                | Self::TransportRequestSend
                | Self::TransportRequestTimeout
                | Self::TransportBodyRead
                | Self::TransportBodyTimeout
        )
    }
    pub(super) fn transport(stage: crate::api_generation::TransportStage) -> Self {
        use crate::api_generation::TransportStage as T;
        match stage {
            T::Connect => Self::TransportConnect,
            T::ConnectTimeout => Self::TransportConnectTimeout,
            T::RequestSend => Self::TransportRequestSend,
            T::RequestTimeout => Self::TransportRequestTimeout,
            T::BodyRead => Self::TransportBodyRead,
            T::BodyTimeout => Self::TransportBodyTimeout,
        }
    }
    pub fn validate_result(&self, result: &DeputyResult) -> Result<(), String> {
        if self.is_transport() {
            return require(
                matches!(
                    result.outcome,
                    DeputyOutcome::Failed {
                        failure: ApiFailure::Transport {}
                    }
                ),
                "transport stage requires the original transport failure",
            );
        }
        let DeputyOutcome::Received {
            billing, envelope, ..
        } = &result.outcome
        else {
            return Err("decode stage requires a received provider response".into());
        };
        require(
            match self {
                Self::Accepted => billing.is_some() && envelope.is_some(),
                Self::Billing => billing.is_none() && envelope.is_none(),
                _ => billing.is_some() && envelope.is_none(),
            },
            "decode stage contradicts original response evidence",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn schema_283_reports_only_fixed_field_categories_and_never_relaxes_the_contract() {
        use DecodeStage::*;
        let original = json!({"stateDigest":"a".repeat(64),"decision":{"action":"accept","taskId":"task","attemptId":"attempt","rationale":"valid"}});
        for (case, expected) in [
            (0, SchemaEnvelopeObject),
            (1, SchemaFieldNaming),
            (2, SchemaStateDigestField),
            (3, SchemaDecisionField),
            (4, SchemaEnvelopeExtraFields),
            (5, SchemaAction),
            (6, SchemaTaskId),
            (7, SchemaAttemptId),
            (8, SchemaRationale),
            (9, SchemaDecisionExtraFields),
            (10, SchemaWorkerId),
            (11, SchemaSuggestion),
            (12, SchemaReason),
            (13, SchemaTasks),
        ] {
            let mut v = original.clone();
            match case {
                0 => v = json!([]),
                1 => {
                    v.as_object_mut().unwrap().remove("stateDigest");
                    v["state_digest"] = json!("a".repeat(64));
                }
                2 => v["stateDigest"] = json!(3),
                3 => v["decision"] = json!("not object"),
                4 => v["PRIVATE_FIELD_SENTINEL"] = json!("PRIVATE_VALUE_SENTINEL"),
                5 => v["decision"]["action"] = json!("unrecognized"),
                6 => {
                    v["decision"].as_object_mut().unwrap().remove("taskId");
                }
                7 => v["decision"]["attemptId"] = json!(7),
                8 => {
                    v["decision"].as_object_mut().unwrap().remove("rationale");
                }
                9 => v["decision"]["PRIVATE_FIELD_SENTINEL"] = json!("PRIVATE_VALUE_SENTINEL"),
                10 => v["decision"] = json!({"action":"assign","taskId":"task","rationale":"ok"}),
                11 => v["decision"] = json!({"action":"advise","taskId":"task","rationale":"ok"}),
                12 => v["decision"] = json!({"action":"wait"}),
                13 => v["decision"] = json!({"action":"plan","rationale":"ok"}),
                _ => unreachable!(),
            }
            assert!(serde_json::from_value::<DecisionEnvelope>(v.clone()).is_err());
            let stage = schema_stage(&v);
            assert_eq!(stage, expected, "case {case}");
            assert!(!serde_json::to_string(&stage).unwrap().contains("PRIVATE"));
        }
        assert!(serde_json::from_value::<DecisionEnvelope>(original).is_ok());
    }
}
