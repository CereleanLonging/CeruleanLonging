//! Local user controls for an isolated single-executor experiment.
use crate::{execution::PlanRevision, team_timing::TeamTimingView};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum BaselineAction {
    Import {
        id: String,
        path: String,
        sha256: String,
    },
    Prepare {
        id: String,
        binding_digest: String,
    },
    Read {
        id: String,
        binding_digest: String,
    },
    Approve {
        id: String,
        start_digest: String,
        spec_digest: String,
    },
    Start {
        id: String,
        start_digest: String,
        spec_digest: String,
    },
    Human {
        id: String,
        start_digest: String,
        active: bool,
    },
    Cancel {
        id: String,
        start_digest: String,
    },
    Finish {
        id: String,
        start_digest: String,
    },
    AbortBeforeExecution {
        id: String,
        start_digest: String,
    },
    CloseInterrupted {
        id: String,
        start_digest: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BaselineView {
    pub id: String,
    pub binding_digest: Option<String>,
    pub phase: String,
    pub busy: bool,
    pub cancel_requested: bool,
    pub spec_digest: Option<String>,
    pub timing: Option<TeamTimingView>,
    pub error: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BaselineReview {
    pub id: String,
    pub start_digest: String,
    pub spec_digest: String,
    pub plan: PlanRevision,
}
