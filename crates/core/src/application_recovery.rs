//! User closure of a prepared local application. The original intent stays authoritative.
//! An immutable closure proves neither remote cancellation nor refunded provider usage.
use super::*;
use crate::management_execution::{ApplicationTarget, DecisionState, UserResolution};
use cerulean_domain::omp_jobs::OmpJobState;
use cerulean_domain::project_setup::ApplicationRecoveryReview;

struct ApplicationInspection {
    target_kind: String,
    task_id: Option<String>,
    attempt_id: Option<String>,
    session_id: Option<String>,
    job_key: Option<(String, String)>,
    omp_job_key: Option<(String, String)>,
    #[cfg(windows)]
    api_job_key: Option<(String, String)>,
}

impl RuntimeHost {
    /// Checks a historical prefix, never the current task state for an old closure.
    fn inspect_unapplied_application(
        &self,
        operation: &str,
        sequence: u32,
        hash: Option<&str>,
    ) -> Result<ApplicationInspection, String> {
        identifier(operation)?;
        let intent = self.management.intent(operation)?;
        let (target, applied, receipt) = self
            .management
            .application(operation)
            .ok_or("请求没有持久化应用意图")?;
        require(!applied && receipt.is_none(), "应用已有成功回执，不能关闭")?;
        let ApplicationTarget::Workflow {
            command, context, ..
        } = target
        else {
            return Err("Wait 无工作流事件是正常状态，应先恢复回执并答复".into());
        };
        let store = self
            .projects
            .get(&intent.project_id)
            .ok_or("项目账本不可用")?;
        require(!store.requires_reopen(), "项目账本需要重开核心恢复")?;
        let history = store.history();
        let end = sequence as usize;
        require(end <= history.len(), "应用关闭锚点超出工作流历史")?;
        let actual = end.checked_sub(1).map(|n| history[n].hash.as_str());
        require(actual == hash, "应用关闭锚点与工作流历史不符")?;
        require(
            !history
                .iter()
                .any(|e| e.body.operation_id == format!("mgmt-{operation}")),
            "原应用已有关联工作流事件，请先对账",
        )?;
        let mut inspected = ApplicationInspection {
            target_kind: String::new(),
            task_id: command.task().map(str::to_owned),
            attempt_id: command.attempt().map(str::to_owned),
            session_id: None,
            job_key: None,
            omp_job_key: None,
            #[cfg(windows)]
            api_job_key: None,
        };
        let linked: Vec<_> = self
            .codex_jobs
            .values()
            .filter(|j| j.management_operation.as_deref() == Some(operation))
            .collect();
        let omp_linked: Vec<_> = self
            .omp_jobs
            .values()
            .filter(|j| j.management_operation.as_deref() == Some(operation))
            .collect();
        match command {
            WorkflowCommand::Assign {
                task,
                worker,
                attempt: id,
            } => {
                inspected.target_kind = "assign".into();
                require(!history.iter().any(|e| {
                    e.body.event.command.attempt() == Some(id.as_str())
                        || matches!(&e.body.context.execution, Some(ExecutionEvidence::Attempt { attempt }) if attempt.id == *id)
                }), "预留尝试已有工作流历史，不能按未分配关闭")?;
                let Some(ExecutionEvidence::Attempt { attempt }) = &context.execution else {
                    return Err("应用缺少原始会话绑定".into());
                };
                let events: Vec<_> = history[..end]
                    .iter()
                    .map(|e| e.body.event.clone())
                    .collect();
                let prefix = Workflow::replay(store.configuration().principals.clone(), &events)?;
                let run = prefix.state().tasks.get(task).ok_or("锚点任务不存在")?;
                require(
                    prefix.state().approved
                        && run.state == TaskState::Pending
                        && run.attempt.is_none(),
                    "关闭锚点不是已批准的待分配任务",
                )?;
                let revision = history[..end]
                    .iter()
                    .rev()
                    .find_map(|e| match &e.body.context.execution {
                        Some(ExecutionEvidence::Revision { revision }) => Some(revision),
                        _ => None,
                    })
                    .ok_or("锚点缺少执行方案")?;
                let revision_digest = revision.approval_digest()?;
                let approval = history[..end]
                    .iter()
                    .rev()
                    .find_map(|e| match &e.body.context.execution {
                        Some(ExecutionEvidence::Approval { approval })
                            if approval.revision_digest == revision_digest =>
                        {
                            Some(approval)
                        }
                        _ => None,
                    })
                    .ok_or("锚点缺少方案批准")?;
                let configuration = store.configuration();
                let scope = configuration
                    .execution_scope
                    .as_ref()
                    .ok_or("项目执行范围缺失")?;
                let user = configuration
                    .principals
                    .iter()
                    .find(|p| p.role == Role::User)
                    .ok_or("用户角色缺失")?;
                approval.validate(revision, &user.id, scope.policy_version)?;
                let spec = revision.task(task)?;
                revision.validate_attempt_workspace(attempt)?;
                require(
                    revision.project == scope.project
                        && revision.project.id == intent.project_id
                        && attempt.id == *id
                        && attempt.task_id == *task
                        && attempt.worker_id == *worker
                        && attempt.revision_digest == revision_digest
                        && attempt.fence == run.fence.checked_add(1).ok_or("fence overflow")?
                        && attempt.binding.adapter_id == spec.adapter_id
                        && *worker == format!("worker-{}", spec.adapter_id)
                        && !attempt.binding.session_id.trim().is_empty()
                        && !attempt.binding.adapter_version.trim().is_empty(),
                    "应用会话绑定与锚点批准不符",
                )?;
                require(
                    matches!(
                        (spec.adapter_id.as_str(), &attempt.binding.identity_strength),
                        ("codex", IdentityStrength::ProviderId)
                            | ("omp", IdentityStrength::ControllerRequest(_))
                    ) || (cerulean_domain::api_worker::is_api_adapter(&spec.adapter_id)
                        && matches!(
                            attempt.binding.identity_strength,
                            IdentityStrength::ControllerRequest(_)
                        )),
                    "此适配器尚无可核验的未派发应用协议",
                )?;
                inspected.session_id = Some(attempt.binding.session_id.clone());
                let key = (intent.project_id.clone(), id.clone());
                if spec.adapter_id == "omp" {
                    require(
                        linked.is_empty() && omp_linked.len() == 1,
                        "OMP应用启动关联不唯一",
                    )?;
                    let startup = self
                        .omp_startups
                        .get(&intent.project_id, id)
                        .ok_or("OMP启动证据缺失")?;
                    require(
                        startup.intent.management_operation.as_deref() == Some(operation)
                            && startup.closure.is_none(),
                        "OMP启动归属不符或已关闭",
                    )?;
                    let job = omp_linked[0];
                    require(
                        job.project_id == intent.project_id
                            && job.task_id == *task
                            && job.attempt_id == *id
                            && job.revision_digest == attempt.revision_digest
                            && spec.worker.as_ref() == Some(&job.worker),
                        "OMP记录与应用准备证据不符",
                    )?;
                    inspected.omp_job_key = Some(key);
                } else if cerulean_domain::api_worker::is_api_adapter(&spec.adapter_id) {
                    #[cfg(windows)]
                    {
                        require(
                            linked.is_empty() && omp_linked.is_empty(),
                            "API应用关联其他执行器",
                        )?;
                        let startup = self.api_startups.get(&key).ok_or("API应用缺少持久启动")?;
                        let job = self.api_jobs.get(&key).ok_or("API应用缺少任务记录")?;
                        require(
                            startup.management_operation == operation
                                && startup.attempt == *attempt
                                && spec.api_worker.as_ref() == Some(&startup.worker)
                                && job.management_operation.as_deref() == Some(operation),
                            "API应用与原启动不符",
                        )?;
                        inspected.api_job_key = Some(key);
                    }
                    #[cfg(not(windows))]
                    return Err("API应用恢复需要Windows".into());
                } else {
                    require(
                        omp_linked.is_empty() && linked.len() <= 1,
                        "原应用关联多个编程记录",
                    )?;
                    if let Some(job) = self.codex_jobs.get(&key) {
                        require(
                            job.management_operation.as_deref() == Some(operation),
                            "预留尝试已绑定另一管理请求",
                        )?;
                    }
                    if let Some(job) = linked.first() {
                        require(
                            job.project_id == intent.project_id
                                && job.task_id == *task
                                && job.attempt_id == *id
                                && job.revision_digest == attempt.revision_digest
                                && spec.worker.as_ref() == Some(&job.worker),
                            "编程记录与应用准备证据不符",
                        )?;
                        inspected.job_key = Some(key);
                    }
                }
            }
            WorkflowCommand::Propose { .. } => inspected.target_kind = "propose".into(),
            WorkflowCommand::Accept { .. } => inspected.target_kind = "accept".into(),
            WorkflowCommand::Advise { .. } => inspected.target_kind = "advise".into(),
            _ => return Err("不支持关闭此工作流目标".into()),
        }
        // Invalid/stale local Propose, Accept or Advise can be closed without trying
        // to execute them. Their exact authority was checked when .2 was loaded.
        if inspected.target_kind != "assign" {
            require(
                linked.is_empty() && omp_linked.is_empty(),
                "非分配应用意外关联编程执行",
            )?;
        }
        Ok(inspected)
    }

