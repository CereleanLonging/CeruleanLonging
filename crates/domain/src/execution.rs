//! Product execution contracts. Authentication and filesystem observation belong
//! to the trusted host; deserializing these values never grants permission.
use crate::workflow::{Principal, Role, Workflow, WorkflowCommand, WorkflowPlan};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use ts_rs::TS;
mod assertions;
pub use assertions::*;
mod worker_directory;
pub use worker_directory::*;
mod workspaces;
pub use workspaces::*;

pub fn digest<T: Serialize>(value: &T) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).map_err(|e| e.to_string())?)
    ))
}
pub fn require(ok: bool, message: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(message.into())
    }
}
pub fn identifier(s: &str) -> Result<(), String> {
    require(
        !s.is_empty()
            && s.len() <= 80
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-')),
        "invalid identifier",
    )
}
pub fn sha256(s: &str) -> Result<(), String> {
    require(
        s.len() == 64
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "invalid sha256",
    )
}
pub fn timestamp(s: &str) -> Result<(), String> {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|_| ())
        .map_err(|_| "invalid timestamp".into())
}
/// Portable relative artifact names; Windows aliases/streams/separators rejected.
pub fn artifact_path(s: &str) -> Result<(), String> {
    require(!s.is_empty() && s.len() <= 2048, "invalid artifact path")?;
    for part in s.split('/') {
        require(
            !part.is_empty()
                && part != "."
                && part != ".."
                && !part.ends_with([' ', '.'])
                && !part
                    .chars()
                    .any(|c| c.is_control() || "\\:<>\"|?*".contains(c)),
            "unsafe artifact path",
        )?;
        let stem = part
            .split('.')
            .next()
            .unwrap()
            .trim_end_matches(' ')
            .to_ascii_uppercase();
        require(
            !matches!(
                stem.as_str(),
                "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
            ) && !(stem.starts_with("COM") || stem.starts_with("LPT"))
                .then(|| &stem[3..])
                .is_some_and(|s| {
                    matches!(
                        s,
                        "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                    )
                }),
            "reserved artifact path",
        )?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Project {
    pub id: String,
    pub root: String,
    pub timezone: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ToolPermission {
    ReadFiles,
    WriteFiles,
    RunCommands,
    Network,
    DesktopSend,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency_budget: Option<crate::currency_budget::BudgetSettings>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supervision: Option<crate::supervision::SupervisionPolicy>,
    pub version: u32,
    pub tools: Vec<ToolPermission>,
    pub max_requests: u32,
    pub max_seconds: u32,
    // Integer cents avoid floating point approval hash ambiguity; unknown usage
    // remains explicit, not represented as zero measured cost.
    pub budget_usd_cents: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptanceCheck {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expected_assertions: Vec<String>,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub program: String,
    pub args: Vec<String>,
    pub timeout_seconds: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<ValidationTool>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ValidationFile {
    pub path: String,
    pub sha256: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ValidationTool {
    pub executable: ValidationFile,
    pub inputs: Vec<ValidationFile>,
    pub environment: BTreeMap<String, String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegisteredValidator {
    pub id: String,
    pub name: String,
    pub tool: ValidationTool,
    pub registered_by: String,
    pub registered_at: String,
}
impl RegisteredValidator {
    pub fn validate(&self) -> Result<(), String> {
        identifier(&self.id)?;
        require(
            !self.name.trim().is_empty()
                && self.name.len() <= 200
                && !self.name.chars().any(char::is_control),
            "invalid validator name",
        )?;
        require(self.registered_by == "user", "invalid validator registrar")?;
        chrono::DateTime::parse_from_rfc3339(&self.registered_at)
            .map_err(|_| "invalid validator registration time")?;
        self.tool.validate()
    }
}
impl ValidationTool {
    pub fn validate(&self) -> Result<(), String> {
        require(self.inputs.len() <= 64, "too many validator inputs")?;
        let mut paths = BTreeSet::new();
        for file in std::iter::once(&self.executable).chain(&self.inputs) {
            require(
                std::path::Path::new(&file.path).is_absolute()
                    && file.path.len() <= 4096
                    && !file.path.chars().any(char::is_control),
                "invalid validator absolute path",
            )?;
            sha256(&file.sha256)?;
            require(
                paths.insert(file.path.to_lowercase()),
                "duplicate validator path",
            )?;
        }
        require(
            self.executable.path.to_ascii_lowercase().ends_with(".exe"),
            "native validator executable required",
        )?;
        // Only non-secret, bounded, literal validation controls. No PATH, loader
        // hooks, credentials or inherited environment overrides in this version.
        for (key, value) in &self.environment {
            let valid = match key.as_str() {
                "CI" => matches!(value.as_str(), "true" | "false" | "1" | "0"),
                "NO_COLOR" | "PYTHONNOUSERSITE" | "PYTHONDONTWRITEBYTECODE" => value == "1",
                "RUST_BACKTRACE" => matches!(value.as_str(), "0" | "1" | "full"),
                "CARGO_NET_OFFLINE" | "NPM_CONFIG_OFFLINE" => {
                    matches!(value.as_str(), "true" | "false")
                }
                _ => false,
            };
            require(valid, "unsupported validator environment setting")?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskExecution {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_directory: Option<WorkerDirectory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supervision_override: Option<crate::supervision::SupervisionOverride>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desktop_worker: Option<crate::desktop_sessions::DesktopWorkerSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_worker: Option<crate::api_worker::ApiWorkerSelection>,
    pub task_id: String,
    pub adapter_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker: Option<WorkerSelection>,
    pub artifact_paths: Vec<String>,
    pub checks: Vec<AcceptanceCheck>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerSelection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub omp_runtime_parent: Option<String>,
    pub provider: String,
    pub model: String,
    pub executable: ValidationFile,
}
impl WorkerSelection {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(path) = &self.omp_runtime_parent {
            require(
                self.provider == "deepseek"
                    && !path.is_empty()
                    && path.len() <= 4096
                    && !path.chars().any(char::is_control),
                "invalid OMP runtime parent",
            )?;
        }
        identifier(&self.provider)?;
        require(
            !self.model.is_empty()
                && self.model.len() <= 200
                && !self.model.chars().any(char::is_control),
            "invalid worker model",
        )?;
        ValidationTool {
            executable: self.executable.clone(),
            inputs: vec![],
            environment: BTreeMap::new(),
        }
        .validate()
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanRevision {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub management_setup_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<GitWorktreePolicy>,
    pub schema_version: u32,
    pub project: Project,
    pub workflow: WorkflowPlan,
    pub policy: ExecutionPolicy,
    pub execution: Vec<TaskExecution>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snapshot_policy: Option<SnapshotPolicy>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SnapshotPolicy {
    pub exclude: Vec<String>,
    pub max_files: u32,
    pub max_bytes: u32,
}
impl SnapshotPolicy {
    pub fn excludes(&self, path: &str) -> bool {
        let path = path.to_lowercase();
        self.exclude.iter().any(|prefix| {
            let prefix = prefix.to_lowercase();
            path == prefix || path.starts_with(&(prefix + "/"))
        })
    }
    pub fn validate(&self) -> Result<(), String> {
        require(
            self.max_files > 0
                && self.max_files <= 10000
                && self.max_bytes > 0
                && self.max_bytes <= 512 * 1024 * 1024,
            "invalid snapshot limits",
        )?;
        let mut paths = BTreeSet::new();
        for path in &self.exclude {
            artifact_path(path)?;
            require(
                paths.insert(path.to_lowercase()),
                "duplicate snapshot exclusion",
            )?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceSnapshot {
    pub operation_id: String,
    pub manifest_sha256: String,
}
impl PlanRevision {
    pub fn supervision_for(
        &self,
        task: &str,
    ) -> Result<Option<&crate::supervision::SupervisionPolicy>, String> {
        let execution = self
            .execution
            .iter()
            .find(|e| e.task_id == task)
            .ok_or("监督任务不属于原方案")?;
        crate::supervision::effective_policy(
            self.policy.supervision.as_ref(),
            execution.supervision_override.as_ref(),
            &execution.adapter_id,
        )
    }
    pub fn validate(&self) -> Result<(), String> {
        self.validate_workspace_policy()?;
        if let Some(binding) = &self.management_setup_digest {
            require(
                binding.len() == 64
                    && binding
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "invalid management setup binding",
            )?;
        }
        if let Some(budget) = &self.policy.currency_budget {
            budget.validate()?;
            require(
                self.policy.budget_usd_cents.is_none(),
                "金额预算不能混用旧金额字段",
            )?;
            for task in &self.execution {
                let api = task
                    .api_worker
                    .as_ref()
                    .ok_or("整个项目的金额上限需要可计量的API执行者")?;
                budget.require_model(&api.target, &api.model)?;
            }
        }
        if let Some(policy) = &self.policy.supervision {
            policy.validate()?;
        }
        if let Some(policy) = &self.snapshot_policy {
            policy.validate()?;
            for task in &self.execution {
                for path in &task.artifact_paths {
                    require(
                        !policy.excludes(path),
                        "artifact excluded from verification snapshot",
                    )?;
                }
            }
        }
        require(
            self.schema_version == 1 && self.policy.version > 0,
            "unsupported plan or policy version",
        )?;
        identifier(&self.project.id)?;
        require(
            !self.project.root.trim().is_empty(),
            "unsupported project configuration",
        )?;
        crate::timezone::parse(&self.project.timezone)?;
        require(
            self.policy.max_requests > 0 && self.policy.max_seconds > 0,
            "finite execution limits required",
        )?;
        let mut core = Workflow::new(vec![Principal {
            id: "chief".into(),
            role: Role::Chief,
            provider: Some("openai".into()),
        }])?;
        core.apply(
            "chief",
            WorkflowCommand::Propose {
                plan: self.workflow.clone(),
            },
        )?;
        let tasks: BTreeSet<_> = self.workflow.tasks.iter().map(|t| t.id.as_str()).collect();
        let specs: BTreeSet<_> = self.execution.iter().map(|t| t.task_id.as_str()).collect();
        require(
            tasks == specs && specs.len() == self.execution.len(),
            "execution tasks mismatch",
        )?;
        for e in &self.execution {
            let supervision = self.supervision_for(&e.task_id)?;
            if let Some(id) = &e.profile_id {
                identifier(id)?;
            }
            identifier(&e.adapter_id)?;
            if let Some(desktop) = &e.desktop_worker {
                desktop.validate()?;
                require(
                    e.worker.is_none()
                        && e.api_worker.is_none()
                        && e.adapter_id == desktop.adapter_id()
                        && desktop.binding.project_id == self.project.id
                        && desktop.project_root
                            == e.worker_directory.as_ref().map_or_else(
                                || Ok(self.project.root.clone()),
                                |d| d.root(&self.project.root),
                            )?,
                    "桌面执行配置与方案范围不符",
                )?;
                require(
                    self.policy.tools.contains(&ToolPermission::DesktopSend),
                    "桌面任务必须明确批准DesktopSend权限",
                )?;
                require(
                    self.policy.budget_usd_cents.is_none()
                        && (supervision.is_none()
                            || (e.adapter_id == "qoder" && e.worker_directory.is_some())),
                    "桌面监督需要Qoder独立执行目录，可计量金额预算尚未接入",
                )?;
            }
            require(
                matches!(e.adapter_id.as_str(), "qoder" | "workbuddy")
                    == e.desktop_worker.is_some(),
                "桌面执行器需要原批准的会话选择",
            )?;
            if let Some(api) = &e.api_worker {
                api.validate()?;
                require(
                    e.worker.is_none() && e.adapter_id == api.target.provider().id(),
                    "API selection conflicts with adapter or executable",
                )?;
                require(
                    api.file_tools.iter().all(|p| self.policy.tools.contains(p)),
                    "API worker file permission exceeds plan",
                )?;
                require(
                    self.policy.budget_usd_cents.is_none(),
                    "API currency budget requires an approved pricing policy",
                )?;
            }
            require(
                !crate::api_worker::is_api_adapter(&e.adapter_id) || e.api_worker.is_some(),
                "API adapter requires an explicit approved selection",
            )?;
            if let Some(worker) = &e.worker {
                worker.validate()?;
                require(
                    worker.omp_runtime_parent.is_none() || e.adapter_id == "omp",
                    "OMP runtime parent requires OMP adapter",
                )?;
            }
            require(
                !e.artifact_paths.is_empty() && !e.checks.is_empty(),
                "artifact scope and independent checks required",
            )?;
            let mut paths = BTreeSet::new();
            for path in &e.artifact_paths {
                artifact_path(path)?;
                require(paths.insert(path.to_lowercase()), "duplicate artifact path")?;
            }
            let mut ids = BTreeSet::new();
            for check in &e.checks {
                validate_assertion_ids(&check.expected_assertions)?;
                if let Some(name) = &check.name {
                    require(
                        !name.trim().is_empty()
                            && name.len() <= 160
                            && !name.chars().any(char::is_control),
                        "invalid acceptance name",
                    )?;
                }
                if let Some(tool) = &check.tool {
                    tool.validate()?;
                }
                identifier(&check.id)?;
                require(
                    ids.insert(&check.id)
                        && !check.program.trim().is_empty()
                        && check.timeout_seconds > 0
                        && check.timeout_seconds <= self.policy.max_seconds
                        && !check.program.contains('\0')
                        && !check.args.iter().any(|s| s.contains('\0')),
                    "invalid acceptance check",
                )?;
            }
        }
        require(
            self.policy.tools.contains(&ToolPermission::RunCommands),
            "checks need command permission",
        )?;
        self.validate_integration()?;
        Ok(())
    }
    pub fn approval_digest(&self) -> Result<String, String> {
        self.validate()?;
        digest(self)
    }
    pub fn task(&self, id: &str) -> Result<&TaskExecution, String> {
        self.execution
            .iter()
            .find(|e| e.task_id == id)
            .ok_or("task execution missing".into())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Approval {
    /// Only the host's fixed ordinary-account/ACL preparation, never a worker grant.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_account_setup: Option<WorkerAccountAuthorization>,
    pub plan_id: String,
    pub revision: u32,
    pub policy_version: u32,
    pub revision_digest: String,
    pub user_id: String,
    pub approved_at: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerAccountAuthorization {
    pub setup_digest: String,
    pub scope_digest: String,
}
impl Approval {
    /// authenticated_user is host channel identity, never a field accepted from an agent.
    pub fn validate(
        &self,
        plan: &PlanRevision,
        authenticated_user: &str,
        active_policy_version: u32,
    ) -> Result<(), String> {
        if let Some(scope) = &self.worker_account_setup {
            sha256(&scope.setup_digest)?;
            sha256(&scope.scope_digest)?;
            require(
                plan.management_setup_digest.as_ref() == Some(&scope.setup_digest),
                "account authorization setup differs from plan",
            )?;
        }
        identifier(&self.user_id)?;
        require(self.user_id == authenticated_user, "approval user mismatch")?;
        require(
            self.plan_id == plan.workflow.id
                && self.revision == plan.workflow.version
                && self.policy_version == plan.policy.version
                && self.policy_version == active_policy_version
                && self.revision_digest == plan.approval_digest()?,
            "stale or altered approval",
        )?;
        timestamp(&self.approved_at)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum IdentityStrength {
    ProviderId,
    UiObservation,
    /// Provider session, but controller-owned request/connection correlation.
    /// Never implies a provider turn that can be looked up after reconnecting.
    ControllerRequest(ControllerRequestIdentity),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ControllerRequestIdentity {
    pub connection_id: String,
    pub request_id: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionBinding {
    pub adapter_id: String,
    pub adapter_version: String,
    pub session_id: String,
    pub identity_strength: IdentityStrength,
    pub workspace: String,
}
impl SessionBinding {
    pub fn validate_controller_identity(&self) -> Result<(), String> {
        if let IdentityStrength::ControllerRequest(ControllerRequestIdentity {
            connection_id,
            request_id,
        }) = &self.identity_strength
        {
            identifier(connection_id)?;
            identifier(request_id)?;
            require(
                self.adapter_id == "omp" || crate::api_worker::is_api_adapter(&self.adapter_id),
                "controller request binding requires OMP or API worker",
            )?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Attempt {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_proof: Option<WorkspaceProof>,
    pub id: String,
    pub task_id: String,
    pub worker_id: String,
    pub revision_digest: String,
    pub fence: u32,
    pub binding: SessionBinding,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum ReceiptStatus {
    Running,
    Completed,
    Cancelled,
    Failed,
    Unknown,
    #[serde(rename = "not-started")]
    NotStarted,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionReceipt {
    pub schema_version: u32,
    pub task_id: String,
    pub attempt_id: String,
    pub fence: u32,
    pub binding: SessionBinding,
    pub turn_id: Option<String>,
    pub observed_at: String,
    pub status: ReceiptStatus,
    pub evidence_sha256: String,
}
impl ExecutionReceipt {
    pub fn validate(
        &self,
        plan: &PlanRevision,
        attempt: &Attempt,
        expected_turn: Option<&str>,
    ) -> Result<(), String> {
        identifier(&attempt.id)?;
        identifier(&attempt.worker_id)?;
        self.binding.validate_controller_identity()?;
        let spec = plan.task(&attempt.task_id)?;
        if let Some(desktop) = &spec.desktop_worker {
            desktop.validate_attempt(&self.binding)?;
            require(self.turn_id.is_none(), "桌面观察不能伪造供应商轮次ID")?;
        }
        require(
            self.schema_version == 1
                && self.task_id == attempt.task_id
                && self.attempt_id == attempt.id
                && self.fence > 0
                && self.fence == attempt.fence
                && self.binding == attempt.binding
                && self.binding.adapter_id == spec.adapter_id
                && self.binding.workspace == plan.execution_root(&attempt.task_id, &attempt.id)?
                && attempt.revision_digest == plan.approval_digest()?,
            "receipt attempt or binding mismatch",
        )?;
        plan.validate_attempt_workspace(attempt)?;
        require(
            !self.binding.session_id.trim().is_empty()
                && !self.binding.adapter_version.trim().is_empty()
                && !self.observed_at.trim().is_empty(),
            "incomplete receipt",
        )?;
        require(
            self.turn_id.as_deref() == expected_turn
                && self.turn_id.as_ref().is_none_or(|s| !s.trim().is_empty()),
            "receipt turn mismatch",
        )?;
        if self.binding.identity_strength == IdentityStrength::ProviderId
            && self.status != ReceiptStatus::NotStarted
        {
            require(self.turn_id.is_some(), "provider turn required")?;
        }
        if let IdentityStrength::ControllerRequest(ControllerRequestIdentity {
            connection_id,
            request_id,
        }) = &self.binding.identity_strength
        {
            identifier(connection_id)?;
            identifier(request_id)?;
            require(
                (self.binding.adapter_id == "omp"
                    || crate::api_worker::is_api_adapter(&self.binding.adapter_id))
                    && self.turn_id.is_none(),
                "controller request requires OMP or API binding without a provider turn",
            )?;
        }
        timestamp(&self.observed_at)?;
        sha256(&self.evidence_sha256)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionScope {
    pub project: Project,
    pub policy_version: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum ExecutionEvidence {
    Revision {
        revision: PlanRevision,
    },
    Approval {
        approval: Approval,
    },
    Attempt {
        attempt: Attempt,
    },
    Receipt {
        receipt: ExecutionReceipt,
        manifest: Option<ArtifactManifest>,
    },
    StartedTurnCancellation {
        receipt: ExecutionReceipt,
        proof: Box<StartedTurnCancellationProof>,
    },
    Verification {
        report: VerificationReport,
    },
    VerificationRecheck {
        approval: EventAnchor,
        result: Box<crate::verification_recheck::RecheckResult>,
    },
}
/// References to immutable workflow events; no remote input is inferred.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventAnchor {
    pub sequence: u32,
    pub hash: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartedCancellationAnchors {
    pub started: EventAnchor,
    pub cancellation: EventAnchor,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InterruptedTurnObservation {
    pub thread_id: String,
    pub turn_id: String,
    pub workspace: String,
    pub status: String,
    pub items_view: String,
    pub item_count: u32,
    pub matching_turns: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StartedTurnCancellationProof {
    pub schema_version: u32,
    pub anchors: StartedCancellationAnchors,
    pub observation: InterruptedTurnObservation,
    pub observed_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionSnapshot {
    pub project: Project,
    pub revision: Option<PlanRevision>,
    pub revision_digest: Option<String>,
    pub approval: Option<Approval>,
    pub state: crate::workflow::WorkflowState,
    pub sequence: u32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum VerificationState {
    Running,
    Cancelling,
    Succeeded,
    Failed,
    Unknown,
    Abandoned,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VerificationJob {
    pub project_id: String,
    pub operation_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub state: VerificationState,
    pub started_at: String,
    pub updated_at: String,
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconciliation_digest: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Artifact {
    pub path: String,
    // JSON safe bounded file size; core rejects files larger than its configured budget.
    pub bytes: u32,
    pub sha256: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArtifactManifest {
    pub schema_version: u32,
    pub revision_digest: String,
    pub task_id: String,
    pub attempt_id: String,
    pub receipt_digest: String,
    pub artifacts: Vec<Artifact>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckObservation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assertion_error: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assertions: Vec<AssertionResult>,
    pub check_id: String,
    pub command_digest: String,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub stdout_sha256: String,
    pub stderr_sha256: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VerificationReport {
    pub schema_version: u32,
    pub verifier_id: String,
    pub observed_at: String,
    pub manifest_digest: String,
    pub checks: Vec<CheckObservation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_snapshot: Option<SourceSnapshot>,
}
impl VerificationReport {
    /// Validate a report already received from the trusted independent runner.
    /// This does not establish its provenance or observe current artifact bytes.
    pub fn require_pass(
        &self,
        plan: &PlanRevision,
        manifest: &ArtifactManifest,
        trusted_verifier: &str,
    ) -> Result<(), String> {
        if plan.snapshot_policy.is_some() {
            let snapshot = self
                .source_snapshot
                .as_ref()
                .ok_or("source snapshot evidence missing")?;
            identifier(&snapshot.operation_id)?;
            sha256(&snapshot.manifest_sha256)?;
        }
        identifier(&self.verifier_id)?;
        timestamp(&self.observed_at)?;
        require(
            self.schema_version == 1
                && manifest.schema_version == 1
                && self.verifier_id == trusted_verifier
                && manifest.revision_digest == plan.approval_digest()?
                && self.manifest_digest == digest(manifest)?,
            "verification binding mismatch",
        )?;
        let spec = plan.task(&manifest.task_id)?;
        let required: BTreeSet<_> = spec.artifact_paths.iter().collect();
        let observed: BTreeSet<_> = manifest.artifacts.iter().map(|a| &a.path).collect();
        require(
            required == observed && observed.len() == manifest.artifacts.len(),
            "verification artifact set mismatch",
        )?;
        for artifact in &manifest.artifacts {
            artifact_path(&artifact.path)?;
            sha256(&artifact.sha256)?;
        }
        let mut seen = BTreeSet::new();
        require(
            self.checks.len() == spec.checks.len(),
            "verification checks missing",
        )?;
        for result in &self.checks {
            require(
                seen.insert(&result.check_id),
                "duplicate verification check",
            )?;
            let check = spec
                .checks
                .iter()
                .find(|c| c.id == result.check_id)
                .ok_or("unexpected verification check")?;
            require(
                result.command_digest == digest(check)?
                    && result.exit_code == Some(0)
                    && !result.timed_out,
                "verification failed or command altered",
            )?;
            require(
                result.assertion_error.is_none(),
                "checker assertion report invalid",
            )?;
            require_assertions(check, &result.assertions, true)?;
            sha256(&result.stdout_sha256)?;
            sha256(&result.stderr_sha256)?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum CapabilityState {
    Supported,
    Unsupported,
    Unverified,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum Capability {
    Discover,
    Bind,
    Send,
    Observe,
    Reconcile,
    Cancel,
    Resume,
    Usage,
    Approval,
    Tools,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AdapterCapability {
    pub capability: Capability,
    pub state: CapabilityState,
    pub tested_version: Option<String>,
    pub evidence_sha256: Option<String>,
}
impl AdapterCapability {
    pub fn validate(&self) -> Result<(), String> {
        if self.state != CapabilityState::Unverified {
            require(
                self.tested_version
                    .as_ref()
                    .is_some_and(|v| !v.trim().is_empty()),
                "capability version missing",
            )?;
            sha256(
                self.evidence_sha256
                    .as_deref()
                    .ok_or("capability evidence missing")?,
            )?;
        }
        Ok(())
    }
}
