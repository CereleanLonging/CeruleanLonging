//! Cross-ledger review of durable Codex startup identities, before assignment.
use super::*;
use crate::management_execution::{ApplicationTarget, UserResolution};
use crate::startup_journal::{StartupClosure, StartupRecord};
use cerulean_domain::management::{ManagementDecision, Purpose};

fn startup_anchor(
    store: &DurableWorkflow,
    sequence: u32,
    hash: Option<&str>,
) -> Result<(), String> {
    let end = sequence as usize;
    require(end <= store.history().len(), "启动证据锚点超出工作流历史")?;
    require(
        end.checked_sub(1).map(|n| store.history()[n].hash.as_str()) == hash,
        "启动证据锚点与工作流历史不符",
    )
}

#[derive(serde::Serialize)]
pub(crate) struct OriginalBasis {
    pub project_digest: String,
    pub sequence: u32,
    pub workflow_hash: Option<String>,
    pub busy_tasks: bool,
    pub records: Vec<CodexStartupView>,
}

pub(crate) fn inspect_workflow_startup(
    store: &DurableWorkflow,
    records: &[StartupRecord],
    jobs: &[CodexJob],
    management: &super::startup_management_proof::StartupManagementProof,
) -> Result<OriginalBasis, String> {
    require(!store.requires_reopen(), "原项目账本需要重开恢复")?;
    let scope = store
        .configuration()
        .execution_scope
        .as_ref()
        .ok_or("项目执行范围缺失")?;
    require(
        scope.project.id == management.project_id,
        "项目管理证据范围不符",
    )?;
    let sequence = u32::try_from(store.history().len()).map_err(|_| "项目历史超出范围")?;
    let project_digest = crate::workspace_history::version(&(
        "workspace-startup-original-project-v1",
        &scope.project,
        store.configuration(),
        store.state(),
        store.revision(),
        store.approval(),
        store.history(),
    ))?;
    for job in jobs {
        require(
            job.project_id == scope.project.id
                && records
                    .iter()
                    .any(|r| r.intent.attempt_id == job.attempt_id),
            "编程记录缺少不可变启动来源",
        )?;
    }
    let mut views = Vec::with_capacity(records.len());
    for record in records {
        let intent = &record.intent;
        require(
            intent.project_id == scope.project.id
                && matches!(intent.schema_version, 1 | 2)
                && intent.protocol == "codex-durable-before-send-v1",
            "启动协议或项目来源不符",
        )?;
        startup_anchor(
            store,
            intent.workflow_sequence,
            intent.workflow_hash.as_deref(),
        )?;
        let prefix = &store.history()[..intent.workflow_sequence as usize];
        let events: Vec<_> = prefix.iter().map(|e| e.body.event.clone()).collect();
        let replay = Workflow::replay(store.configuration().principals.clone(), &events)?;
        let task = replay
            .state()
            .tasks
            .get(&intent.task_id)
            .ok_or("启动锚点任务缺失")?;
        require(
            replay.state().approved && task.state == TaskState::Pending && task.attempt.is_none(),
            "启动锚点不是批准的待分配任务",
        )?;
        let revision = prefix
            .iter()
            .rev()
            .find_map(|e| match &e.body.context.execution {
                Some(ExecutionEvidence::Revision { revision }) => Some(revision),
                _ => None,
            })
            .ok_or("启动锚点缺少原方案")?;
        let approval = prefix
            .iter()
            .rev()
            .find_map(|e| match &e.body.context.execution {
                Some(ExecutionEvidence::Approval { approval })
                    if approval.revision_digest == intent.revision_digest =>
                {
                    Some(approval)
                }
                _ => None,
            })
            .ok_or("启动锚点缺少原批准")?;
        let user = store
            .configuration()
            .principals
            .iter()
            .find(|p| p.role == Role::User)
            .ok_or("用户角色缺失")?;
        approval.validate(revision, &user.id, scope.policy_version)?;
        let spec = revision.task(&intent.task_id)?;
        require(
            revision.project == scope.project
                && revision.approval_digest()? == intent.revision_digest
                && spec.adapter_id == "codex"
                && spec.worker.as_ref() == Some(&intent.worker),
            "启动身份与原批准不符",
        )?;
        let definition = revision
            .workflow
            .tasks
            .iter()
            .find(|t| t.id == intent.task_id)
            .ok_or("启动任务定义缺失")?;
        require(
            definition.dependencies.iter().all(|d| {
                replay
                    .state()
                    .tasks
                    .get(d)
                    .is_some_and(|t| t.state == TaskState::Accepted)
            }),
            "启动锚点依赖未验收",
        )?;
        let fence = task.fence.checked_add(1).ok_or("fence overflow")?;
        let owner = intent.management_operation.as_ref().map_or_else(
            || format!("codex-{}-assign", intent.attempt_id),
            |op| format!("mgmt-{op}"),
        );
        let operation = intent
            .management_operation
            .as_ref()
            .map(|id| {
                management
                    .operations
                    .iter()
                    .find(|op| op.intent.operation_id == *id)
                    .ok_or("启动缺少原管理证据")
            })
            .transpose()?;
        for op in &management.operations {
            if let Some((
                ApplicationTarget::Workflow {
                    command: WorkflowCommand::Assign { attempt, .. },
                    ..
                },
                _,
                _,
            )) = &op.application
            {
                require(
                    attempt != &intent.attempt_id
                        || intent.management_operation.as_deref()
                            == Some(op.intent.operation_id.as_str()),
                    "启动预留与另一管理应用冲突",
                )?;
            }
        }
        if let Some(op) = operation {
            require(
                op.intent.project_id == intent.project_id
                    && op.intent.purpose == Purpose::Assign
                    && op.response_head.as_deref() == intent.management_hash.as_deref(),
                "启动意图与原管理响应链不符",
            )?;
            require(
                matches!(op.envelope().map(|o| &o.decision),
                Some(ManagementDecision::Assign { task_id, worker_id, .. })
                    if task_id == &intent.task_id && worker_id == "worker-codex"),
                "启动意图与原管理分配决定不符",
            )?;
            if op.application.is_none() && op.closure.is_none() {
                require(
                    op.ready
                        && Some(op.evidence_digest.as_str()) == intent.management_hash.as_deref(),
                    "原管理链已变化或被其他方式处置",
                )?;
            }
        }
        let validate_scope = |through| -> Result<(), String> {
            if intent.schema_version == 2 {
                let op = operation.ok_or("启动管理归属缺失")?;
                let original: ExecutionSnapshot =
                    serde_json::from_value(op.intent.context["workflow"].clone())
                        .map_err(|_| "原管理快照无效")?;
                require(
                    original.project.id == op.intent.project_id
                        && original.sequence == intent.workflow_sequence,
                    "启动与管理锚点不符",
                )?;
                #[cfg(windows)]
                super::api_assignment_scope::unchanged_target(
                    store,
                    &original,
                    &intent.task_id,
                    through,
                )?;
                #[cfg(not(windows))]
                return Err("原管理范围证明不可用".into());
            }
            Ok(())
        };
        validate_scope(intent.workflow_sequence)?;
        let mut advanced = false;
        for entry in store.history().iter().filter(|e| e.body.operation_id == owner
            || matches!(&e.body.event.command, WorkflowCommand::Assign { attempt, .. } if attempt == &intent.attempt_id)
            || matches!(&e.body.context.execution, Some(ExecutionEvidence::Attempt { attempt }) if attempt.id == intent.attempt_id)) {
            require(entry.body.operation_id == owner && entry.body.event.principal == "deputy" && entry.body.event.sequence > intent.workflow_sequence, "启动尝试被另一工作流操作占用")?;
            let Some(ExecutionEvidence::Attempt { attempt }) = &entry.body.context.execution else { return Err("启动分配缺少会话绑定".into()); };
            revision.validate_attempt_workspace(attempt)?;
            require(matches!(&entry.body.event.command, WorkflowCommand::Assign { task, worker, attempt: id } if task == &intent.task_id && worker == "worker-codex" && id == &intent.attempt_id)
                && attempt.task_id == intent.task_id && attempt.id == intent.attempt_id && attempt.worker_id == "worker-codex"
                && attempt.revision_digest == intent.revision_digest && attempt.fence == fence
                && attempt.binding.adapter_id == "codex" && attempt.binding.identity_strength == IdentityStrength::ProviderId, "启动与实际分配绑定不符")?;
            validate_scope(entry.body.event.sequence.checked_sub(1).ok_or("分配序号无效")?)?;
            advanced = true;
        }
        if let Some(job) = jobs.iter().find(|j| j.attempt_id == intent.attempt_id) {
            require(
                job.project_id == intent.project_id
                    && job.task_id == intent.task_id
                    && job.revision_digest == intent.revision_digest
                    && job.worker == intent.worker
                    && job.management_operation == intent.management_operation
                    && job.started_at == intent.started_at,
                "编程记录与启动身份不符",
            )?;
        }
        if let Some(op) = operation {
            if let Some((target, _, _)) = &op.application {
                let ApplicationTarget::Workflow {
                    command:
                        WorkflowCommand::Assign {
                            task,
                            worker,
                            attempt: id,
                        },
                    context,
                    ..
                } = target
                else {
                    return Err("启动关联非分配管理应用".into());
                };
                let Some(ExecutionEvidence::Attempt { attempt }) = &context.execution else {
                    return Err("管理应用缺少原会话绑定".into());
                };
                revision.validate_attempt_workspace(attempt)?;
                require(
                    task == &intent.task_id
                        && worker == "worker-codex"
                        && id == &intent.attempt_id
                        && attempt.revision_digest == intent.revision_digest
                        && attempt.fence == fence
                        && attempt.binding.adapter_id == "codex"
                        && attempt.binding.identity_strength == IdentityStrength::ProviderId,
                    "管理应用与启动身份不符",
                )?;
                advanced = true;
            } else if let Some(resolution) = &op.closure {
                let closure = record.closure.as_ref().ok_or("管理关闭缺少启动关闭证据")?;
                require(
                    resolution.attempt_id == intent.attempt_id
                        && resolution.startup_hash == record.head
                        && resolution.reason == closure.reason,
                    "管理与启动关闭引用不符",
                )?;
            }
        }
        if let Some(closure) = &record.closure {
            startup_anchor(
                store,
                closure.workflow_sequence,
                closure.workflow_hash.as_deref(),
            )?;
            require(
                !advanced
                    && !store.history().iter().any(|e| {
                        e.body.event.command.attempt() == Some(intent.attempt_id.as_str())
                    }),
                "已关闭启动出现执行历史",
            )?;
        }
        views.push(CodexStartupView {
            summary: CodexStartupSummary {
                project_id: intent.project_id.clone(),
                task_id: intent.task_id.clone(),
                attempt_id: intent.attempt_id.clone(),
                management_operation: intent.management_operation.clone(),
                state: if record.closure.is_some() {
                    "closed"
                } else if advanced {
                    "advanced"
                } else {
                    "prepared"
                }
                .into(),
                intent_hash: Some(record.intent_hash.clone()),
            },
            workspace: revision.execution_root(&intent.task_id, &intent.attempt_id)?,
            revision_digest: intent.revision_digest.clone(),
            worker_model: intent.worker.model.clone(),
            started_at: intent.started_at.clone(),
            protocol: Some(intent.protocol.clone()),
            evidence_digest: None,
            blocked_reason: Some(
                if record.closure.is_some() {
                    "启动记录已经关闭"
                } else if advanced {
                    "已进入应用或分配阶段，请使用相应恢复入口"
                } else {
                    "请使用原启动恢复入口审阅关闭证据"
                }
                .into(),
            ),
            reason: record.closure.as_ref().map(|c| c.reason.clone()),
            workflow_sequence: sequence,
        });
    }
    views.sort_by(|a, b| a.summary.attempt_id.cmp(&b.summary.attempt_id));
    Ok(OriginalBasis {
        project_digest,
        sequence,
        records: views,
        workflow_hash: store.history().last().map(|e| e.hash.clone()),
        busy_tasks: store.state().tasks.values().any(|t| {
            matches!(
                t.state,
                TaskState::Dispatching
                    | TaskState::Running
                    | TaskState::Cancelling
                    | TaskState::Unknown
            )
        }),
    })
}

