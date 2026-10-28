//! Shared durable assignment exchange for owned Codex and OMP workers.
use crate::scheduler::{CommandContext, Entry};
use crate::workflow_access::WorkflowAccess;
use cerulean_domain::{
    execution::{Attempt, ExecutionEvidence, ExecutionSnapshot},
    workflow::WorkflowCommand,
};
use tokio::sync::oneshot;
pub(crate) enum Update {
    Prepare {
        operation: String,
        attempt: Box<Attempt>,
        snapshot: Box<ExecutionSnapshot>,
        reply: oneshot::Sender<Result<(), String>>,
    },
    Commit {
        operation: String,
        entry: Box<Entry>,
        snapshot: Box<ExecutionSnapshot>,
        reply: oneshot::Sender<Result<(), String>>,
    },
}
async fn acknowledged(reply: oneshot::Receiver<Result<(), String>>) -> Result<(), String> {
    tokio::time::timeout(std::time::Duration::from_secs(30), reply)
        .await
        .map_err(|_| "management assignment acknowledgement timed out")?
        .map_err(|_| "management assignment acknowledgement lost")?
}
pub(crate) async fn assign(
    store: &mut impl WorkflowAccess,
    operation: &str,
    attempt: Attempt,
    mut notify: impl FnMut(Update) -> Result<(), String>,
) -> Result<(), String> {
    store.refresh()?;
    let (reply, response) = oneshot::channel();
    notify(Update::Prepare {
        operation: operation.into(),
        attempt: Box::new(attempt.clone()),
        snapshot: Box::new(crate::async_verification::snapshot(store)),
        reply,
    })?;
    acknowledged(response).await?;
    let entry = store.command(
        &format!("mgmt-{operation}"),
        "deputy",
        WorkflowCommand::Assign {
            task: attempt.task_id.clone(),
            worker: attempt.worker_id.clone(),
            attempt: attempt.id.clone(),
        },
        CommandContext {
            execution: Some(ExecutionEvidence::Attempt { attempt }),
            ..Default::default()
        },
        true,
    )?;
    let (reply, response) = oneshot::channel();
    notify(Update::Commit {
        operation: operation.into(),
        entry: Box::new(entry),
        snapshot: Box::new(crate::async_verification::snapshot(store)),
        reply,
    })?;
    acknowledged(response).await
}
