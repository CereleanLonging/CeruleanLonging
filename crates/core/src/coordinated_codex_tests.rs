use super::*;
use crate::{
    codex_execution::{WorkerChannel, WorkerWorkflow},
    codex_workflow_access::CodexWorkflowAccess,
};
use cerulean_adapters::codex::{
    Terminal, TurnEvidence, TurnObservation, WaitOutcome, WorkerBinding,
};
use std::{sync::mpsc, time::Duration};

struct Channel {
    mode: &'static str,
    workspace: PathBuf,
    entered: Option<mpsc::Sender<()>>,
    release: Option<tokio::sync::oneshot::Receiver<()>>,
    starts: u32,
    interrupts: u32,
    invalidated: bool,
}
impl Channel {
    fn terminal(&self, t: &str, u: &str) -> Terminal {
        Terminal {
            thread_id: t.into(),
            turn_id: u.into(),
            status: if self.mode.starts_with("cancel") {
                "interrupted"
            } else {
                "completed"
            }
            .into(),
        }
    }
}
impl WorkerChannel for Channel {
    fn invalidate(&mut self) {
        self.invalidated = true;
    }
    async fn start(&mut self, _: &WorkerBinding, _: &str, _: &str) -> Result<String, String> {
        self.starts += 1;
        if self.mode != "missing" {
            fs::write(
                self.workspace.join("sum.mjs"),
                "export const sum=(a,b)=>a+b;",
            )
            .unwrap();
        }
        Ok("native-turn".into())
    }
    async fn wait(&mut self, t: &str, u: &str, _: Duration) -> Result<Terminal, String> {
        Ok(self.terminal(t, u))
    }
    async fn wait_scoped(
        &mut self,
        store: &mut WorkerWorkflow<'_>,
        t: &str,
        u: &str,
        _: Duration,
        cancel: &mut tokio::sync::oneshot::Receiver<String>,
    ) -> Result<WaitOutcome, String> {
        assert_eq!(store.store.state().tasks["task"].state, TaskState::Running);
        self.entered.take().unwrap().send(()).unwrap();
        self.release.take().unwrap().await.unwrap();
        if self.mode.starts_with("cancel") {
            Ok(WaitOutcome::CancelRequested(cancel.await.unwrap()))
        } else {
            Ok(WaitOutcome::Terminal(self.terminal(t, u)))
        }
    }
    async fn interrupt(&mut self, t: &str, u: &str) -> Result<Terminal, String> {
        self.interrupts += 1;
        if self.mode == "cancel-lost" {
            Err("controlled interrupt ACK lost".into())
        } else {
            Ok(self.terminal(t, u))
        }
    }
    async fn read(
        &mut self,
        t: &str,
        a: &str,
        p: &str,
        u: Option<&str>,
    ) -> Result<TurnObservation, String> {
        use sha2::{Digest, Sha256};
        assert_eq!(u, Some("native-turn"));
        Ok(TurnObservation {
            thread_id: t.into(),
            turn_id: u.unwrap().into(),
            client_message_id: a.into(),
            status: self.terminal(t, u.unwrap()).status,
            prompt_sha256: if self.mode == "wrong-proof" {
                "f".repeat(64)
            } else {
                format!("{:x}", Sha256::digest(p.as_bytes()))
            },
            item_types: vec!["userMessage".into(), "agentMessage".into()],
        })
    }
    async fn read_evidence(
        &mut self,
        t: &str,
        a: &str,
        p: &str,
        u: Option<&str>,
        allow: bool,
    ) -> Result<TurnEvidence, String> {
        if self.mode.starts_with("cancel") {
            assert!(
                allow,
                "writer must provide original Started/CancelIntent anchors"
            );
            Ok(TurnEvidence::EmptyInterrupted(InterruptedTurnObservation {
                thread_id: t.into(),
                turn_id: u.unwrap().into(),
                status: "interrupted".into(),
                workspace: self.workspace.to_string_lossy().into(),
                items_view: "full".into(),
                item_count: 0,
                matching_turns: 1,
            }))
        } else {
            self.read(t, a, p, u).await.map(TurnEvidence::RecordedInput)
        }
    }
}
fn binding(attempt: &Attempt) -> WorkerBinding {
    WorkerBinding {
        thread_id: attempt.binding.session_id.clone(),
        workspace: attempt.binding.workspace.clone(),
        model: "fixture-worker".into(),
        provider: "openai".into(),
    }
}
#[test]
fn scoped_native_wait_preserves_completion_cancel_anchors_and_unknown_without_resend() {
    for mode in [
        "completed",
        "cancel-empty",
        "cancel-lost",
        "wrong-proof",
        "missing",
    ] {
        let root = tempfile::tempdir().unwrap();
        let (_host, mut owner, scope, attempt) = fixture(root.path());
        let mut client = owner.codex_client(scope.clone()).unwrap();
        assign(&mut client, attempt.clone()).unwrap();
        let (entered, waiting) = mpsc::channel();
        let (release, resume) = tokio::sync::oneshot::channel();
        let (cancel, mut cancellation) = tokio::sync::oneshot::channel();
        cancel
            .send("original explicit user cancellation".into())
            .unwrap();
        let thread = std::thread::spawn(move || {
            let mut channel = Channel {
                mode,
                workspace: PathBuf::from(&attempt.binding.workspace),
                entered: Some(entered),
                release: Some(resume),
                starts: 0,
                interrupts: 0,
                invalidated: false,
            };
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            let result = runtime.block_on(crate::codex_execution::dispatch_inner(
                &mut client,
                &mut channel,
                &binding(&attempt),
                "task",
                &attempt.id,
                Some(&mut cancellation),
                |_| {},
            ));
            assert_eq!(
                result.is_ok(),
                matches!(mode, "completed" | "missing" | "cancel-empty"),
                "{mode}: {result:?}"
            );
            if mode == "cancel-lost" {
                assert_eq!(client.state().tasks["task"].state, TaskState::Unknown);
                channel.invalidated = false;
                runtime
                    .block_on(crate::codex_execution::reconcile_scoped(
                        &mut client,
                        &mut channel,
                        "task",
                        &attempt.id,
                    ))
                    .unwrap();
            }
            assert_eq!(channel.starts, 1);
            assert_eq!(channel.interrupts, u32::from(mode.starts_with("cancel")));
            let before = client.history().len();
            assert!(runtime
                .block_on(crate::codex_execution::dispatch_inner(
                    &mut client,
                    &mut channel,
                    &binding(&attempt),
                    "task",
                    &attempt.id,
                    None,
                    |_| {}
                ))
                .is_err());
            assert_eq!(client.history().len(), before);
            assert_eq!(channel.starts, 1);
        });
        waiting.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(
            owner.snapshot().unwrap().state.tasks["task"].state,
            TaskState::Running
        );
        assert!(owner.finish(false).is_err());
        release.send(()).unwrap();
        thread.join().unwrap();
        let store = owner.finish(false).unwrap();
        let expected = if matches!(mode, "completed" | "missing") {
            TaskState::Submitted
        } else if mode.starts_with("cancel") {
            TaskState::Cancelled
        } else {
            TaskState::Unknown
        };
        assert_eq!(store.state().tasks["task"].state, expected);
        let history = store.history().to_vec();
        let config = store.configuration().clone();
        drop(store);
        let store =
            DurableWorkflow::open(&root.path().join("data/workflows/project"), config).unwrap();
        assert_eq!(store.history(), history);
        println!("Codex scoped {mode}: writer responsive during native wait; original evidence, one send, reopen events 0");
    }
}

