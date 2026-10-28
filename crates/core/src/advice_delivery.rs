//! Durable send intent and acknowledgement, separate from local workflow Advise.
//! A fresh journal never offers an automatic retry: intent without ACK is Unknown.
use crate::host::ordinary;
use cerulean_adapters::codex::SteerAck;
use cerulean_domain::execution::{artifact_path, digest, identifier, require, sha256, timestamp};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_OPERATIONS: usize = 1000;
const MAX_BYTES: usize = 16 * 1024;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeliveryIntent {
    pub delivery_id: String,
    pub management_operation: String,
    pub project_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub incident_id: String,
    pub revision_digest: String,
    pub run_digest: String,
    pub observation_digest: String,
    pub management_response_digest: String,
    pub advice_event_hash: String,
    pub text_digest: String,
}
impl DeliveryIntent {
    pub fn id_for(management_operation: &str) -> Result<String, String> {
        identifier(management_operation)?;
        Ok(format!(
            "delivery-{}",
            digest(&("supervision-delivery-v1", management_operation))?
        ))
    }
    fn validate(&self) -> Result<(), String> {
        for id in [
            &self.delivery_id,
            &self.management_operation,
            &self.project_id,
            &self.task_id,
            &self.attempt_id,
            &self.incident_id,
        ] {
            identifier(id)?;
            artifact_path(id)?;
        }
        require(
            self.delivery_id == Self::id_for(&self.management_operation)?,
            "delivery ID differs from management operation",
        )?;
        for value in [&self.thread_id, &self.turn_id] {
            require(
                !value.is_empty() && value.len() <= 1024 && !value.chars().any(char::is_control),
                "invalid delivery thread or turn",
            )?;
        }
        for hash in [
            &self.revision_digest,
            &self.run_digest,
            &self.observation_digest,
            &self.management_response_digest,
            &self.advice_event_hash,
            &self.text_digest,
        ] {
            sha256(hash)?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum DeliveryOutcome {
    Acknowledged { ack: SteerAck },
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryRecord {
    pub intent: DeliveryIntent,
    pub intent_hash: String,
    /// None is also unknown after restart, never permission to send again.
    pub outcome: Option<DeliveryOutcome>,
}
impl DeliveryRecord {
    pub fn acknowledged(&self) -> bool {
        matches!(self.outcome, Some(DeliveryOutcome::Acknowledged { .. }))
    }
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Evidence {
    SendIntent { intent: Box<DeliveryIntent> },
    Outcome { outcome: DeliveryOutcome },
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Body {
    version: u32,
    delivery_id: String,
    sequence: u8,
    previous: String,
    at: String,
    evidence: Evidence,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    body: Body,
    hash: String,
}

/// Single owner, normally moved together with a project's workflow to its worker.
/// The caller must cross-check these hashes against its trusted management/workflow
/// ledgers and current authorization; this storage type does not grant permission.
pub struct DeliveryJournal {
    root: PathBuf,
    records: BTreeMap<String, DeliveryRecord>,
    poisoned: bool,
    #[cfg(test)]
    fail_after_sync: bool,
}
impl DeliveryJournal {
    pub fn open(root: &Path) -> Result<Self, String> {
        if !root.exists() {
            fs::create_dir_all(root).map_err(|e| e.to_string())?;
        }
        ordinary(root, true)?;
        let mut journal = Self {
            root: root.into(),
            records: BTreeMap::new(),
            poisoned: false,
            #[cfg(test)]
            fail_after_sync: false,
        };
        let mut files = fs::read_dir(root)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        require(files.len() <= MAX_OPERATIONS * 2, "delivery history full")?;
        files.sort_by_key(|f| f.file_name());
        for f in files {
            ordinary(&f.path(), false)?;
            // Pending writes and unknown files are never silently discarded.
            require(
                f.file_name().to_string_lossy().ends_with(".json"),
                "unfinished or unexpected delivery record",
            )?;
            let mut bytes = vec![];
            fs::File::open(f.path())
                .map_err(|e| e.to_string())?
                .take((MAX_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            require(bytes.len() <= MAX_BYTES, "delivery record too large")?;
            let r: Record =
                serde_json::from_slice(&bytes).map_err(|_| "invalid delivery record")?;
            let b = r.body;
            require(
                b.version == 1 && b.sequence <= 1 && digest(&b)? == r.hash,
                "delivery record version or hash mismatch",
            )?;
            timestamp(&b.at)?;
            require(
                f.file_name().to_string_lossy() == format!("{}.{}.json", b.delivery_id, b.sequence),
                "delivery filename mismatch",
            )?;
            match b.evidence {
                Evidence::SendIntent { intent } => {
                    intent.validate()?;
                    require(
                        b.sequence == 0
                            && b.previous == "genesis"
                            && b.delivery_id == intent.delivery_id
                            && !journal.records.contains_key(&b.delivery_id),
                        "invalid delivery intent order",
                    )?;
                    journal.records.insert(
                        b.delivery_id,
                        DeliveryRecord {
                            intent: *intent,
                            intent_hash: r.hash,
                            outcome: None,
                        },
                    );
                }
                Evidence::Outcome { outcome } => {
                    let old = journal
                        .records
                        .get_mut(&b.delivery_id)
                        .ok_or("delivery outcome without intent")?;
                    require(
                        b.sequence == 1 && b.previous == old.intent_hash && old.outcome.is_none(),
                        "delivery outcome chain mismatch",
                    )?;
                    validate_outcome(&old.intent, &outcome)?;
                    old.outcome = Some(outcome);
                }
            }
        }
        Ok(journal)
    }
    pub fn records(&self) -> impl Iterator<Item = &DeliveryRecord> {
        self.records.values()
    }
    pub fn record(&self, id: &str) -> Option<&DeliveryRecord> {
        self.records.get(id)
    }
    pub fn begin(&mut self, intent: DeliveryIntent) -> Result<(), String> {
        intent.validate()?;
        require(!self.poisoned, "delivery journal requires reopen")?;
        require(self.records.len() < MAX_OPERATIONS, "delivery history full")?;
        require(
            !self.records.contains_key(&intent.delivery_id),
            "delivery already recorded; never resend",
        )?;
        let id = intent.delivery_id.clone();
        let hash = self.publish(
            &id,
            0,
            "genesis".into(),
            Evidence::SendIntent {
                intent: Box::new(intent.clone()),
            },
        )?;
        self.records.insert(
            id,
            DeliveryRecord {
                intent,
                intent_hash: hash,
                outcome: None,
            },
        );
        Ok(())
    }
    pub fn finish(&mut self, id: &str, outcome: DeliveryOutcome) -> Result<(), String> {
        let old = self.records.get(id).ok_or("delivery intent missing")?;
        require(old.outcome.is_none(), "delivery outcome already recorded")?;
        validate_outcome(&old.intent, &outcome)?;
        self.publish(
            id,
            1,
            old.intent_hash.clone(),
            Evidence::Outcome {
                outcome: outcome.clone(),
            },
        )?;
        self.records.get_mut(id).expect("validated intent").outcome = Some(outcome);
        Ok(())
    }
    fn publish(
        &mut self,
        id: &str,
        sequence: u8,
        previous: String,
        evidence: Evidence,
    ) -> Result<String, String> {
        require(!self.poisoned, "delivery journal requires reopen")?;
        let body = Body {
            version: 1,
            delivery_id: id.into(),
            sequence,
            previous,
            at: chrono::Utc::now().to_rfc3339(),
            evidence,
        };
        let hash = digest(&body)?;
        let bytes = serde_json::to_vec(&Record {
            body,
            hash: hash.clone(),
        })
        .map_err(|e| e.to_string())?;
        require(bytes.len() <= MAX_BYTES, "delivery record too large")?;
        let temp = self.root.join(format!("{id}.{sequence}.pending"));
        let target = self.root.join(format!("{id}.{sequence}.json"));
        require(
            !temp.exists() && !target.exists(),
            "delivery publication already exists",
        )?;
        self.poisoned = true;
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);
        #[cfg(test)]
        if self.fail_after_sync {
            return Err("injected interruption after delivery sync".into());
        }
        fs::rename(temp, target).map_err(|e| e.to_string())?;
        self.poisoned = false;
        Ok(hash)
    }
}
fn validate_outcome(intent: &DeliveryIntent, outcome: &DeliveryOutcome) -> Result<(), String> {
    if let DeliveryOutcome::Acknowledged { ack } = outcome {
        require(
            ack.delivery_id == intent.delivery_id
                && ack.thread_id == intent.thread_id
                && ack.turn_id == intent.turn_id,
            "delivery ACK scope mismatch",
        )?;
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn intent() -> DeliveryIntent {
        DeliveryIntent {
            delivery_id: DeliveryIntent::id_for("advice-operation").unwrap(),
            management_operation: "advice-operation".into(),
            project_id: "project".into(),
            task_id: "task".into(),
            attempt_id: "attempt".into(),
            thread_id: "thread".into(),
            turn_id: "turn".into(),
            incident_id: "incident".into(),
            revision_digest: "a".repeat(64),
            run_digest: "b".repeat(64),
            observation_digest: "c".repeat(64),
            management_response_digest: "d".repeat(64),
            advice_event_hash: "e".repeat(64),
            text_digest: "f".repeat(64),
        }
    }
    fn ack(i: &DeliveryIntent) -> DeliveryOutcome {
        DeliveryOutcome::Acknowledged {
            ack: SteerAck {
                delivery_id: i.delivery_id.clone(),
                thread_id: i.thread_id.clone(),
                turn_id: i.turn_id.clone(),
            },
        }
    }
    #[test]
    fn crash_after_intent_never_reauthorizes_send_and_ack_is_separate() {
        for outcome in [None, Some(DeliveryOutcome::Unknown), Some(ack(&intent()))] {
            let dir = tempfile::tempdir().unwrap();
            let mut journal = DeliveryJournal::open(dir.path()).unwrap();
            let i = intent();
            journal.begin(i.clone()).unwrap();
            if let Some(ref o) = outcome {
                journal.finish(&i.delivery_id, o.clone()).unwrap();
            }
            drop(journal);
            let mut reopened = DeliveryJournal::open(dir.path()).unwrap();
            let record = reopened.record(&i.delivery_id).unwrap();
            assert_eq!(record.outcome, outcome);
            assert_eq!(
                record.acknowledged(),
                matches!(outcome, Some(DeliveryOutcome::Acknowledged { .. }))
            );
            assert!(reopened.begin(i.clone()).is_err());
            assert_eq!(reopened.records().count(), 1);
        }
    }
    #[test]
    fn wrong_ack_and_rehashed_cross_turn_evidence_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let mut journal = DeliveryJournal::open(dir.path()).unwrap();
        let i = intent();
        journal.begin(i.clone()).unwrap();
        let mut wrong = ack(&i);
        if let DeliveryOutcome::Acknowledged { ack } = &mut wrong {
            ack.turn_id = "other".into();
        }
        assert!(journal.finish(&i.delivery_id, wrong.clone()).is_err());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
        journal.finish(&i.delivery_id, ack(&i)).unwrap();
        let path = dir.path().join(format!("{}.1.json", i.delivery_id));
        let mut r: Record = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        r.body.evidence = Evidence::Outcome { outcome: wrong };
        r.hash = digest(&r.body).unwrap();
        fs::write(path, serde_json::to_vec(&r).unwrap()).unwrap();
        assert!(DeliveryJournal::open(dir.path()).is_err());
    }
    #[test]
    fn interrupted_publication_poison_and_missing_intent_block_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let mut journal = DeliveryJournal::open(dir.path()).unwrap();
        let i = intent();
        journal.begin(i.clone()).unwrap();
        // Stop after a complete synced pending file, before publication.
        journal.fail_after_sync = true;
        assert!(journal.finish(&i.delivery_id, ack(&i)).is_err());
        assert!(journal.poisoned);
        assert!(!dir
            .path()
            .join(format!("{}.1.json", i.delivery_id))
            .exists());
        let pending = dir.path().join(format!("{}.1.pending", i.delivery_id));
        assert!(serde_json::from_slice::<Record>(&fs::read(&pending).unwrap()).is_ok());
        assert!(DeliveryJournal::open(dir.path()).is_err());
        journal.fail_after_sync = false;
        fs::write(&pending, b"partial").unwrap();
        assert!(journal.finish(&i.delivery_id, ack(&i)).is_err());
        assert!(DeliveryJournal::open(dir.path()).is_err());
        // A receipt detached from its intent is not acceptance evidence.
        let fresh = tempfile::tempdir().unwrap();
        let mut journal = DeliveryJournal::open(fresh.path()).unwrap();
        journal.begin(i.clone()).unwrap();
        journal.finish(&i.delivery_id, ack(&i)).unwrap();
        fs::remove_file(fresh.path().join(format!("{}.0.json", i.delivery_id))).unwrap();
        assert!(DeliveryJournal::open(fresh.path()).is_err());
    }
    #[test]
    fn invalid_windows_path_or_inconsistent_delivery_id_writes_nothing() {
        for case in 0..3 {
            let dir = tempfile::tempdir().unwrap();
            let mut journal = DeliveryJournal::open(dir.path()).unwrap();
            let mut i = intent();
            match case {
                0 => i.project_id = "CON".into(),
                1 => i.delivery_id = "other".into(),
                2 => i.advice_event_hash = "bad".into(),
                _ => unreachable!(),
            }
            assert!(journal.begin(i).is_err());
            assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
        }
    }
}