impl RuntimeHost {
    fn codex_startup_workspace(
        &self,
        project: &str,
        task: &str,
        attempt: &str,
        revision_digest: &str,
    ) -> Result<String, String> {
        let revision = self.projects.get(project).and_then(|store| {
            store
                .history()
                .iter()
                .find_map(|entry| match &entry.body.context.execution {
                    Some(ExecutionEvidence::Revision { revision })
                        if revision
                            .approval_digest()
                            .is_ok_and(|d| d == revision_digest) =>
                    {
                        Some(revision)
                    }
                    _ => None,
                })
        });
        #[cfg(windows)]
        let revision = revision.or_else(|| {
            self.codex_active
                .get(&(project.into(), attempt.into()))
                .and_then(|active| active.snapshot.revision.as_ref())
                .filter(|revision| {
                    revision
                        .approval_digest()
                        .is_ok_and(|d| d == revision_digest)
                })
        });
        let revision = revision.ok_or("Codex启动目录对应的原方案缺失")?;
        revision.execution_root(task, attempt)
    }
    pub(super) fn validate_startup_reservations(&self) -> Result<(), String> {
        for (journal, r) in self
            .startups
            .closure_reviews
            .iter()
            .map(|r| (&self.startups, r))
            .chain(
                self.omp_startups
                    .closure_reviews
                    .iter()
                    .map(|r| (&self.omp_startups, r)),
            )
        {
            let record = journal
                .get(&r.project_id, &r.attempt_id)
                .ok_or("关闭复核缺少有效启动身份")?;
            require(
                record.intent_hash == r.intent_hash,
                "关闭复核与启动身份不符",
            )?;
            let store = self.projects.get(&r.project_id).ok_or("关闭复核项目缺失")?;
            for path in [
                store.omp_attempt_path(&r.attempt_id)?,
                store.omp_file_service_path(&r.attempt_id)?,
            ] {
                require(
                    matches!(fs::symlink_metadata(path),Err(e) if e.kind()==std::io::ErrorKind::NotFound),
                    "关闭复核仍有关联执行或工具证据",
                )?;
            }
            require(
                !self
                    .jobs
                    .values()
                    .any(|j| j.project_id == r.project_id && j.attempt_id == r.attempt_id),
                "关闭复核仍有关联验收记录",
            )?;
            require(!store.history().iter().any(|e|e.body.event.command.attempt()==Some(r.attempt_id.as_str())
                ||matches!(&e.body.context.execution,Some(ExecutionEvidence::Attempt{attempt}) if attempt.id==r.attempt_id)),"已有分配的尝试不能复核为未完成关闭")?;
            for s in self
                .management
                .statuses()
                .iter()
                .filter(|s| s.project_id == r.project_id)
            {
                require(!self.management.application(&s.operation_id).is_some_and(|(target,..)|matches!(target,ApplicationTarget::Workflow{command:WorkflowCommand::Assign{attempt,..},..} if attempt==&r.attempt_id)),"关闭复核与管理应用证据冲突")?;
            }
        }
        for r in self
            .startups
            .reservations
            .iter()
            .chain(&self.omp_startups.reservations)
        {
            let key = (r.project_id.clone(), r.attempt_id.clone());
            require(
                self.startups.get(&r.project_id, &r.attempt_id).is_none()
                    && self
                        .omp_startups
                        .get(&r.project_id, &r.attempt_id)
                        .is_none()
                    && !self.codex_jobs.contains_key(&key)
                    && !self.omp_jobs.contains_key(&key)
                    && !self
                        .jobs
                        .values()
                        .any(|j| j.project_id == r.project_id && j.attempt_id == r.attempt_id),
                "半写启动复核与既有启动、执行或验收证据冲突",
            )?;
            if let Some(store) = self.projects.get(&r.project_id) {
                require(!store.history().iter().any(|e|e.body.event.command.attempt()==Some(r.attempt_id.as_str())
                    || matches!(&e.body.context.execution,Some(ExecutionEvidence::Attempt{attempt}) if attempt.id==r.attempt_id)),
                    "已分配的尝试不能按半写启动隔离")?;
                for path in [
                    store.omp_attempt_path(&r.attempt_id)?,
                    store.omp_file_service_path(&r.attempt_id)?,
                ] {
                    require(
                        matches!(fs::symlink_metadata(path),Err(e) if e.kind()==std::io::ErrorKind::NotFound),
                        "半写启动仍有关联的OMP执行或工具证据",
                    )?;
                }
            }
            for s in self
                .management
                .statuses()
                .iter()
                .filter(|s| s.project_id == r.project_id)
            {
                require(!self.management.application(&s.operation_id).is_some_and(|(target,..)|
                    matches!(target,ApplicationTarget::Workflow{command:WorkflowCommand::Assign{attempt,..},..} if attempt==&r.attempt_id)),
                    "半写启动与管理分配事务冲突")?;
            }
        }
        Ok(())
    }