    pub(super) fn application_recovery_review(
        &self,
        operation: &str,
    ) -> Result<ApplicationRecoveryReview, String> {
        require(self.management.healthy(), "管理账本需要重开恢复")?;
        require(
            self.management.application_closure(operation).is_none(),
            "应用事务已经关闭",
        )?;
        let intent = self.management.intent(operation)?;
        let project = &intent.project_id;
        let statuses = self.management.statuses();
        require(
            statuses
                .iter()
                .any(|s| s.operation_id == operation && s.state == DecisionState::Applying),
            "请求不是待核对的应用事务",
        )?;
        #[cfg(windows)]
        require(
            !self.has_codex_activity(project)
                && !self.has_omp_activity(project)
                && self
                    .desktop_active
                    .as_ref()
                    .is_none_or(|a| a.snapshot.project.id != project.as_str())
                && !self.has_api_activity(project)
                && !self.active.contains_key(project),
            "执行线程仍持有项目，不能关闭应用",
        )?;
        require(
            self.runs.healthy() && self.runner_error.is_none(),
            "项目运行控制需要重开恢复",
        )?;
        let runs: Vec<_> = self
            .runs
            .all()
            .into_iter()
            .filter(|r| r.project_id == *project)
            .collect();
        require(
            !runs
                .iter()
                .any(|r| r.state == project_setup::RunState::Active),
            "请先暂停项目调度，再关闭应用事务",
        )?;
        require(
            !self.management_active.keys().any(|id| {
                self.management
                    .intent(id)
                    .is_ok_and(|i| i.project_id == *project)
            }),
            "项目指挥请求仍在运行",
        )?;
        require(
            !statuses.iter().any(|s| {
                s.project_id == *project && s.operation_id != operation && !s.state.resolved()
            }),
            "项目存在另一条待核对管理请求",
        )?;
        require(
            !self.jobs.values().any(|j| {
                j.project_id == *project
                    && matches!(
                        j.state,
                        VerificationState::Running
                            | VerificationState::Cancelling
                            | VerificationState::Unknown
                    )
            }),
            "项目存在待对账验收",
        )?;
        let store = self.projects.get(project).ok_or("项目账本不可用")?;
        // Job files are projections and may be absent. The workflow itself can
        // still hold uncertain execution; this is a live submission guard only.
        require(
            !store.state().tasks.values().any(|task| {
                matches!(
                    task.state,
                    TaskState::Dispatching
                        | TaskState::Running
                        | TaskState::Cancelling
                        | TaskState::Unknown
                )
            }),
            "项目存在执行中或待对账任务，不能关闭应用事务",
        )?;
        let sequence = u32::try_from(store.history().len()).map_err(|_| "工作流历史超限")?;
        let hash = store.history().last().map(|e| e.hash.as_str());
        let inspected = self.inspect_unapplied_application(operation, sequence, hash)?;
        require(
            !self.codex_jobs.iter().any(|(key, job)| {
                job.project_id == *project
                    && matches!(
                        job.state,
                        CodexJobState::Starting
                            | CodexJobState::Executing
                            | CodexJobState::Reconciling
                            | CodexJobState::Unknown
                    )
                    && inspected.job_key.as_ref() != Some(key)
            }),
            "项目存在另一条待对账编程尝试",
        )?;
        let job = inspected
            .job_key
            .as_ref()
            .and_then(|key| self.codex_jobs.get(key));
        if let Some(job) = job {
            require(
                matches!(job.state, CodexJobState::Unknown | CodexJobState::Failed),
                "编程投影与未应用准备不符，请重开核对",
            )?;
        }
        require(
            !self.omp_jobs.iter().any(|(key, job)| {
                job.project_id == *project
                    && matches!(
                        job.state,
                        OmpJobState::Starting
                            | OmpJobState::Executing
                            | OmpJobState::CancelRequested
                            | OmpJobState::Unknown
                            | OmpJobState::Failed
                    )
                    && inspected.omp_job_key.as_ref() != Some(key)
            }),
            "项目存在另一条待对账OMP尝试",
        )?;
        let omp_job = inspected
            .omp_job_key
            .as_ref()
            .and_then(|key| self.omp_jobs.get(key));
        if let Some(job) = omp_job {
            require(
                matches!(job.state, OmpJobState::Unknown | OmpJobState::Failed),
                "OMP投影与未应用准备不符，请重开核对",
            )?;
        }
        let heads = statuses
            .iter()
            .filter(|s| s.project_id == *project)
            .map(|s| {
                self.management
                    .evidence_digest(&s.operation_id)
                    .map(|head| (s.operation_id.as_str(), head))
            })
            .collect::<Result<Vec<_>, _>>()?;
        #[cfg(windows)]
        require(
            self.api_fault.is_none()
                && !self.api_jobs.iter().any(|(key, job)| {
                    job.project_id == *project
                        && job.state == api_jobs::ApiJobState::Unknown
                        && inspected.api_job_key.as_ref() != Some(key)
                }),
            "项目存在另一待核对API尝试或API账本故障",
        )?;
        let evidence_digest = digest(&serde_json::json!({
            "purpose":"abandon-application-v1", "operation":operation, "project":project,
            "management":heads, "workflowSequence":sequence, "workflowHash":hash,
            "target":self.management.application(operation).map(|a| a.0), "job":job, "ompJob":omp_job, "runs":runs,
        }))?;
        Ok(ApplicationRecoveryReview {
            evidence_digest,
            target_kind: inspected.target_kind,
            task_id: inspected.task_id,
            attempt_id: inspected.attempt_id,
            session_id: inspected.session_id,
            workflow_sequence: sequence,
        })
    }

