use super::*;
use crate::reader_generation::tests::{held, snapshot};

pub(crate) fn promote_fixture(root: &Path) {
    let archive = root.with_file_name("scoped-pricing-cold-backup");
    let (manifest, hold) = held(root, &archive);
    let r = review(root, &archive, &manifest, &hold).unwrap();
    promote(root, &archive, &manifest, &hold, &r.digest).unwrap();
    crate::upgrade::release(root, &hold).unwrap();
}

fn reader7(root: &Path) {
    drop(crate::Store::open(root).unwrap());
    crate::currency_guard::tests::promote_fixture(root);
    crate::currency_execution_guard::tests::promote_fixture(root);
    crate::currency_count_guard::tests::promote_fixture(root);
    crate::currency_count_resolution_guard::tests::promote_fixture(root);
}

#[test]
fn scoped_pricing_guard_preserves_reader7_requires_review_and_keeps_hold() {
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
        let before7 = temp.path().join("before-reader7");
        let (manifest, hold) = held(&data, &before7);
        assert!(review(&data, &before7, &manifest, &hold).is_err());
        crate::upgrade::release(&data, &hold).unwrap();
        crate::currency_count_resolution_guard::tests::promote_fixture(&data);
        let backup = temp.path().join("before-reader8");
        let (manifest, hold) = held(&data, &backup);
        let original = snapshot(&data);
        let archived = snapshot(&backup);
        assert!(require_protected(&data, true).is_err());
        let r = review(&data, &backup, &manifest, &hold).unwrap();
        assert!(promote(&data, &backup, &manifest, &hold, &"a".repeat(64)).is_err());
        assert_eq!(snapshot(&data), original);
        let result = promote(&data, &backup, &manifest, &hold, &r.digest).unwrap();
        assert_eq!(result.minimum_reader_generation, 8);
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
fn scoped_pricing_guard_rejects_changed_reader7_binding_and_truncated_marker() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    reader7(&data);
    promote_fixture(&data);
    let path = data.join(MARKER);
    let original = fs::read(&path).unwrap();
    for (field, changed) in [
        ("schema", serde_json::json!(2)),
        ("minimumReaderGeneration", serde_json::json!(7)),
        ("dataSchema", serde_json::json!(2)),
        ("sourceMarkerDigest", serde_json::json!("0".repeat(64))),
        ("sourceReaderGeneration", serde_json::json!(6)),
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
    let prior = data.join(crate::currency_count_resolution_guard::MARKER);
    let mut changed = fs::read(&prior).unwrap();
    changed.push(b' ');
    fs::write(prior, changed).unwrap();
    assert!(crate::Store::open(&data).is_err());
}

fn at_pointer(pointer: &str, value: serde_json::Value) -> serde_json::Value {
    pointer
        .split('/')
        .skip(1)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .fold(value, |nested, key| serde_json::json!({key:nested}))
}

#[test]
fn scoped_pricing_guard_without_marker_rejects_owned_fields_before_store_write() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    reader7(&data);
    let folder = data.join("workflows/project/events");
    fs::create_dir_all(&folder).unwrap();
    let file = folder.join("00000001.json");
    let mut cases = Vec::new();
    for pointer in [
        "/setup/policy/currencyBudget",
        "/body/context/planningBudget/policy",
        "/body/context/execution/revision/policy/currencyBudget",
        "/body/evidence/target/context/execution/revision/policy/currencyBudget",
        "/body/evidence/intent/context/executionSetup/setup/policy/currencyBudget",
        "/body/evidence/intent/context/execution/revision/policy/currencyBudget",
        "/body/evidence/intent/context/workflow/revision/policy/currencyBudget",
        "/body/event/policy",
        "/body/event/authorization/policy",
        "/body/event/preparation/intent/context/executionSetup/setup/policy/currencyBudget",
        "/body/event/preparation/intent/context/workflow/revision/policy/currencyBudget",
        "/body/event/attempt/intent/context/executionSetup/setup/policy/currencyBudget",
        "/body/event/attempt/intent/context/workflow/revision/policy/currencyBudget",
    ] {
        cases.push(at_pointer(pointer, serde_json::json!({"scopedQuotes":[]})));
        // A malformed new policy also cannot be adopted by adding the marker later.
        cases.push(at_pointer(
            pointer,
            serde_json::json!({"schema":2,"scopedQuotes":null}),
        ));
    }
    for pointer in [
        "/body/event/reservation/pricing",
        "/body/event/preparation/reservation/pricing",
        "/body/event/settlement/actualService",
    ] {
        cases.push(at_pointer(pointer, serde_json::json!({})));
    }
    for value in cases {
        fs::write(&file, serde_json::to_vec(&value).unwrap()).unwrap();
        let before = snapshot(&data);
        let error = match crate::Store::open(&data) {
            Ok(_) => panic!("unprotected scoped record accepted"),
            Err(e) => e,
        };
        assert!(error.contains("no reader8 guard"), "{value}: {error}");
        assert_eq!(before, snapshot(&data));
    }
    let backup = temp.path().join("no-adoption");
    let (manifest, hold) = held(&data, &backup);
    let before = snapshot(&data);
    let archived = snapshot(&backup);
    assert!(review(&data, &backup, &manifest, &hold).is_err());
    assert_eq!(before, snapshot(&data));
    assert_eq!(archived, snapshot(&backup));
}

