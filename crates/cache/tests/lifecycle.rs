use cerulean_cache::Project;
use std::{fs, path::Path};

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::write(
        root.path().join("package.json"),
        r#"{"name":"cerulean-longing"}"#,
    )
    .unwrap();
    fs::write(root.path().join("Cargo.toml"), "[workspace]\n").unwrap();
    root
}

fn put(path: &Path, contents: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

#[test]
fn completion_removes_payload_and_keeps_all_directories_and_outputs() {
    let root = fixture();
    let project = Project::open(root.path()).unwrap();
    let session = project.begin().unwrap();
    let nested = session.directory.join("cargo/debug/deps");
    put(&nested.join("library.rlib"), b"compiled cache");
    put(
        &root.path().join("target/release/CeruleanLonging.exe"),
        b"retained program",
    );
    put(&root.path().join("source.rs"), b"source");
    project.finish(session, true).unwrap();
    assert!(nested.is_dir());
    assert_eq!(fs::read_dir(nested).unwrap().count(), 0);
    assert_eq!(fs::read(root.path().join("source.rs")).unwrap(), b"source");
    assert_eq!(
        fs::read(root.path().join("target/release/CeruleanLonging.exe")).unwrap(),
        b"retained program"
    );
    let snapshot = project.snapshot(false).unwrap();
    assert_eq!(snapshot.total_bytes, 0);
    assert_eq!(snapshot.cumulative_cleaned_bytes, 14);
    assert_eq!(snapshot.pending_operations, 0);
}

#[test]
fn active_session_is_observed_but_manual_cleanup_skips_it() {
    let root = fixture();
    let project = Project::open(root.path()).unwrap();
    let session = project.begin().unwrap();
    let file = session.directory.join("cargo/live.rlib");
    put(&file, b"live");
    let scan = project.snapshot(true).unwrap();
    assert_eq!(scan.total_bytes, 4);
    assert_eq!(scan.available_bytes, 0);
    assert_eq!(scan.scan_count, 1);
    let result = project.clean().unwrap();
    assert_eq!(fs::read(file).unwrap(), b"live");
    assert_eq!(result.cumulative_cleaned_bytes, 0);
    project.finish(session, false).unwrap();
    assert_eq!(project.snapshot(false).unwrap().cumulative_cleaned_bytes, 4);
}

#[test]
fn abandoned_session_is_reclaimed_once_and_reopened_statistics_persist() {
    let root = fixture();
    {
        let project = Project::open(root.path()).unwrap();
        let session = project.begin().unwrap();
        put(&session.directory.join("scratch/file.tmp"), b"orphan");
        drop(session);
        assert_eq!(project.clean().unwrap().cumulative_cleaned_bytes, 6);
        assert_eq!(project.clean().unwrap().cumulative_cleaned_bytes, 6);
    }
    let project = Project::open(root.path()).unwrap();
    assert_eq!(project.snapshot(false).unwrap().cumulative_cleaned_bytes, 6);
}

#[test]
fn invalid_project_and_log_destination_fail_before_deletion() {
    let root = tempfile::tempdir().unwrap();
    assert!(Project::open(root.path()).is_err());
    let root = fixture();
    let log = root.path().join("docs/build-cache/cleanup-logs");
    put(&log, b"not a directory");
    let cache = root.path().join(".temp/build-cache/manual/cache.bin");
    put(&cache, b"keep");
    assert!(Project::open(root.path()).is_err());
    assert_eq!(fs::read(cache).unwrap(), b"keep");
}

#[test]
fn malformed_log_and_pending_attempt_are_visible() {
    let root = fixture();
    let project = Project::open(root.path()).unwrap();
    let scan = project.snapshot(true).unwrap();
    fs::write(scan.log_directory.join("broken.md"), "damaged receipt").unwrap();
    let result = project.snapshot(false).unwrap();
    assert_eq!(result.warnings.len(), 1);
    assert_eq!(result.scan_count, 1);
}

#[cfg(windows)]
#[test]
fn junctions_are_never_followed_during_cache_cleanup() {
    let root = fixture();
    let outside = tempfile::tempdir().unwrap();
    put(&outside.path().join("source.rs"), b"outside source");
    let project = Project::open(root.path()).unwrap();
    let session = project.begin().unwrap();
    let link = session.directory.join("linked-source");
    let output = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&link)
        .arg(outside.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(project.finish(session, true).is_err());
    assert_eq!(
        fs::read(outside.path().join("source.rs")).unwrap(),
        b"outside source"
    );
    assert!(project
        .snapshot(false)
        .unwrap()
        .warnings
        .iter()
        .any(|item| item.contains("链接")));
    fs::remove_dir(link).unwrap();
}

#[test]
fn cleanup_intent_without_completion_remains_visible() {
    let root = fixture();
    let project = Project::open(root.path()).unwrap();
    let snapshot = project.snapshot(false).unwrap();
    let operation = cerulean_cache::Operation {
        id: "interrupted".into(),
        at: "2026-11-04T00:00:00+08:00".into(),
        kind: "手动清理".into(),
        completed: false,
        bytes_before: 0,
        bytes_removed: 0,
        files_removed: 0,
        skipped_active: 0,
        errors: vec![],
    };
    fs::write(
        snapshot.log_directory.join("interrupted-started.md"),
        format!(
            "<!-- cerulean-cache-v1 {} -->\n",
            serde_json::to_string(&operation).unwrap()
        ),
    )
    .unwrap();
    let result = project.snapshot(false).unwrap();
    assert_eq!(result.pending_operations, 1);
    assert_eq!(result.cumulative_cleaned_bytes, 0);
}

#[cfg(windows)]
#[test]
fn locked_file_is_retained_and_partial_cleanup_is_recorded() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = fixture();
    let project = Project::open(root.path()).unwrap();
    let session = project.begin().unwrap();
    let path = session.directory.join("held.pdb");
    put(&path, b"held");
    let handle = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)
        .unwrap();
    assert!(project.finish(session, true).is_err());
    assert!(path.exists());
    let snapshot = project.snapshot(false).unwrap();
    assert_eq!(snapshot.cumulative_cleaned_bytes, 0);
    assert!(!snapshot.recent[0].errors.is_empty());
    drop(handle);
    assert_eq!(project.clean().unwrap().cumulative_cleaned_bytes, 4);
}
