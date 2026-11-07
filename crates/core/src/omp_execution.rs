//! Trusted workflow bridge for a pre-connected, exclusively owned OMP channel.
//! Process pinning, credentials and tool installation remain the host's job;
//! this function is not exposed as an untrusted IPC command.
use crate::{
    artifacts::ArtifactObserver,
    omp_journal::{CompletionEvidence, OmpJournal},
    omp_workflow_access::OmpWorkflowAccess,
    scheduler::{CommandContext, DurableWorkflow},
};
use cerulean_adapters::{
    omp_protocol::PromptStatus,
    omp_transport::worker::{DispatchIntent, PromptOutcome, WorkerEvent, WorkerTransport},
};
use cerulean_domain::{
    execution::*,
    workflow::{ReconcileOutcome, Role, TaskState, WorkflowCommand},
};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio::{
    io::{AsyncBufRead, AsyncWrite},
    sync::oneshot,
};

fn role(store: &impl OmpWorkflowAccess, role: Role) -> Result<String, String> {
    store
        .configuration()
        .principals
        .iter()
        .find(|p| p.role == role)
        .map(|p| p.id.clone())
        .ok_or("OMP workflow role missing".into())
}
fn record(
    store: &mut impl OmpWorkflowAccess,
    who: &str,
    command: WorkflowCommand,
    context: CommandContext,
) -> Result<(), String> {
    store.record_omp(who, command, context)
}
pub(crate) fn scope(
    store: &impl OmpWorkflowAccess,
    task: &str,
    id: &str,
) -> Result<(PlanRevision, Attempt, DispatchIntent, String), String> {
    let plan = store.revision().ok_or("OMP plan missing")?.clone();
    store.approval().ok_or("OMP approval missing")?.validate(
        &plan,
        &role(store, Role::User)?,
        store
            .configuration()
            .execution_scope
            .as_ref()
            .ok_or("OMP execution scope missing")?
            .policy_version,
    )?;
    let attempt = store
        .attempt_record(task, id)
        .ok_or("OMP attempt missing")?
        .clone();
    plan.validate_attempt_workspace(&attempt)?;
    require(
        attempt.binding.adapter_id == "omp"
            && plan.task(task)?.adapter_id == "omp"
            && attempt.revision_digest == plan.approval_digest()?,
        "OMP attempt outside approved scope",
    )?;
    let worker = plan
        .task(task)?
        .worker
        .as_ref()
        .ok_or("OMP approved worker selection missing")?;
    worker.validate()?;
    let prompt = approved_prompt(&plan, task)?;
    let prompt = crate::task_rework::append_prompt(prompt, store.history(), task, id)?;
    let intent = DispatchIntent {
        version: 1,
        task_id: task.into(),
        attempt_id: id.into(),
        revision_digest: plan.approval_digest()?,
        fence: attempt.fence,
        binding: attempt.binding.clone(),
        provider: worker.provider.clone(),
        model: worker.model.clone(),
        prompt_sha256: format!("{:x}", Sha256::digest(prompt.as_bytes())),
    };
    intent.validate_metadata()?;
    Ok((plan, attempt, intent, prompt))
}
pub(crate) fn approved_prompt(plan: &PlanRevision, task: &str) -> Result<String, String> {
    let goal = &plan
        .workflow
        .tasks
        .iter()
        .find(|t| t.id == task)
        .ok_or("OMP task missing")?
        .goal;
    let prompt=format!("Execute this approved task only within the supplied workspace.\nTask: {task}\nGoal: {goal}\nRequired artifacts: {}\nDo not access credentials or other projects. Use only tools granted by the host. Independent verification determines success.",plan.task(task)?.artifact_paths.join(", "));
    Ok(prompt)
}
fn unknown(store: &mut impl OmpWorkflowAccess, task: &str, id: &str) -> Result<(), String> {
    if matches!(
        store.state().tasks.get(task).map(|t| &t.state),
        Some(TaskState::Dispatching | TaskState::Running | TaskState::Cancelling)
    ) {
        record(
            store,
            &role(store, Role::Core)?,
            WorkflowCommand::RecoveryUnknown {
                task: task.into(),
                attempt: id.into(),
            },
            CommandContext::default(),
        )?;
    }
    Ok(())
}
fn receipt(attempt: &Attempt, status: ReceiptStatus, head: &str) -> ExecutionReceipt {
    ExecutionReceipt {
        schema_version: 1,
        task_id: attempt.task_id.clone(),
        attempt_id: attempt.id.clone(),
        fence: attempt.fence,
        binding: attempt.binding.clone(),
        turn_id: None,
        observed_at: chrono::Utc::now().to_rfc3339(),
        status,
        evidence_sha256: head.into(),
    }
}
fn context(receipt: ExecutionReceipt, manifest: Option<ArtifactManifest>) -> CommandContext {
    CommandContext {
        thread_id: Some(receipt.binding.session_id.clone()),
        turn_id: None,
        terminal_status: if receipt.status == ReceiptStatus::Completed {
            Some("completed".into())
        } else {
            None
        },
        execution: Some(ExecutionEvidence::Receipt { receipt, manifest }),
        ..Default::default()
    }
}
fn apply_terminal(
    store: &mut impl OmpWorkflowAccess,
    plan: &PlanRevision,
    attempt: &Attempt,
    journal: &OmpJournal,
) -> Result<(), String> {
    let task = &attempt.task_id;
    let id = &attempt.id;
    let terminal = journal
        .observation()
        .terminal
        .as_ref()
        .ok_or("OMP durable terminal missing")?;
    let current = &store.state().tasks[task].state;
    if matches!(
        current,
        TaskState::Submitted | TaskState::Accepted | TaskState::Cancelled
    ) {
        return Err("OMP terminal already applied or conflicts with workflow".into());
    }
    let (_, _, intent, _) = scope(store, task, id)?;
    let actual_tools = tool_head(store, &intent)?;
    match terminal.status {
        PromptStatus::Completed => {
            let CompletionEvidence {
                receipt: completed,
                manifest,
                tool_head: sealed_tools,
            } = journal
                .completion()
                .ok_or("OMP original artifact seal missing; cannot recapture during recovery")?
                .clone();
            require(
                actual_tools == sealed_tools,
                "OMP tool evidence differs from original completion seal",
            )?;
            ArtifactObserver::new(
                plan,
                store.approval().ok_or("OMP approval missing")?,
                &role(store, Role::User)?,
                plan.policy.version,
                attempt,
                &completed,
                None,
            )?
            .require_unchanged(&manifest)?;
            let evidence = vec![digest(&manifest)?];
            if *current == TaskState::Running {
                record(store,&attempt.worker_id,WorkflowCommand::Submit{task:task.clone(),attempt:id.clone(),summary:"OMP returned a correlated completed result; independent checks still required".into(),evidence:Some(evidence)},context(completed,Some(manifest)))?;
            } else {
                unknown(store, task, id)?;
                record(
                    store,
                    &role(store, Role::Core)?,
                    WorkflowCommand::Reconcile {
                        task: task.clone(),
                        attempt: id.clone(),
                        outcome: ReconcileOutcome::Submitted,
                        evidence,
                    },
                    context(completed, Some(manifest)),
                )?;
            }
        }
        PromptStatus::Aborted => {
            unknown(store, task, id)?;
            record(
                store,
                &role(store, Role::Core)?,
                WorkflowCommand::Reconcile {
                    task: task.clone(),
                    attempt: id.clone(),
                    outcome: ReconcileOutcome::Cancelled,
                    evidence: vec![journal.head().into()],
                },
                context(
                    receipt(attempt, ReceiptStatus::Cancelled, journal.head()),
                    None,
                ),
            )?;
        }
        PromptStatus::Error => {
            unknown(store, task, id)?;
            return Err("OMP returned an error; task not submitted".into());
        }
    }
    Ok(())
}

