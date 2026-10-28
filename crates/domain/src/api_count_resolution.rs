//! Reconcile one original API count fee; this never authorizes generation.
use crate::{
    currency_budget::MAX_USD_NANOS,
    execution::{identifier, require},
};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiCountResolutionInput {
    pub project_id: String,
    pub request_id: String,
    #[ts(type = "number")]
    pub amount_usd_nanos: u64,
    pub billing_reference: String,
    pub explanation: String,
}
impl ApiCountResolutionInput {
    pub fn validate(&self) -> Result<(), String> {
        identifier(&self.project_id)?;
        identifier(&self.request_id)?;
        require(
            self.amount_usd_nanos <= MAX_USD_NANOS,
            "API计数金额超出精确表示范围",
        )?;
        for text in [&self.billing_reference, &self.explanation] {
            require(
                !text.trim().is_empty() && text.len() <= 8000 && !text.contains('\0'),
                "API计数核对需非空账单引用和说明，均不含NUL且不超过8000字节",
            )?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiCountEntry {
    pub project_id: String,
    pub request_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub round: u32,
    pub original_outcome: String,
    #[ts(type = "number")]
    pub maximum_usd_nanos: u64,
    pub fee_resolution: Option<crate::count_resolution::CountResolutionSummary>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiCountPage {
    pub revision: String,
    pub offset: u32,
    pub total: u32,
    pub next_offset: Option<u32>,
    pub entries: Vec<ApiCountEntry>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiCountReview {
    pub input: ApiCountResolutionInput,
    pub task_id: String,
    pub attempt_id: String,
    pub round: u32,
    pub source_digest: String,
    pub generation_digest: String,
    pub policy_digest: String,
    pub previous_ledger_head: String,
    pub review_digest: String,
    #[ts(type = "number")]
    pub maximum_usd_nanos: u64,
    pub original_outcome: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiCountRecord {
    pub review: ApiCountReview,
    pub user_event_digest: String,
    pub state: crate::count_resolution::CountResolutionState,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiCountLookup {
    pub project_id: String,
    pub request_id: String,
    pub review_digest: String,
    pub record: Option<ApiCountRecord>,
}
impl crate::Command {
    pub fn is_api_count_resolution(&self) -> bool {
        matches!(
            self,
            Self::ReadApiCounts { .. }
                | Self::ReviewApiCountResolution { .. }
                | Self::ConfirmApiCountResolution { .. }
                | Self::ReadApiCountConfirmation { .. }
        )
    }
}
