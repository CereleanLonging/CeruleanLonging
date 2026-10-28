//! Original live OMP tool-owner evidence. Historical bytes cannot create a
//! checkpoint or a continuation permit, and observing a result is not its ACK.
use crate::omp_tool_journal::{ToolCheckpoint, ToolEvidence};
use cerulean_adapters::omp_transport::worker::DispatchIntent;
use cerulean_domain::execution::{require, PlanRevision};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
#[path = "omp_failure_context.rs"]
mod failure_context;
pub(crate) use failure_context::{ErrorTexts, MAX_ERROR_BYTES};
#[path = "omp_supervision_reference.rs"]
mod reference;
pub use reference::OmpSupervisionReference;

pub(crate) type Observer = Box<
    dyn FnMut(
            OmpToolCheckpoint,
        ) -> Result<
            Option<cerulean_adapters::omp_transport::worker::advice::AdviceBoundary>,
            String,
        > + Send,
>;

pub(crate) struct OmpToolCheckpoint {
    pub(crate) intent: DispatchIntent,
    pub(crate) tools: ToolCheckpoint,
    errors: ErrorTexts,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OmpToolEvidence {
    intent: DispatchIntent,
    tools: ToolEvidence,
    errors: ErrorTexts,
}
impl OmpToolCheckpoint {
    pub(crate) fn new(
        intent: &DispatchIntent,
        tools: ToolCheckpoint,
        errors: ErrorTexts,
    ) -> Result<Self, String> {
        tools.matches_scope(intent)?;
        failure_context::validate_errors(tools.evidence(), &errors)?;
        Ok(Self {
            intent: intent.clone(),
            tools,
            errors,
        })
    }
    pub(crate) fn evidence(&self) -> OmpToolEvidence {
        OmpToolEvidence {
            intent: self.intent.clone(),
            tools: self.tools.evidence().clone(),
            errors: self.errors.clone(),
        }
    }
}
impl OmpToolEvidence {
    pub fn intent(&self) -> &DispatchIntent {
        &self.intent
    }
    pub fn tools(&self) -> &ToolEvidence {
        &self.tools
    }
    pub fn validate_plan(&self, plan: &PlanRevision) -> Result<(), String> {
        validate_intent_plan(&self.intent, plan)
    }
    /// Caller supplies the original file service directory, never a new service.
    pub fn verify_original(&self, file_service: &Path) -> Result<(), String> {
        self.intent.validate_metadata()?;
        failure_context::validate_errors(&self.tools, &self.errors)?;
        self.tools
            .verify_original(&file_service.join("events"), &self.intent)
    }
}
fn validate_intent_plan(intent: &DispatchIntent, plan: &PlanRevision) -> Result<(), String> {
    intent.validate_metadata()?;
    let task = plan.task(&intent.task_id)?;
    let worker = task.worker.as_ref().ok_or("Original OMP worker missing")?;
    require(
        task.adapter_id == "omp"
            && intent.binding.adapter_id == "omp"
            && intent.prompt_sha256
                == format!(
                    "{:x}",
                    Sha256::digest(
                        crate::omp_execution::approved_prompt(plan, &intent.task_id)?.as_bytes()
                    )
                )
            && intent.revision_digest == plan.approval_digest()?
            && intent.provider == worker.provider
            && intent.model == worker.model
            && intent.binding.workspace
                == plan.execution_root(&intent.task_id, &intent.attempt_id)?,
        "OMP tool observation differs from original approved scope",
    )
}