    fn startup_advanced(&self, record: &StartupRecord, store: &DurableWorkflow) -> bool {
        let intent = &record.intent;
        intent.management_operation.as_ref().is_some_and(|op| self.management.application(op).is_some())
            || store.history().iter().any(|e| {
                e.body.event.command.attempt() == Some(intent.attempt_id.as_str())
                    || matches!(&e.body.context.execution, Some(ExecutionEvidence::Attempt { attempt }) if attempt.id == intent.attempt_id)
            })
    }

    fn startup_summary(&self, record: &StartupRecord) -> CodexStartupSummary {
        let intent = &record.intent;
        CodexStartupSummary {
            project_id: intent.project_id.clone(),
            task_id: intent.task_id.clone(),
            attempt_id: intent.attempt_id.clone(),
            management_operation: intent.management_operation.clone(),
            state: if record.closure.is_some() {
                "closed"
            } else if intent
                .management_operation
                .as_ref()
                .is_some_and(|op| self.management.application(op).is_some())
                || self
                    .projects
                    .get(&intent.project_id)
                    .is_some_and(|store| self.startup_advanced(record, store))
            {
                "advanced"
            } else {
                "prepared"
            }
            .into(),
            intent_hash: Some(record.intent_hash.clone()),
        }
    }

    pub(super) fn startup_summaries(&self) -> Vec<CodexStartupSummary> {
        let mut summaries: Vec<_> = self
            .startups
            .all()
            .into_iter()
            .map(|r| self.startup_summary(r))
            .collect();
        for job in self.codex_jobs.values() {
            if self
                .startups
                .get(&job.project_id, &job.attempt_id)
                .is_none()
            {
                summaries.push(CodexStartupSummary {
                    project_id: job.project_id.clone(),
                    task_id: job.task_id.clone(),
                    attempt_id: job.attempt_id.clone(),
                    management_operation: job.management_operation.clone(),
                    state: "legacy".into(),
                    intent_hash: None,
                });
            }
        }
        summaries
            .sort_by(|a, b| (&a.project_id, &a.attempt_id).cmp(&(&b.project_id, &b.attempt_id)));
        summaries
    }

    // Permanent validation uses the original approved prefix, never today's task/revision.
    fn inspect_startup(&self, record: &StartupRecord) -> Result<(), String> {
        let store = self
            .projects
            .get(&record.intent.project_id)
            .ok_or("项目账本不可用或仍被执行持有")?;
        self.inspect_startup_with_store(record, store)
    }

