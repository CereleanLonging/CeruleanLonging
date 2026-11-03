//! User-authored project configuration. Saving it never approves execution.
use crate::execution::*;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
mod profiles;
pub use profiles::*;
mod management;
pub use management::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum ProjectWorkerAdapter {
    #[default]
    Codex,
    Omp,
    Openai,
    Anthropic,
    Deepseek,
    Qwen,
    Kimi,
    Grok,
    A6api,
    Qoder,
    Workbuddy,
}
impl ProjectWorkerAdapter {
    /// Applied only at live entry points, never when replaying old approvals.
    pub fn require_available(&self) -> Result<(), String> {
        if matches!(self, Self::Workbuddy) {
            crate::desktop_sessions::DesktopApp::Workbuddy.require_available()?;
        }
        Ok(())
    }
    pub fn is_codex(&self) -> bool {
        *self == Self::Codex
    }
    pub fn id(&self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Omp => "omp",
            Self::Openai => "openai",
            Self::Anthropic => "anthropic",
            Self::Deepseek => "deepseek",
            Self::Qwen => "qwen",
            Self::Kimi => "kimi",
            Self::Grok => "grok",
            Self::A6api => "a6api",
            Self::Qoder => "qoder",
            Self::Workbuddy => "workbuddy",
        }
    }
    pub fn provider(&self) -> &'static str {
        match self {
            Self::Codex => "openai",
            Self::Omp => "deepseek",
            _ => self.id(),
        }
    }
    pub fn worker_id(&self) -> String {
        format!("worker-{}", self.id())
    }
    pub fn is_api(&self) -> bool {
        !matches!(
            self,
            Self::Codex | Self::Omp | Self::Qoder | Self::Workbuddy
        )
    }
    pub fn is_desktop(&self) -> bool {
        matches!(self, Self::Qoder | Self::Workbuddy)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectSetupDraft {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chief_directory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub management_sources: Option<ManagementSources>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency_budget: Option<crate::currency_budget::BudgetSettings>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<WorkerProfileDraft>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<WorktreeSetupDraft>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desktop_worker: Option<crate::desktop_sessions::DesktopWorkerSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_worker: Option<crate::api_worker::ApiWorkerSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub omp_runtime_parent: Option<String>,
    #[serde(default, skip_serializing_if = "ProjectWorkerAdapter::is_codex")]
    pub worker_adapter: ProjectWorkerAdapter,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supervision: Option<crate::supervision::SupervisionPolicy>,
    pub project_id: String,
    pub requirement: String,
    pub codex_executable: String,
    pub worker_model: String,
    pub chief_model: String,
    pub deputy_model: String,
    pub validator_id: String,
    pub check_args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acceptance: Option<AcceptanceDraft>,
    pub artifact_paths: Vec<String>,
    pub worker_max_requests: u32,
    pub management_max_requests: u32,
    pub max_seconds: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectSetup {
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub worker_directories: std::collections::BTreeMap<String, WorkerDirectory>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chief_directory: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub management_sources: Option<ManagementSources>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profiles: Vec<WorkerProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<GitWorktreePolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desktop_worker: Option<crate::desktop_sessions::DesktopWorkerSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_worker: Option<crate::api_worker::ApiWorkerSelection>,
    #[serde(default, skip_serializing_if = "ProjectWorkerAdapter::is_codex")]
    pub worker_adapter: ProjectWorkerAdapter,
    pub project_id: String,
    pub version: u32,
    pub requirement: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker: Option<WorkerSelection>,
    pub chief_model: String,
    pub deputy_model: String,
    pub validator_id: String,
    pub check: AcceptanceCheck,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acceptance: Option<AcceptanceSetup>,
    pub artifact_paths: Vec<String>,
    pub policy: ExecutionPolicy,
    pub snapshot_policy: SnapshotPolicy,
    pub management_max_requests: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavedProjectSetup {
    pub setup: ProjectSetup,
    pub digest: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkerAccountSetupView {
    pub project_id: String,
    pub state: String,
    pub message: Option<String>,
    pub receipt_path: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorktreeSetupDraft {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integration: Option<IntegrationDraft>,
    #[serde(default, skip_serializing_if = "disabled")]
    pub inherit_dependencies: bool,
    pub base_commit: String,
    pub pool_root: String,
    pub git_executable: String,
}
fn disabled(value: &bool) -> bool {
    !*value
}
impl WorktreeSetupDraft {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(integration) = &self.integration {
            require(self.inherit_dependencies, "最终集成需要启用依赖输入")?;
            integration.validate(3600)?;
        }
        require(
            matches!(self.base_commit.len(), 40 | 64)
                && self
                    .base_commit
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "隔离目录需要完整的小写Git提交ID",
        )?;
        for path in [&self.pool_root, &self.git_executable] {
            require(
                path.len() <= 4096
                    && !path.chars().any(char::is_control)
                    && path
                        .split(['/', '\\'])
                        .all(|part| !matches!(part, "." | ".."))
                    && std::path::Path::new(path).is_absolute()
                    && std::path::Path::new(path).components().all(|c| {
                        !matches!(
                            c,
                            std::path::Component::CurDir | std::path::Component::ParentDir
                        )
                    }),
                "目录池和Git程序须使用规范绝对路径",
            )?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IntegrationDraft {
    pub task_id: String,
    pub artifact_paths: Vec<String>,
    pub validator_id: String,
    pub args: Vec<String>,
    pub timeout_seconds: u32,
}
impl IntegrationDraft {
    pub fn validate(&self, max_seconds: u32) -> Result<(), String> {
        identifier(&self.task_id)?;
        identifier(&self.validator_id)?;
        require(
            !self.artifact_paths.is_empty() && self.artifact_paths.len() <= 64,
            "最终交付需要1–64个文件",
        )?;
        let mut paths = std::collections::BTreeSet::new();
        for path in &self.artifact_paths {
            artifact_path(path)?;
            require(paths.insert(path.to_lowercase()), "最终交付文件重复")?;
        }
        require(
            self.timeout_seconds > 0
                && self.timeout_seconds <= max_seconds
                && self.args.len() <= 32
                && self
                    .args
                    .iter()
                    .all(|s| s.len() <= 4096 && !s.contains('\0')),
            "整体检查参数或时限无效",
        )
    }
}
impl ProjectIntegration {
    pub fn draft(&self) -> IntegrationDraft {
        IntegrationDraft {
            task_id: self.task_id.clone(),
            artifact_paths: self.artifact_paths.clone(),
            validator_id: self.validator_id.clone(),
            args: self.check.args.clone(),
            timeout_seconds: self.check.timeout_seconds,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptanceStageDraft {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expected_assertions: Vec<String>,
    pub id: String,
    pub name: String,
    pub validator_id: String,
    pub args: Vec<String>,
    pub timeout_seconds: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptanceDraft {
    pub schema_version: u32,
    pub stages: Vec<AcceptanceStageDraft>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptanceStage {
    pub validator_id: String,
    pub check: AcceptanceCheck,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AcceptanceSetup {
    pub schema_version: u32,
    pub stages: Vec<AcceptanceStage>,
}
impl ProjectSetup {
    pub fn require_available_adapters(&self) -> Result<(), String> {
        self.worker_adapter.require_available()?;
        for profile in &self.profiles {
            profile.worker_adapter.require_available()?;
        }
        Ok(())
    }
    pub fn integration(&self) -> Option<&ProjectIntegration> {
        self.worktree
            .as_ref()
            .and_then(|w| w.integration.as_deref())
    }
    pub fn task_checks(&self, task: &str) -> Vec<AcceptanceCheck> {
        self.integration()
            .filter(|i| i.task_id == task)
            .map_or_else(|| self.checks(), |i| vec![i.check.clone()])
    }
    pub fn task_artifacts(&self, task: &str) -> Vec<String> {
        self.integration()
            .filter(|i| i.task_id == task)
            .map_or_else(|| self.artifact_paths.clone(), |i| i.artifact_paths.clone())
    }
    pub fn task_validator(&self, task: &str) -> &str {
        self.integration()
            .filter(|i| i.task_id == task)
            .map_or(&self.validator_id, |i| &i.validator_id)
    }
    pub fn checks(&self) -> Vec<AcceptanceCheck> {
        self.acceptance.as_ref().map_or_else(
            || vec![self.check.clone()],
            |a| a.stages.iter().map(|s| s.check.clone()).collect(),
        )
    }
}
impl AcceptanceDraft {
    pub fn validate(&self, total_seconds: u32) -> Result<(), String> {
        require(
            self.schema_version == 1,
            "unsupported acceptance configuration",
        )?;
        require(
            !self.stages.is_empty() && self.stages.len() <= 8,
            "请配置 1–8 项验收检查",
        )?;
        let mut ids = std::collections::BTreeSet::new();
        let mut seconds = 0u64;
        let mut arg_bytes = 0usize;
        for stage in &self.stages {
            validate_assertion_ids(&stage.expected_assertions)?;
            identifier(&stage.id)?;
            identifier(&stage.validator_id)?;
            require(ids.insert(&stage.id), "验收检查 ID 重复")?;
            require(
                !stage.name.trim().is_empty()
                    && stage.name.len() <= 160
                    && !stage.name.chars().any(char::is_control),
                "验收名称须为 1–160 字节且无控制字符",
            )?;
            require(
                stage.timeout_seconds > 0 && stage.timeout_seconds <= 3600,
                "每项验收时限须为 1–3600 秒",
            )?;
            require(
                stage.args.len() <= 32
                    && stage
                        .args
                        .iter()
                        .all(|a| a.len() <= 4096 && !a.contains('\0')),
                "验收参数无效",
            )?;
            seconds += u64::from(stage.timeout_seconds);
            arg_bytes += stage.args.iter().map(String::len).sum::<usize>();
        }
        require(
            seconds <= u64::from(total_seconds),
            "验收时限合计不能超过单次执行时限",
        )?;
        require(arg_bytes <= 16384, "全部验收参数合计不能超过 16 KiB")
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ManagementView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refinement: Option<crate::management::RefinedDocument>,
    #[serde(default)]
    pub supervision_advice: Option<crate::supervision::SupervisionAdviceView>,
    pub operation_id: String,
    pub project_id: String,
    pub purpose: String,
    pub state: String,
    pub message: Option<String>,
    pub evidence_digest: String,
    pub wait_reason: Option<String>,
    pub user_reply: Option<String>,
    pub discard_reason: Option<String>,
    pub can_discard: bool,
    pub can_reply: bool,
    #[serde(default)]
    pub application_recovery: Option<ApplicationRecoveryReview>,
    #[serde(default)]
    pub advice_tracking_review: Option<AdviceTrackingReview>,
    #[serde(default)]
    pub advice_tracking_closure: Option<AdviceTrackingClosure>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdviceTrackingReview {
    pub evidence_digest: String,
    pub workflow_sequence: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct AdviceTrackingClosure {
    pub reason: String,
    pub at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ApplicationRecoveryReview {
    pub evidence_digest: String,
    pub target_kind: String,
    pub task_id: Option<String>,
    pub attempt_id: Option<String>,
    pub session_id: Option<String>,
    pub workflow_sequence: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ManagementSummary {
    pub operation_id: String,
    pub project_id: String,
    pub purpose: String,
    pub state: String,
    pub message: Option<String>,
    pub evidence_digest: String,
    pub can_discard: bool,
    pub can_reply: bool,
    pub has_wait: bool,
    pub has_reply: bool,
    pub has_discard: bool,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ManagementHistory {
    pub total: u32,
    pub offset: u32,
    pub next_offset: Option<u32>,
    pub project_id: Option<String>,
    pub revision: String,
    pub projects: Vec<ManagementCount>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ManagementCount {
    pub project_id: String,
    pub total: u32,
    pub pending: u32,
    pub running: u32,
    pub waiting: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectRun {
    pub id: String,
    pub project_id: String,
    pub revision_digest: String,
    pub setup_digest: String,
    pub state: RunState,
    pub message: String,
    pub acknowledged_decisions: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRevisionReview {
    pub project_id: String,
    pub revision_digest: String,
    pub evidence_digest: String,
    pub accepted_tasks: u32,
    pub cancelled_tasks: u32,
    #[serde(default)]
    pub retired_tasks: u32,
    pub pending_tasks: u32,
    pub submitted_tasks: u32,
    pub verified_submissions: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RunState {
    Active,
    Paused,
    Attention,
    Completed,
    Superseded,
}
impl RunState {
    pub fn terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Superseded)
    }
}
impl ProjectSetupDraft {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(chief) = &self.chief_directory {
            require(!chief.trim().is_empty(), "请指定独立的主指挥目录")?;
            require(
                self.worktree.is_none(),
                "固定模型目录不能与按尝试创建的Git目录池混用",
            )?;
            let checks = &self
                .acceptance
                .as_ref()
                .ok_or("隔离项目需要明确的验收检查及断言")?
                .stages;
            require(
                !checks.is_empty() && checks.iter().all(|s| !s.expected_assertions.is_empty()),
                "隔离项目的每项检查至少需要一个预期断言；退出码0不能代替检查覆盖",
            )?;
        }
        if let Some(sources) = &self.management_sources {
            sources.validate(&self.chief_model, self.currency_budget.is_some())?;
        }
        if let Some(budget) = &self.currency_budget {
            budget.validate()?;
            let worker = self
                .api_worker
                .as_ref()
                .ok_or("整个项目的金额上限需要可计量的API执行者")?;
            budget.require_model(&worker.target, &worker.model)?;
            budget.require_model(&crate::api_worker::ApiTarget::OpenAi {}, &self.chief_model)?;
            budget.require_model(&crate::api_worker::ApiTarget::OpenAi {}, &self.deputy_model)?;
        }
        validate_resource_keys(&self.resource_keys)?;
        require(self.profiles.len() <= 16, "最多配置16个附加执行者")?;
        let mut profile_ids = std::collections::BTreeSet::new();
        for profile in &self.profiles {
            merge_resource_keys(&self.resource_keys, &profile.resource_keys)?;
            identifier(&profile.id)?;
            require(profile_ids.insert(&profile.id), "执行配置ID重复")?;
            require(
                !profile.description.trim().is_empty()
                    && profile.description.len() <= 2000
                    && !profile.description.chars().any(char::is_control),
                "执行配置用途描述无效",
            )?;
            profile.apply(self).validate()?;
        }
        if let Some(worktree) = &self.worktree {
            worktree.validate()?;
            if let Some(integration) = &worktree.integration {
                integration.validate(self.max_seconds)?;
            }
            require(
                !self.worker_adapter.is_desktop(),
                "已有桌面会话不能迁入隔离任务目录",
            )?;
        }
        if let Some(desktop) = &self.desktop_worker {
            desktop.validate()?;
            require(
                self.worker_adapter.is_desktop()
                    && desktop.adapter_id() == self.worker_adapter.id()
                    && desktop.binding.project_id == self.project_id
                    && desktop.model_label == self.worker_model
                    && self.api_worker.is_none()
                    && self.codex_executable.is_empty()
                    && self.omp_runtime_parent.is_none(),
                "桌面配置与项目、执行器、模型或程序路径冲突",
            )?;
        }
        require(
            self.worker_adapter.is_desktop() == self.desktop_worker.is_some(),
            "桌面执行器需要明确的会话与权限配置",
        )?;
        match &self.api_worker {
            Some(api) => {
                api.validate()?;
                require(
                    self.worker_adapter.is_api()
                        && api.target.provider().id() == self.worker_adapter.id()
                        && api.model == self.worker_model
                        && self.codex_executable.is_empty()
                        && self.omp_runtime_parent.is_none(),
                    "API配置与执行器、模型或程序路径不匹配",
                )?;
            }
            None => require(!self.worker_adapter.is_api(), "API执行配置缺失")?,
        }
        if let Some(path) = &self.omp_runtime_parent {
            require(
                self.worker_adapter == ProjectWorkerAdapter::Omp
                    && !path.is_empty()
                    && path.len() <= 4096
                    && !path.chars().any(char::is_control),
                "OMP运行目录无效或执行器不匹配",
            )?;
        }
        crate::supervision::effective_policy(
            self.supervision.as_ref(),
            None,
            self.worker_adapter.id(),
        )?;

        if let Some(policy) = &self.supervision {
            policy.validate()?;
        }
        identifier(&self.project_id)?;
        identifier(&self.validator_id)?;
        if let Some(acceptance) = &self.acceptance {
            acceptance.validate(self.max_seconds)?;
            require(
                acceptance.stages[0].validator_id == self.validator_id
                    && acceptance.stages[0].args == self.check_args,
                "首项验收与兼容字段不一致",
            )?;
        }
        require(
            !self.requirement.trim().is_empty() && !self.requirement.contains('\0'),
            "需求不能为空，且不能含 NUL 字符",
        )?;
        for (model, allow_slash, ui_label) in [
            (
                &self.worker_model,
                self.worker_adapter.is_api(),
                self.worker_adapter.is_desktop(),
            ),
            (&self.chief_model, false, false),
            (&self.deputy_model, false, false),
        ] {
            require(
                !model.is_empty()
                    && model.len() <= 200
                    && (ui_label
                        || model.bytes().all(|c| {
                            c.is_ascii_alphanumeric()
                                || b"-_.:".contains(&c)
                                || (allow_slash && c == b'/')
                        })),
                "模型 ID 无效",
            )?;
        }
        require(
            self.codex_executable.len() <= 4096
                && self.check_args.len() <= 32
                && self
                    .check_args
                    .iter()
                    .all(|s| s.len() <= 4096 && !s.contains('\0')),
            "执行程序或验收参数无效",
        )?;
        require(
            !self.artifact_paths.is_empty() && self.artifact_paths.len() <= 64,
            "请指定 1–64 个产物文件",
        )?;
        let mut unique = std::collections::BTreeSet::new();
        for path in &self.artifact_paths {
            artifact_path(path)?;
            require(unique.insert(path.to_lowercase()), "产物文件重复")?;
        }
        require(
            (1..=1000).contains(&self.worker_max_requests)
                && (1..=1000).contains(&self.management_max_requests)
                && (10..=3600).contains(&self.max_seconds),
            "请求上限须为 1–1000，执行时限须为 10–3600 秒",
        )
    }
}
