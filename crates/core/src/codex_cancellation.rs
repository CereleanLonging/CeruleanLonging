//! User cancellation is queued to the sole owner of the workflow and transport.
//! The worker persists CancelIntent before any remote interruption.
use super::*;

impl RuntimeHost {
    #[cfg(windows)]
    fn cancellation_review(
        &self,
        project: &str,
        attempt: &str,
    ) -> Result<CodexCancellationReview, String> {
        require(self.runs.healthy(), "运行控制需要恢复，停止请求尚未提交")?;
        let active = self
            .codex_active
            .get(&(project.into(), attempt.into()))
            .ok_or("当前没有可控制的编程执行")?;
        require(
            !active.disconnected
                && active.cancel.is_some()
                && active.job.attempt_id == attempt
                && matches!(
                    active.job.state,
                    CodexJobState::Starting | CodexJobState::Executing
                ),
            "执行状态已变化，或停止请求已提交，请刷新后核对",
        )?;
        // Started can be durable before the worker's Progress reaches the host.
        // Cancellation must read the same authority as the project projection.
        let authoritative = self
            .api_writers
            .get(project)
            .map(|w| w.owner.snapshot())
            .transpose()?;
        let snapshot = authoritative.as_ref().unwrap_or(&active.snapshot);
        let task = snapshot
            .state
            .tasks
            .get(&active.job.task_id)
            .ok_or("执行任务缺失")?;
        require(
            (active.job.state == CodexJobState::Starting
                && task.state == TaskState::Pending
                && task.attempt.is_none())
                || (task.state == TaskState::Running
                    && task.attempt.as_deref() == Some(attempt)
                    && task.cancellation_requested != Some(true)),
            "当前不在启动准备或可停止的执行阶段",
        )?;
        Ok(CodexCancellationReview {
            project_id: project.into(),
            task_id: active.job.task_id.clone(),
            attempt_id: attempt.into(),
            evidence_digest: digest(&("codex-cancel-v1", snapshot, &active.job.worker, attempt))?,
        })
    }

    pub(super) fn codex_cancellations(&self) -> Vec<CodexCancellationReview> {
        #[cfg(windows)]
        {
            self.codex_active
                .iter()
                .filter_map(|((project, _), active)| {
                    self.cancellation_review(project, &active.job.attempt_id)
                        .ok()
                })
                .collect()
        }
        #[cfg(not(windows))]
        {
            vec![]
        }
    }