    pub(super) fn abandon_management_application(
        &mut self,
        operation: &str,
        expected: &str,
        reason: &str,
    ) -> Result<(), String> {
        sha256(expected)?;
        require(
            !reason.trim().is_empty() && reason.len() <= 8000 && !reason.contains('\0'),
            "请输入不超过8000字节的关闭原因",
        )?;
        let review = self.application_recovery_review(operation)?;
        require(
            review.evidence_digest == expected,
            "应用证据已变化，请刷新详情后重新审阅",
        )?;
        let project = &self.management.intent(operation)?.project_id;
        let workflow_hash = self.projects[project]
            .history()
            .last()
            .map(|e| e.hash.clone());
        let head = self.management.evidence_digest(operation)?.to_owned();
        self.management.resolve(
            operation,
            &head,
            UserResolution::AbandonApplication {
                reason: reason.into(),
                workflow_sequence: review.workflow_sequence,
                workflow_hash,
            },
        )?;
        self.planning_errors.remove(operation);
        self.repair_closed_application_job(operation)
    }

    pub(super) fn validate_application_closures(&self) -> Result<(), String> {
        require(self.management.healthy(), "管理账本需要重开恢复")?;
        for status in self.management.statuses() {
            if let Some(closure) = self.management.application_closure(&status.operation_id) {
                require(
                    !closure.reason.trim().is_empty()
                        && closure.reason.len() <= 8000
                        && !closure.reason.contains('\0'),
                    "应用关闭原因无效",
                )?;
                self.inspect_unapplied_application(
                    &status.operation_id,
                    closure.workflow_sequence,
                    closure.workflow_hash.as_deref(),
                )?;
            }
        }
        Ok(())
    }

