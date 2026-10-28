//! Synthetic stdio peer for cancellation tests. It never connects to a provider.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs::{self, OpenOptions},
    io::{self, BufRead, Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

const MODEL: &str = "fixture-worker";
const THREAD: &str = "fixture-thread";
const TURN: &str = "fixture-turn";
const LIMIT: usize = 64 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Config {
    version: u32,
    mode: String,
    workspace: PathBuf,
}

// Private fixture state supplies exact readback after a new helper process starts.
// This is deliberately separate from the method-only protocol log.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct State {
    client_id: String,
    prompt: String,
    status: String,
}

fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("fixture input is not an ordinary file".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err("fixture input is a reparse point".into());
        }
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > LIMIT {
        return Err("fixture input exceeds limit".into());
    }
    Ok(bytes)
}

fn save_state(path: &Path, state: &State) -> Result<(), String> {
    let bytes = serde_json::to_vec(state).map_err(|_| "fixture state serialization failed")?;
    if bytes.len() > LIMIT {
        return Err("fixture state exceeds limit".into());
    }
    if path.exists() {
        read_bounded(path)?;
    }
    let pending = path.with_extension("pending");
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)
        .map_err(|e| e.to_string())?;
    output
        .write_all(&bytes)
        .and_then(|_| output.sync_all())
        .map_err(|e| e.to_string())?;
    drop(output);
    fs::rename(pending, path).map_err(|e| e.to_string())
}

fn wire(output: &mut impl Write, message: Value) -> Result<(), String> {
    serde_json::to_writer(&mut *output, &message).map_err(|_| "fixture reply failed")?;
    output
        .write_all(b"\n")
        .and_then(|_| output.flush())
        .map_err(|_| "fixture output closed".into())
}

fn log_method(path: &Path, method: &str) -> Result<(), String> {
    let mut output = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut output, &json!({"method":method,"count":1}))
        .map_err(|_| "fixture log serialization failed")?;
    output
        .write_all(b"\n")
        .and_then(|_| output.sync_all())
        .map_err(|e| e.to_string())
}