    pub(super) fn inspect_startup_with_store(
        &self,
        record: &StartupRecord,
        store: &DurableWorkflow,
    ) -> Result<(), String> {
        require(
            self.startups.healthy() && self.omp_startups.healthy() && self.management.healthy(),
            "启动或管理账本需要重开恢复",
        )?;
        let intent = &record.intent;
        require(
            matches!(intent.schema_version, 1 | 2)
                && intent.protocol == "codex-durable-before-send-v1",
            "启动协议来源不受支持",
        )?;
        require(!store.requires_reopen(), "项目账本需要重开恢复")?;
        require(
            store
                .configuration()
                .execution_scope
                .as_ref()
                .is_some_and(|s| s.project.id == intent.project_id),
            "项目账本与启动项目身份不符",
        )?;
        startup_anchor(
            store,
            intent.workflow_sequence,
            intent.workflow_hash.as_deref(),
        )?;
        let prefix = &store.history()[..intent.workflow_sequence as usize];
        let events: Vec<_> = prefix.iter().map(|e| e.body.event.clone()).collect();
        let replay = Workflow::replay(store.configuration().principals.clone(), &events)?;
        let task = replay
            .state()
            .tasks
            .get(&intent.task_id)
            .ok_or("启动锚点任务不存在")?;
        require(
            replay.state().approved && task.state == TaskState::Pending && task.attempt.is_none(),
            "启动锚点不是批准的待分配任务",
        )?;
        let revision = prefix
            .iter()
            .rev()
            .find_map(|e| match &e.body.context.execution {
                Some(ExecutionEvidence::Revision { revision }) => Some(revision),
                _ => None,
            })
            .ok_or("启动锚点缺少执行方案")?;
        let approval = prefix
            .iter()
            .rev()
            .find_map(|e| match &e.body.context.execution {
                Some(ExecutionEvidence::Approval { approval })
                    if approval.revision_digest == intent.revision_digest =>
                {
                    Some(approval)
                }
                _ => None,
            })
            .ok_or("启动锚点缺少方案批准")?;
        let scope = store
            .configuration()
            .execution_scope
            .as_ref()
            .ok_or("项目执行范围缺失")?;
        let user = store
            .configuration()
            .principals
            .iter()
            .find(|p| p.role == Role::User)
            .ok_or("用户角色缺失")?;
        approval.validate(revision, &user.id, scope.policy_version)?;
        let spec = revision.task(&intent.task_id)?;
        require(
            revision.project == scope.project
                && revision.project.id == intent.project_id
                && revision.approval_digest()? == intent.revision_digest
                && spec.adapter_id == "codex"
                && spec.worker.as_ref() == Some(&intent.worker),
            "启动身份与历史批准不符",
        )?;
        let definition = revision
            .workflow
            .tasks
            .iter()
            .find(|t| t.id == intent.task_id)
            .ok_or("启动任务定义缺失")?;
        require(
            definition.dependencies.iter().all(|id| {
                replay
                    .state()
                    .tasks
                    .get(id)
                    .is_some_and(|t| t.state == TaskState::Accepted)
            }),
            "启动锚点依赖尚未验收",
        )?;
        let expected_fence = task.fence.checked_add(1).ok_or("fence overflow")?;
        let owner = intent.management_operation.as_ref().map_or_else(
            || format!("codex-{}-assign", intent.attempt_id),
            |op| format!("mgmt-{op}"),
        );
        for entry in store.history().iter().filter(|e| {
            e.body.operation_id == owner
                || matches!(&e.body.event.command, WorkflowCommand::Assign { attempt, .. } if attempt == &intent.attempt_id)
                || matches!(&e.body.context.execution, Some(ExecutionEvidence::Attempt { attempt }) if attempt.id == intent.attempt_id)
        }) {
            require(entry.body.operation_id == owner && entry.body.event.principal == "deputy"
                && entry.body.event.sequence > intent.workflow_sequence, "启动尝试被另一工作流操作或先前历史占用")?;
            let Some(ExecutionEvidence::Attempt { attempt }) = &entry.body.context.execution else { return Err("启动分配缺少会话绑定".into()); };
            revision.validate_attempt_workspace(attempt)?;
            require(matches!(&entry.body.event.command, WorkflowCommand::Assign { task, worker, attempt: id }
                if task == &intent.task_id && worker == "worker-codex" && id == &intent.attempt_id)
                && attempt.id == intent.attempt_id && attempt.task_id == intent.task_id
                && attempt.worker_id == "worker-codex" && attempt.revision_digest == intent.revision_digest
                && attempt.fence == expected_fence && attempt.binding.adapter_id == "codex"
                && attempt.binding.identity_strength == IdentityStrength::ProviderId,
                "启动意图与实际分配绑定不符")?;
            if intent.schema_version==2 {
                self.validate_api_assignment_scope(intent.management_operation.as_deref().ok_or("Codex管理归属缺失")?,store,&intent.task_id,
                    entry.body.event.sequence.checked_sub(1).ok_or("Codex分配序号无效")?,false)?;
            }
        }
        if let Some(job) = self
            .codex_jobs
            .get(&(intent.project_id.clone(), intent.attempt_id.clone()))
        {
            require(
                job.project_id == intent.project_id
                    && job.task_id == intent.task_id
                    && job.attempt_id == intent.attempt_id
                    && job.revision_digest == intent.revision_digest
                    && job.worker == intent.worker
                    && job.management_operation == intent.management_operation
                    && job.started_at == intent.started_at,
                "编程记录与不可变启动身份不符",
            )?;
        }
        // Even without a mutable job, a managed application must retain this startup owner.
        for status in self
            .management
            .statuses()
            .into_iter()
            .filter(|s| s.project_id == intent.project_id)
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
                if attempt == &intent.attempt_id {
                    require(
                        intent.management_operation.as_deref()
                            == Some(status.operation_id.as_str()),
                        "启动预留与另一管理应用冲突",
                    )?;
                }
            }
        }
        if let Some(operation) = &intent.management_operation {
            let management = self.management.intent(operation)?;
            if intent.schema_version == 2 {
                let original = self.validate_api_assignment_scope(
                    operation,
                    store,
                    &intent.task_id,
                    intent.workflow_sequence,
                    false,
                )?;
                require(
                    original.sequence == intent.workflow_sequence,
                    "Codex启动与原管理锚点不符",
                )?;
            }
            require(
                management.project_id == intent.project_id
                    && management.purpose == Purpose::Assign
                    && Some(self.management.response_head(operation)?)
                        == intent.management_hash.as_deref(),
                "启动意图与原管理响应链不符",
            )?;
            require(
                matches!(&self.management.original_envelope(operation)?.decision,
                ManagementDecision::Assign { task_id, worker_id, .. } if task_id == &intent.task_id && worker_id == "worker-codex"),
                "启动意图与原管理分配决定不符",
            )?;
            require(
                self.codex_jobs
                    .values()
                    .filter(|j| j.management_operation.as_deref() == Some(operation))
                    .all(|j| {
                        j.project_id == intent.project_id
                            && j.task_id == intent.task_id
                            && j.attempt_id == intent.attempt_id
                    }),
                "管理请求关联多个或错误启动记录",
            )?;
            if let Some((target, _, _)) = self.management.application(operation) {
                let ApplicationTarget::Workflow {
                    command:
                        WorkflowCommand::Assign {
                            task,
                            worker,
                            attempt: id,
                        },
                    context,
                    ..
                } = target
                else {
                    return Err("启动请求关联了非分配应用".into());
                };
                let Some(ExecutionEvidence::Attempt { attempt }) = &context.execution else {
                    return Err("管理应用缺少原始会话绑定".into());
                };
                revision.validate_attempt_workspace(attempt)?;
                require(
                    task == &intent.task_id
                        && worker == "worker-codex"
                        && id == &intent.attempt_id
                        && attempt.revision_digest == intent.revision_digest
                        && attempt.fence == expected_fence
                        && attempt.binding.adapter_id == "codex"
                        && attempt.binding.identity_strength == IdentityStrength::ProviderId,
                    "管理应用与启动身份不符",
                )?;
            } else if let Some(resolution) = self.management.startup_closure(operation) {
                let closure = record
                    .closure
                    .as_ref()
                    .ok_or("管理启动关闭缺少权威启动关闭证据")?;
                require(
                    resolution.attempt_id == intent.attempt_id
                        && resolution.startup_hash == record.head
                        && resolution.reason == closure.reason,
                    "管理关闭与启动关闭引用冲突",
                )?;
            } else {
                require(
                    self.management.evidence_digest(operation)?
                        == intent.management_hash.as_deref().unwrap(),
                    "原管理链已变化或被其他方式处置",
                )?;
                self.management
                    .ready_envelope(operation, &management.state_digest)?;
            }
        }
        if let Some(closure) = &record.closure {
            startup_anchor(
                store,
                closure.workflow_sequence,
                closure.workflow_hash.as_deref(),
            )?;
            require(
                !self.startup_advanced(record, store)
                    && !store.history().iter().any(|e| e.body.operation_id == owner),
                "已关闭启动身份后来出现应用或执行历史",
            )?;
        }
        Ok(())
    }

    fn startup_review_digest(&self, record: &StartupRecord) -> Result<String, String> {
        self.inspect_startup(record)?;
        require(record.closure.is_none(), "启动记录已经关闭")?;
        require(
            !self.startup_advanced(
                record,
                self.projects
                    .get(&record.intent.project_id)
                    .ok_or("项目账本不可用")?,
            ),
            "已进入应用或分配阶段，请使用相应恢复入口",
        )?;
        let intent = &record.intent;
        let project = &intent.project_id;
        #[cfg(windows)]
        require(
            !self.has_codex_activity(project) && !self.active.contains_key(project),
            "执行或验收线程仍持有项目",
        )?;
        require(
            self.runs.healthy() && self.runner_error.is_none(),
            "项目运行控制需要重开恢复",
        )?;
        let runs: Vec<_> = self
            .runs
            .all()
            .into_iter()
            .filter(|r| &r.project_id == project)
            .collect();
        require(
            !runs
                .iter()
                .any(|r| r.state == project_setup::RunState::Active),
            "请先暂停项目调度",
        )?;
        require(
            !self.management_active.keys().any(|op| {
                self.management
                    .intent(op)
                    .is_ok_and(|i| &i.project_id == project)
            }),
            "项目指挥请求仍在运行",
        )?;
        let statuses = self.management.statuses();
        require(
            !statuses.iter().any(|s| {
                &s.project_id == project
                    && Some(s.operation_id.as_str()) != intent.management_operation.as_deref()
                    && !s.state.resolved()
            }),
            "项目存在另一待处置管理请求",
        )?;
        let store = self.projects.get(project).ok_or("项目账本不可用")?;
        require(
            !store.state().tasks.values().any(|t| {
                matches!(
                    t.state,
                    TaskState::Dispatching
                        | TaskState::Running
                        | TaskState::Cancelling
                        | TaskState::Unknown
                )
            }),
            "项目存在执行中或待对账任务",
        )?;
        require(
            !self.jobs.values().any(|j| {
                &j.project_id == project
                    && matches!(
                        j.state,
                        VerificationState::Running
                            | VerificationState::Cancelling
                            | VerificationState::Unknown
                    )
            }),
            "项目存在待对账验收",
        )?;
        let job = self
            .codex_jobs
            .get(&(project.clone(), intent.attempt_id.clone()));
        require(
            !self.codex_jobs.values().any(|j| {
                &j.project_id == project
                    && j.attempt_id != intent.attempt_id
                    && matches!(
                        j.state,
                        CodexJobState::Starting
                            | CodexJobState::Executing
                            | CodexJobState::Reconciling
                            | CodexJobState::Unknown
                    )
            }),
            "项目存在另一待对账编程记录",
        )?;
        if let Some(job) = job {
            require(
                matches!(
                    job.state,
                    CodexJobState::Unknown | CodexJobState::Failed | CodexJobState::Starting
                ),
                "编程投影与未分配启动不符",
            )?;
        }
        let heads = statuses
            .iter()
            .filter(|s| &s.project_id == project)
            .map(|s| {
                self.management
                    .evidence_digest(&s.operation_id)
                    .map(|h| (s.operation_id.as_str(), h))
            })
            .collect::<Result<Vec<_>, _>>()?;
        digest(
            &serde_json::json!({"purpose":"close-codex-startup-v1", "intent":intent, "startupHead":record.head,
            "workflowSequence":store.history().len(), "workflowHash":store.history().last().map(|e| &e.hash),
            "management":heads, "job":job, "runs":runs}),
        )
    }

    pub(super) fn startup_detail(
        &self,
        project: &str,
        attempt: &str,
    ) -> Result<CodexStartupView, String> {
        identifier(project)?;
        identifier(attempt)?;
        let store = self.projects.get(project);
        let sequence = store.map_or(0, |s| s.history().len() as u32);
        if let Some(record) = self.startups.get(project, attempt) {
            let intent = &record.intent;
            let review = self.startup_review_digest(record);
            return Ok(CodexStartupView {
                summary: self.startup_summary(record),
                workspace: self.codex_startup_workspace(
                    project,
                    &intent.task_id,
                    attempt,
                    &intent.revision_digest,
                )?,
                revision_digest: intent.revision_digest.clone(),
                worker_model: intent.worker.model.clone(),
                started_at: intent.started_at.clone(),
                protocol: Some(intent.protocol.clone()),
                evidence_digest: review.as_ref().ok().cloned(),
                blocked_reason: review.err(),
                reason: record.closure.as_ref().map(|c| c.reason.clone()),
                workflow_sequence: sequence,
            });
        }
        let job = self
            .codex_jobs
            .get(&(project.into(), attempt.into()))
            .ok_or("启动记录不存在")?;
        Ok(CodexStartupView {
            summary: CodexStartupSummary {
                project_id: project.into(),
                task_id: job.task_id.clone(),
                attempt_id: attempt.into(),
                management_operation: job.management_operation.clone(),
                state: "legacy".into(),
                intent_hash: None,
            },
            workspace: self.codex_startup_workspace(
                project,
                &job.task_id,
                attempt,
                &job.revision_digest,
            )?,
            revision_digest: job.revision_digest.clone(),
            worker_model: job.worker.model.clone(),
            started_at: job.started_at.clone(),
            protocol: None,
            evidence_digest: None,
            blocked_reason: Some(
                "旧编程记录缺少不可变启动来源及发送顺序证明，不能据此关闭或推断未派发".into(),
            ),
            reason: None,
            workflow_sequence: sequence,
        })
    }

    pub(super) fn abandon_codex_startup(
        &mut self,
        project: &str,
        attempt: &str,
        expected: &str,
        reason: &str,
    ) -> Result<(), String> {
        sha256(expected)?;
        require(
            !reason.trim().is_empty() && reason.len() <= 8000 && !reason.contains('\0'),
            "请输入不超过8000字节的关闭原因",
        )?;
        let record = self
            .startups
            .get(project, attempt)
            .ok_or("缺少受支持的不可变启动意图")?;
        require(
            self.startup_review_digest(record)? == expected,
            "启动证据已变化，请刷新后重新审阅",
        )?;
        let head = record.head.clone();
        let store = &self.projects[project];
        let closure = StartupClosure {
            reason: reason.into(),
            workflow_sequence: store.history().len() as u32,
            workflow_hash: store.history().last().map(|e| e.hash.clone()),
        };
        self.startups.close(project, attempt, &head, closure)?;
        self.repair_startup_projection(project, attempt)
    }

    pub(super) fn validate_startups(&self) -> Result<(), String> {
        require(
            self.startups.healthy() && self.omp_startups.healthy() && self.management.healthy(),
            "启动或管理账本需要重开恢复",
        )?;
        // Finish all cross-ledger checks before writing either management or job projections.
        for record in self.startups.all() {
            self.inspect_startup(record)?;
        }
        for status in self.management.statuses() {
            if let Some(resolution) = self.management.startup_closure(&status.operation_id) {
                #[cfg(windows)]
                if let Some(startup) = self
                    .api_startups
                    .get(&(status.project_id.clone(), resolution.attempt_id.clone()))
                {
                    require(
                        startup.management_operation == status.operation_id,
                        "API管理启动关闭归属不符",
                    )?;
                    self.validate_api_startup_closure(startup)?;
                    continue;
                }
                let record = self
                    .startups
                    .get(&status.project_id, &resolution.attempt_id)
                    .or_else(|| {
                        self.omp_startups
                            .get(&status.project_id, &resolution.attempt_id)
                    })
                    .ok_or("孤立管理启动关闭记录")?;
                let closure = record
                    .closure
                    .as_ref()
                    .ok_or("管理关闭没有权威启动关闭记录")?;
                require(
                    record.intent.management_operation.as_deref()
                        == Some(status.operation_id.as_str())
                        && resolution.startup_hash == record.head
                        && resolution.reason == closure.reason,
                    "管理启动关闭双向引用冲突",
                )?;
            }
        }
        Ok(())
    }

    pub(super) fn recover_startups(&mut self) -> Result<(), String> {
        self.validate_startups()?;
        let closed: Vec<_> = self
            .startups
            .all()
            .into_iter()
            .filter(|r| r.closure.is_some())
            .map(|r| (r.intent.project_id.clone(), r.intent.attempt_id.clone()))
            .collect();
        for (project, attempt) in closed {
            self.repair_startup_projection(&project, &attempt)?;
        }
        Ok(())
    }

    fn repair_startup_projection(&mut self, project: &str, attempt: &str) -> Result<(), String> {
        let record = self.startups.get(project, attempt).ok_or("启动记录缺失")?;
        self.inspect_startup(record)?;
        let intent = record.intent.clone();
        let closure = record.closure.clone().ok_or("启动关闭证据缺失")?;
        let head = record.head.clone();
        if let Some(operation) = &intent.management_operation {
            if self.management.startup_closure(operation).is_none() {
                self.management.resolve(
                    operation,
                    intent
                        .management_hash
                        .as_deref()
                        .ok_or("管理原响应锚点缺失")?,
                    UserResolution::AbandonStartup {
                        reason: closure.reason,
                        attempt_id: intent.attempt_id.clone(),
                        startup_hash: head,
                    },
                )?;
            }
            self.planning_errors.remove(operation);
        }
        if let Some(job) = self
            .codex_jobs
            .get_mut(&(intent.project_id, intent.attempt_id))
        {
            job.state = CodexJobState::Abandoned;
            job.message = Some(
                "用户已关闭尚未分配的受控启动记录；原证据保留，会话建立情况可能需要单独核对".into(),
            );
            #[cfg(windows)]
            crate::async_codex::save(&self.root, job)?;
        }
        Ok(())
    }
}

