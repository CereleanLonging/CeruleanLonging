//! Explicit local-user controls for the fixed team experiment. No role callbacks.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum TeamTimingAction {
    Import {
        path: String,
        sha256: String,
    },
    Begin {
        project_id: String,
        setup_digest: String,
    },
    Human {
        project_id: String,
        start_digest: String,
        active: bool,
    },
    Finish {
        project_id: String,
        start_digest: String,
    },
    CloseInterrupted {
        project_id: String,
        start_digest: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TeamTimingView {
    pub project_id: String,
    pub start_digest: Option<String>,
    pub phase: String,
    pub clock_live: bool,
    pub human_active: bool,
    #[ts(type = "number | null")]
    pub elapsed_ms: Option<u64>,
    #[ts(type = "number | null")]
    pub planning_ms: Option<u64>,
    #[ts(type = "number | null")]
    pub approval_wait_ms: Option<u64>,
    #[ts(type = "number | null")]
    pub execution_and_closure_ms: Option<u64>,
    #[ts(type = "number | null")]
    pub human_active_ms: Option<u64>,
    pub accepted: Option<bool>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TeamImportView {
    pub request_id: String,
    pub source_path: String,
    pub source_sha256: String,
    pub state: String,
    pub message: Option<String>,
}
