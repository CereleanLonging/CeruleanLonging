//! Chief acceptance binds to an unchanged verified submission in the original journal.
use crate::{workflow_access::WorkflowAccess, workflow_prefix::prefix};
use cerulean_domain::{
    execution::*,
    workflow::{TaskState, WorkflowCommand},
};

pub(crate) fn isolated(plan: &PlanRevision, task: &str) -> Result<bool, String> {
    Ok(plan.worktree.is_some()
        && plan.workflow.tasks.iter().any(|t| {
            t.id == task
                && isolated_task_key(&plan.project.id, task).is_ok_and(|key| key == t.workspace_key)
        }))
}

pub(crate) fn validate(
    store: &dyn WorkflowAccess,
    original: &ExecutionSnapshot,
    task: &str,
    attempt: &str,
) -> Result<(), String> {
    let actual = prefix(store, original.sequence)?;
    require(
        digest(&actual)? == digest(original)?,
        "主指挥原快照不属于账本历史",
    )?;
    let current = prefix(store, store.history().len() as u32)?;
    require(
        original.project == current.project
            && original.revision_digest == current.revision_digest
            && original.approval == current.approval
            && current.state.approved
            && current.state.revision_requested.is_none(),
        "主指挥原方案或批准已变化",
    )?;
    require(
        isolated(current.revision.as_ref().ok_or("验收方案缺失")?, task)?,
        "并行收口需要批准的独立任务目录",
    )?;
    let before = original.state.tasks.get(task).ok_or("原收口任务缺失")?;
    let after = current.state.tasks.get(task).ok_or("当前收口任务缺失")?;
    require(
        before == after
            && after.state == TaskState::Submitted
            && after.verified
            && after.attempt.as_deref() == Some(attempt),
        "收口目标已变化或没有通过独立验收",
    )?;
    // The historical snapshot must include the real verifier evidence, not a worker claim.
    require(store.history()[..original.sequence as usize].iter().any(|e|
        matches!(&e.body.event.command, WorkflowCommand::Verify {task: t,attempt: a,passed:true,..}
            if t==task && a==attempt)
        && matches!(&e.body.context.execution,Some(ExecutionEvidence::Verification {..}))),
        "收口缺少原独立验收入账证据")?;
    for entry in &store.history()[original.sequence as usize..] {
        let command = &entry.body.event.command;
        require(
            command.task().is_some_and(|other| other != task)
                && matches!(
                    command,
                    WorkflowCommand::Assign { .. }
                        | WorkflowCommand::DispatchIntent { .. }
                        | WorkflowCommand::Started { .. }
                        | WorkflowCommand::Submit { .. }
                        | WorkflowCommand::RecoveryUnknown { .. }
                        | WorkflowCommand::Reconcile { .. }
                        | WorkflowCommand::Verify { .. }
                        | WorkflowCommand::Accept { .. }
                        | WorkflowCommand::CancelIntent { .. }
                        | WorkflowCommand::Advise { .. }
                ),
            "收口授权间隔含目标或全局控制变更",
        )?;
    }
    Ok(())
}
