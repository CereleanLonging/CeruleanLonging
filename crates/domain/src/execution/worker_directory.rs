//! Stable model work directories. Task attempts never choose or retarget them.
use super::*;
use std::path::{Component, Path};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerDirectory {
    pub identity: String,
    pub relative_path: String,
}

impl TaskExecution {
    pub fn model_identity(&self) -> Result<String, String> {
        let native = self
            .worker
            .as_ref()
            .map(|w| (&w.provider, &w.model, w.executable.path.to_lowercase()));
        let api = self.api_worker.as_ref().map(|w| (&w.target, &w.model));
        let desktop = self
            .desktop_worker
            .as_ref()
            .map(|w| (&self.adapter_id, &w.model_label));
        digest(&(&self.adapter_id, native, api, desktop))
    }
}

impl WorkerDirectory {
    pub fn for_desktop(
        selection: &crate::desktop_sessions::DesktopWorkerSelection,
    ) -> Result<Self, String> {
        let identity = digest(&(
            selection.adapter_id(),
            None::<()>,
            None::<()>,
            Some((selection.adapter_id(), &selection.model_label)),
        ))?;
        Ok(Self {
            relative_path: format!("executors/{identity}"),
            identity,
        })
    }
    pub fn for_task(task: &TaskExecution) -> Result<Self, String> {
        let identity = task.model_identity()?;
        Ok(Self {
            relative_path: format!("executors/{identity}"),
            identity,
        })
    }
    pub fn validate(&self, task: &TaskExecution) -> Result<(), String> {
        sha256(&self.identity)?;
        require(
            self.identity == task.model_identity()?
                && self.relative_path == format!("executors/{}", self.identity),
            "执行目录与模型身份不符",
        )
    }
    pub fn root(&self, project: &str) -> Result<String, String> {
        let root = Path::new(project);
        require(
            root.is_absolute()
                && root
                    .components()
                    .all(|c| !matches!(c, Component::ParentDir | Component::CurDir)),
            "invalid shared worker root",
        )?;
        artifact_path(&self.relative_path)?;
        self.relative_path
            .split('/')
            .fold(root.to_path_buf(), |p, part| p.join(part))
            .to_str()
            .map(str::to_owned)
            .ok_or_else(|| "invalid worker directory encoding".into())
    }
}
