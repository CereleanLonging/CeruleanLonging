#![cfg(windows)]
//! Explicit offline process fixture: build the example first and provide its
//! path plus an isolated parent directory. Never uses installed OMP or real keys.
use cerulean_adapters::omp_transport::worker::{DispatchIntent, PromptOutcome};
use cerulean_core::{omp_journal::OmpJournal, omp_process::WorkerProcess, verifier::PinnedCheck};
use cerulean_domain::{execution::*, secret::SecretInput};
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::{Path, PathBuf},
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::WAIT_OBJECT_0,
    System::Threading::{
        OpenProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    },
};

const PLACEHOLDER: &str = "cerulean-offline-placeholder-not-a-real-key";

#[tokio::test]
#[ignore = "explicit offline worker executable and isolated parent required"]
async fn forced_parent_exit_waits_for_original_handle_after_job_accounting() {
    let (root, exe) = fixture();
    let worker = selection(&exe, "fixture-parent-wait");
    let key = SecretInput::new(PLACEHOLDER.into()).unwrap();
    let mut process = WorkerProcess::launch(&worker, &root.path().join("profile"), &key).unwrap();
    let original = handle(process.pid());
    process.connect().await.unwrap();
    let exit = process.close().await.unwrap();
    exited(&original);
    assert!(exit.forced);
    assert_eq!(exit.exit_code, 1);
    assert_eq!(exit.remaining_processes, 0);
    assert_eq!(exit.stderr_bytes, 0);
}
fn handle(pid: u32) -> OwnedHandle {
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    };
    assert!(!handle.is_null());
    unsafe { OwnedHandle::from_raw_handle(handle) }
}
fn exited(handle: &OwnedHandle) {
    assert_eq!(
        unsafe { WaitForSingleObject(handle.as_raw_handle(), 5000) },
        WAIT_OBJECT_0
    );
}
fn fixture() -> (tempfile::TempDir, PathBuf) {
    let parent = PathBuf::from(
        std::env::var_os("CERULEAN_OMP_TEST_PARENT").expect("explicit isolated test parent"),
    );
    let exe = PathBuf::from(
        std::env::var_os("CERULEAN_OMP_FIXTURE_EXE").expect("built offline fixture example"),
    );
    assert_eq!(exe.file_name().unwrap(), "omp_offline_worker_fixture.exe");
    let root = tempfile::tempdir_in(parent).unwrap();
    let copy = root.path().join("approved-worker.exe");
    fs::copy(exe, &copy).unwrap();
    (root, copy)
}
fn selection(exe: &Path, mode: &str) -> WorkerSelection {
    WorkerSelection {
        omp_runtime_parent: None,
        provider: "deepseek".into(),
        model: mode.into(),
        executable: PinnedCheck::describe(exe, &[], Default::default())
            .unwrap()
            .executable,
    }
}
fn journal(
    root: &Path,
    process: &WorkerProcess,
    worker: &WorkerSelection,
    session: String,
) -> OmpJournal {
    let prompt = "Offline ownership fixture; no network or model invocation.";
    OmpJournal::open(
        &root.join("journal"),
        &DispatchIntent {
            version: 1,
            task_id: "task".into(),
            attempt_id: "attempt".into(),
            revision_digest: "a".repeat(64),
            fence: 1,
            binding: SessionBinding {
                adapter_id: "omp".into(),
                adapter_version: "offline-fixture".into(),
                session_id: session,
                workspace: process.profile().join("workspace").to_string_lossy().into(),
                identity_strength: IdentityStrength::ControllerRequest(ControllerRequestIdentity {
                    connection_id: "connection".into(),
                    request_id: "prompt-request".into(),
                }),
            },
            provider: worker.provider.clone(),
            model: worker.model.clone(),
            prompt_sha256: format!("{:x}", Sha256::digest(prompt.as_bytes())),
        },
    )
    .unwrap()
}
#[tokio::test]
#[ignore = "explicit offline worker executable and isolated parent required"]
async fn actual_worker_ownership_protocol_and_isolation() {
    for mode in [
        "fixture-completed",
        "fixture-lost-ack",
        "fixture-wrong-model",
        "fixture-tree",
    ] {
        let (root, exe) = fixture();
        let worker = selection(&exe, mode);
        let profile = root.path().join("profile");
        let key = SecretInput::new(PLACEHOLDER.into()).unwrap();
        let mut process = WorkerProcess::launch(&worker, &profile, &key).unwrap();
        let parent = handle(process.pid());
        assert!(
            fs::OpenOptions::new().write(true).open(&exe).is_err(),
            "approved binary must be write locked"
        );
        assert!(process.channel().is_err());
        let connected = process.connect().await;
        if mode == "fixture-wrong-model" {
            assert!(connected.is_err());
            assert!(process.channel().is_err());
        } else {
            let session = connected.unwrap();
            if mode != "fixture-tree" {
                let mut journal = journal(root.path(), &process, &worker, session.session_id);
                let (_tx, mut rx) = tokio::sync::oneshot::channel();
                let outcome = journal
                    .execute_once(
                        process.channel().unwrap(),
                        "Offline ownership fixture; no network or model invocation.",
                        Duration::from_secs(3),
                        &mut rx,
                    )
                    .await;
                if mode == "fixture-completed" {
                    assert!(matches!(outcome, Ok(PromptOutcome::Finished(_))));
                    assert!(journal.observation().terminal.is_some());
                } else {
                    assert!(outcome.is_err());
                    assert!(journal.observation().unknown());
                }
                assert_eq!(process.command_attempts().get("prompt"), Some(&1));
            }
        }
        let descendant = if mode == "fixture-tree" {
            Some(handle(
                fs::read_to_string(profile.join("workspace/descendant.pid"))
                    .unwrap()
                    .parse()
                    .unwrap(),
            ))
        } else {
            None
        };
        let exit = process.close().await.unwrap();
        exited(&parent);
        if let Some(child) = descendant {
            exited(&child);
        }
        assert_eq!(exit.remaining_processes, 0);
        assert_eq!(exit.forced, mode == "fixture-tree");
        assert_eq!(
            exit.exit_code,
            if mode == "fixture-lost-ack" { 23 } else { 0 }
        );
        assert!(!serde_json::to_string(&exit).unwrap().contains(PLACEHOLDER));
        assert!(
            fs::OpenOptions::new().write(true).open(&exe).is_ok(),
            "binary lock must release on close"
        );
        assert!(
            WorkerProcess::launch(&worker, &profile, &key).is_err(),
            "never reuse a profile"
        );
        assert!(!fs::read_to_string(profile.join("config.yml"))
            .unwrap()
            .contains(PLACEHOLDER));
    }
}
#[tokio::test]
#[ignore = "explicit offline worker executable and isolated parent required"]
async fn drop_kills_actual_parent_and_descendant_and_changed_binary_never_starts() {
    let (root, exe) = fixture();
    let worker = selection(&exe, "fixture-tree");
    let profile = root.path().join("profile");
    let key = SecretInput::new(PLACEHOLDER.into()).unwrap();
    let mut process = WorkerProcess::launch(&worker, &profile, &key).unwrap();
    let parent = handle(process.pid());
    process.connect().await.unwrap();
    let child = handle(
        fs::read_to_string(profile.join("workspace/descendant.pid"))
            .unwrap()
            .parse()
            .unwrap(),
    );
    drop(process);
    exited(&parent);
    exited(&child);
    fs::write(&exe, "changed approved executable").unwrap();
    let next = root.path().join("changed-profile");
    let error = WorkerProcess::launch(&worker, &next, &key).err().unwrap();
    assert!(error.contains("changed since approval"));
    assert!(!next.exists());
}

