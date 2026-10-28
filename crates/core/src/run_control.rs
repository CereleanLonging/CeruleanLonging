//! Immutable authorization and pause events. Project outcome is still derived from the workflow.
use crate::host::ordinary;
use cerulean_domain::{
    execution::{digest, identifier, require, sha256, timestamp},
    project_setup::{ProjectRun, RunState},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Body {
    sequence: u32,
    previous: String,
    at: String,
    actor: String,
    run: ProjectRun,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    body: Body,
    hash: String,
}
pub(crate) struct RunLedger {
    root: PathBuf,
    runs: BTreeMap<String, ProjectRun>,
    project_heads: BTreeMap<String, String>,
    authorizations: std::collections::BTreeSet<(String, String)>,
    sequence: u32,
    hash: String,
    poisoned: bool,
}
#[cfg(test)]
pub(crate) fn forge_completed_for_test(
    path: &Path,
    value: serde_json::Value,
) -> Result<(), String> {
    let mut record: Record = serde_json::from_value(value).map_err(|e| e.to_string())?;
    record.hash = digest(&record.body)?;
    fs::write(
        path,
        serde_json::to_vec(&record).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
impl RunLedger {
    pub(crate) fn open(root: &Path) -> Result<Self, String> {
        Self::open_inner(root, true)
    }
    pub(crate) fn read_existing(root: &Path) -> Result<Self, String> {
        Self::open_inner(root, false)
    }
    fn open_inner(root: &Path, create: bool) -> Result<Self, String> {
        let root = root.join("run-control");
        if !root.exists() {
            require(create, "original run control directory missing")?;
            fs::create_dir(&root).map_err(|e| e.to_string())?;
        }
        ordinary(&root, true)?;
        let mut ledger = Self {
            root,
            runs: BTreeMap::new(),
            project_heads: BTreeMap::new(),
            authorizations: Default::default(),
            sequence: 0,
            hash: "genesis".into(),
            poisoned: false,
        };
        let mut files = fs::read_dir(&ledger.root)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        require(files.len() <= 10000, "run control history full")?;
        files.sort_by_key(|f| f.file_name());
        for f in files {
            ordinary(&f.path(), false)?;
            require(
                f.file_name().to_string_lossy() == format!("{:08}.json", ledger.sequence + 1),
                "run control sequence gap or unfinished write",
            )?;
            let mut bytes = vec![];
            fs::File::open(f.path())
                .map_err(|e| e.to_string())?
                .take(32769)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            require(bytes.len() <= 32768, "run event too large")?;
            let record: Record =
                serde_json::from_slice(&bytes).map_err(|_| "invalid run control record")?;
            timestamp(&record.body.at)?;
            require(
                record.body.sequence == ledger.sequence + 1
                    && record.body.previous == ledger.hash
                    && digest(&record.body)? == record.hash,
                "run control hash or sequence mismatch",
            )?;
            ledger.validate(&record.body.run, &record.body.actor)?;
            if record.body.run.state == RunState::Active {
                ledger
                    .authorizations
                    .insert((record.body.run.id.clone(), digest(&record.body.run)?));
            }
            ledger.sequence = record.body.sequence;
            ledger
                .project_heads
                .insert(record.body.run.project_id.clone(), record.hash.clone());
            ledger.hash = record.hash;
            ledger
                .runs
                .insert(record.body.run.id.clone(), record.body.run);
        }
        Ok(ledger)
    }
    fn validate(&self, run: &ProjectRun, actor: &str) -> Result<(), String> {
        identifier(&run.id)?;
        identifier(&run.project_id)?;
        sha256(&run.revision_digest)?;
        sha256(&run.setup_digest)?;
        require(
            !run.message.is_empty() && run.message.len() <= 4096,
            "invalid run message",
        )?;
        require(
            matches!(
                (actor, &run.state),
                ("user", RunState::Active | RunState::Paused)
                    | (
                        "core",
                        RunState::Attention | RunState::Completed | RunState::Superseded
                    )
            ),
            "invalid run control authority",
        )?;
        if let Some(old) = self.runs.get(&run.id) {
            require(
                !old.state.terminal()
                    && old.project_id == run.project_id
                    && old.revision_digest == run.revision_digest
                    && old.setup_digest == run.setup_digest
                    && run.acknowledged_decisions >= old.acknowledged_decisions,
                "run authorization changed or completed",
            )?;
            require(
                run.state != RunState::Superseded || old.state == RunState::Paused,
                "revision must pause the old run before closing it",
            )?;
            require(
                actor == "user" || run.acknowledged_decisions == old.acknowledged_decisions,
                "core cannot acknowledge user attention",
            )?;
        } else {
            require(
                actor == "user" && run.state == RunState::Active,
                "user must start run",
            )?;
        }
        require(
            !self
                .runs
                .values()
                .any(|r| r.id != run.id && r.project_id == run.project_id && !r.state.terminal()),
            "project already has an unfinished run",
        )
    }
    pub(crate) fn save(&mut self, run: ProjectRun, actor: &str) -> Result<(), String> {
        require(
            !self.poisoned && self.sequence < 10000,
            "run control requires recovery or archive",
        )?;
        self.validate(&run, actor)?;
        let body = Body {
            sequence: self.sequence + 1,
            previous: self.hash.clone(),
            at: chrono::Utc::now().to_rfc3339(),
            actor: actor.into(),
            run: run.clone(),
        };
        let hash = digest(&body)?;
        let bytes = serde_json::to_vec(&Record {
            body,
            hash: hash.clone(),
        })
        .map_err(|e| e.to_string())?;
        require(bytes.len() <= 32768, "run event too large")?;
        let target = self.root.join(format!("{:08}.json", self.sequence + 1));
        let temp = target.with_extension("pending");
        require(
            !target.exists() && !temp.exists(),
            "run event already exists",
        )?;
        self.poisoned = true;
        let mut f = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        f.write_all(&bytes)
            .and_then(|_| f.sync_all())
            .map_err(|e| e.to_string())?;
        drop(f);
        fs::rename(temp, target).map_err(|e| e.to_string())?;
        self.sequence += 1;
        self.project_heads
            .insert(run.project_id.clone(), hash.clone());
        self.hash = hash;
        if run.state == RunState::Active {
            self.authorizations.insert((run.id.clone(), digest(&run)?));
        }
        self.runs.insert(run.id.clone(), run);
        self.poisoned = false;
        Ok(())
    }
    pub(crate) fn all(&self) -> Vec<ProjectRun> {
        self.iter().cloned().collect()
    }
    pub(crate) fn evidence_digest(&self) -> &str {
        &self.hash
    }
    pub(crate) fn healthy(&self) -> bool {
        !self.poisoned
    }
    pub(crate) fn project_evidence_digest(&self, project: &str) -> &str {
        self.project_heads
            .get(project)
            .map(String::as_str)
            .unwrap_or("genesis")
    }
    pub(crate) fn has_authorization(&self, run: &str, hash: &str) -> bool {
        self.authorizations.contains(&(run.into(), hash.into()))
    }
    pub(crate) fn iter(&self) -> impl Iterator<Item = &ProjectRun> {
        self.runs.values()
    }
    pub(crate) fn project_runs<'a>(
        &'a self,
        project_id: &'a str,
    ) -> impl Iterator<Item = &'a ProjectRun> + 'a {
        self.runs
            .values()
            .filter(move |run| run.project_id == project_id)
    }
    pub(crate) fn sequence(&self) -> u32 {
        self.sequence
    }
}

#[cfg(test)]
impl RunLedger {
    pub(crate) fn poison_for_test(&mut self) {
        self.poisoned = true;
    }
}
