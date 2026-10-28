use super::*;
use crate::omp_file_tools::tests::request;

async fn file_peer(stream: DuplexStream, mode: &'static str, cancel: oneshot::Sender<()>) {
    let (r, mut w) = tokio::io::split(stream);
    let mut r = BufReader::new(r);
    let mut state = idle();
    send(&mut w,json!({"type":"ready","protocolVersion":1,"supportedProtocolVersions":[1,2],"maxFrameBytes":1024*1024,"maxReassembledFrameBytes":64*1024*1024})).await;
    let q = read(&mut r).await;
    reply(&mut w, &q, json!({"protocolVersion":2})).await;
    for _ in 0..2 {
        let q = read(&mut r).await;
        assert_eq!(q["type"], "get_state");
        reply(&mut w, &q, state.clone()).await;
    }
    let q = read(&mut r).await;
    assert_eq!(q["type"], "set_host_tools");
    let names = q["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].clone())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![json!("cerulean_read_file"), json!("cerulean_write_file")]
    );
    state["dumpTools"] = q["tools"].clone();
    reply(&mut w, &q, json!({"toolNames":names})).await;
    for _ in 0..2 {
        let q = read(&mut r).await;
        assert_eq!(q["type"], "get_state");
        reply(&mut w, &q, state.clone()).await;
    }
    let prompt = read(&mut r).await;
    assert_eq!(prompt["type"], "prompt");
    reply(&mut w, &prompt, Value::Null).await;
    send(
        &mut w,
        serde_json::to_value(request(
            "write-1",
            "write_file",
            json!({"path":"result.txt","content":"fixture output","expectedSha256":"absent"}),
        ))
        .unwrap(),
    )
    .await;
    let result = read(&mut r).await;
    assert_eq!(result["type"], "host_tool_result");
    assert_eq!(result["id"], "write-1");
    assert_eq!(result["isError"], false);
    if mode == "lost-terminal" {
        return;
    }
    if mode == "cancel" {
        cancel.send(()).unwrap();
        let q = read(&mut r).await;
        assert_eq!(q["type"], "abort");
        reply(&mut w, &q, Value::Null).await;
    }
    let status = match mode {
        "cancel" => "aborted",
        "error" => "error",
        _ => "completed",
    };
    send(&mut w,json!({"type":"prompt_result","id":prompt["id"],"status":status,"agentInvoked":true,"sessionSettled":true})).await;
    let q = read(&mut r).await;
    assert_eq!(q["type"], "get_state");
    reply(&mut w, &q, state).await;
    let mut extra = vec![];
    r.read_to_end(&mut extra).await.unwrap();
    assert!(extra.is_empty(), "no retry or extra prompt");
}

async fn run(
    store: &mut DurableWorkflow,
    mode: &'static str,
    observe: &mut impl FnMut(&str) -> Result<(), String>,
) -> Result<PromptOutcome, String> {
    let (wire, peer) = tokio::io::duplex(8192);
    let (r, w) = tokio::io::split(wire);
    let mut channel = WorkerTransport::new(BufReader::new(r), w);
    let (tx, mut rx) = oneshot::channel();
    let peer = tokio::spawn(file_peer(peer, mode, tx));
    channel.connect(Duration::from_secs(3)).await.unwrap();
    let result = dispatch_inner(
        store,
        &mut channel,
        "task",
        "attempt",
        &mut rx,
        true,
        &mut |_, at| observe(at),
    )
    .await;
    assert_eq!(channel.command_attempts().get("prompt"), Some(&1));
    channel.close_input().await.unwrap();
    peer.await.unwrap();
    result
}

