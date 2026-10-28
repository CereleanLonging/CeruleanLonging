//! Compact supervision request evidence. The complete tool prefix stays in its
//! original journal; neither deserialization nor recovery creates live authority.
use super::{failure_context::FailureContext, *};
use crate::omp_tool_journal::OmpToolJournal;
use cerulean_domain::{
    execution::{digest, sha256},
    supervision::SupervisionPolicy,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OmpSupervisionReference {
    intent: DispatchIntent,
    tool_head: String,
    observation_digest: String,
    held_result_sha256: String,
    failure_context: FailureContext,
}

impl OmpSupervisionReference {
    pub fn from_evidence(
        evidence: &OmpToolEvidence,
        policy: &SupervisionPolicy,
        incident_id: &str,
    ) -> Result<Self, String> {
        evidence.intent.validate_metadata()?;
        sha256(evidence.tools.head())?;
        Ok(Self {
            intent: evidence.intent.clone(),
            tool_head: evidence.tools.head().into(),
            // The original journal can reconstruct these exact bytes. Error
            // texts are separate, checked projections of original wire hashes.
            observation_digest: digest(&(&evidence.intent, &evidence.tools))?,
            held_result_sha256: held_result(&evidence.tools)?.into(),
            failure_context: FailureContext::from_tools(
                &evidence.tools,
                &evidence.errors,
                policy,
                incident_id,
            )?,
        })
    }
    pub fn intent(&self) -> &DispatchIntent {
        &self.intent
    }
    pub fn tool_head(&self) -> &str {
        &self.tool_head
    }
    pub fn observation_digest(&self) -> &str {
        &self.observation_digest
    }
    pub fn held_result_sha256(&self) -> &str {
        &self.held_result_sha256
    }
    pub fn failure_context(&self) -> Result<serde_json::Value, String> {
        serde_json::to_value(&self.failure_context).map_err(|e| e.to_string())
    }
    pub fn validate_plan(&self, plan: &PlanRevision) -> Result<(), String> {
        validate_intent_plan(&self.intent, plan)
    }
    pub fn validate_live(
        &self,
        evidence: &OmpToolEvidence,
        policy: &SupervisionPolicy,
        incident_id: &str,
    ) -> Result<(), String> {
        let expected = Self::from_evidence(evidence, policy, incident_id)?;
        require(
            self == &expected,
            "OMP reference differs from the original live checkpoint",
        )
    }
    /// Inspection only. Opens the original journal under its existing lock,
    /// verifies the complete chain and selects the exact saved prefix.
    pub fn verify_original(
        &self,
        file_service: &Path,
        plan: &PlanRevision,
        policy: &SupervisionPolicy,
        incident_id: &str,
    ) -> Result<ToolEvidence, String> {
        self.validate_plan(plan)?;
        require(
            plan.supervision_for(&self.intent.task_id)? == Some(policy),
            "OMP reference policy differs from original plan",
        )?;
        sha256(&self.tool_head)?;
        sha256(&self.observation_digest)?;
        let tools = OmpToolJournal::read_evidence(
            &file_service.join("events"),
            &self.intent,
            &self.tool_head,
        )?;
        require(
            digest(&(&self.intent, &tools))? == self.observation_digest,
            "OMP reference observation differs from original journal prefix",
        )?;
        self.failure_context.verify(&tools, policy, incident_id)?;
        require(
            self.held_result_sha256 == held_result(&tools)?,
            "OMP original held result changed",
        )?;
        Ok(tools)
    }
}
fn held_result(tools: &ToolEvidence) -> Result<&str, String> {
    tools
        .observation()
        .calls
        .values()
        .max_by_key(|c| c.result_sequence)
        .and_then(|c| c.result_sha256.as_deref())
        .ok_or("OMP completed result missing".into())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use cerulean_adapters::omp_transport::worker::host_calls::HostEvent;
    use serde_json::json;

    fn fixture(root: &Path) -> (PlanRevision, DispatchIntent, OmpToolJournal) {
        let (store, mut plan) = crate::omp_execution::tests::assigned(root, false);
        let (_, _, mut intent, _) = crate::omp_execution::scope(&store, "task", "attempt").unwrap();
        plan.policy.supervision = Some(policy());
        intent.revision_digest = plan.approval_digest().unwrap();
        intent.prompt_sha256 = format!(
            "{:x}",
            Sha256::digest(
                crate::omp_execution::approved_prompt(&plan, "task")
                    .unwrap()
                    .as_bytes()
            )
        );
        std::fs::create_dir(root.join("tools")).unwrap();
        let journal = OmpToolJournal::create(&root.join("tools/events"), &intent).unwrap();
        (plan, intent, journal)
    }
    fn append(
        journal: &mut OmpToolJournal,
        intent: &DispatchIntent,
        id: &str,
        failed: bool,
        text: &str,
    ) {
        journal
            .record(
                intent,
                &HostEvent::Dispatch {
                    id: id.into(),
                    tool_call_id: id.into(),
                    tool_name: "cerulean_read_file".into(),
                    arguments_sha256: "a".repeat(64),
                },
            )
            .unwrap();
        let wire = json!({"type":"host_tool_result","id":id,"result":{"content":[{"type":"text","text":text}]},"isError":failed});
        journal
            .record(
                intent,
                &HostEvent::Result {
                    id: id.into(),
                    result_sha256: digest(&wire).unwrap(),
                    is_error: failed,
                },
            )
            .unwrap();
    }
    fn policy() -> SupervisionPolicy {
        SupervisionPolicy {
            schema_version: 1,
            repeated_failure_threshold: 2,
            max_advice_per_attempt: 1,
            delivery_enabled: false,
        }
    }

    #[test]
    fn maximum_tool_prefix_uses_bounded_reference_and_reconstructs_original() {
        let temp = tempfile::tempdir().unwrap();
        let (plan, intent, mut journal) = fixture(temp.path());
        let mut errors = ErrorTexts::new();
        for n in 0..128 {
            let id = format!("{n:03}{}", "x".repeat(253));
            let failed = n >= 126;
            let text = if failed {
                "Original file unavailable"
            } else {
                "file contents never sent to deputy"
            };
            append(&mut journal, &intent, &id, failed, text);
            if failed {
                errors.insert(id, text.into());
            }
        }
        let evidence = OmpToolCheckpoint::new(&intent, journal.checkpoint().unwrap(), errors)
            .unwrap()
            .evidence();
        let incident = evidence.tools.incidents(&policy()).unwrap().remove(0);
        let reference =
            OmpSupervisionReference::from_evidence(&evidence, &policy(), &incident.id).unwrap();
        assert!(serde_json::to_vec(&evidence).unwrap().len() > 64 * 1024);
        let serialized = serde_json::to_vec(&reference).unwrap();
        assert!(serialized.len() < 16 * 1024);
        assert!(!String::from_utf8(serialized.clone())
            .unwrap()
            .contains("file contents never sent"));
        let restored: OmpSupervisionReference = serde_json::from_slice(&serialized).unwrap();
        restored
            .validate_live(&evidence, &policy(), &incident.id)
            .unwrap();
        assert!(restored
            .verify_original(&temp.path().join("tools"), &plan, &policy(), &incident.id)
            .is_err());
        drop(journal);
        let tools = restored
            .verify_original(&temp.path().join("tools"), &plan, &policy(), &incident.id)
            .unwrap();
        assert_eq!(tools.observation().calls.len(), 128);
        assert_eq!(
            restored.failure_context().unwrap()["failures"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        for field in [
            "observationDigest",
            "toolHead",
            "error",
            "resultSequence",
            "scope",
        ] {
            let mut changed = serde_json::to_value(&restored).unwrap();
            match field {
                "error" => {
                    changed["failureContext"]["failures"][0]["error"] = json!("invented diagnosis")
                }
                "resultSequence" => {
                    changed["failureContext"]["failures"][0]["resultSequence"] = json!(2)
                }
                "scope" => changed["intent"]["attemptId"] = json!("other-attempt"),
                _ => changed[field] = json!("b".repeat(64)),
            }
            let changed: OmpSupervisionReference = serde_json::from_value(changed).unwrap();
            assert!(
                changed
                    .validate_live(&evidence, &policy(), &incident.id)
                    .is_err(),
                "{field}"
            );
            assert!(
                changed
                    .verify_original(&temp.path().join("tools"), &plan, &policy(), &incident.id)
                    .is_err(),
                "{field}"
            );
        }
    }

    #[test]
    fn error_projection_never_truncates_unverifiable_wire_text() {
        let temp = tempfile::tempdir().unwrap();
        let (plan, intent, mut journal) = fixture(temp.path());
        let mut errors = ErrorTexts::new();
        // Valid bounded raw text, but JSON escaping expands beyond the diagnostic budget.
        let text = "\u{0001}".repeat(MAX_ERROR_BYTES);
        for id in ["a", "b"] {
            append(&mut journal, &intent, id, true, &text);
            errors.insert(id.into(), text.clone());
        }
        let evidence = OmpToolCheckpoint::new(&intent, journal.checkpoint().unwrap(), errors)
            .unwrap()
            .evidence();
        let incident = evidence.tools.incidents(&policy()).unwrap().remove(0);
        let reference =
            OmpSupervisionReference::from_evidence(&evidence, &policy(), &incident.id).unwrap();
        assert!(reference.failure_context().unwrap()["failures"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f["error"].is_null()));
        drop(journal);
        reference
            .verify_original(&temp.path().join("tools"), &plan, &policy(), &incident.id)
            .unwrap();
    }
}
