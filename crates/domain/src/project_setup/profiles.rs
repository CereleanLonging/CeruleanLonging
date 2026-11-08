//! Named execution configurations, separate from authenticated adapter principals.
//! A task's profile choice and concrete model/tool bindings enter its approval.
use super::*;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerProfileDraft {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_reasoning_effort: Option<crate::reasoning::ReasoningEffort>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supervision_override: Option<crate::supervision::SupervisionOverride>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_keys: Vec<String>,
    pub id: String,
    pub description: String,
    pub worker_adapter: ProjectWorkerAdapter,
    pub codex_executable: String,
    pub worker_model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub omp_runtime_parent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_worker: Option<crate::api_worker::ApiWorkerSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desktop_worker: Option<crate::desktop_sessions::DesktopWorkerSelection>,
    pub artifact_paths: Vec<String>,
    pub acceptance: AcceptanceDraft,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerProfile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supervision_override: Option<crate::supervision::SupervisionOverride>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resource_keys: Vec<String>,
    pub id: String,
    pub description: String,
    pub worker_adapter: ProjectWorkerAdapter,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker: Option<WorkerSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_worker: Option<crate::api_worker::ApiWorkerSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desktop_worker: Option<crate::desktop_sessions::DesktopWorkerSelection>,
    pub artifact_paths: Vec<String>,
    pub acceptance: AcceptanceSetup,
}
impl WorkerProfileDraft {
    /// Validate/pin using the same single-execution rules; never recursively add profiles.
    pub fn apply(&self, base: &ProjectSetupDraft) -> ProjectSetupDraft {
        let mut result = base.clone();
        result.profiles.clear();
        result.resource_keys = self.resource_keys.clone();
        result.worker_adapter = self.worker_adapter;
        // Profile validation must use its effective policy rather than granting
        // the inherited Codex capability to an API, OMP or desktop worker.
        result.supervision = match &self.supervision_override {
            None => base.supervision.clone(),
            Some(crate::supervision::SupervisionOverride::Disabled {}) => None,
            Some(crate::supervision::SupervisionOverride::Enabled { policy }) => {
                Some(policy.clone())
            }
        };
        result.codex_executable = self.codex_executable.clone();
        result.worker_model = self.worker_model.clone();
        result.worker_reasoning_effort = self.worker_reasoning_effort;
        result.omp_runtime_parent = self.omp_runtime_parent.clone();
        result.api_worker = self.api_worker.clone();
        result.desktop_worker = self.desktop_worker.clone();
        result.artifact_paths = self.artifact_paths.clone();
        result.validator_id = self
            .acceptance
            .stages
            .first()
            .map_or_else(String::new, |s| s.validator_id.clone());
        result.check_args = self
            .acceptance
            .stages
            .first()
            .map_or_else(Vec::new, |s| s.args.clone());
        result.acceptance = Some(self.acceptance.clone());
        result
    }
}
impl WorkerProfile {
    pub fn from_setup(
        id: String,
        description: String,
        setup: ProjectSetup,
        supervision_override: Option<crate::supervision::SupervisionOverride>,
    ) -> Result<Self, String> {
        Ok(Self {
            supervision_override,
            resource_keys: setup.resource_keys,
            id,
            description,
            worker_adapter: setup.worker_adapter,
            worker: setup.worker,
            api_worker: setup.api_worker,
            desktop_worker: setup.desktop_worker,
            artifact_paths: setup.artifact_paths,
            acceptance: setup.acceptance.ok_or("执行配置验收缺失")?,
        })
    }
    pub fn draft(&self) -> WorkerProfileDraft {
        WorkerProfileDraft {
            worker_reasoning_effort: self.worker.as_ref().and_then(|w| w.reasoning_effort),
            supervision_override: self.supervision_override.clone(),
            resource_keys: self.resource_keys.clone(),
            id: self.id.clone(),
            description: self.description.clone(),
            worker_adapter: self.worker_adapter,
            codex_executable: self
                .worker
                .as_ref()
                .map_or_else(String::new, |w| w.executable.path.clone()),
            worker_model: self
                .worker
                .as_ref()
                .map(|w| w.model.clone())
                .or_else(|| self.api_worker.as_ref().map(|w| w.model.clone()))
                .or_else(|| self.desktop_worker.as_ref().map(|w| w.model_label.clone()))
                .unwrap_or_default(),
            omp_runtime_parent: self
                .worker
                .as_ref()
                .and_then(|w| w.omp_runtime_parent.clone()),
            api_worker: self.api_worker.clone(),
            desktop_worker: self.desktop_worker.clone(),
            artifact_paths: self.artifact_paths.clone(),
            acceptance: AcceptanceDraft {
                schema_version: self.acceptance.schema_version,
                stages: self
                    .acceptance
                    .stages
                    .iter()
                    .map(|s| AcceptanceStageDraft {
                        expected_assertions: s.check.expected_assertions.clone(),
                        id: s.check.id.clone(),
                        name: s.check.name.clone().unwrap_or_default(),
                        validator_id: s.validator_id.clone(),
                        args: s.check.args.clone(),
                        timeout_seconds: s.check.timeout_seconds,
                    })
                    .collect(),
            },
        }
    }
}
impl ProjectSetup {
    pub fn profile(&self, id: &str) -> Result<&WorkerProfile, String> {
        self.profiles
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| "主指挥选择了未配置的执行配置".into())
    }
    pub fn execution_for(
        &self,
        task: &str,
        profile: Option<&str>,
    ) -> Result<TaskExecution, String> {
        let mut result = if let Some(id) = profile {
            let p = self.profile(id)?;
            TaskExecution {
                worker_directory: None,
                supervision_override: p.supervision_override.clone(),
                profile_id: Some(id.into()),
                task_id: task.into(),
                adapter_id: p.worker_adapter.id().into(),
                worker: p.worker.clone(),
                api_worker: p.api_worker.clone(),
                desktop_worker: p.desktop_worker.clone(),
                artifact_paths: p.artifact_paths.clone(),
                checks: p
                    .acceptance
                    .stages
                    .iter()
                    .map(|s| s.check.clone())
                    .collect(),
            }
        } else {
            TaskExecution {
                worker_directory: None,
                supervision_override: None,
                profile_id: None,
                task_id: task.into(),
                adapter_id: self.worker_adapter.id().into(),
                worker: self.worker.clone(),
                api_worker: self.api_worker.clone(),
                desktop_worker: self.desktop_worker.clone(),
                artifact_paths: self.artifact_paths.clone(),
                checks: self.checks(),
            }
        };
        if let Some(i) = self.integration().filter(|i| i.task_id == task) {
            result.artifact_paths = i.artifact_paths.clone();
            result.checks = vec![i.check.clone()];
        }
        result.worker_directory = self
            .worker_directories
            .get(&result.model_identity()?)
            .cloned();
        Ok(result)
    }
    pub fn validator_for(&self, task: &str, profile: Option<&str>) -> Result<&str, String> {
        if let Some(i) = self.integration().filter(|i| i.task_id == task) {
            return Ok(&i.validator_id);
        }
        match profile {
            None => Ok(&self.validator_id),
            Some(id) => self
                .profile(id)?
                .acceptance
                .stages
                .first()
                .map(|s| s.validator_id.as_str())
                .ok_or_else(|| "执行配置验收缺失".into()),
        }
    }
    pub fn has_desktop(&self) -> bool {
        self.worker_adapter.is_desktop()
            || self.profiles.iter().any(|p| p.worker_adapter.is_desktop())
    }
}

