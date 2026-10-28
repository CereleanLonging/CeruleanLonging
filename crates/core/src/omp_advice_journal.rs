//! One-way original OMP advice evidence. A reader can inspect but never resend.
use cerulean_adapters::omp_transport::worker::{
    advice::{AdviceEvent, SteerAdvice},
    DispatchIntent,
};
use cerulean_domain::execution::{digest, identifier, require, sha256};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Reference {
    pub version: u32,
    pub operation: String,
    pub dispatch: DispatchIntent,
    pub request_digest: String,
    pub response_digest: String,
    pub workflow_hash: String,
    pub tool_head: String,
    pub suggestion_digest: String,
    pub message_sha256: String,
    pub held_result_sha256: String,
}
impl Reference {
    pub(crate) fn validate(&self) -> Result<(), String> {
        require(self.version == 1, "Unsupported OMP advice reference")?;
        identifier(&self.operation)?;
        self.dispatch.validate_metadata()?;
        for h in [
            &self.request_digest,
            &self.response_digest,
            &self.workflow_hash,
            &self.tool_head,
            &self.suggestion_digest,
            &self.message_sha256,
            &self.held_result_sha256,
        ] {
            sha256(h)?;
        }
        Ok(())
    }
    pub(crate) fn folder(&self, attempt_root: &Path) -> Result<PathBuf, String> {
        self.validate()?;
        Ok(attempt_root.join(format!("advice-{}", self.operation)))
    }
}
pub(crate) fn message(operation: &str, suggestion: &str) -> Result<String, String> {
    identifier(operation)?;
    require(
        !suggestion.trim().is_empty()
            && suggestion.len() <= 16 * 1024
            && !suggestion.contains('\0'),
        "OMP advice text invalid",
    )?;
    Ok(format!("[cerulean-advice:{operation}]\nDeputy advice for this original approved task; permissions remain unchanged:\n{suggestion}"))
}
pub(crate) fn text_hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

