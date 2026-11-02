//! One desktop writer, with a durable intent before transferring workflow ownership.
use super::*;
use crate::desktop_execution::{self as execution, ReservedSend, TaskReview};
use cerulean_domain::desktop_sessions::DesktopTaskStatus;
use execution::cancellation::{self, ReservedRequest};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
#[path = "desktop_stop_host.rs"]
mod stop;
fn check_task_reader(original: &str, current: &str, requested: Option<&str>) -> Result<(), String> {
    match requested {
        Some(reader) => require(reader == current, "读取器版本已变化，请刷新后回读原任务"),
        None => require(original == current, "请使用原批准版本核对任务"),
    }
}

enum Operation {
    Task(Option<Box<ReservedSend>>),
    Stop(Option<ReservedRequest>),
}

struct Done {
    store: Result<Box<DurableWorkflow>, String>,
    outcome: Result<(), String>,
}
pub(in crate::host) struct Active {
    progress: super::desktop_progress::Tracker,
    pub snapshot: ExecutionSnapshot,
    pub status: DesktopTaskStatus,
    config: SchedulerConfig,
    receiver: mpsc::Receiver<Done>,
    thread: Option<std::thread::JoinHandle<()>>,
    fault: bool,
    cancel: Arc<AtomicBool>,
    pending_stop: Option<ReservedRequest>,
}
impl RuntimeHost {
    pub(in crate::host) fn refresh_desktop_tasks(&mut self) -> Result<(), String> {
        let mut tasks = vec![];
        for (project, attempt) in execution::recorded_attempts(&self.root)? {
            if self
                .desktop_active
                .as_ref()
                .is_some_and(|a| a.status.review.project_id == project)
            {
                // Preserve all earlier attempts while this project's writer is away.
                if let Some(old) = self
                    .desktop_access
                    .tasks
                    .iter()
                    .find(|t| t.review.project_id == project && t.review.attempt.id == attempt)
                {
                    tasks.push(old.clone());
                }
                continue;
            }
            let store = self.projects.get(&project).ok_or("桌面任务项目暂不可用")?;
            let (review, proof) = execution::inspect(&self.root, store, &project, &attempt)?;
            let submitted = store.history().iter().any(|e| matches!(&e.body.event.command,
                WorkflowCommand::Submit { attempt: id, .. }
                | WorkflowCommand::Reconcile { attempt: id, outcome: ReconcileOutcome::Submitted, .. }
                if id == &attempt));
            let error = self
                .desktop_access
                .tasks
                .iter()
                .find(|t| t.review.id == review.id)
                .and_then(|t| t.error.clone());
            let retired = execution::is_retired(store, &attempt);
            let error = if retired { None } else { error };
            tasks.push(DesktopTaskStatus {
                progress: self
                    .desktop_access
                    .tasks
                    .iter()
                    .find(|t| t.review.id == review.id)
                    .and_then(|t| t.progress.clone()),
                reviewed_stop_record: cancellation::review::reviewed(&self.root, store, &review)?,
                cancellation: cancellation::inspect(&self.root, store, &review)?,
                stopping: false,
                retired,
                retirement_digest: None,
                review,
                proof,
                submitted,
                active: false,
                reading: false,
                error,
            });
        }
        self.desktop_access.tasks = tasks;
        Ok(())
    }
    pub(in crate::host) fn prepare_desktop_task(
        &mut self,
        project: &str,
        task: &str,
        attempt: &str,
    ) -> Result<(), String> {
        self.require_available(project)?;
        self.require_desktop_tasks_resolved()?;
        require(
            load_trials(&self.root)?.iter().all(|t| t.proof.is_some()),
            "请先核对原固定消息验证",
        )?;
        let store = self.projects.get(project).ok_or("项目不存在")?;
        let selection = store
            .revision()
            .ok_or("方案缺失")?
            .task(task)?
            .desktop_worker
            .as_ref()
            .ok_or("桌面配置缺失")?;
        crate::windows_worker_account::desktop(
            &self.root,
            store.revision().ok_or("方案缺失")?,
            task,
        )?;
        self.validate_desktop_selection(project, selection)?;
        require(
            selection.binding.bridge_sha256 == fingerprint(),
            "桥接版本变化，需要重新配置和批准",
        )?;
        require(
            store.state().tasks.get(task).is_some_and(|t| {
                t.state == TaskState::Assigned && t.attempt.as_deref() == Some(attempt)
            }),
            "必须先由副指挥分配本次桌面尝试",
        )?;
        let previous = execution::predecessor(&self.root, store, &selection.binding.id)?;
        let window = selection.binding.observation.window.clone();
        self.desktop_access.task_review = None;
        self.launch_desktop(
            ReadRequest::TaskPrepare {
                version: 1,
                window,
                previous,
            },
            Action::PrepareTask {
                project: project.into(),
                task: task.into(),
                attempt: attempt.into(),
            },
        )
    }
    pub(in crate::host) fn send_desktop_task(&mut self, id: &str) -> Result<(), String> {
        require(
            self.desktop_active.is_none() && self.desktop_probe.is_none(),
            "桌面操作仍在进行",
        )?;
        let review = self
            .desktop_access
            .task_review
            .clone()
            .ok_or("请先准备并审阅任务消息")?;
        require(
            review.id == id && review.bridge_sha256 == fingerprint(),
            "任务审阅内容或桥接版本已变化",
        )?;
        self.require_available(&review.project_id)?;
        self.require_worker_slot()?;
        self.authorize_desktop_send(&review.project_id, &review.attempt.id)?;
        self.require_desktop_tasks_resolved()?;
        require(
            load_trials(&self.root)?.iter().all(|t| t.proof.is_some()),
            "请先核对原固定消息验证",
        )?;
        self.require_desktop_capacity()?;
        // Retire the transient review even if publication or launch fails. A saved
        // intent is never reconstituted as a send capability after a restart.
        self.desktop_access.task_review = None;
        let store = self
            .projects
            .get_mut(&review.project_id)
            .ok_or("项目不存在")?;
        crate::windows_worker_account::desktop(
            &self.root,
            store.revision().ok_or("方案缺失")?,
            &review.attempt.task_id,
        )?;
        let permit = execution::reserve(&self.root, store, &review);
        self.refresh_desktop_tasks()?;
        self.launch_desktop_task(review, Some(permit?))
    }
    pub(in crate::host) fn read_desktop_task(
        &mut self,
        project: &str,
        attempt: &str,
    ) -> Result<(), String> {
        self.read_desktop_task_with_reader(project, attempt, None)
    }
    pub(in crate::host) fn read_desktop_task_with_reader(
        &mut self,
        project: &str,
        attempt: &str,
        reader: Option<&str>,
    ) -> Result<(), String> {
        require(
            self.desktop_active.is_none() && self.desktop_probe.is_none(),
            "桌面操作仍在进行",
        )?;
        self.require_available(project)?;
        self.require_desktop_capacity()?;
        let store = self.projects.get(project).ok_or("项目不存在")?;
        require(
            !execution::is_retired(store, attempt),
            "本地跟踪已结束，原尝试不能恢复或重发",
        )?;
        let (review, _) = execution::inspect(&self.root, store, project, attempt)?;
        cancellation::review::require_unrestricted(&self.root, store, &review)?;
        check_task_reader(&review.bridge_sha256, &fingerprint(), reader)?;
        require(
            !self
                .desktop_access
                .tasks
                .iter()
                .any(|t| t.review.id == review.id && t.submitted),
            "此任务已提交，请查看独立验收",
        )?;
        if let Some(stop) = cancellation::inspect(&self.root, store, &review)? {
            // Upgraded task readers do not authorize a changed cancellation reader.
            require(
                review.bridge_sha256 == fingerprint(),
                "原停止记录需要原批准版本核对",
            )?;
            require(!stop.confirmed, "原桌面任务已确认取消")?;
            self.launch_desktop_operation(review, Operation::Stop(None))
        } else {
            self.launch_desktop_task(review, None)
        }
    }
    fn require_desktop_capacity(&self) -> Result<(), String> {
        require(
            self.active.len()
                + self.api_active.len()
                + self.omp_active.len()
                + self.codex_active.len()
                < 4,
            "后台执行已达并发上限",
        )
    }
    fn launch_desktop_task(
        &mut self,
        review: TaskReview,
        permit: Option<ReservedSend>,
    ) -> Result<(), String> {
        self.launch_desktop_operation(review, Operation::Task(permit.map(Box::new)))
    }
    fn launch_desktop_operation(
        &mut self,
        review: TaskReview,
        operation: Operation,
    ) -> Result<(), String> {
        let project = review.project_id.clone();
        let account = crate::windows_worker_account::desktop(
            &self.root,
            self.projects
                .get(&project)
                .and_then(|s| s.revision())
                .ok_or("方案缺失")?,
            &review.attempt.task_id,
        )?;
        let cancellation = cancellation::inspect(
            &self.root,
            self.projects.get(&project).ok_or("项目账本暂不可用")?,
            &review,
        )?;
        let mut store = self.projects.remove(&project).ok_or("项目账本暂不可用")?;
        let snapshot = crate::async_verification::snapshot(&store);
        let config = store.configuration().clone();
        let worker_config = config.clone();
        let root = self.root.clone();
        let path = root.join("workflows").join(&project);
        let worker_review = review.clone();
        let reading = matches!(&operation, Operation::Task(None) | Operation::Stop(None));
        let stopping = matches!(&operation, Operation::Stop(_));
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let (sender, receiver) = mpsc::channel();
        #[cfg(test)]
        let fixture = self.desktop_fixture;
        let thread = std::thread::Builder::new()
            .name("desktop-task".into())
            .spawn(move || {
                let _account_guard = account;
                let call = |r: &TaskReview| {
                    let request = if reading {
                        ReadRequest::TaskRead {
                            version: 1,
                            review: r.clone(),
                        }
                    } else {
                        ReadRequest::TaskSend {
                            version: 1,
                            review: r.clone(),
                        }
                    };
                    #[cfg(test)]
                    let reply = if let Some(f) = fixture {
                        f(&request)
                    } else {
                        observe_cancellable(&root, &request, &worker_cancel)
                    };
                    #[cfg(not(test))]
                    let reply = observe_cancellable(&root, &request, &worker_cancel);
                    let mut proof = reply?
                        .task_proof
                        .ok_or("任务回复尚未核实；请回读原记录，禁止重发")?;
                    // Native adapters cannot choose their own provenance.
                    proof.reader_sha256 = Some(fingerprint());
                    Ok(proof)
                };
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if let Operation::Stop(permit) = operation {
                        let mut transport = stop::BridgeTransport {
                            root: &root,
                            #[cfg(test)]
                            fixture,
                        };
                        return cancellation::run(
                            &root,
                            &mut store,
                            &worker_review,
                            permit,
                            &mut transport,
                        );
                    }
                    let Operation::Task(permit) = operation else {
                        unreachable!()
                    };
                    if let Some(permit) = permit {
                        execution::execute_reserved(&root, &mut store, *permit, call)
                    } else {
                        execution::recover_and_submit(
                            &root,
                            &mut store,
                            &worker_review.project_id,
                            &worker_review.attempt.id,
                            call,
                        )
                    }
                    .map(|_| ())
                }));
                let panicked = result.is_err();
                let outcome =
                    result.unwrap_or_else(|_| Err("桌面后台异常；保留原意图，禁止重发".into()));
                let store = if panicked || store.requires_reopen() {
                    drop(store);
                    DurableWorkflow::open(&path, worker_config)
                } else {
                    Ok(store)
                };
                let _ = sender.send(Done {
                    store: store.map(Box::new),
                    outcome,
                });
            });
        let error = thread
            .as_ref()
            .err()
            .map(|_| "后台线程未启动；保留原意图，请回读核对".to_string());
        self.desktop_active = Some(Active {
            progress: Default::default(),
            snapshot,
            config,
            receiver,
            thread: thread.ok(),
            fault: false,
            cancel,
            pending_stop: None,
            status: DesktopTaskStatus {
                progress: None,
                reviewed_stop_record: None,
                cancellation,
                stopping,
                retired: false,
                retirement_digest: None,
                review,
                proof: None,
                submitted: false,
                active: true,
                reading,
                error: error.clone(),
            },
        });
        self.desktop_access.error = None;
        if let Some(error) = error {
            self.poll_desktop_tasks();
            return Err(error);
        }
        Ok(())
    }
    pub(in crate::host) fn poll_desktop_tasks(&mut self) {
        let Some(active) = &mut self.desktop_active else {
            return;
        };
        let finished = active
            .thread
            .as_ref()
            .is_none_or(|thread| thread.is_finished());
        let stop_observing = finished
            || active.fault
            || active.pending_stop.is_some()
            || active.status.reading
            || active.status.stopping;
        active.progress.poll(
            &self.root,
            &mut active.status,
            stop_observing,
            #[cfg(test)]
            self.desktop_fixture,
        );
        // Neither channel disconnect nor Done releases ownership before the
        // thread exits. Thus snapshots and controls never race a live writer.
        if active.fault || !finished || !active.progress.idle() {
            return;
        }
        if let Some(thread) = active.thread.take() {
            let _ = thread.join();
        }
        let project = active.status.review.project_id.clone();
        let done = active.receiver.try_recv().unwrap_or_else(|_| Done {
            store: DurableWorkflow::open(
                &self.root.join("workflows").join(&project),
                active.config.clone(),
            )
            .map(Box::new),
            outcome: Err("后台通道中断或线程未启动；请回读原记录".into()),
        });
        match done.store {
            Ok(store) => {
                self.projects.insert(project, *store);
                let active = self.desktop_active.take().unwrap();
                self.desktop_access.error = done.outcome.as_ref().err().cloned();
                if let Some(t) = self
                    .desktop_access
                    .tasks
                    .iter_mut()
                    .find(|t| t.review.id == active.status.review.id)
                {
                    t.error = done.outcome.err();
                    t.progress = active.status.progress.clone();
                }
                if let Err(error) = self.refresh_desktop_tasks() {
                    self.desktop_access.error = Some(error);
                    return;
                }
                if let Some(permit) = active.pending_stop {
                    if let Err(error) = self.launch_desktop_operation(
                        active.status.review,
                        Operation::Stop(Some(permit)),
                    ) {
                        self.desktop_access.error = Some(error);
                    }
                }
            }
            Err(error) => {
                active.fault = true;
                active.status.error = Some(format!("任务账本无法恢复，需修复后重开：{error}"));
                self.desktop_access.error = active.status.error.clone();
                // Keep the unavailable project visible and exclusively owned.
            }
        }
    }
}

#[cfg(test)]
mod reader_tests {
    use super::*;
    #[test]
    fn upgraded_reader_requires_explicit_current_identity() {
        assert!(check_task_reader("old", "new", None).is_err());
        assert!(check_task_reader("old", "new", Some("old")).is_err());
        assert!(check_task_reader("old", "new", Some("")).is_err());
        assert!(check_task_reader("old", "new", Some("new")).is_ok());
        assert!(check_task_reader("new", "new", None).is_ok());
        assert!(check_task_reader("new", "new", Some("old")).is_err());
    }
}
