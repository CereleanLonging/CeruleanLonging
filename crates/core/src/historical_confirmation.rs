//! Review packet stored in the original user workflow, without a circular event hash.
use crate::currency_budget::{HistoricalCharge, HistoricalSource};
use cerulean_domain::execution::{digest, require};
use serde::{Deserialize, Serialize};

pub const APPROVAL_SOURCE: &str = "authenticated local-user historical-charge confirmation";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HistoricalReview {
    pub schema: u32,
    pub project_id: String,
    pub source: HistoricalSource,
    pub policy_digest: String,
    pub previous_ledger_head: String,
    pub amount_usd_nanos: u64,
    pub billing_reference: String,
    pub explanation: String,
}
impl HistoricalReview {
    pub fn input(&self) -> cerulean_domain::historical_money::HistoricalChargeInput {
        cerulean_domain::historical_money::HistoricalChargeInput {
            project_id: self.project_id.clone(),
            scope: self.source.scope.clone(),
            amount_usd_nanos: self.amount_usd_nanos,
            billing_reference: self.billing_reference.clone(),
            explanation: self.explanation.clone(),
        }
    }
    pub fn view(
        &self,
    ) -> Result<cerulean_domain::historical_money::HistoricalChargeReview, String> {
        Ok(cerulean_domain::historical_money::HistoricalChargeReview {
            input: self.input(),
            source: self.source.clone(),
            policy_digest: self.policy_digest.clone(),
            previous_ledger_head: self.previous_ledger_head.clone(),
            review_digest: self.digest()?,
        })
    }
    pub fn digest(&self) -> Result<String, String> {
        digest(&("historical-charge-review-v1", self))
    }
    pub(crate) fn validate(&self, project: &str, expected: &str) -> Result<(), String> {
        require(
            self.schema == 1 && self.project_id == project && self.digest()? == expected,
            "historical confirmation differs from the original review or project",
        )?;
        require(
            !self.source.references.is_empty() && self.source.references.len() <= 256,
            "historical review must contain the original bounded source references",
        )?;
        for reference in &self.source.references {
            require(
                !reference.is_empty() && reference.len() <= 2048 && !reference.contains('\0'),
                "invalid historical source reference",
            )?;
        }
        self.charge(&"0".repeat(64)).validate()
    }
    pub(crate) fn charge(&self, event: &str) -> HistoricalCharge {
        HistoricalCharge {
            project_id: self.project_id.clone(),
            scope: self.source.scope.clone(),
            source_digest: self.source.digest.clone(),
            policy_digest: self.policy_digest.clone(),
            previous_ledger_head: self.previous_ledger_head.clone(),
            amount_usd_nanos: self.amount_usd_nanos,
            billing_reference: self.billing_reference.clone(),
            explanation: self.explanation.clone(),
            user_event_digest: event.into(),
        }
    }
}
