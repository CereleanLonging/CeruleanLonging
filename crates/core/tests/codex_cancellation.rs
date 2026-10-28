#![cfg(windows)]

use cerulean_adapters::codex::{
    Terminal, TurnEvidence, TurnObservation, WaitOutcome, WorkerBinding,
};
use cerulean_core::{
    codex_execution::{self, WorkerChannel},
    scheduler::{CommandContext, DurableWorkflow, Entry, SchedulerConfig},
};
use cerulean_domain::{execution::*, workflow::*};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::sync::oneshot;

fn config(plan: &PlanRevision) -> SchedulerConfig {
    SchedulerConfig {
        schema_version: 1,
        core_id: "core".into(),
        execution_scope: Some(ExecutionScope {
            project: plan.project.clone(),
            policy_version: 1,
        }),
        principals: [
            ("user", Role::User),
            ("chief", Role::Chief),
            ("deputy", Role::Deputy),
            ("core", Role::Core),
            ("worker-codex", Role::Worker),
        ]
        .into_iter()
        .map(|(id, role)| Principal {
            id: id.into(),
            role,
            provider: matches!(role, Role::Chief | Role::Deputy).then(|| "openai".into()),
        })
        .collect(),
    }
}

fn ready(root: &Path) -> (DurableWorkflow, PlanRevision, WorkerBinding) {
    let project = root.join("project");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("result.txt"), "independently observed result").unwrap();
    let plan = PlanRevision {
        management_setup_digest: None,
        worktree: None,
        schema_version: 1,
        project: Project {
            id: "project".into(),
            root: project.to_string_lossy().into(),
            timezone: "Asia/Shanghai".into(),
        },
        workflow: WorkflowPlan {
            id: "plan".into(),
            version: 1,
            tasks: vec![WorkflowTask {
                id: "task".into(),
                goal: "Produce a result".into(),
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
            max_requests: 2,
            max_seconds: 60,
            currency_budget: None,
            budget_usd_cents: None,
        },
        snapshot_policy: None,
        execution: vec![TaskExecution {
            supervision_override: None,
            profile_id: None,
            desktop_worker: None,
            api_worker: None,
            task_id: "task".into(),
            adapter_id: "codex".into(),
            worker: None,
            artifact_paths: vec!["result.txt".into()],
            checks: vec![AcceptanceCheck {
                name: None,
                id: "check".into(),
                program: "fixture-validator".into(),
                args: vec![],
                timeout_seconds: 10,
                tool: None,
            }],
        }],
    };
    let mut store = DurableWorkflow::open(&root.join("ledger"), config(&plan)).unwrap();
    store
        .command(
            "proposal",
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
            false,
        )
        .unwrap();
    let approval = Approval {
        plan_id: "plan".into(),
        revision: 1,
        policy_version: 1,
        revision_digest: plan.approval_digest().unwrap(),
        user_id: "user".into(),
        approved_at: chrono::Utc::now().to_rfc3339(),
    };
    store
        .command(
            "approval",
            "user",
            WorkflowCommand::Approve {
                digest: store.state().digest.clone().unwrap(),
            },
            CommandContext {
                approval_source: Some("fixture local user".into()),
                execution: Some(ExecutionEvidence::Approval { approval }),
                ..Default::default()
            },
            false,
        )
        .unwrap();
    let attempt = Attempt {
        workspace_proof: None,
        id: "attempt".into(),
        task_id: "task".into(),
        worker_id: "worker-codex".into(),
        revision_digest: plan.approval_digest().unwrap(),
        fence: 1,
        binding: SessionBinding {
            adapter_id: "codex".into(),
            adapter_version: "fixture".into(),
            session_id: "session".into(),
            identity_strength: IdentityStrength::ProviderId,
            workspace: plan.project.root.clone(),
        },
    };
    store
        .command(
            "assignment",
            "deputy",
            WorkflowCommand::Assign {
                task: "task".into(),
                worker: "worker-codex".into(),
                attempt: "attempt".into(),
            },
            CommandContext {
                execution: Some(ExecutionEvidence::Attempt { attempt }),
                ..Default::default()
            },
            false,
        )
        .unwrap();
    let binding = WorkerBinding {
        thread_id: "session".into(),
        model: "fixture".into(),
        provider: "openai".into(),
        workspace: plan.project.root.clone(),
    };
    (store, plan, binding)
}

struct Channel {
    ledger: PathBuf,
    status: String,
    terminal_first: bool,
    lose_interrupt_ack: bool,
    fault: &'static str,
    turn: String,
    starts: usize,
    interrupts: usize,
    reads: usize,
    invalidated: bool,
    known_read_turn: Option<String>,
    empty: bool,
}
impl Channel {
    fn new(root: &Path, status: &str) -> Self {
        Self {
            ledger: root.join("ledger"),
            status: status.into(),
            terminal_first: false,
            lose_interrupt_ack: false,
            fault: "",
            turn: "turn".into(),
            starts: 0,
            interrupts: 0,
            reads: 0,
            invalidated: false,
            known_read_turn: None,
            empty: false,
        }
    }
    fn terminal(&self, thread: &str, turn: &str) -> Terminal {
        Terminal {
            thread_id: thread.into(),
            turn_id: if self.fault == "terminal-turn" {
                "wrong-turn".into()
            } else {
                turn.into()
            },
            status: self.status.clone(),
        }
    }
    fn events(&self) -> Vec<Entry> {
        fs::read_dir(self.ledger.join("events"))
            .unwrap()
            .map(|e| serde_json::from_slice(&fs::read(e.unwrap().path()).unwrap()).unwrap())
            .collect()
    }
}
impl WorkerChannel for Channel {
    async fn read_evidence(
        &mut self,
        thread: &str,
        attempt: &str,
        prompt: &str,
        turn: Option<&str>,
        allow_empty: bool,
    ) -> Result<TurnEvidence, String> {
        if !self.empty {
            return self
                .read(thread, attempt, prompt, turn)
                .await
                .map(TurnEvidence::RecordedInput);
        }
        self.reads += 1;
        self.known_read_turn = turn.map(str::to_owned);
        if !allow_empty {
            return Err("no original anchors; empty turn is not proof".into());
        }
        Ok(TurnEvidence::EmptyInterrupted(InterruptedTurnObservation {
            thread_id: thread.into(),
            turn_id: self.turn.clone(),
            status: self.status.clone(),
            workspace: self
                .ledger
                .parent()
                .unwrap()
                .join("project")
                .to_string_lossy()
                .into(),
            items_view: "full".into(),
            item_count: 0,
            matching_turns: 1,
        }))
    }
    fn invalidate(&mut self) {
        self.invalidated = true;
    }
    async fn start(&mut self, binding: &WorkerBinding, _: &str, _: &str) -> Result<String, String> {
        assert!(self
            .events()
            .iter()
            .any(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. })));
        assert_eq!(binding.thread_id, "session");
        self.starts += 1;
        Ok(self.turn.clone())
    }
    async fn wait(&mut self, thread: &str, turn: &str, _: Duration) -> Result<Terminal, String> {
        Ok(self.terminal(thread, turn))
    }
    async fn wait_or_cancel(
        &mut self,
        thread: &str,
        turn: &str,
        _: Duration,
        cancel: &mut oneshot::Receiver<String>,
    ) -> Result<WaitOutcome, String> {
        assert!(self
            .events()
            .iter()
            .any(|e| matches!(e.body.event.command, WorkflowCommand::Started { .. })));
        if self.fault == "publication" {
            fs::write(
                self.ledger
                    .join("events/000000000006_codex-attempt-cancel.json"),
                b"existing publication conflict",
            )
            .unwrap();
        }
        if self.terminal_first {
            Ok(WaitOutcome::Terminal(self.terminal(thread, turn)))
        } else {
            Ok(WaitOutcome::CancelRequested(
                cancel.await.map_err(|_| "test control disconnected")?,
            ))
        }
    }
    async fn interrupt(&mut self, thread: &str, turn: &str) -> Result<Terminal, String> {
        let events = self.events();
        let intents: Vec<_> = events
            .iter()
            .filter(|e| matches!(e.body.event.command, WorkflowCommand::CancelIntent { .. }))
            .collect();
        assert_eq!(
            intents.len(),
            1,
            "one durable intent must precede interrupt"
        );
        let intent = intents[0];
        assert_eq!(intent.body.event.principal, "core");
        assert_eq!(intent.body.context.thread_id.as_deref(), Some(thread));
        assert_eq!(intent.body.context.turn_id.as_deref(), Some(turn));
        assert!(
            intent.body.context.cwd.is_some() && intent.body.context.cancellation_source.is_some()
        );
        self.interrupts += 1;
        if self.lose_interrupt_ack {
            Err("interrupt ACK lost; delivery unknown".into())
        } else {
            Ok(self.terminal(thread, turn))
        }
    }
    async fn read(
        &mut self,
        thread: &str,
        attempt: &str,
        prompt: &str,
        known: Option<&str>,
    ) -> Result<TurnObservation, String> {
        assert!(
            !self.invalidated,
            "readback must use a fresh channel after transport failure"
        );
        self.reads += 1;
        self.known_read_turn = known.map(str::to_owned);
        if self.fault == "read-error" {
            return Err("partial readback, no terminal proof".into());
        }
        Ok(TurnObservation {
            thread_id: thread.into(),
            turn_id: self.turn.clone(),
            client_message_id: if self.fault == "read-attempt" {
                "wrong-attempt".into()
            } else {
                attempt.into()
            },
            status: if self.fault == "read-status" {
                "inProgress".into()
            } else {
                self.status.clone()
            },
            prompt_sha256: if self.fault == "read-prompt" {
                "a".repeat(64)
            } else {
                format!("{:x}", Sha256::digest(prompt.as_bytes()))
            },
            item_types: vec!["userMessage".into(), "agentMessage".into()],
        })
    }
}

