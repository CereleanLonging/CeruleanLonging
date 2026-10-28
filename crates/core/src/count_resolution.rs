//! Exact local-user review for count billing and continuation retirement.
//! Neither action changes the original count outcome or permits another send.
use cerulean_domain::{
    count_resolution::{CountResolutionAction, CountResolutionInput},
    execution::{digest, require, sha256},
};
use serde::{Deserialize, Serialize};

pub const APPROVAL_SOURCE: &str = "authenticated local-user management-count resolution";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CountResolutionReview {
    pub input: CountResolutionInput,
    pub source_digest: String,
    pub policy_digest: String,
    pub previous_ledger_head: String,
    pub maximum_usd_nanos: u64,
    pub original_outcome: String,
    pub generation_digest: String,
}
impl CountResolutionReview {
    pub fn input(&self) -> CountResolutionInput {
        self.input.clone()
    }
    pub fn digest(&self) -> Result<String, String> {
        digest(&("management-count-resolution-review-v1", self))
    }
    pub fn key(&self) -> Result<String, String> {
        self.input.validate()?;
        Ok(format!(
            "{}:{}:{}",
            self.input.project_id,
            self.input.operation_id,
            match self.input.action {
                CountResolutionAction::SettleFee { .. } => "fee",
                CountResolutionAction::RetireContinuation { .. } => "continuation",
            }
        ))
    }
    pub fn view(&self) -> Result<cerulean_domain::count_resolution::CountResolutionReview, String> {
        Ok(cerulean_domain::count_resolution::CountResolutionReview {
            input: self.input(),
            source_digest: self.source_digest.clone(),
            policy_digest: self.policy_digest.clone(),
            previous_ledger_head: self.previous_ledger_head.clone(),
            maximum_usd_nanos: self.maximum_usd_nanos,
            original_outcome: self.original_outcome.clone(),
            generation_digest: self.generation_digest.clone(),
            review_digest: self.digest()?,
        })
    }
    pub(crate) fn validate(&self, project: &str, expected: &str) -> Result<(), String> {
        self.input.validate()?;
        require(
            self.input.project_id == project && self.digest()? == expected,
            "count resolution differs from original project or review",
        )?;
        require(
            matches!(self.original_outcome.as_str(), "counted" | "unknown"),
            "pending count cannot be resolved while its outcome is outstanding",
        )?;
        require(
            self.maximum_usd_nanos <= cerulean_domain::currency_budget::MAX_USD_NANOS,
            "count fee maximum exceeds exact monetary range",
        )?;
        for hash in [
            &self.source_digest,
            &self.policy_digest,
            &self.previous_ledger_head,
            &self.generation_digest,
        ] {
            sha256(hash)?;
        }
        Ok(())
    }
    pub(crate) fn resolution(&self, event: &str) -> ManagementCountResolution {
        ManagementCountResolution {
            review: self.clone(),
            user_event_digest: event.into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManagementCountResolution {
    pub review: CountResolutionReview,
    pub user_event_digest: String,
}
impl ManagementCountResolution {
    pub fn validate(&self) -> Result<(), String> {
        self.review
            .validate(&self.review.input.project_id, &self.review.digest()?)?;
        sha256(&self.user_event_digest)
    }
}
