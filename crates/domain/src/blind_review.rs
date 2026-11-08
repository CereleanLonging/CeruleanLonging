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
    StartFunctionalTest {
        project_id: String,
        round_id: String,
        digest: String,
    },
    PrepareFunctionalEnvironment {
        project_id: String,
        round_id: String,
        digest: String,
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
    AwaitingFunctionalTest,
    FunctionalTesting,
    FunctionalTestFailed,
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
        matches!(
            self,
            Self::Prepared
                | Self::Collecting
                | Self::AwaitingFunctionalTest
                | Self::FunctionalTesting
                | Self::AwaitingUser
        )
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewSeat {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<crate::reasoning::ReasoningEffort>,
    pub id: String,
    pub role: String,
    pub target: Option<ApiTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<ReviewTransport>,
    pub model: String,
    pub allowed_models: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ReviewTransport {
    OmpDeputy {
        worker: crate::execution::WorkerSelection,
    },
    Cli {
        adapter: String,
        task_id: String,
    },
    Desktop {
        task_id: String,
        session_id: String,
    },
    CodexSession {
        session_id: String,
        provider: String,
    },
}
impl ReviewSeat {
    pub fn output_limit(&self) -> u32 {
        self.max_output_tokens.unwrap_or(4096)
    }

    pub fn identity(&self) -> Result<String, String> {
        let channel = match &self.transport {
            Some(ReviewTransport::OmpDeputy { .. }) => "omp",
            None => "api",
            Some(ReviewTransport::Cli { adapter, .. }) => adapter.as_str(),
            Some(ReviewTransport::Desktop { .. }) => "qoder",
            Some(ReviewTransport::CodexSession { .. }) => "codex-session",
        };
        digest(&(&self.target, channel, &self.model))
    }
    pub fn api_target(&self) -> Result<&ApiTarget, String> {
        require(self.transport.is_none(), "会话评审必须使用原接入通道")?;
        self.target.as_ref().ok_or_else(|| "API评审端点缺失".into())
    }
    pub fn validate_source(&self) -> Result<(), String> {
        require(
            self.max_output_tokens.is_none()
                || (self.transport.is_none() && self.output_limit() > 0),
            "评审输出额度只能绑定到有效的 API 席位",
        )?;
        match (&self.target, &self.transport) {
            (None, Some(ReviewTransport::OmpDeputy { worker })) => {
                worker.validate()?;
                require(
                    self.role == "deputy"
                        && self.id == "deputy"
                        && worker.provider == "deepseek"
                        && worker.model == self.model
                        && worker.reasoning_effort == self.reasoning_effort,
                    "OMP 副指挥评审配置不符",
                )?;
            }
            (Some(target), None) => {
                target.url()?;
                crate::reasoning::validate_api(target, &self.model, self.reasoning_effort)?;
            }
            (None, Some(ReviewTransport::Cli { adapter, task_id })) => {
                require(
                    matches!(adapter.as_str(), "codex" | "omp"),
                    "CLI评审接入无效",
                )?;
                identifier(task_id)?;
            }
            (
                None,
                Some(ReviewTransport::Desktop {
                    task_id,
                    session_id,
                }),
            ) => {
                identifier(task_id)?;
                identifier(session_id)?;
            }
            (
                None,
                Some(ReviewTransport::CodexSession {
                    session_id,
                    provider,
                }),
            ) => {
                identifier(session_id)?;
                identifier(provider)?;
            }
            _ => return Err("评审必须且只能绑定一种接入方式".into()),
        }
        Ok(())
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub independent_check_evidence: Option<ReviewCheckEvidence>,
}

/// Numeric facts from authenticated independent reports, without reviewer identities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewCheckEvidence {
    pub task_deliveries: u32,
    pub checks_total: u32,
    pub checks_passed: u32,
    pub source_files: u32,
    pub assembled_delivery: bool,
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
        if let Some(evidence) = &self.independent_check_evidence {
            require(
                (1..=100).contains(&evidence.task_deliveries)
                    && evidence.checks_total == self.objective_checks
                    && evidence.checks_passed == evidence.checks_total
                    && evidence.source_files as usize
                        == self
                            .fragments
                            .iter()
                            .map(|fragment| &fragment.file)
                            .collect::<std::collections::BTreeSet<_>>()
                            .len(),
                "盲评材料与独立检查结果摘要不符",
            )?;
        }
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assessments: Vec<crate::review_weights::CriterionAssessment>,
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
        let mut criteria = std::collections::BTreeSet::new();
        for assessment in &self.assessments {
            require(
                assessment.criterion > 0
                    && assessment.criterion as usize <= material.criteria.len()
                    && criteria.insert(assessment.criterion)
                    && assessment.citations.len() <= 32,
                "逐项核对包含重复或不存在的验收标准",
            )?;
            let mut citations = std::collections::BTreeSet::new();
            for id in &assessment.citations {
                require(
                    citations.insert(id) && material.fragments.iter().any(|f| &f.id == id),
                    "逐项核对引用不属于本轮材料",
                )?;
            }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_audit: Option<ReviewToolAudit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_ballot: Option<Ballot>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ReviewToolStatus {
    Clean,
    ToolCallObserved,
    Unverifiable,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewToolAudit {
    pub status: ReviewToolStatus,
    pub source: String,
    pub request: String,
    pub evidence_digest: String,
    pub tools: Vec<String>,
    pub reason: String,
}
impl ReviewToolAudit {
    pub fn validate(&self) -> Result<(), String> {
        identifier(&self.request)?;
        sha256(&self.evidence_digest)?;
        require(
            matches!(
                self.source.as_str(),
                "api-response"
                    | "codex-history"
                    | "omp-events"
                    | "desktop-history"
                    | "transport-failure"
            ),
            "评分工具核验来源无效",
        )?;
        require(
            !self.reason.trim().is_empty()
                && self.reason.len() <= 2048
                && self.tools.len() <= 32
                && self
                    .tools
                    .iter()
                    .all(|t| !t.is_empty() && t.len() <= 128 && !t.chars().any(char::is_control)),
            "评分工具核验记录无效",
        )?;
        require(
            match self.status {
                ReviewToolStatus::Clean => {
                    self.tools.is_empty() && self.source != "transport-failure"
                }
                ReviewToolStatus::ToolCallObserved => !self.tools.is_empty(),
                ReviewToolStatus::Unverifiable => true,
            },
            "评分工具核验状态不符",
        )
    }
    pub fn effective_ballot(
        &self,
        original: Option<&Ballot>,
        material: &ReviewMaterial,
    ) -> Result<Ballot, String> {
        self.validate()?;
        if self.status == ReviewToolStatus::Clean {
            let ballot = original.ok_or("缺少模型原始评分")?;
            ballot.validate(material)?;
            return Ok(ballot.clone());
        }
        Ok(Ballot {
            assessments: vec![],
            vote: Vote::Fail,
            citations: vec![],
            reason: match self.status {
                ReviewToolStatus::ToolCallObserved => format!(
                    "独立验收判定未通过。评分期间观察到外部工具调用或调用请求。{}",
                    self.reason
                ),
                _ => format!(
                    "独立验收判定未通过。评分调用记录的核验依据不足。{}",
                    self.reason
                ),
            },
        })
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assembled_delivery: Option<String>,
    pub check_archive_digest: Option<String>,
    pub weight_policy: Option<crate::review_weights::WeightPolicy>,
    pub weighted_result: Option<crate::review_weights::WeightedResult>,
    pub session_progress: Option<ReviewSessionProgress>,
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
    pub functional_test: Option<FunctionalTestView>,
    pub confirmation_digest: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ReviewSessionProgress {
    pub seat: String,
    pub model: String,
    pub session_id: Option<String>,
    pub request: String,
    pub packet: u32,
    pub packets: u32,
    pub material_message: Option<String>,
    pub acknowledged: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FunctionalDecision {
    pub vote: Vote,
    pub reason: String,
}
impl FunctionalDecision {
    pub fn validate(&self) -> Result<(), String> {
        require(
            !self.reason.trim().is_empty() && self.reason.len() <= 4096,
            "主指挥功能测试说明缺失或超限",
        )
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum FunctionalTestStatus {
    Locked,
    Ready,
    Running,
    Passed,
    Failed,
    Interrupted,
    Cancelled,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FunctionalTestView {
    pub status: FunctionalTestStatus,
    pub operation_id: Option<String>,
    pub model: Option<String>,
    pub checks: Vec<crate::execution::CheckObservation>,
    pub report_digest: Option<String>,
    pub decision: Option<FunctionalDecision>,
    pub message: Option<String>,
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
