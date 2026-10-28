use super::*;
use crate::{reader_generation::tests::snapshot, Store};
use serde_json::json;

pub(crate) fn promote_fixture(root: &Path, archive: &Path) {
    let manifest = crate::backup::create(root, archive)
        .unwrap()
        .manifest_digest;
    let review = review(root, archive, &manifest).unwrap();
    promote(root, archive, &manifest, &review.digest).unwrap();
}

#[test]
fn rejected_usage_held_promotion_binds_hold_and_preserves_original_until_explicit_release() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data");
    drop(Store::open(&root).unwrap());
    let archive = temp.path().join("cold");
    let (manifest, hold) = crate::reader_generation::tests::held(&root, &archive);
    let before = snapshot(&root);
    assert!(review(&root, &archive, &manifest).is_err());
    assert!(review_held(&root, &archive, &manifest, &"0".repeat(64)).is_err());
    let reviewed = review_held(&root, &archive, &manifest, &hold).unwrap();
    assert_eq!(reviewed.source_hold_digest.as_deref(), Some(hold.as_str()));
    assert!(promote_held(&root, &archive, &manifest, &hold, &"0".repeat(64)).is_err());
    let writer = crate::backup::open(&root.join("writer.lock"), false, true).unwrap();
    assert!(promote_held(&root, &archive, &manifest, &hold, &reviewed.digest).is_err());
    drop(writer);
    assert_eq!(snapshot(&root), before);
    promote_held(&root, &archive, &manifest, &hold, &reviewed.digest).unwrap();
    let mut after = snapshot(&root);
    assert!(after.remove(&root.join(MARKER)).is_some());
    assert_eq!(after, before);
    assert_eq!(crate::upgrade::status(&root).unwrap().hold_digest, hold);
    assert!(Store::open(&root).is_err());
    assert!(promote_held(&root, &archive, &manifest, &hold, &reviewed.digest).is_err());
    crate::upgrade::release(&root, &hold).unwrap();
    drop(Store::open(&root).unwrap());
    assert_eq!(
        crate::data_format::status(&root)
            .unwrap()
            .minimum_reader_generation,
        17
    );
}

#[test]
fn rejected_usage_cold_review_does_not_gain_a_hold_or_accept_a_held_command() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data");
    drop(Store::open(&root).unwrap());
    let archive = temp.path().join("cold");
    let manifest = crate::backup::create(&root, &archive)
        .unwrap()
        .manifest_digest;
    let before = snapshot(&root);
    let reviewed = review(&root, &archive, &manifest).unwrap();
    assert!(serde_json::to_value(&reviewed)
        .unwrap()
        .get("sourceHoldDigest")
        .is_none());
    assert!(review_held(&root, &archive, &manifest, &"0".repeat(64)).is_err());
    assert_eq!(snapshot(&root), before);
    promote(&root, &archive, &manifest, &reviewed.digest).unwrap();
    drop(Store::open(&root).unwrap());
}

#[test]
fn rejected_usage_guard_promotion_requires_original_backup_and_preserves_prior_bytes() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data");
    drop(Store::open(&root).unwrap());
    crate::relay_guard::tests::promote_fixture(&root, &temp.path().join("before16"));
    let archive = temp.path().join("before17");
    let manifest = crate::backup::create(&root, &archive)
        .unwrap()
        .manifest_digest;
    let before = snapshot(&root);
    let saved = snapshot(&archive);
    let review = review(&root, &archive, &manifest).unwrap();
    assert!(promote(&root, &archive, &manifest, &"0".repeat(64)).is_err());
    assert_eq!(snapshot(&root), before);
    let owner = Store::open(&root).unwrap();
    assert!(promote(&root, &archive, &manifest, &review.digest).is_err());
    drop(owner);
    assert_eq!(
        promote(&root, &archive, &manifest, &review.digest)
            .unwrap()
            .minimum_reader_generation,
        17
    );
    let mut after = snapshot(&root);
    let guard: Guard = serde_json::from_slice(&after.remove(&root.join(MARKER)).unwrap()).unwrap();
    assert_eq!(guard.source_reader_generation, 16);
    assert_eq!(after, before);
    assert_eq!(snapshot(&archive), saved);
    assert!(promote(&root, &archive, &manifest, &review.digest).is_err());
    drop(Store::open(&root).unwrap());
    assert_eq!(
        crate::data_format::status(&root)
            .unwrap()
            .minimum_reader_generation,
        17
    );
    let bytes = fs::read(root.join(MARKER)).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    value["sourceReaderGuardDigest"] = json!("f".repeat(64));
    fs::write(root.join(MARKER), serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(Store::open(&root).is_err());
}

#[test]
fn rejected_usage_guard_preflight_refuses_unprotected_records_without_promoting_them() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data");
    drop(Store::open(&root).unwrap());
    let round = root.join("workflows/project/api-attempt-fixture/rounds/001");
    fs::create_dir_all(&round).unwrap();
    fs::write(round.join("01.json"),json!({"body":{"event":{"kind":"outcome","outcome":{"observation":{"toolCalls":[{"arguments":{"rejected_usage":"ordinary task text"}}]}}}}}).to_string()).unwrap();
    reject_unprotected(&root).unwrap();
    fs::write(
        round.join("01.json"),
        json!({"body":{"event":{"kind":"outcome","rejected_usage":{"schema":1}}}}).to_string(),
    )
    .unwrap();
    let before = snapshot(&root);
    let error = match Store::open(&root) {
        Err(e) => e,
        Ok(_) => panic!("unprotected data opened"),
    };
    assert!(error.contains("reader17"), "{error}");
    assert_eq!(snapshot(&root), before);
    let archive = temp.path().join("cold");
    let manifest = crate::backup::create(&root, &archive)
        .unwrap()
        .manifest_digest;
    assert!(review(&root, &archive, &manifest).is_err());
    assert_eq!(snapshot(&root), before);
}

#[test]
fn rejected_usage_guard_stale_backup_and_foreign_root_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data");
    drop(Store::open(&root).unwrap());
    let archive = temp.path().join("cold");
    let manifest = crate::backup::create(&root, &archive)
        .unwrap()
        .manifest_digest;
    let reviewed = review(&root, &archive, &manifest).unwrap();
    fs::write(root.join("changed.txt"), b"changed after backup").unwrap();
    let before = snapshot(&root);
    assert!(promote(&root, &archive, &manifest, &reviewed.digest).is_err());
    assert_eq!(snapshot(&root), before);
    let other = temp.path().join("other");
    drop(Store::open(&other).unwrap());
    assert!(review(&other, &archive, &manifest).is_err());
}

#[test]
fn rejected_usage_guard_prepare_original_fixture_for_frozen_reader() {
    let Some(path) = std::env::var_os("CL544_OLD_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(path);
    assert!(!root.exists(), "owned compatibility fixture must be new");
    drop(Store::open(&root).unwrap());
    promote_fixture(&root, &root.with_file_name("reader17-before"));
    assert_eq!(
        crate::data_format::status(&root)
            .unwrap()
            .minimum_reader_generation,
        17
    );
    drop(Store::open(&root).unwrap());
}
