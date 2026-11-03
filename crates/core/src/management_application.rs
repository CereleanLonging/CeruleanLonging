//! Trusted decision-to-workflow transaction; never a general user command endpoint.
use super::*;
use crate::management_execution::ApplicationTarget;

pub(super) fn matches_target(entry: &crate::scheduler::Entry, target: &ApplicationTarget) -> bool {
    match target {
        ApplicationTarget::Wait => false,
        ApplicationTarget::Workflow {
            principal,
            command,
            context,
        } => {
            entry.body.event.principal == *principal
                && entry.body.event.command == *command
                && entry.body.context == **context
        }
    }
}

fn validate_assignment_reservation(
    store: &DurableWorkflow,
    operation: &str,
    target: &ApplicationTarget,
) -> Result<(), String> {
    let ApplicationTarget::Workflow {
        command: WorkflowCommand::Assign { attempt, .. },
        ..
    } = target
    else {
        return Ok(());
    };
    let owner = format!("mgmt-{operation}");
    for entry in store.history().iter().filter(|entry| {
        matches!(&entry.body.event.command, WorkflowCommand::Assign { attempt: assigned, .. } if assigned == attempt)
            || matches!(&entry.body.context.execution, Some(ExecutionEvidence::Attempt { attempt: bound }) if &bound.id == attempt)
    }) {
        require(
            entry.body.operation_id == owner && matches_target(entry, target),
            "reserved management attempt conflicts with workflow assignment history",
        )?;
    }
    Ok(())
}

