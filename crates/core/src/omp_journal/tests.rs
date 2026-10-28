use super::*;
use cerulean_adapters::{
    omp_protocol::PromptStatus,
    omp_transport::worker::{PromptOutcome, WorkerTransport},
};
use cerulean_domain::execution::{ControllerRequestIdentity, IdentityStrength, SessionBinding};
use serde_json::{json, Value};
use std::{
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
const PROMPT: &str = "Carry out the approved isolated test task.";
fn intent() -> DispatchIntent {
    use sha2::{Digest, Sha256};
    DispatchIntent {
        version: 1,
        task_id: "task".into(),
        attempt_id: "attempt".into(),
        revision_digest: "a".repeat(64),
        fence: 1,
        binding: SessionBinding {
            adapter_id: "omp".into(),
            adapter_version: "18.4.4".into(),
            session_id: "session".into(),
            workspace: std::env::current_dir().unwrap().to_string_lossy().into(),
            identity_strength: IdentityStrength::ControllerRequest(ControllerRequestIdentity {
                connection_id: "connection".into(),
                request_id: "prompt-request".into(),
            }),
        },
        provider: "deepseek".into(),
        model: "fixture".into(),
        prompt_sha256: format!("{:x}", Sha256::digest(PROMPT.as_bytes())),
    }
}
fn dispatch() -> WorkerEvent {
    WorkerEvent::DispatchIntent { intent: intent() }
}
fn ack() -> WorkerEvent {
    WorkerEvent::PromptAcknowledged {
        request_id: "prompt-request".into(),
    }
}
fn abort() -> WorkerEvent {
    WorkerEvent::AbortIntent {
        request_id: "prompt-request".into(),
        abort_id: "cerulean-omp-4".into(),
    }
}
fn abort_ack() -> WorkerEvent {
    WorkerEvent::AbortAcknowledged {
        abort_id: "cerulean-omp-4".into(),
    }
}
fn terminal(requested: bool, acknowledged: bool) -> WorkerEvent {
    let (identity, request_id) = intent().validate_metadata().unwrap();
    WorkerEvent::Terminal {
        receipt: PromptReceipt {
            identity,
            request_id,
            correlation_origin: "controller-rpc-request-id".into(),
            status: if requested {
                PromptStatus::Aborted
            } else {
                PromptStatus::Completed
            },
        },
        abort_requested: requested,
        abort_acknowledged: acknowledged,
    }
}
#[test]
fn exclusive_owner_and_early_abort_replay_without_reauthorizing_dispatch() {
    let root = tempfile::tempdir().unwrap();
    let expected = intent();
    let mut journal = OmpJournal::open(root.path(), &expected).unwrap();
    assert!(OmpJournal::open(root.path(), &expected).is_err());
    assert!(journal.record(&ack()).is_err());
    assert!(journal.healthy());
    journal.record(&dispatch()).unwrap();
    assert!(journal.observation().unknown());
    journal.record(&abort()).unwrap();
    journal.record(&abort_ack()).unwrap();
    assert!(journal.record(&terminal(true, true)).is_err());
    journal.record(&ack()).unwrap();
    journal.record(&terminal(true, true)).unwrap();
    let observed = journal.observation().clone();
    let head = journal.head().to_owned();
    drop(journal);
    let mut reopened = OmpJournal::open(root.path(), &expected).unwrap();
    assert_eq!(reopened.observation(), &observed);
    assert_eq!(reopened.head(), head);
    assert!(!reopened.observation().unknown());
    assert!(reopened.record(&dispatch()).is_err());
}
#[test]
fn unknown_no_send_and_terminal_contradictions_have_distinct_evidence() {
    for mode in ["intent", "ack", "no-send", "terminal"] {
        let root = tempfile::tempdir().unwrap();
        let mut journal = OmpJournal::open(root.path(), &intent()).unwrap();
        journal.record(&dispatch()).unwrap();
        match mode {
            "ack" => journal.record(&ack()).unwrap(),
            "no-send" => journal
                .record(&WorkerEvent::NotStarted {
                    request_id: "prompt-request".into(),
                })
                .unwrap(),
            "terminal" => {
                journal.record(&ack()).unwrap();
                assert!(journal.record(&terminal(true, false)).is_err());
                journal.record(&terminal(false, false)).unwrap();
            }
            _ => {}
        }
        drop(journal);
        let mut journal = OmpJournal::open(root.path(), &intent()).unwrap();
        assert_eq!(
            journal.observation().unknown(),
            matches!(mode, "intent" | "ack")
        );
        assert_eq!(journal.observation().not_started, mode == "no-send");
        assert!(journal.record(&dispatch()).is_err());
        if mode == "ack" {
            assert!(journal
                .record(&WorkerEvent::NotStarted {
                    request_id: "prompt-request".into()
                })
                .is_err());
        }
    }
}
#[test]
fn rehashed_scope_ack_flags_and_foreign_receipts_fail_replay() {
    for mode in [
        "scope",
        "request",
        "origin",
        "model",
        "abort",
        "unknown-field",
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut journal = OmpJournal::open(root.path(), &intent()).unwrap();
        journal.record(&dispatch()).unwrap();
        journal.record(&ack()).unwrap();
        journal.record(&terminal(false, false)).unwrap();
        drop(journal);
        let path = root.path().join("02.json");
        let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        match mode {
            "scope" => record["body"]["intentDigest"] = json!("b".repeat(64)),
            "request" => record["body"]["event"]["receipt"]["requestId"] = json!("foreign"),
            "origin" => {
                record["body"]["event"]["receipt"]["correlationOrigin"] = json!("provider-turn")
            }
            "model" => record["body"]["event"]["receipt"]["identity"]["model"] = json!("other"),
            "abort" => record["body"]["event"]["abort_acknowledged"] = json!(true),
            _ => record["body"]["event"]["receipt"]["trusted"] = json!(true),
        }
        // Recompute using the same typed canonical encoding whenever it parses:
        // these checks must enforce semantics, not merely detect a stale hash.
        if let Ok(body) = serde_json::from_value::<Body>(record["body"].clone()) {
            record["hash"] = json!(digest(&body).unwrap());
        }
        fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();
        assert!(OmpJournal::open(root.path(), &intent()).is_err(), "{mode}");
    }
    let root = tempfile::tempdir().unwrap();
    let mut journal = OmpJournal::open(root.path(), &intent()).unwrap();
    journal.record(&dispatch()).unwrap();
    drop(journal);
    let mut changed = intent();
    changed.fence += 1;
    assert!(OmpJournal::open(root.path(), &changed).is_err());
}
#[test]
fn interrupted_publication_and_destination_races_preserve_original_bytes() {
    for point in ["synced", "published", "committed", "race"] {
        let root = tempfile::tempdir().unwrap();
        let mut journal = OmpJournal::open(root.path(), &intent()).unwrap();
        let result = journal.publish(&dispatch(), |at| {
            if point == "race" && at == "synced" {
                fs::write(root.path().join("00.json"), b"existing-evidence").unwrap();
                return Ok(());
            }
            if at == point {
                return Err("simulated interruption".into());
            }
            Ok(())
        });
        assert!(result.is_err());
        assert!(!journal.healthy());
        assert!(journal.record(&dispatch()).is_err());
        drop(journal);
        if point == "committed" {
            let mut reopened = OmpJournal::open(root.path(), &intent()).unwrap();
            assert!(reopened.observation().unknown());
            assert!(reopened.record(&dispatch()).is_err());
        } else {
            assert!(OmpJournal::open(root.path(), &intent()).is_err());
        }
        if point == "race" {
            assert_eq!(
                fs::read(root.path().join("00.json")).unwrap(),
                b"existing-evidence"
            );
        }
    }
}

fn idle() -> Value {
    json!({"sessionId":"session","model":{"provider":"deepseek","id":"fixture"},"isStreaming":false,"isCompacting":false,"hasPendingAsyncWork":false,"isSettled":true,"queuedMessageCount":0,"dumpTools":[]})
}
async fn write_frame(writer: &mut (impl tokio::io::AsyncWrite + Unpin), value: Value) {
    let mut b = serde_json::to_vec(&value).unwrap();
    b.push(b'\n');
    writer.write_all(&b).await.unwrap();
}
async fn read_frame(reader: &mut (impl tokio::io::AsyncBufRead + Unpin)) -> Value {
    let mut s = String::new();
    assert!(reader.read_line(&mut s).await.unwrap() > 0);
    serde_json::from_str(&s).unwrap()
}
async fn respond(writer: &mut (impl tokio::io::AsyncWrite + Unpin), request: &Value, data: Value) {
    write_frame(writer,json!({"type":"response","command":request["type"],"id":request["id"],"success":true,"data":data})).await;
}
#[tokio::test]
async fn real_journal_precedes_wire_and_reopened_intent_sends_nothing() {
    for fail_after_prompt in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().to_path_buf();
        let mut journal = OmpJournal::open(root.path(), &intent()).unwrap();
        let (client, peer) = tokio::io::duplex(4096);
        let (reader, writer) = tokio::io::split(client);
        let mut client = WorkerTransport::new(BufReader::new(reader), writer);
        let peer = tokio::spawn(async move {
            let (reader, mut writer) = tokio::io::split(peer);
            let mut reader = BufReader::new(reader);
            write_frame(&mut writer,json!({"type":"ready","protocolVersion":1,"supportedProtocolVersions":[1,2],"maxFrameBytes":1024*1024,"maxReassembledFrameBytes":64*1024*1024})).await;
            let request = read_frame(&mut reader).await;
            respond(&mut writer, &request, json!({"protocolVersion":2})).await;
            for _ in 0..2 {
                let request = read_frame(&mut reader).await;
                assert_eq!(request["type"], "get_state");
                respond(&mut writer, &request, idle()).await;
            }
            let prompt = read_frame(&mut reader).await;
            assert_eq!(prompt["type"], "prompt");
            let record: Record =
                serde_json::from_slice(&fs::read(directory.join("00.json")).unwrap()).unwrap();
            assert_eq!(record.body.event, dispatch());
            assert!(!directory.join("00.pending").exists());
            if fail_after_prompt {
                return;
            }
            respond(&mut writer, &prompt, Value::Null).await;
            write_frame(&mut writer,json!({"type":"prompt_result","id":prompt["id"],"status":"completed","agentInvoked":true,"sessionSettled":true})).await;
            let request = read_frame(&mut reader).await;
            respond(&mut writer, &request, idle()).await;
            let mut tail = Vec::new();
            reader.read_to_end(&mut tail).await.unwrap();
            assert!(tail.is_empty());
        });
        let (_cancel, mut rx) = tokio::sync::oneshot::channel();
        client.connect(Duration::from_secs(2)).await.unwrap();
        let result = journal
            .execute_once(&mut client, PROMPT, Duration::from_secs(2), &mut rx)
            .await;
        if fail_after_prompt {
            assert!(result.is_err());
        } else {
            assert!(matches!(result, Ok(PromptOutcome::Finished(_))));
        }
        client.close_input().await.unwrap();
        peer.await.unwrap();
        drop(journal);
        let mut reopened = OmpJournal::open(root.path(), &intent()).unwrap();
        assert_eq!(reopened.observation().unknown(), fail_after_prompt);
        // Supply a fresh, even unconnected channel. The journal must reject
        // before preflight/handshake/prompt bytes, not just rely on old transport.
        let (new, mut peer) = tokio::io::duplex(4096);
        let (reader, writer) = tokio::io::split(new);
        let mut new = WorkerTransport::new(BufReader::new(reader), writer);
        assert!(reopened
            .execute_once(&mut new, PROMPT, Duration::from_secs(2), &mut rx)
            .await
            .unwrap_err()
            .contains("never resend"));
        assert!(new.command_attempts().is_empty());
        new.close_input().await.unwrap();
        let mut wire = Vec::new();
        peer.read_to_end(&mut wire).await.unwrap();
        assert!(wire.is_empty());
    }
}

