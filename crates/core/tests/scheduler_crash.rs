use cerulean_core::scheduler::*;
use cerulean_domain::workflow::*;
use serde_json::json;
use std::{
    fs::{self, OpenOptions},
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};

fn config() -> SchedulerConfig {
    SchedulerConfig {
        schema_version: 1,
        core_id: "core".into(),
        execution_scope: None,
        principals: vec![
            Principal {
                id: "user".into(),
                role: Role::User,
                provider: None,
            },
            Principal {
                id: "chief".into(),
                role: Role::Chief,
                provider: Some("openai".into()),
            },
            Principal {
                id: "deputy".into(),
                role: Role::Deputy,
                provider: Some("openai".into()),
            },
            Principal {
                id: "core".into(),
                role: Role::Core,
                provider: None,
            },
            Principal {
                id: "worker".into(),
                role: Role::Worker,
                provider: None,
            },
        ],
    }
}
fn initialize(root: &Path) -> DurableWorkflow {
    let mut s = DurableWorkflow::open(root, config()).unwrap();
    s.command(
        "plan",
        "chief",
        WorkflowCommand::Propose {
            plan: WorkflowPlan {
                id: "plan".into(),
                version: 1,
                tasks: vec![WorkflowTask {
                    id: "a".into(),
                    dependencies: vec![],
                    workspace_key: "space".into(),
                    goal: "synthetic fault fixture".into(),
                    resource_keys: None,
                }],
            },
        },
        CommandContext::default(),
        false,
    )
    .unwrap();
    s.command(
        "approval",
        "user",
        WorkflowCommand::Approve {
            digest: s.state().digest.clone().unwrap(),
        },
        CommandContext {
            approval_source: Some("synthetic process test".into()),
            ..Default::default()
        },
        false,
    )
    .unwrap();
    s.command(
        "assign",
        "deputy",
        WorkflowCommand::Assign {
            task: "a".into(),
            worker: "worker".into(),
            attempt: "attempt".into(),
        },
        CommandContext::default(),
        false,
    )
    .unwrap();
    s
}
fn intent() -> WorkflowCommand {
    WorkflowCommand::DispatchIntent {
        task: "a".into(),
        attempt: "attempt".into(),
    }
}
fn context() -> CommandContext {
    CommandContext {
        thread_id: Some("external-session".into()),
        cwd: Some("fixture-workspace".into()),
        ..Default::default()
    }
}

#[cfg(windows)]
#[test]
fn projection_denial_preserves_committed_intent_and_blocks_redispatch() {
    use std::os::windows::fs::OpenOptionsExt;
    let root = tempfile::tempdir().unwrap();
    let mut store = initialize(root.path());
    store.wait_for_projections(Duration::from_secs(20)).unwrap();
    let previous = store.history().len();
    let held = OpenOptions::new()
        .read(true)
        .share_mode(3)
        .open(root.path().join("state.json"))
        .unwrap();
    store
        .command("dispatch", "core", intent(), context(), true)
        .unwrap();
    let error = store
        .wait_for_projections(Duration::from_secs(20))
        .unwrap_err();
    // A legitimate idempotent readback succeeds; the writer is not poisoned.
    let assignment = store.history()[2].clone();
    assert_eq!(
        store
            .command(
                "assign",
                "deputy",
                assignment.body.event.command.clone(),
                CommandContext::default(),
                false
            )
            .unwrap(),
        assignment
    );
    store.wait_for_index(Duration::from_secs(20)).unwrap();
    assert!(
        !fs::read_dir(root.path()).unwrap().any(|entry| entry
            .unwrap()
            .path()
            .extension()
            .is_some_and(|e| e == "tmp")),
        "failed derived writes must not accumulate owned temporary files"
    );
    assert!(
        error.contains("projection replacement failed") && error.contains("os error 5"),
        "{error}"
    );
    assert_eq!(store.history().len(), previous + 1);
    let original = store.history().last().unwrap().clone();
    assert!(store
        .command("retry", "core", intent(), context(), true)
        .is_err());
    assert_eq!(store.history().len(), previous + 1);
    drop(held);
    drop(store);
    let mut recovered = DurableWorkflow::open(root.path(), config()).unwrap();
    assert_eq!(recovered.history()[previous], original);
    assert_eq!(recovered.state().tasks["a"].state, TaskState::Unknown);
    assert!(recovered
        .command("retry", "core", intent(), context(), true)
        .is_err());
    assert_eq!(
        recovered
            .history()
            .iter()
            .filter(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. }))
            .count(),
        1
    );
    let count = recovered.history().len();
    drop(recovered);
    let recovered = DurableWorkflow::open(root.path(), config()).unwrap();
    assert_eq!(recovered.history().len(), count);
}
fn root(name: &str) -> PathBuf {
    let reports = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports");
    fs::create_dir_all(&reports).unwrap();
    tempfile::Builder::new()
        .prefix(name)
        .tempdir_in(reports)
        .unwrap()
        .keep()
}
struct Process {
    child: Child,
    lines: Receiver<String>,
}
impl Process {
    fn spawn(role: &str, root: &Path, point: &str) -> Self {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--ignored",
                "--exact",
                "child_fixture",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("CERULEAN_FIXTURE_ROLE", role)
            .env("CERULEAN_FIXTURE_ROOT", root)
            .env("CERULEAN_FIXTURE_POINT", point)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Some(msg) = line.strip_prefix("CL:") {
                    let _ = tx.send(msg.into());
                }
            }
        });
        Self { child, lines }
    }
    fn line(&self) -> String {
        self.lines
            .recv_timeout(Duration::from_secs(10))
            .expect("fixture observation deadline")
    }
    fn send(&mut self, line: &str) {
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
    }
    fn exit(&mut self) -> i32 {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status.code().unwrap_or(-1);
            }
            assert!(Instant::now() < until, "fixture failed to exit");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}