#[cfg(all(test, windows))]
pub(super) mod tests {
    use super::*;
    use crate::management_execution::{DecisionIntent, DecisionState};
    use crate::startup_journal::StartupIntent;
    use cerulean_adapters::openai::DecisionObservation;
    use cerulean_domain::management::DecisionEnvelope;

    pub(crate) fn fixture(root: &Path, managed: bool, with_job: bool) -> RuntimeHost {
        let (mut host, mut draft) = super::super::project_planning::tests::fixture(root);
        draft.management_max_requests = 16;
        host.configure_project(draft).unwrap();
        super::super::project_planning::tests::complete(
            &mut host,
            super::super::project_planning::tests::proposal("tests"),
        );
        let revision = host.projects["project"]
            .revision()
            .unwrap()
            .approval_digest()
            .unwrap();
        host.approve("project", &revision).unwrap();
        let operation = managed.then(|| "startup-deputy-1".to_owned());
        let management_hash = operation.as_ref().map(|op| {
            let context = host
                .management_context("project", "Implement a sum function")
                .unwrap();
            let state_digest = digest(&context).unwrap();
            host.management
                .begin(DecisionIntent {
                    operation_id: op.clone(),
                    project_id: "project".into(),
                    purpose: Purpose::Assign,
                    model: "fixture-deputy".into(),
                    state_digest: state_digest.clone(),
                    context,
                    created_at: chrono::Utc::now().to_rfc3339(),
                })
                .unwrap();
            host.management
                .finish(
                    op,
                    Ok(DecisionObservation {
                        response_id: "resp_startup_fixture".into(),
                        requested_model: "fixture-deputy".into(),
                        actual_model: "fixture-deputy".into(),
                        input_tokens: 0,
                        output_tokens: 0,
                        envelope: DecisionEnvelope {
                            state_digest,
                            decision: ManagementDecision::Assign {
                                task_id: "task".into(),
                                worker_id: "worker-codex".into(),
                                rationale: "Synthetic controlled startup".into(),
                            },
                        },
                    }),
                )
                .unwrap();
            host.management.response_head(op).unwrap().to_owned()
        });
        let store = &host.projects["project"];
        let worker = store
            .revision()
            .unwrap()
            .task("task")
            .unwrap()
            .worker
            .clone()
            .unwrap();
        let started_at = chrono::Utc::now().to_rfc3339();
        host.startups
            .begin(StartupIntent {
                schema_version: 1,
                protocol: "codex-durable-before-send-v1".into(),
                project_id: "project".into(),
                task_id: "task".into(),
                attempt_id: "startup-native".into(),
                revision_digest: revision.clone(),
                worker: worker.clone(),
                management_operation: operation.clone(),
                management_hash,
                workflow_sequence: store.history().len() as u32,
                workflow_hash: store.history().last().map(|e| e.hash.clone()),
                started_at: started_at.clone(),
            })
            .unwrap();
        if with_job {
            let job = CodexJob {
                management_operation: operation,
                project_id: "project".into(),
                task_id: "task".into(),
                attempt_id: "startup-native".into(),
                revision_digest: revision,
                worker,
                state: CodexJobState::Unknown,
                started_at: started_at.clone(),
                updated_at: started_at,
                message: Some(
                    "Synthetic interruption before Prepare; session creation may be uncertain"
                        .into(),
                ),
            };
            crate::async_codex::save(&host.root, &job).unwrap();
            host.codex_jobs
                .insert(("project".into(), "startup-native".into()), job);
        }
        host
    }

