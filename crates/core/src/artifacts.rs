//! Independent filesystem observations for an approved task. Does not execute
//! model-provided commands, mark workflow tasks verified, or authenticate callers.
use cerulean_domain::execution::*;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Read,
    path::{Component, Path, PathBuf},
};

const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 256 * 1024 * 1024;

/// Canonical absolute paths only. Windows containment must respect case folding
/// and component boundaries (a project named 'app' does not contain 'apple').
pub(crate) fn contains_path(root: &Path, child: &Path) -> bool {
    #[cfg(windows)]
    {
        let root: Vec<_> = root
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
            .collect();
        let child: Vec<_> = child
            .components()
            .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
            .collect();
        child.starts_with(&root)
    }
    #[cfg(not(windows))]
    {
        child.starts_with(root)
    }
}

fn ordinary(path: &Path) -> Result<fs::Metadata, String> {
    let m = fs::symlink_metadata(path).map_err(|e| format!("artifact metadata: {e}"))?;
    require(!m.file_type().is_symlink(), "linked artifact path")?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        require(m.file_attributes() & 0x400 == 0, "reparse artifact path")?;
    }
    Ok(m)
}
pub(crate) fn root_path(root: &str) -> Result<PathBuf, String> {
    let path = Path::new(root);
    require(path.is_absolute(), "project root must be absolute")?;
    let mut checked = PathBuf::new();
    for part in path.components() {
        match part {
            Component::Prefix(_) | Component::RootDir => checked.push(part.as_os_str()),
            Component::Normal(_) => {
                checked.push(part.as_os_str());
                require(
                    ordinary(&checked)?.is_dir(),
                    "project ancestor is not a directory",
                )?;
            }
            _ => return Err("project root is not normalized".into()),
        }
    }
    require(ordinary(&checked)?.is_dir(), "invalid project root")?;
    fs::canonicalize(&checked).map_err(|e| e.to_string())
}
pub(crate) fn read_artifact(root: &Path, relative: &str) -> Result<Artifact, String> {
    artifact_path(relative)?;
    let mut path = root.to_path_buf();
    let parts: Vec<_> = relative.split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        path.push(part);
        let m = ordinary(&path)?;
        require(
            if i + 1 == parts.len() {
                m.is_file()
            } else {
                m.is_dir()
            },
            "artifact path type mismatch",
        )?;
    }
    require(
        fs::canonicalize(&path)
            .map_err(|e| e.to_string())?
            .starts_with(root),
        "artifact escaped project",
    )?;
    let mut file = File::open(&path).map_err(|e| e.to_string())?;
    let before = file.metadata().map_err(|e| e.to_string())?;
    require(before.len() <= MAX_FILE_BYTES, "artifact size limit")?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut bytes = 0;
    loop {
        let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        bytes += n as u64;
        require(bytes <= MAX_FILE_BYTES, "artifact grew beyond limit")?;
        hasher.update(&buffer[..n]);
    }
    let after = file.metadata().map_err(|e| e.to_string())?;
    let path_after = ordinary(&path)?;
    require(
        before.len() == bytes
            && after.len() == bytes
            && path_after.len() == bytes
            && before.modified().ok() == after.modified().ok()
            && after.modified().ok() == path_after.modified().ok(),
        "artifact changed during observation",
    )?;
    Ok(Artifact {
        path: relative.into(),
        bytes: bytes as u32,
        sha256: format!("{:x}", hasher.finalize()),
    })
}

pub struct ArtifactObserver<'a> {
    plan: &'a PlanRevision,
    attempt: &'a Attempt,
    receipt: &'a ExecutionReceipt,
    root: PathBuf,
}
impl<'a> ArtifactObserver<'a> {
    pub fn new(
        plan: &'a PlanRevision,
        approval: &Approval,
        authenticated_user: &str,
        active_policy_version: u32,
        attempt: &'a Attempt,
        receipt: &'a ExecutionReceipt,
        expected_turn: Option<&str>,
    ) -> Result<Self, String> {
        approval.validate(plan, authenticated_user, active_policy_version)?;
        receipt.validate(plan, attempt, expected_turn)?;
        #[cfg(windows)]
        crate::git_workspaces::require_prepared(plan, attempt)?;
        #[cfg(not(windows))]
        require(
            plan.worktree.is_none(),
            "worktree execution requires Windows",
        )?;
        require(
            receipt.status == ReceiptStatus::Completed,
            "completed receipt required",
        )?;
        require(
            plan.policy.tools.contains(&ToolPermission::ReadFiles),
            "artifact read permission required",
        )?;
        Ok(Self {
            plan,
            attempt,
            receipt,
            root: root_path(&plan.execution_root(&attempt.task_id, &attempt.id)?)?,
        })
    }
    pub fn capture(&self) -> Result<ArtifactManifest, String> {
        // Revalidate the root for each observation; it may have been replaced.
        require(
            root_path(
                &self
                    .plan
                    .execution_root(&self.attempt.task_id, &self.attempt.id)?,
            )? == self.root,
            "project binding changed",
        )?;
        let mut paths = self
            .plan
            .task(&self.attempt.task_id)?
            .artifact_paths
            .clone();
        paths.sort();
        let mut artifacts = Vec::new();
        let mut total = 0;
        for path in paths {
            let artifact = read_artifact(&self.root, &path)?;
            total += u64::from(artifact.bytes);
            require(total <= MAX_TOTAL_BYTES, "artifact set size limit")?;
            artifacts.push(artifact);
        }
        Ok(ArtifactManifest {
            schema_version: 1,
            revision_digest: self.plan.approval_digest()?,
            task_id: self.attempt.task_id.clone(),
            attempt_id: self.attempt.id.clone(),
            receipt_digest: digest(self.receipt)?,
            artifacts,
        })
    }
    /// Re-read the exact approved file set. Caller must still require an independent
    /// test report tied to this manifest, before recording verify/accept events.
    pub fn require_unchanged(&self, manifest: &ArtifactManifest) -> Result<(), String> {
        require(
            self.capture()? == *manifest,
            "artifact manifest changed or belongs to another attempt",
        )
    }
    pub fn require_verified(
        &self,
        manifest: &ArtifactManifest,
        report: &VerificationReport,
        trusted_verifier: &str,
    ) -> Result<(), String> {
        report.require_pass(self.plan, manifest, trusted_verifier)?;
        self.require_unchanged(manifest)
    }
}
