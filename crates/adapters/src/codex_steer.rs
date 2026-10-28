//! The caller owns durable application/delivery intent; this module owns one wire attempt.
use super::*;

/// Narrow wire input: never carries model, workspace, sandbox, or tool overrides.
#[derive(Debug, Clone)]
pub struct SteerRequest {
    pub delivery_id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub text: String,
}

struct PendingSteer<'a, R, W> {
    rpc: &'a mut Transport<R, W>,
    confirmed: bool,
}
impl<R, W> Drop for PendingSteer<'_, R, W> {
    fn drop(&mut self) {
        // An outer execution deadline may drop the future during a partial write/read.
        if !self.confirmed {
            self.rpc.broken = true;
        }
    }
}
impl<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin> Transport<R, W> {
    async fn steer(
        &mut self,
        active: Option<&(String, String)>,
        request: &SteerRequest,
        timeout: Duration,
    ) -> Result<SteerAck, String> {
        use cerulean_domain::execution::{identifier, require};
        identifier(&request.delivery_id)?;
        for value in [&request.thread_id, &request.turn_id] {
            require(
                !value.is_empty() && value.len() <= 1024 && !value.chars().any(char::is_control),
                "invalid steer scope",
            )?;
        }
        require(
            !request.text.trim().is_empty()
                && request.text.len() <= 16 * 1024
                && !request.text.contains('\0'),
            "invalid steer text",
        )?;
        require(
            active.is_some_and(|(thread, turn)| {
                thread == &request.thread_id && turn == &request.turn_id
            }),
            "Codex steer binding mismatch",
        )?;
        require(
            !self.read_only && !self.broken,
            "Codex steer channel unavailable",
        )?;
        require(
            !self
                .terminals
                .iter()
                .any(|t| t.thread_id == request.thread_id && t.turn_id == request.turn_id),
            "Codex turn already ended; do not steer",
        )?;
        require(
            self.steer_operations.len() < 128,
            "Codex steer operation history full",
        )?;
        require(
            self.steer_operations.insert(request.delivery_id.clone()),
            "Codex steer already attempted; never resend",
        )?;
        let mut pending = PendingSteer {
            rpc: self,
            confirmed: false,
        };
        let result = pending
            .rpc
            .request(
                "turn/steer",
                json!({
                    "threadId":request.thread_id,
                    "expectedTurnId":request.turn_id,
                    "clientUserMessageId":request.delivery_id,
                    "input":[{"type":"text","text":request.text,"text_elements":[]}]
                }),
                timeout,
            )
            .await?;
        require(
            result.get("turnId").and_then(Value::as_str) == Some(request.turn_id.as_str()),
            "Codex steer acknowledgement turn mismatch",
        )?;
        pending.confirmed = true;
        Ok(SteerAck {
            delivery_id: request.delivery_id.clone(),
            thread_id: request.thread_id.clone(),
            turn_id: request.turn_id.clone(),
        })
    }
}
impl CodexClient {
    /// Sole-reader waiting. The wake-up is not authorization and does not send advice.
    /// Keep one outer execution deadline across all wake-ups.
    pub async fn wait_turn_or_control(
        &mut self,
        thread: &str,
        turn: &str,
        timeout: Duration,
        cancel: &mut tokio::sync::oneshot::Receiver<String>,
        supervision: &mut tokio::sync::watch::Receiver<()>,
    ) -> Result<ControlOutcome, String> {
        if self.active.as_ref() != Some(&(thread.into(), turn.into())) {
            return Err("Codex turn binding mismatch".into());
        }
        let result = self
            .rpc
            .terminal_or_control(thread, turn, timeout, cancel, Some(supervision))
            .await?;
        if matches!(result, ControlOutcome::Finished(WaitOutcome::Terminal(_))) {
            self.active = None;
        }
        Ok(result)
    }
    /// Caller MUST have durably recorded this exact delivery before calling.
    /// This cache prevents duplicates only on this connection; fresh connections need
    /// the caller's journal. An ACK proves acceptance, never execution or task success.
    pub async fn steer_turn(&mut self, request: &SteerRequest) -> Result<SteerAck, String> {
        self.rpc
            .steer(self.active.as_ref(), request, Duration::from_secs(15))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{duplex, split};
    fn request() -> SteerRequest {
        SteerRequest {
            delivery_id: "advice-delivery-1".into(),
            thread_id: "thread".into(),
            turn_id: "turn".into(),
            text: "Inspect the failing test within the approved scope.".into(),
        }
    }
    fn active() -> Option<(String, String)> {
        Some(("thread".into(), "turn".into()))
    }
    fn terminal() -> &'static [u8] {
        b"{\"method\":\"turn/completed\",\"params\":{\"threadId\":\"thread\",\"turn\":{\"id\":\"turn\",\"status\":\"completed\"}}}\n"
    }

