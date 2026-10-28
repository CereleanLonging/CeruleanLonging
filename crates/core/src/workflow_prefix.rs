//! Reconstruct an authoritative immutable prefix of the existing workflow journal.
use crate::workflow_access::WorkflowAccess;
use cerulean_domain::execution::*;

pub(crate) fn prefix(
    store: &dyn WorkflowAccess,
    sequence: u32,
) -> Result<ExecutionSnapshot, String> {
    let history = store
        .history()
        .get(..sequence as usize)
        .ok_or("API授权历史前缀越界")?;
    let events: Vec<_> = history.iter().map(|e| e.body.event.clone()).collect();
    let mut workflow = store.configuration().workflow()?;
    for event in &events {
        require(
            event.sequence as usize == workflow.journal().len() + 1,
            "workflow prefix sequence gap",
        )?;
        workflow.apply(&event.principal, event.command.clone())?;
    }
    let revision = history
        .iter()
        .rev()
        .find_map(|e| match &e.body.context.execution {
            Some(ExecutionEvidence::Revision { revision }) => Some(revision.clone()),
            _ => None,
        })
        .ok_or("API授权前缀缺少方案")?;
    let revision_digest = revision.approval_digest()?;
    let approval = history
        .iter()
        .rev()
        .find_map(|e| match &e.body.context.execution {
            Some(ExecutionEvidence::Approval { approval })
                if approval.revision_digest == revision_digest =>
            {
                Some(approval.clone())
            }
            _ => None,
        })
        .ok_or("API授权前缀缺少批准")?;
    Ok(ExecutionSnapshot {
        project: revision.project.clone(),
        revision: Some(revision),
        revision_digest: Some(revision_digest),
        approval: Some(approval),
        state: workflow.state().clone(),
        sequence,
    })
}
