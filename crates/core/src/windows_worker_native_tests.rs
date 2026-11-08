//! Explicit, credential-free compatibility probe. Never run as part of normal CI.
use super::*;

#[tokio::test]
#[ignore = "requires a fresh disposable Windows account and a pinned local OMP executable; no model prompt"]
async fn omp_account_handshake_and_context_guard() {
    let plan_path = std::env::var("WORKER_ACCOUNT_TEST_PLAN").expect("fixture plan required");
    let value: serde_json::Value =
        serde_json::from_slice(&bounded(Path::new(&plan_path), 65536).unwrap()).unwrap();
    let data = Path::new(value["dataRoot"].as_str().unwrap());
    let project = Project {
        id: value["projectId"].as_str().unwrap().into(),
        root: value["sharedRoot"].as_str().unwrap().into(),
        timezone: "UTC".into(),
    };
    let id = value["slots"][0]["identity"].as_str().unwrap();
    let binding = WorkerDirectory {
        identity: id.into(),
        relative_path: format!("executors/{id}"),
    };
    let account = load_binding(data, &project, &binding).unwrap();
    let workspace = PathBuf::from(&account.record.workspace);
    let base = workspace.join(".cerulean-omp");
    if !base.exists() {
        fs::create_dir(&base).unwrap();
    }
    ordinary(&base, true).unwrap();
    let run_id = std::env::var("WORKER_OMP_TEST_RUN_ID").unwrap_or_else(|_| "native-1".into());
    assert!(
        !run_id.is_empty()
            && run_id.len() <= 32
            && run_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    );
    let profiles = base.join(run_id);
    fs::create_dir(&profiles).unwrap();
    assert!(
        crate::omp_process::check_profile_parent(&profiles).is_err(),
        "host sees private shared ancestor config"
    );
    with_identity(&account, || {
        assert_eq!(
            fs::read(Path::new(&project.root).join("AGENTS.md"))
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::PermissionDenied
        );
        Ok(())
    })
    .unwrap();
    let executable = PathBuf::from(std::env::var("WORKER_OMP_TEST_EXE").unwrap());
    let tool =
        crate::verifier::PinnedCheck::describe(&executable, &[], Default::default()).unwrap();
    let worker = WorkerSelection {
        reasoning_effort: None,
        omp_runtime_parent: None,
        provider: "deepseek".into(),
        model: "deepseek-flash".into(),
        executable: tool.executable,
    };
    let key = cerulean_domain::secret::SecretInput::new(
        "synthetic-native-probe-no-network-request".into(),
    )
    .unwrap();
    let marker = workspace.join(".env");
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&marker)
        .unwrap();
    let denied_profile = profiles.join("must-not-start");
    let denied = crate::omp_process::WorkerProcess::launch_with_account(
        &worker,
        &denied_profile,
        &key,
        Some(load_binding(data, &project, &binding).unwrap()),
    );
    assert!(
        matches!(denied, Err(ref e) if e.contains(".env")),
        "readable local config must still block"
    );
    assert!(!denied_profile.exists());
    fs::remove_file(marker).unwrap();
    let mut process = crate::omp_process::WorkerProcess::launch_with_account(
        &worker,
        &profiles.join("handshake"),
        &key,
        Some(account),
    )
    .unwrap();
    let identity = process.connect().await.unwrap();
    assert_eq!(identity.provider, "deepseek");
    assert_eq!(identity.model, "deepseek-flash");
    assert_eq!(
        process.command_attempts(),
        &std::collections::BTreeMap::from([
            ("negotiate_protocol".into(), 1),
            ("get_state".into(), 1),
        ])
    );
    let exit = process.close().await.unwrap();
    assert_eq!(exit.remaining_processes, 0);
    assert_eq!(exit.exit_code, 0);
    assert!(!exit.forced);
    println!("OMP independent-account handshake: privateAncestorUnreadable=true, readableConfigRejected=true, modelPrompts=0, gracefulExit=true");
}

