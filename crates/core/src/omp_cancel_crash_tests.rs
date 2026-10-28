//! Actual owner termination inside the production cancellation publication.
use super::*;
use cerulean_domain::{
    omp_jobs::OmpJobState,
    project_setup::{ProjectWorkerAdapter, RunState},
};
use std::{
    io::Write,
    os::windows::{
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::CommandExt,
    },
    process::{Child, Command as ProcessCommand, Stdio},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::WAIT_OBJECT_0,
    System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
};
struct Owner(Child);
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn events(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fs::read_dir(root.join("data/workflows/project/events"))
        .unwrap()
        .map(|f| {
            let path = f.unwrap().path();
            let bytes = fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect()
}
fn pause_at(root: &Path, point: &str, worker: u32) -> ! {
    let mut marker = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(root.join("ready.json"))
        .unwrap();
    write!(
        marker,
        "{}",
        serde_json::json!({"point":point,"workerPid":worker})
    )
    .unwrap();
    marker.sync_all().unwrap();
    drop(marker);
    loop {
        std::thread::park_timeout(Duration::from_secs(1));
    }
}

#[test]
#[ignore = "owned cancellation crash helper; only parent supplies fresh case directory"]
fn cancellation_owner() {
    let root = PathBuf::from(std::env::var_os("CERULEAN_OMP_CANCEL_CASE").unwrap());
    let point = std::env::var("CERULEAN_OMP_CANCEL_POINT").unwrap();
    let exe = PathBuf::from(std::env::var_os("CERULEAN_OMP_FIXTURE_EXE").unwrap());
    assert_eq!(exe.file_name().unwrap(), "omp_offline_worker_fixture.exe");
    let (mut host, mut draft) = super::project_planning::tests::fixture(&root);
    draft.worker_adapter = ProjectWorkerAdapter::Omp;
    draft.codex_executable = exe.to_string_lossy().into();
    draft.worker_model = "fixture-file-wait".into();
    draft.artifact_paths = vec!["result.txt".into()];
    draft.management_max_requests = 5;
    host.configure_project(draft).unwrap();
    crate::credential_vault::save_for(
        &host.root,
        credentials::CredentialProvider::DeepSeek,
        &secret::SecretInput::new("cerulean-offline-placeholder-not-a-real-key".into()).unwrap(),
    )
    .unwrap();
    super::omp_project_tests::enable(&mut host);
    let setup = host.project_setups["project"].digest.clone();
    host.request_chief_plan("project", &setup).unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while host.projects["project"].revision().is_none() {
        host.poll_verifications();
        assert!(Instant::now() < deadline, "plan timeout");
        std::thread::sleep(Duration::from_millis(5));
    }
    let wanted = host.projects["project"]
        .revision()
        .unwrap()
        .approval_digest()
        .unwrap();
    host.approve("project", &wanted).unwrap();
    host.start_project_run("project", &wanted, &setup).unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    let (attempt, worker) = loop {
        host.poll_verifications();
        host.poll_project_runs();
        assert!(
            Instant::now() < deadline,
            "worker timeout: {:?}",
            host.snapshot()
        );
        if let Some(job) = host.omp_jobs.values().next() {
            let workspace = host
                .root
                .join("omp-profiles")
                .join(digest(&(job.project_id.as_str(), job.attempt_id.as_str())).unwrap())
                .join("workspace");
            if workspace.join("fixture-waiting").exists() {
                let info: serde_json::Value = serde_json::from_slice(
                    &fs::read(workspace.join("fixture-ready.json")).unwrap(),
                )
                .unwrap();
                break (job.attempt_id.clone(), info["pid"].as_u64().unwrap() as u32);
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    host.cancel_omp_observed("project", &attempt, "original durable stop request", |at| {
        if at == point {
            pause_at(&root, at, worker);
        }
        Ok(())
    })
    .unwrap();
    panic!("selected cancellation boundary was not reached");
}

#[test]
#[ignore = "explicit neutral parent, fresh report and offline worker required; kills only owned test processes"]
fn six_real_cancellation_interruptions_preserve_stop_and_never_resend() {
    let parent = PathBuf::from(std::env::var_os("CERULEAN_OMP_TEST_PARENT").unwrap());
    let report = PathBuf::from(std::env::var_os("CERULEAN_OMP_CANCEL_REPORT").unwrap());
    assert!(report.is_absolute() && !report.exists());
    let mut results = vec![];
    for point in [
        "run-paused",
        "temp-created",
        "partial-written",
        "temp-synced",
        "published",
        "pending-removed",
    ] {
        let case = tempfile::tempdir_in(&parent).unwrap();
        let root = case.path();
        let log = fs::File::create(root.join("owner.log")).unwrap();
        let mut owner = Owner(
            ProcessCommand::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "host::omp_cancel_crash_tests::cancellation_owner",
                    "--nocapture",
                ])
                .env("CERULEAN_OMP_CANCEL_CASE", root)
                .env("CERULEAN_OMP_CANCEL_POINT", point)
                .creation_flags(0x08000000)
                .stdin(Stdio::null())
                .stdout(Stdio::from(log.try_clone().unwrap()))
                .stderr(Stdio::from(log))
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(35);
        let marker: serde_json::Value = loop {
            if let Ok(bytes) = fs::read(root.join("ready.json")) {
                if let Ok(value) = serde_json::from_slice(&bytes) {
                    break value;
                }
            }
            assert!(
                owner.0.try_wait().unwrap().is_none(),
                "owner exited early: {point}: {}",
                fs::read_to_string(root.join("owner.log")).unwrap_or_default()
            );
            assert!(Instant::now() < deadline, "marker timeout: {point}");
            std::thread::sleep(Duration::from_millis(10));
        };
        let worker = unsafe {
            OpenProcess(
                PROCESS_SYNCHRONIZE,
                0,
                marker["workerPid"].as_u64().unwrap() as u32,
            )
        };
        assert!(!worker.is_null());
        let worker = unsafe { OwnedHandle::from_raw_handle(worker) };
        let before = events(root);
        let pending = fs::read_dir(root.join("data/omp-cancellations"))
            .ok()
            .and_then(|entries| {
                entries
                    .map(|e| e.unwrap().path())
                    .find(|p| p.extension().is_some_and(|e| e == "pending"))
            });
        let original_pending = pending.as_ref().map(|p| fs::read(p).unwrap());
        owner.0.kill().unwrap();
        owner.0.wait().unwrap();
        assert_eq!(
            unsafe { WaitForSingleObject(worker.as_raw_handle(), 5000) },
            WAIT_OBJECT_0,
            "owned worker survived: {point}"
        );
        assert_eq!(
            crate::run_control::RunLedger::open(&root.join("data"))
                .unwrap()
                .all()[0]
                .state,
            RunState::Paused
        );
        let incomplete = matches!(point, "temp-created" | "partial-written");
        let reopened = RuntimeHost::open(&root.join("data"));
        let after_first_open = events(root);
        if incomplete {
            let mut host = reopened.unwrap();
            assert_eq!(host.omp_stop_issues.len(), 1);
            host.poll_verifications();
            host.poll_project_runs();
            assert!(host.omp_active.is_empty());
            let path = pending.as_ref().unwrap();
            let original_pending = original_pending.unwrap();
            assert_eq!(fs::read(path).unwrap(), original_pending);
            assert!(!path.with_extension("json").exists());
            let issue = host.omp_stop_issues[0].clone();
            host.acknowledge_omp_stop_issue(
                &issue.project_id,
                &issue.attempt_id,
                issue.evidence_digest.as_ref().unwrap(),
                "Reviewed interrupted stop; keep outcome unknown",
            )
            .unwrap();
            assert!(host.omp_stop_issues[0].acknowledged);
            drop(host);
            let mut host = RuntimeHost::open(&root.join("data")).unwrap();
            assert!(host.omp_stop_issues.is_empty());
            assert_eq!(host.snapshot().omp_jobs[0].state, OmpJobState::Unknown);
            assert_eq!(host.runs.all()[0].state, RunState::Paused);
            assert_eq!(
                host.projects["project"].state().tasks["b"].state,
                TaskState::Pending
            );
            assert!(host
                .recover_omp(&issue.project_id, &issue.attempt_id)
                .is_err());
            assert_eq!(fs::read(path).unwrap(), original_pending);
            assert!(!path.with_extension("json").exists());
        } else {
            let mut host = reopened.unwrap();
            super::omp_project_tests::enable(&mut host);
            host.poll_verifications();
            host.poll_project_runs();
            let job = host.snapshot().omp_jobs[0].clone();
            assert_eq!(job.state, OmpJobState::Unknown);
            assert!(host.omp_active.is_empty());
            assert_eq!(
                host.projects["project"].state().tasks["b"].state,
                TaskState::Pending
            );
            let record = host.omp_startups.get("project", &job.attempt_id).unwrap();
            let reason =
                crate::async_omp::cancellation(&host.root, &record.intent, &record.intent_hash)
                    .unwrap();
            assert_eq!(
                reason.as_deref(),
                if point == "run-paused" {
                    None
                } else {
                    Some("original durable stop request")
                }
            );
            assert_eq!(host.runs.all()[0].state, RunState::Paused);
            assert!(
                host.recover_omp("project", &job.attempt_id).is_err(),
                "no terminal proof: cannot claim cancellation or completion"
            );
            drop(host);
            let host = RuntimeHost::open(&root.join("data")).unwrap();
            assert_eq!(host.snapshot().omp_jobs[0].state, OmpJobState::Unknown);
        }
        let after = events(root);
        assert!(
            after == after_first_open,
            "second open/control added events: {point}"
        );
        for (path, bytes) in &before {
            assert!(
                after.get(path) == Some(bytes),
                "original event changed: {}",
                path.display()
            );
        }
        let recovery: Vec<_> = after
            .iter()
            .filter(|(path, _)| !before.contains_key(*path))
            .collect();
        assert!(recovery.len() <= 1, "multiple recovery events: {point}");
        for (_, bytes) in &recovery {
            let event: serde_json::Value = serde_json::from_slice(bytes).unwrap();
            assert_eq!(
                event["body"]["event"]["command"]["kind"],
                "recovery-unknown"
            );
            assert_eq!(event["body"]["event"]["command"]["task"], "a");
        }
        assert_eq!(
            fs::read_to_string(root.join("project/result.txt")).unwrap(),
            "fixture output"
        );
        results.push(serde_json::json!({"point":point,"passed":true,"workerExited":true,"runPaused":true,
            "reopen":if incomplete{"diagnostic-reviewed-unknown-no-resend"}else{"unknown-no-resend"},"originalWorkflowBytesUnchanged":true,"recoveryEventsAdded":recovery.len(),"filePreserved":true}));
    }
    fs::write(
        report,
        serde_json::to_vec_pretty(
            &serde_json::json!({"version":1,"status":"passed","realModelCalls":0,"cases":results}),
        )
        .unwrap(),
    )
    .unwrap();
}

#[test]
#[ignore = "owned stop-review crash helper; parent provides an already interrupted task"]
fn stop_review_owner() {
    let root = PathBuf::from(std::env::var_os("CERULEAN_OMP_CANCEL_CASE").unwrap());
    let point = std::env::var("CERULEAN_OMP_CANCEL_POINT").unwrap();
    let mut host = RuntimeHost::open(&root.join("data")).unwrap();
    assert_eq!(host.omp_stop_issues.len(), 1);
    let issue = host.omp_stop_issues[0].clone();
    host.acknowledge_omp_stop_issue_observed(
        &issue.project_id,
        &issue.attempt_id,
        issue.evidence_digest.as_ref().unwrap(),
        "original user review before process termination",
        |at| {
            if at == point {
                let mut file = fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(root.join("review-ready.json"))
                    .unwrap();
                write!(
                    file,
                    "{}",
                    serde_json::json!({"point":at,"ownerPid":std::process::id()})
                )
                .unwrap();
                file.sync_all().unwrap();
                drop(file);
                loop {
                    std::thread::park_timeout(Duration::from_secs(1));
                }
            }
            Ok(())
        },
    )
    .unwrap();
    panic!("review boundary was not reached");
}

fn start_crash_owner(root: &Path, name: &str, point: &str, log_name: &str) -> Owner {
    let log = fs::File::create(root.join(log_name)).unwrap();
    Owner(
        ProcessCommand::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", name, "--nocapture"])
            .env("CERULEAN_OMP_CANCEL_CASE", root)
            .env("CERULEAN_OMP_CANCEL_POINT", point)
            .creation_flags(0x08000000)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .spawn()
            .unwrap(),
    )
}
fn wait_marker(owner: &mut Owner, root: &Path, name: &str, log: &str) -> serde_json::Value {
    let deadline = Instant::now() + Duration::from_secs(35);
    loop {
        if let Ok(bytes) = fs::read(root.join(name)) {
            if let Ok(value) = serde_json::from_slice(&bytes) {
                return value;
            }
        }
        assert!(
            owner.0.try_wait().unwrap().is_none(),
            "owner exited: {}",
            fs::read_to_string(root.join(log)).unwrap_or_default()
        );
        assert!(Instant::now() < deadline, "marker timeout: {name}");
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn review_files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    match fs::read_dir(root.join("data/omp-stop-reviews")) {
        Ok(files) => files
            .map(|entry| {
                let path = entry.unwrap().path();
                let bytes = fs::read(&path).unwrap();
                (path, bytes)
            })
            .collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
        Err(e) => panic!("review directory: {e}"),
    }
}

#[test]
#[ignore = "seven actual stop-review owner terminations; explicit neutral parent, offline worker and fresh report"]
fn seven_real_review_interruptions_preserve_original_and_require_explicit_retry() {
    let parent = PathBuf::from(std::env::var_os("CERULEAN_OMP_TEST_PARENT").unwrap());
    let report = PathBuf::from(std::env::var_os("CERULEAN_OMP_REVIEW_CRASH_REPORT").unwrap());
    assert!(report.is_absolute() && !report.exists());
    let mut results = vec![];
    for point in [
        "paused",
        "created",
        "partial",
        "synced",
        "published",
        "finished",
        "acknowledged",
    ] {
        let dir = tempfile::tempdir_in(&parent).unwrap();
        let root = dir.path();
        // First produce an actual interrupted write after a real offline worker
        // has modified the project. Never synthesize a "stopped" terminal event.
        let mut owner = start_crash_owner(
            root,
            "host::omp_cancel_crash_tests::cancellation_owner",
            "partial-written",
            "worker-owner.log",
        );
        let marker = wait_marker(&mut owner, root, "ready.json", "worker-owner.log");
        let worker = unsafe {
            OpenProcess(
                PROCESS_SYNCHRONIZE,
                0,
                marker["workerPid"].as_u64().unwrap() as u32,
            )
        };
        assert!(!worker.is_null());
        let worker = unsafe { OwnedHandle::from_raw_handle(worker) };
        let originals = events(root);
        let pending = fs::read_dir(root.join("data/omp-cancellations"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.extension().is_some_and(|e| e == "pending"))
            .unwrap();
        let pending_bytes = fs::read(&pending).unwrap();
        owner.0.kill().unwrap();
        owner.0.wait().unwrap();
        assert_eq!(
            unsafe { WaitForSingleObject(worker.as_raw_handle(), 5000) },
            WAIT_OBJECT_0
        );
        let mut owner = start_crash_owner(
            root,
            "host::omp_cancel_crash_tests::stop_review_owner",
            point,
            "review-owner.log",
        );
        let marker = wait_marker(&mut owner, root, "review-ready.json", "review-owner.log");
        assert_eq!(
            marker["ownerPid"].as_u64().unwrap(),
            u64::from(owner.0.id())
        );
        let before = events(root);
        let files = review_files(root);
        assert_eq!(before.len(), originals.len() + 1);
        for (path, bytes) in &originals {
            assert_eq!(before.get(path), Some(bytes));
        }
        owner.0.kill().unwrap();
        owner.0.wait().unwrap();
        let published = matches!(point, "published" | "finished" | "acknowledged");
        let mut host = RuntimeHost::open(&root.join("data")).unwrap();
        let job = host.snapshot().omp_jobs[0].clone();
        assert_eq!(job.state, OmpJobState::Unknown);
        assert_eq!(host.omp_stop_issues.is_empty(), published);
        host.poll_verifications();
        host.poll_project_runs();
        assert!(host.omp_active.is_empty() && host.management_active.is_empty());
        assert_eq!(host.runs.all()[0].state, RunState::Paused);
        assert_eq!(
            host.projects["project"].state().tasks["a"].state,
            TaskState::Unknown
        );
        assert_eq!(
            host.projects["project"].state().tasks["b"].state,
            TaskState::Pending
        );
        assert_eq!(events(root), before);
        assert_eq!(review_files(root), files);
        let record = host.omp_startups.get("project", &job.attempt_id).unwrap();
        let accepted =
            crate::omp_stop_review::accepted(&host.root, &record.intent, &record.intent_hash)
                .unwrap();
        assert_eq!(
            accepted.as_deref(),
            if published {
                Some("original user review before process termination")
            } else {
                None
            }
        );
        if !published {
            let issue = host.omp_stop_issues[0].clone();
            assert!(!host.can_restart_reviewed_stops());
            assert!(host
                .acknowledge_omp_stop_issue("project", &job.attempt_id, &"0".repeat(64), "stale")
                .is_err());
            assert_eq!(review_files(root), files);
            host.acknowledge_omp_stop_issue(
                "project",
                &job.attempt_id,
                issue.evidence_digest.as_ref().unwrap(),
                "explicit second user confirmation",
            )
            .unwrap();
            assert!(host.can_restart_reviewed_stops());
        }
        drop(host);
        let mut host = RuntimeHost::open(&root.join("data")).unwrap();
        assert!(host.omp_stop_issues.is_empty());
        assert_eq!(host.snapshot().omp_jobs[0].state, OmpJobState::Unknown);
        assert!(host.recover_omp("project", &job.attempt_id).is_err());
        assert_eq!(events(root), before);
        assert_eq!(fs::read(&pending).unwrap(), pending_bytes);
        assert!(!pending.with_extension("json").exists());
        for (path, bytes) in &files {
            assert_eq!(&fs::read(path).unwrap(), bytes);
        }
        assert_eq!(
            fs::read_to_string(root.join("project/result.txt")).unwrap(),
            "fixture output"
        );
        results.push(serde_json::json!({"point":point,"passed":true,"publishedBeforeKill":published,"requiredExplicitRetry":!published,"originalStopBytesPreserved":true,"unpublishedReviewBytesPreserved":true,"workerExited":true,"runPaused":true,"dependentPending":true,"taskUnknown":true,"workflowEventsStable":true,"filePreserved":true}));
    }
    fs::write(
        report,
        serde_json::to_vec_pretty(
            &serde_json::json!({"version":1,"status":"passed","realModelCalls":0,"cases":results}),
        )
        .unwrap(),
    )
    .unwrap();
}
