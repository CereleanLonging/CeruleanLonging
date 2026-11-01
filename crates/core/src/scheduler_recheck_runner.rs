//! Single-use corrected checks retain the original plan and source authority.
//! Reconciliation publishes observed evidence only; it never launches a process.
use super::*;
use crate::verifier::{PinnedCheck, RecheckAuthority, VerificationInput};
use cerulean_domain::{
    execution::{digest as proof_digest, EventAnchor, VerificationJob, VerificationState},
    verification_recheck::{RecheckApproval, RecheckBasis, RecheckResult},
};
use std::sync::atomic::AtomicBool;

#[derive(Serialize)]
pub(crate) struct PreparedVerificationRecheck {
    recheck: RecheckApproval,
    event: EventAnchor,
    approved_at: String,
    original_report: VerificationReport,
    plan: PlanRevision,
    original_approval: Approval,
    attempt: Attempt,
    receipt: ExecutionReceipt,
    manifest: ArtifactManifest,
    user: String,
    policy_version: u32,
    core: String,
    evidence: PathBuf,
    retired_rechecks: Vec<String>,
}
impl PreparedVerificationRecheck {
    fn basis(&self) -> RecheckBasis<'_> {
        RecheckBasis {
            plan: &self.plan,
            manifest: &self.manifest,
            original_job: &self.recheck.original_job,
            original_report: &self.original_report,
            ledger_head: &self.recheck.review.ledger_head,
            trusted_verifier: &self.core,
        }
    }
    fn pins(&self) -> Result<Vec<PinnedCheck>, String> {
        self.recheck
            .review
            .replacement_checks
            .iter()
            .map(PinnedCheck::from_approved)
            .collect()
    }
    fn result(&self, report: VerificationReport) -> Result<RecheckResult, String> {
        Ok(RecheckResult {
            schema_version: 1,
            operation_id: self.recheck.operation_id.clone(),
            review_digest: proof_digest(&self.recheck.review)?,
            report,
        })
    }
    pub(crate) fn run(&self, cancel: &AtomicBool) -> Result<RecheckResult, String> {
        // The existing original evidence root is already present and must stay ordinary.
        safe_metadata(&self.evidence)?;
        let report = crate::verifier::verify_recheck(
            VerificationInput {
                plan: &self.plan,
                approval: &self.original_approval,
                authenticated_user: &self.user,
                policy_version: self.policy_version,
                attempt: &self.attempt,
                receipt: &self.receipt,
                manifest: &self.manifest,
                verifier_id: &self.core,
            },
            &self.pins()?,
            &self.evidence,
            RecheckAuthority {
                approval: &self.recheck,
                event: &self.event,
                original_report: &self.original_report,
                retired_operations: &self.retired_rechecks,
            },
            cancel,
        )?;
        let result = self.result(report)?;
        result.validate(&self.recheck.review, &self.basis())?;
        Ok(result)
    }
}

impl DurableWorkflow {
    pub(crate) fn prepare_verification_recheck(
        &self,
        operation: &str,
    ) -> Result<PreparedVerificationRecheck, String> {
        require(!self.requires_reopen(), "workflow requires reopen")?;
        cerulean_domain::execution::identifier(operation)?;
        require(
            !self
                .entries
                .iter()
                .any(|e| e.body.operation_id == operation),
            "recheck operation already recorded; do not rerun",
        )?;
        let approved = self
            .unresolved_verification_recheck()
            .ok_or("no unresolved recheck approval")?;
        require(
            self.entries.last().is_some_and(|e| e.hash == approved.hash),
            "recheck approval is stale; preserve original operation",
        )?;
        crate::reader_generation::require_workflow_context(&self.root, &approved.body.context)?;
        let saved = approved
            .body
            .context
            .verification_recheck
            .as_ref()
            .ok_or("recheck approval missing")?;
        require(
            saved.operation_id == operation
                && approved.body.event.principal == self.role_id(Role::User)?,
            "recheck operation differs from original user approval",
        )?;
        let task = &saved.review.task_id;
        let attempt = &saved.review.attempt_id;
        let run = self.state().tasks.get(task).ok_or("recheck task missing")?;
        require(
            self.state().approved
                && self.state().revision_requested.is_none()
                && run.state == TaskState::Submitted
                && !run.verified
                && run.attempt.as_deref() == Some(attempt),
            "recheck submission is no longer current",
        )?;
        self.with_recheck_basis(
            saved,
            self.entries.len() - 1,
            &approved.body.previous_hash,
            |basis| {
                let scope = self
                    .config
                    .execution_scope
                    .as_ref()
                    .ok_or("recheck scope missing")?;
                let approval = self.approval().ok_or("original approval missing")?;
                approval.validate(basis.plan, self.role_id(Role::User)?, scope.policy_version)?;
                // Approval is durable, but the original on-disk failure evidence
                // could have changed since review. Re-read its stages and logs
                // before claiming a new operation, and again before publication.
                let data = self
                    .root
                    .parent()
                    .filter(|p| p.file_name().is_some_and(|s| s == "workflows"))
                    .and_then(Path::parent)
                    .ok_or("recheck requires original Host root")?;
                let original_basis = crate::verification_detail::basis(self, &saved.original_job)?;
                let (original_detail, original_report) = crate::verification_detail::inspect(
                    data,
                    &saved.original_job,
                    &original_basis,
                )?;
                require(
                    original_detail.ledger_recorded
                        && original_report.as_ref() == Some(basis.original_report),
                    "original failed verification evidence changed after approval",
                )?;
                self.require_recheck_source(
                    basis.plan,
                    basis.manifest,
                    &saved.review.original_source_snapshot,
                )?;
                Ok(PreparedVerificationRecheck {
                    recheck: (**saved).clone(),
                    event: EventAnchor {
                        sequence: approved.body.event.sequence,
                        hash: approved.hash.clone(),
                    },
                    approved_at: approved.body.received_at.clone(),
                    original_report: basis.original_report.clone(),
                    plan: basis.plan.clone(),
                    original_approval: approval.clone(),
                    attempt: self
                        .attempt_record(task, attempt)
                        .ok_or("original attempt missing")?
                        .clone(),
                    receipt: self
                        .completed_artifacts(task, attempt)
                        .ok_or("submission missing")?
                        .0
                        .clone(),
                    manifest: basis.manifest.clone(),
                    user: self.role_id(Role::User)?.into(),
                    policy_version: scope.policy_version,
                    core: self.config.core_id.clone(),
                    evidence: self.root.join("verification"),
                    retired_rechecks: self.retired_recheck_operations()?,
                })
            },
        )
    }

