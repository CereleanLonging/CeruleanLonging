use super::*;
use crate::omp_execution::tests::assigned;
use cerulean_adapters::omp_transport::worker::{PromptOutcome, WorkerTransport};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
const PROMPT: &str = "Write the approved fixture";
#[test]
fn file_tool_contract_identifies_the_assigned_relative_root() {
    let temp = tempfile::tempdir().unwrap();
    let (service, _, _, _) = fixture(temp.path());
    let definitions = service.definitions();
    assert_eq!(definitions.len(), 2);
    for definition in definitions {
        assert!(definition
            .description
            .contains("host has already selected the directory"));
        assert!(definition.parameters["properties"]["path"]["description"]
            .as_str()
            .unwrap()
            .contains("relative to the assigned execution directory"));
    }
}
#[tokio::test]
async fn live_tool_checkpoints_wait_for_complete_prefix_and_verify_original_scope() {
    let temp = tempfile::tempdir().unwrap();
    let (mut service, intent, plan, _) = fixture(temp.path());
    let (sender, receiver) = std::sync::mpsc::channel();
    service.observe_completed_tools(Box::new(move |checkpoint| {
        sender
            .send(checkpoint)
            .map(|_| None)
            .map_err(|_| "observer closed".into())
    }));
    let a = request("a", "read_file", json!({"path":"missing.txt","offset":0}));
    let b = request("b", "read_file", json!({"path":"missing.txt","offset":0}));
    service.record(&intent, &dispatch_event(&a)).unwrap();
    service.record(&intent, &dispatch_event(&b)).unwrap();
    let (_cancel, rx) = watch::channel(false);
    let a = service.start(a, rx.clone()).await.unwrap();
    assert!(a.is_error);
    service
        .record(
            &intent,
            &HostEvent::Result {
                id: "a".into(),
                result_sha256: wire_hash("a", &a),
                is_error: true,
            },
        )
        .unwrap();
    assert!(receiver.try_recv().is_err());
    let b = service.start(b, rx).await.unwrap();
    service
        .record(
            &intent,
            &HostEvent::Result {
                id: "b".into(),
                result_sha256: wire_hash("b", &b),
                is_error: true,
            },
        )
        .unwrap();
    let checkpoint = receiver.try_recv().unwrap();
    assert!(receiver.try_recv().is_err());
    assert_eq!(checkpoint.tools.observation().unresolved(), 0);
    let policy = cerulean_domain::supervision::SupervisionPolicy {
        schema_version: 1,
        repeated_failure_threshold: 2,
        max_advice_per_attempt: 1,
        delivery_enabled: false,
    };
    assert_eq!(
        checkpoint
            .tools
            .supervision_incidents(&policy)
            .unwrap()
            .len(),
        1
    );
    let evidence = checkpoint.evidence();
    // This low-level fixture uses a different prompt; it must not impersonate
    // the product's approved task prompt, even with the same plan and attempt.
    assert!(evidence.validate_plan(&plan).is_err());
    let mut changed = plan.clone();
    changed.policy.version += 1;
    assert!(evidence.validate_plan(&changed).is_err());
    drop(service);
    evidence
        .verify_original(&temp.path().join("tools"))
        .unwrap();
    let mut altered = serde_json::to_value(&evidence).unwrap();
    altered["intent"]["attemptId"] = json!("other");
    let altered: crate::omp_tool_checkpoint::OmpToolEvidence =
        serde_json::from_value(altered).unwrap();
    assert!(altered.verify_original(&temp.path().join("tools")).is_err());
}

