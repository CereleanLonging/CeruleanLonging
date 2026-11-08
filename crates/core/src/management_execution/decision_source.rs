//! Read validated original decisions without inventing a provider receipt.
use super::*;
use cerulean_adapters::deputy_management::{DeputyRequest, DeputyResult};

#[derive(Clone, Debug, Serialize)]
#[serde(
    tag = "source",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum StoredDecision {
    Omp {
        request: Box<super::omp::OmpRequest>,
        response: Option<Box<super::omp::OmpResponse>>,
    },
    OpenAi {
        observation: Option<Box<DecisionObservation>>,
    },
    CodexSession {
        request: Box<ManagementHandoff>,
        response: Option<Box<ExternalChiefDecision>>,
    },
    Provider {
        request: Box<DeputyRequest>,
        response: Option<Box<DeputyResult>>,
    },
    DeepSeek {
        request: Box<DeputyRequest>,
        response: Option<Box<DeputyResult>>,
    },
}
impl StoredDecision {
    pub(crate) fn envelope(&self) -> Option<&DecisionEnvelope> {
        match self {
            Self::Omp { response, .. } => response.as_ref().and_then(|r| r.envelope.as_ref()),
            Self::OpenAi { observation } => observation.as_ref().map(|o| &o.envelope),
            Self::CodexSession { response, .. } => response.as_ref().map(|r| &r.envelope),
            Self::DeepSeek { response, .. } | Self::Provider { response, .. } => {
                response.as_ref().and_then(|r| r.envelope())
            }
        }
    }
}
impl DecisionJournal {
    pub(crate) fn stored_decision(&self, id: &str) -> Result<StoredDecision, String> {
        require(self.healthy(), "management source publication unresolved")?;
        let op = self
            .operations
            .get(id)
            .ok_or("management request missing")?;
        if op.envelope().is_some() {
            self.original_envelope(id)?;
        }
        Ok(if let Some(request) = &op.omp_request {
            StoredDecision::Omp {
                request: Box::new(request.clone()),
                response: op.omp_response.clone().map(Box::new),
            }
        } else if let Some(request) = &op.external_request {
            StoredDecision::CodexSession {
                request: Box::new(request.clone()),
                response: op.external_response.clone().map(Box::new),
            }
        } else if let Some(request) = &op.deputy_request {
            if matches!(
                request.handoff.source,
                cerulean_domain::management_ingress::ManagementSource::Provider { .. }
            ) {
                StoredDecision::Provider {
                    request: Box::new(request.clone()),
                    response: op.deputy_response.clone().map(Box::new),
                }
            } else {
                StoredDecision::DeepSeek {
                    request: Box::new(request.clone()),
                    response: op.deputy_response.clone().map(Box::new),
                }
            }
        } else {
            StoredDecision::OpenAi {
                observation: op.observation.clone().map(Box::new),
            }
        })
    }
    /// Preserve the exact legacy observation digest for old advice receipts.
    /// New sources hash their original provider-bound response, never a fake
    /// OpenAI observation and never the mutable live configuration.
    pub(crate) fn original_response_digest(&self, id: &str) -> Result<String, String> {
        self.original_envelope(id)?;
        let op = &self.operations[id];
        if let Some(r) = &op.omp_response {
            digest(r)
        } else if let Some(r) = &op.external_response {
            digest(r)
        } else if let Some(r) = &op.deputy_response {
            digest(r)
        } else {
            digest(self.observation(id)?)
        }
    }
}
