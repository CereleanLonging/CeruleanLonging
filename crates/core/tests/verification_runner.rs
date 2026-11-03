#![cfg(windows)]
use cerulean_core::artifacts::ArtifactObserver;
use cerulean_core::verifier::{verify, PinnedCheck, VerificationInput};
use cerulean_domain::{execution::*, workflow::*};
use std::{
    fs,
    io::{Read, Write},
    sync::atomic::AtomicBool,
    time::Duration,
};

struct OwnedTestProcess(std::process::Child);
impl Drop for OwnedTestProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn read_ready_pid(path: &std::path::Path) -> Option<u32> {
    let text = fs::read_to_string(path).ok()?;
    text.strip_suffix('\n')?
        .parse::<u32>()
        .ok()
        .filter(|pid| *pid > 0)
}
#[test]
fn process_marker_requires_complete_delimited_pid() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("pid");
    for text in ["", "1", "123", "oops\n", "0\n"] {
        fs::write(&file, text).unwrap();
        assert_eq!(read_ready_pid(&file), None);
    }
    fs::write(&file, "123\n").unwrap();
    assert_eq!(read_ready_pid(&file), Some(123));
}

struct Fixture {
    root: tempfile::TempDir,
    plan: PlanRevision,
    approval: Approval,
    attempt: Attempt,
    receipt: ExecutionReceipt,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("src")).unwrap();
        fs::write(root.path().join("src/result.txt"), "abc").unwrap();
        let plan = PlanRevision {
            management_setup_digest: None,
            worktree: None,
            snapshot_policy: None,
            schema_version: 1,
            project: Project {
                id: "project".into(),
                root: root.path().to_string_lossy().into(),
                timezone: "Asia/Shanghai".into(),
            },
            workflow: WorkflowPlan {
                id: "plan".into(),
                version: 1,
                tasks: vec![WorkflowTask {
                    id: "task".into(),
                    dependencies: vec![],
                    workspace_key: "project".into(),
                    goal: "Produce result".into(),
                    resource_keys: None,
                }],
            },
            policy: ExecutionPolicy {
                supervision: None,
                version: 1,
                tools: vec![
                    ToolPermission::ReadFiles,
                    ToolPermission::WriteFiles,
                    ToolPermission::RunCommands,
                ],
                max_requests: 3,
                max_seconds: 60,
                currency_budget: None,
                budget_usd_cents: Some(25),
            },
            execution: vec![TaskExecution {
                worker_directory: None,
                supervision_override: None,
                profile_id: None,
                desktop_worker: None,
                api_worker: None,
                worker: None,
                task_id: "task".into(),
                adapter_id: "codex".into(),
                artifact_paths: vec!["src/result.txt".into()],
                checks: vec![AcceptanceCheck {
                    expected_assertions: vec![],
                    name: None,
                    tool: Some(
                        PinnedCheck::describe(
                            &std::env::current_exe().unwrap(),
                            &[],
                            Default::default(),
                        )
                        .unwrap(),
                    ),
                    id: "check".into(),
                    program: "test-runner".into(),
                    args: vec!["--check".into()],
                    timeout_seconds: 10,
                }],
            }],
        };
        let approval = Approval {
            worker_account_setup: None,
            plan_id: "plan".into(),
            revision: 1,
            policy_version: 1,
            revision_digest: plan.approval_digest().unwrap(),
            user_id: "user".into(),
            approved_at: "2026-10-07T14:00:00Z".into(),
        };
        let binding = SessionBinding {
            adapter_id: "codex".into(),
            adapter_version: "fixture-1".into(),
            session_id: "session-1".into(),
            identity_strength: IdentityStrength::ProviderId,
            workspace: plan.project.root.clone(),
        };
        let attempt = Attempt {
            workspace_proof: None,
            id: "attempt".into(),
            task_id: "task".into(),
            worker_id: "worker".into(),
            revision_digest: approval.revision_digest.clone(),
            fence: 1,
            binding: binding.clone(),
        };
        let receipt = ExecutionReceipt {
            schema_version: 1,
            task_id: "task".into(),
            attempt_id: "attempt".into(),
            fence: 1,
            binding,
            turn_id: Some("turn".into()),
            observed_at: "2026-10-07T14:01:00Z".into(),
            status: ReceiptStatus::Completed,
            evidence_sha256: "a".repeat(64),
        };
        Self {
            root,
            plan,
            approval,
            attempt,
            receipt,
        }
    }
    fn observer(&self) -> Result<ArtifactObserver<'_>, String> {
        ArtifactObserver::new(
            &self.plan,
            &self.approval,
            "user",
            1,
            &self.attempt,
            &self.receipt,
            Some("turn"),
        )
    }
}

