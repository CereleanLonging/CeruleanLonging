//! Closing an assignment is a local user action, not remote cancellation evidence.
//! Only the durable ledger can prove that no dispatch intent was ever published.
use super::*;

impl RuntimeHost {
    pub(super) fn unsent_review(
        &self,
        project: &str,
        task: &str,
        attempt: &str,
    ) -> Result<String, String> {
        for id in [project, task, attempt] {
            identifier(id)?;
        }
        #[cfg(windows)]
        require(
            !self.has_codex_activity(project)
                && !self.active.contains_key(project)
                && !self.has_omp_activity(project)
                && self
                    .desktop_active
                    .as_ref()
                    .is_none_or(|a| a.snapshot.project.id != project)
                && !self.has_api_activity(project),
            "执行或验收线程仍持有项目，不能关闭尝试",
        )?;
        require(
            !self
                .runs
                .all()
                .iter()
                .any(|r| r.project_id == project && r.state == project_setup::RunState::Active),
            "请先暂停项目调度，再关闭未派发尝试",
        )?;
        require(
            !self.management_active.keys().any(|id| {
                self.management
                    .intent(id)
                    .is_ok_and(|i| i.project_id == project)
            }),
            "项目指挥请求仍在运行",
        )?;
        let management: Vec<_> = self
            .management
            .statuses()
            .into_iter()
            .filter(|s| s.project_id == project)
            .collect();
        require(
            management.iter().all(|s| s.state.resolved()),
            "请先核对未完成的指挥应用事务",
        )?;
        require(
            !self
                .jobs
                .values()
                .any(|j| j.project_id == project && j.state == VerificationState::Unknown),
            "项目存在待对账验收",
        )?;
        require(
            !self.codex_jobs.values().any(|j| {
                j.project_id == project
                    && j.attempt_id != attempt
                    && j.state == CodexJobState::Unknown
            }),
            "项目存在另一条待对账编程尝试",
        )?;
        require(
            !self.omp_jobs.values().any(|j| {
                j.project_id == project
                    && j.attempt_id != attempt
                    && matches!(
                        j.state,
                        omp_jobs::OmpJobState::Unknown | omp_jobs::OmpJobState::Failed
                    )
            }),
            "项目存在另一条待核对OMP尝试",
        )?;
        let store = self.projects.get(project).ok_or("项目账本尚未恢复")?;
        #[cfg(windows)]
        require(
            self.api_fault.is_none()
                && !self.api_jobs.values().any(|j| {
                    j.project_id == project
                        && j.attempt_id != attempt
                        && j.state == api_jobs::ApiJobState::Unknown
                }),
            "API账本需要重开或存在其他未决尝试",
        )?;
        require(!store.requires_reopen(), "项目账本需要重开核心恢复")?;
        let run = store.state().tasks.get(task).ok_or("任务不存在")?;
        require(
            run.state == TaskState::Assigned && run.attempt.as_deref() == Some(attempt),
            "当前任务不是该未派发分配",
        )?;
        let binding = store
            .attempt_record(task, attempt)
            .ok_or("原始分配证据缺失")?;
        require(
            !store.history().iter().any(|e| {
                matches!(&e.body.event.command,
            WorkflowCommand::DispatchIntent { attempt: a, .. } if a == attempt)
            }),
            "原尝试已有派发意图，必须核对原执行结果",
        )?;
        let job = self.codex_jobs.get(&(project.into(), attempt.into()));
        if let Some(job) = job {
            require(
                job.task_id == task
                    && job.revision_digest == binding.revision_digest
                    && job.state == CodexJobState::Unknown,
                "编程记录与未派发分配不符",
            )?;
        }
        let management_heads = management
            .iter()
            .map(|s| {
                self.management
                    .evidence_digest(&s.operation_id)
                    .map(|head| (s.operation_id.as_str(), head))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let runs: Vec<_> = self
            .runs
            .all()
            .into_iter()
            .filter(|r| r.project_id == project)
            .collect();
        digest(&serde_json::json!({
            "purpose": "abandon-unsent-v1", "project": project, "task": task,
            "attempt": attempt, "binding": binding, "head": store.history().last().map(|e| &e.hash),
            "job": job, "management": management_heads, "runs": runs,
        }))
    }

    pub(super) fn unsent_assignments(&self) -> Vec<UnsentAssignmentReview> {
        let mut reviews = Vec::new();
        for (project, store) in &self.projects {
            for (task, run) in &store.state().tasks {
                if run.state != TaskState::Assigned {
                    continue;
                }
                let Some(attempt) = &run.attempt else {
                    continue;
                };
                if let Ok(evidence_digest) = self.unsent_review(project, task, attempt) {
                    reviews.push(UnsentAssignmentReview {
                        project_id: project.clone(),
                        task_id: task.clone(),
                        attempt_id: attempt.clone(),
                        evidence_digest,
                    });
                }
            }
        }
        reviews
    }

    pub(super) fn abandon_unsent_attempt(
        &mut self,
        project: &str,
        task: &str,
        attempt: &str,
        expected: &str,
        reason: &str,
    ) -> Result<(), String> {
        sha256(expected)?;
        require(
            !reason.trim().is_empty() && reason.len() <= 8000 && !reason.contains('\0'),
            "请输入不超过8000字节的处置原因",
        )?;
        require(
            self.unsent_review(project, task, attempt)? == expected,
            "尝试证据已变化，请刷新后重新审阅",
        )?;
        // Single host writer and idle project guarantee no worker can race this check/commit.
        // The immutable event is authoritative even if saving the mutable job fails afterward.
        let store = self.projects.get_mut(project).ok_or("项目账本不存在")?;
        store.command(
            &format!("user-unsent-{}", store.history().len() + 1),
            "user",
            WorkflowCommand::AbandonUnsent {
                task: task.into(),
                attempt: attempt.into(),
                reason: reason.into(),
            },
            CommandContext::default(),
            false,
        )?;
        #[cfg(windows)]
        if let Some(job) = self.api_jobs.get_mut(&(project.into(), attempt.into())) {
            job.state = api_jobs::ApiJobState::Abandoned;
            job.message = Some("用户已放弃未派发尝试；没有重新发送任务".into());
        }
        #[cfg(windows)]
        if let Some(job) = self.omp_jobs.get_mut(&(project.into(), attempt.into())) {
            job.state = omp_jobs::OmpJobState::Cancelled;
            job.message = None;
        }
        #[cfg(windows)]
        if let Some(job) = self.codex_jobs.get_mut(&(project.into(), attempt.into())) {
            job.state = crate::async_codex::observed_state(job, store);
            require(job.state == CodexJobState::Abandoned, "关闭事件尚未恢复")?;
            job.updated_at = chrono::Utc::now().to_rfc3339();
            job.message =
                Some("用户已放弃未派发尝试；原始分配和处置原因保留，未重新发送任务".into());
            crate::async_codex::save(&self.root, job)?;
        }
        Ok(())
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::management_execution::{ApplicationTarget, DecisionIntent};
    use cerulean_domain::management::{DecisionEnvelope, ManagementDecision, Purpose};

    fn fixture(root: &Path) -> RuntimeHost {
        fixture_with_job(root, true)
    }
    fn fixture_with_job(root: &Path, include_job: bool) -> RuntimeHost {
        let (mut host, mut draft) = super::super::project_planning::tests::fixture(root);
        draft.management_max_requests = 8;
        host.configure_project(draft).unwrap();
        super::super::project_planning::tests::complete(
            &mut host,
            super::super::project_planning::tests::proposal("tests"),
        );
        let revision = host.projects["project"].revision().unwrap().clone();
        let revision_digest = revision.approval_digest().unwrap();
        host.approve("project", &revision_digest).unwrap();
        let context = host
            .management_context("project", "Implement a sum function")
            .unwrap();
        let state_digest = digest(&context).unwrap();
        host.management
            .begin(DecisionIntent {
                operation_id: "assign-fixture".into(),
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
                "assign-fixture",
                Ok(cerulean_adapters::openai::DecisionObservation {
                    response_id: "resp_unsent_fixture".into(),
                    requested_model: "fixture-deputy".into(),
                    actual_model: "fixture-deputy".into(),
                    input_tokens: 0,
                    output_tokens: 0,
                    envelope: DecisionEnvelope {
                        state_digest,
                        decision: ManagementDecision::Assign {
                            task_id: "task".into(),
                            worker_id: "worker-codex".into(),
                            rationale: "isolated unsent fixture".into(),
                        },
                    },
                }),
            )
            .unwrap();
        let attempt = Attempt {
            workspace_proof: None,
            id: "unsent-native".into(),
            task_id: "task".into(),
            worker_id: "worker-codex".into(),
            revision_digest: revision_digest.clone(),
            fence: 1,
            binding: SessionBinding {
                adapter_id: "codex".into(),
                adapter_version: "fixture".into(),
                session_id: "fixture-unsent-session".into(),
                identity_strength: IdentityStrength::ProviderId,
                workspace: revision.project.root.clone(),
            },
        };
        host.apply_management_decision_inner(
            "assign-fixture",
            ApplicationTarget::Workflow {
                principal: "deputy".into(),
                command: WorkflowCommand::Assign {
                    task: "task".into(),
                    worker: "worker-codex".into(),
                    attempt: attempt.id.clone(),
                },
                context: Box::new(CommandContext {
                    execution: Some(ExecutionEvidence::Attempt { attempt }),
                    ..Default::default()
                }),
            },
        )
        .unwrap();
        let job = CodexJob {
            management_operation: Some("assign-fixture".into()),
            project_id: "project".into(),
            task_id: "task".into(),
            attempt_id: "unsent-native".into(),
            revision_digest,
            worker: revision.task("task").unwrap().worker.clone().unwrap(),
            state: CodexJobState::Unknown,
            started_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            message: Some("Synthetic assigned-before-dispatch crash".into()),
        };
        if include_job {
            crate::async_codex::save(&host.root, &job).unwrap();
            host.codex_jobs
                .insert(("project".into(), "unsent-native".into()), job);
        }
        host
    }
    fn close(host: &mut RuntimeHost, hash: &str) -> Response {
        host.handle(Request {
            id: "close-unsent".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::AbandonUnsentAttempt {
                project_id: "project".into(),
                task_id: "task".into(),
                attempt_id: "unsent-native".into(),
                evidence_digest: hash.into(),
                reason: "用户关闭尚未派发的分配，稍后手动恢复".into(),
            },
        })
    }
    #[test]
    fn unsent_disposition_replays_and_repairs_stale_job_without_resending() {
        let dir = tempfile::tempdir().unwrap();
        let host = fixture(dir.path());
        let key = ("project".into(), "unsent-native".into());
        let stale_job = host.codex_jobs[&key].clone();
        let mut forged = stale_job.clone();
        forged.state = CodexJobState::Abandoned;
        crate::async_codex::save(&host.root, &forged).unwrap();
        drop(host);
        let mut host = RuntimeHost::open(&dir.path().join("data")).unwrap();
        assert_eq!(
            host.codex_jobs[&key].state,
            CodexJobState::Unknown,
            "a mutable status flag cannot replace the immutable disposition event"
        );
        let records = fs::read_dir(host.root.join("management-decisions"))
            .unwrap()
            .count();
        let original: Vec<_> = host.projects["project"]
            .history()
            .iter()
            .map(|e| e.hash.clone())
            .collect();
        let hash = host.snapshot().unsent_assignments[0]
            .evidence_digest
            .clone();
        assert!(close(&mut host, &"a".repeat(64)).error.is_some());
        assert_eq!(host.projects["project"].history().len(), original.len());
        let response = close(&mut host, &hash);
        assert!(response.error.is_none(), "{:?}", response.error);
        assert!(close(&mut host, &hash).error.is_some());
        assert_eq!(host.projects["project"].history().len(), original.len() + 1);
        let run = &host.projects["project"].state().tasks["task"];
        assert_eq!(run.state, TaskState::Pending);
        assert_eq!(run.fence, 1);
        assert!(run.attempt.is_none());
        assert_eq!(host.codex_jobs[&key].state, CodexJobState::Abandoned);
        // Simulate crash after immutable event but before mutable job projection.
        crate::async_codex::save(&host.root, &stale_job).unwrap();
        drop(host);
        let mut host = RuntimeHost::open(&dir.path().join("data")).unwrap();
        assert_eq!(host.codex_jobs[&key].state, CodexJobState::Abandoned);
        assert!(host.snapshot().unsent_assignments.is_empty());
        host.poll_verifications();
        assert_eq!(host.projects["project"].history().len(), original.len() + 1);
        assert_eq!(
            host.projects["project"].history()[..original.len()]
                .iter()
                .map(|e| e.hash.clone())
                .collect::<Vec<_>>(),
            original
        );
        assert_eq!(
            fs::read_dir(host.root.join("management-decisions"))
                .unwrap()
                .count(),
            records
        );
        assert!(host.require_available("project").is_ok());
        assert!(host.codex_active.is_empty());
        assert!(host.management_active.is_empty());
        assert!(
            host.start_managed_codex("assign-fixture", "new-attempt")
                .is_err(),
            "resolved deputy operation must never be reused"
        );
        let store = &host.projects["project"];
        let revision = store.revision().unwrap().approval_digest().unwrap();
        assert!(crate::async_codex::preflight(store, "task", "unsent-native", &revision).is_err());
        assert!(
            crate::async_codex::preflight(store, "task", "new-attempt", &revision).is_ok(),
            "new approved attempt remains eligible after explicit disposition"
        );
    }
    #[test]
    fn unsent_disposition_rejects_active_run_worker_and_dispatch_history() {
        let dir = tempfile::tempdir().unwrap();
        let mut host = fixture(dir.path());
        let hash = host.snapshot().unsent_assignments[0]
            .evidence_digest
            .clone();
        let setup = host.project_setups["project"].clone();
        let revision = host.projects["project"]
            .revision()
            .unwrap()
            .approval_digest()
            .unwrap();
        host.start_project_run("project", &revision, &setup.digest)
            .unwrap();
        assert!(close(&mut host, &hash).error.is_some());
        assert!(
            host.management_active.is_empty(),
            "rejected disposition must not poll/schedule"
        );
        let run_id = host.runs.all()[0].id.clone();
        host.pause_project_run(&run_id).unwrap();
        assert!(
            close(&mut host, &hash).error.is_some(),
            "run authorization changed review"
        );
        let current = host.snapshot().unsent_assignments[0]
            .evidence_digest
            .clone();
        let (_sender, receiver) = std::sync::mpsc::channel();
        let job = host.codex_jobs.values().next().unwrap().clone();
        host.codex_active.insert(
            (job.project_id.clone(), job.attempt_id.clone()),
            crate::async_codex::ActiveCodex {
                preparation_cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                advice_wake: tokio::sync::watch::channel(()).0,
                advice_gate: std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0)),
                observation: None,
                observation_saved: None,
                observation_error: None,
                snapshot: host.snapshot().workflows[0].clone(),
                job,
                receiver,
                disconnected: true,
                cancel: None,
            },
        );
        assert!(
            close(&mut host, &current).error.is_some(),
            "even disconnected active ownership blocks close"
        );
        host.codex_active.clear();
        let store = host.projects.get_mut("project").unwrap();
        let binding = store
            .attempt_record("task", "unsent-native")
            .unwrap()
            .binding
            .clone();
        store
            .command(
                "sent",
                "core",
                WorkflowCommand::DispatchIntent {
                    task: "task".into(),
                    attempt: "unsent-native".into(),
                },
                CommandContext {
                    thread_id: Some(binding.session_id),
                    cwd: Some(binding.workspace),
                    ..Default::default()
                },
                false,
            )
            .unwrap();
        assert!(close(&mut host, &current).error.is_some());
        assert!(host.snapshot().unsent_assignments.is_empty());
    }
    #[test]
    fn unsent_assignment_recovers_missing_management_commit_receipt_before_disposition() {
        let dir = tempfile::tempdir().unwrap();
        let host = fixture(dir.path());
        let before: Vec<_> = host.projects["project"]
            .history()
            .iter()
            .map(|e| e.hash.clone())
            .collect();
        let receipt = host.root.join("management-decisions/assign-fixture.3.json");
        drop(host);
        // A fresh synthetic ledger truncated after Assign publication but before its ACK.
        fs::remove_file(receipt).unwrap();
        let mut host = RuntimeHost::open(&dir.path().join("data")).unwrap();
        assert!(host.management.application("assign-fixture").unwrap().1);
        assert_eq!(
            host.projects["project"]
                .history()
                .iter()
                .map(|e| e.hash.clone())
                .collect::<Vec<_>>(),
            before
        );
        let review = host.snapshot().unsent_assignments[0]
            .evidence_digest
            .clone();
        assert!(close(&mut host, &review).error.is_none());
        assert_eq!(host.projects["project"].history().len(), before.len() + 1);
        assert!(!host.projects["project"]
            .history()
            .iter()
            .any(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. })));
        assert_eq!(
            host.snapshot().codex_jobs[0].state,
            CodexJobState::Abandoned
        );
    }
    #[test]
    #[ignore = "isolated native UI fixture, no real provider calls"]
    fn seed_native_unsent_assignment_fixture() {
        let root = PathBuf::from(
            std::env::var_os("CERULEAN_UNSENT_NATIVE_DIR").expect("fixture directory"),
        );
        assert!(root.is_absolute());
        assert!(
            !root.join("data").exists(),
            "fixture requires fresh directory"
        );
        let root = root.canonicalize().unwrap();
        let reports = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../reports")
            .canonicalize()
            .unwrap();
        assert!(root.starts_with(reports), "fixture must be under reports");
        let host = fixture_with_job(
            &root,
            std::env::var("CERULEAN_UNSENT_NO_JOB").as_deref() != Ok("1"),
        );
        assert_eq!(host.snapshot().unsent_assignments.len(), 1);
    }
    #[test]
    fn workspace_unsent_assignment_bounded_page_detail_and_no_dispatch_proof_are_read_only() {
        use cerulean_domain::workspace::{WorkspaceQuery, WorkspaceResult};
        let dir = tempfile::tempdir().unwrap();
        let host = fixture(dir.path());
        let before = crate::reader_generation::tests::snapshot(&host.root);
        let query = WorkspaceQuery::UnsentAssignments {
            project_id: "project".into(),
            offset: 0,
            revision: None,
        };
        let WorkspaceResult::UnsentAssignments(page) = host.workspace_query(&query).unwrap() else {
            panic!("unsent page")
        };
        assert_eq!(page.project_id, "project");
        assert_eq!(page.offset, 0);
        assert_eq!(page.total, 1);
        assert_eq!(page.next_offset, None);
        assert_eq!(page.unavailable_reason, None);
        assert_eq!(page.entries.len(), 1);
        let record = &page.entries[0];
        assert_eq!(record.project_id, "project");
        assert_eq!(record.task_id, "task");
        assert_eq!(record.attempt_id, "unsent-native");
        assert_eq!(
            record.evidence_digest,
            host.unsent_review("project", "task", "unsent-native")
                .unwrap()
        );
        assert!(
            serde_json::to_vec(&WorkspaceResult::UnsentAssignments(page.clone()))
                .unwrap()
                .len()
                <= 64 * 1024
        );
        let detail_query = WorkspaceQuery::UnsentAssignment {
            project_id: "project".into(),
            task_id: "task".into(),
            attempt_id: "unsent-native".into(),
            revision: page.revision.clone(),
        };
        let WorkspaceResult::UnsentAssignment(detail) =
            host.workspace_query(&detail_query).unwrap()
        else {
            panic!("unsent detail")
        };
        assert_eq!(detail.revision, page.revision);
        assert_eq!(detail.record.evidence_digest, record.evidence_digest);
        assert!(
            serde_json::to_vec(&WorkspaceResult::UnsentAssignment(detail))
                .unwrap()
                .len()
                <= crate::workspace_read::RESULT_BYTES
        );
        assert!(!host.projects["project"]
            .history()
            .iter()
            .any(|entry| matches!(
                &entry.body.event.command,
                WorkflowCommand::DispatchIntent { attempt, .. } if attempt == "unsent-native"
            )));
        assert_eq!(
            host.projects["project"].state().tasks["task"].state,
            TaskState::Assigned
        );
        assert_eq!(
            before,
            crate::reader_generation::tests::snapshot(&host.root)
        );
    }

