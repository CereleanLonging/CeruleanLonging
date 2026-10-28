//! Pure price coverage calculations for a future guarded monetary workflow.
//! These contracts do not establish provider terms, authenticate approval, count input or grant
//! permission to send. Usage must already have provider-specific, complete,
//! nonoverlapping cache semantics; no endpoint inherits another one's rules.
use super::{model, ManagementCountPrice, Rates, TokenCeiling, Usage, MAX_TOKENS};
use crate::{
    api_worker::ApiTarget,
    execution::{require, timestamp},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use ts_rs::TS;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum RequestedService {
    Omitted,
    Explicit { tier: String },
}
impl RequestedService {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Omitted => Ok(()),
            Self::Explicit { tier } => service(tier),
        }
    }
}

/// An explicit selection axis, never a provider-dependent implicit default.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum ContextMetric {
    TotalInputIncludingCache,
}

/// Session-wide repricing cannot be inferred from per-request accounting. A
/// model whose provider terms charge the whole session requires another
/// workflow and contract; it cannot be represented by this supported variant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum BillingBasis {
    PerRequest,
}

/// Integer closed interval. A provider threshold "over T" starts at T + 1.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InputRange {
    #[ts(type = "number")]
    pub from: u64,
    #[ts(type = "number")]
    pub through: u64,
}
impl InputRange {
    pub fn validate(&self) -> Result<(), String> {
        require(
            self.from <= self.through && self.through <= MAX_TOKENS,
            "invalid complete-input price interval",
        )
    }
    fn contains(&self, tokens: u64) -> bool {
        self.from <= tokens && tokens <= self.through
    }
    fn intersection(&self, other: &Self) -> Option<Self> {
        let from = self.from.max(other.from);
        let through = self.through.min(other.through);
        (from <= through).then_some(Self { from, through })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PriceBand {
    pub actual_model: String,
    pub actual_service: String,
    pub input: InputRange,
    pub rates: Rates,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopedPriceQuote {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub count_price: Option<ManagementCountPrice>,
    pub schema: u32,
    pub target: ApiTarget,
    pub requested_model: String,
    pub requested_service: RequestedService,
    /// Every model/service combination must cover the entire request interval.
    pub reported_models: Vec<String>,
    pub reported_services: Vec<String>,
    pub billing_basis: BillingBasis,
    pub context_metric: ContextMetric,
    pub bands: Vec<PriceBand>,
    pub valid_from: String,
    pub valid_until: String,
    /// User-reviewed terms; not a fetched price or an authentic supplier bill.
    pub source: String,
}
pub type ScopedQuote = ScopedPriceQuote;

/// Caller-proved bounds for the complete exact request, including all history,
/// tools and schemas. Constructing this value is not itself proof of a bound.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PricingRequest {
    pub target: ApiTarget,
    pub requested_model: String,
    pub requested_service: RequestedService,
    pub input: InputRange,
    #[ts(type = "number")]
    pub output_ceiling: u64,
    pub cache_read_allowed: bool,
    pub cache_write_allowed: bool,
}
impl PricingRequest {
    pub fn validate(&self) -> Result<(), String> {
        self.target.url()?;
        require(
            matches!(self.target, ApiTarget::OpenAi {}),
            "scoped pricing currently supports the official OpenAI endpoint only",
        )?;
        model(&self.requested_model)?;
        self.requested_service.validate()?;
        self.input.validate()?;
        self.ceiling(self.input.through).validate()
    }
    fn ceiling(&self, total_input: u64) -> TokenCeiling {
        TokenCeiling {
            total_input,
            output: self.output_ceiling,
            cache_read_allowed: self.cache_read_allowed,
            cache_write_allowed: self.cache_write_allowed,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PricingObservation {
    pub actual_model: String,
    /// Missing/null is unknown, never the requested tier or a default tier.
    pub actual_service: Option<String>,
    pub usage: Usage,
}

pub(super) fn service(value: &str) -> Result<(), String> {
    require(
        !value.is_empty()
            && value.len() <= 64
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b)),
        "invalid service tier",
    )
}
fn reported_service(value: &str) -> Result<(), String> {
    service(value)?;
    require(
        value != "auto",
        "requested service selector is not an actual billing service",
    )
}

impl ScopedPriceQuote {
    pub fn validate(&self) -> Result<(), String> {
        require(self.schema == 1, "unsupported scoped price quote")?;
        self.target.url()?;
        require(
            matches!(self.target, ApiTarget::OpenAi {}),
            "scoped pricing currently supports the official OpenAI endpoint only",
        )?;
        if let Some(price) = &self.count_price {
            price.validate()?;
        }
        model(&self.requested_model)?;
        self.requested_service.validate()?;
        require(
            !self.reported_models.is_empty() && self.reported_models.len() <= 16,
            "scoped price requires explicit reported models",
        )?;
        require(
            !self.reported_services.is_empty() && self.reported_services.len() <= 16,
            "scoped price requires explicit reported services",
        )?;
        let mut models = BTreeSet::new();
        for value in &self.reported_models {
            model(value)?;
            require(
                models.insert(value.clone()),
                "duplicate scoped reported model",
            )?;
        }
        let mut services = BTreeSet::new();
        for value in &self.reported_services {
            reported_service(value)?;
            require(
                services.insert(value.clone()),
                "duplicate scoped reported service",
            )?;
        }
        require(
            !self.bands.is_empty() && self.bands.len() <= 512,
            "scoped price bands required and bounded",
        )?;
        for band in &self.bands {
            require(
                models.contains(&band.actual_model) && services.contains(&band.actual_service),
                "price band outside declared models or services",
            )?;
            band.input.validate()?;
            band.rates.validate()?;
        }
        for model in &self.reported_models {
            for service in &self.reported_services {
                let bands = self.bands_for(model, service);
                require(!bands.is_empty(), "model/service price coverage missing")?;
                for pair in bands.windows(2) {
                    require(
                        pair[0].input.through < pair[1].input.from,
                        "overlapping model/service price bands",
                    )?;
                }
            }
        }
        timestamp(&self.valid_from)?;
        timestamp(&self.valid_until)?;
        require(
            chrono::DateTime::parse_from_rfc3339(&self.valid_from).map_err(|e| e.to_string())?
                < chrono::DateTime::parse_from_rfc3339(&self.valid_until)
                    .map_err(|e| e.to_string())?,
            "invalid scoped price validity interval",
        )?;
        require(
            !self.source.trim().is_empty()
                && self.source.len() <= 1024
                && !self.source.chars().any(char::is_control),
            "scoped price provenance required",
        )
    }

    pub fn valid_at(&self, at: &str) -> Result<(), String> {
        self.validate()?;
        let at = chrono::DateTime::parse_from_rfc3339(at)
            .map_err(|_| "invalid scoped price observation time")?;
        let from =
            chrono::DateTime::parse_from_rfc3339(&self.valid_from).map_err(|e| e.to_string())?;
        let until =
            chrono::DateTime::parse_from_rfc3339(&self.valid_until).map_err(|e| e.to_string())?;
        require(from <= at && at < until, "scoped price quote not current")
    }

    fn bands_for(&self, model: &str, service: &str) -> Vec<&PriceBand> {
        let mut bands: Vec<_> = self
            .bands
            .iter()
            .filter(|band| band.actual_model == model && band.actual_service == service)
            .collect();
        bands.sort_by_key(|band| band.input.from);
        bands
    }

    fn bind(&self, request: &PricingRequest) -> Result<(), String> {
        request.validate()?;
        require(
            self.target == request.target
                && self.requested_model == request.requested_model
                && self.requested_service == request.requested_service,
            "request differs from scoped price target, model or service",
        )
    }

    fn maximum(&self, request: &PricingRequest) -> Result<u64, String> {
        self.bind(request)?;
        let mut maximum = 0;
        for model in &self.reported_models {
            for service in &self.reported_services {
                let mut covered_through = None;
                for band in self.bands_for(model, service) {
                    let Some(overlap) = band.input.intersection(&request.input) else {
                        continue;
                    };
                    let expected_from = covered_through.map_or(request.input.from, |n| n + 1);
                    require(
                        overlap.from == expected_from,
                        "scoped price does not cover complete request input range",
                    )?;
                    // Rates are nonnegative, so the greatest input *within this
                    // band* gives its largest charge. Never extend a short but
                    // expensive band to the whole request's upper bound.
                    maximum = maximum.max(band.rates.reserve(&request.ceiling(overlap.through))?);
                    covered_through = Some(overlap.through);
                }
                require(
                    covered_through == Some(request.input.through),
                    "scoped price does not cover complete request input range",
                )?;
            }
        }
        Ok(maximum)
    }

    /// Maximum liability across every permitted model/service and reachable
    /// input band. No monotonic relationship between different bands is assumed.
    pub fn reserve(&self, request: &PricingRequest, at: &str) -> Result<u64, String> {
        self.valid_at(at)?;
        self.maximum(request)
    }

    /// Evaluate an original response under its original quote/request. The
    /// caller must bind the original valid reservation time and evidence; late
    /// responses are not repriced or rejected merely because time has passed.
    pub fn charge(
        &self,
        request: &PricingRequest,
        observation: &PricingObservation,
    ) -> Result<u64, String> {
        self.validate()?;
        let maximum = self.maximum(request)?;
        model(&observation.actual_model)?;
        let actual_service = observation
            .actual_service
            .as_deref()
            .ok_or("actual service tier unknown")?;
        reported_service(actual_service)?;
        observation.usage.validate()?;
        let input = observation
            .usage
            .input
            .checked_add(observation.usage.cache_read)
            .and_then(|n| n.checked_add(observation.usage.cache_write))
            .ok_or("complete billable input overflow")?;
        require(
            request.input.contains(input)
                && request
                    .ceiling(request.input.through)
                    .contains(&observation.usage)?,
            "observed usage outside scoped request limits",
        )?;
        let bands: Vec<_> = self
            .bands_for(&observation.actual_model, actual_service)
            .into_iter()
            .filter(|band| band.input.contains(input))
            .collect();
        require(
            bands.len() == 1,
            "actual model/service/context not uniquely priced",
        )?;
        let charged = bands[0].rates.charge(&observation.usage)?;
        require(
            charged <= maximum,
            "scoped charge exceeds maximum liability",
        )?;
        Ok(charged)
    }
}

#[cfg(test)]
mod tests;
