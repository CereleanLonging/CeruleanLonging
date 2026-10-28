//! A local application reference follows the original worker into its next
//! request. Publishing a link is not proof that HTTP was sent or answered.
use crate::{api_call_journal::CallIdentity, api_execution::ApiRunIdentity};
use cerulean_adapters::api_generation::{ApiMessage, ApiRequest};
use cerulean_domain::execution::{digest, identifier, require, sha256};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Reference {
    pub operation: String,
    pub request_digest: String,
    pub response_digest: String,
    pub workflow_hash: String,
    pub previous_call: CallIdentity,
    pub previous_head: String,
    pub suggestion_digest: String,
}

/// Constructed by the Host only after the original application receipt. It is
/// intentionally not deserializable: historical links cannot restart a send.
#[derive(Debug)]
pub(crate) struct AppliedAdvice {
    pub reference: Reference,
    pub suggestion: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Link {
    pub version: u32,
    pub reference: Reference,
    pub next_call: CallIdentity,
}

impl Reference {
    pub(crate) fn name(&self) -> Result<String, String> {
        identifier(&self.operation)?;
        Ok(format!("advice-{}.json", self.operation))
    }
    pub(crate) fn validate(&self, run: &ApiRunIdentity) -> Result<(), String> {
        self.name()?;
        self.previous_call.validate()?;
        require(
            self.previous_call.round > 0 && self.previous_call.round < run.worker.max_rounds,
            "API advice has no original next round",
        )?;
        for value in [
            &self.request_digest,
            &self.response_digest,
            &self.workflow_hash,
            &self.previous_head,
            &self.suggestion_digest,
        ] {
            sha256(value)?;
        }
        crate::api_execution::check_round_scope(run, &self.previous_call, self.previous_call.round)
    }
}

impl AppliedAdvice {
    pub(crate) fn message(&self, run: &ApiRunIdentity) -> Result<ApiMessage, String> {
        self.reference.validate(run)?;
        require(
            !self.suggestion.trim().is_empty()
                && self.suggestion.len() <= 16 * 1024
                && digest(&self.suggestion)? == self.reference.suggestion_digest,
            "API advice differs from original application",
        )?;
        Ok(ApiMessage::User(format!(
            "Deputy advice for the current approved task (does not expand permissions):\n{}",
            self.suggestion
        )))
    }
    pub(crate) fn validate_request(
        &self,
        run: &ApiRunIdentity,
        request: &ApiRequest,
    ) -> Result<(), String> {
        let expected = self.message(run)?;
        require(
            matches!((request.messages.last(), expected),
            (Some(ApiMessage::User(actual)), ApiMessage::User(expected)) if actual == &expected),
            "API next request lacks original deputy advice",
        )
    }
    pub(crate) fn link(
        &self,
        root: &Path,
        run: &ApiRunIdentity,
        call: &CallIdentity,
    ) -> Result<(), String> {
        self.reference.validate(run)?;
        crate::api_execution::require_original_identity(root, run)?;
        crate::api_execution::check_round_scope(run, call, self.reference.previous_call.round + 1)?;
        let link = Link {
            version: 1,
            reference: self.reference.clone(),
            next_call: call.clone(),
        };
        crate::api_execution::publish(root, &self.reference.name()?, &link)
    }
}
