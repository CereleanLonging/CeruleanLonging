use super::*;
use crate::{
    artifacts::ArtifactObserver,
    scheduler::{CommandContext, DurableWorkflow, SchedulerConfig},
    verifier::PinnedCheck,
};
use cerulean_domain::{execution::*, workflow::*};
mod inputs;
mod overlay;

struct Flow {
    _temp: TempDir,
    store: DurableWorkflow,
    plan: PlanRevision,
    journal: PathBuf,
    config: SchedulerConfig,
}
fn apply(
    store: &mut DurableWorkflow,
    who: &str,
    command: WorkflowCommand,
    context: CommandContext,
) -> Result<(), String> {
    store
        .command(
            &format!("event-{}", store.history().len() + 1),
            who,
            command,
            context,
            true,
        )
        .map(|_| ())
}
impl Flow {
    fn new() -> Self {
        Self::with_dependency(false)
    }
    fn with_dependency(dependent: bool) -> Self {
        Self::with_plan(|plan| {
            if dependent {
                let mut task = plan.workflow.tasks[0].clone();
                task.id = "downstream".into();
                task.dependencies = vec!["task".into()];
                plan.workflow.tasks.push(task);
                let mut spec = plan.execution[0].clone();
                spec.task_id = "downstream".into();
                plan.execution.push(spec);
            }
        })
    }
    fn with_plan(edit: impl FnOnce(&mut PlanRevision)) -> Self {
        let f = Fixture::new();
        let validator = std::env::current_exe().unwrap();
        let tool = PinnedCheck::describe(&validator, &[], Default::default()).unwrap();
        let mut plan = PlanRevision {
            management_setup_digest: None,
            worktree: Some(GitWorktreePolicy {
                integration: None,
                schema_version: 1,
                base_commit: f.base.clone(),
                pool_root: text(&f.store.root).unwrap(),
                git: f.pin.clone(),
                input: GitWorktreeInput::CommittedRawBlobs,
            }),
            schema_version: 1,
            project: Project {
                id: "project".into(),
                root: text(&f.repo).unwrap(),
                timezone: "Asia/Shanghai".into(),
            },
            workflow: WorkflowPlan {
                id: "plan".into(),
                version: 1,
                tasks: vec![WorkflowTask {
                    id: "task".into(),
                    dependencies: vec![],
                    workspace_key: "project".into(),
                    goal: "edit isolated b.txt".into(),
                    resource_keys: Some(vec!["database-fixture".into()]),
                }],
            },
            policy: ExecutionPolicy {
                supervision: None,
                version: 1,
                tools: vec![
                    ToolPermission::ReadFiles,
                    ToolPermission::WriteFiles,
                    ToolPermission::RunCommands,
                ],
                max_requests: 2,
                max_seconds: 60,
                currency_budget: None,
                budget_usd_cents: None,
            },
            execution: vec![TaskExecution {
                worker_directory: None,
                supervision_override: None,
                profile_id: None,
                desktop_worker: None,
                api_worker: None,
                worker: None,
                task_id: "task".into(),
                adapter_id: "codex".into(),
                artifact_paths: vec!["b.txt".into()],
                checks: vec![AcceptanceCheck {
                    expected_assertions: vec![],
                    id: "isolated".into(),
                    name: None,
                    program: tool.executable.path.clone(),
                    args: vec![
                        "--exact".into(),
                        "worktree_validation_fixture::only_expected_isolated_inputs".into(),
                        "--ignored".into(),
                        "--nocapture".into(),
                    ],
                    timeout_seconds: 10,
                    tool: Some(tool),
                }],
            }],
            snapshot_policy: Some(SnapshotPolicy {
                exclude: vec![".git".into()],
                max_files: 100,
                max_bytes: 1024 * 1024,
            }),
        };
        edit(&mut plan);
        let config = SchedulerConfig {
            schema_version: 1,
            principals: vec![
                Principal {
                    id: "user".into(),
                    role: Role::User,
                    provider: None,
                },
                Principal {
                    id: "chief".into(),
                    role: Role::Chief,
                    provider: Some("openai".into()),
                },
                Principal {
                    id: "deputy".into(),
                    role: Role::Deputy,
                    provider: Some("openai".into()),
                },
                Principal {
                    id: "worker".into(),
                    role: Role::Worker,
                    provider: None,
                },
                Principal {
                    id: "core".into(),
                    role: Role::Core,
                    provider: None,
                },
            ],
            core_id: "core".into(),
            execution_scope: Some(ExecutionScope {
                project: plan.project.clone(),
                policy_version: 1,
            }),
        };
        let journal = f.temp.path().join("journal");
        drop(f.store);
        let mut store = DurableWorkflow::open(&journal, config.clone()).unwrap();
        apply(
            &mut store,
            "chief",
            WorkflowCommand::Propose {
                plan: plan.workflow.clone(),
            },
            CommandContext {
                execution: Some(ExecutionEvidence::Revision {
                    revision: plan.clone(),
                }),
                ..Default::default()
            },
        )
        .unwrap();
        Self {
            _temp: f.temp,
            store,
            plan,
            journal,
            config,
        }
    }
    fn approve(&mut self) {
        let approval = Approval {
            worker_account_setup: None,
            plan_id: "plan".into(),
            revision: 1,
            policy_version: 1,
            revision_digest: self.plan.approval_digest().unwrap(),
            user_id: "user".into(),
            approved_at: chrono::Utc::now().to_rfc3339(),
        };
        let command = WorkflowCommand::Approve {
            digest: self.store.state().digest.clone().unwrap(),
        };
        apply(
            &mut self.store,
            "user",
            command,
            CommandContext {
                approval_source: Some("local-user-fixture".into()),
                execution: Some(ExecutionEvidence::Approval { approval }),
                ..Default::default()
            },
        )
        .unwrap();
    }
    fn prepare(&self) -> Attempt {
        let (workspace, proof) = self
            .store
            .prepare_task_workspace("task", "attempt")
            .unwrap();
        Attempt {
            workspace_proof: Some(proof),
            id: "attempt".into(),
            task_id: "task".into(),
            worker_id: "worker".into(),
            revision_digest: self.plan.approval_digest().unwrap(),
            fence: 1,
            binding: SessionBinding {
                adapter_id: "codex".into(),
                adapter_version: "fixture".into(),
                session_id: "session".into(),
                identity_strength: IdentityStrength::ProviderId,
                workspace,
            },
        }
    }
    fn assign(&mut self, attempt: &Attempt) -> Result<(), String> {
        apply(
            &mut self.store,
            "deputy",
            WorkflowCommand::Assign {
                task: attempt.task_id.clone(),
                worker: attempt.worker_id.clone(),
                attempt: attempt.id.clone(),
            },
            CommandContext {
                execution: Some(ExecutionEvidence::Attempt {
                    attempt: attempt.clone(),
                }),
                ..Default::default()
            },
        )
    }
    fn dispatch(&mut self, attempt: &Attempt) -> Result<(), String> {
        apply(
            &mut self.store,
            "core",
            WorkflowCommand::DispatchIntent {
                task: attempt.task_id.clone(),
                attempt: attempt.id.clone(),
            },
            CommandContext {
                thread_id: Some("session".into()),
                cwd: Some(attempt.binding.workspace.clone()),
                ..Default::default()
            },
        )
    }
}
fn receipt(a: &Attempt, status: ReceiptStatus) -> ExecutionReceipt {
    ExecutionReceipt {
        schema_version: 1,
        task_id: a.task_id.clone(),
        attempt_id: a.id.clone(),
        fence: a.fence,
        binding: a.binding.clone(),
        turn_id: Some("turn".into()),
        observed_at: chrono::Utc::now().to_rfc3339(),
        status,
        evidence_sha256: "c".repeat(64),
    }
}
fn context(r: ExecutionReceipt, manifest: Option<ArtifactManifest>) -> CommandContext {
    CommandContext {
        thread_id: Some(r.binding.session_id.clone()),
        turn_id: r.turn_id.clone(),
        terminal_status: (r.status == ReceiptStatus::Completed).then(|| "completed".into()),
        execution: Some(ExecutionEvidence::Receipt {
            receipt: r,
            manifest,
        }),
        ..Default::default()
    }
}

