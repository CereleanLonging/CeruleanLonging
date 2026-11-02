use super::*;
use crate::management_execution::{ApplicationTarget, DecisionState};
use cerulean_domain::management::{ManagementDecision, Purpose};
use cerulean_domain::project_setup::*;

impl RuntimeHost {
    pub(super) fn planning_idle(&self, project: &str) -> Result<(), String> {
        self.require_available(project)?;
        let store = self.projects.get(project).ok_or("项目尚未登记")?;
        let current = store
            .revision()
            .map(PlanRevision::approval_digest)
            .transpose()?;
        let revising = store.state().revision_requested.is_some();
        require(
            !self.runs.all().iter().any(|r| {
                r.project_id == project
                    && !r.state.terminal()
                    && current.as_ref() == Some(&r.revision_digest)
                    && !(revising && r.state == RunState::Paused)
            }),
            "项目已有未完成运行，请先完成或处置",
        )?;
        require(
            !store.state().tasks.values().any(|t| {
                matches!(
                    t.state,
                    TaskState::Accepted
                        | TaskState::Cancelled
                        | TaskState::Retired
                        | TaskState::Submitted
                )
            }) || revising,
            "请先明确请求修订下一版方案",
        )?;
        require(
            store.state().tasks.values().all(|t| {
                (revising && t.state == TaskState::Submitted)
                    || matches!(
                        t.state,
                        TaskState::Pending
                            | TaskState::Accepted
                            | TaskState::Cancelled
                            | TaskState::Retired
                    )
            }),
            "请先完成或处置当前任务",
        )?;
        require(
            store.approval().is_none()
                || revising
                || store.state().tasks.values().all(|t| {
                    matches!(
                        t.state,
                        TaskState::Accepted | TaskState::Cancelled | TaskState::Retired
                    )
                }),
            "已有批准的待执行任务，请先完成或处置",
        )?;
        require(
            !self
                .management
                .statuses()
                .iter()
                .any(|s| s.project_id == project && !s.state.resolved()),
            "项目已有待处理的指挥请求",
        )
    }
    pub(super) fn validate_project_setups(&self) -> Result<(), String> {
        for saved in self.project_setups.values() {
            let binding = self
                .projects
                .get(&saved.setup.project_id)
                .ok_or("project setup has no registered project")?
                .configuration();
            require_bound_worker(binding, saved.setup.worker_adapter.id())?;
            if saved.setup.chief_directory.is_some() {
                crate::worker_directories::verify_saved(
                    &self.root,
                    &binding
                        .execution_scope
                        .as_ref()
                        .ok_or("项目范围缺失")?
                        .project,
                    &saved.setup.worker_directories,
                )?;
            }
            for profile in &saved.setup.profiles {
                require_bound_worker(binding, profile.worker_adapter.id())?;
                #[cfg(windows)]
                if let Some(desktop) = &profile.desktop_worker {
                    self.validate_desktop_selection(&saved.setup.project_id, desktop)?;
                }
                for stage in &profile.acceptance.stages {
                    let validator = self.registered_validator(&stage.validator_id)?;
                    require(
                        stage.check.tool.as_ref() == Some(&validator.tool),
                        "执行配置检查与登记不符",
                    )?;
                }
            }
            if let Some(integration) = saved.setup.integration() {
                let validator = self.registered_validator(&integration.validator_id)?;
                require(
                    integration.check.tool.as_ref() == Some(&validator.tool),
                    "整体检查工具与登记不符",
                )?;
            }
            #[cfg(windows)]
            if let Some(desktop) = &saved.setup.desktop_worker {
                self.validate_desktop_selection(&saved.setup.project_id, desktop)?;
            }
            let validator = self.registered_validator(&saved.setup.validator_id)?;
            require(
                saved.setup.check.tool.as_ref() == Some(&validator.tool),
                "configured validator differs from registration",
            )?;
            if let Some(acceptance) = &saved.setup.acceptance {
                for stage in &acceptance.stages {
                    let validator = self.registered_validator(&stage.validator_id)?;
                    require(
                        stage.check.tool.as_ref() == Some(&validator.tool),
                        "configured stage validator differs from registration",
                    )?;
                }
            }
        }
        Ok(())
    }
    pub(super) fn configure_project(&mut self, draft: ProjectSetupDraft) -> Result<(), String> {
        let binding = self
            .projects
            .get(&draft.project_id)
            .ok_or("项目尚未登记")?
            .configuration();
        for adapter in std::iter::once(&draft.worker_adapter)
            .chain(draft.profiles.iter().map(|p| &p.worker_adapter))
        {
            require_bound_worker(binding, adapter.id())?;
        }
        crate::relay_guard::require_protected(
            &self.root,
            draft
                .management_sources
                .as_ref()
                .is_some_and(|s| s.uses_provider_selection())
                || draft.worker_adapter.id() == "a6api"
                || draft
                    .profiles
                    .iter()
                    .any(|p| p.worker_adapter.id() == "a6api"),
        )?;
        crate::reader_generation::require_override(
            &self.root,
            draft
                .profiles
                .iter()
                .any(|p| p.supervision_override.is_some()),
        )?;
        draft.validate()?;
        crate::management_source_guard::require_protected(
            &self.root,
            draft.management_sources.is_some(),
        )?;
        self.planning_idle(&draft.project_id)?;
        if let Some(old) = self
            .project_setups
            .get(&draft.project_id)
            .filter(|s| s.setup.chief_directory.is_some())
        {
            let requested = draft
                .chief_directory
                .as_deref()
                .ok_or("已启用的角色隔离不能从执行配置中移除")?;
            require(
                crate::artifacts::root_path(requested)?
                    == crate::artifacts::root_path(old.setup.chief_directory.as_deref().unwrap())?,
                "已登记的主指挥目录不能通过模型切换迁移",
            )?;
        }
        let mut setup = self.prepare_project_setup(draft)?;
        if setup.chief_directory.is_some() {
            require(
                setup.worktree.is_none(),
                "固定模型工作目录与Git按尝试目录池不能同时启用",
            )?;
            let project = &self.projects[&setup.project_id]
                .configuration()
                .execution_scope
                .as_ref()
                .ok_or("项目范围缺失")?
                .project;
            let mut selected = vec![setup.execution_for("workspace-primary", None)?];
            for profile in &setup.profiles {
                selected.push(setup.execution_for("workspace-profile", Some(&profile.id))?);
            }
            setup.worker_directories =
                crate::worker_directories::register(&self.root, project, &selected)?;
        }
        let saved = SavedProjectSetup {
            digest: digest(&setup)?,
            setup,
        };
        crate::role_workspace::save_requirements(&saved)?;
        crate::project_setup::save(&self.root, &saved)?;
        self.project_setups
            .insert(saved.setup.project_id.clone(), saved);
        Ok(())
    }
    fn prepare_project_setup(&self, draft: ProjectSetupDraft) -> Result<ProjectSetup, String> {
        draft.validate()?;
        #[cfg(windows)]
        {
            let profiles = draft
                .profiles
                .iter()
                .map(|p| {
                    WorkerProfile::from_setup(
                        p.id.clone(),
                        p.description.clone(),
                        self.prepare_project_setup(p.apply(&draft))?,
                        p.supervision_override.clone(),
                    )
                })
                .collect::<Result<Vec<_>, String>>()?;
            let has_desktop = draft.worker_adapter.is_desktop()
                || profiles.iter().any(|p| p.worker_adapter.is_desktop());
            if let Some(desktop) = &draft.desktop_worker {
                self.validate_desktop_selection(&draft.project_id, desktop)?;
            }
            let worker = match draft.worker_adapter {
                ProjectWorkerAdapter::Codex => Some(Self::describe_codex_worker(
                    Path::new(&draft.codex_executable),
                    &draft.worker_model,
                )?),
                ProjectWorkerAdapter::Omp => {
                    let executable = Path::new(&draft.codex_executable);
                    require(executable.is_absolute(), "OMP程序必须使用绝对路径")?;
                    let worker = WorkerSelection {
                        omp_runtime_parent: draft
                            .omp_runtime_parent
                            .as_deref()
                            .map(|p| self.validate_omp_runtime_parent(p))
                            .transpose()?,
                        provider: "deepseek".into(),
                        model: draft.worker_model.clone(),
                        executable: crate::verifier::PinnedCheck::describe(
                            executable,
                            &[],
                            Default::default(),
                        )?
                        .executable,
                    };
                    worker.validate()?;
                    let project = &self.projects[&draft.project_id]
                        .configuration()
                        .execution_scope
                        .as_ref()
                        .ok_or("项目范围缺失")?
                        .project;
                    require(
                        !crate::artifacts::contains_path(
                            Path::new(&project.root),
                            Path::new(&worker.executable.path),
                        ),
                        "OMP程序必须位于项目目录之外",
                    )?;
                    Some(worker)
                }
                _ => None,
            };
            let validator = self.registered_validator(&draft.validator_id)?;
            let acceptance = draft
                .acceptance
                .as_ref()
                .map(|a| {
                    let stages = a
                        .stages
                        .iter()
                        .map(|s| {
                            let validator = self.registered_validator(&s.validator_id)?;
                            Ok(AcceptanceStage {
                                validator_id: s.validator_id.clone(),
                                check: AcceptanceCheck {
                                    expected_assertions: s.expected_assertions.clone(),
                                    id: s.id.clone(),
                                    name: Some(s.name.clone()),
                                    program: validator.tool.executable.path.clone(),
                                    args: s.args.clone(),
                                    timeout_seconds: s.timeout_seconds,
                                    tool: Some(validator.tool.clone()),
                                },
                            })
                        })
                        .collect::<Result<Vec<_>, String>>()?;
                    Ok::<_, String>(AcceptanceSetup {
                        schema_version: a.schema_version,
                        stages,
                    })
                })
                .transpose()?;
            let check = acceptance.as_ref().map_or_else(
                || AcceptanceCheck {
                    expected_assertions: vec![],
                    id: "project-check".into(),
                    name: None,
                    program: validator.tool.executable.path.clone(),
                    args: draft.check_args.clone(),
                    timeout_seconds: draft.max_seconds.min(300),
                    tool: Some(validator.tool.clone()),
                },
                |a| a.stages[0].check.clone(),
            );
            let worktree = draft
                .worktree
                .as_ref()
                .map(|w| self.configure_worktree(w))
                .transpose()?;
            if let (Some(policy), Some(worker)) = (&worktree, &worker) {
                require(
                    !crate::artifacts::contains_path(
                        Path::new(&policy.pool_root),
                        Path::new(&worker.executable.path),
                    ),
                    "编程程序不能位于任务目录池内",
                )?;
            }
            let mut setup = ProjectSetup {
                worker_directories: Default::default(),
                chief_directory: self.prepare_chief_directory(&draft)?,
                management_sources: draft.management_sources,
                resource_keys: draft.resource_keys,
                profiles,
                worktree,
                desktop_worker: draft.desktop_worker,
                api_worker: draft.api_worker,
                worker_adapter: draft.worker_adapter,
                project_id: draft.project_id.clone(),
                version: self
                    .project_setups
                    .get(&draft.project_id)
                    .map_or(1, |s| s.setup.version + 1),
                requirement: draft.requirement,
                worker,
                chief_model: draft.chief_model,
                deputy_model: draft.deputy_model,
                validator_id: draft.validator_id,
                check,
                acceptance,
                artifact_paths: draft.artifact_paths,
                policy: ExecutionPolicy {
                    supervision: draft.supervision,
                    version: 1,
                    tools: vec![
                        ToolPermission::ReadFiles,
                        ToolPermission::WriteFiles,
                        ToolPermission::RunCommands,
                    ]
                    .into_iter()
                    .chain(has_desktop.then_some(ToolPermission::DesktopSend))
                    .collect(),
                    max_requests: draft.worker_max_requests,
                    max_seconds: draft.max_seconds,
                    currency_budget: draft.currency_budget,
                    budget_usd_cents: None,
                },
                snapshot_policy: SnapshotPolicy {
                    exclude: vec![".git".into(), "node_modules".into(), "target".into()],
                    max_files: 10000,
                    max_bytes: 64 * 1024 * 1024,
                },
                management_max_requests: draft.management_max_requests,
            };
            if setup.chief_directory.is_some() {
                setup
                    .snapshot_policy
                    .exclude
                    .extend([".cerulean-runtime".into(), ".cerulean-omp".into()]);
            }
            Ok(setup)
        }
        #[cfg(not(windows))]
        {
            Err("项目执行配置需要 Windows".into())
        }
    }
    pub(super) fn request_chief_plan(&mut self, project: &str, wanted: &str) -> Result<(), String> {
        self.require_management_planning_available(project)?;
        self.planning_idle(project)?;
        let saved = self
            .project_setups
            .get(project)
            .ok_or("请先保存项目需求和执行配置")?
            .clone();
        require(saved.digest == wanted, "配置已改变，请重新读取后发起规划")?;
        if let Some(ManagementSources {
            chief: ChiefSource::CodexSession { session_id },
            ..
        }) = &saved.setup.management_sources
        {
            self.prepare_external_chief_plan(project, wanted, session_id)?;
            return Ok(());
        }
        #[cfg(windows)]
        {
            let count = self.management_operation_ids(project).len();
            let operation = format!("ui-plan-{}-{}", &digest(&project)?[..32], count + 1);
            self.start_management_decision(
                project,
                &operation,
                Purpose::Plan,
                &saved.setup.chief_model,
                &saved.setup.requirement,
            )
        }
        #[cfg(not(windows))]
        {
            Err("主指挥调用需要 Windows 凭证".into())
        }
    }
    fn materialize_chief_plan(&mut self, operation: &str) -> Result<(), String> {
        let intent = self.management.intent(operation)?.clone();
        let saved = self
            .project_setups
            .get(&intent.project_id)
            .ok_or("project setup missing")?
            .clone();
        require(
            intent.context["executionSetup"]
                == serde_json::to_value(&saved).map_err(|e| e.to_string())?,
            "项目配置已改变，不能应用旧方案",
        )?;
        let decision = self.management_envelope(operation)?.decision.clone();
        let tasks = match decision {
            ManagementDecision::Wait { reason } => {
                self.apply_management_decision_inner(operation, ApplicationTarget::Wait)?;
                self.planning_errors
                    .insert(operation.into(), format!("主指挥需要补充信息：{reason}"));
                return Ok(());
            }
            ManagementDecision::Plan { tasks, .. } => tasks,
            _ => return Err("主指挥规划返回了不匹配的决定".into()),
        };
        let setup = saved.setup;
        require(
            setup.worktree.as_ref().is_none_or(|w| {
                w.input == GitWorktreeInput::CommittedRawBlobsWithAcceptedDependencies
            }) || tasks.iter().all(|t| t.dependencies.is_empty()),
            "隔离目录的依赖产物传递未获配置授权，请启用依赖输入后重新规划",
        )?;
        require(
            tasks.len() <= setup.policy.max_requests as usize,
            "方案任务数超过编程请求上限，请调整需求或配置",
        )?;
        for task in &tasks {
            let selected = setup.execution_for(&task.id, task.profile_id.as_deref())?;
            let binding = self
                .projects
                .get(&intent.project_id)
                .ok_or("project unavailable")?
                .configuration();
            require_bound_worker(binding, &selected.adapter_id)?;
            require(
                task.worker_id == format!("worker-{}", selected.adapter_id)
                    && task.validator_id
                        == setup.validator_for(&task.id, task.profile_id.as_deref())?,
                "主指挥方案使用了未配置的执行者或验收工具",
            )?;
        }
        let store = self
            .projects
            .get(&intent.project_id)
            .ok_or("project unavailable")?;
        let project = store
            .configuration()
            .execution_scope
            .as_ref()
            .ok_or("project scope missing")?
            .project
            .clone();
        let version = store.revision().map_or(Ok(1), |r| {
            r.workflow
                .version
                .checked_add(1)
                .ok_or("plan version exhausted")
        })?;
        let workflow = WorkflowPlan {
            id: format!("plan-{}", &digest(&project.id)?[..32]),
            version,
            tasks: tasks
                .iter()
                .map(|t| {
                    Ok(WorkflowTask {
                        id: t.id.clone(),
                        goal: t.goal.clone(),
                        dependencies: t.dependencies.clone(),
                        workspace_key: if setup.worktree.is_some() {
                            cerulean_domain::execution::isolated_task_key(&project.id, &t.id)?
                        } else {
                            project.id.clone()
                        },
                        resource_keys: setup.resources_for(t.profile_id.as_deref())?,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?,
        };
        let execution = tasks
            .iter()
            .map(|t| setup.execution_for(&t.id, t.profile_id.as_deref()))
            .collect::<Result<Vec<_>, String>>()?;
        let revision = PlanRevision {
            management_setup_digest: (setup.management_sources.is_some()
                || setup.chief_directory.is_some())
            .then_some(saved.digest),
            worktree: setup.worktree,
            schema_version: 1,
            project,
            workflow: workflow.clone(),
            execution,
            policy: setup.policy,
            snapshot_policy: Some(setup.snapshot_policy),
        };
        revision.validate()?;
        if let Some(chief) = &setup.chief_directory {
            crate::role_workspace::publish(
                chief,
                "plans",
                &revision.approval_digest()?,
                &serde_json::to_value(&revision).map_err(|e| e.to_string())?,
            )?;
        }
        self.apply_management_decision_inner(
            operation,
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
    pub(super) fn poll_planning_results(&mut self) {
        let ready: Vec<_> = self
            .management
            .statuses()
            .into_iter()
            .filter(|s| {
                s.operation_id.starts_with("ui-plan-")
                    && s.purpose == Purpose::Plan
                    // A recovered application without its event requires explicit disposition.
                    // Fresh Ready decisions still apply in the same trusted transaction.
                    && s.state == DecisionState::Ready
                    && !self.planning_errors.contains_key(&s.operation_id)
            })
            .map(|s| s.operation_id)
            .collect();
        for operation in ready {
            if let Err(error) = self.materialize_chief_plan(&operation) {
                self.planning_errors.insert(operation, error);
            }
        }
    }
}

#[cfg(all(test, windows))]
pub(super) mod tests {
    use super::*;
    use crate::management_execution::DecisionIntent;
    use cerulean_adapters::openai::DecisionObservation;
    use cerulean_domain::management::{DecisionEnvelope, ProposedTask};
    #[test]
    fn isolated_configuration_requires_assertions_reuses_models_and_survives_reopen() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, mut draft) = fixture(root.path());
        draft.chief_directory = Some(root.path().join("chief").to_string_lossy().into());
        assert!(host.configure_project(draft.clone()).is_err());
        draft.acceptance = Some(AcceptanceDraft {
            schema_version: 1,
            stages: vec![AcceptanceStageDraft {
                id: "tests".into(),
                name: "assertion protocol".into(),
                validator_id: "tests".into(),
                args: vec![],
                timeout_seconds: 10,
                expected_assertions: vec!["correct-sum".into()],
            }],
        });
        draft.check_args = draft.acceptance.as_ref().unwrap().stages[0].args.clone();
        host.configure_project(draft.clone()).unwrap();
        let first = host.project_setups["project"]
            .setup
            .execution_for("task", None)
            .unwrap();
        let first_dir = first.worker_directory.clone().unwrap();
        assert_eq!(first.checks[0].expected_assertions, vec!["correct-sum"]);
        let context = host
            .role_context(
                host.management_context("project", &draft.requirement)
                    .unwrap(),
                Purpose::Plan,
                true,
            )
            .unwrap();
        assert_eq!(context["roleWorkspace"]["deputyChiefAccess"], "read-only");
        assert!(context["roleWorkspace"].get("workerDirectory").is_none());
        assert_eq!(
            context["roleWorkspace"]["workerDirectories"][&first_dir.identity],
            first_dir
                .root(
                    &host.projects["project"]
                        .configuration()
                        .execution_scope
                        .as_ref()
                        .unwrap()
                        .project
                        .root
                )
                .unwrap()
        );
        let mut other = host.project_setups["project"].clone();
        other.setup.project_id = "other-project".into();
        host.project_setups.insert("other-project".into(), other);
        assert!(host
            .prepare_chief_directory(&draft)
            .unwrap_err()
            .contains("其他项目"));
        host.project_setups.remove("other-project");
        assert_eq!(
            context["roleWorkspace"]["material"]["requirement"],
            draft.requirement
        );
        draft.worker_model = "second-model".into();
        host.configure_project(draft.clone()).unwrap();
        assert_eq!(
            host.project_setups["project"]
                .setup
                .worker_directories
                .len(),
            2
        );
        draft.worker_model = "fixture-worker".into();
        host.configure_project(draft.clone()).unwrap();
        assert_eq!(
            host.project_setups["project"]
                .setup
                .execution_for("task", None)
                .unwrap()
                .worker_directory,
            Some(first_dir)
        );
        let saved = host.project_setups["project"].clone();
        let mut downgrade = draft.clone();
        downgrade.chief_directory = None;
        assert!(host.configure_project(downgrade).is_err());
        let chief = Path::new(saved.setup.chief_directory.as_deref().unwrap());
        let material = chief
            .join("requirements")
            .join(format!("{}.json", saved.digest));
        let original = fs::read(&material).unwrap();
        fs::write(&material, b"{}").unwrap();
        assert!(crate::role_workspace::read_requirements(
            &saved,
            &host.projects["project"]
                .configuration()
                .execution_scope
                .as_ref()
                .unwrap()
                .project
                .root
        )
        .is_err());
        fs::write(material, original).unwrap();
        drop(host);
        let host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(host.project_setups["project"].digest, saved.digest);
        let task = host.project_setups["project"]
            .setup
            .execution_for("task", None)
            .unwrap();
        assert!(crate::windows_worker_account::load_binding(
            &host.root,
            &host.projects["project"]
                .configuration()
                .execution_scope
                .as_ref()
                .unwrap()
                .project,
            task.worker_directory.as_ref().unwrap()
        )
        .is_err());
        assert!(host.codex_jobs.is_empty());
    }
    pub(in crate::host) fn fixture(root: &Path) -> (RuntimeHost, ProjectSetupDraft) {
        fs::create_dir(root.join("project")).unwrap();
        fixture_existing_empty_project(root)
    }
    pub(in crate::host) fn fixture_existing_empty_project(
        root: &Path,
    ) -> (RuntimeHost, ProjectSetupDraft) {
        fixture_at_existing_project(root, &root.join("project"))
    }
    pub(in crate::host) fn fixture_at_existing_project(
        root: &Path,
        project: &Path,
    ) -> (RuntimeHost, ProjectSetupDraft) {
        assert!(project.is_dir());
        assert!(fs::read_dir(project).unwrap().next().is_none());
        fs::write(
            root.join("codex.exe"),
            b"non-executable configuration fixture",
        )
        .unwrap();
        let mut host = RuntimeHost::open(&root.join("data")).unwrap();
        host.register(Project {
            id: "project".into(),
            root: project.to_string_lossy().into(),
            timezone: "Asia/Shanghai".into(),
        })
        .unwrap();
        host.register_validator(
            "tests".into(),
            "Fixture validator".into(),
            std::env::current_exe().unwrap().to_string_lossy().into(),
            vec![],
            Default::default(),
        )
        .unwrap();
        let draft = ProjectSetupDraft {
            chief_directory: None,
            management_sources: None,
            resource_keys: vec![],
            profiles: vec![],
            worktree: None,
            desktop_worker: None,
            api_worker: None,
            omp_runtime_parent: None,
            worker_adapter: Default::default(),
            supervision: None,
            project_id: "project".into(),
            requirement: "Implement a sum function".into(),
            codex_executable: root.join("codex.exe").to_string_lossy().into(),
            worker_model: "fixture-worker".into(),
            chief_model: "fixture-chief".into(),
            deputy_model: "fixture-deputy".into(),
            validator_id: "tests".into(),
            check_args: vec!["--exact".into(), "independent-fixture".into()],
            acceptance: None,
            artifact_paths: vec!["sum.mjs".into()],
            currency_budget: None,
            worker_max_requests: 2,
            management_max_requests: 1,
            max_seconds: 120,
        };
        (host, draft)
    }
    pub(in crate::host) fn complete(host: &mut RuntimeHost, decision: ManagementDecision) {
        complete_at(host, decision, "ui-plan-fixture-1");
    }
    pub(in crate::host) fn complete_at(
        host: &mut RuntimeHost,
        decision: ManagementDecision,
        operation: &str,
    ) {
        let context = host
            .management_context("project", &host.project_setups["project"].setup.requirement)
            .unwrap();
        let state_digest = digest(&context).unwrap();
        host.management
            .begin(DecisionIntent {
                operation_id: operation.into(),
                project_id: "project".into(),
                purpose: Purpose::Plan,
                model: "fixture-chief".into(),
                state_digest: state_digest.clone(),
                context,
                created_at: chrono::Utc::now().to_rfc3339(),
            })
            .unwrap();
        host.management
            .finish(
                operation,
                Ok(DecisionObservation {
                    response_id: "resp_planning_fixture".into(),
                    requested_model: "fixture-chief".into(),
                    actual_model: "fixture-chief".into(),
                    input_tokens: 10,
                    output_tokens: 20,
                    envelope: DecisionEnvelope {
                        state_digest,
                        decision,
                    },
                }),
            )
            .unwrap();
        host.poll_planning_results();
    }
    pub(in crate::host) fn proposal(validator: &str) -> ManagementDecision {
        ManagementDecision::Plan {
            tasks: vec![ProposedTask {
                profile_id: None,
                id: "task".into(),
                goal: "Implement a sum function".into(),
                dependencies: vec![],
                worker_id: "worker-codex".into(),
                validator_id: validator.into(),
            }],
            rationale: "Single task with independent check".into(),
        }
    }
    #[test]
    #[ignore = "isolated native multistage configuration fixture; no model calls"]
    fn native_multicheck_configuration_fixture() {
        let root = PathBuf::from(std::env::var("CERULEAN_MULTICHECK_NATIVE_DIR").unwrap());
        let root = fs::canonicalize(root).unwrap();
        let reports = fs::canonicalize(std::env::current_dir().unwrap().join("../../reports"))
            .or_else(|_| fs::canonicalize(std::env::current_dir().unwrap().join("reports")))
            .unwrap();
        assert!(root.starts_with(reports));
        assert!(!root.join("data").exists());
        let (mut host, draft) = fixture(&root);
        host.configure_project(draft).unwrap();
        assert!(host.management.statuses().is_empty());
    }
    #[test]
    #[ignore = "isolated native multistage proposal fixture; synthetic chief reply"]
    fn native_multicheck_proposal_fixture() {
        let root =
            fs::canonicalize(std::env::var("CERULEAN_MULTICHECK_NATIVE_DIR").unwrap()).unwrap();
        let reports = fs::canonicalize(std::env::current_dir().unwrap().join("../../reports"))
            .or_else(|_| fs::canonicalize(std::env::current_dir().unwrap().join("reports")))
            .unwrap();
        assert!(root.starts_with(reports));
        let mut host = RuntimeHost::open(&root.join("data")).unwrap();
        assert_eq!(host.project_setups["project"].setup.checks().len(), 2);
        assert!(host.management.statuses().is_empty());
        complete(&mut host, proposal("tests"));
        assert!(host.snapshot().workflows[0].revision.is_some());
        assert!(host.snapshot().workflows[0].approval.is_none());
        assert!(host.codex_jobs.is_empty());
        assert!(host.jobs.is_empty());
    }
    #[test]
    fn supervision_policy_changes_approval_and_preserves_legacy_history() {
        use cerulean_domain::supervision::SupervisionPolicy;
        let root = tempfile::tempdir().unwrap();
        let (mut host, mut draft) = fixture(root.path());
        host.configure_project(draft.clone()).unwrap();
        complete(&mut host, proposal("tests"));
        let legacy_revision = host.projects["project"].revision().unwrap().clone();
        let legacy_digest = legacy_revision.approval_digest().unwrap();
        assert!(serde_json::to_value(&legacy_revision).unwrap()["policy"]
            .get("supervision")
            .is_none());
        let legacy_file = root
            .path()
            .join("data/project-setups/project.00000001.json");
        let legacy_bytes = fs::read(&legacy_file).unwrap();
        draft.supervision = Some(SupervisionPolicy {
            schema_version: 1,
            repeated_failure_threshold: 2,
            max_advice_per_attempt: 2,
            delivery_enabled: false,
        });
        host.configure_project(draft.clone()).unwrap();
        complete_at(&mut host, proposal("tests"), "ui-plan-supervision-2");
        let revision = host.projects["project"].revision().unwrap().clone();
        assert_eq!(revision.policy.supervision, draft.supervision);
        let mut delivery = revision.clone();
        delivery
            .policy
            .supervision
            .as_mut()
            .unwrap()
            .delivery_enabled = true;
        assert_ne!(
            delivery.approval_digest().unwrap(),
            revision.approval_digest().unwrap()
        );
        let old_policy =
            serde_json::to_string(revision.policy.supervision.as_ref().unwrap()).unwrap();
        assert!(!old_policy.contains("deliveryEnabled"));
        for (threshold, limit) in [(3, 2), (2, 1), (2, 0)] {
            let mut changed = revision.clone();
            let p = changed.policy.supervision.as_mut().unwrap();
            p.repeated_failure_threshold = threshold;
            p.max_advice_per_attempt = limit;
            assert_ne!(
                changed.approval_digest().unwrap(),
                revision.approval_digest().unwrap()
            );
        }
        let approve = |digest| Request {
            id: "approve".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::ApproveRevision {
                project_id: "project".into(),
                digest,
            },
        };
        assert!(host.handle(approve(legacy_digest)).error.is_some());
        let approved = revision.approval_digest().unwrap();
        assert!(host.handle(approve(approved.clone())).error.is_none());
        assert!(host.configure_project(draft).is_err());
        let history = serde_json::to_vec(host.projects["project"].history()).unwrap();
        drop(host);
        let host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(
            host.projects["project"].approval().unwrap().revision_digest,
            approved
        );
        assert_eq!(fs::read(legacy_file).unwrap(), legacy_bytes);
        assert_eq!(
            serde_json::to_vec(host.projects["project"].history()).unwrap(),
            history
        );
        assert!(host.codex_jobs.is_empty());
        assert!(host.jobs.is_empty());
        assert!(host.runs.all().is_empty());
    }
    #[test]
    fn invalid_supervision_never_publishes_configuration() {
        use cerulean_domain::supervision::SupervisionPolicy;
        let root = tempfile::tempdir().unwrap();
        let (mut host, mut draft) = fixture(root.path());
        for (schema, threshold, limit) in [(2, 2, 2), (1, 1, 2), (1, 9, 2), (1, 2, 3)] {
            draft.supervision = Some(SupervisionPolicy {
                schema_version: schema,
                repeated_failure_threshold: threshold,
                max_advice_per_attempt: limit,
                delivery_enabled: false,
            });
            assert!(host.configure_project(draft.clone()).is_err());
        }
        assert!(host.project_setups.is_empty());
        assert!(host.management.statuses().is_empty());
        assert_eq!(
            fs::read_dir(root.path().join("data/project-setups"))
                .unwrap()
                .count(),
            0
        );
    }
    #[test]
    fn ordered_checks_are_bound_to_plan_approval_and_survive_restart() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, mut draft) = fixture(root.path());
        host.configure_project(draft.clone()).unwrap();
        let legacy = root
            .path()
            .join("data/project-setups/project.00000001.json");
        let legacy_bytes = fs::read(&legacy).unwrap();
        let legacy_value: serde_json::Value = serde_json::from_slice(&legacy_bytes).unwrap();
        assert!(legacy_value["setup"].get("acceptance").is_none());
        assert!(legacy_value["setup"]["check"].get("name").is_none());
        draft.acceptance = Some(AcceptanceDraft {
            schema_version: 1,
            stages: vec![
                AcceptanceStageDraft {
                    expected_assertions: vec![],
                    id: "syntax".into(),
                    name: "语法检查".into(),
                    validator_id: "tests".into(),
                    args: draft.check_args.clone(),
                    timeout_seconds: 20,
                },
                AcceptanceStageDraft {
                    expected_assertions: vec![],
                    id: "unit".into(),
                    name: "单元测试".into(),
                    validator_id: "tests".into(),
                    args: vec!["--list".into()],
                    timeout_seconds: 40,
                },
            ],
        });
        host.configure_project(draft).unwrap();
        let saved = host.project_setups["project"].clone();
        assert_eq!(saved.setup.version, 2);
        complete(&mut host, proposal("tests"));
        let revision = host.snapshot().workflows[0].revision.clone().unwrap();
        assert_eq!(revision.execution[0].checks, saved.setup.checks());
        assert_eq!(revision.execution[0].checks[1].id, "unit");
        let original = revision.approval_digest().unwrap();
        for change in 0..4 {
            let mut altered = revision.clone();
            let checks = &mut altered.execution[0].checks;
            match change {
                0 => checks.swap(0, 1),
                1 => checks[1].args.push("changed".into()),
                2 => checks[1].name = Some("changed name".into()),
                _ => checks[1].timeout_seconds += 1,
            }
            assert_ne!(original, altered.approval_digest().unwrap());
        }
        assert!(host
            .start_project_run("project", &original, &saved.digest)
            .is_err());
        assert!(host.codex_jobs.is_empty());
        assert!(host.jobs.is_empty());
        host.handle(Request {
            id: "approve".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::ApproveRevision {
                project_id: "project".into(),
                digest: original.clone(),
            },
        })
        .result
        .unwrap();
        host.start_project_run("project", &original, &saved.digest)
            .unwrap();
        let run = host.runs.all()[0].clone();
        host.pause_project_run(&run.id).unwrap();
        let history = serde_json::to_value(host.projects["project"].history()).unwrap();
        drop(host);
        let host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(fs::read(&legacy).unwrap(), legacy_bytes);
        assert_eq!(host.project_setups["project"].digest, saved.digest);
        assert_eq!(
            serde_json::to_value(host.projects["project"].history()).unwrap(),
            history
        );
        assert_eq!(host.runs.all()[0].state, RunState::Paused);
        assert!(host.codex_jobs.is_empty());
        assert!(host.jobs.is_empty());
    }
    #[test]
    fn invalid_multicheck_configuration_never_publishes_or_dispatches() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, mut draft) = fixture(root.path());
        draft.acceptance = Some(AcceptanceDraft {
            schema_version: 1,
            stages: vec![
                AcceptanceStageDraft {
                    expected_assertions: vec![],
                    id: "first".into(),
                    name: "First".into(),
                    validator_id: "tests".into(),
                    args: draft.check_args.clone(),
                    timeout_seconds: 30,
                },
                AcceptanceStageDraft {
                    expected_assertions: vec![],
                    id: "second".into(),
                    name: "Second".into(),
                    validator_id: "tests".into(),
                    args: vec![],
                    timeout_seconds: 30,
                },
            ],
        });
        for case in 0..11 {
            let mut invalid = draft.clone();
            let acceptance = invalid.acceptance.as_mut().unwrap();
            match case {
                0 => acceptance.schema_version = 2,
                1 => acceptance.stages.clear(),
                2 => acceptance.stages[1].id = "first".into(),
                3 => acceptance.stages[1].validator_id = "not-registered".into(),
                4 => acceptance.stages[1].timeout_seconds = 0,
                5 => acceptance.stages[1].timeout_seconds = 120,
                6 => acceptance.stages[1].name = "\0bad".into(),
                7 => acceptance.stages[1].args = vec!["x".repeat(4096); 5],
                8 => acceptance.stages[0].args.clear(),
                9 => acceptance.stages[1].id = "../outside".into(),
                _ => acceptance.stages = vec![acceptance.stages[0].clone(); 9],
            }
            assert!(host.configure_project(invalid).is_err(), "case {case}");
            assert!(host.project_setups.is_empty());
            assert_eq!(
                fs::read_dir(root.path().join("data/project-setups"))
                    .unwrap()
                    .count(),
                0
            );
            assert!(host.codex_jobs.is_empty());
            assert!(host.jobs.is_empty());
        }
        host.configure_project(draft).unwrap();
        assert_eq!(host.project_setups["project"].setup.version, 1);
    }
    #[test]
    fn changed_secondary_validator_cannot_be_hidden_by_rehashing_setup() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, mut draft) = fixture(root.path());
        draft.acceptance = Some(AcceptanceDraft {
            schema_version: 1,
            stages: vec![
                AcceptanceStageDraft {
                    expected_assertions: vec![],
                    id: "first".into(),
                    name: "First".into(),
                    validator_id: "tests".into(),
                    args: draft.check_args.clone(),
                    timeout_seconds: 30,
                },
                AcceptanceStageDraft {
                    expected_assertions: vec![],
                    id: "second".into(),
                    name: "Second".into(),
                    validator_id: "tests".into(),
                    args: vec![],
                    timeout_seconds: 30,
                },
            ],
        });
        host.configure_project(draft).unwrap();
        let mut saved = host.project_setups["project"].clone();
        drop(host);
        saved.setup.acceptance.as_mut().unwrap().stages[1]
            .check
            .tool
            .as_mut()
            .unwrap()
            .executable
            .sha256 = "a".repeat(64);
        saved.digest = digest(&saved.setup).unwrap();
        fs::write(
            root.path()
                .join("data/project-setups/project.00000001.json"),
            serde_json::to_vec(&saved).unwrap(),
        )
        .unwrap();
        assert!(RuntimeHost::open(&root.path().join("data"))
            .err()
            .unwrap()
            .contains("configured stage validator differs"));
    }
    #[test]
    fn saved_setup_survives_restart_and_model_plan_still_requires_user_approval() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, draft) = fixture(root.path());
        let reply = host.handle(Request {
            id: "save".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::ConfigureProject {
                draft: Box::new(draft),
            },
        });
        assert!(reply.error.is_none(), "{:?}", reply.error);
        let saved = host.project_setups["project"].clone();
        assert!(host.snapshot().workflows[0].revision.is_none());
        assert!(host.request_chief_plan("project", &saved.digest).is_err());
        assert!(host.management.statuses().is_empty());
        drop(host);
        let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(host.project_setups["project"].digest, saved.digest);
        complete(&mut host, proposal("tests"));
        let snapshot = host.snapshot();
        let workflow = &snapshot.workflows[0];
        let revision = workflow.revision.as_ref().unwrap();
        assert!(workflow.approval.is_none());
        assert_eq!(revision.execution[0].artifact_paths, vec!["sum.mjs"]);
        assert_eq!(
            revision.execution[0].worker.as_ref(),
            saved.setup.worker.as_ref()
        );
        assert_eq!(revision.execution[0].checks[0], saved.setup.check);
        assert!(host.codex_jobs.is_empty());
        host.handle(Request {
            id: "approve".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::ApproveRevision {
                project_id: "project".into(),
                digest: revision.approval_digest().unwrap(),
            },
        })
        .result
        .unwrap();
        assert!(host.snapshot().workflows[0].approval.is_some());
        assert!(host.request_chief_plan("project", &saved.digest).is_err());
    }
    #[test]
    fn configuration_cannot_reset_request_budget_or_authorize_other_validator() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, mut draft) = fixture(root.path());
        host.configure_project(draft.clone()).unwrap();
        complete(
            &mut host,
            ManagementDecision::Wait {
                reason: "Clarify acceptance criteria".into(),
            },
        );
        draft.requirement = "Clarified requirement".into();
        host.configure_project(draft).unwrap();
        let saved = &host.project_setups["project"];
        assert_eq!(saved.setup.version, 2);
        let hash = saved.digest.clone();
        assert!(host
            .request_chief_plan("project", &hash)
            .unwrap_err()
            .contains("上限"));
        assert_eq!(host.management.statuses().len(), 1);
        let other = tempfile::tempdir().unwrap();
        let (mut host, draft) = fixture(other.path());
        host.configure_project(draft).unwrap();
        complete(&mut host, proposal("not-registered"));
        assert!(host.snapshot().workflows[0].revision.is_none());
        assert!(host.snapshot().management_decisions[0]
            .message
            .as_ref()
            .unwrap()
            .contains("未配置"));
    }
    #[test]
    fn invalid_configuration_and_corrupt_saved_configuration_are_rejected() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, mut draft) = fixture(root.path());
        draft.artifact_paths = vec!["../outside.mjs".into()];
        assert!(host.configure_project(draft.clone()).is_err());
        assert!(host.project_setups.is_empty());
        draft.artifact_paths = vec!["sum.mjs".into()];
        draft.worker_max_requests = 0;
        assert!(host.configure_project(draft.clone()).is_err());
        draft.worker_max_requests = 1;
        host.configure_project(draft).unwrap();
        drop(host);
        let path = root
            .path()
            .join("data/project-setups/project.00000001.json");
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["setup"]["requirement"] = "Tampered".into();
        fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(RuntimeHost::open(&root.path().join("data"))
            .err()
            .unwrap()
            .contains("digest mismatch"));
    }
    #[test]
    fn chief_plan_cannot_exceed_configured_worker_request_capacity() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, mut draft) = fixture(root.path());
        draft.worker_max_requests = 1;
        host.configure_project(draft).unwrap();
        let mut decision = proposal("tests");
        if let ManagementDecision::Plan { tasks, .. } = &mut decision {
            let mut second = tasks[0].clone();
            second.id = "second".into();
            second.dependencies = vec!["task".into()];
            tasks.push(second);
        }
        complete(&mut host, decision);
        assert!(host.snapshot().workflows[0].revision.is_none());
        assert!(host.snapshot().management_decisions[0]
            .message
            .as_ref()
            .unwrap()
            .contains("请求上限"));
    }
}

