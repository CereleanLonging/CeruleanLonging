use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ObservedToolKind {
    Command,
    FileChange,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ObservedToolStatus {
    Completed,
    Failed,
    Declined,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolObservation {
    pub sequence: u32,
    pub received_at_ms: u64,
    pub item_digest: String,
    pub kind: ObservedToolKind,
    pub status: ObservedToolStatus,
    pub exit_code: Option<i32>,
    pub action_digest: Option<String>,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkerObservationTrace {
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub events: Vec<ToolObservation>,
    pub dropped: u32,
    pub rejected: u32,
    pub duplicates: u32,
    pub finished: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkerObservationView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supervision: Option<crate::supervision::SupervisionPolicy>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub incidents: Vec<crate::supervision::SupervisionIncident>,
    pub project_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub revision_digest: String,
    pub trace: WorkerObservationTrace,
    pub evidence_digest: String,
    pub storage_error: Option<String>,
}
