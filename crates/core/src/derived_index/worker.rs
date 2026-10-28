//! One in-flight build plus one replaceable pending snapshot per ledger.
//! A process-wide executor bounds threads and queued ledgers. Owners drain their
//! own publications before releasing the authoritative writer lock.
use super::{Expected, Head, Workload};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

pub(super) mod pool;

#[cfg(test)]
pub(super) fn wait_for_test_root(root: &Path) {
    pool::global().wait_root_idle(root);
}

type Outcome = Result<Arc<Expected>, String>;
struct Receipt {
    head: Head,
    index: Outcome,
    documents: Result<(), String>,
}
#[derive(Default)]
struct State {
    pending: Option<Workload>,
    running: bool,
    receipt: Option<Receipt>,
    failed: bool,
    checkpoint_error: Option<String>,
    #[cfg(test)]
    gate: Option<Arc<TestGate>>,
    #[cfg(test)]
    builds: usize,
    #[cfg(test)]
    panic_next: bool,
}
struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    budget: Arc<super::memory::Budget>,
}
/// Cheap, read-only diagnostics; deliberately outside authoritative snapshots.
#[derive(Clone)]
pub(crate) struct Diagnostics(Arc<Shared>);
impl Diagnostics {
    pub fn error(&self) -> Option<String> {
        let state = self.0.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.failed {
            return Some("derived worker stopped; authoritative events remain committed".into());
        }
        state.checkpoint_error.clone().or_else(|| {
            state.receipt.as_ref().and_then(|r| {
                r.documents
                    .as_ref()
                    .err()
                    .or_else(|| r.index.as_ref().err())
                    .cloned()
            })
        })
    }
}
pub(super) struct Worker {
    root: Option<PathBuf>,
    shared: Arc<Shared>,
    pool: Arc<pool::Pool>,
}
impl Default for Worker {
    fn default() -> Self {
        let pool = pool::global();
        Self {
            root: None,
            shared: Arc::new(Shared {
                state: Mutex::default(),
                changed: Condvar::new(),
                budget: pool.budget(),
            }),
            pool,
        }
    }
}
impl Worker {
    pub(super) fn reserve(&self, bytes: usize) -> Result<super::memory::Lease, String> {
        self.shared.budget.reserve(bytes)
    }
    pub fn encode(
        &self,
        value: &impl serde::Serialize,
        limit: usize,
    ) -> Result<super::memory::Encoded, String> {
        super::memory::Encoded::new(value, &self.shared.budget, limit)
    }
    pub fn discard_pending_views(&self) {
        let mut state = self.shared.state.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(pending) = &mut state.pending {
            pending.index = Err("view superseded while latest snapshot is prepared".into());
            pending.documents = None;
        }
        // Existing readers retain their Arc/lease until they finish. A replaced
        // receipt must not reserve bytes forever or prevent its own replacement.
        state.receipt = None;
    }
    #[cfg(test)]
    pub fn with_pool(pool: Arc<pool::Pool>) -> Self {
        Self {
            root: None,
            shared: Arc::new(Shared {
                state: Mutex::default(),
                changed: Condvar::new(),
                budget: pool.budget(),
            }),
            pool,
        }
    }
    #[cfg(test)]
    pub fn panic_next(&self) {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .panic_next = true;
    }

    #[cfg(test)]
    pub fn test_gate(&self) -> GateLease {
        let gate = Arc::new(TestGate::default());
        self.shared
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .gate = Some(gate.clone());
        GateLease(gate)
    }
    #[cfg(test)]
    pub fn test_stats(&self) -> (usize, Option<usize>, bool) {
        let state = self.shared.state.lock().unwrap_or_else(|p| p.into_inner());
        (
            state.builds,
            state.pending.as_ref().map(|d| d.head.count),
            state.running,
        )
    }
    pub fn diagnostics(&self) -> Diagnostics {
        Diagnostics(self.shared.clone())
    }
    pub fn projection_error(&self) -> Option<String> {
        let state = self.shared.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.failed {
            return Some("derived worker stopped; original events remain committed".into());
        }
        state
            .receipt
            .as_ref()
            .and_then(|r| r.documents.as_ref().err().cloned())
    }
    pub fn submit(&mut self, root: &Path, mut data: Workload) -> Result<(), String> {
        if self
            .root
            .as_deref()
            .is_some_and(|previous| previous != root)
        {
            return Err("index worker root changed".into());
        }
        self.root = Some(root.to_path_buf());
        let mut state = self.shared.state.lock().unwrap_or_else(|p| p.into_inner());
        // A newer index-only snapshot must not erase an already due checkpoint.
        if data.checkpoint.is_none() {
            data.checkpoint = state.pending.as_mut().and_then(|p| p.checkpoint.take());
        }
        state.pending = Some(data);
        if state.running {
            return Ok(());
        }
        state.running = true;
        state.failed = false;
        drop(state);
        if let Err(error) = self.pool.submit(root.to_path_buf(), self.shared.clone()) {
            failed(&self.shared);
            return Err(error);
        }
        Ok(())
    }