#[tokio::test]
async fn failed_checkpoint_notification_preserves_result_without_replaying_tool() {
    let temp = tempfile::tempdir().unwrap();
    let (mut service, intent, _, _) = fixture(temp.path());
    service.observe_completed_tools(Box::new(|_| Err("original host disconnected".into())));
    let request = request("a", "read_file", json!({"path":"missing.txt","offset":0}));
    service.record(&intent, &dispatch_event(&request)).unwrap();
    let (_cancel, rx) = watch::channel(false);
    let output = service.start(request, rx).await.unwrap();
    let event = HostEvent::Result {
        id: "a".into(),
        result_sha256: wire_hash("a", &output),
        is_error: output.is_error,
    };
    assert!(service
        .record(&intent, &event)
        .unwrap_err()
        .contains("disconnected"));
    assert!(service.observation().calls["a"].result_sha256.is_some());
    let head = service.journal.head().to_string();
    drop(service);
    let evidence =
        OmpToolJournal::read_evidence(&temp.path().join("tools/events"), &intent, &head).unwrap();
    assert_eq!(evidence.observation().calls.len(), 1);
    assert_eq!(evidence.observation().unresolved(), 0);
}
pub(super) fn fixture(root: &Path) -> (OmpFileTools, DispatchIntent, PlanRevision, Approval) {
    let (store, plan) = assigned(root, false);
    let attempt = store.attempt_record("task", "attempt").unwrap();
    let intent = DispatchIntent {
        version: 1,
        task_id: "task".into(),
        attempt_id: "attempt".into(),
        revision_digest: plan.approval_digest().unwrap(),
        fence: attempt.fence,
        binding: attempt.binding.clone(),
        provider: "deepseek".into(),
        model: "fixture".into(),
        prompt_sha256: hash(PROMPT.as_bytes()),
    };
    let approval = store.approval().unwrap().clone();
    let service =
        OmpFileTools::create(&plan, &approval, "user", 1, &intent, &root.join("tools")).unwrap();
    (service, intent, plan, approval)
}
pub(crate) fn request(id: &str, name: &str, args: Value) -> HostRequest {
    HostRequest::HostToolCall {
        id: id.into(),
        tool_call_id: format!("model-{id}"),
        tool_name: format!("cerulean_{name}"),
        arguments: args.as_object().unwrap().clone(),
    }
}
pub(crate) fn dispatch_event(request: &HostRequest) -> HostEvent {
    let HostRequest::HostToolCall {
        id,
        tool_call_id,
        tool_name,
        arguments,
    } = request
    else {
        panic!()
    };
    HostEvent::Dispatch {
        id: id.clone(),
        tool_call_id: tool_call_id.clone(),
        tool_name: tool_name.clone(),
        arguments_sha256: digest(arguments).unwrap(),
    }
}
pub(super) async fn run(
    service: &mut OmpFileTools,
    intent: &DispatchIntent,
    request: HostRequest,
    cancelled: bool,
) -> HostOutput {
    service.record(intent, &dispatch_event(&request)).unwrap();
    let HostRequest::HostToolCall { id, .. } = &request else {
        panic!()
    };
    let id = id.clone();
    let (_tx, rx) = watch::channel(cancelled);
    let output = service.start(request, rx).await.unwrap();
    service
        .record(
            intent,
            &HostEvent::Result {
                id: id.clone(),
                result_sha256: wire_hash(&id, &output),
                is_error: output.is_error,
            },
        )
        .unwrap();
    output
}
#[tokio::test]
async fn actual_create_read_update_and_stale_version_protect_user_edits() {
    let temp = tempfile::tempdir().unwrap();
    let (mut service, intent, plan, _) = fixture(temp.path());
    let write = request(
        "1",
        "write_file",
        json!({"path":"src/main.rs","content":"fn main() {}\n","expectedSha256":"absent"}),
    );
    let output = run(&mut service, &intent, write, false).await;
    assert!(!output.is_error, "{}", output.text);
    let path = Path::new(&plan.project.root).join("src/main.rs");
    assert_eq!(fs::read_to_string(&path).unwrap(), "fn main() {}\n");
    let read = run(
        &mut service,
        &intent,
        request("2", "read_file", json!({"path":"src/main.rs","offset":0})),
        false,
    )
    .await;
    let data: Value = serde_json::from_str(&read.text).unwrap();
    assert_eq!(data["sha256"], hash(b"fn main() {}\n"));
    fs::write(&path, "user update").unwrap();
    let conflict = run(
        &mut service,
        &intent,
        request(
            "3",
            "write_file",
            json!({"path":"src/main.rs","content":"overwrite","expectedSha256":data["sha256"]}),
        ),
        false,
    )
    .await;
    assert!(conflict.is_error);
    assert_eq!(fs::read_to_string(&path).unwrap(), "user update");
    let success=run(&mut service,&intent,request("4","write_file",json!({"path":"src/main.rs","content":"approved update","expectedSha256":hash(b"user update")})),false).await;
    assert!(!success.is_error);
    assert_eq!(fs::read_to_string(&path).unwrap(), "approved update");
    assert_eq!(service.observation().unresolved(), 0);
    drop(service);
    let journal = OmpToolJournal::open(&temp.path().join("tools/events"), &intent).unwrap();
    assert_eq!(journal.observation().calls.len(), 4);
    let evidence = fs::read_to_string(temp.path().join("tools/events/000.json")).unwrap();
    assert!(!evidence.contains("fn main"));
}
#[tokio::test]
async fn traversal_links_protected_files_unknown_fields_and_cancel_do_not_write() {
    let temp = tempfile::tempdir().unwrap();
    let (service, intent, plan, approval) = fixture(temp.path());
    drop(service);
    let project = Path::new(&plan.project.root);
    let outside = temp.path().join("outside.txt");
    fs::write(&outside, "outside").unwrap();
    fs::hard_link(&outside, project.join("alias.txt")).unwrap();
    let external_dir = temp.path().join("external");
    fs::create_dir(&external_dir).unwrap();
    fs::write(external_dir.join("data.txt"), "outside").unwrap();
    let mut junction = std::process::Command::new("cmd.exe");
    use std::os::windows::process::CommandExt;
    let output = junction
        .args(["/d", "/c", "mklink", "/J"])
        .arg(project.join("linked"))
        .arg(&external_dir)
        .creation_flags(0x08000000)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "junction fixture could not be created"
    );
    let mut service = OmpFileTools::create(
        &plan,
        &approval,
        "user",
        1,
        &intent,
        &temp.path().join("tools-linked"),
    )
    .unwrap();
    for (i, path) in [
        "../outside.txt",
        ".env",
        ".git/config",
        "auth.json",
        "sub/../escape",
        "x:stream",
        "C:/outside",
        "ALIAS~1.TXT",
        "alias.txt",
        "linked/data.txt",
    ]
    .iter()
    .enumerate()
    {
        let output = run(
            &mut service,
            &intent,
            request(
                &format!("deny{i}"),
                "write_file",
                json!({"path":path,"content":"bad","expectedSha256":hash(b"outside")}),
            ),
            false,
        )
        .await;
        assert!(output.is_error, "{path}");
    }
    assert_eq!(fs::read_to_string(outside).unwrap(), "outside");
    assert_eq!(
        fs::read_to_string(external_dir.join("data.txt")).unwrap(),
        "outside"
    );
    let output = run(
        &mut service,
        &intent,
        request(
            "bad-fields",
            "write_file",
            json!({"path":"result.txt","content":"bad","expectedSha256":"absent","extra":true}),
        ),
        false,
    )
    .await;
    assert!(output.is_error);
    let output = run(
        &mut service,
        &intent,
        request(
            "cancelled",
            "write_file",
            json!({"path":"result.txt","content":"bad","expectedSha256":"absent"}),
        ),
        true,
    )
    .await;
    assert!(output.is_error);
    assert!(!project.join("result.txt").exists());
    // Directory handles prevent replacing a pinned root with another tree.
    assert!(fs::rename(project, temp.path().join("renamed")).is_err());
}
#[tokio::test]
async fn missing_or_changed_dispatch_never_grants_file_access_and_dropped_future_does_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let (mut service, intent, plan, _) = fixture(temp.path());
    let request = request(
        "1",
        "write_file",
        json!({"path":"result.txt","content":"output","expectedSha256":"absent"}),
    );
    let (_tx, rx) = watch::channel(false);
    assert!(service.start(request.clone(), rx.clone()).await.is_err());
    service.record(&intent, &dispatch_event(&request)).unwrap();
    let future = service.start(request.clone(), rx.clone());
    drop(future);
    assert!(!Path::new(&plan.project.root).join("result.txt").exists());
    assert!(service.start(request, rx).await.is_err());
    assert_eq!(service.observation().unresolved(), 1);
}
#[test]
fn altered_approval_or_model_is_rejected_before_journal_creation() {
    let temp = tempfile::tempdir().unwrap();
    let (service, intent, plan, approval) = fixture(temp.path());
    drop(service);
    let mut changed = intent.clone();
    changed.model = "another".into();
    for (scope, user) in [(&changed, "user"), (&intent, "other")] {
        assert!(OmpFileTools::create(
            &plan,
            &approval,
            user,
            1,
            scope,
            &temp.path().join("denied")
        )
        .is_err());
        assert!(!temp.path().join("denied").exists());
    }
}
async fn send(w: &mut (impl tokio::io::AsyncWrite + Unpin), value: Value) {
    let mut b = serde_json::to_vec(&value).unwrap();
    b.push(b'\n');
    w.write_all(&b).await.unwrap();
}
async fn read(r: &mut (impl tokio::io::AsyncBufRead + Unpin)) -> Value {
    let mut s = String::new();
    assert!(r.read_line(&mut s).await.unwrap() > 0);
    serde_json::from_str(&s).unwrap()
}
async fn respond(w: &mut (impl tokio::io::AsyncWrite + Unpin), q: &Value, data: Value) {
    send(
        w,
        json!({"type":"response","command":q["type"],"id":q["id"],"success":true,"data":data}),
    )
    .await;
}
#[tokio::test]
async fn rpc_file_service_and_durable_journal_work_together() {
    let temp = tempfile::tempdir().unwrap();
    let (mut service, intent, plan, _) = fixture(temp.path());
    let directory = temp.path().to_path_buf();
    let defs = service.definitions();
    let prompt_id = intent.validate_metadata().unwrap().1;
    let (wire, peer) = tokio::io::duplex(8192);
    let (r, w) = tokio::io::split(wire);
    let mut channel = WorkerTransport::new(BufReader::new(r), w);
    let peer = tokio::spawn(async move {
        let (r, mut w) = tokio::io::split(peer);
        let mut r = BufReader::new(r);
        let mut state = json!({"sessionId":"session","model":{"provider":"deepseek","id":"fixture"},"isStreaming":false,"isCompacting":false,"hasPendingAsyncWork":false,"isSettled":true,"queuedMessageCount":0,"dumpTools":[]});
        send(&mut w,json!({"type":"ready","protocolVersion":1,"supportedProtocolVersions":[1,2],"maxFrameBytes":1024*1024,"maxReassembledFrameBytes":64*1024*1024})).await;
        let q = read(&mut r).await;
        respond(&mut w, &q, json!({"protocolVersion":2})).await;
        for _ in 0..2 {
            let q = read(&mut r).await;
            respond(&mut w, &q, state.clone()).await;
        }
        let q = read(&mut r).await;
        assert_eq!(q["tools"], json!(defs));
        respond(
            &mut w,
            &q,
            json!({"toolNames":defs.iter().map(|d|&d.name).collect::<Vec<_>>()}),
        )
        .await;
        state["dumpTools"] = json!(defs);
        for _ in 0..2 {
            let q = read(&mut r).await;
            respond(&mut w, &q, state.clone()).await;
        }
        let q = read(&mut r).await;
        assert_eq!(q["type"], "prompt");
        respond(&mut w, &q, Value::Null).await;
        for (i, name, args) in [
            (
                "1",
                "write_file",
                json!({"path":"result.txt","content":"actual tool output","expectedSha256":"absent"}),
            ),
            ("2", "read_file", json!({"path":"result.txt","offset":0})),
        ] {
            send(
                &mut w,
                serde_json::to_value(request(i, name, args)).unwrap(),
            )
            .await;
            let result = read(&mut r).await;
            assert_eq!(result["type"], "host_tool_result");
            assert_eq!(result["id"], i);
            assert_eq!(result["isError"], false);
            let record = directory.join(if i == "1" {
                "tools/events/001.json"
            } else {
                "tools/events/003.json"
            });
            assert!(record.is_file());
            let durable: Value = serde_json::from_slice(&fs::read(&record).unwrap()).unwrap();
            assert_eq!(
                durable["body"]["event"]["result_sha256"],
                digest(&result).unwrap()
            );
            if i == "2" {
                assert!(result["result"]["content"][0]["text"]
                    .as_str()
                    .unwrap()
                    .contains("actual tool output"));
            }
        }
        send(&mut w,json!({"type":"prompt_result","id":prompt_id,"status":"completed","agentInvoked":true,"sessionSettled":true})).await;
        let q = read(&mut r).await;
        respond(&mut w, &q, state).await;
        let mut extra = Vec::new();
        r.read_to_end(&mut extra).await.unwrap();
        assert!(extra.is_empty());
    });
    channel.connect(Duration::from_secs(3)).await.unwrap();
    channel
        .register_host_tools(
            &intent.validate_metadata().unwrap().0,
            &service.definitions(),
        )
        .await
        .unwrap();
    let mut journal =
        crate::omp_journal::OmpJournal::open(&temp.path().join("prompt"), &intent).unwrap();
    let (_tx, mut rx) = tokio::sync::oneshot::channel();
    let output = channel
        .execute_with_tools(
            intent.clone(),
            PROMPT,
            Duration::from_secs(3),
            &mut rx,
            |event| journal.record(event),
            &mut service,
        )
        .await
        .unwrap();
    assert!(matches!(output, PromptOutcome::Finished(_)));
    assert_eq!(service.observation().unresolved(), 0);
    assert_eq!(
        fs::read_to_string(Path::new(&plan.project.root).join("result.txt")).unwrap(),
        "actual tool output"
    );
    channel.close_input().await.unwrap();
    peer.await.unwrap();
    drop(service);
    assert_eq!(
        OmpToolJournal::open(&temp.path().join("tools/events"), &intent)
            .unwrap()
            .observation()
            .calls
            .len(),
        2
    );
}

