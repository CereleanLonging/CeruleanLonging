//! The original verifier record format is unchanged when this binding is absent.
use cerulean_domain::{
    execution::{digest, EventAnchor},
    verification_recheck::RecheckApproval,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RecheckBinding {
    schema_version: u32,
    approval: EventAnchor,
    approval_digest: String,
    review_digest: String,
}
impl RecheckBinding {
    pub(crate) fn new(approval: &RecheckApproval, event: &EventAnchor) -> Result<Self, String> {
        Ok(Self {
            schema_version: 1,
            approval: event.clone(),
            approval_digest: digest(approval)?,
            review_digest: digest(&approval.review)?,
        })
    }
}