impl Fixture {
    fn prepare(mode: &str, timeout: u32) -> Self {
        let mut f = Self::new();
        f.plan.snapshot_policy = Some(SnapshotPolicy {
            exclude: vec![],
            max_files: 100,
            max_bytes: 1024 * 1024,
        });
        fs::write(f.root.path().join("mode"), mode).unwrap();
        f.plan.execution[0].checks[0].program = "trusted-test-helper".into();
        f.plan.execution[0].checks[0].args = vec![
            "--ignored".into(),
            "--exact".into(),
            "runner_child".into(),
            "--nocapture".into(),
        ];
        f.plan.execution[0].checks[0].timeout_seconds = timeout;
        f.approval.revision_digest = f.plan.approval_digest().unwrap();
        f.attempt.revision_digest = f.approval.revision_digest.clone();
        f
    }
    fn run(
        &self,
        evidence: &std::path::Path,
        cancel: &AtomicBool,
    ) -> Result<VerificationReport, String> {
        let manifest = self.observer()?.capture()?;
        let pins = self.plan.execution[0]
            .checks
            .iter()
            .map(PinnedCheck::from_approved)
            .collect::<Result<Vec<_>, _>>()?;
        verify(
            VerificationInput {
                plan: &self.plan,
                approval: &self.approval,
                authenticated_user: "user",
                policy_version: 1,
                attempt: &self.attempt,
                receipt: &self.receipt,
                manifest: &manifest,
                verifier_id: "core",
            },
            &pins,
            evidence,
            "verify-1",
            cancel,
        )
    }
}
#[test]
fn ordered_stages_keep_individual_outcomes_and_never_rerun() {
    for mode in ["success", "first-fail", "second-fail", "first-timeout"] {
        let mut f = Fixture::prepare("success", 5);
        let first = &mut f.plan.execution[0].checks[0];
        first.id = if mode.starts_with("first-") {
            mode.into()
        } else {
            "first-success".into()
        };
        first.name = Some("First stage".into());
        first.args = vec![
            "--ignored".into(),
            "--exact".into(),
            "ordered_stage_child".into(),
            "--nocapture".into(),
        ];
        first.timeout_seconds = 1;
        let mut second = first.clone();
        second.id = if mode == "second-fail" {
            mode.into()
        } else {
            "second-success".into()
        };
        second.name = Some("Second stage".into());
        f.plan.execution[0].checks.push(second);
        f.approval.revision_digest = f.plan.approval_digest().unwrap();
        f.attempt.revision_digest = f.approval.revision_digest.clone();
        let evidence = tempfile::tempdir().unwrap();
        let report = f.run(evidence.path(), &AtomicBool::new(false)).unwrap();
        let passed = report
            .require_pass(&f.plan, &f.observer().unwrap().capture().unwrap(), "core")
            .is_ok();
        assert_eq!(passed, mode == "success", "{mode}");
        assert_eq!(
            report.checks.len(),
            if mode == "first-timeout" { 1 } else { 2 }
        );
        for (result, spec) in report.checks.iter().zip(&f.plan.execution[0].checks) {
            assert_eq!(result.check_id, spec.id);
            assert_eq!(result.command_digest, digest(spec).unwrap());
            assert_eq!(result.timed_out, spec.id == "first-timeout");
            if spec.id.ends_with("-fail") {
                assert_eq!(result.exit_code, Some(7));
            }
            if spec.id.ends_with("-success") {
                assert_eq!(result.exit_code, Some(0));
            }
            assert_eq!(
                fs::read_to_string(
                    evidence
                        .path()
                        .join(format!("verify-1/{}-temp/count", spec.id))
                )
                .unwrap(),
                "x"
            );
        }
        let record = fs::read(evidence.path().join("verify-1/result.json")).unwrap();
        assert!(f.run(evidence.path(), &AtomicBool::new(false)).is_err());
        assert_eq!(
            fs::read(evidence.path().join("verify-1/result.json")).unwrap(),
            record
        );
        if mode == "first-timeout" {
            assert!(!evidence
                .path()
                .join("verify-1/second-success-temp")
                .exists());
        }
    }
}

