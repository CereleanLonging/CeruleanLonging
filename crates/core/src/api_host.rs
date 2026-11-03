//! API lifecycle owned by RuntimeHost. Startup, deputy assignment and workflow are separate evidence.
use super::*;
use crate::{api_startup::ApiStartup, management_execution::ApplicationTarget};
use cerulean_domain::{
    api_jobs::{ApiJob, ApiJobState},
    management::{ManagementDecision, Purpose},
};

fn observed(
    startup: &ApiStartup,
    store: &impl crate::workflow_access::WorkflowAccess,
) -> ApiJobState {
    if store.history().iter().any(|e|matches!(&e.body.event.command,WorkflowCommand::RetireUnknownApi{task,attempt,..} if task==&startup.attempt.task_id&&attempt==&startup.attempt.id)) {return ApiJobState::Retired;}
    if store.history().iter().any(|e|matches!(&e.body.event.command,WorkflowCommand::AbandonUnsent{task,attempt,..} if task==&startup.attempt.task_id&&attempt==&startup.attempt.id)) {return ApiJobState::Abandoned;}
    if store.history().iter().any(|e|matches!(&e.body.event.command,
        WorkflowCommand::Submit{task,attempt,..}|WorkflowCommand::Reconcile{task,attempt,outcome:ReconcileOutcome::Submitted,..}
        if task==&startup.attempt.task_id&&attempt==&startup.attempt.id)) { ApiJobState::Submitted } else { ApiJobState::Unknown }
}
fn job(startup: &ApiStartup, state: ApiJobState) -> ApiJob {
    ApiJob {
        project_id: startup.project_id.clone(),
        task_id: startup.attempt.task_id.clone(),
        attempt_id: startup.attempt.id.clone(),
        revision_digest: startup.attempt.revision_digest.clone(),
        worker: startup.worker.clone(),
        management_operation: Some(startup.management_operation.clone()),
        state,
        started_at: startup.started_at.clone(),
        message: None,
        closure_digest: None,
        retirement_digest: None,
    }
}
impl RuntimeHost {
    pub(super) fn has_api_activity(&self, project: &str) -> bool {
        self.api_writers.contains_key(project) || self.api_active.keys().any(|(p, _)| p == project)
    }

