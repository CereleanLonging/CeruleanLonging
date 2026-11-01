//! Review contracts for checking an unchanged submission with corrected tools.
//! These serializable values are evidence, never an execution capability. The
//! authenticated host must still prove the original ledger event and sealed
//! source bytes, obtain user approval, exclude active writers, and persist a
//! new single-use intent before starting any check. Corrected results need a
//! separate ledger evidence variant; no original plan/report is modified here.
use crate::execution::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecheckRequest {
    pub project_id: String,
    pub original_operation_id: String,
    pub replacement_checks: Vec<AcceptanceCheck>,
    pub reason: String,
}

/// Trusted original facts supplied by the host, not by a worker or request JSON.
pub struct RecheckBasis<'a> {
    pub plan: &'a PlanRevision,
    pub manifest: &'a ArtifactManifest,
    pub original_job: &'a VerificationJob,
    pub original_report: &'a VerificationReport,
    pub ledger_head: &'a str,
    pub trusted_verifier: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecheckReview {
    pub schema_version: u32,
    pub project_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub original_operation_id: String,
    pub revision_digest: String,
    pub manifest_digest: String,
    pub original_report_digest: String,
    pub original_checks_digest: String,
    pub original_source_snapshot: SourceSnapshot,
    pub ledger_head: String,
    pub replacement_checks: Vec<AcceptanceCheck>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecheckResult {
    pub schema_version: u32,
    pub operation_id: String,
    pub review_digest: String,
    pub report: VerificationReport,
}

/// Immutable content of a separate authenticated user ledger event. Its parent
/// hash is the review head; its operation names exactly one future local check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecheckApproval {
    pub schema_version: u32,
    pub operation_id: String,
    pub review: RecheckReview,
    pub original_job: VerificationJob,
}

/// Read-only lookup of one original user approval and its optional launch.
/// No job means the approval alone has not started a checker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecheckStatus {
    pub approval: RecheckApproval,
    pub approval_event: EventAnchor,
    pub review_digest: String,
    pub job: Option<VerificationJob>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub disposition: Option<RecheckDispositionReceipt>,
}

/// Original durable user event, even if no checker was ever launched. The
/// integrity observation is current and never changes the historical receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecheckDispositionReceipt {
    pub event_operation_id: String,
    pub event: EventAnchor,
    pub review: RecheckDispositionReview,
    pub evidence_unchanged: bool,
    pub message: Option<String>,
}

/// Explicit user retirement of one check, never acceptance of its submission.
/// The host binds a read-only review to the original approval and exact current
/// evidence. Process proof is host-generated; a digest from a caller is not proof.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecheckDispositionReview {
    pub schema_version: u32,
    pub project_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub operation_id: String,
    pub approval: EventAnchor,
    pub approval_digest: String,
    pub ledger_head: String,
    pub launch: Option<VerificationJob>,
    pub evidence_digest: String,
    pub process_evidence_digest: String,
    pub reason: String,
}
impl RecheckDispositionReview {
    pub fn validate(
        &self,
        approval: &RecheckApproval,
        anchor: &EventAnchor,
        head: &str,
    ) -> Result<(), String> {
        for id in [
            &self.project_id,
            &self.task_id,
            &self.attempt_id,
            &self.operation_id,
        ] {
            identifier(id)?;
        }
        for hash in [
            &self.approval.hash,
            &self.approval_digest,
            &self.ledger_head,
            &self.evidence_digest,
            &self.process_evidence_digest,
        ] {
            sha256(hash)?;
        }
        require(
            self.schema_version == 1
                && self.approval == *anchor
                && self.approval_digest == digest(approval)?
                && self.ledger_head == head
                && self.project_id == approval.review.project_id
                && self.task_id == approval.review.task_id
                && self.attempt_id == approval.review.attempt_id
                && self.operation_id == approval.operation_id,
            "recheck disposition does not bind the original approval and ledger",
        )?;
        require(
            !self.reason.trim().is_empty()
                && self.reason.len() <= 2000
                && !self.reason.contains('\0'),
            "recheck disposition requires a bounded reason",
        )?;
        if let Some(job) = &self.launch {
            timestamp(&job.started_at)?;
            require(
                job.project_id == self.project_id
                    && job.task_id == self.task_id
                    && job.attempt_id == self.attempt_id
                    && job.operation_id == self.operation_id
                    && job.state == VerificationState::Running
                    && job.updated_at == job.started_at
                    && job.message.is_none()
                    && job.reconciliation_digest.is_none(),
                "recheck disposition launch identity changed",
            )?;
        }
        Ok(())
    }
}

