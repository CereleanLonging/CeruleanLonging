//! Seal the exact observed files before publishing a workflow submission.
//! A terminal without this seal cannot recover Completed by recapturing files.
use super::*;
use cerulean_adapters::omp_protocol::PromptStatus;
use cerulean_domain::execution::{
    artifact_path, sha256, ArtifactManifest, ExecutionReceipt, ReceiptStatus,
};

const LIMIT: usize = 256 * 1024;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompletionEvidence {
    pub receipt: ExecutionReceipt,
    pub manifest: ArtifactManifest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_head: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SealBody {
    version: u32,
    intent_digest: String,
    terminal_head: String,
    evidence: CompletionEvidence,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Seal {
    body: SealBody,
    hash: String,
}

impl OmpJournal {
    fn validate_completion(&self, evidence: &CompletionEvidence) -> Result<(), String> {
        require(
            self.observation
                .terminal
                .as_ref()
                .is_some_and(|r| r.status == PromptStatus::Completed),
            "OMP completion seal requires durable completed terminal",
        )?;
        if let Some(head) = &evidence.tool_head {
            if head != "genesis" {
                sha256(head)?;
            }
        }
        let r = &evidence.receipt;
        let m = &evidence.manifest;
        require(
            r.schema_version == 1
                && r.status == ReceiptStatus::Completed
                && r.task_id == self.intent.task_id
                && r.attempt_id == self.intent.attempt_id
                && r.fence == self.intent.fence
                && r.binding == self.intent.binding
                && r.turn_id.is_none()
                && r.evidence_sha256 == self.head,
            "OMP completion receipt binding mismatch",
        )?;
        timestamp(&r.observed_at)?;
        require(
            m.schema_version == 1
                && m.revision_digest == self.intent.revision_digest
                && m.task_id == r.task_id
                && m.attempt_id == r.attempt_id
                && m.receipt_digest == digest(r)?,
            "OMP completion manifest binding mismatch",
        )?;
        let mut previous: Option<&str> = None;
        for a in &m.artifacts {
            artifact_path(&a.path)?;
            sha256(&a.sha256)?;
            require(
                previous.is_none_or(|p| p < a.path.as_str()),
                "OMP completion artifacts must be unique and sorted",
            )?;
            previous = Some(&a.path);
        }
        Ok(())
    }
    pub fn completion(&self) -> Option<&CompletionEvidence> {
        self.completion.as_ref()
    }
    pub(super) fn load_completion(&mut self) -> Result<(), String> {
        let path = self.root.join("completion.json");
        match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err("OMP completion seal inaccessible".into()),
            Ok(_) => {}
        }
        ordinary(&path, false)?;
        let mut bytes = Vec::new();
        File::open(path)
            .map_err(|_| "OMP completion seal read failed")?
            .take((LIMIT + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| "OMP completion seal read failed")?;
        require(bytes.len() <= LIMIT, "OMP completion seal too large")?;
        let seal: Seal =
            serde_json::from_slice(&bytes).map_err(|_| "OMP completion seal invalid")?;
        require(
            seal.body.version == 1
                && seal.body.intent_digest == self.intent_digest
                && seal.body.terminal_head == self.head
                && digest(&seal.body)? == seal.hash,
            "OMP completion seal chain mismatch",
        )?;
        self.validate_completion(&seal.body.evidence)?;
        self.completion = Some(seal.body.evidence);
        Ok(())
    }
    pub fn seal_completion(&mut self, evidence: CompletionEvidence) -> Result<(), String> {
        self.seal_completion_observed(evidence, &mut |_| Ok(()))
    }
    pub(crate) fn seal_completion_observed(
        &mut self,
        evidence: CompletionEvidence,
        observe: &mut impl FnMut(&str) -> Result<(), String>,
    ) -> Result<(), String> {
        require(
            self.healthy() && self.completion.is_none(),
            "OMP completion seal already exists or journal requires recovery",
        )?;
        self.validate_completion(&evidence)?;
        let body = SealBody {
            version: 1,
            intent_digest: self.intent_digest.clone(),
            terminal_head: self.head.clone(),
            evidence: evidence.clone(),
        };
        let hash = digest(&body)?;
        let bytes = serde_json::to_vec(&Seal { body, hash })
            .map_err(|_| "OMP completion seal encoding failed")?;
        require(bytes.len() <= LIMIT, "OMP completion seal too large")?;
        self.poisoned = true;
        ordinary(&self.root, true)?;
        let pending = self.root.join("completion.pending");
        let target = self.root.join("completion.json");
        require(
            fs::symlink_metadata(&target).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
            "OMP immutable completion seal already exists or is inaccessible",
        )?;
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&pending)
            .map_err(|_| "OMP pending seal creation failed")?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "OMP completion seal sync failed")?;
        drop(file);
        observe("seal-synced")?;
        fs::hard_link(&pending, &target)
            .map_err(|_| "OMP non-replacing seal publication failed")?;
        observe("seal-published")?;
        fs::remove_file(&pending).map_err(|_| "OMP pending seal removal failed")?;
        self.completion = Some(evidence);
        self.poisoned = false;
        observe("seal-committed")?;
        Ok(())
    }
}