#[test]
fn zero_exit_requires_complete_passing_assertions_and_preserves_original_logs() {
    for (text, passed) in [
        ("", false),
        (r#"{"schemaVersion":1,"assertions":[]}"#, false),
        (
            r#"{"schemaVersion":1,"assertions":[{"id":"sum","passed":false,"detail":"wrong result"}]}"#,
            false,
        ),
        (
            r#"{"schemaVersion":1,"assertions":[{"id":"sum","passed":true,"detail":"2 + 3 = 5"}]}"#,
            true,
        ),
    ] {
        let trusted = tempfile::tempdir().unwrap();
        let script = trusted.path().join("check.ps1");
        fs::write(
            &script,
            format!("[Console]::Write('{}'); exit 0", text.replace('\'', "''")),
        )
        .unwrap();
        let shell = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let mut f = Fixture::prepare("success", 10);
        let check = &mut f.plan.execution[0].checks[0];
        check.expected_assertions = vec!["sum".into()];
        check.args = vec![
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-ExecutionPolicy".into(),
            "Bypass".into(),
            "-File".into(),
            script.to_string_lossy().into(),
        ];
        check.tool = Some(
            PinnedCheck::describe(&shell, std::slice::from_ref(&script), Default::default())
                .unwrap(),
        );
        f.approval.revision_digest = f.plan.approval_digest().unwrap();
        f.attempt.revision_digest = f.approval.revision_digest.clone();
        let evidence = tempfile::tempdir().unwrap();
        let report = f.run(evidence.path(), &AtomicBool::new(false)).unwrap();
        assert_eq!(report.checks[0].exit_code, Some(0));
        let manifest = f.observer().unwrap().capture().unwrap();
        assert_eq!(
            report.require_pass(&f.plan, &manifest, "core").is_ok(),
            passed
        );
        assert_eq!(
            fs::read(evidence.path().join("verify-1/check.stdout")).unwrap(),
            text.as_bytes()
        );
        assert!(evidence.path().join("verify-1/check.json").is_file());
        assert!(f.run(evidence.path(), &AtomicBool::new(false)).is_err());
    }
}
#[test]
#[ignore = "owned child for actual ordered verification subprocess tests"]
fn ordered_stage_child() {
    let temp = std::path::PathBuf::from(std::env::var_os("TEMP").unwrap());
    let name = temp.file_name().unwrap().to_string_lossy();
    assert_eq!(fs::read_to_string("src/result.txt").unwrap(), "abc");
    if name.starts_with("second-") {
        assert!(
            fs::read_dir(temp.parent().unwrap()).unwrap().any(|entry| {
                let entry = entry.unwrap();
                entry.file_name().to_string_lossy().starts_with("first-")
                    && entry.path().join("count").exists()
            }),
            "second child cannot start before first child"
        );
    }
    assert!(!temp.join("count").exists());
    fs::write(temp.join("count"), "x").unwrap();
    println!(
        "observed approved source in {}",
        std::env::current_dir().unwrap().display()
    );
    if name.contains("timeout") {
        std::thread::sleep(Duration::from_secs(30));
    }
    if name.contains("-fail-") {
        std::process::exit(7);
    }
}
#[test]
fn actual_output_hashes_exit_code_and_duplicate_never_reruns() {
    let f = Fixture::prepare("success", 5);
    let evidence = tempfile::tempdir().unwrap();
    let report = f.run(evidence.path(), &AtomicBool::new(false)).unwrap();
    report
        .require_pass(&f.plan, &f.observer().unwrap().capture().unwrap(), "core")
        .unwrap();
    let out = fs::read(evidence.path().join("verify-1/check.stdout")).unwrap();
    assert!(String::from_utf8_lossy(&out).contains("real validator output"));
    use sha2::{Digest, Sha256};
    assert_eq!(
        report.checks[0].stdout_sha256,
        format!("{:x}", Sha256::digest(out))
    );
    assert_eq!(
        fs::read_to_string(evidence.path().join("verify-1/check-temp/count")).unwrap(),
        "x"
    );
    assert!(f.run(evidence.path(), &AtomicBool::new(false)).is_err());
    assert_eq!(
        fs::read_to_string(evidence.path().join("verify-1/check-temp/count")).unwrap(),
        "x"
    );
}

fn interstage_fixture(base: &std::path::Path, mode: &str) -> Fixture {
    let mut f = Fixture::prepare(mode, 15);
    f.root = tempfile::tempdir_in(base).unwrap();
    fs::create_dir(f.root.path().join("src")).unwrap();
    fs::write(f.root.path().join("src/result.txt"), "abc").unwrap();
    fs::write(f.root.path().join("mode"), mode).unwrap();
    fs::write(
        f.root.path().join("source-path"),
        f.root.path().to_str().unwrap(),
    )
    .unwrap();
    let tool = base.join("pinned-input.txt");
    fs::write(&tool, "original trusted input").unwrap();
    fs::write(f.root.path().join("tool-path"), tool.to_str().unwrap()).unwrap();
    f.plan.project.root = f.root.path().to_string_lossy().into();
    f.attempt.binding.workspace = f.plan.project.root.clone();
    f.receipt.binding.workspace = f.plan.project.root.clone();
    let first = &mut f.plan.execution[0].checks[0];
    first.id = "first".into();
    first.args = vec![
        "--ignored".into(),
        "--exact".into(),
        "interstage_child".into(),
        "--nocapture".into(),
    ];
    first.tool = Some(
        PinnedCheck::describe(
            &std::env::current_exe().unwrap(),
            &[tool],
            Default::default(),
        )
        .unwrap(),
    );
    let mut second = first.clone();
    second.id = "second".into();
    f.plan.execution[0].checks.push(second);
    f.approval.revision_digest = f.plan.approval_digest().unwrap();
    f.attempt.revision_digest = f.approval.revision_digest.clone();
    f
}

#[test]
fn interstage_mutations_stop_remaining_checks_and_locked_replacements_are_refused() {
    for mode in [
        "source-artifact",
        "source-extra",
        "work-extra",
        "frozen-extra",
        "work-write",
        "frozen-write",
        "tool-write",
    ] {
        let base = tempfile::tempdir().unwrap();
        let f = interstage_fixture(base.path(), mode);
        let evidence = base.path().join("evidence");
        fs::create_dir(&evidence).unwrap();
        let outcome = f.run(&evidence, &AtomicBool::new(false));
        let dir = evidence.join("verify-1");
        let locked = matches!(mode, "work-write" | "frozen-write" | "tool-write");
        assert_eq!(
            fs::read_to_string(dir.join("first-temp/count")).unwrap(),
            "once"
        );
        let first: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join("first.json")).unwrap()).unwrap();
        assert_eq!(first["exitCode"], 0, "{mode}");
        if locked {
            assert_eq!(
                fs::read_to_string(dir.join("first-temp/mutation-blocked")).unwrap(),
                "blocked by file sharing"
            );
            let report = outcome.unwrap();
            report
                .require_pass(&f.plan, &f.observer().unwrap().capture().unwrap(), "core")
                .unwrap();
            assert_eq!(report.checks.len(), 2);
            assert_eq!(
                fs::read_to_string(dir.join("second-temp/count")).unwrap(),
                "once"
            );
            assert_eq!(
                fs::read_to_string(base.path().join("pinned-input.txt")).unwrap(),
                "original trusted input"
            );
        } else {
            let error = outcome.unwrap_err();
            assert!(
                error.contains("changed") || error.contains("differ"),
                "{mode}: {error}"
            );
            assert!(
                !dir.join("second-temp").exists(),
                "{mode}: second stage must not start"
            );
            let terminal: serde_json::Value =
                serde_json::from_slice(&fs::read(dir.join("result.json")).unwrap()).unwrap();
            assert!(terminal["report"].is_null());
            assert!(terminal["error"].is_string());
        }
        let terminal = fs::read(dir.join("result.json")).unwrap();
        assert!(f.run(&evidence, &AtomicBool::new(false)).is_err());
        assert_eq!(fs::read(dir.join("result.json")).unwrap(), terminal);
        assert_eq!(
            fs::read_to_string(dir.join("first-temp/count")).unwrap(),
            "once"
        );
    }
}

