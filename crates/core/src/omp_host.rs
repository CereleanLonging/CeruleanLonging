//! Trusted deputy startup and local-user control for resident OMP jobs.
use super::*;
use crate::startup_journal::{StartupClosure, StartupIntent, OMP_PROTOCOL};
use cerulean_domain::omp_jobs::{OmpJob, OmpJobState};

impl RuntimeHost {
    pub(super) fn acknowledge_omp_stop_issue(
        &mut self,
        project: &str,
        attempt: &str,
        evidence: &str,
        text: &str,
    ) -> Result<(), String> {
        self.acknowledge_omp_stop_issue_observed(project, attempt, evidence, text, |_| Ok(()))
    }
    pub(super) fn acknowledge_omp_stop_issue_observed(
        &mut self,
        project: &str,
        attempt: &str,
        evidence: &str,
        text: &str,
        mut observer: impl FnMut(&str) -> Result<(), String>,
    ) -> Result<(), String> {
        require(
            !self.omp_stop_issues.is_empty()
                && self.desktop_active.is_none()
                && self.omp_active.is_empty()
                && self.codex_active.is_empty()
                && self.active.is_empty()
                && self.management_work_count() == 0,
            "仅无活动执行器的诊断模式可复核停止记录",
        )?;
        let issue = self
            .omp_stop_issues
            .iter()
            .find(|i| i.project_id == project && i.attempt_id == attempt)
            .ok_or("停止诊断记录缺失")?;
        require(
            issue.evidence_digest.as_deref() == Some(evidence),
            "停止复核摘要已变化或此类故障不能确认",
        )?;
        // Never release an active run on the next open, even if evidence was
        // externally damaged instead of interrupted after the normal pause.
        let record = self
            .omp_startups
            .get(project, attempt)
            .ok_or("OMP启动记录缺失")?;
        crate::omp_stop_review::validate_review(
            &self.root,
            &record.intent,
            &record.intent_hash,
            evidence,
            text,
        )?;
        let runs: Vec<_> = self
            .runs
            .all()
            .into_iter()
            .filter(|r| r.project_id == project && !r.state.terminal())
            .map(|r| r.id)
            .collect();
        for run in runs {
            self.pause_project_run(&run)?;
        }
        observer("paused")?;
        let record = self
            .omp_startups
            .get(project, attempt)
            .ok_or("OMP启动记录缺失")?;
        crate::omp_stop_review::acknowledge_observed(
            &self.root,
            &record.intent,
            &record.intent_hash,
            evidence,
            text,
            &mut observer,
        )?;
        self.omp_stop_issues
            .iter_mut()
            .find(|i| i.project_id == project && i.attempt_id == attempt)
            .unwrap()
            .acknowledged = true;
        // Stay diagnostic until a full reopen revalidates every ledger/link.
        observer("acknowledged")
    }
    #[cfg(windows)]
    pub(super) fn validate_omp_runtime_parent(&self, path: &str) -> Result<String, String> {
        let parent = crate::omp_process::check_profile_parent(Path::new(path))?;
        for scope in std::iter::once(self.root.clone()).chain(
            self.snapshot()
                .workflows
                .into_iter()
                .map(|s| PathBuf::from(s.project.root)),
        ) {
            require(
                !crate::artifacts::contains_path(&scope, &parent)
                    && !crate::artifacts::contains_path(&parent, &scope),
                "OMP运行目录不能与核心数据目录或已登记项目重叠",
            )?;
        }
        parent
            .to_str()
            .map(str::to_owned)
            .ok_or("OMP运行目录须为Unicode路径".into())
    }
    pub(super) fn load_omp_jobs(&mut self) -> Result<(), String> {
        for record in self.omp_startups.all() {
            let i = &record.intent;
            let store = self.projects.get(&i.project_id).ok_or("OMP启动项目缺失")?;

            let anchor = store
                .history()
                .get(
                    i.workflow_sequence
                        .checked_sub(1)
                        .ok_or("OMP启动锚点缺失")? as usize,
                )
                .ok_or("OMP启动锚点超出账本")?;
            require(
                Some(&anchor.hash) == i.workflow_hash.as_ref(),
                "OMP启动锚点与账本不符",
            )?;
            let plan = store
                .history()
                .iter()
                .find_map(|e| match &e.body.context.execution {
                    Some(ExecutionEvidence::Revision { revision })
                        if revision
                            .approval_digest()
                            .is_ok_and(|d| d == i.revision_digest) =>
                    {
                        Some(revision)
                    }
                    _ => None,
                })
                .ok_or("OMP启动方案缺失")?;
            require(
                plan.project.id == i.project_id
                    && plan.task(&i.task_id)?.adapter_id == "omp"
                    && plan.task(&i.task_id)?.worker.as_ref() == Some(&i.worker),
                "OMP启动执行器与方案不符",
            )?;
            require(store.history().iter().take(i.workflow_sequence as usize).any(|e|matches!(&e.body.context.execution,Some(ExecutionEvidence::Approval{approval}) if approval.revision_digest==i.revision_digest)),"OMP启动前没有批准证据")?;
            require(
                self.startups.get(&i.project_id, &i.attempt_id).is_none(),
                "尝试ID同时属于两个执行器",
            )?;
            if let Some(closure) = &record.closure {
                require(
                    store
                        .history()
                        .get(
                            closure
                                .workflow_sequence
                                .checked_sub(1)
                                .ok_or("OMP关闭锚点缺失")? as usize,
                        )
                        .map(|e| &e.hash)
                        == closure.workflow_hash.as_ref(),
                    "OMP关闭锚点与账本不符",
                )?;
            }
            if record.closure.is_some() {
                require(
                    !store
                        .history()
                        .iter()
                        .any(|e| e.body.event.command.attempt() == Some(&i.attempt_id)),
                    "已关闭OMP启动出现工作流尝试",
                )?;
            }
            if let Some(attempt) = store.attempt_record(&i.task_id, &i.attempt_id) {
                plan.validate_attempt_workspace(attempt)?;
                require(
                    attempt.worker_id == "worker-omp"
                        && attempt.revision_digest == i.revision_digest
                        && attempt.binding.adapter_id == "omp",
                    "OMP已分配尝试与启动不符",
                )?;
            }
            if let Err(error) = crate::async_omp::cancellation(&self.root, i, &record.intent_hash) {
                self.omp_stop_issues.push(crate::omp_stop_review::inspect(
                    &self.root,
                    i,
                    &record.intent_hash,
                    error,
                ));
            }
            let state = if record.closure.is_some() {
                OmpJobState::Abandoned
            } else {
                crate::async_omp::observed_state(i, store)
            };
            self.omp_jobs.insert(
                (i.project_id.clone(), i.attempt_id.clone()),
                OmpJob {
                    management_operation: i.management_operation.clone(),
                    project_id: i.project_id.clone(),
                    task_id: i.task_id.clone(),
                    attempt_id: i.attempt_id.clone(),
                    revision_digest: i.revision_digest.clone(),
                    worker: i.worker.clone(),
                    state,
                    started_at: i.started_at.clone(),
                    message: None,
                    closure_digest: None,
                },
            );
        }
        Ok(())
    }
    pub(super) fn omp_summaries(&self) -> Vec<OmpJob> {
        self.omp_jobs
            .values()
            .map(|job| self.omp_job_view(job))
            .collect()
    }
    pub(super) fn omp_job_view(&self, original: &OmpJob) -> OmpJob {
        let mut job = original.clone();
        job.closure_digest = self
            .omp_closure_digest(&job.project_id, &job.attempt_id)
            .ok();
        job
    }
    fn omp_closure_digest(&self, project: &str, attempt: &str) -> Result<String, String> {
        require(
            !self.has_omp_activity(project)
                && !self.has_codex_activity(project)
                && !self.active.contains_key(project),
            "项目后台尚未结束",
        )?;
        let record = self
            .omp_startups
            .get(project, attempt)
            .ok_or("OMP启动记录缺失")?;
        require(record.closure.is_none(), "OMP启动已经关闭")?;
        let store = self.projects.get(project).ok_or("OMP项目不可用")?;
        require(!store.requires_reopen(), "OMP项目需要重开")?;
        require(
            !store
                .history()
                .iter()
                .any(|e| e.body.event.command.attempt() == Some(attempt)),
            "已分配尝试需要工作流对账，不能按未分配关闭",
        )?;
        let operation = record.intent.management_operation.as_deref();
        if let Some(op) = operation {
            require(
                self.management.application(op).is_none(),
                "OMP应用已有准备记录，请在管理应用详情处理",
            )?;
        }
        require(
            !self
                .runs
                .all()
                .iter()
                .any(|r| r.project_id == project && r.state == project_setup::RunState::Active),
            "请先暂停项目调度",
        )?;
        require(
            !self.management_active.keys().any(|op| {
                self.management
                    .intent(op)
                    .is_ok_and(|i| i.project_id == project)
            }),
            "项目指挥仍在运行",
        )?;
        let statuses = self.management.statuses();
        require(
            !statuses.iter().any(|s| {
                s.project_id == project
                    && Some(s.operation_id.as_str()) != operation
                    && !s.state.resolved()
            }),
            "项目存在其他未决指挥",
        )?;
        let heads = statuses
            .iter()
            .filter(|s| s.project_id == project)
            .map(|s| {
                self.management
                    .evidence_digest(&s.operation_id)
                    .map(|h| (s.operation_id.as_str(), h))
            })
            .collect::<Result<Vec<_>, _>>()?;
        digest(&(&record.head, store.history().last().map(|e| &e.hash), heads))
    }
    /// Trusted deputy callback. No user IPC command accepts roles or starts a model.
    pub fn start_omp(
        &mut self,
        project: &str,
        task: &str,
        attempt: &str,
        wanted: &str,
    ) -> Result<(), String> {
        self.start_omp_inner(project, task, attempt, wanted, None)
    }
    pub(super) fn start_omp_inner(
        &mut self,
        project: &str,
        task: &str,
        attempt: &str,
        wanted: &str,
        operation: Option<String>,
    ) -> Result<(), String> {
        self.poll_verifications();
        self.require_available_inner(project, true)?;
        self.require_worker_slot()?;
        self.require_task_resources(project, task, attempt)?;
        self.require_execution_boundaries(project)?;
        require(
            !self.management.statuses().iter().any(|s| {
                s.project_id == project
                    && Some(s.operation_id.as_str()) != operation.as_deref()
                    && !s.state.resolved()
            }),
            "OMP不能绕过其他未决指挥事务",
        )?;
        require(
            self.omp_active.len()
                + self.codex_active.len()
                + self.active.len()
                + self.api_active.len()
                + usize::from(self.desktop_active.is_some())
                < 4,
            "后台执行数量达到上限",
        )?;
        require(
            self.startups.get(project, attempt).is_none()
                && self.omp_startups.get(project, attempt).is_none(),
            "执行尝试ID已保留，不能重用",
        )?;
        self.startups
            .check_reservation(project, attempt, operation.as_deref())?;
        self.omp_startups
            .check_reservation(project, attempt, operation.as_deref())?;
        self.management
            .check_attempt_reservation(project, attempt, operation.as_deref())?;
        let view = self.omp_read(project, task, attempt)?;
        let store = &*view;
        let worker = crate::async_omp::preflight(store, task, attempt, wanted)?;
        let scoped = operation.is_some()
            && api_assignment_scope::isolated(store.revision().ok_or("OMP方案缺失")?, task)?;
        if self.api_writers.contains_key(project) {
            require(scoped, "OMP并行启动需要批准的独立目录与原管理决定")?;
        }
        let anchor = if scoped {
            self.validate_api_assignment_scope(
                operation.as_deref().unwrap(),
                store,
                task,
                store.history().len() as u32,
                true,
            )?
            .sequence
        } else {
            store.history().len() as u32
        };
        if let Some(parent) = &worker.omp_runtime_parent {
            require(
                self.validate_omp_runtime_parent(parent)? == *parent,
                "OMP运行目录须使用已保存的规范路径，请重新配置",
            )?;
        }
        let key = crate::credential_vault::load_for(
            &self.root,
            credentials::CredentialProvider::DeepSeek,
        )?;
        let management_hash = if let Some(op) = &operation {
            require(
                self.management.intent(op)?.project_id == project
                    && self.management.application(op).is_none(),
                "OMP管理归属不符或已有应用",
            )?;
            let observed = self.management_envelope(op)?;
            require(
                matches!(&observed.decision,cerulean_domain::management::ManagementDecision::Assign{task_id,worker_id,..} if task_id==task && worker_id=="worker-omp"),
                "OMP启动与副指挥分配不符",
            )?;
            Some(self.management.response_head(op)?.to_owned())
        } else {
            None
        };
        let job = OmpJob {
            management_operation: operation.clone(),
            project_id: project.into(),
            task_id: task.into(),
            attempt_id: attempt.into(),
            revision_digest: wanted.into(),
            worker,
            state: OmpJobState::Starting,
            started_at: chrono::Utc::now().to_rfc3339(),
            message: None,
            closure_digest: None,
        };
        let intent = StartupIntent {
            schema_version: if scoped { 2 } else { 1 },
            protocol: OMP_PROTOCOL.into(),
            project_id: project.into(),
            task_id: task.into(),
            attempt_id: attempt.into(),
            revision_digest: wanted.into(),
            worker: job.worker.clone(),
            management_operation: operation,
            management_hash,
            workflow_sequence: anchor,
            workflow_hash: store
                .history()
                .get(anchor.checked_sub(1).ok_or("OMP批准锚点缺失")? as usize)
                .map(|e| e.hash.clone()),
            started_at: job.started_at.clone(),
        };
        drop(view);
        self.omp_startups.begin(intent)?;
        self.omp_jobs
            .insert((project.into(), attempt.into()), job.clone());
        let mut new_config = None;
        let launched = (|| {
            if !self.api_writers.contains_key(project) {
                let store = self.projects.remove(project).ok_or("OMP项目不可用")?;
                new_config = Some(store.configuration().clone());
                let writer = crate::async_api::ApiWorkflow::start(
                    store,
                    self.root.join("workflows").join(project),
                )?;
                self.api_writers.insert(project.into(), writer);
            }
            let client =
                self.api_writers[project]
                    .owner
                    .omp_client(crate::workflow_coordinator::Scope {
                        project: project.into(),
                        task: task.into(),
                        attempt: attempt.into(),
                        revision: wanted.into(),
                    })?;
            crate::async_omp::launch(
                &self.root,
                client,
                job,
                key,
                #[cfg(test)]
                self.omp_prepare_gate.take(),
            )
        })();
        match launched {
            Ok(active) => {
                self.omp_active
                    .insert((project.into(), attempt.into()), active);
                Ok(())
            }
            Err(error) => {
                if let Some(config) = new_config {
                    let store = match self.api_writers.remove(project) {
                        Some(mut writer) => writer.owner.finish(false),
                        None => DurableWorkflow::open(
                            &self.root.join("workflows").join(project),
                            config,
                        ),
                    }?;
                    self.projects.insert(project.into(), store);
                }
                let job = self
                    .omp_jobs
                    .get_mut(&(project.into(), attempt.into()))
                    .unwrap();
                job.state = OmpJobState::Failed;
                job.message = Some(error.clone());
                Err(error)
            }
        }
    }
    pub(super) fn poll_omp(&mut self) {
        let mut done = vec![];
        let mut assignments = vec![];
        let mut checkpoints = vec![];
        for ((project, attempt), active) in &mut self.omp_active {
            if active.disconnected {
                continue;
            }
            loop {
                match active.receiver.try_recv() {
                    Ok(crate::async_omp::Update::ToolCheckpoint { checkpoint, reply }) => {
                        checkpoints.push((project.clone(), attempt.clone(), *checkpoint, reply));
                    }
                    Ok(crate::async_omp::Update::Assignment(update)) => {
                        assignments.push((project.clone(), update));
                        break;
                    }
                    Ok(crate::async_omp::Update::Progress(snapshot)) => {
                        active.snapshot = *snapshot;
                        if active.job.state != OmpJobState::CancelRequested {
                            active.job.state = OmpJobState::Executing;
                        }
                        self.omp_jobs.insert(
                            (project.clone(), active.job.attempt_id.clone()),
                            active.job.clone(),
                        );
                    }
                    Ok(crate::async_omp::Update::Done {
                        interrupted,
                        outcome,
                        cleaned,
                    }) => {
                        done.push((
                            project.clone(),
                            attempt.clone(),
                            interrupted,
                            outcome,
                            cleaned,
                        ));
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        active.disconnected = true;
                        active.job.state = OmpJobState::Unknown;
                        active.job.message = Some("OMP后台通道中断，需要重开核查".into());
                        self.omp_jobs.insert(
                            (project.clone(), active.job.attempt_id.clone()),
                            active.job.clone(),
                        );
                        break;
                    }
                }
            }
        }
        for (project, attempt, checkpoint, reply) in checkpoints {
            if let Err(error) = self.accept_omp_tool_checkpoint(&project, &attempt, checkpoint) {
                if let Some(reply) = reply {
                    let _ = reply.send(Err(error.clone()));
                }
                if let Some(active) = self.omp_active.get_mut(&(project, attempt)) {
                    active.tool_checkpoint = None;
                    active.job.message = Some(error);
                }
            } else if let Err(error) = self.handle_omp_advice_boundary(&project, &attempt, reply) {
                if let Some(active) = self.omp_active.get_mut(&(project, attempt)) {
                    active.job.message = Some(error);
                }
            }
        }
        for (project, update) in assignments {
            match update {
                crate::managed_assignment::Update::Prepare {
                    operation,
                    attempt,
                    snapshot,
                    reply,
                } => {
                    let result = self.prepare_managed_assignment(
                        &project, &operation, *attempt, *snapshot, "omp",
                    );
                    let _ = reply.send(result);
                }
                crate::managed_assignment::Update::Commit {
                    operation,
                    entry,
                    snapshot,
                    reply,
                } => {
                    let result = self
                        .commit_managed_assignment(&project, &operation, *entry, *snapshot, "omp");
                    let _ = reply.send(result);
                }
            }
        }
        for (project, attempt, interrupted, outcome, cleaned) in done {
            let mut active = self
                .omp_active
                .remove(&(project.clone(), attempt.clone()))
                .unwrap();
            if let Some(thread) = active.thread.take() {
                let _ = thread.join();
            }
            if !cleaned {
                active.disconnected = true;
                active.cancel = None;
                active.job.state = OmpJobState::Unknown;
                active.job.message = Some("OMP进程回收未确认，需要重开核查".into());
                self.omp_jobs.insert(
                    (project.clone(), active.job.attempt_id.clone()),
                    active.job.clone(),
                );
                self.omp_active.insert((project.clone(), attempt), active);
                if let Some(writer) = self.api_writers.get_mut(&project) {
                    writer.interrupted = true;
                }
                continue;
            }
            let mut job = active.job;
            if let Err(error) = self.finish_omp_owner(&project, &mut job, interrupted, outcome) {
                self.api_fault = Some(error.clone());
                job.state = OmpJobState::Unknown;
                job.message = Some(error);
            }
            self.omp_jobs.insert((project, job.attempt_id.clone()), job);
        }
    }
    pub(super) fn cancel_omp(
        &mut self,
        project: &str,
        attempt: &str,
        reason: &str,
    ) -> Result<(), String> {
        self.cancel_omp_observed(project, attempt, reason, |_| Ok(()))
    }
    pub(super) fn cancel_omp_observed(
        &mut self,
        project: &str,
        attempt: &str,
        reason: &str,
        mut observer: impl FnMut(&str) -> Result<(), String>,
    ) -> Result<(), String> {
        let active = self
            .omp_active
            .get(&(project.into(), attempt.into()))
            .ok_or("OMP任务当前没有活动执行器")?;
        require(
            active.job.attempt_id == attempt && !active.disconnected,
            "OMP停止目标与当前执行不符",
        )?;
        require(
            !reason.trim().is_empty() && reason.len() <= 8000 && !reason.contains('\0'),
            "请输入有效的停止原因",
        )?;
        let runs: Vec<_> = self
            .runs
            .all()
            .into_iter()
            .filter(|r| r.project_id == project && !r.state.terminal())
            .map(|r| r.id)
            .collect();
        for run in runs {
            self.pause_project_run(&run)?;
        }
        observer("run-paused")?;
        let active = self
            .omp_active
            .get_mut(&(project.into(), attempt.into()))
            .ok_or("OMP任务已经结束")?;
        let record = self
            .omp_startups
            .get(project, attempt)
            .ok_or("OMP启动记录缺失")?;
        crate::async_omp::record_cancel_observed(
            &self.root,
            &record.intent,
            &record.intent_hash,
            reason,
            observer,
        )?;
        active
            .preparation_cancel
            .store(true, std::sync::atomic::Ordering::Release);
        if let Some(sender) = active.cancel.take() {
            let _ = sender.send(());
        }
        active.job.state = OmpJobState::CancelRequested;
        self.omp_jobs
            .insert((project.into(), attempt.into()), active.job.clone());
        Ok(())
    }
    pub(super) fn recover_omp(&mut self, project: &str, attempt: &str) -> Result<(), String> {
        require(
            !self.has_omp_activity(project)
                && !self.has_codex_activity(project)
                && !self.active.contains_key(project),
            "项目仍在执行或验收",
        )?;
        let job = self
            .omp_jobs
            .get(&(project.into(), attempt.into()))
            .ok_or("OMP执行记录缺失")?
            .clone();
        require(job.state == OmpJobState::Unknown, "仅未知OMP尝试可以对账")?;
        let store = self.projects.get_mut(project).ok_or("OMP项目需要重开")?;
        let (_, _, intent, _) = crate::omp_execution::scope(store, &job.task_id, attempt)?;
        let path = store.omp_attempt_path(attempt)?;
        require(path.is_dir(), "OMP原始调用证据缺失")?;
        let not_started = crate::omp_journal::OmpJournal::open(&path, &intent)?
            .observation()
            .not_started;
        if not_started {
            crate::omp_execution::resolve_unsent_cancellation(store, &job.task_id, attempt)?;
        } else {
            crate::omp_execution::recover_terminal(store, &job.task_id, attempt)?;
        }
        let state = crate::async_omp::observed_state(
            &self.omp_startups.get(project, attempt).unwrap().intent,
            store,
        );
        let job = self
            .omp_jobs
            .get_mut(&(project.into(), attempt.into()))
            .unwrap();
        job.state = state;
        job.message = None;
        Ok(())
    }
    pub(super) fn abandon_omp_startup(
        &mut self,
        project: &str,
        attempt: &str,
        evidence: &str,
        reason: &str,
    ) -> Result<(), String> {
        crate::async_omp::reason(reason)?;
        require(
            self.omp_closure_digest(project, attempt)? == evidence,
            "OMP启动复核已变化",
        )?;
        let record = self.omp_startups.get(project, attempt).unwrap();
        let head = record.head.clone();
        let store = self.projects.get(project).unwrap();
        self.omp_startups.close(
            project,
            attempt,
            &head,
            StartupClosure {
                reason: reason.into(),
                workflow_sequence: store.history().len() as u32,
                workflow_hash: store.history().last().map(|e| e.hash.clone()),
            },
        )?;
        let job = self
            .omp_jobs
            .get_mut(&(project.into(), attempt.into()))
            .ok_or("OMP执行记录缺失")?;
        job.state = OmpJobState::Abandoned;
        job.message = None;
        self.recover_omp_startups()?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "omp_host/tests.rs"]
pub(crate) mod tests;
