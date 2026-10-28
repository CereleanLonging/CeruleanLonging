use super::*;
use crate::reader_generation::tests::{held, snapshot};

pub(crate) fn promote_fixture(root: &Path) {
    let archive = root.with_file_name("scoped-count-cold-backup");
    let (manifest, hold) = held(root, &archive);
    let r = review(root, &archive, &manifest, &hold).unwrap();
    promote(root, &archive, &manifest, &hold, &r.digest).unwrap();
    crate::upgrade::release(root, &hold).unwrap();
}

fn reader8(root: &Path) {
    drop(crate::Store::open(root).unwrap());
    crate::currency_guard::tests::promote_fixture(root);
    crate::currency_execution_guard::tests::promote_fixture(root);
    crate::currency_count_guard::tests::promote_fixture(root);
    crate::currency_count_resolution_guard::tests::promote_fixture(root);
    crate::currency_scoped_guard::tests::promote_fixture(root);
}

#[test]
fn scoped_count_guard_preserves_reader8_requires_review_and_keeps_hold() {
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
        crate::currency_count_guard::tests::promote_fixture(&data);
        let before7 = temp.path().join("before-reader8");
        let (manifest, hold) = held(&data, &before7);
        assert!(review(&data, &before7, &manifest, &hold).is_err());
        crate::upgrade::release(&data, &hold).unwrap();
        crate::currency_count_resolution_guard::tests::promote_fixture(&data);
        crate::currency_scoped_guard::tests::promote_fixture(&data);
        let backup = temp.path().join("before-reader9");
        let (manifest, hold) = held(&data, &backup);
        let original = snapshot(&data);
        let archived = snapshot(&backup);
        assert!(require_protected(&data, true).is_err());
        let r = review(&data, &backup, &manifest, &hold).unwrap();
        assert!(promote(&data, &backup, &manifest, &hold, &"a".repeat(64)).is_err());
        assert_eq!(snapshot(&data), original);
        let result = promote(&data, &backup, &manifest, &hold, &r.digest).unwrap();
        assert_eq!(result.minimum_reader_generation, 9);
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
fn scoped_count_guard_rejects_changed_reader8_binding_and_truncated_marker() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    reader8(&data);
    promote_fixture(&data);
    let path = data.join(MARKER);
    let original = fs::read(&path).unwrap();
    for (field, changed) in [
        ("schema", serde_json::json!(2)),
        ("minimumReaderGeneration", serde_json::json!(8)),
        ("dataSchema", serde_json::json!(2)),
        ("sourceMarkerDigest", serde_json::json!("0".repeat(64))),
        ("sourceReaderGeneration", serde_json::json!(7)),
        ("sourceReaderGuardDigest", serde_json::json!("0".repeat(64))),
        ("sourceBackupDigest", serde_json::json!("invalid")),
        ("root", serde_json::json!(temp.path().join("different"))),
        ("ownerSid", serde_json::json!("S-1-0-0")),
        ("promotedAt", serde_json::json!("invalid")),
    ] {
        let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
        value[field] = changed;
        fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        let before = snapshot(&data);
        assert!(crate::Store::open(&data).is_err());
        assert_eq!(before, snapshot(&data));
    }
    fs::write(&path, b"{").unwrap();
    assert!(crate::Store::open(&data).is_err());
    fs::write(&path, vec![b' '; 65_537]).unwrap();
    assert!(crate::Store::open(&data).is_err());
    fs::write(&path, original).unwrap();
    let prior = data.join(crate::currency_scoped_guard::MARKER);
    let mut changed = fs::read(&prior).unwrap();
    changed.push(b' ');
    fs::write(prior, changed).unwrap();
    assert!(crate::Store::open(&data).is_err());
}