#[tokio::test]
#[ignore = "child of host_crash_kills_owned_tree; requires explicit fixture scope"]
async fn owner_crash_helper() {
    let root =
        PathBuf::from(std::env::var_os("CERULEAN_OMP_OWNER_ROOT").expect("explicit owner root"));
    let worker = selection(&root.join("approved-worker.exe"), "fixture-tree");
    let mut process = WorkerProcess::launch(
        &worker,
        &root.join("owner-profile"),
        &SecretInput::new(PLACEHOLDER.into()).unwrap(),
    )
    .unwrap();
    process.connect().await.unwrap();
    let child: u32 = fs::read_to_string(root.join("owner-profile/workspace/descendant.pid"))
        .unwrap()
        .parse()
        .unwrap();
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(root.join("owner-ready.json"))
        .unwrap();
    use std::io::Write;
    file.write_all(
        serde_json::to_string(&(process.pid(), child))
            .unwrap()
            .as_bytes(),
    )
    .unwrap();
    file.sync_all().unwrap();
    drop(file);
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[test]
#[ignore = "explicit offline worker executable and isolated parent required"]
fn host_crash_kills_owned_tree_without_running_drop_handlers() {
    use std::{
        os::windows::process::CommandExt,
        process::{Child, Command, Stdio},
        time::Instant,
    };
    struct Owner(Child);
    impl Drop for Owner {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let (root, _) = fixture();
    let mut owner = Owner(
        Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "owner_crash_helper", "--nocapture"])
            .env("CERULEAN_OMP_OWNER_ROOT", root.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x08000000)
            .spawn()
            .unwrap(),
    );
    let marker = root.path().join("owner-ready.json");
    let deadline = Instant::now() + Duration::from_secs(15);
    let (pid, child) = loop {
        assert!(
            owner.0.try_wait().unwrap().is_none(),
            "owned host exited before barrier"
        );
        if let Ok(bytes) = fs::read(&marker) {
            if let Ok(ids) = serde_json::from_slice::<(u32, u32)>(&bytes) {
                break ids;
            }
        }
        assert!(Instant::now() < deadline, "owned host startup deadline");
        std::thread::sleep(Duration::from_millis(10));
    };
    let parent_handle = handle(pid);
    let child_handle = handle(child);
    owner.0.kill().unwrap();
    owner.0.wait().unwrap();
    exited(&parent_handle);
    exited(&child_handle);
    assert_eq!(
        serde_json::from_slice::<(u32, u32)>(&fs::read(marker).unwrap()).unwrap(),
        (pid, child)
    );
}
