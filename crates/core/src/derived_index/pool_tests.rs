use super::*;
use crate::Store;
use cerulean_domain::Command;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use worker::{pool::Pool, Worker};

fn store(root: &Path, pool: &Arc<Pool>) -> Store {
    let mut store = Store::open(root).unwrap();
    store.wait_for_index(Duration::from_secs(20)).unwrap();
    store.index = Cache {
        worker: Worker::with_pool(pool.clone()),
        ..Default::default()
    };
    store.refresh_index();
    store.wait_for_index(Duration::from_secs(20)).unwrap();
    pool.wait_idle();
    store
}
fn append(store: &mut Store, n: usize) {
    store
        .execute_command(Command::CreatePlan {
            title: format!("plan-{n}"),
            tasks: vec!["isolated".into()],
        })
        .unwrap();
}

#[test]
fn many_ledgers_have_bounded_threads_and_latest_snapshots() {
    let root = tempfile::tempdir().unwrap();
    let pool = Arc::new(Pool::new(2, 32));
    let mut stores: Vec<_> = (0..24)
        .map(|i| store(&root.path().join(format!("ledger-{i}")), &pool))
        .collect();
    let a = stores[0].index.worker.test_gate();
    let b = stores[1].index.worker.test_gate();
    append(&mut stores[0], 0);
    a.entered();
    append(&mut stores[1], 0);
    b.entered();
    let start = Instant::now();
    for (i, store) in stores.iter_mut().enumerate() {
        for n in if i < 2 { 1..12 } else { 0..12 } {
            append(store, n);
        }
        assert_eq!(store.snapshot().plans.len(), 12);
        assert!(store.wait_for_index(Duration::ZERO).is_err());
    }
    let append_ms = start.elapsed().as_millis();
    let bounded = pool.stats();
    assert_eq!(bounded, (2, 24, 22, 2, 2));
    a.release();
    b.release();
    for store in &stores {
        store.wait_for_index(Duration::from_secs(20)).unwrap();
        assert!(store.index_error().is_none());
        assert_eq!(
            store
                .index
                .read(&store.root, &store.index_head())
                .unwrap()
                .plans
                .len(),
            12
        );
        assert_eq!(fs::read_dir(store.root.join("events")).unwrap().count(), 12);
    }
    pool.wait_idle();
    assert_eq!(pool.stats(), (2, 0, 0, 0, 2));
    let report = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports/projection-pool-305");
    fs::create_dir_all(&report).unwrap();
    let name = format!(
        "multi-ledger-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    fs::write(report.join(name), serde_json::to_vec_pretty(&serde_json::json!({
        "ledgers":24,"events":288,"executorThreads":2,"peakConcurrentPublications":2,
        "queuedLedgersWhileBlocked":22,"appendWhileBothThreadsBlockedMs":append_ms,
        "finalCountsAndLedgerFilesChecked":true,"modelCalls":0,
        "scope":"local debug synthetic ledgers; test-owned pool with production concurrency; not GUI or model efficiency"
    })).unwrap()).unwrap();
}

#[test]
fn hot_ledger_yields_its_next_snapshot_to_queued_ledgers() {
    let root = tempfile::tempdir().unwrap();
    let pool = Arc::new(Pool::new(1, 3));
    let mut a = store(&root.path().join("a"), &pool);
    let mut b = store(&root.path().join("b"), &pool);
    let a_gate = a.index.worker.test_gate();
    let b_gate = b.index.worker.test_gate();
    append(&mut a, 0);
    a_gate.entered();
    append(&mut b, 0);
    for n in 1..15 {
        append(&mut a, n);
    }
    a_gate.release();
    b_gate.entered();
    assert_eq!(a.index.worker.test_stats().1, Some(15));
    assert!(a.wait_for_index(Duration::ZERO).is_err());
    b_gate.release();
    a.wait_for_index(Duration::from_secs(20)).unwrap();
    b.wait_for_index(Duration::from_secs(20)).unwrap();
    assert_eq!(a.snapshot().plans.len(), 15);
}

#[test]
fn saturation_keeps_authority_and_explicit_refresh_does_not_repeat_command() {
    let root = tempfile::tempdir().unwrap();
    let pool = Arc::new(Pool::new(1, 2));
    let mut a = store(&root.path().join("a"), &pool);
    let mut b = store(&root.path().join("b"), &pool);
    let mut c = store(&root.path().join("c"), &pool);
    let gate = a.index.worker.test_gate();
    append(&mut a, 0);
    gate.entered();
    append(&mut b, 0);
    append(&mut c, 0);
    assert!(c
        .wait_for_index(Duration::ZERO)
        .unwrap_err()
        .contains("capacity"));
    assert!(c.index.diagnostics().error().is_some());
    assert_eq!(c.snapshot().plans.len(), 1);
    let files: Vec<_> = fs::read_dir(c.root.join("events"))
        .unwrap()
        .map(|e| fs::read(e.unwrap().path()).unwrap())
        .collect();
    assert_eq!(files.len(), 1);
    gate.release();
    pool.wait_idle();
    c.refresh_index();
    c.wait_for_index(Duration::from_secs(20)).unwrap();
    assert!(c.index.diagnostics().error().is_none());
    assert_eq!(c.snapshot().plans.len(), 1);
    let after: Vec<_> = fs::read_dir(c.root.join("events"))
        .unwrap()
        .map(|e| fs::read(e.unwrap().path()).unwrap())
        .collect();
    assert_eq!(after, files);
}

#[test]
fn panic_is_confined_to_one_ledger_and_pool_remains_usable() {
    let root = tempfile::tempdir().unwrap();
    let pool = Arc::new(Pool::new(1, 2));
    let mut a = store(&root.path().join("a"), &pool);
    let mut b = store(&root.path().join("b"), &pool);
    a.index.worker.panic_next();
    append(&mut a, 0);
    assert!(a.wait_for_index(Duration::from_secs(20)).is_err());
    assert_eq!(a.snapshot().plans.len(), 1);
    append(&mut b, 0);
    b.wait_for_index(Duration::from_secs(20)).unwrap();
    pool.wait_idle();
    a.refresh_index();
    a.wait_for_index(Duration::from_secs(20)).unwrap();
    assert!(a.index_error().is_none());
    assert_eq!(a.snapshot().plans.len(), 1);
    assert_eq!(b.snapshot().plans.len(), 1);
    assert_eq!(pool.stats().0, 1);
}

#[cfg(windows)]
#[test]
fn queued_ledger_drop_keeps_writer_lock_until_its_turn_finishes() {
    let root = tempfile::tempdir().unwrap();
    let pool = Arc::new(Pool::new(1, 2));
    let mut a = store(&root.path().join("a"), &pool);
    let path = root.path().join("b");
    let mut b = store(&path, &pool);
    let gate = a.index.worker.test_gate();
    append(&mut a, 0);
    gate.entered();
    append(&mut b, 0);
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        drop(b);
        done_tx.send(()).unwrap();
    });
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(Store::open(&path).is_err());
    assert!(done_rx.try_recv().is_err());
    gate.release();
    done_rx.recv_timeout(Duration::from_secs(20)).unwrap();
    thread.join().unwrap();
    let reopened = Store::open(&path).unwrap();
    assert_eq!(reopened.snapshot().plans.len(), 1);
}
