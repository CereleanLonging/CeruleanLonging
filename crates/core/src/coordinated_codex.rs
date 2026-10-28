//! Host-issued native-turn capability; other executor command rights are unchanged.
use super::*;
use crate::codex_workflow_access::CodexWorkflowAccess;
use cerulean_domain::workflow::{Role, TaskState};

pub(super) fn validate(scope: &Scope, store: &DurableWorkflow) -> Result<(), String> {
    scope.validate(store)?;
    require(
        store
            .revision()
            .ok_or("Codex方案缺失")?
            .task(&scope.task)?
            .adapter_id
            == "codex",
        "Codex协调器拒绝其他执行器身份",
    )
}
pub(super) fn command(
    scope: &Scope,
    store: &DurableWorkflow,
    operation: &str,
    command: &WorkflowCommand,
    context: &CommandContext,
) -> Result<(), String> {
    require(
        command.task() == Some(scope.task.as_str()),
        "Codex协调器拒绝跨任务命令",
    )?;
    if matches!(command, WorkflowCommand::Advise { .. }) {
        let run = store
            .state()
            .tasks
            .get(&scope.task)
            .ok_or("Codex任务缺失")?;
        require(
            operation.starts_with("mgmt-")
                && run.state == TaskState::Running
                && run.attempt.as_deref() == Some(scope.attempt.as_str()),
            "Codex纠偏必须绑定原运行尝试和管理操作",
        )?;
    } else {
        require(
            command.attempt() == Some(scope.attempt.as_str()),
            "Codex协调器拒绝跨尝试命令",
        )?;
        require(
            matches!(
                command,
                WorkflowCommand::Assign { .. }
                    | WorkflowCommand::DispatchIntent { .. }
                    | WorkflowCommand::Started { .. }
                    | WorkflowCommand::Submit { .. }
                    | WorkflowCommand::RecoveryUnknown { .. }
                    | WorkflowCommand::Reconcile { .. }
                    | WorkflowCommand::CancelIntent { .. }
            ),
            "Codex线程不能批准、验收或修订方案",
        )?;
    }
    if matches!(command, WorkflowCommand::CancelIntent { .. }) {
        let user = &store
            .configuration()
            .principals
            .iter()
            .find(|p| p.role == Role::User)
            .ok_or("Codex用户身份缺失")?
            .id;
        require(
            context.cancellation_source.as_deref()
                == Some(format!("authenticated local-user cancellation: {user}").as_str()),
            "Codex停止必须保留原用户来源",
        )?;
    }
    if matches!(command, WorkflowCommand::DispatchIntent { .. }) {
        let plan = store.revision().ok_or("Codex方案缺失")?;
        let run = store
            .state()
            .tasks
            .get(&scope.task)
            .ok_or("Codex任务缺失")?;
        require(
            plan.policy.budget_usd_cents.is_none(),
            "Codex金额硬预算尚不能执行",
        )?;
        require(
            run.state == TaskState::Assigned
                && run.attempt.as_deref() == Some(scope.attempt.as_str()),
            "Codex派发需要原已分配尝试",
        )?;
        require(
            !store.history().iter().any(|e| {
                matches!(&e.body.event.command,
            WorkflowCommand::DispatchIntent { attempt, .. } if attempt == &scope.attempt)
            }),
            "Codex原尝试已有派发意图，禁止重发",
        )?;
        crate::codex_workflow_access::require_budget(store)?;
    }
    Ok(())
}
impl WorkflowCoordinator {
    pub(crate) fn codex_client(&self, scope: Scope) -> Result<WorkflowClient, String> {
        let mut client = self.client(scope)?;
        require(
            client
                .revision()
                .ok_or("Codex方案缺失")?
                .task(&client.scope.task)?
                .adapter_id
                == "codex",
            "Codex客户端与批准执行器不符",
        )?;
        client.kind = ClientKind::Codex;
        Ok(client)
    }
}
impl CodexWorkflowAccess for WorkflowClient {
    fn requires_reopen(&self) -> bool {
        self.failed
    }
    fn started_cancellation_anchors(
        &self,
        task: &str,
        attempt: &str,
    ) -> Result<StartedCancellationAnchors, String> {
        require(
            self.kind == ClientKind::Codex
                && !self.failed
                && task == self.scope.task
                && attempt == self.scope.attempt,
            "Codex取消证据超出原尝试或客户端失去确认",
        )?;
        let (reply, response) = mpsc::channel();
        self.sender
            .send(Message::CodexCancellationAnchors {
                scope: self.scope.clone(),
                reply,
            })
            .map_err(|_| "Codex协调器不可用")?;
        receive(response)?
    }
    fn prepare_codex_workspace(
        &mut self,
        task: &str,
        attempt: &str,
        cancel: Arc<AtomicBool>,
    ) -> Result<(String, WorkspaceProof), String> {
        require(
            self.kind == ClientKind::Codex,
            "其他客户端不能准备Codex目录",
        )?;
        self.prepare_task_workspace_cancellable(task, attempt, cancel)
    }
}
