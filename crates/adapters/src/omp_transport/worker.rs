//! One prompt per owned OMP connection. Persistence callbacks must commit before
//! wire side effects. This transport does not grant filesystem or command tools.
use super::{Transport, FRAME_LIMIT};
use crate::omp_protocol::{PromptReceipt, PromptTracker, SessionIdentity};
use cerulean_domain::execution::{
    identifier, require, sha256, ControllerRequestIdentity, IdentityStrength, SessionBinding,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncBufRead, AsyncWrite},
    sync::oneshot,
    time::{sleep_until, Duration, Instant},
};
pub mod advice;
pub mod host_calls;
pub mod host_tools;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DispatchIntent {
    pub version: u32,
    pub task_id: String,
    pub attempt_id: String,
    pub revision_digest: String,
    pub fence: u32,
    pub binding: SessionBinding,
    pub provider: String,
    pub model: String,
    pub prompt_sha256: String,
}
impl DispatchIntent {
    pub fn validate_metadata(&self) -> Result<(SessionIdentity, String), String> {
        identifier(&self.task_id)?;
        identifier(&self.attempt_id)?;
        sha256(&self.revision_digest)?;
        sha256(&self.prompt_sha256)?;
        identifier(&self.provider)?;
        let IdentityStrength::ControllerRequest(ControllerRequestIdentity {
            connection_id,
            request_id,
        }) = &self.binding.identity_strength
        else {
            return Err("OMP requires controller request correlation".into());
        };
        identifier(connection_id)?;
        identifier(request_id)?;
        require(
            !request_id.starts_with("cerulean-omp-"),
            "OMP prompt ID overlaps control namespace",
        )?;
        require(
            self.version == 1
                && self.fence > 0
                && self.binding.adapter_id == "omp"
                && !self.binding.adapter_version.trim().is_empty()
                && self.binding.adapter_version.len() <= 128
                && !self.binding.adapter_version.chars().any(char::is_control)
                && self.binding.workspace.len() <= 4096
                && !self.binding.workspace.chars().any(char::is_control)
                && std::path::Path::new(&self.binding.workspace).is_absolute(),
            "OMP dispatch binding incomplete",
        )?;
        let identity = SessionIdentity {
            session_id: self.binding.session_id.clone(),
            provider: self.provider.clone(),
            model: self.model.clone(),
        };
        PromptTracker::new(identity.clone(), request_id.clone())?;
        Ok((identity, request_id.clone()))
    }
    fn validate(&self, prompt: &str) -> Result<(SessionIdentity, String), String> {
        let identity = self.validate_metadata()?;
        require(
            !prompt.trim().is_empty()
                && prompt.len() <= FRAME_LIMIT / 4
                && !prompt.trim_start().starts_with('/')
                && !prompt.contains('\0'),
            "OMP prompt must be bounded task text, not a slash command",
        )?;
        require(
            format!("{:x}", Sha256::digest(prompt.as_bytes())) == self.prompt_sha256,
            "OMP prompt differs from dispatch intent",
        )?;
        Ok(identity)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum WorkerEvent {
    DispatchIntent {
        intent: DispatchIntent,
    },
    PromptAcknowledged {
        request_id: String,
    },
    NotStarted {
        request_id: String,
    },
    AbortIntent {
        request_id: String,
        abort_id: String,
    },
    AbortAcknowledged {
        abort_id: String,
    },
    Terminal {
        receipt: PromptReceipt,
        abort_requested: bool,
        abort_acknowledged: bool,
    },
}
#[derive(Debug)]
pub enum PromptOutcome {
    NotStarted,
    Finished(PromptReceipt),
}

pub struct WorkerTransport<R, W> {
    rpc: Transport<R, W>,
    consumed: bool,
    host_tools: Option<Vec<host_tools::HostToolDefinition>>,
}
impl<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin> WorkerTransport<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            rpc: Transport::new(reader, writer),
            consumed: false,
            host_tools: None,
        }
    }
    pub async fn connect(&mut self, timeout: Duration) -> Result<Value, String> {
        self.rpc.connect(timeout).await?;
        self.rpc.get_state(timeout).await
    }
    pub async fn close_input(&mut self) -> Result<(), String> {
        self.rpc.close_input().await
    }
    pub fn command_attempts(&self) -> &std::collections::BTreeMap<String, u64> {
        self.rpc.command_attempts()
    }

    /// `persist` is a trusted local journal callback, not a model/tool callback.
    /// The core must validate approval, budget, scope and freshness before calling.
    /// An error after a dispatch intent is unknown; this object cannot resend.
    pub async fn execute(
        &mut self,
        intent: DispatchIntent,
        prompt: &str,
        timeout: Duration,
        cancellation: &mut oneshot::Receiver<()>,
        persist: impl FnMut(&WorkerEvent) -> Result<(), String>,
    ) -> Result<PromptOutcome, String> {
        self.execute_inner(intent, prompt, timeout, cancellation, persist, None)
            .await
    }
    /// Explicit tool-capable entry; a trusted host must provide durable records
    /// and approved, cancellation-safe implementations for the registered catalog.
    #[allow(clippy::too_many_arguments)]
    pub async fn execute_with_tools(
        &mut self,
        intent: DispatchIntent,
        prompt: &str,
        timeout: Duration,
        cancellation: &mut oneshot::Receiver<()>,
        persist: impl FnMut(&WorkerEvent) -> Result<(), String>,
        service: &mut dyn host_calls::HostService,
    ) -> Result<PromptOutcome, String> {
        self.execute_inner(
            intent,
            prompt,
            timeout,
            cancellation,
            persist,
            Some(service),
        )
        .await
    }
    #[allow(clippy::too_many_arguments)]
    async fn execute_inner(
        &mut self,
        intent: DispatchIntent,
        prompt: &str,
        timeout: Duration,
        cancellation: &mut oneshot::Receiver<()>,
        mut persist: impl FnMut(&WorkerEvent) -> Result<(), String>,
        service: Option<&mut dyn host_calls::HostService>,
    ) -> Result<PromptOutcome, String> {
        let (identity, request_id) = intent.validate(prompt)?;
        require(
            timeout >= Duration::from_millis(1) && timeout <= Duration::from_secs(3600),
            "OMP prompt deadline out of bounds",
        )?;
        require(
            !self.consumed && self.rpc.connected && !self.rpc.broken,
            "OMP worker connection unavailable or already consumed",
        )?;
        self.consumed = true;
        let state = self.rpc.get_state(Duration::from_secs(10)).await?;
        require(
            idle_bound(&state, &identity)?,
            "OMP worker requires a bound idle session",
        )?;
        let mut calls = if let Some(service) = service {
            let tools = self
                .host_tools
                .as_deref()
                .ok_or("OMP host tools not registered")?;
            host_tools::validate_catalog(&state, tools)?;
            Some(host_calls::Calls::new(service, intent.clone(), tools))
        } else {
            require(
                state["dumpTools"].as_array().is_some_and(Vec::is_empty),
                "OMP no-tool worker requires a bound idle session without tools",
            )?;
            None
        };
        if cancellation.try_recv().is_ok() {
            return Ok(PromptOutcome::NotStarted);
        }
        let mut tracker = PromptTracker::new(identity.clone(), request_id.clone())?;
        let wire = json!({"type":"prompt","id":request_id,"message":prompt});
        // Check encoded size before durable intent, including JSON escaping.
        require(
            serde_json::to_vec(&wire)
                .map_err(|_| "OMP prompt serialization failed")?
                .len()
                < FRAME_LIMIT,
            "OMP serialized prompt exceeds frame limit",
        )?;
        self.rpc.broken = true;
        persist(&WorkerEvent::DispatchIntent { intent })?;
        if cancellation.try_recv().is_ok() {
            persist(&WorkerEvent::NotStarted { request_id })?;
            return Ok(PromptOutcome::NotStarted);
        }
        let deadline = Instant::now() + timeout;
        *self
            .rpc
            .command_attempts
            .entry("prompt".into())
            .or_default() += 1;
        self.rpc.send(wire, deadline).await?;
        let mut abort_id: Option<String> = None;
        let mut abort_acknowledged = false;
        let mut cancel_open = true;
        let mut state_id: Option<String> = None;
        let mut next_state = Instant::now();
        let mut frames = 0u32;
        // State lives outside each select future: a readable RPC frame must
        // never drop a completed tool result or the sole Host permission.
        let mut held_result: Option<Value> = None;
        let mut advice_boundary: Option<advice::AdviceBoundary> = None;
        let mut active_advice: Option<advice::ActiveAdvice> = None;
        let mut advice_operations = std::collections::BTreeSet::new();
        loop {
            require(
                Instant::now() < deadline,
                "OMP execution deadline exceeded; result unknown",
            )?;
            if tracker.needs_state_check()
                && calls.as_ref().is_none_or(|c| c.idle())
                && held_result.is_none()
                && state_id.is_none()
                && Instant::now() >= next_state
            {
                state_id = Some(self.control("get_state", deadline).await?);
            }
            enum Wake {
                Frame(Value),
                Tool(Value),
                Cancel(Result<(), oneshot::error::RecvError>),
                Poll,
                Advice(
                    Result<Result<Option<advice::SteerAdvice>, String>, oneshot::error::RecvError>,
                ),
            }
            let wake = tokio::select! {
                biased;
                tool=async { calls.as_mut().unwrap().next().await }, if held_result.is_none() && calls.as_ref().is_some_and(|c|!c.idle()) => Wake::Tool(tool?),
                frame=self.rpc.read(deadline) => Wake::Frame(frame?),
                cancel=&mut *cancellation, if cancel_open && abort_id.is_none() && !tracker.terminal_observed() => Wake::Cancel(cancel),
                boundary=async { advice_boundary.as_mut().unwrap().await }, if advice_boundary.is_some() => Wake::Advice(boundary),
                _=sleep_until(next_state), if tracker.needs_state_check() && held_result.is_none() && calls.as_ref().is_none_or(|c|c.idle()) && state_id.is_none() => Wake::Poll,
            };
            // Cancellation which arrived with a ready permission wins before
            // its send intent. Existing readable terminal handling is retained.
            let wake = if matches!(wake, Wake::Advice(_)) && cancellation.try_recv().is_ok() {
                Wake::Cancel(Ok(()))
            } else {
                wake
            };
            let value = match wake {
                Wake::Poll => continue,
                Wake::Tool(result) => {
                    if let Some(boundary) = calls.as_mut().unwrap().take_advice_boundary() {
                        if abort_id.is_none() && !tracker.terminal_observed() {
                            require(
                                active_advice.as_ref().is_none_or(|a| a.complete()),
                                "OMP new advice boundary precedes original advice observation",
                            )?;
                            active_advice = None;
                            held_result = Some(result);
                            advice_boundary = Some(boundary);
                            continue;
                        }
                        // A cancelled parallel call still resolves its owned
                        // effects. Drop a late advice permission and let its
                        // unchanged result participate in original cleanup.
                        drop(boundary);
                    }
                    *self
                        .rpc
                        .command_attempts
                        .entry("host_tool_result".into())
                        .or_default() += 1;
                    self.rpc.send(result, deadline).await?;
                    continue;
                }
                Wake::Advice(reply) => {
                    advice_boundary = None;
                    require(
                        abort_id.is_none() && !tracker.terminal_observed() && held_result.is_some(),
                        "OMP advice boundary expired; never resend",
                    )?;
                    if let Some(advice) =
                        reply.map_err(|_| "OMP original advice owner closed; never resend")??
                    {
                        require(
                            advice_operations.len() < 2
                                && !advice_operations.contains(&advice.operation_id),
                            "OMP advice budget or unique operation violated",
                        )?;
                        advice_operations.insert(advice.operation_id.clone());
                        let prepared = advice::ActiveAdvice::begin(
                            advice,
                            self.next_control_id()?,
                            &identity,
                        )?;
                        match prepared {
                            advice::PreparedAdvice::Send(advice) => {
                                *self.rpc.command_attempts.entry("steer".into()).or_default() += 1;
                                self.rpc.send(advice.wire(), deadline).await?;
                                active_advice = Some(advice);
                            }
                            advice::PreparedAdvice::Revoked(advice) => {
                                let result =
                                    held_result.take().ok_or("OMP held tool result missing")?;
                                *self
                                    .rpc
                                    .command_attempts
                                    .entry("host_tool_result".into())
                                    .or_default() += 1;
                                self.rpc.send(result.clone(), deadline).await?;
                                advice.released(&result)?;
                            }
                        }
                    } else {
                        let result = held_result.take().ok_or("OMP held tool result missing")?;
                        *self
                            .rpc
                            .command_attempts
                            .entry("host_tool_result".into())
                            .or_default() += 1;
                        self.rpc.send(result, deadline).await?;
                    }
                    continue;
                }
                Wake::Cancel(Err(_)) => {
                    cancel_open = false;
                    continue;
                }
                Wake::Cancel(Ok(())) => {
                    advice_boundary = None;
                    held_result = None;
                    let id = self.next_control_id()?;
                    persist(&WorkerEvent::AbortIntent {
                        request_id: request_id.clone(),
                        abort_id: id.clone(),
                    })?;
                    if let Some(calls) = &mut calls {
                        calls.cancel_all()?;
                    }
                    *self.rpc.command_attempts.entry("abort".into()).or_default() += 1;
                    self.rpc
                        .send(json!({"id":id,"type":"abort"}), deadline)
                        .await?;
                    abort_id = Some(id);
                    continue;
                }
                Wake::Frame(value) => value,
            };
            frames = frames
                .checked_add(1)
                .ok_or("OMP observation count overflow")?;
            require(frames <= 100_000, "OMP observation frame count exceeded")?;
            match value["type"].as_str() {
                Some("response")
                    if active_advice
                        .as_ref()
                        .is_some_and(|a| value["id"] == a.command_id) =>
                {
                    let advice = active_advice.as_mut().unwrap();
                    advice.accept(&value)?;
                    if abort_id.is_none() && !tracker.terminal_observed() {
                        let id = self.control("get_state", deadline).await?;
                        advice.state_id = Some(id);
                    }
                }
                Some("response")
                    if active_advice.as_ref().is_some_and(|a| {
                        a.state_id.as_ref().is_some_and(|id| value["id"] == *id)
                    }) =>
                {
                    if abort_id.is_some() || tracker.terminal_observed() {
                        require(
                            value["command"] == "get_state",
                            "OMP obsolete advice response command differs",
                        )?;
                        continue;
                    }
                    let advice = active_advice.as_mut().unwrap();
                    advice.queue(&value)?;
                    // Avoid releasing after an already-requested cancellation.
                    if cancellation.try_recv().is_ok() {
                        let id = self.next_control_id()?;
                        persist(&WorkerEvent::AbortIntent {
                            request_id: request_id.clone(),
                            abort_id: id.clone(),
                        })?;
                        held_result = None;
                        if let Some(calls) = &mut calls {
                            calls.cancel_all()?;
                        }
                        *self.rpc.command_attempts.entry("abort".into()).or_default() += 1;
                        self.rpc
                            .send(json!({"id":id,"type":"abort"}), deadline)
                            .await?;
                        abort_id = Some(id);
                    } else {
                        let result = held_result
                            .take()
                            .ok_or("OMP original held result missing")?;
                        *self
                            .rpc
                            .command_attempts
                            .entry("host_tool_result".into())
                            .or_default() += 1;
                        self.rpc.send(result.clone(), deadline).await?;
                        active_advice.as_mut().unwrap().released(&result)?;
                    }
                }
                Some("response") if value["id"] == request_id => {
                    tracker.observe(&value)?;
                    persist(&WorkerEvent::PromptAcknowledged {
                        request_id: request_id.clone(),
                    })?;
                }
                Some("response") if abort_id.as_ref().is_some_and(|id| value["id"] == *id) => {
                    require(
                        !abort_acknowledged
                            && value["command"] == "abort"
                            && value["success"] == true,
                        "OMP abort acknowledgement invalid",
                    )?;
                    abort_acknowledged = true;
                    persist(&WorkerEvent::AbortAcknowledged {
                        abort_id: abort_id.clone().unwrap(),
                    })?;
                }
                Some("response") if state_id.as_ref().is_some_and(|id| value["id"] == *id) => {
                    require(
                        value["command"] == "get_state" && value["success"] == true,
                        "OMP final state request failed",
                    )?;
                    state_id = None;
                    if let Some(receipt) = tracker.confirm_idle(&value["data"])? {
                        persist(&WorkerEvent::Terminal {
                            receipt: receipt.clone(),
                            abort_requested: abort_id.is_some(),
                            abort_acknowledged,
                        })?;
                        return Ok(PromptOutcome::Finished(receipt));
                    }
                    next_state = Instant::now() + Duration::from_millis(50);
                }
                Some("host_tool_call" | "host_tool_cancel") => {
                    require(
                        !(value["type"] == "host_tool_cancel"
                            && held_result
                                .as_ref()
                                .is_some_and(|r| value["targetId"] == r["id"])),
                        "OMP held tool was cancelled; advice outcome unknown",
                    )?;
                    calls
                        .as_mut()
                        .ok_or("OMP unsolicited host action")?
                        .accept(value, !tracker.terminal_observed() && abort_id.is_none())?;
                }
                Some("prompt_result") => {
                    tracker.observe(&value)?;
                    if value["status"] == "completed" {
                        require(held_result.is_none() && advice_boundary.is_none(), "OMP original prompt completed while tool result was held; never resend")?;
                    }
                    advice_boundary = None;
                    held_result = None;
                    if value["status"] == "completed" {
                        require(active_advice.as_ref().is_none_or(|a| a.complete()), "OMP original prompt completed without exact advice message observation; never resend")?;
                    }
                    if value["status"] != "completed" {
                        if let Some(calls) = &mut calls {
                            calls.cancel_all()?;
                        }
                    }
                }
                Some("message_start" | "message_end") => {
                    if !tracker.terminal_observed() {
                        if let Some(advice) = &mut active_advice {
                            advice.message(&value)?;
                        }
                    }
                }
                Some(
                    "agent_start" | "turn_start" | "turn_end" | "message_update" | "agent_end"
                    | "session_settled",
                ) => {}
                // v18.4.4 rpc-types.ts documents queue_update beside the
                // get_state queue snapshot. This notification grants nothing;
                // only the correlated state response can confirm our queue.
                Some("queue_update") => {}
                Some("tool_execution_start" | "tool_execution_update" | "tool_execution_end")
                    if calls.is_some() => {}
                _ if Transport::<R, W>::display_only(&value) => {}
                _ => {
                    return Err(
                        "OMP unsupported event, host action, retry or uncorrelated response".into(),
                    )
                }
            }
            // A peer with continuously readable observations must not starve
            // cancellation. Prefer readable completion, but only for a bounded burst.
            if frames.is_multiple_of(32)
                && cancel_open
                && abort_id.is_none()
                && !tracker.terminal_observed()
            {
                match cancellation.try_recv() {
                    Ok(()) => {
                        advice_boundary = None;
                        held_result = None;
                        let id = self.next_control_id()?;
                        persist(&WorkerEvent::AbortIntent {
                            request_id: request_id.clone(),
                            abort_id: id.clone(),
                        })?;
                        if let Some(calls) = &mut calls {
                            calls.cancel_all()?;
                        }
                        *self.rpc.command_attempts.entry("abort".into()).or_default() += 1;
                        self.rpc
                            .send(json!({"id":id,"type":"abort"}), deadline)
                            .await?;
                        abort_id = Some(id);
                    }
                    Err(oneshot::error::TryRecvError::Closed) => cancel_open = false,
                    Err(oneshot::error::TryRecvError::Empty) => {}
                }
            }
        }
    }
    fn next_control_id(&mut self) -> Result<String, String> {
        let id = format!("cerulean-omp-{}", self.rpc.next_id);
        self.rpc.next_id = self
            .rpc
            .next_id
            .checked_add(1)
            .ok_or("OMP request ID exhausted")?;
        Ok(id)
    }
    async fn control(&mut self, command: &str, deadline: Instant) -> Result<String, String> {
        let id = self.next_control_id()?;
        *self.rpc.command_attempts.entry(command.into()).or_default() += 1;
        self.rpc
            .send(json!({"id":id,"type":command}), deadline)
            .await?;
        Ok(id)
    }
}

fn idle_bound(state: &Value, identity: &SessionIdentity) -> Result<bool, String> {
    require(
        state["sessionId"] == identity.session_id
            && state["model"]["provider"] == identity.provider
            && state["model"]["id"] == identity.model,
        "OMP preflight session/model changed",
    )?;
    for name in [
        "isStreaming",
        "isCompacting",
        "hasPendingAsyncWork",
        "isSettled",
    ] {
        require(
            state[name].is_boolean(),
            "OMP preflight activity evidence missing",
        )?;
    }
    require(
        state["queuedMessageCount"].as_u64().is_some(),
        "OMP preflight queue evidence missing",
    )?;
    Ok(state["isStreaming"] == false
        && state["isCompacting"] == false
        && state["hasPendingAsyncWork"] == false
        && state["isSettled"] == true
        && state["queuedMessageCount"] == 0)
}

#[cfg(test)]
mod tests;
