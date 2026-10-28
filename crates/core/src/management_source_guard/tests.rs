use super::*;
use crate::{
    reader_generation::tests::{held, snapshot},
    Store,
};
pub(crate) fn promote_fixture(root: &Path) {
    let archive = root.with_file_name("management-source-cold-backup");
    let (manifest, hold) = held(root, &archive);
    let r = review(root, &archive, &manifest, &hold).unwrap();
    promote(root, &archive, &manifest, &hold, &r.digest).unwrap();
    crate::upgrade::release(root, &hold).unwrap();
}
#[test]
fn management_source_guard_preserves_prior_data_and_backup_across_reader_generations() {
    for generation in [1, 3, 12] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("data");
        drop(Store::open(&root).unwrap());
        if generation == 3 {
            crate::reader_generation::tests::promote_fixture(&root);
        }
        if generation == 12 {
            crate::currency_api_resolution_guard::tests::reader11(&root);
            crate::currency_api_resolution_guard::tests::promote_fixture(&root);
        }
        let archive = temp.path().join("new-cold-backup");
        let (manifest, hold) = held(&root, &archive);
        let before = snapshot(&root);
        let saved = snapshot(&archive);
        let original = crate::data_format::status(&root).unwrap();
        let r = review(&root, &archive, &manifest, &hold).unwrap();
        assert!(promote(&root, &archive, &manifest, &hold, &"0".repeat(64)).is_err());
        assert_eq!(snapshot(&root), before);
        let status = promote(&root, &archive, &manifest, &hold, &r.digest).unwrap();
        assert_eq!(status.minimum_reader_generation, 13);
        assert_eq!(status.marker_digest, original.marker_digest);
        let mut after = snapshot(&root);
        let bytes = after.remove(&root.join(MARKER)).unwrap();
        assert_eq!(after, before);
        assert_eq!(snapshot(&archive), saved);
        let guard: Guard = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(guard.source_reader_generation, generation);
        assert!(Store::open(&root).is_err());
        crate::upgrade::release(&root, &hold).unwrap();
        drop(Store::open(&root).unwrap());
    }
}
#[test]
fn management_source_guard_rejects_missing_protection_and_mutated_identity() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data");
    drop(Store::open(&root).unwrap());
    assert!(require_protected(&root, true).is_err());
    promote_fixture(&root);
    let original = fs::read(root.join(MARKER)).unwrap();
    for field in [
        "sourceReaderGeneration",
        "sourceReaderGuardDigest",
        "sourceBackupDigest",
        "root",
        "ownerSid",
    ] {
        let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
        value[field] = serde_json::json!("changed");
        fs::write(root.join(MARKER), serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(Store::open(&root).is_err(), "{field}");
    }
    fs::write(root.join(MARKER), &original).unwrap();
    fs::remove_file(root.join(MARKER)).unwrap();
    let record = root.join("management-decisions/example.0.json");
    fs::create_dir_all(record.parent().unwrap()).unwrap();
    for version in [1, 2] {
        fs::write(&record,serde_json::to_vec(&serde_json::json!({"body":{"version":version,"evidence":{"kind":"externalIntent"}}})).unwrap()).unwrap();
        let before = snapshot(&root);
        assert!(crate::reader_generation::reject_unprotected(&root)
            .unwrap_err()
            .contains("reader13"));
        assert_eq!(snapshot(&root), before);
    }
}