#[test]
#[ignore = "requires a disposable Windows account and an explicitly selected desktop executable"]
fn desktop_window_in_independent_account() {
    use std::time::{Duration, Instant};
    #[link(name = "user32")]
    unsafe extern "system" {
        fn PostMessageW(hwnd: *mut std::ffi::c_void, message: u32, w: usize, l: isize) -> i32;
        fn IsWindowVisible(hwnd: *mut std::ffi::c_void) -> i32;
    }
    let plan_path = std::env::var("WORKER_ACCOUNT_TEST_PLAN").expect("fixture plan required");
    let value: serde_json::Value =
        serde_json::from_slice(&bounded(Path::new(&plan_path), 65536).unwrap()).unwrap();
    let data = Path::new(value["dataRoot"].as_str().unwrap());
    let project = Project {
        id: value["projectId"].as_str().unwrap().into(),
        root: value["sharedRoot"].as_str().unwrap().into(),
        timezone: "UTC".into(),
    };
    let id = value["slots"][0]["identity"].as_str().unwrap();
    let binding = WorkerDirectory {
        identity: id.into(),
        relative_path: format!("executors/{id}"),
    };
    let account = load_binding(data, &project, &binding).unwrap();
    let workspace = PathBuf::from(&account.record.workspace);
    let (environment, runtime) = runtime_environment(&workspace).unwrap();
    let exe = PathBuf::from(std::env::var("WORKER_DESKTOP_TEST_EXE").unwrap());
    let user_data = runtime.join("desktop");
    assert!(!user_data.exists(), "probe requires fresh application data");
    let _pin = crate::backup::open(&exe, false, false).unwrap();
    let args = vec![format!("--user-data-dir={}", user_data.display())];
    let spawned = crate::verifier::process::interactive::InteractiveChild::spawn_visible_as(
        &exe,
        &args,
        &workspace,
        &environment,
        &account,
    )
    .unwrap();
    drop((spawned.stdin, spawned.stdout, spawned.stderr));
    let until = Instant::now() + Duration::from_secs(40);
    let window = loop {
        assert_ne!(
            spawned.child.active().unwrap(),
            0,
            "desktop exited before window discovery"
        );
        let windows = crate::host::desktop_sessions::discover_for_account_probe(data).unwrap();
        if let Some(window) = windows.into_iter().find(|w| w.pid == spawned.child.pid) {
            break window;
        }
        assert!(
            Instant::now() < until,
            "independent desktop window was not discovered"
        );
        std::thread::sleep(Duration::from_secs(1));
    };
    verify_desktop_process(&account, window.pid).unwrap();
    assert!(verify_desktop_process(&account, std::process::id()).is_err());
    assert_eq!(
        fs::canonicalize(&window.exe).unwrap(),
        fs::canonicalize(&exe).unwrap()
    );
    println!("independent desktop: app={:?}, version={}, matchingSid=true, providerLoginAttempted=false, modelTurns=0", window.app, window.version);
    let hwnd: usize = window.hwnd.parse().unwrap();
    assert_ne!(unsafe { PostMessageW(hwnd as *mut _, 0x0010, 0, 0) }, 0);
    let until = Instant::now() + Duration::from_secs(20);
    while unsafe { IsWindowVisible(hwnd as *mut _) } != 0 && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        unsafe { IsWindowVisible(hwnd as *mut _) },
        0,
        "test window remained visible after close"
    );
    // Closing Electron windows may intentionally retain a tray process. This
    // probe owns a credential-free app with no task; explicitly clean its job
    // and report that separately instead of claiming a graceful process exit.
    let background = spawned.child.active().unwrap();
    if background != 0 {
        spawned.child.terminate().unwrap();
    }
    let until = Instant::now() + Duration::from_secs(10);
    while spawned.child.active().unwrap() != 0 && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(
        spawned.child.active().unwrap(),
        0,
        "owned test process cleanup incomplete"
    );
    println!("windowHidden=true, backgroundProcessesAfterClose={background}, ownedJobCleanupComplete=true");
}