    #[tokio::test]
    async fn wake_preserves_partial_frame_and_steer_ack_is_not_completion() {
        let (client, mut server) = duplex(4096);
        let (read, write) = split(client);
        let mut rpc = Transport::new(BufReader::new(read), write);
        let partial = b"{\"method\":\"item/agentMessage/delta\",\"params\":";
        server.write_all(partial).await.unwrap();
        let (_cancel_sender, mut cancel) = tokio::sync::oneshot::channel();
        let (sender, mut control) = tokio::sync::watch::channel(());
        sender.send(()).unwrap();
        assert_eq!(
            rpc.terminal_or_control(
                "thread",
                "turn",
                Duration::from_secs(1),
                &mut cancel,
                Some(&mut control)
            )
            .await
            .unwrap(),
            ControlOutcome::SupervisionReady
        );
        assert_eq!(rpc.partial_frame, partial);
        assert!(!rpc.broken);
        server
            .write_all(b"{}}\n{\"id\":1,\"result\":{\"turnId\":\"turn\"}}\n")
            .await
            .unwrap();
        let ack = rpc
            .steer(active().as_ref(), &request(), Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(ack.turn_id, "turn");
        assert!(rpc.terminals.is_empty());
        assert!(rpc
            .steer(active().as_ref(), &request(), Duration::from_secs(1))
            .await
            .is_err());
        assert_eq!(rpc.request_counts.get("turn/steer"), Some(&1));
        let mut server = BufReader::new(server);
        let mut line = String::new();
        server.read_line(&mut line).await.unwrap();
        let wire: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(
            wire,
            json!({"id":1,"method":"turn/steer","params":{
            "threadId":"thread","expectedTurnId":"turn","clientUserMessageId":"advice-delivery-1",
            "input":[{"type":"text","text":request().text,"text_elements":[]}]}})
        );
        server.get_mut().write_all(terminal()).await.unwrap();
        assert_eq!(
            rpc.terminal("thread", "turn", Duration::from_secs(1))
                .await
                .unwrap()
                .status,
            "completed"
        );
    }
    #[tokio::test]
    async fn cancelled_or_readable_completion_precedes_supervision() {
        for completed in [false, true] {
            let (client, mut server) = duplex(4096);
            let (read, write) = split(client);
            let mut rpc = Transport::new(BufReader::new(read), write);
            if completed {
                server.write_all(terminal()).await.unwrap();
            }
            let (stop, mut cancel) = tokio::sync::oneshot::channel();
            stop.send("stop".into()).unwrap();
            let (sender, mut control) = tokio::sync::watch::channel(());
            sender.send(()).unwrap();
            let result = rpc
                .terminal_or_control(
                    "thread",
                    "turn",
                    Duration::from_secs(1),
                    &mut cancel,
                    Some(&mut control),
                )
                .await
                .unwrap();
            if completed {
                assert!(matches!(
                    result,
                    ControlOutcome::Finished(WaitOutcome::Terminal(_))
                ));
            } else {
                assert_eq!(
                    result,
                    ControlOutcome::Finished(WaitOutcome::CancelRequested("stop".into()))
                );
            }
            assert!(control.has_changed().unwrap());
            assert!(rpc.request_counts.is_empty());
        }
    }
    #[tokio::test]
    async fn invalid_ack_timeout_or_dropped_future_poison_connection_without_replay() {
        for mode in 0..4 {
            let (client, mut server) = duplex(4096);
            let (read, write) = split(client);
            let mut rpc = Transport::new(BufReader::new(read), write);
            if mode == 0 {
                server
                    .write_all(b"{\"id\":1,\"result\":{\"turnId\":\"wrong\"}}\n")
                    .await
                    .unwrap();
            }
            if mode == 1 {
                server.write_all(b"{\"id\":1,\"error\":{\"code\":-1,\"message\":\"private-provider-error\"}}\n").await.unwrap();
            }
            if mode == 3 {
                assert!(tokio::time::timeout(
                    Duration::from_millis(20),
                    rpc.steer(active().as_ref(), &request(), Duration::from_secs(1))
                )
                .await
                .is_err());
            } else {
                let error = rpc
                    .steer(active().as_ref(), &request(), Duration::from_millis(20))
                    .await
                    .unwrap_err();
                assert!(!error.contains("private-provider-error"));
            }
            assert!(rpc.broken);
            assert!(rpc
                .steer(active().as_ref(), &request(), Duration::from_secs(1))
                .await
                .is_err());
            assert_eq!(rpc.request_counts.get("turn/steer"), Some(&1));
        }
    }
    #[tokio::test]
    async fn wrong_scope_readonly_finished_and_invalid_inputs_never_send() {
        for mode in 0..7 {
            let (client, _server) = duplex(4096);
            let (read, write) = split(client);
            let mut rpc = Transport::new(BufReader::new(read), write);
            let mut r = request();
            let mut bound = active();
            match mode {
                0 => bound = None,
                1 => r.turn_id = "wrong".into(),
                2 => rpc.read_only = true,
                3 => rpc.terminals.push_back(Terminal {
                    thread_id: "thread".into(),
                    turn_id: "turn".into(),
                    status: "completed".into(),
                }),
                4 => r.delivery_id = "../bad".into(),
                5 => r.text = "x".repeat(16385),
                6 => r.text = "\0".into(),
                _ => unreachable!(),
            }
            assert!(rpc
                .steer(bound.as_ref(), &r, Duration::from_secs(1))
                .await
                .is_err());
            assert!(rpc.request_counts.is_empty());
            assert!(!rpc.broken);
        }
    }
    #[tokio::test]
    async fn notification_flood_and_closed_controls_do_not_starve_or_spin() {
        let (client, mut server) = duplex(64 * 1024);
        let (read, write) = split(client);
        let mut rpc = Transport::new(BufReader::new(read), write);
        for _ in 0..100 {
            server
                .write_all(b"{\"method\":\"item/agentMessage/delta\",\"params\":{}}\n")
                .await
                .unwrap();
        }
        let (stop, mut cancel) = tokio::sync::oneshot::channel();
        let (wake, mut control) = tokio::sync::watch::channel(());
        wake.send(()).unwrap();
        assert_eq!(
            rpc.terminal_or_control(
                "thread",
                "turn",
                Duration::from_secs(1),
                &mut cancel,
                Some(&mut control)
            )
            .await
            .unwrap(),
            ControlOutcome::SupervisionReady
        );
        drop(wake);
        drop(stop);
        server.write_all(terminal()).await.unwrap();
        assert!(matches!(
            rpc.terminal_or_control(
                "thread",
                "turn",
                Duration::from_secs(1),
                &mut cancel,
                Some(&mut control)
            )
            .await
            .unwrap(),
            ControlOutcome::Finished(WaitOutcome::Terminal(_))
        ));
        assert!(!rpc.broken);
    }
    #[tokio::test]
    async fn supervision_waits_until_approval_denial_write_finishes() {
        let (client, server) = duplex(16);
        let (read, write) = split(client);
        let mut rpc = Transport::new(BufReader::new(read), write);
        let (_stop, mut cancel) = tokio::sync::oneshot::channel();
        let (wake, mut control) = tokio::sync::watch::channel(());
        let (done, finished) = tokio::sync::oneshot::channel();
        let peer = tokio::spawn(async move {
            let (read, mut write) = split(server);
            write
                .write_all(
                    b"{\"id\":99,\"method\":\"item/fileChange/requestApproval\",\"params\":{}}\n",
                )
                .await
                .unwrap();
            wake.send(()).unwrap();
            let mut read = BufReader::new(read);
            let mut line = String::new();
            read.read_line(&mut line).await.unwrap();
            let reply: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(reply["id"], 99);
            assert_eq!(reply["result"]["decision"], "decline");
            finished.await.unwrap();
        });
        assert_eq!(
            rpc.terminal_or_control(
                "thread",
                "turn",
                Duration::from_secs(1),
                &mut cancel,
                Some(&mut control)
            )
            .await
            .unwrap(),
            ControlOutcome::SupervisionReady
        );
        assert_eq!(rpc.denied_requests, 1);
        assert!(!rpc.broken);
        done.send(()).unwrap();
        peer.await.unwrap();
    }
}
