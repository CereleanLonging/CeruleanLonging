//! Bounded local file-tool errors, proven against the actual host_tool_result.
//! No successful file contents or write arguments enter management context.
use super::*;
use cerulean_domain::{execution::digest, supervision::SupervisionPolicy};
use serde_json::json;
pub(crate) type ErrorTexts = std::collections::BTreeMap<String, String>;
pub(crate) const MAX_ERROR_BYTES: usize = 1024;
const MAX_ENCODED_ERROR_BYTES: usize = 2048;
const MAX_FAILURE_CONTEXT_BYTES: usize = 24 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct FailureContext {
    schema_version: u32,
    incident_id: String,
    failures: Vec<Failure>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Failure {
    call_id: String,
    tool_name: String,
    result_sequence: u32,
    result_sha256: String,
    arguments_sha256: String,
    error: Option<String>,
}

pub(crate) fn validate_errors(tools: &ToolEvidence, errors: &ErrorTexts) -> Result<(), String> {
    require(errors.len() <= 128, "OMP error context exceeds tool bound")?;
    for (id, text) in errors {
        let call = tools
            .observation()
            .calls
            .get(id)
            .ok_or("OMP error lacks original tool result")?;
        let result = json!({"type":"host_tool_result","id":id,"result":{"content":[{"type":"text","text":text}]},"isError":true});
        require(
            text.len() <= MAX_ERROR_BYTES
                && !text.contains('\0')
                && call.result_is_error == Some(true)
                && call.result_sha256.as_deref() == Some(digest(&result)?.as_str()),
            "OMP error differs from original wire result",
        )?;
    }
    Ok(())
}
impl FailureContext {
    pub(super) fn from_tools(
        tools: &ToolEvidence,
        errors: &ErrorTexts,
        policy: &SupervisionPolicy,
        incident_id: &str,
    ) -> Result<Self, String> {
        validate_errors(tools, errors)?;
        let incident = tools
            .incidents(policy)?
            .into_iter()
            .find(|i| i.id == incident_id)
            .ok_or("OMP original incident missing")?;
        let mut calls = tools
            .observation()
            .calls
            .iter()
            .filter(|(_, c)| {
                c.result_sequence
                    .is_some_and(|s| s >= incident.first_sequence && s <= incident.trigger_sequence)
            })
            .collect::<Vec<_>>();
        calls.sort_by_key(|(_, c)| c.result_sequence);
        require(
            calls.len() == policy.repeated_failure_threshold as usize && calls.len() <= 8,
            "OMP original failure range changed",
        )?;
        let failures = calls
            .into_iter()
            .map(|(id, call)| {
                // Escaping control characters can expand a bounded UTF-8 error
                // sixfold. Omit oversized diagnostics rather than truncate text
                // and lose the original wire-result hash binding.
                let error = match errors.get(id) {
                    Some(text)
                        if serde_json::to_vec(text).map_err(|e| e.to_string())?.len()
                            <= MAX_ENCODED_ERROR_BYTES =>
                    {
                        Some(text.clone())
                    }
                    _ => None,
                };
                Ok(Failure {
                    call_id: id.clone(),
                    tool_name: call.tool_name.clone(),
                    result_sequence: call.result_sequence.ok_or("OMP result sequence missing")?,
                    result_sha256: call
                        .result_sha256
                        .clone()
                        .ok_or("OMP result hash missing")?,
                    arguments_sha256: call.arguments_sha256.clone(),
                    error,
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        let context = Self {
            schema_version: 1,
            incident_id: incident_id.into(),
            failures,
        };
        require(
            serde_json::to_vec(&context)
                .map_err(|e| e.to_string())?
                .len()
                <= MAX_FAILURE_CONTEXT_BYTES,
            "OMP failure context exceeds bound",
        )?;
        Ok(context)
    }
    pub(super) fn verify(
        &self,
        tools: &ToolEvidence,
        policy: &SupervisionPolicy,
        incident_id: &str,
    ) -> Result<(), String> {
        require(
            self.schema_version == 1
                && self.incident_id == incident_id
                && self.failures.len() == policy.repeated_failure_threshold as usize
                && self.failures.len() <= 8,
            "OMP saved failure context shape changed",
        )?;
        let errors = self
            .failures
            .iter()
            .filter_map(|f| {
                f.error
                    .as_ref()
                    .map(|text| (f.call_id.clone(), text.clone()))
            })
            .collect::<ErrorTexts>();
        let expected = Self::from_tools(tools, &errors, policy, incident_id)?;
        require(
            self == &expected,
            "OMP saved failure context differs from original results",
        )
    }
}
impl OmpToolEvidence {
    pub fn failure_context(
        &self,
        policy: &SupervisionPolicy,
        incident_id: &str,
    ) -> Result<serde_json::Value, String> {
        serde_json::to_value(FailureContext::from_tools(
            &self.tools,
            &self.errors,
            policy,
            incident_id,
        )?)
        .map_err(|e| e.to_string())
    }
}
