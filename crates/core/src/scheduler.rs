//! Durable host for the pure workflow. Dispatch remains caller-owned; the
//! explicit run_verification method runs approved checks through the verifier.
//! A caller may dispatch only after a fresh intent has returned successfully.
use crate::handoff::{DailyVolume, HandoffEvent, HandoffOptions, HandoffStatus};
#[cfg(windows)]
#[path = "accepted_inputs.rs"]
mod accepted_inputs;
#[path = "early_cancellation.rs"]
mod early_cancellation;
#[path = "scheduler_handoff_cache.rs"]
mod handoff_cache;
#[cfg(test)]
#[path = "scheduler_handoff_cache_tests.rs"]
mod handoff_cache_tests;
#[path = "scheduler_index_rows.rs"]
mod index_rows;
#[cfg(test)]
#[path = "scheduler_index_rows_tests.rs"]
mod index_rows_tests;
#[path = "scheduler_projection.rs"]
pub(crate) mod projection;
#[cfg(test)]
#[path = "scheduler_projection_tests.rs"]
mod projection_tests;
#[path = "scheduler_recheck.rs"]
mod recheck;
#[path = "scheduler_recheck_disposition.rs"]
mod recheck_disposition;
#[cfg(windows)]
#[path = "scheduler_recheck_runner.rs"]
mod recheck_runner;
pub(crate) use recheck::APPROVAL_SOURCE as RECHECK_APPROVAL_SOURCE;
#[cfg(windows)]
#[path = "scheduler_verification.rs"]
mod verification;
#[cfg(windows)]
pub use accepted_inputs::{AcceptedDependencyInput, AcceptedInputBinding};
use cerulean_domain::execution::{
    Approval, ArtifactManifest, Attempt, ExecutionEvidence, ExecutionReceipt, ExecutionScope,
    PlanRevision, ReceiptStatus, VerificationReport,
};
use cerulean_domain::workflow::{
    Principal, Role, TaskState, Workflow, WorkflowCommand, WorkflowEvent, WorkflowState,
};
use chrono::Utc;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
#[cfg(windows)]
pub(crate) use verification::PreparedVerification;

