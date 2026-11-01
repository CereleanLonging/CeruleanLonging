//! Exclusive OMP v18.4.4 JSONL transport. No credentials, tool authority or model
//! invocation are granted by the read-only connection exposed in this module.
//! Wire contract: upstream modes/rpc/rpc-frame.ts and rpc-types.ts at v18.4.4.
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use std::collections::VecDeque;
use tokio::{
    io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt},
    time::{timeout_at, Duration, Instant},
};

pub const FRAME_LIMIT: usize = 1024 * 1024;
pub const REASSEMBLED_LIMIT: usize = 64 * 1024 * 1024;
const CHUNK_LIMIT: usize = 256 * 1024;

struct Assembly {
    id: String,
    count: u64,
    length: usize,
    next: u64,
    bytes: Vec<u8>,
}
#[derive(Default)]
struct Decoder {
    pending: Option<Assembly>,
    version_two: bool,
}
impl Decoder {
    fn push(&mut self, frame: Value) -> Result<Option<Value>, String> {
        if !frame.is_object() {
            return Err("OMP frame must be an object".into());
        }
        if frame["type"] != "rpc_chunk" {
            if self.pending.is_some() {
                return Err("OMP chunk sequence interrupted".into());
            }
            return Ok(Some(frame));
        }
        if !self.version_two {
            return Err("OMP chunk before protocol negotiation".into());
        }
        let id = frame["chunkId"].as_str().ok_or("OMP chunk ID missing")?;
        let index = frame["index"].as_u64().ok_or("OMP chunk index invalid")?;
        let count = frame["count"].as_u64().ok_or("OMP chunk count invalid")?;
        let length = frame["byteLength"]
            .as_u64()
            .ok_or("OMP chunk length invalid")?;
        if id.is_empty()
            || id.len() > 128
            || !(2..=256).contains(&count)
            || index >= count
            || !(FRAME_LIMIT as u64..=REASSEMBLED_LIMIT as u64).contains(&length)
        {
            return Err("OMP chunk metadata outside bounds".into());
        }
        let data = frame["data"].as_str().ok_or("OMP chunk data invalid")?;
        if data.is_empty() || data.len() > CHUNK_LIMIT.div_ceil(3) * 4 {
            return Err("OMP encoded chunk exceeds bounds".into());
        }
        let bytes = STANDARD
            .decode(data)
            .map_err(|_| "OMP chunk base64 invalid")?;
        if bytes.len() > CHUNK_LIMIT || STANDARD.encode(&bytes) != data {
            return Err("OMP chunk data outside canonical bounds".into());
        }
        if self.pending.is_none() {
            if index != 0 {
                return Err("OMP chunk sequence did not start at zero".into());
            }
            self.pending = Some(Assembly {
                id: id.into(),
                count,
                length: length as usize,
                next: 0,
                bytes: Vec::new(),
            });
        }
        let pending = self.pending.as_mut().unwrap();
        if pending.id != id
            || pending.count != count
            || pending.length as u64 != length
            || pending.next != index
        {
            return Err("OMP chunk sequence mismatch".into());
        }
        if pending.bytes.len() + bytes.len() > pending.length {
            return Err("OMP chunks exceed declared length".into());
        }
        pending.bytes.extend_from_slice(&bytes);
        pending.next += 1;
        if pending.next != pending.count {
            return Ok(None);
        }
        let pending = self.pending.take().unwrap();
        if pending.bytes.len() != pending.length {
            return Err("OMP reassembled length mismatch".into());
        }
        // serde_json validates UTF-8, and uses its bounded default nesting depth.
        let value: Value = serde_json::from_slice(&pending.bytes)
            .map_err(|_| "OMP reassembled JSON invalid (body omitted)")?;
        if !value.is_object() || value["type"] == "rpc_chunk" {
            return Err("OMP nested chunk or non-object payload rejected".into());
        }
        Ok(Some(value))
    }
}

