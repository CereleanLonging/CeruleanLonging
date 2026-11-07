//! Borrowed workflow event summaries for the existing bounded encoder.
//! No new retained cache, event schema, or authorization path.
use super::{Entry, WorkflowCommand};
use serde::{ser::SerializeSeq, Serialize, Serializer};

/// Exhaustive matching makes a new command variant require an explicit index label.
fn kind(command: &WorkflowCommand) -> &'static str {
    match command {
        WorkflowCommand::ApproveVerificationRecheck { .. } => "approve-verification-recheck",
        WorkflowCommand::RetireVerificationRecheck { .. } => "retire-verification-recheck",
        WorkflowCommand::ConfirmApiGenerationResolution { .. } => {
            "confirm-api-generation-resolution"
        }
        WorkflowCommand::ConfirmApiCountResolution { .. } => "confirm-api-count-resolution",
        WorkflowCommand::ConfirmManagementCountResolution { .. } => {
            "confirm-management-count-resolution"
        }
        WorkflowCommand::ConfirmHistoricalCharge { .. } => "confirm-historical-charge",
        WorkflowCommand::AuthorizePlanningBudget { .. } => "authorize-planning-budget",
        WorkflowCommand::RetireSubmittedPlan { .. } => "retire-submitted-plan",
        WorkflowCommand::RequestRevision { .. } => "request-revision",
        WorkflowCommand::Propose { .. } => "propose",
        WorkflowCommand::Approve { .. } => "approve",
        WorkflowCommand::Assign { .. } => "assign",
        WorkflowCommand::AbandonUnsent { .. } => "abandon-unsent",
        WorkflowCommand::RetireUnknownDesktop { .. } => "retire-unknown-desktop",
        WorkflowCommand::RetireUnknownApi { .. } => "retire-unknown-api",
        WorkflowCommand::DispatchIntent { .. } => "dispatch-intent",
        WorkflowCommand::Started { .. } => "started",
        WorkflowCommand::CancelIntent { .. } => "cancel-intent",
        WorkflowCommand::RecoveryUnknown { .. } => "recovery-unknown",
        WorkflowCommand::Reconcile { .. } => "reconcile",
        WorkflowCommand::Submit { .. } => "submit",
        WorkflowCommand::Verify { .. } => "verify",
        WorkflowCommand::Accept { .. } => "accept",
        WorkflowCommand::Advise { .. } => "advise",
        WorkflowCommand::ReturnForRework { .. } => "return-for-rework",
    }
}

// Field ordering and null treatment deliberately match derived_index::IndexedEvent.
#[derive(Serialize)]
struct Row<'a> {
    sequence: u64,
    operation_id: &'a str,
    kind: &'static str,
    received_at: &'a str,
    hash: &'a str,
    task: Option<&'a str>,
    attempt: Option<&'a str>,
}
impl<'a> Row<'a> {
    fn new(entry: &'a Entry) -> Self {
        let event = &entry.body.event;
        Self {
            sequence: event.sequence.into(),
            operation_id: &entry.body.operation_id,
            kind: kind(&event.command),
            received_at: &entry.body.received_at,
            hash: &entry.hash,
            task: event.command.task(),
            attempt: event.command.attempt(),
        }
    }
}

/// This is only a stack view while encoding; queued work retains the same Encoded.
pub(super) struct Events<'a>(pub &'a [Entry]);
impl Serialize for Events<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.0.len()))?;
        for entry in self.0 {
            sequence.serialize_element(&Row::new(entry))?;
        }
        sequence.end()
    }
}