#[cfg(all(test, windows))]
mod run_tests {
    use super::tests::{complete, fixture, proposal};
    use super::*;
    use cerulean_adapters::openai::DecisionObservation;
    use cerulean_domain::management::{DecisionEnvelope, ProposedTask};
    use cerulean_domain::project_setup::RunState;
    use std::time::{Duration, Instant};
    fn configured(root: &Path) -> RuntimeHost {
        let (mut host, mut draft) = fixture(root);
        draft.management_max_requests = 6;
        host.configure_project(draft).unwrap();
        complete(&mut host, proposal("tests"));
        let hash = host.snapshot().workflows[0]
            .revision_digest
            .clone()
            .unwrap();
        host.approve("project", &hash).unwrap();
        host
    }
    fn start(host: &mut RuntimeHost) -> String {
        let revision = host.snapshot().workflows[0]
            .revision_digest
            .clone()
            .unwrap();
        let setup = host.project_setups["project"].digest.clone();
        host.start_project_run("project", &revision, &setup)
            .unwrap();
        host.runs.all()[0].id.clone()
    }
    fn wait_fixture(
        i: crate::management_execution::DecisionIntent,
    ) -> Result<DecisionObservation, String> {
        Ok(DecisionObservation {
            response_id: "resp_wait_fixture".into(),
            requested_model: i.model.clone(),
            actual_model: i.model,
            input_tokens: 1,
            output_tokens: 1,
            envelope: DecisionEnvelope {
                state_digest: i.state_digest,
                decision: ManagementDecision::Wait {
                    reason: "Need user clarification".into(),
                },
            },
        })
    }
    #[test]
    fn run_authorization_pause_resume_are_durable_and_bound_to_approval() {
        let root = tempfile::tempdir().unwrap();
        let mut host = configured(root.path());
        let setup = host.project_setups["project"].digest.clone();
        assert!(host
            .start_project_run("project", &"a".repeat(64), &setup)
            .is_err());
        assert!(host.runs.all().is_empty());
        let id = start(&mut host);
        host.pause_project_run(&id).unwrap();
        drop(host);
        let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
        host.poll_verifications();
        assert_eq!(host.runs.all()[0].state, RunState::Paused);
        assert_eq!(host.management.statuses().len(), 1); // Only the initial synthetic chief plan.
        let operation = format!("{id}-2");
        assert!(host.ensure_run_dispatch_authorized(&operation).is_err());
        start(&mut host);
        assert!(host.ensure_run_dispatch_authorized(&operation).is_ok());
        // Explicit pause is handled before polling, so it cannot enqueue a new request.
        host.handle(Request {
            id: "pause".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::PauseProjectRun { run_id: id },
        })
        .result
        .unwrap();
        assert_eq!(host.management.statuses().len(), 1);
        assert_eq!(host.runs.all()[0].state, RunState::Paused);
    }
    #[test]
    fn wait_stops_loop_and_survives_reopen_without_another_billed_intent() {
        let root = tempfile::tempdir().unwrap();
        let mut host = configured(root.path());
        host.openai_connection.key_configured = true;
        host.management_fixture = Some(wait_fixture);
        start(&mut host);
        let deadline = Instant::now() + Duration::from_secs(5);
        while host.runs.all()[0].state == RunState::Active && Instant::now() < deadline {
            host.poll_verifications();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(host.runs.all()[0].state, RunState::Attention);
        assert_eq!(host.management.statuses().len(), 2);
        for _ in 0..5 {
            host.poll_verifications();
        }
        assert_eq!(host.management.statuses().len(), 2);
        drop(host);
        let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
        host.poll_verifications();
        assert_eq!(host.management.statuses().len(), 2);
        assert_eq!(host.runs.all()[0].state, RunState::Attention);
        let run = host.runs.all()[0].clone();
        assert!(host
            .start_project_run(&run.project_id, &run.revision_digest, &run.setup_digest)
            .unwrap_err()
            .contains("补充信息"));
        let question = host
            .snapshot()
            .management_decisions
            .into_iter()
            .find(|d| d.can_reply)
            .unwrap();
        host.handle(Request {
            id: "answer-run-wait".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::AnswerManagementWait {
                operation_id: question.operation_id,
                evidence_digest: question.evidence_digest,
                answer: "Continue within the approved scope".into(),
            },
        })
        .result
        .unwrap();
        assert_eq!(host.runs.all()[0].state, RunState::Attention);
        assert_eq!(host.management.statuses().len(), 2);
        host.start_project_run(&run.project_id, &run.revision_digest, &run.setup_digest)
            .unwrap();
        assert_eq!(host.runs.all()[0].state, RunState::Active);
        // Pause immediately: answering and explicitly resuming have not themselves sent a request.
        host.pause_project_run(&run.id).unwrap();
        assert_eq!(host.management.statuses().len(), 2);
    }
    #[test]
    fn unknown_request_during_restart_never_resends_and_completion_badge_requires_acceptance() {
        let root = tempfile::tempdir().unwrap();
        let mut host = configured(root.path());
        let id = start(&mut host);
        let context = host
            .management_context("project", "Implement a sum function")
            .unwrap();
        host.management
            .begin(crate::management_execution::DecisionIntent {
                operation_id: format!("{id}-2"),
                project_id: "project".into(),
                purpose: Purpose::Assign,
                model: "fixture-deputy".into(),
                state_digest: digest(&context).unwrap(),
                context,
                created_at: chrono::Utc::now().to_rfc3339(),
            })
            .unwrap();
        drop(host);
        let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
        host.poll_verifications();
        assert_eq!(host.runs.all()[0].state, RunState::Attention);
        assert_eq!(host.management.statuses().len(), 2);
        drop(host);
        let path = root.path().join("data/run-control/00000002.json");
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        value["body"]["run"]["state"] = "completed".into();
        // Recompute using struct field order through the test-only helper.
        crate::run_control::forge_completed_for_test(&path, value).unwrap();
        assert!(RuntimeHost::open(&root.path().join("data"))
            .err()
            .unwrap()
            .contains("收口证据"));
    }
    #[test]
    #[ignore = "native paused-run fixture; no provider calls"]
    fn seed_native_paused_run_fixture() {
        let root = PathBuf::from(std::env::var_os("CERULEAN_RUN_NATIVE_DIR").unwrap())
            .canonicalize()
            .unwrap();
        let reports = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../reports")
            .canonicalize()
            .unwrap();
        assert!(root.starts_with(reports) && !root.join("data").exists());
        let mut host = configured(&root);
        let id = start(&mut host);
        host.pause_project_run(&id).unwrap();
        assert_eq!(host.management.statuses().len(), 1);
    }
    fn pipeline_fixture(
        i: crate::management_execution::DecisionIntent,
    ) -> Result<DecisionObservation, String> {
        let setup: &serde_json::Value = &i.context["executionSetup"]["setup"];
        let decision = match i.purpose {
            Purpose::Plan => ManagementDecision::Plan {
                tasks: vec![ProposedTask {
                    profile_id: None,
                    id: "task".into(),
                    goal: setup["requirement"].as_str().unwrap().into(),
                    dependencies: vec![],
                    worker_id: "worker-codex".into(),
                    validator_id: setup["validatorId"].as_str().unwrap().into(),
                }],
                rationale: "Bounded independent task".into(),
            },
            Purpose::Assign => {
                assert_eq!(i.context["workflow"]["state"]["approved"], true);
                ManagementDecision::Assign {
                    task_id: "task".into(),
                    worker_id: "worker-codex".into(),
                    rationale: "Approved eligible work".into(),
                }
            }
            Purpose::Accept => {
                assert_eq!(
                    i.context["workflow"]["state"]["tasks"]["task"]["verified"],
                    true
                );
                ManagementDecision::Accept {
                    task_id: "task".into(),
                    attempt_id: i.context["workflow"]["state"]["tasks"]["task"]["attempt"]
                        .as_str()
                        .unwrap()
                        .into(),
                    rationale: "Independent verification passed".into(),
                }
            }
            _ => return Err("fixture unsupported purpose".into()),
        };
        Ok(DecisionObservation {
            response_id: "resp_pipeline_fixture".into(),
            requested_model: i.model.clone(),
            actual_model: i.model,
            input_tokens: 1,
            output_tokens: 1,
            envelope: DecisionEnvelope {
                state_digest: i.state_digest,
                decision,
            },
        })
    }
    #[test]
    #[ignore = "explicit real worker autonomous run; synthetic management transport; fresh report directory required"]
    fn live_project_runner_through_independent_verification_and_acceptance() {
        let root = PathBuf::from(
            std::env::var_os("CERULEAN_RUN_LIVE_DIR").expect("fresh reports path required"),
        )
        .canonicalize()
        .unwrap();
        let reports = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../reports")
            .canonicalize()
            .unwrap();
        assert!(root.starts_with(reports) && !root.join("data").exists());
        let (mut host, mut draft) = fixture(&root);
        let node = std::env::var("CERULEAN_NODE_EXE").unwrap();
        let check = root.join("check-sum.mjs");
        fs::write(&check,"import assert from 'node:assert/strict';import{pathToFileURL}from'node:url';import{resolve}from'node:path';const{sum}=await import(pathToFileURL(resolve('sum.mjs')));for(const[a,b,c]of [[0,0,0],[1,2,3],[-1,1,0],[-2,-3,-5],[0.5,0.25,0.75],[100,200,300],[1,-2,-1],[0,4,4],[7,0,7],[99,1,100]])assert.equal(sum(a,b),c);console.log('10 independent assertions passed');").unwrap();
        host.register_validator(
            "node-check".into(),
            "Independent sum verification".into(),
            node,
            vec![check.to_string_lossy().into()],
            Default::default(),
        )
        .unwrap();
        draft.validator_id = "node-check".into();
        draft.check_args = vec![check.to_string_lossy().into()];
        draft.codex_executable = std::env::var("CERULEAN_CODEX_EXE").unwrap();
        draft.worker_model = std::env::var("CERULEAN_CODEX_MODEL").unwrap();
        draft.management_max_requests = 3;
        draft.worker_max_requests = 1;
        draft.max_seconds = 180;
        draft.requirement="Create sum.mjs as an ES module exporting sum(a,b) that returns a+b for numeric inputs. Keep it minimal.".into();
        host.configure_project(draft).unwrap();
        host.openai_connection.key_configured = true;
        host.management_fixture = Some(pipeline_fixture);
        let setup = host.project_setups["project"].digest.clone();
        host.request_chief_plan("project", &setup).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while host.snapshot().workflows[0].revision.is_none() && Instant::now() < deadline {
            host.poll_verifications();
            std::thread::sleep(Duration::from_millis(5));
        }
        let revision = host.snapshot().workflows[0]
            .revision_digest
            .clone()
            .unwrap();
        assert!(host.snapshot().workflows[0].approval.is_none());
        assert!(host.codex_jobs.is_empty());
        host.handle(Request {
            id: "actual-user-fixture-approval".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::ApproveRevision {
                project_id: "project".into(),
                digest: revision.clone(),
            },
        })
        .result
        .unwrap();
        fs::write(root.join("result.json"),serde_json::to_vec_pretty(&serde_json::json!({"status":"starting","maxWorkerTurns":1,"realManagementCalls":0})).unwrap()).unwrap();
        host.handle(Request {
            id: "user-run".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::StartProjectRun {
                project_id: "project".into(),
                revision_digest: revision,
                setup_digest: setup,
            },
        })
        .result
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(240);
        let mut polls = 0;
        while host.runs.all()[0].state == RunState::Active && Instant::now() < deadline {
            host.poll_verifications();
            polls += 1;
            std::thread::sleep(Duration::from_millis(30));
        }
        assert_eq!(
            host.runs.all()[0].state,
            RunState::Completed,
            "{:?}",
            host.runs.all()
        );
        assert_eq!(
            host.snapshot().workflows[0].state.tasks["task"].state,
            TaskState::Accepted
        );
        assert_eq!(host.management.statuses().len(), 3);
        let history = host.projects["project"].history();
        let sends = history
            .iter()
            .filter(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. }))
            .count();
        assert_eq!(sends, 1);
        fs::write(
            root.join("completed-snapshot.json"),
            serde_json::to_vec_pretty(&host.snapshot()).unwrap(),
        )
        .unwrap();
        drop(host);
        let mut host = RuntimeHost::open(&root.join("data")).unwrap();
        host.poll_verifications();
        assert_eq!(host.runs.all()[0].state, RunState::Completed);
        assert_eq!(host.management.statuses().len(), 3);
        fs::write(root.join("result.json"),serde_json::to_vec_pretty(&serde_json::json!({"at":chrono::Utc::now().to_rfc3339(),"status":"passed","scope":"synthetic management transport and simulated user clicks; real Codex worker, independent verifier and production orchestration","workerModelCalls":1,"realManagementCalls":0,"syntheticManagementDecisions":3,"dispatchIntents":sends,"independentAssertions":10,"workflowState":"accepted","runState":"completed","restartNewRequests":0,"polls":polls})).unwrap()).unwrap();
    }
}