#[test]
fn native_capabilities_reject_foreign_attempts_privilege_escalation_and_wrong_client_kind() {
    let root = tempfile::tempdir().unwrap();
    let (_host, mut owner, scope, attempt) = fixture(root.path());
    assert!(owner.omp_client(scope.clone()).is_err());
    let api = owner.client(scope.clone()).unwrap();
    assert!(api
        .started_cancellation_anchors("task", &attempt.id)
        .is_err());
    drop(api);
    let mut client = owner.codex_client(scope.clone()).unwrap();
    assert!(client.begin_api_dispatch("task", &attempt.id).is_err());
    drop(client);
    for command in [
        WorkflowCommand::DispatchIntent {
            task: "task".into(),
            attempt: "foreign".into(),
        },
        WorkflowCommand::Accept {
            task: "task".into(),
            attempt: attempt.id.clone(),
        },
        WorkflowCommand::Advise {
            task: "task".into(),
            suggestion: "unauthorized before Running".into(),
        },
    ] {
        let before = owner.snapshot().unwrap().sequence;
        let mut client = owner.codex_client(scope.clone()).unwrap();
        assert!(client
            .command(
                "mgmt-forged",
                "deputy",
                command,
                CommandContext::default(),
                true
            )
            .is_err());
        assert!(client.refresh().is_err());
        assert_eq!(owner.snapshot().unwrap().sequence, before);
    }
    let _ = owner.finish(false).unwrap();
}

#[test]
fn native_dispatch_lost_ack_and_interrupted_publication_keep_intent_without_send() {
    for crash in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (_host, mut owner, scope, attempt) = fixture(root.path());
        let mut client = owner.codex_client(scope.clone()).unwrap();
        assign(&mut client, attempt.clone()).unwrap();
        if crash {
            owner.interrupt_next_publication()
        } else {
            owner.lose_next_acknowledgement()
        }
        let mut channel = Channel {
            mode: "completed",
            workspace: PathBuf::from(&attempt.binding.workspace),
            entered: None,
            release: None,
            starts: 0,
            interrupts: 0,
            invalidated: false,
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        assert!(runtime
            .block_on(crate::codex_execution::dispatch_inner(
                &mut client,
                &mut channel,
                &binding(&attempt),
                "task",
                &attempt.id,
                None,
                |_| {}
            ))
            .is_err());
        assert_eq!(channel.starts, 0);
        drop(client);
        let store = owner.finish(true).unwrap();
        assert_eq!(store.state().tasks["task"].state, TaskState::Unknown);
        assert_eq!(
            store
                .history()
                .iter()
                .filter(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. }))
                .count(),
            1
        );
        assert!(store
            .history()
            .iter()
            .any(|e| e.body.operation_id == format!("codex-{}-intent", attempt.id)));
    }
}
