//! One durable API request. This binds evidence; it does not grant workflow,
//! filesystem or budget permission. Reopening never authorizes another send.
use crate::host::ordinary;
use cerulean_adapters::api_generation::{
    ApiClient, ApiFailure, ApiObservation, ApiReply, PreparedRequest, RejectedUsage,
    RequestFingerprint,
};
use cerulean_domain::execution::{digest, identifier, require, sha256, timestamp};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    future::Future,
    io::{Read, Write},
    path::{Path, PathBuf},
};
mod gateway;
mod scoped;
pub use scoped::{ApiFeeAck, ApiScopedBinding, ApiScopedResponse, PendingScopedReply};
const MAX_RECORD: usize = 3 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CallIdentity {
    pub project_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub revision_digest: String,
    pub request_id: String,
    pub round: u32,
    pub fingerprint: RequestFingerprint,
}
impl CallIdentity {
    pub(crate) fn validate(&self) -> Result<(), String> {
        for id in [
            &self.project_id,
            &self.task_id,
            &self.attempt_id,
            &self.request_id,
        ] {
            identifier(id)?;
        }
        sha256(&self.revision_digest)?;
        sha256(&self.fingerprint.body_sha256)?;
        sha256(&self.fingerprint.tool_catalog_sha256)?;
        self.fingerprint.target.url()?;
        let model = &self.fingerprint.requested_model;
        require(
            !model.is_empty()
                && model.len() <= 200
                && model
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b)),
            "API journal model invalid",
        )?;
        require(
            (1..=128).contains(&self.round)
                && (1..=16384).contains(&self.fingerprint.max_output_tokens)
                && (1..=600).contains(&self.fingerprint.timeout_seconds),
            "API journal limits invalid",
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum CallOutcome {
    Reply { observation: ApiObservation },
    Failure { failure: ApiFailure },
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CallState {
    pub intent_recorded: bool,
    pub outcome: Option<CallOutcome>,
    pub rejected_usage: Option<RejectedUsage>,
    pub scoped_request: Option<ApiScopedBinding>,
    pub scoped_response: Option<Box<ApiScopedResponse>>,
    pub response_head: Option<String>,
    pub fee_ack: Option<ApiFeeAck>,
}
impl CallState {
    pub fn remote_outcome_unknown(&self) -> bool {
        self.intent_recorded
            && !matches!(
                &self.outcome,
                Some(CallOutcome::Reply { .. })
                    | Some(CallOutcome::Failure {
                        failure: ApiFailure::NotSent {},
                    })
            )
    }
    fn advance(&mut self, expected: &CallIdentity, event: &Event) -> Result<(), String> {
        match event {
            Event::ScopedIntent { .. } | Event::ScopedOutcome { .. } | Event::FeeAck { .. } => {
                scoped::advance(self, expected, event)?
            }
            Event::Intent { identity } => {
                require(
                    !self.intent_recorded && identity == expected,
                    "API intent repeated or scope changed",
                )?;
                self.intent_recorded = true;
            }
            Event::Outcome {
                outcome,
                rejected_usage,
            } => {
                require(
                    self.intent_recorded && self.outcome.is_none() && self.scoped_request.is_none(),
                    "API outcome order invalid",
                )?;
                match outcome {
                    CallOutcome::Reply { observation } => observation.validate()?,
                    CallOutcome::Failure {
                        failure: ApiFailure::Http { status },
                    } => require(
                        (100..=599).contains(status) && !(200..=299).contains(status),
                        "API failure HTTP status invalid",
                    )?,
                    _ => {}
                }
                if let Some(usage) = rejected_usage {
                    require(
                        matches!(
                            outcome,
                            CallOutcome::Failure {
                                failure: ApiFailure::InvalidResponse {}
                                    | ApiFailure::ResponseRejected { .. }
                            }
                        ),
                        "usage requires a rejected response",
                    )?;
                    usage.validate(&expected.fingerprint)?;
                }
                self.rejected_usage = rejected_usage.clone();
                self.outcome = Some(outcome.clone());
            }
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Event {
    ScopedIntent {
        identity: CallIdentity,
        binding: Box<ApiScopedBinding>,
    },
    ScopedOutcome {
        outcome: Box<scoped::ScopedCallOutcome>,
    },
    FeeAck {
        ack: ApiFeeAck,
    },
    Intent {
        identity: CallIdentity,
    },
    Outcome {
        outcome: CallOutcome,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "non_null_rejected_usage"
        )]
        rejected_usage: Option<RejectedUsage>,
    },
}
fn non_null_rejected_usage<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<RejectedUsage>, D::Error> {
    RejectedUsage::deserialize(deserializer).map(Some)
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Body {
    version: u32,
    sequence: usize,
    previous: String,
    identity_digest: String,
    at: String,
    event: Event,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    body: Body,
    hash: String,
}
pub struct ApiCallJournal {
    root: PathBuf,
    _lock: File,
    expected: CallIdentity,
    identity_digest: String,
    intent_head: Option<String>,
    head: String,
    count: usize,
    state: CallState,
    poisoned: bool,
    read_only: bool,
}
impl ApiCallJournal {
    pub fn open(root: &Path, expected: &CallIdentity) -> Result<Self, String> {
        Self::load(root, Some(expected), true, false)
    }
    /// Recovery never creates missing evidence or grants another send.
    pub fn open_existing(root: &Path, expected: &CallIdentity) -> Result<Self, String> {
        Self::load(root, Some(expected), false, false)
    }
    /// Discover identity from the original intent while holding its existing
    /// lock. The caller must still cross-check it against the original run.
    pub fn read_existing(root: &Path) -> Result<Self, String> {
        Self::load(root, None, false, false)
    }
    /// Original monetary preparation may exist before Intent publication.
    /// This path never creates a directory/lock and never permits sending.
    pub(crate) fn read_prepared_existing(
        root: &Path,
        expected: &CallIdentity,
    ) -> Result<Self, String> {
        scoped::require_guard(root)?;
        Self::load(root, Some(expected), false, true)
    }
    fn load(
        root: &Path,
        expected: Option<&CallIdentity>,
        create: bool,
        allow_prepared: bool,
    ) -> Result<Self, String> {
        if let Some(expected) = expected {
            expected.validate()?;
        }
        require(
            root.is_absolute()
                && !root
                    .components()
                    .any(|p| matches!(p, std::path::Component::ParentDir)),
            "API evidence requires an absolute ordinary path",
        )?;
        for parent in root.ancestors() {
            match fs::symlink_metadata(parent) {
                Ok(_) => ordinary(parent, true)?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err("API evidence path inaccessible".into()),
            }
        }
        if create {
            fs::create_dir_all(root).map_err(|_| "API evidence directory unavailable")?;
        }
        ordinary(root, true)?;
        let lock_path = root.join("writer.lock");
        match fs::symlink_metadata(&lock_path) {
            Ok(_) => ordinary(&lock_path, false)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("API evidence lock inaccessible".into()),
        }
        let lock = OpenOptions::new()
            .create(create)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(|_| "API evidence lock unavailable")?;
        lock.try_lock_exclusive()
            .map_err(|_| "API evidence already owned")?;
        require(
            lock.metadata().map_err(|e| e.to_string())?.len() == 0,
            "API evidence lock contains unexpected data",
        )?;
        let discovered;
        let expected = if let Some(expected) = expected {
            expected
        } else {
            let path = root.join("00.json");
            ordinary(&path, false)?;
            let mut bytes = Vec::new();
            File::open(path)
                .map_err(|_| "original API intent missing")?
                .take((MAX_RECORD + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|_| "original API intent unreadable")?;
            require(bytes.len() <= MAX_RECORD, "API intent too large")?;
            let record: Record =
                serde_json::from_slice(&bytes).map_err(|_| "original API intent invalid")?;
            let identity = match record.body.event {
                Event::Intent { identity } | Event::ScopedIntent { identity, .. } => identity,
                _ => return Err("original API intent missing".into()),
            };
            identity.validate()?;
            discovered = identity;
            &discovered
        };
        let mut files = fs::read_dir(root)
            .map_err(|_| "API evidence listing failed")?
            .take(6)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "API evidence listing failed")?;
        require(files.len() <= 4, "API evidence has extra records")?;
        files.sort_by_key(|e| e.file_name());
        let mut result = Self {
            root: root.into(),
            _lock: lock,
            expected: expected.clone(),
            identity_digest: digest(expected)?,
            intent_head: None,
            head: "genesis".into(),
            count: 0,
            state: CallState::default(),
            poisoned: false,
            read_only: !create,
        };
        for file in files {
            if file.file_name() == "writer.lock" {
                continue;
            }
            ordinary(&file.path(), false)?;
            require(
                result.count < 3
                    && file.file_name() == format!("{:02}.json", result.count).as_str(),
                "API evidence pending, missing or unexpected; never resend",
            )?;
            let mut bytes = Vec::new();
            File::open(file.path())
                .map_err(|_| "API evidence unreadable")?
                .take((MAX_RECORD + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|_| "API evidence unreadable")?;
            require(bytes.len() <= MAX_RECORD, "API evidence too large")?;
            let record: Record =
                serde_json::from_slice(&bytes).map_err(|_| "API evidence invalid")?;
            require(
                record.body.version == 1
                    && record.body.sequence == result.count
                    && record.body.previous == result.head
                    && record.body.identity_digest == result.identity_digest
                    && digest(&record.body)? == record.hash,
                "API evidence scope or chain mismatch",
            )?;
            timestamp(&record.body.at)?;
            scoped::require_event(root, &record.body.event)?;
            result.state.advance(expected, &record.body.event)?;
            if matches!(
                record.body.event,
                Event::Intent { .. } | Event::ScopedIntent { .. }
            ) {
                result.intent_head = Some(record.hash.clone());
            }
            if matches!(
                &record.body.event,
                Event::Outcome { .. } | Event::ScopedOutcome { .. }
            ) {
                result.state.response_head = Some(record.hash.clone());
            }
            result.head = record.hash;
            result.count += 1;
        }
        require(
            create || result.state.intent_recorded || allow_prepared,
            "original API intent missing; do not initialize during recovery",
        )?;
        Ok(result)
    }
    pub fn state(&self) -> &CallState {
        &self.state
    }
    pub fn identity(&self) -> &CallIdentity {
        &self.expected
    }
    pub fn healthy(&self) -> bool {
        !self.poisoned
    }
    pub fn head(&self) -> &str {
        &self.head
    }
    /// Host must first verify current approval, worker lease and budgets. The
    /// return value proposes tool calls; this module executes none of them.
    pub async fn execute_once(
        &mut self,
        client: &ApiClient,
        prepared: PreparedRequest,
        cancel: &mut tokio::sync::oneshot::Receiver<()>,
    ) -> Result<Result<ApiReply, ApiFailure>, String> {
        let fingerprint = prepared.fingerprint().clone();
        // Preserve the old no-send outcome without requiring an upgrade merely
        // to record a cancellation already received before any HTTP dispatch.
        if !matches!(
            cancel.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ) {
            return self
                .execute_reply(&fingerprint, || async { Err(ApiFailure::NotSent {}) })
                .await;
        }
        self.execute_attempt(
            &fingerprint,
            None,
            async |gateway: &mut cerulean_adapters::api_generation::GatewayObserver<'_>| {
                client
                    .send_measured_with_gateway(prepared, cancel, gateway)
                    .await
            },
        )
        .await
    }
    pub(crate) async fn execute_reply<F: Future<Output = Result<ApiReply, ApiFailure>>>(
        &mut self,
        fingerprint: &RequestFingerprint,
        send: impl FnOnce() -> F,
    ) -> Result<Result<ApiReply, ApiFailure>, String> {
        self.execute_with(fingerprint, || async {
            send().await.map(|reply| {
                let observation = reply.observation().clone();
                (reply, observation)
            })
        })
        .await
    }
    async fn execute_with<T, F: Future<Output = Result<(T, ApiObservation), ApiFailure>>>(
        &mut self,
        fingerprint: &RequestFingerprint,
        send: impl FnOnce() -> F,
    ) -> Result<Result<T, ApiFailure>, String> {
        require(
            !self.read_only && !self.poisoned && !self.state.intent_recorded,
            "API request already attempted or journal requires recovery; never resend",
        )?;
        require(
            fingerprint == &self.expected.fingerprint,
            "API prepared request differs from bound intent",
        )?;
        self.publish(
            &Event::Intent {
                identity: self.expected.clone(),
            },
            |_| Ok(()),
        )?;
        let result = send().await;
        let outcome = match &result {
            Ok((_, observation)) => CallOutcome::Reply {
                observation: observation.clone(),
            },
            Err(failure) => CallOutcome::Failure {
                failure: failure.clone(),
            },
        };
        // No reply/tool proposal is released until this publication succeeds.
        self.publish(
            &Event::Outcome {
                outcome,
                rejected_usage: None,
            },
            |_| Ok(()),
        )?;
        Ok(result.map(|(reply, _)| reply))
    }
    fn publish(
        &mut self,
        event: &Event,
        mut observe: impl FnMut(&str) -> Result<(), String>,
    ) -> Result<(), String> {
        require(
            !self.read_only && !self.poisoned && self.count < 3,
            "API journal unavailable",
        )?;
        scoped::require_event(&self.root, event)?;
        let mut next = self.state.clone();
        next.advance(&self.expected, event)?;
        let body = Body {
            version: 1,
            sequence: self.count,
            previous: self.head.clone(),
            identity_digest: self.identity_digest.clone(),
            at: chrono::Utc::now().to_rfc3339(),
            event: event.clone(),
        };
        let hash = digest(&body)?;
        let bytes = serde_json::to_vec(&Record {
            body,
            hash: hash.clone(),
        })
        .map_err(|_| "API evidence encoding failed")?;
        require(bytes.len() <= MAX_RECORD, "API evidence too large")?;
        self.poisoned = true;
        ordinary(&self.root, true)?;
        let pending = self.root.join(format!("{:02}.pending", self.count));
        let target = self.root.join(format!("{:02}.json", self.count));
        require(
            fs::symlink_metadata(&target).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
            "API immutable evidence already exists or inaccessible",
        )?;
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&pending)
            .map_err(|_| "API pending record unavailable")?;
        observe("created")?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "API record sync failed")?;
        drop(file);
        observe("synced")?;
        fs::hard_link(&pending, &target).map_err(|_| "API non-replacing publication failed")?;
        observe("published")?;
        fs::remove_file(&pending).map_err(|_| "API pending removal failed")?;
        observe("committed")?;
        if matches!(event, Event::Outcome { .. } | Event::ScopedOutcome { .. }) {
            next.response_head = Some(hash.clone());
        }
        if matches!(event, Event::Intent { .. } | Event::ScopedIntent { .. }) {
            self.intent_head = Some(hash.clone());
        }
        self.state = next;
        self.head = hash;
        self.count += 1;
        self.poisoned = false;
        Ok(())
    }
}
#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) fn fixture_rehash_record(value: serde_json::Value) -> Vec<u8> {
    let mut record: Record = serde_json::from_value(value).unwrap();
    record.hash = digest(&record.body).unwrap();
    serde_json::to_vec(&record).unwrap()
}

#[cfg(all(test, windows))]
pub(crate) mod measured_tests;