fn run() -> Result<(), String> {
    if std::env::args().nth(1).as_deref() != Some("app-server") {
        return Err("test helper requires the app-server invocation".into());
    }
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let bin = executable
        .parent()
        .ok_or("fixture binary directory missing")?;
    let config: Config = serde_json::from_slice(&read_bounded(&bin.join("fixture-config.json"))?)
        .map_err(|_| "invalid fixture configuration")?;
    if config.version != 1
        || !matches!(
            config.mode.as_str(),
            "interrupted"
                | "completed"
                | "ack-lost"
                | "empty-interrupted"
                | "empty-ack-lost"
                | "worktree-completed"
                | "worktree-wait"
                | "worktree-lost-terminal"
        )
    {
        return Err("unsupported fixture mode".into());
    }
    let workspace = fs::canonicalize(&config.workspace).map_err(|e| e.to_string())?;
    if fs::canonicalize(std::env::current_dir().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?
        != workspace
    {
        return Err("fixture workspace differs from process cwd".into());
    }
    let state_path = bin.join(".fixture-state.json");
    let log = bin.join("protocol-log.jsonl");
    let mut state: Option<State> = if state_path.exists() {
        Some(
            serde_json::from_slice(&read_bounded(&state_path)?)
                .map_err(|_| "invalid private fixture state")?,
        )
    } else {
        None
    };
    let input = io::stdin();
    let mut input = input.lock();
    let output = io::stdout();
    let mut output = output.lock();
    loop {
        let mut line = Vec::new();
        let count = (&mut input)
            .take((LIMIT + 1) as u64)
            .read_until(b'\n', &mut line)
            .map_err(|_| "fixture input failed")?;
        if count == 0 {
            return Ok(());
        }
        if line.len() > LIMIT || !line.ends_with(b"\n") {
            return Err("fixture request exceeds limit or is incomplete".into());
        }
        let request: Value =
            serde_json::from_slice(&line).map_err(|_| "invalid fixture request")?;
        let method = request["method"]
            .as_str()
            .ok_or("fixture request method missing")?;
        if method.len() > 100
            || !method
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'/')
        {
            return Err("invalid fixture method".into());
        }
        log_method(&log, method)?;
        if method == "initialized" {
            continue;
        }
        let id = request
            .get("id")
            .ok_or("fixture request id missing")?
            .clone();
        let params = &request["params"];
        let result = match method {
            "initialize" => json!({"userAgent":"cerulean-protocol-fixture/1"}),
            "account/read" => json!({"account":{"type":"fixture"}}),
            "model/list" => {
                json!({"data":[{"id":MODEL,"model":MODEL,"isDefault":true}],"nextCursor":null})
            }
            "config/read" => json!({"config":{"mcp_servers":{},"plugins":{}}}),
            "thread/start" => {
                if params["model"] != MODEL
                    || params["modelProvider"] != "openai"
                    || params["cwd"]
                        .as_str()
                        .is_none_or(|cwd| fs::canonicalize(cwd).ok().as_ref() != Some(&workspace))
                {
                    return Err("fixture worker scope mismatch".into());
                }
                json!({"thread":{"id":THREAD},"model":MODEL,"modelProvider":"openai",
                    "cwd":workspace,"approvalPolicy":"never","sandbox":{"type":"workspaceWrite",
                    "writableRoots":[workspace],"networkAccess":false,"excludeTmpdirEnvVar":true,"excludeSlashTmp":true}})
            }
            "turn/start" => {
                if state.is_some() || params["threadId"] != THREAD {
                    return Err("fixture turn may be started exactly once".into());
                }
                let content = params["input"].as_array().ok_or("fixture input missing")?;
                if content.len() != 1 || content[0]["type"] != "text" {
                    return Err("fixture expects one approved text prompt".into());
                }
                let started = State {
                    client_id: params["clientUserMessageId"]
                        .as_str()
                        .ok_or("fixture client id missing")?
                        .into(),
                    prompt: content[0]["text"]
                        .as_str()
                        .ok_or("fixture prompt missing")?
                        .into(),
                    status: "inProgress".into(),
                };
                save_state(&state_path, &started)?;
                state = Some(started);
                if config.mode.starts_with("worktree-") {
                    if params["cwd"]
                        .as_str()
                        .is_none_or(|cwd| fs::canonicalize(cwd).ok().as_ref() != Some(&workspace))
                        || params["sandboxPolicy"]["writableRoots"] != json!([workspace])
                        || fs::read_to_string(workspace.join("base.txt"))
                            .map_err(|e| e.to_string())?
                            != "committed base\n"
                    {
                        return Err("worktree launch/turn/input identity mismatch".into());
                    }
                    fs::write(
                        workspace.join("sum.mjs"),
                        b"export function sum(a, b) { return a + b; }\n",
                    )
                    .map_err(|e| e.to_string())?;
                    if config.mode != "worktree-wait" {
                        state.as_mut().unwrap().status = "completed".into();
                        save_state(&state_path, state.as_ref().unwrap())?;
                        wire(
                            &mut output,
                            json!({"id":id,"result":{"turn":{"id":TURN,"status":"inProgress"}}}),
                        )?;
                        if config.mode == "worktree-lost-terminal" {
                            return Ok(());
                        }
                        wire(
                            &mut output,
                            json!({"method":"turn/completed","params":{"threadId":THREAD,"turn":{"id":TURN,"status":"completed"}}}),
                        )?;
                        continue;
                    }
                }
                json!({"turn":{"id":TURN,"status":"inProgress"}})
            }
            "turn/interrupt" => {
                let active = state.as_mut().ok_or("fixture has no active turn")?;
                if params["threadId"] != THREAD
                    || params["turnId"] != TURN
                    || active.status != "inProgress"
                {
                    return Err("fixture interrupt identity or state mismatch".into());
                }
                active.status = if config.mode == "completed" {
                    let mut artifact = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(workspace.join("sum.mjs"))
                        .map_err(|e| e.to_string())?;
                    artifact
                        .write_all(b"export function sum(a, b) { return a + b; }\n")
                        .and_then(|_| artifact.sync_all())
                        .map_err(|e| e.to_string())?;
                    "completed".into()
                } else {
                    "interrupted".into()
                };
                save_state(&state_path, active)?;
                // Give the UI an observable Cancelling interval; assertions use state, not timing.
                std::thread::sleep(Duration::from_millis(400));
                if matches!(config.mode.as_str(), "ack-lost" | "empty-ack-lost") {
                    return Ok(());
                }
                wire(&mut output, json!({"id":id,"result":{}}))?;
                wire(
                    &mut output,
                    json!({"method":"turn/completed","params":{"threadId":THREAD,"turn":{"id":TURN,"status":active.status}}}),
                )?;
                continue;
            }
            "thread/read" => {
                if params["threadId"] != THREAD || params["includeTurns"] != true {
                    return Err("fixture readback identity mismatch".into());
                }
                let saved = state.as_ref().ok_or("fixture turn is not persisted")?;
                let items = if matches!(
                    config.mode.as_str(),
                    "empty-interrupted" | "empty-ack-lost"
                ) {
                    json!([])
                } else {
                    json!([{"type":"userMessage","clientId":saved.client_id,"content":[{"type":"text","text":saved.prompt}]}])
                };
                json!({"thread":{"id":THREAD,"cwd":workspace,"turns":[{"id":TURN,
                    "status":saved.status,"itemsView":"full","items":items}]}})
            }
            _ => {
                wire(
                    &mut output,
                    json!({"id":id,"error":{"code":-32601,"message":"Unsupported synthetic fixture request"}}),
                )?;
                continue;
            }
        };
        wire(&mut output, json!({"id":id,"result":result}))?;
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Codex protocol fixture failed: {error}");
        std::process::exit(1);
    }
}