fn cancellation() -> oneshot::Receiver<String> {
    let (sender, receiver) = oneshot::channel();
    sender
        .send("Explicit user cancellation; preserve original attempt".into())
        .unwrap();
    receiver
}

#[tokio::test]
async fn empty_started_cancellation_has_separate_durable_proof_and_replays() {
    let root = tempfile::tempdir().unwrap();
    let (mut store, plan, binding) = ready(root.path());
    let mut channel = Channel::new(root.path(), "interrupted");
    channel.empty = true;
    let before = fs::read(root.path().join("project/result.txt")).unwrap();
    let observed = codex_execution::dispatch_controlled(
        &mut store,
        &mut channel,
        &binding,
        "task",
        "attempt",
        &mut cancellation(),
        |_| {},
    )
    .await
    .unwrap();
    assert!(matches!(observed, TurnEvidence::EmptyInterrupted(_)));
    assert_eq!(
        (channel.starts, channel.interrupts, channel.reads),
        (1, 1, 1)
    );
    assert_eq!(store.state().tasks["task"].state, TaskState::Cancelled);
    assert!(!store.state().tasks["task"].verified);
    assert!(store.completed_artifacts("task", "attempt").is_none());
    let history = store.history().to_vec();
    let Some(ExecutionEvidence::StartedTurnCancellation { receipt, proof }) =
        &history.last().unwrap().body.context.execution
    else {
        panic!("separate proof required")
    };
    assert_eq!(receipt.status, ReceiptStatus::Cancelled);
    assert_eq!(receipt.evidence_sha256, digest(proof).unwrap());
    assert_eq!(proof.anchors.started.sequence, 5);
    assert_eq!(proof.anchors.cancellation.sequence, 6);
    let raw = serde_json::to_string(proof).unwrap();
    assert!(!raw.contains("promptSha256") && !raw.contains("clientMessageId"));
    drop(store);
    let restored = DurableWorkflow::open(&root.path().join("ledger"), config(&plan)).unwrap();
    assert_eq!(restored.history(), history);
    assert_eq!(restored.state().tasks["task"].state, TaskState::Cancelled);
    assert_eq!(
        fs::read(root.path().join("project/result.txt")).unwrap(),
        before
    );
    drop(restored);
    // Even after recomputing all public digests, an altered historical anchor must fail replay.
    let mut changed = history.last().unwrap().clone();
    if let Some(ExecutionEvidence::StartedTurnCancellation { receipt, proof }) =
        &mut changed.body.context.execution
    {
        proof.anchors.started.hash = "f".repeat(64);
        receipt.evidence_sha256 = digest(proof).unwrap();
        if let WorkflowCommand::Reconcile { evidence, .. } = &mut changed.body.event.command {
            *evidence = vec![receipt.evidence_sha256.clone()];
        }
    }
    changed.hash = digest(&changed.body).unwrap();
    let path = root.path().join("ledger/events").join(format!(
        "{:010}_{}.json",
        changed.body.event.sequence, changed.body.operation_id
    ));
    fs::write(path, serde_json::to_vec(&changed).unwrap()).unwrap();
    assert!(DurableWorkflow::open(&root.path().join("ledger"), config(&plan)).is_err());
}