    #[test]
    fn workspace_unsent_assignment_bounded_refuses_unknown_stale_and_poisoned_sources() {
        use cerulean_domain::workspace::WorkspaceQuery;
        let dir = tempfile::tempdir().unwrap();
        let mut host = fixture(dir.path());
        let page = match host
            .workspace_query(&WorkspaceQuery::UnsentAssignments {
                project_id: "project".into(),
                offset: 0,
                revision: None,
            })
            .unwrap()
        {
            cerulean_domain::workspace::WorkspaceResult::UnsentAssignments(page) => page,
            _ => panic!("unsent page"),
        };
        for query in [
            WorkspaceQuery::UnsentAssignments {
                project_id: "missing".into(),
                offset: 0,
                revision: None,
            },
            WorkspaceQuery::UnsentAssignments {
                project_id: "../invalid".into(),
                offset: 0,
                revision: None,
            },
            WorkspaceQuery::UnsentAssignments {
                project_id: "project".into(),
                offset: 1,
                revision: None,
            },
            WorkspaceQuery::UnsentAssignments {
                project_id: "project".into(),
                offset: 2,
                revision: Some(page.revision.clone()),
            },
            WorkspaceQuery::UnsentAssignment {
                project_id: "project".into(),
                task_id: "task".into(),
                attempt_id: "unsent-native".into(),
                revision: "0".repeat(64),
            },
            WorkspaceQuery::UnsentAssignment {
                project_id: "project".into(),
                task_id: "missing".into(),
                attempt_id: "unsent-native".into(),
                revision: page.revision.clone(),
            },
        ] {
            assert!(host.workspace_query(&query).is_err(), "{query:?}");
        }
        let before = crate::reader_generation::tests::snapshot(&host.root);
        host.base.poisoned = true;
        assert!(host
            .workspace_query(&WorkspaceQuery::UnsentAssignments {
                project_id: "project".into(),
                offset: 0,
                revision: None,
            })
            .is_err());
        host.base.poisoned = false;
        host.runs.poison_for_test();
        assert!(host
            .workspace_query(&WorkspaceQuery::UnsentAssignments {
                project_id: "project".into(),
                offset: 0,
                revision: None,
            })
            .is_err());
        assert_eq!(
            before,
            crate::reader_generation::tests::snapshot(&host.root)
        );
    }

