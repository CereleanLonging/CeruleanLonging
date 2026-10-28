use super::*;
use cerulean_core::{data_format, rejected_usage_guard, Store};
use std::{collections::BTreeMap, fs, path::Path};

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut result = BTreeMap::new();
    for entry in fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            result.extend(snapshot(&path));
        } else {
            result.insert(path.clone(), fs::read(path).unwrap());
        }
    }
    result
}

#[test]
fn relay_promoted_workbench_keeps_original_catalog_identity_and_pins_new_guard() {
    use cerulean_core::{backup, relay_guard};
    let temp = tempfile::tempdir().unwrap();
    let catalog = temp.path().join("catalog");
    let created = catalog::create_legacy(&catalog, "exp-relay").unwrap();
    let catalog = fs::canonicalize(&catalog).unwrap();
    let archive = temp.path().join("cold");
    let manifest = backup::create(&created.directory, &archive)
        .unwrap()
        .manifest_digest;
    let reviewed = relay_guard::review(&created.directory, &archive, &manifest).unwrap();
    let before = snapshot(&catalog);
    relay_guard::promote(&created.directory, &archive, &manifest, &reviewed.digest).unwrap();
    let mut after = snapshot(&catalog);
    let marker = created.directory.join(relay_guard::MARKER);
    after.remove(&marker).unwrap();
    assert_eq!(before, after);
    let frozen = snapshot(&catalog);
    for _ in 0..2 {
        let (entry, binding) = catalog::open(&catalog, "exp-relay").unwrap();
        assert_eq!(entry.directory, created.directory);
        assert!(fs::OpenOptions::new().write(true).open(&marker).is_err());
        drop(Store::open(&entry.directory).unwrap());
        drop(binding);
    }
    assert_eq!(snapshot(&catalog), frozen);
}
#[test]
fn new_workbench_protects_only_new_data_and_reopens_without_execution() {
    let temp = tempfile::tempdir().unwrap();
    let ordinary = temp.path().join("ordinary");
    drop(Store::open(&ordinary).unwrap());
    let before = snapshot(&ordinary);
    let catalog = temp.path().join("catalog");
    assert!(catalog::list(&catalog).unwrap().is_empty());
    assert!(!catalog.exists());
    let created = catalog::create(&catalog, "exp-first").unwrap();
    assert!(created.ready);
    assert_eq!(
        data_format::status(&created.directory)
            .unwrap()
            .minimum_reader_generation,
        rejected_usage_guard::GENERATION
    );
    assert!(!created.directory.join("credentials").exists());
    assert!(!created.directory.join("baseline-experiments").exists());
    let frozen = snapshot(&catalog);
    for _ in 0..2 {
        let (entry, binding) = catalog::open(&catalog, "exp-first").unwrap();
        assert_eq!(entry.directory, binding.directory);
        assert_eq!(catalog::list(&catalog).unwrap().len(), 1);
        drop(Store::open(&entry.directory).unwrap());
    }
    assert!(catalog::create(&catalog, "exp-first").is_err());
    assert_eq!(snapshot(&ordinary), before);
    assert_eq!(snapshot(&catalog), frozen);
}
#[test]
fn partial_or_corrupt_entries_are_visible_but_never_adopted_or_repaired() {
    let temp = tempfile::tempdir().unwrap();
    let catalog = temp.path().join("catalog");
    fs::create_dir_all(catalog.join("exp-interrupted/data")).unwrap();
    fs::write(catalog.join("exp-interrupted/intent.json"), b"partial").unwrap();
    catalog::create(&catalog, "exp-ready").unwrap();
    fs::write(
        catalog.join("exp-ready/ready.pending"),
        b"uncertain publication",
    )
    .unwrap();
    let before = snapshot(&catalog);
    let entries = catalog::list(&catalog).unwrap();
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().all(|e| !e.ready && e.error.is_some()));
    for id in ["exp-interrupted", "exp-ready"] {
        assert!(catalog::open(&catalog, id).is_err());
        assert!(catalog::create(&catalog, id).is_err());
    }
    assert_eq!(snapshot(&catalog), before);
}
#[test]
fn identity_guard_backup_and_symlink_changes_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let catalog = temp.path().join("catalog");
    for (id, relative) in [
        ("exp-intent", "intent.json"),
        ("exp-review", "protection-review.json"),
        ("exp-ready", "ready.json"),
        ("exp-guard", "data/events/00000000.reader17.json"),
        ("exp-backup", "initial-cold-backup/manifest.json"),
    ] {
        catalog::create(&catalog, id).unwrap();
        let path = catalog.join(id).join(relative);
        assert!(path.exists(), "{relative}");
        fs::write(&path, b"{}").unwrap();
        let before = snapshot(&catalog);
        assert!(catalog::open(&catalog, id).is_err(), "{relative}");
        assert_eq!(snapshot(&catalog), before);
    }
    // Directory junction is tested with an actual reparse point, without admin privileges.
    let target = temp.path().join("foreign");
    fs::create_dir(&target).unwrap();
    let link = catalog.join("exp-link");
    let status = std::process::Command::new("cmd.exe")
        .args(["/c", "mklink", "/J"])
        .arg(&link)
        .arg(&target)
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(catalog::open(&catalog, "exp-link").is_err());
    assert!(catalog::create(&catalog, "exp-link").is_err());
    assert!(fs::read_dir(&target).unwrap().next().is_none());
    // Remove only the junction itself; never recursively traverse its target.
    fs::remove_dir(&link).unwrap();
}
#[test]
fn immutable_window_routes_and_pins_preserve_an_active_other_host() {
    let temp = tempfile::tempdir().unwrap();
    let catalog = temp.path().join("catalog");
    let a = catalog::create(&catalog, "exp-one").unwrap();
    let b = catalog::create(&catalog, "exp-two").unwrap();
    let windows = Windows::default();
    let (_, first) = catalog::open(&catalog, "exp-one").unwrap();
    windows.bind("experiment-exp-one", first).unwrap();
    let in_flight_directory = windows.directory("experiment-exp-one").unwrap();
    let active = Store::open(&a.directory).unwrap();
    let (_, second) = catalog::open(&catalog, "exp-two").unwrap();
    windows.bind("experiment-exp-two", second).unwrap();
    let (_, second) = catalog::open(&catalog, "exp-two").unwrap();
    assert!(windows.bind("experiment-exp-one", second).is_err());
    assert_eq!(
        windows.directory("experiment-exp-one").unwrap(),
        in_flight_directory
    );
    assert_eq!(
        windows.directory("experiment-exp-two").unwrap(),
        b.directory
    );
    assert!(windows.directory("unknown").is_err());
    assert!(Store::open(&a.directory).is_err()); // original writer still owns its root
    assert!(fs::write(a.directory.join(rejected_usage_guard::MARKER), b"replace").is_err());
    assert!(fs::rename(catalog.join("exp-one"), catalog.join("moved")).is_err());
    drop(active);
    drop(Store::open(&a.directory).unwrap());
}
#[test]
fn invalid_names_never_create_or_escape_catalog() {
    let temp = tempfile::tempdir().unwrap();
    let catalog = temp.path().join("catalog");
    for id in [
        "../escape",
        "exp-../escape",
        "exp-test/child",
        "exp-test\\child",
        "exp-test:ads",
        "CON",
        "exp-",
        "EXP-test",
        "exp-test.",
        "exp-test ",
        "exp-test-",
    ] {
        assert!(catalog::create(&catalog, id).is_err());
        assert!(catalog::open(&catalog, id).is_err());
    }
    assert!(!catalog.exists());
    assert!(!catalog::valid_id(&format!("exp-{}", "a".repeat(45))));
}

