use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DesktopApp {
    Qoder,
    Workbuddy,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopWindow {
    pub app: DesktopApp,
    pub hwnd: String,
    pub pid: u32,
    pub start_ticks: String,
    pub exe: String,
    pub title: String,
    pub version: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopObservation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_label: Option<String>,
    pub window: DesktopWindow,
    pub captured_at: String,
    pub document_runtime_id: String,
    pub session_id: Option<String>,
    pub content_digest: String,
    pub layout_digest: String,
    pub node_count: u32,
    pub draft_empty: bool,
    pub busy: bool,
}

/// Frozen UI evidence and explicit user acknowledgement. This is not a provider
/// identity or a sandbox claim; desktop execution must re-observe before input.
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopWorkerSelection {
    pub binding: DesktopBinding,
    pub project_root: String,
    pub model_label: String,
    pub workspace_confirmed: bool,
    pub external_tools_acknowledged: bool,
}
impl DesktopWorkerSelection {
    pub fn adapter_id(&self) -> &'static str {
        match self.binding.observation.window.app {
            DesktopApp::Qoder => "qoder",
            DesktopApp::Workbuddy => "workbuddy",
        }
    }
    pub fn session_id(&self) -> String {
        self.binding
            .observation
            .session_id
            .clone()
            .unwrap_or_else(|| format!("ui-binding-{}", self.binding.id))
    }
    pub fn validate(&self) -> Result<(), String> {
        use crate::execution::*;
        let b = &self.binding;
        let o = &b.observation;
        identifier(&b.project_id)?;
        for value in [&b.id, &b.bridge_sha256, &o.content_digest, &o.layout_digest] {
            sha256(value)?;
        }
        timestamp(&b.confirmed_at)?;
        timestamp(&o.captured_at)?;
        require(
            b.id == digest(&(
                &b.project_id,
                &b.label,
                &b.observation,
                &b.confirmed_at,
                &b.bridge_sha256,
            ))?,
            "桌面关联摘要不符",
        )?;
        require(
            std::path::Path::new(&self.project_root).is_absolute()
                && self.project_root.len() <= 4096
                && !self.project_root.chars().any(char::is_control),
            "桌面项目目录无效",
        )?;
        require(
            self.workspace_confirmed && self.external_tools_acknowledged,
            "请核对桌面应用项目目录并确认其沿用自身工具权限",
        )?;
        require(
            !self.model_label.trim().is_empty()
                && self.model_label.len() <= 200
                && !self.model_label.chars().any(char::is_control)
                && o.model_label.as_ref() == Some(&self.model_label),
            "模型显示名须匹配原会话观察",
        )?;
        require(
            o.draft_empty
                && !o.busy
                && !o.document_runtime_id.is_empty()
                && o.document_runtime_id.len() <= 512
                && o.node_count > 0
                && o.node_count <= 1000,
            "桌面会话观察不完整",
        )?;
        require(
            !o.window.version.is_empty()
                && o.window.version.len() <= 100
                && o.window.pid > 0
                && o.window.hwnd.parse::<u64>().is_ok_and(|n| n > 0)
                && o.window.start_ticks.parse::<u64>().is_ok_and(|n| n > 0),
            "桌面应用身份无效",
        )?;
        require(
            std::path::Path::new(&o.window.exe).is_absolute()
                && std::path::Path::new(&o.window.exe)
                    .file_name()
                    .is_some_and(|p| {
                        p.to_string_lossy()
                            .eq_ignore_ascii_case(&format!("{}.exe", self.adapter_id()))
                    }),
            "桌面应用程序不符",
        )?;
        match o.window.app {
            DesktopApp::Qoder => require(
                o.session_id.as_ref().is_some_and(|s| {
                    s.len() == 36
                        && s.bytes().enumerate().all(|(i, c)| {
                            if [8, 13, 18, 23].contains(&i) {
                                c == b'-'
                            } else {
                                c.is_ascii_hexdigit()
                            }
                        })
                }),
                "Qoder聊天UUID无效",
            )?,
            DesktopApp::Workbuddy => {
                require(o.session_id.is_none(), "WorkBuddy不能伪造供应商会话ID")?
            }
        }
        Ok(())
    }
    pub fn validate_attempt(
        &self,
        binding: &crate::execution::SessionBinding,
    ) -> Result<(), String> {
        use crate::execution::*;
        self.validate()?;
        require(
            binding.adapter_id == self.adapter_id()
                && binding.workspace == self.project_root
                && binding.session_id == self.session_id()
                && binding.adapter_version
                    == format!("desktop-ui-v1:{}", self.binding.bridge_sha256)
                && binding.identity_strength == IdentityStrength::UiObservation,
            "桌面尝试必须对应原批准会话与UI观察身份",
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopBinding {
    pub id: String,
    pub project_id: String,
    pub label: String,
    pub observation: DesktopObservation,
    pub confirmed_at: String,
    pub bridge_sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS, Default)]
#[serde(rename_all = "camelCase")]
pub struct DesktopAccess {
    #[serde(default)]
    pub bridge_sha256: String,
    pub checking: bool,
    pub windows: Vec<DesktopWindow>,
    pub observation: Option<DesktopObservation>,
    pub observation_digest: Option<String>,
    pub bindings: Vec<DesktopBinding>,
    pub error: Option<String>,
    #[serde(default)]
    pub trial_review: Option<DesktopTrialReview>,
    #[serde(default)]
    pub trials: Vec<DesktopTrial>,
    #[serde(default)]
    pub task_review: Option<DesktopTaskReview>,
    #[serde(default)]
    pub tasks: Vec<DesktopTaskStatus>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopTaskReview {
    pub id: String,
    pub binding_id: String,
    pub marker: String,
    pub prompt: String,
    pub prompt_sha256: String,
    pub bridge_sha256: String,
    pub observation: DesktopObservation,
    pub baseline: Vec<String>,
    pub attempt: crate::execution::Attempt,
    pub project_id: String,
    pub workflow_sequence: u32,
    pub workflow_hash: String,
    pub max_wait_seconds: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation: Option<DesktopTaskContinuation>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopTaskContinuation {
    pub previous_attempt_id: String,
    pub previous_review_digest: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopTaskProof {
    /// Host-recorded reader identity; absent for legacy proofs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reader_sha256: Option<String>,
    pub marker: String,
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_binding: Option<DesktopTaskLocalBindingProof>,
    pub prompt_sha256: String,
    pub observed_baseline: Vec<String>,
    pub reply_sha256: String,
    pub observed_at: String,
    pub completed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub round_sha256: Option<String>,
}
/// Local UI continuity evidence. Never an authenticated provider session ID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopTaskLocalBindingProof {
    pub binding_id: String,
    pub task_review_digest: String,
    pub observation: DesktopObservation,
}
/// Derived from immutable dispatch evidence and the workflow; never an acceptance claim.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DesktopTaskStatus {
    /// Ephemeral observation from this original task owner; never a terminal receipt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub progress: Option<DesktopProgressView>,
    #[serde(default)]
    pub reviewed_stop_record: Option<String>,
    #[serde(default)]
    pub cancellation: Option<DesktopCancellationStatus>,
    #[serde(default)]
    pub stopping: bool,
    #[serde(default)]
    pub retired: bool,
    #[serde(default)]
    pub retirement_digest: Option<String>,
    pub review: DesktopTaskReview,
    pub proof: Option<DesktopTaskProof>,
    pub submitted: bool,
    pub active: bool,
    pub reading: bool,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopTaskProgress {
    pub schema_version: u32,
    pub task_review_digest: String,
    pub observation: DesktopObservation,
    pub baseline: Vec<String>,
    pub turn_runtime_id: String,
    pub prompt_runtime_id: String,
    pub visible_digest: String,
    pub status_text: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopProgressView {
    pub latest: Option<DesktopTaskProgress>,
    pub checking: bool,
    pub samples: u32,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopCancellationStatus {
    pub requested_at: String,
    pub reason: String,
    pub control_reserved: bool,
    pub confirmed: bool,
}

/// A fixed, text-only connectivity check. It never authorizes a project task.
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopTrialReview {
    pub id: String,
    pub binding_id: String,
    pub observation: DesktopObservation,
    pub marker: String,
    pub prompt: String,
    pub baseline: Vec<String>,
    pub bridge_sha256: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopTrialProof {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_baseline: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reader_sha256: Option<String>,
    pub marker: String,
    pub session_id: String,
    pub baseline: Vec<String>,
    pub prompt_seen: bool,
    pub completed: bool,
    pub reply_matches: bool,
    pub observed_at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopTrial {
    pub review: DesktopTrialReview,
    pub started_at: String,
    // None includes interrupted/uncertain sends. Never automatically replay.
    pub proof: Option<DesktopTrialProof>,
}
