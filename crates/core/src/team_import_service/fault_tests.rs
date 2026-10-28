//! Actual isolated-process interruption and Windows I/O failures. No model calls.
use super::*;
use crate::{diagnostic_service::ServiceHost, reader_generation::tests::snapshot, Store};
use cerulean_domain::{Command, Request, PROTOCOL_VERSION};
use serde_json::json;
use sha2::Digest;
use std::{
    fs,
    os::windows::{fs::OpenOptionsExt, process::CommandExt},
    time::{Duration, Instant},
};

fn protected(temp: &Path) -> PathBuf {
    let root = temp.join("data");
    drop(Store::open(&root).unwrap());
    let backup = temp.join("initial-backup");
    let manifest = crate::backup::create(&root, &backup)
        .unwrap()
        .manifest_digest;
    let review = crate::experiment_guard::review(&root, &backup, &manifest).unwrap();
    crate::experiment_guard::promote(&root, &backup, &manifest, &review.digest).unwrap();
    fs::canonicalize(root).unwrap()
}
fn source(root: &Path) -> (PathBuf, String) {
    let missing = root.parent().unwrap().join("unavailable-material");
    let path = root.parent().unwrap().join("request.json");
    let bytes=serde_json::to_vec(&json!({"material":{"bundle":missing,"manifestSha256":"a".repeat(64),"schedule":missing,"launcher":missing,"checker":missing,
        "node":{"path":missing,"sha256":"a".repeat(64)},"projectRoot":missing,"dataRoot":root,
        "worktree":{"schemaVersion":1,"baseCommit":"a".repeat(40),"poolRoot":missing,"git":{"path":missing,"sha256":"a".repeat(64)},"input":"committed-raw-blobs"}},
        "workloads":missing,"branchSpecs":{},"chiefSessionId":null})).unwrap();
    fs::write(&path, &bytes).unwrap();
    (path, format!("{:x}", sha2::Sha256::digest(&bytes)))
}
fn new_import(root: &Path, file: &Path, sha: &str) -> (Box<RuntimeHost>, Importing) {
    let mut host = Box::new(RuntimeHost::open(root).unwrap());
    let job = begin(
        &mut host,
        "original-fault-import",
        file.to_str().unwrap(),
        sha,
    )
    .unwrap();
    (host, job)
}
fn finish(job: &mut Importing) -> Box<RuntimeHost> {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(host) = job.take_finished() {
            return host;
        }
        assert!(
            Instant::now() < until,
            "Original worker still active; do not start another"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn frozen_reopens(root: &Path, state: &str) {
    let original = snapshot(root);
    for _ in 0..2 {
        let mut service = ServiceHost::open(root).unwrap();
        let s = service.snapshot();
        assert_eq!(s.team_import.unwrap().state, state);
        assert!(s.workflows.is_empty());
        assert!(s.api_jobs.is_empty());
        assert!(s.project_runs.is_empty());
        assert!(service
            .handle_command(Request {
                id: "do-not-replay".into(),
                protocol_version: PROTOCOL_VERSION,
                command: Command::CreatePlan {
                    title: "blocked".into(),
                    tasks: vec!["blocked".into()]
                }
            })
            .error
            .is_some());
        assert!(!crate::upgrade::review(&mut service).unwrap().0.ready);
        assert!(service.can_shutdown());
        drop(service);
        assert_eq!(original, snapshot(root));
    }
}
#[test]
fn request_sharing_violation_after_intent_preserves_host_and_failed_outcome() {
    let temp = tempfile::tempdir().unwrap();
    let root = protected(temp.path());
    let (file, sha) = source(&root);
    let (host, mut job) = new_import(&root, &file, &sha);
    let intent = fs::read(root.join(OPERATION)).unwrap();
    let pin = fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(&file)
        .unwrap();
    let error = fs::File::open(&file).unwrap_err();
    assert_eq!(error.raw_os_error(), Some(32));
    job.launch(host);
    let returned = finish(&mut job);
    assert!(
        RuntimeHost::open(&root).is_err(),
        "Original writer must still be exclusively owned"
    );
    let result: Outcome = load(&root, OUTCOME).unwrap();
    assert!(!result.completed);
    assert_eq!(view(&root).unwrap().state, "failed");
    assert_eq!(fs::read(root.join(OPERATION)).unwrap(), intent);
    assert!(!root.join("team-material-import.json").exists());
    drop(pin);
    drop(returned);
    drop(job);
    frozen_reopens(&root, "failed");
}
#[test]
fn actual_outcome_publication_failure_preserves_pending_and_fences_cold_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let root = protected(temp.path());
    let (file, sha) = source(&root);
    let (host, mut job) = new_import(&root, &file, &sha);
    // An existing directory forces the real filesystem link publication to fail.
    fs::create_dir(root.join(OUTCOME)).unwrap();
    job.launch(host);
    let returned = finish(&mut job);
    assert!(root.join(OUTCOME).is_dir());
    let pending = root.join(format!("{OUTCOME}.pending"));
    let recorded: Outcome = serde_json::from_slice(&fs::read(&pending).unwrap()).unwrap();
    assert!(!recorded.completed);
    assert_eq!(view(&root).unwrap().state, "interrupted");
    assert!(RuntimeHost::open(&root).is_err());
    drop(returned);
    drop(job);
    frozen_reopens(&root, "interrupted");
    // Existing cold backup + explicit retention can preserve this actual I/O failure.
    let backup = temp.path().join("fault-backup");
    let manifest = crate::backup::create(&root, &backup)
        .unwrap()
        .manifest_digest;
    let to = temp.path().join("retention");
    let before = snapshot(&root);
    let r = crate::team_import_retention::review(
        &root,
        &backup,
        &manifest,
        &to,
        "Preserve publication failure",
    )
    .unwrap();
    crate::team_import_retention::retain(
        &root,
        &backup,
        &manifest,
        &to,
        "Preserve publication failure",
        &r.digest,
    )
    .unwrap();
    assert_eq!(before, snapshot(&root));
    crate::team_import_retention::inspect(
        &root,
        &backup,
        &manifest,
        &to,
        "Preserve publication failure",
        &r.digest,
    )
    .unwrap();
}
#[test]
#[ignore = "child-only interruption fixture, invoked by the parent with an isolated protected root"]
fn intent_boundary_child() {
    let root = PathBuf::from(
        std::env::var_os("CERULEAN_IMPORT_265_CHILD_ROOT").expect("parent-selected fixture root"),
    );
    assert_eq!(root.file_name().unwrap(), "data");
    let file = root.parent().unwrap().join("request.json");
    let sha = format!("{:x}", sha2::Sha256::digest(fs::read(&file).unwrap()));
    let (_host, _job) = new_import(&root, &file, &sha);
    assert!(root.join(OPERATION).is_file());
    assert!(!root.join(OUTCOME).exists());
    // Deliberately exit this newly spawned test process after durable intent.
    // No worker, child command, model task, or user data has been started.
    std::process::exit(73);
}
#[test]
fn actual_process_exit_after_durable_intent_releases_lock_without_replay() {
    let temp = tempfile::tempdir().unwrap();
    let root = protected(temp.path());
    source(&root);
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "team_import_service::fault_tests::intent_boundary_child",
            "--ignored",
            "--nocapture",
        ])
        .env("CERULEAN_IMPORT_265_CHILD_ROOT", &root)
        .creation_flags(0x08000000)
        .output()
        .unwrap();
    assert_eq!(
        child.status.code(),
        Some(73),
        "{}",
        String::from_utf8_lossy(&child.stderr)
    );
    let op: Operation = load(&root, OPERATION).unwrap();
    assert_eq!(op.request_id, "original-fault-import");
    assert!(!root.join(OUTCOME).exists());
    assert!(!root.join("team-material-import.json").exists());
    frozen_reopens(&root, "interrupted");
}