#[test]
#[ignore = "owned process for multistage mutation and crash tests"]
fn interstage_child() {
    let temp = std::path::PathBuf::from(std::env::var_os("TEMP").unwrap());
    let dir = temp.parent().unwrap();
    let mode = fs::read_to_string("mode").unwrap();
    assert!(!temp.join("count").exists());
    fs::write(temp.join("count"), "once").unwrap();
    assert_eq!(fs::read_to_string("src/result.txt").unwrap(), "abc");
    if temp.file_name().unwrap() == "first-temp" {
        let source = std::path::PathBuf::from(fs::read_to_string("source-path").unwrap());
        let tool = std::path::PathBuf::from(fs::read_to_string("tool-path").unwrap());
        match mode.as_str() {
            "source-artifact" => fs::write(source.join("src/result.txt"), "xyz").unwrap(),
            "source-extra" => fs::write(source.join("added.txt"), "extra source input").unwrap(),
            "work-extra" => fs::write("added.txt", "extra work input").unwrap(),
            "frozen-extra" => {
                fs::write(dir.join("inputs/added.txt"), "extra frozen input").unwrap()
            }
            "work-write" | "frozen-write" | "tool-write" => {
                let target = match mode.as_str() {
                    "work-write" => std::path::PathBuf::from("src/result.txt"),
                    "frozen-write" => dir.join("inputs/src/result.txt"),
                    _ => tool,
                };
                assert!(fs::write(&target, "replacement").is_err());
                assert!(fs::remove_file(&target).is_err());
                fs::write(temp.join("mutation-blocked"), "blocked by file sharing").unwrap();
            }
            "crash-second" => {}
            _ => panic!("unknown interstage fixture mode"),
        }
    } else {
        assert_eq!(temp.file_name().unwrap(), "second-temp");
        let first: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join("first.json")).unwrap()).unwrap();
        assert_eq!(first["exitCode"], 0);
        if mode == "crash-second" {
            fs::write(
                temp.join("validator-pid"),
                format!("{}\n", std::process::id()),
            )
            .unwrap();
            std::thread::sleep(Duration::from_secs(30));
            fs::write(temp.join("escaped"), "must not survive owner").unwrap();
        }
    }
    println!("stage checked {mode}");
}

#[test]
fn multistage_owner_crash_preserves_first_observation_without_restarting_second() {
    use std::os::windows::{
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::CommandExt,
    };
    use windows_sys::Win32::{
        Foundation::WAIT_OBJECT_0,
        System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
    };
    let base = tempfile::tempdir().unwrap();
    let mut owner = OwnedTestProcess(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "interstage_owner", "--nocapture"])
            .env("CERULEAN_INTERSTAGE_OWNER", base.path())
            .creation_flags(0x08000000)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    let dir = base.path().join("evidence/verify-1");
    let mut child = None;
    for _ in 0..500 {
        if let Some(pid) = read_ready_pid(&dir.join("second-temp/validator-pid")) {
            let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
            if !raw.is_null() {
                child = Some(unsafe { OwnedHandle::from_raw_handle(raw) });
                break;
            }
        }
        if owner.0.try_wait().unwrap().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = owner.0.kill();
    let _ = owner.0.wait();
    let child = child.expect("second validator must start before crash");
    assert_eq!(
        unsafe { WaitForSingleObject(child.as_raw_handle(), 5000) },
        WAIT_OBJECT_0
    );
    assert!(dir.join("intent.json").exists() && dir.join("snapshot.json").exists());
    let first = fs::read(dir.join("first.json")).unwrap();
    let first_value: serde_json::Value = serde_json::from_slice(&first).unwrap();
    assert_eq!(first_value["exitCode"], 0);
    assert!(!dir.join("second.json").exists() && !dir.join("result.json").exists());
    assert!(!dir.join("second-temp/escaped").exists());
    let retry = Fixture::prepare("success", 5);
    let error = retry
        .run(&base.path().join("evidence"), &AtomicBool::new(false))
        .unwrap_err();
    assert!(error.contains("unfinished verification"), "{error}");
    let manifest = retry.observer().unwrap().capture().unwrap();
    let pins = retry.plan.execution[0]
        .checks
        .iter()
        .map(PinnedCheck::from_approved)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let error = verify(
        VerificationInput {
            plan: &retry.plan,
            approval: &retry.approval,
            authenticated_user: "user",
            policy_version: 1,
            attempt: &retry.attempt,
            receipt: &retry.receipt,
            manifest: &manifest,
            verifier_id: "core",
        },
        &pins,
        &base.path().join("evidence"),
        "bypass-new-operation",
        &AtomicBool::new(false),
    )
    .unwrap_err();
    assert!(error.contains("unfinished verification"), "{error}");
    assert!(!base.path().join("evidence/bypass-new-operation").exists());
    assert_eq!(fs::read(dir.join("first.json")).unwrap(), first);
    assert_eq!(
        fs::read_to_string(dir.join("second-temp/count")).unwrap(),
        "once"
    );
}

