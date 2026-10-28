//! An immutable native-running task in the original management snapshot.
use crate::{workflow_access::WorkflowAccess, workflow_prefix::prefix};
use cerulean_domain::{
    execution::*,
    workflow::{TaskState, WorkflowCommand},
};

pub(crate) fn validate(
    store: &dyn WorkflowAccess,
    original: &ExecutionSnapshot,
    task: &str,
    attempt: &str,
    thread: &str,
    turn: &str,
    through: u32,
) -> Result<(), String> {
    require(
        digest(&prefix(store, original.sequence)?)? == digest(original)?,
        "监督原快照不属于账本历史",
    )?;
    let current = prefix(store, through)?;
    require(
        original.sequence <= through
            && original.project == current.project
            && original.revision_digest == current.revision_digest
            && original.approval == current.approval
            && current.state.approved
            && current.state.revision_requested.is_none(),
        "监督原方案或批准变化",
    )?;
    require(
        crate::acceptance_scope::isolated(current.revision.as_ref().ok_or("监督方案缺失")?, task)?,
        "共享监督需要批准的独立目录",
    )?;
    let before = original.state.tasks.get(task).ok_or("监督原任务缺失")?;
    let after = current.state.tasks.get(task).ok_or("监督当前任务缺失")?;
    require(
        before == after
            && after.state == TaskState::Running
            && after.attempt.as_deref() == Some(attempt)
            && after.cancellation_requested != Some(true),
        "监督原运行尝试变化",
    )?;
    require(
        store.history()[..original.sequence as usize]
            .iter()
            .any(|e| {
                matches!(&e.body.event.command,
        WorkflowCommand::Started{task:t,attempt:a} if t==task && a==attempt)
                    && e.body.context.thread_id.as_deref() == Some(thread)
                    && e.body.context.turn_id.as_deref() == Some(turn)
            }),
        "监督缺少原Started轮次",
    )?;
    for e in &store.history()[original.sequence as usize..through as usize] {
        let command = &e.body.event.command;
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
            "监督授权间隔含目标或全局控制变更",
        )?;
    }
    Ok(())
}
