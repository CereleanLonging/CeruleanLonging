//! A completed, independently rejected submission can receive a new attempt.
//! Rework keeps the approved plan, checker and worker directory authority.
use crate::scheduler::Entry;
use cerulean_domain::{execution::*, workflow::WorkflowCommand};

pub(crate) fn append_prompt(
    mut prompt: String,
    history: &[Entry],
    task: &str,
    attempt: &str,
) -> Result<String, String> {
    // Anchor to this assignment, so later advice never rewrites an old prompt.
    let assignment = history
        .iter()
        .position(|e| {
            matches!(
                &e.body.event.command, WorkflowCommand::Assign { task: t, attempt: a, .. }
                    if t == task && a == attempt
            )
        })
        .ok_or("rework prompt assignment missing")?;
    for entry in history[..assignment].iter().rev() {
        match &entry.body.event.command {
            WorkflowCommand::Propose { .. } => break,
            WorkflowCommand::ReturnForRework {
                task: t,
                attempt: previous,
                verification_operation,
                report_digest,
                suggestion,
            } if t == task => {
                let check = history[..assignment]
                    .iter()
                    .find(|e| e.body.operation_id == *verification_operation)
                    .ok_or("rework check anchor missing")?;
                let report = match &check.body.context.execution {
                    Some(ExecutionEvidence::Verification { report }) => report,
                    Some(ExecutionEvidence::VerificationRecheck { result, .. }) => &result.report,
                    _ => return Err("rework report missing".into()),
                };
                require(digest(report)? == *report_digest, "rework report changed")?;
                let feedback = serde_json::json!({
                    "previousAttempt":previous,"reportDigest":report_digest,
                    "deputyInstructions":suggestion,"checks":report.checks
                });
                prompt.push_str(" Rework feedback for this task only. Correct the delivered source within the original approved scope and directory. Keep the approved checks and acceptance criteria. If the feedback requires a plan change, stop and report it. Feedback data: ");
                prompt.push_str(&serde_json::to_string(&feedback).map_err(|e| e.to_string())?);
                break;
            }
            _ => (),
        }
    }
    Ok(prompt)
}
