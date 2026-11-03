//! Provider response evidence survives an unusable decision. Raw answer and
//! reasoning text are never serialized into the management ledger.
use super::*;
use cerulean_domain::{credentials::CredentialProvider, execution::sha256};
use serde_json::Value;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeputyRequest {
    pub handoff: ManagementHandoff,
    pub wire: RequestFingerprint,
}
impl DeputyRequest {
    pub fn validate(&self) -> Result<(), String> {
        self.handoff.validate()?;
        let (endpoint, model) = self.handoff.source.api_selection()?;
        require(
            self.wire.target == (ApiTarget::Provider { endpoint })
                && self.wire.requested_model == model
                && self.wire.tool_catalog_sha256 == digest(&Vec::<Value>::new())?
                && self.wire.max_output_tokens
                    == if matches!(self.handoff.purpose, Purpose::Plan | Purpose::Refine) {
                        4096
                    } else {
                        2048
                    }
                && self.wire.timeout_seconds == 90,
            "deputy wire differs from source or bounded tool-free policy",
        )?;
        sha256(&self.wire.body_sha256)?;
        require(
            self.wire
                == crate::api_generation::request_fingerprint(&super::request(&self.handoff)?)?,
            "deputy wire does not encode original handoff",
        )
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeputyBilling {
    pub response_id: String,
    pub actual_model: String,
    pub usage: ApiUsage,
}
impl DeputyBilling {
    pub fn validate_for(&self, provider: CredentialProvider) -> Result<(), String> {
        ApiObservation {
            response_id: self.response_id.clone(),
            actual_model: self.actual_model.clone(),
            usage: self.usage.clone(),
            text: "billing metadata".into(),
            tool_calls: vec![],
        }
        .validate()?;
        self.usage.total_tokens(provider)?;
        require(
            provider == CredentialProvider::Anthropic || self.usage.cache_write_tokens.is_none(),
            "DeepSeek cache-write usage is not part of this wire contract",
        )
    }
    fn from_value(v: &Value, provider: CredentialProvider) -> Result<Self, String> {
        if provider == CredentialProvider::Anthropic {
            let n = |v: &Value| v.as_u64().ok_or("invalid management token count");
            let optional = |v: &Value| {
                if v.is_null() {
                    Ok(None)
                } else {
                    n(v).map(Some)
                }
            };
            let result = Self {
                response_id: v["id"].as_str().ok_or("response ID missing")?.into(),
                actual_model: v["model"].as_str().ok_or("response model missing")?.into(),
                usage: ApiUsage {
                    input_tokens: n(&v["usage"]["input_tokens"])?,
                    output_tokens: n(&v["usage"]["output_tokens"])?,
                    cache_read_tokens: optional(&v["usage"]["cache_read_input_tokens"])?,
                    cache_write_tokens: optional(&v["usage"]["cache_creation_input_tokens"])?,
                },
            };
            result.validate_for(provider)?;
            return Ok(result);
        }
        let number = |v: &Value| v.as_u64().ok_or("invalid deputy token count");
        let read = &v["usage"]["prompt_cache_hit_tokens"];
        let fallback = &v["usage"]["prompt_tokens_details"]["cached_tokens"];
        let read = if !read.is_null() {
            Some(number(read)?)
        } else if !fallback.is_null() {
            Some(number(fallback)?)
        } else {
            None
        };
        let result = Self {
            response_id: v["id"].as_str().ok_or("response ID missing")?.into(),
            actual_model: v["model"].as_str().ok_or("response model missing")?.into(),
            usage: ApiUsage {
                input_tokens: number(&v["usage"]["prompt_tokens"])?,
                output_tokens: number(&v["usage"]["completion_tokens"])?,
                cache_read_tokens: read,
                cache_write_tokens: None,
            },
        };
        result.validate_for(provider)?;
        Ok(result)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum DeputyOutcome {
    Received {
        response_sha256: String,
        billing: Option<DeputyBilling>,
        envelope: Option<Box<DecisionEnvelope>>,
    },
    Failed {
        failure: ApiFailure,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeputyResult {
    pub request: DeputyRequest,
    pub outcome: DeputyOutcome,
    // Observational only. Persisted separately by the host so strict old readers
    // and the original decision/billing hashes retain their exact wire format.
    #[serde(skip)]
    pub decode_stage: Option<DecodeStage>,
}
impl DeputyResult {
    pub fn validate(&self, expected: &DeputyRequest) -> Result<(), String> {
        if let Some(stage) = self.decode_stage {
            stage.validate_result(self)?;
        }
        self.request.validate()?;
        require(
            digest(&self.request)? == digest(expected)?,
            "deputy response belongs to another original request",
        )?;
        if let DeputyOutcome::Received {
            response_sha256,
            billing,
            envelope,
        } = &self.outcome
        {
            sha256(response_sha256)?;
            if let Some(bill) = billing {
                bill.validate_for(expected.wire.target.provider())?;
            }
            if let Some(e) = envelope {
                require(billing.is_some(), "deputy decision lacks provider usage")?;
                e.validate(expected.handoff.purpose, &expected.handoff.state_digest)?;
                validate_target(&expected.handoff, e)?;
            }
        }
        Ok(())
    }
    pub fn envelope(&self) -> Option<&DecisionEnvelope> {
        match &self.outcome {
            DeputyOutcome::Received { envelope, .. } => envelope.as_deref(),
            _ => None,
        }
    }
    pub fn billing(&self) -> Option<&DeputyBilling> {
        match &self.outcome {
            DeputyOutcome::Received { billing, .. } => billing.as_ref(),
            _ => None,
        }
    }
}
pub(super) fn validate_target(h: &ManagementHandoff, e: &DecisionEnvelope) -> Result<(), String> {
    if h.purpose == Purpose::Advise && h.context["supervision"].is_object() {
        let task = h.context["supervision"]["taskId"]
            .as_str()
            .ok_or("supervision task missing")?;
        require(
            matches!(&e.decision,cerulean_domain::management::ManagementDecision::Advise{task_id,..} if task_id==task)
                || matches!(
                    e.decision,
                    cerulean_domain::management::ManagementDecision::Wait { .. }
                ),
            "supervision decision targets another task",
        )?;
    }
    Ok(())
}
impl PreparedDeputy {
    fn decode_evidenced(&self, value: Value) -> DeputyResult {
        let billing = DeputyBilling::from_value(&value, self.fingerprint().target.provider()).ok();
        let response_sha256 = digest(&value).expect("JSON value serialization");
        let decoded = if billing.is_some() {
            self.prepared
                .decode(value)
                .map_err(|_| DecodeStage::Wire)
                .and_then(|reply| diagnostics::envelope(&self.handoff, reply.observation()))
        } else {
            Err(DecodeStage::Billing)
        };
        let stage = decoded
            .as_ref()
            .map(|_| DecodeStage::Accepted)
            .unwrap_or_else(|stage| *stage);
        DeputyResult {
            request: self.request(),
            decode_stage: Some(stage),
            outcome: DeputyOutcome::Received {
                response_sha256,
                billing,
                envelope: decoded.ok().map(Box::new),
            },
        }
    }
}
/// One POST. Successful HTTP billing survives malformed JSON decisions, wrong
/// roles, tool attempts and incomplete choices. Unknown usage remains unknown.
pub async fn send_evidenced(
    client: &ApiClient,
    prepared: PreparedDeputy,
    cancel: &mut tokio::sync::oneshot::Receiver<()>,
) -> DeputyResult {
    let (result, transport) = client.send_evidence_json(&prepared.prepared, cancel).await;
    evidenced_result(prepared, result, transport)
}
/// Same original response wire; only a bounded out-of-band header observer is added.
pub async fn send_evidenced_with_gateway(
    client: &ApiClient,
    prepared: PreparedDeputy,
    cancel: &mut tokio::sync::oneshot::Receiver<()>,
    gateway: &mut crate::api_generation::GatewayObserver<'_>,
) -> DeputyResult {
    let (result, transport) = client
        .send_evidence_json_with_gateway(&prepared.prepared, cancel, gateway)
        .await;
    evidenced_result(prepared, result, transport)
}
fn evidenced_result(
    prepared: PreparedDeputy,
    result: Result<Value, ApiFailure>,
    transport: Option<crate::api_generation::TransportStage>,
) -> DeputyResult {
    match result {
        Ok(value) => prepared.decode_evidenced(value),
        Err(failure) => DeputyResult {
            request: prepared.request(),
            decode_stage: transport.map(DecodeStage::transport),
            outcome: DeputyOutcome::Failed { failure },
        },
    }
}

#[cfg(any(test, feature = "test-fixtures"))]
pub fn fixture_result(prepared: &PreparedDeputy, value: Value) -> DeputyResult {
    prepared.decode_evidenced(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cerulean_domain::secret::SecretInput;
    fn prepared() -> PreparedDeputy {
        let context = json!({"taskId":"sum"});
        prepare(
            &ApiClient::new(SecretInput::new("invalid-fixture-value".into()).unwrap()).unwrap(),
            ManagementHandoff {
                operation_id: "deputy-211".into(),
                project_id: "project".into(),
                purpose: Purpose::Advise,
                source: ManagementSource::DeepSeek {
                    model: "deepseek-flash".into(),
                },
                state_digest: digest(&context).unwrap(),
                context,
            },
        )
        .unwrap()
    }
    fn reply(p: &PreparedDeputy) -> Value {
        json!({"id":"ds-reply","model":"deepseek-flash-version","choices":[{"index":0,"finish_reason":"stop","message":{"role":"assistant","content":json!({"stateDigest":p.handoff.state_digest,"decision":{"action":"advise","taskId":"sum","suggestion":"check inputs","rationale":"bounded review"}}).to_string(),"reasoning_content":"PRIVATE_REASONING_MUST_NOT_BE_SAVED"}}],"usage":{"prompt_tokens":20,"completion_tokens":9,"prompt_cache_hit_tokens":4}})
    }
    #[test]
    fn diagnostic_281_classifies_rejection_without_changing_legacy_wire_or_preserving_raw_text() {
        let mut p = prepared();
        p.handoff.context = json!({"supervision":{"taskId":"sum"}});
        p.handoff.state_digest = digest(&p.handoff.context).unwrap();
        // Re-prepare to bind the modified context to the request fingerprint.
        let client =
            ApiClient::new(SecretInput::new("invalid-fixture-value".into()).unwrap()).unwrap();
        let p = prepare(&client, p.handoff).unwrap();
        for (case, expected) in [
            (0, DecodeStage::Accepted),
            (1, DecodeStage::Billing),
            (2, DecodeStage::Wire),
            (3, DecodeStage::DecisionJson),
            (4, DecodeStage::SchemaEnvelopeExtraFields),
            (5, DecodeStage::StateDigest),
            (6, DecodeStage::Role),
            (7, DecodeStage::DecisionValidation),
            (8, DecodeStage::SupervisionTarget),
        ] {
            let mut v = reply(&p);
            let mut content: Value =
                serde_json::from_str(v["choices"][0]["message"]["content"].as_str().unwrap())
                    .unwrap();
            match case {
                1 => v["usage"]["prompt_tokens"] = Value::Null,
                2 => v["choices"][0]["finish_reason"] = json!("length"),
                4 => content["unexpected"] = json!("RAW_DIAGNOSTIC_MUST_NOT_LEAK"),
                5 => content["stateDigest"] = json!("0".repeat(64)),
                6 => {
                    content["decision"] = json!({"action":"accept","taskId":"sum","attemptId":"a1","rationale":"role mismatch"})
                }
                7 => content["decision"]["rationale"] = json!(""),
                8 => content["decision"]["taskId"] = json!("different-task"),
                _ => (),
            }
            v["choices"][0]["message"]["content"] = if case == 3 {
                json!("RAW_DIAGNOSTIC_MUST_NOT_LEAK")
            } else {
                json!(content.to_string())
            };
            let r = fixture_result(&p, v);
            r.validate(&p.request()).unwrap();
            assert_eq!(r.decode_stage, Some(expected), "case {case}");
            let bytes = serde_json::to_vec(&r).unwrap();
            let wire: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(wire.as_object().unwrap().len(), 2);
            assert_eq!(wire["outcome"].as_object().unwrap().len(), 4);
            let text = String::from_utf8(bytes.clone()).unwrap();
            assert!(!text.contains("PRIVATE_REASONING_MUST_NOT_BE_SAVED"));
            assert!(!text.contains("RAW_DIAGNOSTIC_MUST_NOT_LEAK"));
            let legacy: DeputyResult = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(legacy.decode_stage, None);
            assert_eq!(digest(&legacy).unwrap(), digest(&r).unwrap());
            legacy.validate(&p.request()).unwrap();
        }
    }
    #[test]
    fn deputy_evidence_keeps_usage_when_decision_is_unusable_and_never_saves_reasoning() {
        let p = prepared();
        for case in 0..6 {
            let mut v = reply(&p);
            match case {
                1=>v["choices"][0]["message"]["content"]=json!("not a decision"),
                2=>v["choices"][0]["message"]["content"]=json!(json!({"stateDigest":p.handoff.state_digest,"decision":{"action":"accept","taskId":"sum","attemptId":"a1","rationale":"wrong role"}}).to_string()),
                3=>v["choices"][0]["finish_reason"]=json!("length"),
                4=>v["choices"][0]["message"]["tool_calls"]=json!([{"id":"c1","type":"function","function":{"name":"cerulean_write","arguments":"{}"}}]),
                5=>v["choices"]=json!([]),_=>{}
            }
            let r = fixture_result(&p, v);
            r.validate(&p.request()).unwrap();
            assert_eq!(r.envelope().is_some(), case == 0);
            assert_eq!(r.billing().unwrap().usage.input_tokens, 20);
            assert!(!serde_json::to_string(&r)
                .unwrap()
                .contains("PRIVATE_REASONING_MUST_NOT_BE_SAVED"));
        }
    }
    #[test]
    fn deputy_evidence_rejects_other_request_and_does_not_invent_missing_usage() {
        let p = prepared();
        let r = fixture_result(&p, reply(&p));
        let mut other = p.request();
        other.handoff.operation_id = "another".into();
        assert!(r.validate(&other).is_err());
        for invalid in [Value::Null, json!("20"), json!(-1), json!(1.5)] {
            let mut v = reply(&p);
            v["usage"]["prompt_tokens"] = invalid;
            let r = fixture_result(&p, v);
            r.validate(&p.request()).unwrap();
            assert!(r.billing().is_none() && r.envelope().is_none());
        }
    }
}
