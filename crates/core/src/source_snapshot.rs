//! Full approved input tree, sealed copy, isolated check workspace and live
//! comparison. Hashes are evidence; Windows file handles enforce write exclusion
//! while checks run. This is not an OS sandbox for arbitrary same-user code.
use crate::artifacts::{read_artifact, root_path};
use cerulean_domain::execution::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Tree {
    pub directories: Vec<String>,
    pub files: Vec<Artifact>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SnapshotManifest {
    pub schema_version: u32,
    pub revision_digest: String,
    pub task_id: String,
    pub attempt_id: String,
    pub artifact_manifest_digest: String,
    pub tree: Tree,
}
fn ordinary(path: &Path) -> Result<fs::Metadata, String> {
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    require(
        !meta.file_type().is_symlink() && meta.file_attributes() & 0x400 == 0,
        "snapshot link/reparse path refused",
    )?;
    Ok(meta)
}
fn names(root: &Path, policy: &SnapshotPolicy) -> Result<(Vec<String>, Vec<String>), String> {
    let mut files = vec![];
    let mut dirs = vec![];
    let mut pending = vec![(root.to_path_buf(), String::new())];
    let mut seen = BTreeSet::new();
    let mut total = 0;
    while let Some((absolute, relative)) = pending.pop() {
        for entry in fs::read_dir(absolute).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| "non-Unicode snapshot filename")?;
            let path = if relative.is_empty() {
                name
            } else {
                format!("{relative}/{name}")
            };
            artifact_path(&path)?;
            require(
                path.split('/').count() <= 64,
                "snapshot directory depth limit",
            )?;
            if policy.excludes(&path) {
                continue;
            }
            require(seen.insert(path.to_lowercase()), "snapshot case alias")?;
            require(seen.len() <= 20000, "snapshot entry limit")?;
            let meta = ordinary(&entry.path())?;
            if meta.is_dir() {
                pending.push((entry.path(), path.clone()));
                dirs.push(path);
            } else {
                require(meta.is_file(), "unsupported snapshot file type")?;
                total += meta.len();
                require(total <= u64::from(policy.max_bytes), "snapshot byte limit")?;
                files.push(path);
                require(
                    files.len() <= policy.max_files as usize,
                    "snapshot file limit",
                )?;
            }
        }
    }
    files.sort();
    dirs.sort();
    Ok((dirs, files))
}
pub fn observe(root: &Path, policy: &SnapshotPolicy) -> Result<Tree, String> {
    policy.validate()?;
    let root = root_path(root.to_str().ok_or("non-Unicode snapshot root")?)?;
    let (directories, names) = names(&root, policy)?;
    let mut files = vec![];
    let mut total = 0u64;
    for path in &names {
        let artifact = read_artifact(&root, path)?;
        total += u64::from(artifact.bytes);
        require(total <= u64::from(policy.max_bytes), "snapshot byte limit")?;
        files.push(artifact);
    }
    let after = self::names(&root, policy)?;
    require(
        after == (directories.clone(), names),
        "snapshot tree changed during observation",
    )?;
    Ok(Tree { directories, files })
}
fn locked(path: &Path) -> Result<File, String> {
    require(ordinary(path)?.is_file(), "snapshot input not a file")?;
    OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(path)
        .map_err(|e| format!("snapshot input lock: {e}"))
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())
}
fn clone_tree(source: &Path, dest: &Path, tree: &Tree) -> Result<Vec<File>, String> {
    fs::create_dir(dest).map_err(|e| e.to_string())?;
    for directory in &tree.directories {
        fs::create_dir_all(dest.join(directory)).map_err(|e| e.to_string())?;
    }
    let mut locks = vec![];
    for artifact in &tree.files {
        let target = dest.join(&artifact.path);
        let mut input = locked(&source.join(&artifact.path))?;
        let mut output = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|e| e.to_string())?;
        std::io::copy(&mut input, &mut output).map_err(|e| e.to_string())?;
        output.sync_all().map_err(|e| e.to_string())?;
        drop(output);
        require(
            read_artifact(dest, &artifact.path)? == *artifact,
            "snapshot copy differs",
        )?;
        locks.push(locked(&target)?);
    }
    Ok(locks)
}
pub struct SnapshotLease {
    pub seal: SourceSnapshot,
    pub work: PathBuf,
    manifest: SnapshotManifest,
    original: PathBuf,
    frozen: PathBuf,
    policy: SnapshotPolicy,
    _locks: Vec<File>,
}

