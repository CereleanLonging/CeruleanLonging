//! Deputy assignments stop at a persisted attempt. Sending needs a separate user review.
use super::*;
use crate::management_execution::ApplicationTarget;
use cerulean_domain::management::{ManagementDecision, Purpose};

impl RuntimeHost {
    pub(super) fn validate_desktop_run(&self, project: &str) -> Result<(), String> {
        // Active worker threads own their ledger; the host snapshot includes them.
        let snapshot = self
            .snapshot()
            .workflows
            .into_iter()
            .find(|w| w.project.id == project)
            .ok_or("项目不可读")?;
        let plan = snapshot.revision.as_ref().ok_or("方案缺失")?;
        let desktops: Vec<_> = plan
            .execution
            .iter()
            .filter(|e| {
                snapshot.state.tasks.get(&e.task_id).is_some_and(|t| {
                    matches!(
                        t.state,
                        TaskState::Pending
                            | TaskState::Assigned
                            | TaskState::Unknown
                            | TaskState::Cancelling
                    )
                })
            })
            .filter_map(|e| e.desktop_worker.as_ref())
            .collect();
        if desktops.is_empty() {
            return Ok(());
        }
        for d in desktops {
            crate::desktop_execution::validate_task_window(&d.binding.observation.window)?;
            require(
                d.binding.bridge_sha256 == desktop_sessions::fingerprint(),
                "桌面桥接版本变化，需要重新配置和批准",
            )?;
        }
        require(
            !snapshot
                .state
                .tasks
                .values()
                .any(|t| matches!(t.state, TaskState::Unknown | TaskState::Cancelling)),
            "原桌面尝试结果待核对，完成回读或处置后再恢复调度",
        )
    }
    pub(super) fn require_desktop_assignment_ready(&self, project: &str) -> Result<(), String> {
        self.validate_desktop_run(project)?;
        self.require_desktop_tasks_resolved()?;
        require(
            desktop_sessions::load_trials(&self.root)?
                .iter()
                .all(|t| t.proof.is_some()),
            "原固定消息验证仍需回读核对，暂不请求桌面分配",
        )?;
        let store = self.projects.get(project).ok_or("项目账本不可用")?;
        let plan = store.revision().ok_or("批准方案缺失")?;
        for selected in plan
            .execution
            .iter()
            .filter(|e| {
                store
                    .state()
                    .tasks
                    .get(&e.task_id)
                    .is_some_and(|t| t.state == TaskState::Pending)
            })
            .filter_map(|e| e.desktop_worker.as_ref())
        {
            crate::desktop_execution::predecessor(&self.root, store, &selected.binding.id)?;
        }
        let revision_digest = plan.approval_digest()?;
        let used = store.history().iter().filter(|e| matches!(&e.body.event.command,
            WorkflowCommand::DispatchIntent { task, attempt } if store.attempt_record(task, attempt).is_some_and(|a| a.revision_digest == revision_digest))).count();
        require(
            used < plan.policy.max_requests as usize,
            "桌面派发额度已用完，不再请求副指挥分配",
        )
    }
    pub(super) fn assign_managed_desktop(
        &mut self,
        operation: &str,
        attempt_id: &str,
    ) -> Result<(), String> {
        self.ensure_run_dispatch_authorized(operation)?;
        let intent = self.management.intent(operation)?;
        let project = intent.project_id.clone();
        require(
            intent.purpose == Purpose::Assign,
            "桌面分配必须来自副指挥的分配请求",
        )?;
        self.require_available(&project)?;
        self.require_desktop_assignment_ready(&project)?;
        let decision = self.management_envelope(operation)?;
        let ManagementDecision::Assign {
            task_id, worker_id, ..
        } = &decision.decision
        else {
            return Err("副指挥没有返回分配决定".into());
        };
        self.require_task_resources(&project, task_id, attempt_id)?;
        let store = &self.projects[&project];
        let revision = store.revision().ok_or("方案缺失")?;
        let d = revision
            .task(task_id)?
            .desktop_worker
            .as_ref()
            .ok_or("任务桌面配置缺失")?;
        self.validate_desktop_selection(&project, d)?;
        require(
            worker_id == &format!("worker-{}", d.adapter_id()),
            "副指挥选择与批准桌面执行器不符",
        )?;
        let state = store.state().tasks.get(task_id).ok_or("分配任务不存在")?;
        require(
            state.state == TaskState::Pending && state.attempt.is_none(),
            "原任务已分配或结束，禁止重新分配",
        )?;
        let attempt = Attempt {
            workspace_proof: None,
            id: attempt_id.into(),
            task_id: task_id.clone(),
            worker_id: worker_id.clone(),
            revision_digest: revision.approval_digest()?,
            fence: state.fence.checked_add(1).ok_or("尝试序号超限")?,
            binding: SessionBinding {
                adapter_id: d.adapter_id().into(),
                adapter_version: format!("desktop-ui-v1:{}", d.binding.bridge_sha256),
                session_id: d.session_id(),
                identity_strength: IdentityStrength::UiObservation,
                workspace: revision.project.root.clone(),
            },
        };
        self.apply_management_decision_inner(
            operation,
            ApplicationTarget::Workflow {
                principal: "deputy".into(),
                command: WorkflowCommand::Assign {
                    task: task_id.clone(),
                    worker: worker_id.clone(),
                    attempt: attempt_id.into(),
                },
                context: Box::new(CommandContext {
                    execution: Some(ExecutionEvidence::Attempt { attempt }),
                    ..Default::default()
                }),
            },
        )
    }
    pub(super) fn authorize_desktop_send(
        &self,
        project: &str,
        attempt: &str,
    ) -> Result<(), String> {
        let store = self.projects.get(project).ok_or("项目账本不可用")?;
        let event = store.history().iter().find(|e| matches!(&e.body.event.command, WorkflowCommand::Assign { attempt: id, .. } if id == attempt)).ok_or("桌面分配证据缺失")?;
        let operation = event
            .body
            .operation_id
            .strip_prefix("mgmt-")
            .ok_or("桌面尝试缺少管理分配关联")?;
        require(
            self.management
                .application(operation)
                .is_some_and(|(_, applied, hash)| applied && hash == Some(event.hash.as_str())),
            "副指挥分配事务尚未完成，请先恢复原记录",
        )?;
        self.ensure_run_dispatch_authorized(operation)
    }
}
