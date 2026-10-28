//! Independently replayable proof for a started turn interrupted before recording input.
use super::require;
use super::*;
use cerulean_domain::{execution::*, workflow::ReconcileOutcome};

impl DurableWorkflow {
    pub fn started_cancellation_anchors(
        &self,
        task: &str,
        id: &str,
    ) -> Result<StartedCancellationAnchors, String> {
        let run = self.state().tasks.get(task).ok_or("task missing")?;
        require(
            matches!(run.state, TaskState::Cancelling | TaskState::Unknown)
                && run.attempt.as_deref() == Some(id)
                && run.cancellation_requested == Some(true),
            "original cancelling attempt required",
        )?;
        let attempt = self.attempt_record(task, id).ok_or("attempt missing")?;
        require(
            attempt.binding.adapter_id == "codex"
                && attempt.binding.identity_strength == IdentityStrength::ProviderId,
            "early cancellation requires Codex provider identity",
        )?;
        let starts: Vec<_> = self
            .entries
            .iter()
            .filter(|e| {
                matches!(&e.body.event.command,
            WorkflowCommand::Started { task:t, attempt:a } if t == task && a == id)
            })
            .collect();
        let cancels: Vec<_> = self
            .entries
            .iter()
            .filter(|e| {
                matches!(&e.body.event.command,
            WorkflowCommand::CancelIntent { task:t, attempt:a, .. } if t == task && a == id)
            })
            .collect();
        require(
            starts.len() == 1 && cancels.len() == 1,
            "unique original Started and CancelIntent required",
        )?;
        let (started, cancel) = (starts[0], cancels[0]);
        let sent = self
            .last_intent(task, id)
            .ok_or("dispatch intent missing")?;
        require(
            sent.body.event.sequence < started.body.event.sequence
                && started.body.event.sequence < cancel.body.event.sequence,
            "cancellation anchor order mismatch",
        )?;
        let Some(ExecutionEvidence::Receipt {
            receipt,
            manifest: None,
        }) = &started.body.context.execution
        else {
            return Err("original Started receipt required".into());
        };
        let turn = started
            .body
            .context
            .turn_id
            .as_deref()
            .ok_or("Started turn missing")?;
        receipt.validate(
            self.revision().ok_or("revision missing")?,
            attempt,
            Some(turn),
        )?;
        require(
            receipt.status == ReceiptStatus::Running
                && started.body.event.principal == self.config.core_id
                && cancel.body.event.principal == self.config.core_id
                && started.body.context.thread_id.as_ref() == Some(&attempt.binding.session_id)
                && cancel.body.context.thread_id == started.body.context.thread_id
                && cancel.body.context.turn_id == started.body.context.turn_id
                && cancel.body.context.cwd.as_ref() == Some(&attempt.binding.workspace)
                && cancel.body.context.cancellation_source.as_deref()
                    == Some(
                        format!(
                            "authenticated local-user cancellation: {}",
                            self.role_id(Role::User)?
                        )
                        .as_str(),
                    ),
            "cancellation anchor binding mismatch",
        )?;
        Ok(StartedCancellationAnchors {
            started: EventAnchor {
                sequence: started.body.event.sequence,
                hash: started.hash.clone(),
            },
            cancellation: EventAnchor {
                sequence: cancel.body.event.sequence,
                hash: cancel.hash.clone(),
            },
        })
    }

    pub(super) fn check_started_turn_cancellation(
        &self,
        command: &WorkflowCommand,
        ctx: &CommandContext,
        attempt: &Attempt,
        receipt: &ExecutionReceipt,
        proof: &StartedTurnCancellationProof,
    ) -> Result<(), String> {
        let WorkflowCommand::Reconcile {
            task,
            attempt: id,
            outcome: ReconcileOutcome::Cancelled,
            evidence,
        } = command
        else {
            return Err("Started-turn proof permits only cancelled reconciliation".into());
        };
        let anchors = self.started_cancellation_anchors(task, id)?;
        require(
            proof.schema_version == 1 && proof.anchors == anchors,
            "cancellation proof anchor mismatch",
        )?;
        let started = &self.entries[anchors.started.sequence as usize - 1];
        let cancel = &self.entries[anchors.cancellation.sequence as usize - 1];
        let obs = &proof.observation;
        let turn = started
            .body
            .context
            .turn_id
            .as_deref()
            .ok_or("Started turn missing")?;
        require(
            obs.thread_id == attempt.binding.session_id
                && obs.turn_id == turn
                && obs.workspace == attempt.binding.workspace
                && obs.status == "interrupted"
                && obs.items_view == "full"
                && obs.item_count == 0
                && obs.matching_turns == 1,
            "empty interrupted turn proof mismatch",
        )?;
        timestamp(&proof.observed_at)?;
        require(
            chrono::DateTime::parse_from_rfc3339(&proof.observed_at)
                .map_err(|_| "invalid observation time")?
                >= chrono::DateTime::parse_from_rfc3339(&cancel.body.received_at)
                    .map_err(|_| "invalid cancellation time")?,
            "observation predates cancellation",
        )?;
        receipt.validate(
            self.revision().ok_or("revision missing")?,
            attempt,
            Some(turn),
        )?;
        let hash = cerulean_domain::execution::digest(proof)?;
        require(
            receipt.status == ReceiptStatus::Cancelled
                && receipt.evidence_sha256 == hash
                && receipt.observed_at == proof.observed_at
                && evidence == &vec![hash]
                && ctx.thread_id.as_ref() == Some(&obs.thread_id)
                && ctx.turn_id.as_ref() == Some(&obs.turn_id)
                && ctx.terminal_status.as_deref() == Some("interrupted")
                && ctx.cwd.as_ref() == Some(&obs.workspace)
                && ctx.approval_source.is_none()
                && ctx.cancellation_source.is_none(),
            "cancellation proof receipt or context mismatch",
        )
    }
}