/// Accepted downstream inputs are read from the sealed copy, never from the
/// mutable worker directory. The workflow must establish acceptance first.
pub struct FrozenSnapshot {
    manifest: SnapshotManifest,
    files: BTreeMap<String, File>,
    _manifest_lock: File,
}
impl FrozenSnapshot {
    pub fn manifest(&self) -> &SnapshotManifest {
        &self.manifest
    }

    pub fn read_file(&mut self, path: &str) -> Result<Vec<u8>, String> {
        use sha2::{Digest, Sha256};
        let artifact = self
            .manifest
            .tree
            .files
            .iter()
            .find(|a| a.path == path)
            .ok_or("file not in accepted input snapshot")?;
        let file = self.files.get_mut(path).ok_or("input file lock missing")?;
        file.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        file.take(u64::from(artifact.bytes) + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        require(
            bytes.len() == artifact.bytes as usize
                && format!("{:x}", Sha256::digest(&bytes)) == artifact.sha256,
            "accepted input bytes changed",
        )?;
        Ok(bytes)
    }
}

pub(crate) fn open_frozen(
    plan: &PlanRevision,
    artifacts: &ArtifactManifest,
    seal: &SourceSnapshot,
    evidence_root: &Path,
    remaining_files: &mut usize,
    remaining_bytes: &mut u64,
) -> Result<FrozenSnapshot, String> {
    identifier(&seal.operation_id)?;
    sha256(&seal.manifest_sha256)?;
    let policy = plan
        .snapshot_policy
        .as_ref()
        .ok_or("snapshot policy missing")?;
    let base = root_path(evidence_root.to_str().ok_or("invalid evidence root")?)?;
    let dir = base.join(&seal.operation_id);
    root_path(dir.to_str().ok_or("invalid snapshot directory")?)?;
    let mut file = locked(&dir.join("snapshot.json"))?;
    require(
        file.metadata().map_err(|e| e.to_string())?.len() <= 8 * 1024 * 1024,
        "snapshot manifest limit",
    )?;
    let mut bytes = vec![];
    (&mut file)
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    require(bytes.len() <= 8 * 1024 * 1024, "snapshot manifest grew")?;
    let manifest: SnapshotManifest = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    require(
        digest(&manifest)? == seal.manifest_sha256
            && manifest.schema_version == 1
            && manifest.revision_digest == plan.approval_digest()?
            && manifest.task_id == artifacts.task_id
            && manifest.attempt_id == artifacts.attempt_id
            && manifest.artifact_manifest_digest == digest(artifacts)?,
        "snapshot binding mismatch",
    )?;
    let input_bytes: u64 = manifest.tree.files.iter().map(|a| u64::from(a.bytes)).sum();
    let next_files = remaining_files
        .checked_sub(manifest.tree.files.len())
        .ok_or("dependency input file budget exceeded")?;
    let next_bytes = remaining_bytes
        .checked_sub(input_bytes)
        .ok_or("dependency input byte budget exceeded")?;
    let frozen = root_path(dir.join("inputs").to_str().ok_or("invalid frozen path")?)?;
    require(
        observe(&frozen, policy)? == manifest.tree,
        "sealed dependency inputs changed",
    )?;
    let files = manifest
        .tree
        .files
        .iter()
        .map(|a| Ok((a.path.clone(), locked(&frozen.join(&a.path))?)))
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    require(
        observe(&frozen, policy)? == manifest.tree,
        "sealed inputs changed before locking",
    )?;
    for artifact in &artifacts.artifacts {
        require(
            manifest.tree.files.contains(artifact),
            "artifact missing from sealed inputs",
        )?;
    }
    *remaining_files = next_files;
    *remaining_bytes = next_bytes;
    Ok(FrozenSnapshot {
        manifest,
        files,
        _manifest_lock: file,
    })
}
impl SnapshotLease {
    pub fn create(
        plan: &PlanRevision,
        artifacts: &ArtifactManifest,
        dir: &Path,
        operation_id: &str,
    ) -> Result<Self, String> {
        identifier(operation_id)?;
        let policy = plan
            .snapshot_policy
            .as_ref()
            .ok_or("approved snapshot policy required")?
            .clone();
        let original = root_path(&plan.execution_root(&artifacts.task_id, &artifacts.attempt_id)?)?;
        require(
            !crate::artifacts::contains_path(&original, dir),
            "snapshot must be outside original project",
        )?;
        let tree = observe(&original, &policy)?;
        // Freeze all existing input files together during capture. The second
        // complete observation also rejects additions/deletions while copying.
        let source_locks: Vec<_> = tree
            .files
            .iter()
            .map(|f| locked(&original.join(&f.path)))
            .collect::<Result<_, _>>()?;
        require(
            observe(&original, &policy)? == tree,
            "source changed before capture",
        )?;
        for artifact in &artifacts.artifacts {
            require(
                tree.files.iter().any(|f| f == artifact),
                "submitted artifact differs from snapshot",
            )?;
        }
        let frozen = dir.join("inputs");
        let work = dir.join("work");
        let mut locks = clone_tree(&original, &frozen, &tree)?;
        locks.extend(clone_tree(&frozen, &work, &tree)?);
        require(
            observe(&original, &policy)? == tree,
            "source changed during capture",
        )?;
        drop(source_locks);
        let manifest = SnapshotManifest {
            schema_version: 1,
            revision_digest: plan.approval_digest()?,
            task_id: artifacts.task_id.clone(),
            attempt_id: artifacts.attempt_id.clone(),
            artifact_manifest_digest: digest(artifacts)?,
            tree,
        };
        let seal = SourceSnapshot {
            operation_id: operation_id.into(),
            manifest_sha256: digest(&manifest)?,
        };
        let bytes = serde_json::to_vec(&manifest).map_err(|e| e.to_string())?;
        require(bytes.len() <= 8 * 1024 * 1024, "snapshot manifest limit")?;
        write_new(&dir.join("snapshot.json"), &bytes)?;
        Ok(Self {
            seal,
            work,
            manifest,
            original,
            frozen,
            policy,
            _locks: locks,
        })
    }
    pub fn require_unchanged(&self) -> Result<(), String> {
        for root in [&self.original, &self.frozen, &self.work] {
            require(
                observe(root, &self.policy)? == self.manifest.tree,
                "full verification input tree changed",
            )?;
        }
        Ok(())
    }
}
/// Live verify/accept gate, also reopens saved snapshot files after service restart.
/// Historical replay validates journal structure without recertifying today's tree.
pub fn require_sealed(
    plan: &PlanRevision,
    artifacts: &ArtifactManifest,
    seal: &SourceSnapshot,
    evidence_root: &Path,
) -> Result<(), String> {
    identifier(&seal.operation_id)?;
    sha256(&seal.manifest_sha256)?;
    let policy = plan
        .snapshot_policy
        .as_ref()
        .ok_or("snapshot policy missing")?;
    let base = root_path(evidence_root.to_str().ok_or("invalid evidence root")?)?;
    let dir = base.join(&seal.operation_id);
    root_path(dir.to_str().ok_or("invalid snapshot directory")?)?;
    let path = dir.join("snapshot.json");
    let mut file = locked(&path)?;
    require(
        file.metadata().map_err(|e| e.to_string())?.len() <= 8 * 1024 * 1024,
        "snapshot manifest limit",
    )?;
    let mut bytes = vec![];
    file.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    let manifest: SnapshotManifest = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    require(
        digest(&manifest)? == seal.manifest_sha256
            && manifest.schema_version == 1
            && manifest.revision_digest == plan.approval_digest()?
            && manifest.task_id == artifacts.task_id
            && manifest.attempt_id == artifacts.attempt_id
            && manifest.artifact_manifest_digest == digest(artifacts)?,
        "snapshot binding mismatch",
    )?;
    for root in [
        PathBuf::from(plan.execution_root(&artifacts.task_id, &artifacts.attempt_id)?),
        dir.join("inputs"),
        dir.join("work"),
    ] {
        require(
            observe(&root, policy)? == manifest.tree,
            "sealed verification inputs changed",
        )?;
    }
    Ok(())
}
