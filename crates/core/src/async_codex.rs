//! Service-owned workers. No UI lifetime or client disconnect owns these threads.
use crate::{
    codex_workflow_access::CodexWorkflowAccess, host::ordinary, scheduler::CommandContext,
    workflow_access::WorkflowAccess, workflow_coordinator::WorkflowClient,
};
use cerulean_adapters::codex::CodexClient;
use cerulean_domain::{execution::*, workflow::*, CodexJob, CodexJobState};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
};

pub(crate) enum Update {
    Advice(crate::supervised_worker::AdviceUpdate),
    PrepareAssignment {
        operation: String,
        attempt: Box<Attempt>,
        snapshot: Box<ExecutionSnapshot>,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    CommitAssignment {
        operation: String,
        entry: Box<crate::scheduler::Entry>,
        snapshot: Box<ExecutionSnapshot>,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    Progress(Box<ExecutionSnapshot>),
    Done {
        interrupted: bool,
        cleaned: bool,
        outcome: Result<(), String>,
    },
}
pub(crate) struct ActiveCodex {
    pub preparation_cancel: Arc<AtomicBool>,
    pub advice_wake: tokio::sync::watch::Sender<()>,
    pub advice_gate: std::sync::Arc<std::sync::atomic::AtomicU8>,
    pub observation: Option<cerulean_adapters::codex_observation::ObservationCapture>,
    pub observation_saved: Option<String>,
    pub observation_error: Option<String>,
    pub snapshot: ExecutionSnapshot,
    pub job: CodexJob,
    pub receiver: mpsc::Receiver<Update>,
    pub disconnected: bool,
    pub cancel: Option<tokio::sync::oneshot::Sender<String>>,
}
#[cfg(test)]
pub(crate) struct PreparationGate {
    pub entered: mpsc::Sender<()>,
    pub release: mpsc::Receiver<()>,
}
fn folder(root: &Path, project: &str) -> PathBuf {
    root.join("codex-jobs").join(project)
}
pub(crate) fn save(root: &Path, job: &CodexJob) -> Result<(), String> {
    identifier(&job.project_id)?;
    identifier(&job.attempt_id)?;
    let base = root.join("codex-jobs");
    fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    ordinary(&base, true)?;
    let dir = folder(root, &job.project_id);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    ordinary(&dir, true)?;
    let dest = dir.join(format!("{}.json", job.attempt_id));
    if dest.exists() {
        ordinary(&dest, false)?;
    }
    let temp = dir.join(format!(
        "{}.{}.tmp",
        job.attempt_id,
        chrono::Utc::now()
            .timestamp_nanos_opt()
            .ok_or("invalid clock")?
    ));
    let bytes = serde_json::to_vec(job).map_err(|e| e.to_string())?;
    require(bytes.len() <= 64 * 1024, "Codex job too large")?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    drop(file);
    fs::rename(temp, dest).map_err(|e| e.to_string())
}
fn validate(job: &CodexJob, store: &(impl WorkflowAccess + ?Sized)) -> Result<(), String> {
    if let Some(operation) = &job.management_operation {
        identifier(operation)?;
    }
    for id in [&job.project_id, &job.task_id, &job.attempt_id] {
        identifier(id)?;
    }
    timestamp(&job.started_at)?;
    timestamp(&job.updated_at)?;
    sha256(&job.revision_digest)?;
    job.worker.validate()?;
    require(
        job.message.as_ref().is_none_or(|m| m.len() <= 4096),
        "Codex status message too large",
    )?;
    let plan = store
        .history()
        .iter()
        .find_map(|entry| match &entry.body.context.execution {
            Some(ExecutionEvidence::Revision { revision })
                if revision
                    .approval_digest()
                    .is_ok_and(|d| d == job.revision_digest) =>
            {
                Some(revision)
            }
            _ => None,
        })
        .ok_or("Codex job approved revision missing")?;
    require(
        plan.project.id == job.project_id
            && plan.task(&job.task_id)?.worker.as_ref() == Some(&job.worker),
        "Codex job differs from approved worker",
    )?;
    require(store.history().iter().any(|entry| matches!(&entry.body.context.execution,Some(ExecutionEvidence::Approval { approval }) if approval.revision_digest==job.revision_digest)),"Codex job approval missing")
}
pub(crate) fn observed_state(
    job: &CodexJob,
    store: &(impl WorkflowAccess + ?Sized),
) -> CodexJobState {
    if store.history().iter().any(|entry| {
        matches!(&entry.body.event.command,
        WorkflowCommand::Reconcile { task, attempt, outcome: ReconcileOutcome::Cancelled, .. }
        if task == &job.task_id && attempt == &job.attempt_id)
    }) {
        return CodexJobState::Cancelled;
    }
    if store.history().iter().any(|entry| {
        matches!(
            &entry.body.event.command,
            WorkflowCommand::AbandonUnsent { task, attempt, .. }
                if task == &job.task_id && attempt == &job.attempt_id
        )
    }) {
        return CodexJobState::Abandoned;
    }
    let submitted = store
        .history()
        .iter()
        .any(|entry| match &entry.body.event.command {
            WorkflowCommand::Submit { task, attempt, .. } => {
                task == &job.task_id && attempt == &job.attempt_id
            }
            WorkflowCommand::Reconcile {
                task,
                attempt,
                outcome: ReconcileOutcome::Submitted,
                ..
            } => task == &job.task_id && attempt == &job.attempt_id,
            _ => false,
        });
    if submitted {
        CodexJobState::Submitted
    } else if store
        .attempt_record(&job.task_id, &job.attempt_id)
        .is_none()
        && job.state == CodexJobState::Failed
    {
        CodexJobState::Failed
    } else {
        CodexJobState::Unknown
    }
}
pub(crate) fn load(
    root: &Path,
    project: &str,
    store: &(impl WorkflowAccess + ?Sized),
) -> Result<Vec<CodexJob>, String> {
    let dir = folder(root, project);
    if !dir.exists() {
        return Ok(vec![]);
    }
    ordinary(&root.join("codex-jobs"), true)?;
    ordinary(&dir, true)?;
    let mut jobs = vec![];
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_some_and(|e| e == "tmp") {
            continue;
        }
        ordinary(&path, false)?;
        require(
            path.extension().is_some_and(|e| e == "json"),
            "unexpected Codex job file",
        )?;
        let mut bytes = vec![];
        File::open(&path)
            .map_err(|e| e.to_string())?
            .take(65537)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        require(bytes.len() <= 65536, "Codex job file too large")?;
        let mut job: CodexJob =
            serde_json::from_slice(&bytes).map_err(|_| "invalid Codex job JSON")?;
        require(
            job.project_id == project
                && path
                    .file_stem()
                    .is_some_and(|s| s == job.attempt_id.as_str()),
            "Codex job scope mismatch",
        )?;
        validate(&job, store)?;
        let previous_state = job.state.clone();
        job.state = observed_state(&job, store);
        if job.state == CodexJobState::Unknown && previous_state != CodexJobState::Unknown {
            job.message = Some("执行结果待对账；不会自动重新发送任务".into());
        } else if job.state == CodexJobState::Cancelled {
            job.message = Some("原会话已确认本次执行停止；已有修改保留，后续调度仍暂停".into());
        } else if job.state == CodexJobState::Abandoned {
            job.message =
                Some("用户已放弃未派发尝试；原始分配和处置原因保留，未重新发送任务".into());
        }
        save(root, &job)?;
        jobs.push(job);
        require(jobs.len() <= 1000, "Codex job history limit")?;
    }
    Ok(jobs)
}
/// Deny executable replacement while this worker owns the process.
fn inspect_executable(path: &str) -> Result<(File, String), String> {
    use sha2::{Digest, Sha256};
    use std::os::windows::fs::OpenOptionsExt;
    ordinary(Path::new(path), false)?;
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(path)
        .map_err(|_| "Codex executable cannot be locked")?;
    require(
        file.metadata().map_err(|e| e.to_string())?.len() <= 512 * 1024 * 1024,
        "Codex executable too large",
    )?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|_| "Codex executable read failed")?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok((file, format!("{:x}", hasher.finalize())))
}
pub(crate) fn describe(executable: &Path, model: &str) -> Result<WorkerSelection, String> {
    require(
        executable.is_absolute(),
        "Codex executable must be absolute",
    )?;
    ordinary(executable, false)?;
    let path = fs::canonicalize(executable).map_err(|e| e.to_string())?;
    let path = path.to_str().ok_or("non-Unicode Codex executable")?;
    let (_, hash) = inspect_executable(path)?;
    let worker = WorkerSelection {
        omp_runtime_parent: None,
        provider: "openai".into(),
        model: model.into(),
        executable: ValidationFile {
            path: path.into(),
            sha256: hash,
        },
    };
    worker.validate()?;
    Ok(worker)
}
fn lock_executable(worker: &WorkerSelection) -> Result<File, String> {
    let (file, hash) = inspect_executable(&worker.executable.path)?;
    require(
        hash == worker.executable.sha256,
        "Codex executable changed since approval",
    )?;
    Ok(file)
}
pub(crate) fn preflight(
    store: &(impl WorkflowAccess + ?Sized),
    task: &str,
    attempt: &str,
    wanted: &str,
) -> Result<WorkerSelection, String> {
    identifier(task)?;
    identifier(attempt)?;
    // Derived operation IDs also need to fit the ledger identifier limit.
    require(attempt.len() <= 64, "Codex attempt ID too long")?;
    let plan = store.revision().ok_or("chief proposal missing")?;
    require(
        plan.approval_digest()? == wanted,
        "approved revision changed",
    )?;
    store.approval().ok_or("user approval missing")?.validate(
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
    let worker = spec
        .worker
        .clone()
        .ok_or("方案需绑定 Codex 模型和程序后重新确认")?;
    worker.validate()?;
    require(
        spec.adapter_id == "codex" && worker.provider == "openai",
        "Codex worker route mismatch",
    )?;
    require(
        Path::new(&worker.executable.path)
            .file_name()
            .is_some_and(|s| s.eq_ignore_ascii_case("codex.exe")),
        "Codex executable name mismatch",
    )?;
    require(
        !crate::artifacts::contains_path(
            Path::new(&plan.project.root),
            Path::new(&worker.executable.path),
        ),
        "Codex executable must be outside project",
    )?;
    require(
        plan.policy.budget_usd_cents.is_none(),
        "subscription monetary budget cannot be measured",
    )?;
    require(
        [
            ToolPermission::ReadFiles,
            ToolPermission::WriteFiles,
            ToolPermission::RunCommands,
        ]
        .iter()
        .all(|p| plan.policy.tools.contains(p)),
        "worker permissions missing",
    )?;
    require(
        store
            .state()
            .tasks
            .get(task)
            .is_some_and(|t| t.state == TaskState::Pending),
        "task is not pending; never resend",
    )?;
    let definition = plan
        .workflow
        .tasks
        .iter()
        .find(|t| t.id == task)
        .ok_or("task missing")?;
    require(
        definition.dependencies.iter().all(|d| {
            store
                .state()
                .tasks
                .get(d)
                .is_some_and(|t| t.state == TaskState::Accepted)
        }),
        "dependencies not accepted",
    )?;
    require(
        !store.history().iter().any(
            |e| matches!(&e.body.event.command,WorkflowCommand::Assign{attempt:a,..} if a==attempt),
        ),
        "attempt ID already used",
    )?;
    crate::codex_workflow_access::require_budget(store)?;
    Ok(worker)
}
pub(crate) fn launch(
    root: &Path,
    mut store: WorkflowClient,
    job: CodexJob,
    recovery: bool,
    #[cfg(test)] preparation_gate: Option<PreparationGate>,
) -> Result<ActiveCodex, String> {
    let snapshot = crate::async_verification::snapshot(&store);
    let root = root.to_path_buf();
    let worker_job = job.clone();
    let observation =
        (!recovery).then(cerulean_adapters::codex_observation::ObservationCapture::default);
    let worker_observation = observation.clone();
    let (sender, receiver) = mpsc::channel();
    let (cancel, mut cancellation) = tokio::sync::oneshot::channel();
    let (advice_wake, mut advice_control) = tokio::sync::watch::channel(());
    let advice_gate = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0));
    let worker_gate = advice_gate.clone();
    let preparation_cancel = Arc::new(AtomicBool::new(false));
    let preparing = preparation_cancel.clone();
    std::thread::Builder::new()
        .name(format!("codex-{}", job.attempt_id))
        .spawn(move || {
            let mut cleaned = true;
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|_| "Codex worker runtime failed")?;
                    runtime.block_on(run(
                        &root,
                        &mut store,
                        &worker_job,
                        recovery,
                        &sender,
                        worker_observation.as_ref(),
                        crate::supervised_worker::WorkerControls {
                            cancellation: &mut cancellation,
                            advice: &mut advice_control,
                            gate: worker_gate,
                        },
                        preparing,
                        &mut cleaned,
                        #[cfg(test)]
                        preparation_gate,
                    ))
                }));
            let panicked = result.is_err();
            if let Some(capture) = &worker_observation {
                if !panicked {
                    capture.finish();
                }
            }
            let outcome = match result {
                Ok(r) => r,
                Err(_) => Err("Codex worker panicked; reconcile before retry".into()),
            };
            // The worker drops its capability before the sole owner is returned.
            let interrupted = store.requires_reopen() || panicked;
            drop(store);
            let _ = sender.send(Update::Done {
                interrupted,
                cleaned: cleaned && !panicked,
                outcome,
            });
        })
        .map_err(|e| format!("Codex worker launch failed: {e}"))?;
    Ok(ActiveCodex {
        preparation_cancel,
        advice_wake,
        advice_gate,
        observation,
        observation_saved: None,
        observation_error: None,
        snapshot,
        job,
        receiver,
        disconnected: false,
        cancel: if recovery { None } else { Some(cancel) },
    })
}
#[allow(clippy::too_many_arguments)]
async fn run(
    root: &Path,
    store: &mut impl CodexWorkflowAccess,
    job: &CodexJob,
    recovery: bool,
    sender: &mpsc::Sender<Update>,
    observation: Option<&cerulean_adapters::codex_observation::ObservationCapture>,
    controls: crate::supervised_worker::WorkerControls<'_>,
    preparing: Arc<AtomicBool>,
    cleaned: &mut bool,
    #[cfg(test)] preparation_gate: Option<PreparationGate>,
) -> Result<(), String> {
    let crate::supervised_worker::WorkerControls {
        cancellation,
        advice: advice_control,
        gate: advice_gate,
    } = controls;
    validate(job, store)?;
    let _executable_lock = lock_executable(&job.worker)?;
    let plan = store.revision().ok_or("revision missing")?.clone();
    require(
        plan.approval_digest()? == job.revision_digest,
        "Codex revision changed",
    )?;
    #[cfg(test)]
    if let Some(gate) = preparation_gate {
        gate.entered
            .send(())
            .map_err(|_| "preparation fixture unavailable")?;
        loop {
            if preparing.load(Ordering::Acquire) {
                return Err("Codex workspace preparation cancelled".into());
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
    let (workspace, workspace_proof) = if recovery {
        let attempt = store
            .attempt_record(&job.task_id, &job.attempt_id)
            .ok_or("Codex recovery attempt missing")?;
        plan.validate_attempt_workspace(attempt)?;
        // Reconciliation reads original remote evidence; never prepare or repair Git here.
        (
            attempt.binding.workspace.clone(),
            attempt.workspace_proof.clone(),
        )
    } else {
        preflight(store, &job.task_id, &job.attempt_id, &job.revision_digest)?;
        require(
            !preparing.load(Ordering::Acquire),
            "Codex preparation cancelled before launch",
        )?;
        if plan.worktree.is_some() {
            let (path, proof) =
                store.prepare_codex_workspace(&job.task_id, &job.attempt_id, preparing.clone())?;
            (path, Some(proof))
        } else {
            (plan.execution_root(&job.task_id, &job.attempt_id)?, None)
        }
    };
    if !recovery {
        require(
            !preparing.load(Ordering::Acquire),
            "Codex preparation cancelled before launch",
        )?;
    }
    if !recovery {
        crate::worker_boundaries::validate(&plan, true)?;
    }
    crate::blind_review_guard::require_isolated_execution(
        root,
        plan.task(&job.task_id)?.worker_directory.is_some(),
    )?;
    let launch = if recovery {
        CodexClient::launch_readonly
    } else {
        CodexClient::launch
    };
    let mut client = if plan.task(&job.task_id)?.worker_directory.is_some() {
        crate::windows_worker_account::codex(
            root,
            &plan,
            &job.task_id,
            Path::new(&job.worker.executable.path),
            Path::new(&workspace),
            recovery,
        )?
    } else {
        launch(
            Path::new(&job.worker.executable.path),
            Path::new(&workspace),
        )?
    };
    *cleaned = false;
    let outcome = async {
        if let Some(capture) = observation {
            client.capture_observations(capture.clone());
        }
        let connection = client.connect().await?;
        require(connection.account_type.is_some(), "Codex login required")?;
        if recovery {
            crate::codex_execution::reconcile_scoped(
                store,
                &mut client,
                &job.task_id,
                &job.attempt_id,
            )
            .await?;
        } else {
            require(
                !preparing.load(Ordering::Acquire),
                "Codex cancelled before thread creation",
            )?;
            require(
                connection
                    .models
                    .iter()
                    .any(|m| m.model == job.worker.model),
                "approved model unavailable",
            )?;
            let binding = client.create_worker(&job.worker.model).await?;
            require(
                !preparing.load(Ordering::Acquire),
                "Codex cancelled before assignment",
            )?;
            let fence = store
                .state()
                .tasks
                .get(&job.task_id)
                .ok_or("task missing")?
                .fence
                .checked_add(1)
                .ok_or("fence overflow")?;
            let attempt = Attempt {
                workspace_proof,
                id: job.attempt_id.clone(),
                task_id: job.task_id.clone(),
                worker_id: "worker-codex".into(),
                revision_digest: job.revision_digest.clone(),
                fence,
                binding: SessionBinding {
                    adapter_id: "codex".into(),
                    adapter_version: connection.server_version,
                    session_id: binding.thread_id.clone(),
                    identity_strength: IdentityStrength::ProviderId,
                    workspace: binding.workspace.clone(),
                },
            };
            if job.management_operation.is_some() {
                managed_assign(store, job, sender, attempt).await?;
            } else {
                store.command(
                    &format!("codex-{}-assign", job.attempt_id),
                    "deputy",
                    WorkflowCommand::Assign {
                        task: job.task_id.clone(),
                        worker: attempt.worker_id.clone(),
                        attempt: attempt.id.clone(),
                    },
                    CommandContext {
                        execution: Some(ExecutionEvidence::Attempt { attempt }),
                        ..Default::default()
                    },
                    false,
                )?;
            }
            let mut running = job.clone();
            require(
                !preparing.load(Ordering::Acquire),
                "Codex cancelled before dispatch",
            )?;
            running.state = CodexJobState::Executing;
            running.updated_at = chrono::Utc::now().to_rfc3339();
            save(root, &running)?;
            let _ = sender.send(Update::Progress(Box::new(
                crate::async_verification::snapshot(store),
            )));
            let mut supervised = crate::supervised_worker::SupervisedWorker {
                client: &mut client,
                root,
                job,
                sender,
                control: advice_control,
                gate: advice_gate,
            };
            crate::codex_execution::dispatch_inner(
                store,
                &mut supervised,
                &binding,
                &job.task_id,
                &job.attempt_id,
                Some(cancellation),
                |store| {
                    let _ = sender.send(Update::Progress(Box::new(
                        crate::async_verification::snapshot(store),
                    )));
                },
            )
            .await?;
        }
        Ok(())
    }
    .await;
    let cleanup = client.close().await;
    *cleaned = cleanup.is_ok();
    outcome.and(cleanup)
}

/// Two acknowledgements separate durable application intent from durable assignment
/// receipt. No turn/start is reachable until both have been confirmed by the host.
pub(crate) async fn managed_assign(
    store: &mut impl CodexWorkflowAccess,
    job: &CodexJob,
    sender: &mpsc::Sender<Update>,
    attempt: Attempt,
) -> Result<(), String> {
    let operation = job
        .management_operation
        .as_ref()
        .ok_or("management operation missing")?;
    crate::managed_assignment::assign(store, operation, attempt, |update| {
        let update = match update {
            crate::managed_assignment::Update::Prepare {
                operation,
                attempt,
                snapshot,
                reply,
            } => Update::PrepareAssignment {
                operation,
                attempt,
                snapshot,
                reply,
            },
            crate::managed_assignment::Update::Commit {
                operation,
                entry,
                snapshot,
                reply,
            } => Update::CommitAssignment {
                operation,
                entry,
                snapshot,
                reply,
            },
        };
        sender
            .send(update)
            .map_err(|_| "management host disconnected".into())
    })
    .await
}