    pub(super) fn load_api_jobs(&mut self) -> Result<(), String> {
        crate::api_startup::validate_cancellations(&self.root, &self.api_startups)?;
        for startup in self.api_startups.values() {
            let store = self
                .projects
                .get(&startup.project_id)
                .ok_or("API启动项目缺失")?;
            let prefix = store
                .history()
                .get(..startup.workflow_sequence as usize)
                .ok_or("API启动锚点越界")?;
            require(
                prefix
                    .last()
                    .is_some_and(|e| e.hash == startup.workflow_hash),
                "API启动锚点摘要不符",
            )?;
            let events = prefix
                .iter()
                .map(|e| e.body.event.clone())
                .collect::<Vec<_>>();
            let replay = Workflow::replay(store.configuration().principals.clone(), &events)?;
            let task = replay
                .state()
                .tasks
                .get(&startup.attempt.task_id)
                .ok_or("API启动任务缺失")?;
            require(
                replay.state().approved
                    && replay.state().revision_requested.is_none()
                    && task.state == TaskState::Pending
                    && task.attempt.is_none()
                    && task.fence.checked_add(1) == Some(startup.attempt.fence),
                "API启动锚点不是批准的待分配任务",
            )?;
            let revision = prefix
                .iter()
                .rev()
                .find_map(|e| match &e.body.context.execution {
                    Some(ExecutionEvidence::Revision { revision }) => Some(revision),
                    _ => None,
                })
                .ok_or("API历史方案缺失")?;
            let approval = prefix
                .iter()
                .rev()
                .find_map(|e| match &e.body.context.execution {
                    Some(ExecutionEvidence::Approval { approval })
                        if approval.revision_digest == startup.attempt.revision_digest =>
                    {
                        Some(approval)
                    }
                    _ => None,
                })
                .ok_or("API历史批准缺失")?;
            approval.validate(
                revision,
                "user",
                store
                    .configuration()
                    .execution_scope
                    .as_ref()
                    .ok_or("API scope missing")?
                    .policy_version,
            )?;
            let spec = revision.task(&startup.attempt.task_id)?;
            startup.validate_workspace(revision)?;
            require(
                revision.project.id == startup.project_id
                    && revision.approval_digest()? == startup.attempt.revision_digest
                    && spec.api_worker.as_ref() == Some(&startup.worker)
                    && spec.worker.is_none()
                    && spec.adapter_id == startup.attempt.binding.adapter_id,
                "API启动与原批准不符",
            )?;
            let definition = revision
                .workflow
                .tasks
                .iter()
                .find(|t| t.id == startup.attempt.task_id)
                .ok_or("API任务定义缺失")?;
            require(
                definition.dependencies.iter().all(|d| {
                    replay
                        .state()
                        .tasks
                        .get(d)
                        .is_some_and(|t| t.state == TaskState::Accepted)
                }),
                "API启动依赖或worker不符",
            )?;
            require(
                !self
                    .startups
                    .all()
                    .iter()
                    .chain(self.omp_startups.all().iter())
                    .any(|r| {
                        r.intent.management_operation.as_deref()
                            == Some(startup.management_operation.as_str())
                            || (r.intent.project_id == startup.project_id
                                && r.intent.attempt_id == startup.attempt.id)
                    }),
                "API启动与其他执行器重复",
            )?;
            let intent = self.management.intent(&startup.management_operation)?;
            require(
                intent.project_id == startup.project_id
                    && intent.purpose == Purpose::Assign
                    && self
                        .management
                        .response_head(&startup.management_operation)?
                        == startup.management_hash,
                "API管理响应锚点不符",
            )?;
            require(
                matches!(&self.management.original_envelope(&startup.management_operation)?.decision,ManagementDecision::Assign{task_id,worker_id,..} if task_id==&startup.attempt.task_id&&worker_id==&startup.attempt.worker_id),
                "API副指挥决定不符",
            )?;
            self.validate_api_startup_closure(startup)?;
            let target = ApplicationTarget::Workflow {
                principal: "deputy".into(),
                command: WorkflowCommand::Assign {
                    task: startup.attempt.task_id.clone(),
                    worker: startup.attempt.worker_id.clone(),
                    attempt: startup.attempt.id.clone(),
                },
                context: Box::new(CommandContext {
                    execution: Some(ExecutionEvidence::Attempt {
                        attempt: startup.attempt.clone(),
                    }),
                    ..Default::default()
                }),
            };
            if let Some((saved, _, _)) = self.management.application(&startup.management_operation)
            {
                require(saved == &target, "API管理应用目标不符")?;
            }
            if startup.version == 2 {
                let original = self.validate_api_assignment_scope(
                    &startup.management_operation,
                    store,
                    &startup.attempt.task_id,
                    startup.workflow_sequence,
                    false,
                )?;
                require(
                    original.sequence == startup.workflow_sequence,
                    "API启动与原管理历史锚点不符",
                )?;
            }
            let owner = format!("mgmt-{}", startup.management_operation);
            for entry in store.history().iter().filter(|e|e.body.operation_id==owner||matches!(&e.body.event.command,WorkflowCommand::Assign{attempt,..} if attempt==&startup.attempt.id)) {
                let ApplicationTarget::Workflow{principal,command,context}=&target else{unreachable!()};
                if startup.version == 2 {
                    self.validate_api_assignment_scope(&startup.management_operation, store, &startup.attempt.task_id,
                        entry.body.event.sequence.checked_sub(1).ok_or("API分配事件序号无效")?, false)?;
                } else { require(entry.body.event.sequence == startup.workflow_sequence + 1, "API分配序号与启动不符")?; }
                require(entry.body.operation_id==owner&&entry.body.event.principal==*principal&&entry.body.event.command==*command&&entry.body.context==**context&&self.management.application(&startup.management_operation).is_some(),"API任务分配与持久启动不符")?;
            }
            let state = if self
                .management
                .startup_closure(&startup.management_operation)
                .is_some()
            {
                ApiJobState::Abandoned
            } else {
                observed(startup, store)
            };
            self.api_jobs.insert(startup.key(), job(startup, state));
        }
        for (project, store) in &self.projects {
            for entry in store.history() {
                if let Some(ExecutionEvidence::Attempt { attempt }) = &entry.body.context.execution
                {
                    if cerulean_domain::api_worker::is_api_adapter(&attempt.binding.adapter_id) {
                        require(
                            self.api_startups
                                .get(&(project.clone(), attempt.id.clone()))
                                .is_some_and(|s| s.attempt == *attempt),
                            "API工作流分配缺少原启动证据",
                        )?;
                    }
                }
            }
        }
        for status in self.management.statuses() {
            if let Some((ApplicationTarget::Workflow { context, .. }, _, _)) =
                self.management.application(&status.operation_id)
            {
                if let Some(ExecutionEvidence::Attempt { attempt }) = &context.execution {
                    if cerulean_domain::api_worker::is_api_adapter(&attempt.binding.adapter_id) {
                        require(
                            self.api_startups
                                .get(&(status.project_id.clone(), attempt.id.clone()))
                                .is_some_and(|s| {
                                    s.attempt == *attempt
                                        && s.management_operation == status.operation_id
                                }),
                            "API管理分配缺少原启动证据",
                        )?;
                    }
                }
            }
        }
        Ok(())
    }
    pub fn start_managed_api(&mut self, operation: &str, attempt: &str) -> Result<(), String> {
        self.poll_verifications();
        identifier(attempt)?;
        require(attempt.len() <= 64, "API尝试ID过长")?;
        let project = self.management.intent(operation)?.project_id.clone();
        self.require_api_currency_available(&project)?;
        require(
            self.management.intent(operation)?.purpose == Purpose::Assign,
            "API启动需要副指挥分配用途",
        )?;
        self.require_available_inner(&project, true)?;
        self.require_worker_slot()?;
        self.ensure_run_dispatch_authorized(operation)?;
        require(
            self.management.application(operation).is_none(),
            "API管理决定已经准备或应用",
        )?;
        require(
            !self.management.statuses().iter().any(|s| {
                s.project_id == project && s.operation_id != operation && !s.state.resolved()
            }),
            "API不能绕过其他未决指挥事务",
        )?;
        require(
            self.api_active.len()
                + usize::from(self.desktop_active.is_some())
                + self.omp_active.len()
                + self.codex_active.len()
                + self.active.len()
                < 4,
            "后台执行数量达到上限",
        )?;
        self.startups
            .check_reservation(&project, attempt, Some(operation))?;
        self.omp_startups
            .check_reservation(&project, attempt, Some(operation))?;
        self.management
            .check_attempt_reservation(&project, attempt, Some(operation))?;
        require(
            !self.api_startups.values().any(|s| {
                s.management_operation == operation
                    || (s.project_id == project && s.attempt.id == attempt)
            }) && !self
                .startups
                .all()
                .iter()
                .chain(self.omp_startups.all().iter())
                .any(|s| {
                    s.intent.management_operation.as_deref() == Some(operation)
                        || (s.intent.project_id == project && s.intent.attempt_id == attempt)
                }),
            "API执行尝试或管理决定已保留，禁止重派",
        )?;
        crate::api_startup::available(&self.root, &project, attempt)?;
        let observation = self.management_envelope(operation)?;
        let ManagementDecision::Assign {
            task_id, worker_id, ..
        } = &observation.decision
        else {
            return Err("需要副指挥分配决定".into());
        };
        self.require_task_resources(&project, task_id, attempt)?;
        self.require_execution_boundaries(&project)?;
        let view = self.api_read(&project, task_id, attempt)?;
        let store = &*view;
        let plan = store.revision().ok_or("API方案缺失")?;
        plan.validate()?;
        store.approval().ok_or("API方案尚未批准")?.validate(
            plan,
            "user",
            store
                .configuration()
                .execution_scope
                .as_ref()
                .ok_or("API范围缺失")?
                .policy_version,
        )?;
        let worker = plan
            .task(task_id)?
            .api_worker
            .clone()
            .ok_or("方案不是API执行")?;
        require(
            worker_id == &format!("worker-{}", worker.target.provider().id()),
            "API执行器与决定不符",
        )?;
        require(
            store.state().revision_requested.is_none(),
            "API项目正在修订或恢复",
        )?;
        let state = store.state().tasks.get(task_id).ok_or("API任务缺失")?;
        require(
            state.state == TaskState::Pending && state.attempt.is_none(),
            "API任务不是待分配状态",
        )?;
        let definition = plan
            .workflow
            .tasks
            .iter()
            .find(|t| &t.id == task_id)
            .ok_or("API任务定义缺失")?;
        require(
            definition.dependencies.iter().all(|d| {
                store
                    .state()
                    .tasks
                    .get(d)
                    .is_some_and(|t| t.state == TaskState::Accepted)
            }),
            "API依赖尚未验收",
        )?;
        require(
            !store
                .history()
                .iter()
                .any(|e| e.body.event.command.attempt() == Some(attempt)),
            "API尝试ID已使用",
        )?;
        require(
            crate::api_execution::prior_requests(store)? < plan.policy.max_requests,
            "API项目请求额度已用完",
        )?;
        let client = cerulean_adapters::api_generation::ApiClient::new(
            crate::credential_vault::load_for(&self.root, worker.target.provider())?,
        )?;
        let workspace = plan.execution_root(task_id, attempt)?;
        let identity = format!("api-{}", &digest(&(&project, attempt))?[..32]);
        let scoped = api_assignment_scope::isolated(plan, task_id)?;
        if self.api_writers.contains_key(&project) {
            require(scoped, "并行API任务必须使用批准的独立工作目录")?;
        }
        let anchor = if scoped {
            self.validate_api_assignment_scope(
                operation,
                store,
                task_id,
                store.history().len() as u32,
                true,
            )?
            .sequence
        } else {
            store.history().len() as u32
        };
        let startup = ApiStartup {
            prepare_workspace: plan.worktree.is_some(),
            version: if scoped { 2 } else { 1 },
            project_id: project.clone(),
            attempt: Attempt {
                workspace_proof: None,
                id: attempt.into(),
                task_id: task_id.clone(),
                worker_id: worker_id.clone(),
                revision_digest: plan.approval_digest()?,
                fence: state.fence.checked_add(1).ok_or("API fence overflow")?,
                binding: SessionBinding {
                    adapter_id: worker.target.provider().id().into(),
                    adapter_version: "api-worker-v1".into(),
                    session_id: identity.clone(),
                    workspace,
                    identity_strength: IdentityStrength::ControllerRequest(
                        ControllerRequestIdentity {
                            connection_id: identity.clone(),
                            request_id: format!("request-{identity}"),
                        },
                    ),
                },
            },
            worker,
            management_operation: operation.into(),
            management_hash: self.management.response_head(operation)?.into(),
            workflow_sequence: anchor,
            workflow_hash: store
                .history()
                .get(anchor.checked_sub(1).ok_or("API批准锚点缺失")? as usize)
                .ok_or("API批准账本缺失")?
                .hash
                .clone(),
            started_at: chrono::Utc::now().to_rfc3339(),
        };
        drop(view);
        if let Err(error) = crate::api_startup::publish(&self.root, &startup) {
            self.api_fault = Some(error.clone());
            return Err(error);
        }
        let mut job = job(&startup, ApiJobState::Starting);
        if startup.prepare_workspace {
            job.message = Some("正在后台准备独立工作目录；尚未派发模型任务".into());
        }
        self.api_startups.insert(startup.key(), startup.clone());
        self.api_jobs.insert(startup.key(), job.clone());
        let mut new_config = None;
        let launched = (|| {
            if !self.api_writers.contains_key(&project) {
                let store = self.projects.remove(&project).ok_or("API项目不可用")?;
                new_config = Some(store.configuration().clone());
                let writer = crate::async_api::ApiWorkflow::start(
                    store,
                    self.root.join("workflows").join(&project),
                )?;
                self.api_writers.insert(project.clone(), writer);
            }
            let worker_store =
                self.api_writers[&project]
                    .owner
                    .client(crate::workflow_coordinator::Scope {
                        project: project.clone(),
                        task: startup.attempt.task_id.clone(),
                        attempt: attempt.into(),
                        revision: startup.attempt.revision_digest.clone(),
                    })?;
            crate::async_api::launch(
                &self.root,
                worker_store,
                startup,
                job,
                client,
                #[cfg(test)]
                self.api_scoped_fixture.clone(),
                #[cfg(test)]
                self.api_fixture,
                #[cfg(test)]
                self.api_gateway_fixture,
                #[cfg(test)]
                self.api_prepare_gate.take(),
            )
        })();
        match launched {
            Ok(active) => {
                self.api_active.insert((project, attempt.into()), active);
                Ok(())
            }
            Err(error) => {
                if let Some(config) = new_config {
                    let store = match self.api_writers.remove(&project) {
                        Some(mut writer) => writer.owner.finish(false),
                        None => DurableWorkflow::open(
                            &self.root.join("workflows").join(&project),
                            config,
                        ),
                    }?;
                    self.projects.insert(project.clone(), store);
                }
                let job = self.api_jobs.get_mut(&(project, attempt.into())).unwrap();
                job.state = ApiJobState::Unknown;
                job.message = Some(error.clone());
                Err(error)
            }
        }
    }
    pub(super) fn poll_api(&mut self) {
        let mut assignments = vec![];
        let mut currency = vec![];
        let mut tool_checkpoints = vec![];
        let mut done = vec![];
        for ((project, attempt_id), active) in &mut self.api_active {
            if active.disconnected {
                continue;
            }
            loop {
                match active.receiver.try_recv() {
                    Ok(crate::async_api::Update::ToolCheckpoint(request)) => {
                        tool_checkpoints.push((project.clone(), attempt_id.clone(), *request));
                        break;
                    }
                    Ok(crate::async_api::Update::Currency(request)) => {
                        currency.push((project.clone(), attempt_id.clone(), *request));
                        break;
                    }
                    Ok(crate::async_api::Update::WorkspaceReady { startup, reply }) => {
                        let result: Result<(), String> = (|| {
                            let current = self
                                .api_startups
                                .get(&startup.key())
                                .ok_or("API workspace reservation missing")?;
                            require(
                                current.project_id == *project
                                    && current.attempt.id == active.job.attempt_id,
                                "API workspace owner mismatch",
                            )?;
                            let saved = crate::api_startup::read_prepared(&self.root, current)?;
                            require(
                                saved == *startup,
                                "API workspace publication differs from notification",
                            )?;
                            saved.validate_workspace(
                                active
                                    .snapshot
                                    .revision
                                    .as_ref()
                                    .ok_or("API workspace revision missing")?,
                            )?;
                            self.api_startups.insert(saved.key(), saved);
                            active.job.message = None;
                            Ok(())
                        })();
                        if let Err(error) = &result {
                            self.api_fault = Some(error.clone());
                        }
                        let _ = reply.send(result);
                        break;
                    }
                    Ok(crate::async_api::Update::Assignment(update)) => {
                        assignments.push((
                            project.clone(),
                            active.job.worker.target.provider().id(),
                            update,
                        ));
                        break;
                    }
                    Ok(crate::async_api::Update::Progress(snapshot)) => {
                        active.snapshot = *snapshot;
                        if active.job.state != ApiJobState::CancelRequested {
                            active.job.state = ApiJobState::Executing;
                        }
                        self.api_jobs.insert(
                            (project.clone(), active.job.attempt_id.clone()),
                            active.job.clone(),
                        );
                    }
                    Ok(crate::async_api::Update::Done {
                        interrupted,
                        outcome,
                    }) => {
                        done.push((project.clone(), attempt_id.clone(), interrupted, outcome));
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        active.disconnected = true;
                        active.job.state = ApiJobState::Unknown;
                        active.job.message = Some("API后台通道中断，请重开核查".into());
                        self.api_jobs.insert(
                            (project.clone(), active.job.attempt_id.clone()),
                            active.job.clone(),
                        );
                        break;
                    }
                }
            }
        }
        for (project, adapter, update) in assignments {
            match update {
                crate::managed_assignment::Update::Prepare {
                    operation,
                    attempt,
                    snapshot,
                    reply,
                } => {
                    let _ = reply.send(self.prepare_managed_assignment(
                        &project, &operation, *attempt, *snapshot, adapter,
                    ));
                }
                crate::managed_assignment::Update::Commit {
                    operation,
                    entry,
                    snapshot,
                    reply,
                } => {
                    let _ = reply.send(self.commit_managed_assignment(
                        &project, &operation, *entry, *snapshot, adapter,
                    ));
                }
            }
        }
        for (project, attempt, request) in currency {
            self.handle_api_currency(&project, &attempt, request);
        }
        for (project, attempt, request) in tool_checkpoints {
            self.handle_api_tool_checkpoint(&project, &attempt, request);
        }
        for (project, attempt, interrupted, outcome) in done {
            let mut active = self
                .api_active
                .remove(&(project.clone(), attempt.clone()))
                .unwrap();
            if let Some(thread) = active.thread.take() {
                let _ = thread.join();
            }
            let mut job = active.job;
            job.state = ApiJobState::Unknown;
            job.message = outcome.err();
            if let Some(writer) = self.api_writers.get_mut(&project) {
                writer.interrupted |= interrupted;
            }
            // A completed attempt must not transfer or reopen another attempt's writer.
            let remaining = self.api_active.keys().any(|(p, _)| p == &project)
                || self.has_omp_activity(&project)
                || self.has_codex_activity(&project)
                || self.active.get(&project).is_some_and(|a| a.shared);
            if remaining {
                let result = (|| {
                    let writer = self.api_writers.get(&project).ok_or("API账本协调器缺失")?;
                    let reader = writer.owner.client(crate::workflow_coordinator::Scope {
                        project: project.clone(),
                        task: job.task_id.clone(),
                        attempt: attempt.clone(),
                        revision: job.revision_digest.clone(),
                    })?;
                    Ok::<_, String>((
                        crate::async_verification::snapshot(&reader),
                        observed(
                            &self.api_startups[&(project.clone(), attempt.clone())],
                            &reader,
                        ),
                    ))
                })();
                match result {
                    Ok((snapshot, state)) => {
                        self.refresh_finished_api_workspace(
                            &project,
                            &mut job,
                            snapshot.revision.as_ref(),
                        );
                        job.state = state;
                    }
                    Err(error) => {
                        self.api_fault = Some(error.clone());
                        job.message = Some(error);
                    }
                }
            } else {
                let store = self
                    .api_writers
                    .remove(&project)
                    .ok_or_else(|| "API账本协调器缺失".to_string())
                    .and_then(|mut writer| writer.owner.finish(writer.interrupted));
                match store {
                    Ok(store) => {
                        self.refresh_finished_api_workspace(&project, &mut job, store.revision());
                        job.state = observed(
                            &self.api_startups[&(project.clone(), attempt.clone())],
                            &store,
                        );
                        self.projects.insert(project.clone(), store);
                    }
                    Err(error) => {
                        self.api_fault = Some(error.clone());
                        job.message = Some(error);
                    }
                }
            }
            self.api_jobs.insert((project, attempt), job);
        }
        // Project projections come only from the writer, never whichever worker
        // happened to deliver its cached progress message last.
        for writer in self.api_writers.values_mut() {
            if let Err(error) = writer.refresh() {
                self.api_fault = Some(error);
            }
        }
    }
    fn refresh_finished_api_workspace(
        &mut self,
        project: &str,
        job: &mut ApiJob,
        plan: Option<&PlanRevision>,
    ) {
        let key = (project.to_owned(), job.attempt_id.clone());
        if !self.api_startups[&key].prepare_workspace {
            return;
        }
        let refreshed = crate::api_startup::load(&self.root).and_then(|saved| {
            let startup = saved
                .get(&key)
                .ok_or("API preparation reservation missing")?;
            require(
                startup.reservation_digest()? == self.api_startups[&key].reservation_digest()?,
                "API preparation reservation changed",
            )?;
            startup.validate_workspace(plan.ok_or("API preparation revision missing")?)?;
            Ok(startup.clone())
        });
        match refreshed {
            Ok(startup) => {
                self.api_startups.insert(key, startup);
            }
            Err(error) => {
                self.api_fault = Some(error.clone());
                job.message = Some(error);
            }
        }
    }
    pub fn recover_api(&mut self, project: &str, attempt: &str) -> Result<(), String> {
        require(
            !self.has_api_activity(project)
                && !self.has_omp_activity(project)
                && !self.has_codex_activity(project)
                && !self.active.contains_key(project),
            "项目仍在执行或验收",
        )?;
        let startup = self
            .api_startups
            .get(&(project.into(), attempt.into()))
            .ok_or("API启动记录缺失")?
            .clone();
        let store = self.projects.get_mut(project).ok_or("API项目需要重开")?;
        crate::api_execution::recover_terminal(store, &startup.attempt.task_id, attempt)?;
        self.api_jobs
            .insert(startup.key(), job(&startup, observed(&startup, store)));
        Ok(())
    }
    pub fn cancel_api(&mut self, project: &str, attempt: &str, reason: &str) -> Result<(), String> {
        require(
            !reason.trim().is_empty() && reason.len() <= 8000 && !reason.contains('\0'),
            "请输入有效停止原因",
        )?;
        let active = self
            .api_active
            .get(&(project.into(), attempt.into()))
            .ok_or("API任务没有活动执行器")?;
        require(
            active.job.attempt_id == attempt && !active.disconnected && active.cancel.is_some(),
            "API停止目标不符或已请求停止",
        )?;
        let runs = self
            .runs
            .all()
            .into_iter()
            .filter(|r| r.project_id == project && !r.state.terminal())
            .map(|r| r.id)
            .collect::<Vec<_>>();
        for run in runs {
            self.pause_project_run(&run)?;
        }
        let startup = self
            .api_startups
            .get(&(project.into(), attempt.into()))
            .ok_or("API启动记录缺失")?;
        if let Err(error) = crate::api_startup::cancel(&self.root, startup, reason) {
            self.api_fault = Some(error.clone());
            return Err(error);
        }
        let active = self
            .api_active
            .get_mut(&(project.into(), attempt.into()))
            .unwrap();
        active
            .preparation_cancel
            .store(true, std::sync::atomic::Ordering::Release);
        if let Some(sender) = active.cancel.take() {
            let _ = sender.send(());
        }
        active.job.state = ApiJobState::CancelRequested;
        self.api_jobs.insert(startup.key(), active.job.clone());
        Ok(())
    }
}
