//! User retirement is an original-chain event. Replay authenticates historical
//! authority without consulting today's OS; live writes re-observe exact facts.
use super::*;
use cerulean_domain::{
    execution::{digest as proof_digest, EventAnchor},
    verification_recheck::RecheckDispositionReview,
};
pub(crate) const DISPOSITION_SOURCE: &str =
    "authenticated local-user verification recheck disposition";

impl DurableWorkflow {
    pub(crate) fn recheck_disposition(&self, approval: &Entry) -> Option<&Entry> {
        self.entries.iter().find(|e| {
            e.body
                .context
                .recheck_disposition
                .as_ref()
                .is_some_and(|r| {
                    r.approval.hash == approval.hash
                        && r.approval.sequence == approval.body.event.sequence
                })
        })
    }
    #[cfg(windows)]
    pub fn review_verification_recheck_disposition(
        &self,
        operation: &str,
        reason: &str,
    ) -> Result<RecheckDispositionReview, String> {
        require(!self.requires_reopen(), "workflow requires reopen")?;
        let entry = self
            .unresolved_verification_recheck()
            .ok_or("no unresolved recheck")?;
        let a = entry
            .body
            .context
            .verification_recheck
            .as_ref()
            .ok_or("original approval missing")?;
        require(a.operation_id == operation, "different unresolved recheck")?;
        let task = self
            .state()
            .tasks
            .get(&a.review.task_id)
            .ok_or("original task missing")?;
        require(
            self.state().approved
                && self.state().revision_requested.is_none()
                && task.state == TaskState::Submitted
                && !task.verified
                && task.attempt.as_deref() == Some(a.review.attempt_id.as_str()),
            "disposition requires current unverified submission",
        )?;
        let data = self
            .root
            .parent()
            .filter(|p| p.file_name().is_some_and(|s| s == "workflows"))
            .and_then(Path::parent)
            .ok_or("original Host root required")?;
        crate::verification_recheck_guard::require_protected(data, true)?;
        let (launch, process_evidence_digest) =
            crate::async_verification_recheck::disposition_process_basis(data, self, operation)?;
        let review = RecheckDispositionReview {
            schema_version: 1,
            project_id: a.review.project_id.clone(),
            task_id: a.review.task_id.clone(),
            attempt_id: a.review.attempt_id.clone(),
            operation_id: operation.into(),
            approval: EventAnchor {
                sequence: entry.body.event.sequence,
                hash: entry.hash.clone(),
            },
            approval_digest: proof_digest(a)?,
            ledger_head: self.entries.last().ok_or("ledger empty")?.hash.clone(),
            launch,
            evidence_digest: crate::recheck_disposition_evidence::fingerprint(data, a)?,
            process_evidence_digest,
            reason: reason.into(),
        };
        review.validate(a, &review.approval, &review.ledger_head)?;
        require(
            !self
                .entries
                .iter()
                .any(|e| e.body.operation_id == operation),
            "recheck result already recorded",
        )?;
        Ok(review)
    }
    #[allow(clippy::too_many_arguments)]
    pub(super) fn check_recheck_disposition(
        &self,
        event_operation: &str,
        task: &str,
        attempt: &str,
        operation: &str,
        wanted: &str,
        context: &CommandContext,
        live: bool,
    ) -> Result<(), String> {
        let r = context
            .recheck_disposition
            .as_ref()
            .ok_or("disposition review missing")?;
        require(
            *context
                == CommandContext {
                    recheck_disposition: Some(r.clone()),
                    approval_source: Some(DISPOSITION_SOURCE.into()),
                    ..Default::default()
                },
            "disposition requires authenticated plain user channel",
        )?;
        require(
            event_operation != operation
                && r.task_id == task
                && r.attempt_id == attempt
                && r.operation_id == operation
                && proof_digest(r)? == wanted,
            "disposition command scope mismatch",
        )?;
        let approved = self
            .unresolved_verification_recheck()
            .ok_or("original recheck already resolved")?;
        let a = approved
            .body
            .context
            .verification_recheck
            .as_ref()
            .ok_or("original approval missing")?;
        let anchor = EventAnchor {
            sequence: approved.body.event.sequence,
            hash: approved.hash.clone(),
        };
        let head = &self.entries.last().ok_or("ledger empty")?.hash;
        r.validate(a, &anchor, head)?;
        require(
            self.config
                .execution_scope
                .as_ref()
                .is_some_and(|s| s.project.id == r.project_id),
            "disposition project mismatch",
        )?;
        if live {
            #[cfg(windows)]
            require(
                self.review_verification_recheck_disposition(operation, &r.reason)? == **r,
                "disposition evidence changed; refresh original review",
            )?;
            #[cfg(not(windows))]
            return Err("recheck disposition needs Windows runtime".into());
        }
        Ok(())
    }
    #[cfg(windows)]
    pub(crate) fn retired_recheck_operations(&self) -> Result<Vec<String>, String> {
        let data = self
            .root
            .parent()
            .and_then(Path::parent)
            .ok_or("Host root missing")?;
        let mut operations = Vec::new();
        for e in &self.entries {
            let Some(r) = &e.body.context.recheck_disposition else {
                continue;
            };
            let approved = self
                .entries
                .iter()
                .find(|a| a.hash == r.approval.hash)
                .ok_or("retired recheck approval missing")?;
            let a = approved
                .body
                .context
                .verification_recheck
                .as_ref()
                .ok_or("retired recheck authority missing")?;
            require(
                crate::recheck_disposition_evidence::fingerprint(data, a)? == r.evidence_digest,
                "retired recheck evidence changed; preserve and reconcile",
            )?;
            operations.push(r.operation_id.clone());
        }
        Ok(operations)
    }
}