impl RecheckApproval {
    pub fn validate(&self, basis: &RecheckBasis<'_>) -> Result<(), String> {
        identifier(&self.operation_id)?;
        require(
            self.schema_version == 1
                && self.operation_id != self.review.original_operation_id
                && self.original_job == *basis.original_job,
            "recheck approval operation or original job mismatch",
        )?;
        require(
            serde_json::to_vec(self).map_err(|e| e.to_string())?.len() <= 192 * 1024,
            "recheck approval exceeds ledger evidence limit",
        )?;
        self.review.require_current(basis)
    }
}

fn report_binding(
    report: &VerificationReport,
    basis: &RecheckBasis<'_>,
    checks: &[AcceptanceCheck],
    operation: &str,
) -> Result<(), String> {
    identifier(operation)?;
    identifier(&report.verifier_id)?;
    timestamp(&report.observed_at)?;
    require(
        report.schema_version == 1
            && report.verifier_id == basis.trusted_verifier
            && report.manifest_digest == digest(basis.manifest)?,
        "recheck report identity mismatch",
    )?;
    let snapshot = report
        .source_snapshot
        .as_ref()
        .ok_or("recheck requires sealed source evidence")?;
    require(
        snapshot.operation_id == operation,
        "recheck snapshot operation mismatch",
    )?;
    sha256(&snapshot.manifest_sha256)?;
    require(
        !report.checks.is_empty()
            && report.checks.len() <= checks.len()
            && (report.checks.len() == checks.len()
                || report.checks.last().is_some_and(|check| check.timed_out)),
        "recheck result is incomplete",
    )?;
    for (index, (observed, check)) in report.checks.iter().zip(checks).enumerate() {
        require(
            observed.check_id == check.id
                && observed.command_digest == digest(check)?
                && observed.exit_code.is_some()
                && (!observed.timed_out || index + 1 == report.checks.len()),
            "recheck command, exit observation, or order mismatch",
        )?;
        sha256(&observed.stdout_sha256)?;
        sha256(&observed.stderr_sha256)?;
    }
    Ok(())
}

impl RecheckRequest {
    pub fn review(&self, basis: &RecheckBasis<'_>) -> Result<RecheckReview, String> {
        identifier(&self.project_id)?;
        identifier(&self.original_operation_id)?;
        identifier(&basis.manifest.task_id)?;
        identifier(&basis.manifest.attempt_id)?;
        identifier(basis.trusted_verifier)?;
        sha256(basis.ledger_head)?;
        require(
            !self.reason.trim().is_empty()
                && self.reason.len() <= 2000
                && !self.reason.contains('\0'),
            "recheck needs a bounded review reason",
        )?;
        require(
            !self.replacement_checks.is_empty() && self.replacement_checks.len() <= 64,
            "recheck stage count out of range",
        )?;
        basis.plan.validate()?;
        require(
            basis.plan.snapshot_policy.is_some(),
            "recheck requires the original full source snapshot policy",
        )?;
        let job = basis.original_job;
        let manifest = basis.manifest;
        require(
            job.state == VerificationState::Failed
                && job.project_id == self.project_id
                && job.project_id == basis.plan.project.id
                && job.operation_id == self.original_operation_id
                && job.task_id == manifest.task_id
                && job.attempt_id == manifest.attempt_id
                && manifest.schema_version == 1
                && manifest.revision_digest == basis.plan.approval_digest()?,
            "recheck requires the same terminal failed submission",
        )?;
        sha256(&manifest.receipt_digest)?;
        let original_checks = &basis.plan.task(&manifest.task_id)?.checks;
        require(
            original_checks.len() <= 64,
            "original stage count out of range",
        )?;
        let expected: BTreeSet<_> = basis
            .plan
            .task(&manifest.task_id)?
            .artifact_paths
            .iter()
            .collect();
        let actual: BTreeSet<_> = manifest.artifacts.iter().map(|item| &item.path).collect();
        require(
            expected == actual && actual.len() == manifest.artifacts.len(),
            "recheck artifact scope mismatch",
        )?;
        for artifact in &manifest.artifacts {
            artifact_path(&artifact.path)?;
            sha256(&artifact.sha256)?;
        }
        report_binding(
            basis.original_report,
            basis,
            original_checks,
            &job.operation_id,
        )?;
        require(
            basis.original_report.checks.len() == original_checks.len()
                && basis
                    .original_report
                    .checks
                    .iter()
                    .all(|check| !check.timed_out)
                && basis
                    .original_report
                    .checks
                    .iter()
                    .any(|check| check.exit_code != Some(0)),
            "only a complete observed check failure can be corrected",
        )?;
        let observed = chrono::DateTime::parse_from_rfc3339(&basis.original_report.observed_at)
            .map_err(|_| "invalid original observation time")?;
        let started = chrono::DateTime::parse_from_rfc3339(&job.started_at)
            .map_err(|_| "invalid original job start")?;
        let updated = chrono::DateTime::parse_from_rfc3339(&job.updated_at)
            .map_err(|_| "invalid original job update")?;
        require(
            started <= observed && observed <= updated,
            "original report outside job lifetime",
        )?;
        require(
            self.replacement_checks.len() == original_checks.len()
                && self
                    .replacement_checks
                    .iter()
                    .zip(original_checks)
                    .all(|(new, old)| new.id == old.id && new.name == old.name),
            "recheck cannot remove, reorder, or rename stages",
        )?;
        require(
            self.replacement_checks != *original_checks,
            "unchanged checker is not a correction",
        )?;
        // Reuse all existing check/tool/environment and policy validation. This
        // temporary clone is not a revised approved plan or a dispatch license.
        let mut checked = basis.plan.clone();
        checked
            .execution
            .iter_mut()
            .find(|task| task.task_id == manifest.task_id)
            .ok_or("recheck task missing")?
            .checks = self.replacement_checks.clone();
        checked.validate()?;
        for check in &self.replacement_checks {
            check
                .tool
                .as_ref()
                .ok_or("recheck requires approved tool pins")?
                .validate()?;
        }
        let review = RecheckReview {
            schema_version: 1,
            project_id: self.project_id.clone(),
            task_id: manifest.task_id.clone(),
            attempt_id: manifest.attempt_id.clone(),
            original_operation_id: job.operation_id.clone(),
            revision_digest: manifest.revision_digest.clone(),
            manifest_digest: digest(manifest)?,
            original_report_digest: digest(basis.original_report)?,
            original_checks_digest: digest(original_checks)?,
            original_source_snapshot: basis
                .original_report
                .source_snapshot
                .clone()
                .ok_or("original source snapshot missing")?,
            ledger_head: basis.ledger_head.into(),
            replacement_checks: self.replacement_checks.clone(),
            reason: self.reason.clone(),
        };
        require(
            serde_json::to_vec(&review)
                .map_err(|e| e.to_string())?
                .len()
                <= 128 * 1024,
            "recheck review exceeds ledger evidence limit",
        )?;
        Ok(review)
    }
}

