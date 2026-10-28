//! Tool-owner evidence crosses the existing API channel, without a second
//! journal reader or a fabricated provider turn. A retained checkpoint is not
//! proof that the worker is still paused, and cannot authorize advice or sends.
use crate::{
    api_call_journal::{ApiCallJournal, CallIdentity, CallOutcome},
    api_execution::{ApiRunIdentity, RoundDriver},
    async_api::Update,
    omp_tool_journal::{ToolCallObservation, ToolCheckpoint, ToolEvidence, ToolScope},
};
use cerulean_adapters::api_generation::{ApiReply, ApiRequest, ToolCall};
use cerulean_domain::execution::{digest, require, sha256};
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::mpsc, time::Duration};
use tokio::sync::oneshot;

#[path = "api_failure_context.rs"]
mod failure_context;
pub(crate) use failure_context::{ErrorTexts, FailureContext, MAX_ERROR_BYTES};

pub(crate) struct ObservedTools {
    pub(crate) checkpoint: ToolCheckpoint,
    pub(crate) errors: ErrorTexts,
}

pub(crate) struct Checkpoint {
    pub(crate) run: ApiRunIdentity,
    pub(crate) call: CallIdentity,
    pub(crate) call_head: String,
    pub(crate) tools: ToolCheckpoint,
    pub(crate) errors: ErrorTexts,
}
/// Historical API evidence suitable for an original management request. It is
/// not a live checkpoint, approval, fee ACK, delivery receipt or continuation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiToolEvidence {
    run: ApiRunIdentity,
    call: CallIdentity,
    call_head: String,
    tools: ToolEvidence,
}
impl ApiToolEvidence {
    pub fn run(&self) -> &ApiRunIdentity {
        &self.run
    }
    pub fn round(&self) -> u32 {
        self.call.round
    }
    pub fn request_id(&self) -> &str {
        &self.call.request_id
    }
    pub fn tool_evidence(&self) -> &ToolEvidence {
        &self.tools
    }
    /// Recovery requires the original stopped worker's journals. No new file,
    /// lock or request is created. A complete later suffix is allowed, but an
    /// incomplete or changed chain cannot silently validate an older prefix.
    /// Approval, run authorization and management budgets remain separate checks.
    pub fn verify_original(&self, root: &Path) -> Result<(), String> {
        self.run.validate_tool_scope()?;
        self.call.validate()?;
        sha256(&self.call_head)?;
        require(
            self.call.round < self.run.worker.max_rounds
                && self.tools.observation().calls.len() <= self.run.worker.max_tool_calls as usize,
            "API tool evidence exceeds original worker limits",
        )?;
        crate::api_execution::require_original_identity(root, &self.run)?;
        self.tools
            .verify_original(&root.join("files/events"), &self.run)?;
        let mut results = self.tools.observation().calls.values().collect::<Vec<_>>();
        results.sort_by_key(|call| call.result_sequence);
        let mut offset = 0;
        let mut pricing = None;
        for round in 1..=self.call.round {
            let journal =
                ApiCallJournal::read_existing(&root.join("rounds").join(format!("{round:03}")))?;
            require(journal.healthy(), "Original API response needs recovery")?;
            crate::api_execution::check_round_scope(&self.run, journal.identity(), round)?;
            if round == self.call.round {
                require(
                    journal.identity() == &self.call && journal.head() == self.call_head,
                    "Original API checkpoint response changed",
                )?;
            }
            let scoped = journal.state().scoped_request.is_some();
            require(
                pricing.is_none_or(|previous| previous == scoped),
                "Original API pricing mode changed",
            )?;
            pricing = Some(scoped);
            if scoped {
                journal.require_fee_ack()?;
            }
            let Some(CallOutcome::Reply { observation }) = &journal.state().outcome else {
                return Err("Original API tool response missing".into());
            };
            require(
                !observation.tool_calls.is_empty(),
                "Original API round had no tools",
            )?;
            let end = offset + observation.tool_calls.len();
            let batch = results
                .get(offset..end)
                .ok_or("Original API tool results missing")?;
            validate_results(batch, &observation.tool_calls)?;
            offset = end;
        }
        require(
            offset == results.len(),
            "API evidence includes tools from another round",
        )
    }
    pub fn verify_incidents(
        &self,
        root: &Path,
        policy: &cerulean_domain::supervision::SupervisionPolicy,
    ) -> Result<Vec<cerulean_domain::supervision::SupervisionIncident>, String> {
        self.verify_original(root)?;
        self.tools.incidents(policy)
    }
}

