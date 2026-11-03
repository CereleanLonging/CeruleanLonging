//! User controls and projections. Ballot submission is deliberately NOT an IPC command.
use crate::{
    api_worker::ApiTarget,
    execution::{digest, identifier, require, sha256},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ts_rs::TS;

pub const PROTOCOL: &str = "cerulean-blind-review-v1";
pub const FRAGMENT_BYTES: usize = 2048;
pub const MAX_MATERIAL_BYTES: usize = 96 * 1024;
pub const MAX_WIRE_BYTES: usize = 192 * 1024;
pub const MAX_SEATS: usize = 7;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Redaction {
    pub literal: String,
    pub replacement: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum BlindReviewAction {
    Prepare {
        project_id: String,
        redactions: Vec<Redaction>,
        #[serde(default)]
        model_aliases: BTreeMap<String, Vec<String>>,
    },
    Start {
        project_id: String,
        round_id: String,
        digest: String,
    },
    Cancel {
        project_id: String,
        round_id: String,
    },
    Confirm {
        project_id: String,
        round_id: String,
        digest: String,
    },
    RequestChanges {
        project_id: String,
        round_id: String,
        reason: String,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ReviewPhase {
    Prepared,
    Collecting,
    AwaitingUser,
    Rejected,
    Incomplete,
    Cancelled,
    Superseded,
    Accepted,
    ChangesRequested,
}
impl ReviewPhase {
    pub fn active(self) -> bool {
        matches!(self, Self::Prepared | Self::Collecting | Self::AwaitingUser)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewSeat {
    pub id: String,
    pub role: String,
    pub target: ApiTarget,
    pub model: String,
    pub allowed_models: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Fragment {
    pub id: String,
    pub file: String,
    pub part: u32,
    pub content: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewMaterial {
    pub protocol: String,
    pub requirement: String,
    pub criteria: Vec<String>,
    pub fragments: Vec<Fragment>,
    pub objective_checks: u32,
}
impl ReviewMaterial {
    pub fn validate(&self) -> Result<(), String> {
        require(self.protocol == PROTOCOL, "盲评材料协议不支持")?;
        require(
            !self.requirement.trim().is_empty() && self.requirement.len() <= 16384,
            "盲评需求缺失或超限",
        )?;
        require(
            !self.criteria.is_empty() && self.criteria.len() <= 100 && self.objective_checks > 0,
            "盲评标准或客观检查缺失",
        )?;
        require(
            !self.fragments.is_empty() && self.fragments.len() <= 128,
            "盲评源码片段缺失或超限",
        )?;
        let mut ids = std::collections::BTreeSet::new();
        for f in &self.fragments {
            identifier(&f.id)?;
            identifier(&f.file)?;
            require(
                ids.insert(&f.id) && f.part > 0 && f.content.len() <= FRAGMENT_BYTES,
                "盲评片段重复或超限",
            )?;
        }
        require(
            serde_json::to_vec(self).map_err(|e| e.to_string())?.len() <= MAX_MATERIAL_BYTES,
            "完整盲评材料超过首版容量；不会截断材料后计票",
        )
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewBinding {
    pub project_id: String,
    pub revision: String,
    pub setup: String,
    pub delivery: String,
    pub evidence: String,
    pub material: String,
    pub policy: String,
}
impl ReviewBinding {
    pub fn validate(&self) -> Result<(), String> {
        identifier(&self.project_id)?;
        for d in [
            &self.revision,
            &self.setup,
            &self.delivery,
            &self.evidence,
            &self.material,
            &self.policy,
        ] {
            sha256(d)?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum Vote {
    Pass,
    Fail,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Ballot {
    pub vote: Vote,
    pub citations: Vec<String>,
    pub reason: String,
}
impl Ballot {
    pub fn validate(&self, material: &ReviewMaterial) -> Result<(), String> {
        require(
            !self.reason.trim().is_empty() && self.reason.len() <= 4096,
            "评审理由缺失或超限",
        )?;
        require(
            !self.citations.is_empty() && self.citations.len() <= 32,
            "评审引用缺失或超限",
        )?;
        let mut seen = std::collections::BTreeSet::new();
        for id in &self.citations {
            require(
                seen.insert(id) && material.fragments.iter().any(|f| &f.id == id),
                "评审引用不属于本轮材料",
            )?;
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RevealedBallot {
    pub seat: String,
    pub model: String,
    pub ballot: Ballot,
}
/// Advisory output after every ballot is revealed; never a new vote or authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FailureAnalysis {
    pub omissions: Vec<String>,
    pub disputed_criteria: Vec<String>,
    pub chief_revision_request: String,
}
impl FailureAnalysis {
    pub fn validate(&self) -> Result<(), String> {
        require(
            self.omissions.len() <= 12
                && self.disputed_criteria.len() <= 12
                && !self.chief_revision_request.trim().is_empty()
                && serde_json::to_vec(self).map_err(|e| e.to_string())?.len() <= 2048,
            "副指挥修订分析缺失或超限",
        )
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BlindReviewView {
    pub project_id: String,
    pub round_id: String,
    pub digest: String,
    pub phase: ReviewPhase,
    pub seats: Vec<ReviewSeat>,
    pub file_count: u32,
    pub fragment_count: u32,
    pub material_bytes: u32,
    pub max_requests: u32,
    pub max_output_tokens: u32,
    pub timeout_seconds: u32,
    pub deadline: Option<String>,
    pub ballots: Option<Vec<RevealedBallot>>,
    pub message: Option<String>,
    pub material_preview: Option<ReviewMaterial>,
    pub coverage: ReviewCoverage,
    pub is_current: bool,
    pub final_accepted: bool,
    pub deputy_analysis: Option<FailureAnalysis>,
    pub analysis_status: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewCoverage {
    pub files: Vec<String>,
    pub excluded: Vec<String>,
    pub integration_task: Option<String>,
}

pub fn review_digest(binding: &ReviewBinding, seats: &[ReviewSeat]) -> Result<String, String> {
    digest(&(
        PROTOCOL,
        binding,
        seats,
        4096u32,
        90u32,
        900u32,
        "one-post-reveal-analysis",
    ))
}
