//! Bounded, auxiliary A6API response-header evidence. It is not a billing or
//! remote-execution verdict. Rejected bytes and unrelated headers never escape.
use super::ApiTarget;
use cerulean_domain::credentials::CredentialProvider;
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};
use std::fmt;

const HEADER_NAME: &str = "x-oneapi-request-id";
const MAX_REQUEST_ID_BYTES: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GatewayHeaders {
    pub status: u16,
    pub request_id: GatewayRequestId,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum GatewayRequestId {
    Missing {},
    Accepted { value: String },
    Rejected { reason: GatewayRequestIdRejection },
}

impl fmt::Debug for GatewayRequestId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing {} => f.debug_struct("Missing").finish(),
            Self::Accepted { .. } => f
                .debug_struct("Accepted")
                .field("value", &"[SAFE_GATEWAY_ID]")
                .finish(),
            Self::Rejected { reason } => {
                f.debug_struct("Rejected").field("reason", reason).finish()
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub enum GatewayRequestIdRejection {
    InvalidLength,
    InvalidCharacters,
    RepeatedHeader,
    CredentialReflection,
}

impl GatewayHeaders {
    /// Revalidate untrusted auxiliary records without a live credential.
    /// This checks the local format and known credential prefixes only; it
    /// cannot establish that a stored value was compared with the original key.
    pub fn validate(&self) -> Result<(), String> {
        if !(100..=599).contains(&self.status) {
            return Err("invalid gateway response status".into());
        }
        if let GatewayRequestId::Accepted { value } = &self.request_id {
            validate_candidate(value.as_bytes(), None)
                .map_err(|_| "invalid gateway request identity".to_owned())?;
        }
        Ok(())
    }
}

/// Capture exactly one fixed header for the fixed A6API target. No request body,
/// other header, arbitrary endpoint or raw rejected value is retained. Callers
/// must validate a returned observation before publishing auxiliary evidence.
pub(super) fn capture(
    target: &ApiTarget,
    status: u16,
    headers: &HeaderMap,
    key: &str,
) -> Option<GatewayHeaders> {
    if target.provider() != CredentialProvider::A6Api {
        return None;
    }
    let values = headers.get_all(HEADER_NAME);
    let mut values = values.iter();
    let request_id = match values.next() {
        None => GatewayRequestId::Missing {},
        Some(_) if values.next().is_some() => GatewayRequestId::Rejected {
            reason: GatewayRequestIdRejection::RepeatedHeader,
        },
        Some(value) => match validate_candidate(value.as_bytes(), Some(key.as_bytes())) {
            Err(reason) => GatewayRequestId::Rejected { reason },
            Ok(()) => match std::str::from_utf8(value.as_bytes()) {
                Ok(value) => GatewayRequestId::Accepted {
                    value: value.to_owned(),
                },
                Err(_) => GatewayRequestId::Rejected {
                    reason: GatewayRequestIdRejection::InvalidCharacters,
                },
            },
        },
    };
    Some(GatewayHeaders { status, request_id })
}

fn validate_candidate(raw: &[u8], key: Option<&[u8]>) -> Result<(), GatewayRequestIdRejection> {
    // Bounds precede any scan or allocation; duplicates are rejected earlier.
    // These are conservative local rules, not the provider's format contract.
    if raw.is_empty() || raw.len() > MAX_REQUEST_ID_BYTES {
        return Err(GatewayRequestIdRejection::InvalidLength);
    }
    let reflected = key.is_some_and(|key| {
        !key.is_empty() && key.len() <= raw.len() && raw.windows(key.len()).any(|part| part == key)
    });
    if reflected
        || raw
            .get(..3)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"sk-"))
        || raw
            .get(..6)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(b"Bearer"))
    {
        return Err(GatewayRequestIdRejection::CredentialReflection);
    }
    if !raw
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(byte))
    {
        return Err(GatewayRequestIdRejection::InvalidCharacters);
    }
    Ok(())
}

#[cfg(test)]
#[path = "gateway_tests.rs"]
mod tests;