#[test]
fn rejected_usage_promoted_workbench_keeps_original_catalog_identity_and_pins_new_guard() {
    use cerulean_core::{backup, rejected_usage_guard};
    let temp = tempfile::tempdir().unwrap();
    let catalog = temp.path().join("catalog");
    let created = catalog::create_legacy(&catalog, "exp-rejected-usage").unwrap();
    let catalog = fs::canonicalize(&catalog).unwrap();
    let archive = temp.path().join("cold");
    let manifest = backup::create(&created.directory, &archive)
        .unwrap()
        .manifest_digest;
    let reviewed = rejected_usage_guard::review(&created.directory, &archive, &manifest).unwrap();
    let before = snapshot(&catalog);
    rejected_usage_guard::promote(&created.directory, &archive, &manifest, &reviewed.digest)
        .unwrap();
    let mut after = snapshot(&catalog);
    let marker = created.directory.join(rejected_usage_guard::MARKER);
    after.remove(&marker).unwrap();
    assert_eq!(before, after);
    let frozen = snapshot(&catalog);
    for _ in 0..2 {
        let (entry, binding) = catalog::open(&catalog, "exp-rejected-usage").unwrap();
        assert_eq!(entry.directory, created.directory);
        assert!(fs::OpenOptions::new().write(true).open(&marker).is_err());
        drop(Store::open(&entry.directory).unwrap());
        drop(binding);
    }
    assert_eq!(snapshot(&catalog), frozen);
}

#[test]
fn existing_reader15_catalog_opens_without_silent_promotion() {
    let temp = tempfile::tempdir().unwrap();
    let catalog = temp.path().join("catalog");
    let created = catalog::create_legacy(&catalog, "exp-legacy").unwrap();
    let before = snapshot(&catalog);
    let (_, binding) = catalog::open(&catalog, "exp-legacy").unwrap();
    assert_eq!(
        data_format::status(&created.directory)
            .unwrap()
            .minimum_reader_generation,
        15
    );
    assert!(!created
        .directory
        .join(rejected_usage_guard::MARKER)
        .exists());
    drop(binding);
    assert_eq!(snapshot(&catalog), before);
}
