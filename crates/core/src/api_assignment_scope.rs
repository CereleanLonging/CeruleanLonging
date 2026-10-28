//! API assignment authority is an original immutable workflow prefix, not a
//! worker-provided replacement of the manager's context. Other attempts may move.
use super::*;
use crate::{
    workflow_access::WorkflowAccess,
    workflow_coordinator::{Scope, WorkflowClient},
};

pub(super) enum ApiRead<'a> {
    Idle(&'a DurableWorkflow),
    Active(Box<WorkflowClient>),
}
impl std::ops::Deref for ApiRead<'_> {
    type Target = dyn WorkflowAccess;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Idle(store) => *store,
            Self::Active(client) => client.as_ref(),
        }
    }
}
pub(super) fn isolated(plan: &PlanRevision, task: &str) -> Result<bool, String> {
    Ok(plan.worktree.is_some()
        && (plan.task(task)?.api_worker.is_some()
            || (matches!(plan.task(task)?.adapter_id.as_str(), "omp" | "codex")
                && plan.task(task)?.worker.is_some()))
        && plan.workflow.tasks.iter().any(|t| {
            t.id == task
                && isolated_task_key(&plan.project.id, task).is_ok_and(|key| key == t.workspace_key)
        }))
}
pub(super) use crate::workflow_prefix::prefix;
/// Validate only an unchanged Pending target; no revision, approval, global or
/// same-task event may be hidden in the intervening history.
pub(super) fn unchanged_target(
    store: &dyn WorkflowAccess,
    original: &ExecutionSnapshot,
    task: &str,
    through: u32,
) -> Result<ExecutionSnapshot, String> {
    let actual = prefix(store, original.sequence)?;
    require(
        digest(&actual)? == digest(original)?,
        "API原管理快照不属于账本历史",
    )?;
    let current = prefix(store, through)?;
    require(
        original.sequence <= through
            && original.project == current.project
            && original.revision_digest == current.revision_digest
            && original.approval == current.approval
            && current.state.approved
            && current.state.revision_requested.is_none(),
        "API分配方案或批准已变化",
    )?;
    let plan = current.revision.as_ref().ok_or("API方案缺失")?;
    require(isolated(plan, task)?, "API并行分配需要批准的独立worktree")?;
    let before = original.state.tasks.get(task).ok_or("API原管理任务缺失")?;
    let after = current.state.tasks.get(task).ok_or("API当前任务缺失")?;
    require(
        before == after && after.state == TaskState::Pending && after.attempt.is_none(),
        "API分配目标已推进或不再待分配",
    )?;
    let definition = plan
        .workflow
        .tasks
        .iter()
        .find(|t| t.id == task)
        .ok_or("API任务定义缺失")?;
    require(
        definition.dependencies.iter().all(|d| {
            original
                .state
                .tasks
                .get(d)
                .is_some_and(|t| t.state == TaskState::Accepted)
                && current
                    .state
                    .tasks
                    .get(d)
                    .is_some_and(|t| t.state == TaskState::Accepted)
        }),
        "API原分配依赖尚未验收",
    )?;
    for entry in &store.history()[original.sequence as usize..through as usize] {
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
            "API授权间隔含目标或全局控制变更",
        )?;
    }
    Ok(current)
}
impl RuntimeHost {
    pub(super) fn worker_slot_available(&self) -> bool {
        self.api_active.len()
            + self.codex_active.len()
            + self.omp_active.len()
            + usize::from(self.desktop_active.is_some())
            < 2
    }
    pub(super) fn require_worker_slot(&self) -> Result<(), String> {
        require(
            self.worker_slot_available(),
            "模型任务并发上限为2，请等待现有任务",
        )
    }

    pub(super) fn api_read(
        &self,
        project: &str,
        task: &str,
        attempt: &str,
    ) -> Result<ApiRead<'_>, String> {
        if let Some(writer) = self.api_writers.get(project) {
            let revision = writer
                .snapshot
                .revision_digest
                .clone()
                .ok_or("API批准摘要缺失")?;
            return Ok(ApiRead::Active(Box::new(writer.owner.client(Scope {
                project: project.into(),
                task: task.into(),
                attempt: attempt.into(),
                revision,
            })?)));
        }
        let store = self.projects.get(project).ok_or("API项目不可用")?;
        require(!store.requires_reopen(), "API项目需要重开核对")?;
        Ok(ApiRead::Idle(store))
    }
    pub(super) fn validate_api_assignment_scope(
        &self,
        operation: &str,
        store: &dyn WorkflowAccess,
        task: &str,
        through: u32,
        check_live_context: bool,
    ) -> Result<ExecutionSnapshot, String> {
        let intent = self.management.intent(operation)?;
        require(
            intent.purpose == management::Purpose::Assign,
            "API授权不是分配决定",
        )?;
        let original: ExecutionSnapshot =
            serde_json::from_value(intent.context["workflow"].clone())
                .map_err(|_| "API原管理快照无效")?;
        require(
            original.project.id == intent.project_id,
            "API管理项目身份不符",
        )?;
        if check_live_context {
            let requirement = intent.context["requirement"]
                .as_str()
                .ok_or("管理需求缺失")?;
            require(
                digest(&self.rebuild_management_context(
                    intent,
                    self.context_for_snapshot(requirement, &original),
                )?)? == intent.state_digest,
                "API管理配置或用户答复已变化",
            )?;
        }
        unchanged_target(store, &original, task, through)?;
        if check_live_context && through as usize != store.history().len() {
            unchanged_target(store, &original, task, store.history().len() as u32)?;
        }
        Ok(original)
    }
    pub(super) fn scoped_api_decision(&self, operation: &str) -> Result<bool, String> {
        let intent = self.management.intent(operation)?;
        if intent.purpose != management::Purpose::Assign {
            return Ok(false);
        }
        let observation = self.management.original_envelope(operation)?;
        let management::ManagementDecision::Assign { task_id, .. } = &observation.decision else {
            return Ok(false);
        };
        let store = self.api_read(&intent.project_id, task_id, "scope-inspection")?;
        if !isolated(store.revision().ok_or("API方案缺失")?, task_id)? {
            return Ok(false);
        }
        self.validate_api_assignment_scope(
            operation,
            &*store,
            task_id,
            store.history().len() as u32,
            true,
        )?;
        Ok(true)
    }
}