#[tokio::test]
async fn empty_readback_without_actual_started_and_local_cancel_never_closes_attempt() {
    for scenario in 0..5 {
        let root = tempfile::tempdir().unwrap();
        let (mut store, _, binding) = ready(root.path());
        store
            .command(
                "dispatch",
                "core",
                WorkflowCommand::DispatchIntent {
                    task: "task".into(),
                    attempt: "attempt".into(),
                },
                CommandContext {
                    thread_id: Some("session".into()),
                    cwd: Some(binding.workspace.clone()),
                    ..Default::default()
                },
                false,
            )
            .unwrap();
        let attempt = store.attempt_record("task", "attempt").unwrap().clone();
        let receipt = ExecutionReceipt {
            schema_version: 1,
            task_id: "task".into(),
            attempt_id: "attempt".into(),
            fence: 1,
            binding: attempt.binding,
            turn_id: Some("turn".into()),
            observed_at: chrono::Utc::now().to_rfc3339(),
            status: ReceiptStatus::Running,
            evidence_sha256: "a".repeat(64),
        };
        if scenario <= 1 {
            store
                .command(
                    "lost-ack",
                    "core",
                    WorkflowCommand::RecoveryUnknown {
                        task: "task".into(),
                        attempt: "attempt".into(),
                    },
                    CommandContext::default(),
                    false,
                )
                .unwrap();
        }
        if scenario > 0 {
            let cmd = if scenario == 1 {
                WorkflowCommand::Reconcile {
                    task: "task".into(),
                    attempt: "attempt".into(),
                    outcome: ReconcileOutcome::Running,
                    evidence: vec!["read-only proof".into()],
                }
            } else {
                WorkflowCommand::Started {
                    task: "task".into(),
                    attempt: "attempt".into(),
                }
            };
            store
                .command(
                    "running",
                    "core",
                    cmd,
                    CommandContext {
                        thread_id: Some("session".into()),
                        turn_id: Some("turn".into()),
                        execution: Some(ExecutionEvidence::Receipt {
                            receipt,
                            manifest: None,
                        }),
                        ..Default::default()
                    },
                    false,
                )
                .unwrap();
        }
        if scenario != 2 {
            store
                .command(
                    "cancel",
                    "core",
                    WorkflowCommand::CancelIntent {
                        task: "task".into(),
                        attempt: "attempt".into(),
                        reason: "stop".into(),
                    },
                    CommandContext {
                        thread_id: Some("session".into()),
                        turn_id: Some("turn".into()),
                        cwd: Some(if scenario == 4 {
                            "wrong".into()
                        } else {
                            binding.workspace.clone()
                        }),
                        cancellation_source: Some(if scenario == 3 {
                            "untrusted source".into()
                        } else {
                            "authenticated local-user cancellation: user".into()
                        }),
                        ..Default::default()
                    },
                    false,
                )
                .unwrap();
        }
        store
            .command(
                "unknown",
                "core",
                WorkflowCommand::RecoveryUnknown {
                    task: "task".into(),
                    attempt: "attempt".into(),
                },
                CommandContext::default(),
                false,
            )
            .unwrap();
        assert!(store
            .started_cancellation_anchors("task", "attempt")
            .is_err());
        let count = store.history().len();
        let mut reader = Channel::new(root.path(), "interrupted");
        reader.empty = true;
        assert!(
            codex_execution::reconcile(&mut store, &mut reader, "task", "attempt")
                .await
                .is_err()
        );
        assert_eq!(store.history().len(), count);
        assert_eq!(store.state().tasks["task"].state, TaskState::Unknown);
        assert_eq!((reader.starts, reader.interrupts, reader.reads), (0, 0, 1));
    }
}

