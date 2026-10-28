use super::*;
use crate::Store;
use cerulean_domain::{
    workspace::{WorkspaceMode, WorkspaceQuery, WorkspaceResult},
    Command,
};
use std::time::{Duration, Instant};

fn append(store: &mut Store, n: usize) {
    store
        .execute_command(Command::CreatePlan {
            title: format!("plan {n}"),
            tasks: vec!["isolated task".into()],
        })
        .unwrap();
}
fn settled(root: &Path) -> Store {
    let store = Store::open(root).unwrap();
    store.wait_for_index(Duration::from_secs(20)).unwrap();
    store
}

#[test]
#[cfg(windows)]
fn byte_snapshot_waits_for_owned_publication_and_not_an_unrelated_blocked_root() {
    use std::sync::mpsc;
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let first_store = settled(first.path());
    let mut second_store = settled(second.path());
    let gate = second_store.index.worker.test_gate();
    append(&mut second_store, 1);
    gate.entered();
    let (sent, done) = mpsc::channel();
    let first_path = first.path().to_path_buf();
    let reader = std::thread::spawn(move || {
        sent.send(crate::reader_generation::tests::snapshot(&first_path))
            .unwrap();
    });
    // An unrelated ledger's paused publisher cannot block this root's snapshot.
    let snapshot = done.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(snapshot.contains_key(&first.path().join("index.sqlite")));
    reader.join().unwrap();
    let (sent, done) = mpsc::channel();
    let second_path = second.path().to_path_buf();
    let reader = std::thread::spawn(move || {
        sent.send(crate::reader_generation::tests::snapshot(&second_path))
            .unwrap();
    });
    assert!(matches!(
        done.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    gate.release();
    let snapshot = done.recv_timeout(Duration::from_secs(20)).unwrap();
    reader.join().unwrap();
    assert!(snapshot.contains_key(&second.path().join("index.sqlite")));
    assert!(!snapshot
        .keys()
        .any(|p| p.extension().is_some_and(|x| x == "tmp")));
    assert_eq!(
        snapshot,
        crate::reader_generation::tests::snapshot(second.path())
    );
    drop((first_store, second_store));
}

#[test]
fn readiness_wait_does_not_return_an_older_receipt_while_same_head_is_building() {
    let root = tempfile::tempdir().unwrap();
    let mut store = settled(root.path());
    let gate = store.index.worker.test_gate();
    store.refresh_index();
    gate.entered();
    assert!(store.wait_for_index(Duration::ZERO).is_err());
    gate.release();
    store.wait_for_index(Duration::from_secs(20)).unwrap();
    assert!(!store.index.worker.test_stats().2);
    fs::remove_file(root.path().join("index.sqlite")).unwrap();
    assert!(store.snapshot().plans.is_empty());
}

#[test]
fn pending_snapshots_coalesce_and_queries_never_wait_for_background_io() {
    let root = tempfile::tempdir().unwrap();
    let mut store = settled(root.path());
    let before = store.index.worker.test_stats().0;
    let gate = store.index.worker.test_gate();
    append(&mut store, 0);
    gate.entered();
    for n in 1..80 {
        append(&mut store, n);
    }
    let (builds, pending, _) = store.index.worker.test_stats();
    assert_eq!(builds, before + 1);
    assert_eq!(pending, Some(80));
    assert!(store.wait_for_index(Duration::ZERO).is_err());
    assert_eq!(store.snapshot().plans.len(), 80);
    let page = store
        .workspace_plans(&WorkspaceQuery::Plans {
            offset: 0,
            revision: None,
        })
        .unwrap();
    let WorkspaceResult::Plans(page) = page else {
        panic!("plans page")
    };
    assert_eq!(page.total, 80);
    assert_eq!(page.entries.len(), 20);
    // Normal background lag is not a damaged-ledger diagnostic.
    assert!(store
        .workspace_overview(WorkspaceMode::Ready, None, Some(0))
        .message
        .is_none());
    gate.release();
    store.wait_for_index(Duration::from_secs(20)).unwrap();
    assert_eq!(store.index.worker.test_stats().0, before + 2);
    assert!(store.index_error().is_none());
    assert_eq!(
        store
            .index
            .plans(&store.root, &store.index_head(), 60, 20)
            .unwrap()[0]
            .title,
        "plan 60"
    );
}

#[cfg(windows)]
#[test]
fn writer_lock_outlives_background_drain_and_backup_sees_a_stable_cache() {
    use std::sync::mpsc;
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    let mut store = settled(&data);
    let gate = store.index.worker.test_gate();
    append(&mut store, 0);
    gate.entered();
    append(&mut store, 1);
    let (started_tx, started_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let owner = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        drop(store);
        done_tx.send(()).unwrap();
    });
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(Store::open(&data).is_err());
    assert!(crate::backup::create(&data, &root.path().join("early-backup")).is_err());
    assert!(done_rx.try_recv().is_err());
    gate.release();
    done_rx.recv_timeout(Duration::from_secs(20)).unwrap();
    owner.join().unwrap();
    let bytes = fs::read(data.join("index.sqlite")).unwrap();
    let archive = root.path().join("backup");
    let review = crate::backup::create(&data, &archive).unwrap();
    assert!(!review.manifest_digest.is_empty());
    assert_eq!(fs::read(data.join("index.sqlite")).unwrap(), bytes);
    let store = settled(&data);
    assert_eq!(store.snapshot().plans.len(), 2);
    assert!(store.index_error().is_none());
}

#[test]
fn bounded_pages_reject_missing_shifted_or_forged_rows_and_use_live_fallback() {
    let root = tempfile::tempdir().unwrap();
    let mut store = settled(root.path());
    for n in 0..75 {
        append(&mut store, n);
    }
    store.wait_for_index(Duration::from_secs(20)).unwrap();
    assert!(store
        .index
        .plans(&store.root, &store.index_head(), 0, 257)
        .is_err());
    assert!(store
        .index
        .plans(&store.root, &store.index_head(), 76, 1)
        .is_err());
    let original = fs::read(root.path().join("index.sqlite")).unwrap();
    for sql in [
        "UPDATE plans SET payload=json_set(payload,'$.approved',1), approved=1 WHERE ordinal=25",
        "DELETE FROM plans WHERE ordinal=25",
        "UPDATE plans SET ordinal=1000 WHERE ordinal=25",
        "UPDATE meta SET head=json_set(head,'$.count',9999)",
    ] {
        fs::write(root.path().join("index.sqlite"), &original).unwrap();
        let conn = Connection::open(root.path().join("index.sqlite")).unwrap();
        conn.execute_batch(sql).unwrap();
        drop(conn);
        assert!(store
            .index
            .plans(&store.root, &store.index_head(), 20, 20)
            .is_err());
        let first = store
            .workspace_plans(&WorkspaceQuery::Plans {
                offset: 0,
                revision: None,
            })
            .unwrap();
        let WorkspaceResult::Plans(first) = first else {
            panic!("first")
        };
        let next = store
            .workspace_plans(&WorkspaceQuery::Plans {
                offset: 20,
                revision: Some(first.revision),
            })
            .unwrap();
        let WorkspaceResult::Plans(next) = next else {
            panic!("next")
        };
        assert_eq!(next.entries.len(), 20);
        assert_eq!(next.entries[5].title, "plan 25");
        assert!(!next.entries[5].approved);
    }
}

#[test]
#[ignore = "explicit 1000-event local latency measurement; no model or network requests"]
fn thousand_event_coalescing_and_bounded_query_measurement() {
    let report = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports/sqlite-async-303");
    let root = tempfile::Builder::new()
        .prefix("large-ledger-")
        .tempdir_in(&report)
        .unwrap();
    let mut store = settled(root.path());
    let gate = store.index.worker.test_gate();
    append(&mut store, 0);
    gate.entered();
    let start = Instant::now();
    for n in 1..1000 {
        append(&mut store, n);
    }
    let append_ms = start.elapsed().as_millis();
    let pending = store.index.worker.test_stats();
    assert_eq!(pending.1, Some(1000));
    let flush = Instant::now();
    gate.release();
    store.wait_for_index(Duration::from_secs(60)).unwrap();
    let flush_ms = flush.elapsed().as_millis();
    let expected = store.index.ready(&store.index_head()).unwrap();
    let mut pages = Vec::new();
    let mut full = Vec::new();
    for _ in 0..5 {
        let t = Instant::now();
        let page: Vec<Plan> = read_page(&store.root, &expected, Table::Plans, 980, 20).unwrap();
        pages.push(t.elapsed().as_micros());
        assert_eq!(page[0].title, "plan 980");
        let t = Instant::now();
        let all = read_complete(&store.root, &expected).unwrap();
        full.push(t.elapsed().as_micros());
        assert_eq!(all.plans.len(), 1000);
    }
    let natural_root = tempfile::Builder::new()
        .prefix("natural-ledger-")
        .tempdir_in(&report)
        .unwrap();
    let mut natural = settled(natural_root.path());
    let natural_start = Instant::now();
    for n in 0..1000 {
        append(&mut natural, n);
    }
    let natural_append_ms = natural_start.elapsed().as_millis();
    natural.wait_for_index(Duration::from_secs(60)).unwrap();
    let natural_total_ms = natural_start.elapsed().as_millis();
    assert_eq!(natural.snapshot().plans.len(), 1000);
    assert!(natural.index_error().is_none());
    let data = serde_json::json!({"events":1000,"append999Milliseconds":append_ms,"finalBuildMilliseconds":flush_ms,"page20Microseconds":pages,"completeReadMicroseconds":full,"buildsStarted":store.index.worker.test_stats().0,"pendingSnapshotsMax":1,"controlledGate":true,"naturalAppend1000Milliseconds":natural_append_ms,"naturalTotalMilliseconds":natural_total_ms,"naturalBuildsStarted":natural.index.worker.test_stats().0,"physicalPowerLossVerified":false,"modelCalls":0});
    let path = report.join(format!("measurement-{}.json", std::process::id()));
    assert!(!path.exists());
    fs::write(&path, serde_json::to_vec_pretty(&data).unwrap()).unwrap();
    println!("INDEX_MEASUREMENT={}", path.display());
}