/// Resources are explicit, case-sensitive names shared by projects in one core.
pub fn validate_resource_keys(keys: &[String]) -> Result<(), String> {
    require(keys.len() <= 32, "最多声明32项共享资源")?;
    let mut unique = std::collections::BTreeSet::new();
    for key in keys {
        identifier(key)?;
        require(unique.insert(key), "共享资源名称重复")?;
    }
    Ok(())
}
pub fn merge_resource_keys(common: &[String], extra: &[String]) -> Result<Vec<String>, String> {
    validate_resource_keys(common)?;
    validate_resource_keys(extra)?;
    let mut merged = common.to_vec();
    for key in extra {
        if !merged.contains(key) {
            merged.push(key.clone());
        }
    }
    validate_resource_keys(&merged)?;
    Ok(merged)
}
impl ProjectSetup {
    pub fn supervision_for(
        &self,
        profile: Option<&str>,
    ) -> Result<Option<&crate::supervision::SupervisionPolicy>, String> {
        let (adapter, selected) = match profile {
            None => (self.worker_adapter.id(), None),
            Some(id) => {
                let p = self.profile(id)?;
                (p.worker_adapter.id(), p.supervision_override.as_ref())
            }
        };
        crate::supervision::effective_policy(self.policy.supervision.as_ref(), selected, adapter)
    }
    pub fn supervision_for_execution(
        &self,
        execution: &TaskExecution,
    ) -> Result<Option<&crate::supervision::SupervisionPolicy>, String> {
        require(
            crate::objective_checker::matches_setup(self, execution)?,
            "监督执行配置与原任务不符",
        )?;
        self.supervision_for(execution.profile_id.as_deref())
    }
    pub fn resources_for(&self, profile: Option<&str>) -> Result<Option<Vec<String>>, String> {
        let extra = match profile {
            Some(id) => self.profile(id)?.resource_keys.as_slice(),
            None => &[],
        };
        let keys = merge_resource_keys(&self.resource_keys, extra)?;
        Ok((!keys.is_empty()).then_some(keys))
    }
}