#[tokio::test]
async fn empty_cancel_after_lost_ack_recovers_readonly_with_original_anchors() {
    let root = tempfile::tempdir().unwrap();
    let (mut store, plan, binding) = ready(root.path());
    let mut channel = Channel::new(root.path(), "interrupted");
    channel.lose_interrupt_ack = true;
    assert!(codex_execution::dispatch_controlled(
        &mut store,
        &mut channel,
        &binding,
        "task",
        "attempt",
        &mut cancellation(),
        |_| {}
    )
    .await
    .is_err());
    let prefix = store.history().to_vec();
    drop(store);
    let mut store = DurableWorkflow::open(&root.path().join("ledger"), config(&plan)).unwrap();
    let mut reader = Channel::new(root.path(), "interrupted");
    reader.empty = true;
    codex_execution::reconcile(&mut store, &mut reader, "task", "attempt")
        .await
        .unwrap();
    assert_eq!((reader.starts, reader.interrupts, reader.reads), (0, 0, 1));
    assert_eq!(&store.history()[..prefix.len()], prefix);
    assert_eq!(store.history().len(), prefix.len() + 1);
    assert_eq!(store.state().tasks["task"].state, TaskState::Cancelled);
    assert!(
        codex_execution::reconcile(&mut store, &mut reader, "task", "attempt")
            .await
            .is_err()
    );
    assert_eq!(reader.reads, 1);
}