    pub(super) fn cancel_codex(
        &mut self,
        project: &str,
        attempt: &str,
        expected: &str,
        reason: &str,
    ) -> Result<(), String> {
        #[cfg(windows)]
        {
            for id in [project, attempt] {
                identifier(id)?;
            }
            require(
                !reason.trim().is_empty() && reason.len() <= 8000 && !reason.contains('\0'),
                "请输入不超过8000字节的停止原因",
            )?;
            let review = self.cancellation_review(project, attempt)?;
            require(
                review.evidence_digest == expected,
                "执行证据已变化，请重新审阅停止请求",
            )?;
            // No scheduling tick occurs between the user's request and this pause.
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
            let mut job = self.codex_active[&(project.into(), attempt.into())]
                .job
                .clone();
            job.state = CodexJobState::CancelRequested;
            job.updated_at = chrono::Utc::now().to_rfc3339();
            job.message =
                Some("停止请求已提交到本地执行队列，等待执行器登记和确认；已有修改保留".into());
            crate::async_codex::save(&self.root, &job)?;
            self.codex_jobs
                .insert((project.into(), attempt.into()), job.clone());
            let active = self
                .codex_active
                .get_mut(&(project.into(), attempt.into()))
                .expect("validated active worker");
            active.job = job.clone();
            let control = active.cancel.take().ok_or("停止请求已提交")?;
            let delivered = control.send(reason.into());
            active
                .preparation_cancel
                .store(true, std::sync::atomic::Ordering::Release);
            if delivered.is_err() {
                job.state = CodexJobState::Unknown;
                job.message = Some("执行器未接收停止请求；后续调度已暂停，请核对原任务结果".into());
                active.job = job.clone();
                self.codex_jobs
                    .insert((project.into(), attempt.into()), job.clone());
                crate::async_codex::save(&self.root, &job)?;
                return Err("执行状态已变化，尚未确认停止；请刷新或核对原会话".into());
            }
            Ok(())
        }
        #[cfg(not(windows))]
        {
            let _ = (project, attempt, expected, reason);
            Err("Codex execution requires Windows".into())
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::async_codex::{ActiveCodex, Update};
    use std::sync::mpsc;

    fn fixture(
        root: &Path,
    ) -> (
        RuntimeHost,
        DurableWorkflow,
        tokio::sync::oneshot::Receiver<String>,
        mpsc::Sender<Update>,
    ) {
        let (mut host, draft) = super::super::project_planning::tests::fixture(root);
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
        let setup = host.project_setups["project"].digest.clone();
        host.start_project_run("project", &revision, &setup)
            .unwrap();
        let store = host.projects.remove("project").unwrap();
        let mut snapshot = crate::async_verification::snapshot(&store);
        // Host control fixture: execution persistence/transport are covered separately.
        let task = snapshot.state.tasks.get_mut("task").unwrap();
        task.state = TaskState::Running;
        task.attempt = Some("cancel-host".into());
        let now = chrono::Utc::now().to_rfc3339();
        let job = CodexJob {
            project_id: "project".into(),
            task_id: "task".into(),
            attempt_id: "cancel-host".into(),
            management_operation: None,
            revision_digest: revision,
            worker: store
                .revision()
                .unwrap()
                .task("task")
                .unwrap()
                .worker
                .clone()
                .unwrap(),
            state: CodexJobState::Executing,
            started_at: now.clone(),
            updated_at: now,
            message: None,
        };
        crate::async_codex::save(&host.root, &job).unwrap();
        host.codex_jobs
            .insert(("project".into(), "cancel-host".into()), job.clone());
        let (sender, receiver) = mpsc::channel();
        let (control, cancellation) = tokio::sync::oneshot::channel();
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
                job,
                receiver,
                disconnected: false,
                cancel: Some(control),
            },
        );
        (host, store, cancellation, sender)
    }
    fn request(expected: &str, reason: &str) -> Request {
        Request {
            id: "cancel-request".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::CancelCodex {
                project_id: "project".into(),
                attempt_id: "cancel-host".into(),
                evidence_digest: expected.into(),
                reason: reason.into(),
            },
        }
    }
    #[test]
    fn host_cancel_validates_before_pause_and_pauses_before_control_delivery() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, store, mut receiver, _updates) = fixture(root.path());
        let hash = host.codex_cancellations()[0].evidence_digest.clone();
        for (expected, reason) in [
            ("bad", "stop"),
            (hash.as_str(), " "),
            (hash.as_str(), "bad\0reason"),
        ] {
            assert!(host.handle(request(expected, reason)).error.is_some());
            assert_eq!(host.runs.all()[0].state, project_setup::RunState::Active);
            assert!(matches!(
                receiver.try_recv(),
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
            ));
        }
        let response = host.handle(request(&hash, "user requested stop"));
        assert!(response.error.is_none(), "{:?}", response.error);
        assert_eq!(host.runs.all()[0].state, project_setup::RunState::Paused);
        assert_eq!(
            crate::run_control::RunLedger::open(&host.root)
                .unwrap()
                .all()[0]
                .state,
            project_setup::RunState::Paused
        );
        assert_eq!(receiver.try_recv().unwrap(), "user requested stop");
        assert_eq!(
            host.snapshot().codex_jobs[0].state,
            CodexJobState::CancelRequested
        );
        assert!(host.codex_cancellations().is_empty());
        assert!(host.handle(request(&hash, "duplicate")).error.is_some());
        let run = host.runs.all()[0].clone();
        assert!(host
            .start_project_run("project", &run.revision_digest, &run.setup_digest)
            .is_err());
        assert_eq!(
            store.history().len(),
            2,
            "host never impersonates the workflow-owning worker"
        );
    }
    #[test]
    fn failed_pause_or_projection_never_delivers_cancel_and_lost_receiver_stays_paused() {
        for mode in ["pause", "projection", "receiver"] {
            let root = tempfile::tempdir().unwrap();
            let (mut host, _store, mut receiver, _updates) = fixture(root.path());
            let hash = host.codex_cancellations()[0].evidence_digest.clone();
            if mode == "pause" {
                fs::write(
                    host.root.join("run-control/00000002.pending"),
                    b"owned test obstruction",
                )
                .unwrap();
            } else if mode == "projection" {
                let path = host.root.join("codex-jobs/project/cancel-host.json");
                fs::remove_file(&path).unwrap();
                fs::create_dir(path).unwrap();
            } else {
                receiver.close();
            }
            assert!(
                host.handle(request(&hash, "stop")).error.is_some(),
                "{mode}"
            );
            assert!(receiver.try_recv().is_err(), "{mode}: no delivered control");
            if mode != "pause" {
                assert_eq!(host.runs.all()[0].state, project_setup::RunState::Paused);
            }
            if mode == "receiver" {
                assert!(host.codex_cancellations().is_empty());
                assert_eq!(host.snapshot().codex_jobs[0].state, CodexJobState::Unknown);
            }
        }
    }
    #[test]
    fn workspace_codex_cancellation_shared_owner_matches_legacy_digest_and_is_read_only() {
        use cerulean_domain::workspace::*;
        let root = tempfile::tempdir().unwrap();
        let (mut host, store, mut receiver, _updates) = fixture(root.path());
        host.codex_active
            .get_mut(&("project".into(), "cancel-host".into()))
            .unwrap()
            .job
            .state = CodexJobState::Starting;
        // The cached worker projection is deliberately wrong. Only the shared
        // durable owner may supply the task state and cancellation proof.
        host.codex_active
            .get_mut(&("project".into(), "cancel-host".into()))
            .unwrap()
            .snapshot
            .state
            .tasks
            .get_mut("task")
            .unwrap()
            .state = TaskState::Unknown;
        host.api_writers.insert(
            "project".into(),
            crate::async_api::ApiWorkflow::start(store, host.root.join("workflows/project"))
                .unwrap(),
        );
        let before = crate::reader_generation::tests::snapshot(&host.root);
        let legacy = host.cancellation_review("project", "cancel-host").unwrap();
        let query = WorkspaceQuery::CodexCancellations {
            project_id: "project".into(),
            offset: 0,
            revision: None,
        };
        let WorkspaceResult::CodexCancellations(page) = host.workspace_query(&query).unwrap()
        else {
            panic!("cancellation page")
        };
        assert_eq!(page.total, 1);
        assert_eq!(page.entries[0].evidence_digest, legacy.evidence_digest);
        let WorkspaceResult::CodexCancellation(detail) = host
            .workspace_query(&WorkspaceQuery::CodexCancellation {
                project_id: "project".into(),
                attempt_id: "cancel-host".into(),
                revision: page.revision.clone(),
            })
            .unwrap()
        else {
            panic!("cancellation detail")
        };
        assert_eq!(detail.review.evidence_digest, legacy.evidence_digest);
        assert_eq!(
            before,
            crate::reader_generation::tests::snapshot(&host.root)
        );
        assert!(matches!(
            receiver.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        assert_eq!(host.runs.all()[0].state, project_setup::RunState::Active);
        let active = host
            .codex_active
            .get_mut(&("project".into(), "cancel-host".into()))
            .unwrap();
        active.disconnected = true;
        assert!(host
            .workspace_query(&WorkspaceQuery::CodexCancellation {
                project_id: "project".into(),
                attempt_id: "cancel-host".into(),
                revision: page.revision,
            })
            .is_err());
        let WorkspaceResult::CodexCancellations(empty) = host.workspace_query(&query).unwrap()
        else {
            panic!("empty page")
        };
        assert_eq!(empty.total, 0);
        assert!(empty.entries.is_empty());
    }

    #[test]
    fn workspace_codex_cancellation_refuses_stale_unknown_interrupted_poisoned_and_nonshared() {
        use cerulean_domain::workspace::*;
        let root = tempfile::tempdir().unwrap();
        let (mut host, store, _receiver, _updates) = fixture(root.path());
        let query = WorkspaceQuery::CodexCancellations {
            project_id: "project".into(),
            offset: 0,
            revision: None,
        };
        assert!(
            host.workspace_query(&query).is_err(),
            "active cached snapshot is not authority"
        );
        host.codex_active
            .get_mut(&("project".into(), "cancel-host".into()))
            .unwrap()
            .job
            .state = CodexJobState::Starting;
        host.api_writers.insert(
            "project".into(),
            crate::async_api::ApiWorkflow::start(store, host.root.join("workflows/project"))
                .unwrap(),
        );
        let WorkspaceResult::CodexCancellations(page) = host.workspace_query(&query).unwrap()
        else {
            panic!("page")
        };
        for bad in [
            WorkspaceQuery::CodexCancellations {
                project_id: "missing".into(),
                offset: 0,
                revision: None,
            },
            WorkspaceQuery::CodexCancellations {
                project_id: "../invalid".into(),
                offset: 0,
                revision: None,
            },
            WorkspaceQuery::CodexCancellations {
                project_id: "project".into(),
                offset: 1,
                revision: None,
            },
            WorkspaceQuery::CodexCancellations {
                project_id: "project".into(),
                offset: 2,
                revision: Some(page.revision.clone()),
            },
            WorkspaceQuery::CodexCancellation {
                project_id: "project".into(),
                attempt_id: "cancel-host".into(),
                revision: "f".repeat(64),
            },
            WorkspaceQuery::CodexCancellation {
                project_id: "project".into(),
                attempt_id: "missing".into(),
                revision: page.revision,
            },
        ] {
            assert!(host.workspace_query(&bad).is_err());
        }
        host.api_writers.get_mut("project").unwrap().interrupted = true;
        assert!(host.workspace_query(&query).is_err());
        host.api_writers.get_mut("project").unwrap().interrupted = false;
        host.base.poisoned = true;
        assert!(host.workspace_query(&query).is_err());
        host.base.poisoned = false;
        host.codex_active
            .get_mut(&("project".into(), "cancel-host".into()))
            .unwrap()
            .job
            .state = CodexJobState::Unknown;
        let WorkspaceResult::CodexCancellations(page) = host.workspace_query(&query).unwrap()
        else {
            panic!("unknown page")
        };
        assert!(page.entries.is_empty());
    }

    #[test]
    fn workspace_codex_cancellation_original_cancel_ordering_regression() {
        host_cancel_validates_before_pause_and_pauses_before_control_delivery();
        failed_pause_or_projection_never_delivers_cancel_and_lost_receiver_stays_paused();
    }
}
