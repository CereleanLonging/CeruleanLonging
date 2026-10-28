//! Process-wide, fixed-size projection executor. One admission per ledger,
//! one snapshot per turn; a busy ledger yields to already queued ledgers.
use super::Shared;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Arc, Condvar, Mutex, OnceLock},
    thread::JoinHandle,
};

const THREADS: usize = 2;
const LEDGERS: usize = 128;

struct Job {
    root: PathBuf,
    ledger: Arc<Shared>,
    #[cfg(test)]
    root_key: PathBuf,
}
#[derive(Default)]
struct State {
    queue: VecDeque<Job>,
    admitted: usize,
    workers: usize,
    closed: bool,
    #[cfg(test)]
    active: usize,
    #[cfg(test)]
    peak_active: usize,
    #[cfg(test)]
    roots: std::collections::BTreeMap<PathBuf, usize>,
}
#[derive(Default)]
struct Inner {
    state: Mutex<State>,
    changed: Condvar,
}
pub(crate) struct Pool {
    inner: Arc<Inner>,
    handles: Mutex<Vec<JoinHandle<()>>>,
    limit: usize,
    capacity: usize,
    budget: Arc<crate::derived_index::memory::Budget>,
}
pub(super) fn global() -> Arc<Pool> {
    static POOL: OnceLock<Arc<Pool>> = OnceLock::new();
    POOL.get_or_init(|| Arc::new(Pool::new(THREADS, LEDGERS)))
        .clone()
}
impl Pool {
    pub fn new(limit: usize, capacity: usize) -> Self {
        Self::with_bytes(limit, capacity, crate::derived_index::memory::GLOBAL)
    }
    pub fn with_bytes(limit: usize, capacity: usize, bytes: usize) -> Self {
        assert!(limit > 0 && capacity >= limit);
        Self {
            inner: Arc::default(),
            handles: Mutex::default(),
            limit,
            capacity,
            budget: crate::derived_index::memory::Budget::new(bytes),
        }
    }
    pub(in crate::derived_index) fn budget(&self) -> Arc<crate::derived_index::memory::Budget> {
        self.budget.clone()
    }
    pub(super) fn submit(&self, root: PathBuf, ledger: Arc<Shared>) -> Result<(), String> {
        let mut state = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
        if state.closed || state.admitted >= self.capacity {
            return Err(
                "derived queue capacity reached; authoritative events remain committed".into(),
            );
        }
        // Lazy start, then retain at most two threads for the lifetime of the
        // process. Idle projects do not own or create threads.
        while state.workers < self.limit {
            let inner = self.inner.clone();
            match std::thread::Builder::new()
                .name("derived-projection".into())
                .spawn(move || run(&inner))
            {
                Ok(handle) => {
                    state.workers += 1;
                    self.handles
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .push(handle);
                }
                Err(_) => break,
            }
        }
        if state.workers == 0 {
            return Err(
                "derived executor could not start; authoritative events remain committed".into(),
            );
        }
        state.admitted += 1;
        #[cfg(test)]
        let root_key = {
            let key = root.canonicalize().unwrap_or_else(|_| root.clone());
            *state.roots.entry(key.clone()).or_default() += 1;
            key
        };
        state.queue.push_back(Job {
            root,
            ledger,
            #[cfg(test)]
            root_key,
        });
        self.inner.changed.notify_one();
        Ok(())
    }
    #[cfg(test)]
    pub fn stats(&self) -> (usize, usize, usize, usize, usize) {
        let state = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
        (
            state.workers,
            state.admitted,
            state.queue.len(),
            state.active,
            state.peak_active,
        )
    }
    #[cfg(test)]
    pub fn wait_idle(&self) {
        let state = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
        let waited = self
            .inner
            .changed
            .wait_timeout_while(state, std::time::Duration::from_secs(20), |s| {
                s.admitted != 0
            })
            .unwrap_or_else(|p| p.into_inner());
        assert_eq!(waited.0.admitted, 0, "projection pool did not drain");
    }
    /// Test evidence must be captured after admitted publications, including
    /// intermediate temp-file cleanup. Do not wait for unrelated test roots.
    #[cfg(test)]
    pub fn wait_root_idle(&self, root: &std::path::Path) {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let state = self.inner.state.lock().unwrap_or_else(|p| p.into_inner());
        let active = |state: &mut State| state.roots.keys().any(|path| path.starts_with(&root));
        let waited = self
            .inner
            .changed
            .wait_timeout_while(state, std::time::Duration::from_secs(60), active)
            .unwrap_or_else(|p| p.into_inner());
        assert!(
            !waited.0.roots.keys().any(|path| path.starts_with(&root)),
            "owned-root projection publications did not drain before byte snapshot"
        );
    }
}
impl Drop for Pool {
    fn drop(&mut self) {
        // Used by owned test pools. The global pool lives until process exit.
        // Drain accepted work without cancelling or killing any thread.
        self.inner
            .state
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .closed = true;
        self.inner.changed.notify_all();
        for handle in self
            .handles
            .get_mut()
            .unwrap_or_else(|p| p.into_inner())
            .drain(..)
        {
            let _ = handle.join();
        }
    }
}
fn run(inner: &Inner) {
    loop {
        let job = {
            let mut state = inner.state.lock().unwrap_or_else(|p| p.into_inner());
            loop {
                if let Some(job) = state.queue.pop_front() {
                    #[cfg(test)]
                    {
                        state.active += 1;
                        state.peak_active = state.peak_active.max(state.active);
                    }
                    break job;
                }
                if state.closed && state.admitted == 0 {
                    state.workers -= 1;
                    return;
                }
                state = inner.changed.wait(state).unwrap_or_else(|p| p.into_inner());
            }
        };
        let again = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            super::run_one(&job.root, &job.ledger)
        }))
        .unwrap_or_else(|_| {
            super::failed(&job.ledger);
            false
        });
        let mut state = inner.state.lock().unwrap_or_else(|p| p.into_inner());
        #[cfg(test)]
        {
            state.active -= 1;
        }
        if again {
            // This ledger retains its admission. It cannot lose its pending
            // final snapshot merely because new ledgers filled the queue.
            state.queue.push_back(job);
        } else {
            state.admitted -= 1;
            #[cfg(test)]
            {
                let count = state.roots.get_mut(&job.root_key).expect("admitted root");
                *count -= 1;
                if *count == 0 {
                    state.roots.remove(&job.root_key);
                }
            }
        }
        inner.changed.notify_all();
    }
}