#[tokio::test]
async fn early_proof_rejects_tampered_anchors_observations_and_completion_commands() {
    let root = tempfile::tempdir().unwrap();
    let (mut store, _, binding) = ready(root.path());
    let mut channel = Channel::new(root.path(), "interrupted");
    channel.lose_interrupt_ack = true;
    assert!(codex_execution::dispatch_controlled(
        &mut store,
        &mut channel,
        &binding,
        "task",
        "attempt",
        &mut cancellation(),
        |_| {}
    )
    .await
    .is_err());
    let attempt = store.attempt_record("task", "attempt").unwrap().clone();
    let proof = StartedTurnCancellationProof {
        schema_version: 1,
        anchors: store
            .started_cancellation_anchors("task", "attempt")
            .unwrap(),
        observation: InterruptedTurnObservation {
            thread_id: "session".into(),
            turn_id: "turn".into(),
            workspace: binding.workspace.clone(),
            status: "interrupted".into(),
            items_view: "full".into(),
            item_count: 0,
            matching_turns: 1,
        },
        observed_at: chrono::Utc::now().to_rfc3339(),
    };
    let make = |proof: StartedTurnCancellationProof| {
        let hash = digest(&proof).unwrap();
        (
            WorkflowCommand::Reconcile {
                task: "task".into(),
                attempt: "attempt".into(),
                outcome: ReconcileOutcome::Cancelled,
                evidence: vec![hash.clone()],
            },
            CommandContext {
                thread_id: Some("session".into()),
                turn_id: Some("turn".into()),
                cwd: Some(binding.workspace.clone()),
                terminal_status: Some("interrupted".into()),
                execution: Some(ExecutionEvidence::StartedTurnCancellation {
                    receipt: ExecutionReceipt {
                        schema_version: 1,
                        task_id: "task".into(),
                        attempt_id: "attempt".into(),
                        fence: 1,
                        binding: attempt.binding.clone(),
                        turn_id: Some("turn".into()),
                        observed_at: proof.observed_at.clone(),
                        status: ReceiptStatus::Cancelled,
                        evidence_sha256: hash,
                    },
                    proof: Box::new(proof),
                }),
                ..Default::default()
            },
        )
    };
    let len = store.history().len();
    for case in 0..14 {
        let mut p = proof.clone();
        match case {
            0 => p.schema_version = 2,
            1 => p.anchors.started.hash = "a".repeat(64),
            2 => p.anchors.cancellation.sequence = 1,
            3 => p.observation.thread_id = "other".into(),
            4 => p.observation.turn_id = "other".into(),
            5 => p.observation.workspace = "other".into(),
            6 => p.observation.status = "completed".into(),
            7 => p.observation.items_view = "summary".into(),
            8 => p.observation.item_count = 1,
            9 => p.observation.matching_turns = 2,
            10 => p.observed_at = "2000-01-01T00:00:00Z".into(),
            _ => {}
        }
        let (mut cmd, mut ctx) = make(p);
        if case == 11 {
            if let Some(ExecutionEvidence::StartedTurnCancellation { receipt, .. }) =
                &mut ctx.execution
            {
                receipt.evidence_sha256 = "b".repeat(64);
            }
        }
        if case == 12 {
            cmd = WorkflowCommand::Reconcile {
                task: "task".into(),
                attempt: "attempt".into(),
                outcome: ReconcileOutcome::Submitted,
                evidence: vec!["x".into()],
            };
        }
        if case == 13 {
            cmd = WorkflowCommand::Started {
                task: "task".into(),
                attempt: "attempt".into(),
            };
        }
        assert!(
            store
                .command(&format!("bad-{case}"), "core", cmd, ctx, false)
                .is_err(),
            "case {case}"
        );
        assert_eq!(store.history().len(), len);
    }
    let (cmd, ctx) = make(proof);
    store.command("correct", "core", cmd, ctx, false).unwrap();
}
fn cancel_count(store: &DurableWorkflow) -> usize {
    store
        .history()
        .iter()
        .filter(|e| matches!(e.body.event.command, WorkflowCommand::CancelIntent { .. }))
        .count()
}

