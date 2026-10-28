//! A review binds the original API request, count outcome and generation source.
//! The Host must authenticate its original local-user confirmation before use.
use cerulean_domain::{
    api_generation_resolution::ApiGenerationResolutionInput,
    currency_budget::MAX_USD_NANOS,
    execution::{digest, require, sha256},
};
use serde::{Deserialize, Serialize};

pub const APPROVAL_SOURCE: &str = "authenticated local-user API-generation fee resolution";
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiGenerationResolutionReview {
    pub input: ApiGenerationResolutionInput,
    pub source_digest: String,
    pub policy_digest: String,
    pub previous_ledger_head: String,
    pub maximum_usd_nanos: u64,
    pub original_outcome: String,
    pub count_digest: String,
}
impl ApiGenerationResolutionReview {
    pub fn resolution(&self, event: &str) -> ApiGenerationResolution {
        ApiGenerationResolution {
            review: self.clone(),
            user_event_digest: event.into(),
        }
    }
    pub fn validate_for(&self, project: &str, expected: &str) -> Result<(), String> {
        self.validate()?;
        require(
            self.input.project_id == project && self.digest()? == expected,
            "API generation review differs from original project or digest",
        )
    }
    pub fn digest(&self) -> Result<String, String> {
        digest(&("api-generation-fee-resolution-review-v1", self))
    }
    pub fn validate(&self) -> Result<(), String> {
        self.input.validate()?;
        require(
            matches!(self.original_outcome.as_str(), "unknown" | "unpriced"),
            "only unknown or unpriced API generation can be reconciled",
        )?;
        require(
            self.maximum_usd_nanos <= MAX_USD_NANOS,
            "API generation maximum exceeds exact range",
        )?;
        for hash in [
            &self.source_digest,
            &self.policy_digest,
            &self.previous_ledger_head,
            &self.count_digest,
        ] {
            sha256(hash)?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiGenerationResolution {
    pub review: ApiGenerationResolutionReview,
    pub user_event_digest: String,
}
impl ApiGenerationResolution {
    pub fn validate(&self) -> Result<(), String> {
        self.review.validate()?;
        sha256(&self.user_event_digest)
    }
}