/// A single owner reads and writes this connection. A dropped request future
/// leaves `broken` set, so partial delivery cannot be retried on this connection.
struct Transport<R, W> {
    reader: R,
    writer: Option<W>,
    partial: Vec<u8>,
    decoder: Decoder,
    inbound: VecDeque<(Value, usize)>,
    inbound_bytes: usize,
    broken: bool,
    connected: bool,
    next_id: u64,
    command_attempts: std::collections::BTreeMap<String, u64>,
}
impl<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin> Transport<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        Self {
            reader,
            writer: Some(writer),
            partial: Vec::new(),
            decoder: Decoder::default(),
            inbound: VecDeque::new(),
            inbound_bytes: 0,
            broken: false,
            connected: false,
            next_id: 1,
            command_attempts: Default::default(),
        }
    }
    async fn read(&mut self, deadline: Instant) -> Result<Value, String> {
        if let Some((frame, bytes)) = self.inbound.pop_front() {
            self.inbound_bytes -= bytes;
            return Ok(frame);
        }
        Self::read_wire(
            &mut self.reader,
            &mut self.partial,
            &mut self.decoder,
            deadline,
        )
        .await
    }
    async fn read_wire(
        reader: &mut R,
        partial: &mut Vec<u8>,
        decoder: &mut Decoder,
        deadline: Instant,
    ) -> Result<Value, String> {
        timeout_at(deadline, async {
            loop {
                let available = reader.fill_buf().await.map_err(|_| "OMP read failed")?;
                if available.is_empty() {
                    return Err("OMP stream ended before response".into());
                }
                let newline = available.iter().position(|b| *b == b'\n');
                let n = newline.map_or(available.len(), |i| i + 1);
                if partial.len() + n > FRAME_LIMIT {
                    return Err("OMP physical frame exceeds limit".into());
                }
                partial.extend_from_slice(&available[..n]);
                reader.consume(n);
                if newline.is_none() {
                    continue;
                }
                let bytes = std::mem::take(partial);
                let frame = serde_json::from_slice(&bytes)
                    .map_err(|_| "OMP JSON invalid (body omitted)")?;
                if let Some(value) = decoder.push(frame)? {
                    return Ok(value);
                }
            }
        })
        .await
        .map_err(|_| "OMP response timeout; connection invalidated")?
    }
    async fn send(&mut self, value: Value, deadline: Instant) -> Result<(), String> {
        let mut bytes =
            serde_json::to_vec(&value).map_err(|_| "OMP request serialization failed")?;
        bytes.push(b'\n');
        if bytes.len() > FRAME_LIMIT {
            return Err("OMP request exceeds physical frame limit".into());
        }
        let writer = self.writer.as_mut().ok_or("OMP input is closed")?;
        let sending = timeout_at(deadline, async {
            writer.write_all(&bytes).await?;
            writer.flush().await
        });
        tokio::pin!(sending);
        loop {
            // Anonymous Windows pipes have finite capacity. The peer may be
            // writing observations while waiting for our tool result, so keep
            // draining its output during this one, non-retried write. Queue
            // frames in order; only the original consumer may act on them.
            tokio::select! {
                biased;
                result = &mut sending => return result
                    .map_err(|_| "OMP write timeout; delivery unknown")?
                    .map_err(|_| "OMP write failed; delivery unknown".into()),
                frame = Self::read_wire(&mut self.reader, &mut self.partial, &mut self.decoder, deadline) => {
                    let frame = frame?;
                    let size = serde_json::to_vec(&frame)
                        .map_err(|_| "OMP received frame serialization failed")?.len();
                    if self.inbound.len() >= 1024 || size > REASSEMBLED_LIMIT - self.inbound_bytes {
                        return Err("OMP buffered observations exceed limit; delivery unknown".into());
                    }
                    self.inbound_bytes += size;
                    self.inbound.push_back((frame, size));
                }
            }
        }
    }
    // With extensions disabled, OMP can still emit display-only widget and
    // available-command metadata updates. They never grant command authority.
    // Every other unsolicited frame is rejected on this no-model channel.
    fn display_only(value: &Value) -> bool {
        (value["type"] == "extension_ui_request" && value["method"] == "setWidget")
            || (value["type"] == "available_commands_update" && value["commands"].is_array())
    }
    async fn response(&mut self, mut request: Value, deadline: Instant) -> Result<Value, String> {
        let id = format!("cerulean-omp-{}", self.next_id);
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or("OMP request ID exhausted")?;
        request["id"] = json!(id);
        let command = request["type"]
            .as_str()
            .ok_or("OMP request type missing")?
            .to_owned();
        *self.command_attempts.entry(command.clone()).or_default() += 1;
        self.send(request, deadline).await?;
        for _ in 0..1024 {
            let value = self.read(deadline).await?;
            if Self::display_only(&value) {
                continue;
            }
            if value["type"] != "response" || value["id"] != id || value["command"] != command {
                return Err("OMP unexpected frame or response correlation mismatch".into());
            }
            if value["success"] != true {
                return Err("OMP command rejected (provider body omitted)".into());
            }
            return Ok(value["data"].clone());
        }
        Err("OMP unsolicited frame count exceeds limit".into())
    }
    pub async fn connect(&mut self, timeout: Duration) -> Result<(), String> {
        if self.broken || self.connected {
            return Err("OMP connection cannot be negotiated again".into());
        }
        self.broken = true;
        let deadline = Instant::now() + timeout;
        let result = async {
            let mut ready = None;
            for _ in 0..1024 {
                let value = self.read(deadline).await?;
                if Self::display_only(&value) {
                    continue;
                }
                ready = Some(value);
                break;
            }
            let ready = ready.ok_or("OMP readiness not received")?;
            if ready["type"] != "ready"
                || ready["protocolVersion"] != 1
                || !ready["supportedProtocolVersions"]
                    .as_array()
                    .is_some_and(|v| v.contains(&json!(2)))
                || ready["maxFrameBytes"] != FRAME_LIMIT
                || ready["maxReassembledFrameBytes"] != REASSEMBLED_LIMIT
            {
                return Err("OMP ready capabilities incompatible".into());
            }
            let negotiated = self
                .response(
                    json!({"type":"negotiate_protocol","protocolVersion":2}),
                    deadline,
                )
                .await?;
            if negotiated["protocolVersion"] != 2 {
                return Err("OMP protocol negotiation unconfirmed".into());
            }
            self.decoder.version_two = true;
            Ok(())
        }
        .await;
        if result.is_ok() {
            self.broken = false;
            self.connected = true;
        }
        result
    }
    pub async fn get_state(&mut self, timeout: Duration) -> Result<Value, String> {
        if self.broken || !self.connected {
            return Err("OMP connection unavailable".into());
        }
        self.broken = true;
        let result = self
            .response(json!({"type":"get_state"}), Instant::now() + timeout)
            .await;
        if result.is_ok() {
            self.broken = false;
        }
        result
    }
    pub async fn close_input(&mut self) -> Result<(), String> {
        self.broken = true;
        let Some(mut writer) = self.writer.take() else {
            return Ok(());
        };
        // Drop the OS pipe handle as well as calling AsyncWrite::shutdown:
        // ChildStdin's shutdown alone does not guarantee EOF on Windows.
        timeout_at(Instant::now() + Duration::from_secs(2), writer.shutdown())
            .await
            .map_err(|_| "OMP input close timed out")?
            .map_err(|_| "OMP input close failed".into())
    }
    pub fn command_attempts(&self) -> &std::collections::BTreeMap<String, u64> {
        &self.command_attempts
    }
}

#[cfg(test)]
mod tests;

mod process;
pub use process::{ProbeClient, ProbeState};

/// Deliberately has no conversion to a worker channel or arbitrary RPC method.
pub struct ReadOnlyTransport<R, W>(Transport<R, W>);
impl<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin> ReadOnlyTransport<R, W> {
    pub fn new(reader: R, writer: W) -> Self {
        Self(Transport::new(reader, writer))
    }
    pub async fn connect(&mut self, timeout: Duration) -> Result<(), String> {
        self.0.connect(timeout).await
    }
    pub async fn get_state(&mut self, timeout: Duration) -> Result<Value, String> {
        self.0.get_state(timeout).await
    }
    pub async fn close_input(&mut self) -> Result<(), String> {
        self.0.close_input().await
    }
    pub fn command_attempts(&self) -> &std::collections::BTreeMap<String, u64> {
        self.0.command_attempts()
    }
}

pub mod worker;
