//! Recheck authority is a user event in the original chain. Result replay uses
//! that exact historical prefix, never today's mutable job projection.
use super::*;
use cerulean_domain::{
    execution::{digest as proof_digest, SourceSnapshot},
    verification_recheck::*,
};

pub(crate) const APPROVAL_SOURCE: &str = "authenticated local-user verification recheck approval";

impl DurableWorkflow {
    pub(crate) fn unresolved_verification_recheck(&self) -> Option<&Entry> {
        self.entries.iter().rev().find(|entry| {
            let WorkflowCommand::ApproveVerificationRecheck { operation_id, .. } =
                &entry.body.event.command
            else {
                return false;
            };
            self.recheck_disposition(entry).is_none()
                && !self.entries.iter().any(|result| {
                    result.body.operation_id == *operation_id
                        && matches!(&result.body.context.execution,
                    Some(ExecutionEvidence::VerificationRecheck { approval, .. })
                        if approval.hash == entry.hash)
                })
        })
    }

    pub(super) fn with_recheck_basis<T>(
        &self,
        approval: &RecheckApproval,
        before: usize,
        head: &str,
        inspect: impl FnOnce(&RecheckBasis<'_>) -> Result<T, String>,
    ) -> Result<T, String> {
        let history = self
            .entries
            .get(..before)
            .ok_or("recheck history prefix out of range")?;
        require(
            history.last().is_some_and(|e| e.hash == head),
            "recheck history head mismatch",
        )?;
        let review = &approval.review;
        let original = history
            .iter()
            .find(|e| e.body.operation_id == review.original_operation_id)
            .ok_or("original failed verification is not in the approved history")?;
        let report = match (
            &original.body.event.command,
            &original.body.context.execution,
        ) {
            (
                WorkflowCommand::Verify {
                    task,
                    attempt,
                    passed: false,
                    evidence,
                },
                Some(ExecutionEvidence::Verification { report }),
            ) if task == &review.task_id
                && attempt == &review.attempt_id
                && original.body.event.principal == self.config.core_id
                && *evidence == vec![proof_digest(report)?] =>
            {
                report
            }
            _ => return Err("original operation is not the bound core check failure".into()),
        };
        let plan = self.revision().ok_or("recheck plan missing")?;
        let (_, manifest) = self
            .completed_artifacts(&review.task_id, &review.attempt_id)
            .ok_or("recheck submission missing")?;
        let basis = RecheckBasis {
            plan,
            manifest,
            original_job: &approval.original_job,
            original_report: report,
            ledger_head: head,
            trusted_verifier: &self.config.core_id,
        };
        approval.validate(&basis)?;
        inspect(&basis)
    }

    pub(super) fn check_recheck_approval(
        &self,
        task: &str,
        attempt: &str,
        operation: &str,
        wanted: &str,
        context: &CommandContext,
        live: bool,
    ) -> Result<(), String> {
        let approval = context
            .verification_recheck
            .as_ref()
            .ok_or("recheck approval evidence missing")?;
        require(
            *context
                == CommandContext {
                    verification_recheck: Some(approval.clone()),
                    approval_source: Some(APPROVAL_SOURCE.into()),
                    ..Default::default()
                },
            "recheck requires authenticated plain user approval",
        )?;
        require(
            approval.operation_id == operation
                && approval.review.task_id == task
                && approval.review.attempt_id == attempt
                && proof_digest(&approval.review)? == wanted,
            "recheck approval command scope mismatch",
        )?;
        let scope = self
            .config
            .execution_scope
            .as_ref()
            .ok_or("strict recheck scope missing")?;
        require(
            approval.review.project_id == scope.project.id,
            "recheck approval project mismatch",
        )?;
        let run = self.state().tasks.get(task).ok_or("recheck task missing")?;
        require(
            self.state().approved
                && self.state().revision_requested.is_none()
                && run.state == TaskState::Submitted
                && !run.verified
                && run.attempt.as_deref() == Some(attempt),
            "recheck requires current unverified submission",
        )?;
        for entry in &self.entries {
            require(
                entry.body.operation_id != operation,
                "recheck operation already used",
            )?;
            if let WorkflowCommand::ApproveVerificationRecheck {
                task: t,
                attempt: a,
                operation_id,
                ..
            } = &entry.body.event.command
            {
                require(
                    operation_id != operation,
                    "recheck operation already approved",
                )?;
                if t == task && a == attempt {
                    require(self.recheck_disposition(entry).is_some() || self.entries.iter().any(|result| {
                        result.body.operation_id==*operation_id
                            && matches!(&result.body.context.execution,
                                Some(ExecutionEvidence::VerificationRecheck {approval,..}) if approval.hash==entry.hash)
                    }),"previous recheck approval is unresolved; do not launch another operation")?;
                }
            }
        }
        let head = &self.entries.last().ok_or("recheck ledger empty")?.hash;
        #[cfg(windows)]
        if live {
            let _ = self.retired_recheck_operations()?;
        }
        self.with_recheck_basis(approval, self.entries.len(), head, |basis| {
            self.approval()
                .ok_or("original plan approval missing")?
                .validate(basis.plan, self.role_id(Role::User)?, scope.policy_version)?;
            if live {
                #[cfg(windows)]
                {
                    self.require_recheck_source(
                        basis.plan,
                        basis.manifest,
                        &approval.review.original_source_snapshot,
                    )?;
                    let root =
                        crate::artifacts::root_path(&basis.plan.execution_root(task, attempt)?)?;
                    let mut locks = vec![];
                    for check in &approval.review.replacement_checks {
                        let tool = check.tool.as_ref().ok_or("corrected check pins missing")?;
                        for file in std::iter::once(&tool.executable).chain(&tool.inputs) {
                            let path = fs::canonicalize(&file.path).map_err(|e| e.to_string())?;
                            require(
                                !crate::artifacts::contains_path(&root, &path),
                                "checker belongs to worker directory",
                            )?;
                        }
                        locks.extend(
                            crate::verifier::PinnedCheck::from_approved(check)?.lock(check)?,
                        );
                    }
                }
                #[cfg(not(windows))]
                {
                    return Err("recheck needs Windows runtime".into());
                }
            }
            Ok(())
        })
    }

    #[cfg(windows)]
    pub(super) fn require_recheck_source(
        &self,
        plan: &PlanRevision,
        manifest: &ArtifactManifest,
        seal: &SourceSnapshot,
    ) -> Result<(), String> {
        let (receipt, _) = self
            .completed_artifacts(&manifest.task_id, &manifest.attempt_id)
            .ok_or("submission missing")?;
        crate::artifacts::ArtifactObserver::new(
            plan,
            self.approval().ok_or("approval missing")?,
            self.role_id(Role::User)?,
            self.config
                .execution_scope
                .as_ref()
                .ok_or("scope missing")?
                .policy_version,
            self.attempt_record(&manifest.task_id, &manifest.attempt_id)
                .ok_or("attempt missing")?,
            receipt,
            receipt.turn_id.as_deref(),
        )?
        .require_unchanged(manifest)?;
        crate::source_snapshot::require_sealed(
            plan,
            manifest,
            seal,
            &self.root.join("verification"),
        )
    }

    pub(super) fn check_recheck_result(
        &self,
        operation: &str,
        command: &WorkflowCommand,
        context: &CommandContext,
        before: usize,
        live: bool,
    ) -> Result<(), String> {
        let WorkflowCommand::Verify {
            task,
            attempt,
            passed,
            evidence,
        } = command
        else {
            return Err("recheck result requires Verify".into());
        };
        let Some(ExecutionEvidence::VerificationRecheck {
            approval: anchor,
            result,
        }) = &context.execution
        else {
            return Err("recheck result evidence missing".into());
        };
        require(
            *context
                == CommandContext {
                    execution: context.execution.clone(),
                    ..Default::default()
                },
            "recheck result cannot carry additional authority",
        )?;
        let index = anchor
            .sequence
            .checked_sub(1)
            .ok_or("invalid recheck approval anchor")? as usize;
        let event = self
            .entries
            .get(index)
            .ok_or("recheck approval anchor missing")?;
        require(
            index + 1 == before
                && event.hash == anchor.hash
                && event.body.event.principal == self.role_id(Role::User)?,
            "recheck approval is stale or belongs to another authority",
        )?;
        let saved = event
            .body
            .context
            .verification_recheck
            .as_ref()
            .ok_or("recheck approval content missing")?;
        require(
            chrono::DateTime::parse_from_rfc3339(&result.report.observed_at)
                .map_err(|_| "invalid recheck observation time")?
                >= chrono::DateTime::parse_from_rfc3339(&event.body.received_at)
                    .map_err(|_| "invalid approval time")?,
            "recheck report predates its user approval",
        )?;
        require(
            event.body.event.command
                == WorkflowCommand::ApproveVerificationRecheck {
                    task: task.clone(),
                    attempt: attempt.clone(),
                    operation_id: operation.into(),
                    review_digest: proof_digest(&saved.review)?,
                }
                && saved.operation_id == operation
                && result.operation_id == operation
                && *evidence == vec![proof_digest(result)?],
            "recheck result operation, command or proof mismatch",
        )?;
        self.with_recheck_basis(saved, index, &event.body.previous_hash, |basis| {
            require(
                result.validate(&saved.review, basis)? == *passed,
                "recheck pass flag conflicts with observed report",
            )?;
            if live {
                #[cfg(windows)]
                {
                    self.require_recheck_source(
                        basis.plan,
                        basis.manifest,
                        &saved.review.original_source_snapshot,
                    )?;
                    self.require_recheck_source(
                        basis.plan,
                        basis.manifest,
                        result
                            .report
                            .source_snapshot
                            .as_ref()
                            .ok_or("recheck snapshot missing")?,
                    )?;
                }
                #[cfg(not(windows))]
                {
                    return Err("recheck needs Windows runtime".into());
                }
            }
            Ok(())
        })
    }

    pub(super) fn verified_report(
        &self,
        task: &str,
        attempt: &str,
    ) -> Result<&VerificationReport, String> {
        let (index,entry)=self.entries.iter().enumerate().rev().find(|(_,entry)|matches!(
            &entry.body.event.command,WorkflowCommand::Verify {task:t,attempt:a,..} if t==task && a==attempt))
            .ok_or("verification evidence missing")?;
        require(
            matches!(
                entry.body.event.command,
                WorkflowCommand::Verify { passed: true, .. }
            ),
            "last verification failed",
        )?;
        match &entry.body.context.execution {
            Some(ExecutionEvidence::Verification { report }) => {
                let plan = self.revision().ok_or("verification plan missing")?;
                let (_, manifest) = self
                    .completed_artifacts(task, attempt)
                    .ok_or("verification submission missing")?;
                report.require_pass(plan, manifest, &self.config.core_id)?;
                Ok(report)
            }
            Some(ExecutionEvidence::VerificationRecheck { result, .. }) => {
                self.check_recheck_result(
                    &entry.body.operation_id,
                    &entry.body.event.command,
                    &entry.body.context,
                    index,
                    false,
                )?;
                Ok(&result.report)
            }
            _ => Err("independent verification evidence missing".into()),
        }
    }
}
