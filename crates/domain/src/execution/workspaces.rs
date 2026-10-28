use super::*;
use std::path::{Component, Path};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum GitWorktreeInput {
    CommittedRawBlobs,
    CommittedRawBlobsWithAcceptedDependencies,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitWorktreePolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub integration: Option<Box<ProjectIntegration>>,
    pub schema_version: u32,
    pub base_commit: String,
    pub pool_root: String,
    pub git: ValidationFile,
    /// Explicitly excludes uncommitted source edits and checkout filters.
    pub input: GitWorktreeInput,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectIntegration {
    pub task_id: String,
    pub artifact_paths: Vec<String>,
    pub validator_id: String,
    pub check: AcceptanceCheck,
}

impl PlanRevision {
    pub fn integration(&self) -> Option<&ProjectIntegration> {
        self.worktree
            .as_ref()
            .and_then(|w| w.integration.as_deref())
    }
    pub fn validate_integration(&self) -> Result<(), String> {
        let Some(finale) = self.integration() else {
            return Ok(());
        };
        identifier(&finale.task_id)?;
        identifier(&finale.validator_id)?;
        require(
            self.worktree.as_ref().is_some_and(|w| {
                w.input == GitWorktreeInput::CommittedRawBlobsWithAcceptedDependencies
            }),
            "最终集成需要已接受依赖输入",
        )?;
        let execution = self.task(&finale.task_id)?;
        require(
            execution.artifact_paths == finale.artifact_paths
                && execution.checks == vec![finale.check.clone()]
                && finale.check.tool.is_some(),
            "最终集成产物或整体检查与配置不符",
        )?;
        let tasks: BTreeMap<_, _> = self
            .workflow
            .tasks
            .iter()
            .map(|t| (t.id.as_str(), t))
            .collect();
        let target = tasks
            .get(finale.task_id.as_str())
            .ok_or("最终集成任务缺失")?;
        let mut ancestors = BTreeSet::new();
        let mut pending = target.dependencies.clone();
        while let Some(id) = pending.pop() {
            require(id != finale.task_id, "最终集成依赖循环")?;
            if ancestors.insert(id.clone()) {
                pending.extend(
                    tasks
                        .get(id.as_str())
                        .ok_or("集成前置任务缺失")?
                        .dependencies
                        .iter()
                        .cloned(),
                );
            }
        }
        require(
            tasks.len() >= 2 && ancestors.len() + 1 == tasks.len(),
            "最终集成必须覆盖方案中的所有其他任务",
        )
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceProof {
    pub record_digest: String,
}

pub fn workspace_key(project: &str, task: &str, attempt: &str) -> Result<String, String> {
    for value in [project, task, attempt] {
        identifier(value)?;
    }
    digest(&(project, task, attempt))
}
/// Approved logical ownership for an isolated task; attempts still use distinct
/// physical directories and the workflow permits only one active attempt/task.
pub fn isolated_task_key(project: &str, task: &str) -> Result<String, String> {
    identifier(project)?;
    identifier(task)?;
    Ok(format!("tree-{}", digest(&(project, task))?))
}
impl PlanRevision {
    pub fn validate_workspace_policy(&self) -> Result<(), String> {
        let Some(policy) = &self.worktree else {
            return Ok(());
        };
        require(policy.schema_version == 1, "unsupported worktree policy")?;
        require(
            matches!(policy.base_commit.len(), 40 | 64)
                && policy
                    .base_commit
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "worktree requires full commit ID",
        )?;
        let root = Path::new(&policy.pool_root);
        require(
            root.is_absolute()
                && !policy.pool_root.chars().any(char::is_control)
                && policy.pool_root.len() <= 4096
                && root
                    .components()
                    .all(|c| !matches!(c, Component::CurDir | Component::ParentDir)),
            "invalid worktree pool path",
        )?;
        ValidationTool {
            executable: policy.git.clone(),
            inputs: vec![],
            environment: BTreeMap::new(),
        }
        .validate()?;
        require(
            self.policy.tools.contains(&ToolPermission::ReadFiles)
                && self.policy.tools.contains(&ToolPermission::WriteFiles),
            "worktree preparation requires approved file access",
        )?;
        require(
            self.snapshot_policy
                .as_ref()
                .is_some_and(|p| p.excludes(".git")),
            "worktree verification must exclude Git metadata",
        )?;
        require(
            self.execution.iter().all(|t| {
                t.desktop_worker.is_none()
                    && !matches!(t.adapter_id.as_str(), "qoder" | "workbuddy")
            }),
            "existing desktop sessions cannot silently move into worktrees",
        )?;
        Ok(())
    }
    pub fn execution_root(&self, task: &str, attempt: &str) -> Result<String, String> {
        self.task(task)?;
        identifier(attempt)?;
        match &self.worktree {
            None => Ok(self.project.root.clone()),
            Some(policy) => Path::new(&policy.pool_root)
                .join("trees")
                .join(workspace_key(&self.project.id, task, attempt)?)
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| "non-Unicode worktree path".into()),
        }
    }
    /// Structural binding only; the trusted core must also inspect the saved
    /// preparation record before dispatch and live artifact observation.
    pub fn validate_attempt_workspace(&self, attempt: &Attempt) -> Result<(), String> {
        require(
            attempt.binding.workspace == self.execution_root(&attempt.task_id, &attempt.id)?,
            "attempt workspace differs from approved policy",
        )?;
        match (&self.worktree, &attempt.workspace_proof) {
            (None, None) => Ok(()),
            (Some(_), Some(proof)) => sha256(&proof.record_digest),
            _ => Err("workspace preparation proof missing or unexpected".into()),
        }
    }
}