fn validate_results(
    results: &[&ToolCallObservation],
    proposals: &[ToolCall],
) -> Result<(), String> {
    require(
        results.len() == proposals.len(),
        "API tool checkpoint result count differs",
    )?;
    for (result, proposal) in results.iter().zip(proposals) {
        require(
            result.tool_call_id == proposal.id
                && result.tool_name == proposal.name
                && result.arguments_sha256 == digest(&proposal.arguments)?
                && !result.cancelled
                && result.result_sequence.is_some()
                && result.result_is_error.is_some()
                && result.result_sha256.is_some(),
            "API tool checkpoint differs from the original tool proposal",
        )?;
    }
    Ok(())
}
impl Checkpoint {
    pub(crate) fn evidence(&self) -> ApiToolEvidence {
        ApiToolEvidence {
            run: self.run.clone(),
            call: self.call.clone(),
            call_head: self.call_head.clone(),
            tools: self.tools.evidence().clone(),
        }
    }
    pub(crate) fn validate(
        &self,
        previous: Option<&Self>,
        journal: &ApiCallJournal,
    ) -> Result<(), String> {
        self.tools.matches_scope(&self.run)?;
        failure_context::validate_errors(self.tools.evidence(), &self.errors)?;
        self.call.validate()?;
        sha256(&self.call_head)?;
        crate::api_execution::check_round_scope(&self.run, &self.call, self.call.round)?;
        require(
            journal.healthy()
                && journal.identity() == &self.call
                && journal.head() == self.call_head,
            "API checkpoint differs from the original response journal",
        )?;
        let Some(CallOutcome::Reply { observation }) = &journal.state().outcome else {
            return Err("API tool checkpoint lacks a completed response".into());
        };
        require(
            !observation.tool_calls.is_empty(),
            "API tool checkpoint has no proposed tools",
        )?;
        // This checks the original ACK, not a new monetary permission.
        if journal.state().scoped_request.is_some() {
            journal.require_fee_ack()?;
        }
        let calls = &self.tools.observation().calls;
        let old_calls = previous.map(|p| &p.tools.observation().calls);
        let expected_round = match previous {
            Some(p) => {
                require(p.run == self.run, "API checkpoint changed original run")?;
                require(
                    p.errors
                        .iter()
                        .all(|(id, text)| self.errors.get(id) == Some(text)),
                    "Original API error text changed or disappeared",
                )?;
                require(
                    p.tools.head() != self.tools.head()
                        && p.tools
                            .observation()
                            .calls
                            .iter()
                            .all(|(id, call)| calls.get(id) == Some(call)),
                    "API tool history changed or did not advance",
                )?;
                p.call
                    .round
                    .checked_add(1)
                    .ok_or("API checkpoint round overflow")?
            }
            None => 1,
        };
        require(
            self.call.round == expected_round,
            "API tool checkpoint skipped or repeated a round",
        )?;
        let mut added = calls
            .iter()
            .filter(|(id, _)| old_calls.is_none_or(|old| !old.contains_key(*id)))
            .map(|(_, call)| call)
            .collect::<Vec<_>>();
        added.sort_by_key(|call| call.result_sequence);
        validate_results(&added, &observation.tool_calls)
    }
}

pub(crate) struct Request {
    pub(crate) checkpoint: Checkpoint,
    pub(crate) reply: Reply,
}
pub(crate) type Reply =
    oneshot::Sender<Result<Option<crate::api_advice_link::AppliedAdvice>, String>>;

/// Delegates request/fee handling unchanged. Both API transports use this one
/// boundary after their original file tools have durably recorded a batch.
pub(crate) struct OwnedDriver<'a, D> {
    pub(crate) inner: D,
    pub(crate) sender: &'a mpsc::Sender<Update>,
}
impl<D: RoundDriver> RoundDriver for OwnedDriver<'_, D> {
    fn scoped(&self) -> bool {
        self.inner.scoped()
    }
    async fn authorize(
        &mut self,
        run: &ApiRunIdentity,
    ) -> Result<Option<crate::api_currency::DispatchPermit>, String> {
        self.inner.authorize(run).await
    }
    async fn round(
        &mut self,
        run: &ApiRunIdentity,
        round: u32,
        request: ApiRequest,
        advice: Option<&crate::api_advice_link::AppliedAdvice>,
        root: &Path,
        cancel: &mut oneshot::Receiver<()>,
    ) -> Result<(CallIdentity, ApiReply, String), String> {
        self.inner
            .round(run, round, request, advice, root, cancel)
            .await
    }
    async fn checkpoint(&mut self, run: &ApiRunIdentity) -> Result<(), String> {
        self.inner.checkpoint(run).await
    }
    async fn tools_observed(
        &mut self,
        run: &ApiRunIdentity,
        call: &CallIdentity,
        call_head: &str,
        tools: ObservedTools,
        cancel: &mut oneshot::Receiver<()>,
    ) -> Result<Option<crate::api_advice_link::AppliedAdvice>, String> {
        tools.checkpoint.matches_scope(run)?;
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Update::ToolCheckpoint(Box::new(Request {
                checkpoint: Checkpoint {
                    run: run.clone(),
                    call: call.clone(),
                    call_head: call_head.into(),
                    tools: tools.checkpoint,
                    errors: tools.errors,
                },
                reply,
            })))
            .map_err(|_| "API tool checkpoint owner unavailable; never retry")?;
        tokio::select! {
            result = tokio::time::timeout(Duration::from_secs(30), response) => {
                result.map_err(|_| "API tool checkpoint acknowledgement timed out; never retry")?
                    .map_err(|_| "API tool checkpoint acknowledgement lost; never retry")?
            }
            _ = cancel => Err("API cancelled while waiting for tool checkpoint; never retry".into()),
        }
    }
}
