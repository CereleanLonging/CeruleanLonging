//! Host calls on the sole owned prompt connection. No filesystem authority here.
use super::*;
use std::{collections::BTreeSet, future::Future, pin::Pin, task::Poll};
use tokio::sync::watch;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum HostRequest {
    HostToolCall {
        id: String,
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        #[serde(rename = "toolName")]
        tool_name: String,
        arguments: serde_json::Map<String, Value>,
    },
    HostToolCancel {
        id: String,
        #[serde(rename = "targetId")]
        target_id: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum HostEvent {
    Dispatch {
        id: String,
        tool_call_id: String,
        tool_name: String,
        arguments_sha256: String,
    },
    Cancel {
        id: String,
    },
    Result {
        id: String,
        result_sha256: String,
        is_error: bool,
    },
}
/// Text-only v1; the host validates/redacts output before it is returned to OMP.
pub struct HostOutput {
    pub text: String,
    pub is_error: bool,
}
pub type HostFuture = Pin<Box<dyn Future<Output = Result<HostOutput, String>> + Send>>;
/// Trusted core implementation. Validate approval and arguments in `start`.
/// `record` must durably commit or fail. Every event is bound to the exact intent.
/// A future must own its operations: dropping it must stop/reap owned processes;
/// successful resolution means all effects are quiescent. Cancellation is cooperative,
/// never evidence of rollback. An infrastructure error leaves the attempt unknown.
pub trait HostService: Send {
    fn record(&mut self, intent: &DispatchIntent, event: &HostEvent) -> Result<(), String>;
    fn start(&mut self, request: HostRequest, cancellation: watch::Receiver<bool>) -> HostFuture;
    /// Called synchronously after a durable Result. Only the original live
    /// owner can return a boundary; default services continue immediately.
    fn take_advice_boundary(
        &mut self,
        _intent: &DispatchIntent,
    ) -> Result<Option<advice::AdviceBoundary>, String> {
        Ok(None)
    }
}
struct Pending {
    id: String,
    cancel: watch::Sender<bool>,
    future: HostFuture,
}
pub(super) struct Calls<'a> {
    service: &'a mut dyn HostService,
    intent: DispatchIntent,
    names: BTreeSet<String>,
    ids: BTreeSet<String>,
    model_ids: BTreeSet<String>,
    cancelled: BTreeSet<String>,
    completed: BTreeSet<String>,
    pending: Vec<Pending>,
    stopped: bool,
    advice_boundary: Option<advice::AdviceBoundary>,
}
impl<'a> Calls<'a> {
    pub(super) fn new(
        service: &'a mut dyn HostService,
        intent: DispatchIntent,
        tools: &[host_tools::HostToolDefinition],
    ) -> Self {
        Self {
            service,
            intent,
            names: tools.iter().map(|t| t.name.clone()).collect(),
            ids: BTreeSet::new(),
            model_ids: BTreeSet::new(),
            cancelled: BTreeSet::new(),
            completed: BTreeSet::new(),
            pending: Vec::new(),
            stopped: false,
            advice_boundary: None,
        }
    }
    pub(super) fn idle(&self) -> bool {
        self.pending.is_empty()
    }
    pub(super) fn take_advice_boundary(&mut self) -> Option<advice::AdviceBoundary> {
        self.advice_boundary.take()
    }
    pub(super) fn accept(&mut self, frame: Value, allow_call: bool) -> Result<(), String> {
        require(
            serde_json::to_vec(&frame)
                .map_err(|_| "OMP tool frame invalid")?
                .len()
                <= 65536,
            "OMP tool arguments exceed limit",
        )?;
        let request: HostRequest =
            serde_json::from_value(frame).map_err(|_| "OMP host request fields invalid")?;
        match &request {
            HostRequest::HostToolCall {
                id,
                tool_call_id,
                tool_name,
                arguments,
            } => {
                opaque(id)?;
                opaque(tool_call_id)?;
                require(
                    allow_call && !self.stopped && self.names.contains(tool_name),
                    "OMP tool call not authorized or prompt ended",
                )?;
                require(
                    self.pending.len() < 4 && self.model_ids.len() < 128,
                    "OMP host tool budget exceeded",
                )?;
                require(
                    !self.ids.contains(id) && !self.model_ids.contains(tool_call_id),
                    "OMP host call repeated",
                )?;
                self.service.record(
                    &self.intent,
                    &HostEvent::Dispatch {
                        id: id.clone(),
                        tool_call_id: tool_call_id.clone(),
                        tool_name: tool_name.clone(),
                        arguments_sha256: digest(&json!(arguments))?,
                    },
                )?;
                self.ids.insert(id.clone());
                self.model_ids.insert(tool_call_id.clone());
                let (cancel, rx) = watch::channel(false);
                self.pending.push(Pending {
                    id: id.clone(),
                    cancel,
                    future: self.service.start(request, rx),
                });
            }
            HostRequest::HostToolCancel { id, target_id } => {
                opaque(id)?;
                opaque(target_id)?;
                require(
                    !self.ids.contains(id) && self.ids.len() < 256,
                    "OMP host cancellation repeated or budget exceeded",
                )?;
                require(
                    self.pending.iter().any(|p| p.id == *target_id)
                        || self.completed.contains(target_id),
                    "OMP cancellation has unknown target",
                )?;
                self.ids.insert(id.clone());
                // A result may have crossed the peer's abort frame in flight.
                self.cancel(target_id)?;
            }
        }
        Ok(())
    }
    fn cancel(&mut self, id: &str) -> Result<(), String> {
        if self.cancelled.contains(id) || self.completed.contains(id) {
            return Ok(());
        }
        self.service
            .record(&self.intent, &HostEvent::Cancel { id: id.into() })?;
        self.cancelled.insert(id.into());
        if let Some(p) = self.pending.iter().find(|p| p.id == id) {
            p.cancel.send_replace(true);
        }
        Ok(())
    }
    pub(super) fn cancel_all(&mut self) -> Result<(), String> {
        self.stopped = true;
        for id in self
            .pending
            .iter()
            .map(|p| p.id.clone())
            .collect::<Vec<_>>()
        {
            self.cancel(&id)?;
        }
        Ok(())
    }
    pub(super) async fn next(&mut self) -> Result<Value, String> {
        require(
            self.advice_boundary.is_none(),
            "OMP previous advice boundary not claimed",
        )?;
        let (index, output) = std::future::poll_fn(|cx| {
            for (i, p) in self.pending.iter_mut().enumerate() {
                if let Poll::Ready(output) = p.future.as_mut().poll(cx) {
                    return Poll::Ready((i, output));
                }
            }
            Poll::Pending
        })
        .await;
        let pending = self.pending.remove(index);
        let output = output?;
        require(
            output.text.len() <= 65536 && !output.text.contains('\0'),
            "OMP host output exceeds limit or contains NUL",
        )?;
        let result = json!({"type":"host_tool_result","id":pending.id,"result":{"content":[{"type":"text","text":output.text}]},"isError":output.is_error});
        self.service.record(
            &self.intent,
            &HostEvent::Result {
                id: pending.id.clone(),
                result_sha256: digest(&result)?,
                is_error: output.is_error,
            },
        )?;
        self.advice_boundary = self.service.take_advice_boundary(&self.intent)?;
        self.completed.insert(pending.id);
        Ok(result)
    }
}
fn opaque(id: &str) -> Result<(), String> {
    require(
        !id.is_empty()
            && id.len() <= 256
            && id.bytes().all(|b| b.is_ascii_graphic())
            && !id.starts_with("cerulean-omp-"),
        "OMP host identifier invalid",
    )
}
fn digest(value: &Value) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).map_err(|_| "OMP host record encoding failed")?)
    ))
}
#[cfg(test)]
mod tests;
