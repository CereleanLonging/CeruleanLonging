use super::*;
use std::{
    collections::BTreeMap,
    process::{Command, Stdio},
};

fn identity(root: &Path) -> String {
    name(
        &scope(
            root,
            &EventAnchor {
                sequence: 1,
                hash: "a".repeat(64),
            },
            "operation",
        )
        .unwrap(),
        "check",
    )
    .unwrap()
}
fn args(helper: &str) -> Vec<String> {
    vec![
        "--ignored".into(),
        "--exact".into(),
        format!("verifier::process::named_job::tests::{helper}"),
        "--test-threads=1".into(),
    ]
}
fn wait_for(path: &Path, seconds: u64) -> bool {
    let until = Instant::now() + Duration::from_secs(seconds);
    while !path.exists() && Instant::now() < until {
        thread::sleep(Duration::from_millis(10));
    }
    path.exists()
}
fn ended(name: &str) -> bool {
    let until = Instant::now() + Duration::from_secs(5);
    loop {
        if observe(name).unwrap().is_none_or(|n| n == 0) {
            return true;
        }
        if Instant::now() >= until {
            return false;
        }
        thread::sleep(Duration::from_millis(10));
    }
}
fn birth(process: HANDLE) -> u64 {
    let mut created: FILETIME = unsafe { std::mem::zeroed() };
    let mut exited = created;
    let mut kernel = created;
    let mut user = created;
    assert_ne!(
        unsafe { GetProcessTimes(process, &mut created, &mut exited, &mut kernel, &mut user) },
        0
    );
    (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime)
}
fn descendant_exited(root: &Path) -> bool {
    let (pid, created): (u32, u64) =
        serde_json::from_slice(&std::fs::read(root.join("descendant-ready")).unwrap()).unwrap();
    let raw = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    };
    if raw.is_null() {
        assert_eq!(unsafe { GetLastError() }, ERROR_INVALID_PARAMETER);
        return true;
    }
    let process = handle(raw).unwrap();
    // PID reuse does not identify the old child. Otherwise prove exit by its
    // process handle, independently of named Job object visibility.
    birth(process.as_raw_handle()) != created
        || unsafe { WaitForSingleObject(process.as_raw_handle(), 5000) == WAIT_OBJECT_0 }
}

#[test]
fn identity_binds_original_location_approval_operation_and_stage() {
    let root = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let approval = EventAnchor {
        sequence: 1,
        hash: "a".repeat(64),
    };
    let original = scope(root.path(), &approval, "original").unwrap();
    assert_ne!(
        original,
        scope(other.path(), &approval, "original").unwrap()
    );
    assert_ne!(
        original,
        scope(root.path(), &approval, "replacement").unwrap()
    );
    assert_ne!(
        original,
        scope(
            root.path(),
            &EventAnchor {
                sequence: 2,
                ..approval
            },
            "original"
        )
        .unwrap()
    );
    assert_ne!(
        name(&original, "one").unwrap(),
        name(&original, "two").unwrap()
    );
    assert!(name("invalid", "one").is_err());
    assert!(observe("Local\\arbitrary").is_err());
    assert!(create("Global\\arbitrary").is_err());
}

#[test]
fn collision_is_rejected_without_altering_or_terminating_existing_job() {
    let root = tempfile::tempdir().unwrap();
    let name = identity(root.path());
    assert_eq!(observe(&name).unwrap(), None);
    let original = create(&name).unwrap();
    assert_eq!(observe(&name).unwrap(), Some(0));
    assert!(create(&name).is_err());
    assert_eq!(active(&original).unwrap(), 0);
    drop(original);
    assert_eq!(observe(&name).unwrap(), None);
    // A different kernel object with the same valid name is an error, not absence.
    let event =
        handle(unsafe { CreateEventW(null(), 1, 0, wide(OsStr::new(&name)).as_ptr()) }).unwrap();
    assert!(observe(&name).is_err());
    assert!(create(&name).is_err());
    drop(event);
}

#[test]
fn named_job_observes_descendants_and_cancel_confirms_entire_tree_exit() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().to_owned();
    let name = identity(&path);
    let child_name = name.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    let token = cancel.clone();
    let worker = thread::spawn(move || {
        run_scoped(
            &std::env::current_exe().unwrap(),
            &args("tree_owner"),
            &path,
            &path,
            Duration::from_secs(20),
            &token,
            &BTreeMap::new(),
            Some(&child_name),
        )
    });
    let ready = wait_for(&root.path().join("descendant-ready"), 10);
    let observation = observe(&name);
    // A second launcher must not join or terminate the original live job.
    let collision = create(&name);
    let after_collision = observe(&name);
    cancel.store(true, Ordering::Release);
    let outcome = worker.join().unwrap().unwrap();
    assert!(ready, "descendant did not start");
    assert!(observation.unwrap().is_some_and(|n| n >= 2));
    assert!(collision.is_err());
    assert!(after_collision.unwrap().is_some_and(|n| n >= 2));
    assert_eq!(outcome.failure.as_deref(), Some("cancelled"));
    assert!(ended(&name));
    assert!(descendant_exited(root.path()));
}

#[test]
fn owner_process_exit_closes_named_job_and_kills_child_without_recovery_action() {
    let root = tempfile::tempdir().unwrap();
    let name = identity(root.path());
    let mut owner = Command::new(std::env::current_exe().unwrap())
        .args(args("crash_owner"))
        .env("CERULEAN_TEST_NAMED_JOB", &name)
        .current_dir(root.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(status) = owner.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= deadline {
            owner.kill().unwrap();
            owner.wait().unwrap();
            break None;
        }
        thread::sleep(Duration::from_millis(10));
    };
    // The observer has only QUERY rights: it cannot cause the child's exit.
    let tree_ended = ended(&name);
    assert!(
        status.is_some_and(|s| s.success()),
        "owner helper timed out or failed"
    );
    assert!(root.path().join("descendant-ready").exists());
    assert!(tree_ended);
    assert!(descendant_exited(root.path()));
}

#[test]
#[ignore = "owned native helper, invoked only by the bounded parent test"]
fn descendant() {
    std::fs::write(
        "descendant-ready",
        serde_json::to_vec(&(std::process::id(), birth(unsafe { GetCurrentProcess() }))).unwrap(),
    )
    .unwrap();
    thread::sleep(Duration::from_secs(25));
}

#[test]
#[ignore = "owned native helper, invoked only by the bounded parent test"]
fn tree_owner() {
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(args("descendant"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    // This child inherits the original Job membership, not an inheritable handle.
    let _ = child.wait();
}

#[test]
#[ignore = "owned native helper, invoked only by the bounded parent test"]
fn crash_owner() {
    let name = std::env::var("CERULEAN_TEST_NAMED_JOB").unwrap();
    thread::spawn(|| {
        if wait_for(Path::new("descendant-ready"), 10) {
            // Bypass Rust destructors, as a lost owner would. Windows closes
            // its handles and kills the named Job's child tree.
            std::process::exit(0);
        }
        std::process::exit(2);
    });
    let cwd = std::env::current_dir().unwrap();
    let _ = run_scoped(
        &std::env::current_exe().unwrap(),
        &args("tree_owner"),
        &cwd,
        &cwd,
        Duration::from_secs(20),
        &AtomicBool::new(false),
        &BTreeMap::new(),
        Some(&name),
    );
    std::process::exit(3);
}