    fn close(host: &mut RuntimeHost) {
        let view = host.startup_detail("project", "startup-native").unwrap();
        host.abandon_codex_startup(
            "project",
            "startup-native",
            view.evidence_digest.as_deref().unwrap(),
            "用户确认关闭启动记录，保留会话不确定事实",
        )
        .unwrap();
    }

    fn assignment(host: &RuntimeHost, id: &str) -> ApplicationTarget {
        let store = &host.projects["project"];
        let revision = store.revision().unwrap();
        ApplicationTarget::Workflow {
            principal: "deputy".into(),
            command: WorkflowCommand::Assign {
                task: "task".into(),
                worker: "worker-codex".into(),
                attempt: id.into(),
            },
            context: Box::new(CommandContext {
                execution: Some(ExecutionEvidence::Attempt {
                    attempt: Attempt {
                        workspace_proof: None,
                        id: id.into(),
                        task_id: "task".into(),
                        worker_id: "worker-codex".into(),
                        revision_digest: revision.approval_digest().unwrap(),
                        fence: store.state().tasks["task"].fence + 1,
                        binding: SessionBinding {
                            adapter_id: "codex".into(),
                            adapter_version: "fixture".into(),
                            session_id: "synthetic-startup-session".into(),
                            identity_strength: IdentityStrength::ProviderId,
                            workspace: revision.project.root.clone(),
                        },
                    },
                }),
                ..Default::default()
            }),
        }
    }

    #[test]
    fn managed_unmanaged_and_missing_job_close_without_workflow_or_budget_changes() {
        for (managed, with_job) in [(true, true), (false, true), (true, false)] {
            let root = tempfile::tempdir().unwrap();
            let mut host = fixture(root.path(), managed, with_job);
            let startup = host
                .startups
                .get("project", "startup-native")
                .unwrap()
                .clone();
            let events = host.projects["project"].history().to_vec();
            let count = host.management.statuses().len();
            let view = host.startup_detail("project", "startup-native").unwrap();
            assert_eq!(view.summary.state, "prepared");
            assert!(host
                .abandon_codex_startup("project", "startup-native", &"a".repeat(64), "stale")
                .is_err());
            close(&mut host);
            assert_eq!(host.projects["project"].history(), events);
            assert_eq!(host.projects["project"].state().tasks["task"].fence, 0);
            assert_eq!(host.codex_jobs.len(), usize::from(with_job));
            assert!(host
                .codex_jobs
                .values()
                .all(|j| j.state == CodexJobState::Abandoned));
            assert_eq!(host.management.statuses().len(), count);
            assert_eq!(
                host.startups
                    .get("project", "startup-native")
                    .unwrap()
                    .intent_hash,
                startup.intent_hash
            );
            assert!(host
                .startups
                .check_reservation(
                    "project",
                    "startup-native",
                    startup.intent.management_operation.as_deref()
                )
                .is_err());
            assert!(host
                .abandon_codex_startup(
                    "project",
                    "startup-native",
                    view.evidence_digest.as_deref().unwrap(),
                    "repeat"
                )
                .is_err());
            if managed {
                assert_eq!(
                    host.management
                        .startup_closure("startup-deputy-1")
                        .unwrap()
                        .startup_hash,
                    host.startups.get("project", "startup-native").unwrap().head
                );
                assert!(host.management.application("startup-deputy-1").is_none());
                assert_eq!(
                    host.management
                        .statuses()
                        .into_iter()
                        .find(|s| s.operation_id == "startup-deputy-1")
                        .unwrap()
                        .state,
                    DecisionState::StartupClosed
                );
            }
            drop(host);
            let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
            host.poll_verifications();
            assert_eq!(
                host.startup_detail("project", "startup-native")
                    .unwrap()
                    .summary
                    .state,
                "closed"
            );
            assert_eq!(host.management.statuses().len(), count);
            assert_eq!(host.projects["project"].history(), events);
            assert!(host.codex_active.is_empty() && host.management_active.is_empty());
            assert!(host
                .codex_jobs
                .values()
                .all(|j| j.state == CodexJobState::Abandoned));
        }
    }

    #[test]
    fn startup_closed_before_management_or_job_write_is_repaired_on_open() {
        let root = tempfile::tempdir().unwrap();
        let mut host = fixture(root.path(), true, true);
        let record = host
            .startups
            .get("project", "startup-native")
            .unwrap()
            .clone();
        host.startups
            .close(
                "project",
                "startup-native",
                &record.head,
                StartupClosure {
                    reason: "Crash after startup closure before projections".into(),
                    workflow_sequence: 2,
                    workflow_hash: record.intent.workflow_hash,
                },
            )
            .unwrap();
        host.validate_startups().unwrap();
        assert!(host
            .management
            .startup_closure("startup-deputy-1")
            .is_none());
        assert_eq!(
            host.codex_jobs.values().next().unwrap().state,
            CodexJobState::Unknown
        );
        drop(host);
        let host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert!(host
            .management
            .startup_closure("startup-deputy-1")
            .is_some());
        assert_eq!(
            host.codex_jobs.values().next().unwrap().state,
            CodexJobState::Abandoned
        );
        assert!(host.management.application("startup-deputy-1").is_none());
        assert_eq!(host.projects["project"].history().len(), 2);
    }

    #[test]
    fn broken_unrelated_management_link_prevents_startup_projection_on_open() {
        let root = tempfile::tempdir().unwrap();
        let mut host = fixture(root.path(), true, true);
        let record = host
            .startups
            .get("project", "startup-native")
            .unwrap()
            .clone();
        host.startups
            .close(
                "project",
                "startup-native",
                &record.head,
                StartupClosure {
                    reason: "Valid closure awaiting management projection".into(),
                    workflow_sequence: 2,
                    workflow_hash: record.intent.workflow_hash,
                },
            )
            .unwrap();
        let mut unrelated = host.codex_jobs.values().next().unwrap().clone();
        unrelated.attempt_id = "unrelated-broken-link".into();
        unrelated.management_operation = Some("missing-management-request".into());
        crate::async_codex::save(&host.root, &unrelated).unwrap();
        host.codex_jobs.insert(
            (unrelated.project_id.clone(), unrelated.attempt_id.clone()),
            unrelated,
        );
        // The startup chain is valid; only the independent managed-job link is broken.
        host.validate_startups().unwrap();
        assert!(host.validate_managed_codex_links().is_err());
        let management_closure = host
            .root
            .join("management-decisions/startup-deputy-1.4.json");
        assert!(!management_closure.exists());
        drop(host);
        assert!(RuntimeHost::open(&root.path().join("data")).is_err());
        assert!(
            !management_closure.exists(),
            "cross-ledger validation must finish before startup recovery writes .4"
        );
        let management =
            crate::management_execution::DecisionJournal::open(&root.path().join("data")).unwrap();
        assert!(management.startup_closure("startup-deputy-1").is_none());
        assert!(management.application("startup-deputy-1").is_none());
    }