    pub(crate) fn finish_verification_recheck(
        &mut self,
        prepared: &PreparedVerificationRecheck,
        result: RecheckResult,
    ) -> Result<Entry, String> {
        let current = self.prepare_verification_recheck(&prepared.recheck.operation_id)?;
        require(
            proof_digest(&current)? == proof_digest(prepared)?,
            "recheck authority changed before publication",
        )?;
        let passed = result.validate(&prepared.recheck.review, &prepared.basis())?;
        self.command(
            &prepared.recheck.operation_id,
            &prepared.core,
            WorkflowCommand::Verify {
                task: prepared.recheck.review.task_id.clone(),
                attempt: prepared.recheck.review.attempt_id.clone(),
                passed,
                evidence: vec![proof_digest(&result)?],
            },
            CommandContext {
                execution: Some(ExecutionEvidence::VerificationRecheck {
                    approval: prepared.event.clone(),
                    result: Box::new(result),
                }),
                ..Default::default()
            },
            true,
        )
    }

    /// Trusted caller only. An existing verifier directory is never run again,
    /// even if publication was interrupted or its files are incomplete.
    pub fn run_verification_recheck(
        &mut self,
        operation: &str,
        cancel: &AtomicBool,
    ) -> Result<Entry, String> {
        let prepared = self.prepare_verification_recheck(operation)?;
        let result = prepared.run(cancel)?;
        self.finish_verification_recheck(&prepared, result)
    }

    /// Uses the original result, stage observations, logs, pins and sealed tree.
    /// Missing or altered evidence stays unresolved and cannot be manufactured.
    pub fn recover_verification_recheck(&mut self, operation: &str) -> Result<Entry, String> {
        require(!self.requires_reopen(), "workflow requires reopen")?;
        if let Some(entry) = self
            .entries
            .iter()
            .find(|e| e.body.operation_id == operation)
        {
            require(
                matches!(
                    entry.body.context.execution,
                    Some(ExecutionEvidence::VerificationRecheck { .. })
                ),
                "recheck operation conflicts with original ledger",
            )?;
            return Ok(entry.clone());
        }
        let prepared = self.prepare_verification_recheck(operation)?;
        let job = VerificationJob {
            project_id: prepared.plan.project.id.clone(),
            operation_id: operation.into(),
            task_id: prepared.recheck.review.task_id.clone(),
            attempt_id: prepared.recheck.review.attempt_id.clone(),
            state: VerificationState::Unknown,
            started_at: prepared.approved_at.clone(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            message: None,
            reconciliation_digest: None,
        };
        let mut locks = vec![];
        for (pin, check) in prepared
            .pins()?
            .iter()
            .zip(&prepared.recheck.review.replacement_checks)
        {
            locks.extend(pin.lock(check)?);
        }
        let data = self
            .root
            .parent()
            .filter(|p| p.file_name().is_some_and(|s| s == "workflows"))
            .and_then(Path::parent)
            .ok_or("recheck requires original Host root")?;
        let basis = crate::verification_detail::basis(self, &job)?;
        let (_, report) = crate::verification_detail::inspect(data, &job, &basis)?;
        let report = report.ok_or("no complete original recheck result; do not run again")?;
        require(
            chrono::DateTime::parse_from_rfc3339(&report.observed_at).map_err(|e| e.to_string())?
                <= chrono::Utc::now(),
            "recheck result timestamp is in the future",
        )?;
        let result = prepared.result(report)?;
        self.finish_verification_recheck(&prepared, result)
    }
}
