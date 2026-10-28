//! Trusted-host Git workspace preparation. This does not grant dispatch approval
//! or release workflow leases. An unfinished intent is reconciled, never recreated.
//! Git metadata is shared: a worktree is file separation, not an OS sandbox.
use crate::artifacts::{contains_path, root_path};
use crate::verifier::{process, PinnedCheck};
use cerulean_domain::execution::{
    artifact_path, digest, identifier, require, sha256, ValidationFile,
};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::windows::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

const MAX_FILES: usize = 4096;
const MAX_BYTES: usize = 256 * 1024 * 1024;
const MAX_BLOB: usize = process::OUTPUT_LIMIT;
mod merge;
mod overlay;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceRequest {
    pub project_id: String,
    pub revision_digest: String,
    pub task_id: String,
    pub attempt_id: String,
    pub repository: String,
    /// Full commit object ID, never a movable ref or a command-line option.
    pub base_commit: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceRecord {
    pub schema_version: u32,
    pub request: WorkspaceRequest,
    pub path: String,
    pub common_dir: String,
    pub files: Vec<TrackedFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay: Option<overlay::OverlayRecord>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrackedFile {
    pub path: String,
    pub mode: String,
    pub object_id: String,
    pub bytes: usize,
}

fn ordinary(path: &Path) -> Result<fs::Metadata, String> {
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    require(
        meta.file_attributes() & 0x400 == 0 && !meta.file_type().is_symlink(),
        "linked workspace path",
    )?;
    Ok(meta)
}
fn text(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| "non-Unicode workspace path".into())
}
fn git_path(path: &Path) -> Result<String, String> {
    // Git's argv path parser rejects Win32 verbatim prefixes, although
    // CreateProcess accepts them for cwd. Keep canonical paths in our records.
    let value = text(path)?;
    let normal = if let Some(unc) = value.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        value.strip_prefix(r"\\?\").unwrap_or(&value).to_owned()
    };
    Ok(normal.replace('\\', "/"))
}
fn oid(value: &str) -> Result<(), String> {
    require(
        matches!(value.len(), 40 | 64)
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "full Git commit/object ID required",
    )
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    f.write_all(bytes)
        .and_then(|_| f.sync_all())
        .map_err(|e| e.to_string())
}
fn load<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let meta = ordinary(path)?;
    require(
        meta.is_file() && meta.len() <= 4 * 1024 * 1024,
        "workspace metadata limit",
    )?;
    serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

fn acquire_writer(
    lock: &File,
    cancel: Option<&AtomicBool>,
    deadline: Instant,
    mut blocked: impl FnMut(),
) -> Result<(), String> {
    loop {
        if let Some(cancel) = cancel {
            require(
                !cancel.load(Ordering::Acquire),
                "workspace preparation cancelled while waiting for writer",
            )?;
        }
        match lock.try_lock_exclusive() {
            Ok(()) => return Ok(()),
            Err(error)
                if cancel.is_some()
                    && error.raw_os_error() == fs2::lock_contended_error().raw_os_error() =>
            {
                require(
                    Instant::now() < deadline,
                    "workspace writer wait deadline exceeded",
                )?;
                blocked();
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => return Err("workspace store already in use".into()),
        }
    }
}

pub struct GitWorkspaces {
    root: PathBuf,
    executable: PathBuf,
    executable_pin: ValidationFile,
    cancel: Arc<AtomicBool>,
    _lock: Option<File>,
}
impl GitWorkspaces {
    /// Read-only committed seed inspection for a material-bound experiment.
    /// Uses an existing pool and the same bounded/pinned Git reader as dispatch.
    pub(crate) fn inspect_committed_seed(
        root: &Path,
        git: &ValidationFile,
        repository: &Path,
        base: &str,
        expected: &crate::source_snapshot::Tree,
    ) -> Result<(), String> {
        use sha2::{Digest, Sha256};
        oid(base)?;
        let store = Self::open_inner(root, git, false, true, None)?;
        let deadline = Instant::now() + Duration::from_secs(30);
        require(
            store.directory(repository, "--show-toplevel", deadline)? == repository
                && store.directory(repository, "--git-common-dir", deadline)?
                    == repository.join(".git"),
            "baseline repository must own its Git metadata",
        )?;
        require(
            store.line(
                repository,
                &["rev-parse", &format!("{base}^{{commit}}")],
                deadline,
            )? == base,
            "baseline base is not a commit",
        )?;
        let tracked = store.tracked(repository, base, deadline)?;
        require(
            tracked.len() == expected.files.len(),
            "committed input inventory differs from seed",
        )?;
        for file in tracked {
            let seed = expected
                .files
                .iter()
                .find(|f| f.path == file.path)
                .ok_or("unexpected committed input")?;
            let data = store.git(repository, &["cat-file", "blob", &file.object_id], deadline)?;
            require(
                data.len() == seed.bytes as usize
                    && format!("{:x}", Sha256::digest(&data)) == seed.sha256,
                "committed raw blob differs from fixed seed",
            )?;
        }
        Ok(())
    }
    /// The host owns this directory and resolves the executable through its tool
    /// registry. No Git executable, root or request comes from a worker receipt.
    pub fn open(root: &Path, git: &ValidationFile) -> Result<Self, String> {
        Self::open_inner(root, git, true, true, None)
    }
    pub fn open_existing(root: &Path, git: &ValidationFile) -> Result<Self, String> {
        Self::open_inner(root, git, false, true, None)
    }
    fn inspect_ready(
        root: &Path,
        git: &ValidationFile,
        request: &WorkspaceRequest,
    ) -> Result<WorkspaceRecord, String> {
        // Ready records are immutable and identify one attempt. Inspecting them
        // cannot create, repair or mutate a different task's preparation.
        Self::open_inner(root, git, false, false, None)?.inspect(request)
    }
    fn open_inner(
        root: &Path,
        git: &ValidationFile,
        create: bool,
        exclusive: bool,
        wait: Option<Arc<AtomicBool>>,
    ) -> Result<Self, String> {
        let root = root_path(root.to_str().ok_or("non-Unicode workspace store")?)?;
        sha256(&git.sha256)?;
        let observed = PinnedCheck::describe(Path::new(&git.path), &[], Default::default())?;
        require(
            observed.executable == *git,
            "Git executable differs from pin",
        )?;
        for name in ["records", "trees", "temp", "hooks"] {
            let path = root.join(name);
            if !path.exists() {
                require(create, "workspace store is not prepared")?;
                if let Err(error) = fs::create_dir(&path) {
                    if error.kind() != std::io::ErrorKind::AlreadyExists {
                        return Err(error.to_string());
                    }
                }
            }
            require(
                ordinary(&path)?.is_dir(),
                "workspace storage directory invalid",
            )?;
        }
        let lock_path = root.join("writer.lock");
        require(create || lock_path.exists(), "workspace lock is missing")?;
        if lock_path.exists() {
            require(ordinary(&lock_path)?.is_file(), "workspace lock invalid")?;
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(exclusive)
            .create(create)
            .truncate(false)
            .open(lock_path)
            .map_err(|e| e.to_string())?;
        if exclusive {
            acquire_writer(
                &lock,
                wait.as_deref(),
                Instant::now() + Duration::from_secs(120),
                || {},
            )?;
        }
        Ok(Self {
            root,
            executable: PathBuf::from(&git.path),
            executable_pin: git.clone(),
            cancel: Arc::new(AtomicBool::new(false)),
            _lock: exclusive.then_some(lock),
        })
    }
    pub(crate) fn with_cancellation(mut self, cancel: Arc<AtomicBool>) -> Self {
        self.cancel = cancel;
        self
    }
    fn git(&self, cwd: &Path, args: &[&str], deadline: Instant) -> Result<Vec<u8>, String> {
        require(
            !self.cancel.load(Ordering::Acquire),
            "workspace preparation cancelled",
        )?;
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or("workspace operation deadline exceeded")?;
        // Pin the executable during every process launch, reusing the established
        // bounded Windows Job runner rather than adding a shell/process owner.
        let held = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&self.executable)
            .map_err(|e| e.to_string())?;
        let observed = PinnedCheck::describe(&self.executable, &[], Default::default())?;
        require(
            observed.executable == self.executable_pin,
            "Git executable changed",
        )?;
        let mut command = vec![
            "--no-optional-locks".into(),
            "-c".into(),
            "core.fsmonitor=false".into(),
            "-c".into(),
            format!("core.hooksPath={}", git_path(&self.root.join("hooks"))?),
            "-c".into(),
            "protocol.allow=never".into(),
        ];
        command.extend(args.iter().map(|s| (*s).to_owned()));
        let environment = [
            ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
            ("GIT_CONFIG_GLOBAL".into(), "NUL".into()),
            ("GIT_TERMINAL_PROMPT".into(), "0".into()),
            ("GIT_NO_REPLACE_OBJECTS".into(), "1".into()),
        ]
        .into();
        let outcome = process::run(
            &self.executable,
            &command,
            cwd,
            &self.root.join("temp"),
            remaining.min(Duration::from_secs(30)),
            &self.cancel,
            &environment,
        )?;
        drop(held);
        #[cfg(test)]
        if outcome.exit_code != 0 || outcome.timed_out || outcome.failure.is_some() {
            eprintln!(
                "Git fixture {:?}: exit={} failure={:?} stderr={}",
                args,
                outcome.exit_code,
                outcome.failure,
                String::from_utf8_lossy(&outcome.stderr)
            );
        }
        require(
            outcome.exit_code == 0 && !outcome.timed_out && outcome.failure.is_none(),
            "Git workspace operation failed; preserve intent and inspect before recovery",
        )?;
        Ok(outcome.stdout)
    }
    fn line(&self, cwd: &Path, args: &[&str], deadline: Instant) -> Result<String, String> {
        String::from_utf8(self.git(cwd, args, deadline)?)
            .map(|s| s.trim_end_matches(['\r', '\n']).to_owned())
            .map_err(|_| "non-Unicode Git metadata".into())
    }
    fn directory(&self, cwd: &Path, arg: &str, deadline: Instant) -> Result<PathBuf, String> {
        let value = self.line(cwd, &["rev-parse", "--path-format=absolute", arg], deadline)?;
        root_path(&value)
    }
    fn key(request: &WorkspaceRequest) -> Result<String, String> {
        for value in [&request.project_id, &request.task_id, &request.attempt_id] {
            identifier(value)?;
        }
        sha256(&request.revision_digest)?;
        oid(&request.base_commit)?;
        // Attempt IDs cannot be reused under a different base/revision/repository.
        cerulean_domain::execution::workspace_key(
            &request.project_id,
            &request.task_id,
            &request.attempt_id,
        )
    }
    fn tracked(
        &self,
        repository: &Path,
        base: &str,
        deadline: Instant,
    ) -> Result<Vec<TrackedFile>, String> {
        let output = self.git(
            repository,
            &["ls-tree", "-r", "-l", "-z", "--full-tree", base],
            deadline,
        )?;
        let mut files = Vec::new();
        let mut seen = BTreeSet::new();
        let mut total = 0usize;
        for entry in output.split(|b| *b == 0).filter(|e| !e.is_empty()) {
            let entry = std::str::from_utf8(entry).map_err(|_| "non-Unicode Git filename")?;
            let (metadata, path) = entry.split_once('\t').ok_or("invalid Git tree entry")?;
            let fields: Vec<_> = metadata.split_whitespace().collect();
            require(
                fields.len() == 4
                    && matches!(fields[0], "100644" | "100755")
                    && fields[1] == "blob",
                "worktree links/submodules require explicit support",
            )?;
            artifact_path(path)?;
            oid(fields[2])?;
            require(
                !path.split('/').any(|p| p.eq_ignore_ascii_case(".git")),
                "reserved Git metadata path",
            )?;
            require(seen.insert(path.to_lowercase()), "case-aliased Git tree")?;
            let bytes: usize = fields[3].parse().map_err(|_| "invalid Git blob length")?;
            total = total.checked_add(bytes).ok_or("Git tree size overflow")?;
            require(
                bytes <= MAX_BLOB && total <= MAX_BYTES && files.len() < MAX_FILES,
                "workspace input limit",
            )?;
            files.push(TrackedFile {
                path: path.into(),
                mode: fields[0].into(),
                object_id: fields[2].into(),
                bytes,
            });
        }
        Ok(files)
    }
    fn identity(&self, record: &WorkspaceRecord, deadline: Instant) -> Result<PathBuf, String> {
        require(record.schema_version == 1, "workspace record version")?;
        let key = Self::key(&record.request)?;
        let expected = self.root.join("trees").join(key);
        require(
            record.path == text(&expected)?,
            "workspace destination changed",
        )?;
        let path = root_path(&record.path)?;
        require(
            path == expected && ordinary(&path.join(".git"))?.is_file(),
            "worktree identity path changed",
        )?;
        let repo = root_path(&record.request.repository)?;
        let common = self.directory(&repo, "--git-common-dir", deadline)?;
        require(
            text(&common)? == record.common_dir
                && self.directory(&path, "--git-common-dir", deadline)? == common,
            "worktree repository identity changed",
        )?;
        require(
            self.directory(&path, "--show-toplevel", deadline)? == path
                && self.line(&path, &["rev-parse", "HEAD"], deadline)?
                    == record.request.base_commit,
            "worktree HEAD or root changed",
        )?;
        // Check the reciprocal Git registration, not just a forgeable .git file.
        let admin = self.directory(&path, "--git-dir", deadline)?;
        require(
            contains_path(&common.join("worktrees"), &admin),
            "not a linked worktree",
        )?;
        require(
            ordinary(&admin.join("HEAD"))?.is_file(),
            "worktree HEAD metadata missing",
        )?;
        require(
            fs::read_to_string(admin.join("HEAD"))
                .map_err(|e| e.to_string())?
                .trim_end()
                == record.request.base_commit,
            "worktree must remain detached from shared branches",
        )?;
        require(
            ordinary(&admin.join("gitdir"))?.is_file(),
            "worktree backlink missing",
        )?;
        let backlink = fs::read_to_string(admin.join("gitdir")).map_err(|e| e.to_string())?;
        require(
            fs::canonicalize(backlink.trim_end()).map_err(|e| e.to_string())?
                == fs::canonicalize(path.join(".git")).map_err(|e| e.to_string())?,
            "worktree backlink changed",
        )?;
        Ok(path)
    }
    /// Persists intent before `worktree add`. Calls for an existing intent only
    /// reconcile that exact worktree; missing/partial output is never recreated.
    pub fn prepare(&self, request: &WorkspaceRequest) -> Result<WorkspaceRecord, String> {
        require(
            self._lock.is_some(),
            "read-only workspace view cannot prepare",
        )?;
        require(
            !self.cancel.load(Ordering::Acquire),
            "workspace preparation cancelled",
        )?;
        let deadline = Instant::now() + Duration::from_secs(120);
        let key = Self::key(request)?;
        let intent = self.root.join("records").join(format!("{key}.intent.json"));
        if intent.exists() {
            let record: WorkspaceRecord = load(&intent)?;
            require(
                record.request == *request,
                "attempt workspace request changed",
            )?;
            return self.recover(&record, deadline);
        }
        let repository = root_path(&request.repository)?;
        require(
            !contains_path(&repository, &self.root) && !contains_path(&self.root, &repository),
            "workspace store and source must be separate",
        )?;
        require(
            self.directory(&repository, "--show-toplevel", deadline)? == repository,
            "repository root required",
        )?;
        let resolved = self.line(
            &repository,
            &[
                "rev-parse",
                "--verify",
                &format!("{}^{{commit}}", request.base_commit),
            ],
            deadline,
        )?;
        require(
            resolved == request.base_commit,
            "base is not an exact commit",
        )?;
        let path = self.root.join("trees").join(key);
        require(!path.exists(), "unowned worktree destination exists")?;
        let record = WorkspaceRecord {
            schema_version: 1,
            request: request.clone(),
            path: text(&path)?,
            common_dir: text(&self.directory(&repository, "--git-common-dir", deadline)?)?,
            files: self.tracked(&repository, &request.base_commit, deadline)?,
            overlay: None,
        };
        let bytes = serde_json::to_vec(&record).map_err(|e| e.to_string())?;
        require(bytes.len() <= 4 * 1024 * 1024, "workspace manifest limit")?;
        write_new(&intent, &bytes)?;
        self.git(
            &repository,
            &[
                "worktree",
                "add",
                "--detach",
                "--no-checkout",
                &git_path(&path)?,
                &request.base_commit,
            ],
            deadline,
        )?;
        self.identity(&record, deadline)?;
        // Populate the index without checkout hooks or attribute filters. Write
        // exact approved blobs ourselves, avoiding repository-provided programs.
        self.git(&path, &["read-tree", &request.base_commit], deadline)?;
        for entry in &record.files {
            let bytes = self.git(
                &repository,
                &["cat-file", "blob", &entry.object_id],
                deadline,
            )?;
            require(bytes.len() == entry.bytes, "blob length changed")?;
            let destination = path.join(&entry.path);
            let parent = destination.parent().ok_or("missing file parent")?;
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            require(
                contains_path(&path, &root_path(&text(parent)?)?),
                "file parent escaped worktree",
            )?;
            write_new(&destination, &bytes)?;
        }
        self.recover(&record, deadline)
    }
    /// Inspect an already-ready workspace without creating or repairing records.
    pub fn inspect(&self, request: &WorkspaceRequest) -> Result<WorkspaceRecord, String> {
        let key = Self::key(request)?;
        let record: WorkspaceRecord =
            load(&self.root.join("records").join(format!("{key}.intent.json")))?;
        require(record.request == *request, "workspace request changed")?;
        let ready: WorkspaceRecord =
            load(&self.root.join("records").join(format!("{key}.ready.json")))?;
        require(record == ready, "workspace readiness differs from intent")?;
        self.identity(&record, Instant::now() + Duration::from_secs(30))?;
        Ok(record)
    }
    fn recover(
        &self,
        record: &WorkspaceRecord,
        deadline: Instant,
    ) -> Result<WorkspaceRecord, String> {
        require(
            self._lock.is_some(),
            "read-only workspace view cannot recover",
        )?;
        let path = self.identity(record, deadline)?;
        let key = Self::key(&record.request)?;
        let ready = self.root.join("records").join(format!("{key}.ready.json"));
        if ready.exists() {
            require(
                load::<WorkspaceRecord>(&ready)? == *record,
                "ready workspace record changed",
            )?;
            return Ok(record.clone()); // Worker edits are preserved after readiness.
        }
        require(
            self.tracked(&path, &record.request.base_commit, deadline)? == record.files,
            "workspace input manifest changed",
        )?;
        let index = self.git(&path, &["ls-files", "--stage", "-z"], deadline)?;
        let expected_index: Vec<u8> = record
            .files
            .iter()
            .flat_map(|f| format!("{} {} 0\t{}\0", f.mode, f.object_id, f.path).into_bytes())
            .collect();
        require(
            index == expected_index,
            "unfinished workspace index differs from base",
        )?;
        if let Some(overlay) = &record.overlay {
            merge::validate_tree(&overlay.tree)?;
            require(
                crate::source_snapshot::observe(&path, &overlay::tree_policy())? == overlay.tree,
                "unfinished dependency worktree differs from approved inputs; do not recreate",
            )?;
            require(
                !self.cancel.load(Ordering::Acquire) && Instant::now() < deadline,
                "dependency preparation cancelled or timed out before ready",
            )?;
            write_new(
                &ready,
                &serde_json::to_vec(record).map_err(|e| e.to_string())?,
            )?;
            return Ok(record.clone());
        }
        let mut pending = vec![path.clone()];
        let mut found = BTreeSet::new();
        while let Some(directory) = pending.pop() {
            require(
                Instant::now() < deadline,
                "workspace recovery deadline exceeded",
            )?;
            for entry in fs::read_dir(&directory).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                let absolute = entry.path();
                if absolute == path.join(".git") {
                    continue;
                }
                let relative = absolute
                    .strip_prefix(&path)
                    .map_err(|_| "workspace entry escaped")?
                    .to_str()
                    .ok_or("non-Unicode workspace entry")?
                    .replace('\\', "/");
                require(
                    relative.split('/').count() <= 64,
                    "workspace directory depth limit",
                )?;
                let metadata = ordinary(&absolute)?;
                if metadata.is_dir() {
                    require(
                        record
                            .files
                            .iter()
                            .any(|f| f.path.starts_with(&(relative.clone() + "/"))),
                        "unexpected directory in unfinished worktree",
                    )?;
                    pending.push(absolute);
                } else {
                    require(
                        metadata.is_file()
                            && found.len() < MAX_FILES
                            && record.files.iter().any(|f| f.path == relative),
                        "unexpected file in unfinished worktree",
                    )?;
                    found.insert(relative);
                }
            }
        }
        require(
            found.len() == record.files.len(),
            "unfinished workspace files missing",
        )?;
        // Interrupted checkout can be adopted only when every expected byte is
        // present. Partial or modified trees remain unresolved; no reset/overwrite.
        for entry in &record.files {
            let file = path.join(&entry.path);
            require(
                ordinary(&file)?.is_file(),
                "unfinished workspace file missing",
            )?;
            require(
                root_path(&text(file.parent().ok_or("file parent missing")?)?)?.starts_with(&path),
                "workspace ancestor changed",
            )?;
            let expected = self.git(&path, &["cat-file", "blob", &entry.object_id], deadline)?;
            let mut actual = Vec::new();
            File::open(&file)
                .map_err(|e| e.to_string())?
                .take((MAX_BLOB + 1) as u64)
                .read_to_end(&mut actual)
                .map_err(|e| e.to_string())?;
            require(
                actual == expected,
                "unfinished worktree was modified; do not recreate",
            )?;
        }
        write_new(
            &ready,
            &serde_json::to_vec(record).map_err(|e| e.to_string())?,
        )?;
        Ok(record.clone())
    }
}

fn approved_request(
    plan: &cerulean_domain::execution::PlanRevision,
    task: &str,
    attempt: &str,
) -> Result<WorkspaceRequest, String> {
    plan.validate()?;
    plan.task(task)?;
    identifier(attempt)?;
    let policy = plan
        .worktree
        .as_ref()
        .ok_or("approved worktree policy required")?;
    require(
        text(&root_path(&policy.pool_root)?)? == policy.pool_root,
        "worktree pool must use its canonical registered path",
    )?;
    Ok(WorkspaceRequest {
        project_id: plan.project.id.clone(),
        revision_digest: plan.approval_digest()?,
        task_id: task.into(),
        attempt_id: attempt.into(),
        repository: plan.project.root.clone(),
        base_commit: policy.base_commit.clone(),
    })
}
pub(crate) fn prepare_approved_cancellable(
    plan: &cerulean_domain::execution::PlanRevision,
    task: &str,
    attempt: &str,
    cancel: Arc<AtomicBool>,
    inputs: &mut [crate::scheduler::AcceptedDependencyInput],
) -> Result<(String, cerulean_domain::execution::WorkspaceProof), String> {
    require(
        !cancel.load(Ordering::Acquire),
        "workspace preparation cancelled",
    )?;
    let request = approved_request(plan, task, attempt)?;
    let policy = plan.worktree.as_ref().ok_or("worktree policy missing")?;
    let store = GitWorkspaces::open_inner(
        Path::new(&policy.pool_root),
        &policy.git,
        true,
        true,
        Some(cancel.clone()),
    )?
    .with_cancellation(cancel);
    let dependencies = &plan
        .workflow
        .tasks
        .iter()
        .find(|t| t.id == task)
        .ok_or("task missing")?
        .dependencies;
    let record = if dependencies.is_empty() {
        require(inputs.is_empty(), "unexpected workspace dependencies")?;
        store.prepare(&request)?
    } else {
        require(policy.input == cerulean_domain::execution::GitWorktreeInput::CommittedRawBlobsWithAcceptedDependencies,
            "dependency overlay not approved")?;
        store.prepare_overlay(&request, plan, inputs)?
    };
    require(
        record.path == plan.execution_root(task, attempt)?,
        "prepared workspace path mismatch",
    )?;
    Ok((
        record.path.clone(),
        cerulean_domain::execution::WorkspaceProof {
            record_digest: digest(&record)?,
        },
    ))
}
pub(crate) fn require_prepared(
    plan: &cerulean_domain::execution::PlanRevision,
    attempt: &cerulean_domain::execution::Attempt,
) -> Result<(), String> {
    plan.validate_attempt_workspace(attempt)?;
    let Some(policy) = &plan.worktree else {
        return Ok(());
    };
    let request = approved_request(plan, &attempt.task_id, &attempt.id)?;
    let record = GitWorkspaces::inspect_ready(Path::new(&policy.pool_root), &policy.git, &request)?;
    let definition = plan
        .workflow
        .tasks
        .iter()
        .find(|t| t.id == attempt.task_id)
        .ok_or("workspace task missing")?;
    if definition.dependencies.is_empty() {
        require(
            record.overlay.is_none(),
            "root task has unexpected dependency inputs",
        )?;
    } else {
        require(policy.input == cerulean_domain::execution::GitWorktreeInput::CommittedRawBlobsWithAcceptedDependencies, "dependency input mode not approved")?;
        let overlay = record
            .overlay
            .as_ref()
            .ok_or("dependent attempt has no prepared input overlay")?;
        let mut expected = BTreeSet::new();
        let mut pending = definition.dependencies.clone();
        while let Some(id) = pending.pop() {
            if expected.insert(id.clone()) {
                let task = plan
                    .workflow
                    .tasks
                    .iter()
                    .find(|t| t.id == id)
                    .ok_or("dependency missing")?;
                pending.extend(task.dependencies.iter().cloned());
            }
        }
        require(
            overlay.inputs.len() == expected.len()
                && overlay
                    .inputs
                    .iter()
                    .map(|i| i.task_id.clone())
                    .collect::<BTreeSet<_>>()
                    == expected,
            "prepared dependency input set differs from approved DAG",
        )?;
        for input in &overlay.inputs {
            require(
                input.schema_version == 1
                    && input.project_id == plan.project.id
                    && input.revision_digest == plan.approval_digest()?,
                "prepared dependency identity differs",
            )?;
            sha256(&input.workspace_record_digest)?;
        }
        merge::validate_tree(&overlay.tree)?;
    }
    require(
        record.path == attempt.binding.workspace
            && attempt
                .workspace_proof
                .as_ref()
                .is_some_and(|p| digest(&record).is_ok_and(|d| d == p.record_digest)),
        "workspace preparation proof differs from original record",
    )
}

#[cfg(test)]
mod tests;