    fn repair_closed_application_job(&mut self, operation: &str) -> Result<(), String> {
        let closure = self
            .management
            .application_closure(operation)
            .ok_or("应用关闭证据缺失")?;
        let inspected = self.inspect_unapplied_application(
            operation,
            closure.workflow_sequence,
            closure.workflow_hash.as_deref(),
        )?;
        #[cfg(windows)]
        if let Some(key) = inspected.api_job_key {
            let job = self.api_jobs.get_mut(&key).ok_or("关联API记录消失")?;
            job.state = api_jobs::ApiJobState::Abandoned;
            job.closure_digest = None;
            job.message = Some("用户已关闭尚未分配的应用准备；没有重新发送任务".into());
        }
        if let Some(key) = inspected.job_key {
            let job = self.codex_jobs.get_mut(&key).ok_or("关联编程记录消失")?;
            job.state = CodexJobState::Abandoned;
            job.message =
                Some("用户已关闭尚未写入工作流的应用准备；原意图保留，未重新发送任务".into());
            // Preserve timestamps: this is a projection of the immutable .4, not a new execution.
            #[cfg(windows)]
            crate::async_codex::save(&self.root, job)?;
        }
        if let Some(key) = inspected.omp_job_key {
            let job = self.omp_jobs.get_mut(&key).ok_or("关联OMP记录消失")?;
            job.state = OmpJobState::Abandoned;
            job.message =
                Some("用户已关闭尚未写入工作流的应用准备；原意图保留，未重新发送任务".into());
            job.closure_digest = None;
        }
        Ok(())
    }