#[test]
#[ignore = "explicit real Git and owned independent verifier"]
fn approved_isolated_artifact_verification_acceptance_and_reopen() {
    let mut f = Flow::new();
    assert!(f.store.prepare_task_workspace("task", "attempt").is_err());
    f.approve();
    let a = f.prepare();
    let mut forged = a.clone();
    forged.workspace_proof.as_mut().unwrap().record_digest = "f".repeat(64);
    assert!(f.assign(&forged).is_err());
    forged = a.clone();
    forged.binding.workspace = f.plan.project.root.clone();
    assert!(f.assign(&forged).is_err());
    f.assign(&a).unwrap();
    f.dispatch(&a).unwrap();
    apply(
        &mut f.store,
        "core",
        WorkflowCommand::Started {
            task: "task".into(),
            attempt: a.id.clone(),
        },
        context(receipt(&a, ReceiptStatus::Running), None),
    )
    .unwrap();
    fs::write(
        Path::new(&a.binding.workspace).join("b.txt"),
        b"worker result\n",
    )
    .unwrap();
    let completed = receipt(&a, ReceiptStatus::Completed);
    let manifest = ArtifactObserver::new(
        &f.plan,
        f.store.approval().unwrap(),
        "user",
        1,
        &a,
        &completed,
        Some("turn"),
    )
    .unwrap()
    .capture()
    .unwrap();
    assert_eq!(
        fs::read(Path::new(&f.plan.project.root).join("b.txt")).unwrap(),
        b"base\n"
    );
    apply(
        &mut f.store,
        "worker",
        WorkflowCommand::Submit {
            task: "task".into(),
            attempt: a.id.clone(),
            summary: "isolated file".into(),
            evidence: Some(vec![digest(&manifest).unwrap()]),
        },
        context(completed, Some(manifest)),
    )
    .unwrap();
    let pins = f.plan.execution[0]
        .checks
        .iter()
        .map(PinnedCheck::from_approved)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    f.store
        .run_verification(
            "isolated-check",
            "task",
            &a.id,
            &pins,
            &AtomicBool::new(false),
        )
        .unwrap();
    assert!(f.store.state().tasks["task"].verified);
    apply(
        &mut f.store,
        "chief",
        WorkflowCommand::Accept {
            task: "task".into(),
            attempt: a.id,
        },
        CommandContext::default(),
    )
    .unwrap();
    drop(f.store);
    let reopened = DurableWorkflow::open(&f.journal, f.config).unwrap();
    assert_eq!(reopened.state().tasks["task"].state, TaskState::Accepted);
}

