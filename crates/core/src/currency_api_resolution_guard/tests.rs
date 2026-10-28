use super::*;
use crate::reader_generation::tests::{held, snapshot};
pub(crate) fn reader11(root: &Path) {
    crate::currency_api_guard::tests::reader10(root);
    crate::currency_api_guard::tests::promote_fixture(root);
}
pub(crate) fn promote_fixture(root: &Path) {
    let archive = root.with_file_name("api-resolution-cold-backup");
    let (manifest, hold) = held(root, &archive);
    let reviewed = review(root, &archive, &manifest, &hold).unwrap();
    promote(root, &archive, &manifest, &hold, &reviewed.digest).unwrap();
    crate::upgrade::release(root, &hold).unwrap();
}
#[test]
fn api_resolution_guard_records_and_pending_refuse_without_reader12() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data");
    reader11(&root);
    for relative in ["currency-budgets/project", "workflows/project/events"] {
        let dir = root.join(relative);
        fs::create_dir_all(&dir).unwrap();
        // Workflow events publish .json; only the money journal recovers .pending.
        let suffixes: &[&str] = if relative.ends_with("events") {
            &["json"]
        } else {
            &["json", "pending"]
        };
        for suffix in suffixes {
            let path = dir.join(format!("00.{suffix}"));
            for body in [
                serde_json::json!({"event":{"kind":"resolveApiCount"}}),
                serde_json::json!({"context":{"apiCountResolution":{}}}),
                serde_json::json!({"event":{"command":{"kind":"confirmApiCountResolution"}}}),
            ] {
                fs::write(
                    &path,
                    serde_json::to_vec(&serde_json::json!({"body":body})).unwrap(),
                )
                .unwrap();
                let before = snapshot(&root);
                assert!(crate::Store::open(&root)
                    .err()
                    .unwrap()
                    .contains("reader12"));
                assert_eq!(snapshot(&root), before);
            }
            fs::remove_file(path).unwrap();
        }
    }
    crate::reader_generation::reject_unprotected(&root).unwrap();
    fs::write(root.join("events/00000000.reader13.json"), b"{}").unwrap();
    let before = snapshot(&root);
    assert!(crate::Store::open(&root).is_err());
    assert_eq!(snapshot(&root), before);
}
#[test]
fn api_resolution_guard_original_reader11_hold_and_backup_for_both_containers() {
    for schema in [1, 2] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("data");
        drop(crate::Store::open(&root).unwrap());
        if schema == 2 {
            let archive = temp.path().join("schema-backup");
            let (manifest, hold) = held(&root, &archive);
            let review = crate::data_format::review(&root, &archive, &manifest, &hold).unwrap();
            crate::data_format::migrate(&root, &archive, &manifest, &hold, &review.digest).unwrap();
            crate::upgrade::release(&root, &hold).unwrap();
        }
        reader11(&root);
        let archive = temp.path().join("reader11-backup");
        let (manifest, hold) = held(&root, &archive);
        let original = snapshot(&root);
        let backup = snapshot(&archive);
        let reviewed = review(&root, &archive, &manifest, &hold).unwrap();
        assert!(promote(&root, &archive, &manifest, &hold, &"a".repeat(64)).is_err());
        assert_eq!(snapshot(&root), original);
        let result = promote(&root, &archive, &manifest, &hold, &reviewed.digest).unwrap();
        assert_eq!(result.minimum_reader_generation, 12);
        assert_eq!(result.data_schema, schema);
        let mut after = snapshot(&root);
        after.remove(&root.join(MARKER)).unwrap();
        assert_eq!(after, original);
        assert_eq!(snapshot(&archive), backup);
        assert!(crate::Store::open(&root).is_err());
        assert!(review(&root, &archive, &manifest, &hold).is_err());
        crate::upgrade::release(&root, &hold).unwrap();
        drop(crate::Store::open(&root).unwrap());
    }
}

#[test]
#[ignore = "Owned reader12 CLI/window fixtures; no user data or models"]
fn export_reader12_maintenance_fixture() {
    let root = std::path::PathBuf::from(
        std::env::var_os("CERULEAN_API_RESOLUTION_MAINTENANCE_FIXTURES").unwrap(),
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
            reader11(&data);
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