#[test]
fn scoped_pricing_guard_scans_all_owned_collections_without_mutating_them() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    reader7(&data);
    for relative in [
        "project-setups",
        "management-decisions",
        "currency-budgets/project",
        "workflows/project/events",
    ] {
        let folder = data.join(relative);
        fs::create_dir_all(&folder).unwrap();
        let file = folder.join("synthetic.json");
        fs::write(
            &file,
            serde_json::to_vec(
                &serde_json::json!({"body":{"event":{"settlement":{"actualService":"default"}}}}),
            )
            .unwrap(),
        )
        .unwrap();
        let before = snapshot(&data);
        assert!(crate::reader_generation::reject_unprotected(&data)
            .unwrap_err()
            .contains("no reader8 guard"));
        assert_eq!(before, snapshot(&data));
        fs::remove_file(file).unwrap();
    }
}

#[test]
fn scoped_pricing_guard_preserves_reader7_fields_and_ignores_business_json() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    reader7(&data);
    let folder = data.join("project-setups");
    fs::create_dir_all(&folder).unwrap();
    let business = serde_json::json!({
        "setup":{"policy":{"currencyBudget":{"schema":1,"scopedQuotes":null,"quotes":[{"countPrice":{}}]}}},
        "body":{
            "context":{"countResolution":{}},
            "event":{"kind":"resolveManagementCount","command":{"kind":"confirm-management-count-resolution"},"reservation":{"pricing":null},"settlement":{"actualService":null}}
        },
        "business":{
            "setup":{"policy":{"currencyBudget":{"schema":2,"scopedQuotes":[]}}},
            "body":{"event":{"reservation":{"pricing":{}},"settlement":{"actualService":"default"}}}
        }
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
fn scoped_pricing_guard_preserves_incomplete_promotion_and_refuses_unknown_reader() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    reader7(&data);
    let pending = data.join("events/.format-migration-scoped-pricing-fixture.pending");
    fs::write(&pending, b"{").unwrap();
    let before = snapshot(&data);
    assert!(crate::Store::open(&data).is_err());
    assert_eq!(before, snapshot(&data));
    // Only this owned fixture removes its synthetic incomplete record.
    fs::remove_file(pending).unwrap();
    fs::write(data.join("events/00000000.reader9.json"), b"{}").unwrap();
    let before = snapshot(&data);
    assert!(crate::Store::open(&data).is_err());
    assert_eq!(before, snapshot(&data));
}

#[test]
#[ignore = "Owned reader8 CLI/window maintenance fixtures; no user data or models"]
fn export_reader8_maintenance_fixture() {
    let root = std::path::PathBuf::from(
        std::env::var_os("CERULEAN_SCOPED_PRICING_MAINTENANCE_FIXTURES").unwrap(),
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