impl RuntimeHost {
    pub(super) fn validate_management_applications(&self) -> Result<(), String> {
        for status in self.management.statuses() {
            self.check_management_application(&status.operation_id)?;
        }
        Ok(())
    }
    fn check_management_application(&self, operation: &str) -> Result<(), String> {
        if self.management.application_closure(operation).is_some() {
            return self.validate_application_closures();
        }
        let Some((target, applied, saved_hash)) = self.management.application(operation) else {
            return Ok(());
        };
        let project = &self.management.intent(operation)?.project_id;
        let store = self
            .projects
            .get(project)
            .ok_or("management project ledger unavailable")?;
        validate_assignment_reservation(store, operation, target)?;
        let event = store
            .history()
            .iter()
            .find(|e| e.body.operation_id == format!("mgmt-{operation}"));
        match target {
            ApplicationTarget::Wait => require(
                event.is_none(),
                "wait decision has unexpected workflow event",
            )?,
            ApplicationTarget::Workflow { .. } => {
                if let Some(event) = event {
                    require(
                        matches_target(event, target),
                        "management workflow event differs from application intent",
                    )?;
                    if applied {
                        require(
                            saved_hash == Some(event.hash.as_str()),
                            "management application receipt differs from workflow evidence",
                        )?;
                    }
                } else {
                    require(
                        !applied,
                        "applied management decision lacks workflow evidence",
                    )?;
                }
            }
        }
        Ok(())
    }
    pub(super) fn reconcile_management_applications(&mut self) -> Result<(), String> {
        // Check all targets before publishing any reconstructed receipt.
        self.validate_management_applications()?;
        for status in self.management.statuses() {
            self.reconcile_management_application(&status.operation_id)?;
        }
        Ok(())
    }
    fn reconcile_management_application(&mut self, operation: &str) -> Result<(), String> {
        self.check_management_application(operation)?;
        if self.management.application_closure(operation).is_some() {
            return Ok(());
        }
        let Some((target, applied, _)) = self.management.application(operation) else {
            return Ok(());
        };
        if applied {
            return Ok(());
        }
        match target {
            ApplicationTarget::Wait => self.management.record_applied(operation, None)?,
            ApplicationTarget::Workflow { .. } => {
                let project = &self.management.intent(operation)?.project_id;
                let hash = self.projects[project]
                    .history()
                    .iter()
                    .find(|e| e.body.operation_id == format!("mgmt-{operation}"))
                    .map(|e| e.hash.clone());
                if let Some(hash) = hash {
                    self.management.record_applied(operation, Some(hash))?;
                }
                // Never execute a missing workflow command during startup.
            }
        }
        Ok(())
    }
    /// Called only by the trusted orchestration layer. The journal validates this
    /// exact target against the saved response; no model can approve its own plan.
    pub fn apply_management_decision(
        &mut self,
        operation: &str,
        target: ApplicationTarget,
    ) -> Result<(), String> {
        self.poll_verifications();
        self.apply_management_decision_inner(operation, target)
    }
    pub(super) fn apply_management_decision_inner(
        &mut self,
        operation: &str,
        target: ApplicationTarget,
    ) -> Result<(), String> {
        if let ApplicationTarget::Workflow {
            command: WorkflowCommand::Accept { task, attempt },
            ..
        } = &target
        {
            self.validate_chief_acceptance_material(operation, task, attempt)?;
        }
        if let ApplicationTarget::Workflow { context, .. } = &target {
            if let Some(ExecutionEvidence::Revision { revision }) = &context.execution {
                crate::reader_generation::require_revision(&self.root, revision)?;
                crate::worker_boundaries::validate(revision, false)?;
            }
        }
        require(
            self.management.application_closure(operation).is_none()
                && self.management.startup_closure(operation).is_none(),
            "应用事务已由用户关闭，禁止继续应用",
        )?;
        let project = self.management.intent(operation)?.project_id.clone();
        #[cfg(windows)]
        if self.has_api_activity(&project)
            || (self.management.application(operation).is_some()
                && matches!(
                    &target,
                    ApplicationTarget::Workflow {
                        command: WorkflowCommand::Accept { .. },
                        ..
                    }
                ))
        {
            return self.apply_shared_acceptance(operation, target);
        }
        if let ApplicationTarget::Workflow {
            command: WorkflowCommand::Assign { attempt, .. },
            ..
        } = &target
        {
            self.management
                .check_attempt_reservation(&project, attempt, Some(operation))?;
            self.startups
                .check_reservation(&project, attempt, Some(operation))?;
            self.omp_startups
                .check_reservation(&project, attempt, Some(operation))?;
        }
        self.require_available(&project)?;
        self.reconcile_management_application(operation)?;
        if let Some((previous, true, _)) = self.management.application(operation) {
            return require(
                previous == &target,
                "management decision already applied to a different target",
            );
        }
        #[cfg(windows)]
        if let ApplicationTarget::Workflow {
            command: WorkflowCommand::Assign { task, attempt, .. },
            ..
        } = &target
        {
            self.require_task_resources(&project, task, attempt)?;
        }
        let current_digest = self.management_envelope(operation)?.state_digest.clone();
        let fee_ack = self.management_fee_ack(operation)?;
        self.management.prepare_application_with_fee(
            operation,
            target.clone(),
            &current_digest,
            fee_ack,
        )?;
        let workflow_hash = match target {
            ApplicationTarget::Wait => None,
            ApplicationTarget::Workflow {
                principal,
                command,
                context,
            } => {
                let store = self
                    .projects
                    .get_mut(&project)
                    .ok_or("project not registered")?;
                let entry = store.command(
                    &format!("mgmt-{operation}"),
                    &principal,
                    command,
                    *context,
                    false,
                )?;
                Some(entry.hash)
            }
        };
        self.management.record_applied(operation, workflow_hash)
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::management_execution::{DecisionIntent, DecisionState};
    use cerulean_adapters::openai::DecisionObservation;
    use cerulean_domain::management::{
        DecisionEnvelope, ManagementDecision, ProposedTask, Purpose,
    };

    pub(super) fn fixture(root: &Path) -> (RuntimeHost, PlanRevision) {
        fs::create_dir(root.join("project")).unwrap();
        let mut host = RuntimeHost::open(&root.join("data")).unwrap();
        host.register(Project {
            id: "project".into(),
            root: root.join("project").to_string_lossy().into(),
            timezone: "Asia/Shanghai".into(),
        })
        .unwrap();
        host.register_validator(
            "tests".into(),
            "Registered test fixture".into(),
            std::env::current_exe().unwrap().to_string_lossy().into(),
            vec![],
            Default::default(),
        )
        .unwrap();
        let p = PlanRevision {
            management_setup_digest: None,
            worktree: None,
            schema_version: 1,
            project: host.snapshot().workflows[0].project.clone(),
            workflow: WorkflowPlan {
                id: "plan".into(),
                version: 1,
                tasks: vec![WorkflowTask {
                    id: "task".into(),
                    goal: "Implement a sum function".into(),
                    dependencies: vec![],
                    workspace_key: "project".into(),
                    resource_keys: None,
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
                max_requests: 3,
                max_seconds: 60,
                currency_budget: None,
                budget_usd_cents: None,
            },
            snapshot_policy: Some(SnapshotPolicy {
                exclude: vec![],
                max_files: 100,
                max_bytes: 1024 * 1024,
            }),
            execution: vec![TaskExecution {
                worker_directory: None,
                supervision_override: None,
                profile_id: None,
                desktop_worker: None,
                api_worker: None,
                task_id: "task".into(),
                adapter_id: "codex".into(),
                artifact_paths: vec!["sum.mjs".into()],
                worker: None,
                checks: vec![AcceptanceCheck {
                    expected_assertions: vec![],
                    name: None,
                    id: "check".into(),
                    program: "fixture".into(),
                    args: vec![],
                    timeout_seconds: 10,
                    tool: Some(host.registered_validator("tests").unwrap().tool.clone()),
                }],
            }],
        };
        (host, p)
    }
    pub(super) fn seed(
        host: &mut RuntimeHost,
        id: &str,
        purpose: Purpose,
        decision: ManagementDecision,
    ) -> String {
        let context = host
            .management_context("project", "Implement a sum function")
            .unwrap();
        let state_digest = digest(&context).unwrap();
        let intent = DecisionIntent {
            operation_id: id.into(),
            project_id: "project".into(),
            purpose,
            model: "fixture-model".into(),
            state_digest: state_digest.clone(),
            context,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        host.management.begin(intent).unwrap();
        host.management
            .finish(
                id,
                Ok(DecisionObservation {
                    response_id: "resp_application_fixture".into(),
                    requested_model: "fixture-model".into(),
                    actual_model: "fixture-model".into(),
                    input_tokens: 10,
                    output_tokens: 20,
                    envelope: DecisionEnvelope {
                        state_digest: state_digest.clone(),
                        decision,
                    },
                }),
            )
            .unwrap();
        state_digest
    }
    fn plan_decision() -> ManagementDecision {
        ManagementDecision::Plan {
            tasks: vec![ProposedTask {
                profile_id: None,
                id: "task".into(),
                goal: "Implement a sum function".into(),
                dependencies: vec![],
                worker_id: "worker-codex".into(),
                validator_id: "tests".into(),
            }],
            rationale: "One independently tested task".into(),
        }
    }
    fn target(p: PlanRevision) -> ApplicationTarget {
        ApplicationTarget::Workflow {
            principal: "chief".into(),
            command: WorkflowCommand::Propose {
                plan: p.workflow.clone(),
            },
            context: Box::new(CommandContext {
                execution: Some(ExecutionEvidence::Revision { revision: p }),
                ..Default::default()
            }),
        }
    }
    #[test]
    fn proposed_plan_is_bound_to_decision_and_never_approves_itself() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, p) = fixture(root.path());
        seed(&mut host, "decision-1", Purpose::Plan, plan_decision());
        let mut changed = p.clone();
        changed.workflow.tasks[0].goal = "Different goal".into();
        assert!(host
            .apply_management_decision("decision-1", target(changed))
            .is_err());
        let fake_approval = ApplicationTarget::Workflow {
            principal: "user".into(),
            command: WorkflowCommand::Approve {
                digest: "a".repeat(64),
            },
            context: Box::default(),
        };
        assert!(host
            .apply_management_decision("decision-1", fake_approval)
            .is_err());
        assert!(host.snapshot().workflows[0].revision.is_none());
        let wanted = target(p);
        host.apply_management_decision("decision-1", wanted.clone())
            .unwrap();
        assert!(host.snapshot().workflows[0].approval.is_none());
        assert!(!host.snapshot().workflows[0].state.approved);
        assert_eq!(host.snapshot().workflows[0].sequence, 1);
        host.apply_management_decision("decision-1", wanted)
            .unwrap();
        assert_eq!(host.snapshot().workflows[0].sequence, 1);
        assert_eq!(host.management_decisions()[0].state, DecisionState::Applied);
        seed(
            &mut host,
            "decision-2",
            Purpose::Assign,
            ManagementDecision::Wait {
                reason: "Actual user approval required".into(),
            },
        );
        host.apply_management_decision("decision-2", ApplicationTarget::Wait)
            .unwrap();
        assert_eq!(host.snapshot().workflows[0].sequence, 1);
        drop(host);
        let host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert!(host
            .management_decisions()
            .iter()
            .all(|s| s.state == DecisionState::Applied));
    }
    #[test]
    fn restart_after_workflow_commit_only_reconstructs_receipt() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, p) = fixture(root.path());
        let state = seed(&mut host, "decision-1", Purpose::Plan, plan_decision());
        let wanted = target(p);
        host.management
            .prepare_application("decision-1", wanted.clone(), &state)
            .unwrap();
        if let ApplicationTarget::Workflow {
            principal,
            command,
            context,
        } = &wanted
        {
            host.projects
                .get_mut("project")
                .unwrap()
                .command(
                    "mgmt-decision-1",
                    principal,
                    command.clone(),
                    *context.clone(),
                    false,
                )
                .unwrap();
        }
        // Simulate a crash after the durable workflow event and before the receipt.
        assert!(!root
            .path()
            .join("data/management-decisions/decision-1.3.json")
            .exists());
        drop(host);
        let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(host.management_decisions()[0].state, DecisionState::Applied);
        assert_eq!(host.snapshot().workflows[0].sequence, 1);
        host.apply_management_decision("decision-1", wanted)
            .unwrap();
        assert_eq!(host.snapshot().workflows[0].sequence, 1);
        assert!(root
            .path()
            .join("data/management-decisions/decision-1.3.json")
            .is_file());
    }
    #[test]
    fn restart_before_workflow_commit_waits_for_validated_continuation() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, p) = fixture(root.path());
        let state = seed(&mut host, "decision-1", Purpose::Plan, plan_decision());
        let wanted = target(p);
        host.management
            .prepare_application("decision-1", wanted.clone(), &state)
            .unwrap();
        drop(host);
        let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(
            host.management_decisions()[0].state,
            DecisionState::Applying
        );
        assert_eq!(host.snapshot().workflows[0].sequence, 0);
        host.apply_management_decision("decision-1", wanted)
            .unwrap();
        assert_eq!(host.snapshot().workflows[0].sequence, 1);
    }
    #[test]
    fn later_application_conflict_never_publishes_an_earlier_wait_receipt() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, p) = fixture(root.path());
        let wait_state = seed(
            &mut host,
            "a-wait",
            Purpose::Assign,
            ManagementDecision::Wait {
                reason: "Await user clarification".into(),
            },
        );
        host.management
            .prepare_application("a-wait", ApplicationTarget::Wait, &wait_state)
            .unwrap();
        host.management.record_applied("a-wait", None).unwrap();
        // Build individually valid historical records, then remove only the
        // fixture's earlier receipt to simulate incomplete recovered evidence.
        let mut intent = host.management.intent("a-wait").unwrap().clone();
        intent.operation_id = "z-conflict".into();
        intent.purpose = Purpose::Plan;
        let state = intent.state_digest.clone();
        host.management.begin(intent).unwrap();
        host.management
            .finish(
                "z-conflict",
                Ok(DecisionObservation {
                    response_id: "resp_conflicting_application_fixture".into(),
                    requested_model: "fixture-model".into(),
                    actual_model: "fixture-model".into(),
                    input_tokens: 10,
                    output_tokens: 20,
                    envelope: DecisionEnvelope {
                        state_digest: state.clone(),
                        decision: plan_decision(),
                    },
                }),
            )
            .unwrap();
        host.management
            .prepare_application("z-conflict", target(p.clone()), &state)
            .unwrap();

        // Both ledgers are individually valid, but this committed plan differs
        // from the management target under the same operation identity.
        let mut changed = p;
        changed.workflow.tasks[0].goal = "A different independently valid goal".into();
        let ApplicationTarget::Workflow {
            principal,
            command,
            context,
        } = target(changed)
        else {
            unreachable!()
        };
        let entry = host
            .projects
            .get_mut("project")
            .unwrap()
            .command("mgmt-z-conflict", &principal, command, *context, false)
            .unwrap();
        host.management
            .record_applied("z-conflict", Some(entry.hash))
            .unwrap();
        assert_eq!(host.management.statuses()[0].operation_id, "a-wait");
        let data = root.path().join("data");
        let wait_receipt = data.join("management-decisions/a-wait.3.json");
        let existing_receipt = data.join("management-decisions/z-conflict.3.json");
        let original_receipt = fs::read(&existing_receipt).unwrap();
        fs::remove_file(&wait_receipt).unwrap();
        let pending_target = data.join("management-decisions/a-wait.2.json");
        let pending = pending_target.with_extension("pending");
        fs::rename(&pending_target, &pending).unwrap();
        let pending_original = fs::read(&pending).unwrap();
        host.management = crate::management_execution::DecisionJournal::open(&data).unwrap();
        assert!(!wait_receipt.exists());

        assert!(host
            .reconcile_management_applications()
            .unwrap_err()
            .contains("management workflow event differs from application intent"));
        assert!(!wait_receipt.exists());
        drop(host);
        // Failure must come from cross-ledger validation, not journal parsing.
        assert!(crate::management_execution::DecisionJournal::open(&data).is_ok());
        assert!(RuntimeHost::open(&data)
            .err()
            .unwrap()
            .contains("management workflow event differs from application intent"));
        assert!(!wait_receipt.exists());
        assert_eq!(fs::read(existing_receipt).unwrap(), original_receipt);
        assert!(!pending_target.exists());
        assert_eq!(fs::read(pending).unwrap(), pending_original);
    }

    #[test]
    fn application_receipt_without_matching_workflow_is_not_trusted() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, p) = fixture(root.path());
        let state = seed(&mut host, "decision-1", Purpose::Plan, plan_decision());
        host.management
            .prepare_application("decision-1", target(p), &state)
            .unwrap();
        // Well-formed local receipt alone is insufficient without an actual event.
        host.management
            .record_applied("decision-1", Some("a".repeat(64)))
            .unwrap();
        drop(host);
        assert!(RuntimeHost::open(&root.path().join("data"))
            .err()
            .unwrap()
            .contains("lacks workflow evidence"));
    }

    #[test]
    fn reserved_attempt_rejects_foreign_historical_assignment_without_a_job() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, p) = fixture(root.path());
        host.propose("project", "fixture-proposal", p.clone())
            .unwrap();
        host.approve("project", &p.approval_digest().unwrap())
            .unwrap();
        let state = seed(
            &mut host,
            "decision-1",
            Purpose::Assign,
            ManagementDecision::Assign {
                task_id: "task".into(),
                worker_id: "worker-codex".into(),
                rationale: "Reserved application".into(),
            },
        );
        let command = WorkflowCommand::Assign {
            task: "task".into(),
            worker: "worker-codex".into(),
            attempt: "reserved-attempt".into(),
        };
        let context = CommandContext {
            execution: Some(ExecutionEvidence::Attempt {
                attempt: Attempt {
                    workspace_proof: None,
                    id: "reserved-attempt".into(),
                    task_id: "task".into(),
                    worker_id: "worker-codex".into(),
                    revision_digest: p.approval_digest().unwrap(),
                    fence: 1,
                    binding: SessionBinding {
                        adapter_id: "codex".into(),
                        adapter_version: "fixture".into(),
                        session_id: "reserved-session".into(),
                        identity_strength: IdentityStrength::ProviderId,
                        workspace: p.project.root,
                    },
                },
            }),
            ..Default::default()
        };
        host.management
            .prepare_application(
                "decision-1",
                ApplicationTarget::Workflow {
                    principal: "deputy".into(),
                    command: command.clone(),
                    context: Box::new(context.clone()),
                },
                &state,
            )
            .unwrap();
        let store = host.projects.get_mut("project").unwrap();
        store
            .command("foreign-assignment", "deputy", command, context, false)
            .unwrap();
        store
            .command(
                "release-foreign-assignment",
                "user",
                WorkflowCommand::AbandonUnsent {
                    task: "task".into(),
                    attempt: "reserved-attempt".into(),
                    reason: "Historical collision must remain detectable".into(),
                },
                CommandContext::default(),
                false,
            )
            .unwrap();
        assert_eq!(store.state().tasks["task"].state, TaskState::Pending);
        assert!(store.state().tasks["task"].attempt.is_none());
        assert!(host.codex_jobs.is_empty());
        assert!(host
            .reconcile_management_applications()
            .unwrap_err()
            .contains("reserved management attempt conflicts with workflow assignment history"));
        drop(host);
        assert!(RuntimeHost::open(&root.path().join("data"))
            .err()
            .unwrap()
            .contains("reserved management attempt conflicts with workflow assignment history"));
        assert!(!root
            .path()
            .join("data/management-decisions/decision-1.3.json")
            .exists());
    }

    #[test]
    fn chief_acceptance_cannot_bypass_real_independent_verification() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, p) = fixture(root.path());
        host.propose("project", "fixture-proposal", p.clone())
            .unwrap();
        host.approve("project", &p.approval_digest().unwrap())
            .unwrap();
        seed(
            &mut host,
            "decision-1",
            Purpose::Accept,
            ManagementDecision::Accept {
                task_id: "task".into(),
                attempt_id: "unverified-attempt".into(),
                rationale: "Model falsely claims success".into(),
            },
        );
        let wanted = ApplicationTarget::Workflow {
            principal: "chief".into(),
            command: WorkflowCommand::Accept {
                task: "task".into(),
                attempt: "unverified-attempt".into(),
            },
            context: Box::default(),
        };
        assert!(host
            .apply_management_decision("decision-1", wanted)
            .is_err());
        assert_eq!(
            host.snapshot().workflows[0].state.tasks["task"].state,
            TaskState::Pending
        );
        assert_eq!(host.snapshot().workflows[0].sequence, 2);
        assert_eq!(
            host.management_decisions()[0].state,
            DecisionState::Applying
        );
    }
    #[test]
    fn deputy_assignment_and_advice_only_commit_bound_control_events() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, p) = fixture(root.path());
        host.propose("project", "fixture-proposal", p.clone())
            .unwrap();
        host.approve("project", &p.approval_digest().unwrap())
            .unwrap();
        seed(
            &mut host,
            "decision-1",
            Purpose::Assign,
            ManagementDecision::Assign {
                task_id: "task".into(),
                worker_id: "worker-codex".into(),
                rationale: "Approved eligible task".into(),
            },
        );
        let attempt = Attempt {
            workspace_proof: None,
            id: "bound-attempt".into(),
            task_id: "task".into(),
            worker_id: "worker-codex".into(),
            revision_digest: p.approval_digest().unwrap(),
            fence: 1,
            binding: SessionBinding {
                adapter_id: "codex".into(),
                adapter_version: "fixture".into(),
                session_id: "synthetic-session".into(),
                identity_strength: IdentityStrength::ProviderId,
                workspace: p.project.root,
            },
        };
        let assignment = ApplicationTarget::Workflow {
            principal: "deputy".into(),
            command: WorkflowCommand::Assign {
                task: "task".into(),
                worker: "worker-codex".into(),
                attempt: "bound-attempt".into(),
            },
            context: Box::new(CommandContext {
                execution: Some(ExecutionEvidence::Attempt { attempt }),
                ..Default::default()
            }),
        };
        host.apply_management_decision("decision-1", assignment)
            .unwrap();
        assert_eq!(
            host.snapshot().workflows[0].state.tasks["task"].state,
            TaskState::Assigned
        );
        seed(
            &mut host,
            "decision-2",
            Purpose::Advise,
            ManagementDecision::Advise {
                task_id: "task".into(),
                suggestion: "Check the failing test before retrying".into(),
                rationale: "Bounded supervision".into(),
            },
        );
        host.apply_management_decision(
            "decision-2",
            ApplicationTarget::Workflow {
                principal: "deputy".into(),
                command: WorkflowCommand::Advise {
                    task: "task".into(),
                    suggestion: "Check the failing test before retrying".into(),
                },
                context: Box::default(),
            },
        )
        .unwrap();
        assert_eq!(host.snapshot().workflows[0].state.advice.len(), 1);
        assert_eq!(host.snapshot().workflows[0].sequence, 4);
        assert!(host.codex_active.is_empty());
        assert!(!host.projects["project"]
            .history()
            .iter()
            .any(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. })));
        drop(host);
        let host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert!(host
            .management_decisions()
            .iter()
            .all(|s| s.state == DecisionState::Applied));
    }
}

