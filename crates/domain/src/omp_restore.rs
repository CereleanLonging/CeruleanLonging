use serde::{Deserialize, Serialize};
use ts_rs::TS;
/// Request-local receipt of a completed restore, independent of a later preview.
/// The digest describes the file at completion, not its current contents.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OmpRestoreOutcome {
    pub project_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub call_id: String,
    pub review_digest: String,
    pub restored_sha256: Option<String>,
    pub at: String,
    pub readback_error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FilePreview {
    pub text: Option<String>,
    pub bytes: Option<u32>,
    pub truncated: bool,
    pub binary: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OmpFileItem {
    pub call_id: String,
    pub path: Option<String>,
    pub state: String,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OmpFileReview {
    pub project_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub call_id: String,
    pub path: String,
    pub workflow_sequence: u32,
    pub state: String,
    pub original_sha256: Option<String>,
    pub current_sha256: Option<String>,
    pub original: FilePreview,
    pub current: FilePreview,
    pub evidence_digest: Option<String>,
    pub restore_status: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OmpFilesView {
    pub project_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub offset: u32,
    pub total: u32,
    pub files: Vec<OmpFileItem>,
    pub detail: Option<OmpFileReview>,
}
