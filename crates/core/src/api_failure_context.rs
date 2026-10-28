//! Bounded factual tool failures for the deputy, backed by original request/result hashes.
//! This adds no tool journal, dispatch authority or successful file content.
use super::*;
use cerulean_domain::supervision::SupervisionPolicy;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub(crate) const MAX_ERROR_BYTES: usize = 1024;
pub(crate) type ErrorTexts = BTreeMap<String, String>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct FailureContext {
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
    // None means unavailable or over the bound, never a successful/empty result.
    error: Option<String>,
    path: Option<String>,
    offset: Option<u64>,
}

pub(super) fn validate_errors(evidence: &ToolEvidence, errors: &ErrorTexts) -> Result<(), String> {
    require(errors.len() <= 128, "API failure text capacity exceeded")?;
    for (id, text) in errors {
        let call = evidence
            .observation()
            .calls
            .values()
            .find(|call| &call.tool_call_id == id)
            .ok_or("API failure text lacks original tool result")?;
        require(
            text.len() <= MAX_ERROR_BYTES
                && call.result_is_error == Some(true)
                && !call.cancelled
                && call.result_sha256.as_deref()
                    == Some(format!("{:x}", Sha256::digest(text.as_bytes())).as_str()),
            "API failure text differs from original error result",
        )?;
    }
    Ok(())
}

impl ApiToolEvidence {
    /// The caller separately validates the live checkpoint or stopped original
    /// evidence. Only existing round journals are read here; no second tool owner.
    pub(crate) fn failure_context(
        &self,
        root: &Path,
        policy: &SupervisionPolicy,
        incident_id: &str,
        errors: &ErrorTexts,
    ) -> Result<FailureContext, String> {
        validate_errors(&self.tools, errors)?;
        let incident = self
            .tools
            .incidents(policy)?
            .into_iter()
            .find(|i| i.id == incident_id)
            .ok_or("Original API failure incident missing")?;
        let mut selected = self
            .tools
            .observation()
            .calls
            .values()
            .filter(|c| {
                c.result_sequence
                    .is_some_and(|s| s >= incident.first_sequence && s <= incident.trigger_sequence)
            })
            .collect::<Vec<_>>();
        selected.sort_by_key(|c| c.result_sequence);
        require(
            selected.len() == policy.repeated_failure_threshold as usize && selected.len() <= 8,
            "API failure context differs from original trigger range",
        )?;
        // Arguments already exist in the original response. Expose only path
        // and read offset; never duplicate write contents or unrelated results.
        let mut proposals = BTreeMap::new();
        for round in 1..=self.call.round {
            let journal =
                ApiCallJournal::read_existing(&root.join("rounds").join(format!("{round:03}")))?;
            require(
                journal.healthy(),
                "API failure context response needs recovery",
            )?;
            crate::api_execution::check_round_scope(&self.run, journal.identity(), round)?;
            if round == self.call.round {
                require(
                    journal.identity() == &self.call && journal.head() == self.call_head,
                    "API failure context response changed",
                )?;
            }
            if journal.state().scoped_request.is_some() {
                journal.require_fee_ack()?;
            }
            let Some(CallOutcome::Reply { observation }) = &journal.state().outcome else {
                return Err("API failure context original response missing".into());
            };
            for call in &observation.tool_calls {
                if selected.iter().any(|c| c.tool_call_id == call.id) {
                    require(
                        proposals.insert(call.id.clone(), call.clone()).is_none(),
                        "API failure context tool identity repeated",
                    )?;
                }
            }
        }
        let mut failures = Vec::new();
        for call in selected {
            let proposal = proposals
                .get(&call.tool_call_id)
                .ok_or("API failure context original proposal missing")?;
            require(
                call.result_is_error == Some(true)
                    && !call.cancelled
                    && proposal.name == call.tool_name
                    && digest(&proposal.arguments)? == call.arguments_sha256,
                "API failure context tool proposal changed",
            )?;
            let path = proposal
                .arguments
                .get("path")
                .and_then(serde_json::Value::as_str)
                .filter(|p| p.len() <= 512)
                .map(String::from);
            let offset = if call.tool_name == "cerulean_read_file" {
                proposal
                    .arguments
                    .get("offset")
                    .and_then(serde_json::Value::as_u64)
            } else {
                None
            };
            failures.push(Failure {
                call_id: call.tool_call_id.clone(),
                tool_name: call.tool_name.clone(),
                result_sequence: call.result_sequence.ok_or("API failure sequence missing")?,
                result_sha256: call
                    .result_sha256
                    .clone()
                    .ok_or("API failure result missing")?,
                error: errors.get(&call.tool_call_id).cloned(),
                path,
                offset,
            });
        }
        Ok(FailureContext {
            schema_version: 1,
            incident_id: incident_id.into(),
            failures,
        })
    }
}

impl FailureContext {
    pub(crate) fn verify(
        &self,
        evidence: &ApiToolEvidence,
        root: &Path,
        policy: &SupervisionPolicy,
        incident: &str,
    ) -> Result<(), String> {
        require(
            self.schema_version == 1 && self.failures.len() <= 8 && self.incident_id == incident,
            "API failure context identity or size changed",
        )?;
        let mut errors = ErrorTexts::new();
        for item in &self.failures {
            if let Some(text) = &item.error {
                require(
                    errors.insert(item.call_id.clone(), text.clone()).is_none(),
                    "API failure context repeats a result",
                )?;
            }
        }
        require(
            self == &evidence.failure_context(root, policy, incident, &errors)?,
            "API failure context differs from original evidence",
        )
    }
}
