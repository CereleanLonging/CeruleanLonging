//! Codex stdio protocol. Never logs wire bodies, credentials or reasoning.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::VecDeque, path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    time::{timeout_at, Instant},
};
#[path = "codex_steer.rs"]
mod steer;
pub use steer::SteerRequest;

const FRAME_LIMIT: usize = 4 * 1024 * 1024;
const REQUEST_LIMIT: usize = 64 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Model {
    pub id: String,
    pub model: String,
    pub is_default: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Connection {
    pub server_version: String,
    pub account_type: Option<String>,
    pub models: Vec<Model>,
    pub model_calls: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Terminal {
    pub thread_id: String,
    pub turn_id: String,
    pub status: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitOutcome {
    Terminal(Terminal),
    CancelRequested(String),
}
/// A wake-up is local only: the sole transport owner must validate and journal advice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlOutcome {
    Finished(WaitOutcome),
    SupervisionReady,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SteerAck {
    pub delivery_id: String,
    pub thread_id: String,
    pub turn_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkerBinding {
    pub thread_id: String,
    pub model: String,
    pub provider: String,
    pub workspace: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnObservation {
    pub thread_id: String,
    pub turn_id: String,
    pub client_message_id: String,
    pub status: String,
    pub prompt_sha256: String,
    pub item_types: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "observation", rename_all = "kebab-case")]
pub enum TurnEvidence {
    RecordedInput(TurnObservation),
    EmptyInterrupted(cerulean_domain::execution::InterruptedTurnObservation),
}
impl TurnEvidence {
    pub fn status(&self) -> &str {
        match self {
            Self::RecordedInput(p) => &p.status,
            Self::EmptyInterrupted(p) => &p.status,
        }
    }
    pub fn turn_id(&self) -> &str {
        match self {
            Self::RecordedInput(p) => &p.turn_id,
            Self::EmptyInterrupted(p) => &p.turn_id,
        }
    }
}

fn observe_evidence(
    result: &Value,
    thread: &str,
    attempt: &str,
    prompt: &str,
    known_turn: Option<&str>,
    workspace: &str,
    allow_empty: bool,
) -> Result<TurnEvidence, String> {
    if allow_empty {
        let turn_id = known_turn.ok_or("empty cancellation proof needs known turn")?;
        let source = &result["thread"];
        let turns = source["turns"]
            .as_array()
            .ok_or("Codex turn history unavailable")?;
        let target: Vec<_> = turns
            .iter()
            .filter(|t| t["id"].as_str() == Some(turn_id))
            .collect();
        if target.len() != 1 {
            return Err("Codex known turn missing or duplicate".into());
        }
        let turn = target[0];
        if turn["status"] == "interrupted"
            && turn["items"]
                .as_array()
                .is_some_and(|items| items.is_empty())
        {
            if source["id"] != thread || turn["itemsView"] != "full" {
                return Err("Codex empty interrupted turn requires bound full view".into());
            }
            let root =
                std::fs::canonicalize(workspace).map_err(|_| "Codex workspace unavailable")?;
            if !source["cwd"]
                .as_str()
                .is_some_and(|s| std::fs::canonicalize(s).is_ok_and(|p| p == root))
            {
                return Err("Codex readback workspace mismatch".into());
            }
            return Ok(TurnEvidence::EmptyInterrupted(
                cerulean_domain::execution::InterruptedTurnObservation {
                    thread_id: thread.into(),
                    turn_id: turn_id.into(),
                    workspace: workspace.into(),
                    status: "interrupted".into(),
                    items_view: "full".into(),
                    item_count: 0,
                    matching_turns: 1,
                },
            ));
        }
    }
    observe_attempt(result, thread, attempt, prompt, known_turn, workspace)
        .map(TurnEvidence::RecordedInput)
}
fn text_hash(text: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(text.as_bytes()))
}
/// Structural diagnostics only: never return IDs, input text or provider error bodies.
fn missing_attempt_diagnostic(turns: &[Value], known_turn: Option<&str>, matches: usize) -> String {
    let known: Vec<_> = turns
        .iter()
        .filter(|turn| known_turn.is_some_and(|id| turn["id"].as_str() == Some(id)))
        .collect();
    let presence = match (known_turn, known.len()) {
        (None, _) => "not-supplied",
        (Some(_), 0) => "absent",
        (Some(_), 1) => "present",
        _ => "duplicate",
    };
    let (status, view) = if known.len() == 1 {
        let status = match known[0]["status"].as_str() {
            Some("completed") => "completed",
            Some("failed") => "failed",
            Some("interrupted") => "interrupted",
            Some("inProgress") => "inProgress",
            None => "missing",
            Some(_) => "other",
        };
        let view = match known[0]["itemsView"].as_str() {
            Some("full") => "full",
            Some("summary") => "summary",
            None => "missing",
            Some(_) => "other",
        };
        (status, view)
    } else {
        ("unavailable", "unavailable")
    };
    let users: Vec<_> = turns
        .iter()
        .filter_map(|turn| turn["items"].as_array())
        .flatten()
        .filter(|item| item["type"] == "userMessage")
        .collect();
    let missing_ids = users
        .iter()
        .filter(|item| item["clientId"].as_str().is_none())
        .count();
    let kind = if matches == 0 { "missing" } else { "ambiguous" };
    format!(
        "Codex attempt missing or ambiguous; do not resend (kind={kind}, turns={}, knownTurn={presence}, knownTurnStatus={status}, knownTurnItemsView={view}, userMessages={}, missingClientIds={missing_ids}, matchedItems={matches})",
        turns.len(),
        users.len()
    )
}
fn observe_attempt(
    result: &Value,
    thread: &str,
    attempt: &str,
    prompt: &str,
    known_turn: Option<&str>,
    workspace: &str,
) -> Result<TurnObservation, String> {
    let source = &result["thread"];
    if source["id"] != thread {
        return Err("Codex readback thread mismatch".into());
    }
    let root = std::fs::canonicalize(workspace).map_err(|_| "Codex workspace unavailable")?;
    if !source["cwd"]
        .as_str()
        .is_some_and(|s| std::fs::canonicalize(s).is_ok_and(|p| p == root))
    {
        return Err("Codex readback workspace mismatch".into());
    }
    let turns = source["turns"]
        .as_array()
        .ok_or("Codex turn history unavailable")?;
    let mut matches = Vec::new();
    for turn in turns {
        let items = turn["items"]
            .as_array()
            .ok_or("Codex turn items unavailable")?;
        for item in items {
            if item["type"] == "userMessage" && item["clientId"] == attempt {
                if turn.get("itemsView").is_some_and(|view| view != "full") {
                    return Err("Codex turn items incomplete".into());
                }
                let content = item["content"].as_array().ok_or("Codex input missing")?;
                if content.len() != 1
                    || content[0]["type"] != "text"
                    || content[0]["text"] != prompt
                {
                    return Err("Codex recorded prompt mismatch".into());
                }
                let turn_id = field(turn, "id")?;
                if known_turn.is_some_and(|expected| expected != turn_id) {
                    return Err("Codex readback turn mismatch".into());
                }
                let status = field(turn, "status")?;
                if !matches!(
                    status.as_str(),
                    "completed" | "failed" | "interrupted" | "inProgress"
                ) {
                    return Err("Codex readback status unknown".into());
                }
                matches.push(TurnObservation {
                    thread_id: thread.into(),
                    turn_id,
                    client_message_id: attempt.into(),
                    status,
                    prompt_sha256: text_hash(prompt),
                    item_types: items
                        .iter()
                        .map(|i| field(i, "type"))
                        .collect::<Result<_, _>>()?,
                });
            }
        }
    }
    if matches.len() != 1 {
        return Err(missing_attempt_diagnostic(turns, known_turn, matches.len()));
    }
    Ok(matches.remove(0))
}

fn request_method_label(method: &str) -> &'static str {
    match method {
        "initialize" => "initialize",
        "account/read" => "account/read",
        "model/list" => "model/list",
        "thread/start" => "thread/start",
        "turn/start" => "turn/start",
        "turn/interrupt" => "turn/interrupt",
        "turn/steer" => "turn/steer",
        "thread/read" => "thread/read",
        _ => "other",
    }
}

struct Transport<R, W> {
    reader: R,
    writer: W,
    partial_frame: Vec<u8>,
    next_id: u64,
    broken: bool,
    terminals: VecDeque<Terminal>,
    denied_requests: u32,
    read_only: bool,
    request_counts: std::collections::BTreeMap<String, u32>,
    observation: Option<crate::codex_observation::ObservationCapture>,
    steer_operations: std::collections::BTreeSet<String>,
}
impl<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin> Transport<R, W> {
    fn new(reader: R, writer: W) -> Self {
        Self {
            reader,
            writer,
            partial_frame: Vec::new(),
            next_id: 1,
            broken: false,
            terminals: VecDeque::new(),
            denied_requests: 0,
            read_only: false,
            request_counts: std::collections::BTreeMap::new(),
            observation: None,
            steer_operations: Default::default(),
        }
    }
    async fn send(&mut self, value: Value, deadline: Instant) -> Result<(), String> {
        let mut bytes = serde_json::to_vec(&value).map_err(|_| "invalid request")?;
        if bytes.len() > REQUEST_LIMIT {
            return Err("Codex request exceeds limit".into());
        }
        bytes.push(b'\n');
        timeout_at(deadline, async {
            self.writer.write_all(&bytes).await?;
            self.writer.flush().await
        })
        .await
        .map_err(|_| "Codex send timeout; delivery unknown")?
        .map_err(|_| "Codex write failed; delivery unknown".into())
    }
    async fn read(&mut self, deadline: Instant) -> Result<Value, String> {
        timeout_at(deadline, async {
            loop {
                let available = self
                    .reader
                    .fill_buf()
                    .await
                    .map_err(|_| "Codex read failed")?;
                if available.is_empty() {
                    return Err("Codex stream ended; result unknown".into());
                }
                let end = available.iter().position(|b| *b == b'\n');
                let n = end.map_or(available.len(), |i| i + 1);
                if self.partial_frame.len() + n > FRAME_LIMIT {
                    return Err("Codex frame exceeds limit".into());
                }
                // This buffer survives dropping only the pending read future
                // when the same owner receives a cancellation request.
                self.partial_frame.extend_from_slice(&available[..n]);
                self.reader.consume(n);
                if end.is_some() {
                    break;
                }
            }
            let bytes = std::mem::take(&mut self.partial_frame);
            let value: Value = serde_json::from_slice(&bytes)
                .map_err(|_| "Invalid Codex JSON frame (body omitted)")?;
            if !value.is_object() {
                return Err("Invalid Codex message".into());
            }
            Ok(value)
        })
        .await
        .map_err(|_| "Codex observation timeout; result unknown")?
    }
    async fn notification(&mut self, value: &Value, deadline: Instant) -> Result<bool, String> {
        let Some(method) = value.get("method").and_then(Value::as_str) else {
            return Ok(false);
        };
        if let Some(id) = value.get("id") {
            self.denied_requests = self.denied_requests.saturating_add(1);
            let response = if matches!(
                method,
                "item/commandExecution/requestApproval" | "item/fileChange/requestApproval"
            ) {
                json!({"id":id,"result":{"decision":"decline"}})
            } else {
                json!({"id":id,"error":{"code":-32601,"message":"This client does not grant extra tools, approvals or credentials"}})
            };
            self.send(response, deadline).await?;
        } else if method == "turn/completed" {
            let p = &value["params"];
            let terminal = Terminal {
                thread_id: field(p, "threadId")?,
                turn_id: field(&p["turn"], "id")?,
                status: field(&p["turn"], "status")?,
            };
            if !matches!(
                terminal.status.as_str(),
                "completed" | "failed" | "interrupted"
            ) {
                return Err("unknown Codex terminal status".into());
            }
            if self.terminals.len() >= 64 {
                return Err(
                    "Codex terminal backlog exceeds limit; reconcile before continuing".into(),
                );
            }
            self.terminals.push_back(terminal);
        }
        if let Some(capture) = &self.observation {
            capture.observe(value);
        }
        // Unsupported notifications, raw reasoning and tool output are discarded.
        Ok(true)
    }
    async fn request(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, String> {
        if self.read_only
            && !matches!(
                method,
                "initialize" | "account/read" | "model/list" | "thread/read"
            )
        {
            return Err("Codex reconciliation channel forbids mutating requests".into());
        }
        if self.broken {
            return Err("Codex transport uncertain; reconnect and reconcile".into());
        }
        let id = self.next_id;
        self.next_id += 1;
        let deadline = Instant::now() + timeout;
        let count = self
            .request_counts
            .entry(request_method_label(method).into())
            .or_default();
        *count = count.saturating_add(1);
        let result = async {
            self.send(json!({"id":id,"method":method,"params":params}), deadline)
                .await?;
            loop {
                let value = self.read(deadline).await?;
                if self.notification(&value, deadline).await? {
                    continue;
                }
                if value.get("id").and_then(Value::as_u64) != Some(id) {
                    return Err("Codex reply identity mismatch".into());
                }
                if value.get("error").is_some() {
                    return Err(format!(
                        "Codex RPC rejected request (method {}, code {})",
                        request_method_label(method),
                        value["error"]["code"].as_i64().unwrap_or(-1)
                    ));
                }
                return value
                    .get("result")
                    .cloned()
                    .ok_or_else(|| "Codex result missing".into());
            }
        }
        .await;
        if result.is_err() {
            self.broken = true;
        }
        result
    }
    async fn terminal(
        &mut self,
        thread: &str,
        turn: &str,
        timeout: Duration,
    ) -> Result<Terminal, String> {
        if self.broken {
            return Err("Codex transport uncertain; reconcile".into());
        }
        let deadline = Instant::now() + timeout;
        let result = async {
            loop {
                if let Some(index) = self
                    .terminals
                    .iter()
                    .position(|t| t.thread_id == thread && t.turn_id == turn)
                {
                    return Ok(self.terminals.remove(index).unwrap());
                }
                let value = self.read(deadline).await?;
                if !self.notification(&value, deadline).await? {
                    return Err("unexpected Codex reply while observing".into());
                }
            }
        }
        .await;
        if result.is_err() {
            self.broken = true;
        }
        result
    }
    async fn terminal_or_cancel(
        &mut self,
        thread: &str,
        turn: &str,
        timeout: Duration,
        cancel: &mut tokio::sync::oneshot::Receiver<String>,
    ) -> Result<WaitOutcome, String> {
        match self
            .terminal_or_control(thread, turn, timeout, cancel, None)
            .await?
        {
            ControlOutcome::Finished(outcome) => Ok(outcome),
            ControlOutcome::SupervisionReady => {
                Err("unexpected unbound supervision wake-up".into())
            }
        }
    }
    async fn terminal_or_control(
        &mut self,
        thread: &str,
        turn: &str,
        timeout: Duration,
        cancel: &mut tokio::sync::oneshot::Receiver<String>,
        mut supervision: Option<&mut tokio::sync::watch::Receiver<()>>,
    ) -> Result<ControlOutcome, String> {
        if self.broken {
            return Err("Codex transport uncertain; reconcile".into());
        }
        let deadline = Instant::now() + timeout;
        let result = async {
            let mut cancel_open = true;
            loop {
                if let Some(index) = self
                    .terminals
                    .iter()
                    .position(|t| t.thread_id == thread && t.turn_id == turn)
                {
                    return Ok(ControlOutcome::Finished(WaitOutcome::Terminal(self.terminals.remove(index).unwrap())));
                }
                let value = {
                    tokio::select! {
                        // A terminal already readable wins a simultaneous cancel.
                        biased;
                        value = self.read(deadline) => value?,
                        reason = &mut *cancel, if cancel_open => {
                            match reason {
                                Ok(reason) => return Ok(ControlOutcome::Finished(WaitOutcome::CancelRequested(reason))),
                                Err(_) => cancel_open = false,
                            }
                            continue;
                        }
                        wake = async { supervision.as_mut().expect("guarded control").changed().await }, if supervision.is_some() => {
                            match wake {
                                Ok(()) => return Ok(ControlOutcome::SupervisionReady),
                                Err(_) => supervision = None,
                            }
                            continue;
                        }
                    }
                };
                // In particular, never drop an approval response/send future.
                if !self.notification(&value, deadline).await? {
                    return Err("unexpected Codex reply while observing".into());
                }
                if self
                    .terminals
                    .iter()
                    .any(|t| t.thread_id == thread && t.turn_id == turn)
                {
                    continue;
                }
                // A notification flood must not starve a ready cancellation.
                if cancel_open {
                    match cancel.try_recv() {
                        Ok(reason) => return Ok(ControlOutcome::Finished(WaitOutcome::CancelRequested(reason))),
                        Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                            cancel_open = false;
                        }
                        Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
                    }
                }
                // Readable notifications must not starve queued advice. Cancellation wins.
                if let Some(control) = supervision.as_mut() {
                    match control.has_changed() {
                        Ok(true) => {
                            control.borrow_and_update();
                            return Ok(ControlOutcome::SupervisionReady);
                        }
                        Err(_) => supervision = None,
                        Ok(false) => {}
                    }
                }
            }
        }
        .await;
        if result.is_err() {
            self.broken = true;
        }
        result
    }
}
fn field(value: &Value, key: &str) -> Result<String, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 1024 && !s.chars().any(char::is_control))
        .map(str::to_owned)
        .ok_or_else(|| format!("invalid Codex field: {key}"))
}

