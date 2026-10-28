use crate::api_worker::ApiWorkerSelection;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ApiRecovery {
    pub message: String,
    pub can_restart: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ApiJobState {
    Starting,
    Executing,
    CancelRequested,
    Submitted,
    Unknown,
    Abandoned,
    Retired,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ApiJob {
    pub project_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub revision_digest: String,
    pub worker: ApiWorkerSelection,
    pub management_operation: Option<String>,
    pub state: ApiJobState,
    pub started_at: String,
    pub message: Option<String>,
    #[serde(default)]
    pub closure_digest: Option<String>,
    #[serde(default)]
    pub retirement_digest: Option<String>,
}