#[tokio::test]
async fn approved_verifier_inputs_and_read_only_grants_cannot_be_written() {
    let temp = tempfile::tempdir().unwrap();
    let (service, mut intent, mut plan, mut approval) = fixture(temp.path());
    drop(service);
    let checker = Path::new(&plan.project.root).join("checker.txt");
    fs::write(&checker, "pinned checker").unwrap();
    plan.execution[0].checks[0]
        .tool
        .as_mut()
        .unwrap()
        .inputs
        .push(ValidationFile {
            path: checker.to_string_lossy().into(),
            sha256: hash(b"pinned checker"),
        });
    approval.revision_digest = plan.approval_digest().unwrap();
    intent.revision_digest = approval.revision_digest.clone();
    let mut service = OmpFileTools::create(
        &plan,
        &approval,
        "user",
        1,
        &intent,
        &temp.path().join("protected-tools"),
    )
    .unwrap();
    let result=run(&mut service,&intent,request("protect","write_file",json!({"path":"checker.txt","content":"changed","expectedSha256":hash(b"pinned checker")})),false).await;
    assert!(result.is_error);
    assert_eq!(fs::read_to_string(&checker).unwrap(), "pinned checker");
    drop(service);
    plan.policy
        .tools
        .retain(|p| *p != ToolPermission::WriteFiles);
    assert!(OmpFileTools::create(
        &plan,
        &approval,
        "user",
        1,
        &intent,
        &temp.path().join("stale")
    )
    .is_err());
    approval.revision_digest = plan.approval_digest().unwrap();
    intent.revision_digest = approval.revision_digest.clone();
    let mut service = OmpFileTools::create(
        &plan,
        &approval,
        "user",
        1,
        &intent,
        &temp.path().join("readonly-tools"),
    )
    .unwrap();
    assert_eq!(service.definitions().len(), 1);
    assert_eq!(service.definitions()[0].name, "cerulean_read_file");
    let result = run(
        &mut service,
        &intent,
        request(
            "readonly",
            "write_file",
            json!({"path":"result.txt","content":"changed","expectedSha256":"absent"}),
        ),
        false,
    )
    .await;
    assert!(result.is_error);
    assert!(!Path::new(&plan.project.root).join("result.txt").exists());
}
#[tokio::test]
async fn bounded_utf8_chunks_conflicting_open_handles_and_changed_arguments_are_checked() {
    let temp = tempfile::tempdir().unwrap();
    let (mut service, intent, plan, _) = fixture(temp.path());
    let path = Path::new(&plan.project.root).join("text.txt");
    let text = "a".repeat(8191) + "中文尾部";
    fs::write(&path, &text).unwrap();
    let first = run(
        &mut service,
        &intent,
        request("read1", "read_file", json!({"path":"text.txt","offset":0})),
        false,
    )
    .await;
    let first: Value = serde_json::from_str(&first.text).unwrap();
    assert_eq!(first["nextOffset"], 8191);
    assert_eq!(first["sha256"], hash(text.as_bytes()));
    let next = run(
        &mut service,
        &intent,
        request(
            "read2",
            "read_file",
            json!({"path":"text.txt","offset":8191}),
        ),
        false,
    )
    .await;
    assert_eq!(
        serde_json::from_str::<Value>(&next.text).unwrap()["content"],
        "中文尾部"
    );
    let bad = run(
        &mut service,
        &intent,
        request(
            "bad-offset",
            "read_file",
            json!({"path":"text.txt","offset":8192}),
        ),
        false,
    )
    .await;
    assert!(bad.is_error);
    let pinned = OpenOptions::new()
        .read(true)
        .write(true)
        .share_mode(3)
        .open(&path)
        .unwrap();
    let bad = run(
        &mut service,
        &intent,
        request(
            "conflict",
            "read_file",
            json!({"path":"text.txt","offset":0}),
        ),
        false,
    )
    .await;
    assert!(bad.is_error);
    drop(pinned);
    let mut changed = request(
        "changed",
        "write_file",
        json!({"path":"result.txt","content":"original","expectedSha256":"absent"}),
    );
    service.record(&intent, &dispatch_event(&changed)).unwrap();
    if let HostRequest::HostToolCall { arguments, .. } = &mut changed {
        arguments.insert("content".into(), json!("swapped"));
    }
    let (_tx, rx) = watch::channel(false);
    assert!(service.start(changed, rx).await.is_err());
    assert!(!Path::new(&plan.project.root).join("result.txt").exists());
    assert_eq!(service.observation().unresolved(), 1);
}

