//! Resident-owned OMP execution. The UI does not own the worker or its lifetime.
use crate::{
    host::ordinary, omp_workflow_access::OmpWorkflowAccess, scheduler::CommandContext,
    startup_journal::StartupIntent, workflow_coordinator::WorkflowClient,
};
use cerulean_domain::{execution::*, omp_jobs::*, secret::SecretInput, workflow::*};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
};

pub(crate) enum Update {
    Assignment(crate::managed_assignment::Update),
    ToolCheckpoint {
        checkpoint: Box<crate::omp_tool_checkpoint::OmpToolCheckpoint>,
        reply: Option<AdviceReply>,
    },
    Progress(Box<ExecutionSnapshot>),
    Done {
        interrupted: bool,
        outcome: Result<(), String>,
        cleaned: bool,
    },
}
pub(crate) struct ActiveOmp {
    pub snapshot: ExecutionSnapshot,
    pub job: OmpJob,
    pub receiver: mpsc::Receiver<Update>,
    pub cancel: Option<tokio::sync::oneshot::Sender<()>>,
    pub thread: Option<std::thread::JoinHandle<()>>,
    pub disconnected: bool,
    pub preparation_cancel: Arc<AtomicBool>,
    pub tool_checkpoint: Option<crate::omp_tool_checkpoint::OmpToolCheckpoint>,
    pub advice_waiter: Option<AdviceReply>,
    pub advice_gate: Option<Arc<std::sync::atomic::AtomicU8>>,
}
pub(crate) type AdviceReply = tokio::sync::oneshot::Sender<
    Result<Option<cerulean_adapters::omp_transport::worker::advice::SteerAdvice>, String>,
