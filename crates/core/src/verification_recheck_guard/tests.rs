use super::*;
use crate::{
    reader_generation::tests::{held, snapshot},
    Store,
};
use serde_json::json;

pub(crate) fn promote_fixture(root: &Path, archive: &Path) {
    let manifest = crate::backup::create(root, archive)
        .unwrap()
        .manifest_digest;
    let review = review(root, archive, &manifest).unwrap();
    promote(root, archive, &manifest, &review.digest).unwrap();
}

#[test]
fn recheck_guard_cold_promotion_preserves_reader17_and_requires_original_backup() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data");
    drop(Store::open(&root).unwrap());
    crate::rejected_usage_guard::tests::promote_fixture(&root, &temp.path().join("before17"));
    let archive = temp.path().join("before18");
    let manifest = crate::backup::create(&root, &archive)
        .unwrap()
        .manifest_digest;
    let before = snapshot(&root);
    let saved = snapshot(&archive);
    let review = review(&root, &archive, &manifest).unwrap();
    assert!(require_protected(&root, true).is_err());
    assert!(promote(&root, &archive, &manifest, &"0".repeat(64)).is_err());
    assert_eq!(snapshot(&root), before);
    let writer = Store::open(&root).unwrap();
    assert!(promote(&root, &archive, &manifest, &review.digest).is_err());
    drop(writer);
    assert_eq!(
        promote(&root, &archive, &manifest, &review.digest)
            .unwrap()
            .minimum_reader_generation,
        18
    );
    let mut after = snapshot(&root);
    let guard: Guard = serde_json::from_slice(&after.remove(&root.join(MARKER)).unwrap()).unwrap();
    assert_eq!(guard.source_reader_generation, 17);
    assert_eq!(after, before);
    assert_eq!(snapshot(&archive), saved);
    assert!(promote(&root, &archive, &manifest, &review.digest).is_err());
    require_protected(&root, true).unwrap();
    drop(Store::open(&root).unwrap());
    let bytes = fs::read(root.join(MARKER)).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    value["sourceReaderGuardDigest"] = json!("0".repeat(64));
    fs::write(root.join(MARKER), serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(Store::open(&root).is_err());
}

#[test]
fn recheck_guard_held_promotion_cannot_release_or_bypass_original_hold() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data");
    drop(Store::open(&root).unwrap());
    let archive = temp.path().join("cold");
    let (manifest, hold) = held(&root, &archive);
    let before = snapshot(&root);
    assert!(review(&root, &archive, &manifest).is_err());
    assert!(review_held(&root, &archive, &manifest, &"0".repeat(64)).is_err());
    let r = review_held(&root, &archive, &manifest, &hold).unwrap();
    assert!(promote_held(&root, &archive, &manifest, &hold, &"0".repeat(64)).is_err());
    assert_eq!(snapshot(&root), before);
    promote_held(&root, &archive, &manifest, &hold, &r.digest).unwrap();
    let mut after = snapshot(&root);
    after.remove(&root.join(MARKER));
    assert_eq!(after, before);
    assert_eq!(crate::upgrade::status(&root).unwrap().hold_digest, hold);
    assert!(Store::open(&root).is_err());
    crate::upgrade::release(&root, &hold).unwrap();
    drop(Store::open(&root).unwrap());
    assert_eq!(
        crate::data_format::status(&root)
            .unwrap()
            .minimum_reader_generation,
        18
    );
}

#[test]
fn recheck_guard_preflight_recognizes_protocol_without_interpreting_task_text() {
    let protocol = [
        json!({"body":{"event":{"command":{"kind":"approve-verification-recheck"}}}}),
        json!({"body":{"context":{"verificationRecheck":{"schemaVersion":1}}}}),
        json!({"body":{"context":{"execution":{"kind":"verification-recheck"}}}}),
    ];
    for value in protocol {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("data");
        drop(Store::open(&root).unwrap());
        let events = root.join("workflows/project/events");
        fs::create_dir_all(&events).unwrap();
        let event = events.join("1.json");
        fs::write(
            &event,
            json!({"body":{"context":{"execution":{"kind":"receipt","receipt":{
                "summary":"approve-verification-recheck","business":{"verificationRecheck":true}
            }}}}})
            .to_string(),
        )
        .unwrap();
        reject_unprotected(&root).unwrap();
        fs::write(event, value.to_string()).unwrap();
        let before = snapshot(&root);
        let error = Store::open(&root)
            .err()
            .expect("unprotected evidence must not open");
        assert!(error.contains("reader18"), "{error}");
        assert_eq!(snapshot(&root), before);
        let archive = temp.path().join("cold");
        let manifest = crate::backup::create(&root, &archive)
            .unwrap()
            .manifest_digest;
        assert!(review(&root, &archive, &manifest).is_err());
        assert_eq!(snapshot(&root), before);
    }
}

#[test]
fn recheck_guard_rejects_unprotected_job_storage_and_incomplete_events() {
    for mode in ["jobs", "partial"] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("data");
        drop(Store::open(&root).unwrap());
        if mode == "jobs" {
            fs::create_dir(root.join("verification-recheck-jobs")).unwrap();
        } else {
            let events = root.join("workflows/project/events");
            fs::create_dir_all(&events).unwrap();
            fs::write(events.join("check.pending"), "{").unwrap();
        }
        let before = snapshot(&root);
        assert!(Store::open(&root).is_err());
        assert_eq!(snapshot(&root), before);
    }
}

#[test]
fn recheck_guard_rejects_changed_backup_and_foreign_root_without_writing() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data");
    drop(Store::open(&root).unwrap());
    let archive = temp.path().join("cold");
    let manifest = crate::backup::create(&root, &archive)
        .unwrap()
        .manifest_digest;
    let r = review(&root, &archive, &manifest).unwrap();
    fs::write(root.join("changed.txt"), "changed since backup").unwrap();
    let before = snapshot(&root);
    assert!(promote(&root, &archive, &manifest, &r.digest).is_err());
    assert_eq!(snapshot(&root), before);
    let other = temp.path().join("other");
    drop(Store::open(&other).unwrap());
    assert!(review(&other, &archive, &manifest).is_err());
}

#[test]
#[ignore = "explicit new-only fixture for testing the frozen reader17 binary"]
fn recheck_guard_export_for_frozen_reader17() {
    let root = std::path::PathBuf::from(
        std::env::var_os("CERULEAN_RECHECK_READER_FIXTURE").expect("new fixture path"),
    );
    assert!(!root.exists());
    drop(Store::open(&root).unwrap());
    promote_fixture(&root, &root.with_file_name("before-reader18"));
    drop(Store::open(&root).unwrap());
}
