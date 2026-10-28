//! Metadata-only, bounded observation of the sole protocol reader. No disk I/O.
use cerulean_domain::worker_observation::*;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::{
        atomic::{AtomicU32, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc, Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};
const CAPACITY: usize = 128;
#[derive(Default)]
struct State {
    expected_thread: Option<String>,
    pending: Vec<(String, ToolObservation)>,
    seen: BTreeMap<String, String>,
    trace: WorkerObservationTrace,
    active: bool,
}
struct Input {
    thread: String,
    turn: String,
    event: ToolObservation,
}
#[derive(Clone)]
pub struct ObservationCapture {
    state: Arc<Mutex<State>>,
    sender: SyncSender<Option<Input>>,
    receiver: Arc<Mutex<Receiver<Option<Input>>>>,
    missed: Arc<AtomicU32>,
}
impl Default for ObservationCapture {
    fn default() -> Self {
        let (sender, receiver) = mpsc::sync_channel(CAPACITY);
        Self {
            state: Arc::default(),
            sender,
            receiver: Arc::new(Mutex::new(receiver)),
            missed: Arc::default(),
        }
    }
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn id(value: &Value) -> Option<&str> {
    value
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 1024 && !s.chars().any(char::is_control))
}
impl ObservationCapture {
    #[cfg(feature = "test-fixtures")]
    pub fn fixture_trace(trace: WorkerObservationTrace) -> Self {
        let capture = Self::default();
        {
            let mut state = capture.state.lock().unwrap();
            state.expected_thread = trace.thread_id.clone();
            state.active = !trace.finished;
            state.trace = trace;
        }
        capture
    }
    // Always called under the state lock; the protocol's hot path only tries it.
    fn drain(&self, s: &mut State) {
        let Ok(receiver) = self.receiver.try_lock() else {
            return;
        };
        while let Ok(input) = receiver.try_recv() {
            if s.expected_thread.is_none() || s.trace.finished {
                continue;
            }
            let Some(input) = input else {
                s.trace.rejected = s.trace.rejected.saturating_add(1);
                continue;
            };
            if s.expected_thread.as_deref() != Some(input.thread.as_str())
                || (s.active && s.trace.turn_id.as_deref() != Some(input.turn.as_str()))
            {
                s.trace.rejected = s.trace.rejected.saturating_add(1);
                continue;
            }
            if s.active {
                s.insert(input.event);
            } else if s.pending.len() < CAPACITY {
                s.pending.push((input.turn, input.event));
            } else {
                s.trace.dropped = s.trace.dropped.saturating_add(1);
            }
        }
    }
    pub(crate) fn prepare(&self, thread: &str) -> Result<(), String> {
        let mut s = self.state.lock().map_err(|_| "observation lock poisoned")?;
        if s.expected_thread.is_some() {
            return Err("observation capture cannot be reused".into());
        }
        self.drain(&mut s);
        s.expected_thread = Some(thread.into());
        Ok(())
    }
    pub(crate) fn activate(&self, thread: &str, turn: &str) -> Result<(), String> {
        let mut s = self.state.lock().map_err(|_| "observation lock poisoned")?;
        if s.expected_thread.as_deref() != Some(thread) || s.active || s.trace.finished {
            return Err("observation scope mismatch".into());
        }
        self.drain(&mut s);
        s.trace.thread_id = Some(thread.into());
        s.trace.turn_id = Some(turn.into());
        s.active = true;
        for (candidate, event) in std::mem::take(&mut s.pending) {
            if candidate == turn {
                s.insert(event);
            } else {
                s.trace.rejected = s.trace.rejected.saturating_add(1);
            }
        }
        Ok(())
    }
    pub fn finish(&self) {
        if let Ok(mut s) = self.state.lock() {
            self.drain(&mut s);
            s.trace.finished = true;
            s.active = false;
        }
    }
    pub fn snapshot(&self) -> WorkerObservationTrace {
        let mut trace = self
            .state
            .lock()
            .map(|mut s| {
                self.drain(&mut s);
                s.trace.clone()
            })
            .unwrap_or_default();
        trace.dropped = trace
            .dropped
            .saturating_add(self.missed.load(Ordering::Relaxed));
        trace
    }
    pub(crate) fn observe(&self, value: &Value) {
        if value["method"] != "item/completed" || value.get("id").is_some() {
            return;
        }
        let p = &value["params"];
        let item = &p["item"];
        let kind = match item["type"].as_str() {
            Some("commandExecution") => ObservedToolKind::Command,
            Some("fileChange") => ObservedToolKind::FileChange,
            _ => return,
        };
        // Queue only already-redacted bounded metadata. Never retain raw Values.
        let input = (|| {
            let thread = id(&p["threadId"])?;
            let turn = id(&p["turnId"])?;
            let item_id = id(&item["id"])?;
            let status = match item["status"].as_str()? {
                "completed" => ObservedToolStatus::Completed,
                "failed" => ObservedToolStatus::Failed,
                "declined" => ObservedToolStatus::Declined,
                _ => return None,
            };
            let (exit_code, action_digest) = if kind == ObservedToolKind::Command {
                let code = if item["exitCode"].is_null() {
                    None
                } else {
                    Some(i32::try_from(item["exitCode"].as_i64()?).ok()?)
                };
                let command = item["command"].as_str().filter(|c| c.len() <= 64 * 1024)?;
                (code, Some(hash(command.as_bytes())))
            } else {
                (None, None)
            };
            Some(Input {
                thread: thread.into(),
                turn: turn.into(),
                event: ToolObservation {
                    sequence: 0,
                    received_at_ms: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0),
                    item_digest: hash(item_id.as_bytes()),
                    kind,
                    status,
                    exit_code,
                    action_digest,
                },
            })
        })();
        if self.sender.try_send(input).is_err() {
            let _ = self
                .missed
                .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                    Some(n.saturating_add(1))
                });
        }
        // Readers may hold a snapshot lock; queue until the next owner can drain.
        if let Ok(mut s) = self.state.try_lock() {
            self.drain(&mut s);
        }
    }
}
impl State {
    fn insert(&mut self, mut event: ToolObservation) {
        let fingerprint = cerulean_domain::execution::digest(&(
            &event.kind,
            &event.status,
            &event.exit_code,
            &event.action_digest,
        ))
        .expect("metadata serializes");
        if let Some(previous) = self.seen.get(&event.item_digest) {
            if previous == &fingerprint {
                self.trace.duplicates = self.trace.duplicates.saturating_add(1);
            } else {
                self.trace.rejected = self.trace.rejected.saturating_add(1);
            }
            return;
        }
        if self.trace.events.len() >= CAPACITY {
            self.trace.dropped = self.trace.dropped.saturating_add(1);
            return;
        }
        self.seen.insert(event.item_digest.clone(), fingerprint);
        event.sequence = self.trace.events.len() as u32 + 1;
        self.trace.events.push(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn event(id: &str) -> Value {
        json!({"method":"item/completed","params":{"threadId":"thread","turnId":"turn","item":{"type":"commandExecution","id":id,"status":"failed","exitCode":7,"command":"tool --token private-sentinel","aggregatedOutput":"private-sentinel","reasoning":"private-sentinel"}}})
    }
    #[test]
    fn binds_after_ack_discards_private_payloads_and_deduplicates_conflicts() {
        let c = ObservationCapture::default();
        c.prepare("thread").unwrap();
        c.observe(&event("private-sentinel"));
        c.observe(&event("private-sentinel"));
        let mut wrong = event("wrong");
        wrong["params"]["turnId"] = json!("foreign");
        c.observe(&wrong);
        c.observe(
            &json!({"method":"item/reasoning/textDelta","params":{"delta":"private-sentinel"}}),
        );
        assert!(c.snapshot().events.is_empty());
        assert!(c.activate("wrong-thread", "turn").is_err());
        c.activate("thread", "turn").unwrap();
        let mut conflict = event("private-sentinel");
        conflict["params"]["item"]["exitCode"] = json!(0);
        c.observe(&conflict);
        let trace = c.snapshot();
        assert_eq!(trace.events.len(), 1);
        assert_eq!(trace.duplicates, 1);
        assert_eq!(trace.rejected, 2);
        assert!(!serde_json::to_string(&trace)
            .unwrap()
            .contains("private-sentinel"));
        c.finish();
        c.observe(&event("late"));
        assert_eq!(c.snapshot().events.len(), 1);
        assert!(c.prepare("thread").is_err());
    }
    #[test]
    fn snapshot_contention_queues_metadata_and_reports_only_capacity_loss() {
        let c = ObservationCapture::default();
        c.prepare("thread").unwrap();
        c.activate("thread", "turn").unwrap();
        let guard = c.state.lock().unwrap();
        c.observe(&event("while-reading"));
        drop(guard);
        assert_eq!(c.snapshot().events.len(), 1);
        assert_eq!(c.snapshot().dropped, 0);
        let guard = c.state.lock().unwrap();
        for n in 0..140 {
            c.observe(&event(&format!("queued-{n}")));
        }
        drop(guard);
        let trace = c.snapshot();
        assert_eq!(trace.events.len(), 128);
        assert_eq!(trace.dropped, 13);
        assert_eq!(trace.events[0].item_digest, hash(b"while-reading"));
    }
    #[test]
    fn capacity_and_reader_contention_report_loss_without_blocking() {
        let c = ObservationCapture::default();
        c.prepare("thread").unwrap();
        c.activate("thread", "turn").unwrap();
        for n in 0..160 {
            c.observe(&event(&format!("item-{n}")));
        }
        let guard = c.state.lock().unwrap();
        c.observe(&event("contended"));
        drop(guard);
        let trace = c.snapshot();
        assert_eq!(trace.events.len(), 128);
        assert_eq!(trace.dropped, 33);
        assert_eq!(trace.events.last().unwrap().sequence, 128);
    }
}
