//! Trusted dispatcher: ledger intent precedes the only model send.
use crate::{
    artifacts::ArtifactObserver,
    codex_workflow_access::CodexWorkflowAccess,
    scheduler::{CommandContext, DurableWorkflow},
};
use cerulean_adapters::codex::{
    CodexClient, Terminal, TurnEvidence, TurnObservation, WaitOutcome, WorkerBinding,
};
use cerulean_domain::{execution::*, workflow::*};
use std::time::Duration;

fn require_supported_terminal(terminal: &Terminal, thread: &str, turn: &str) -> Result<(), String> {
    require(
        terminal.thread_id == thread && terminal.turn_id == turn,
        "Codex terminal thread or turn differs from the original dispatch; retain unknown",
    )?;
    match terminal.status.as_str() {
        "completed" | "interrupted" => Ok(()),
        "failed" => Err("Codex reported a failed turn; original attempt and artifacts retained, no automatic retry".into()),
        // Never quote an untrusted status or provider error body in product logs.
        _ => Err("Codex terminal status is unsupported; retain original unknown attempt".into()),
    }
}

/// Opaque access to a scoped native worker. The resident host retains the journal.
pub struct WorkerWorkflow<'a> {
    pub(crate) store: &'a mut dyn CodexWorkflowAccess,
}

