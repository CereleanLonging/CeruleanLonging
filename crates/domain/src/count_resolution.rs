//! Local-user reconciliation of count fees and continuation responsibility.
//! Neither action authorizes another provider request or changes its outcome.
use crate::{
    currency_budget::MAX_USD_NANOS,
    execution::{identifier, require},
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum CountResolutionAspect {
    SettleFee,
    RetireContinuation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CountResolutionAction {
    SettleFee {
        #[ts(type = "number")]
        amount_usd_nanos: u64,
        billing_reference: String,
        explanation: String,
    },
    RetireContinuation {
        reason: String,
    },
}
impl CountResolutionAction {
    pub fn aspect(&self) -> CountResolutionAspect {
        match self {
            Self::SettleFee { .. } => CountResolutionAspect::SettleFee,
            Self::RetireContinuation { .. } => CountResolutionAspect::RetireContinuation,
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        let text = |s: &str| {
            require(
                !s.trim().is_empty() && s.len() <= 8000 && !s.contains('\0'),
                "计数核对说明须非空、不含 NUL 且不超过 8000 字节",
            )
        };
        match self {
            Self::SettleFee {
                amount_usd_nanos,
                billing_reference,
                explanation,
            } => {
                require(
                    *amount_usd_nanos <= MAX_USD_NANOS,
                    "计数金额超出精确表示范围",
                )?;
                text(billing_reference)?;
                text(explanation)
            }
            Self::RetireContinuation { reason } => text(reason),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CountResolutionInput {
    pub project_id: String,
    pub operation_id: String,
    pub action: CountResolutionAction,
}
impl CountResolutionInput {
    pub fn validate(&self) -> Result<(), String> {
        identifier(&self.project_id)?;
        identifier(&self.operation_id)?;
        self.action.validate()
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CountResolutionReview {
    pub input: CountResolutionInput,
    pub source_digest: String,
    pub policy_digest: String,
    pub previous_ledger_head: String,
    pub review_digest: String,
    #[ts(type = "number")]
    pub maximum_usd_nanos: u64,
    pub original_outcome: String,
    pub generation_digest: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum CountResolutionState {
    ConfirmedPending,
    Recorded,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CountResolutionRecord {
    pub review: CountResolutionReview,
    pub user_event_digest: String,
    pub state: CountResolutionState,
}
/// The bounded list omits free-form explanations. Detail is read explicitly.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CountResolutionSummary {
    pub review_digest: String,
    pub user_event_digest: String,
    pub state: CountResolutionState,
    #[ts(type = "number | null")]
    pub amount_usd_nanos: Option<u64>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManagementCountEntry {
    pub project_id: String,
    pub operation_id: String,
    pub original_outcome: String,
    #[ts(type = "number")]
    pub maximum_usd_nanos: u64,
    pub fee_resolution: Option<CountResolutionSummary>,
    pub retirement: Option<CountResolutionSummary>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManagementCountPage {
    pub revision: String,
    pub offset: u32,
    pub total: u32,
    pub next_offset: Option<u32>,
    pub entries: Vec<ManagementCountEntry>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CountConfirmationLookup {
    pub project_id: String,
    pub operation_id: String,
    pub aspect: CountResolutionAspect,
    pub review_digest: String,
    pub record: Option<CountResolutionRecord>,
}
/// Independent response: never replace a full workspace Snapshot with this.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CountResolutionResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub api_generations: Option<crate::api_generation_resolution::ApiGenerationPage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub api_generation_review: Option<crate::api_generation_resolution::ApiGenerationReview>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub api_generation_confirmation: Option<crate::api_generation_resolution::ApiGenerationLookup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub api_counts: Option<crate::api_count_resolution::ApiCountPage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub api_review: Option<crate::api_count_resolution::ApiCountReview>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub api_confirmation: Option<crate::api_count_resolution::ApiCountLookup>,
    pub counts: Option<ManagementCountPage>,
    pub review: Option<CountResolutionReview>,
    pub confirmation: Option<CountConfirmationLookup>,
    pub currency_budgets: Vec<crate::currency_budget::ProjectCurrencyBudget>,
}
impl crate::Command {
    pub fn is_count_resolution(&self) -> bool {
        self.is_api_generation_resolution()
            || self.is_api_count_resolution()
            || matches!(
                self,
                Self::ReadManagementCounts { .. }
                    | Self::ReviewManagementCountResolution { .. }
                    | Self::ConfirmManagementCountResolution { .. }
                    | Self::ReadManagementCountConfirmation { .. }
            )
    }
}