#[tokio::test]
async fn file_tool_to_submission_verification_and_cancellation_chain() {
    for mode in ["completed", "cancel", "error", "lost-terminal"] {
        let root = tempfile::tempdir().unwrap();
        let (mut store, plan) = assigned(root.path(), false);
        let result = run(&mut store, mode, &mut |_| Ok(())).await;
        let expected = match mode {
            "completed" => TaskState::Submitted,
            "cancel" => TaskState::Cancelled,
            _ => TaskState::Unknown,
        };
        assert_eq!(
            result.is_ok(),
            matches!(mode, "completed" | "cancel"),
            "{mode}: {result:?}"
        );
        assert_eq!(store.state().tasks["task"].state, expected);
        assert_eq!(
            fs::read_to_string(root.path().join("project/result.txt")).unwrap(),
            "fixture output"
        );
        let (_, _, intent, _) = scope(&store, "task", "attempt").unwrap();
        let tools = store.omp_file_service_path("attempt").unwrap();
        let j =
            crate::omp_tool_journal::OmpToolJournal::open(&tools.join("events"), &intent).unwrap();
        assert_eq!(j.observation().calls.len(), 1);
        assert_eq!(j.observation().unresolved(), 0);
        let head = j.head().to_owned();
        drop(j);
        let prompt =
            OmpJournal::open(&store.omp_attempt_path("attempt").unwrap(), &intent).unwrap();
        if mode == "completed" {
            assert_eq!(prompt.completion().unwrap().tool_head.as_ref(), Some(&head));
        } else {
            assert!(prompt.completion().is_none());
        }
        drop(prompt);
        let mut store = reopen(store, root.path());
        assert_eq!(store.state().tasks["task"].state, expected);
        if mode == "completed" {
            assert!(record(
                &mut store,
                "chief",
                WorkflowCommand::Accept {
                    task: "task".into(),
                    attempt: "attempt".into()
                },
                CommandContext::default()
            )
            .is_err());
            let check =
                crate::verifier::PinnedCheck::from_approved(&plan.execution[0].checks[0]).unwrap();
            store
                .run_verification(
                    "file-tool-check",
                    "task",
                    "attempt",
                    &[check],
                    &std::sync::atomic::AtomicBool::new(false),
                )
                .unwrap();
            record(
                &mut store,
                "chief",
                WorkflowCommand::Accept {
                    task: "task".into(),
                    attempt: "attempt".into(),
                },
                CommandContext::default(),
            )
            .unwrap();
            assert_eq!(
                reopen(store, root.path()).state().tasks["task"].state,
                TaskState::Accepted
            );
        } else if expected == TaskState::Unknown {
            assert!(recover_terminal(&mut store, "task", "attempt").is_err());
            let inspection =
                crate::omp_file_tools::write_evidence::inspect(&tools, &intent, "write-1").unwrap();
            assert!(inspection.tool_result_recorded);
            assert!(crate::omp_file_tools::write_evidence::restore::review(
                &store, &tools, "task", "attempt", "write-1"
            )
            .is_ok());
        }
    }
}

#[tokio::test]
async fn original_tool_chain_is_required_for_sealed_recovery_and_never_replayed() {
    for mode in ["intact", "missing", "truncated", "corrupt"] {
        let root = tempfile::tempdir().unwrap();
        let (mut store, _) = assigned(root.path(), false);
        assert!(run(
            &mut store,
            "completed",
            &mut |at| if at == "artifacts-sealed" {
                Err("injected stop before submission".into())
            } else {
                Ok(())
            }
        )
        .await
        .is_err());
        assert_eq!(store.state().tasks["task"].state, TaskState::Unknown);
        let service = store.omp_file_service_path("attempt").unwrap();
        match mode {
            "missing" => fs::rename(&service, root.path().join("saved-tools")).unwrap(),
            "truncated" => fs::rename(
                service.join("events/001.json"),
                root.path().join("saved-result.json"),
            )
            .unwrap(),
            "corrupt" => fs::write(service.join("events/001.json"), "{}").unwrap(),
            _ => {}
        }
        let mut store = reopen(store, root.path());
        let before = store.history().len();
        let result = recover_terminal(&mut store, "task", "attempt");
        assert_eq!(result.is_ok(), mode == "intact", "{mode}: {result:?}");
        if mode != "intact" {
            assert_eq!(store.history().len(), before);
            assert_eq!(store.state().tasks["task"].state, TaskState::Unknown);
        }
        let (wire, mut peer) = tokio::io::duplex(4096);
        let (r, w) = tokio::io::split(wire);
        let mut channel = WorkerTransport::new(BufReader::new(r), w);
        let (_tx, mut rx) = oneshot::channel();
        assert!(
            dispatch_with_files(&mut store, &mut channel, "task", "attempt", &mut rx)
                .await
                .is_err()
        );
        assert!(channel.command_attempts().is_empty());
        channel.close_input().await.unwrap();
        let mut extra = vec![];
        peer.read_to_end(&mut extra).await.unwrap();
        assert!(extra.is_empty());
    }
}

