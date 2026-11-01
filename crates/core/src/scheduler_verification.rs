//! Capture original authority under the writer; run processes outside it; then
//! revalidate the same task's evidence before publishing the existing Verify.
use super::*;
use crate::verifier::{verify_with_retirements, PinnedCheck, VerificationInput};
use cerulean_domain::execution::{digest, ExecutionReceipt};
use std::sync::atomic::AtomicBool;

#[derive(Serialize)]
pub(crate) struct PreparedVerification {
    operation: String,
    plan: PlanRevision,
    approval: Approval,
    attempt: Attempt,
    receipt: ExecutionReceipt,
    manifest: ArtifactManifest,
    user: String,
    policy_version: u32,
    core: String,
    evidence: PathBuf,
    retired_rechecks: Vec<String>,
}
impl PreparedVerification {
    pub(crate) fn require_scope(
        &self,
        project: &str,
        task: &str,
        attempt: &str,
        revision: &str,
    ) -> Result<(), String> {
        require(
            self.plan.project.id == project
                && self.attempt.task_id == task
                && self.attempt.id == attempt
                && self.plan.approval_digest()? == revision,
            "verification ticket scope mismatch",
        )
    }
    pub(crate) fn run(
        &self,
        pins: &[PinnedCheck],
        cancel: &AtomicBool,
    ) -> Result<VerificationReport, String> {
        fs::create_dir_all(&self.evidence).map_err(|e| e.to_string())?;
        require(
            safe_metadata(&self.evidence)?.is_dir(),
            "invalid verification directory",
        )?;
        verify_with_retirements(
            VerificationInput {
                plan: &self.plan,
                approval: &self.approval,
                authenticated_user: &self.user,
                policy_version: self.policy_version,
                attempt: &self.attempt,
                receipt: &self.receipt,
                manifest: &self.manifest,
                verifier_id: &self.core,
            },
            pins,
            &self.evidence,
            &self.operation,
            cancel,
            &self.retired_rechecks,
        )
    }
}
impl DurableWorkflow {
    pub(crate) fn prepare_verification(
        &self,
        operation: &str,
        task: &str,
        attempt: &str,
    ) -> Result<PreparedVerification, String> {
        require(!self.poisoned, "workflow requires reopen")?;
        require(
            self.unresolved_verification_recheck().is_none(),
            "original recheck approval is unresolved; do not start another verification",
        )?;
        require(
            self.state().revision_requested.is_none(),
            "old plan is awaiting revision; checks cannot start",
        )?;
        require(valid_id(operation), "invalid operation ID")?;
        require(
            !self
                .entries
                .iter()
                .any(|e| e.body.operation_id == operation),
            "verification already recorded; do not rerun",
        )?;
        let run = self.state().tasks.get(task).ok_or("task missing")?;
        require(
            run.state == TaskState::Submitted
                && run.attempt.as_deref() == Some(attempt)
                && !run.verified,
            "submitted unverified attempt required",
        )?;
        let scope = self
            .config
            .execution_scope
            .as_ref()
            .ok_or("strict execution scope required")?;
        let (receipt, manifest) = self
            .completed_artifacts(task, attempt)
            .ok_or("completion missing")?;
        Ok(PreparedVerification {
            operation: operation.into(),
            plan: self.revision().ok_or("revision missing")?.clone(),
            approval: self.approval().ok_or("approval missing")?.clone(),
            attempt: self
                .attempt_record(task, attempt)
                .ok_or("attempt missing")?
                .clone(),
            receipt: receipt.clone(),
            manifest: manifest.clone(),
            user: self.role_id(Role::User)?.into(),
            policy_version: scope.policy_version,
            core: self.config.core_id.clone(),
            evidence: self.root.join("verification"),
            retired_rechecks: self.retired_recheck_operations()?,
        })
    }
    pub(crate) fn finish_prepared_verification(
        &mut self,
        prepared: &PreparedVerification,
        report: VerificationReport,
    ) -> Result<Entry, String> {
        let current = self.prepare_verification(
            &prepared.operation,
            &prepared.attempt.task_id,
            &prepared.attempt.id,
        )?;
        require(
            digest(&current)? == digest(prepared)?,
            "verification authority or submitted evidence changed",
        )?;
        let passed = report
            .require_pass(&prepared.plan, &prepared.manifest, &prepared.core)
            .is_ok();
        let evidence_digest = digest(&report)?;
        self.command(
            &prepared.operation,
            &prepared.core,
            WorkflowCommand::Verify {
                task: prepared.attempt.task_id.clone(),
                attempt: prepared.attempt.id.clone(),
                passed,
                evidence: vec![evidence_digest],
            },
            CommandContext {
                execution: Some(ExecutionEvidence::Verification { report }),
                ..Default::default()
            },
            true,
        )
    }
    /// Trusted host only. The original verifier intent claims the ID before any
    /// process starts; interrupted or duplicate operations never rerun checks.
    pub fn run_verification(
        &mut self,
        operation: &str,
        task: &str,
        attempt: &str,
        pins: &[PinnedCheck],
        cancel: &AtomicBool,
    ) -> Result<Entry, String> {
        let prepared = self.prepare_verification(operation, task, attempt)?;
        let report = prepared.run(pins, cancel)?;
        self.finish_prepared_verification(&prepared, report)
    }
}
