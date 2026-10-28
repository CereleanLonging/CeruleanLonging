//! Disposable, current-user authenticated replay accelerators. Events remain
//! authoritative. A hit proves the exact bytes previously validated by this
//! executable, at this root and with these bindings; current guards run again.
//! DPAPI is not a security boundary against code running as the same user.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::OnceLock,
};

#[cfg(all(test, windows))]
mod tests;
#[cfg(windows)]
mod windows;

pub(crate) const FILE: &str = "replay-checkpoint.dpapi";
const PERIOD: usize = 256;
const MAX_EVENTS: usize = 65_536;
const MAX_PREFIX: usize = 8 * 1024 * 1024;
const MAX_PLAIN: usize = 32 * 1024 * 1024;
const MAX_SEALED: usize = MAX_PLAIN + 64 * 1024;

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Proof {
    name: String,
    bytes: usize,
    sha256: String,
}
impl Proof {
    pub fn new(name: String, bytes: &[u8]) -> Self {
        Self {
            name,
            bytes: bytes.len(),
            sha256: format!("{:x}", Sha256::digest(bytes)),
        }
    }
}

pub(crate) struct Tracker {
    proofs: Option<Vec<Proof>>,
    bytes: usize,
    next: usize,
    pub restored: usize,
}
impl Default for Tracker {
    fn default() -> Self {
        Self {
            proofs: Some(vec![]),
            bytes: 0,
            next: PERIOD,
            restored: 0,
        }
    }
}
impl Tracker {
    pub fn record(&mut self, name: String, bytes: &[u8]) {
        self.bytes = self.bytes.saturating_add(bytes.len());
        if self.bytes > MAX_PREFIX || self.proofs.as_ref().is_some_and(|p| p.len() >= MAX_EVENTS) {
            self.proofs = None;
        }
        if let Some(proofs) = &mut self.proofs {
            proofs.push(Proof::new(name, bytes));
        }
    }
    pub fn restore(&mut self, proofs: Vec<Proof>) {
        self.bytes = proofs.iter().map(|p| p.bytes).sum();
        self.restored = proofs.len();
        self.next = proofs.len() + PERIOD;
        self.proofs = Some(proofs);
    }
    pub fn due(&mut self) -> Option<Vec<Proof>> {
        if !cfg!(windows) {
            return None;
        }
        let proofs = self.proofs.as_ref()?;
        if proofs.len() < self.next {
            return None;
        }
        self.next = proofs.len() + PERIOD;
        Some(proofs.clone())
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) enum Payload {
    Store {
        plans: Vec<cerulean_domain::Plan>,
        events: Vec<cerulean_domain::EventSummary>,
        indexed: Vec<crate::derived_index::IndexedEvent>,
        last_hash: String,
    },
    Workflow {
        state: cerulean_domain::workflow::WorkflowState,
        entries: Vec<crate::scheduler::Entry>,
    },
}
#[derive(Serialize, Deserialize)]
pub(crate) struct Publication {
    pub kind: String,
    pub bindings: String,
    pub proofs: Vec<Proof>,
    pub payload: Payload,
}
#[derive(Serialize)]
pub(crate) enum PayloadRef<'a> {
    Store {
        plans: &'a [cerulean_domain::Plan],
        events: &'a [cerulean_domain::EventSummary],
        indexed: &'a [crate::derived_index::IndexedEvent],
        last_hash: &'a str,
    },
    Workflow {
        state: &'a cerulean_domain::workflow::WorkflowState,
        entries: &'a [crate::scheduler::Entry],
    },
}
#[derive(Serialize)]
pub(crate) struct PublicationRef<'a> {
    pub kind: &'a str,
    pub bindings: &'a str,
    pub proofs: &'a [Proof],
    pub payload: PayloadRef<'a>,
}
#[derive(Serialize, Deserialize)]
struct Envelope {
    schema: u32,
    identity: String,
    publication: Publication,
}