fn wire_hash(id: &str, output: &HostOutput) -> String {
    digest(&json!({"type":"host_tool_result","id":id,"result":{"content":[{"type":"text","text":output.text}]},"isError":output.is_error})).unwrap()
}

#[test]
fn directory_pin_blocks_index_publication_but_preserves_ledger_and_next_commit() {
    // 343: exercise the real pin and asynchronous publisher, without racing to
    // reproduce the historical failure in the opposite (pin acquisition) order.
    struct RetainOnFailure(Option<tempfile::TempDir>);
    impl Drop for RetainOnFailure {
        fn drop(&mut self) {
            if std::thread::panicking() {
                if let Some(temp) = self.0.take() {
                    eprintln!("343 index/pin failed fixture retained at {:?}", temp.keep());
                }
            }
        }
    }
    let fixture = RetainOnFailure(Some(tempfile::tempdir().unwrap()));
    let root = fixture.0.as_ref().unwrap().path();
    let mut store = crate::Store::open(root).unwrap();
    store.wait_for_index(Duration::from_secs(20)).unwrap();
    let old_index = fs::read(root.join("index.sqlite")).unwrap();
    let pin = pin_directory(root).unwrap();
    store
        .execute_command(cerulean_domain::Command::CreatePlan {
            title: "committed while directory pinned".into(),
            tasks: vec!["local fixture only".into()],
        })
        .unwrap();
    let failure = store.wait_for_index(Duration::from_secs(20)).unwrap_err();
    eprintln!("343 expected publication failure: {failure}");
    assert!(failure.contains("os error 32"), "{failure}");
    assert_eq!(fs::read(root.join("index.sqlite")).unwrap(), old_index);
    assert!(store.index_error().is_some());
    let snapshot = store.snapshot();
    assert_eq!(snapshot.events.len(), 1);
    assert_eq!(snapshot.plans.len(), 1);
    assert_eq!(snapshot.plans[0].title, "committed while directory pinned");
    let event_path = root.join("events").join("00000001.json");
    let event_bytes = fs::read(&event_path).unwrap();
    drop(pin);
    // A new legitimate commit, not replay of the first command or task.
    store
        .execute_command(cerulean_domain::Command::CreatePlan {
            title: "next commit after pin release".into(),
            tasks: vec!["another local fixture".into()],
        })
        .unwrap();
    store.wait_for_index(Duration::from_secs(20)).unwrap();
    assert!(store.index_error().is_none());
    assert_eq!(fs::read(&event_path).unwrap(), event_bytes);
    assert_ne!(fs::read(root.join("index.sqlite")).unwrap(), old_index);
    assert_eq!(store.snapshot().plans.len(), 2);
    assert_eq!(store.snapshot().events.len(), 2);
    drop(store);
    let reopened = crate::Store::open(root).unwrap();
    reopened.wait_for_index(Duration::from_secs(20)).unwrap();
    assert_eq!(reopened.snapshot().plans.len(), 2);
    assert_eq!(reopened.snapshot().events.len(), 2);
    assert_eq!(fs::read(&event_path).unwrap(), event_bytes);
}