#[tokio::test]
async fn cancel_persists_before_one_interrupt_and_bound_terminal_survives_reopen() {
    let root = tempfile::tempdir().unwrap();
    let (mut store, plan, binding) = ready(root.path());
    let mut channel = Channel::new(root.path(), "interrupted");
    let mut states = vec![];
    codex_execution::dispatch_controlled(
        &mut store,
        &mut channel,
        &binding,
        "task",
        "attempt",
        &mut cancellation(),
        |s| states.push(s.state().tasks["task"].state),
    )
    .await
    .unwrap();
    assert_eq!(
        states,
        [
            TaskState::Dispatching,
            TaskState::Running,
            TaskState::Cancelling,
            TaskState::Unknown,
            TaskState::Cancelled
        ]
    );
    assert_eq!(
        (channel.starts, channel.interrupts, channel.reads),
        (1, 1, 1)
    );
    assert_eq!(cancel_count(&store), 1);
    assert!(store.completed_artifacts("task", "attempt").is_none());
    let terminal = store.history().last().unwrap();
    let Some(ExecutionEvidence::Receipt { receipt, manifest }) = &terminal.body.context.execution
    else {
        panic!("bound terminal receipt required")
    };
    assert_eq!(receipt.status, ReceiptStatus::Cancelled);
    assert_eq!(receipt.binding.session_id, "session");
    assert_eq!(receipt.turn_id.as_deref(), Some("turn"));
    assert_eq!(receipt.fence, 1);
    assert!(manifest.is_none());
    assert!(codex_execution::dispatch_controlled(
        &mut store,
        &mut channel,
        &binding,
        "task",
        "attempt",
        &mut cancellation(),
        |_| {}
    )
    .await
    .is_err());
    assert_eq!((channel.starts, channel.interrupts), (1, 1));
    drop(store);
    let store = DurableWorkflow::open(&root.path().join("ledger"), config(&plan)).unwrap();
    assert_eq!(store.state().tasks["task"].state, TaskState::Cancelled);
    assert_eq!(
        store.state().tasks["task"].cancellation_requested,
        Some(true)
    );
}

