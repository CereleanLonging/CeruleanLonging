//! Immutable startup identities and user closure evidence, owned by the host writer.
//! Absence of a workflow event is meaningful only after host cross-ledger validation.
use crate::host::ordinary;
use cerulean_domain::execution::{
    artifact_path, digest, identifier, require, sha256, timestamp, WorkerSelection,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_BYTES: usize = 64 * 1024;
const MAX_PER_PROJECT: usize = 1000;
const PROTOCOL: &str = "codex-durable-before-send-v1";
pub(crate) const OMP_PROTOCOL: &str = "omp-durable-before-send-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StartupIntent {
    pub schema_version: u32,
    pub protocol: String,
    pub project_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub revision_digest: String,
    pub worker: WorkerSelection,
    pub management_operation: Option<String>,
    pub management_hash: Option<String>,
    pub workflow_sequence: u32,
    pub workflow_hash: Option<String>,
    pub started_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StartupClosure {
    pub reason: String,
    pub workflow_sequence: u32,
    pub workflow_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StartupRecord {
    pub intent: StartupIntent,
    pub intent_hash: String,
    pub closure: Option<StartupClosure>,
    pub head: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Evidence {
    Intent { intent: Box<StartupIntent> },
    Closure { closure: StartupClosure },
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Body {
    schema_version: u32,
    project_id: String,
    attempt_id: String,
    sequence: u8,
    previous: String,
    at: String,
    evidence: Evidence,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    body: Body,
    hash: String,
}

pub(crate) struct StartupJournal {
    pub(crate) closure_reviews: Vec<cerulean_domain::StartupClosureReview>,
    pub(crate) reservations: Vec<cerulean_domain::StartupReservation>,
    protocol: &'static str,
    root: PathBuf,
    records: BTreeMap<(String, String), StartupRecord>,
    poisoned: bool,
    pending_publications: Vec<PendingPublication>,
}
struct PendingPublication {
    pending: PathBuf,
    target: PathBuf,
    bytes: Vec<u8>,
    published: bool,
}

fn path_id(id: &str) -> Result<(), String> {
    identifier(id)?;
    // The shared identifier grammar permits Windows device names; paths do not.
    artifact_path(id)
}
fn anchor(sequence: u32, hash: Option<&str>) -> Result<(), String> {
    match (sequence, hash) {
        (0, None) => Ok(()),
        (n, Some(hash)) if n > 0 => sha256(hash),
        _ => Err("invalid startup workflow anchor".into()),
    }
}
impl StartupIntent {
    fn validate(&self) -> Result<(), String> {
        require(
            (self.schema_version == 1 && matches!(self.protocol.as_str(), PROTOCOL | OMP_PROTOCOL))
                || (self.schema_version == 2
                    && matches!(self.protocol.as_str(), PROTOCOL | OMP_PROTOCOL)
                    && self.management_operation.is_some()),
            "unsupported startup protocol",
        )?;
        path_id(&self.project_id)?;
        path_id(&self.attempt_id)?;
        identifier(&self.task_id)?;
        sha256(&self.revision_digest)?;
        self.worker.validate()?;
        require(
            (self.protocol == PROTOCOL && self.worker.provider == "openai")
                || (self.protocol == OMP_PROTOCOL && self.worker.provider == "deepseek"),
            "startup protocol and worker provider differ",
        )?;
        match (&self.management_operation, &self.management_hash) {
            (None, None) => {}
            (Some(operation), Some(hash)) => {
                identifier(operation)?;
                sha256(hash)?;
            }
            _ => return Err("startup management operation and hash must be paired".into()),
        }
        anchor(self.workflow_sequence, self.workflow_hash.as_deref())?;
        timestamp(&self.started_at)
    }
}
impl StartupClosure {
    fn validate(&self, intent: &StartupIntent) -> Result<(), String> {
        require(
            !self.reason.trim().is_empty()
                && self.reason.len() <= 8000
                && !self.reason.contains('\0'),
            "invalid startup closure reason",
        )?;
        anchor(self.workflow_sequence, self.workflow_hash.as_deref())?;
        require(
            self.workflow_sequence >= intent.workflow_sequence,
            "startup closure precedes launch anchor",
        )?;
        require(
            self.workflow_sequence != intent.workflow_sequence
                || self.workflow_hash == intent.workflow_hash,
            "startup closure changed the same workflow anchor",
        )
    }
}
fn read(path: &Path) -> Result<(Record, Vec<u8>), String> {
    ordinary(path, false)?;
    require(
        fs::metadata(path).map_err(|e| e.to_string())?.len() <= MAX_BYTES as u64,
        "startup record too large",
    )?;
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|e| e.to_string())?
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    require(bytes.len() <= MAX_BYTES, "startup record too large")?;
    let record: Record =
        serde_json::from_slice(&bytes).map_err(|_| "invalid startup record JSON")?;
    require(
        record.body.schema_version == 1 && digest(&record.body)? == record.hash,
        "startup record hash or schema mismatch",
    )?;
    timestamp(&record.body.at)?;
    Ok((record, bytes))
}
fn absent(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) => Err("startup evidence already exists".into()),
        Err(error) => Err(error.to_string()),
    }
}

pub(crate) fn review_intent_head(
    root: &Path,
    folder: &str,
    project: &str,
    attempt: &str,
) -> Result<String, String> {
    require(
        matches!(folder, "codex-startups" | "omp-startups"),
        "invalid startup folder",
    )?;
    path_id(project)?;
    path_id(attempt)?;
    let dir = root.join(folder).join(project);
    ordinary(&dir, true)?;
    let (record, _) = read(&dir.join(format!("{attempt}.0.json")))?;
    let Evidence::Intent { intent } = &record.body.evidence else {
        return Err("closure review needs original intent".into());
    };
    intent.validate()?;
    require(
        record.body.sequence == 0
            && record.body.previous == "genesis"
            && record.body.project_id == project
            && record.body.attempt_id == attempt
            && intent.project_id == project
            && intent.attempt_id == attempt
            && intent.protocol
                == if folder == "omp-startups" {
                    OMP_PROTOCOL
                } else {
                    PROTOCOL
                },
        "closure review intent identity mismatch",
    )?;
    Ok(record.hash)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PublishPoint {
    TempCreated,
    PartialWritten,
    TempSynced,
    Published,
    PendingRemoved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RecoveryPoint {
    Revalidated,
    Synced,
    FinalNamePresent,
    PendingRemoved,
    Finished,
}

impl StartupJournal {
    pub(crate) fn open(root: &Path) -> Result<Self, String> {
        Self::open_for(root, "codex-startups", PROTOCOL)
    }
    pub(crate) fn open_omp(root: &Path) -> Result<Self, String> {
        Self::open_for(root, "omp-startups", OMP_PROTOCOL)
    }
    fn open_for(root: &Path, folder: &str, protocol: &'static str) -> Result<Self, String> {
        ordinary(root, true)?;
        let root = root.join(folder);
        match fs::symlink_metadata(&root) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&root).map_err(|e| e.to_string())?
            }
            Err(error) => return Err(error.to_string()),
            Ok(_) => {}
        }
        ordinary(&root, true)?;
        let mut journal = Self {
            closure_reviews: crate::startup_closure_review::load(root.parent().unwrap())?
                .into_iter()
                .filter(|r| format!("{}-startups", r.executor) == folder)
                .collect(),
            reservations: crate::startup_review::load(root.parent().unwrap())?
                .into_iter()
                .filter(|r| format!("{}-startups", r.executor) == folder)
                .collect(),
            protocol,
            root,
            records: BTreeMap::new(),
            poisoned: false,
            pending_publications: vec![],
        };
        let mut operations = BTreeSet::new();
        for project in fs::read_dir(&journal.root).map_err(|e| e.to_string())? {
            let project = project.map_err(|e| e.to_string())?;
            ordinary(&project.path(), true)?;
            let project_id = project
                .file_name()
                .into_string()
                .map_err(|_| "invalid startup project directory")?;
            path_id(&project_id)?;
            let files = fs::read_dir(project.path())
                .map_err(|e| e.to_string())?
                .take(MAX_PER_PROJECT * 4 + 1)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            require(
                files.len() <= MAX_PER_PROJECT * 4,
                "startup project history too large",
            )?;
            let mut publications: BTreeMap<String, (Option<PathBuf>, Option<PathBuf>)> =
                BTreeMap::new();
            for file in files {
                ordinary(&file.path(), false)?;
                let name = file
                    .file_name()
                    .into_string()
                    .map_err(|_| "invalid startup filename")?;
                let relative = format!("{folder}/{project_id}/{name}");
                if journal
                    .closure_reviews
                    .iter()
                    .any(|r| r.relative_path == relative)
                {
                    continue;
                }
                if let Some(stem) = name.strip_suffix(".json") {
                    publications.entry(stem.into()).or_default().0 = Some(file.path());
                } else if let Some(stem) = name.strip_suffix(".pending") {
                    let stem = if let Some((base, hash)) = stem.rsplit_once('.') {
                        if hash.len() == 64 {
                            sha256(hash)?;
                            let attempt = base
                                .strip_suffix(".1")
                                .ok_or("invalid closure retry filename")?;
                            require(
                                journal
                                    .closure_reviews
                                    .iter()
                                    .any(|r| r.project_id == project_id && r.attempt_id == attempt),
                                "closure retry lacks prior user review",
                            )?;
                            base
                        } else {
                            stem
                        }
                    } else {
                        stem
                    };
                    require(
                        publications
                            .entry(stem.into())
                            .or_default()
                            .1
                            .replace(file.path())
                            .is_none(),
                        "conflicting startup temporary publications",
                    )?;
                } else {
                    return Err("unknown startup evidence filename".into());
                }
            }
            require(
                publications.len() <= MAX_PER_PROJECT * 2,
                "startup logical history too large",
            )?;
            let mut count = 0;
            for (stem, (published, pending)) in publications {
                if journal
                    .reservations
                    .iter()
                    .any(|r| r.project_id == project_id && stem == format!("{}.0", r.attempt_id))
                {
                    require(
                        published.is_none() && pending.is_some(),
                        "reviewed startup evidence changed",
                    )?;
                    count += 1;
                    require(count <= MAX_PER_PROJECT, "startup project history full")?;
                    continue;
                }
                let chosen = published
                    .as_ref()
                    .or(pending.as_ref())
                    .ok_or("startup publication missing")?;
                let (record, bytes) = read(chosen)?;
                if let Some(pending) = pending {
                    let pending_stem = pending
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .ok_or("invalid startup pending name")?;
                    if let Some(hash) = pending_stem.strip_prefix(&format!("{stem}.")) {
                        require(hash == record.hash, "closure retry filename hash mismatch")?;
                    }
                    let (_, pending_bytes) = read(&pending)?;
                    require(
                        bytes == pending_bytes,
                        "conflicting startup publication bytes",
                    )?;
                    journal.pending_publications.push(PendingPublication {
                        pending,
                        target: project.path().join(format!("{stem}.json")),
                        bytes,
                        published: published.is_some(),
                    });
                }
                path_id(&record.body.attempt_id)?;
                require(
                    record.body.project_id == project_id
                        && stem == format!("{}.{}", record.body.attempt_id, record.body.sequence),
                    "startup record scope or filename mismatch",
                )?;
                let key = (project_id.clone(), record.body.attempt_id.clone());
                match record.body.evidence {
                    Evidence::Intent { intent } => {
                        intent.validate()?;
                        require(
                            intent.protocol == journal.protocol,
                            "startup stored in wrong executor journal",
                        )?;
                        require(
                            record.body.sequence == 0
                                && record.body.previous == "genesis"
                                && intent.project_id == project_id
                                && intent.attempt_id == key.1,
                            "invalid startup intent link",
                        )?;
                        require(
                            !journal.records.contains_key(&key),
                            "duplicate startup attempt",
                        )?;
                        if let Some(operation) = &intent.management_operation {
                            require(
                                operations.insert(operation.clone()),
                                "management operation owns multiple startups",
                            )?;
                        }
                        count += 1;
                        require(count <= MAX_PER_PROJECT, "startup project history full")?;
                        journal.records.insert(
                            key,
                            StartupRecord {
                                intent: *intent,
                                intent_hash: record.hash.clone(),
                                closure: None,
                                head: record.hash,
                            },
                        );
                    }
                    Evidence::Closure { closure } => {
                        let previous = journal
                            .records
                            .get_mut(&key)
                            .ok_or("startup closure lacks intent")?;
                        require(
                            record.body.sequence == 1
                                && previous.closure.is_none()
                                && record.body.previous == previous.head,
                            "invalid startup closure link",
                        )?;
                        closure.validate(&previous.intent)?;
                        previous.closure = Some(closure);
                        previous.head = record.hash;
                    }
                }
            }
        }
        Ok(journal)
    }
    /// Loading is non-mutating for pending names. The host must finish all
    /// approval/workflow/management validation before completing publication.
    pub(crate) fn recover_publications(&mut self) -> Result<(), String> {
        self.recover_publications_observed(|_, _| Ok(()))
    }
    /// Trusted host/test observer only; never selected by IPC or environment.
    pub(crate) fn recover_publications_observed(
        &mut self,
        mut observe: impl FnMut(RecoveryPoint, usize) -> Result<(), String>,
    ) -> Result<(), String> {
        require(self.healthy(), "startup journal requires reopen")?;
        for item in &self.pending_publications {
            ordinary(item.pending.parent().unwrap(), true)?;
            require(
                read(&item.pending)?.1 == item.bytes,
                "startup pending changed after review",
            )?;
            if item.published {
                require(
                    read(&item.target)?.1 == item.bytes,
                    "startup target changed after review",
                )?;
            } else {
                absent(&item.target)?;
            }
        }
        if self.pending_publications.is_empty() {
            return Ok(());
        }
        self.poisoned = true;
        observe(RecoveryPoint::Revalidated, 0)?;
        for (index, item) in self.pending_publications.iter().enumerate() {
            if !item.published {
                OpenOptions::new()
                    .write(true)
                    .open(&item.pending)
                    .and_then(|f| f.sync_all())
                    .map_err(|e| e.to_string())?;
                observe(RecoveryPoint::Synced, index)?;
                fs::hard_link(&item.pending, &item.target).map_err(|e| e.to_string())?;
            }
            observe(RecoveryPoint::FinalNamePresent, index)?;
            fs::remove_file(&item.pending).map_err(|e| e.to_string())?;
            observe(RecoveryPoint::PendingRemoved, index)?;
        }
        observe(RecoveryPoint::Finished, 0)?;
        self.pending_publications.clear();
        self.poisoned = false;
        Ok(())
    }
    pub(crate) fn healthy(&self) -> bool {
        !self.poisoned
    }
    pub(crate) fn all(&self) -> Vec<&StartupRecord> {
        self.records.values().collect()
    }
    pub(crate) fn get(&self, project: &str, attempt: &str) -> Option<&StartupRecord> {
        self.records.get(&(project.into(), attempt.into()))
    }
    pub(crate) fn check_reservation(
        &self,
        project: &str,
        attempt: &str,
        owner: Option<&str>,
    ) -> Result<(), String> {
        require(self.healthy(), "startup journal requires reopen")?;
        require(
            self.pending_publications.is_empty(),
            "startup publication recovery required",
        )?;
        path_id(project)?;
        path_id(attempt)?;
        require(
            !self
                .reservations
                .iter()
                .any(|r| r.project_id == project && r.attempt_id == attempt),
            "attempt ID permanently reserved by reviewed incomplete startup",
        )?;
        if let Some(owner) = owner {
            identifier(owner)?;
        }
        if let Some(record) = self.get(project, attempt) {
            require(
                record.closure.is_none()
                    && owner.is_some()
                    && record.intent.management_operation.as_deref() == owner,
                "attempt ID is reserved by an existing or closed startup",
            )?;
        }
        Ok(())
    }
    fn check_new(&self, intent: &StartupIntent) -> Result<(), String> {
        require(self.healthy(), "startup journal requires reopen")?;
        intent.validate()?;
        require(
            !self
                .reservations
                .iter()
                .any(|r| r.project_id == intent.project_id && r.attempt_id == intent.attempt_id),
            "reviewed incomplete startup ID cannot be reused",
        )?;
        require(
            intent.protocol == self.protocol,
            "startup journal executor mismatch",
        )?;
        require(
            !self
                .records
                .contains_key(&(intent.project_id.clone(), intent.attempt_id.clone())),
            "startup attempt already recorded; never restart",
        )?;
        require(
            self.records
                .values()
                .filter(|r| r.intent.project_id == intent.project_id)
                .count()
                + self
                    .reservations
                    .iter()
                    .filter(|r| r.project_id == intent.project_id)
                    .count()
                < MAX_PER_PROJECT,
            "startup project history full",
        )?;
        if let Some(operation) = &intent.management_operation {
            require(
                !self
                    .records
                    .values()
                    .any(|r| r.intent.management_operation.as_ref() == Some(operation)),
                "management operation already owns a startup",
            )?;
        }
        Ok(())
    }
    /// Only the trusted host can supply this hook; it is never an RPC field.
    fn publish(
        &mut self,
        body: Body,
        mut observer: impl FnMut(PublishPoint) -> Result<(), String>,
    ) -> Result<String, String> {
        require(self.healthy(), "startup journal requires reopen")?;
        require(
            self.pending_publications.is_empty(),
            "startup publication recovery required",
        )?;
        let hash = digest(&body)?;
        let record = Record {
            body,
            hash: hash.clone(),
        };
        let bytes = serde_json::to_vec(&record).map_err(|e| e.to_string())?;
        require(bytes.len() <= MAX_BYTES, "startup record too large")?;
        self.poisoned = true;
        ordinary(&self.root, true)?;
        let project = &record.body.project_id;
        // Refuse case aliases before traversing an existing Windows directory.
        for entry in fs::read_dir(&self.root).map_err(|e| e.to_string())? {
            let name = entry.map_err(|e| e.to_string())?.file_name();
            if name.to_string_lossy().eq_ignore_ascii_case(project) {
                require(
                    name == project.as_str(),
                    "startup project path uses a case alias",
                )?;
            }
        }
        let folder = self.root.join(project);
        match fs::symlink_metadata(&folder) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&folder).map_err(|e| e.to_string())?
            }
            Err(error) => return Err(error.to_string()),
            Ok(_) => {}
        }
        ordinary(&folder, true)?;
        let stem = format!("{}.{}", record.body.attempt_id, record.body.sequence);
        let target = folder.join(format!("{stem}.json"));
        let reviewed_closure = record.body.sequence == 1
            && self
                .closure_reviews
                .iter()
                .any(|r| r.project_id == *project && r.attempt_id == record.body.attempt_id);
        let pending = folder.join(if reviewed_closure {
            format!("{stem}.{hash}.pending")
        } else {
            format!("{stem}.pending")
        });
        absent(&target)?;
        absent(&pending)?;
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&pending)
            .map_err(|e| e.to_string())?;
        observer(PublishPoint::TempCreated)?;
        let split = bytes.len() / 2;
        file.write_all(&bytes[..split]).map_err(|e| e.to_string())?;
        observer(PublishPoint::PartialWritten)?;
        file.write_all(&bytes[split..])
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);
        observer(PublishPoint::TempSynced)?;
        // hard_link creates the final name atomically and fails if it exists;
        // unlike rename, it never replaces someone else's immutable evidence.
        fs::hard_link(&pending, &target)
            .map_err(|e| format!("startup non-replacing publication failed: {e}"))?;
        observer(PublishPoint::Published)?;
        fs::remove_file(&pending).map_err(|e| e.to_string())?;
        observer(PublishPoint::PendingRemoved)?;
        Ok(hash)
    }
    pub(crate) fn begin(&mut self, intent: StartupIntent) -> Result<(), String> {
        self.begin_observed(intent, |_| Ok(()))
    }
    pub(crate) fn begin_observed(
        &mut self,
        intent: StartupIntent,
        observer: impl FnMut(PublishPoint) -> Result<(), String>,
    ) -> Result<(), String> {
        self.check_new(&intent)?;
        let hash = self.publish(
            Body {
                schema_version: 1,
                project_id: intent.project_id.clone(),
                attempt_id: intent.attempt_id.clone(),
                sequence: 0,
                previous: "genesis".into(),
                at: chrono::Utc::now().to_rfc3339(),
                evidence: Evidence::Intent {
                    intent: Box::new(intent.clone()),
                },
            },
            observer,
        )?;
        self.records.insert(
            (intent.project_id.clone(), intent.attempt_id.clone()),
            StartupRecord {
                intent,
                intent_hash: hash.clone(),
                closure: None,
                head: hash,
            },
        );
        self.poisoned = false;
        Ok(())
    }
    pub(crate) fn close(
        &mut self,
        project: &str,
        attempt: &str,
        expected_head: &str,
        closure: StartupClosure,
    ) -> Result<(), String> {
        self.close_observed(project, attempt, expected_head, closure, |_| Ok(()))
    }
    pub(crate) fn close_observed(
        &mut self,
        project: &str,
        attempt: &str,
        expected_head: &str,
        closure: StartupClosure,
        observer: impl FnMut(PublishPoint) -> Result<(), String>,
    ) -> Result<(), String> {
        require(self.healthy(), "startup journal requires reopen")?;
        path_id(project)?;
        path_id(attempt)?;
        sha256(expected_head)?;
        let previous = self.get(project, attempt).ok_or("startup intent missing")?;
        require(previous.closure.is_none(), "startup already closed")?;
        require(previous.head == expected_head, "startup review is stale")?;
        closure.validate(&previous.intent)?;
        let hash = self.publish(
            Body {
                schema_version: 1,
                project_id: project.into(),
                attempt_id: attempt.into(),
                sequence: 1,
                previous: expected_head.into(),
                at: chrono::Utc::now().to_rfc3339(),
                evidence: Evidence::Closure {
                    closure: closure.clone(),
                },
            },
            observer,
        )?;
        let previous = self
            .records
            .get_mut(&(project.into(), attempt.into()))
            .expect("validated startup");
        previous.closure = Some(closure);
        previous.head = hash;
        self.poisoned = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cerulean_domain::execution::ValidationFile;

    fn intent(project: &str, attempt: &str, operation: Option<&str>) -> StartupIntent {
        StartupIntent {
            schema_version: 1,
            protocol: PROTOCOL.into(),
            project_id: project.into(),
            task_id: "task".into(),
            attempt_id: attempt.into(),
            revision_digest: "a".repeat(64),
            worker: WorkerSelection {
                omp_runtime_parent: None,
                provider: "openai".into(),
                model: "fixture-model".into(),
                executable: ValidationFile {
                    path: std::env::current_exe()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned(),
                    sha256: "b".repeat(64),
                },
            },
            management_operation: operation.map(str::to_owned),
            management_hash: operation.map(|_| "c".repeat(64)),
            workflow_sequence: 2,
            workflow_hash: Some("d".repeat(64)),
            started_at: "2026-10-08T00:00:00Z".into(),
        }
    }
    fn closure() -> StartupClosure {
        StartupClosure {
            reason: "User closed local startup without retry".into(),
            workflow_sequence: 2,
            workflow_hash: Some("d".repeat(64)),
        }
    }
    fn path(root: &Path, suffix: &str) -> PathBuf {
        root.join("codex-startups/project")
            .join(format!("attempt.{suffix}"))
    }
    fn rewrite(path: &Path, change: impl FnOnce(&mut Record)) {
        let mut record = read(path).unwrap().0;
        change(&mut record);
        record.hash = digest(&record.body).unwrap();
        fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();
    }

    #[test]
    fn scoped_omp_startup_requires_management_and_keeps_legacy_records() {
        let dir = tempfile::tempdir().unwrap();
        let mut journal = StartupJournal::open_omp(dir.path()).unwrap();
        let mut legacy = intent("project", "legacy", Some("legacy-operation"));
        legacy.protocol = OMP_PROTOCOL.into();
        legacy.worker.provider = "deepseek".into();
        journal.begin(legacy.clone()).unwrap();
        let old = journal.get("project", "legacy").unwrap().clone();
        let mut scoped = legacy.clone();
        scoped.attempt_id = "scoped".into();
        scoped.management_operation = Some("scoped-operation".into());
        scoped.schema_version = 2;
        let mut invalid = scoped.clone();
        invalid.management_operation = None;
        invalid.management_hash = None;
        assert!(journal.begin(invalid).is_err());
        let mut invalid = scoped.clone();
        invalid.protocol = PROTOCOL.into();
        invalid.worker.provider = "openai".into();
        assert!(invalid.validate().is_ok());
        assert!(journal.begin(invalid).is_err());
        journal.begin(scoped.clone()).unwrap();
        drop(journal);
        let reopened = StartupJournal::open_omp(dir.path()).unwrap();
        assert_eq!(reopened.get("project", "legacy"), Some(&old));
        assert_eq!(reopened.get("project", "scoped").unwrap().intent, scoped);
        assert_eq!(reopened.all().len(), 2);
    }

    #[test]
    fn scoped_codex_startup_keeps_legacy_and_rejects_unmanaged_or_wrong_provider() {
        let dir = tempfile::tempdir().unwrap();
        let mut journal = StartupJournal::open(dir.path()).unwrap();
        let legacy = intent("project", "legacy", Some("legacy-management"));
        journal.begin(legacy).unwrap();
        let original = journal.get("project", "legacy").unwrap().clone();
        let mut scoped = intent("project", "scoped", Some("scoped-management"));
        scoped.schema_version = 2;
        let mut invalid = scoped.clone();
        invalid.management_operation = None;
        invalid.management_hash = None;
        assert!(journal.begin(invalid).is_err());
        let mut invalid = scoped.clone();
        invalid.worker.provider = "deepseek".into();
        assert!(journal.begin(invalid).is_err());
        journal.begin(scoped.clone()).unwrap();
        drop(journal);
        let reopened = StartupJournal::open(dir.path()).unwrap();
        assert_eq!(reopened.get("project", "legacy"), Some(&original));
        assert_eq!(reopened.get("project", "scoped").unwrap().intent, scoped);
    }
    #[test]
    fn startup_roundtrip_closure_reservation_and_read_only_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let mut journal = StartupJournal::open(dir.path()).unwrap();
        let original = intent("project", "attempt", Some("management"));
        journal.begin(original.clone()).unwrap();
        let record = journal.get("project", "attempt").unwrap().clone();
        assert_eq!(record.head, record.intent_hash);
        assert_eq!(record.intent, original);
        assert_eq!(journal.all().len(), 1);
        assert!(journal
            .check_reservation("project", "attempt", Some("management"))
            .is_ok());
        assert!(journal
            .check_reservation("project", "attempt", None)
            .is_err());
        assert!(journal
            .check_reservation("project", "attempt", Some("different"))
            .is_err());
        assert!(journal
            .check_reservation("different-project", "attempt", None)
            .is_ok());
        assert!(journal.begin(original).is_err());
        assert!(journal
            .close("project", "attempt", &"e".repeat(64), closure())
            .is_err());
        assert!(journal.healthy());
        journal
            .close("project", "attempt", &record.head, closure())
            .unwrap();
        assert!(journal
            .check_reservation("project", "attempt", Some("management"))
            .is_err());
        assert!(journal
            .close("project", "attempt", &record.head, closure())
            .is_err());
        let record = journal.get("project", "attempt").unwrap().clone();
        assert_ne!(record.intent_hash, record.head);
        let before = [
            fs::read(path(dir.path(), "0.json")).unwrap(),
            fs::read(path(dir.path(), "1.json")).unwrap(),
        ];
        drop(journal);
        let reopened = StartupJournal::open(dir.path()).unwrap();
        assert_eq!(reopened.get("project", "attempt"), Some(&record));
        assert_eq!(fs::read(path(dir.path(), "0.json")).unwrap(), before[0]);
        assert_eq!(fs::read(path(dir.path(), "1.json")).unwrap(), before[1]);
        assert_eq!(
            fs::read_dir(dir.path().join("codex-startups/project"))
                .unwrap()
                .count(),
            2
        );
    }

    #[test]
    fn unmanaged_ids_and_management_operations_are_never_reused() {
        let dir = tempfile::tempdir().unwrap();
        let mut journal = StartupJournal::open(dir.path()).unwrap();
        journal.begin(intent("project", "attempt", None)).unwrap();
        assert!(journal
            .check_reservation("project", "attempt", None)
            .is_err());
        assert!(journal
            .check_reservation("project", "attempt", Some("operation"))
            .is_err());
        journal
            .begin(intent("project", "managed", Some("operation")))
            .unwrap();
        assert!(journal
            .begin(intent("project-two", "new", Some("operation")))
            .is_err());
        let hash = journal.get("project", "managed").unwrap().head.clone();
        journal
            .close("project", "managed", &hash, closure())
            .unwrap();
        assert!(journal
            .begin(intent("project-two", "new", Some("operation")))
            .is_err());
        journal
            .begin(intent("project-two", "attempt", None))
            .unwrap();
        assert_eq!(journal.all().len(), 3);
    }

    #[test]
    fn malformed_authority_and_anchors_are_rejected_before_publication() {
        let dir = tempfile::tempdir().unwrap();
        let mut journal = StartupJournal::open(dir.path()).unwrap();
        let mut variants = Vec::new();
        let base = intent("project", "attempt", Some("operation"));
        let mut v = base.clone();
        v.protocol = "unknown-legacy-protocol".into();
        variants.push(v);
        let mut v = base.clone();
        v.schema_version = 3;
        variants.push(v);
        let mut v = base.clone();
        v.management_hash = None;
        variants.push(v);
        let mut v = base.clone();
        v.management_operation = None;
        variants.push(v);
        let mut v = base.clone();
        v.workflow_sequence = 0;
        variants.push(v);
        let mut v = base.clone();
        v.project_id = "../escape".into();
        variants.push(v);
        let mut v = base.clone();
        v.attempt_id = "CON".into();
        variants.push(v);
        let mut v = base.clone();
        v.revision_digest = "invalid".into();
        variants.push(v);
        for invalid in variants {
            assert!(journal.begin(invalid).is_err());
            assert!(journal.healthy());
        }
        assert_eq!(
            fs::read_dir(dir.path().join("codex-startups"))
                .unwrap()
                .count(),
            0
        );
        journal.begin(base).unwrap();
        let head = journal.get("project", "attempt").unwrap().head.clone();
        for invalid in [
            StartupClosure {
                reason: "".into(),
                ..closure()
            },
            StartupClosure {
                reason: "\0".into(),
                ..closure()
            },
            StartupClosure {
                reason: "x".repeat(8001),
                ..closure()
            },
            StartupClosure {
                workflow_sequence: 1,
                ..closure()
            },
            StartupClosure {
                workflow_hash: Some("e".repeat(64)),
                ..closure()
            },
        ] {
            assert!(journal.close("project", "attempt", &head, invalid).is_err());
        }
        assert!(!path(dir.path(), "1.json").exists());
    }

    #[test]
    fn replay_rejects_hash_scope_parent_and_unknown_field_tampering() {
        for mode in [
            "hash",
            "scope",
            "parent",
            "unknown-field",
            "orphan",
            "oversized",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let mut journal = StartupJournal::open(dir.path()).unwrap();
            journal.begin(intent("project", "attempt", None)).unwrap();
            let head = journal.get("project", "attempt").unwrap().head.clone();
            journal
                .close("project", "attempt", &head, closure())
                .unwrap();
            drop(journal);
            let first = path(dir.path(), "0.json");
            match mode {
                "hash" => {
                    let mut value: serde_json::Value =
                        serde_json::from_slice(&fs::read(&first).unwrap()).unwrap();
                    value["hash"] = "e".repeat(64).into();
                    fs::write(&first, serde_json::to_vec(&value).unwrap()).unwrap();
                }
                "scope" => rewrite(&first, |record| {
                    record.body.project_id = "another-project".into()
                }),
                "parent" => rewrite(&path(dir.path(), "1.json"), |record| {
                    record.body.previous = "e".repeat(64)
                }),
                "unknown-field" => {
                    let mut value: serde_json::Value =
                        serde_json::from_slice(&fs::read(&first).unwrap()).unwrap();
                    value["unexpectedAuthority"] = true.into();
                    fs::write(&first, serde_json::to_vec(&value).unwrap()).unwrap();
                }
                "orphan" => fs::remove_file(first).unwrap(),
                "oversized" => fs::write(first, vec![b' '; MAX_BYTES + 1]).unwrap(),
                _ => unreachable!(),
            }
            assert!(StartupJournal::open(dir.path()).is_err(), "{mode}");
        }
    }

    #[test]
    fn interrupted_writers_are_poisoned_and_complete_publications_recover_explicitly() {
        for closing in [false, true] {
            for interruption in [
                PublishPoint::TempCreated,
                PublishPoint::PartialWritten,
                PublishPoint::TempSynced,
                PublishPoint::Published,
                PublishPoint::PendingRemoved,
            ] {
                let dir = tempfile::tempdir().unwrap();
                let mut journal = StartupJournal::open(dir.path()).unwrap();
                let observer = |point| {
                    if point == interruption {
                        Err("synthetic publication interruption".into())
                    } else {
                        Ok(())
                    }
                };
                let result = if closing {
                    journal.begin(intent("project", "attempt", None)).unwrap();
                    let head = journal.get("project", "attempt").unwrap().head.clone();
                    journal.close_observed("project", "attempt", &head, closure(), observer)
                } else {
                    journal.begin_observed(intent("project", "attempt", None), observer)
                };
                assert!(result.is_err());
                assert!(!journal.healthy());
                assert!(journal.begin(intent("project", "different", None)).is_err());
                assert!(journal.check_reservation("project", "fresh", None).is_err());
                drop(journal);
                let reopened = StartupJournal::open(dir.path());
                if matches!(
                    interruption,
                    PublishPoint::TempCreated | PublishPoint::PartialWritten
                ) {
                    assert!(reopened.is_err());
                } else {
                    let mut journal = reopened.unwrap();
                    assert_eq!(
                        journal.get("project", "attempt").unwrap().closure.is_some(),
                        closing
                    );
                    assert_eq!(journal.all().len(), 1);
                    if interruption != PublishPoint::PendingRemoved {
                        assert!(journal.check_reservation("project", "fresh", None).is_err());
                        assert!(journal.begin(intent("project", "different", None)).is_err());
                    }
                    journal.recover_publications().unwrap();
                    assert!(journal.pending_publications.is_empty());
                    assert!(journal.check_reservation("project", "fresh", None).is_ok());
                    assert!(
                        !path(dir.path(), if closing { "1.pending" } else { "0.pending" }).exists()
                    );
                }
            }
        }
    }

    #[test]
    fn conflicting_or_semantically_invalid_pending_never_changes_any_file() {
        for mode in [
            "whitespace-conflict",
            "scope",
            "protocol",
            "closure-parent",
            "other-invalid",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let mut journal = StartupJournal::open(dir.path()).unwrap();
            journal.begin(intent("project", "attempt", None)).unwrap();
            if mode == "closure-parent" {
                let head = journal.get("project", "attempt").unwrap().head.clone();
                journal
                    .close("project", "attempt", &head, closure())
                    .unwrap();
            }
            drop(journal);
            let suffix = if mode == "closure-parent" { "1" } else { "0" };
            let final_path = path(dir.path(), &format!("{suffix}.json"));
            let pending = path(dir.path(), &format!("{suffix}.pending"));
            fs::copy(&final_path, &pending).unwrap();
            if mode == "whitespace-conflict" {
                let mut b = fs::read(&pending).unwrap();
                b.push(b' ');
                fs::write(&pending, b).unwrap();
            } else {
                fs::remove_file(&final_path).unwrap();
                match mode {
                    "scope" => rewrite(&pending, |r| r.body.project_id = "foreign".into()),
                    "protocol" => rewrite(&pending, |r| {
                        if let Evidence::Intent { intent } = &mut r.body.evidence {
                            intent.protocol = OMP_PROTOCOL.into();
                            intent.worker.provider = "deepseek".into();
                        }
                    }),
                    "closure-parent" => rewrite(&pending, |r| r.body.previous = "0".repeat(64)),
                    "other-invalid" => {
                        fs::write(dir.path().join("codex-startups/project/zzz.0.json"), b"{}")
                            .unwrap();
                    }
                    _ => unreachable!(),
                }
            }
            let before = fs::read(&pending).unwrap();
            let committed = fs::read(&final_path).ok();
            assert!(StartupJournal::open(dir.path()).is_err(), "{mode}");
            assert_eq!(fs::read(&pending).unwrap(), before);
            assert_eq!(fs::read(&final_path).ok(), committed);
        }
    }

    #[test]
    fn startup_recovery_rechecks_bytes_and_never_overwrites_changed_destination() {
        for mode in [
            "pending-changed",
            "target-appeared",
            "duplicate-target-changed",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let mut j = StartupJournal::open(dir.path()).unwrap();
            let stop = if mode == "duplicate-target-changed" {
                PublishPoint::Published
            } else {
                PublishPoint::TempSynced
            };
            assert!(j
                .begin_observed(intent("project", "attempt", None), |p| if p == stop {
                    Err("interrupted".into())
                } else {
                    Ok(())
                })
                .is_err());
            drop(j);
            let mut j = StartupJournal::open(dir.path()).unwrap();
            let target = path(dir.path(), "0.json");
            let pending = path(dir.path(), "0.pending");
            if mode == "pending-changed" {
                fs::write(&pending, b"{}").unwrap();
            } else {
                fs::write(&target, b"foreign target").unwrap();
            }
            let old = fs::read(&pending).unwrap();
            let published = fs::read(&target).ok();
            assert!(j.recover_publications().is_err());
            assert_eq!(fs::read(&pending).unwrap(), old);
            assert_eq!(fs::read(&target).ok(), published);
        }
    }

    #[test]
    fn publication_never_overwrites_a_destination_created_after_preflight() {
        let dir = tempfile::tempdir().unwrap();
        let mut journal = StartupJournal::open(dir.path()).unwrap();
        let final_path = path(dir.path(), "0.json");
        let result = journal.begin_observed(intent("project", "attempt", None), |point| {
            if point == PublishPoint::TempSynced {
                fs::write(&final_path, b"other writer evidence").map_err(|e| e.to_string())?;
            }
            Ok(())
        });
        assert!(result.is_err());
        assert!(!journal.healthy());
        assert_eq!(fs::read(&final_path).unwrap(), b"other writer evidence");
        assert!(path(dir.path(), "0.pending").is_file());
    }

    #[test]
    fn partial_batch_recovery_requires_reopen_and_preserves_all_records() {
        for point in [
            RecoveryPoint::Synced,
            RecoveryPoint::FinalNamePresent,
            RecoveryPoint::PendingRemoved,
        ] {
            let dir = tempfile::tempdir().unwrap();
            let mut j = StartupJournal::open(dir.path()).unwrap();
            for id in ["a", "b"] {
                j.begin(intent("project", id, None)).unwrap();
            }
            let mut originals = BTreeMap::new();
            for id in ["a", "b"] {
                let path = dir
                    .path()
                    .join(format!("codex-startups/project/{id}.0.json"));
                originals.insert(path.clone(), fs::read(&path).unwrap());
                fs::hard_link(&path, path.with_extension("pending")).unwrap();
                fs::remove_file(path).unwrap();
            }
            drop(j);
            let mut j = StartupJournal::open(dir.path()).unwrap();
            assert!(j
                .recover_publications_observed(|p, index| if p == point && index == 1 {
                    Err("second item interrupted".into())
                } else {
                    Ok(())
                })
                .is_err());
            assert!(!j.healthy());
            assert!(j.begin(intent("project", "new", None)).is_err());
            drop(j);
            let mut j = StartupJournal::open(dir.path()).unwrap();
            j.recover_publications().unwrap();
            assert_eq!(j.all().len(), 2);
            for (path, raw) in originals {
                assert_eq!(fs::read(&path).unwrap(), raw);
                assert!(!path.with_extension("pending").exists());
            }
        }
    }

    #[test]
    fn project_limit_is_checked_before_disk_write_and_is_project_scoped() {
        let dir = tempfile::tempdir().unwrap();
        let mut journal = StartupJournal::open(dir.path()).unwrap();
        for n in 0..MAX_PER_PROJECT {
            let prepared = intent("project", &format!("fixture-{n}"), None);
            journal.records.insert(
                (prepared.project_id.clone(), prepared.attempt_id.clone()),
                StartupRecord {
                    intent: prepared,
                    intent_hash: "a".repeat(64),
                    closure: None,
                    head: "a".repeat(64),
                },
            );
        }
        assert!(journal.begin(intent("project", "overflow", None)).is_err());
        assert!(journal.healthy());
        assert!(!dir.path().join("codex-startups/project").exists());
        journal
            .begin(intent("different-project", "attempt", None))
            .unwrap();
    }
}
