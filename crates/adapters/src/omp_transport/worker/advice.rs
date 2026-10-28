//! One native steering message on the original prompt connection. A local
//! send, a command ACK, a queue observation and a message observation are
//! deliberately different receipts; none proves that a model obeyed advice.
use super::*;

pub type AdviceBoundary = oneshot::Receiver<Result<Option<SteerAdvice>, String>>;
pub type AdviceObserver = Box<dyn FnMut(&AdviceEvent) -> Result<(), String> + Send>;

/// Constructed only by the trusted Host after its original management
/// application. Persisted bytes cannot deserialize into a live permission.
pub struct SteerAdvice {
    pub operation_id: String,
    pub message: String,
    pub observe: AdviceObserver,
    /// One-shot authorization reservation. The Host defines its atomic
    /// ordering against pause; false can never produce a steering command.
    pub may_send: Box<dyn FnOnce() -> bool + Send>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum AdviceEvent {
    RevokedBeforeSend,
    SendIntent {
        command_id: String,
        message_sha256: String,
    },
    Accepted {
        command_id: String,
    },
    Queued {
        state_id: String,
    },
    MessageObserved {
        message_id: String,
    },
    /// The original result was written and flushed locally. Not a peer ACK.
    ToolResultReleased {
        result_sha256: String,
    },
}

pub(super) struct ActiveAdvice {
    message: String,
    observe: AdviceObserver,
    pub(super) command_id: String,
    pub(super) state_id: Option<String>,
    identity: SessionIdentity,
    accepted: bool,
    state_checked: bool,
    start: Option<String>,
    ended: bool,
    observed: bool,
    released: bool,
}
pub(super) enum PreparedAdvice {
    Send(ActiveAdvice),
    Revoked(RevokedAdvice),
}
/// A rejected reservation retains only the local receipt writer. There is no
/// command body, permission callback or route back to native steering.
pub(super) struct RevokedAdvice {
    observe: AdviceObserver,
}
impl RevokedAdvice {
    pub(super) fn released(mut self, result: &Value) -> Result<(), String> {
        (self.observe)(&AdviceEvent::ToolResultReleased {
            result_sha256: result_hash(result)?,
        })
    }
}
fn result_hash(result: &Value) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(result).map_err(|_| "OMP result encoding failed")?)
    ))
}
impl ActiveAdvice {
    pub(super) fn begin(
        advice: SteerAdvice,
        command_id: String,
        identity: &SessionIdentity,
    ) -> Result<PreparedAdvice, String> {
        let SteerAdvice {
            operation_id,
            message,
            mut observe,
            may_send,
        } = advice;
        identifier(&operation_id)?;
        require(
            message.starts_with(&format!("[cerulean-advice:{operation_id}]\n"))
                && message.len() <= 20 * 1024
                && !message.contains('\0'),
            "OMP advice requires bounded exact operation-marked text",
        )?;
        if !may_send() {
            observe(&AdviceEvent::RevokedBeforeSend)?;
            return Ok(PreparedAdvice::Revoked(RevokedAdvice { observe }));
        }
        observe(&AdviceEvent::SendIntent {
            command_id: command_id.clone(),
            message_sha256: format!("{:x}", Sha256::digest(message.as_bytes())),
        })?;
        Ok(PreparedAdvice::Send(Self {
            message,
            observe,
            command_id,
            state_id: None,
            identity: identity.clone(),
            accepted: false,
            state_checked: false,
            start: None,
            ended: false,
            observed: false,
            released: false,
        }))
    }
    pub(super) fn wire(&self) -> Value {
        json!({"type":"steer","id":self.command_id,"message":self.message})
    }
    pub(super) fn complete(&self) -> bool {
        self.observed && self.released
    }
    pub(super) fn accept(&mut self, frame: &Value) -> Result<(), String> {
        require(
            !self.accepted
                && frame["id"] == self.command_id
                && frame["command"] == "steer"
                && frame["success"] == true,
            "OMP advice command acknowledgement invalid; never resend",
        )?;
        (self.observe)(&AdviceEvent::Accepted {
            command_id: self.command_id.clone(),
        })?;
        self.accepted = true;
        self.commit_message()?;
        Ok(())
    }
    pub(super) fn queue(&mut self, frame: &Value) -> Result<(), String> {
        require(
            self.accepted
                && !self.state_checked
                && self.state_id.as_ref().is_some_and(|id| frame["id"] == *id)
                && frame["command"] == "get_state"
                && frame["success"] == true,
            "OMP advice queue response invalid",
        )?;
        let data = &frame["data"];
        require(
            data["sessionId"] == self.identity.session_id
                && data["model"]["provider"] == self.identity.provider
                && data["model"]["id"] == self.identity.model
                && data["isStreaming"] == true
                && data["isCompacting"] == false
                && data["isSettled"] == false,
            "OMP advice queue lost original active session",
        )?;
        let steering = data["queuedMessages"]["steering"]
            .as_array()
            .ok_or("OMP steering queue missing")?;
        let follow_up = data["queuedMessages"]["followUp"]
            .as_array()
            .ok_or("OMP follow-up queue missing")?;
        require(
            follow_up.is_empty(),
            "OMP advice observed foreign follow-up work",
        )?;
        if steering.len() == 1 && steering[0] == self.message && data["queuedMessageCount"] == 1 {
            // The queue snapshot may precede message events even when its
            // response is read after them. Both independent observations stand.
            (self.observe)(&AdviceEvent::Queued {
                state_id: self.state_id.clone().unwrap(),
            })?;
        } else {
            require(
                self.observed && steering.is_empty() && data["queuedMessageCount"] == 0,
                "OMP exact advice queue entry missing or ambiguous; never resend",
            )?;
        }
        self.state_checked = true;
        Ok(())
    }
    /// Only an exact user-message start/end pair on this sole live prompt is
    /// evidence. Tool/assistant quotes, substrings and an ACK are insufficient.
    pub(super) fn message(&mut self, frame: &Value) -> Result<(), String> {
        if !matches!(
            frame["type"].as_str(),
            Some("message_start" | "message_end")
        ) || frame["message"]["role"] != "user"
        {
            return Ok(());
        }
        let content = &frame["message"]["content"];
        let exact = content.as_array().is_some_and(|parts| {
            parts.len() == 1 && parts[0]["type"] == "text" && parts[0]["text"] == self.message
        });
        if !exact {
            return Ok(());
        }
        let id = frame["messageId"]
            .as_str()
            .ok_or("OMP advice message ID missing")?;
        require(
            !id.is_empty() && id.len() <= 256 && id.bytes().all(|b| b.is_ascii_graphic()),
            "OMP advice message ID invalid",
        )?;
        require(!self.ended, "OMP advice user message repeated")?;
        if frame["type"] == "message_start" {
            require(self.start.is_none(), "OMP advice message start repeated")?;
            self.start = Some(id.into());
        } else {
            require(
                self.start.as_deref() == Some(id),
                "OMP advice message end lacks original start",
            )?;
            self.ended = true;
            self.commit_message()?;
        }
        Ok(())
    }
    fn commit_message(&mut self) -> Result<(), String> {
        if self.accepted && self.ended && !self.observed {
            (self.observe)(&AdviceEvent::MessageObserved {
                message_id: self
                    .start
                    .clone()
                    .ok_or("OMP advice message start missing")?,
            })?;
            self.observed = true;
        }
        Ok(())
    }
    pub(super) fn released(&mut self, result: &Value) -> Result<(), String> {
        require(
            self.state_checked && !self.released,
            "OMP advice result release lacks queue observation",
        )?;
        (self.observe)(&AdviceEvent::ToolResultReleased {
            result_sha256: result_hash(result)?,
        })?;
        self.released = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