    #[test]
    fn startup_application_closure_survives_a_fresh_startup_and_reopen() {
        let root = tempfile::tempdir().unwrap();
        let mut host = fixture(root.path(), true, true);
        let target = assignment(&host, "startup-native");
        let state = host
            .management
            .intent("startup-deputy-1")
            .unwrap()
            .state_digest
            .clone();
        host.management
            .prepare_application("startup-deputy-1", target, &state)
            .unwrap();
        let review = host
            .application_recovery_review("startup-deputy-1")
            .unwrap();
        host.abandon_management_application(
            "startup-deputy-1",
            &review.evidence_digest,
            "Close the prepared application, preserve startup identity",
        )
        .unwrap();
        assert!(host
            .startups
            .get("project", "startup-native")
            .unwrap()
            .closure
            .is_none());
        assert_eq!(
            host.startup_detail("project", "startup-native")
                .unwrap()
                .summary
                .state,
            "advanced"
        );
        let mut fresh = host
            .startups
            .get("project", "startup-native")
            .unwrap()
            .intent
            .clone();
        fresh.attempt_id = "fresh-startup".into();
        fresh.management_operation = None;
        fresh.management_hash = None;
        fresh.started_at = chrono::Utc::now().to_rfc3339();
        let mut job = host.codex_jobs.values().next().unwrap().clone();
        job.attempt_id = fresh.attempt_id.clone();
        job.management_operation = None;
        job.state = CodexJobState::Starting;
        job.started_at = fresh.started_at.clone();
        job.updated_at = fresh.started_at.clone();
        job.message = None;
        host.startups.begin(fresh).unwrap();
        crate::async_codex::save(&host.root, &job).unwrap();
        host.codex_jobs
            .insert((job.project_id.clone(), job.attempt_id.clone()), job);
        let ApplicationTarget::Workflow {
            principal,
            command,
            context,
        } = assignment(&host, "fresh-startup")
        else {
            unreachable!()
        };
        host.projects
            .get_mut("project")
            .unwrap()
            .command(
                "codex-fresh-startup-assign",
                &principal,
                command,
                *context,
                false,
            )
            .unwrap();
        let history = host.projects["project"].history().to_vec();
        drop(host);
        let host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(host.projects["project"].history(), history);
        assert_eq!(
            host.projects["project"].state().tasks["task"].state,
            TaskState::Assigned
        );
        assert_eq!(host.projects["project"].state().tasks["task"].fence, 1);
        assert_eq!(
            host.projects["project"].state().tasks["task"]
                .attempt
                .as_deref(),
            Some("fresh-startup")
        );
        assert_eq!(
            host.codex_jobs[&("project".into(), "startup-native".into())].state,
            CodexJobState::Abandoned
        );
        assert_eq!(
            host.codex_jobs[&("project".into(), "fresh-startup".into())].state,
            CodexJobState::Unknown
        );
        assert_eq!(
            host.startup_detail("project", "startup-native")
                .unwrap()
                .summary
                .state,
            "advanced"
        );
        assert_eq!(
            host.startup_detail("project", "fresh-startup")
                .unwrap()
                .summary
                .state,
            "advanced"
        );
        assert!(host
            .management
            .application_closure("startup-deputy-1")
            .is_some());
        assert!(host
            .management
            .startup_closure("startup-deputy-1")
            .is_none());
    }

    #[test]
    fn all_startups_are_validated_before_any_management_projection_repair() {
        let root = tempfile::tempdir().unwrap();
        let mut host = fixture(root.path(), true, true);
        let record = host
            .startups
            .get("project", "startup-native")
            .unwrap()
            .clone();
        host.startups
            .close(
                "project",
                "startup-native",
                &record.head,
                StartupClosure {
                    reason: "pending projection".into(),
                    workflow_sequence: 2,
                    workflow_hash: record.intent.workflow_hash.clone(),
                },
            )
            .unwrap();
        let mut other = record.intent;
        other.attempt_id = "wrong-worker-startup".into();
        other.management_operation = None;
        other.management_hash = None;
        other.worker.model = "not-the-approved-worker".into();
        host.startups.begin(other).unwrap();
        assert!(host.recover_startups().is_err());
        assert!(host
            .management
            .startup_closure("startup-deputy-1")
            .is_none());
        assert!(!host
            .root
            .join("management-decisions/startup-deputy-1.4.json")
            .exists());
        drop(host);
        assert!(RuntimeHost::open(&root.path().join("data")).is_err());
        assert!(!root
            .path()
            .join("data/management-decisions/startup-deputy-1.4.json")
            .exists());
    }

    #[test]
    fn orphan_management_closure_and_legacy_mutable_job_cannot_prove_startup_closure() {
        let root = tempfile::tempdir().unwrap();
        let mut host = fixture(root.path(), true, true);
        let head = host
            .management
            .response_head("startup-deputy-1")
            .unwrap()
            .to_owned();
        host.management
            .resolve(
                "startup-deputy-1",
                &head,
                UserResolution::AbandonStartup {
                    reason: "orphan".into(),
                    attempt_id: "missing-startup".into(),
                    startup_hash: "a".repeat(64),
                },
            )
            .unwrap();
        assert!(host.validate_startups().is_err());
        assert!(host.recover_startups().is_err());
        drop(host);
        assert!(RuntimeHost::open(&root.path().join("data")).is_err());

        let root = tempfile::tempdir().unwrap();
        let mut host = fixture(root.path(), false, true);
        let original = host.codex_jobs.values().next().unwrap().clone();
        let mut legacy = original;
        legacy.attempt_id = "legacy-without-origin".into();
        legacy.state = CodexJobState::Abandoned;
        crate::async_codex::save(&host.root, &legacy).unwrap();
        host.codex_jobs.insert(
            (legacy.project_id.clone(), legacy.attempt_id.clone()),
            legacy,
        );
        let detail = host
            .startup_detail("project", "legacy-without-origin")
            .unwrap();
        assert_eq!(detail.summary.state, "legacy");
        assert!(
            detail.protocol.is_none()
                && detail.evidence_digest.is_none()
                && detail.blocked_reason.is_some()
        );
        assert!(host
            .abandon_codex_startup(
                "project",
                "legacy-without-origin",
                &"a".repeat(64),
                "not proof"
            )
            .is_err());
        drop(host);
        let host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(
            host.codex_jobs[&("project".into(), "legacy-without-origin".into())].state,
            CodexJobState::Unknown
        );
    }

