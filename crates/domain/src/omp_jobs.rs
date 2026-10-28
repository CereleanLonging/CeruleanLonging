use crate::execution::WorkerSelection;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OmpStopIssue {
    pub project_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub message: String,
    pub pending_path: String,
    pub pending_sha256: Option<String>,
    pub pending_bytes: Option<u32>,
    pub evidence_digest: Option<String>,
    pub acknowledged: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum OmpJobState {
    Starting,
    Executing,
    CancelRequested,
    Submitted,
    Cancelled,
    Unknown,
    Failed,
    Abandoned,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OmpJob {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub management_operation: Option<String>,
    pub project_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub revision_digest: String,
    pub worker: WorkerSelection,
    pub state: OmpJobState,
    pub started_at: String,
    pub message: Option<String>,
    pub closure_digest: Option<String>,
}
