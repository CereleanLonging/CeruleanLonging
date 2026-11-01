//! Resolve approved dependency identities from the journal before reading a
//! sealed verification snapshot. No current worker files or model claims enter
//! the bundle, and merely reading inputs never prepares or dispatches a task.
use super::*;
use cerulean_domain::execution::{digest, SourceSnapshot};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptedInputBinding {
    pub schema_version: u32,
    pub project_id: String,
    pub revision_digest: String,
    pub task_id: String,
    pub attempt_id: String,
    pub acceptance_event_hash: String,
    pub receipt_digest: String,
    pub artifact_manifest_digest: String,
    pub verification_report_digest: String,
    pub workspace_record_digest: String,
    pub snapshot: SourceSnapshot,
}

pub struct AcceptedDependencyInput {
    binding: AcceptedInputBinding,
    snapshot: crate::source_snapshot::FrozenSnapshot,
}
impl AcceptedDependencyInput {
    pub fn binding(&self) -> &AcceptedInputBinding {
        &self.binding
    }
    pub fn tree(&self) -> &crate::source_snapshot::Tree {
        &self.snapshot.manifest().tree
    }
    pub fn read_file(&mut self, path: &str) -> Result<Vec<u8>, String> {
        self.snapshot.read_file(path)
    }
}

impl DurableWorkflow {
    /// Trusted host input collection. Caller must retain these locks while
    /// staging bytes and bind all identities into the future preparation record.
    /// Reading alone does not prepare a directory or authorize dispatch.
    pub fn accepted_dependency_inputs(
        &self,
        task: &str,
    ) -> Result<Vec<AcceptedDependencyInput>, String> {
        self.collect_dependency_inputs(task, false)
    }
    pub(crate) fn accepted_ancestor_inputs(
        &self,
        task: &str,
    ) -> Result<Vec<AcceptedDependencyInput>, String> {
        self.collect_dependency_inputs(task, true)
    }
    fn collect_dependency_inputs(
        &self,
        task: &str,
        transitive: bool,
    ) -> Result<Vec<AcceptedDependencyInput>, String> {
        require(!self.poisoned, "workflow requires reopen")?;
        require(
            self.state().revision_requested.is_none(),
            "dependency inputs paused for revision",
        )?;
        let plan = self.revision().ok_or("plan revision missing")?;
        require(
            plan.worktree.is_some(),
            "dependency snapshots require worktree plan",
        )?;
        let scope = self
            .config
            .execution_scope
            .as_ref()
            .ok_or("execution scope missing")?;
        self.approval().ok_or("plan not approved")?.validate(
            plan,
            self.role_id(Role::User)?,
            scope.policy_version,
        )?;
        let run = self.state().tasks.get(task).ok_or("task missing")?;
        require(
            run.state == TaskState::Pending,
            "dependency inputs require pending target",
        )?;
        let definition = plan
            .workflow
            .tasks
            .iter()
            .find(|t| t.id == task)
            .ok_or("task definition missing")?;
        let mut dependencies = definition.dependencies.clone();
        if transitive {
            let mut seen = std::collections::BTreeSet::new();
            let mut pending = dependencies;
            while let Some(id) = pending.pop() {
                if seen.insert(id.clone()) {
                    let parent = plan
                        .workflow
                        .tasks
                        .iter()
                        .find(|t| t.id == id)
                        .ok_or("ancestor missing")?;
                    pending.extend(parent.dependencies.iter().cloned());
                }
            }
            dependencies = seen.into_iter().collect();
        }
        dependencies.sort();
        // Bound the aggregate, not just each individual snapshot. Counting the
        // same base twice is intentional: both input file sets stay locked.
        let mut remaining_files = 20000usize;
        let mut remaining_bytes = 512 * 1024 * 1024u64;
        dependencies
            .iter()
            .map(|id| self.accepted_input(id, &mut remaining_files, &mut remaining_bytes))
            .collect()
    }

    fn accepted_input(
        &self,
        task: &str,
        remaining_files: &mut usize,
        remaining_bytes: &mut u64,
    ) -> Result<AcceptedDependencyInput, String> {
        let plan = self.revision().ok_or("plan revision missing")?;
        let run = self.state().tasks.get(task).ok_or("dependency missing")?;
        require(
            run.state == TaskState::Accepted && run.verified,
            "dependency not accepted and verified",
        )?;
        let attempt = run.attempt.as_deref().ok_or("accepted attempt missing")?;
        let original = self
            .attempt_record(task, attempt)
            .ok_or("accepted attempt record missing")?;
        require(
            original.revision_digest == plan.approval_digest()?,
            "dependency revision mismatch",
        )?;
        let acceptance = self
            .entries
            .iter()
            .rev()
            .find(|entry| {
                matches!(
                    &entry.body.event.command,
                    WorkflowCommand::Accept { task: t, attempt: a } if t == task && a == attempt
                )
            })
            .ok_or("dependency acceptance event missing")?;
        let (receipt, artifacts) = self
            .completed_artifacts(task, attempt)
            .ok_or("dependency artifacts missing")?;
        let report = self.verified_report(task, attempt)?;
        let seal = report
            .source_snapshot
            .as_ref()
            .ok_or("accepted dependency has no sealed snapshot")?;
        let snapshot = crate::source_snapshot::open_frozen(
            plan,
            artifacts,
            seal,
            &self.root.join("verification"),
            remaining_files,
            remaining_bytes,
        )?;
        Ok(AcceptedDependencyInput {
            binding: AcceptedInputBinding {
                schema_version: 1,
                project_id: plan.project.id.clone(),
                revision_digest: plan.approval_digest()?,
                task_id: task.into(),
                attempt_id: attempt.into(),
                acceptance_event_hash: acceptance.hash.clone(),
                receipt_digest: digest(receipt)?,
                artifact_manifest_digest: digest(artifacts)?,
                verification_report_digest: digest(report)?,
                workspace_record_digest: original
                    .workspace_proof
                    .as_ref()
                    .ok_or("accepted workspace proof missing")?
                    .record_digest
                    .clone(),
                snapshot: seal.clone(),
            },
            snapshot,
        })
    }
}