struct Owned(Child);
impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
#[ignore = "owned journal crash helper; parent supplies an isolated temporary directory"]
fn crash_owner() {
    let directory = PathBuf::from(
        std::env::var_os("CERULEAN_OMP_JOURNAL_CHILD_ROOT").expect("explicit fixture directory"),
    );
    let point = std::env::var("CERULEAN_OMP_JOURNAL_CHILD_POINT").unwrap();
    let mut journal = OmpJournal::open(&directory.join("journal"), &intent()).unwrap();
    journal
        .publish(&dispatch(), |at| {
            if at == point {
                let marker = directory.join("ready");
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(marker)
                    .unwrap();
                file.write_all(at.as_bytes()).unwrap();
                file.sync_all().unwrap();
                loop {
                    std::thread::park_timeout(Duration::from_secs(1));
                }
            }
            Ok(())
        })
        .unwrap();
}
#[test]
fn actual_owner_termination_at_three_publication_boundaries_never_reauthorizes_send() {
    for point in ["synced", "published", "committed"] {
        let root = tempfile::tempdir().unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--ignored",
                "--exact",
                "omp_journal::tests::crash_owner",
                "--nocapture",
            ])
            .env("CERULEAN_OMP_JOURNAL_CHILD_ROOT", root.path())
            .env("CERULEAN_OMP_JOURNAL_CHILD_POINT", point)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut owner = Owned(command.spawn().unwrap());
        let deadline = Instant::now() + Duration::from_secs(20);
        while !root.path().join("ready").exists() {
            assert!(
                owner.0.try_wait().unwrap().is_none(),
                "owner exited early at {point}"
            );
            assert!(Instant::now() < deadline, "owner marker timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
        let files = fs::read_dir(root.path().join("journal"))
            .unwrap()
            .filter(|e| e.as_ref().unwrap().file_name() != "writer.lock")
            .map(|e| {
                let p = e.unwrap().path();
                let b = fs::read(&p).unwrap();
                (p, b)
            })
            .collect::<Vec<_>>();
        owner.0.kill().unwrap();
        owner.0.wait().unwrap();
        let reopened = OmpJournal::open(&root.path().join("journal"), &intent());
        if point == "committed" {
            let mut reopened = reopened.unwrap();
            assert!(reopened.observation().unknown());
            assert!(reopened.record(&dispatch()).is_err());
        } else {
            assert!(reopened.is_err());
        }
        for (path, bytes) in files {
            assert_eq!(
                fs::read(path).unwrap(),
                bytes,
                "existing evidence changed at {point}"
            );
        }
    }
}