#[test]
#[ignore = "owned multistage verifier to terminate during second check"]
fn interstage_owner() {
    let base = std::path::PathBuf::from(std::env::var_os("CERULEAN_INTERSTAGE_OWNER").unwrap());
    let f = interstage_fixture(&base, "crash-second");
    let evidence = base.join("evidence");
    fs::create_dir(&evidence).unwrap();
    f.run(&evidence, &AtomicBool::new(false)).unwrap();
}
#[test]
fn tool_binding_changes_invalidate_approval_and_substitution_is_rejected() {
    let f = Fixture::prepare("success", 5);
    let approved = &f.plan.execution[0].checks[0];
    let roundtrip: AcceptanceCheck =
        serde_json::from_slice(&serde_json::to_vec(approved).unwrap()).unwrap();
    PinnedCheck::from_approved(&roundtrip).unwrap();
    for field in ["path", "hash", "input", "environment"] {
        let mut changed = f.plan.clone();
        let tool = changed.execution[0].checks[0].tool.as_mut().unwrap();
        match field {
            "path" => {
                tool.executable.path = f.root.path().join("other.exe").to_string_lossy().into()
            }
            "hash" => tool.executable.sha256 = "0".repeat(64),
            "input" => tool.inputs.push(ValidationFile {
                path: f.root.path().join("script.txt").to_string_lossy().into(),
                sha256: "1".repeat(64),
            }),
            "environment" => {
                tool.environment.insert("CI".into(), "true".into());
            }
            _ => unreachable!(),
        }
        assert!(f.approval.validate(&changed, "user", 1).is_err(), "{field}");
    }
    let copy = tempfile::tempdir().unwrap();
    let replacement = copy.path().join("same-content.exe");
    fs::copy(std::env::current_exe().unwrap(), &replacement).unwrap();
    assert!(PinnedCheck::new(approved, &replacement, &[])
        .unwrap_err()
        .contains("differs"));
    let mut missing = approved.clone();
    missing.tool = None;
    assert!(PinnedCheck::from_approved(&missing).is_err());
    let mut wrong_hash = approved.clone();
    wrong_hash.tool.as_mut().unwrap().executable.sha256 = "0".repeat(64);
    assert!(PinnedCheck::from_approved(&wrong_hash).is_err());
}

#[test]
fn approved_environment_reaches_real_child_and_rejects_loader_or_secret_settings() {
    let mut f = Fixture::prepare("environment", 5);
    let tool = f.plan.execution[0].checks[0].tool.as_mut().unwrap();
    tool.environment = [
        ("CI".into(), "true".into()),
        ("NO_COLOR".into(), "1".into()),
        ("RUST_BACKTRACE".into(), "full".into()),
    ]
    .into();
    for (key, value) in [
        ("PATH", "x"),
        ("NODE_OPTIONS", "--require=x"),
        ("OPENAI_API_KEY", "placeholder"),
        ("HTTP_PROXY", "x"),
        ("TEMP", "x"),
        ("CI", "true\n"),
        ("ci", "true"),
    ] {
        let mut invalid = tool.clone();
        invalid.environment.insert(key.into(), value.into());
        assert!(invalid.validate().is_err(), "{key}");
    }
    f.approval.revision_digest = f.plan.approval_digest().unwrap();
    f.attempt.revision_digest = f.approval.revision_digest.clone();
    let evidence = tempfile::tempdir().unwrap();
    let report = f.run(evidence.path(), &AtomicBool::new(false)).unwrap();
    report
        .require_pass(&f.plan, &f.observer().unwrap().capture().unwrap(), "core")
        .unwrap();
    let output = fs::read_to_string(evidence.path().join("verify-1/check.stdout")).unwrap();
    assert!(output.contains("approved environment observed"));
}