fn identity(root: &Path) -> Result<String, String> {
    static EXECUTABLE: OnceLock<Result<String, String>> = OnceLock::new();
    let exe = EXECUTABLE
        .get_or_init(|| {
            let mut file = fs::File::open(std::env::current_exe().map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            let mut sha = Sha256::new();
            let mut buffer = [0; 65536];
            loop {
                let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                sha.update(&buffer[..n]);
            }
            Ok(format!("{:x}", sha.finalize()))
        })
        .as_ref()
        .map_err(Clone::clone)?;
    let canonical = fs::canonicalize(root).map_err(|e| e.to_string())?;
    Ok(format!(
        "CeruleanLonging/replay-checkpoint/v1/{exe}/{}",
        canonical.to_str().ok_or("checkpoint root encoding")?
    ))
}

fn ordinary(path: &Path) -> Result<(), String> {
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err("checkpoint requires an ordinary file".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err("checkpoint refuses reparse points".into());
        }
    }
    Ok(())
}
fn read(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    ordinary(path)?;
    #[cfg(windows)]
    let file = crate::backup::open(path, false, false).map_err(|e| e.to_string())?;
    #[cfg(not(windows))]
    let file = fs::File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > limit as u64 {
        return Err("checkpoint input exceeds limit".into());
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err("checkpoint input exceeds limit".into());
    }
    Ok(bytes)
}

pub(crate) fn load(
    root: &Path,
    kind: &str,
    bindings: &str,
    paths: &[PathBuf],
) -> Option<Publication> {
    load_checked(root, kind, bindings, paths).ok()
}
fn load_checked(
    root: &Path,
    kind: &str,
    bindings: &str,
    paths: &[PathBuf],
) -> Result<Publication, String> {
    let sealed = read(&root.join(FILE), MAX_SEALED)?;
    let identity = identity(root)?;
    let plain = crypt(&sealed, identity.as_bytes(), true)?;
    let envelope: Envelope = serde_json::from_slice(&plain).map_err(|e| e.to_string())?;
    let p = envelope.publication;
    if envelope.schema != 1
        || envelope.identity != identity
        || p.kind != kind
        || p.bindings != bindings
        || p.proofs.is_empty()
        || p.proofs.len() > MAX_EVENTS
        || p.proofs.len() > paths.len()
    {
        return Err("checkpoint binding or length mismatch".into());
    }
    let mut total = 0usize;
    for (proof, path) in p.proofs.iter().zip(paths) {
        total = total
            .checked_add(proof.bytes)
            .ok_or("checkpoint byte overflow")?;
        if total > MAX_PREFIX || path.file_name().and_then(|s| s.to_str()) != Some(&proof.name) {
            return Err("checkpoint prefix mismatch".into());
        }
        let bytes = read(path, proof.bytes)?;
        if bytes.len() != proof.bytes || format!("{:x}", Sha256::digest(&bytes)) != proof.sha256 {
            return Err("checkpoint event bytes changed".into());
        }
    }
    Ok(p)
}

struct Bounded(Vec<u8>);
impl Write for Bounded {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_PLAIN.saturating_sub(self.0.len()) {
            return Err(std::io::Error::other(
                "checkpoint serialization exceeds limit",
            ));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl Publication {
    #[cfg(test)]
    pub fn publish(self, root: &Path) -> Result<(), String> {
        let mut bytes = Bounded(Vec::new());
        serde_json::to_writer(&mut bytes, &self).map_err(|e| e.to_string())?;
        publish_serialized(root, &bytes.0)
    }
}
/// Only accepts the host's bounded serialized PublicationRef, never RPC bytes.
pub(crate) fn publish_serialized(root: &Path, bytes: &[u8]) -> Result<(), String> {
    let identity = identity(root)?;
    let mut plain = Bounded(Vec::new());
    plain
        .write_all(b"{\"schema\":1,\"identity\":")
        .map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut plain, &identity).map_err(|e| e.to_string())?;
    plain
        .write_all(b",\"publication\":")
        .and_then(|_| plain.write_all(bytes))
        .and_then(|_| plain.write_all(b"}"))
        .map_err(|e| e.to_string())?;
    let sealed = crypt(&plain.0, identity.as_bytes(), false)?;
    let destination = root.join(FILE);
    if destination.try_exists().map_err(|e| e.to_string())? {
        ordinary(&destination)?;
    }
    let temp = super::scheduler::temp_path(&destination);
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    let result = file
        .write_all(&sealed)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string());
    drop(file);
    let result = result.and_then(|_| super::scheduler::projection::publish(&temp, &destination));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[cfg(windows)]
use windows::crypt;
#[cfg(not(windows))]
fn crypt(_: &[u8], _: &[u8], _: bool) -> Result<Vec<u8>, String> {
    Err("authenticated replay checkpoints require Windows".into())
}
