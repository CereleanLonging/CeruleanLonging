use crate::execution::{Artifact, VerificationState};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VerificationDetail {
    pub project_id: String,
    pub operation_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub state: VerificationState,
    pub revision_digest: String,
    pub manifest_digest: String,
    pub snapshot_digest: Option<String>,
    pub ledger_recorded: bool,
    pub evidence_digest: String,
    pub observed_at: String,
    pub message: Option<String>,
    pub artifacts: Vec<Artifact>,
    pub stages: Vec<VerificationStageView>,
    pub recovery: Option<VerificationRecoveryReview>,
    pub recovery_error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VerificationRecoveryReview {
    pub evidence_digest: String,
    pub passed: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VerificationStageView {
    pub id: String,
    pub name: String,
    pub command_digest: String,
    pub timeout_seconds: u32,
    pub state: VerificationStageState,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub stdout: Option<VerificationLogRef>,
    pub stderr: Option<VerificationLogRef>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum VerificationStageState {
    Passed,
    Failed,
    TimedOut,
    Observed,
    Pending,
    NotRun,
    Unknown,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct VerificationLogRef {
    pub path: String,
    pub bytes: u32,
    pub sha256: String,
}
