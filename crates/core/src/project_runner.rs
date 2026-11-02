//! Project orchestration with bounded isolated API parallel dispatch. Persist user run authorization before scheduling.
use super::*;
use crate::management_execution::{ApplicationTarget, DecisionState};
use cerulean_domain::{
    management::{ManagementDecision, Purpose},
    project_setup::{ProjectRun, RunState},
};

#[cfg(windows)]
#[path = "project_runner_parallel.rs"]
mod parallel;

impl RuntimeHost {
    pub(super) fn run_scope(&self, run: &ProjectRun) -> Result<(), String> {
        require(
            self.projects
                .get(&run.project_id)
                .is_none_or(|s| s.state().revision_requested.is_none()),
            "原方案已进入修订，请等待新方案批准后启动",
        )?;
        let setup = self
            .project_setups
            .get(&run.project_id)
            .ok_or("运行配置缺失")?;
        require(
            setup.digest == run.setup_digest,
            "运行配置已变化，需要重新授权",
        )?;
        let snapshot = self
            .snapshot()
            .workflows
            .into_iter()
            .find(|w| w.project.id == run.project_id)
            .ok_or("项目状态不可读")?;
        require(
            snapshot.state.revision_requested.is_none(),
            "原方案已进入修订，请等待新方案批准后启动",
        )?;
        let revision = snapshot.revision.ok_or("执行方案缺失")?;
        require(
            revision.management_setup_digest.as_ref()
                == (setup.setup.management_sources.is_some()
                    || setup.setup.chief_directory.is_some())
                .then_some(&setup.digest),
            "主副指挥配置与批准方案绑定不符，请重新规划和批准",
        )?;
        require(
            revision.approval_digest()? == run.revision_digest
                && snapshot
                    .approval
                    .as_ref()
                    .is_some_and(|a| a.revision_digest == run.revision_digest),
            "执行方案或批准已变化",
        )?;
        require(
            revision.policy == setup.setup.policy
                && revision.worktree == setup.setup.worktree
                && revision.snapshot_policy.as_ref() == Some(&setup.setup.snapshot_policy)
                && revision.workflow.tasks.iter().all(|t| {
                    revision.task(&t.id).is_ok_and(|e| {
                        setup
                            .setup
                            .resources_for(e.profile_id.as_deref())
                            .is_ok_and(|keys| keys == t.resource_keys)
                    })
                })
                && revision.execution.iter().all(|e| {
                    setup
                        .setup
                        .execution_for(&e.task_id, e.profile_id.as_deref())
                        .is_ok_and(|wanted| wanted == *e)
                }),
            "执行方案与已选运行配置不符",
        )
    }
    pub(super) fn validate_runs(&self) -> Result<(), String> {
        for run in self.runs.all() {
            let store = self
                .projects
                .get(&run.project_id)
                .ok_or("运行项目账本缺失")?;
            if run.state == RunState::Active {
                require(
                    store.state().revision_requested.is_none()
                        && store.revision().is_some_and(|r| {
                            r.approval_digest().is_ok_and(|d| d == run.revision_digest)
                        }),
                    "旧运行不能在修订后继续活动",
                )?;
            }
            require(store.history().iter().any(|e|matches!(&e.body.context.execution,Some(ExecutionEvidence::Approval { approval }) if approval.revision_digest==run.revision_digest)),"运行授权缺少实际批准证据")?;
            if run.state == RunState::Superseded {
                require(
                    self.has_run_revision_request(&run)?,
                    "修订结束的运行缺少用户修订证据",
                )?;
            }
            if run.state == RunState::Completed {
                let proof = store
                    .history()
                    .iter()
                    .find_map(|e| match &e.body.context.execution {
                        Some(ExecutionEvidence::Revision { revision })
                            if revision
                                .approval_digest()
                                .is_ok_and(|d| d == run.revision_digest) =>
                        {
                            Some((e.body.event.sequence, revision))
                        }
                        _ => None,
                    })
                    .ok_or("运行方案证据缺失")?;
                let next = store
                    .history()
                    .iter()
                    .filter(|e| {
                        e.body.event.sequence > proof.0
                            && matches!(e.body.event.command, WorkflowCommand::Propose { .. })
                    })
                    .map(|e| e.body.event.sequence)
                    .min()
                    .unwrap_or(u32::MAX);
                require(proof.1.workflow.tasks.iter().all(|t|store.history().iter().any(|e|e.body.event.sequence>proof.0 && e.body.event.sequence<next && matches!(&e.body.event.command,WorkflowCommand::Accept { task,.. } if task==&t.id))),"已完成运行缺少工作流收口证据")?;
            }
        }
        Ok(())
    }
    pub(super) fn start_project_run(
        &mut self,
        project: &str,
        revision: &str,
        setup: &str,
    ) -> Result<(), String> {
        // Only fully synthetic count/generation transports can exercise the
        // monetary run in tests. No flag or permission exists in production.
        #[cfg(test)]
        let synthetic =
            self.management_scoped_fixture.is_some() && self.api_scoped_fixture.is_some();
        #[cfg(not(test))]
        let synthetic = false;
        if !synthetic {
            self.require_currency_execution_available(project)?;
        }
        require(
            self.runner_error.is_none() && self.runs.healthy(),
            "运行控制需要重开恢复",
        )?;
        let id = format!("run-{}", &digest(&(project, revision))?[..32]);
        let previous = self.runs.all().into_iter().find(|r| r.id == id);
        let run = ProjectRun {
            id,
            project_id: project.into(),
            revision_digest: revision.into(),
            setup_digest: setup.into(),
            state: RunState::Active,
            message: "已授权持续调度；关闭界面后核心继续运行".into(),
            acknowledged_decisions: self.management_operation_total() as u32,
        };
        self.run_scope(&run)?;
        #[cfg(windows)]
        self.validate_desktop_run(project)?;
        #[cfg(not(windows))]
        require(
            !self.project_setups[project].setup.has_desktop(),
            "桌面执行需要Windows",
        )?;
        #[cfg(windows)]
        self.api_run_credentials(project)?;
        #[cfg(windows)]
        require(
            self.api_fault.is_none()
                && !self.api_jobs.values().any(|j| {
                    j.project_id == project
                        && matches!(
                            j.state,
                            api_jobs::ApiJobState::Unknown | api_jobs::ApiJobState::CancelRequested
                        )
                }),
            "API执行结果或停止请求待核对，不能恢复调度",
        )?;
        require(
            !self.omp_jobs.values().any(|job| {
                job.project_id == project
                    && job.state == cerulean_domain::omp_jobs::OmpJobState::CancelRequested
            }),
            "OMP停止请求尚未确认，不能恢复调度",
        )?;
        require(
            !self.codex_jobs.values().any(|job| {
                job.project_id == project
                    && matches!(
                        job.state,
                        CodexJobState::CancelRequested | CodexJobState::Cancelling
                    )
            }),
            "停止请求尚未确认，不能恢复调度",
        )?;
        require(
            !self
                .snapshot()
                .workflows
                .iter()
                .filter(|w| w.project.id == project)
                .any(|w| {
                    w.state.tasks.values().any(|task| {
                        matches!(task.state, TaskState::Cancelled | TaskState::Retired)
                            || (task.cancellation_requested == Some(true)
                                && matches!(
                                    task.state,
                                    TaskState::Running | TaskState::Cancelling | TaskState::Unknown
                                ))
                    })
                }),
            "请先核对停止结果；已取消或本地结束的任务需要修订方案后继续",
        )?;
        require(
            !self.management.statuses().iter().any(|s| {
                s.project_id == project && self.management.unanswered_wait(&s.operation_id)
            }),
            "请先回答指挥提出的补充信息，再恢复项目调度",
        )?;
        if let Some(old) = previous {
            if old.state == RunState::Active {
                return require(
                    old.revision_digest == revision && old.setup_digest == setup,
                    "运行授权不匹配",
                );
            }
            require(!old.state.terminal(), "该运行已结束")?;
        }
        #[cfg(windows)]
        if self.has_codex_activity(project) {
            require(
                !self.management.statuses().iter().any(|s| {
                    s.project_id == project && s.purpose == Purpose::Advise && !s.state.resolved()
                }),
                "原执行存在未决监督建议，请先核对；恢复不会重新授权旧建议",
            )?;
            self.require_resolved_advice_deliveries(project)?;
        }
        self.runs.save(run, "user")?;
        #[cfg(windows)]
        for active in self
            .codex_active
            .values()
            .filter(|a| a.job.project_id == project)
        {
            // Old replies remain bound to their earlier run digest and counted request.
            active
                .advice_gate
                .store(0, std::sync::atomic::Ordering::SeqCst);
        }
        Ok(())
    }
    pub(super) fn pause_project_run(&mut self, id: &str) -> Result<(), String> {
        let mut run = self
            .runs
            .all()
            .into_iter()
            .find(|r| r.id == id)
            .ok_or("运行记录不存在")?;
        #[cfg(windows)]
        self.revoke_omp_advice(&run.project_id);
        #[cfg(windows)]
        for active in self
            .codex_active
            .values()
            .filter(|a| a.job.project_id == run.project_id)
        {
            active
                .advice_gate
                .store(3, std::sync::atomic::Ordering::SeqCst);
        }
        if run.state == RunState::Paused {
            return Ok(());
        }
        require(!run.state.terminal(), "该运行已结束")?;
        run.state = RunState::Paused;
        run.message = "用户暂停后续调度；已发送任务仍会收集结果，暂停不等于取消".into();
        self.runs.save(run, "user")
    }
    pub(super) fn set_run_outcome(
        &mut self,
        mut run: ProjectRun,
        state: RunState,
        message: String,
    ) -> Result<(), String> {
        run.state = state;
        #[cfg(windows)]
        self.revoke_omp_advice(&run.project_id);
        #[cfg(windows)]
        for active in self
            .codex_active
            .values()
            .filter(|a| a.job.project_id == run.project_id)
        {
            active
                .advice_gate
                .store(3, std::sync::atomic::Ordering::SeqCst);
        }
        run.message = message.chars().take(1000).collect();
        self.runs.save(run, "core")
    }
    pub(super) fn ensure_run_dispatch_authorized(&self, operation: &str) -> Result<(), String> {
        if let Some(run) = self
            .runs
            .all()
            .into_iter()
            .find(|r| operation.starts_with(&format!("{}-", r.id)))
        {
            require(
                run.state == RunState::Active && self.runner_error.is_none(),
                "项目已暂停后续调度",
            )?;
            self.run_scope(&run)?;
        }
        Ok(())
    }
    #[cfg(windows)]
    pub(super) fn poll_project_runs(&mut self) {
        if !self.omp_stop_issues.is_empty() {
            return;
        }
        if self.orchestrating || self.runner_error.is_some() || !self.runs.healthy() {
            return;
        }
        self.orchestrating = true;
        for run in self
            .runs
            .all()
            .into_iter()
            .filter(|r| r.state == RunState::Active)
        {
            if let Err(error) = self.advance_run(&run) {
                if let Err(save_error) = self.set_run_outcome(run, RunState::Attention, error) {
                    self.runner_error = Some(format!("运行状态保存失败，停止新调度：{save_error}"));
                    break;
                }
            }
        }
        self.orchestrating = false;
    }
    #[cfg(windows)]
    fn advance_run(&mut self, run: &ProjectRun) -> Result<(), String> {
        self.run_scope(run)?;
        let project = &run.project_id;
        // Exclusive verification temporarily owns the store outside `projects`.
        // Wait for its return before rechecking advice against that store.
        if self.active.get(project).is_some_and(|a| !a.shared) {
            return Ok(());
        }
        self.require_resolved_advice_deliveries(project)?;
        if !self.has_omp_activity(project) {
            self.require_resolved_omp_advice(project)?;
        }
        self.poll_omp_supervision(run)?;
        if self.omp_supervision_pending(run)? {
            return Ok(());
        }
        self.poll_api_supervision(run)?;
        if self.api_supervision_pending(run)? {
            return Ok(());
        }
        if self.has_codex_activity(project) {
            self.poll_supervision_request(run)?;
            if self.management.statuses().iter().any(|s| {
                s.project_id == *project && s.purpose == Purpose::Advise && !s.state.resolved()
            }) {
                return Ok(());
            }
        }
        if self.desktop_active.as_ref().is_some_and(|a| {
            a.snapshot.project.id == *project
                || self.projects.get(project).is_some_and(|s| {
                    s.revision().is_some_and(|p| {
                        p.execution.iter().any(|e| {
                            e.desktop_worker.is_some()
                                && s.state().tasks.get(&e.task_id).is_some_and(|t| {
                                    matches!(t.state, TaskState::Pending | TaskState::Assigned)
                                })
                        })
                    })
                })
        }) || self.counting_project(project)
            || self.management_active.keys().any(|id| {
                self.management
                    .intent(id)
                    .is_ok_and(|i| i.project_id == *project)
            })
        {
            return Ok(());
        }
        if self.has_api_activity(project) {
            return self.advance_parallel_api_run(run);
        }
        self.require_available(project)?;
        let statuses = self.management.statuses();
        if let Some(pending) = statuses
            .iter()
            .find(|s| s.project_id == *project && !s.state.resolved())
        {
            if pending.purpose == Purpose::Advise
                && self
                    .management
                    .intent(&pending.operation_id)?
                    .context
                    .get("supervision")
                    .is_some()
            {
                return Err(
                    "监督建议尚未应用或结果待核对，请先审阅指挥记录；不会重复请求或另开任务".into(),
                );
            }
            require(
                pending.operation_id.starts_with(&format!("{}-", run.id)),
                "项目存在运行之外的待处理指挥请求",
            )?;
            if pending.state == DecisionState::AwaitingExternal {
                require(
                    pending.purpose == Purpose::Accept,
                    "unexpected external management purpose in run",
                )?;
                self.management.external_request(&pending.operation_id)?;
                self.require_external_accept_run(project, &pending.operation_id)?;
                return Ok(());
            }
            require(
                pending.state == DecisionState::Ready,
                "指挥结果或应用状态未知，请先对账；不会重发",
            )?;
            let observation = self.management_envelope(&pending.operation_id)?;
            match observation.decision.clone() {
                ManagementDecision::Assign {
                    task_id, worker_id, ..
                } => {
                    if !self.worker_slot_available() {
                        return Ok(());
                    }
                    let candidate_attempt =
                        format!("attempt-{}", &digest(&pending.operation_id)?[..32]);
                    if self
                        .task_resource_conflict(project, &task_id, &candidate_attempt)?
                        .is_some()
                    {
                        return Ok(());
                    }
                    let spec = self
                        .projects
                        .get(project)
                        .ok_or("项目账本缺失")?
                        .revision()
                        .ok_or("方案缺失")?
                        .task(&task_id)?;
                    let selected: project_setup::ProjectWorkerAdapter =
                        serde_json::from_value(serde_json::Value::String(spec.adapter_id.clone()))
                            .map_err(|_| "批准的执行接入无效")?;
                    require(
                        worker_id == selected.worker_id(),
                        "副指挥选择了未配置的执行器",
                    )?;
                    let attempt = format!("attempt-{}", &digest(&pending.operation_id)?[..32]);
                    match selected {
                        project_setup::ProjectWorkerAdapter::Qoder
                        | project_setup::ProjectWorkerAdapter::Workbuddy => {
                            self.assign_managed_desktop(&pending.operation_id, &attempt)?
                        }
                        project_setup::ProjectWorkerAdapter::Codex => {
                            self.start_managed_codex(&pending.operation_id, &attempt)?
                        }
                        project_setup::ProjectWorkerAdapter::Omp => {
                            self.start_managed_omp(&pending.operation_id, &attempt)?
                        }
                        _ => self.start_managed_api(&pending.operation_id, &attempt)?,
                    }
                }
                ManagementDecision::Accept {
                    task_id,
                    attempt_id,
                    ..
                } => self.apply_management_decision_inner(
                    &pending.operation_id,
                    ApplicationTarget::Workflow {
                        principal: "chief".into(),
                        command: WorkflowCommand::Accept {
                            task: task_id,
                            attempt: attempt_id,
                        },
                        context: Box::default(),
                    },
                )?,
                ManagementDecision::Wait { reason } => {
                    self.apply_management_decision_inner(
                        &pending.operation_id,
                        ApplicationTarget::Wait,
                    )?;
                    return self.set_run_outcome(
                        run.clone(),
                        RunState::Attention,
                        format!("指挥请求补充信息：{reason}"),
                    );
                }
                _ => return Err("持续运行返回了用途不匹配的指挥决定".into()),
            }
            return Ok(());
        }
        // A crash between consuming Wait and saving Attention must not start another billed call.
        let all_count = self.management_operation_total();
        for status in statuses
            .iter()
            .filter(|s| s.operation_id.starts_with(&format!("{}-", run.id)))
        {
            let intent = self.management.intent(&status.operation_id)?;
            // Request IDs include the global count at creation, avoiding lexical order assumptions.
            let ordinal = status
                .operation_id
                .rsplit('-')
                .next()
                .and_then(|v| v.parse::<u32>().ok())
                .ok_or("invalid run request ordinal")?;
            if ordinal > run.acknowledged_decisions
                && self
                    .management
                    .application(&intent.operation_id)
                    .is_some_and(|(t, _, _)| matches!(t, ApplicationTarget::Wait))
            {
                return Err("此前指挥要求补充信息，请确认后手动恢复".into());
            }
        }
        let store = self.projects.get(project).ok_or("项目账本暂不可用")?;
        let tasks = &store.state().tasks;
        require(!tasks.is_empty(), "批准方案没有任务")?;
        if tasks.values().all(|t| t.state == TaskState::Accepted) {
            let message = if let Some(finale) = store.revision().and_then(|p| p.integration()) {
                let plan = store.revision().ok_or("方案缺失")?;
                plan.validate_integration()?;
                let task = &tasks[&finale.task_id];
                require(task.verified, "项目整体检查尚未通过")?;
                format!(
                    "最终集成已通过整体检查及主指挥收口；交付目录：{}",
                    plan.execution_root(
                        &finale.task_id,
                        task.attempt.as_deref().ok_or("集成尝试缺失")?
                    )?
                )
            } else {
                "全部任务已有独立验收及主指挥收口记录".into()
            };
            return self.set_run_outcome(run.clone(), RunState::Completed, message);
        }
        if tasks.iter().any(|(id, t)| {
            t.state == TaskState::Assigned
                && store
                    .revision()
                    .is_some_and(|p| p.task(id).is_ok_and(|e| e.desktop_worker.is_some()))
        }) {
            self.require_desktop_tasks_resolved()?;
            let assigned = tasks
                .values()
                .filter(|t| t.state == TaskState::Assigned)
                .collect::<Vec<_>>();
            require(assigned.len() == 1, "桌面串行调度只能有一个待审阅尝试")?;
            self.authorize_desktop_send(
                project,
                assigned[0].attempt.as_deref().ok_or("尝试缺失")?,
            )?;
            // Waiting is a projection of Assigned, not a new user run authorization.
            return Ok(());
        }
        require(
            !tasks.values().any(|t| {
                matches!(
                    t.state,
                    TaskState::Assigned
                        | TaskState::Dispatching
                        | TaskState::Running
                        | TaskState::Cancelling
                        | TaskState::Unknown
                        | TaskState::Cancelled
                        | TaskState::Retired
                )
            }),
            "任务中断或取消，先处理原尝试；不会自动重新派发",
        )?;
        let setup = self.project_setups[project].setup.clone();
        let purpose = if let Some((task, attempt)) = tasks
            .iter()
            .find(|(_, t)| t.state == TaskState::Submitted)
            .map(|(id, t)| (id.clone(), t.clone()))
        {
            if !attempt.verified {
                let attempt_id = attempt.attempt.ok_or("提交缺少尝试关联")?;
                require(
                    !self
                        .jobs
                        .values()
                        .any(|j| j.project_id == *project && j.attempt_id == attempt_id),
                    "本次独立验收未通过或待处置，禁止自动重跑",
                )?;
                let pins = store
                    .revision()
                    .ok_or("revision missing")?
                    .task(&task)?
                    .checks
                    .iter()
                    .map(crate::verifier::PinnedCheck::from_approved)
                    .collect::<Result<Vec<_>, _>>()?;
                let verification = format!(
                    "verify-{}",
                    &digest(&(run.id.as_str(), attempt_id.as_str()))?[..32]
                );
                self.start_verification(project, &verification, &task, &attempt_id, pins)?;
                return Ok(());
            }
            Purpose::Accept
        } else {
            if !self.worker_slot_available() {
                return Ok(());
            }
            let next_attempt = format!(
                "attempt-{}",
                &digest(&format!("{}-{}", run.id, all_count + 1))?[..32]
            );
            let mut available = false;
            for task in &store.revision().ok_or("方案缺失")?.workflow.tasks {
                if tasks
                    .get(&task.id)
                    .is_some_and(|t| t.state == TaskState::Pending)
                    && task
                        .dependencies
                        .iter()
                        .all(|d| tasks.get(d).is_some_and(|t| t.state == TaskState::Accepted))
                    && self
                        .task_resource_conflict(project, &task.id, &next_attempt)?
                        .is_none()
                {
                    available = true;
                    break;
                }
            }
            if !available {
                return Ok(());
            }
            if store.revision().is_some_and(|p| {
                p.execution.iter().any(|e| {
                    e.desktop_worker.is_some()
                        && tasks
                            .get(&e.task_id)
                            .is_some_and(|t| t.state == TaskState::Pending)
                })
            }) {
                self.require_desktop_assignment_ready(project)?;
            }
            if store.revision().is_some_and(|p| {
                p.execution.iter().any(|e| {
                    e.api_worker.is_some()
                        && tasks
                            .get(&e.task_id)
                            .is_some_and(|t| t.state == TaskState::Pending)
                })
            }) {
                require(
                    crate::api_execution::prior_requests(store)? < setup.policy.max_requests,
                    "API项目请求额度已用完；不会再请求副指挥分配",
                )?;
            }
            Purpose::Assign
        };
        self.api_run_credentials(project)?;
        let operation = format!("{}-{}", run.id, all_count + 1);
        let model = if purpose == Purpose::Accept {
            &setup.chief_model
        } else {
            &setup.deputy_model
        };
        self.start_management_decision(project, &operation, purpose, model, &setup.requirement)
    }
    #[cfg(windows)]
    fn api_run_credentials(&self, project: &str) -> Result<(), String> {
        // During pause/resume a worker may still hold the project ledger.
        let snapshot = self
            .snapshot()
            .workflows
            .into_iter()
            .find(|w| w.project.id == project)
            .ok_or("项目状态不可读")?;
        let revision = snapshot.revision.as_ref().ok_or("方案缺失")?;
        let explicit = self
            .project_setups
            .get(project)
            .is_some_and(|s| s.setup.management_sources.is_some());
        if explicit {
            self.load_management_credentials(project)?;
        }

        if !revision
            .execution
            .iter()
            .any(|e| e.api_worker.is_some() || e.desktop_worker.is_some())
        {
            return Ok(());
        }
        // This checks local decryptability, not remote authentication; no provider call here.
        if !explicit {
            self.load_management_credentials(project)?;
        }
        for worker in revision
            .execution
            .iter()
            .filter_map(|e| e.api_worker.as_ref())
        {
            crate::credential_vault::load_for(&self.root, worker.target.provider()).map_err(
                |_| {
                    format!(
                        "请先在本工作台配置可读取的 {} 编程模型凭证",
                        worker.target.provider().id()
                    )
                },
            )?;
        }
        Ok(())
    }
}