const MAX_EVENT_BYTES: usize = 256 * 1024;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SchedulerConfig {
    pub schema_version: u32,
    pub principals: Vec<Principal>,
    pub core_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_scope: Option<ExecutionScope>,
}
impl SchedulerConfig {
    /// Separate bindings generation for experimental, user-planned single API
    /// execution. Older readers reject schema 3; never convert an existing
    /// team ledger into this mode or import it as a managed RuntimeHost job.
    pub fn single_executor(scope: ExecutionScope) -> Self {
        Self {
            schema_version: 3,
            principals: [
                ("user", Role::User),
                ("core", Role::Core),
                ("worker", Role::Worker),
            ]
            .into_iter()
            .map(|(id, role)| Principal {
                id: id.into(),
                role,
                provider: None,
            })
            .collect(),
            core_id: "core".into(),
            execution_scope: Some(scope),
        }
    }
    pub(crate) fn workflow(&self) -> Result<Workflow, String> {
        if self.schema_version == 3 {
            require(
                self.execution_scope.is_some(),
                "single executor needs strict scope",
            )?;
            Workflow::new_single_executor(self.principals.clone())
        } else {
            Workflow::new(self.principals.clone())
        }
    }
    fn validate_timezone(&self) -> Result<(), String> {
        if matches!(self.schema_version, 2 | 3) {
            cerulean_domain::timezone::require_archived_iana_rules()?;
        }
        let zone = self
            .execution_scope
            .as_ref()
            .map(|s| s.project.timezone.as_str());
        if let Some(name) = zone {
            cerulean_domain::timezone::parse(name)?;
        }
        require(
            match self.schema_version {
                1 => zone.is_none_or(|name| name == cerulean_domain::timezone::DEFAULT),
                2 => zone.is_some_and(|name| name != cerulean_domain::timezone::DEFAULT),
                3 => zone.is_some(),
                _ => false,
            },
            "unsupported scheduler schema or timezone binding",
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recheck_disposition:
        Option<Box<cerulean_domain::verification_recheck::RecheckDispositionReview>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_recheck: Option<Box<cerulean_domain::verification_recheck::RecheckApproval>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_generation_resolution:
        Option<crate::api_generation_resolution::ApiGenerationResolutionReview>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_count_resolution: Option<crate::api_count_resolution::ApiCountResolutionReview>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count_resolution: Option<crate::count_resolution::CountResolutionReview>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub historical_charge: Option<crate::historical_confirmation::HistoricalReview>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planning_budget: Option<crate::currency_approval::PlanningApproval>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency_approval: Option<crate::currency_approval::ApprovalAnchor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approval_source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancellation_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<ExecutionEvidence>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntryBody {
    pub schema_version: u32,
    pub operation_id: String,
    pub received_at: String,
    pub bindings_digest: String,
    pub event: WorkflowEvent,
    pub context: CommandContext,
    pub previous_hash: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub body: EntryBody,
    pub hash: String,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitPoint {
    BeforeEventWrite,
    TempSynced,
    EventPublished,
    ProjectionQueued,
}

pub struct DurableWorkflow {
    root: PathBuf,
    _lock: File,
    config: SchedulerConfig,
    bindings_digest: String,
    core: Workflow,
    entries: Vec<Entry>,
    poisoned: bool,
    index: crate::derived_index::Cache,
    checkpoint: crate::replay_checkpoint::Tracker,
    handoff_cache: Option<handoff_cache::Cached>,
}
impl Drop for DurableWorkflow {
    fn drop(&mut self) {
        self.index.shutdown();
    }
}
impl DurableWorkflow {
    /// Trusted host only: preparation is a filesystem side effect covered by the
    /// full approved worktree policy, never a model-selected working directory.
    #[cfg(windows)]
    pub fn prepare_task_workspace(
        &self,
        task: &str,
        attempt: &str,
    ) -> Result<(String, cerulean_domain::execution::WorkspaceProof), String> {
        self.prepare_task_workspace_cancellable(
            task,
            attempt,
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        )
    }
    #[cfg(windows)]
    pub(crate) fn prepare_task_workspace_cancellable(
        &self,
        task: &str,
        attempt: &str,
        cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Result<(String, cerulean_domain::execution::WorkspaceProof), String> {
        let (plan, mut inputs) = self.workspace_preparation(task, attempt)?;
        crate::git_workspaces::prepare_approved_cancellable(
            &plan,
            task,
            attempt,
            cancel,
            &mut inputs,
        )
    }
    #[cfg(windows)]
    pub(crate) fn workspace_preparation(
        &self,
        task: &str,
        attempt: &str,
    ) -> Result<(PlanRevision, Vec<AcceptedDependencyInput>), String> {
        require(
            !self.poisoned,
            "workflow must be reopened before preparing workspace",
        )?;
        require(
            self.state().revision_requested.is_none(),
            "workspace preparation paused for plan revision",
        )?;
        require(
            !self
                .history()
                .iter()
                .any(|entry| entry.body.event.command.attempt() == Some(attempt)),
            "workspace preparation requires an unused attempt ID",
        )?;
        let plan = self.revision().ok_or("plan revision missing")?;
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
            "workspace preparation requires pending task",
        )?;
        let definition = plan
            .workflow
            .tasks
            .iter()
            .find(|t| t.id == task)
            .ok_or("task definition missing")?;
        require(
            definition.dependencies.iter().all(|id| {
                self.state()
                    .tasks
                    .get(id)
                    .is_some_and(|t| t.state == TaskState::Accepted)
            }),
            "workspace dependencies not accepted",
        )?;
        let inputs = if definition.dependencies.is_empty() {
            vec![]
        } else {
            require(plan.worktree.as_ref().is_some_and(|w| w.input == cerulean_domain::execution::GitWorktreeInput::CommittedRawBlobsWithAcceptedDependencies),
                "dependent worktree preparation requires approved sealed input overlay")?;
            self.accepted_ancestor_inputs(task)?
        };
        Ok((plan.clone(), inputs))
    }
    pub(crate) fn api_attempt_path(&self, attempt: &str) -> Result<PathBuf, String> {
        cerulean_domain::execution::identifier(attempt)?;
        Ok(self.root.join(format!("api-attempt-{}", digest(&attempt)?)))
    }
    #[cfg(windows)]
    pub(crate) fn api_attempt_directories(&self) -> Result<Vec<PathBuf>, String> {
        require(
            !self.poisoned,
            "workflow requires reopen before API inventory",
        )?;
        crate::host::ordinary(&self.root, true)?;
        let entries = fs::read_dir(&self.root)
            .map_err(|e| e.to_string())?
            .take(10001)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        require(
            entries.len() <= 10000,
            "workflow evidence inventory too large",
        )?;
        let mut paths = Vec::new();
        for entry in entries {
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_| "invalid workflow evidence name")?;
            if name.starts_with("api-attempt-") {
                crate::host::ordinary(&entry.path(), true)?;
                paths.push(entry.path());
            }
        }
        Ok(paths)
    }
    pub(crate) fn omp_file_service_path(&self, attempt: &str) -> Result<PathBuf, String> {
        cerulean_domain::execution::identifier(attempt)?;
        Ok(self.root.join(format!("omp-files-{}", digest(&attempt)?)))
    }
    pub(crate) fn omp_attempt_path(&self, attempt: &str) -> Result<PathBuf, String> {
        cerulean_domain::execution::identifier(attempt)?;
        let folder = self.root.join("omp");
        if !folder.exists() {
            fs::create_dir(&folder).map_err(|e| e.to_string())?;
        }
        require(
            safe_metadata(&folder)?.is_dir(),
            "OMP evidence root invalid",
        )?;
        Ok(folder.join(format!("attempt-{}", digest(&attempt)?)))
    }
}
fn digest<T: Serialize>(value: &T) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).map_err(|e| e.to_string())?)
    ))
}
fn valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 100
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn require(ok: bool, message: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(message.into())
    }
}
fn safe_metadata(path: &Path) -> Result<fs::Metadata, String> {
    let m = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    require(!m.file_type().is_symlink(), "linked storage path")?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        require(m.file_attributes() & 0x400 == 0, "reparse storage path")?;
    }
    Ok(m)
}
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    serde_json::from_slice(&read_event_bytes(path)?).map_err(|e| e.to_string())
}
fn read_event_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let m = safe_metadata(path)?;
    require(
        m.is_file() && m.len() <= MAX_EVENT_BYTES as u64,
        "invalid storage file",
    )?;
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|e| e.to_string())?
        .take((MAX_EVENT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    require(bytes.len() <= MAX_EVENT_BYTES, "storage file too large")?;
    Ok(bytes)
}
fn write_synced(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut f = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    f.write_all(bytes)
        .and_then(|_| f.sync_all())
        .map_err(|e| e.to_string())
}
pub(crate) fn temp_path(destination: &Path) -> PathBuf {
    destination.with_extension(format!(
        "{}-{}.tmp",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ))
}
impl DurableWorkflow {
    /// Inspect an existing journal under its writer lock without restoration,
    /// projection writes, directory creation, or exposing a mutable workflow.
    #[cfg(windows)]
    pub(crate) fn inspect_existing<T>(
        root: &Path,
        inspect: impl FnOnce(&Self) -> Result<T, String>,
    ) -> Result<T, String> {
        require(safe_metadata(root)?.is_dir(), "invalid workflow directory")?;
        require(
            safe_metadata(&root.join("events"))?.is_dir(),
            "invalid events directory",
        )?;
        require(
            safe_metadata(&root.join("writer.lock"))?.is_file(),
            "missing workflow lock",
        )?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .open(root.join("writer.lock"))
            .map_err(|e| e.to_string())?;
        lock.try_lock_exclusive()
            .map_err(|_| "workflow writer already active")?;
        let config: SchedulerConfig = read_json(&root.join("bindings.json"))?;
        config.validate_timezone()?;
        let mut store = Self {
            root: root.to_path_buf(),
            _lock: lock,
            bindings_digest: digest(&config)?,
            core: config.workflow()?,
            config,
            entries: vec![],
            poisoned: true,
            index: crate::derived_index::Cache::default(),
            checkpoint: crate::replay_checkpoint::Tracker::default(),
            handoff_cache: None,
        };
        store.load(false)?;
        inspect(&store)
    }
    pub fn open(root: &Path, config: SchedulerConfig) -> Result<Self, String> {
        config.validate_timezone()?;
        require(
            config
                .principals
                .iter()
                .any(|p| p.id == config.core_id && p.role == Role::Core),
            "invalid core binding",
        )?;
        let core = config.workflow()?;
        if let Some(scope) = &config.execution_scope {
            require(scope.policy_version > 0, "invalid active policy")?;
            let required = if config.schema_version == 3 {
                vec![Role::User, Role::Core, Role::Worker]
            } else {
                vec![Role::User, Role::Chief, Role::Deputy, Role::Core]
            };
            for role in required {
                require(
                    config.principals.iter().filter(|p| p.role == role).count() == 1,
                    "execution role must be uniquely bound",
                )?;
            }
        }
        fs::create_dir_all(root).map_err(|e| e.to_string())?;
        safe_metadata(root)?;
        let root = fs::canonicalize(root).map_err(|e| e.to_string())?;
        let lock_path = root.join("writer.lock");
        if lock_path.exists() {
            require(safe_metadata(&lock_path)?.is_file(), "invalid writer lock")?;
        }
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(|e| e.to_string())?;
        lock.try_lock_exclusive()
            .map_err(|_| "workflow writer already active")?;
        let events = root.join("events");
        fs::create_dir_all(&events).map_err(|e| e.to_string())?;
        require(safe_metadata(&events)?.is_dir(), "invalid event directory")?;
        let config_path = root.join("bindings.json");
        if config_path.exists() {
            require(
                read_json::<SchedulerConfig>(&config_path)? == config,
                "principal bindings changed",
            )?;
        } else {
            require(
                fs::read_dir(&events)
                    .map_err(|e| e.to_string())?
                    .next()
                    .is_none(),
                "missing bindings for existing journal",
            )?;
            let bytes = serde_json::to_vec(&config).map_err(|e| e.to_string())?;
            require(bytes.len() <= MAX_EVENT_BYTES, "bindings too large")?;
            let temp = temp_path(&config_path);
            write_synced(&temp, &bytes)?;
            fs::rename(temp, config_path).map_err(|e| e.to_string())?;
        }
        let bindings_digest = digest(&config)?;
        let mut store = Self {
            root,
            _lock: lock,
            config,
            bindings_digest,
            core,
            entries: vec![],
            poisoned: false,
            index: crate::derived_index::Cache::default(),
            checkpoint: crate::replay_checkpoint::Tracker::default(),
            handoff_cache: None,
        };
        store.load(true)?;
        let restored = store.core.clone().restore_current(&store.config.core_id)?;
        let pending = restored.journal()[store.entries.len()..].to_vec();
        for event in pending {
            let base = format!("recovery-{}", event.sequence);
            let mut operation_id = base.clone();
            let mut suffix = 0;
            while store
                .entries
                .iter()
                .any(|e| e.body.operation_id == operation_id)
            {
                suffix += 1;
                operation_id = format!("{base}-{suffix}");
            }
            store.command(
                &operation_id,
                &event.principal,
                event.command,
                CommandContext::default(),
                true,
            )?;
        }
        store.project()?;
        Ok(store)
    }
    pub fn state(&self) -> &WorkflowState {
        self.core.state()
    }
    pub(crate) fn requires_reopen(&self) -> bool {
        self.poisoned
    }
    #[cfg(test)]
    pub(crate) fn request_revision_state_for_workspace_test(
        &mut self,
        project_id: &str,
        reason: &str,
    ) -> Result<(), String> {
        require(
            self.config
                .execution_scope
                .as_ref()
                .is_some_and(|scope| scope.project.id == project_id),
            "test project identity mismatch",
        )?;
        let digest = self
            .state()
            .digest
            .clone()
            .ok_or("test workflow digest missing")?;
        self.core
            .apply(
                "user",
                WorkflowCommand::RequestRevision {
                    digest,
                    reason: reason.into(),
                },
            )
            .map(|_| ())
    }
    pub fn history(&self) -> &[Entry] {
        &self.entries
    }
    pub(crate) fn configuration(&self) -> &SchedulerConfig {
        &self.config
    }
    pub fn revision(&self) -> Option<&PlanRevision> {
        self.entries
            .iter()
            .rev()
            .find_map(|e| match &e.body.context.execution {
                Some(ExecutionEvidence::Revision { revision }) => Some(revision),
                _ => None,
            })
    }
    pub fn approval(&self) -> Option<&Approval> {
        let revision = self.revision()?;
        self.entries
            .iter()
            .rev()
            .find_map(|e| match &e.body.context.execution {
                Some(ExecutionEvidence::Approval { approval })
                    if approval.plan_id == revision.workflow.id
                        && approval.revision == revision.workflow.version =>
                {
                    Some(approval)
                }
                _ => None,
            })
    }
    fn role_id(&self, role: Role) -> Result<&str, String> {
        self.config
            .principals
            .iter()
            .find(|p| p.role == role)
            .map(|p| p.id.as_str())
            .ok_or("role binding missing".into())
    }
    pub fn attempt_record(&self, task: &str, id: &str) -> Option<&Attempt> {
        self.entries
            .iter()
            .rev()
            .find_map(|e| match &e.body.context.execution {
                Some(ExecutionEvidence::Attempt { attempt })
                    if attempt.task_id == task && attempt.id == id =>
                {
                    Some(attempt)
                }
                _ => None,
            })
    }
    pub fn completed_artifacts(
        &self,
        task: &str,
        attempt: &str,
    ) -> Option<(&ExecutionReceipt, &ArtifactManifest)> {
        self.entries
            .iter()
            .rev()
            .find_map(|e| match &e.body.context.execution {
                Some(ExecutionEvidence::Receipt {
                    receipt,
                    manifest: Some(manifest),
                }) if receipt.task_id == task
                    && receipt.attempt_id == attempt
                    && receipt.status == ReceiptStatus::Completed =>
                {
                    Some((receipt, manifest))
                }
                _ => None,
            })
    }
    fn check_execution(
        &self,
        operation: &str,
        principal: &str,
        command: &WorkflowCommand,
        context: &CommandContext,
        live: bool,
    ) -> Result<(), String> {
        use cerulean_domain::execution::{digest as proof_digest, sha256};
        use cerulean_domain::workflow::ReconcileOutcome;
        require(
            context.recheck_disposition.is_some()
                == matches!(command, WorkflowCommand::RetireVerificationRecheck { .. }),
            "recheck disposition requires its explicit user command",
        )?;
        require(
            context.verification_recheck.is_some()
                == matches!(command, WorkflowCommand::ApproveVerificationRecheck { .. }),
            "recheck approval requires its explicit user command",
        )?;
        require(
            context.api_generation_resolution.is_some()
                == matches!(
                    command,
                    WorkflowCommand::ConfirmApiGenerationResolution { .. }
                ),
            "API generation review requires its explicit user command",
        )?;
        require(
            context.api_count_resolution.is_some()
                == matches!(command, WorkflowCommand::ConfirmApiCountResolution { .. }),
            "API count review requires its explicit user command",
        )?;
        require(
            context.count_resolution.is_some()
                == matches!(
                    command,
                    WorkflowCommand::ConfirmManagementCountResolution { .. }
                ),
            "count resolution requires its explicit user command",
        )?;
        require(
            context.historical_charge.is_some()
                == matches!(command, WorkflowCommand::ConfirmHistoricalCharge { .. }),
            "historical review requires its explicit user confirmation command",
        )?;
        if context.currency_approval.is_some() {
            require(
                matches!(command, WorkflowCommand::Approve { .. }),
                "currency approval anchor requires user approval",
            )?;
        }
        require(
            context.planning_budget.is_some()
                == matches!(command, WorkflowCommand::AuthorizePlanningBudget { .. }),
            "planning permission requires its explicit user command",
        )?;
        let Some(scope) = &self.config.execution_scope else {
            return require(
                context.execution.is_none()
                    && context.recheck_disposition.is_none()
                    && context.verification_recheck.is_none()
                    && context.currency_approval.is_none()
                    && context.planning_budget.is_none()
                    && context.historical_charge.is_none()
                    && context.api_count_resolution.is_none()
                    && context.api_generation_resolution.is_none()
                    && context.count_resolution.is_none(),
                "execution evidence requires strict scope",
            );
        };
        let expected = match command {
            WorkflowCommand::Propose { .. } if self.config.schema_version == 3 => Role::User,
            WorkflowCommand::Assign { .. } | WorkflowCommand::Accept { .. }
                if self.config.schema_version == 3 =>
            {
                Role::Core
            }
            WorkflowCommand::Propose { .. } | WorkflowCommand::Accept { .. } => Role::Chief,
            WorkflowCommand::Approve { .. }
            | WorkflowCommand::ApproveVerificationRecheck { .. }
            | WorkflowCommand::RetireVerificationRecheck { .. }
            | WorkflowCommand::AuthorizePlanningBudget { .. }
            | WorkflowCommand::ConfirmHistoricalCharge { .. }
            | WorkflowCommand::ConfirmManagementCountResolution { .. }
            | WorkflowCommand::ConfirmApiGenerationResolution { .. }
            | WorkflowCommand::ConfirmApiCountResolution { .. }
            | WorkflowCommand::AbandonUnsent { .. }
            | WorkflowCommand::RetireUnknownApi { .. }
            | WorkflowCommand::RetireUnknownDesktop { .. }
            | WorkflowCommand::RequestRevision { .. } => Role::User,
            WorkflowCommand::RetireSubmittedPlan { .. } => Role::User,
            WorkflowCommand::Assign { .. }
            | WorkflowCommand::Advise { .. }
            | WorkflowCommand::ReturnForRework { .. } => Role::Deputy,
            WorkflowCommand::Submit { .. } => Role::Worker,
            _ => Role::Core,
        };
        require(
            self.config
                .principals
                .iter()
                .any(|p| p.id == principal && p.role == expected),
            "execution channel role mismatch",
        )?;
        if let WorkflowCommand::RetireVerificationRecheck {
            task,
            attempt,
            operation_id,
            review_digest,
        } = command
        {
            return self.check_recheck_disposition(
                operation,
                task,
                attempt,
                operation_id,
                review_digest,
                context,
                live,
            );
        }
        if let WorkflowCommand::ApproveVerificationRecheck {
            task,
            attempt,
            operation_id,
            review_digest,
        } = command
        {
            require(
                operation != operation_id,
                "approval and verification need distinct operation IDs",
            )?;
            return self.check_recheck_approval(
                task,
                attempt,
                operation_id,
                review_digest,
                context,
                live,
            );
        }
        if let WorkflowCommand::ConfirmApiGenerationResolution { review_digest } = command {
            let review = context
                .api_generation_resolution
                .as_ref()
                .ok_or("API generation review missing")?;
            require(
                *context
                    == CommandContext {
                        api_generation_resolution: Some(review.clone()),
                        approval_source: Some(
                            crate::api_generation_resolution::APPROVAL_SOURCE.into(),
                        ),
                        ..Default::default()
                    },
                "API generation confirmation requires authenticated plain user context",
            )?;
            let data = self
                .root
                .parent()
                .and_then(Path::parent)
                .ok_or("API confirmation needs original Host root")?;
            crate::currency_api_resolution_guard::require_protected(data, true)?;
            return review.validate_for(&scope.project.id, review_digest);
        }
        if let WorkflowCommand::ConfirmApiCountResolution { review_digest } = command {
            let review = context
                .api_count_resolution
                .as_ref()
                .ok_or("API count review missing")?;
            require(
                *context
                    == CommandContext {
                        api_count_resolution: Some(review.clone()),
                        approval_source: Some(crate::api_count_resolution::APPROVAL_SOURCE.into()),
                        ..Default::default()
                    },
                "API count confirmation requires authenticated plain user context",
            )?;
            let data = self
                .root
                .parent()
                .and_then(Path::parent)
                .ok_or("API confirmation needs original Host root")?;
            crate::currency_api_resolution_guard::require_protected(data, true)?;
            return review.validate_for(&scope.project.id, review_digest);
        }
        if let WorkflowCommand::ConfirmManagementCountResolution { review_digest } = command {
            let review = context
                .count_resolution
                .as_ref()
                .ok_or("count resolution review missing")?;
            require(
                *context
                    == CommandContext {
                        count_resolution: Some(review.clone()),
                        approval_source: Some(crate::count_resolution::APPROVAL_SOURCE.into()),
                        ..Default::default()
                    },
                "count resolution requires authenticated plain user context",
            )?;
            return review.validate(&scope.project.id, review_digest);
        }
        if let WorkflowCommand::ConfirmHistoricalCharge { review_digest } = command {
            let review = context
                .historical_charge
                .as_ref()
                .ok_or("historical review missing")?;
            require(
                *context
                    == CommandContext {
                        historical_charge: Some(review.clone()),
                        approval_source: Some(
                            crate::historical_confirmation::APPROVAL_SOURCE.into(),
                        ),
                        ..Default::default()
                    },
                "historical confirmation requires authenticated plain user review context",
            )?;
            return review.validate(&scope.project.id, review_digest);
        }
        if let WorkflowCommand::AuthorizePlanningBudget { setup_digest } = command {
            require(
                context.execution.is_none()
                    && context.currency_approval.is_none()
                    && context.approval_source.as_deref()
                        == Some("authenticated local-user planning-budget approval"),
                "planning permission cannot grant execution approval",
            )?;
            return context
                .planning_budget
                .as_ref()
                .ok_or("full planning budget permission required")?
                .validate(&scope.project.id, setup_digest);
        }
        if let WorkflowCommand::Propose { plan } = command {
            let Some(ExecutionEvidence::Revision { revision }) = &context.execution else {
                return Err("full plan revision required".into());
            };
            revision.validate()?;
            if self.config.schema_version == 3 {
                require(
                    revision.execution.len() == 1
                        && revision.execution[0].api_worker.is_some()
                        && revision.management_setup_digest.is_none()
                        && revision.policy.supervision.is_none()
                        && revision.execution[0].supervision_override.is_none()
                        && revision.policy.currency_budget.is_none(),
                    "single executor requires one API worker without management or monetary execution",
                )?;
            }
            #[cfg(not(windows))]
            require(
                revision.worktree.is_none(),
                "worktree execution requires Windows host",
            )?;
            require(
                revision.workflow == *plan
                    && revision.project == scope.project
                    && revision.policy.version == scope.policy_version,
                "revision scope mismatch",
            )?;
            // Legacy project-wide keys remain valid and retain their exact bytes.
            // Distinct keys require the approved physical worktree policy; host
            // admission still checks directories/resources and active ownership.
            for task in &plan.tasks {
                require(
                    task.workspace_key == scope.project.id
                        || (revision.worktree.is_some()
                            && task.workspace_key
                                == cerulean_domain::execution::isolated_task_key(
                                    &scope.project.id,
                                    &task.id,
                                )?),
                    "workspace isolation not configured",
                )?;
            }
            return Ok(());
        }
        let revision = self.revision().ok_or("plan revision missing")?;
        if matches!(
            command,
            WorkflowCommand::RequestRevision { .. } | WorkflowCommand::RetireSubmittedPlan { .. }
        ) {
            let audit = if matches!(command, WorkflowCommand::RetireSubmittedPlan { .. }) {
                "authenticated local-user submitted-plan retirement"
            } else {
                "authenticated local-user revision request"
            };
            require(
                context.approval_source.as_deref() == Some(audit) && context.execution.is_none(),
                "authenticated revision request required",
            )?;
            return Ok(());
        }
        if let WorkflowCommand::Approve { .. } = command {
            let Some(ExecutionEvidence::Approval { approval }) = &context.execution else {
                return Err("full revision approval required".into());
            };
            crate::currency_approval::validate_anchor(
                revision,
                context.currency_approval.as_ref(),
            )?;
            return approval.validate(revision, principal, scope.policy_version);
        }
        if matches!(
            command,
            WorkflowCommand::Advise { .. } | WorkflowCommand::RecoveryUnknown { .. }
        ) {
            return require(context.execution.is_none(), "unexpected execution evidence");
        }
        let approval = self.approval().ok_or("full revision not approved")?;
        approval.validate(revision, self.role_id(Role::User)?, scope.policy_version)?;
        if let WorkflowCommand::ReturnForRework {
            task,
            attempt,
            verification_operation,
            report_digest,
            ..
        } = command
        {
            require(
                *context == CommandContext::default(),
                "rework requires plain deputy context",
            )?;
            require(
                self.unresolved_verification_recheck().is_none(),
                "original recheck is unresolved",
            )?;
            let entry = self
                .entries
                .iter()
                .rev()
                .find(|e| {
                    matches!(
                        &e.body.event.command, WorkflowCommand::Verify { task: t, attempt: a, .. }
                            if t == task && a == attempt
                    )
                })
                .ok_or("failed check evidence missing")?;
            let report = match &entry.body.context.execution {
                Some(ExecutionEvidence::Verification { report }) => report,
                Some(ExecutionEvidence::VerificationRecheck { .. }) => {
                    return Err("检查标准已复验修订，请先由主指挥修订方案再返工".into());
                }
                _ => return Err("failed check report missing".into()),
            };
            return require(
                entry.body.operation_id == *verification_operation
                    && proof_digest(report)? == *report_digest
                    && matches!(
                        entry.body.event.command,
                        WorkflowCommand::Verify { passed: false, .. }
                    ),
                "rework differs from the latest failed check",
            );
        }
        if let WorkflowCommand::Assign {
            task,
            worker,
            attempt: id,
        } = command
        {
            let Some(ExecutionEvidence::Attempt { attempt }) = &context.execution else {
                return Err("bound attempt required".into());
            };
            let run = self.core.state().tasks.get(task).ok_or("task missing")?;
            attempt.binding.validate_controller_identity()?;
            if let Some(desktop) = &revision.task(task)?.desktop_worker {
                desktop.validate_attempt(&attempt.binding)?;
                require(
                    worker == &format!("worker-{}", desktop.adapter_id()),
                    "桌面分配执行者与原批准会话不符",
                )?;
            }
            require(
                attempt.id == *id
                    && attempt.task_id == *task
                    && attempt.worker_id == *worker
                    && attempt.fence == run.fence.checked_add(1).ok_or("fence overflow")?
                    && attempt.revision_digest == revision.approval_digest()?
                    && attempt.binding.workspace == revision.execution_root(task, id)?
                    && attempt.binding.adapter_id == revision.task(task)?.adapter_id
                    && !attempt.binding.session_id.trim().is_empty()
                    && !attempt.binding.adapter_version.trim().is_empty(),
                "attempt scope mismatch",
            )?;
            revision.validate_attempt_workspace(attempt)?;
            #[cfg(windows)]
            if live {
                crate::git_workspaces::require_prepared(revision, attempt)?;
            }
            return Ok(());
        }
        let (task, id) = match command {
            WorkflowCommand::DispatchIntent { task, attempt }
            | WorkflowCommand::AbandonUnsent { task, attempt, .. }
            | WorkflowCommand::RetireUnknownApi { task, attempt, .. }
            | WorkflowCommand::RetireUnknownDesktop { task, attempt, .. }
            | WorkflowCommand::Started { task, attempt }
            | WorkflowCommand::CancelIntent { task, attempt, .. }
            | WorkflowCommand::Submit { task, attempt, .. }
            | WorkflowCommand::Reconcile { task, attempt, .. }
            | WorkflowCommand::Verify { task, attempt, .. }
            | WorkflowCommand::Accept { task, attempt } => (task, attempt),
            _ => return Err("unexpected execution command".into()),
        };
        let attempt = self
            .attempt_record(task, id)
            .ok_or("attempt binding missing")?;
        revision.validate_attempt_workspace(attempt)?;
        #[cfg(windows)]
        if live && matches!(command, WorkflowCommand::DispatchIntent { .. }) {
            crate::git_workspaces::require_prepared(revision, attempt)?;
        }
        if let Some(ExecutionEvidence::StartedTurnCancellation { receipt, proof }) =
            &context.execution
        {
            return self.check_started_turn_cancellation(command, context, attempt, receipt, proof);
        }
        match command {
            WorkflowCommand::RetireUnknownDesktop {
                evidence_digest, ..
            } => {
                sha256(evidence_digest)?;
                require(
                    *context == CommandContext::default(),
                    "desktop retirement requires plain user context",
                )?;
                revision
                    .task(task)?
                    .desktop_worker
                    .as_ref()
                    .ok_or("desktop retirement requires approved desktop binding")?
                    .validate_attempt(&attempt.binding)?;
            }
            WorkflowCommand::RetireUnknownApi { .. } => require(
                *context == CommandContext::default()
                    && revision.task(task)?.api_worker.is_some()
                    && cerulean_domain::api_worker::is_api_adapter(&attempt.binding.adapter_id),
                "local API retirement requires approved API binding and plain user context",
            )?,
            WorkflowCommand::AbandonUnsent { .. } => require(
                *context == CommandContext::default(),
                "local unsent disposition requires plain user context",
            )?,
            WorkflowCommand::DispatchIntent { .. } => {
                require(
                    context.execution.is_none()
                        && context.thread_id.as_ref() == Some(&attempt.binding.session_id)
                        && context.cwd.as_ref() == Some(&attempt.binding.workspace),
                    "dispatch scope mismatch",
                )?;
            }
            WorkflowCommand::CancelIntent { .. } => {
                require(context.execution.is_none(), "unexpected cancel evidence")?
            }
            WorkflowCommand::Started { .. }
            | WorkflowCommand::Submit { .. }
            | WorkflowCommand::Reconcile { .. } => {
                let Some(ExecutionEvidence::Receipt { receipt, manifest }) = &context.execution
                else {
                    return Err("structured receipt required".into());
                };
                let recorded_turn = self
                    .last_start(task, id)
                    .and_then(|e| e.body.context.turn_id.as_deref());
                let turn = recorded_turn.or(context.turn_id.as_deref());
                receipt.validate(revision, attempt, turn)?;
                require(
                    context.thread_id.as_ref() == Some(&attempt.binding.session_id)
                        && context.turn_id == receipt.turn_id,
                    "receipt context mismatch",
                )?;
                let status = match command {
                    WorkflowCommand::Started { .. } => ReceiptStatus::Running,
                    WorkflowCommand::Submit { .. } => ReceiptStatus::Completed,
                    WorkflowCommand::Reconcile { outcome, .. } => match outcome {
                        ReconcileOutcome::NotStarted => {
                            require(
                                self.last_start(task, id).is_none(),
                                "started attempt cannot become not-started",
                            )?;
                            ReceiptStatus::NotStarted
                        }
                        ReconcileOutcome::Running => ReceiptStatus::Running,
                        ReconcileOutcome::Submitted => ReceiptStatus::Completed,
                        ReconcileOutcome::Cancelled => ReceiptStatus::Cancelled,
                    },
                    _ => unreachable!(),
                };
                require(receipt.status == status, "receipt outcome mismatch")?;
                if status == ReceiptStatus::Completed {
                    let manifest = manifest
                        .as_ref()
                        .ok_or("completed artifact manifest required")?;
                    require(
                        manifest.schema_version == 1
                            && manifest.revision_digest == revision.approval_digest()?
                            && manifest.task_id == *task
                            && manifest.attempt_id == *id
                            && manifest.receipt_digest == proof_digest(receipt)?,
                        "artifact binding mismatch",
                    )?;
                    let mut expected = revision.task(task)?.artifact_paths.clone();
                    expected.sort();
                    let observed = manifest
                        .artifacts
                        .iter()
                        .map(|a| &a.path)
                        .collect::<Vec<_>>();
                    require(
                        observed.windows(2).all(|pair| pair[0] < pair[1])
                            && observed.iter().all(|path| expected.contains(path)),
                        "artifact set contains duplicates or unapproved paths",
                    )?;
                    for artifact in &manifest.artifacts {
                        sha256(&artifact.sha256)?;
                    }
                    if live {
                        crate::artifacts::ArtifactObserver::new(
                            revision,
                            approval,
                            self.role_id(Role::User)?,
                            scope.policy_version,
                            attempt,
                            receipt,
                            turn,
                        )?
                        .require_unchanged(manifest)?;
                    }
                } else {
                    require(manifest.is_none(), "non-completed receipt has artifacts")?;
                }
            }
            WorkflowCommand::Verify { passed, .. } => {
                if matches!(
                    context.execution,
                    Some(ExecutionEvidence::VerificationRecheck { .. })
                ) {
                    return self.check_recheck_result(
                        operation,
                        command,
                        context,
                        self.entries.len(),
                        live,
                    );
                }
                let Some(ExecutionEvidence::Verification { report }) = &context.execution else {
                    return Err("independent verification report required".into());
                };
                let (receipt, manifest) = self
                    .completed_artifacts(task, id)
                    .ok_or("completion evidence missing")?;
                require(
                    report.verifier_id == self.config.core_id
                        && report.manifest_digest == proof_digest(manifest)?,
                    "verification provenance or manifest mismatch",
                )?;
                if *passed {
                    report.require_pass(revision, manifest, &self.config.core_id)?;
                    if live {
                        #[cfg(windows)]
                        if revision.snapshot_policy.is_some() {
                            crate::source_snapshot::require_sealed(
                                revision,
                                manifest,
                                report
                                    .source_snapshot
                                    .as_ref()
                                    .ok_or("snapshot evidence missing")?,
                                &self.root.join("verification"),
                            )?;
                        }
                        crate::artifacts::ArtifactObserver::new(
                            revision,
                            approval,
                            self.role_id(Role::User)?,
                            scope.policy_version,
                            attempt,
                            receipt,
                            receipt.turn_id.as_deref(),
                        )?
                        .require_unchanged(manifest)?;
                    }
                } else {
                    require(
                        report
                            .require_pass(revision, manifest, &self.config.core_id)
                            .is_err(),
                        "passing report cannot be recorded as failure",
                    )?;
                }
            }
            WorkflowCommand::Accept { .. } => {
                require(
                    context.execution.is_none(),
                    "accept cannot supply replacement evidence",
                )?;
                let (receipt, manifest) = self
                    .completed_artifacts(task, id)
                    .ok_or("completion evidence missing")?;
                let report = self.verified_report(task, id)?;
                if live {
                    #[cfg(windows)]
                    if revision.snapshot_policy.is_some() {
                        crate::source_snapshot::require_sealed(
                            revision,
                            manifest,
                            report
                                .source_snapshot
                                .as_ref()
                                .ok_or("snapshot evidence missing")?,
                            &self.root.join("verification"),
                        )?;
                    }
                    crate::artifacts::ArtifactObserver::new(
                        revision,
                        approval,
                        self.role_id(Role::User)?,
                        scope.policy_version,
                        attempt,
                        receipt,
                        receipt.turn_id.as_deref(),
                    )?
                    .require_unchanged(manifest)?;
                }
            }
            _ => unreachable!(),
        }
        Ok(())
    }
    pub fn handoffs(&self) -> Result<Vec<DailyVolume>, String> {
        if let Ok(daily) = self.index.daily(&self.root, &self.index_head()) {
            return Ok(daily);
        }
        if let Some(cached) = self
            .handoff_cache
            .as_ref()
            .filter(|c| c.head == self.index_head())
        {
            return self.render_handoffs(cached.events.iter());
        }
        self.handoffs_for(self.entries.iter())
    }
    fn index_head(&self) -> crate::derived_index::Head {
        crate::derived_index::Head {
            ledger: "workflow".into(),
            count: self.entries.len(),
            hash: self
                .entries
                .last()
                .map(|e| e.hash.clone())
                .unwrap_or("genesis".into()),
            bindings: self.bindings_digest.clone(),
        }
    }
    fn index_tasks(&self) -> Result<Vec<crate::derived_index::IndexedTask>, String> {
        use crate::derived_index::IndexedTask;
        let zone = cerulean_domain::timezone::parse(
            self.config
                .execution_scope
                .as_ref()
                .map(|s| s.project.timezone.as_str())
                .unwrap_or(cerulean_domain::timezone::DEFAULT),
        )?;
        let mut tasks = Vec::new();
        for (id, run) in &self.state().tasks {
            // A revised plan may reuse task IDs: derive creation from the current proposal.
            let proposal = self.entries.iter().rev().find(|e| matches!(
                &e.body.event.command, WorkflowCommand::Propose { plan } if plan.tasks.iter().any(|t| &t.id == id)
            )).ok_or("task proposal missing")?;
            let first = chrono::DateTime::parse_from_rfc3339(&proposal.body.received_at)
                .map_err(|_| "invalid task creation time")?;
            let updated_at = self
                .entries
                .iter()
                .rev()
                .find(|e| {
                    e.body.event.command.task() == Some(id.as_str())
                        && e.body.event.sequence >= proposal.body.event.sequence
                })
                .map(|e| e.body.received_at.clone())
                .unwrap_or_else(|| proposal.body.received_at.clone());
            tasks.push(IndexedTask {
                id: id.clone(),
                state: serde_json::to_value(run.state)
                    .map_err(|e| e.to_string())?
                    .as_str()
                    .ok_or("missing task state")?
                    .into(),
                attempt: run.attempt.clone(),
                first_day: if self.config.schema_version == 1 {
                    first
                        .with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).unwrap())
                        .format("%Y-%m-%d")
                        .to_string()
                } else {
                    first.with_timezone(&zone).format("%Y-%m-%d").to_string()
                },
                updated_at,
                unfinished: !matches!(run.state, TaskState::Accepted | TaskState::Cancelled),
            });
        }
        Ok(tasks)
    }
    fn refresh_index(&mut self, daily: Vec<DailyVolume>) {
        self.index.prepare_refresh();
        if let Some(proofs) = self.checkpoint.due() {
            self.index
                .checkpoint(&crate::replay_checkpoint::PublicationRef {
                    kind: "workflow",
                    bindings: &self.bindings_digest,
                    proofs: &proofs,
                    payload: crate::replay_checkpoint::PayloadRef::Workflow {
                        state: self.core.state(),
                        entries: &self.entries,
                    },
                });
        }
        let tasks = self.index_tasks();
        let events = index_rows::Events(&self.entries);
        let head = self.index_head();
        let data = tasks
            .as_ref()
            .map(|tasks| crate::derived_index::DataRef {
                head: &head,
                events: &events,
                plans: &[],
                summaries: &[],
                tasks,
                daily: &daily,
            })
            .map_err(Clone::clone);
        let documents = projection::SnapshotRef {
            sequence: head.count,
            last_hash: &head.hash,
            state: self.core.state(),
            daily: &daily,
        };
        self.index
            .refresh_workflow(&self.root, head.clone(), data, documents);
    }
    pub fn index_error(&self) -> Option<String> {
        self.index
            .read(&self.root, &self.index_head())
            .err()
            .or_else(|| self.index.error())
    }
    pub fn wait_for_index(&self, timeout: std::time::Duration) -> Result<(), String> {
        self.index.wait(timeout)
    }
    /// Diagnostic only; never changes the authoritative execution snapshot.
    pub fn checkpoint_events(&self) -> usize {
        self.checkpoint.restored
    }
    /// Explicit maintenance barrier; normal dispatch never waits for derived files.
    pub fn wait_for_projections(&self, timeout: std::time::Duration) -> Result<(), String> {
        self.index.wait_for_projections(timeout)
    }
    pub fn projection_error(&self) -> Option<String> {
        self.index.projection_error()
    }
    pub(crate) fn derived_diagnostics(&self) -> crate::derived_index::Diagnostics {
        self.index.diagnostics()
    }
    /// A reading view only. Scheduling and resource ownership use the replayed domain state.
    pub fn unfinished_before(
        &self,
        local_day: &str,
    ) -> Result<Vec<crate::derived_index::IndexedTask>, String> {
        let parsed = chrono::NaiveDate::parse_from_str(local_day, "%Y-%m-%d")
            .map_err(|_| "invalid local day")?;
        require(
            parsed.format("%Y-%m-%d").to_string() == local_day,
            "invalid local day",
        )?;
        if let Ok(tasks) = self
            .index
            .unfinished_before(&self.root, &self.index_head(), local_day)
        {
            return Ok(tasks);
        }
        Ok(self
            .index_tasks()?
            .into_iter()
            .filter(|t| t.unfinished && t.first_day.as_str() < local_day)
            .collect())
    }
    fn handoff_events<'a>(
        &self,
        entries: impl Iterator<Item = &'a Entry>,
    ) -> Result<Vec<HandoffEvent>, String> {
        let mut core = self.config.workflow()?;
        let mut events = Vec::new();
        for entry in entries {
            core.apply(
                &entry.body.event.principal,
                entry.body.event.command.clone(),
            )?;
            if let Some(event) = handoff_cache::event(entry, core.state(), events.len() as u64 + 1)?
            {
                events.push(event);
            }
        }
        Ok(events)
    }
    fn render_handoffs<'a>(
        &self,
        events: impl Iterator<Item = &'a HandoffEvent>,
    ) -> Result<Vec<DailyVolume>, String> {
        let zone = if self.config.schema_version >= 2 {
            cerulean_domain::timezone::require_archived_iana_rules()?;
            Some(cerulean_domain::timezone::parse(
                &self
                    .config
                    .execution_scope
                    .as_ref()
                    .ok_or("missing timezone scope")?
                    .project
                    .timezone,
            )?)
        } else {
            None
        };
        crate::handoff::project_handoff_refs(events, &HandoffOptions::default(), zone)
    }
    fn handoffs_for<'a>(
        &self,
        entries: impl Iterator<Item = &'a Entry>,
    ) -> Result<Vec<DailyVolume>, String> {
        let events = self.handoff_events(entries)?;
        self.render_handoffs(events.iter())
    }
    fn prepare_handoffs(
        &self,
        entry: &Entry,
        next: &WorkflowState,
    ) -> Result<(Vec<DailyVolume>, Option<HandoffEvent>), String> {
        if let Some(cached) = self
            .handoff_cache
            .as_ref()
            .filter(|c| c.head == self.index_head())
        {
            let pending = handoff_cache::event(entry, next, cached.events.len() as u64 + 1)?;
            let daily = self.render_handoffs(cached.events.iter().chain(pending.iter()))?;
            Ok((daily, pending))
        } else {
            Ok((
                self.handoffs_for(self.entries.iter().chain(std::iter::once(entry)))?,
                None,
            ))
        }
    }
    fn filename(entry: &Entry) -> String {
        format!(
            "{:012}_{}.json",
            entry.body.event.sequence, entry.body.operation_id
        )
    }
    fn load(&mut self, allow_checkpoint: bool) -> Result<(), String> {
        let mut paths = fs::read_dir(self.root.join("events"))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        paths.sort_by_key(|e| e.file_name());
        let paths: Vec<_> = paths
            .into_iter()
            .map(|p| p.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("json"))
            .collect();
        if allow_checkpoint {
            if let Some(cached) = crate::replay_checkpoint::load(
                &self.root,
                "workflow",
                &self.bindings_digest,
                &paths,
            ) {
                if let crate::replay_checkpoint::Payload::Workflow { state, entries } =
                    cached.payload
                {
                    if entries.len() == cached.proofs.len() {
                        // Never cache filesystem-dependent reader/monetary permission checks.
                        for entry in &entries {
                            self.check_replay_guards(entry)?;
                        }
                        if self
                            .core
                            .resume_authenticated_checkpoint(
                                state,
                                entries.iter().map(|e| e.body.event.clone()).collect(),
                            )
                            .is_ok()
                        {
                            self.entries = entries;
                            self.checkpoint.restore(cached.proofs);
                        }
                    }
                }
            }
        }
        for path in paths.into_iter().skip(self.checkpoint.restored) {
            let bytes = read_event_bytes(&path)?;
            let entry: Entry = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            self.check_replay_guards(&entry)?;
            require(
                entry.body.schema_version == 1
                    && entry.body.bindings_digest == self.bindings_digest,
                "journal schema or binding mismatch",
            )?;
            require(
                valid_id(&entry.body.operation_id)
                    && path.file_name().unwrap_or_default().to_string_lossy()
                        == Self::filename(&entry),
                "invalid event filename",
            )?;
            require(
                entry.body.event.sequence as usize == self.entries.len() + 1
                    && entry.body.previous_hash
                        == self
                            .entries
                            .last()
                            .map(|e| e.hash.as_str())
                            .unwrap_or("genesis")
                    && entry.hash == digest(&entry.body)?,
                "journal integrity failure",
            )?;
            require(
                !self
                    .entries
                    .iter()
                    .any(|e| e.body.operation_id == entry.body.operation_id),
                "duplicate operation ID",
            )?;
            let time = chrono::DateTime::parse_from_rfc3339(&entry.body.received_at)
                .map_err(|_| "invalid event time")?;
            if let Some(prev) = self.entries.last() {
                require(
                    time >= chrono::DateTime::parse_from_rfc3339(&prev.body.received_at)
                        .map_err(|_| "invalid event time")?,
                    "event clock regressed",
                )?;
            }
            self.check_context(&entry.body.event.command, &entry.body.context)?;
            self.check_execution(
                &entry.body.operation_id,
                &entry.body.event.principal,
                &entry.body.event.command,
                &entry.body.context,
                false,
            )?;
            self.core.apply(
                &entry.body.event.principal,
                entry.body.event.command.clone(),
            )?;
            self.checkpoint.record(Self::filename(&entry), &bytes);
            self.entries.push(entry);
        }
        Ok(())
    }
    fn check_replay_guards(&self, entry: &Entry) -> Result<(), String> {
        crate::reader_generation::require_workflow_context(&self.root, &entry.body.context)?;
        if matches!(
            entry.body.event.command,
            WorkflowCommand::ConfirmApiGenerationResolution { .. }
                | WorkflowCommand::ConfirmApiCountResolution { .. }
        ) {
            let data = self
                .root
                .parent()
                .and_then(Path::parent)
                .ok_or("API confirmation needs original Host root")?;
            crate::currency_api_resolution_guard::require_protected(data, true)?;
        }
        Ok(())
    }
    fn last_intent(&self, task: &str, attempt: &str) -> Option<&Entry> {
        self.entries.iter().rev().find(|e|matches!(&e.body.event.command,WorkflowCommand::DispatchIntent{task:t,attempt:a} if t==task&&a==attempt))
    }
    fn last_start(&self, task: &str, attempt: &str) -> Option<&Entry> {
        self.entries.iter().rev().find(|e| matches!(&e.body.event.command,
            WorkflowCommand::Started{task:t,attempt:a} | WorkflowCommand::Reconcile{task:t,attempt:a,outcome:cerulean_domain::workflow::ReconcileOutcome::Running,..} if t==task&&a==attempt))
    }
    fn check_context(&self, command: &WorkflowCommand, ctx: &CommandContext) -> Result<(), String> {
        for s in [
            &ctx.approval_source,
            &ctx.thread_id,
            &ctx.turn_id,
            &ctx.cwd,
            &ctx.terminal_status,
            &ctx.cancellation_source,
        ]
        .into_iter()
        .flatten()
        {
            require(
                !s.trim().is_empty() && s.encode_utf16().count() <= 8192,
                "invalid context value",
            )?;
        }
        match command {
            WorkflowCommand::RetireUnknownApi { .. }
            | WorkflowCommand::RetireUnknownDesktop { .. } => require(
                *ctx == CommandContext::default() && self.config.execution_scope.is_some(),
                "local API retirement requires strict scope and plain user context",
            )?,
            WorkflowCommand::AbandonUnsent { .. } => require(
                *ctx == CommandContext::default(),
                "local unsent disposition requires plain user context",
            )?,
            WorkflowCommand::Approve { .. } => {
                require(ctx.approval_source.is_some(), "approval source required")?
            }
            WorkflowCommand::DispatchIntent { .. } => require(
                ctx.thread_id.is_some() && ctx.cwd.is_some(),
                "dispatch binding required",
            )?,
            WorkflowCommand::Started { task, attempt }
            | WorkflowCommand::Submit { task, attempt, .. }
            | WorkflowCommand::CancelIntent { task, attempt, .. } => {
                let sent = self
                    .last_intent(task, attempt)
                    .ok_or("dispatch intent missing")?;
                let controller_request = self.attempt_record(task, attempt).is_some_and(|a| {
                    (a.binding.adapter_id == "omp"
                        || cerulean_domain::api_worker::is_api_adapter(&a.binding.adapter_id))
                        && matches!(
                            a.binding.identity_strength,
                            cerulean_domain::execution::IdentityStrength::ControllerRequest(_)
                        )
                });
                require(
                    ctx.thread_id == sent.body.context.thread_id
                        && if controller_request
                            || self.attempt_record(task, attempt).is_some_and(|a| {
                                a.binding.identity_strength
                                    == cerulean_domain::execution::IdentityStrength::UiObservation
                                    && self
                                        .revision()
                                        .and_then(|r| r.task(task).ok())
                                        .and_then(|s| s.desktop_worker.as_ref())
                                        .is_some_and(|d| d.validate_attempt(&a.binding).is_ok())
                            })
                        {
                            ctx.turn_id.is_none()
                        } else {
                            ctx.turn_id.is_some()
                        },
                    "executor binding mismatch",
                )?;
                let ack = self.last_start(task, attempt);
                if matches!(command, WorkflowCommand::Submit { .. }) {
                    require(
                        ack.is_some_and(|e| e.body.context.turn_id == ctx.turn_id)
                            && ctx.terminal_status.as_deref() == Some("completed"),
                        "completed turn evidence required",
                    )?;
                }
                if matches!(command, WorkflowCommand::CancelIntent { .. }) {
                    require(
                        ctx.cancellation_source.is_some(),
                        "cancellation source required",
                    )?;
                    if let Some(ack) = ack {
                        require(
                            ack.body.context.turn_id == ctx.turn_id,
                            "cancellation turn mismatch",
                        )?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
    pub fn command(
        &mut self,
        id: &str,
        principal: &str,
        command: WorkflowCommand,
        context: CommandContext,
        require_new: bool,
    ) -> Result<Entry, String> {
        self.command_observed(id, principal, command, context, require_new, |_| {})
    }
    /// Hook is supplied only by trusted host/tests, never deserialized from RPC input.
    pub fn command_observed(
        &mut self,
        id: &str,
        principal: &str,
        command: WorkflowCommand,
        context: CommandContext,
        require_new: bool,
        mut observer: impl FnMut(CommitPoint),
    ) -> Result<Entry, String> {
        require(!self.poisoned, "workflow requires reopen")?;
        crate::reader_generation::require_workflow_context(&self.root, &context)?;
        require(valid_id(id), "invalid operation ID")?;
        if let Some(previous) = self.entries.iter().find(|e| e.body.operation_id == id) {
            require(
                !(require_new
                    || matches!(
                        command,
                        WorkflowCommand::DispatchIntent { .. }
                            | WorkflowCommand::CancelIntent { .. }
                    )),
                "intent already recorded; reconcile without resending",
            )?;
            require(
                previous.body.event.principal == principal
                    && previous.body.event.command == command
                    && previous.body.context == context,
                "operation ID conflict",
            )?;
            return Ok(previous.clone());
        }
        self.check_context(&command, &context)?;
        self.check_execution(id, principal, &command, &context, true)?;
        let mut next = self.core.clone();
        let event = next.apply(principal, command)?;
        let mut received_at = Utc::now();
        if let Some(prev) = self.entries.last() {
            received_at = received_at.max(
                chrono::DateTime::parse_from_rfc3339(&prev.body.received_at)
                    .map_err(|_| "invalid event time")?
                    .with_timezone(&Utc),
            );
        }
        let body = EntryBody {
            schema_version: 1,
            operation_id: id.into(),
            received_at: received_at.to_rfc3339(),
            bindings_digest: self.bindings_digest.clone(),
            event,
            context,
            previous_hash: self
                .entries
                .last()
                .map(|e| e.hash.clone())
                .unwrap_or("genesis".into()),
        };
        let entry = Entry {
            hash: digest(&body)?,
            body,
        };
        // Reject unrenderable text before committing an authoritative event.
        let previous_head = self.index_head();
        let (handoffs, pending_handoff) = self.prepare_handoffs(&entry, next.state())?;
        let bytes = serde_json::to_vec(&entry).map_err(|e| e.to_string())?;
        require(bytes.len() <= MAX_EVENT_BYTES, "event too large")?;
        let destination = self.root.join("events").join(Self::filename(&entry));
        require(!destination.exists(), "event destination already exists")?;
        self.poisoned = true;
        observer(CommitPoint::BeforeEventWrite);
        let temp = temp_path(&destination);
        write_synced(&temp, &bytes)?;
        observer(CommitPoint::TempSynced);
        fs::rename(&temp, &destination).map_err(|e| e.to_string())?;
        observer(CommitPoint::EventPublished);
        self.core = next;
        self.checkpoint.record(Self::filename(&entry), &bytes);
        self.entries.push(entry.clone());
        if let Some(cached) = self
            .handoff_cache
            .take()
            .filter(|c| c.head == previous_head)
        {
            // Optional retained cache cannot turn a committed event into an error.
            self.handoff_cache = cached.advance(self.index_head(), pending_handoff).ok();
        }
        // Only the event determines command success. Derived files are rebuildable.
        self.poisoned = false;
        self.refresh_index(handoffs);
        observer(CommitPoint::ProjectionQueued);
        Ok(entry)
    }
    fn project(&mut self) -> Result<(), String> {
        let events = self.handoff_events(self.entries.iter())?;
        let handoffs = self.render_handoffs(events.iter())?;
        self.handoff_cache = None;
        self.handoff_cache =
            handoff_cache::Cached::new(self.index_head(), events, &self.index).ok();
        self.refresh_index(handoffs);
        Ok(())
    }
}