>;
#[cfg(test)]
pub(crate) struct PreparationGate {
    pub entered: mpsc::Sender<()>,
    pub release: mpsc::Receiver<()>,
}
pub(crate) fn preflight(
    store: &(impl OmpWorkflowAccess + ?Sized),
    task: &str,
    attempt: &str,
    wanted: &str,
) -> Result<WorkerSelection, String> {
    identifier(task)?;
    identifier(attempt)?;
    artifact_path(attempt)?;
    require(attempt.len() <= 64, "OMP attempt ID too long")?;
    let plan = store.revision().ok_or("OMP方案缺失")?;
    require(plan.approval_digest()? == wanted, "OMP方案已变化")?;
    store.approval().ok_or("OMP方案尚未批准")?.validate(
        plan,
        "user",
        store
            .configuration()
            .execution_scope
            .as_ref()
            .ok_or("scope missing")?
            .policy_version,
    )?;
    let spec = plan.task(task)?;
    let worker = spec.worker.clone().ok_or("OMP执行模型和程序尚未绑定")?;
    worker.validate()?;
    require(
        spec.adapter_id == "omp" && worker.provider == "deepseek",
        "OMP执行器与批准供应商不符",
    )?;
    require(
        !crate::artifacts::contains_path(
            Path::new(&plan.project.root),
            Path::new(&worker.executable.path),
        ),
        "OMP程序必须位于任务目录之外",
    )?;
    require(
        plan.policy.budget_usd_cents.is_none(),
        "OMP金额硬预算尚不能执行",
    )?;
    plan.supervision_for(task)?;
    require(plan.policy.max_seconds <= 3600, "OMP单次时限不能超过一小时")?;
    require(
        plan.policy
            .tools
            .iter()
            .any(|p| matches!(p, ToolPermission::ReadFiles | ToolPermission::WriteFiles)),
        "OMP文件权限缺失",
    )?;
    require(
        store.state().revision_requested.is_none() && !store.requires_reopen(),
        "OMP项目正在修订或恢复",
    )?;
    require(
        store
            .state()
            .tasks
            .get(task)
            .is_some_and(|t| t.state == TaskState::Pending),
        "OMP任务不是待分配状态",
    )?;
    let definition = plan
        .workflow
        .tasks
        .iter()
        .find(|t| t.id == task)
        .ok_or("OMP任务缺失")?;
    require(
        definition.dependencies.iter().all(|d| {
            store
                .state()
                .tasks
                .get(d)
                .is_some_and(|t| t.state == TaskState::Accepted)
        }),
        "OMP依赖尚未验收",
    )?;
    require(
        !store
            .history()
            .iter()
            .any(|e| e.body.event.command.attempt() == Some(attempt)),
        "OMP尝试ID已经使用",
    )?;
    crate::omp_workflow_access::require_budget(store)?;
    Ok(worker)
}
pub(crate) fn observed_state(
    intent: &StartupIntent,
    store: &(impl crate::workflow_access::WorkflowAccess + ?Sized),
) -> OmpJobState {
    for e in store.history().iter().rev() {
        match &e.body.event.command {
            WorkflowCommand::Submit { task, attempt, .. }
            | WorkflowCommand::Reconcile {
                task,
                attempt,
                outcome: ReconcileOutcome::Submitted,
                ..
            } if task == &intent.task_id && attempt == &intent.attempt_id => {
                return OmpJobState::Submitted
            }
            WorkflowCommand::Reconcile {
                task,
                attempt,
                outcome: ReconcileOutcome::Cancelled,
                ..
            }
            | WorkflowCommand::AbandonUnsent { task, attempt, .. }
                if task == &intent.task_id && attempt == &intent.attempt_id =>
            {
                return OmpJobState::Cancelled
            }
            _ => {}
        }
    }
    OmpJobState::Unknown
}
pub(crate) fn launch(
    root: &Path,
    mut store: WorkflowClient,
    job: OmpJob,
    key: SecretInput,
    #[cfg(test)] gate: Option<PreparationGate>,
) -> Result<ActiveOmp, String> {
    let snapshot = crate::async_verification::snapshot(&store);
    let root = root.to_owned();
    let running = job.clone();
    let (sender, receiver) = mpsc::channel();
    let (cancel, mut cancellation) = tokio::sync::oneshot::channel();
    let preparation_cancel = Arc::new(AtomicBool::new(false));
    let preparing = preparation_cancel.clone();
    let thread = std::thread::Builder::new()
        .name(format!("omp-{}", job.attempt_id))
        .spawn(move || {
            let mut cleaned = false;
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|_| "OMP后台运行时启动失败")?;
                    runtime.block_on(run(
                        &root,
                        &mut store,
                        &running,
                        key,
                        &sender,
                        &mut cancellation,
                        &mut cleaned,
                        preparing,
                        #[cfg(test)]
                        gate,
                    ))
                }));
            let panicked = result.is_err();
            let outcome = match result {
                Ok(r) => r,
                Err(_) => Err("OMP后台异常，请核对原尝试".into()),
            };
            let interrupted = store.requires_reopen() || panicked;
            drop(store); // The host returns the writer only after every user exits.
            let _ = sender.send(Update::Done {
                interrupted,
                outcome,
                cleaned,
            });
        })
        .map_err(|_| "OMP后台线程启动失败")?;
    Ok(ActiveOmp {
        snapshot,
        job,
        receiver,
        cancel: Some(cancel),
        thread: Some(thread),
        disconnected: false,
        preparation_cancel,
        tool_checkpoint: None,
        advice_waiter: None,
        advice_gate: None,
    })
}
#[allow(clippy::too_many_arguments)]
async fn run(
    root: &Path,
    store: &mut impl OmpWorkflowAccess,
    job: &OmpJob,
    key: SecretInput,
    sender: &mpsc::Sender<Update>,
    cancel: &mut tokio::sync::oneshot::Receiver<()>,
    cleaned: &mut bool,
    preparing: Arc<AtomicBool>,
    #[cfg(test)] gate: Option<PreparationGate>,
) -> Result<(), String> {
    *cleaned = true; // No process exists during any filesystem preflight failure.
    store.refresh()?;
    #[cfg(test)]
    if let Some(gate) = gate {
        gate.entered
            .send(())
            .map_err(|_| "preparation fixture unavailable")?;
        loop {
            if preparing.load(Ordering::Acquire) {
                return Err("OMP workspace preparation cancelled".into());
            }
            match gate
                .release
                .recv_timeout(std::time::Duration::from_millis(20))
            {
                Ok(()) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(_) => return Err("preparation fixture closed".into()),
            }
        }
    }
    require(
        preflight(store, &job.task_id, &job.attempt_id, &job.revision_digest)? == job.worker,
        "OMP启动程序、模型或运行目录与批准不符",
    )?;
    if !matches!(
        cancel.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    ) {
        *cleaned = true;
        return Err("OMP启动已取消，尚未分配或发送".into());
    }
    let plan = store.revision().ok_or("OMP方案缺失")?.clone();
    let (workspace, workspace_proof) = if plan.worktree.is_some() {
        let (path, proof) =
            store.prepare_omp_workspace(&job.task_id, &job.attempt_id, preparing.clone())?;
        (path, Some(proof))
    } else {
        (plan.execution_root(&job.task_id, &job.attempt_id)?, None)
    };
    if preparing.load(Ordering::Acquire)
        || !matches!(
            cancel.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        )
    {
        return Err("OMP目录准备已停止，尚未启动执行器".into());
    }
    crate::blind_review_guard::require_isolated_execution(
        root,
        plan.task(&job.task_id)?.worker_directory.is_some(),
    )?;
    let account = if plan.task(&job.task_id)?.worker_directory.is_some() {
        Some(crate::windows_worker_account::load(
            root,
            &plan,
            &job.task_id,
        )?)
    } else {
        None
    };
    let (profiles, id) = if account.is_some() {
        (
            Path::new(&workspace).join(".cerulean-omp"),
            digest(&(job.project_id.as_str(), job.attempt_id.as_str()))?,
        )
    } else if let Some(parent) = &job.worker.omp_runtime_parent {
        let parent = crate::omp_process::check_profile_parent(Path::new(parent))?;
        (
            parent.join("cerulean-omp-profiles"),
            digest(&(
                root.to_str().ok_or("OMP数据目录无效")?,
                job.project_id.as_str(),
                job.attempt_id.as_str(),
            ))?,
        )
    } else {
        (
            root.join("omp-profiles"),
            digest(&(job.project_id.as_str(), job.attempt_id.as_str()))?,
        )
    };
    match fs::create_dir(&profiles) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err("OMP隔离目录不可用，请检查已批准运行目录的写入权限".into()),
    }
    ordinary(&profiles, true)?;
    let profile = profiles.join(id);
    let mut process = match crate::omp_process::WorkerProcess::launch_with_account(
        &job.worker,
        &profile,
        &key,
        account,
    ) {
        Ok(p) => p,
        Err(e) => {
            *cleaned = true;
            return Err(e);
        }
    };
    *cleaned = false;
    drop(key);
    let outcome = async {
        let session = tokio::select! {
            value = process.connect() => value?,
            _ = &mut *cancel => return Err("OMP握手已取消，尚未发送".into()),
        };
        if !matches!(
            cancel.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ) {
            return Err("OMP分配前已取消".into());
        }
        let fence = store.state().tasks[&job.task_id]
            .fence
            .checked_add(1)
            .ok_or("OMP fence overflow")?;
        let attempt = Attempt {
            workspace_proof,
            id: job.attempt_id.clone(),
            task_id: job.task_id.clone(),
            worker_id: "worker-omp".into(),
            revision_digest: job.revision_digest.clone(),
            fence,
            binding: SessionBinding {
                adapter_id: "omp".into(),
                adapter_version: "host-rpc-v2".into(),
                session_id: session.session_id,
                workspace,
                identity_strength: IdentityStrength::ControllerRequest(ControllerRequestIdentity {
                    connection_id: format!(
                        "omp-{}",
                        &digest(&(job.project_id.as_str(), job.attempt_id.as_str()))?[..32]
                    ),
                    request_id: format!("prompt-{}", job.attempt_id),
                }),
            },
        };
        if let Some(operation) = &job.management_operation {
            crate::managed_assignment::assign(store, operation, attempt, |update| {
                sender
                    .send(Update::Assignment(update))
                    .map_err(|_| "OMP管理通道中断".into())
            })
            .await?;
        } else {
            store.command(
                &format!("omp-{}-assign", job.attempt_id),
                "deputy",
                WorkflowCommand::Assign {
                    task: job.task_id.clone(),
                    worker: "worker-omp".into(),
                    attempt: job.attempt_id.clone(),
                },
                CommandContext {
                    execution: Some(ExecutionEvidence::Attempt { attempt }),
                    ..Default::default()
                },
                false,
            )?;
        }
        let _ = sender.send(Update::Progress(Box::new(
            crate::async_verification::snapshot(store),
        )));
        let tool_sender = sender.clone();
        let policy = store
            .revision()
            .ok_or("OMP plan missing")?
            .supervision_for(&job.task_id)?
            .cloned();
        let mut seen = std::collections::BTreeSet::new();
        crate::omp_execution::dispatch_files_progress(
            store,
            process.channel()?,
            &job.task_id,
            &job.attempt_id,
            cancel,
            Box::new(move |checkpoint| {
                let wait = match &policy {
                    Some(p) if p.delivery_enabled => checkpoint
                        .tools
                        .supervision_incidents(p)?
                        .iter()
                        .any(|i| seen.insert(i.id.clone())),
                    _ => false,
                };
                let (reply, boundary) = if wait {
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    (Some(tx), Some(rx))
                } else {
                    (None, None)
                };
                tool_sender
                    .send(Update::ToolCheckpoint {
                        checkpoint: Box::new(checkpoint),
                        reply,
                    })
                    .map_err(|_| "Original OMP tool observer unavailable".to_string())?;
                Ok(boundary)
            }),
            |store| {
                let _ = sender.send(Update::Progress(Box::new(
                    crate::async_verification::snapshot(store),
                )));
            },
        )
        .await?;
        Ok(())
    }
    .await;
    let cleanup = process.close().await;
    *cleaned = cleanup.is_ok();
    outcome.and(cleanup.map(|_| ()))
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CancelRecord {
    version: u32,
    startup_head: String,
    reason: String,
    at: String,
    hash: String,
}
pub(crate) const MAX_CANCEL_BYTES: usize = 65536;
pub(crate) fn cancel_path(
    root: &Path,
    intent: &StartupIntent,
) -> Result<std::path::PathBuf, String> {
    identifier(&intent.project_id)?;
    identifier(&intent.attempt_id)?;
    Ok(root.join("omp-cancellations").join(format!(
        "{}.json",
        digest(&(&intent.project_id, &intent.attempt_id))?
    )))
}
fn read_cancel(path: &Path, head: &str) -> Result<Option<(CancelRecord, Vec<u8>)>, String> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("OMP停止记录不可读".into()),
        Ok(_) => {}
    }
    ordinary(path, false)?;
    let mut data = vec![];
    fs::File::open(path)
        .map_err(|_| "OMP停止记录不可读")?
        .take((MAX_CANCEL_BYTES + 1) as u64)
        .read_to_end(&mut data)
        .map_err(|_| "OMP停止记录不可读")?;
    require(data.len() <= MAX_CANCEL_BYTES, "OMP停止记录过大")?;
    let r: CancelRecord = serde_json::from_slice(&data).map_err(|_| "OMP停止记录无效")?;
    require(
        r.version == 1
            && r.startup_head == head
            && r.hash == digest(&(r.version, &r.startup_head, &r.reason, &r.at))?,
        "OMP停止记录绑定错误",
    )?;
    timestamp(&r.at)?;
    reason(&r.reason)?;
    Ok(Some((r, data)))
}
pub(crate) fn cancellation(
    root: &Path,
    intent: &StartupIntent,
    head: &str,
) -> Result<Option<String>, String> {
    if let Some(reason) = crate::omp_stop_review::accepted(root, intent, head)? {
        return Ok(Some(format!("中断停止记录已人工复核：{reason}")));
    }
    let path = cancel_path(root, intent)?;
    let parent = path.parent().unwrap();
    match fs::symlink_metadata(parent) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("OMP停止目录不可读".into()),
        Ok(_) => ordinary(parent, true)?,
    }
    let pending = path.with_extension("pending");
    // The resident owns the data-root writer lock. Validate both names before
    // completing an interrupted immutable publication; never overwrite evidence.
    let mut committed = read_cancel(&path, head)?;
    if let Some((record, bytes)) = read_cancel(&pending, head)? {
        if let Some((_, published)) = &committed {
            require(*published == bytes, "OMP停止记录发布冲突")?;
        } else {
            // Complete bytes may survive a crash before the original sync.
            // Confirm their durability before publishing the recovered name.
            fs::OpenOptions::new()
                .write(true)
                .open(&pending)
                .and_then(|f| f.sync_all())
                .map_err(|_| "OMP停止记录恢复同步失败")?;
            fs::hard_link(&pending, &path).map_err(|_| "OMP停止记录恢复发布失败")?;
            committed = Some((record, bytes));
        }
        fs::remove_file(&pending).map_err(|_| "OMP停止记录恢复收尾失败")?;
    }
    Ok(committed.map(|(r, _)| r.reason))
}
pub(crate) fn reason(s: &str) -> Result<(), String> {
    require(
        !s.trim().is_empty() && s.len() <= 8000 && !s.contains('\0'),
        "请填写有效的停止或关闭原因",
    )
}
#[cfg(test)]
pub(crate) fn record_cancel(
    root: &Path,
    intent: &StartupIntent,
    head: &str,
    text: &str,
) -> Result<(), String> {
    record_cancel_observed(root, intent, head, text, |_| Ok(()))
}
pub(crate) fn record_cancel_observed(
    root: &Path,
    intent: &StartupIntent,
    head: &str,
    text: &str,
    mut observer: impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    reason(text)?;
    sha256(head)?;
    if cancellation(root, intent, head)?.is_some() {
        return Ok(());
    }
    let path = cancel_path(root, intent)?;
    let parent = path.parent().unwrap();
    fs::create_dir_all(parent).map_err(|_| "OMP停止目录不可用")?;
    ordinary(parent, true)?;
    let mut r = CancelRecord {
        version: 1,
        startup_head: head.into(),
        reason: text.into(),
        at: chrono::Utc::now().to_rfc3339(),
        hash: String::new(),
    };
    r.hash = digest(&(r.version, &r.startup_head, &r.reason, &r.at))?;
    let bytes = serde_json::to_vec(&r).map_err(|_| "OMP停止记录编码失败")?;
    require(bytes.len() <= MAX_CANCEL_BYTES, "OMP停止记录过大")?;
    let pending = path.with_extension("pending");
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&pending)
        .map_err(|_| "OMP停止记录不能创建")?;
    observer("temp-created")?;
    let split = bytes.len() / 2;
    file.write_all(&bytes[..split])
        .map_err(|_| "OMP停止记录写入失败")?;
    observer("partial-written")?;
    file.write_all(&bytes[split..])
        .and_then(|_| file.sync_all())
        .map_err(|_| "OMP停止记录同步失败")?;
    drop(file);
    observer("temp-synced")?;
    fs::hard_link(&pending, &path).map_err(|_| "OMP停止记录发布失败")?;
    observer("published")?;
    fs::remove_file(pending).map_err(|_| "OMP停止记录收尾失败")?;
    observer("pending-removed")
}
