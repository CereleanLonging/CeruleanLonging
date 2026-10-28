//! OMP joins the same two-phase management assignment contract as Codex.
use super::*;
use crate::{
    management_execution::{ApplicationTarget, UserResolution},
    startup_journal::StartupIntent,
};
use cerulean_domain::management::{ManagementDecision, Purpose};

fn binding(
    i: &StartupIntent,
    command: &WorkflowCommand,
    context: &CommandContext,
    fence: u32,
    plan: &PlanRevision,
) -> Result<(), String> {
    let Some(ExecutionEvidence::Attempt { attempt }) = &context.execution else {
        return Err("OMP分配会话证据缺失".into());
    };
    plan.validate_attempt_workspace(attempt)?;
    require(
        matches!(command,WorkflowCommand::Assign{task,worker,attempt:id} if task==&i.task_id&&worker=="worker-omp"&&id==&i.attempt_id)
            && attempt.id == i.attempt_id
            && attempt.task_id == i.task_id
            && attempt.worker_id == "worker-omp"
            && attempt.revision_digest == i.revision_digest
            && attempt.fence == fence
            && attempt.binding.adapter_id == "omp"
            && matches!(
                attempt.binding.identity_strength,
                IdentityStrength::ControllerRequest(_)
            ),
        "OMP启动与管理分配绑定不符",
    )?;
    cerulean_adapters::omp_transport::worker::DispatchIntent {
        version: 1,
        task_id: i.task_id.clone(),
        attempt_id: i.attempt_id.clone(),
        revision_digest: i.revision_digest.clone(),
        fence,
        binding: attempt.binding.clone(),
        provider: i.worker.provider.clone(),
        model: i.worker.model.clone(),
        prompt_sha256: "0".repeat(64),
    }
    .validate_metadata()?;
    Ok(())
}
impl RuntimeHost {
    pub fn start_managed_omp(&mut self, operation: &str, attempt: &str) -> Result<(), String> {
        self.poll_verifications();
        require(
            self.management.application(operation).is_none(),
            "管理决定已准备或应用，不能重新启动",
        )?;
        require(
            !self
                .omp_startups
                .all()
                .iter()
                .chain(self.startups.all().iter())
                .any(|r| r.intent.management_operation.as_deref() == Some(operation)),
            "管理决定已经拥有启动尝试",
        )?;
        let project = self.management.intent(operation)?.project_id.clone();
        let observed = self.management_envelope(operation)?;
        let ManagementDecision::Assign {
            task_id, worker_id, ..
        } = &observed.decision
        else {
            return Err("需要副指挥分配决定".into());
        };
        require(worker_id == "worker-omp", "决定选择了其他执行器")?;
        let task = task_id.clone();
        let revision = self
            .omp_read(&project, &task, attempt)?
            .revision()
            .ok_or("OMP批准方案缺失")?
            .approval_digest()?;
        self.start_omp_inner(&project, &task, attempt, &revision, Some(operation.into()))
    }
    pub(super) fn validate_omp_managed_links(&self) -> Result<(), String> {
        let mut linked = std::collections::BTreeSet::new();
        for record in self.omp_startups.all() {
            let i = &record.intent;
            let store = self.projects.get(&i.project_id).ok_or("OMP项目账本缺失")?;
            let prefix = store
                .history()
                .get(..i.workflow_sequence as usize)
                .ok_or("OMP启动锚点无效")?;
            let events = prefix
                .iter()
                .map(|e| e.body.event.clone())
                .collect::<Vec<_>>();
            let replay = Workflow::replay(store.configuration().principals.clone(), &events)?;
            let task = replay
                .state()
                .tasks
                .get(&i.task_id)
                .ok_or("OMP启动锚点任务缺失")?;
            require(
                replay.state().approved
                    && task.state == TaskState::Pending
                    && task.attempt.is_none(),
                "OMP启动锚点不是已批准的待分配任务",
            )?;
            let revision = prefix
                .iter()
                .rev()
                .find_map(|e| match &e.body.context.execution {
                    Some(ExecutionEvidence::Revision { revision }) => Some(revision),
                    _ => None,
                })
                .ok_or("OMP启动锚点方案缺失")?;
            let approval = prefix
                .iter()
                .rev()
                .find_map(|e| match &e.body.context.execution {
                    Some(ExecutionEvidence::Approval { approval })
                        if approval.revision_digest == i.revision_digest =>
                    {
                        Some(approval)
                    }
                    _ => None,
                })
                .ok_or("OMP启动锚点批准缺失")?;
            approval.validate(
                revision,
                "user",
                store
                    .configuration()
                    .execution_scope
                    .as_ref()
                    .ok_or("scope missing")?
                    .policy_version,
            )?;
            require(
                revision.approval_digest()? == i.revision_digest
                    && revision.project.id == i.project_id
                    && revision.task(&i.task_id)?.adapter_id == "omp"
                    && revision.task(&i.task_id)?.worker.as_ref() == Some(&i.worker),
                "OMP历史批准与启动不符",
            )?;
            let definition = revision
                .workflow
                .tasks
                .iter()
                .find(|t| t.id == i.task_id)
                .ok_or("OMP任务定义缺失")?;
            require(
                definition.dependencies.iter().all(|d| {
                    replay
                        .state()
                        .tasks
                        .get(d)
                        .is_some_and(|t| t.state == TaskState::Accepted)
                }),
                "OMP启动前依赖未验收",
            )?;
            let fence = task.fence.checked_add(1).ok_or("fence overflow")?;
            let owner = i.management_operation.as_ref().map_or_else(
                || format!("omp-{}-assign", i.attempt_id),
                |op| format!("mgmt-{op}"),
            );
            for e in store.history().iter().filter(|e|matches!(&e.body.event.command,WorkflowCommand::Assign{attempt,..} if attempt==&i.attempt_id)||e.body.operation_id==owner){
                require(e.body.operation_id==owner&&e.body.event.principal=="deputy"&&e.body.event.sequence>i.workflow_sequence,"OMP分配来源与启动不符")?;
                binding(i,&e.body.event.command,&e.body.context,fence,revision)?;
                if i.schema_version == 2 {
                    self.validate_api_assignment_scope(i.management_operation.as_deref().ok_or("OMP管理归属缺失")?, store, &i.task_id, e.body.event.sequence.checked_sub(1).ok_or("OMP分配序号无效")?, false)?;
                }
            }
            for status in self
                .management
                .statuses()
                .iter()
                .filter(|s| s.project_id == i.project_id)
            {
                if let Some((
                    ApplicationTarget::Workflow {
                        command: WorkflowCommand::Assign { attempt, .. },
                        ..
                    },
                    _,
                    _,
                )) = self.management.application(&status.operation_id)
                {
                    if attempt == &i.attempt_id {
                        require(
                            i.management_operation.as_deref() == Some(status.operation_id.as_str()),
                            "OMP启动丢失或改变管理归属",
                        )?;
                    }
                }
            }
            let Some(op) = &i.management_operation else {
                continue;
            };
            require(
                linked.insert(op)
                    && !self
                        .startups
                        .all()
                        .iter()
                        .any(|r| r.intent.management_operation.as_ref() == Some(op)),
                "管理决定关联多个执行器启动",
            )?;
            let intent = self.management.intent(op)?;
            if i.schema_version == 2 {
                let original = self.validate_api_assignment_scope(
                    op,
                    store,
                    &i.task_id,
                    i.workflow_sequence,
                    false,
                )?;
                require(
                    original.sequence == i.workflow_sequence,
                    "OMP启动与原管理历史锚点不符",
                )?;
            }
            require(
                intent.project_id == i.project_id
                    && intent.purpose == Purpose::Assign
                    && Some(self.management.response_head(op)?) == i.management_hash.as_deref(),
                "OMP管理响应链与启动不符",
            )?;
            require(
                matches!(&self.management.original_envelope(op)?.decision,ManagementDecision::Assign{task_id,worker_id,..} if task_id==&i.task_id&&worker_id=="worker-omp"),
                "OMP管理决定与启动不符",
            )?;
            if let Some((target, _, _)) = self.management.application(op) {
                require(record.closure.is_none(), "OMP已准备应用的启动不能单独关闭")?;
                let ApplicationTarget::Workflow {
                    principal,
                    command,
                    context,
                } = target
                else {
                    return Err("OMP关联了非工作流应用".into());
                };
                require(principal == "deputy", "OMP应用角色不符")?;
                binding(i, command, context, fence, revision)?;
            } else if let Some(closed) = self.management.startup_closure(op) {
                let closure = record.closure.as_ref().ok_or("OMP管理关闭缺少启动关闭")?;
                require(
                    closed.attempt_id == i.attempt_id
                        && closed.startup_hash == record.head
                        && closed.reason == closure.reason,
                    "OMP管理关闭与原证据不符",
                )?;
            } else {
                require(
                    self.management.evidence_digest(op)?
                        == i.management_hash.as_deref().ok_or("OMP管理响应锚点缺失")?,
                    "OMP原管理链已变化",
                )?;
                self.management.ready_envelope(op, &intent.state_digest)?;
            }
        }
        Ok(())
    }
    pub(super) fn recover_omp_startups(&mut self) -> Result<(), String> {
        self.validate_omp_managed_links()?;
        for record in self.omp_startups.all() {
            if let (Some(closure), Some(operation)) =
                (&record.closure, &record.intent.management_operation)
            {
                if self.management.startup_closure(operation).is_none() {
                    self.management.resolve(
                        operation,
                        record
                            .intent
                            .management_hash
                            .as_deref()
                            .ok_or("OMP响应锚点缺失")?,
                        UserResolution::AbandonStartup {
                            reason: closure.reason.clone(),
                            attempt_id: record.intent.attempt_id.clone(),
                            startup_hash: record.head.clone(),
                        },
                    )?;
                }
                self.planning_errors.remove(operation);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "omp_management_bridge/tests.rs"]
pub(crate) mod tests;
