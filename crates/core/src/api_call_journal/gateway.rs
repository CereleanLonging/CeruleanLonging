//! Optional gateway diagnostics follow the original Intent and Outcome.
use super::*;
use crate::gateway_response_evidence::{
    Binding, CompletionSource, GatewayAttempt, GatewayReview, Lane, Outcome,
};
use cerulean_adapters::api_generation::GatewayObserver;
use cerulean_domain::credentials::CredentialProvider;

impl ApiCallJournal {
    fn gateway_binding(&self) -> Result<Option<Binding>, String> {
        if self.expected.fingerprint.target.provider() != CredentialProvider::A6Api {
            return Ok(None);
        }
        require(
            self.healthy() && self.state.intent_recorded,
            "gateway observation requires original API intent",
        )?;
        Ok(Some(Binding {
            lane: Lane::Worker,
            project_id: self.expected.project_id.clone(),
            local_id: self.expected.request_id.clone(),
            request_digest: self.identity_digest.clone(),
            intent_hash: self
                .intent_head
                .clone()
                .ok_or("original API intent hash missing")?,
        }))
    }
    fn gateway_source(&self) -> Result<Option<CompletionSource>, String> {
        let Some(outcome) = &self.state.outcome else {
            return Ok(None);
        };
        let (kind, response) = match outcome {
            CallOutcome::Reply { observation } => {
                (Outcome::Reply {}, Some(observation.response_id.clone()))
            }
            CallOutcome::Failure { failure } => (Outcome::failure(failure), None),
        };
        Ok(Some(CompletionSource {
            record_hash: self
                .state
                .response_head
                .clone()
                .ok_or("original API response hash missing")?,
            outcome_digest: digest(outcome)?,
            outcome: kind,
            body_response_id: response,
        }))
    }
    /// Root is the existing run/attempt evidence directory, never rounds or a round.
    /// Header publication errors are ignored; synchronous IO may delay HTTP processing.
    pub(crate) async fn execute_reply_with_gateway<
        O: cerulean_adapters::api_generation::ApiAttemptResult,
    >(
        &mut self,
        fingerprint: &RequestFingerprint,
        evidence_base: &Path,
        send: impl AsyncFnOnce(&mut GatewayObserver<'_>) -> O,
    ) -> Result<Result<ApiReply, ApiFailure>, String> {
        self.execute_attempt(fingerprint, Some(evidence_base), send)
            .await
    }
    /// The standalone journal API has no run-root authority for gateway files.
    pub(super) async fn execute_attempt<O: cerulean_adapters::api_generation::ApiAttemptResult>(
        &mut self,
        fingerprint: &RequestFingerprint,
        evidence_base: Option<&Path>,
        send: impl AsyncFnOnce(&mut GatewayObserver<'_>) -> O,
    ) -> Result<Result<ApiReply, ApiFailure>, String> {
        require(
            !self.read_only && !self.poisoned && !self.state.intent_recorded,
            "API request already attempted or journal requires recovery; never resend",
        )?;
        require(
            fingerprint == &self.expected.fingerprint,
            "API prepared request differs from bound intent",
        )?;
        if O::REJECTED_USAGE {
            crate::rejected_usage_guard::require_round(&self.root)?;
        }
        self.publish(
            &Event::Intent {
                identity: self.expected.clone(),
            },
            |_| Ok(()),
        )?;
        let mut gateway = evidence_base.and_then(|base| {
            self.gateway_binding()
                .ok()
                .flatten()
                .map(|binding| GatewayAttempt::new(base, binding))
        });
        let attempt = send(&mut |headers| {
            if let Some(capture) = &mut gateway {
                capture.headers(headers);
            }
        })
        .await
        .into();
        let result = attempt.result;
        let outcome = match &result {
            Ok(reply) => CallOutcome::Reply {
                observation: reply.observation().clone(),
            },
            Err(failure) => CallOutcome::Failure {
                failure: failure.clone(),
            },
        };
        // No proposal escapes until the original authoritative response is durable.
        // An earlier headers record remains useful if publication fails or this
        // future is dropped, but never permits retries or repairs the journal.
        self.publish(
            &Event::Outcome {
                outcome,
                rejected_usage: attempt.rejected_usage,
            },
            |_| Ok(()),
        )?;
        if let (Some(capture), Ok(Some(source))) = (gateway, self.gateway_source()) {
            let _ = capture.finish(source);
        }
        Ok(result)
    }
    /// Reads the original journal first; missing diagnostics do not initialize data.
    pub fn gateway_evidence(&self, evidence_base: &Path) -> Result<Option<GatewayReview>, String> {
        let Some(binding) = self.gateway_binding()? else {
            return Ok(None);
        };
        crate::gateway_response_evidence::read(
            evidence_base,
            &binding,
            self.gateway_source()?.as_ref(),
        )
        .map(Some)
    }
}

#[cfg(test)]
mod tests;