#[tokio::test]
async fn unsupported_budget_creates_no_file_service_or_registration() {
    let root = tempfile::tempdir().unwrap();
    let (mut store, _) = assigned(root.path(), true);
    let (wire, mut peer) = tokio::io::duplex(4096);
    let (r, w) = tokio::io::split(wire);
    let mut channel = WorkerTransport::new(BufReader::new(r), w);
    let (_tx, mut rx) = oneshot::channel();
    let before = store.history().len();
    assert!(
        dispatch_with_files(&mut store, &mut channel, "task", "attempt", &mut rx)
            .await
            .unwrap_err()
            .contains("money enforcement")
    );
    assert_eq!(store.history().len(), before);
    assert!(!store.omp_file_service_path("attempt").unwrap().exists());
    assert!(channel.command_attempts().is_empty());
    channel.close_input().await.unwrap();
    let mut extra = vec![];
    peer.read_to_end(&mut extra).await.unwrap();
    assert!(extra.is_empty());
}

struct Owned(std::process::Child);
impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "owned OMP dispatch crash helper; explicit isolated parent and boundary required"]
fn crash_dispatch() {
    use std::io::Write;
    let root = PathBuf::from(std::env::var_os("CERULEAN_FILE_DISPATCH_ROOT").unwrap());
    let point = std::env::var("CERULEAN_FILE_DISPATCH_POINT").unwrap();
    let (mut store, _) = assigned(&root, false);
    fs::write(
        root.join("config.json"),
        serde_json::to_vec(store.configuration()).unwrap(),
    )
    .unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime
        .block_on(run(&mut store, "completed", &mut |at| {
            if at == point {
                let mut f = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(root.join("ready"))
                    .unwrap();
                f.write_all(at.as_bytes()).unwrap();
                f.sync_all().unwrap();
                loop {
                    std::thread::park_timeout(Duration::from_secs(1));
                }
            }
            Ok(())
        }))
        .unwrap();
}

#[test]
fn actual_owner_termination_from_terminal_through_seal_to_submit() {
    use std::{
        os::windows::process::CommandExt,
        process::{Command, Stdio},
        time::Instant,
    };
    for point in [
        "terminal-recorded",
        "seal-synced",
        "seal-published",
        "seal-committed",
        "artifacts-sealed",
        "workflow-applied",
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--ignored",
                "--exact",
                "omp_execution::tests::files::crash_dispatch",
                "--nocapture",
            ])
            .env("CERULEAN_FILE_DISPATCH_ROOT", root.path())
            .env("CERULEAN_FILE_DISPATCH_POINT", point)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x08000000);
        let mut owner = Owned(command.spawn().unwrap());
        let deadline = Instant::now() + Duration::from_secs(20);
        while !root.path().join("ready").exists() {
            assert!(owner.0.try_wait().unwrap().is_none(), "early exit {point}");
            assert!(Instant::now() < deadline, "timeout {point}");
            std::thread::sleep(Duration::from_millis(10));
        }
        owner.0.kill().unwrap();
        owner.0.wait().unwrap();
        let config: SchedulerConfig =
            serde_json::from_slice(&fs::read(root.path().join("config.json")).unwrap()).unwrap();
        let mut store = DurableWorkflow::open(&root.path().join("ledger"), config).unwrap();
        assert_eq!(
            fs::read_to_string(root.path().join("project/result.txt")).unwrap(),
            "fixture output"
        );
        let initial = if point == "workflow-applied" {
            TaskState::Submitted
        } else {
            TaskState::Unknown
        };
        assert_eq!(store.state().tasks["task"].state, initial);
        let count = store.history().len();
        let result = recover_terminal(&mut store, "task", "attempt");
        if matches!(point, "seal-committed" | "artifacts-sealed") {
            result.unwrap();
            assert_eq!(store.state().tasks["task"].state, TaskState::Submitted);
        } else {
            assert!(result.is_err(), "{point}");
            assert_eq!(store.history().len(), count);
        }
        let count = store.history().len();
        assert!(recover_terminal(&mut store, "task", "attempt").is_err());
        assert_eq!(store.history().len(), count);
        let (_, _, intent, _) = scope(&store, "task", "attempt").unwrap();
        let journal = crate::omp_tool_journal::OmpToolJournal::open(
            &store
                .omp_file_service_path("attempt")
                .unwrap()
                .join("events"),
            &intent,
        )
        .unwrap();
        assert_eq!(journal.observation().calls.len(), 1);
        assert_eq!(journal.observation().unresolved(), 0);
    }
}