impl RecheckReview {
    /// Must be rechecked against host-observed facts before approval/launch. A
    /// persisted approval will additionally need its own authenticated event.
    pub fn require_current(&self, basis: &RecheckBasis<'_>) -> Result<(), String> {
        let rebuilt = RecheckRequest {
            project_id: self.project_id.clone(),
            original_operation_id: self.original_operation_id.clone(),
            replacement_checks: self.replacement_checks.clone(),
            reason: self.reason.clone(),
        }
        .review(basis)?;
        require(*self == rebuilt, "recheck review is stale or altered")
    }
}

impl RecheckResult {
    /// Pure validation only. A true result does not authorize Accept, establish
    /// runner provenance, or prove the sealed filesystem snapshot unchanged.
    pub fn validate(
        &self,
        review: &RecheckReview,
        basis: &RecheckBasis<'_>,
    ) -> Result<bool, String> {
        review.require_current(basis)?;
        identifier(&self.operation_id)?;
        sha256(&self.review_digest)?;
        require(
            self.schema_version == 1
                && self.operation_id != review.original_operation_id
                && self.review_digest == digest(review)?,
            "recheck result approval or operation mismatch",
        )?;
        report_binding(
            &self.report,
            basis,
            &review.replacement_checks,
            &self.operation_id,
        )?;
        require(
            self.report
                .source_snapshot
                .as_ref()
                .is_some_and(|snapshot| {
                    snapshot.manifest_sha256 == review.original_source_snapshot.manifest_sha256
                }),
            "recheck source tree differs from original failed check",
        )?;
        let observed = chrono::DateTime::parse_from_rfc3339(&self.report.observed_at)
            .map_err(|_| "invalid recheck observation time")?;
        let original = chrono::DateTime::parse_from_rfc3339(&basis.original_report.observed_at)
            .map_err(|_| "invalid original observation time")?;
        require(observed >= original, "recheck predates original failure")?;
        Ok(self.report.checks.len() == review.replacement_checks.len()
            && self
                .report
                .checks
                .iter()
                .all(|check| check.exit_code == Some(0) && !check.timed_out))
    }
}

#[cfg(test)]
mod tests;