#[test]
fn approved_input_set_cannot_be_omitted_or_rehashed_after_edit() {
    let mut f = Fixture::prepare("success", 5);
    let external = tempfile::tempdir().unwrap();
    let script = external.path().join("check.txt");
    fs::write(&script, "before").unwrap();
    let exe = std::env::current_exe().unwrap();
    let check = &mut f.plan.execution[0].checks[0];
    check.tool = Some(
        PinnedCheck::describe(&exe, std::slice::from_ref(&script), Default::default()).unwrap(),
    );
    assert!(PinnedCheck::new(check, &exe, &[]).is_err());
    PinnedCheck::from_approved(check).unwrap();
    fs::write(&script, "after!").unwrap();
    assert!(PinnedCheck::from_approved(check).is_err());
}
#[test]
fn failure_and_timeout_are_real_observations() {
    for mode in ["failure", "sleep"] {
        let f = Fixture::prepare(mode, 1);
        let evidence = tempfile::tempdir().unwrap();
        let start = std::time::Instant::now();
        let report = f.run(evidence.path(), &AtomicBool::new(false)).unwrap();
        assert!(report
            .require_pass(&f.plan, &f.observer().unwrap().capture().unwrap(), "core")
            .is_err());
        if mode == "failure" {
            assert_eq!(report.checks[0].exit_code, Some(7));
        } else {
            assert!(report.checks[0].timed_out);
            assert!(start.elapsed() < Duration::from_secs(6));
        }
    }
}
#[test]
fn output_flood_cannot_pass() {
    {
        let f = Fixture::prepare("flood", 5);
        let evidence = tempfile::tempdir().unwrap();
        let error = f.run(evidence.path(), &AtomicBool::new(false)).unwrap_err();
        assert!(error.contains("output-limit"), "{error}");
        assert!(
            fs::metadata(evidence.path().join("verify-1/check.stdout"))
                .unwrap()
                .len()
                <= 1024 * 1024
        );
        let record: serde_json::Value = serde_json::from_slice(
            &fs::read(evidence.path().join("verify-1/result.json")).unwrap(),
        )
        .unwrap();
        assert!(record["report"].is_null());
    }
}

#[test]
fn snapshot_existing_inputs_cannot_be_overwritten_and_whole_tree_is_bound() {
    use cerulean_core::source_snapshot::require_sealed;
    let f = Fixture::prepare("mutate", 5);
    let evidence = tempfile::tempdir().unwrap();
    fs::write(f.root.path().join("dependency.txt"), "version-one").unwrap();
    let manifest = f.observer().unwrap().capture().unwrap();
    let report = f.run(evidence.path(), &AtomicBool::new(false)).unwrap();
    let seal = report.source_snapshot.as_ref().unwrap();
    report.require_pass(&f.plan, &manifest, "core").unwrap();
    require_sealed(&f.plan, &manifest, seal, evidence.path()).unwrap();
    let log = fs::read_to_string(evidence.path().join("verify-1/check.stdout")).unwrap();
    assert!(log.contains("snapshot write denied"));
    assert_eq!(
        fs::read_to_string(f.root.path().join("src/result.txt")).unwrap(),
        "abc"
    );
    fs::write(f.root.path().join("dependency.txt"), "version-two").unwrap();
    assert!(require_sealed(&f.plan, &manifest, seal, evidence.path()).is_err());
    fs::write(f.root.path().join("dependency.txt"), "version-one").unwrap();
    fs::write(f.root.path().join("new-input.txt"), "new").unwrap();
    assert!(require_sealed(&f.plan, &manifest, seal, evidence.path()).is_err());
    fs::remove_file(f.root.path().join("new-input.txt")).unwrap();
    fs::write(
        evidence.path().join("verify-1/inputs/dependency.txt"),
        "tampered",
    )
    .unwrap();
    assert!(require_sealed(&f.plan, &manifest, seal, evidence.path()).is_err());
}

#[test]
fn excluded_outputs_are_explicit_and_artifacts_cannot_be_excluded() {
    let mut f = Fixture::prepare("success", 5);
    let evidence = tempfile::tempdir().unwrap();
    f.plan.snapshot_policy.as_mut().unwrap().exclude = vec!["build-output".into()];
    f.approval.revision_digest = f.plan.approval_digest().unwrap();
    f.attempt.revision_digest = f.approval.revision_digest.clone();
    fs::create_dir(f.root.path().join("build-output")).unwrap();
    fs::write(f.root.path().join("build-output/not-input"), "ignored").unwrap();
    let manifest = f.observer().unwrap().capture().unwrap();
    let report = f.run(evidence.path(), &AtomicBool::new(false)).unwrap();
    assert!(!evidence.path().join("verify-1/work/build-output").exists());
    fs::write(
        f.root.path().join("build-output/not-input"),
        "changed output",
    )
    .unwrap();
    cerulean_core::source_snapshot::require_sealed(
        &f.plan,
        &manifest,
        report.source_snapshot.as_ref().unwrap(),
        evidence.path(),
    )
    .unwrap();
    f.plan
        .snapshot_policy
        .as_mut()
        .unwrap()
        .exclude
        .push("src".into());
    assert!(f.plan.validate().unwrap_err().contains("artifact excluded"));
    f.plan.snapshot_policy.as_mut().unwrap().exclude = vec![];
    assert!(f.approval.validate(&f.plan, "user", 1).is_err());
}