    pub fn ready(&self, head: &Head) -> Outcome {
        let state = self.shared.state.lock().unwrap_or_else(|p| p.into_inner());
        match &state.receipt {
            Some(receipt) if &receipt.head == head => receipt.index.clone(),
            _ => Err("index is catching up; ledger fallback active".into()),
        }
    }
    pub fn failure(&self, head: &Head) -> Option<String> {
        let state = self.shared.state.lock().unwrap_or_else(|p| p.into_inner());
        match &state.receipt {
            Some(receipt) if &receipt.head == head => receipt.index.as_ref().err().cloned(),
            _ if state.failed => Some("index worker stopped; ledger fallback active".into()),
            _ => None,
        }
    }
    pub fn wait(&self, head: &Head, timeout: Duration) -> Result<(), String> {
        self.wait_for(head, timeout, false)
    }
    pub fn wait_documents(&self, head: &Head, timeout: Duration) -> Result<(), String> {
        self.wait_for(head, timeout, true)
    }
    fn wait_for(&self, head: &Head, timeout: Duration, documents: bool) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        let mut state = self.shared.state.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            if state.failed {
                return Err("derived worker stopped before expected publication".into());
            }
            if let Some(receipt) = &state.receipt {
                if &receipt.head == head && !state.running && state.pending.is_none() {
                    return if documents {
                        receipt.documents.clone()
                    } else {
                        receipt.index.as_ref().map(|_| ()).map_err(Clone::clone)
                    };
                }
            }
            if !state.running {
                return Err("index worker stopped before expected head".into());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("index wait timed out; original build remains owned".into());
            }
            state = self
                .shared
                .changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(|p| p.into_inner())
                .0;
        }
    }
    pub fn shutdown(&mut self) {
        // The shared threads also serve other ledgers. Drain this ledger only,
        // without cancelling it or releasing its authoritative writer lock early.
        let mut state = self.shared.state.lock().unwrap_or_else(|p| p.into_inner());
        while state.running {
            state = self
                .shared
                .changed
                .wait(state)
                .unwrap_or_else(|p| p.into_inner());
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn failed(shared: &Shared) {
    let mut state = shared.state.lock().unwrap_or_else(|p| p.into_inner());
    state.running = false;
    state.failed = true;
    state.pending = None;
    state.receipt = None;
    shared.changed.notify_all();
}

fn run_one(root: &Path, shared: &Shared) -> bool {
    let data = {
        let mut state = shared.state.lock().unwrap_or_else(|p| p.into_inner());
        match state.pending.take() {
            Some(data) => {
                #[cfg(test)]
                {
                    state.builds += 1;
                    if std::mem::take(&mut state.panic_next) {
                        panic!("controlled derived executor failure");
                    }
                }
                data
            }
            None => {
                state.running = false;
                shared.changed.notify_all();
                return false;
            }
        }
    };
    #[cfg(test)]
    {
        let gate = shared
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .gate
            .take();
        if let Some(gate) = gate {
            let mut flags = gate.flags.lock().unwrap();
            flags.0 = true;
            gate.changed.notify_all();
            while !flags.1 {
                flags = gate.changed.wait(flags).unwrap();
            }
        }
    }
    let head = data.head;
    // Either failure must still allow the other projection to be attempted.
    // A panic here never crosses the event publication boundary.
    let documents = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        data.documents.map_or(Ok(()), |snapshot| {
            snapshot
                .and_then(|bytes| bytes.decode::<crate::scheduler::projection::Snapshot>())
                .and_then(|snapshot| snapshot.publish(root))
        })
    }))
    .unwrap_or_else(|_| Err("document projection panicked".into()));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        data.index
            .and_then(|bytes| bytes.decode::<super::Data>())
            .and_then(|index| super::build(root, &index, &shared.budget))
            .map(Arc::new)
    }))
    .unwrap_or_else(|_| Err("index projection panicked".into()));
    let checkpoint = data.checkpoint.map(|snapshot| {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            snapshot.and_then(|bytes| {
                crate::replay_checkpoint::publish_serialized(root, bytes.as_slice())
            })
        }))
        .unwrap_or_else(|_| Err("replay checkpoint publication panicked".into()))
    });
    let mut state = shared.state.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(checkpoint) = checkpoint {
        state.checkpoint_error = checkpoint.err();
    }
    state.receipt = Some(Receipt {
        head,
        index: result,
        documents,
    });
    let again = state.pending.is_some();
    state.running = again;
    shared.changed.notify_all();
    again
}

#[cfg(test)]
#[derive(Default)]
struct TestGate {
    flags: Mutex<(bool, bool)>,
    changed: Condvar,
}
#[cfg(test)]
pub(crate) struct GateLease(Arc<TestGate>);
#[cfg(test)]
impl GateLease {
    pub fn entered(&self) {
        let flags = self.0.flags.lock().unwrap();
        let waited = self
            .0
            .changed
            .wait_timeout_while(flags, Duration::from_secs(10), |f| !f.0)
            .unwrap();
        let entered = waited.0 .0;
        drop(waited);
        assert!(entered, "index test gate was not reached");
    }
    pub fn release(&self) {
        self.0.flags.lock().unwrap_or_else(|p| p.into_inner()).1 = true;
        self.0.changed.notify_all();
    }
}
#[cfg(test)]
impl Drop for GateLease {
    fn drop(&mut self) {
        self.release();
    }
}
