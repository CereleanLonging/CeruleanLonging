use super::*;
use crate::reader_generation::tests::{held, snapshot};

pub(crate) fn promote_fixture(root: &Path) {
    let archive = root.with_file_name("preparation-cold-backup");
    let (manifest, hold) = held(root, &archive);
    let r = review(root, &archive, &manifest, &hold).unwrap();
    promote(root, &archive, &manifest, &hold, &r.digest).unwrap();
    crate::upgrade::release(root, &hold).unwrap();
}

#[test]
fn preparation_guard_requires_reader4_backup_exact_review_and_explicit_release() {
    for schema in [1, 2] {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        drop(crate::Store::open(&data).unwrap());
        if schema == 2 {
            let backup = temp.path().join("schema-backup");
            let (manifest, hold) = held(&data, &backup);
            let r = crate::data_format::review(&data, &backup, &manifest, &hold).unwrap();
            crate::data_format::migrate(&data, &backup, &manifest, &hold, &r.digest).unwrap();
            crate::upgrade::release(&data, &hold).unwrap();
        }
        let before_backup = temp.path().join("before-reader4");
        let (manifest, hold) = held(&data, &before_backup);
        assert!(review(&data, &before_backup, &manifest, &hold).is_err());
        crate::upgrade::release(&data, &hold).unwrap();
        crate::currency_guard::tests::promote_fixture(&data);
        let backup = temp.path().join("before-reader5");
        let (manifest, hold) = held(&data, &backup);
        let before = snapshot(&data);
        let archived = snapshot(&backup);
        let r = review(&data, &backup, &manifest, &hold).unwrap();
        assert!(promote(&data, &backup, &manifest, &hold, &"a".repeat(64)).is_err());
        assert_eq!(snapshot(&data), before);
        let status = promote(&data, &backup, &manifest, &hold, &r.digest).unwrap();
        assert_eq!(status.minimum_reader_generation, 5);
        assert_eq!(status.data_schema, schema);
        let mut after = snapshot(&data);
        assert!(after.remove(&data.join(MARKER)).is_some());
        assert_eq!(after, before);
        assert_eq!(snapshot(&backup), archived);
        assert!(crate::Store::open(&data).is_err());
        assert!(review(&data, &backup, &manifest, &hold).is_err());
        crate::upgrade::release(&data, &hold).unwrap();
        drop(crate::Store::open(&data).unwrap());
    }
}

#[test]
fn preparation_guard_rejects_changed_origin_and_truncated_records() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    drop(crate::Store::open(&data).unwrap());
    crate::currency_guard::tests::promote_fixture(&data);
    promote_fixture(&data);
    let path = data.join(MARKER);
    let bytes = fs::read(&path).unwrap();
    for field in [
        "sourceReaderGeneration",
        "sourceReaderGuardDigest",
        "sourceBackupDigest",
        "ownerSid",
        "root",
    ] {
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        value[field] = serde_json::json!("changed");
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(crate::Store::open(&data).is_err());
    }
    fs::write(&path, b"{").unwrap();
    assert!(crate::Store::open(&data).is_err());
    fs::write(&path, bytes).unwrap();
    let prior = data.join(crate::currency_guard::MARKER);
    let mut bytes = fs::read(&prior).unwrap();
    bytes.push(b' ');
    fs::write(prior, bytes).unwrap();
    assert!(crate::Store::open(&data).is_err());
}
