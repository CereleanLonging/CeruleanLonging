//! Advice is applied by the existing workflow/transport owner, never by a second reader.
use crate::{
    advice_delivery::*,
    async_codex::Update,
    codex_execution::{WorkerChannel, WorkerWorkflow},
    codex_workflow_access::CodexWorkflowAccess,
    scheduler::{CommandContext, DurableWorkflow, Entry},
};
use cerulean_adapters::codex::*;
use cerulean_domain::{execution::*, workflow::WorkflowCommand, CodexJob};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU8, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};

pub(crate) enum AdviceUpdate {
    Prepare {
        snapshot: Box<ExecutionSnapshot>,
        reply: tokio::sync::oneshot::Sender<Result<Option<Prepared>, String>>,
    },
    Commit {
        operation: String,
        entry: Box<Entry>,
        snapshot: Box<ExecutionSnapshot>,
        reply: tokio::sync::oneshot::Sender<Result<Option<DeliveryIntent>, String>>,
    },
    Finish {
        operation: String,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
}
pub(crate) struct Prepared {
    pub operation: String,
    pub suggestion: String,
}
pub(crate) struct WorkerControls<'a> {
    pub cancellation: &'a mut tokio::sync::oneshot::Receiver<String>,
    pub advice: &'a mut tokio::sync::watch::Receiver<()>,
    pub gate: Arc<AtomicU8>,
}
pub(crate) fn folder(root: &Path, project: &str, attempt: &str) -> PathBuf {
    root.join("advice-deliveries").join(project).join(attempt)
}
/// Compiled only into the unit-test owner process. The parent terminates that
/// process after seeing a flushed marker; this does not simulate recovery files.
#[cfg(test)]
fn crash_barrier(root: &Path, point: &str) {
    if std::env::var("CERULEAN_ADVICE_CRASH_POINT").as_deref() != Ok(point) {
        return;
    }
    use std::io::Write;
    let expected = PathBuf::from(std::env::var_os("CERULEAN_DAG_DIR").unwrap())
        .join("data")
        .canonicalize()
        .unwrap();
    assert_eq!(root.canonicalize().unwrap(), expected);
    let marker = expected.join("advice-crash-marker.json");
    let pending = marker.with_extension("pending");
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&pending)
        .unwrap();
    write!(
        file,
        "{}",
        serde_json::json!({"point":point,"ownerPid":std::process::id()})
    )
    .unwrap();
    file.sync_all().unwrap();
    drop(file);
    std::fs::rename(pending, marker).unwrap();
    loop {
        std::thread::park();
    }
}
pub(crate) struct SupervisedWorker<'a> {
    pub client: &'a mut CodexClient,
    pub root: &'a Path,
    pub job: &'a CodexJob,
    pub sender: &'a mpsc::Sender<Update>,
    pub control: &'a mut tokio::sync::watch::Receiver<()>,
    /// 0 idle, 1 reserved, 2 send begun, 3 revoked. Pause/stop only moves to 3.
    pub gate: Arc<AtomicU8>,
}
async fn response<T>(rx: tokio::sync::oneshot::Receiver<Result<T, String>>) -> Result<T, String> {
    tokio::time::timeout(Duration::from_secs(15), rx)
        .await
        .map_err(|_| "supervision host acknowledgement timed out")?
        .map_err(|_| "supervision host acknowledgement lost")?
}
impl SupervisedWorker<'_> {
    async fn wait_with_advice(
        &mut self,
        store: &mut (impl CodexWorkflowAccess + ?Sized),
        t: &str,
        u: &str,
        d: Duration,
        cancel: &mut tokio::sync::oneshot::Receiver<String>,
    ) -> Result<WaitOutcome, String> {
        let deadline = tokio::time::Instant::now() + d;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err("supervised execution deadline expired".into());
            }
            match self
                .client
                .wait_turn_or_control(t, u, remaining, cancel, self.control)
                .await?
            {
                ControlOutcome::Finished(outcome) => return Ok(outcome),
                ControlOutcome::SupervisionReady => self.advice(store, t, u).await?,
            }
        }
    }

    async fn advice(
        &mut self,
        store: &mut (impl CodexWorkflowAccess + ?Sized),
        thread: &str,
        turn: &str,
    ) -> Result<(), String> {
        // Another attempt may have advanced the shared journal while this one
        // waited for model output. Prepare must include the original management
        // snapshot, not the worker's older cached prefix. This is a scoped read;
        // the host still validates the original turn and both acknowledgements.
        store.refresh()?;
        let (reply, rx) = tokio::sync::oneshot::channel();
        self.sender
            .send(Update::Advice(AdviceUpdate::Prepare {
                snapshot: Box::new(crate::async_verification::snapshot(store)),
                reply,
            }))
            .map_err(|_| "supervision host disconnected")?;
        let Some(prepared) = response(rx).await? else {
            return Ok(());
        };
        #[cfg(test)]
        crash_barrier(self.root, "prepared");
        if self.gate.load(Ordering::SeqCst) != 1 {
            return Ok(());
        }
        let entry = store.command(
            &format!("mgmt-{}", prepared.operation),
            "deputy",
            WorkflowCommand::Advise {
                task: self.job.task_id.clone(),
                suggestion: prepared.suggestion.clone(),
            },
            CommandContext::default(),
            true,
        )?;
        #[cfg(test)]
        crash_barrier(self.root, "advised");
        let (reply, rx) = tokio::sync::oneshot::channel();
        self.sender
            .send(Update::Advice(AdviceUpdate::Commit {
                operation: prepared.operation.clone(),
                entry: Box::new(entry),
                snapshot: Box::new(crate::async_verification::snapshot(store)),
                reply,
            }))
            .map_err(|_| "supervision host disconnected after application")?;
        let Some(intent) = response(rx).await? else {
            return Ok(());
        };
        require(
            intent.thread_id == thread
                && intent.turn_id == turn
                && intent.text_digest == digest(&prepared.suggestion)?,
            "advice delivery differs from original turn",
        )?;
        let mut journal = DeliveryJournal::open(&folder(
            self.root,
            &self.job.project_id,
            &self.job.attempt_id,
        ))?;
        journal.begin(intent.clone())?;
        #[cfg(test)]
        crash_barrier(self.root, "intent");
        let result = if self
            .gate
            .compare_exchange(1, 2, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            self.client
                .steer_turn(&SteerRequest {
                    delivery_id: intent.delivery_id.clone(),
                    thread_id: thread.into(),
                    turn_id: turn.into(),
                    text: prepared.suggestion,
                })
                .await
        } else {
            Err("delivery revoked before send".into())
        };
        #[cfg(test)]
        crash_barrier(self.root, "received");
        let outcome = match &result {
            Ok(ack) => DeliveryOutcome::Acknowledged { ack: ack.clone() },
            Err(_) => DeliveryOutcome::Unknown,
        };
        journal.finish(&intent.delivery_id, outcome)?;
        #[cfg(test)]
        crash_barrier(self.root, "acknowledged");
        let (reply, rx) = tokio::sync::oneshot::channel();
        self.sender
            .send(Update::Advice(AdviceUpdate::Finish {
                operation: prepared.operation,
                reply,
            }))
            .map_err(|_| "supervision host disconnected after delivery")?;
        response(rx).await?;
        let _ = self
            .gate
            .compare_exchange(2, 0, Ordering::SeqCst, Ordering::SeqCst);
        // A revoked unsent request leaves the transport usable for stop/completion.
        // A wire failure poisons CodexClient; the next wait returns unknown.
        Ok(())
    }
}
impl WorkerChannel for SupervisedWorker<'_> {
    fn activate_observations(&mut self, t: &str, u: &str) -> Result<(), String> {
        self.client.activate_observations(t, u)
    }
    fn invalidate(&mut self) {
        self.client.invalidate();
    }
    async fn start(&mut self, b: &WorkerBinding, a: &str, p: &str) -> Result<String, String> {
        self.client.start_turn(b, a, p).await
    }
    async fn wait(&mut self, t: &str, u: &str, d: Duration) -> Result<Terminal, String> {
        self.client.wait_turn(t, u, d).await
    }
    async fn wait_supervised(
        &mut self,
        store: &mut DurableWorkflow,
        t: &str,
        u: &str,
        d: Duration,
        cancel: &mut tokio::sync::oneshot::Receiver<String>,
    ) -> Result<WaitOutcome, String> {
        self.wait_with_advice(store, t, u, d, cancel).await
    }
    async fn wait_scoped(
        &mut self,
        store: &mut WorkerWorkflow<'_>,
        t: &str,
        u: &str,
        d: Duration,
        cancel: &mut tokio::sync::oneshot::Receiver<String>,
    ) -> Result<WaitOutcome, String> {
        self.wait_with_advice(store.store, t, u, d, cancel).await
    }
    async fn interrupt(&mut self, t: &str, u: &str) -> Result<Terminal, String> {
        self.client.interrupt(t, u).await
    }
    async fn read(
        &mut self,
        t: &str,
        a: &str,
        p: &str,
        u: Option<&str>,
    ) -> Result<TurnObservation, String> {
        self.client.read_attempt(t, a, p, u).await
    }
    async fn read_evidence(
        &mut self,
        t: &str,
        a: &str,
        p: &str,
        u: Option<&str>,
        allow: bool,
    ) -> Result<TurnEvidence, String> {
        self.client.read_evidence(t, a, p, u, allow).await
    }
}