#[test]
#[ignore = "explicit real Git workflow recovery"]
fn removed_readiness_blocks_dispatch_and_unknown_reopen_does_not_recreate() {
    let mut f = Flow::new();
    f.approve();
    let a = f.prepare();
    f.assign(&a).unwrap();
    let pool = Path::new(&f.plan.worktree.as_ref().unwrap().pool_root);
    let key = workspace_key("project", "task", "attempt").unwrap();
    let ready = pool.join("records").join(format!("{key}.ready.json"));
    let bytes = fs::read(&ready).unwrap();
    fs::remove_file(&ready).unwrap();
    assert!(f.dispatch(&a).is_err());
    assert!(!ready.exists());
    fs::write(&ready, bytes).unwrap();
    f.dispatch(&a).unwrap();
    let history = f.store.history().len();
    drop(f.store);
    let reopened = DurableWorkflow::open(&f.journal, f.config).unwrap();
    assert_eq!(reopened.state().tasks["task"].state, TaskState::Unknown);
    assert_eq!(reopened.history().len(), history + 1);
    assert!(reopened.prepare_task_workspace("task", "attempt").is_err());
    assert!(Path::new(&a.binding.workspace).join("b.txt").exists());
}

#[test]
#[ignore = "explicit real Git approved-policy compatibility checks"]
fn approved_policy_changes_invalidate_approval_and_old_fields_remain_omitted() {
    let mut f = Flow::new();
    f.approve();
    let approval = f.store.approval().unwrap().clone();
    for change in 0..3 {
        let mut p = f.plan.clone();
        let w = p.worktree.as_mut().unwrap();
        match change {
            0 => w.base_commit = "b".repeat(40),
            1 => w.pool_root.push_str("-other"),
            _ => w.git.sha256 = "b".repeat(64),
        };
        assert!(approval.validate(&p, "user", 1).is_err());
    }
    let mut old = f.plan.clone();
    old.worktree = None;
    let serialized = serde_json::to_string(&old).unwrap();
    assert!(serde_json::from_str::<serde_json::Value>(&serialized)
        .unwrap()
        .get("worktree")
        .is_none());
    assert_eq!(
        serde_json::from_str::<PlanRevision>(&serialized).unwrap(),
        old
    );
    let mut a = f.prepare();
    a.workspace_proof = None;
    a.binding.workspace = old.project.root.clone();
    let serialized = serde_json::to_string(&a).unwrap();
    assert!(serde_json::from_str::<serde_json::Value>(&serialized)
        .unwrap()
        .get("workspaceProof")
        .is_none());
    old.validate_attempt_workspace(&a).unwrap();
    assert!(f.plan.validate_attempt_workspace(&a).is_err());
}

#[test]
#[ignore = "explicit real Git approval and attempt lifecycle"]
fn revision_request_and_used_attempt_block_preparation_without_mutation() {
    let mut f = Flow::new();
    f.approve();
    let a = f.prepare();
    // Recovery before assignment can adopt the same ready directory.
    assert_eq!(f.prepare(), a);
    f.assign(&a).unwrap();
    apply(
        &mut f.store,
        "user",
        WorkflowCommand::AbandonUnsent {
            task: "task".into(),
            attempt: a.id.clone(),
            reason: "fixture abandonment before dispatch".into(),
        },
        CommandContext::default(),
    )
    .unwrap();
    assert_eq!(f.store.state().tasks["task"].state, TaskState::Pending);
    assert!(f.store.prepare_task_workspace("task", &a.id).is_err());
    let digest = f.store.state().digest.clone().unwrap();
    apply(
        &mut f.store,
        "user",
        WorkflowCommand::RequestRevision {
            digest,
            reason: "fixture scope revision".into(),
        },
        CommandContext {
            approval_source: Some("authenticated local-user revision request".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(f
        .store
        .prepare_task_workspace("task", "new-attempt")
        .is_err());
    let next = f.plan.execution_root("task", "new-attempt").unwrap();
    assert!(!Path::new(&next).exists());
    assert_eq!(
        fs::read(Path::new(&a.binding.workspace).join("b.txt")).unwrap(),
        b"base\n"
    );
}