// Static dispatch permits deterministic fault tests without exposing a wire endpoint.
#[allow(async_fn_in_trait)]
pub trait WorkerChannel {
    fn activate_observations(&mut self, _thread: &str, _turn: &str) -> Result<(), String> {
        Ok(())
    }
    fn invalidate(&mut self);
    async fn start(
        &mut self,
        binding: &WorkerBinding,
        attempt: &str,
        prompt: &str,
    ) -> Result<String, String>;
    async fn wait(
        &mut self,
        thread: &str,
        turn: &str,
        timeout: Duration,
    ) -> Result<Terminal, String>;
    async fn wait_or_cancel(
        &mut self,
        _thread: &str,
        _turn: &str,
        _timeout: Duration,
        _cancel: &mut tokio::sync::oneshot::Receiver<String>,
    ) -> Result<WaitOutcome, String> {
        Err("controlled Codex waiting is unsupported by this channel".into())
    }
    async fn interrupt(&mut self, _thread: &str, _turn: &str) -> Result<Terminal, String> {
        Err("Codex interruption is unsupported by this channel".into())
    }
    async fn wait_supervised(
        &mut self,
        _store: &mut DurableWorkflow,
        thread: &str,
        turn: &str,
        timeout: Duration,
        cancel: &mut tokio::sync::oneshot::Receiver<String>,
    ) -> Result<WaitOutcome, String> {
        self.wait_or_cancel(thread, turn, timeout, cancel).await
    }
    async fn wait_scoped(
        &mut self,
        _store: &mut WorkerWorkflow<'_>,
        thread: &str,
        turn: &str,
        timeout: Duration,
        cancel: &mut tokio::sync::oneshot::Receiver<String>,
    ) -> Result<WaitOutcome, String> {
        self.wait_or_cancel(thread, turn, timeout, cancel).await
    }
    async fn read(
        &mut self,
        thread: &str,
        attempt: &str,
        prompt: &str,
        turn: Option<&str>,
    ) -> Result<TurnObservation, String>;
    async fn read_evidence(
        &mut self,
        thread: &str,
        attempt: &str,
        prompt: &str,
        turn: Option<&str>,
        _allow_empty: bool,
    ) -> Result<TurnEvidence, String> {
        self.read(thread, attempt, prompt, turn)
            .await
            .map(TurnEvidence::RecordedInput)
    }
}
impl WorkerChannel for CodexClient {
    fn activate_observations(&mut self, thread: &str, turn: &str) -> Result<(), String> {
        CodexClient::activate_observations(self, thread, turn)
    }
    async fn read_evidence(
        &mut self,
        t: &str,
        a: &str,
        p: &str,
        u: Option<&str>,
        allow: bool,
    ) -> Result<TurnEvidence, String> {
        CodexClient::read_evidence(self, t, a, p, u, allow).await
    }
    fn invalidate(&mut self) {
        CodexClient::invalidate(self);
    }
    async fn start(&mut self, b: &WorkerBinding, a: &str, p: &str) -> Result<String, String> {
        self.start_turn(b, a, p).await
    }
    async fn wait(&mut self, t: &str, u: &str, d: Duration) -> Result<Terminal, String> {
        self.wait_turn(t, u, d).await
    }
    async fn wait_or_cancel(
        &mut self,
        t: &str,
        u: &str,
        d: Duration,
        cancel: &mut tokio::sync::oneshot::Receiver<String>,
    ) -> Result<WaitOutcome, String> {
        self.wait_turn_or_cancel(t, u, d, cancel).await
    }
    async fn interrupt(&mut self, t: &str, u: &str) -> Result<Terminal, String> {
        CodexClient::interrupt(self, t, u).await
    }
    async fn read(
        &mut self,
        t: &str,
        a: &str,
        p: &str,
        u: Option<&str>,
    ) -> Result<TurnObservation, String> {
        self.read_attempt(t, a, p, u).await
    }
}
fn role(store: &(impl CodexWorkflowAccess + ?Sized), expected: Role) -> Result<String, String> {
    store
        .configuration()
        .principals
        .iter()
        .find(|p| p.role == expected)
        .map(|p| p.id.clone())
        .ok_or("role missing".into())
}
fn record(
    store: &mut (impl CodexWorkflowAccess + ?Sized),
    id: &str,
    who: &str,
    command: WorkflowCommand,
    context: CommandContext,
) -> Result<(), String> {
    store.command(id, who, command, context, false).map(|_| ())
}
pub fn approved_prompt(plan: &PlanRevision, task: &str) -> Result<String, String> {
    let spec = plan.task(task)?;
    let goal = &plan
        .workflow
        .tasks
        .iter()
        .find(|t| t.id == task)
        .ok_or("task missing")?
        .goal;
    Ok(format!("Execute this approved programming task within the supplied workspace only.\nTask: {task}\nGoal: {goal}\nRequired artifacts: {}\nDo not alter other projects, read credentials, use network, install packages, or spawn agents. Do not modify external validation tools. Report limitations; independent verification determines success.",spec.artifact_paths.join(", ")))
}
fn attempt_scope(
    store: &(impl CodexWorkflowAccess + ?Sized),
    task: &str,
    id: &str,
) -> Result<(PlanRevision, Attempt, String), String> {
    let plan = store.revision().ok_or("revision missing")?.clone();
    let user = role(store, Role::User)?;
    store.approval().ok_or("approval missing")?.validate(
        &plan,
        &user,
        store
            .configuration()
            .execution_scope
            .as_ref()
            .ok_or("scope missing")?
            .policy_version,
    )?;
    let attempt = store
        .attempt_record(task, id)
        .ok_or("attempt missing")?
        .clone();
    plan.validate_attempt_workspace(&attempt)?;
    require(
        attempt.binding.adapter_id == "codex"
            && plan.task(task)?.adapter_id == "codex"
            && attempt.revision_digest == plan.approval_digest()?,
        "Codex attempt scope mismatch",
    )?;
    Ok((plan, attempt, user))
}
fn receipt(
    attempt: &Attempt,
    turn: &str,
    status: ReceiptStatus,
    proof: &impl serde::Serialize,
) -> Result<ExecutionReceipt, String> {
    Ok(ExecutionReceipt {
        schema_version: 1,
        task_id: attempt.task_id.clone(),
        attempt_id: attempt.id.clone(),
        fence: attempt.fence,
        binding: attempt.binding.clone(),
        turn_id: Some(turn.into()),
        observed_at: chrono::Utc::now().to_rfc3339(),
        status,
        evidence_sha256: digest(proof)?,
    })
}
fn context(receipt: ExecutionReceipt, manifest: Option<ArtifactManifest>) -> CommandContext {
    CommandContext {
        thread_id: Some(receipt.binding.session_id.clone()),
        turn_id: receipt.turn_id.clone(),
        terminal_status: if receipt.status == ReceiptStatus::Completed {
            Some("completed".into())
        } else {
            None
        },
        execution: Some(ExecutionEvidence::Receipt { receipt, manifest }),
        ..Default::default()
    }
}
fn unknown(
    store: &mut (impl CodexWorkflowAccess + ?Sized),
    task: &str,
    id: &str,
) -> Result<(), String> {
    if matches!(
        store.state().tasks.get(task).map(|t| &t.state),
        Some(TaskState::Dispatching | TaskState::Running | TaskState::Cancelling)
    ) {
        let identity = digest(&(task, id, store.history().len()))?;
        record(
            store,
            &format!("codex-unknown-{}", &identity[..32]),
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
fn known_turn(store: &(impl CodexWorkflowAccess + ?Sized), task: &str, id: &str) -> Option<String> {
    store
        .history()
        .iter()
        .rev()
        .find_map(|entry| match &entry.body.event.command {
            WorkflowCommand::Started {
                task: t,
                attempt: a,
            }
            | WorkflowCommand::CancelIntent {
                task: t,
                attempt: a,
                ..
            }
            | WorkflowCommand::Reconcile {
                task: t,
                attempt: a,
                outcome: ReconcileOutcome::Running,
                ..
            } if t == task && a == id => entry.body.context.turn_id.clone(),
            _ => None,
        })
}
fn check_observation(
    observation: &TurnObservation,
    attempt: &Attempt,
    prompt: &str,
    turn: Option<&str>,
) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    require(
        observation.thread_id == attempt.binding.session_id
            && observation.client_message_id == attempt.id
            && turn.is_none_or(|id| id == observation.turn_id)
            && observation.prompt_sha256 == format!("{:x}", Sha256::digest(prompt.as_bytes())),
        "Codex observation binding mismatch",
    )
}
fn submit(
    store: &mut (impl CodexWorkflowAccess + ?Sized),
    plan: &PlanRevision,
    attempt: &Attempt,
    user: &str,
    proof: &TurnObservation,
    reconcile: bool,
) -> Result<(), String> {
    require(proof.status == "completed", "Codex turn not completed")?;
    let receipt = receipt(attempt, &proof.turn_id, ReceiptStatus::Completed, proof)?;
    let manifest = ArtifactObserver::new(
        plan,
        store.approval().unwrap(),
        user,
        plan.policy.version,
        attempt,
        &receipt,
        Some(&proof.turn_id),
    )?
    .capture()?;
    let command = if reconcile {
        WorkflowCommand::Reconcile {
            task: attempt.task_id.clone(),
            attempt: attempt.id.clone(),
            outcome: ReconcileOutcome::Submitted,
            evidence: vec![digest(proof)?],
        }
    } else {
        WorkflowCommand::Submit {
            task: attempt.task_id.clone(),
            attempt: attempt.id.clone(),
            summary: "Codex completed; independently captured artifacts await verification".into(),
            evidence: Some(vec![digest(proof)?]),
        }
    };
    let actor = if reconcile {
        role(store, Role::Core)?
    } else {
        attempt.worker_id.clone()
    };
    record(
        store,
        &format!(
            "codex-{}-{}",
            attempt.id,
            if reconcile { "reconciled" } else { "submitted" }
        ),
        &actor,
        command,
        context(receipt, Some(manifest)),
    )
}

fn reconcile_terminal(
    store: &mut (impl CodexWorkflowAccess + ?Sized),
    plan: &PlanRevision,
    attempt: &Attempt,
    user: &str,
    proof: &TurnObservation,
) -> Result<(), String> {
    match proof.status.as_str() {
        "completed" => submit(store, plan, attempt, user, proof, true),
        "interrupted" => {
            let bound = receipt(attempt, &proof.turn_id, ReceiptStatus::Cancelled, proof)?;
            record(
                store,
                &format!("codex-{}-cancelled", attempt.id),
                &role(store, Role::Core)?,
                WorkflowCommand::Reconcile {
                    task: attempt.task_id.clone(),
                    attempt: attempt.id.clone(),
                    outcome: ReconcileOutcome::Cancelled,
                    evidence: vec![digest(proof)?],
                },
                context(bound, None),
            )
        }
        _ => Err("Codex readback has no supported terminal outcome; retain unknown".into()),
    }
}

pub async fn dispatch<C: WorkerChannel>(
    store: &mut DurableWorkflow,
    channel: &mut C,
    binding: &WorkerBinding,
    task: &str,
    id: &str,
) -> Result<TurnObservation, String> {
    match dispatch_inner(store, channel, binding, task, id, None, |_| {}).await? {
        TurnEvidence::RecordedInput(proof) => Ok(proof),
        TurnEvidence::EmptyInterrupted(_) => {
            Err("unexpected empty proof without controlled cancellation".into())
        }
    }
}

pub async fn dispatch_controlled<C: WorkerChannel>(
    store: &mut DurableWorkflow,
    channel: &mut C,
    binding: &WorkerBinding,
    task: &str,
    id: &str,
    cancel: &mut tokio::sync::oneshot::Receiver<String>,
    progress: impl FnMut(&DurableWorkflow),
) -> Result<TurnEvidence, String> {
    dispatch_inner(store, channel, binding, task, id, Some(cancel), progress).await
}

pub(crate) async fn dispatch_inner<C: WorkerChannel, S: CodexWorkflowAccess>(
    store: &mut S,
    channel: &mut C,
    binding: &WorkerBinding,
    task: &str,
    id: &str,
    cancel: Option<&mut tokio::sync::oneshot::Receiver<String>>,
    mut progress: impl FnMut(&S),
) -> Result<TurnEvidence, String> {
    let (plan, attempt, user) = attempt_scope(store, task, id)?;
    crate::git_workspaces::require_prepared(&plan, &attempt)?;
    if let Some(worker) = &plan.task(task)?.worker {
        require(
            worker.model == binding.model && worker.provider == binding.provider,
            "Codex model differs from approval",
        )?;
    }
    let run = store.state().tasks.get(task).ok_or("task missing")?;
    require(
        run.state == TaskState::Assigned && run.attempt.as_deref() == Some(id),
        "Codex task not assigned; never resend",
    )?;
    require(
        binding.thread_id == attempt.binding.session_id
            && binding.workspace == attempt.binding.workspace
            && binding.provider == "openai",
        "Codex worker binding mismatch",
    )?;
    require(
        [
            ToolPermission::ReadFiles,
            ToolPermission::WriteFiles,
            ToolPermission::RunCommands,
        ]
        .iter()
        .all(|permission| plan.policy.tools.contains(permission)),
        "Codex worker needs approved file and command permissions",
    )?;
    require(
        plan.policy.budget_usd_cents.is_none(),
        "Codex subscription cost is not measurable; monetary budget requires metered route",
    )?;
    crate::codex_workflow_access::require_budget(store)?;
    let prompt = approved_prompt(&plan, task)?;
    require(prompt.len() <= 48 * 1024, "approved task prompt too large")?;
    let core = role(store, Role::Core)?;
    record(
        store,
        &format!("codex-{id}-intent"),
        &core,
        WorkflowCommand::DispatchIntent {
            task: task.into(),
            attempt: id.into(),
        },
        CommandContext {
            thread_id: Some(binding.thread_id.clone()),
            cwd: Some(binding.workspace.clone()),
            ..Default::default()
        },
    )?;
    progress(store);
    let result = tokio::time::timeout(Duration::from_secs(plan.policy.max_seconds as u64), async {
        let turn = channel.start(binding, id, &prompt).await?;
        let running = receipt(
            &attempt,
            &turn,
            ReceiptStatus::Running,
            &serde_json::json!({"threadId":binding.thread_id,"turnId":turn,"attempt":id}),
        )?;
        record(
            store,
            &format!("codex-{id}-started"),
            &core,
            WorkflowCommand::Started {
                task: task.into(),
                attempt: id.into(),
            },
            context(running, None),
        )?;
        progress(store);
        let limit = Duration::from_secs(plan.policy.max_seconds as u64);
        channel.activate_observations(&binding.thread_id, &turn)?;
        let waited = if let Some(cancel) = cancel {
            if let Some(direct) = store.direct() {
                channel
                    .wait_supervised(direct, &binding.thread_id, &turn, limit, cancel)
                    .await?
            } else {
                channel
                    .wait_scoped(
                        &mut WorkerWorkflow { store },
                        &binding.thread_id,
                        &turn,
                        limit,
                        cancel,
                    )
                    .await?
            }
        } else {
            WaitOutcome::Terminal(channel.wait(&binding.thread_id, &turn, limit).await?)
        };
        let terminal = match waited {
            WaitOutcome::Terminal(terminal) => terminal,
            WaitOutcome::CancelRequested(reason) => {
                require(
                    !reason.trim().is_empty() && reason.len() <= 8000 && !reason.contains('\0'),
                    "invalid user cancellation reason",
                )?;
                record(
                    store,
                    &format!("codex-{id}-cancel"),
                    &core,
                    WorkflowCommand::CancelIntent {
                        task: task.into(),
                        attempt: id.into(),
                        reason,
                    },
                    CommandContext {
                        thread_id: Some(binding.thread_id.clone()),
                        turn_id: Some(turn.clone()),
                        cwd: Some(binding.workspace.clone()),
                        cancellation_source: Some(format!(
                            "authenticated local-user cancellation: {user}"
                        )),
                        ..Default::default()
                    },
                )?;
                progress(store);
                // The sole owner sends at most once, and only after the intent is durable.
                channel.interrupt(&binding.thread_id, &turn).await?
            }
        };
        require_supported_terminal(&terminal, &binding.thread_id, &turn)?;
        let anchors = store.started_cancellation_anchors(task, id).ok();
        let proof = channel
            .read_evidence(
                &binding.thread_id,
                id,
                &prompt,
                Some(&turn),
                anchors.is_some(),
            )
            .await?;
        require(
            proof.status() == terminal.status,
            "Codex terminal and readback status conflict",
        )?;
        match &proof {
            TurnEvidence::RecordedInput(observation) => {
                check_observation(observation, &attempt, &prompt, Some(&turn))?;
                if store.state().tasks[task].state == TaskState::Cancelling
                    || observation.status == "interrupted"
                {
                    unknown(store, task, id)?;
                    progress(store);
                    reconcile_terminal(store, &plan, &attempt, &user, observation)?;
                } else {
                    submit(store, &plan, &attempt, &user, observation, false)?;
                }
            }
            TurnEvidence::EmptyInterrupted(observation) => {
                let anchors = anchors.ok_or("original cancellation anchors required")?;
                unknown(store, task, id)?;
                progress(store);
                record_empty_cancellation(store, &attempt, anchors, observation)?;
            }
        }
        progress(store);
        Ok(proof)
    })
    .await
    .unwrap_or_else(|_| Err("Codex execution deadline exceeded; reconcile before retry".into()));
    if result.is_err() {
        channel.invalidate();
        unknown(store, task, id)?;
        progress(store);
    }
    result
}

/// Read-only remote reconciliation; absence never authorizes a second send.
pub async fn reconcile<C: WorkerChannel>(
    store: &mut DurableWorkflow,
    channel: &mut C,
    task: &str,
    id: &str,
) -> Result<TurnEvidence, String> {
    reconcile_scoped(store, channel, task, id).await
}
pub(crate) async fn reconcile_scoped<C: WorkerChannel>(
    store: &mut (impl CodexWorkflowAccess + ?Sized),
    channel: &mut C,
    task: &str,
    id: &str,
) -> Result<TurnEvidence, String> {
    let (plan, attempt, user) = attempt_scope(store, task, id)?;
    require(
        store
            .state()
            .tasks
            .get(task)
            .is_some_and(|t| t.state == TaskState::Unknown && t.attempt.as_deref() == Some(id)),
        "Codex attempt not awaiting reconciliation",
    )?;
    let known = known_turn(store, task, id);
    let anchors = store.started_cancellation_anchors(task, id).ok();
    let prompt = approved_prompt(&plan, task)?;
    let proof = channel
        .read_evidence(
            &attempt.binding.session_id,
            id,
            &prompt,
            known.as_deref(),
            anchors.is_some(),
        )
        .await?;
    match &proof {
        TurnEvidence::RecordedInput(observation) => {
            check_observation(observation, &attempt, &prompt, known.as_deref())?;
            reconcile_terminal(store, &plan, &attempt, &user, observation)?;
        }
        TurnEvidence::EmptyInterrupted(observation) => {
            record_empty_cancellation(
                store,
                &attempt,
                anchors.ok_or("original cancellation anchors required")?,
                observation,
            )?;
        }
    }
    Ok(proof)
}

fn record_empty_cancellation(
    store: &mut (impl CodexWorkflowAccess + ?Sized),
    attempt: &Attempt,
    anchors: StartedCancellationAnchors,
    observation: &InterruptedTurnObservation,
) -> Result<(), String> {
    let proof = StartedTurnCancellationProof {
        schema_version: 1,
        anchors,
        observation: observation.clone(),
        observed_at: chrono::Utc::now().to_rfc3339(),
    };
    let mut bound = receipt(
        attempt,
        &observation.turn_id,
        ReceiptStatus::Cancelled,
        &proof,
    )?;
    bound.observed_at = proof.observed_at.clone();
    record(
        store,
        &format!("codex-{}-cancelled", attempt.id),
        &role(store, Role::Core)?,
        WorkflowCommand::Reconcile {
            task: attempt.task_id.clone(),
            attempt: attempt.id.clone(),
            outcome: ReconcileOutcome::Cancelled,
            evidence: vec![digest(&proof)?],
        },
        CommandContext {
            thread_id: Some(observation.thread_id.clone()),
            turn_id: Some(observation.turn_id.clone()),
            cwd: Some(observation.workspace.clone()),
            terminal_status: Some("interrupted".into()),
            execution: Some(ExecutionEvidence::StartedTurnCancellation {
                receipt: bound,
                proof: Box::new(proof),
            }),
            ..Default::default()
        },
    )
}

#[cfg(test)]
mod terminal_diagnostics_tests {
    use super::*;
    #[test]
    fn preserved_shutdown_terminal_failure_diagnostic_keeps_identity_and_untrusted_text_private() {
        let mut terminal = Terminal {
            thread_id: "original-thread".into(),
            turn_id: "original-turn".into(),
            status: "failed".into(),
        };
        let error =
            require_supported_terminal(&terminal, "original-thread", "original-turn").unwrap_err();
        assert!(error.contains("failed turn") && error.contains("no automatic retry"));
        assert!(
            require_supported_terminal(&terminal, "other", "original-turn")
                .unwrap_err()
                .contains("differs")
        );
        terminal.status = "private-provider-body-sentinel".into();
        let error =
            require_supported_terminal(&terminal, "original-thread", "original-turn").unwrap_err();
        assert!(error.contains("unsupported") && !error.contains("sentinel"));
        for status in ["completed", "interrupted"] {
            terminal.status = status.into();
            assert!(
                require_supported_terminal(&terminal, "original-thread", "original-turn").is_ok()
            );
        }
    }
}
