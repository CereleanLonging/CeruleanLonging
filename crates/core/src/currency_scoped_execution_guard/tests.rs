use super::*;
use crate::reader_generation::tests::{held, snapshot};
pub(crate) fn promote_fixture(root: &Path) {
    let archive = root.with_file_name("scoped-execution-cold-backup");
    let (manifest, hold) = held(root, &archive);
    let r = review(root, &archive, &manifest, &hold).unwrap();
    promote(root, &archive, &manifest, &hold, &r.digest).unwrap();
    crate::upgrade::release(root, &hold).unwrap();
}
fn reader9(root: &Path) {
    drop(crate::Store::open(root).unwrap());
    crate::currency_guard::tests::promote_fixture(root);
    crate::currency_execution_guard::tests::promote_fixture(root);
    crate::currency_count_guard::tests::promote_fixture(root);
    crate::currency_count_resolution_guard::tests::promote_fixture(root);
    crate::currency_scoped_guard::tests::promote_fixture(root);
    crate::currency_scoped_count_guard::tests::promote_fixture(root);
}
#[test]
fn scoped_execution_guard_original_review_and_hold_preserve_both_containers() {
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
        reader9(&data);
        let backup = temp.path().join("before-reader10");
        let (manifest, hold) = held(&data, &backup);
        let original = snapshot(&data);
        let archived = snapshot(&backup);
        let r = review(&data, &backup, &manifest, &hold).unwrap();
        assert!(promote(&data, &backup, &manifest, &hold, &"a".repeat(64)).is_err());
        assert_eq!(snapshot(&data), original);
        let result = promote(&data, &backup, &manifest, &hold, &r.digest).unwrap();
        assert_eq!(result.minimum_reader_generation, 10);
        assert_eq!(result.data_schema, schema);
        let mut after = snapshot(&data);
        assert!(after.remove(&data.join(MARKER)).is_some());
        assert_eq!(after, original);
        assert_eq!(snapshot(&backup), archived);
        assert!(crate::Store::open(&data).is_err());
        assert!(review(&data, &backup, &manifest, &hold).is_err());
        crate::upgrade::release(&data, &hold).unwrap();
        drop(crate::Store::open(&data).unwrap());
    }
}
#[test]
fn scoped_execution_guard_owned_fields_and_pending_precede_store_recovery() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    reader9(&data);
    let dir = data.join("management-decisions");
    fs::create_dir_all(&dir).unwrap();
    for suffix in ["json", "pending"] {
        let path = dir.join(format!("x.0.{suffix}"));
        for value in [
            serde_json::json!({"body":{"event":{"scopedPreparation":{}}}}),
            serde_json::json!({"body":{"evidence":{"scoped":{}}}}),
            serde_json::json!({"body":{"evidence":{"currencyAck":{}}}}),
            serde_json::json!({"body":{"evidence":{"kind":"scopedResponse"}}}),
        ] {
            fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
            let before = snapshot(&data);
            assert!(crate::Store::open(&data).is_err());
            assert_eq!(snapshot(&data), before);
        }
        fs::remove_file(path).unwrap();
    }
    fs::write(dir.join("old.0.pending"), b"{").unwrap();
    crate::reader_generation::reject_unprotected(&data).unwrap();
    fs::write(
        dir.join("business.json"),
        br#"{"text":"scopedPreparation currencyAck scopedResponse"}"#,
    )
    .unwrap();
    crate::reader_generation::reject_unprotected(&data).unwrap();
}
#[test]
fn scoped_execution_guard_binds_original_reader_path_owner_and_refuses_future() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    reader9(&data);
    promote_fixture(&data);
    let path = data.join(MARKER);
    let bytes = fs::read(&path).unwrap();
    for (field, value) in [
        ("sourceReaderGeneration", serde_json::json!(8)),
        ("minimumReaderGeneration", serde_json::json!(11)),
        ("sourceReaderGuardDigest", serde_json::json!("a".repeat(64))),
        ("ownerSid", serde_json::json!("S-1-0-0")),
        ("root", serde_json::json!(temp.path().join("other"))),
    ] {
        let mut v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        v[field] = value;
        fs::write(&path, serde_json::to_vec(&v).unwrap()).unwrap();
        let before = snapshot(&data);
        assert!(crate::Store::open(&data).is_err());
        assert_eq!(snapshot(&data), before);
    }
    fs::write(&path, bytes).unwrap();
    fs::write(data.join("events/00000000.reader11.json"), b"{}").unwrap();
    assert!(crate::Store::open(&data).is_err());
}

#[test]
#[ignore = "Owned reader10 CLI/window maintenance fixtures; no user data or models"]
fn export_reader10_maintenance_fixture() {
    let root = std::path::PathBuf::from(
        std::env::var_os("CERULEAN_SCOPED_EXECUTION_MAINTENANCE_FIXTURES").unwrap(),
    );
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
                let r = crate::data_format::review(&data, &backup, &manifest, &hold).unwrap();
                crate::data_format::migrate(&data, &backup, &manifest, &hold, &r.digest).unwrap();
                crate::upgrade::release(&data, &hold).unwrap();
            }
            crate::currency_guard::tests::promote_fixture(&data);
            crate::currency_execution_guard::tests::promote_fixture(&data);
            crate::currency_count_guard::tests::promote_fixture(&data);
            crate::currency_count_resolution_guard::tests::promote_fixture(&data);
            crate::currency_scoped_guard::tests::promote_fixture(&data);
            crate::currency_scoped_count_guard::tests::promote_fixture(&data);
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