#[cfg(test)]
fn seal_artifacts(
    store: &impl OmpWorkflowAccess,
    plan: &PlanRevision,
    attempt: &Attempt,
    journal: &mut OmpJournal,
) -> Result<(), String> {
    seal_artifacts_observed(store, plan, attempt, journal, &mut |_| Ok(()))
}
fn seal_artifacts_observed(
    store: &impl OmpWorkflowAccess,
    plan: &PlanRevision,
    attempt: &Attempt,
    journal: &mut OmpJournal,
    observe: &mut impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    let completed = receipt(attempt, ReceiptStatus::Completed, journal.head());
    let manifest = ArtifactObserver::new(
        plan,
        store.approval().ok_or("OMP approval missing")?,
        &role(store, Role::User)?,
        plan.policy.version,
        attempt,
        &completed,
        None,
    )?
    .capture()?;
    journal.seal_completion_observed(
        CompletionEvidence {
            receipt: completed,
            manifest,
            tool_head: tool_head(store, &scope(store, &attempt.task_id, &attempt.id)?.2)?,
        },
        observe,
    )
}

/// A closed local result is necessary for submission; it does not claim remote ACK.
fn tool_head(
    store: &impl OmpWorkflowAccess,
    intent: &DispatchIntent,
) -> Result<Option<String>, String> {
    let path = store.omp_file_service_path(&intent.attempt_id)?;
    match std::fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("OMP tool evidence inaccessible".into()),
        Ok(_) => {}
    }
    crate::host::ordinary(&path, true)?;
    let tools = crate::omp_tool_journal::OmpToolJournal::open(&path.join("events"), intent)?;
    require(
        tools.observation().unresolved() == 0,
        "OMP tool calls still unresolved",
    )?;
    Ok(Some(tools.head().into()))
}

