//! Production host scheduling and native UIA, with synthetic management only.
use super::*;
use std::{io::Write, os::windows::process::CommandExt};
#[path = "desktop_joint_ui_tests.rs"]
mod ui;
struct Owned(std::process::Child);
impl Drop for Owned {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
fn ready(path: &Path, child: &mut Owned) -> serde_json::Value {
    let start = Instant::now();
    loop {
        if let Ok(b) = fs::read(path) {
            if let Ok(v) = serde_json::from_slice(&b) {
                return v;
            }
        }
        assert!(child.0.try_wait().unwrap().is_none(), "owned window exited");
        assert!(
            start.elapsed() < Duration::from_secs(20),
            "window marker timeout: {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}
fn counter(root: &Path, name: &str) -> u32 {
    fs::read_to_string(root.join(name))
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}
fn save(path: &Path, value: &impl Serialize) {
    let mut f = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .unwrap();
    f.write_all(&serde_json::to_vec_pretty(value).unwrap())
        .unwrap();
    f.sync_all().unwrap();
}
fn command(host: &mut RuntimeHost, c: Command) -> Response {
    host.handle(Request {
        id: "joint-user-command".into(),
        protocol_version: PROTOCOL_VERSION,
        command: c,
    })
}
fn records(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut result = BTreeMap::new();
    for e in fs::read_dir(root).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            result.extend(records(&p));
        } else {
            result.insert(p.clone(), fs::read(p).unwrap());
        }
    }
    result
}
#[test]
#[ignore = "owned WPF send/stop controls with production scheduling and cancellation; synthetic manager only"]
fn native_desktop_joint_cancellation() {
    assert_eq!(
        std::env::var("CERULEAN_NATIVE_OWNED_JOB").as_deref(),
        Ok("1")
    );
    let root = fs::canonicalize(std::env::var_os("CERULEAN_DESKTOP_JOINT_DIR").unwrap()).unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert!(root.starts_with(fs::canonicalize(repo.join("reports")).unwrap()));
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    let root = normal_path(&root).unwrap();
    let framework = PathBuf::from(std::env::var_os("WINDIR").unwrap())
        .join("Microsoft.NET/Framework64/v4.0.30319");
    let exe = root.join("Qoder.exe");
    let mut compiler = std::process::Command::new(framework.join("csc.exe"));
    compiler.creation_flags(0x08000000).args([
        "/nologo",
        "/target:winexe",
        "/platform:x64",
        "/r:System.Web.Extensions.dll",
        "/r:System.Xaml.dll",
    ]);
    for reference in [
        "PresentationFramework.dll",
        "PresentationCore.dll",
        "WindowsBase.dll",
        "UIAutomationProvider.dll",
        "UIAutomationTypes.dll",
    ] {
        compiler.arg(format!(
            "/r:{}",
            framework.join("WPF").join(reference).display()
        ));
    }
    let built = compiler
        .arg(format!("/out:{}", exe.display()))
        .arg(
            normal_path(
                &fs::canonicalize(repo.join("tests/fixtures/DesktopJointWindow.cs")).unwrap(),
            )
            .unwrap(),
        )
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{} {}",
        String::from_utf8_lossy(&built.stdout),
        String::from_utf8_lossy(&built.stderr)
    );
    let mut cases = vec![];
    for mode in ["success", "unconfirmed"] {
        let case = root.join(mode);
        fs::create_dir(&case).unwrap();
        let mut window = Owned(
            std::process::Command::new(&exe)
                .creation_flags(0x08000000)
                .arg(&case)
                .arg(mode)
                .arg(std::process::id().to_string())
                .spawn()
                .unwrap(),
        );
        let mut identity = ready(&case.join("window.json"), &mut window);
        assert_eq!(
            identity
                .as_object_mut()
                .unwrap()
                .remove("foregroundGranted")
                .unwrap(),
            true
        );
        let identity: DesktopWindow = serde_json::from_value(identity).unwrap();
        let (mut host, mut draft) = project_planning::tests::fixture(&case);
        assert!(host.desktop_fixture.is_none());
        host.discover_desktop().unwrap();
        spin(&mut host);
        assert!(
            host.desktop_access.windows.contains(&identity),
            "owned window must pass production discovery"
        );
        host.inspect_desktop(identity).unwrap();
        spin(&mut host);
        let observed = host
            .desktop_access
            .observation_digest
            .clone()
            .expect("real UIA observation");
        host.bind_desktop("project", &observed, "owned UIA cancellation window")
            .unwrap();
        spin(&mut host);
        let binding = host.desktop_access.bindings[0].clone();
        draft.worker_adapter = cerulean_domain::project_setup::ProjectWorkerAdapter::Qoder;
        draft.codex_executable.clear();
        draft.worker_model = binding.observation.model_label.clone().unwrap();
        draft.desktop_worker = Some(DesktopWorkerSelection {
            binding,
            project_root: host.snapshot().workflows[0].project.root.clone(),
            model_label: draft.worker_model.clone(),
            workspace_confirmed: true,
            external_tools_acknowledged: true,
        });
        draft.max_seconds = 90;
        let mut host = configured_draft(host, draft, "JOINT_UIA");
        host.desktop_fixture = None; // Every desktop call crosses the production process/UIA bridge.
        plan_and_approve(&mut host);
        start(&mut host);
        let attempt = assigned(&mut host);
        let digest = prepared(&mut host, &attempt);
        let review = host.desktop_access.task_review.clone().unwrap();
        assert_eq!(review.max_wait_seconds, 50);
        save(&case.join("original-review.json"), &review);
        let response = command(
            &mut host,
            Command::SendDesktopTask {
                review_digest: digest.clone(),
            },
        );
        assert!(response.error.is_none(), "{:?}", response.error);
        ready(&case.join("started.json"), &mut window);
        assert_eq!(counter(&case, "send-count.json"), 1);
        assert_eq!(counter(&case, "stop-count.json"), 0);
        let started = Instant::now();
        let snapshot = command(&mut host, Command::Snapshot);
        assert!(snapshot.error.is_none());
        let query_ms = started.elapsed().as_millis();
        assert!(query_ms < 2000, "background blocks snapshot");
        assert!(snapshot.result.unwrap().desktop_access.unwrap().tasks[0].active);
        let before = records(&host.root.join("desktop-task-dispatch-v1"));
        let stopped_at = Instant::now();
        let response = command(
            &mut host,
            Command::CancelDesktopTask {
                project_id: "project".into(),
                attempt_id: attempt.clone(),
                review_digest: digest.clone(),
                reason: "joint UIA stop original task".into(),
            },
        );
        assert!(response.error.is_none(), "{:?}", response.error);
        let request_ms = stopped_at.elapsed().as_millis();
        assert!(request_ms < 2000, "stop request blocks on native observer");
        assert!(host.runs.all().iter().all(|r| r.state != RunState::Active));
        until(&mut host, |h| h.desktop_active.is_none());
        let stop_ms = stopped_at.elapsed().as_millis();
        assert!(
            stop_ms < 25000,
            "old observer should be cancelled before its 50-second wait expires"
        );
        assert_eq!(counter(&case, "send-count.json"), 1);
        assert_eq!(
            counter(&case, "stop-count.json"),
            1,
            "{:?}",
            host.desktop_access.error
        );
        assert!(
            window.0.try_wait().unwrap().is_none(),
            "external window must survive observer cancellation"
        );
        let expected = if mode == "success" {
            TaskState::Cancelled
        } else {
            TaskState::Unknown
        };
        assert_eq!(
            host.projects["project"].state().tasks["task"].state,
            expected,
            "{:?}",
            host.desktop_access.error
        );
        assert!(!host.projects["project"].state().tasks["task"].verified);
        assert_eq!(
            host.desktop_access.tasks[0]
                .cancellation
                .as_ref()
                .unwrap()
                .confirmed,
            mode == "success"
        );
        assert_eq!(host.management.statuses().len(), 2);
        assert!(host.snapshot().verification_jobs.is_empty());
        for (p, b) in &before {
            assert_eq!(&fs::read(p).unwrap(), b);
        }
        let data = host.root.clone();
        let retained = records(&data.join("desktop-task-dispatch-v1"));
        save(&case.join("stopped-snapshot.json"), &host.snapshot());
        drop(host);
        for _ in 0..2 {
            let mut host = RuntimeHost::open(&data).unwrap();
            assert!(host.desktop_fixture.is_none());
            assert!(command(
                &mut host,
                Command::CancelDesktopTask {
                    project_id: "project".into(),
                    attempt_id: attempt.clone(),
                    review_digest: digest.clone(),
                    reason: "must not repeat".into()
                }
            )
            .error
            .is_some());
            if mode == "unconfirmed" {
                host.read_desktop_task("project", &attempt).unwrap();
                until(&mut host, |h| h.desktop_active.is_none());
            }
            assert_eq!(
                host.projects["project"].state().tasks["task"].state,
                expected
            );
            assert_eq!(counter(&case, "send-count.json"), 1);
            assert_eq!(counter(&case, "stop-count.json"), 1);
            assert_eq!(records(&data.join("desktop-task-dispatch-v1")), retained);
            assert!(window.0.try_wait().unwrap().is_none());
            let history = host.projects["project"].history();
            assert_eq!(
                history
                    .iter()
                    .filter(|e| matches!(
                        e.body.event.command,
                        WorkflowCommand::DispatchIntent { .. }
                    ))
                    .count(),
                1
            );
            assert_eq!(
                history
                    .iter()
                    .filter(|e| matches!(
                        e.body.event.command,
                        WorkflowCommand::CancelIntent { .. }
                    ))
                    .count(),
                1
            );
            assert!(!history.iter().any(|e| matches!(
                e.body.event.command,
                WorkflowCommand::Submit { .. }
                    | WorkflowCommand::Verify { .. }
                    | WorkflowCommand::Accept { .. }
            )));
        }
        let item = serde_json::json!({"mode":mode,"realUia":true,"desktopFixtureUsed":false,"syntheticManagementRequests":2,"sendInvocations":1,"stopInvocations":1,"snapshotLatencyMs":query_ms,"stopRequestLatencyMs":request_ms,"stopCompletedMs":stop_ms,"originalObserverWaitSeconds":50,"windowSurvived":true,"reopens":2,"originalBytesPreserved":true,"finalState":expected,"verified":false});
        save(&case.join("result.json"), &item);
        cases.push(item);
    }
    save(
        &root.join("result.json"),
        &serde_json::json!({"status":"passed","scope":"production RuntimeHost user commands/background scheduling with actual owned WPF UIA send, observer cancellation and native stop; synthetic management; no Tauri UI or real third-party application","cases":cases,"realModelCalls":0,"externalUserMessages":0,"externalUserAppStops":0}),
    );
}