    #[test]
    fn application_advancement_and_active_ownership_reject_startup_disposition() {
        let root = tempfile::tempdir().unwrap();
        let mut host = fixture(root.path(), true, true);
        let target = assignment(&host, "startup-native");
        let state = host
            .management
            .intent("startup-deputy-1")
            .unwrap()
            .state_digest
            .clone();
        host.management
            .prepare_application("startup-deputy-1", target, &state)
            .unwrap();
        assert_eq!(
            host.startup_detail("project", "startup-native")
                .unwrap()
                .summary
                .state,
            "advanced"
        );
        assert!(host
            .startup_detail("project", "startup-native")
            .unwrap()
            .evidence_digest
            .is_none());
        host.recover_startups().unwrap();
        assert!(host
            .startups
            .get("project", "startup-native")
            .unwrap()
            .closure
            .is_none());

        let root = tempfile::tempdir().unwrap();
        let mut host = fixture(root.path(), true, true);
        let view = host.startup_detail("project", "startup-native").unwrap();
        let (_sender, receiver) = std::sync::mpsc::channel();
        host.codex_active.insert(
            ("project".into(), "startup-native".into()),
            crate::async_codex::ActiveCodex {
                preparation_cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                advice_wake: tokio::sync::watch::channel(()).0,
                advice_gate: std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0)),
                observation: None,
                observation_saved: None,
                observation_error: None,
                snapshot: crate::async_verification::snapshot(&host.projects["project"]),
                job: host.codex_jobs.values().next().unwrap().clone(),
                receiver,
                disconnected: true,
                cancel: None,
            },
        );
        assert!(host
            .abandon_codex_startup(
                "project",
                "startup-native",
                view.evidence_digest.as_deref().unwrap(),
                "blocked"
            )
            .is_err());
        host.codex_active
            .remove(&("project".into(), "startup-native".into()));
        host.codex_jobs.values_mut().next().unwrap().updated_at = "2026-10-08T01:00:00Z".into();
        assert!(host
            .abandon_codex_startup(
                "project",
                "startup-native",
                view.evidence_digest.as_deref().unwrap(),
                "stale job"
            )
            .is_err());
    }

    #[test]
    fn original_prefix_remains_valid_after_new_attempt_or_new_revision() {
        for new_revision in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let mut host = fixture(root.path(), true, true);
            close(&mut host);
            if new_revision {
                let mut revision = host.projects["project"].revision().unwrap().clone();
                revision.workflow.version += 1;
                host.propose("project", "new-revision", revision.clone())
                    .unwrap();
                host.approve("project", &revision.approval_digest().unwrap())
                    .unwrap();
            } else {
                let ApplicationTarget::Workflow {
                    principal,
                    command,
                    context,
                } = assignment(&host, "fresh-attempt")
                else {
                    unreachable!()
                };
                host.projects
                    .get_mut("project")
                    .unwrap()
                    .command(
                        "codex-fresh-attempt-assign",
                        &principal,
                        command,
                        *context,
                        false,
                    )
                    .unwrap();
            }
            let history = host.projects["project"].history().to_vec();
            drop(host);
            let host = RuntimeHost::open(&root.path().join("data")).unwrap();
            assert_eq!(host.projects["project"].history(), history);
            assert_eq!(
                host.startup_detail("project", "startup-native")
                    .unwrap()
                    .summary
                    .state,
                "closed"
            );
            assert_eq!(
                host.projects["project"].state().tasks["task"].state,
                if new_revision {
                    TaskState::Pending
                } else {
                    TaskState::Assigned
                }
            );
        }
    }

    #[test]
    fn other_attempt_without_job_is_checked_from_workflow_and_never_reset() {
        for dispatch in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let mut host = fixture(root.path(), false, true);
            let ApplicationTarget::Workflow {
                principal,
                command,
                context,
            } = assignment(&host, "other-attempt")
            else {
                unreachable!()
            };
            let store = host.projects.get_mut("project").unwrap();
            store
                .command(
                    "codex-other-attempt-assign",
                    &principal,
                    command,
                    *context,
                    false,
                )
                .unwrap();
            if dispatch {
                store
                    .command(
                        "codex-other-attempt-dispatch",
                        "core",
                        WorkflowCommand::DispatchIntent {
                            task: "task".into(),
                            attempt: "other-attempt".into(),
                        },
                        CommandContext {
                            thread_id: Some("synthetic-startup-session".into()),
                            cwd: Some(store.revision().unwrap().project.root.clone()),
                            ..Default::default()
                        },
                        false,
                    )
                    .unwrap();
            }
            let history = store.history().to_vec();
            assert!(!host
                .codex_jobs
                .contains_key(&("project".into(), "other-attempt".into())));
            let view = host.startup_detail("project", "startup-native").unwrap();
            if dispatch {
                assert!(view.evidence_digest.is_none());
                assert!(view.blocked_reason.unwrap().contains("待对账任务"));
            } else {
                // Assigned has its own recovery route; closing this older startup must not reset it.
                close(&mut host);
                assert_eq!(
                    host.projects["project"].state().tasks["task"].state,
                    TaskState::Assigned
                );
            }
            assert_eq!(host.projects["project"].history(), history);
        }
    }

    #[test]
    fn unmanaged_startup_cannot_be_adopted_by_a_different_management_application() {
        let root = tempfile::tempdir().unwrap();
        let mut host = fixture(root.path(), false, true);
        host.codex_jobs.values_mut().next().unwrap().state = CodexJobState::Failed;
        let context = host
            .management_context("project", "Implement a sum function")
            .unwrap();
        let state_digest = digest(&context).unwrap();
        host.management
            .begin(DecisionIntent {
                operation_id: "foreign-management".into(),
                project_id: "project".into(),
                purpose: Purpose::Assign,
                model: "fixture-deputy".into(),
                state_digest: state_digest.clone(),
                context,
                created_at: chrono::Utc::now().to_rfc3339(),
            })
            .unwrap();
        host.management
            .finish(
                "foreign-management",
                Ok(DecisionObservation {
                    response_id: "resp_foreign_management".into(),
                    requested_model: "fixture-deputy".into(),
                    actual_model: "fixture-deputy".into(),
                    input_tokens: 0,
                    output_tokens: 0,
                    envelope: DecisionEnvelope {
                        state_digest: state_digest.clone(),
                        decision: ManagementDecision::Assign {
                            task_id: "task".into(),
                            worker_id: "worker-codex".into(),
                            rationale: "conflicting reservation".into(),
                        },
                    },
                }),
            )
            .unwrap();
        let target = assignment(&host, "startup-native");
        host.management
            .prepare_application("foreign-management", target, &state_digest)
            .unwrap();
        assert!(host
            .recover_startups()
            .unwrap_err()
            .contains("另一管理应用冲突"));
        assert!(host
            .startup_detail("project", "startup-native")
            .unwrap()
            .evidence_digest
            .is_none());
        use cerulean_domain::workspace::WorkspaceQuery;
        assert!(host
            .workspace_query(&WorkspaceQuery::CodexStartups {
                project_id: "project".into(),
                offset: 0,
                revision: None,
            })
            .is_err());
        assert!(host
            .workspace_query(&WorkspaceQuery::CodexStartup {
                project_id: "project".into(),
                attempt_id: "startup-native".into(),
                revision: "0".repeat(64),
            })
            .is_err());
        drop(host);
        assert!(RuntimeHost::open(&root.path().join("data")).is_err());
    }

    #[test]
    #[ignore = "isolated native startup recovery fixture; zero real provider calls"]
    fn seed_native_startup_recovery_fixture() {
        let root = PathBuf::from(
            std::env::var_os("CERULEAN_STARTUP_NATIVE_DIR").expect("fixture directory"),
        );
        assert!(root.is_absolute() && !root.join("data").exists());
        let root = root.canonicalize().unwrap();
        let reports = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../reports")
            .canonicalize()
            .unwrap();
        assert!(root.starts_with(reports));
        let mode = std::env::var("CERULEAN_STARTUP_MODE").unwrap_or_else(|_| "managed".into());
        assert!(matches!(
            mode.as_str(),
            "managed" | "unmanaged" | "missing-job"
        ));
        let host = fixture(&root, mode != "unmanaged", mode != "missing-job");
        let view = host.startup_detail("project", "startup-native").unwrap();
        assert!(view.evidence_digest.is_some());
        fs::write(root.join("fixture.json"), serde_json::to_vec_pretty(&serde_json::json!({
            "project":"project", "task":"task", "attempt":"startup-native", "operation":view.summary.management_operation,
            "mode":mode, "withJob":mode != "missing-job", "workflowSequence":2, "taskState":"Pending", "fence":0, "realModelCalls":0,
            "startupIntentHash":view.summary.intent_hash,
        })).unwrap()).unwrap();
    }
}
