use super::*;
use crate::reader_generation::tests::{held, snapshot};

pub(crate) fn promote_fixture(root: &Path) {
    let archive = root.with_file_name("count-resolution-cold-backup");
    let (manifest, hold) = held(root, &archive);
    let r = review(root, &archive, &manifest, &hold).unwrap();
    promote(root, &archive, &manifest, &hold, &r.digest).unwrap();
    crate::upgrade::release(root, &hold).unwrap();
}

fn reader6(root: &Path) {
    drop(crate::Store::open(root).unwrap());
    crate::currency_guard::tests::promote_fixture(root);
    crate::currency_execution_guard::tests::promote_fixture(root);
    crate::currency_count_guard::tests::promote_fixture(root);
}

#[test]
fn count_resolution_guard_preserves_reader6_requires_review_and_keeps_hold() {
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
        crate::currency_execution_guard::tests::promote_fixture(&data);
        let before6 = temp.path().join("before-reader6");
        let (manifest, hold) = held(&data, &before6);
        assert!(review(&data, &before6, &manifest, &hold).is_err());
        crate::upgrade::release(&data, &hold).unwrap();
        crate::currency_count_guard::tests::promote_fixture(&data);
        let backup = temp.path().join("before-reader7");
        let (manifest, hold) = held(&data, &backup);
        let original = snapshot(&data);
        let archived = snapshot(&backup);
        assert!(require_protected(&data, true).is_err());
        let r = review(&data, &backup, &manifest, &hold).unwrap();
        assert!(promote(&data, &backup, &manifest, &hold, &"a".repeat(64)).is_err());
        assert_eq!(snapshot(&data), original);
        let result = promote(&data, &backup, &manifest, &hold, &r.digest).unwrap();
        assert_eq!(result.minimum_reader_generation, 7);
        assert_eq!(result.data_schema, schema);
        let mut after = snapshot(&data);
        assert!(after.remove(&data.join(MARKER)).is_some());
        assert_eq!(after, original);
        assert_eq!(snapshot(&backup), archived);
        assert!(crate::Store::open(&data).is_err());
        assert!(review(&data, &backup, &manifest, &hold).is_err());
        crate::upgrade::release(&data, &hold).unwrap();
        require_protected(&data, true).unwrap();
        drop(crate::Store::open(&data).unwrap());
    }
}

#[test]
fn count_resolution_guard_rejects_changed_reader6_binding_and_truncated_marker() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    reader6(&data);
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
    let prior = data.join(crate::currency_count_guard::MARKER);
    let mut changed = fs::read(&prior).unwrap();
    changed.push(b' ');
    fs::write(prior, changed).unwrap();
    assert!(crate::Store::open(&data).is_err());
}

#[test]
fn count_resolution_guard_without_marker_rejects_owned_protocol_fields_before_store_write() {
    let cases = [
        serde_json::json!({"body":{"context":{"countResolution":{}}}}),
        serde_json::json!({"body":{"event":{"command":{"kind":"confirm-management-count-resolution"}}}}),
        serde_json::json!({"body":{"event":{"kind":"resolveManagementCount"}}}),
    ];
    for value in cases {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        reader6(&data);
        let folder = data.join("workflows/project/events");
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join("00000001.json"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        let before = snapshot(&data);
        let error = match crate::Store::open(&data) {
            Ok(_) => panic!("unprotected count resolution accepted"),
            Err(e) => e,
        };
        assert!(error.contains("no reader7 guard"), "{error}");
        assert_eq!(before, snapshot(&data));
        let backup = temp.path().join("no-adoption");
        let (manifest, hold) = held(&data, &backup);
        assert!(review(&data, &backup, &manifest, &hold).is_err());
    }
}

#[test]
fn count_resolution_guard_ignores_arbitrary_business_json_and_preserves_reader6_count_fields() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    reader6(&data);
    let folder = data.join("project-setups");
    fs::create_dir_all(&folder).unwrap();
    let business = serde_json::json!({
        "setup":{"policy":{"currencyBudget":{"quotes":[{"countPrice":{}}]}}},
        "business":{"body":{"context":{"countResolution":{}},"event":{"kind":"resolveManagementCount","command":{"kind":"confirm-management-count-resolution"}}}}
    });
    fs::write(
        folder.join("synthetic.json"),
        serde_json::to_vec(&business).unwrap(),
    )
    .unwrap();
    let before = snapshot(&data);
    crate::reader_generation::reject_unprotected(&data).unwrap();
    assert_eq!(before, snapshot(&data));
}

#[test]
#[ignore = "Owned reader7 CLI/window maintenance fixtures; no user data or models"]
fn export_reader7_maintenance_fixture() {
    let root = std::path::PathBuf::from(
        std::env::var_os("CERULEAN_COUNT_RESOLUTION_MAINTENANCE_FIXTURES").unwrap(),
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
