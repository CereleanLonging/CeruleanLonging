//! Bounded immutable tool-action evidence, bound to the full approved attempt.
//! Reopening permits inspection only; pending calls are never replayed.
use crate::host::ordinary;
use cerulean_adapters::omp_transport::worker::{host_calls::HostEvent, DispatchIntent};
use cerulean_domain::execution::{digest, identifier, require, sha256, timestamp};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_EVENTS: usize = 384;
const MAX_BYTES: usize = 16 * 1024;
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolObservation {
    pub calls: std::collections::BTreeMap<String, ToolCallObservation>,
}
/// Immutable evidence from the live journal owner, never a send/continuation
/// permit. No deserialization or public constructor can manufacture one.
pub struct ToolCheckpoint {
    evidence: ToolEvidence,
}
/// A portable description of a historical journal prefix. Deserialization is
/// not verification and never constructs a live ToolCheckpoint or send grant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolEvidence {
    schema_version: u32,
    intent_digest: String,
    head: String,
    event_count: usize,
    observation: ToolObservation,
}
impl ToolCheckpoint {
    pub fn evidence(&self) -> &ToolEvidence {
        &self.evidence
    }
    pub fn matches_scope<I: ToolScope>(&self, scope: &I) -> Result<(), String> {
        scope.validate_tool_scope()?;
        require(
            self.evidence.intent_digest == digest(scope)?,
            "Tool checkpoint scope changed",
        )
    }
    pub fn head(&self) -> &str {
        &self.evidence.head
    }
    pub fn observation(&self) -> &ToolObservation {
        &self.evidence.observation
    }
    pub fn supervision_incidents(
        &self,
        policy: &cerulean_domain::supervision::SupervisionPolicy,
    ) -> Result<Vec<cerulean_domain::supervision::SupervisionIncident>, String> {
        incidents(
            &self.evidence.intent_digest,
            &self.evidence.observation,
            policy,
        )
    }
}
impl ToolEvidence {
    pub fn head(&self) -> &str {
        &self.head
    }
    pub fn observation(&self) -> &ToolObservation {
        &self.observation
    }
    pub(crate) fn incidents(
        &self,
        policy: &cerulean_domain::supervision::SupervisionPolicy,
    ) -> Result<Vec<cerulean_domain::supervision::SupervisionIncident>, String> {
        incidents(&self.intent_digest, &self.observation, policy)
    }
    /// Uses the original parser, OS lock and complete chain validation. This
    /// cannot run while the execution owner holds the journal, and never writes.
    pub fn verify_original<I: ToolScope>(&self, root: &Path, scope: &I) -> Result<(), String> {
        let original = OmpToolJournal::read_evidence(root, scope, &self.head)?;
        require(
            self == &original,
            "Tool evidence differs from the original completed prefix",
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCallObservation {
    pub tool_call_id: String,
    pub dispatch_digest: String,
    pub tool_name: String,
    pub arguments_sha256: String,
    pub cancelled: bool,
    /// This seals a local result before wire transmission, not a remote ACK.
    pub result_sha256: Option<String>,
    /// Derived from the existing Result event, never inferred from its text.
    pub result_is_error: Option<bool>,
    /// One-based journal event sequence; independent of lexicographic RPC IDs.
    pub result_sequence: Option<u32>,
}
impl ToolObservation {
    pub fn unresolved(&self) -> usize {
        self.calls
            .values()
            .filter(|c| c.result_sha256.is_none())
            .count()
    }
    fn advance(&mut self, event: &HostEvent, sequence: usize) -> Result<(), String> {
        match event {
            HostEvent::Dispatch {
                id,
                tool_call_id,
                tool_name,
                arguments_sha256,
            } => {
                opaque(id)?;
                opaque(tool_call_id)?;
                identifier(tool_name)?;
                sha256(arguments_sha256)?;
                require(
                    tool_name.starts_with("cerulean_") && tool_name.len() <= 80,
                    "OMP tool namespace invalid",
                )?;
                require(
                    self.calls.len() < 128
                        && self.unresolved() < 4
                        && !self.calls.contains_key(id)
                        && !self.calls.values().any(|c| c.tool_call_id == *tool_call_id),
                    "OMP tool dispatch duplicate or over budget",
                )?;
                self.calls.insert(
                    id.clone(),
                    ToolCallObservation {
                        tool_call_id: tool_call_id.clone(),
                        dispatch_digest: digest(event)?,
                        tool_name: tool_name.clone(),
                        arguments_sha256: arguments_sha256.clone(),
                        cancelled: false,
                        result_sha256: None,
                        result_is_error: None,
                        result_sequence: None,
                    },
                );
            }
            HostEvent::Cancel { id } => {
                let call = self
                    .calls
                    .get_mut(id)
                    .ok_or("OMP cancellation lacks dispatch")?;
                require(
                    !call.cancelled && call.result_sha256.is_none(),
                    "OMP cancellation repeated or already resolved",
                )?;
                call.cancelled = true;
            }
            HostEvent::Result {
                id,
                result_sha256,
                is_error,
            } => {
                sha256(result_sha256)?;
                let call = self.calls.get_mut(id).ok_or("OMP result lacks dispatch")?;
                require(call.result_sha256.is_none(), "OMP result repeated")?;
                call.result_sha256 = Some(result_sha256.clone());
                call.result_is_error = Some(*is_error);
                call.result_sequence =
                    Some(u32::try_from(sequence + 1).map_err(|_| "Tool result sequence overflow")?);
            }
        }
        Ok(())
    }
}
fn opaque(id: &str) -> Result<(), String> {
    require(
        !id.is_empty()
            && id.len() <= 256
            && id.bytes().all(|b| b.is_ascii_graphic())
            && !id.starts_with("cerulean-omp-"),
        "OMP tool identifier invalid",
    )
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Body {
    version: u32,
    sequence: usize,
    previous: String,
    intent_digest: String,
    at: String,
    event: HostEvent,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    body: Body,
    hash: String,
}

/// Owns an exclusive OS lock for its lifetime. No method executes an RPC.
pub trait ToolScope: Serialize + Clone + PartialEq {
    fn validate_tool_scope(&self) -> Result<(), String>;
}
impl ToolScope for DispatchIntent {
    fn validate_tool_scope(&self) -> Result<(), String> {
        self.validate_metadata().map(|_| ())
    }
}

pub struct OmpToolJournal<I: ToolScope = DispatchIntent> {
    root: PathBuf,
    _lock: File,
    intent: I,
    intent_digest: String,
    head: String,
    count: usize,
    observation: ToolObservation,
    poisoned: bool,
    writable: bool,
}
impl<I: ToolScope> OmpToolJournal<I> {
    /// An execution must allocate a never-used tool directory, separate from the
    /// prompt journal. Reopened evidence can never become an execution writer.
    pub fn create(root: &Path, expected: &I) -> Result<Self, String> {
        expected.validate_tool_scope()?;
        fs::create_dir(root).map_err(|_| "OMP tool journal must be a new directory")?;
        let mut result = Self::open(root, expected)?;
        require(
            result.count == 0,
            "OMP new tool directory unexpectedly populated",
        )?;
        result.writable = true;
        Ok(result)
    }
    pub fn open(root: &Path, expected: &I) -> Result<Self, String> {
        Self::open_prefix(root, expected, None).map(|(journal, _)| journal)
    }
    pub fn read_evidence(root: &Path, expected: &I, head: &str) -> Result<ToolEvidence, String> {
        sha256(head)?;
        let (_, evidence) = Self::open_prefix(root, expected, Some(head))?;
        let evidence = evidence.ok_or("Original tool prefix missing")?;
        require(
            evidence.observation.unresolved() == 0,
            "Original tool prefix has unresolved calls",
        )?;
        Ok(evidence)
    }
    fn projection(&self) -> ToolEvidence {
        ToolEvidence {
            schema_version: 1,
            intent_digest: self.intent_digest.clone(),
            head: self.head.clone(),
            event_count: self.count,
            observation: self.observation.clone(),
        }
    }
    fn open_prefix(
        root: &Path,
        expected: &I,
        selected: Option<&str>,
    ) -> Result<(Self, Option<ToolEvidence>), String> {
        expected.validate_tool_scope()?;
        ordinary(root, true)?;
        let lock_path = root.join("writer.lock");
        if lock_path.exists() {
            ordinary(&lock_path, false)?;
        }
        let lock = OpenOptions::new()
            .create(selected.is_none())
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
            files.len() <= MAX_EVENTS + 1,
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
            observation: ToolObservation::default(),
            poisoned: false,
            writable: false,
        };
        let mut evidence = None;
        for file in files {
            if file.file_name() == "writer.lock" {
                continue;
            }
            ordinary(&file.path(), false)?;
            require(
                file.file_name() == format!("{:03}.json", result.count).as_str(),
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
            result
                .observation
                .advance(&record.body.event, record.body.sequence)?;
            result.count += 1;
            result.head = record.hash;
            if selected == Some(result.head.as_str()) {
                evidence = Some(result.projection());
            }
        }
        // Validate the whole chain, including any later suffix, before exposing
        // the selected prefix. A partial publication is never silently skipped.
        Ok((result, evidence))
    }
    pub fn observation(&self) -> &ToolObservation {
        &self.observation
    }
    pub fn head(&self) -> &str {
        &self.head
    }
    pub fn checkpoint(&self) -> Result<ToolCheckpoint, String> {
        require(
            self.writable && self.healthy() && self.observation.unresolved() == 0,
            "Only the healthy live tool owner can capture a completed checkpoint",
        )?;
        Ok(ToolCheckpoint {
            evidence: self.projection(),
        })
    }
    /// Read-only failure evidence from this original API/OMP tool owner. This
    /// grants no advice request, send, retry, cancellation or acceptance permit.
    pub fn supervision_incidents(
        &self,
        policy: &cerulean_domain::supervision::SupervisionPolicy,
    ) -> Result<Vec<cerulean_domain::supervision::SupervisionIncident>, String> {
        require(
            self.healthy() && self.observation.unresolved() == 0,
            "Tool observations are incomplete; no supervision inference",
        )?;
        incidents(&self.intent_digest, &self.observation, policy)
    }
    pub fn healthy(&self) -> bool {
        !self.poisoned
    }
    pub fn record(&mut self, intent: &I, event: &HostEvent) -> Result<(), String> {
        require(intent == &self.intent, "OMP tool journal scope changed")?;
        self.publish(event, |_| Ok(()))
    }
    fn publish(
        &mut self,
        event: &HostEvent,
        mut observe: impl FnMut(&str) -> Result<(), String>,
    ) -> Result<(), String> {
        require(
            self.writable && !self.poisoned,
            "OMP tool journal is inspection-only or requires recovery",
        )?;
        require(self.count < MAX_EVENTS, "OMP journal full")?;
        let mut next = self.observation.clone();
        next.advance(event, self.count)?;
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
        let pending = self.root.join(format!("{:03}.pending", self.count));
        let target = self.root.join(format!("{:03}.json", self.count));
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

fn incidents(
    intent_digest: &str,
    observation: &ToolObservation,
    policy: &cerulean_domain::supervision::SupervisionPolicy,
) -> Result<Vec<cerulean_domain::supervision::SupervisionIncident>, String> {
    use cerulean_domain::supervision::{repeated_failure_ranges, SupervisionIncident};
    policy.validate()?;
    let mut calls = observation.calls.values().collect::<Vec<_>>();
    require(
        calls
            .iter()
            .all(|c| c.result_sequence.is_some() && c.result_is_error.is_some()),
        "Tool result evidence is incomplete",
    )?;
    calls.sort_by_key(|c| c.result_sequence);
    let actions = calls
        .iter()
        .map(|c| {
            if c.result_is_error == Some(true) && !c.cancelled {
                Some((&c.tool_name, &c.arguments_sha256))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    repeated_failure_ranges(policy.repeated_failure_threshold, &actions)?
        .into_iter()
        .map(|(first, trigger)| {
            let evidence = digest(&(
                "tool-supervision-v1",
                intent_digest,
                policy,
                &calls[..=trigger],
            ))?;
            Ok(SupervisionIncident {
                id: format!("incident-{}", &evidence[..48]),
                first_sequence: calls[first].result_sequence.unwrap(),
                trigger_sequence: calls[trigger].result_sequence.unwrap(),
                action_digest: digest(&(
                    &calls[trigger].tool_name,
                    &calls[trigger].arguments_sha256,
                ))?,
                evidence_digest: evidence,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
