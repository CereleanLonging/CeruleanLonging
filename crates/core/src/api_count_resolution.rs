//! A review binds the original API request, count outcome and generation source.
//! The Host must authenticate its original local-user confirmation before use.
use cerulean_domain::{
    api_count_resolution::ApiCountResolutionInput,
    currency_budget::MAX_USD_NANOS,
    execution::{digest, require, sha256},
};
use serde::{Deserialize, Serialize};

pub const APPROVAL_SOURCE: &str = "authenticated local-user API-count fee resolution";
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiCountResolutionReview {
    pub input: ApiCountResolutionInput,
    pub source_digest: String,
    pub policy_digest: String,
    pub previous_ledger_head: String,
    pub maximum_usd_nanos: u64,
    pub original_outcome: String,
    pub generation_digest: String,
}
impl ApiCountResolutionReview {
    pub fn resolution(&self, event: &str) -> ApiCountResolution {
        ApiCountResolution {
            review: self.clone(),
            user_event_digest: event.into(),
        }
    }
    pub fn validate_for(&self, project: &str, expected: &str) -> Result<(), String> {
        self.validate()?;
        require(
            self.input.project_id == project && self.digest()? == expected,
            "API count review differs from original project or digest",
        )
    }
    pub fn digest(&self) -> Result<String, String> {
        digest(&("api-count-fee-resolution-review-v1", self))
    }
    pub fn validate(&self) -> Result<(), String> {
        self.input.validate()?;
        require(
            matches!(self.original_outcome.as_str(), "counted" | "unknown"),
            "pending API count cannot be reconciled",
        )?;
        require(
            self.maximum_usd_nanos <= MAX_USD_NANOS,
            "API count maximum exceeds exact range",
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
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiCountResolution {
    pub review: ApiCountResolutionReview,
    pub user_event_digest: String,
}
impl ApiCountResolution {
    pub fn validate(&self) -> Result<(), String> {
        self.review.validate()?;
        sha256(&self.user_event_digest)
    }
}
