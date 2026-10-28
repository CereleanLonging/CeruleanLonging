use super::*;
use crate::reader_generation::tests::{held, snapshot};

pub(crate) fn promote_fixture(root: &Path) {
    let archive = root.with_file_name("count-cold-backup");
    let (manifest, hold) = held(root, &archive);
    let r = review(root, &archive, &manifest, &hold).unwrap();
    promote(root, &archive, &manifest, &hold, &r.digest).unwrap();
    crate::upgrade::release(root, &hold).unwrap();
}
#[test]
fn count_guard_preserves_original_reader5_and_requires_backup_review_and_explicit_release() {
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
        crate::currency_guard::tests::promote_fixture(&data);
        let backup = temp.path().join("before-reader5");
        let (manifest, hold) = held(&data, &backup);
        assert!(review(&data, &backup, &manifest, &hold).is_err());
        crate::upgrade::release(&data, &hold).unwrap();
        crate::currency_execution_guard::tests::promote_fixture(&data);
        let backup = temp.path().join("before-reader6");
        let (manifest, hold) = held(&data, &backup);
        let before = snapshot(&data);
        let archived = snapshot(&backup);
        let r = review(&data, &backup, &manifest, &hold).unwrap();
        assert!(promote(&data, &backup, &manifest, &hold, &"a".repeat(64)).is_err());
        assert_eq!(snapshot(&data), before);
        let status = promote(&data, &backup, &manifest, &hold, &r.digest).unwrap();
        assert_eq!(status.minimum_reader_generation, 6);
        assert_eq!(status.data_schema, schema);
        let mut after = snapshot(&data);
        assert!(after.remove(&data.join(MARKER)).is_some());
        assert_eq!(before, after);
        assert_eq!(snapshot(&backup), archived);
        assert!(crate::Store::open(&data).is_err());
        assert!(review(&data, &backup, &manifest, &hold).is_err());
        crate::upgrade::release(&data, &hold).unwrap();
        drop(crate::Store::open(&data).unwrap());
    }
}
#[test]
fn count_guard_rejects_changed_reader5_origin_and_truncated_marker() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    drop(crate::Store::open(&data).unwrap());
    crate::currency_guard::tests::promote_fixture(&data);
    crate::currency_execution_guard::tests::promote_fixture(&data);
    promote_fixture(&data);
    let path = data.join(MARKER);
    let original = fs::read(&path).unwrap();
    for field in [
        "sourceReaderGeneration",
        "sourceReaderGuardDigest",
        "sourceBackupDigest",
        "root",
        "ownerSid",
    ] {
        let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
        value[field] = serde_json::json!("changed");
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(crate::Store::open(&data).is_err());
    }
    fs::write(&path, b"{").unwrap();
    assert!(crate::Store::open(&data).is_err());
    fs::write(&path, original).unwrap();
    let prior = data.join(crate::currency_execution_guard::MARKER);
    let mut changed = fs::read(&prior).unwrap();
    changed.push(b' ');
    fs::write(prior, changed).unwrap();
    assert!(crate::Store::open(&data).is_err());
}

#[test]
#[ignore = "Owned reader6 CLI/window maintenance fixtures; no user data or models"]
fn export_reader6_maintenance_fixture() {
    let root =
        std::path::PathBuf::from(std::env::var_os("CERULEAN_COUNT_MAINTENANCE_FIXTURES").unwrap());
    assert!(!root.exists());
    fs::create_dir_all(&root).unwrap();
    let mut cases = vec![];
    for schema in [1, 2] {
        for mode in ["cli", "window"] {
            let folder = root.join(format!("{mode}-{schema}"));
            let data = folder.join("data");
            drop(crate::Store::open(&data).unwrap());
            if schema == 2 {
                let backup = folder.join("schema-backup");
                let (manifest, hold) = held(&data, &backup);
                let review = crate::data_format::review(&data, &backup, &manifest, &hold).unwrap();
                crate::data_format::migrate(&data, &backup, &manifest, &hold, &review.digest)
                    .unwrap();
                crate::upgrade::release(&data, &hold).unwrap();
            }
            crate::currency_guard::tests::promote_fixture(&data);
            crate::currency_execution_guard::tests::promote_fixture(&data);
            let archive = folder.join("cold-backup");
            let (manifest, hold) = held(&data, &archive);
            cases.push(serde_json::json!({"mode":mode,"schema":schema,"data":data,"backup":archive,"manifest":manifest,"hold":hold}));
        }
    }
    fs::write(
        root.join("cases.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}
