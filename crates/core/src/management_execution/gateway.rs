//! Auxiliary gateway observations bound to the original provider management intent.
use super::*;
use crate::gateway_response_evidence::GatewayReview;
use crate::gateway_response_evidence::{Binding, CompletionSource, GatewayAttempt, Lane, Outcome};
use cerulean_adapters::deputy_management::DeputyOutcome;
use cerulean_domain::credentials::CredentialProvider;

impl DecisionJournal {
    fn gateway_binding(&self, id: &str) -> Result<Option<Binding>, String> {
        require(self.healthy(), "management journal requires recovery")?;
        let op = self
            .operations
            .get(id)
            .ok_or("original management intent missing")?;
        let Some(request) = &op.deputy_request else {
            return Ok(None);
        };
        if request.wire.target.provider() != CredentialProvider::A6Api {
            return Ok(None);
        }
        Ok(Some(Binding {
            endpoint: Binding::endpoint_override(&request.wire.target)?,
            lane: Lane::Management,
            project_id: op.intent.project_id.clone(),
            local_id: id.into(),
            request_digest: digest(request)?,
            intent_hash: op.intent_hash.clone(),
        }))
    }
    fn gateway_source(&self, id: &str) -> Result<Option<CompletionSource>, String> {
        let op = self
            .operations
            .get(id)
            .ok_or("original management intent missing")?;
        let Some(response) = &op.deputy_response else {
            return Ok(None);
        };
        let outcome = match &response.outcome {
            DeputyOutcome::Received { .. } => Outcome::ManagementReceived {},
            DeputyOutcome::Failed { failure } => Outcome::failure(failure),
        };
        Ok(Some(CompletionSource {
            record_hash: op
                .response_hash
                .clone()
                .ok_or("original management response hash missing")?,
            outcome_digest: digest(response)?,
            outcome,
            body_response_id: response.billing().map(|b| b.response_id.clone()),
        }))
    }
    /// Called only after begin_deputy publishes the original Intent.
    pub(crate) fn gateway_capture(&self, id: &str) -> Option<GatewayAttempt> {
        let root = self.root.parent()?;
        self.gateway_binding(id)
            .ok()
            .flatten()
            .map(|binding| GatewayAttempt::new(root, binding))
    }
    /// Failure stays a diagnostic gap; it cannot change a published decision.
    pub(crate) fn finish_gateway(&self, id: &str, capture: GatewayAttempt) {
        let original = self.gateway_binding(id).and_then(|binding| {
            let binding = binding.ok_or("gateway completion provider changed")?;
            require(
                capture.binding_matches(&binding),
                "gateway completion original intent differs",
            )?;
            self.gateway_source(id)?
                .ok_or("gateway original management response missing".into())
        });
        if let Ok(source) = original {
            let _ = capture.finish(source);
        }
    }
    // Read only the original operation's sidecar. Errors remain diagnostic
    // failures and cannot resolve, resend or alter its management result.
    pub(crate) fn gateway_evidence(&self, id: &str) -> Result<Option<GatewayReview>, String> {
        let Some(binding) = self.gateway_binding(id)? else {
            return Ok(None);
        };
        crate::gateway_response_evidence::read(
            self.root.parent().ok_or("management data root missing")?,
            &binding,
            self.gateway_source(id)?.as_ref(),
        )
        .map(Some)
    }
}

#[cfg(all(test, windows))]
mod tests;
