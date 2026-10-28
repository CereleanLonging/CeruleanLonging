//! Host-only management publication, deliberately absent from WorkflowClient.
//! Recording Advise is a local workflow event, never a remote delivery receipt.
use super::*;

pub(super) struct ManagementPublication {
    pub original: ExecutionSnapshot,
    pub operation: String,
    pub principal: String,
    pub attempt: String,
    pub command: WorkflowCommand,
    pub context: CommandContext,
}
impl ManagementPublication {
    pub(super) fn publish(self, store: &mut DurableWorkflow) -> Result<Entry, String> {
        require(
            self.context == CommandContext::default(),
            "共享管理发布不能附加执行或取消授权",
        )?;
        match &self.command {
            WorkflowCommand::Accept { task, attempt } => {
                require(
                    self.principal == "chief" && attempt == &self.attempt,
                    "共享收口仅允许主指挥原Accept",
                )?;
                crate::acceptance_scope::validate(store, &self.original, task, attempt)?;
            }
            WorkflowCommand::Advise { task, suggestion } => {
                require(
                    self.principal == "deputy"
                        && self.operation.starts_with("mgmt-")
                        && !suggestion.trim().is_empty()
                        && suggestion.len() <= 16 * 1024,
                    "API建议发布需要原副指挥管理操作和有界建议",
                )?;
                validate_api_advice(store, &self.original, task, &self.attempt)?;
            }
            _ => return Err("共享管理发布仅支持原Accept或API Advise".into()),
        }
        store.command(
            &self.operation,
            &self.principal,
            self.command,
            self.context,
            true,
        )
    }
}

fn validate_api_advice(
    store: &DurableWorkflow,
    original: &ExecutionSnapshot,
    task: &str,
    attempt: &str,
) -> Result<(), String> {
    let current = crate::async_verification::snapshot(store);
    require(
        digest(&current)? == digest(original)?,
        "API建议原工作流快照已变化",
    )?;
    let plan = current.revision.as_ref().ok_or("API建议原方案缺失")?;
    let state = current.state.tasks.get(task).ok_or("API建议原任务缺失")?;
    require(
        current.state.approved
            && current.state.revision_requested.is_none()
            && current
                .approval
                .as_ref()
                .is_some_and(|a| Some(&a.revision_digest) == current.revision_digest.as_ref())
            && current.revision_digest.as_deref() == Some(plan.approval_digest()?.as_str())
            && (cerulean_domain::api_worker::is_api_adapter(&plan.task(task)?.adapter_id)
                || plan.task(task)?.adapter_id == "omp")
            && state.state == cerulean_domain::workflow::TaskState::Running
            && state.attempt.as_deref() == Some(attempt)
            && state.cancellation_requested != Some(true),
        "API建议仅允许原已批准且未取消的运行尝试",
    )?;
    // The Host remains responsible for the original management response, fee
    // ACK and live boundary permission. This writer cannot grant any of them.
    Ok(())
}
impl WorkflowCoordinator {
    pub(crate) fn accept(
        &self,
        original: ExecutionSnapshot,
        operation: String,
        principal: String,
        task: String,
        attempt: String,
        context: CommandContext,
    ) -> Result<Entry, String> {
        self.publish_management(
            original,
            operation,
            principal,
            attempt.clone(),
            WorkflowCommand::Accept { task, attempt },
            context,
        )
    }
    pub(crate) fn publish_management(
        &self,
        original: ExecutionSnapshot,
        operation: String,
        principal: String,
        attempt: String,
        command: WorkflowCommand,
        context: CommandContext,
    ) -> Result<Entry, String> {
        let (reply, response) = mpsc::channel();
        self.sender
            .send(Message::ManagementPublication {
                request: Box::new(ManagementPublication {
                    original,
                    operation,
                    principal,
                    attempt,
                    command,
                    context,
                }),
                reply,
            })
            .map_err(|_| "管理发布协调器不可用，须核对原应用事务")?;
        receive(response)?
    }
}