fn emit(message: &str) {
    // Rust's test harness may leave its test-name prefix on the current line.
    println!("\nCL:{message}");
    std::io::stdout().flush().unwrap();
}

#[test]
#[ignore = "helper invoked only by crash integration tests in a dedicated process"]
fn child_fixture() {
    let role = std::env::var("CERULEAN_FIXTURE_ROLE").unwrap();
    let root = PathBuf::from(std::env::var_os("CERULEAN_FIXTURE_ROOT").unwrap());
    let point = std::env::var("CERULEAN_FIXTURE_POINT").unwrap();
    if role == "worker" {
        emit("READY");
        let mut count = 0;
        for line in std::io::stdin().lock().lines() {
            assert_eq!(line.unwrap(), "EXECUTE");
            count += 1;
            // Deliberately no deduplication: every received request creates another effect.
            let receipt = json!({"attempt":"attempt","threadId":"external-session","status":"completed","execution":count});
            let mut log = OpenOptions::new()
                .create(true)
                .append(true)
                .open(root.join("effects.jsonl"))
                .unwrap();
            writeln!(log, "{receipt}").unwrap();
            log.sync_all().unwrap();
            let mut file = OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(root.join("receipt.json"))
                .unwrap();
            file.write_all(receipt.to_string().as_bytes()).unwrap();
            file.sync_all().unwrap();
            emit("COMPLETED");
        }
        return;
    }
    let mut store = initialize(&root.join("scheduler"));
    if role == "storage" {
        store
            .command_observed("intent", "core", intent(), context(), true, |p| {
                if format!("{p:?}") == point {
                    std::process::exit(73);
                }
            })
            .unwrap();
        panic!("fault point was not reached");
    }
    assert_eq!(role, "controller");
    store
        .command("intent", "core", intent(), context(), true)
        .unwrap();
    if point == "before-send" {
        std::process::exit(75);
    }
    emit("SEND");
    let mut reply = String::new();
    std::io::stdin().read_line(&mut reply).unwrap();
    assert_eq!(reply.trim(), "CONTINUE");
    if point == "after-effect-before-ack" {
        std::process::exit(75);
    }
    assert_eq!(point, "after-ack");
    store
        .command(
            "ack",
            "core",
            WorkflowCommand::Started {
                task: "a".into(),
                attempt: "attempt".into(),
            },
            CommandContext {
                turn_id: Some("external-turn".into()),
                ..context()
            },
            false,
        )
        .unwrap();
    std::process::exit(75);
}