#[test]
fn snapshot_limits_and_external_links_fail_before_check_runs() {
    for limit in ["files", "bytes", "junction"] {
        let mut f = Fixture::prepare("success", 5);
        let evidence = tempfile::tempdir().unwrap();
        if limit == "files" {
            f.plan.snapshot_policy.as_mut().unwrap().max_files = 1;
        }
        if limit == "bytes" {
            f.plan.snapshot_policy.as_mut().unwrap().max_bytes = 1;
        }
        let outside = tempfile::tempdir().unwrap();
        if limit == "junction" {
            use std::os::windows::process::CommandExt;
            let output = std::process::Command::new("cmd.exe")
                .args(["/c", "mklink", "/J"])
                .arg(f.root.path().join("external"))
                .arg(outside.path())
                .creation_flags(0x08000000)
                .output()
                .unwrap();
            assert!(output.status.success());
        }
        f.approval.revision_digest = f.plan.approval_digest().unwrap();
        f.attempt.revision_digest = f.approval.revision_digest.clone();
        let error = f.run(evidence.path(), &AtomicBool::new(false)).unwrap_err();
        assert!(
            error.contains(if limit == "junction" {
                "reparse"
            } else {
                "limit"
            }),
            "{error}"
        );
        assert!(!evidence.path().join("verify-1/check-temp/count").exists());
        if limit == "junction" {
            fs::remove_dir(f.root.path().join("external")).unwrap();
        }
    }
}
#[test]
fn leftover_descendant_is_killed_and_report_is_rejected() {
    let f = Fixture::prepare("descendant", 5);
    let evidence = tempfile::tempdir().unwrap();
    let error = f.run(evidence.path(), &AtomicBool::new(false)).unwrap_err();
    assert!(error.contains("descendants-left-running"), "{error}");
    std::thread::sleep(Duration::from_millis(1400));
    assert!(!evidence.path().join("verify-1/check-temp/escaped").exists());
}
#[test]
fn cancellation_stops_real_process_and_prelaunch_cancel_has_no_effect() {
    let f = Fixture::prepare("sleep", 10);
    let evidence = tempfile::tempdir().unwrap();
    let cancel = AtomicBool::new(false);
    let error = std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(200));
            cancel.store(true, std::sync::atomic::Ordering::Release);
        });
        f.run(evidence.path(), &cancel).unwrap_err()
    });
    assert!(error.contains("cancelled"), "{error}");
    let f = Fixture::prepare("success", 5);
    let evidence = tempfile::tempdir().unwrap();
    assert!(f.run(evidence.path(), &AtomicBool::new(true)).is_err());
    assert!(!f.root.path().join("count").exists());
}
#[test]
fn pinned_validation_source_change_refuses_before_spawn() {
    let mut f = Fixture::prepare("success", 5);
    let evidence = tempfile::tempdir().unwrap();
    let source = evidence.path().join("trusted-script.txt");
    fs::write(&source, "original").unwrap();
    f.plan.execution[0].checks[0].tool = Some(
        PinnedCheck::describe(
            &std::env::current_exe().unwrap(),
            std::slice::from_ref(&source),
            Default::default(),
        )
        .unwrap(),
    );
    f.approval.revision_digest = f.plan.approval_digest().unwrap();
    f.attempt.revision_digest = f.approval.revision_digest.clone();
    let pins = [PinnedCheck::new(
        &f.plan.execution[0].checks[0],
        &std::env::current_exe().unwrap(),
        std::slice::from_ref(&source),
    )
    .unwrap()];
    fs::write(source, "changed").unwrap();
    let manifest = f.observer().unwrap().capture().unwrap();
    let error = verify(
        VerificationInput {
            plan: &f.plan,
            approval: &f.approval,
            authenticated_user: "user",
            policy_version: 1,
            attempt: &f.attempt,
            receipt: &f.receipt,
            manifest: &manifest,
            verifier_id: "core",
        },
        &pins,
        evidence.path(),
        "verify-1",
        &AtomicBool::new(false),
    )
    .unwrap_err();
    assert!(error.contains("validator source changed"));
    assert!(!f.root.path().join("count").exists());
}