#[test]
fn scoped_count_guard_unprotected_protocol_fields_refuse_before_store_write() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    reader8(&data);
    for relative in [
        "project-setups",
        "management-decisions",
        "currency-budgets/project",
        "workflows/project/events",
    ] {
        let folder = data.join(relative);
        fs::create_dir_all(&folder).unwrap();
        let path = folder.join("synthetic.json");
        for event in [
            serde_json::json!({"kind":"beginManagementCount","attempt":{"schema":2}}),
            serde_json::json!({"attempt":{"identity":{"request":{"requestedService":{}}}}}),
            serde_json::json!({"kind":"recordScopedManagementCount"}),
        ] {
            fs::write(
                &path,
                serde_json::to_vec(&serde_json::json!({"body":{"event":event}})).unwrap(),
            )
            .unwrap();
            let before = snapshot(&data);
            assert!(crate::reader_generation::reject_unprotected(&data)
                .unwrap_err()
                .contains("no reader9 guard"));
            assert!(crate::Store::open(&data).is_err());
            assert_eq!(before, snapshot(&data));
        }
        fs::remove_file(&path).unwrap();
    }
    let path = data.join("project-setups/synthetic.json");
    fs::write(
        &path,
        br#"{"body":{"event":{"kind":"recordScopedManagementCount"}}}"#,
    )
    .unwrap();
    let archive = temp.path().join("no-adoption");
    let (manifest, hold) = held(&data, &archive);
    let before = snapshot(&data);
    let backup = snapshot(&archive);
    assert!(review(&data, &archive, &manifest, &hold).is_err());
    assert_eq!(before, snapshot(&data));
    assert_eq!(backup, snapshot(&archive));
}
#[test]
fn scoped_count_guard_keeps_reader8_protocol_and_ignores_business_text() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    reader8(&data);
    let folder = data.join("project-setups");
    fs::create_dir_all(&folder).unwrap();
    let value = serde_json::json!({"setup":{"policy":{"currencyBudget":{"schema":2,"scopedQuotes":[]}}},
        "body":{"event":{"kind":"beginManagementCount","attempt":{"schema":1},"reservation":{"pricing":{}},"settlement":{"actualService":"default"}}},
        "business":{"body":{"event":{"kind":"recordScopedManagementCount","attempt":{"schema":2}}}}});
    fs::write(
        folder.join("synthetic.json"),
        serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    let before = snapshot(&data);
    crate::reader_generation::reject_unprotected(&data).unwrap();
    assert_eq!(before, snapshot(&data));
}

#[test]
fn scoped_count_guard_unprotected_pending_count_refuses_before_recovery() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    reader8(&data);
    let folder = data.join("currency-budgets/project");
    fs::create_dir_all(&folder).unwrap();
    let pending = folder.join("00000002.pending");
    fs::write(
        &pending,
        br#"{"body":{"event":{"kind":"recordScopedManagementCount"}}}"#,
    )
    .unwrap();
    let before = snapshot(&data);
    assert!(crate::Store::open(&data)
        .err()
        .unwrap()
        .contains("no reader9 guard"));
    assert_eq!(before, snapshot(&data));
}

#[test]
fn scoped_count_guard_preserves_incomplete_promotion_and_refuses_unknown_reader() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    reader8(&data);
    let pending = data.join("events/.format-migration-scoped-count-fixture.pending");
    fs::write(&pending, b"{").unwrap();
    let before = snapshot(&data);
    assert!(crate::Store::open(&data).is_err());
    assert_eq!(before, snapshot(&data));
    // Only this owned fixture removes its synthetic incomplete record.
    fs::remove_file(pending).unwrap();
    fs::write(data.join("events/00000000.reader10.json"), b"{}").unwrap();
    let before = snapshot(&data);
    assert!(crate::Store::open(&data).is_err());
    assert_eq!(before, snapshot(&data));
}

#[test]
#[ignore = "Owned reader9 CLI/window maintenance fixtures; no user data or models"]
fn export_reader9_maintenance_fixture() {
    let root = std::path::PathBuf::from(
        std::env::var_os("CERULEAN_SCOPED_COUNT_MAINTENANCE_FIXTURES").unwrap(),
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