#[test]
fn four_storage_exit_points_recover_without_duplicate_commit() {
    for point in [
        "BeforeEventWrite",
        "TempSynced",
        "EventPublished",
        "ProjectionQueued",
    ] {
        let root = root("p1-storage-crash-");
        let mut process = Process::spawn("storage", &root, point);
        assert_eq!(process.exit(), 73);
        let mut s = DurableWorkflow::open(&root.join("scheduler"), config()).unwrap();
        let committed = matches!(point, "EventPublished" | "ProjectionQueued");
        if committed {
            assert_eq!(s.state().tasks["a"].state, TaskState::Unknown);
            assert_eq!(s.history().len(), 5);
            assert!(s
                .command("intent", "core", intent(), context(), false)
                .is_err());
        } else {
            assert_eq!(s.state().tasks["a"].state, TaskState::Assigned);
            assert_eq!(s.history().len(), 3);
            s.command("intent", "core", intent(), context(), true)
                .unwrap();
        }
        assert_eq!(
            s.history()
                .iter()
                .filter(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. }))
                .count(),
            1
        );
        drop(s);
        let s = DurableWorkflow::open(&root.join("scheduler"), config()).unwrap();
        assert_eq!(s.state().tasks["a"].state, TaskState::Unknown);
        assert_eq!(s.history().len(), 5);
        fs::write(root.join("review.json"),serde_json::to_vec_pretty(&json!({"point":point,"exit":73,"initialIntentCommitted":committed,"finalEvents":5,"dispatchIntents":1,"state":"unknown","modelCalls":0})).unwrap()).unwrap();
    }
}

#[test]
fn external_worker_survives_three_controller_exit_points_without_resend() {
    for point in ["before-send", "after-effect-before-ack", "after-ack"] {
        let root = root("p1-dispatch-crash-");
        let mut worker = Process::spawn("worker", &root, point);
        assert_eq!(worker.line(), "READY");
        let mut controller = Process::spawn("controller", &root, point);
        let mut sends = 0;
        if point != "before-send" {
            assert_eq!(controller.line(), "SEND");
            sends += 1;
            worker.send("EXECUTE");
            assert_eq!(worker.line(), "COMPLETED");
            controller.send("CONTINUE");
        }
        assert_eq!(controller.exit(), 75);
        assert!(worker.child.try_wait().unwrap().is_none());
        let mut s = DurableWorkflow::open(&root.join("scheduler"), config()).unwrap();
        assert_eq!(s.state().tasks["a"].state, TaskState::Unknown);
        assert!(s
            .command("intent", "core", intent(), context(), false)
            .is_err());
        assert!(s
            .command("automatic-resend", "core", intent(), context(), true)
            .is_err());
        let (outcome, proof, effects) = if sends == 0 {
            // Zero sends is observed by the live router, not inferred solely from file absence.
            assert!(!root.join("receipt.json").exists());
            (
                ReconcileOutcome::NotStarted,
                "router=0; controller-exit=75; worker-live",
                0,
            )
        } else {
            let receipt: serde_json::Value =
                serde_json::from_slice(&fs::read(root.join("receipt.json")).unwrap()).unwrap();
            assert_eq!(receipt["attempt"], "attempt");
            assert_eq!(receipt["threadId"], "external-session");
            assert_eq!(receipt["status"], "completed");
            assert_eq!(receipt["execution"], 1);
            assert_eq!(
                fs::read_to_string(root.join("effects.jsonl"))
                    .unwrap()
                    .lines()
                    .count(),
                1
            );
            (
                ReconcileOutcome::Submitted,
                "matched-completed-receipt; observed-effect-count=1",
                1,
            )
        };
        s.command(
            "reconcile",
            "core",
            WorkflowCommand::Reconcile {
                task: "a".into(),
                attempt: "attempt".into(),
                outcome,
                evidence: vec![proof.into()],
            },
            CommandContext::default(),
            false,
        )
        .unwrap();
        assert_eq!(
            s.state().tasks["a"].state,
            if sends == 0 {
                TaskState::Assigned
            } else {
                TaskState::Submitted
            }
        );
        assert!(!s.state().tasks["a"].verified);
        assert!(s
            .command(
                "premature-accept",
                "chief",
                WorkflowCommand::Accept {
                    task: "a".into(),
                    attempt: "attempt".into()
                },
                CommandContext::default(),
                false
            )
            .is_err());
        fs::write(root.join("review.json"),serde_json::to_vec_pretty(&json!({"point":point,"controllerExit":75,"workerConfirmedAlive":true,"dispatches":sends,"effects":effects,"resends":0,"automaticResendDenied":true,"reconciledState":s.state().tasks["a"].state,"modelCalls":0,"scope":"synthetic worker; real controller and worker processes; real file side effects"})).unwrap()).unwrap();
    }
}