    #[test]
    fn workspace_unsent_assignment_bounded_uses_shared_owner_and_rejects_interruption() {
        use cerulean_domain::workspace::{WorkspaceQuery, WorkspaceResult};
        let dir = tempfile::tempdir().unwrap();
        let mut host = fixture(dir.path());
        let original_digest = host
            .unsent_review("project", "task", "unsent-native")
            .unwrap();
        let query = WorkspaceQuery::UnsentAssignments {
            project_id: "project".into(),
            offset: 0,
            revision: None,
        };
        let store = host.projects.remove("project").unwrap();
        let writer =
            crate::async_api::ApiWorkflow::start(store, host.root.join("workflows/project"))
                .unwrap();
        host.api_writers.insert("project".into(), writer);
        let before = crate::reader_generation::tests::snapshot(&host.root);
        let WorkspaceResult::UnsentAssignments(page) = host.workspace_query(&query).unwrap() else {
            panic!("shared unsent page")
        };
        assert!(page.entries.is_empty());
        assert_eq!(page.total, 0);
        assert!(page
            .unavailable_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("持有项目")));
        assert_eq!(
            before,
            crate::reader_generation::tests::snapshot(&host.root)
        );
        host.api_writers.get_mut("project").unwrap().interrupted = true;
        assert!(host.workspace_query(&query).is_err());
        let mut writer = host.api_writers.remove("project").unwrap();
        let store = writer.owner.finish(false).unwrap();
        host.projects.insert("project".into(), store);
        let WorkspaceResult::UnsentAssignments(reopened) = host.workspace_query(&query).unwrap()
        else {
            panic!("reopened unsent page")
        };
        assert_eq!(reopened.total, 1);
        assert_eq!(reopened.entries[0].evidence_digest, original_digest);
    }

    fn unsent_page_query(
        offset: u32,
        revision: Option<String>,
    ) -> cerulean_domain::workspace::WorkspaceQuery {
        cerulean_domain::workspace::WorkspaceQuery::UnsentAssignments {
            project_id: "project".into(),
            offset,
            revision,
        }
    }

    fn unsent_first(
        host: &RuntimeHost,
    ) -> cerulean_domain::workspace::WorkspaceUnsentAssignmentPage {
        let cerulean_domain::workspace::WorkspaceResult::UnsentAssignments(page) =
            host.workspace_query(&unsent_page_query(0, None)).unwrap()
        else {
            panic!("unsent page")
        };
        page
    }

    fn unsent_detail_query(revision: &str) -> cerulean_domain::workspace::WorkspaceQuery {
        cerulean_domain::workspace::WorkspaceQuery::UnsentAssignment {
            project_id: "project".into(),
            task_id: "task".into(),
            attempt_id: "unsent-native".into(),
            revision: revision.into(),
        }
    }

    #[test]
    fn workspace_unsent_assignment_streamed_basis_matches_original_tuple_sha_and_owner_handoff() {
        use super::super::workspace_host::unsent_assignments::original_basis;
        use sha2::{Digest, Sha256};
        let dir = tempfile::tempdir().unwrap();
        let mut host = fixture(dir.path());
        let store = &host.projects["project"];
        let encoded = serde_json::to_vec(&(
            "workspace-unsent-assignments-original-basis-v1",
            "project",
            store.configuration(),
            store.state(),
            store.revision(),
            store.approval(),
            store.history(),
        ))
        .unwrap();
        let expected = format!("{:x}", Sha256::digest(&encoded));
        let basis = original_basis(store, "project").unwrap();
        assert_eq!(basis.version, expected);
        assert_eq!(
            basis.assigned,
            vec![("task".into(), "unsent-native".into(), true)]
        );
        let page = unsent_first(&host);
        let original = serde_json::to_value(
            host.workspace_query(&unsent_detail_query(&page.revision))
                .unwrap(),
        )
        .unwrap();
        let before = crate::reader_generation::tests::snapshot(&host.root);
        let store = host.projects.remove("project").unwrap();
        let writer =
            crate::async_api::ApiWorkflow::start(store, host.root.join("workflows/project"))
                .unwrap();
        host.api_writers.insert("project".into(), writer);
        let shared = host.api_writers["project"]
            .owner
            .workspace_unsent_assignments_basis("project")
            .unwrap();
        assert_eq!(shared.version, expected);
        assert_eq!(shared.assigned, basis.assigned);
        let busy = unsent_first(&host);
        assert_eq!(busy.total, 0);
        assert!(busy.entries.is_empty());
        assert_eq!(
            busy.unavailable_reason.as_deref(),
            Some("执行或验收线程仍持有项目，不能关闭尝试")
        );
        assert!(host
            .workspace_query(&unsent_detail_query(&busy.revision))
            .is_err());
        host.api_writers
            .get_mut("project")
            .unwrap()
            .snapshot
            .sequence += 999;
        assert_eq!(unsent_first(&host).revision, busy.revision);
        assert_eq!(
            host.api_writers["project"]
                .owner
                .workspace_unsent_assignments_basis("project")
                .unwrap()
                .version,
            expected
        );
        let mut writer = host.api_writers.remove("project").unwrap();
        host.projects
            .insert("project".into(), writer.owner.finish(false).unwrap());
        assert_eq!(
            original_basis(&host.projects["project"], "project")
                .unwrap()
                .version,
            expected
        );
        assert_eq!(unsent_first(&host).revision, page.revision);
        assert_eq!(
            serde_json::to_value(
                host.workspace_query(&unsent_detail_query(&page.revision))
                    .unwrap()
            )
            .unwrap(),
            original
        );
        assert_eq!(
            before,
            crate::reader_generation::tests::snapshot(&host.root)
        );
    }

    #[test]
    fn workspace_unsent_assignment_same_sequence_job_change_stales_page_detail_and_digest() {
        let dir = tempfile::tempdir().unwrap();
        let mut host = fixture(dir.path());
        let page = unsent_first(&host);
        let history = host.projects["project"].history().to_vec();
        let runs = host.runs.sequence();
        host.codex_jobs
            .get_mut(&("project".into(), "unsent-native".into()))
            .unwrap()
            .message = Some("changed original job observation".into());
        let changed = unsent_first(&host);
        assert_eq!(host.projects["project"].history(), history);
        assert_eq!(host.runs.sequence(), runs);
        assert_ne!(changed.revision, page.revision);
        assert_ne!(
            changed.entries[0].evidence_digest,
            page.entries[0].evidence_digest
        );
        let before = crate::reader_generation::tests::snapshot(&host.root);
        assert!(host
            .workspace_query(&unsent_page_query(0, Some(page.revision.clone())))
            .is_err());
        assert!(host
            .workspace_query(&unsent_detail_query(&page.revision))
            .is_err());
        assert!(close(&mut host, &page.entries[0].evidence_digest)
            .error
            .is_some());
        assert_eq!(
            changed.entries[0].evidence_digest,
            host.unsent_review("project", "task", "unsent-native")
                .unwrap()
        );
        assert_eq!(
            before,
            crate::reader_generation::tests::snapshot(&host.root)
        );
    }

    #[test]
    fn workspace_unsent_assignment_returned_digest_abandons_once_preserving_history_and_no_dispatch(
    ) {
        let dir = tempfile::tempdir().unwrap();
        let mut host = fixture(dir.path());
        let page = unsent_first(&host);
        let history = host.projects["project"].history().to_vec();
        let digest = &page.entries[0].evidence_digest;
        assert!(close(&mut host, digest).error.is_none());
        let store = &host.projects["project"];
        assert_eq!(&store.history()[..history.len()], history);
        assert_eq!(store.history().len(), history.len() + 1);
        assert!(
            matches!(&store.history().last().unwrap().body.event.command,
            WorkflowCommand::AbandonUnsent { task, attempt, .. } if task == "task" && attempt == "unsent-native")
        );
        assert!(!store.history().iter().any(|entry| matches!(
            &entry.body.event.command,
            WorkflowCommand::DispatchIntent { .. }
        )));
        assert_eq!(store.state().tasks["task"].state, TaskState::Pending);
        assert_eq!(
            host.codex_jobs[&("project".into(), "unsent-native".into())].state,
            CodexJobState::Abandoned
        );
        let before = crate::reader_generation::tests::snapshot(&host.root);
        assert!(close(&mut host, digest).error.is_some());
        assert!(host
            .workspace_query(&unsent_detail_query(&page.revision))
            .is_err());
        let empty = unsent_first(&host);
        assert_eq!(empty.total, 0);
        assert!(empty.entries.is_empty());
        assert_eq!(empty.unavailable_reason, None);
        assert_ne!(empty.revision, page.revision);
        assert_eq!(
            before,
            crate::reader_generation::tests::snapshot(&host.root)
        );
    }

    #[test]
    fn workspace_unsent_assignment_dispatch_intent_removes_eligibility_and_invalidates_version() {
        let dir = tempfile::tempdir().unwrap();
        let mut host = fixture(dir.path());
        let page = unsent_first(&host);
        let store = host.projects.get_mut("project").unwrap();
        let binding = store
            .attempt_record("task", "unsent-native")
            .unwrap()
            .binding
            .clone();
        store
            .command(
                "workspace-unsent-dispatch",
                "core",
                WorkflowCommand::DispatchIntent {
                    task: "task".into(),
                    attempt: "unsent-native".into(),
                },
                CommandContext {
                    thread_id: Some(binding.session_id),
                    cwd: Some(binding.workspace),
                    ..Default::default()
                },
                false,
            )
            .unwrap();
        let before = crate::reader_generation::tests::snapshot(&host.root);
        let unavailable = unsent_first(&host);
        assert_eq!(unavailable.total, 0);
        assert!(unavailable.entries.is_empty());
        assert_eq!(
            unavailable.unavailable_reason.as_deref(),
            Some("原尝试已有派发意图，必须核对原执行结果")
        );
        assert_ne!(unavailable.revision, page.revision);
        assert!(host
            .workspace_query(&unsent_page_query(0, Some(page.revision.clone())))
            .is_err());
        assert!(host
            .workspace_query(&unsent_detail_query(&page.revision))
            .is_err());
        assert!(host
            .workspace_query(&unsent_detail_query(&unavailable.revision))
            .is_err());
        assert!(close(&mut host, &page.entries[0].evidence_digest)
            .error
            .is_some());
        assert_eq!(
            before,
            crate::reader_generation::tests::snapshot(&host.root)
        );
    }

    #[test]
    fn workspace_unsent_assignment_valid_end_page_is_known_empty_and_read_only() {
        use cerulean_domain::workspace::WorkspaceResult;
        let dir = tempfile::tempdir().unwrap();
        let host = fixture(dir.path());
        let page = unsent_first(&host);
        let before = crate::reader_generation::tests::snapshot(&host.root);
        let WorkspaceResult::UnsentAssignments(end) = host
            .workspace_query(&unsent_page_query(page.total, Some(page.revision.clone())))
            .unwrap()
        else {
            panic!("end page")
        };
        assert_eq!(end.revision, page.revision);
        assert_eq!(end.offset, 1);
        assert_eq!(end.total, 1);
        assert!(end.entries.is_empty());
        assert_eq!(end.next_offset, None);
        assert_eq!(end.unavailable_reason, None);
        assert_eq!(
            before,
            crate::reader_generation::tests::snapshot(&host.root)
        );
    }

    #[test]
    fn workspace_unsent_assignment_service_reopen_preserves_original_detail_and_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let host = fixture(dir.path());
        let page = unsent_first(&host);
        let query = unsent_detail_query(&page.revision);
        let original = serde_json::to_value(host.workspace_query(&query).unwrap()).unwrap();
        let root = host.root.clone();
        let before = crate::reader_generation::tests::snapshot(&root);
        drop(host);
        let service = crate::diagnostic_service::ServiceHost::open(&root).unwrap();
        assert!(!service.is_diagnostic());
        assert_eq!(
            serde_json::to_value(service.workspace_query(&query).unwrap()).unwrap(),
            original
        );
        assert_eq!(before, crate::reader_generation::tests::snapshot(&root));
    }

    #[test]
    fn workspace_unsent_assignment_diagnostic_service_refuses_without_writes() {
        let dir = tempfile::tempdir().unwrap();
        let host = fixture(dir.path());
        let page = unsent_first(&host);
        let root = host.root.clone();
        drop(host);
        let damaged = root.join("desktop-task-dispatch-v1").join("a".repeat(64));
        fs::create_dir_all(&damaged).unwrap();
        fs::write(damaged.join("intent.pending"), br#"{"unfinished":"#).unwrap();
        let before = crate::reader_generation::tests::snapshot(&root);
        let service = crate::diagnostic_service::ServiceHost::open(&root).unwrap();
        assert!(service.is_diagnostic());
        for query in [
            unsent_page_query(0, None),
            unsent_detail_query(&page.revision),
        ] {
            assert_eq!(
                service.workspace_query(&query).unwrap_err(),
                "项目记录处于诊断状态，清单暂不可用；不代表没有项目"
            );
        }
        assert_eq!(before, crate::reader_generation::tests::snapshot(&root));
    }

    #[test]
    fn workspace_unsent_assignment_poisoned_durable_and_shared_writer_refuse_original_evidence() {
        let dir = tempfile::tempdir().unwrap();
        let mut host = fixture(dir.path());
        let page = unsent_first(&host);
        let state = host.projects["project"].state().clone();
        let history = host.projects["project"].history().to_vec();
        let before = crate::reader_generation::tests::snapshot(&host.root);
        let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            host.projects.get_mut("project").unwrap().command_observed(
                "unsent-reader-poisoned-writer",
                "user",
                WorkflowCommand::AbandonUnsent {
                    task: "task".into(),
                    attempt: "unsent-native".into(),
                    reason: "controlled original write interruption".into(),
                },
                CommandContext::default(),
                false,
                |point| {
                    if point == crate::scheduler::CommitPoint::BeforeEventWrite {
                        panic!("controlled original write interruption");
                    }
                },
            )
        }));
        assert!(interrupted.is_err());
        assert!(host.projects["project"].requires_reopen());
        assert_eq!(host.projects["project"].state(), &state);
        assert_eq!(host.projects["project"].history(), history);
        assert!(host.workspace_query(&unsent_page_query(0, None)).is_err());
        assert!(host
            .workspace_query(&unsent_detail_query(&page.revision))
            .is_err());
        let store = host.projects.remove("project").unwrap();
        let writer =
            crate::async_api::ApiWorkflow::start(store, host.root.join("workflows/project"))
                .unwrap();
        host.api_writers.insert("project".into(), writer);
        assert!(host.workspace_query(&unsent_page_query(0, None)).is_err());
        assert!(host
            .workspace_query(&unsent_detail_query(&page.revision))
            .is_err());
        assert_eq!(
            before,
            crate::reader_generation::tests::snapshot(&host.root)
        );
    }

    #[test]
    fn workspace_unsent_assignment_synthetic_dto_page20_and_64kib_boundaries() {
        // Synthetic DTO stress only: these strings are not legitimate task identifiers.
        let record = UnsentAssignmentReview {
            project_id: "project".into(),
            task_id: "synthetic".into(),
            attempt_id: "synthetic".into(),
            evidence_digest: "a".repeat(64),
        };
        let records: Vec<_> = (0..21)
            .map(|index| UnsentAssignmentReview {
                task_id: format!("synthetic-{index:02}"),
                ..record.clone()
            })
            .collect();
        let version = "b".repeat(64);
        let (first, next) =
            crate::workspace_read::page(records.clone(), 21, 0, None, &version).unwrap();
        assert_eq!(first.len(), 20);
        assert_eq!(next, Some(20));
        assert_eq!(
            serde_json::to_value(&first).unwrap(),
            serde_json::to_value(&records[..20]).unwrap()
        );
        let (last, next) =
            crate::workspace_read::page(records.clone(), 21, 20, Some(&version), &version).unwrap();
        assert_eq!(
            serde_json::to_value(&last).unwrap(),
            serde_json::to_value(&records[20..]).unwrap()
        );
        assert_eq!(next, None);
        let mut exact = record.clone();
        exact.task_id.clear();
        let overhead = serde_json::to_vec(&exact).unwrap().len();
        exact.task_id = "x".repeat(64 * 1024 - 1024 - 1 - overhead);
        assert_eq!(
            serde_json::to_vec(&exact).unwrap().len() + 1024 + 1,
            64 * 1024
        );
        let (full, next) =
            crate::workspace_read::page(vec![exact.clone(), record], 2, 0, None, &version).unwrap();
        assert_eq!(full.len(), 1);
        assert_eq!(next, Some(1));
        assert_eq!(
            serde_json::to_value(&full[0]).unwrap(),
            serde_json::to_value(&exact).unwrap()
        );
        exact.task_id.push('x');
        assert!(crate::workspace_read::page(vec![exact], 1, 0, None, &version).is_err());
    }

    #[test]
    fn workspace_unsent_assignment_synthetic_dto_result512kib_and_persistence_caps() {
        use cerulean_domain::workspace::{WorkspaceResult, WorkspaceUnsentAssignmentDetail};
        // Synthetic DTO stress only; no oversize identity enters the real query or ledger.
        let mut detail = WorkspaceUnsentAssignmentDetail {
            project_id: "project".into(),
            revision: "a".repeat(64),
            record: UnsentAssignmentReview {
                project_id: "project".into(),
                task_id: String::new(),
                attempt_id: "synthetic".into(),
                evidence_digest: "b".repeat(64),
            },
        };
        let overhead =
            serde_json::to_vec(&WorkspaceResult::UnsentAssignment(Box::new(detail.clone())))
                .unwrap()
                .len();
        detail.record.task_id = "x".repeat(512 * 1024 - overhead);
        let result = WorkspaceResult::UnsentAssignment(Box::new(detail.clone()));
        assert_eq!(serde_json::to_vec(&result).unwrap().len(), 512 * 1024);
        assert!(crate::workspace_read::bounded(result).is_ok());
        detail.record.task_id.push('x');
        assert!(
            crate::workspace_read::bounded(WorkspaceResult::UnsentAssignment(Box::new(detail)))
                .is_err()
        );
        let dir = tempfile::tempdir().unwrap();
        let host = fixture(dir.path());
        let original = crate::reader_generation::tests::snapshot(&host.root);
        let mut job = host.codex_jobs.values().next().unwrap().clone();
        job.message = Some(String::new());
        let overhead = serde_json::to_vec(&job).unwrap().len();
        // Exercise the existing persistence encoder cap, independently of semantic job validation.
        job.message = Some("x".repeat(64 * 1024 + 1 - overhead));
        assert_eq!(serde_json::to_vec(&job).unwrap().len(), 64 * 1024 + 1);
        assert_eq!(
            crate::async_codex::save(&host.root, &job).unwrap_err(),
            "Codex job too large"
        );
        assert_eq!(
            original,
            crate::reader_generation::tests::snapshot(&host.root)
        );
        let path = host.root.join("codex-jobs/project/unsent-native.json");
        let bytes = fs::read(&path).unwrap();
        assert_eq!(
            bytes,
            serde_json::to_vec(host.codex_jobs.values().next().unwrap()).unwrap()
        );
        job.message = Some("x".repeat(64 * 1024 - overhead));
        let exact = serde_json::to_vec(&job).unwrap();
        assert_eq!(exact.len(), 64 * 1024);
        crate::async_codex::save(&host.root, &job).unwrap();
        assert_eq!(fs::read(&path).unwrap(), exact);
    }
}
