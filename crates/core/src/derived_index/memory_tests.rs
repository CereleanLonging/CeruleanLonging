use super::*;
use memory::{Budget, Encoded};
use std::{sync::Arc, time::Duration};
use worker::{pool::Pool, Worker};

fn store(root: &Path, pool: &Arc<Pool>) -> crate::Store {
    let mut s = crate::Store::open(root).unwrap();
    s.wait_for_index(Duration::from_secs(20)).unwrap();
    s.index = Cache {
        worker: Worker::with_pool(pool.clone()),
        ..Default::default()
    };
    s
}
fn append(s: &mut crate::Store, n: usize) {
    s.execute_command(cerulean_domain::Command::CreatePlan {
        title: format!("retained-{n}"),
        tasks: vec!["local memory boundary".into()],
    })
    .unwrap();
}

#[test]
fn encoded_capacity_is_reserved_until_decode_drop_and_overflow_stops_serializing() {
    use serde::ser::SerializeSeq;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Large<'a>(&'a AtomicUsize);
    impl Serialize for Large<'_> {
        fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
            let mut seq = s.serialize_seq(Some(10000))?;
            for _ in 0..10000 {
                self.0.fetch_add(1, Ordering::Relaxed);
                seq.serialize_element(&"x".repeat(1024))?;
            }
            seq.end()
        }
    }
    let budget = Budget::new(128 * 1024);
    let bytes = Encoded::new(&"x".repeat(70_000), &budget, 100_000).unwrap();
    assert!(budget.stats().0 >= bytes.as_slice().len());
    let value: String = bytes.decode().unwrap();
    assert_eq!(value.len(), 70_000);
    assert_eq!(budget.stats().0, 0);
    let count = AtomicUsize::new(0);
    assert!(Encoded::new(&Large(&count), &budget, 96 * 1024).is_err());
    assert!(count.load(Ordering::Relaxed) < 100);
    assert_eq!(budget.stats().0, 0);
    assert!(budget.stats().1 <= 128 * 1024);
    let invalid = Encoded::new(&"string", &budget, 64 * 1024).unwrap();
    assert!(invalid.decode::<u64>().is_err());
    assert_eq!(budget.stats().0, 0);
}

#[test]
fn byte_saturation_preserves_commits_and_recovery_does_not_append_or_replay() {
    let root = tempfile::tempdir().unwrap();
    let pool = Arc::new(Pool::with_bytes(1, 8, 2 * 65536 + 8192));
    let mut a = store(&root.path().join("a"), &pool);
    let mut b = store(&root.path().join("b"), &pool);
    let mut c = store(&root.path().join("c"), &pool);
    let gate = a.index.test_gate();
    append(&mut a, 1);
    gate.entered();
    append(&mut b, 2);
    append(&mut c, 3);
    assert_eq!(c.events.len(), 1);
    assert_eq!(c.snapshot().plans.len(), 1);
    gate.release();
    a.wait_for_index(Duration::from_secs(20)).unwrap();
    b.wait_for_index(Duration::from_secs(20)).unwrap();
    assert!(c.wait_for_index(Duration::from_secs(20)).is_err());
    assert!(c.index_error().unwrap().contains("memory capacity"));
    let before = fs::read(c.root.join("events/00000001.json")).unwrap();
    c.refresh_index();
    c.wait_for_index(Duration::from_secs(20)).unwrap();
    assert_eq!(
        fs::read(c.root.join("events/00000001.json")).unwrap(),
        before
    );
    assert_eq!(c.events.len(), 1);
    assert!(pool.budget().stats().1 <= 2 * 65536 + 8192);
    drop((a, b, c));
    pool.wait_idle();
    assert_eq!(pool.budget().stats().0, 0);
}

#[test]
fn replacement_reclaims_pending_views_before_encoding_latest_and_proofs_keep_a_lease() {
    let root = tempfile::tempdir().unwrap();
    let pool = Arc::new(Pool::with_bytes(1, 8, 4 * 65536));
    let mut s = store(root.path(), &pool);
    let gate = s.index.test_gate();
    append(&mut s, 0);
    gate.entered();
    for n in 1..180 {
        append(&mut s, n);
    }
    assert_eq!(s.events.len(), 180);
    assert!(pool.budget().stats().1 <= 4 * 65536);
    gate.release();
    s.wait_for_index(Duration::from_secs(20)).unwrap();
    let proof = s.index.ready(&s.index_head()).unwrap();
    drop(s);
    pool.wait_idle();
    assert!(
        pool.budget().stats().0 > 0,
        "reader must retain the proof lease"
    );
    drop(proof);
    assert_eq!(pool.budget().stats().0, 0);
}

#[test]
fn panic_and_queue_rejection_return_all_owned_encoded_capacity() {
    let root = tempfile::tempdir().unwrap();
    let pool = Arc::new(Pool::with_bytes(1, 1, 512 * 1024));
    let mut a = store(&root.path().join("a"), &pool);
    let mut b = store(&root.path().join("b"), &pool);
    let gate = a.index.test_gate();
    append(&mut a, 0);
    gate.entered();
    append(&mut b, 1);
    assert!(b.wait_for_index(Duration::ZERO).is_err());
    assert_eq!(pool.budget().stats().0, 65536);
    a.index.worker.panic_next();
    append(&mut a, 2);
    gate.release();
    assert!(a.wait_for_index(Duration::from_secs(20)).is_err());
    drop((a, b));
    pool.wait_idle();
    assert_eq!(pool.budget().stats().0, 0);
}

#[test]
fn streaming_index_fingerprint_matches_original_json_bytes() {
    let root = tempfile::tempdir().unwrap();
    let mut s = crate::Store::open(root.path()).unwrap();
    append(&mut s, 0);
    s.wait_for_index(Duration::from_secs(20)).unwrap();
    let data = s.index.read(&s.root, &s.index_head()).unwrap();
    assert_eq!(
        fingerprint(&data).unwrap(),
        format!("{:x}", Sha256::digest(json(&data).unwrap().as_bytes()))
    );
}