pub struct CodexClient {
    child: Child,
    rpc: Transport<BufReader<ChildStdout>, ChildStdin>,
    workspace: String,
    active: Option<(String, String)>,
    binding: Option<WorkerBinding>,
}
impl CodexClient {
    pub fn capture_observations(&mut self, capture: crate::codex_observation::ObservationCapture) {
        self.rpc.observation = Some(capture);
    }
    pub fn activate_observations(&mut self, thread: &str, turn: &str) -> Result<(), String> {
        if self.active.as_ref() != Some(&(thread.into(), turn.into())) {
            return Err("observation turn is not active".into());
        }
        if let Some(capture) = &self.rpc.observation {
            capture.activate(thread, turn)?;
        }
        Ok(())
    }
    pub fn invalidate(&mut self) {
        self.rpc.broken = true;
    }
    pub fn launch(executable: &Path, workspace: &Path) -> Result<Self, String> {
        if !executable.is_absolute()
            || !workspace.is_absolute()
            || !executable.is_file()
            || !workspace.is_dir()
        {
            return Err("Codex executable and workspace must be existing absolute paths".into());
        }
        let mut command = Command::new(executable);
        command
            .args([
                "app-server",
                "--listen",
                "stdio://",
                "-c",
                "features.hooks=false",
                "-c",
                "features.apps=false",
                "-c",
                "features.multi_agent=false",
                "-c",
                "web_search=\"disabled\"",
            ])
            .current_dir(workspace)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let mut child = command.spawn().map_err(|_| "Codex process launch failed")?;
        let stdout = child.stdout.take().ok_or("Codex stdout missing")?;
        let stdin = child.stdin.take().ok_or("Codex stdin missing")?;
        Ok(Self {
            child,
            rpc: Transport::new(BufReader::new(stdout), stdin),
            workspace: workspace.to_str().ok_or("non-Unicode workspace")?.into(),
            active: None,
            binding: None,
        })
    }
    pub fn launch_readonly(executable: &Path, workspace: &Path) -> Result<Self, String> {
        let mut client = Self::launch(executable, workspace)?;
        client.rpc.read_only = true;
        Ok(client)
    }
    pub fn request_counts(&self) -> &std::collections::BTreeMap<String, u32> {
        &self.rpc.request_counts
    }
    pub async fn connect(&mut self) -> Result<Connection, String> {
        let timeout = Duration::from_secs(20);
        let initialized=self.rpc.request("initialize",json!({"clientInfo":{"name":"cerulean_longing","title":"CeruleanLonging","version":env!("CARGO_PKG_VERSION")}}),timeout).await?;
        self.rpc
            .send(
                json!({"method":"initialized","params":{}}),
                Instant::now() + timeout,
            )
            .await?;
        let account = self
            .rpc
            .request("account/read", json!({"refreshToken":false}), timeout)
            .await?;
        // Keep only authentication type; account e-mail/tokens never leave here.
        let account_type = if account["account"].is_null() {
            None
        } else {
            Some(field(&account["account"], "type")?)
        };
        let mut models = Vec::new();
        let mut cursor = Value::Null;
        for _ in 0..10 {
            let result = self
                .rpc
                .request(
                    "model/list",
                    json!({"limit":50,"includeHidden":false,"cursor":cursor}),
                    timeout,
                )
                .await?;
            for model in result["data"]
                .as_array()
                .ok_or("Codex model list missing")?
            {
                models.push(Model {
                    id: field(model, "id")?,
                    model: field(model, "model")?,
                    is_default: model["isDefault"].as_bool().unwrap_or(false),
                });
            }
            if models.len() > 500 {
                return Err("Codex model catalog too large".into());
            }
            cursor = result["nextCursor"].clone();
            if cursor.is_null() {
                break;
            }
        }
        if !cursor.is_null() {
            return Err("Codex model pagination incomplete".into());
        }
        Ok(Connection {
            server_version: field(&initialized, "userAgent")?,
            account_type,
            models,
            model_calls: 0,
        })
    }
    /// Trusted controller must persist a session binding before turn dispatch.
    pub async fn create_worker(&mut self, model: &str) -> Result<WorkerBinding, String> {
        if self.active.is_some() || self.binding.is_some() {
            return Err("Codex turn already active".into());
        }
        let current = self
            .rpc
            .request(
                "config/read",
                json!({"includeLayers":false,"cwd":self.workspace}),
                Duration::from_secs(20),
            )
            .await?;
        let disabled = |key: &str| -> Value {
            Value::Object(
                current["config"][key]
                    .as_object()
                    .map(|map| {
                        map.keys()
                            .map(|id| (id.clone(), json!({"enabled":false})))
                            .collect()
                    })
                    .unwrap_or_default(),
            )
        };
        let result=self.rpc.request("thread/start",json!({
            "model":model,"modelProvider":"openai","cwd":self.workspace,"sandbox":"workspace-write","approvalPolicy":"never","ephemeral":false,
            "config":{"features":{"hooks":false,"apps":false,"multi_agent":false},"web_search":"disabled","mcp_servers":disabled("mcp_servers"),"plugins":disabled("plugins"),
                "sandbox_workspace_write":{"writable_roots":[self.workspace],"network_access":false,"exclude_tmpdir_env_var":true,"exclude_slash_tmp":true}},
            "developerInstructions":"Execute only the approved task in the supplied workspace. Do not inspect credentials, change other projects, use network, install packages, spawn agents, or request broader permissions. Report restrictions instead of bypassing them. Completion will be independently verified."
        }),Duration::from_secs(30)).await?;
        let expected_root =
            std::fs::canonicalize(&self.workspace).map_err(|_| "Codex workspace disappeared")?;
        let same = |v: &Value| {
            v.as_str()
                .is_some_and(|s| std::fs::canonicalize(s).is_ok_and(|path| path == expected_root))
        };
        let sandbox = &result["sandbox"];
        if result["modelProvider"] != "openai"
            || result["model"] != model
            || result["approvalPolicy"] != "never"
            || !same(&result["cwd"])
            || sandbox["type"] != "workspaceWrite"
            || sandbox["networkAccess"] != false
            || sandbox["excludeTmpdirEnvVar"] != true
            || sandbox["excludeSlashTmp"] != true
            || !sandbox["writableRoots"]
                .as_array()
                .is_some_and(|roots| roots.iter().all(same))
        {
            return Err("Codex did not confirm approved worker scope".into());
        }
        let binding = WorkerBinding {
            thread_id: field(&result["thread"], "id")?,
            model: field(&result, "model")?,
            provider: field(&result, "modelProvider")?,
            workspace: self.workspace.clone(),
        };
        self.binding = Some(binding.clone());
        Ok(binding)
    }
    /// Caller must persist dispatch intent first. Errors never trigger a retry.
    pub async fn start_turn(
        &mut self,
        binding: &WorkerBinding,
        attempt: &str,
        prompt: &str,
    ) -> Result<String, String> {
        if self.active.is_some()
            || self.binding.as_ref() != Some(binding)
            || binding.workspace != self.workspace
        {
            return Err("Codex worker binding mismatch or active turn".into());
        }
        cerulean_domain::execution::identifier(attempt)?;
        if let Some(capture) = &self.rpc.observation {
            capture.prepare(&binding.thread_id)?;
        }
        // Missing ACK/turn identity cannot permit another dispatch on this client.
        self.active = Some((binding.thread_id.clone(), "unknown".into()));
        let result=self.rpc.request("turn/start",json!({"threadId":binding.thread_id,"clientUserMessageId":attempt,"cwd":self.workspace,"approvalPolicy":"never",
            "sandboxPolicy":{"type":"workspaceWrite","writableRoots":[self.workspace],"networkAccess":false,"excludeTmpdirEnvVar":true,"excludeSlashTmp":true},
            "input":[{"type":"text","text":prompt,"text_elements":[]}]}),Duration::from_secs(30)).await?;
        let turn = match field(&result["turn"], "id") {
            Ok(turn) => turn,
            Err(error) => {
                self.rpc.broken = true;
                return Err(error);
            }
        };
        self.active = Some((binding.thread_id.clone(), turn.clone()));
        Ok(turn)
    }
    pub async fn wait_turn(
        &mut self,
        thread: &str,
        turn: &str,
        timeout: Duration,
    ) -> Result<Terminal, String> {
        if self.active.as_ref() != Some(&(thread.into(), turn.into())) {
            return Err("Codex turn binding mismatch".into());
        }
        let result = self.rpc.terminal(thread, turn, timeout).await?;
        self.active = None;
        Ok(result)
    }
    pub async fn wait_turn_or_cancel(
        &mut self,
        thread: &str,
        turn: &str,
        timeout: Duration,
        cancel: &mut tokio::sync::oneshot::Receiver<String>,
    ) -> Result<WaitOutcome, String> {
        if self.active.as_ref() != Some(&(thread.into(), turn.into())) {
            return Err("Codex turn binding mismatch".into());
        }
        let result = self
            .rpc
            .terminal_or_cancel(thread, turn, timeout, cancel)
            .await?;
        if matches!(result, WaitOutcome::Terminal(_)) {
            self.active = None;
        }
        Ok(result)
    }
    pub async fn interrupt(&mut self, thread: &str, turn: &str) -> Result<Terminal, String> {
        if self.active.as_ref() != Some(&(thread.into(), turn.into())) {
            return Err("Codex interrupt binding mismatch".into());
        }
        if self
            .rpc
            .terminals
            .iter()
            .any(|item| item.thread_id == thread && item.turn_id == turn)
        {
            return self.wait_turn(thread, turn, Duration::from_secs(1)).await;
        }
        self.rpc
            .request(
                "turn/interrupt",
                json!({"threadId":thread,"turnId":turn}),
                Duration::from_secs(15),
            )
            .await?;
        self.wait_turn(thread, turn, Duration::from_secs(15)).await
    }
    /// Read-only reconciliation works on a fresh connection without starting a turn.
    pub async fn read_attempt(
        &mut self,
        thread: &str,
        attempt: &str,
        prompt: &str,
        known_turn: Option<&str>,
    ) -> Result<TurnObservation, String> {
        let result = self
            .rpc
            .request(
                "thread/read",
                json!({"threadId":thread,"includeTurns":true}),
                Duration::from_secs(30),
            )
            .await?;
        observe_attempt(
            &result,
            thread,
            attempt,
            prompt,
            known_turn,
            &self.workspace,
        )
    }
    /// One read, with an explicit caller-owned durable-anchor gate for empty cancellation.
    pub async fn read_evidence(
        &mut self,
        thread: &str,
        attempt: &str,
        prompt: &str,
        known_turn: Option<&str>,
        allow_empty: bool,
    ) -> Result<TurnEvidence, String> {
        let result = self
            .rpc
            .request(
                "thread/read",
                json!({"threadId":thread,"includeTurns":true}),
                Duration::from_secs(30),
            )
            .await?;
        observe_evidence(
            &result,
            thread,
            attempt,
            prompt,
            known_turn,
            &self.workspace,
            allow_empty,
        )
    }
    pub async fn close(mut self) -> Result<(), String> {
        // ChildStdin::shutdown is a no-op on Windows. Drop the pipe handle so
        // an idle app-server can actually observe EOF before we wait for it.
        drop(self.rpc.writer);
        match timeout_at(Instant::now() + Duration::from_secs(2), self.child.wait()).await {
            Ok(Ok(status)) if status.success() => {}
            Ok(Ok(_)) => return Err("Codex process exited unsuccessfully".into()),
            Ok(Err(_)) => return Err("Codex process wait failed".into()),
            Err(_) => {
                self.child
                    .kill()
                    .await
                    .map_err(|_| "Codex process cleanup failed")?;
                return Err("Codex process required forced cleanup after close timeout".into());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{duplex, split};

    #[cfg(windows)]
    async fn eof_client(exit_code: u32) -> CodexClient {
        let shell = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let script = format!(
            "[Console]::Out.WriteLine('READY'); [Console]::Out.Flush(); $null = [Console]::In.ReadToEnd(); exit {exit_code}"
        );
        let mut command = Command::new(shell);
        command
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .creation_flags(0x08000000);
        let mut child = command.spawn().unwrap();
        let stdin = child.stdin.take().unwrap();
        let mut reader = BufReader::new(child.stdout.take().unwrap());
        let mut ready = String::new();
        timeout_at(
            Instant::now() + Duration::from_secs(10),
            reader.read_line(&mut ready),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(ready.trim(), "READY");
        CodexClient {
            child,
            rpc: Transport::new(reader, stdin),
            workspace: std::env::current_dir().unwrap().to_string_lossy().into(),
            active: None,
            binding: None,
        }
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn close_releases_stdin_for_normal_eof_exit() {
        eof_client(0).await.close().await.unwrap();
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn close_reports_original_nonzero_eof_exit() {
        assert_eq!(
            eof_client(7).await.close().await.unwrap_err(),
            "Codex process exited unsuccessfully"
        );
    }

    fn terminal_frame(status: &str) -> Vec<u8> {
        let mut frame = serde_json::to_vec(&json!({
            "method":"turn/completed",
            "params":{"threadId":"thread","turn":{"id":"turn","status":status}}
        }))
        .unwrap();
        frame.push(b'\n');
        frame
    }

    #[tokio::test]
    async fn observation_flood_does_not_starve_cancel_or_consume_terminal() {
        let (client, mut server) = duplex(256 * 1024);
        let (read, write) = split(client);
        let mut rpc = Transport::new(BufReader::new(read), write);
        let capture = crate::codex_observation::ObservationCapture::default();
        capture.prepare("thread").unwrap();
        capture.activate("thread", "turn").unwrap();
        rpc.observation = Some(capture.clone());
        for n in 0..200 {
            let mut bytes = serde_json::to_vec(&json!({"method":"item/completed",
                "params":{"threadId":"thread","turnId":"turn","item":{
                    "type":"commandExecution","id":format!("item-{n}"),
                    "status":"failed","exitCode":7,"command":"fixture"}}}))
            .unwrap();
            bytes.push(b'\n');
            server.write_all(&bytes).await.unwrap();
        }
        server
            .write_all(&terminal_frame("interrupted"))
            .await
            .unwrap();
        let (sender, mut cancel) = tokio::sync::oneshot::channel();
        sender.send("Stop during observation flood".into()).unwrap();
        assert_eq!(
            rpc.terminal_or_cancel("thread", "turn", Duration::from_secs(1), &mut cancel)
                .await
                .unwrap(),
            WaitOutcome::CancelRequested("Stop during observation flood".into())
        );
        assert_eq!(capture.snapshot().events.len(), 1);
        assert!(!rpc.broken);
        assert_eq!(
            rpc.terminal("thread", "turn", Duration::from_secs(1))
                .await
                .unwrap()
                .status,
            "interrupted"
        );
        let trace = capture.snapshot();
        assert_eq!(trace.events.len(), 128);
        assert_eq!(trace.dropped, 72);
        assert_eq!(trace.rejected, 0);
    }

    #[tokio::test]
    async fn cancellation_preserves_partial_frame_for_interrupt_ack_and_terminal() {
        let (client, mut server) = duplex(4096);
        let (read, write) = split(client);
        let mut rpc = Transport::new(BufReader::new(read), write);
        let partial = b"{\"method\":\"item/agentMessage/delta\",\"params\":";
        server.write_all(partial).await.unwrap();
        let (sender, mut cancel) = tokio::sync::oneshot::channel();
        sender.send("Stop this attempt".into()).unwrap();
        assert_eq!(
            rpc.terminal_or_cancel("thread", "turn", Duration::from_secs(1), &mut cancel)
                .await
                .unwrap(),
            WaitOutcome::CancelRequested("Stop this attempt".into())
        );
        assert_eq!(rpc.partial_frame, partial);
        assert!(!rpc.broken);

        // Finish the pre-cancel frame before acknowledging the new interrupt.
        server
            .write_all(b"{}}\n{\"id\":1,\"result\":{}}\n")
            .await
            .unwrap();
        assert_eq!(
            rpc.request(
                "turn/interrupt",
                json!({"threadId":"thread","turnId":"turn"}),
                Duration::from_secs(1)
            )
            .await
            .unwrap(),
            json!({})
        );
        assert!(
            rpc.terminals.is_empty(),
            "an interrupt ACK is not a terminal"
        );
        assert!(rpc.partial_frame.is_empty());
        server
            .write_all(&terminal_frame("interrupted"))
            .await
            .unwrap();
        assert_eq!(
            rpc.terminal("thread", "turn", Duration::from_secs(1))
                .await
                .unwrap()
                .status,
            "interrupted"
        );
        let mut request = String::new();
        BufReader::new(server)
            .read_line(&mut request)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&request).unwrap()["method"],
            "turn/interrupt"
        );
    }

    #[tokio::test]
    async fn cached_or_readable_completion_wins_simultaneous_cancellation() {
        for cached in [false, true] {
            let (client, mut server) = duplex(4096);
            let (read, write) = split(client);
            let mut rpc = Transport::new(BufReader::new(read), write);
            if cached {
                rpc.terminals.push_back(Terminal {
                    thread_id: "thread".into(),
                    turn_id: "turn".into(),
                    status: "completed".into(),
                });
            } else {
                server
                    .write_all(&terminal_frame("completed"))
                    .await
                    .unwrap();
            }
            let (sender, mut cancel) = tokio::sync::oneshot::channel();
            sender
                .send("A concurrent user cancellation".into())
                .unwrap();
            let outcome = rpc
                .terminal_or_cancel("thread", "turn", Duration::from_secs(1), &mut cancel)
                .await
                .unwrap();
            assert!(matches!(outcome, WaitOutcome::Terminal(t) if t.status == "completed"));
            assert_eq!(cancel.try_recv().unwrap(), "A concurrent user cancellation");
            assert!(!rpc.broken);
        }
    }

    #[tokio::test]
    async fn disconnected_cancel_control_continues_waiting_for_the_original_turn() {
        let (client, mut server) = duplex(4096);
        let (read, write) = split(client);
        let mut rpc = Transport::new(BufReader::new(read), write);
        let (sender, mut cancel) = tokio::sync::oneshot::channel();
        drop(sender);
        let wait = rpc.terminal_or_cancel("thread", "turn", Duration::from_secs(1), &mut cancel);
        tokio::pin!(wait);
        // Poll the disconnected receiver before making any wire result available.
        tokio::select! {
            biased;
            result = &mut wait => panic!("control disconnection ended observation: {result:?}"),
            _ = tokio::task::yield_now() => {}
        }
        server
            .write_all(&terminal_frame("completed"))
            .await
            .unwrap();
        assert!(matches!(wait.await.unwrap(), WaitOutcome::Terminal(t) if t.status == "completed"));
    }

    #[tokio::test]
    async fn cancellation_waits_for_a_complete_approval_denial_write() {
        let (client, server) = duplex(16);
        let (read, write) = split(client);
        let mut rpc = Transport::new(BufReader::new(read), write);
        let (sender, mut cancel) = tokio::sync::oneshot::channel();
        let (done, finished) = tokio::sync::oneshot::channel::<()>();
        let server = tokio::spawn(async move {
            let (read, mut write) = split(server);
            write
                .write_all(
                    b"{\"id\":99,\"method\":\"item/fileChange/requestApproval\",\"params\":{}}\n",
                )
                .await
                .unwrap();
            sender
                .send("Cancel while declining approval".into())
                .unwrap();
            let mut read = BufReader::new(read);
            let mut line = String::new();
            read.read_line(&mut line).await.unwrap();
            let denied: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(denied["id"], 99);
            assert_eq!(denied["result"]["decision"], "decline");
            // Keep the stream open so EOF cannot mask cancellation handling.
            finished.await.unwrap();
        });
        assert_eq!(
            rpc.terminal_or_cancel("thread", "turn", Duration::from_secs(1), &mut cancel)
                .await
                .unwrap(),
            WaitOutcome::CancelRequested("Cancel while declining approval".into())
        );
        assert_eq!(rpc.denied_requests, 1);
        assert!(!rpc.broken);
        done.send(()).unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn interrupt_without_confirmed_terminal_cannot_be_repeated() {
        for acknowledge in [false, true] {
            let (client, mut server) = duplex(4096);
            let (read, write) = split(client);
            let mut rpc = Transport::new(BufReader::new(read), write);
            if acknowledge {
                server
                    .write_all(b"{\"id\":1,\"result\":{}}\n")
                    .await
                    .unwrap();
            }
            let result = rpc
                .request("turn/interrupt", json!({}), Duration::from_millis(20))
                .await;
            if acknowledge {
                result.unwrap();
                assert!(rpc.terminals.is_empty());
                assert!(rpc
                    .terminal("thread", "turn", Duration::from_millis(20))
                    .await
                    .is_err());
            } else {
                assert!(result.is_err());
            }
            assert!(rpc.broken);
            assert!(rpc
                .request("turn/interrupt", json!({}), Duration::from_millis(20))
                .await
                .is_err());
            let mut read = BufReader::new(server);
            let mut line = String::new();
            read.read_line(&mut line).await.unwrap();
            assert_eq!(serde_json::from_str::<Value>(&line).unwrap()["id"], 1);
            assert!(timeout_at(
                Instant::now() + Duration::from_millis(20),
                read.read_line(&mut String::new())
            )
            .await
            .is_err());
        }
    }

    #[tokio::test]
    async fn readonly_channel_rejects_mutation_before_any_wire_bytes() {
        let (client, server) = duplex(4096);
        let (read, write) = split(client);
        let mut rpc = Transport::new(BufReader::new(read), write);
        rpc.read_only = true;
        for method in [
            "thread/start",
            "thread/resume",
            "turn/start",
            "turn/interrupt",
            "turn/steer",
            "config/read",
        ] {
            assert!(rpc
                .request(method, json!({}), Duration::from_millis(20))
                .await
                .unwrap_err()
                .contains("forbids"));
        }
        assert!(rpc.request_counts.is_empty());
        assert_eq!(rpc.next_id, 1);
        let mut reader = BufReader::new(server);
        let mut line = String::new();
        assert!(timeout_at(
            Instant::now() + Duration::from_millis(20),
            reader.read_line(&mut line)
        )
        .await
        .is_err());
        assert!(line.is_empty());
    }
    #[test]
    fn empty_interruption_requires_unique_full_bound_turn_and_explicit_gate() {
        let workspace = std::env::current_dir().unwrap();
        let workspace = workspace.to_str().unwrap();
        let record = json!({"thread":{"id":"thread","cwd":workspace,"turns":[
            {"id":"turn","status":"interrupted","itemsView":"full","items":[]}]}});
        let read = |value: &Value, known, allow| {
            observe_evidence(
                value,
                "thread",
                "attempt",
                "secret-prompt",
                known,
                workspace,
                allow,
            )
        };
        let proof = read(&record, Some("turn"), true).unwrap();
        assert!(matches!(proof, TurnEvidence::EmptyInterrupted(_)));
        let saved = serde_json::to_string(&proof).unwrap();
        for forbidden in ["secret-prompt", "clientMessageId", "promptSha256"] {
            assert!(!saved.contains(forbidden));
        }
        assert!(read(&record, Some("turn"), false).is_err());
        assert!(read(&record, None, true).is_err());
        assert!(read(&record, Some("other"), true).is_err());
        let mut cases = Vec::new();
        for status in ["completed", "failed", "inProgress", "other"] {
            let mut value = record.clone();
            value["thread"]["turns"][0]["status"] = status.into();
            cases.push(value);
        }
        for view in [Value::Null, json!("summary"), json!("other")] {
            let mut value = record.clone();
            value["thread"]["turns"][0]["itemsView"] = view;
            cases.push(value);
        }
        for items in [
            Value::Null,
            json!({}),
            json!([{"type":"reasoning"}]),
            json!([{"type":"userMessage","clientId":"wrong","content":[]}]),
        ] {
            let mut value = record.clone();
            value["thread"]["turns"][0]["items"] = items;
            cases.push(value);
        }
        let mut duplicate = record.clone();
        duplicate["thread"]["turns"]
            .as_array_mut()
            .unwrap()
            .push(record["thread"]["turns"][0].clone());
        cases.push(duplicate);
        let mut wrong = record.clone();
        wrong["thread"]["id"] = "other".into();
        cases.push(wrong);
        let mut wrong = record.clone();
        wrong["thread"]["cwd"] = "missing-workspace".into();
        cases.push(wrong);
        for value in cases {
            assert!(read(&value, Some("turn"), true).is_err());
        }
        let mut unrelated = record.clone();
        unrelated["thread"]["turns"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":"unrelated","status":"completed","items":[]}));
        assert!(read(&unrelated, Some("turn"), true).is_ok());
    }

    #[test]
    fn readback_requires_exact_attempt_prompt_and_unique_full_turn() {
        let workspace = std::env::current_dir().unwrap();
        let workspace = workspace.to_str().unwrap();
        let record = json!({"thread":{"id":"thread","cwd":workspace,"turns":[{"id":"turn","status":"completed","itemsView":"full","items":[{"type":"userMessage","clientId":"attempt","content":[{"type":"text","text":"approved prompt"}]},{"type":"reasoning","text":"private"}]}]}});
        let proof = observe_attempt(
            &record,
            "thread",
            "attempt",
            "approved prompt",
            None,
            workspace,
        )
        .unwrap();
        assert_eq!(proof.turn_id, "turn");
        assert!(!serde_json::to_string(&proof).unwrap().contains("private"));
        for (thread, attempt, prompt, known) in [
            ("other", "attempt", "approved prompt", None),
            ("thread", "other", "approved prompt", None),
            ("thread", "attempt", "different", None),
            ("thread", "attempt", "approved prompt", Some("other")),
        ] {
            assert!(observe_attempt(&record, thread, attempt, prompt, known, workspace).is_err());
        }
        let mut duplicate = record.clone();
        let turn = duplicate["thread"]["turns"][0].clone();
        duplicate["thread"]["turns"]
            .as_array_mut()
            .unwrap()
            .push(turn);
        assert!(observe_attempt(
            &duplicate,
            "thread",
            "attempt",
            "approved prompt",
            None,
            workspace
        )
        .is_err());
        let mut partial = record;
        partial["thread"]["turns"][0]["itemsView"] = "summary".into();
        assert!(observe_attempt(
            &partial,
            "thread",
            "attempt",
            "approved prompt",
            None,
            workspace
        )
        .is_err());
    }
    #[test]
    fn readback_missing_client_id_reports_structure_without_provider_content() {
        let workspace = std::env::current_dir().unwrap();
        let workspace = workspace.to_str().unwrap();
        for client_id in [None, Some(Value::Null), Some(json!("private-client-id"))] {
            let mut item =
                json!({"type":"userMessage","content":[{"type":"text","text":"private-prompt"}]});
            if let Some(id) = &client_id {
                item["clientId"] = id.clone();
            }
            let record = json!({"thread":{"id":"private-thread","cwd":workspace,"turns":[{
                "id":"private-turn","status":"interrupted","itemsView":"full","items":[item]
            }]}});
            let error = observe_attempt(
                &record,
                "private-thread",
                "private-attempt",
                "private-prompt",
                Some("private-turn"),
                workspace,
            )
            .unwrap_err();
            let missing = usize::from(client_id.as_ref().is_none_or(|id| id.as_str().is_none()));
            assert_eq!(
                error,
                format!(
                    "Codex attempt missing or ambiguous; do not resend (kind=missing, turns=1, knownTurn=present, knownTurnStatus=interrupted, knownTurnItemsView=full, userMessages=1, missingClientIds={missing}, matchedItems=0)"
                )
            );
            for secret in [
                "private-thread",
                "private-turn",
                "private-attempt",
                "private-prompt",
                "private-client-id",
                workspace,
            ] {
                assert!(!error.contains(secret));
            }
        }
    }

    #[test]
    fn duplicate_readback_items_and_turns_remain_ambiguous_with_safe_counts() {
        let workspace = std::env::current_dir().unwrap();
        let workspace = workspace.to_str().unwrap();
        let item = json!({"type":"userMessage","clientId":"private-attempt","content":[{"type":"text","text":"private-prompt"}]});
        for duplicate_turn in [false, true] {
            let mut turn = json!({"id":"private-turn","status":"interrupted","itemsView":"full","items":[item.clone()]});
            let turns = if duplicate_turn {
                vec![turn.clone(), turn]
            } else {
                turn["items"].as_array_mut().unwrap().push(item.clone());
                vec![turn]
            };
            let record = json!({"thread":{"id":"private-thread","cwd":workspace,"turns":turns}});
            let error = observe_attempt(
                &record,
                "private-thread",
                "private-attempt",
                "private-prompt",
                Some("private-turn"),
                workspace,
            )
            .unwrap_err();
            assert!(error.contains("kind=ambiguous"));
            assert!(error.contains("userMessages=2, missingClientIds=0, matchedItems=2"));
            assert!(error.contains(if duplicate_turn {
                "turns=2, knownTurn=duplicate, knownTurnStatus=unavailable"
            } else {
                "turns=1, knownTurn=present, knownTurnStatus=interrupted"
            }));
            assert!(!error.contains("private-"));
            assert!(!error.contains(workspace));
        }
    }

    #[test]
    fn early_cancel_empty_turn_and_missing_known_turn_stay_unproven() {
        let workspace = std::env::current_dir().unwrap();
        let workspace = workspace.to_str().unwrap();
        let record = json!({"thread":{"id":"private-thread","cwd":workspace,"turns":[{
            "id":"private-turn","status":"interrupted","itemsView":"full","items":[]
        }]}});
        for (known, presence) in [
            (Some("private-turn"), "present"),
            (Some("other-private-turn"), "absent"),
            (None, "not-supplied"),
        ] {
            let error = observe_attempt(
                &record,
                "private-thread",
                "private-attempt",
                "private-prompt",
                known,
                workspace,
            )
            .unwrap_err();
            assert!(error.contains("kind=missing"));
            assert!(error.contains(&format!("knownTurn={presence},")));
            assert!(error.contains("userMessages=0, missingClientIds=0, matchedItems=0"));
            assert!(!error.contains("private-"));
        }
        let mut unfamiliar = record;
        unfamiliar["thread"]["turns"][0]["status"] = "private-provider-status".into();
        unfamiliar["thread"]["turns"][0]["itemsView"] = "private-provider-view".into();
        let error = observe_attempt(
            &unfamiliar,
            "private-thread",
            "private-attempt",
            "private-prompt",
            Some("private-turn"),
            workspace,
        )
        .unwrap_err();
        assert!(error.contains("knownTurnStatus=other, knownTurnItemsView=other"));
        assert!(!error.contains("private-"));
    }

    #[tokio::test]
    async fn rpc_rejection_names_only_an_allowlisted_method_and_numeric_code() {
        for (method, label) in [
            ("turn/interrupt", "turn/interrupt"),
            ("thread/read", "thread/read"),
            ("private-method", "other"),
        ] {
            let (client, mut server) = duplex(4096);
            server.write_all(b"{\"id\":1,\"error\":{\"code\":-32600,\"message\":\"private-provider-message\",\"data\":{\"secret\":\"private-data\"}}}\n").await.unwrap();
            let (read, write) = split(client);
            let mut rpc = Transport::new(BufReader::new(read), write);
            let error = rpc
                .request(
                    method,
                    json!({"private-input":"private-value"}),
                    Duration::from_secs(1),
                )
                .await
                .unwrap_err();
            assert_eq!(
                error,
                format!("Codex RPC rejected request (method {label}, code -32600)")
            );
            assert!(!error.contains("private-"));
            assert!(rpc.broken);
        }
    }

    #[tokio::test]
    async fn early_completion_is_correlated_and_approval_requests_are_denied() {
        let (client, server) = duplex(8192);
        let (read, write) = split(client);
        let mut rpc = Transport::new(BufReader::new(read), write);
        let server = tokio::spawn(async move {
            let (read, mut write) = split(server);
            let mut read = BufReader::new(read);
            let mut line = String::new();
            read.read_line(&mut line).await.unwrap();
            write.write_all(b"{\"method\":\"turn/completed\",\"params\":{\"threadId\":\"thread\",\"turn\":{\"id\":\"turn\",\"status\":\"completed\"}}}\n{\"id\":99,\"method\":\"item/fileChange/requestApproval\",\"params\":{}}\n").await.unwrap();
            line.clear();
            read.read_line(&mut line).await.unwrap();
            let denied: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(denied["result"]["decision"], "decline");
            write
                .write_all(b"{\"id\":1,\"result\":{\"turn\":{\"id\":\"turn\"}}}\n")
                .await
                .unwrap();
        });
        rpc.request("turn/start", json!({}), Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(
            rpc.terminal("thread", "turn", Duration::from_secs(1))
                .await
                .unwrap()
                .status,
            "completed"
        );
        assert_eq!(rpc.denied_requests, 1);
        server.await.unwrap();
    }
    #[tokio::test]
    async fn invalid_frames_and_wrong_reply_ids_poison_transport_without_exposing_body() {
        for bytes in [
            b"not json secret\n".as_slice(),
            b"{\"id\":2,\"result\":{}}\n",
            b"{\"id\":1,\"error\":{\"code\":401,\"message\":\"secret\"}}\n",
        ] {
            let (client, mut server) = duplex(4096);
            server.write_all(bytes).await.unwrap();
            let (read, write) = split(client);
            let mut rpc = Transport::new(BufReader::new(read), write);
            let error = rpc
                .request("test", json!({}), Duration::from_secs(1))
                .await
                .unwrap_err();
            assert!(!error.contains("secret"));
            assert!(rpc.broken);
            assert!(rpc
                .request("test", json!({}), Duration::from_secs(1))
                .await
                .unwrap_err()
                .contains("reconnect"));
        }
    }
    #[tokio::test]
    async fn timeout_is_unknown_and_no_request_is_resent() {
        let (client, server) = duplex(4096);
        let (read, write) = split(client);
        let mut rpc = Transport::new(BufReader::new(read), write);
        assert!(rpc
            .request("turn/start", json!({}), Duration::from_millis(20))
            .await
            .unwrap_err()
            .contains("unknown"));
        assert!(rpc
            .request("turn/start", json!({}), Duration::from_millis(20))
            .await
            .is_err());
        let mut reader = BufReader::new(server);
        let mut line = String::new();
        reader.read_line(&mut line).await.unwrap();
        assert_eq!(serde_json::from_str::<Value>(&line).unwrap()["id"], 1);
        assert!(timeout_at(
            Instant::now() + Duration::from_millis(20),
            reader.read_line(&mut String::new())
        )
        .await
        .is_err());
    }
    #[tokio::test]
    async fn oversized_frames_and_blocked_writes_are_bounded() {
        let (client, mut server) = duplex(8192);
        let (read, write) = split(client);
        let mut rpc = Transport::new(BufReader::new(read), write);
        let writer = tokio::spawn(async move {
            let _ = server.write_all(&vec![b'x'; FRAME_LIMIT + 1]).await;
        });
        assert!(rpc
            .request("test", json!({}), Duration::from_secs(2))
            .await
            .unwrap_err()
            .contains("exceeds"));
        writer.abort();
        let (client, _server) = duplex(1);
        let (read, write) = split(client);
        let mut rpc = Transport::new(BufReader::new(read), write);
        assert!(rpc
            .request("test", json!({}), Duration::from_millis(20))
            .await
            .unwrap_err()
            .contains("send timeout"));
        assert!(rpc.broken);
    }
}
