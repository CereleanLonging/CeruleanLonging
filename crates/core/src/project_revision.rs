//! User-authored replacement of a quiescent plan, including undispatched work.
use super::*;
use cerulean_domain::project_setup::{ProjectRevisionReview, RunState};
impl RuntimeHost {
    pub(super) fn has_run_revision_request(
        &self,
        run: &project_setup::ProjectRun,
    ) -> Result<bool, String> {
        let store = self
            .projects
            .get(&run.project_id)
            .ok_or("运行项目账本缺失")?;
        let mut original_plan = None;
        for entry in store.history() {
            match &entry.body.event.command {
                WorkflowCommand::Propose { .. } => {
                    original_plan = match &entry.body.context.execution {
                        Some(ExecutionEvidence::Revision { revision })
                            if revision.approval_digest()? == run.revision_digest =>
                        {
                            Some(workflow_digest(&revision.workflow)?)
                        }
                        _ => None,
                    };
                }
                WorkflowCommand::RequestRevision { digest, .. }
                    if original_plan.as_ref() == Some(digest)
                        && entry.body.context.approval_source.as_deref()
                            == Some("authenticated local-user revision request") =>
                {
                    return Ok(true)
                }
                WorkflowCommand::RetireSubmittedPlan { digest, .. }
                    if original_plan.as_ref() == Some(digest)
                        && entry.body.context.approval_source.as_deref()
                            == Some("authenticated local-user submitted-plan retirement") =>
                {
                    return Ok(true)
                }
                _ => {}
            }
        }
        Ok(false)
    }
    // Derived closure only: never dispatch, approve, or synthesize a revision request.
    pub(super) fn recover_revision_runs(&mut self) -> Result<(), String> {
        for mut run in self
            .runs
            .all()
            .into_iter()
            .filter(|r| r.state == RunState::Paused)
        {
            if self.has_run_revision_request(&run)? {
                run.state = RunState::Superseded;
                run.message = "用户已请求修订；原运行已结束，下一版须重新批准并启动".into();
                self.runs.save(run, "core")?;
            }
        }
        Ok(())
    }
    fn project_revision_review(&self, project: &str) -> Result<ProjectRevisionReview, String> {
        identifier(project)?;
        self.require_available(project)?;
        require(
            !self
                .management
                .statuses()
                .iter()
                .any(|s| s.project_id == project && !s.state.resolved()),
            "项目有待处理指挥请求，请先处置",
        )?;
        let store = self.projects.get(project).ok_or("项目未登记")?;
        let state = store.state();
        require(
            state.revision_requested.is_none()
                && state.approved
                && !state.tasks.is_empty()
                && state.tasks.values().all(|t| {
                    matches!(
                        t.state,
                        TaskState::Pending
                            | TaskState::Accepted
                            | TaskState::Cancelled
                            | TaskState::Retired
                            | TaskState::Submitted
                    )
                }),
            "请先完成或处置已派发任务，再修订完整方案",
        )?;
        let revision_digest = store.revision().ok_or("方案缺失")?.approval_digest()?;
        let runs: Vec<_> = self
            .runs
            .all()
            .into_iter()
            .filter(|r| r.project_id == project)
            .collect();
        let evidence_digest = digest(&(
            project,
            &revision_digest,
            store.history().last().map(|e| &e.hash),
            &runs,
            self.project_setups.get(project),
        ))?;
        Ok(ProjectRevisionReview {
            project_id: project.into(),
            revision_digest,
            evidence_digest,
            accepted_tasks: state
                .tasks
                .values()
                .filter(|t| t.state == TaskState::Accepted)
                .count() as u32,
            cancelled_tasks: state
                .tasks
                .values()
                .filter(|t| t.state == TaskState::Cancelled)
                .count() as u32,
            retired_tasks: state
                .tasks
                .values()
                .filter(|t| t.state == TaskState::Retired)
                .count() as u32,
            pending_tasks: state
                .tasks
                .values()
                .filter(|t| t.state == TaskState::Pending)
                .count() as u32,
            submitted_tasks: state
                .tasks
                .values()
                .filter(|t| t.state == TaskState::Submitted)
                .count() as u32,
            verified_submissions: state
                .tasks
                .values()
                .filter(|t| t.state == TaskState::Submitted && t.verified)
                .count() as u32,
        })
    }
    pub(super) fn project_revision_candidates(&self) -> Vec<ProjectRevisionReview> {
        self.projects
            .keys()
            .filter_map(|id| self.project_revision_review(id).ok())
            .collect()
    }
    pub(super) fn request_project_revision(
        &mut self,
        project: &str,
        wanted: &str,
        reason: &str,
    ) -> Result<(), String> {
        self.request_plan_revision(project, wanted, reason, false)
    }
    pub(super) fn retire_submitted_plan(
        &mut self,
        project: &str,
        wanted: &str,
        reason: &str,
    ) -> Result<(), String> {
        self.request_plan_revision(project, wanted, reason, true)
    }
    fn request_plan_revision(
        &mut self,
        project: &str,
        wanted: &str,
        reason: &str,
        retire_submitted: bool,
    ) -> Result<(), String> {
        require(
            !reason.trim().is_empty() && reason.len() <= 8000 && !reason.contains('\0'),
            "请填写不超过8000字节的修订原因",
        )?;
        sha256(wanted)?;
        let review = self.project_revision_review(project)?;
        require(
            (review.submitted_tasks > 0) == retire_submitted,
            "已有提交需要明确选择结束本版提交；普通修订不能代替该确认",
        )?;
        require(
            review.evidence_digest == wanted,
            "方案或运行状态已变化，请重新审阅",
        )?;
        // Persist pause first. A later journal failure leaves the old plan safely paused.
        for run in self
            .runs
            .all()
            .into_iter()
            .filter(|r| r.project_id == project && !r.state.terminal())
        {
            self.pause_project_run(&run.id)?;
        }
        let store = self.projects.get_mut(project).ok_or("项目未登记")?;
        let operation = format!("revision-request-{}", store.history().len() + 1);
        let digest = store.state().digest.clone().ok_or("方案摘要缺失")?;
        let (command, audit) = if retire_submitted {
            (
                WorkflowCommand::RetireSubmittedPlan {
                    digest,
                    reason: reason.into(),
                },
                "authenticated local-user submitted-plan retirement",
            )
        } else {
            (
                WorkflowCommand::RequestRevision {
                    digest,
                    reason: reason.into(),
                },
                "authenticated local-user revision request",
            )
        };
        store.command(
            &operation,
            "user",
            command,
            CommandContext {
                approval_source: Some(audit.into()),
                ..Default::default()
            },
            true,
        )?;
        self.recover_revision_runs()
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::host::project_planning::tests::{complete, complete_at, fixture, proposal};
    use cerulean_domain::project_setup::ProjectSetupDraft;
    fn cancelled(root: &Path) -> (RuntimeHost, ProjectSetupDraft) {
        cancelled_with_dependent(root, false)
    }
    fn cancelled_with_dependent(root: &Path, pending: bool) -> (RuntimeHost, ProjectSetupDraft) {
        let (mut host, mut draft) = fixture(root);
        project_planning::tests::isolate_draft(root, &mut draft);
        host.configure_project(draft.clone()).unwrap();
        let mut decision = proposal("tests");
        if pending {
            if let management::ManagementDecision::Plan { tasks, .. } = &mut decision {
                let mut dependent = tasks[0].clone();
                dependent.id = "dependent".into();
                dependent.goal = "Only execute after the first task is accepted".into();
                dependent.dependencies = vec!["task".into()];
                tasks.push(dependent);
            }
        }
        complete(&mut host, decision);
        let revision = host.projects["project"].revision().unwrap().clone();
        host.approve("project", &revision.approval_digest().unwrap())
            .unwrap();
        let setup = host.project_setups["project"].digest.clone();
        host.start_project_run("project", &revision.approval_digest().unwrap(), &setup)
            .unwrap();
        let store = host.projects.get_mut("project").unwrap();
        let attempt = Attempt {
            workspace_proof: None,
            id: "old-attempt".into(),
            task_id: "task".into(),
            worker_id: "worker-codex".into(),
            revision_digest: revision.approval_digest().unwrap(),
            fence: 1,
            binding: SessionBinding {
                adapter_id: "codex".into(),
                adapter_version: "synthetic".into(),
                session_id: "old-thread".into(),
                identity_strength: IdentityStrength::ProviderId,
                workspace: revision.execution_root("task", "old-attempt").unwrap(),
            },
        };
        store
            .command(
                "assign-old",
                "deputy",
                WorkflowCommand::Assign {
                    task: "task".into(),
                    worker: "worker-codex".into(),
                    attempt: attempt.id.clone(),
                },
                CommandContext {
                    execution: Some(ExecutionEvidence::Attempt {
                        attempt: attempt.clone(),
                    }),
                    ..Default::default()
                },
                true,
            )
            .unwrap();
        store
            .command(
                "dispatch-old",
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
        store
            .command(
                "unknown-old",
                "core",
                WorkflowCommand::RecoveryUnknown {
                    task: "task".into(),
                    attempt: attempt.id.clone(),
                },
                CommandContext::default(),
                true,
            )
            .unwrap();
        let receipt = ExecutionReceipt {
            schema_version: 1,
            task_id: "task".into(),
            attempt_id: attempt.id.clone(),
            fence: 1,
            binding: attempt.binding.clone(),
            turn_id: Some("old-turn".into()),
            observed_at: chrono::Utc::now().to_rfc3339(),
            status: ReceiptStatus::Cancelled,
            evidence_sha256: "a".repeat(64),
        };
        store
            .command(
                "cancelled-old",
                "core",
                WorkflowCommand::Reconcile {
                    task: "task".into(),
                    attempt: attempt.id.clone(),
                    outcome: ReconcileOutcome::Cancelled,
                    evidence: vec!["synthetic cancellation proof".into()],
                },
                CommandContext {
                    thread_id: Some(attempt.binding.session_id),
                    turn_id: Some("old-turn".into()),
                    cwd: Some(attempt.binding.workspace),
                    execution: Some(ExecutionEvidence::Receipt {
                        receipt,
                        manifest: None,
                    }),
                    ..Default::default()
                },
                true,
            )
            .unwrap();
        (host, draft)
    }
    #[test]
    fn cancelled_run_revision_pauses_before_planning_and_never_reauthorizes_old_run() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, mut draft) = cancelled(root.path());
        let old_run = host.runs.all()[0].clone();
        assert!(host.configure_project(draft.clone()).is_err());
        let candidate = host.project_revision_candidates()[0].clone();
        let old_history = host.projects["project"].history().to_vec();
        host.request_project_revision("project", &candidate.evidence_digest, "重新规划已停止项目")
            .unwrap();
        assert_eq!(host.runs.all()[0].state, RunState::Superseded);
        assert!(host.pause_project_run(&old_run.id).is_err());
        assert_eq!(
            &host.projects["project"].history()[..old_history.len()],
            old_history.as_slice()
        );
        assert!(host
            .start_project_run("project", &old_run.revision_digest, &old_run.setup_digest)
            .is_err());
        assert_eq!(host.management.statuses().len(), 1);
        assert!(host.codex_jobs.is_empty());
        draft.requirement = "第二版需求".into();
        host.configure_project(draft.clone()).unwrap();
        let saved = host.project_setups["project"].clone();
        assert_eq!(saved.setup.version, 2);
        let mut decision = proposal("tests");
        if let management::ManagementDecision::Plan { tasks, .. } = &mut decision {
            tasks[0].goal = draft.requirement;
        }
        complete_at(&mut host, decision, "ui-plan-fixture-2");
        let revision = host.projects["project"].revision().unwrap().clone();
        assert_eq!(revision.workflow.version, 2, "{:?}", host.planning_errors);
        assert!(host.projects["project"].approval().is_none());
        assert_eq!(host.projects["project"].state().tasks["task"].fence, 1);
        assert!(host
            .start_project_run(
                "project",
                &revision.approval_digest().unwrap(),
                &saved.digest
            )
            .is_err());
        host.approve("project", &revision.approval_digest().unwrap())
            .unwrap();
        assert_eq!(host.runs.all().len(), 1);
        host.start_project_run(
            "project",
            &revision.approval_digest().unwrap(),
            &saved.digest,
        )
        .unwrap();
        let new_run = host
            .runs
            .all()
            .into_iter()
            .find(|r| r.revision_digest != old_run.revision_digest)
            .unwrap();
        host.pause_project_run(&new_run.id).unwrap();
        drop(host);
        let host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(
            host.runs
                .all()
                .iter()
                .filter(|r| r.state == RunState::Paused)
                .count(),
            1
        );
        assert_eq!(
            host.runs
                .all()
                .iter()
                .filter(|r| r.state == RunState::Superseded)
                .count(),
            1
        );
        assert_eq!(host.management.statuses().len(), 2);
        assert!(host.codex_jobs.is_empty());
        assert_eq!(
            &host.projects["project"].history()[..old_history.len()],
            old_history.as_slice()
        );
    }
    #[test]
    fn pending_dependents_are_explicitly_retired_with_the_plan_without_fake_cancellation() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, mut draft) = cancelled_with_dependent(root.path(), true);
        let candidate = host.project_revision_candidates()[0].clone();
        assert_eq!(candidate.pending_tasks, 1);
        assert_eq!(candidate.cancelled_tasks, 1);
        let before = host.projects["project"].history().to_vec();
        let old_run = host.runs.all()[0].clone();
        host.request_project_revision(
            "project",
            &candidate.evidence_digest,
            "停止原方案未开始的后继任务并重新规划",
        )
        .unwrap();
        let store = &host.projects["project"];
        assert_eq!(store.state().tasks["dependent"].state, TaskState::Pending);
        assert_eq!(store.history().len(), before.len() + 1);
        assert!(matches!(
            store.history().last().unwrap().body.event.command,
            WorkflowCommand::RequestRevision { .. }
        ));
        assert!(host
            .start_project_run("project", &old_run.revision_digest, &old_run.setup_digest)
            .is_err());
        assert!(host
            .start_codex(
                "project",
                "dependent",
                "forbidden-next",
                &old_run.revision_digest
            )
            .is_err());
        assert!(host.codex_jobs.is_empty());
        drop(host);
        let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert!(host.projects["project"]
            .state()
            .revision_requested
            .is_some());
        assert_eq!(host.runs.all()[0].state, RunState::Superseded);
        draft.requirement = "重新规划后不再包含旧后继任务".into();
        draft.management_max_requests = 4;
        host.configure_project(draft).unwrap();
        complete_at(
            &mut host,
            proposal("tests"),
            "ui-plan-without-old-dependent",
        );
        assert_eq!(
            host.projects["project"]
                .revision()
                .unwrap()
                .workflow
                .version,
            2
        );
        assert!(!host.projects["project"]
            .state()
            .tasks
            .contains_key("dependent"));
        assert_eq!(
            &host.projects["project"].history()[..before.len()],
            before.as_slice()
        );
        assert!(host.projects["project"].approval().is_none());
        assert!(host.codex_jobs.is_empty());
        drop(host);
        assert!(RuntimeHost::open(&root.path().join("data")).is_ok());
    }
    #[test]
    fn entirely_pending_approved_plan_can_be_withdrawn_but_assigned_work_cannot() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, draft) = fixture(root.path());
        host.configure_project(draft.clone()).unwrap();
        complete(&mut host, proposal("tests"));
        let revision = host.projects["project"].revision().unwrap().clone();
        host.approve("project", &revision.approval_digest().unwrap())
            .unwrap();
        let review = host.project_revision_candidates()[0].clone();
        assert_eq!(review.pending_tasks, 1);
        let store = host.projects.get_mut("project").unwrap();
        let attempt = Attempt {
            workspace_proof: None,
            id: "assigned-only".into(),
            task_id: "task".into(),
            worker_id: "worker-codex".into(),
            revision_digest: revision.approval_digest().unwrap(),
            fence: 1,
            binding: SessionBinding {
                adapter_id: "codex".into(),
                adapter_version: "synthetic".into(),
                session_id: "unstarted-thread".into(),
                identity_strength: IdentityStrength::ProviderId,
                workspace: revision.project.root.clone(),
            },
        };
        store
            .command(
                "assign-test",
                "deputy",
                WorkflowCommand::Assign {
                    task: "task".into(),
                    worker: "worker-codex".into(),
                    attempt: attempt.id.clone(),
                },
                CommandContext {
                    execution: Some(ExecutionEvidence::Attempt { attempt }),
                    ..Default::default()
                },
                true,
            )
            .unwrap();
        assert!(host.project_revision_candidates().is_empty());
        assert!(host
            .request_project_revision("project", &review.evidence_digest, "stale pending review")
            .is_err());
        host.projects
            .get_mut("project")
            .unwrap()
            .command(
                "abandon-test",
                "user",
                WorkflowCommand::AbandonUnsent {
                    task: "task".into(),
                    attempt: "assigned-only".into(),
                    reason: "withdraw unsent assignment".into(),
                },
                CommandContext::default(),
                true,
            )
            .unwrap();
        let review = host.project_revision_candidates()[0].clone();
        host.request_project_revision(
            "project",
            &review.evidence_digest,
            "withdraw unstarted plan",
        )
        .unwrap();
        host.configure_project(draft).unwrap();
        assert!(host.projects["project"]
            .state()
            .revision_requested
            .is_some());
        assert_eq!(host.projects["project"].state().tasks["task"].fence, 1);
    }
    #[test]
    fn committed_revision_recovers_missing_run_closure_once_without_dispatch() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, _) = cancelled(root.path());
        let old = host.runs.all()[0].clone();
        host.pause_project_run(&old.id).unwrap();
        let candidate = host.project_revision_candidates()[0].clone();
        let control = root.path().join("data/run-control");
        let held = root.path().join("held-run-control");
        fs::rename(&control, &held).unwrap();
        fs::write(&control, "closure publication failure").unwrap();
        assert!(host
            .request_project_revision("project", &candidate.evidence_digest, "recover closure")
            .is_err());
        assert!(host.projects["project"]
            .state()
            .revision_requested
            .is_some());
        assert_eq!(host.runs.all()[0].state, RunState::Paused);
        let history = host.projects["project"].history().to_vec();
        fs::remove_file(&control).unwrap();
        fs::rename(&held, &control).unwrap();
        drop(host);
        let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(host.runs.all()[0].state, RunState::Superseded);
        assert_eq!(host.projects["project"].history(), history.as_slice());
        assert!(host
            .start_project_run("project", &old.revision_digest, &old.setup_digest)
            .is_err());
        assert!(host.codex_jobs.is_empty());
        assert_eq!(host.management.statuses().len(), 1);
        let count = fs::read_dir(&control).unwrap().count();
        drop(host);
        let host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(fs::read_dir(&control).unwrap().count(), count);
        assert_eq!(host.projects["project"].history(), history.as_slice());
    }
    #[test]
    fn forged_revision_closure_without_user_request_is_rejected_on_open() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, _) = cancelled(root.path());
        let mut run = host.runs.all()[0].clone();
        run.state = RunState::Superseded;
        assert!(
            host.runs.save(run.clone(), "core").is_err(),
            "active cannot be superseded directly"
        );
        host.pause_project_run(&run.id).unwrap();
        host.runs.save(run, "core").unwrap();
        drop(host);
        let error = RuntimeHost::open(&root.path().join("data"))
            .err()
            .expect("missing workflow proof");
        assert!(error.contains("缺少用户修订证据"), "{error}");
    }
    #[test]
    fn revision_journal_failure_keeps_old_run_paused_and_can_be_retried() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, _) = cancelled(root.path());
        let candidate = host.project_revision_candidates()[0].clone();
        let events = root.path().join("data/workflows/project/events");
        let held = root.path().join("held-events");
        fs::rename(&events, &held).unwrap();
        fs::write(&events, "publication failure fixture").unwrap();
        assert!(host
            .request_project_revision(
                "project",
                &candidate.evidence_digest,
                "retry after storage recovery"
            )
            .is_err());
        assert_eq!(host.runs.all()[0].state, RunState::Paused);
        assert!(host.projects["project"]
            .state()
            .revision_requested
            .is_none());
        fs::remove_file(&events).unwrap();
        fs::rename(&held, &events).unwrap();
        drop(host);
        let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
        let candidate = host.project_revision_candidates()[0].clone();
        host.request_project_revision(
            "project",
            &candidate.evidence_digest,
            "retry after storage recovery",
        )
        .unwrap();
        assert!(host.projects["project"]
            .state()
            .revision_requested
            .is_some());
        assert_eq!(host.management.statuses().len(), 1);
    }
    #[test]
    #[ignore = "isolated native terminal revision fixture; synthetic worker cancellation"]
    fn seed_native_terminal_revision() {
        let root =
            fs::canonicalize(std::env::var("CERULEAN_REVISION_NATIVE_DIR").unwrap()).unwrap();
        let reports =
            fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports")).unwrap();
        assert!(root.starts_with(reports) && !root.join("data").exists());
        let (mut host, _) = cancelled_with_dependent(
            &root,
            std::env::var_os("CERULEAN_REVISION_PENDING").is_some(),
        );
        let run = host.runs.all()[0].clone();
        host.pause_project_run(&run.id).unwrap();
        assert_eq!(host.project_revision_candidates().len(), 1);
    }
    #[test]
    #[ignore = "isolated native next revision proposal; synthetic chief, no model calls"]
    fn propose_native_terminal_revision() {
        let root =
            fs::canonicalize(std::env::var("CERULEAN_REVISION_NATIVE_DIR").unwrap()).unwrap();
        let reports =
            fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports")).unwrap();
        assert!(root.starts_with(reports));
        let mut host = RuntimeHost::open(&root.join("data")).unwrap();
        assert!(host.projects["project"]
            .state()
            .revision_requested
            .is_some());
        assert_eq!(host.project_setups["project"].setup.version, 2);
        let mut decision = proposal(&host.project_setups["project"].setup.validator_id);
        if let management::ManagementDecision::Plan { tasks, .. } = &mut decision {
            tasks[0].goal = host.project_setups["project"].setup.requirement.clone();
            if std::env::var_os("CERULEAN_REVISION_SUBMITTED").is_some() {
                tasks[0].id = "a".into();
            }
        }
        complete_at(&mut host, decision, "ui-plan-native-revision-2");
        assert_eq!(
            host.projects["project"]
                .revision()
                .unwrap()
                .workflow
                .version,
            2,
            "{:?}",
            host.planning_errors
        );
        assert!(host.projects["project"].approval().is_none());
    }
}