#[tokio::test]
async fn lost_interrupt_ack_reopens_unknown_and_only_reads_the_original_turn() {
    for status in ["interrupted", "completed"] {
        let root = tempfile::tempdir().unwrap();
        let (mut store, plan, binding) = ready(root.path());
        let mut channel = Channel::new(root.path(), status);
        channel.lose_interrupt_ack = true;
        assert!(codex_execution::dispatch_controlled(
            &mut store,
            &mut channel,
            &binding,
            "task",
            "attempt",
            &mut cancellation(),
            |_| {}
        )
        .await
        .is_err());
        assert_eq!(store.state().tasks["task"].state, TaskState::Unknown);
        assert_eq!(cancel_count(&store), 1);
        assert!(channel.invalidated);
        assert_eq!(
            (channel.starts, channel.interrupts, channel.reads),
            (1, 1, 0)
        );
        drop(store);
        let mut store = DurableWorkflow::open(&root.path().join("ledger"), config(&plan)).unwrap();
        let mut readback = Channel::new(root.path(), status);
        assert!(codex_execution::dispatch_controlled(
            &mut store,
            &mut readback,
            &binding,
            "task",
            "attempt",
            &mut cancellation(),
            |_| {}
        )
        .await
        .is_err());
        codex_execution::reconcile(&mut store, &mut readback, "task", "attempt")
            .await
            .unwrap();
        assert_eq!(
            (readback.starts, readback.interrupts, readback.reads),
            (0, 0, 1)
        );
        assert_eq!(readback.known_read_turn.as_deref(), Some("turn"));
        assert_eq!(
            store.state().tasks["task"].state,
            if status == "completed" {
                TaskState::Submitted
            } else {
                TaskState::Cancelled
            }
        );
        assert_eq!(
            store.completed_artifacts("task", "attempt").is_some(),
            status == "completed"
        );
        assert_eq!(cancel_count(&store), 1);
    }
}

#[tokio::test]
async fn natural_completion_wins_before_or_after_cancel_without_false_cancelled_state() {
    for terminal_first in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let (mut store, _, binding) = ready(root.path());
        let mut channel = Channel::new(root.path(), "completed");
        channel.terminal_first = terminal_first;
        codex_execution::dispatch_controlled(
            &mut store,
            &mut channel,
            &binding,
            "task",
            "attempt",
            &mut cancellation(),
            |_| {},
        )
        .await
        .unwrap();
        assert_eq!(store.state().tasks["task"].state, TaskState::Submitted);
        assert!(!store.state().tasks["task"].verified);
        assert!(store.completed_artifacts("task", "attempt").is_some());
        assert_eq!(cancel_count(&store), usize::from(!terminal_first));
        assert_eq!(channel.interrupts, usize::from(!terminal_first));
        assert_eq!(channel.starts, 1);
    }
}

#[tokio::test]
async fn wrong_or_incomplete_terminal_evidence_keeps_cancellation_unknown() {
    for fault in [
        "terminal-turn",
        "read-attempt",
        "read-prompt",
        "read-status",
        "read-error",
    ] {
        let root = tempfile::tempdir().unwrap();
        let (mut store, _, binding) = ready(root.path());
        let mut channel = Channel::new(root.path(), "interrupted");
        channel.fault = fault;
        assert!(codex_execution::dispatch_controlled(
            &mut store,
            &mut channel,
            &binding,
            "task",
            "attempt",
            &mut cancellation(),
            |_| {}
        )
        .await
        .is_err());
        assert_eq!(
            store.state().tasks["task"].state,
            TaskState::Unknown,
            "{fault}"
        );
        assert_eq!(
            store.state().tasks["task"].cancellation_requested,
            Some(true)
        );
        assert_eq!(channel.interrupts, 1);
        assert!(store.completed_artifacts("task", "attempt").is_none());
    }
}

