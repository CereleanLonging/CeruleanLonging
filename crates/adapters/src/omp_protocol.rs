//! OMP v18.4.4 prompt completion contract. This module neither launches a process
//! nor grants tool authority. A request ID is controller correlation, not a native turn ID.
//! Required evidence: v18.4.4 rpc-types.ts. Installed 17.2.14 lacks correlated
//! completion/settlement evidence and is intentionally rejected, not downgraded.
use cerulean_domain::execution::require;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionIdentity {
    pub session_id: String,
    pub provider: String,
    pub model: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PromptStatus {
    Completed,
    Aborted,
    Error,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PromptReceipt {
    pub identity: SessionIdentity,
    pub request_id: String,
    pub correlation_origin: String,
    pub status: PromptStatus,
}
pub struct PromptTracker {
    identity: SessionIdentity,
    request_id: String,
    acknowledged: bool,
    terminal: Option<PromptStatus>,
    poisoned: bool,
    confirmed: bool,
}
impl PromptTracker {
    pub fn new(identity: SessionIdentity, request_id: String) -> Result<Self, String> {
        for id in [
            &identity.session_id,
            &identity.provider,
            &identity.model,
            &request_id,
        ] {
            require(
                !id.is_empty() && id.len() <= 512 && !id.chars().any(char::is_control),
                "invalid OMP identity",
            )?;
        }
        Ok(Self {
            identity,
            request_id,
            acknowledged: false,
            terminal: None,
            poisoned: false,
            confirmed: false,
        })
    }
    fn healthy(&self) -> Result<(), String> {
        require(
            !self.poisoned && !self.confirmed,
            "OMP prompt tracker is closed",
        )
    }
    /// Called by a sole transport reader. Raw text/thinking/error messages are not retained.
    /// Host requests and tool execution remain the transport/permission layer's responsibility.
    pub fn observe(&mut self, frame: &Value) -> Result<(), String> {
        self.healthy()?;
        let result = self.observe_inner(frame);
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
    fn observe_inner(&mut self, frame: &Value) -> Result<(), String> {
        let kind = frame
            .get("type")
            .and_then(Value::as_str)
            .ok_or("invalid OMP frame")?;
        match kind {
            "auto_retry_start" | "retry_fallback_applied" => {
                Err("OMP retry or model fallback is not authorized".into())
            }
            "response" if frame.get("id").and_then(Value::as_str) == Some(&self.request_id) => {
                require(
                    frame["command"] == "prompt" && frame["success"] == true && !self.acknowledged,
                    "OMP prompt acknowledgement rejected or repeated",
                )?;
                require(
                    frame["data"].get("agentInvoked").is_none_or(|v| *v == true),
                    "OMP prompt did not invoke the agent",
                )?;
                self.acknowledged = true;
                Ok(())
            }
            "prompt_result" => {
                require(
                    frame.get("id").and_then(Value::as_str) == Some(&self.request_id)
                        && self.terminal.is_none(),
                    "OMP prompt result has foreign, missing or repeated ID",
                )?;
                require(
                    frame.get("status").is_some() && frame.get("sessionSettled").is_some(),
                    "OMP protocol incompatible: prompt_result lacks status or sessionSettled (17.2.14 local-only result is not completion)",
                )?;
                require(
                    frame["agentInvoked"] == true && frame["sessionSettled"].is_boolean(),
                    "OMP prompt result lacks agent or settlement evidence",
                )?;
                let status = match frame["status"].as_str() {
                    Some("completed") => PromptStatus::Completed,
                    Some("aborted") => PromptStatus::Aborted,
                    Some("error") => PromptStatus::Error,
                    _ => return Err("invalid OMP prompt status".into()),
                };
                self.terminal = Some(status);
                Ok(())
            }
            // agent_end may precede background work. session_settled carries no
            // request ID. Neither may substitute for this request's prompt_result.
            _ => Ok(()),
        }
    }
    pub fn needs_state_check(&self) -> bool {
        !self.poisoned && !self.confirmed && self.acknowledged && self.terminal.is_some()
    }
    pub(crate) fn terminal_observed(&self) -> bool {
        self.terminal.is_some()
    }
    /// Only pass the response to a fresh, correlated get_state after needs_state_check.
    /// Idle is checked independently even if prompt_result claimed sessionSettled=true.
    pub fn confirm_idle(&mut self, state: &Value) -> Result<Option<PromptReceipt>, String> {
        self.healthy()?;
        let result = self.confirm_idle_inner(state);
        if result.is_err() {
            self.poisoned = true;
        }
        result
    }
    fn confirm_idle_inner(&mut self, state: &Value) -> Result<Option<PromptReceipt>, String> {
        require(
            self.needs_state_check(),
            "OMP completion lacks prompt acknowledgement or result",
        )?;
        require(
            state["sessionId"] == self.identity.session_id
                && state["model"]["provider"] == self.identity.provider
                && state["model"]["id"] == self.identity.model,
            "OMP session or model changed",
        )?;
        for field in [
            "isStreaming",
            "isCompacting",
            "hasPendingAsyncWork",
            "isSettled",
        ] {
            require(
                state[field].is_boolean(),
                "OMP state lacks explicit activity evidence",
            )?;
        }
        require(
            state["queuedMessageCount"].as_u64().is_some(),
            "OMP state lacks queue evidence",
        )?;
        if state["isStreaming"] != false
            || state["isCompacting"] != false
            || state["hasPendingAsyncWork"] != false
            || state["isSettled"] != true
            || state["queuedMessageCount"] != 0
        {
            return Ok(None);
        }
        self.confirmed = true;
        Ok(Some(PromptReceipt {
            identity: self.identity.clone(),
            request_id: self.request_id.clone(),
            correlation_origin: "controller-rpc-request-id".into(),
            status: self.terminal.unwrap(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn tracker() -> PromptTracker {
        PromptTracker::new(
            SessionIdentity {
                session_id: "session".into(),
                provider: "deepseek".into(),
                model: "fixture-model".into(),
            },
            "request".into(),
        )
        .unwrap()
    }
    fn ack() -> Value {
        json!({"type":"response","command":"prompt","id":"request","success":true})
    }
    fn terminal(status: &str) -> Value {
        json!({"type":"prompt_result","id":"request","agentInvoked":true,"status":status,"sessionSettled":false})
    }
    fn idle() -> Value {
        json!({"sessionId":"session","model":{"provider":"deepseek","id":"fixture-model"},"isStreaming":false,"isCompacting":false,"hasPendingAsyncWork":false,"isSettled":true,"queuedMessageCount":0})
    }
    #[test]
    fn acknowledgement_agent_end_and_settled_event_never_prove_prompt_completion() {
        let mut t = tracker();
        t.observe(&ack()).unwrap();
        t.observe(&json!({"type":"agent_end","isTerminal":true,"messages":[{"role":"assistant","content":[{"type":"text","text":"done"}]}]})).unwrap();
        t.observe(&json!({"type":"session_settled"})).unwrap();
        assert!(!t.needs_state_check());
        assert!(t.confirm_idle(&idle()).is_err());
    }
    #[test]
    fn result_before_ack_is_retained_and_background_work_must_settle() {
        let mut t = tracker();
        t.observe(&terminal("completed")).unwrap();
        assert!(!t.needs_state_check());
        t.observe(&ack()).unwrap();
        assert!(t.needs_state_check());
        for field in ["isStreaming", "isCompacting", "hasPendingAsyncWork"] {
            let mut busy = idle();
            busy[field] = json!(true);
            assert!(t.confirm_idle(&busy).unwrap().is_none());
        }
        let mut queued = idle();
        queued["queuedMessageCount"] = json!(1);
        assert!(t.confirm_idle(&queued).unwrap().is_none());
        let receipt = t.confirm_idle(&idle()).unwrap().unwrap();
        assert_eq!(receipt.status, PromptStatus::Completed);
        assert_eq!(receipt.correlation_origin, "controller-rpc-request-id");
        assert!(t.observe(&ack()).is_err());
    }
    #[test]
    fn foreign_duplicate_local_only_retry_and_fallback_frames_close_tracking() {
        let mut foreign = terminal("completed");
        foreign["id"] = json!("other");
        let mut missing = terminal("completed");
        missing.as_object_mut().unwrap().remove("id");
        let mut local = ack();
        local["data"] = json!({"agentInvoked":false});
        for bad in [
            foreign,
            missing,
            local,
            json!({"type":"auto_retry_start"}),
            json!({"type":"retry_fallback_applied"}),
        ] {
            let mut t = tracker();
            assert!(t.observe(&bad).is_err());
            assert!(t.observe(&ack()).is_err());
            assert!(!t.needs_state_check());
        }
        let mut t = tracker();
        t.observe(&terminal("completed")).unwrap();
        assert!(t.observe(&terminal("completed")).is_err());
        let mut t = tracker();
        t.observe(&ack()).unwrap();
        assert!(t.observe(&ack()).is_err());
    }
    #[test]
    fn abort_error_and_identity_changes_cannot_be_misreported_as_success() {
        for status in ["aborted", "error"] {
            let mut t = tracker();
            t.observe(&ack()).unwrap();
            t.observe(&terminal(status)).unwrap();
            assert_ne!(
                t.confirm_idle(&idle()).unwrap().unwrap().status,
                PromptStatus::Completed
            );
        }
        for change in ["session", "model", "provider", "missingActivity"] {
            let mut t = tracker();
            t.observe(&ack()).unwrap();
            t.observe(&terminal("completed")).unwrap();
            let mut state = idle();
            match change {
                "session" => state["sessionId"] = json!("other"),
                "model" => state["model"]["id"] = json!("other"),
                "provider" => state["model"]["provider"] = json!("other"),
                _ => {
                    state.as_object_mut().unwrap().remove("isSettled");
                }
            }
            assert!(t.confirm_idle(&state).is_err());
            assert!(t.confirm_idle(&idle()).is_err());
        }
    }
    #[test]
    fn installed_17_2_14_local_only_result_is_not_correlated_completion() {
        let mut t = tracker();
        t.observe(&ack()).unwrap();
        let error = t
            .observe(&json!({"type":"prompt_result","id":"request","agentInvoked":false}))
            .unwrap_err();
        assert!(error.contains("OMP protocol incompatible"));
        assert!(!t.needs_state_check());
        assert!(t.observe(&terminal("completed")).is_err());
        assert!(t.confirm_idle(&idle()).is_err());
    }

    #[test]
    fn installed_17_2_14_idle_snapshot_cannot_prove_async_settlement() {
        let mut t = tracker();
        t.observe(&ack()).unwrap();
        t.observe(&terminal("completed")).unwrap();
        let state = json!({
            "sessionId":"session","model":{"provider":"deepseek","id":"fixture-model"},
            "isStreaming":false,"isCompacting":false,"queuedMessageCount":0,
            "steeringMode":"all","followUpMode":"all","interruptMode":"immediate",
            "autoCompactionEnabled":false,"fastModeEnabled":false,"fastModeActive":false,
            "tokensPerSecond":null,"messageCount":2,"todoPhases":[]
        });
        assert!(t.confirm_idle(&state).is_err());
        assert!(t.confirm_idle(&idle()).is_err());
    }

    #[test]
    fn installed_17_2_14_steer_success_and_terminal_agent_end_are_not_prompt_receipts() {
        let mut t = tracker();
        t.observe(&ack()).unwrap();
        t.observe(
            &json!({"type":"response","id":"steer-request","command":"steer","success":true}),
        )
        .unwrap();
        t.observe(&json!({"type":"agent_end","isTerminal":true,"messages":[]}))
            .unwrap();
        assert!(!t.needs_state_check());
        assert!(!t.terminal_observed());
        assert!(t.confirm_idle(&idle()).is_err());
    }
}
