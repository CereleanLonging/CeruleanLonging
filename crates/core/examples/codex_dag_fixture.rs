//! Offline protocol peer + independent validator for the two-task orchestration probe.
//! No provider, credential, network, or arbitrary executable invocation.
use serde_json::{json, Value};
use std::{
    fs,
    io::{self, BufRead, Write},
    path::PathBuf,
};
fn send(out: &mut impl Write, value: Value) {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    out.write_all(&bytes).unwrap();
    out.flush().unwrap();
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let bin = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .to_owned();
    let workspace = std::env::current_dir().unwrap().canonicalize().unwrap();
    if args.get(1).map(String::as_str) == Some("validate") {
        let stage = &args[2];
        assert!(matches!(stage.as_str(), "syntax" | "unit"));
        let result = fs::read_to_string("result.txt").unwrap();
        assert!(matches!(result.as_str(), "alpha" | "alpha beta"));
        let task = if result == "alpha" { "a" } else { "b" };
        let temp = PathBuf::from(std::env::var_os("TEMP").unwrap());
        fs::write(
            temp.join("executed.json"),
            json!({"task":task,"stage":stage}).to_string(),
        )
        .unwrap();
        println!("independent {stage} check on task {task}");
        let mode = fs::read_to_string("fixture-mode").unwrap();
        if std::path::Path::new("base.txt").exists() {
            assert_eq!(
                fs::read_to_string("base.txt").unwrap(),
                "committed dependency base\n"
            );
            assert!(!std::path::Path::new(".git").exists());
        }
        if mode == format!("fail-{task}-{stage}") {
            std::process::exit(7);
        }
        return;
    }
    assert_eq!(args.get(1).map(String::as_str), Some("app-server"));
    fs::write(bin.join("app-server-pid"), std::process::id().to_string()).unwrap();
    let expected: PathBuf =
        serde_json::from_slice(&fs::read(bin.join("workspace.json")).unwrap()).unwrap();
    if bin.join("worktree-pool.json").exists() {
        let pool: PathBuf =
            serde_json::from_slice(&fs::read(bin.join("worktree-pool.json")).unwrap()).unwrap();
        assert_eq!(
            workspace.parent().unwrap(),
            pool.canonicalize().unwrap().join("trees")
        );
        assert!(workspace.join(".git").is_file());
    } else {
        assert_eq!(workspace, expected.canonicalize().unwrap());
    }
    let log = bin.join("protocol.jsonl");
    let mut session = None;
    let mut out = io::stdout();
    let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        assert!(line.len() < 128 * 1024);
        let req: Value = serde_json::from_str(&line).unwrap();
        let method = req["method"].as_str().unwrap();
        let mut log_file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log)
            .unwrap();
        log_file
            .write_all(format!("{}\n", json!({"method":method,"session":session})).as_bytes())
            .unwrap();
        log_file.sync_all().unwrap();
        if method == "initialized" {
            continue;
        }
        let id = req["id"].clone();
        let p = &req["params"];
        let result = match method {
            "initialize" => json!({"userAgent":"cerulean-dag-fixture/1"}),
            "account/read" => json!({"account":{"type":"fixture"}}),
            "config/read" => json!({"config":{"mcp_servers":{},"plugins":{}}}),
            "model/list" => {
                json!({"data":[{"id":"fixture-worker","model":"fixture-worker","isDefault":true}],"nextCursor":null})
            }
            "thread/start" => {
                assert_eq!(p["model"], "fixture-worker");
                assert_eq!(p["modelProvider"], "openai");
                assert_eq!(
                    PathBuf::from(p["cwd"].as_str().unwrap())
                        .canonicalize()
                        .unwrap(),
                    workspace
                );
                let n = if bin.join("gate-codex").exists() {
                    std::process::id()
                } else {
                    let n = fs::read_to_string(bin.join("threads-count"))
                        .unwrap_or_else(|_| "0".into())
                        .parse::<u32>()
                        .unwrap()
                        + 1;
                    fs::write(bin.join("threads-count"), n.to_string()).unwrap();
                    n
                };
                let thread = format!("fixture-thread-{n}");
                session = Some(thread.clone());
                json!({"thread":{"id":thread},"model":"fixture-worker","modelProvider":"openai","cwd":workspace,
                    "approvalPolicy":"never","sandbox":{"type":"workspaceWrite","writableRoots":[workspace],"networkAccess":false,"excludeTmpdirEnvVar":true,"excludeSlashTmp":true}})
            }
            "turn/start" => {
                let thread = session.as_ref().unwrap();
                assert_eq!(p["threadId"], *thread);
                let saved_path = bin.join(format!("{thread}.json"));
                assert!(!saved_path.exists(), "duplicate turn/start");
                let text = p["input"][0]["text"].as_str().unwrap();
                let artifact = workspace.join("result.txt");
                if text.contains("INTEGRATION_") {
                    assert_eq!(
                        fs::read_to_string(workspace.join("base.txt")).unwrap(),
                        "committed dependency base\n"
                    );
                    if text.contains("INTEGRATION_integration") {
                        assert_eq!(fs::read_to_string(workspace.join("a.txt")).unwrap(), "A");
                        assert_eq!(fs::read_to_string(workspace.join("b.txt")).unwrap(), "B");
                        assert!(!workspace.join("bundle.txt").exists());
                        fs::write(workspace.join("bundle.txt"), "AB").unwrap();
                    } else {
                        let b = text.contains("INTEGRATION_b");
                        assert!(b || text.contains("INTEGRATION_a"));
                        assert!(!artifact.exists());
                        fs::write(&artifact, "alpha").unwrap();
                        fs::write(
                            workspace.join(if b { "b.txt" } else { "a.txt" }),
                            if b { "B" } else { "A" },
                        )
                        .unwrap();
                    }
                } else if text.contains("DAG_STEP_A") {
                    if artifact.exists() {
                        assert!(
                            bin.join("allow-explicit-rework").exists(),
                            "unexpected repeat of A"
                        );
                        assert_eq!(fs::read_to_string(&artifact).unwrap(), "alpha");
                    }
                    fs::write(&artifact, "alpha").unwrap();
                } else {
                    assert!(text.contains("DAG_STEP_B"));
                    assert_eq!(fs::read_to_string(&artifact).unwrap(), "alpha");
                    fs::write(&artifact, "alpha beta").unwrap();
                }
                let turn = format!("{thread}-turn");
                fs::write(
                    &saved_path,
                    json!({"turn":turn,"clientId":p["clientUserMessageId"],"prompt":text})
                        .to_string(),
                )
                .unwrap();
                let observe = bin.join("emit-observations").exists()
                    && (!bin.join("observations-only-b").exists()
                        || text.contains("INTEGRATION_b"));
                let observed = |item: &str, turn: &str| json!({"method":"item/completed","params":{"threadId":thread,"turnId":turn,"item":{"id":item,"type":"commandExecution","status":"failed","exitCode":7,"command":"synthetic-check --token private-observation-sentinel","aggregatedOutput":"private-observation-sentinel"}}});
                if observe {
                    send(&mut out, observed("first", &turn));
                    send(&mut out, observed("first", &turn));
                    send(
                        &mut out,
                        json!({"method":"item/reasoning/textDelta","params":{"delta":"private-observation-sentinel"}}),
                    );
                }
                send(
                    &mut out,
                    json!({"id":id,"result":{"turn":{"id":turn,"status":"inProgress"}}}),
                );
                if observe {
                    if !bin.join("observations-clean").exists() {
                        send(&mut out, observed("foreign", "foreign-turn"));
                    }
                    send(&mut out, observed("second", &turn));
                    send(
                        &mut out,
                        json!({"method":"item/completed","params":{"threadId":thread,"turnId":turn,"item":{"id":"file","type":"fileChange","status":"completed","changes":[{"diff":"private-observation-sentinel"}]}}}),
                    );
                    if bin.join("workspace-observation-pages").exists() {
                        for n in 0..42 {
                            send(&mut out, observed(&format!("paged-{n:03}"), &turn));
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(350));
                }
                if bin.join("gate-codex").exists() && !text.contains("INTEGRATION_integration") {
                    let task = if text.contains("INTEGRATION_b") {
                        "b"
                    } else {
                        "a"
                    };
                    if bin.join("delay-observations").exists() {
                        let go = bin.join("observe-both");
                        let cancelled = cancelled.clone();
                        let thread = thread.clone();
                        let turn = turn.clone();
                        std::thread::spawn(move || {
                            let limit =
                                std::time::Instant::now() + std::time::Duration::from_secs(120);
                            while !go.exists()
                                && !cancelled.load(std::sync::atomic::Ordering::Acquire)
                            {
                                assert!(
                                    std::time::Instant::now() < limit,
                                    "observation gate expired"
                                );
                                std::thread::sleep(std::time::Duration::from_millis(5));
                            }
                            if !cancelled.load(std::sync::atomic::Ordering::Acquire) {
                                for item in ["delayed-first", "delayed-second"] {
                                    send(
                                        &mut io::stdout(),
                                        json!({"method":"item/completed","params":{"threadId":thread,"turnId":turn,"item":{"id":item,"type":"commandExecution","status":"failed","exitCode":7,"command":"synthetic-check"}}}),
                                    );
                                }
                            }
                        });
                    }
                    fs::write(bin.join(format!("gated-{task}")), thread).unwrap();
                    let release = bin.join(format!("release-{task}"));
                    let cancelled = cancelled.clone();
                    let thread = thread.clone();
                    std::thread::spawn(move || {
                        let limit = std::time::Instant::now() + std::time::Duration::from_secs(120);
                        while !release.exists()
                            && !cancelled.load(std::sync::atomic::Ordering::Acquire)
                        {
                            assert!(
                                std::time::Instant::now() < limit,
                                "Codex fixture gate expired"
                            );
                            std::thread::sleep(std::time::Duration::from_millis(5));
                        }
                        if !cancelled.load(std::sync::atomic::Ordering::Acquire) {
                            send(
                                &mut io::stdout(),
                                json!({"method":"turn/completed","params":{"threadId":thread,"turn":{"id":turn,"status":"completed"}}}),
                            );
                        }
                    });
                    continue;
                }
                if bin.join("await-steer").exists() {
                    continue;
                }
                send(
                    &mut out,
                    json!({"method":"turn/completed","params":{"threadId":thread,"turn":{"id":turn,"status":"completed"}}}),
                );
                continue;
            }
            "turn/steer" => {
                assert!(bin.join("await-steer").exists());
                let thread = session.as_ref().unwrap();
                let saved: Value =
                    serde_json::from_slice(&fs::read(bin.join(format!("{thread}.json"))).unwrap())
                        .unwrap();
                assert_eq!(p["threadId"], *thread);
                assert_eq!(p["expectedTurnId"], saved["turn"]);
                assert!(p["clientUserMessageId"]
                    .as_str()
                    .is_some_and(|s| !s.is_empty()));
                assert_eq!(p.as_object().unwrap().len(), 4);
                assert_eq!(p["input"].as_array().unwrap().len(), 1);
                assert_eq!(p["input"][0]["type"], "text");
                let delivered = bin.join(format!("{thread}-steer-request.json"));
                assert!(!delivered.exists(), "duplicate steer");
                fs::write(
                    bin.join("steer-request.json"),
                    serde_json::to_vec(p).unwrap(),
                )
                .unwrap();
                fs::write(delivered, serde_json::to_vec(p).unwrap()).unwrap();
                if bin.join("lost-steer-ack").exists() {
                    continue;
                }
                send(
                    &mut out,
                    json!({"id":id,"result":{"turnId":if bin.join("wrong-steer-ack").exists() {json!("foreign-turn")} else {saved["turn"].clone()}}}),
                );
                if bin.join("gate-codex").exists() {
                    continue;
                }
                send(
                    &mut out,
                    json!({"method":"turn/completed","params":{"threadId":thread,"turn":{"id":saved["turn"],"status":"completed"}}}),
                );
                continue;
            }
            "turn/interrupt" => {
                cancelled.store(true, std::sync::atomic::Ordering::Release);
                let thread = session.as_ref().unwrap();
                let path = bin.join(format!("{thread}.json"));
                let mut saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                assert_eq!(p["threadId"], *thread);
                assert_eq!(p["turnId"], saved["turn"]);
                saved["status"] = json!("interrupted");
                fs::write(path, serde_json::to_vec(&saved).unwrap()).unwrap();
                send(&mut out, json!({"id":id,"result":{}}));
                send(
                    &mut out,
                    json!({"method":"turn/completed","params":{"threadId":thread,"turn":{"id":saved["turn"],"status":"interrupted"}}}),
                );
                continue;
            }
            "thread/read" => {
                let thread = p["threadId"].as_str().unwrap();
                assert!(thread.starts_with("fixture-thread-") && !thread.contains(['/', '\\']));
                let saved: Value =
                    serde_json::from_slice(&fs::read(bin.join(format!("{thread}.json"))).unwrap())
                        .unwrap();
                json!({"thread":{"id":thread,"cwd":workspace,"turns":[{"id":saved["turn"],"status":saved["status"].as_str().unwrap_or("completed"),"itemsView":"full",
                    "items":[{"type":"userMessage","clientId":saved["clientId"],"content":[{"type":"text","text":saved["prompt"]}]}]}]}})
            }
            _ => panic!("unsupported fixture method: {method}"),
        };
        send(&mut out, json!({"id":id,"result":result}));
    }
}