#[derive(Clone, Default)]
pub(crate) struct State {
    pub revoked: bool,
    pub send_id: Option<String>,
    pub accepted: bool,
    pub queued: bool,
    pub message_id: Option<String>,
    pub released: bool,
    pub pending: bool,
}
impl State {
    fn advance(&mut self, reference: &Reference, event: &AdviceEvent) -> Result<(), String> {
        match event {
            AdviceEvent::RevokedBeforeSend => {
                require(
                    !self.revoked && self.send_id.is_none(),
                    "OMP advice cannot be withdrawn after send reservation",
                )?;
                self.revoked = true;
            }
            AdviceEvent::SendIntent {
                command_id,
                message_sha256,
            } => {
                require(
                    !self.revoked
                        && self.send_id.is_none()
                        && !command_id.is_empty()
                        && command_id.len() <= 256
                        && message_sha256 == &reference.message_sha256,
                    "OMP advice send intent repeated or changed",
                )?;
                self.send_id = Some(command_id.clone());
            }
            AdviceEvent::Accepted { command_id } => {
                require(
                    !self.accepted && self.send_id.as_ref() == Some(command_id),
                    "OMP advice ACK lacks original send",
                )?;
                self.accepted = true;
            }
            AdviceEvent::Queued { state_id } => {
                require(
                    self.accepted && !self.queued && !state_id.is_empty() && state_id.len() <= 256,
                    "OMP queue observation lacks original ACK",
                )?;
                self.queued = true;
            }
            AdviceEvent::MessageObserved { message_id } => {
                require(
                    self.accepted
                        && self.message_id.is_none()
                        && !message_id.is_empty()
                        && message_id.len() <= 256,
                    "OMP original advice message observation invalid",
                )?;
                self.message_id = Some(message_id.clone());
            }
            AdviceEvent::ToolResultReleased { result_sha256 } => {
                sha256(result_sha256)?;
                require(
                    result_sha256 == &reference.held_result_sha256,
                    "OMP advice released a different original tool result",
                )?;
                require(
                    (self.revoked || (self.accepted && (self.queued || self.message_id.is_some())))
                        && !self.released,
                    "OMP original tool release lacks advice readiness",
                )?;
                self.released = true;
            }
        }
        Ok(())
    }
    pub(crate) fn confirmed(&self) -> bool {
        !self.pending && self.released && self.message_id.is_some()
    }
    pub(crate) fn label(&self) -> &'static str {
        if self.pending {
            "adviceOutcomeUnknown"
        } else if self.revoked && self.released {
            "notSent"
        } else if self.revoked {
            "adviceOutcomeUnknown"
        } else if self.confirmed() {
            "messageObserved"
        } else if self.released {
            "toolResultReleased"
        } else if self.queued {
            "queued"
        } else if self.accepted {
            "commandAccepted"
        } else if self.send_id.is_some() {
            "adviceOutcomeUnknown"
        } else {
            "appliedLocally"
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Entry {
    sequence: usize,
    previous: String,
    reference_digest: String,
    event: AdviceEvent,
}
pub(crate) fn prepare(
    root: &Path,
    reference: Reference,
    suggestion: String,
) -> Result<SteerAdvice, String> {
    reference.validate()?;
    let message = message(&reference.operation, &suggestion)?;
    require(
        reference.message_sha256 == text_hash(&message)
            && reference.suggestion_digest == digest(&suggestion)?,
        "OMP original advice text changed",
    )?;
    let path = reference.folder(root)?;
    crate::host::ordinary(root, true)?;
    fs::create_dir(&path)
        .map_err(|_| "OMP advice reference already exists or cannot be created; never resend")?;
    crate::api_execution::publish(&path, "reference.json", &reference)?;
    let mut sequence = 0;
    let mut previous = digest(&reference)?;
    let reference_digest = previous.clone();
    let mut state = State::default();
    Ok(SteerAdvice {
        operation_id: reference.operation.clone(),
        message,
        may_send: Box::new(|| true),
        observe: Box::new(move |event| {
            require(sequence < 5, "OMP advice event capacity exceeded")?;
            let mut next = state.clone();
            next.advance(&reference, event)?;
            let entry = Entry {
                sequence: sequence + 1,
                previous: previous.clone(),
                reference_digest: reference_digest.clone(),
                event: event.clone(),
            };
            crate::api_execution::publish(&path, &format!("{:02}.json", sequence + 1), &entry)?;
            previous = digest(&entry)?;
            state = next;
            sequence += 1;
            Ok(())
        }),
    })
}
pub(crate) fn inspect(root: &Path, reference: &Reference) -> Result<Option<State>, String> {
    let path = reference.folder(root)?;
    if !path.try_exists().map_err(|e| e.to_string())? {
        return Ok(None);
    }
    crate::host::ordinary(&path, true)?;
    let entries = fs::read_dir(&path)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    require(entries.len() <= 12, "OMP advice directory capacity invalid")?;
    let mut pending = false;
    for e in &entries {
        crate::host::ordinary(&e.path(), false)?;
        let n = e.file_name().to_string_lossy().into_owned();
        let valid = n == "reference.json"
            || n == "reference.json.pending"
            || (1..=5).any(|i| n == format!("{i:02}.json") || n == format!("{i:02}.json.pending"));
        require(valid, "Unexpected OMP advice record")?;
        pending |= n.ends_with(".pending");
    }
    if !path
        .join("reference.json")
        .try_exists()
        .map_err(|e| e.to_string())?
    {
        require(
            !(1..=5).any(|i| path.join(format!("{i:02}.json")).exists()),
            "OMP published advice events lack their original reference",
        )?;
        return Ok(Some(State {
            pending: true,
            ..Default::default()
        }));
    }
    let original: Reference = load_published(&path.join("reference.json"))?;
    require(
        &original == reference,
        "OMP advice reference differs from original application",
    )?;
    let mut state = State {
        pending,
        ..Default::default()
    };
    let mut previous = digest(reference)?;
    let reference_digest = previous.clone();
    let mut gap = false;
    for i in 1..=5 {
        let name = format!("{i:02}.json");
        if !path.join(&name).try_exists().map_err(|e| e.to_string())? {
            gap = true;
            continue;
        }
        require(!gap, "OMP advice event gap")?;
        let entry: Entry = load_published(&path.join(&name))?;
        require(
            entry.sequence == i
                && entry.previous == previous
                && entry.reference_digest == reference_digest,
            "OMP advice chain changed",
        )?;
        state.advance(reference, &entry.event)?;
        previous = digest(&entry)?;
    }
    Ok(Some(state))
}

// An interrupted hard-link publication may leave both names. Read committed
// bytes without removing the pending name or promoting Unknown to confirmed.
fn load_published<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    crate::host::ordinary(path, false)?;
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    require(bytes.len() <= 1024 * 1024, "OMP advice record too large")?;
    serde_json::from_slice(&bytes).map_err(|_| "OMP advice record invalid".into())
}
