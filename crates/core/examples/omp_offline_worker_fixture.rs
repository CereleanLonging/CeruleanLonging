//! Offline test peer. No networking or model API. Only accepts the documented
//! synthetic credential, fixed fixture model names and isolated worker flags.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, BufRead, Write},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
fn send(v: Value) {
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "{v}").unwrap();
    stdout.flush().unwrap();
}
fn reply(q: &Value, data: Value) {
    send(json!({"type":"response","id":q["id"],"command":q["type"],"success":true,"data":data}));
}
fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args == ["--descendant"] {
        loop {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    assert_eq!(
        std::env::var("DEEPSEEK_API_KEY").unwrap(),
        "cerulean-offline-placeholder-not-a-real-key"
    );
    for name in [
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "NODE_OPTIONS",
        "BUN_OPTIONS",
        "CERULEAN_OMP_AMBIENT_CANARY",
    ] {
        assert!(std::env::var_os(name).is_none());
    }
    for flag in [
        "--no-tools",
        "--no-extensions",
        "--no-skills",
        "--no-rules",
        "--no-lsp",
        "--no-ui",
        "--no-pty",
        "--no-prewalk",
        "--no-session",
    ] {
        assert!(args.iter().any(|a| a == flag));
    }
    let model = args.windows(2).find(|a| a[0] == "--model").unwrap()[1]
        .strip_prefix("deepseek/")
        .unwrap()
        .to_owned();
    assert!([
        "fixture-completed",
        "fixture-tree",
        "fixture-parent-wait",
        "fixture-wrong-model",
        "fixture-file-completed",
        "fixture-file-worktree",
        "fixture-file-dag",
        "fixture-file-integration",
        "fixture-file-integration-gated",
        "fixture-file-wait",
        "fixture-file-lost-terminal",
        "fixture-lost-ack"
    ]
    .contains(&model.as_str()));
    let cwd = std::env::current_dir().unwrap();
    let home = std::env::var("HOME").unwrap();
    assert!(std::path::Path::new(&home).is_dir());
    fs::write(
        cwd.join("fixture-ready.json"),
        json!({"pid":std::process::id(),"ambientRemoved":true,"cwd":cwd,"model":model}).to_string(),
    )
    .unwrap();
    if model == "fixture-tree" {
        let child = Command::new(std::env::current_exe().unwrap())
            .arg("--descendant")
            .stdin(Stdio::null())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        fs::write(cwd.join("descendant.pid"), child.id().to_string()).unwrap();
        // Retain the child handle; the owner's Job is responsible for exit.
        std::mem::forget(child);
    }
    send(
        json!({"type":"ready","protocolVersion":1,"supportedProtocolVersions":[1,2],"maxFrameBytes":1024*1024,"maxReassembledFrameBytes":64*1024*1024}),
    );
    let mut tools = json!([]);
    let mut prompt_id = Value::Null;
    let mut integration_task = "";
    let cancelled = Arc::new(AtomicBool::new(false));
    for line in io::stdin().lock().lines() {
        let q: Value = serde_json::from_str(&line.unwrap()).unwrap();
        match q["type"].as_str().unwrap() {
            "negotiate_protocol" => reply(&q, json!({"protocolVersion":2})),
            "get_state" => reply(
                &q,
                json!({"sessionId":"fixture-session","model":{"provider":"deepseek","id":if model=="fixture-wrong-model"{"unapproved"}else{&model}},"isStreaming":false,"isCompacting":false,"hasPendingAsyncWork":false,"isSettled":true,"queuedMessageCount":0,"messageCount":0,"dumpTools":tools}),
            ),
            "set_host_tools" => {
                assert!(model.starts_with("fixture-file-"));
                tools = q["tools"].clone();
                reply(
                    &q,
                    json!({"toolNames":tools.as_array().unwrap().iter().map(|t|t["name"].clone()).collect::<Vec<_>>()}),
                );
            }
            "host_tool_result" => {
                if model.starts_with("fixture-file-integration") {
                    assert_eq!(q["isError"], false);
                    let next = match q["id"].as_str().unwrap() {
                        "mixed-base" | "mixed-a" | "mixed-b" => {
                            let content: Value = serde_json::from_str(
                                q["result"]["content"][0]["text"].as_str().unwrap(),
                            )
                            .unwrap();
                            match q["id"].as_str().unwrap() {
                                "mixed-base" => {
                                    assert_eq!(content["content"], "committed dependency base\n");
                                    if integration_task == "integration" {
                                        Some((
                                            "mixed-a",
                                            "cerulean_read_file",
                                            json!({"path":"a.txt","offset":0}),
                                        ))
                                    } else {
                                        Some((
                                            "mixed-result",
                                            "cerulean_write_file",
                                            json!({"path":"result.txt","content":"alpha","expectedSha256":"absent"}),
                                        ))
                                    }
                                }
                                "mixed-a" => {
                                    assert_eq!(content["content"], "A");
                                    Some((
                                        "mixed-b",
                                        "cerulean_read_file",
                                        json!({"path":"b.txt","offset":0}),
                                    ))
                                }
                                _ => {
                                    assert_eq!(content["content"], "B");
                                    Some((
                                        "mixed-bundle",
                                        "cerulean_write_file",
                                        json!({"path":"bundle.txt","content":"AB","expectedSha256":"absent"}),
                                    ))
                                }
                            }
                        }
                        "mixed-result" => Some((
                            "mixed-branch",
                            "cerulean_write_file",
                            json!({"path":if integration_task=="a"{"a.txt"}else{"b.txt"},"content":if integration_task=="a"{"A"}else{"B"},"expectedSha256":"absent"}),
                        )),
                        "mixed-branch" | "mixed-bundle" => None,
                        _ => panic!("unexpected integration tool result"),
                    };
                    if let Some((id, tool, arguments)) = next {
                        send(
                            json!({"type":"host_tool_call","id":id,"toolCallId":id,"toolName":tool,"arguments":arguments}),
                        );
                    } else if model == "fixture-file-integration-gated" {
                        fs::write(
                            cwd.join("fixture-gated"),
                            "original tools finished; terminal pending",
                        )
                        .unwrap();
                        let release = cwd.join("fixture-release");
                        let id = prompt_id.clone();
                        let stopped = cancelled.clone();
                        std::thread::spawn(move || {
                            let limit = std::time::Instant::now() + Duration::from_secs(120);
                            while !release.exists() && !stopped.load(Ordering::Acquire) {
                                assert!(
                                    std::time::Instant::now() < limit,
                                    "offline OMP gate timeout"
                                );
                                std::thread::sleep(Duration::from_millis(5));
                            }
                            if !stopped.load(Ordering::Acquire) {
                                send(
                                    json!({"type":"prompt_result","id":id,"status":"completed","agentInvoked":true,"sessionSettled":true}),
                                );
                            }
                        });
                    } else {
                        send(
                            json!({"type":"prompt_result","id":prompt_id,"status":"completed","agentInvoked":true,"sessionSettled":true}),
                        );
                    }
                    continue;
                }
                if model == "fixture-file-worktree" && q["id"] == "fixture-read" {
                    assert_eq!(q["isError"], false);
                    let result: Value =
                        serde_json::from_str(q["result"]["content"][0]["text"].as_str().unwrap())
                            .unwrap();
                    assert_eq!(result["content"], "committed base\n");
                    send(
                        json!({"type":"host_tool_call","id":"fixture-write","toolCallId":"model-write","toolName":"cerulean_write_file","arguments":{"path":"result.txt","content":"fixture output","expectedSha256":"absent"}}),
                    );
                    continue;
                }
                assert_eq!(q["id"], "fixture-write");
                assert_eq!(q["isError"], false);
                if model == "fixture-file-wait" {
                    fs::write(cwd.join("fixture-waiting"), "host result received").unwrap();
                }
                if model == "fixture-file-lost-terminal" {
                    std::process::exit(23);
                }
                if matches!(
                    model.as_str(),
                    "fixture-file-completed" | "fixture-file-dag" | "fixture-file-worktree"
                ) {
                    send(
                        json!({"type":"prompt_result","id":prompt_id,"status":"completed","agentInvoked":true,"sessionSettled":true}),
                    );
                }
            }
            "abort" => {
                cancelled.store(true, Ordering::Release);
                reply(&q, Value::Null);
                send(
                    json!({"type":"prompt_result","id":prompt_id,"status":"aborted","agentInvoked":true,"sessionSettled":true}),
                );
            }
            "prompt" => {
                if model == "fixture-lost-ack" {
                    std::process::exit(23);
                }
                reply(&q, Value::Null);
                if model.starts_with("fixture-file-") {
                    prompt_id = q["id"].clone();
                    if model.starts_with("fixture-file-integration") {
                        let prompt = q["message"].as_str().unwrap();
                        integration_task = if prompt.contains("INTEGRATION_integration") {
                            "integration"
                        } else if prompt.contains("INTEGRATION_a") {
                            "a"
                        } else {
                            assert!(prompt.contains("INTEGRATION_b"));
                            "b"
                        };
                        send(
                            json!({"type":"host_tool_call","id":"mixed-base","toolCallId":"mixed-base","toolName":"cerulean_read_file","arguments":{"path":"base.txt","offset":0}}),
                        );
                        continue;
                    }
                    if model == "fixture-file-worktree" {
                        send(
                            json!({"type":"host_tool_call","id":"fixture-read","toolCallId":"model-read","toolName":"cerulean_read_file","arguments":{"path":"base.txt","offset":0}}),
                        );
                        continue;
                    }
                    let second = model == "fixture-file-dag"
                        && q["message"].as_str().unwrap().contains("OMP_DAG_SECOND");
                    let expected = if second {
                        format!("{:x}", Sha256::digest(b"fixture output"))
                    } else {
                        "absent".into()
                    };
                    let content = if second {
                        "fixture output second"
                    } else {
                        "fixture output"
                    };
                    send(
                        json!({"type":"host_tool_call","id":"fixture-write","toolCallId":"model-write","toolName":"cerulean_write_file","arguments":{"path":"result.txt","content":content,"expectedSha256":expected}}),
                    );
                    continue;
                }
                send(
                    json!({"type":"prompt_result","id":q["id"],"status":"completed","agentInvoked":true,"sessionSettled":true}),
                );
            }
            _ => panic!("unexpected fixture command"),
        }
    }
    if model == "fixture-parent-wait" {
        // Offline owner test: deliberately ignore stdin EOF so the original
        // process must be terminated and its retained handle observed.
        loop {
            std::thread::park_timeout(Duration::from_secs(1));
        }
    }
    // A deliberately surviving descendant proves tree ownership on graceful EOF.
}