#[test]
#[ignore = "explicit process fixture; only run by parent integration tests"]
#[allow(clippy::zombie_processes)] // Deliberately orphan a child to test Job cleanup.
fn runner_child() {
    let mode = fs::read_to_string("mode").unwrap();
    let temp = std::path::PathBuf::from(std::env::var_os("TEMP").unwrap());
    fs::write(
        temp.join("validator-pid"),
        format!("{}\n", std::process::id()),
    )
    .unwrap();
    assert!(std::env::var_os("PATH").is_none());
    assert!(std::env::var_os("OPENAI_API_KEY").is_none());
    let mut stdin = String::new();
    std::io::stdin().read_to_string(&mut stdin).unwrap();
    assert!(stdin.is_empty());
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(temp.join("count"))
        .unwrap()
        .write_all(b"x")
        .unwrap();
    match mode.as_str() {
        "success" => println!("real validator output"),
        "environment" => {
            assert_eq!(std::env::var("CI").unwrap(), "true");
            assert_eq!(std::env::var("NO_COLOR").unwrap(), "1");
            assert_eq!(std::env::var("RUST_BACKTRACE").unwrap(), "full");
            assert!(std::env::var_os("NODE_OPTIONS").is_none());
            println!("approved environment observed");
        }
        "failure" => std::process::exit(7),
        "sleep" => std::thread::sleep(Duration::from_secs(30)),
        "mutate" => {
            assert!(fs::write("src/result.txt", "xyz").is_err());
            println!("snapshot write denied");
        }
        "flood" => {
            let block = [b'x'; 8192];
            for _ in 0..400 {
                let _ = std::io::stdout().write_all(&block);
            }
        }
        "descendant" => {
            use std::os::windows::process::CommandExt;
            let mut child = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--ignored", "--exact", "runner_grandchild", "--nocapture"])
                .creation_flags(0x08000000)
                .spawn()
                .unwrap();
            for _ in 0..100 {
                if temp.join("grandchild-ready").exists() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = child.kill();
            let _ = child.wait();
            panic!("grandchild failed to start");
        }
        _ => panic!("unknown fixture"),
    }
}
#[test]
#[ignore = "explicit descendant fixture"]
fn runner_grandchild() {
    let temp = std::path::PathBuf::from(std::env::var_os("TEMP").unwrap());
    fs::write(temp.join("grandchild-ready"), "ready").unwrap();
    std::thread::sleep(Duration::from_secs(1));
    fs::write(temp.join("escaped"), "bad").unwrap();
}

#[test]
fn owner_process_crash_kills_job_child_and_leaves_unfinished_intent() {
    use std::os::windows::{
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::CommandExt,
    };
    use windows_sys::Win32::{
        Foundation::WAIT_OBJECT_0,
        System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
    };
    let base = tempfile::tempdir().unwrap();
    let mut owner = OwnedTestProcess(
        std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "runner_owner", "--nocapture"])
            .env("CERULEAN_TEST_RUNNER_OWNER", base.path())
            .creation_flags(0x08000000)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap(),
    );
    let mut child = None;
    for _ in 0..300 {
        if base.path().join("project-path").exists() {
            if let Some(pid) = read_ready_pid(
                &base
                    .path()
                    .join("evidence/verify-1/check-temp/validator-pid"),
            ) {
                let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
                if !raw.is_null() {
                    child = Some(unsafe { OwnedHandle::from_raw_handle(raw) });
                    break;
                }
            }
        }
        if owner.0.try_wait().unwrap().is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let _ = owner.0.kill();
    let _ = owner.0.wait();
    let child = child.expect("owned validator must have started before owner was killed");
    assert_eq!(
        unsafe { WaitForSingleObject(child.as_raw_handle(), 5000) },
        WAIT_OBJECT_0
    );
    assert!(base.path().join("evidence/verify-1/intent.json").exists());
    assert!(!base.path().join("evidence/verify-1/result.json").exists());
    // A durable operation claim cannot be silently recreated after the owner died.
    assert!(fs::create_dir(base.path().join("evidence/verify-1")).is_err());
    let retry = Fixture::prepare("success", 5);
    let manifest = retry.observer().unwrap().capture().unwrap();
    let pins = [PinnedCheck::new(
        &retry.plan.execution[0].checks[0],
        &std::env::current_exe().unwrap(),
        &[],
    )
    .unwrap()];
    let error = verify(
        VerificationInput {
            plan: &retry.plan,
            approval: &retry.approval,
            authenticated_user: "user",
            policy_version: 1,
            attempt: &retry.attempt,
            receipt: &retry.receipt,
            manifest: &manifest,
            verifier_id: "core",
        },
        &pins,
        &base.path().join("evidence"),
        "different-id",
        &AtomicBool::new(false),
    )
    .unwrap_err();
    assert!(error.contains("unfinished verification"), "{error}");
    assert!(!retry.root.path().join("count").exists());
}
#[test]
#[ignore = "explicit crashing verifier owner fixture"]
fn runner_owner() {
    let base = std::path::PathBuf::from(
        std::env::var_os("CERULEAN_TEST_RUNNER_OWNER").expect("parent fixture directory"),
    );
    let mut f = Fixture::prepare("sleep", 30);
    f.root = tempfile::tempdir_in(&base).unwrap();
    fs::create_dir(f.root.path().join("src")).unwrap();
    fs::write(f.root.path().join("src/result.txt"), "abc").unwrap();
    fs::write(f.root.path().join("mode"), "sleep").unwrap();
    f.plan.project.root = f.root.path().to_string_lossy().into();
    f.attempt.binding.workspace = f.plan.project.root.clone();
    f.receipt.binding.workspace = f.plan.project.root.clone();
    f.approval.revision_digest = f.plan.approval_digest().unwrap();
    f.attempt.revision_digest = f.approval.revision_digest.clone();
    fs::write(base.join("project-path"), &f.plan.project.root).unwrap();
    let evidence = base.join("evidence");
    fs::create_dir(&evidence).unwrap();
    f.run(&evidence, &AtomicBool::new(false)).unwrap();
}