    pub(super) fn repair_closed_application_jobs(&mut self) -> Result<(), String> {
        self.validate_application_closures()?;
        for status in self.management.statuses() {
            if self
                .management
                .application_closure(&status.operation_id)
                .is_some()
            {
                self.repair_closed_application_job(&status.operation_id)?;
            }
        }
        Ok(())
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::management_execution::DecisionIntent;
    use cerulean_domain::management::{DecisionEnvelope, ManagementDecision, Purpose};

    fn configured(root: &Path) -> RuntimeHost {
        let (mut host, mut draft) = super::super::project_planning::tests::fixture(root);
        draft.management_max_requests = 16;
        host.configure_project(draft).unwrap();
        host
    }
    fn approved(root: &Path) -> RuntimeHost {
        let mut host = configured(root);
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
        host
    }
    fn prepare(
        host: &mut RuntimeHost,
        operation: &str,
        purpose: Purpose,
        decision: ManagementDecision,
        target: ApplicationTarget,
    ) {
        let context = host
            .management_context("project", "Implement a sum function")
            .unwrap();
        let state_digest = digest(&context).unwrap();
        host.management
            .begin(DecisionIntent {
                operation_id: operation.into(),
                project_id: "project".into(),
                purpose,
                model: "fixture-management".into(),
                state_digest: state_digest.clone(),
                context,
                created_at: chrono::Utc::now().to_rfc3339(),
            })
            .unwrap();
        host.management
            .finish(
                operation,
                Ok(cerulean_adapters::openai::DecisionObservation {
                    response_id: format!("resp_{}", operation.replace('-', "_")),
                    requested_model: "fixture-management".into(),
                    actual_model: "fixture-management".into(),
                    input_tokens: 0,
                    output_tokens: 0,
                    envelope: DecisionEnvelope {
                        state_digest: state_digest.clone(),
                        decision,
                    },
                }),
            )
            .unwrap();
        host.management
            .prepare_application(operation, target, &state_digest)
            .unwrap();
    }
    fn assignment(host: &RuntimeHost, id: &str) -> ApplicationTarget {
        let store = &host.projects["project"];
        let revision = store.revision().unwrap();
        let attempt = Attempt {
            workspace_proof: None,
            id: id.into(),
            task_id: "task".into(),
            worker_id: "worker-codex".into(),
            revision_digest: revision.approval_digest().unwrap(),
            fence: store.state().tasks["task"].fence + 1,
            binding: SessionBinding {
                adapter_id: "codex".into(),
                adapter_version: "fixture".into(),
                session_id: "fixture-prepared-session".into(),
                identity_strength: IdentityStrength::ProviderId,
                workspace: revision.project.root.clone(),
            },
        };
        ApplicationTarget::Workflow {
            principal: "deputy".into(),
            command: WorkflowCommand::Assign {
                task: "task".into(),
                worker: "worker-codex".into(),
                attempt: id.into(),
            },
            context: Box::new(CommandContext {
                execution: Some(ExecutionEvidence::Attempt { attempt }),
                ..Default::default()
            }),
        }
    }
    fn assign_fixture(root: &Path, with_job: bool) -> RuntimeHost {
        let mut host = approved(root);
        let target = assignment(&host, "prepared-native");
        prepare(
            &mut host,
            "application-fixture",
            Purpose::Assign,
            ManagementDecision::Assign {
                task_id: "task".into(),
                worker_id: "worker-codex".into(),
                rationale: "isolated preparation fixture".into(),
            },
            target,
        );
        if with_job {
            let revision = host.projects["project"].revision().unwrap();
            let now = chrono::Utc::now().to_rfc3339();
            let job = CodexJob {
                management_operation: Some("application-fixture".into()),
                project_id: "project".into(),
                task_id: "task".into(),
                attempt_id: "prepared-native".into(),
                revision_digest: revision.approval_digest().unwrap(),
                worker: revision.task("task").unwrap().worker.clone().unwrap(),
                state: CodexJobState::Unknown,
                started_at: now.clone(),
                updated_at: now,
                message: Some("Synthetic prepare acknowledgement loss".into()),
            };
            crate::async_codex::save(&host.root, &job).unwrap();
            host.codex_jobs
                .insert(("project".into(), "prepared-native".into()), job);
        }
        host
    }
    fn close_operation(host: &mut RuntimeHost, operation: &str) {
        let review = host.application_recovery_review(operation).unwrap();
        host.abandon_management_application(
            operation,
            &review.evidence_digest,
            "用户确认关闭原应用准备，稍后使用新请求继续",
        )
        .unwrap();
    }
    fn close(host: &mut RuntimeHost) {
        close_operation(host, "application-fixture");
    }

    #[test]
    fn assignment_closure_with_or_without_job_preserves_intent_and_survives_new_attempt() {
        for with_job in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let mut host = assign_fixture(root.path(), with_job);
            let path = host
                .root
                .join("management-decisions/application-fixture.2.json");
            let original = fs::read(&path).unwrap();
            let events = host.projects["project"].history().to_vec();
            let review = host
                .application_recovery_review("application-fixture")
                .unwrap();
            assert_eq!(review.target_kind, "assign");
            assert_eq!(
                review.session_id.as_deref(),
                Some("fixture-prepared-session")
            );
            assert!(host
                .abandon_management_application("application-fixture", &"a".repeat(64), "stale")
                .is_err());
            close(&mut host);
            assert_eq!(host.projects["project"].history(), events);
            assert_eq!(fs::read(path).unwrap(), original);
            assert!(!host
                .root
                .join("management-decisions/application-fixture.3.json")
                .exists());
            assert_eq!(
                host.management
                    .statuses()
                    .into_iter()
                    .find(|s| s.operation_id == "application-fixture")
                    .unwrap()
                    .state,
                DecisionState::Abandoned
            );
            assert!(host
                .abandon_management_application(
                    "application-fixture",
                    &review.evidence_digest,
                    "duplicate"
                )
                .is_err());
            assert_eq!(host.codex_jobs.len(), usize::from(with_job));
            assert!(host
                .codex_jobs
                .values()
                .all(|j| j.state == CodexJobState::Abandoned));
            let target = assignment(&host, "fresh-attempt");
            prepare(
                &mut host,
                "fresh-application",
                Purpose::Assign,
                ManagementDecision::Assign {
                    task_id: "task".into(),
                    worker_id: "worker-codex".into(),
                    rationale: "new application".into(),
                },
                target.clone(),
            );
            host.apply_management_decision_inner("fresh-application", target)
                .unwrap();
            assert_eq!(host.projects["project"].state().tasks["task"].fence, 1);
            drop(host);
            let host = RuntimeHost::open(&root.path().join("data")).unwrap();
            host.validate_application_closures().unwrap();
            assert_eq!(
                host.projects["project"].state().tasks["task"].state,
                TaskState::Assigned
            );
            assert_eq!(
                host.projects["project"].state().tasks["task"]
                    .attempt
                    .as_deref(),
                Some("fresh-attempt")
            );
            assert!(host
                .codex_jobs
                .values()
                .all(|j| j.state == CodexJobState::Abandoned));
        }
    }