/// No model send is possible unless both the workflow intent and journal intent
/// commit. Errors preserve evidence and move any dispatched workflow to Unknown.
pub async fn dispatch<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin>(
    store: &mut DurableWorkflow,
    channel: &mut WorkerTransport<R, W>,
    task: &str,
    id: &str,
    cancel: &mut oneshot::Receiver<()>,
) -> Result<PromptOutcome, String> {
    dispatch_inner(store, channel, task, id, cancel, false, &mut |_, _| Ok(())).await
}

/// Install only approved host file tools on this fresh, pre-connected worker.
#[cfg(windows)]
pub async fn dispatch_with_files<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin>(
    store: &mut DurableWorkflow,
    channel: &mut WorkerTransport<R, W>,
    task: &str,
    id: &str,
    cancel: &mut oneshot::Receiver<()>,
) -> Result<PromptOutcome, String> {
    dispatch_inner(store, channel, task, id, cancel, true, &mut |_, _| Ok(())).await
}

#[cfg(windows)]
pub(crate) async fn dispatch_files_progress<
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
    S: OmpWorkflowAccess,
>(
    store: &mut S,
    channel: &mut WorkerTransport<R, W>,
    task: &str,
    id: &str,
    cancel: &mut oneshot::Receiver<()>,
    observer: crate::omp_tool_checkpoint::Observer,
    mut progress: impl FnMut(&S),
) -> Result<PromptOutcome, String> {
    dispatch_inner_observed(
        store,
        channel,
        task,
        id,
        cancel,
        true,
        Some(observer),
        &mut |store, _| {
            progress(store);
            Ok(())
        },
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_inner<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin, S: OmpWorkflowAccess>(
    store: &mut S,
    channel: &mut WorkerTransport<R, W>,
    task: &str,
    id: &str,
    cancel: &mut oneshot::Receiver<()>,
    with_files: bool,
    observe: &mut impl FnMut(&S, &str) -> Result<(), String>,
) -> Result<PromptOutcome, String> {
    dispatch_inner_observed(store, channel, task, id, cancel, with_files, None, observe).await
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_inner_observed<
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
    S: OmpWorkflowAccess,
>(
    store: &mut S,
    channel: &mut WorkerTransport<R, W>,
    task: &str,
    id: &str,
    cancel: &mut oneshot::Receiver<()>,
    with_files: bool,
    observer: Option<crate::omp_tool_checkpoint::Observer>,
    observe: &mut impl FnMut(&S, &str) -> Result<(), String>,
) -> Result<PromptOutcome, String> {
    #[cfg(not(windows))]
    let _ = observer;
    store.refresh()?;
    let (plan, attempt, intent, prompt) = scope(store, task, id)?;
    require(
        store.state().tasks[task].state == TaskState::Assigned
            && store.state().tasks[task].attempt.as_deref() == Some(id),
        "OMP task is not freshly assigned",
    )?;
    require(!store.history().iter().any(|e|matches!(&e.body.event.command,WorkflowCommand::DispatchIntent{attempt,..} if attempt==id)),"OMP attempt already has dispatch history; never resend")?;
    crate::omp_workflow_access::require_budget(store)?;
    require(
        plan.policy.budget_usd_cents.is_none(),
        "OMP metered money enforcement is not yet available",
    )?;
    #[cfg(windows)]
    crate::git_workspaces::require_prepared(&plan, &attempt)?;
    let mut journal = OmpJournal::open(&store.omp_attempt_path(id)?, &intent)?;
    require(
        !journal.observation().dispatched,
        "OMP journal already contains a send intent; never resend",
    )?;
    #[cfg(not(windows))]
    require(!with_files, "OMP file tools require Windows")?;
    #[cfg(windows)]
    let mut files = if with_files {
        let mut service = crate::omp_file_tools::OmpFileTools::create(
            &plan,
            store.approval().ok_or("OMP approval missing")?,
            &role(store, Role::User)?,
            plan.policy.version,
            &intent,
            &store.omp_file_service_path(id)?,
        )?;
        if let Some(observer) = observer {
            service.observe_completed_tools(observer);
        }
        channel
            .register_host_tools(&intent.validate_metadata()?.0, &service.definitions())
            .await?;
        Some(service)
    } else {
        None
    };
    let core = role(store, Role::Core)?;
    let persist = |event: &WorkerEvent| {
        if matches!(event, WorkerEvent::DispatchIntent { .. }) {
            record(
                store,
                &core,
                WorkflowCommand::DispatchIntent {
                    task: task.into(),
                    attempt: id.into(),
                },
                CommandContext {
                    thread_id: Some(attempt.binding.session_id.clone()),
                    cwd: Some(attempt.binding.workspace.clone()),
                    ..Default::default()
                },
            )?;
        }
        journal.record(event)?;
        match event {
            WorkerEvent::PromptAcknowledged { .. }
                if store.state().tasks[task].state == TaskState::Dispatching =>
            {
                record(
                    store,
                    &core,
                    WorkflowCommand::Started {
                        task: task.into(),
                        attempt: id.into(),
                    },
                    context(
                        receipt(&attempt, ReceiptStatus::Running, journal.head()),
                        None,
                    ),
                )?;
            }
            WorkerEvent::AbortIntent { .. } => {
                if store.state().tasks[task].state == TaskState::Dispatching {
                    unknown(store, task, id)?;
                }
                record(
                    store,
                    &core,
                    WorkflowCommand::CancelIntent {
                        task: task.into(),
                        attempt: id.into(),
                        reason: "User requested cancellation of this OMP attempt".into(),
                    },
                    CommandContext {
                        thread_id: Some(attempt.binding.session_id.clone()),
                        cancellation_source: Some("user".into()),
                        ..Default::default()
                    },
                )?;
            }
            WorkerEvent::NotStarted { .. } => unknown(store, task, id)?,
            WorkerEvent::Terminal { .. } => observe(store, "terminal-recorded")?,
            _ => {}
        }
        observe(store, "progress")?;
        Ok(())
    };
    let timeout = Duration::from_secs(plan.policy.max_seconds as u64);
    #[cfg(windows)]
    let result = if let Some(service) = files.as_mut() {
        channel
            .execute_with_tools(intent.clone(), &prompt, timeout, cancel, persist, service)
            .await
    } else {
        channel
            .execute(intent.clone(), &prompt, timeout, cancel, persist)
            .await
    };
    #[cfg(not(windows))]
    let result = channel
        .execute(intent.clone(), &prompt, timeout, cancel, persist)
        .await;
    // All handler futures have finished or been dropped. Release the tool writer
    // before reopening its durable chain for the independent completion seal.
    #[cfg(windows)]
    drop(files);
    let result = result.and_then(|outcome| {
        if let PromptOutcome::Finished(terminal) = &outcome {
            let tools = tool_head(store, &intent)?;
            require(
                !with_files || tools.is_some(),
                "OMP file evidence disappeared",
            )?;
            if terminal.status == PromptStatus::Completed {
                seal_artifacts_observed(store, &plan, &attempt, &mut journal, &mut |at| {
                    observe(store, at)
                })?;
                observe(store, "artifacts-sealed")?;
            }
            apply_terminal(store, &plan, &attempt, &journal)?;
            observe(store, "workflow-applied")?;
        }
        Ok(outcome)
    });
    if result.is_err() {
        unknown(store, task, id)?;
    }
    result
}

/// Local durable terminal recovery only. Does not contact OMP, synthesize ACKs,
/// replay a prompt, or infer completion from a quiet process or an old ACK.
pub fn recover_terminal(store: &mut DurableWorkflow, task: &str, id: &str) -> Result<(), String> {
    let (plan, attempt, intent, _) = scope(store, task, id)?;
    require(
        store.state().tasks[task].state == TaskState::Unknown
            && store.state().tasks[task].attempt.as_deref() == Some(id),
        "OMP local recovery requires original unknown attempt",
    )?;
    require(store.history().iter().any(|e|matches!(&e.body.event.command,WorkflowCommand::DispatchIntent{task:t,attempt:a} if t==task&&a==id)),"OMP workflow dispatch intent missing")?;
    let journal = OmpJournal::open(&store.omp_attempt_path(id)?, &intent)?;
    apply_terminal(store, &plan, &attempt, &journal)
}

/// Explicit trusted-host resolution of the narrow intent-before-wire cancellation
/// window. This does not refund budget or make the same attempt dispatchable.
pub fn resolve_unsent_cancellation(
    store: &mut DurableWorkflow,
    task: &str,
    id: &str,
) -> Result<(), String> {
    let (_, attempt, intent, _) = scope(store, task, id)?;
    require(
        store.state().tasks[task].state == TaskState::Unknown
            && store.state().tasks[task].attempt.as_deref() == Some(id),
        "OMP no-send resolution requires original unknown attempt",
    )?;
    require(store.history().iter().any(|e| matches!(&e.body.event.command, WorkflowCommand::DispatchIntent { task:t, attempt:a } if t==task && a==id)), "OMP workflow dispatch intent missing")?;
    let journal = OmpJournal::open(&store.omp_attempt_path(id)?, &intent)?;
    require(
        journal.observation().not_started,
        "OMP explicit durable no-send observation missing",
    )?;
    record(
        store,
        &role(store, Role::Core)?,
        WorkflowCommand::Reconcile {
            task: task.into(),
            attempt: id.into(),
            outcome: ReconcileOutcome::Cancelled,
            evidence: vec![journal.head().into()],
        },
        context(
            receipt(&attempt, ReceiptStatus::Cancelled, journal.head()),
            None,
        ),
    )
}

#[cfg(test)]
pub(crate) mod tests;
