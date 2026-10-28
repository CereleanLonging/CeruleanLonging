//! Append-only evidence for one OMP connection/attempt. Opening old evidence never
//! authorizes a send. The trusted workflow must independently validate approval.
use crate::host::ordinary;
use cerulean_adapters::{
    omp_protocol::PromptReceipt,
    omp_transport::worker::{DispatchIntent, WorkerEvent},
};
use cerulean_domain::execution::{digest, identifier, require, timestamp};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_EVENTS: usize = 5;
const MAX_BYTES: usize = 16 * 1024;
mod completion;
pub use completion::CompletionEvidence;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Observation {
    pub dispatched: bool,
    pub acknowledged: bool,
    pub abort_id: Option<String>,
    pub abort_acknowledged: bool,
    pub not_started: bool,
    pub terminal: Option<PromptReceipt>,
}
impl Observation {
    /// ACK proves acceptance, not current liveness after the owner is gone.
    pub fn unknown(&self) -> bool {
        self.dispatched && !self.not_started && self.terminal.is_none()
    }
    fn advance(&mut self, expected: &DispatchIntent, event: &WorkerEvent) -> Result<(), String> {
        let (identity, request) = expected.validate_metadata()?;
        require(
            !self.not_started && self.terminal.is_none(),
            "OMP journal already terminal",
        )?;
        if let WorkerEvent::DispatchIntent { intent } = event {
            require(
                !self.dispatched && intent == expected,
                "OMP dispatch intent repeated or changed",
            )?;
            self.dispatched = true;
            return Ok(());
        }
        require(self.dispatched, "OMP event lacks dispatch intent")?;
        match event {
            WorkerEvent::DispatchIntent { .. } => unreachable!(),
            WorkerEvent::PromptAcknowledged { request_id } => {
                require(
                    request_id == &request && !self.acknowledged,
                    "OMP acknowledgement identity/order mismatch",
                )?;
                self.acknowledged = true;
            }
            WorkerEvent::NotStarted { request_id } => {
                require(
                    request_id == &request && !self.acknowledged && self.abort_id.is_none(),
                    "OMP no-send proof contradicts recorded acceptance or abort",
                )?;
                self.not_started = true;
            }
            WorkerEvent::AbortIntent {
                request_id,
                abort_id,
            } => {
                identifier(abort_id)?;
                require(
                    request_id == &request
                        && self.abort_id.is_none()
                        && abort_id != &request
                        && abort_id.strip_prefix("cerulean-omp-").is_some_and(|s| {
                            !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
                        }),
                    "OMP abort identity/order mismatch",
                )?;
                self.abort_id = Some(abort_id.clone());
            }
            WorkerEvent::AbortAcknowledged { abort_id } => {
                require(
                    self.abort_id.as_ref() == Some(abort_id) && !self.abort_acknowledged,
                    "OMP abort acknowledgement identity/order mismatch",
                )?;
                self.abort_acknowledged = true;
            }
            WorkerEvent::Terminal {
                receipt,
                abort_requested,
                abort_acknowledged,
            } => {
                require(
                    self.acknowledged
                        && receipt.identity == identity
                        && receipt.request_id == request
                        && receipt.correlation_origin == "controller-rpc-request-id"
                        && *abort_requested == self.abort_id.is_some()
                        && *abort_acknowledged == self.abort_acknowledged,
                    "OMP terminal contradicts bound request or durable acknowledgements",
                )?;
                self.terminal = Some(receipt.clone());
            }
        }
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Body {
    version: u32,
    sequence: usize,
    previous: String,
    intent_digest: String,
    at: String,
    event: WorkerEvent,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    body: Body,
    hash: String,
}

/// Owns an exclusive OS lock for its lifetime. No method executes an RPC.
pub struct OmpJournal {
    root: PathBuf,
    _lock: File,
    intent: DispatchIntent,
    intent_digest: String,
    head: String,
    count: usize,
    observation: Observation,
    poisoned: bool,
    completion: Option<CompletionEvidence>,
}
impl OmpJournal {
    pub fn open(root: &Path, expected: &DispatchIntent) -> Result<Self, String> {
        expected.validate_metadata()?;
        if !root.exists() {
            fs::create_dir_all(root).map_err(|_| "OMP evidence directory creation failed")?;
        }
        ordinary(root, true)?;
        let lock_path = root.join("writer.lock");
        if lock_path.exists() {
            ordinary(&lock_path, false)?;
        }
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(|_| "OMP journal lock unavailable")?;
        lock.try_lock_exclusive()
            .map_err(|_| "OMP journal already has an owner")?;
        let mut files = fs::read_dir(root)
            .map_err(|_| "OMP journal listing failed")?
            .take(MAX_EVENTS + 4)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "OMP journal listing failed")?;
        require(
            files.len() <= MAX_EVENTS + 2,
            "OMP journal exceeds event limit",
        )?;
        files.sort_by_key(|e| e.file_name());
        let mut result = Self {
            root: root.into(),
            _lock: lock,
            intent: expected.clone(),
            intent_digest: digest(expected)?,
            head: "genesis".into(),
            count: 0,
            observation: Observation::default(),
            poisoned: false,
            completion: None,
        };
        for file in files {
            if file.file_name() == "writer.lock" || file.file_name() == "completion.json" {
                continue;
            }
            ordinary(&file.path(), false)?;
            require(
                file.file_name() == format!("{:02}.json", result.count).as_str(),
                "OMP journal has pending, missing or unexpected record",
            )?;
            let mut bytes = Vec::new();
            File::open(file.path())
                .map_err(|_| "OMP journal read failed")?
                .take((MAX_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|_| "OMP journal read failed")?;
            require(bytes.len() <= MAX_BYTES, "OMP journal record too large")?;
            let record: Record =
                serde_json::from_slice(&bytes).map_err(|_| "OMP journal record invalid")?;
            require(
                record.body.version == 1
                    && record.body.sequence == result.count
                    && record.body.previous == result.head
                    && record.body.intent_digest == result.intent_digest
                    && digest(&record.body)? == record.hash,
                "OMP journal chain or scope mismatch",
            )?;
            timestamp(&record.body.at)?;
            result.observation.advance(expected, &record.body.event)?;
            result.count += 1;
            result.head = record.hash;
        }
        result.load_completion()?;
        Ok(result)
    }
    pub fn observation(&self) -> &Observation {
        &self.observation
    }
    pub fn head(&self) -> &str {
        &self.head
    }
    pub fn healthy(&self) -> bool {
        !self.poisoned
    }
    /// Trusted host entry point after approval/budget checks. An existing intent
    /// is rejected before any new RPC, even on a newly supplied connection.
    pub async fn execute_once<
        R: tokio::io::AsyncBufRead + Unpin,
        W: tokio::io::AsyncWrite + Unpin,
    >(
        &mut self,
        channel: &mut cerulean_adapters::omp_transport::worker::WorkerTransport<R, W>,
        prompt: &str,
        timeout: std::time::Duration,
        cancellation: &mut tokio::sync::oneshot::Receiver<()>,
    ) -> Result<cerulean_adapters::omp_transport::worker::PromptOutcome, String> {
        require(
            self.healthy() && !self.observation.dispatched,
            "OMP attempt already dispatched or journal requires recovery; never resend",
        )?;
        channel
            .execute(
                self.intent.clone(),
                prompt,
                timeout,
                cancellation,
                |event| self.record(event),
            )
            .await
    }
    pub fn record(&mut self, event: &WorkerEvent) -> Result<(), String> {
        self.publish(event, |_| Ok(()))
    }
    fn publish(
        &mut self,
        event: &WorkerEvent,
        mut observe: impl FnMut(&str) -> Result<(), String>,
    ) -> Result<(), String> {
        require(!self.poisoned, "OMP journal requires recovery")?;
        require(self.count < MAX_EVENTS, "OMP journal full")?;
        let mut next = self.observation.clone();
        next.advance(&self.intent, event)?;
        let body = Body {
            version: 1,
            sequence: self.count,
            previous: self.head.clone(),
            intent_digest: self.intent_digest.clone(),
            at: chrono::Utc::now().to_rfc3339(),
            event: event.clone(),
        };
        let hash = digest(&body)?;
        let bytes = serde_json::to_vec(&Record {
            body,
            hash: hash.clone(),
        })
        .map_err(|_| "OMP journal encoding failed")?;
        require(bytes.len() <= MAX_BYTES, "OMP journal record too large")?;
        self.poisoned = true;
        ordinary(&self.root, true)?;
        let pending = self.root.join(format!("{:02}.pending", self.count));
        let target = self.root.join(format!("{:02}.json", self.count));
        require(
            fs::symlink_metadata(&target).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
            "OMP immutable record already exists or is inaccessible",
        )?;
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&pending)
            .map_err(|_| "OMP pending creation failed")?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "OMP evidence sync failed")?;
        drop(file);
        observe("synced")?;
        fs::hard_link(&pending, &target).map_err(|_| "OMP non-replacing publication failed")?;
        observe("published")?;
        fs::remove_file(&pending).map_err(|_| "OMP pending removal failed")?;
        observe("committed")?;
        self.observation = next;
        self.count += 1;
        self.head = hash;
        self.poisoned = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