#[cfg(all(test, windows))]
mod bridge_tests {
    use super::tests::{fixture, seed};
    use super::*;
    use crate::async_codex::{ActiveCodex, Update};
    use crate::management_execution::DecisionState;
    use cerulean_domain::management::{ManagementDecision, Purpose};
    use std::{
        sync::{
            atomic::{AtomicUsize, Ordering},
            mpsc, Arc,
        },
        time::{Duration, Instant},
    };

    fn decision(host: &mut RuntimeHost) {
        seed(
            host,
            "deputy-1",
            Purpose::Assign,
            ManagementDecision::Assign {
                task_id: "task".into(),
                worker_id: "worker-codex".into(),
                rationale: "Execute approved bounded task".into(),
            },
        );
    }
    fn fixture_worker() -> WorkerSelection {
        WorkerSelection {
            omp_runtime_parent: None,
            provider: "openai".into(),
            model: "fixture-model".into(),
            executable: ValidationFile {
                path: "C:/fixture/codex.exe".into(),
                sha256: "a".repeat(64),
            },
        }
    }
    fn simulated_worker(
        host: &mut RuntimeHost,
        p: &PlanRevision,
    ) -> (std::thread::JoinHandle<()>, Arc<AtomicUsize>) {
        let store = host.projects.remove("project").unwrap();
        let snapshot = crate::async_verification::snapshot(&store);
        let now = chrono::Utc::now().to_rfc3339();
        let job = CodexJob {
            management_operation: Some("deputy-1".into()),
            project_id: "project".into(),
            task_id: "task".into(),
            attempt_id: "attempt-1".into(),
            revision_digest: p.approval_digest().unwrap(),
            worker: fixture_worker(),
            state: CodexJobState::Starting,
            started_at: now.clone(),
            updated_at: now,
            message: None,
        };
        let (sender, receiver) = mpsc::channel();
        host.codex_jobs
            .insert(("project".into(), "attempt-1".into()), job.clone());
        host.codex_active.insert(
            (job.project_id.clone(), job.attempt_id.clone()),
            ActiveCodex {
                preparation_cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                advice_wake: tokio::sync::watch::channel(()).0,
                advice_gate: std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0)),
                observation: None,
                observation_saved: None,
                observation_error: None,
                snapshot,
                job: job.clone(),
                receiver,
                disconnected: false,
                cancel: None,
            },
        );
        let count = Arc::new(AtomicUsize::new(0));
        let after_ack = count.clone();
        let attempt = Attempt {
            workspace_proof: None,
            id: "attempt-1".into(),
            task_id: "task".into(),
            worker_id: "worker-codex".into(),
            revision_digest: p.approval_digest().unwrap(),
            fence: 1,
            binding: SessionBinding {
                adapter_id: "codex".into(),
                adapter_version: "fixture".into(),
                session_id: "fixture-session".into(),
                identity_strength: IdentityStrength::ProviderId,
                workspace: p.project.root.clone(),
            },
        };
        let writer =
            crate::async_api::ApiWorkflow::start(store, host.root.join("workflows/project"))
                .unwrap();
        let mut store = writer
            .owner
            .codex_client(crate::workflow_coordinator::Scope {
                project: job.project_id.clone(),
                task: job.task_id.clone(),
                attempt: job.attempt_id.clone(),
                revision: job.revision_digest.clone(),
            })
            .unwrap();
        host.api_writers.insert("project".into(), writer);
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let outcome = runtime.block_on(crate::async_codex::managed_assign(
                &mut store, &job, &sender, attempt,
            ));
            if outcome.is_ok() {
                after_ack.fetch_add(1, Ordering::SeqCst);
            }
            use crate::codex_workflow_access::CodexWorkflowAccess;
            let interrupted = store.requires_reopen();
            drop(store);
            sender
                .send(Update::Done {
                    interrupted,
                    cleaned: true,
                    outcome,
                })
                .unwrap();
        });
        (thread, count)
    }
    fn next_message(host: &RuntimeHost) -> Update {
        host.codex_active[&("project".into(), "attempt-1".into())]
            .receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
    }
    fn drain(host: &mut RuntimeHost, thread: std::thread::JoinHandle<()>) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !host.codex_active.is_empty() && Instant::now() < deadline {
            host.poll_verifications();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(host.codex_active.is_empty());
        thread.join().unwrap();
    }
    #[test]
    fn managed_assignment_requires_both_acknowledgements_and_writes_one_event() {
        for lost in ["none", "prepare", "commit"] {
            let root = tempfile::tempdir().unwrap();
            let (mut host, p) = fixture(root.path());
            host.propose("project", "proposal", p.clone()).unwrap();
            host.approve("project", &p.approval_digest().unwrap())
                .unwrap();
            decision(&mut host);
            let (thread, after_ack) = simulated_worker(&mut host, &p);
            if lost == "prepare" {
                drop(next_message(&host));
            } else if lost == "commit" {
                match next_message(&host) {
                    Update::PrepareAssignment {
                        operation,
                        attempt,
                        snapshot,
                        reply,
                    } => {
                        reply
                            .send(host.prepare_managed_codex_assignment(
                                "project", &operation, *attempt, *snapshot,
                            ))
                            .unwrap();
                    }
                    _ => panic!("preparation required first"),
                }
                match next_message(&host) {
                    Update::CommitAssignment {
                        operation,
                        entry,
                        snapshot,
                        reply,
                    } => {
                        host.commit_managed_codex_assignment(
                            "project", &operation, *entry, *snapshot,
                        )
                        .unwrap();
                        drop(reply);
                    }
                    _ => panic!("commit required second"),
                }
            }
            drain(&mut host, thread);
            assert_eq!(
                after_ack.load(Ordering::SeqCst),
                usize::from(lost == "none")
            );
            let assignments = host.projects["project"]
                .history()
                .iter()
                .filter(|e| matches!(e.body.event.command, WorkflowCommand::Assign { .. }))
                .collect::<Vec<_>>();
            assert_eq!(assignments.len(), usize::from(lost != "prepare"));
            if let Some(entry) = assignments.first() {
                assert_eq!(entry.body.operation_id, "mgmt-deputy-1");
            }
            assert!(!host.projects["project"]
                .history()
                .iter()
                .any(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. })));
            assert!(host
                .start_managed_codex("deputy-1", "different-attempt")
                .is_err());
        }
    }

    #[test]
    fn durable_prepare_with_lost_ack_can_close_after_worker_releases_ownership() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, mut p) = fixture(root.path());
        p.execution[0].worker = Some(fixture_worker());
        host.propose("project", "proposal", p.clone()).unwrap();
        host.approve("project", &p.approval_digest().unwrap())
            .unwrap();
        decision(&mut host);
        let (thread, after_ack) = simulated_worker(&mut host, &p);
        let application = root
            .path()
            .join("data/management-decisions/deputy-1.2.json");
        match next_message(&host) {
            Update::PrepareAssignment {
                operation,
                attempt,
                snapshot,
                reply,
            } => {
                host.prepare_managed_codex_assignment("project", &operation, *attempt, *snapshot)
                    .unwrap();
                assert!(
                    application.is_file(),
                    "Prepare must be durable before losing its ACK"
                );
                assert!(
                    host.application_recovery_review("deputy-1").is_err(),
                    "the live worker still owns the project"
                );
                drop(reply);
            }
            _ => panic!("preparation required first"),
        }
        let original = fs::read(&application).unwrap();
        drain(&mut host, thread);
        assert_eq!(after_ack.load(Ordering::SeqCst), 0);
        assert!(host.codex_active.is_empty());
        assert_eq!(host.projects["project"].history().len(), 2);
        assert!(!host.projects["project"]
            .history()
            .iter()
            .any(|entry| matches!(
                entry.body.event.command,
                WorkflowCommand::Assign { .. } | WorkflowCommand::DispatchIntent { .. }
            )));
        let review = host.application_recovery_review("deputy-1").unwrap();
        host.abandon_management_application(
            "deputy-1",
            &review.evidence_digest,
            "Close preparation after lost acknowledgement",
        )
        .unwrap();
        assert_eq!(fs::read(&application).unwrap(), original);
        assert!(root
            .path()
            .join("data/management-decisions/deputy-1.4.json")
            .is_file());
        assert!(!root
            .path()
            .join("data/management-decisions/deputy-1.3.json")
            .exists());
        assert_eq!(
            host.codex_jobs.values().next().unwrap().state,
            CodexJobState::Abandoned
        );
        assert_eq!(host.projects["project"].state().tasks["task"].fence, 0);
        drop(host);
        let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
        host.poll_verifications();
        assert_eq!(
            host.management_decisions()[0].state,
            DecisionState::Abandoned
        );
        assert_eq!(
            host.codex_jobs.values().next().unwrap().state,
            CodexJobState::Abandoned
        );
        assert_eq!(host.projects["project"].history().len(), 2);
        assert!(!host.projects["project"]
            .history()
            .iter()
            .any(|entry| matches!(
                entry.body.event.command,
                WorkflowCommand::Assign { .. } | WorkflowCommand::DispatchIntent { .. }
            )));
        assert_eq!(fs::read(application).unwrap(), original);
        assert!(!root
            .path()
            .join("data/management-decisions/deputy-1.3.json")
            .exists());
        assert!(host.codex_active.is_empty() && host.management_active.is_empty());
    }

    #[test]
    fn managed_assignment_rejects_changed_context_and_unpublished_event() {
        for changed_context in [true, false] {
            let root = tempfile::tempdir().unwrap();
            let (mut host, p) = fixture(root.path());
            host.propose("project", "proposal", p.clone()).unwrap();
            host.approve("project", &p.approval_digest().unwrap())
                .unwrap();
            decision(&mut host);
            let (thread, after_ack) = simulated_worker(&mut host, &p);
            if changed_context {
                host.validators.get_mut("tests").unwrap().name = "Changed while preparing".into();
            } else {
                match next_message(&host) {
                    Update::PrepareAssignment {
                        operation,
                        attempt,
                        snapshot,
                        reply,
                    } => {
                        reply
                            .send(host.prepare_managed_codex_assignment(
                                "project", &operation, *attempt, *snapshot,
                            ))
                            .unwrap();
                    }
                    _ => panic!("prepare expected"),
                }
                match next_message(&host) {
                    Update::CommitAssignment {
                        operation,
                        mut entry,
                        snapshot,
                        reply,
                    } => {
                        entry.body.received_at = chrono::Utc::now().to_rfc3339();
                        entry.hash = digest(&entry.body).unwrap();
                        let rejected = host.commit_managed_codex_assignment(
                            "project", &operation, *entry, *snapshot,
                        );
                        assert!(rejected.is_err());
                        reply.send(rejected).unwrap();
                    }
                    _ => panic!("commit expected"),
                }
            }
            drain(&mut host, thread);
            assert_eq!(after_ack.load(Ordering::SeqCst), 0);
            assert_ne!(host.management_decisions()[0].state, DecisionState::Applied);
        }
    }

    #[test]
    #[ignore = "explicit one-turn real Codex bridge test; fresh report directory required"]
    fn live_managed_codex_assignment_and_verification() {
        let root = PathBuf::from(
            std::env::var_os("CERULEAN_MANAGED_LIVE_DIR").expect("fresh report directory required"),
        )
        .canonicalize()
        .unwrap();
        let reports = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../reports")
            .canonicalize()
            .unwrap();
        assert!(root.starts_with(reports) && !root.join("data").exists());
        let executable = PathBuf::from(
            std::env::var_os("CERULEAN_CODEX_EXE").expect("Codex executable required"),
        );
        let node =
            PathBuf::from(std::env::var_os("CERULEAN_NODE_EXE").expect("Node executable required"));
        let model = std::env::var("CERULEAN_CODEX_MODEL").expect("model required");
        let (mut host, mut p) = fixture(&root);
        p.execution[0].worker =
            Some(RuntimeHost::describe_codex_worker(&executable, &model).unwrap());
        p.policy.max_requests = 1;
        p.policy.max_seconds = 180;
        p.workflow.tasks[0].goal="Create sum.mjs as an ES module exporting function sum(a,b) which returns a+b for numeric inputs. Keep the implementation minimal.".into();
        let check = root.join("check-sum.mjs");
        fs::write(&check,"import assert from 'node:assert/strict'; import {pathToFileURL} from 'node:url'; import {resolve} from 'node:path'; const {sum}=await import(pathToFileURL(resolve('sum.mjs'))); const cases=[[0,0,0],[1,2,3],[-1,1,0],[-2,-3,-5],[0.5,0.25,0.75],[100,200,300],[1,-2,-1],[0,4,4],[7,0,7],[99,1,100]]; for(const [a,b,c] of cases) assert.equal(sum(a,b),c); console.log('10 independent assertions passed');").unwrap();
        host.register_validator(
            "node-check".into(),
            "Independent sum checks".into(),
            node.to_string_lossy().into(),
            vec![check.to_string_lossy().into()],
            Default::default(),
        )
        .unwrap();
        p.execution[0].checks[0].program = "node".into();
        p.execution[0].checks[0].args = vec![check.to_string_lossy().into()];
        p.execution[0].checks[0].tool = Some(
            host.registered_validator("node-check")
                .unwrap()
                .tool
                .clone(),
        );
        host.propose("project", "fixture-chief", p.clone()).unwrap();
        host.approve("project", &p.approval_digest().unwrap())
            .unwrap();
        decision(&mut host);
        fs::write(
            root.join("approved-plan.json"),
            serde_json::to_vec_pretty(&p).unwrap(),
        )
        .unwrap();
        fs::write(root.join("result.json"),serde_json::to_vec_pretty(&serde_json::json!({"status":"starting","maxWorkerTurns":1,"managementModelCalls":0,"at":chrono::Utc::now().to_rfc3339()})).unwrap()).unwrap();
        host.start_managed_codex("deputy-1", "managed-attempt")
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(240);
        let mut polls = 0;
        while !host.codex_active.is_empty() && Instant::now() < deadline {
            host.poll_verifications();
            polls += 1;
            std::thread::sleep(Duration::from_millis(30));
        }
        assert!(host.codex_active.is_empty(), "live Codex deadline");
        assert_eq!(
            host.snapshot().codex_jobs[0].state,
            CodexJobState::Submitted,
            "{:?}",
            host.snapshot().codex_jobs
        );
        assert_eq!(host.management_decisions()[0].state, DecisionState::Applied);
        assert!(host
            .start_managed_codex("deputy-1", "repeat-attempt")
            .is_err());
        let history = host.projects["project"].history();
        assert_eq!(
            history
                .iter()
                .filter(|e| matches!(e.body.event.command, WorkflowCommand::Assign { .. }))
                .count(),
            1
        );
        assert_eq!(
            history
                .iter()
                .filter(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. }))
                .count(),
            1
        );
        let assignment = history
            .iter()
            .find(|e| e.body.operation_id == "mgmt-deputy-1")
            .unwrap();
        let entry_hash = assignment.hash.clone();
        let Some(ExecutionEvidence::Attempt { attempt }) = &assignment.body.context.execution
        else {
            panic!("binding missing")
        };
        let thread_id = attempt.binding.session_id.clone();
        host.start_verification(
            "project",
            "managed-verify",
            "task",
            "managed-attempt",
            vec![crate::verifier::PinnedCheck::from_approved(&p.execution[0].checks[0]).unwrap()],
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(60);
        while !host.active.is_empty() && Instant::now() < deadline {
            host.poll_verifications();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(host.snapshot().workflows[0].state.tasks["task"].verified);
        drop(host);
        let mut host = RuntimeHost::open(&root.join("data")).unwrap();
        assert_eq!(host.management_decisions()[0].state, DecisionState::Applied);
        assert!(host
            .start_managed_codex("deputy-1", "restart-repeat")
            .is_err());
        assert!(host.snapshot().workflows[0].state.tasks["task"].verified);
        fs::write(root.join("result.json"),serde_json::to_vec_pretty(&serde_json::json!({"at":chrono::Utc::now().to_rfc3339(),"status":"passed","scope":"synthetic deputy response and user approval fixture; real Codex worker and independent verifier","workerModelCalls":1,"managementModelCalls":0,"assignments":1,"dispatchIntents":1,"duplicateRequestsRejected":true,"restartResends":0,"independentAssertions":10,"assignmentEventHash":entry_hash,"threadId":thread_id,"polls":polls,"chiefAccepted":false})).unwrap()).unwrap();
    }
}
