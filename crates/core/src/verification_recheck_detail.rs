//! Reconstruct corrected-check authority from the authenticated original history.
use super::*;
use crate::verification_recheck_evidence::RecheckBinding;
use cerulean_domain::verification_recheck::{RecheckApproval, RecheckBasis, RecheckResult};

#[derive(Clone)]
pub(super) struct RecheckDetail {
    pub approval: RecheckApproval,
    pub(super) event: EventAnchor,
    approved_at: String,
    original_report: VerificationReport,
    pub recorded: Option<(VerificationReport, bool)>,
}
impl RecheckDetail {
    pub(super) fn from_history(
        history: &[crate::scheduler::Entry],
        job: &VerificationJob,
        plan: &PlanRevision,
        manifest: &ArtifactManifest,
    ) -> Result<Option<Self>, String> {
        let approvals: Vec<_> = history
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                matches!(
            &entry.body.event.command, WorkflowCommand::ApproveVerificationRecheck {operation_id,..}
                if operation_id == &job.operation_id)
            })
            .collect();
        if approvals.is_empty() {
            return Ok(None);
        }
        require(approvals.len() == 1, "复验操作存在重复批准")?;
        let (index, event) = approvals[0];
        let approval = event
            .body
            .context
            .verification_recheck
            .as_ref()
            .ok_or("复验批准证据缺失")?;
        require(
            event.body.event.principal == "user"
                && approval.operation_id == job.operation_id
                && approval.review.project_id == job.project_id
                && approval.review.task_id == job.task_id
                && approval.review.attempt_id == job.attempt_id
                && event.body.previous_hash == approval.review.ledger_head
                && event.body.event.command
                    == WorkflowCommand::ApproveVerificationRecheck {
                        task: job.task_id.clone(),
                        attempt: job.attempt_id.clone(),
                        operation_id: job.operation_id.clone(),
                        review_digest: digest(&approval.review)?,
                    },
            "复验批准归属不匹配",
        )?;
        let original = history[..index]
            .iter()
            .find(|e| e.body.operation_id == approval.review.original_operation_id)
            .ok_or("复验原失败事件不存在")?;
        let Some(ExecutionEvidence::Verification { report }) = &original.body.context.execution
        else {
            return Err("复验原失败报告不存在".into());
        };
        require(
            original.body.event.principal == "core"
                && original.body.event.command
                    == WorkflowCommand::Verify {
                        task: job.task_id.clone(),
                        attempt: job.attempt_id.clone(),
                        passed: false,
                        evidence: vec![digest(report)?],
                    },
            "复验依据不是原核心失败事件",
        )?;
        let mut detail = Self {
            approval: (**approval).clone(),
            event: EventAnchor {
                sequence: event.body.event.sequence,
                hash: event.hash.clone(),
            },
            approved_at: event.body.received_at.clone(),
            original_report: report.clone(),
            recorded: None,
        };
        detail.approval.validate(&detail.basis(plan, manifest))?;
        if let Some(recorded) = history
            .iter()
            .find(|e| e.body.operation_id == job.operation_id)
        {
            let Some(ExecutionEvidence::VerificationRecheck {
                approval: anchor,
                result,
            }) = &recorded.body.context.execution
            else {
                return Err("复验操作与原账本类型冲突".into());
            };
            let passed = detail.validate_report(&result.report, plan, manifest)?;
            require(
                recorded.body.event.principal == "core"
                    && anchor == &detail.event
                    && result.schema_version == 1
                    && result.operation_id == job.operation_id
                    && result.review_digest == digest(&detail.approval.review)?
                    && recorded.body.previous_hash == detail.event.hash
                    && recorded.body.event.sequence.checked_sub(1) == Some(detail.event.sequence)
                    && recorded.body.event.command
                        == WorkflowCommand::Verify {
                            task: job.task_id.clone(),
                            attempt: job.attempt_id.clone(),
                            passed,
                            evidence: vec![digest(result)?],
                        },
                "复验结果未绑定原批准",
            )?;
            detail.recorded = Some((result.report.clone(), passed));
        }
        Ok(Some(detail))
    }
    fn basis<'a>(
        &'a self,
        plan: &'a PlanRevision,
        manifest: &'a ArtifactManifest,
    ) -> RecheckBasis<'a> {
        RecheckBasis {
            plan,
            manifest,
            original_job: &self.approval.original_job,
            original_report: &self.original_report,
            ledger_head: &self.approval.review.ledger_head,
            trusted_verifier: "core",
        }
    }
    pub(super) fn binding(&self) -> Result<RecheckBinding, String> {
        RecheckBinding::new(&self.approval, &self.event)
    }
    pub(super) fn validate_report(
        &self,
        report: &VerificationReport,
        plan: &PlanRevision,
        manifest: &ArtifactManifest,
    ) -> Result<bool, String> {
        require(
            chrono::DateTime::parse_from_rfc3339(&report.observed_at).map_err(|e| e.to_string())?
                >= chrono::DateTime::parse_from_rfc3339(&self.approved_at)
                    .map_err(|e| e.to_string())?,
            "复验结果早于用户批准",
        )?;
        RecheckResult {
            schema_version: 1,
            operation_id: self.approval.operation_id.clone(),
            review_digest: digest(&self.approval.review)?,
            report: report.clone(),
        }
        .validate(&self.approval.review, &self.basis(plan, manifest))
    }
}
