//! The WPF fixture supplies UIA controls, not a model or an external user chat.
use super::*;
use std::io::Write;
use std::os::windows::process::CommandExt;

struct OwnedWindow(std::process::Child);
impl Drop for OwnedWindow {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
fn wait_json(path: &Path, child: &mut OwnedWindow) -> serde_json::Value {
    let start = Instant::now();
    loop {
        if let Ok(bytes) = fs::read(path) {
            if let Ok(value) = serde_json::from_slice(&bytes) {
                return value;
            }
        }
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "fixture exited: {}",
            path.display()
        );
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "fixture timed out: {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(30));
    }
}
fn persist_json(path: &Path, value: &impl serde::Serialize) {
    let pending = path.with_extension("pending");
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)
        .unwrap();
    f.write_all(&serde_json::to_vec_pretty(value).unwrap())
        .unwrap();
    f.sync_all().unwrap();
    drop(f);
    fs::rename(&pending, path).unwrap();
}
fn count(root: &Path) -> u32 {
    fs::read_to_string(root.join("stop-count.json"))
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

#[test]
fn desktop_stop_bridge_rejects_missing_confirmation_before_window_access() {
    let root = tempfile::tempdir().unwrap();
    let (_, review) = assigned_desktop_task(root.path());
    for request in [
        ReadRequest::TaskStop {
            version: 1,
            review: review.clone(),
            observation: serde_json::Value::Null,
        },
        ReadRequest::TaskStopRead {
            version: 1,
            review,
            observation: serde_json::Value::Null,
        },
    ] {
        assert_eq!(
            observe(root.path(), &request).unwrap_err(),
            "停止审阅无效或过期，未点击"
        );
    }
}

#[test]
#[ignore = "owned WPF/UIA stop validation; no model or external user application input"]
fn native_desktop_stop_bridge() {
    assert_eq!(
        std::env::var("CERULEAN_NATIVE_OWNED_JOB").as_deref(),
        Ok("1")
    );
    let root = fs::canonicalize(std::env::var_os("CERULEAN_DESKTOP_STOP_DIR").unwrap()).unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert!(root.starts_with(fs::canonicalize(repo.join("reports")).unwrap()));
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    let root = normal_path(&root).unwrap();
    let framework = PathBuf::from(std::env::var_os("WINDIR").unwrap())
        .join("Microsoft.NET/Framework64/v4.0.30319");
    let executable = root.join("Qoder.exe");
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
        .arg(format!("/out:{}", executable.display()))
        .arg(
            normal_path(
                &fs::canonicalize(repo.join("tests/fixtures/DesktopStopWindow.cs")).unwrap(),
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
    for mode in [
        "success",
        "clipped-history",
        "quoted-prompt",
        "foreign-prompt",
        "history-changed",
        "changed-model",
        "changed-session",
        "duplicate-stop",
        "history-button",
        "already-completed",
        "already-failed",
        "draft",
        "wrong-turn",
        "expired",
        "unconfirmed",
        "completed-on-stop",
        "failed-on-stop",
    ] {
        let case = root.join(mode);
        fs::create_dir(&case).unwrap();
        let mut child = OwnedWindow(
            std::process::Command::new(&executable)
                .creation_flags(0x08000000)
                .arg(&case)
                .arg(mode)
                .arg(std::process::id().to_string())
                .spawn()
                .unwrap(),
        );
        let mut identity = wait_json(&case.join("window.json"), &mut child);
        let foreground_granted = identity
            .as_object_mut()
            .unwrap()
            .remove("foregroundGranted")
            .unwrap();
        let window: DesktopWindow = serde_json::from_value(identity).unwrap();
        let before = observe(&case, &ReadRequest::TrialPrepare { version: 1, window }).unwrap();
        let fixture = tempfile::tempdir().unwrap();
        let (_host, mut task) = assigned_desktop_task(fixture.path());
        task.observation = before.observation.unwrap();
        task.baseline = before.baseline;
        persist_json(&case.join("start.json"), &task.prompt);
        wait_json(&case.join("started.json"), &mut child);
        let prepared = observe(
            &case,
            &ReadRequest::TaskStopPrepare {
                version: 1,
                review: task.clone(),
            },
        );
        if ![
            "success",
            "clipped-history",
            "wrong-turn",
            "expired",
            "unconfirmed",
            "completed-on-stop",
            "failed-on-stop",
        ]
        .contains(&mode)
        {
            let error = prepared.unwrap_err();
            let expected_error = match mode {
                "changed-session" => "会话、历史或布局已变化；请核对原窗口和发送记录，勿重发",
                "duplicate-stop" => "无法唯一确认停止按钮",
                "history-button" | "already-completed" | "already-failed" => {
                    "原任务已结束或没有可确认的停止控件"
                }
                _ => "停止目标的聊天、原任务或历史已变化",
            };
            assert_eq!(error, expected_error, "{mode}");
            assert_eq!(count(&case), 0, "{mode}");
            persist_json(
                &case.join("case.json"),
                &serde_json::json!({"status":"passed","mode":mode,"rejectedBeforeInvoke":true,"error":error,"stopInvocations":0}),
            );
        } else {
            let mut observation = prepared.unwrap().stop_observation.unwrap();
            if mode == "wrong-turn" {
                observation["turnRuntimeId"] = "foreign.turn".into();
            }
            if mode == "expired" {
                observation["observation"]["capturedAt"] = (chrono::Utc::now()
                    - chrono::Duration::seconds(31))
                .to_rfc3339()
                .into();
            }
            let request = ReadRequest::TaskStop {
                version: 1,
                review: task.clone(),
                observation: observation.clone(),
            };
            // Test controller intent precedes the actual UIA call. Product ledger integration is separate.
            persist_json(&case.join("stop-intent.json"), &request);
            let stopped = observe(&case, &request);
            if mode == "success" || mode == "clipped-history" {
                assert_eq!(foreground_granted, true, "foreground delegation denied");
                let proof = stopped.unwrap().stop_proof.unwrap();
                assert_eq!(proof["cancelled"], true);
                assert_eq!(count(&case), 1);
                let first = chrono::DateTime::parse_from_rfc3339(
                    proof["firstObservedAt"].as_str().unwrap(),
                )
                .unwrap();
                let second = chrono::DateTime::parse_from_rfc3339(
                    proof["observation"]["capturedAt"].as_str().unwrap(),
                )
                .unwrap();
                assert!((second - first).num_milliseconds() >= 250);
                let reread = observe(
                    &case,
                    &ReadRequest::TaskStopRead {
                        version: 1,
                        review: task,
                        observation,
                    },
                )
                .unwrap()
                .stop_proof
                .unwrap();
                assert_eq!(reread["turnRuntimeId"], proof["turnRuntimeId"]);
                assert_eq!(
                    observe(&case, &request).unwrap_err(),
                    "原任务已结束或没有可确认的停止控件"
                );
                assert_eq!(count(&case), 1);
                persist_json(
                    &case.join("case.json"),
                    &serde_json::json!({"status":"passed","mode":mode,"stopInvocations":1,"proof":proof,"readOnlyProof":reread,"repeatRefused":true}),
                );
            } else {
                let error = stopped.unwrap_err();
                let expected_error = match mode {
                    "wrong-turn" => "停止目标的聊天、原任务或历史已变化",
                    "expired" => "停止审阅无效或过期，未点击",
                    _ => "停止后的取消终态尚未核实，禁止重复点击",
                };
                assert_eq!(error, expected_error, "{mode}");
                let expected =
                    if ["unconfirmed", "completed-on-stop", "failed-on-stop"].contains(&mode) {
                        1
                    } else {
                        0
                    };
                assert_eq!(count(&case), expected, "{mode}: {error}");
                let read = ReadRequest::TaskStopRead {
                    version: 1,
                    review: task,
                    observation,
                };
                let read_error = observe(&case, &read).unwrap_err();
                assert_eq!(
                    read_error,
                    if mode == "wrong-turn" {
                        "停止目标的聊天、原任务或历史已变化"
                    } else {
                        "停止后的取消终态尚未核实，禁止重复点击"
                    },
                    "{mode} read-only"
                );
                assert_eq!(count(&case), expected);
                persist_json(
                    &case.join("case.json"),
                    &serde_json::json!({"status":"passed","mode":mode,"stopInvocations":expected,"terminalUnverified":expected==1,"error":error,"readOnlyRejected":true}),
                );
            }
        }
        cases.push(
            serde_json::from_slice::<serde_json::Value>(&fs::read(case.join("case.json")).unwrap())
                .unwrap(),
        );
    }
    persist_json(
        &root.join("result.json"),
        &serde_json::json!({"status":"passed","scope":"actual owned WPF UIA controls and production embedded bridge; synthetic chat/history; no product cancellation IPC or durable workflow yet","realModelCalls":0,"externalMessages":0,"externalUserAppStops":0,"cases":cases}),
    );
}
