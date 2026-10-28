use super::*;
use crate::{
    scheduler::{AcceptedDependencyInput, AcceptedInputBinding},
    source_snapshot::Tree,
};
use cerulean_domain::execution::{Artifact, PlanRevision, SnapshotPolicy};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OverlayRecord {
    pub inputs: Vec<AcceptedInputBinding>,
    pub tree: Tree,
}
pub(super) fn tree_policy() -> SnapshotPolicy {
    SnapshotPolicy {
        exclude: vec![".git".into()],
        max_files: MAX_FILES as u32,
        max_bytes: MAX_BYTES as u32,
    }
}
fn project_tree(tree: &Tree, policy: &SnapshotPolicy) -> Tree {
    Tree {
        directories: tree
            .directories
            .iter()
            .filter(|p| !policy.excludes(p))
            .cloned()
            .collect(),
        files: tree
            .files
            .iter()
            .filter(|f| !policy.excludes(&f.path))
            .cloned()
            .collect(),
    }
}
fn add_parents(dirs: &mut BTreeSet<String>, path: &str) {
    let mut parent = path;
    while let Some((p, _)) = parent.rsplit_once('/') {
        dirs.insert(p.into());
        parent = p;
    }
}
impl GitWorkspaces {
    pub(super) fn prepare_overlay(
        &self,
        request: &WorkspaceRequest,
        plan: &PlanRevision,
        inputs: &mut [AcceptedDependencyInput],
    ) -> Result<WorkspaceRecord, String> {
        require(
            self._lock.is_some(),
            "read-only workspace view cannot prepare overlay",
        )?;
        let deadline = Instant::now() + Duration::from_secs(120);
        let key = Self::key(request)?;
        let intent = self.root.join("records").join(format!("{key}.intent.json"));
        let bindings: Vec<_> = inputs.iter().map(|i| i.binding().clone()).collect();
        require(!bindings.is_empty(), "dependency overlay has no inputs")?;
        if intent.exists() {
            let record: WorkspaceRecord = load(&intent)?;
            require(
                record.request == *request
                    && record
                        .overlay
                        .as_ref()
                        .is_some_and(|o| o.inputs == bindings),
                "dependency workspace inputs changed",
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
        require(
            self.line(
                &repository,
                &[
                    "rev-parse",
                    "--verify",
                    &format!("{}^{{commit}}", request.base_commit),
                ],
                deadline,
            )? == request.base_commit,
            "exact base commit required",
        )?;
        let tracked = self.tracked(&repository, &request.base_commit, deadline)?;
        let mut blobs = BTreeMap::new();
        let mut base = Tree {
            directories: vec![],
            files: vec![],
        };
        let mut dirs = BTreeSet::new();
        for entry in &tracked {
            let bytes = self.git(
                &repository,
                &["cat-file", "blob", &entry.object_id],
                deadline,
            )?;
            require(bytes.len() == entry.bytes, "base blob length changed")?;
            base.files.push(Artifact {
                path: entry.path.clone(),
                bytes: bytes.len() as u32,
                sha256: format!("{:x}", Sha256::digest(&bytes)),
            });
            add_parents(&mut dirs, &entry.path);
            blobs.insert(entry.path.clone(), bytes);
        }
        base.files.sort_by(|a, b| a.path.cmp(&b.path));
        base.directories = dirs.into_iter().collect();
        let policy = plan
            .snapshot_policy
            .as_ref()
            .ok_or("snapshot policy missing")?;
        let mut trees = vec![];
        for input in inputs.iter() {
            let binding = input.binding();
            require(
                binding.project_id == request.project_id
                    && binding.revision_digest == request.revision_digest,
                "dependency project/revision mismatch",
            )?;
            let original = approved_request(plan, &binding.task_id, &binding.attempt_id)?;
            let original_key = Self::key(&original)?;
            let before: WorkspaceRecord = load(
                &self
                    .root
                    .join("records")
                    .join(format!("{original_key}.ready.json")),
            )?;
            let before_intent: WorkspaceRecord = load(
                &self
                    .root
                    .join("records")
                    .join(format!("{original_key}.intent.json")),
            )?;
            require(
                before == before_intent
                    && before.request == original
                    && digest(&before)? == binding.workspace_record_digest
                    && before.files == tracked,
                "dependency initial tree proof mismatch",
            )?;
            let initial = before.overlay.as_ref().map_or(&base, |o| &o.tree);
            merge::validate_tree(initial)?;
            trees.push(merge::InputTree {
                task: binding.task_id.clone(),
                before: project_tree(initial, policy),
                after: input.tree().clone(),
            });
        }
        let mut merged = merge::compose(
            &plan.workflow.tasks,
            &request.task_id,
            &project_tree(&base, policy),
            &trees,
        )?;
        merged.files.extend(
            base.files
                .iter()
                .filter(|f| policy.excludes(&f.path))
                .cloned(),
        );
        let mut dirs: BTreeSet<_> = merged.directories.into_iter().collect();
        dirs.extend(
            base.directories
                .iter()
                .filter(|d| policy.excludes(d))
                .cloned(),
        );
        for file in &merged.files {
            add_parents(&mut dirs, &file.path);
        }
        merged.directories = dirs.into_iter().collect();
        merged.files.sort_by(|a, b| a.path.cmp(&b.path));
        merge::validate_tree(&merged)?;
        let path = self.root.join("trees").join(key);
        require(!path.exists(), "unowned dependency workspace exists")?;
        let record = WorkspaceRecord {
            schema_version: 1,
            request: request.clone(),
            path: text(&path)?,
            common_dir: text(&self.directory(&repository, "--git-common-dir", deadline)?)?,
            files: tracked,
            overlay: Some(OverlayRecord {
                inputs: bindings,
                tree: merged.clone(),
            }),
        };
        let encoded = serde_json::to_vec(&record).map_err(|e| e.to_string())?;
        require(encoded.len() <= 4 * 1024 * 1024, "workspace manifest limit")?;
        write_new(&intent, &encoded)?;
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
        self.git(&path, &["read-tree", &request.base_commit], deadline)?;
        for directory in &merged.directories {
            fs::create_dir_all(path.join(directory)).map_err(|e| e.to_string())?;
            require(
                contains_path(&path, &root_path(&text(&path.join(directory))?)?),
                "overlay directory escaped",
            )?;
        }
        for artifact in &merged.files {
            require(
                !self.cancel.load(Ordering::Acquire) && Instant::now() < deadline,
                "dependency workspace preparation cancelled or timed out",
            )?;
            let bytes = if base.files.iter().any(|f| f == artifact) {
                blobs
                    .get(&artifact.path)
                    .ok_or("base bytes missing")?
                    .clone()
            } else {
                let source = inputs
                    .iter_mut()
                    .find(|i| i.tree().files.contains(artifact))
                    .ok_or("sealed input bytes missing")?;
                source.read_file(&artifact.path)?
            };
            require(
                bytes.len() == artifact.bytes as usize
                    && format!("{:x}", Sha256::digest(&bytes)) == artifact.sha256,
                "dependency copy differs",
            )?;
            let destination = path.join(&artifact.path);
            require(
                contains_path(
                    &path,
                    &root_path(&text(
                        destination.parent().ok_or("overlay parent missing")?,
                    )?)?,
                ),
                "overlay file escaped",
            )?;
            write_new(&destination, &bytes)?;
        }
        self.recover(&record, deadline)
    }
}
