//! Host-issued OMP capability; API worker privileges remain unchanged.
use super::*;
use crate::omp_workflow_access::OmpWorkflowAccess;
use cerulean_domain::workflow::TaskState;

pub(super) fn validate(scope: &Scope, store: &DurableWorkflow) -> Result<(), String> {
    scope.validate(store)?;
    require(
        store
            .revision()
            .ok_or("OMP方案缺失")?
            .task(&scope.task)?
            .adapter_id
            == "omp",
        "OMP协调器拒绝其他执行器身份",
    )
}
pub(super) fn command(
    scope: &Scope,
    store: &DurableWorkflow,
    command: &WorkflowCommand,
    context: &CommandContext,
) -> Result<(), String> {
    require(
        command.task() == Some(scope.task.as_str())
            && command.attempt() == Some(scope.attempt.as_str()),
        "OMP协调器拒绝跨任务或跨尝试命令",
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
        "OMP执行线程不能批准、验收或修订方案",
    )?;
    if matches!(command, WorkflowCommand::CancelIntent { .. }) {
        require(
            context.cancellation_source.as_deref() == Some("user"),
            "OMP停止必须保留原用户来源",
        )?;
    }
    if matches!(command, WorkflowCommand::DispatchIntent { .. }) {
        let (plan, _, _, _) = crate::omp_execution::scope(store, &scope.task, &scope.attempt)?;
        require(
            plan.policy.budget_usd_cents.is_none(),
            "OMP金额硬预算尚不能执行",
        )?;
        let state = store.state().tasks.get(&scope.task).ok_or("OMP任务缺失")?;
        require(
            state.state == TaskState::Assigned
                && state.attempt.as_deref() == Some(scope.attempt.as_str()),
            "OMP派发需要原已分配尝试",
        )?;
        require(
            !store.history().iter().any(|e| {
                matches!(&e.body.event.command,
            WorkflowCommand::DispatchIntent {attempt,..} if attempt==&scope.attempt)
            }),
            "OMP原尝试已有派发意图，禁止重发",
        )?;
        crate::omp_workflow_access::require_budget(store)?;
    }
    Ok(())
}
impl WorkflowCoordinator {
    pub(crate) fn omp_client(&self, scope: Scope) -> Result<WorkflowClient, String> {
        let mut client = self.client(scope)?;
        require(
            client
                .revision()
                .ok_or("OMP方案缺失")?
                .task(&client.scope.task)?
                .adapter_id
                == "omp",
            "OMP客户端与批准执行器不符",
        )?;
        client.kind = ClientKind::Omp;
        Ok(client)
    }
}
impl WorkflowClient {
    fn omp_paths(&self, attempt: &str) -> Result<(PathBuf, PathBuf), String> {
        require(
            self.kind == ClientKind::Omp && !self.failed && attempt == self.scope.attempt,
            "OMP证据路径超出原尝试或客户端失去确认",
        )?;
        let (reply, response) = mpsc::channel();
        self.sender
            .send(Message::OmpPaths {
                scope: self.scope.clone(),
                reply,
            })
            .map_err(|_| "OMP协调器不可用")?;
        receive(response)?
    }
}
impl OmpWorkflowAccess for WorkflowClient {
    fn omp_attempt_path(&self, attempt: &str) -> Result<PathBuf, String> {
        self.omp_paths(attempt).map(|p| p.0)
    }
    fn omp_file_service_path(&self, attempt: &str) -> Result<PathBuf, String> {
        self.omp_paths(attempt).map(|p| p.1)
    }
    fn requires_reopen(&self) -> bool {
        self.failed
    }
    fn record_omp(
        &mut self,
        who: &str,
        command: WorkflowCommand,
        context: CommandContext,
    ) -> Result<(), String> {
        require(self.kind == ClientKind::Omp, "API客户端不能使用OMP能力")?;
        self.apply(None, who, true, command, context).map(|_| ())
    }
    fn prepare_omp_workspace(
        &mut self,
        task: &str,
        attempt: &str,
        cancel: Arc<AtomicBool>,
    ) -> Result<(String, WorkspaceProof), String> {
        require(self.kind == ClientKind::Omp, "API客户端不能准备OMP目录")?;
        self.prepare_task_workspace_cancellable(task, attempt, cancel)
    }
}
