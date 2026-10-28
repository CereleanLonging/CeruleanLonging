//! User disposition is separate from delivery evidence: it never creates an ACK.
use crate::host::ordinary;
use cerulean_domain::execution::{digest, identifier, require, sha256, timestamp};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ClosureProof {
    pub operation: String,
    pub project: String,
    pub attempt: String,
    pub management_digest: String,
    pub delivery_digest: String,
    pub advice_event_hash: String,
    pub workflow_sequence: u32,
    pub workflow_hash: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Closure {
    pub version: u32,
    pub actor: String,
    pub at: String,
    pub reason: String,
    pub reviewed_digest: String,
    pub proof: ClosureProof,
}
impl Closure {
    fn validate(&self) -> Result<(), String> {
        require(
            self.version == 1 && self.actor == "user",
            "invalid advice closure authority",
        )?;
        timestamp(&self.at)?;
        require(
            !self.reason.trim().is_empty()
                && self.reason.len() <= 8000
                && !self.reason.contains('\0'),
            "invalid advice closure reason",
        )?;
        for id in [
            &self.proof.operation,
            &self.proof.project,
            &self.proof.attempt,
        ] {
            identifier(id)?;
        }
        for h in [
            &self.reviewed_digest,
            &self.proof.management_digest,
            &self.proof.delivery_digest,
            &self.proof.advice_event_hash,
            &self.proof.workflow_hash,
        ] {
            sha256(h)?;
        }
        require(
            self.proof.workflow_sequence > 0,
            "advice closure requires workflow anchor",
        )
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    body: Closure,
    hash: String,
}
pub(crate) struct ClosureJournal {
    root: PathBuf,
    records: BTreeMap<String, Closure>,
    poisoned: bool,
    #[cfg(test)]
    fail_after_sync: bool,
}
impl ClosureJournal {
    fn filename(operation: &str) -> Result<String, String> {
        Ok(format!(
            "{}.json",
            digest(&("advice-closure-v1", operation))?
        ))
    }
    pub fn open(root: &Path) -> Result<Self, String> {
        let root = root.join("advice-closures");
        if !root.exists() {
            fs::create_dir(&root).map_err(|e| e.to_string())?;
        }
        ordinary(&root, true)?;
        let mut journal = Self {
            root,
            records: BTreeMap::new(),
            poisoned: false,
            #[cfg(test)]
            fail_after_sync: false,
        };
        let entries = fs::read_dir(&journal.root)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        require(entries.len() <= 10000, "advice closure history full")?;
        for entry in entries {
            ordinary(&entry.path(), false)?;
            let mut bytes = vec![];
            fs::File::open(entry.path())
                .map_err(|e| e.to_string())?
                .take(16385)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            require(bytes.len() <= 16384, "advice closure too large")?;
            let saved: Saved =
                serde_json::from_slice(&bytes).map_err(|_| "invalid advice closure")?;
            saved.body.validate()?;
            require(
                saved.hash == digest(&saved.body)?
                    && entry.file_name().to_string_lossy()
                        == Self::filename(&saved.body.proof.operation)?,
                "unfinished or mismatched advice closure",
            )?;
            require(
                !journal.records.contains_key(&saved.body.proof.operation),
                "duplicate advice closure",
            )?;
            journal
                .records
                .insert(saved.body.proof.operation.clone(), saved.body);
        }
        Ok(journal)
    }
    pub fn healthy(&self) -> bool {
        !self.poisoned
    }
    pub fn get(&self, operation: &str) -> Option<&Closure> {
        self.records.get(operation)
    }
    pub fn all(&self) -> impl Iterator<Item = &Closure> {
        self.records.values()
    }
    pub fn save(&mut self, body: Closure) -> Result<(), String> {
        body.validate()?;
        require(self.healthy(), "advice closure requires reopen")?;
        require(
            self.records.len() < 10000 && self.get(&body.proof.operation).is_none(),
            "advice tracking already closed or history full",
        )?;
        let destination = self.root.join(Self::filename(&body.proof.operation)?);
        let pending = destination.with_extension("pending");
        let saved = Saved {
            hash: digest(&body)?,
            body: body.clone(),
        };
        let bytes = serde_json::to_vec(&saved).map_err(|e| e.to_string())?;
        require(bytes.len() <= 16384, "advice closure too large")?;
        let result = (|| {
            require(!destination.exists(), "advice closure already exists")?;
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&pending)
                .map_err(|e| e.to_string())?;
            file.write_all(&bytes)
                .and_then(|_| file.sync_all())
                .map_err(|e| e.to_string())?;
            #[cfg(test)]
            if self.fail_after_sync {
                return Err("injected closure interruption".into());
            }
            drop(file);
            fs::rename(&pending, &destination).map_err(|e| e.to_string())
        })();
        if let Err(error) = result {
            self.poisoned = true;
            return Err(error);
        }
        self.records.insert(body.proof.operation.clone(), body);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn closure() -> Closure {
        Closure {
            version: 1,
            actor: "user".into(),
            at: chrono::Utc::now().to_rfc3339(),
            reason: "Proceed after reviewing the uncertain advice".into(),
            reviewed_digest: "a".repeat(64),
            proof: ClosureProof {
                operation: "advice-1".into(),
                project: "project".into(),
                attempt: "attempt-1".into(),
                management_digest: "b".repeat(64),
                delivery_digest: "c".repeat(64),
                advice_event_hash: "d".repeat(64),
                workflow_sequence: 8,
                workflow_hash: "e".repeat(64),
            },
        }
    }
    #[test]
    fn closure_is_immutable_and_interrupted_publication_fails_closed() {
        let root = tempfile::tempdir().unwrap();
        let mut journal = ClosureJournal::open(root.path()).unwrap();
        journal.save(closure()).unwrap();
        assert!(journal.save(closure()).is_err());
        assert_eq!(
            ClosureJournal::open(root.path())
                .unwrap()
                .get("advice-1")
                .unwrap()
                .actor,
            "user"
        );
        let other = tempfile::tempdir().unwrap();
        let mut interrupted = ClosureJournal::open(other.path()).unwrap();
        interrupted.fail_after_sync = true;
        assert!(interrupted.save(closure()).is_err());
        assert!(!interrupted.healthy());
        assert!(interrupted.get("advice-1").is_none());
        assert!(ClosureJournal::open(other.path()).is_err());
    }
    #[test]
    fn closure_rejects_forged_authority_filename_and_content() {
        let root = tempfile::tempdir().unwrap();
        let mut journal = ClosureJournal::open(root.path()).unwrap();
        let mut invalid = closure();
        invalid.actor = "deputy".into();
        assert!(journal.save(invalid).is_err());
        journal.save(closure()).unwrap();
        let path = journal
            .root
            .join(ClosureJournal::filename("advice-1").unwrap());
        let bytes = fs::read(&path).unwrap();
        let mut saved: Saved = serde_json::from_slice(&bytes).unwrap();
        saved.body.reason = "changed".into();
        fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();
        assert!(ClosureJournal::open(root.path()).is_err());
        fs::write(&path, bytes).unwrap();
        fs::rename(&path, path.with_file_name("foreign.json")).unwrap();
        assert!(ClosureJournal::open(root.path()).is_err());
    }
}