    #[test]
    fn published_closure_repairs_job_after_projection_write_failure() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = tempfile::tempdir().unwrap();
        let mut host = assign_fixture(root.path(), true);
        let path = host.root.join("codex-jobs/project/prepared-native.json");
        let held = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        let review = host
            .application_recovery_review("application-fixture")
            .unwrap();
        assert!(host
            .abandon_management_application(
                "application-fixture",
                &review.evidence_digest,
                "projection failure fixture"
            )
            .is_err());
        assert!(host
            .management
            .application_closure("application-fixture")
            .is_some());
        drop(held);
        drop(host);
        let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(
            host.codex_jobs.values().next().unwrap().state,
            CodexJobState::Abandoned
        );
        let count = host.management.statuses().len();
        host.poll_verifications();
        assert_eq!(host.management.statuses().len(), count);
        assert_eq!(host.projects["project"].history().len(), 2);
        assert!(host.codex_active.is_empty() && host.management_active.is_empty());
    }

    #[test]
    fn local_targets_can_close_even_when_command_is_inapplicable_and_wait_is_excluded() {
        for kind in ["propose", "accept", "advise", "wait"] {
            let root = tempfile::tempdir().unwrap();
            let mut host = if kind == "propose" {
                configured(root.path())
            } else {
                approved(root.path())
            };
            let (purpose, decision, target) = match kind {
                "propose" => {
                    let setup = host.project_setups["project"].setup.clone();
                    let project = host.projects["project"]
                        .configuration()
                        .execution_scope
                        .as_ref()
                        .unwrap()
                        .project
                        .clone();
                    let workflow = WorkflowPlan {
                        id: "fixture-plan".into(),
                        version: 1,
                        tasks: vec![WorkflowTask {
                            id: "task".into(),
                            goal: "Implement a sum function".into(),
                            dependencies: vec![],
                            workspace_key: "project".into(),
                            resource_keys: None,
                        }],
                    };
                    let revision = PlanRevision {
                        management_setup_digest: None,
                        worktree: None,
                        schema_version: 1,
                        project,
                        workflow: workflow.clone(),
                        execution: vec![TaskExecution {
                            supervision_override: None,
                            profile_id: None,
                            desktop_worker: None,
                            api_worker: None,
                            task_id: "task".into(),
                            adapter_id: "codex".into(),
                            worker: setup.worker,
                            artifact_paths: setup.artifact_paths,
                            checks: vec![setup.check],
                        }],
                        policy: setup.policy,
                        snapshot_policy: Some(setup.snapshot_policy),
                    };
                    (
                        Purpose::Plan,
                        super::super::project_planning::tests::proposal("tests"),
                        ApplicationTarget::Workflow {
                            principal: "chief".into(),
                            command: WorkflowCommand::Propose { plan: workflow },
                            context: Box::new(CommandContext {
                                execution: Some(ExecutionEvidence::Revision { revision }),
                                ..Default::default()
                            }),
                        },
                    )
                }
                "accept" => (
                    Purpose::Accept,
                    ManagementDecision::Accept {
                        task_id: "task".into(),
                        attempt_id: "unverified".into(),
                        rationale: "inapplicable decision".into(),
                    },
                    ApplicationTarget::Workflow {
                        principal: "chief".into(),
                        command: WorkflowCommand::Accept {
                            task: "task".into(),
                            attempt: "unverified".into(),
                        },
                        context: Box::default(),
                    },
                ),
                "advise" => (
                    Purpose::Advise,
                    ManagementDecision::Advise {
                        task_id: "missing-task".into(),
                        suggestion: "inspect evidence".into(),
                        rationale: "stale suggestion".into(),
                    },
                    ApplicationTarget::Workflow {
                        principal: "deputy".into(),
                        command: WorkflowCommand::Advise {
                            task: "missing-task".into(),
                            suggestion: "inspect evidence".into(),
                        },
                        context: Box::default(),
                    },
                ),
                _ => (
                    Purpose::Assign,
                    ManagementDecision::Wait {
                        reason: "need user input".into(),
                    },
                    ApplicationTarget::Wait,
                ),
            };
            let operation = if kind == "propose" {
                "ui-plan-application-fixture"
            } else {
                "application-fixture"
            };
            prepare(&mut host, operation, purpose, decision, target);
            let original = host.projects["project"].history().to_vec();
            if kind == "propose" {
                host.poll_verifications();
                assert_eq!(host.projects["project"].history(), original);
                drop(host);
                host = RuntimeHost::open(&root.path().join("data")).unwrap();
                host.poll_verifications();
                assert_eq!(host.projects["project"].history(), original);
            }
            if kind == "wait" {
                assert!(host.application_recovery_review(operation).is_err());
            } else {
                assert_eq!(
                    host.application_recovery_review(operation)
                        .unwrap()
                        .target_kind,
                    kind
                );
                close_operation(&mut host, operation);
                assert_eq!(host.projects["project"].history(), original);
                drop(host);
                let host = RuntimeHost::open(&root.path().join("data")).unwrap();
                host.validate_application_closures().unwrap();
            }
        }
    }

    #[test]
    fn recovery_rejects_ownership_job_mismatch_and_historical_attempt_collision() {
        let root = tempfile::tempdir().unwrap();
        let mut host = assign_fixture(root.path(), true);
        let review = host
            .application_recovery_review("application-fixture")
            .unwrap();
        let key = ("project".into(), "prepared-native".into());
        let job = host.codex_jobs[&key].clone();
        host.codex_jobs.get_mut(&key).unwrap().revision_digest = "a".repeat(64);
        assert!(host
            .application_recovery_review("application-fixture")
            .is_err());
        host.codex_jobs.insert(key, job.clone());
        let (_send, receiver) = std::sync::mpsc::channel();
        let snapshot = host.snapshot().workflows[0].clone();
        host.codex_active.insert(
            (job.project_id.clone(), job.attempt_id.clone()),
            crate::async_codex::ActiveCodex {
                preparation_cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                advice_wake: tokio::sync::watch::channel(()).0,
                advice_gate: std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0)),
                observation: None,
                observation_saved: None,
                observation_error: None,
                snapshot,
                job,
                receiver,
                disconnected: true,
                cancel: None,
            },
        );
        assert!(host
            .abandon_management_application(
                "application-fixture",
                &review.evidence_digest,
                "blocked"
            )
            .is_err());
        host.codex_active
            .remove(&("project".into(), "prepared-native".into()));
        let ApplicationTarget::Workflow {
            principal,
            command,
            context,
        } = assignment(&host, "prepared-native")
        else {
            unreachable!()
        };
        host.projects
            .get_mut("project")
            .unwrap()
            .command("unexpected-assignment", &principal, command, *context, true)
            .unwrap();
        assert!(host
            .application_recovery_review("application-fixture")
            .is_err());
    }

    #[test]
    fn closure_anchor_is_a_real_prefix_and_not_a_future_or_forged_head() {
        for sequence in [1, 3] {
            let root = tempfile::tempdir().unwrap();
            let mut host = assign_fixture(root.path(), false);
            let head = host
                .management
                .evidence_digest("application-fixture")
                .unwrap()
                .to_owned();
            host.management
                .resolve(
                    "application-fixture",
                    &head,
                    UserResolution::AbandonApplication {
                        reason: "forged anchor fixture".into(),
                        workflow_sequence: sequence,
                        workflow_hash: Some("a".repeat(64)),
                    },
                )
                .unwrap();
            assert!(host.validate_application_closures().is_err());
            drop(host);
            assert!(RuntimeHost::open(&root.path().join("data")).is_err());
        }
    }

    #[test]
    fn workflow_execution_without_job_blocks_live_close_but_not_historical_replay() {
        for wanted in [
            TaskState::Dispatching,
            TaskState::Running,
            TaskState::Cancelling,
            TaskState::Unknown,
        ] {
            let root = tempfile::tempdir().unwrap();
            let mut host = approved(root.path());
            for operation in ["closed-earlier", "application-fixture"] {
                prepare(
                    &mut host,
                    operation,
                    Purpose::Advise,
                    ManagementDecision::Advise {
                        task_id: "missing-task".into(),
                        suggestion: "inspect original evidence".into(),
                        rationale: "local stale advice fixture".into(),
                    },
                    ApplicationTarget::Workflow {
                        principal: "deputy".into(),
                        command: WorkflowCommand::Advise {
                            task: "missing-task".into(),
                            suggestion: "inspect original evidence".into(),
                        },
                        context: Box::default(),
                    },
                );
                if operation == "closed-earlier" {
                    close_operation(&mut host, operation);
                }
            }
            let review = host
                .application_recovery_review("application-fixture")
                .unwrap();
            let management_head = host
                .management
                .evidence_digest("application-fixture")
                .unwrap()
                .to_owned();
            let ApplicationTarget::Workflow {
                principal,
                command,
                context,
            } = assignment(&host, "unrelated-attempt")
            else {
                unreachable!()
            };
            host.projects
                .get_mut("project")
                .unwrap()
                .command("other-assignment", &principal, command, *context, true)
                .unwrap();
            assert!(
                host.application_recovery_review("application-fixture")
                    .is_ok(),
                "Assigned retains its existing disposition path"
            );
            let store = host.projects.get_mut("project").unwrap();
            let attempt = store
                .attempt_record("task", "unrelated-attempt")
                .unwrap()
                .clone();
            store
                .command(
                    "other-dispatch",
                    "core",
                    WorkflowCommand::DispatchIntent {
                        task: "task".into(),
                        attempt: attempt.id.clone(),
                    },
                    CommandContext {
                        thread_id: Some(attempt.binding.session_id.clone()),
                        cwd: Some(attempt.binding.workspace.clone()),
                        ..Default::default()
                    },
                    true,
                )
                .unwrap();
            if matches!(wanted, TaskState::Running | TaskState::Cancelling) {
                let receipt = ExecutionReceipt {
                    schema_version: 1,
                    task_id: "task".into(),
                    attempt_id: attempt.id.clone(),
                    fence: attempt.fence,
                    binding: attempt.binding.clone(),
                    turn_id: Some("fixture-turn".into()),
                    observed_at: chrono::Utc::now().to_rfc3339(),
                    status: ReceiptStatus::Running,
                    evidence_sha256: "a".repeat(64),
                };
                store
                    .command(
                        "other-start",
                        "core",
                        WorkflowCommand::Started {
                            task: "task".into(),
                            attempt: attempt.id.clone(),
                        },
                        CommandContext {
                            thread_id: Some(attempt.binding.session_id.clone()),
                            turn_id: receipt.turn_id.clone(),
                            execution: Some(ExecutionEvidence::Receipt {
                                receipt,
                                manifest: None,
                            }),
                            ..Default::default()
                        },
                        true,
                    )
                    .unwrap();
            }
            if wanted == TaskState::Cancelling {
                store
                    .command(
                        "other-cancel",
                        "core",
                        WorkflowCommand::CancelIntent {
                            task: "task".into(),
                            attempt: attempt.id.clone(),
                            reason: "synthetic cancellation intent".into(),
                        },
                        CommandContext {
                            thread_id: Some(attempt.binding.session_id.clone()),
                            turn_id: Some("fixture-turn".into()),
                            cancellation_source: Some("synthetic user request".into()),
                            ..Default::default()
                        },
                        true,
                    )
                    .unwrap();
            } else if wanted == TaskState::Unknown {
                store
                    .command(
                        "other-unknown",
                        "core",
                        WorkflowCommand::RecoveryUnknown {
                            task: "task".into(),
                            attempt: attempt.id,
                        },
                        CommandContext::default(),
                        true,
                    )
                    .unwrap();
            }
            assert_eq!(store.state().tasks["task"].state, wanted);
            let history = store.history().to_vec();
            assert!(host.codex_jobs.is_empty() && host.codex_active.is_empty());
            let expected_error = "项目存在执行中或待对账任务";
            assert!(host
                .application_recovery_review("application-fixture")
                .unwrap_err()
                .contains(expected_error));
            assert!(host
                .abandon_management_application(
                    "application-fixture",
                    &review.evidence_digest,
                    "must reject"
                )
                .unwrap_err()
                .contains(expected_error));
            assert!(!host
                .root
                .join("management-decisions/application-fixture.4.json")
                .exists());
            assert_eq!(
                host.management
                    .evidence_digest("application-fixture")
                    .unwrap(),
                management_head
            );
            assert_eq!(host.projects["project"].history(), history);
            host.validate_application_closures().unwrap();
        }
    }

    #[test]
    #[ignore = "isolated native application recovery fixture; no real provider calls"]
    fn seed_native_application_recovery_fixture() {
        let root = PathBuf::from(
            std::env::var_os("CERULEAN_APPLICATION_NATIVE_DIR").expect("fixture directory"),
        );
        assert!(root.is_absolute() && !root.join("data").exists());
        let root = root.canonicalize().unwrap();
        let reports = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../reports")
            .canonicalize()
            .unwrap();
        assert!(root.starts_with(reports));
        let with_job = std::env::var("CERULEAN_APPLICATION_NO_JOB").as_deref() != Ok("1");
        let host = assign_fixture(&root, with_job);
        let review = host
            .application_recovery_review("application-fixture")
            .unwrap();
        assert_eq!(review.workflow_sequence, 2);
        fs::write(root.join("fixture.json"), serde_json::to_vec_pretty(&serde_json::json!({
            "operation":"application-fixture", "project":"project", "task":"task", "attempt":"prepared-native",
            "session":"fixture-prepared-session", "workflowSequence":2, "taskState":"Pending", "fence":0,
            "withJob":with_job, "realModelCalls":0,
        })).unwrap()).unwrap();
    }
}
