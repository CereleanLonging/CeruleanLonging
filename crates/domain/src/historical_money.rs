//! Public historical review contracts; amounts do not prove a supplier invoice.
use crate::execution::{digest, identifier, require};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// A distinct, bounded response. Omitted unrelated workbench collections are
/// not empty collections and must never replace a complete Snapshot.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoricalResult {
    pub historical_sources: Option<HistoricalSourcePage>,
    pub historical_accounting: Option<HistoricalAccountingPage>,
    pub historical_charge_review: Option<HistoricalChargeReview>,
    pub historical_confirmation: Option<HistoricalConfirmationLookup>,
    /// At most the single account explicitly named by this request.
    pub currency_budgets: Vec<crate::currency_budget::ProjectCurrencyBudget>,
}

impl crate::Command {
    pub fn is_historical(&self) -> bool {
        matches!(
            self,
            Self::ReadHistoricalSources { .. }
                | Self::ReadHistoricalAccounting { .. }
                | Self::ReadHistoricalConfirmation { .. }
                | Self::ReviewHistoricalCharge { .. }
                | Self::ConfirmHistoricalCharge { .. }
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoricalChargeInput {
    pub project_id: String,
    pub scope: HistoricalScope,
    #[ts(type = "number")]
    pub amount_usd_nanos: u64,
    pub billing_reference: String,
    pub explanation: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoricalChargeReview {
    pub input: HistoricalChargeInput,
    pub source: HistoricalSource,
    pub policy_digest: String,
    pub previous_ledger_head: String,
    pub review_digest: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum HistoricalAccountingState {
    ConfirmedPending,
    Accounted,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoricalAccounting {
    pub review: HistoricalChargeReview,
    pub user_event_digest: String,
    pub state: HistoricalAccountingState,
}

/// Lists omit the original potentially large explanation and source references.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoricalAccountingSummary {
    pub project_id: String,
    pub scope: HistoricalScope,
    #[ts(type = "number")]
    pub amount_usd_nanos: u64,
    pub review_digest: String,
    pub user_event_digest: String,
    pub source_digest: String,
    pub uncertain_outcomes: u32,
    pub state: HistoricalAccountingState,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoricalAccountingPage {
    pub revision: String,
    pub offset: u32,
    pub total: u32,
    pub next_offset: Option<u32>,
    pub entries: Vec<HistoricalAccountingSummary>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoricalSourceEntry {
    pub gap: crate::currency_budget::CurrencyHistoryGap,
    /// None means the original confirmation/account projection is unavailable.
    pub accounted: Option<bool>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoricalSourcePage {
    pub revision: String,
    pub offset: u32,
    pub total: u32,
    pub pending_total: Option<u32>,
    pub next_offset: Option<u32>,
    pub entries: Vec<HistoricalSourceEntry>,
}
/// A read-only lookup of the exact originally reviewed confirmation. A missing
/// record is explicit and never causes the service to repeat a confirmation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoricalConfirmationLookup {
    pub project_id: String,
    pub scope: HistoricalScope,
    pub review_digest: String,
    pub record: Option<HistoricalAccounting>,
}

pub fn gap_matches(
    scope: &HistoricalScope,
    gap: &crate::currency_budget::CurrencyHistoryGap,
) -> bool {
    use crate::currency_budget::CurrencyHistoryReason;
    match scope {
        HistoricalScope::Api {
            project_id,
            attempt_id,
        } => {
            gap.reason == CurrencyHistoryReason::ApiWithoutBudget
                && gap.project_id.as_ref() == Some(project_id)
                && gap.source_id == *attempt_id
        }
        HistoricalScope::Management {
            operation_id,
            original_project_id,
        } => {
            gap.reason != CurrencyHistoryReason::ApiWithoutBudget
                && gap.project_id == *original_project_id
                && gap.source_id == *operation_id
        }
    }
}

/// Verified local source scope. This is not a supplier invoice or user approval.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoricalSource {
    pub scope: HistoricalScope,
    pub digest: String,
    pub references: Vec<String>,
    pub uncertain_outcomes: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum HistoricalScope {
    Management {
        operation_id: String,
        original_project_id: Option<String>,
    },
    Api {
        project_id: String,
        attempt_id: String,
    },
}
impl HistoricalScope {
    pub fn key(&self) -> Result<String, String> {
        // The logical operation key stays the same when an originally unknown
        // project is assigned. Multiple reviewed fragments are one call scope.
        match self {
            Self::Management { operation_id, .. } => digest(&("management", operation_id)),
            Self::Api {
                project_id,
                attempt_id,
            } => digest(&("api", project_id, attempt_id)),
        }
    }
    pub fn operation_id(&self) -> &str {
        match self {
            Self::Management { operation_id, .. } => operation_id,
            Self::Api { attempt_id, .. } => attempt_id,
        }
    }
    pub fn validate(&self, project: &str) -> Result<(), String> {
        identifier(self.operation_id())?;
        let original = match self {
            Self::Management {
                original_project_id,
                ..
            } => original_project_id.as_deref(),
            Self::Api { project_id, .. } => Some(project_id.as_str()),
        };
        if let Some(original) = original {
            identifier(original)?;
            require(
                original == project,
                "historical charge cannot change its known original project",
            )?;
        }
        Ok(())
    }
}