#[tokio::test]
async fn cancellation_publication_failure_never_calls_interrupt() {
    let root = tempfile::tempdir().unwrap();
    let (mut store, _, binding) = ready(root.path());
    let mut channel = Channel::new(root.path(), "interrupted");
    channel.fault = "publication";
    let blocked = root
        .path()
        .join("ledger/events/000000000006_codex-attempt-cancel.json");
    let result = codex_execution::dispatch_controlled(
        &mut store,
        &mut channel,
        &binding,
        "task",
        "attempt",
        &mut cancellation(),
        |_| {},
    )
    .await;
    assert!(result.is_err());
    assert_eq!(
        (channel.starts, channel.interrupts, channel.reads),
        (1, 0, 0)
    );
    assert_eq!(cancel_count(&store), 0);
    assert_eq!(fs::read(blocked).unwrap(), b"existing publication conflict");
}

#[tokio::test]
async fn readback_of_running_or_failed_turn_does_not_claim_cancellation_or_resend() {
    let root = tempfile::tempdir().unwrap();
    let (mut store, _, binding) = ready(root.path());
    let mut channel = Channel::new(root.path(), "interrupted");
    channel.lose_interrupt_ack = true;
    assert!(codex_execution::dispatch_controlled(
        &mut store,
        &mut channel,
        &binding,
        "task",
        "attempt",
        &mut cancellation(),
        |_| {}
    )
    .await
    .is_err());
    let count = store.history().len();
    for status in ["inProgress", "failed"] {
        let mut readback = Channel::new(root.path(), status);
        assert!(
            codex_execution::reconcile(&mut store, &mut readback, "task", "attempt")
                .await
                .is_err()
        );
        assert_eq!(store.state().tasks["task"].state, TaskState::Unknown);
        assert_eq!(store.history().len(), count);
        assert_eq!(
            (readback.starts, readback.interrupts, readback.reads),
            (0, 0, 1)
        );
    }
}

#[tokio::test]
async fn external_interrupt_and_reconciled_running_turn_keep_original_binding() {
    let root = tempfile::tempdir().unwrap();
    let (mut store, plan, binding) = ready(root.path());
    store
        .command(
            "lost-ack-dispatch",
            "core",
            WorkflowCommand::DispatchIntent {
                task: "task".into(),
                attempt: "attempt".into(),
            },
            CommandContext {
                thread_id: Some("session".into()),
                cwd: Some(binding.workspace),
                ..Default::default()
            },
            false,
        )
        .unwrap();
    drop(store);
    let mut store = DurableWorkflow::open(&root.path().join("ledger"), config(&plan)).unwrap();
    let attempt = store.attempt_record("task", "attempt").unwrap().clone();
    let running = ExecutionReceipt {
        schema_version: 1,
        task_id: "task".into(),
        attempt_id: "attempt".into(),
        fence: 1,
        binding: attempt.binding,
        turn_id: Some("readback-turn".into()),
        observed_at: chrono::Utc::now().to_rfc3339(),
        status: ReceiptStatus::Running,
        evidence_sha256: "a".repeat(64),
    };
    store
        .command(
            "running-readback",
            "core",
            WorkflowCommand::Reconcile {
                task: "task".into(),
                attempt: "attempt".into(),
                outcome: ReconcileOutcome::Running,
                evidence: vec!["bound fixture readback".into()],
            },
            CommandContext {
                thread_id: Some("session".into()),
                turn_id: Some("readback-turn".into()),
                execution: Some(ExecutionEvidence::Receipt {
                    receipt: running,
                    manifest: None,
                }),
                ..Default::default()
            },
            false,
        )
        .unwrap();
    drop(store);
    let mut store = DurableWorkflow::open(&root.path().join("ledger"), config(&plan)).unwrap();
    let mut readback = Channel::new(root.path(), "interrupted");
    readback.turn = "readback-turn".into();
    codex_execution::reconcile(&mut store, &mut readback, "task", "attempt")
        .await
        .unwrap();
    assert_eq!(readback.known_read_turn.as_deref(), Some("readback-turn"));
    assert_eq!(
        (readback.starts, readback.interrupts, readback.reads),
        (0, 0, 1)
    );
    assert_eq!(cancel_count(&store), 0);
    assert_eq!(store.state().tasks["task"].state, TaskState::Cancelled);
}
