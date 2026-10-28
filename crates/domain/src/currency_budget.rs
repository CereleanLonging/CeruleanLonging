//! Explicit USD quotes and conservative reservations. These values do not grant
//! execution permission or prove a provider's billing/token ceiling.
pub mod scoped;
use crate::{
    api_worker::ApiTarget,
    execution::{digest, identifier, require, sha256, timestamp},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use ts_rs::TS;

/// JSON-safe integer nanodollars. No floats or automatic currency conversion.
pub const MAX_USD_NANOS: u64 = 9_007_199_254_740_991;
const MAX_RATE: u64 = 1_000_000_000_000;
pub const MAX_TOKENS: u64 = 100_000_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum CurrencyHistoryReason {
    LegacyManagementRequest,
    ManagementWithoutBudget,
    ReviewedManagementRecord,
    ApiWithoutBudget,
}

/// A retained historical record or execution scope, not a counted charge.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CurrencyHistoryGap {
    pub project_id: Option<String>,
    pub source_id: String,
    pub reason: CurrencyHistoryReason,
    pub reference: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectCurrencyBudget {
    pub project_id: String,
    pub planning_setup_digest: Option<String>,
    pub revision_digest: String,
    pub ledger_head: String,
    #[ts(type = "number")]
    pub limit_usd_nanos: u64,
    #[ts(type = "number")]
    pub charged_usd_nanos: u64,
    #[ts(type = "number")]
    pub reserved_usd_nanos: u64,
    #[ts(type = "number | null")]
    pub available_usd_nanos: Option<u64>,
    pub approval_linked: bool,
    pub healthy: bool,
    pub blocked: bool,
    pub unknown_requests: u32,
    pub unpriced_requests: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub management_counts: Option<ManagementCountSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub api_counts: Option<ApiCountSummary>,
}

/// Read-only API count liabilities, separate from generation and management fees.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApiCountSummary {
    pub pending: u32,
    pub counted: u32,
    pub unknown: u32,
    pub counted_without_generation: u32,
    #[ts(type = "number")]
    pub reserved_usd_nanos: u64,
}

/// Read-only aggregate of durable count attempts; never permission to resume.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManagementCountSummary {
    pub pending: u32,
    pub counted: u32,
    pub unknown: u32,
    pub awaiting_generation: u32,
    #[ts(type = "number")]
    pub reserved_usd_nanos: u64,
    pub needs_review: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Rates {
    /// Integer USD microdollars per million tokens (e.g. $2.50 => 2_500_000).
    #[ts(type = "number")]
    pub input: u64,
    #[ts(type = "number")]
    pub output: u64,
    #[ts(type = "number | null")]
    pub cache_read: Option<u64>,
    #[ts(type = "number | null")]
    pub cache_write: Option<u64>,
}
impl Rates {
    fn validate(&self) -> Result<(), String> {
        require(
            [
                Some(self.input),
                Some(self.output),
                self.cache_read,
                self.cache_write,
            ]
            .into_iter()
            .flatten()
            .all(|n| n <= MAX_RATE),
            "USD quote rate out of range",
        )
    }
    pub fn charge(&self, usage: &Usage) -> Result<u64, String> {
        self.validate()?;
        usage.validate()?;
        let read = match (usage.cache_read, self.cache_read) {
            (0, _) => 0,
            (_, Some(n)) => n,
            _ => return Err("cache-read price is unknown".into()),
        };
        let write = match (usage.cache_write, self.cache_write) {
            (0, _) => 0,
            (_, Some(n)) => n,
            _ => return Err("cache-write price is unknown".into()),
        };
        // Sum before rounding; round upward once for this whole request.
        let numerator = u128::from(usage.input) * u128::from(self.input)
            + u128::from(usage.output) * u128::from(self.output)
            + u128::from(usage.cache_read) * u128::from(read)
            + u128::from(usage.cache_write) * u128::from(write);
        let nanos = numerator.div_ceil(1000);
        require(
            nanos <= u128::from(MAX_USD_NANOS),
            "USD charge out of range",
        )?;
        Ok(nanos as u64)
    }
    pub fn reserve(&self, ceiling: &TokenCeiling) -> Result<u64, String> {
        self.validate()?;
        ceiling.validate()?;
        require(
            !ceiling.cache_read_allowed || self.cache_read.is_some(),
            "possible cache-read usage requires an approved price",
        )?;
        require(
            !ceiling.cache_write_allowed || self.cache_write.is_some(),
            "possible cache-write usage requires an approved price",
        )?;
        // All input, including caches, fits this combined ceiling. Reserve at
        // the highest allowed input rate, never assume a cache discount.
        let input = [
            Some(self.input),
            self.cache_read.filter(|_| ceiling.cache_read_allowed),
            self.cache_write.filter(|_| ceiling.cache_write_allowed),
        ]
        .into_iter()
        .flatten()
        .max()
        .unwrap_or(0);
        Self {
            input,
            output: self.output,
            cache_read: None,
            cache_write: None,
        }
        .charge(&Usage {
            input: ceiling.total_input,
            output: ceiling.output,
            cache_read: 0,
            cache_write: 0,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PriceQuote {
    /// Explicit maximum liability for one input-count request. None is unknown,
    /// never a free request. This is a user-reviewed quote, not a supplier bill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub count_price: Option<ManagementCountPrice>,
    pub schema: u32,
    pub target: ApiTarget,
    pub requested_model: String,
    pub reported_models: Vec<String>,
    pub rates: Rates,
    pub valid_from: String,
    pub valid_until: String,
    /// User-reviewed provenance, not an automatically fetched price.
    pub source: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManagementCountPrice {
    pub schema: u32,
    /// Retained even after a successful count until billing is reconciled.
    #[ts(type = "number")]
    pub maximum_usd_nanos: u64,
    /// Upper bound applies to the entire count request, including its schema.
    pub maximum_body_bytes: u32,
    /// Evidence reviewed by the user; an empty or absent quote is not permission.
    pub source: String,
}
impl ManagementCountPrice {
    pub fn validate(&self) -> Result<(), String> {
        require(
            self.schema == 1
                && self.maximum_usd_nanos <= MAX_USD_NANOS
                && (1..=256 * 1024).contains(&self.maximum_body_bytes),
            "invalid management count price scope",
        )?;
        require(
            !self.source.trim().is_empty()
                && self.source.len() <= 1024
                && !self.source.chars().any(char::is_control),
            "count price provenance required",
        )
    }
}
/// Reviewable configuration, without a recursive reference to its plan digest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BudgetSettings {
    pub schema: u32,
    #[ts(type = "number")]
    pub limit_usd_nanos: u64,
    pub quotes: Vec<PriceQuote>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub scoped_quotes: Option<Vec<scoped::ScopedPriceQuote>>,
}
impl BudgetSettings {
    pub fn validate(&self) -> Result<(), String> {
        BudgetPolicy {
            planning: None,
            schema: self.schema,
            project_id: "validation".into(),
            revision_digest: "0".repeat(64),
            limit_usd_nanos: self.limit_usd_nanos,
            quotes: self.quotes.clone(),
            scoped_quotes: self.scoped_quotes.clone(),
        }
        .validate()
    }
    pub fn require_model(&self, target: &ApiTarget, requested_model: &str) -> Result<(), String> {
        self.validate()?;
        require(
            match self.schema {
                1 => self
                    .quotes
                    .iter()
                    .any(|q| &q.target == target && q.requested_model == requested_model),
                2 => self.scoped_quotes.as_ref().is_some_and(|quotes| {
                    quotes
                        .iter()
                        .any(|q| &q.target == target && q.requested_model == requested_model)
                }),
                _ => false,
            },
            "金额预算缺少此端点和模型的明确报价",
        )
    }
    pub fn bind(&self, project_id: &str, revision_digest: &str) -> Result<BudgetPolicy, String> {
        let policy = BudgetPolicy {
            planning: None,
            schema: self.schema,
            project_id: project_id.into(),
            revision_digest: revision_digest.into(),
            limit_usd_nanos: self.limit_usd_nanos,
            quotes: self.quotes.clone(),
            scoped_quotes: self.scoped_quotes.clone(),
        };
        policy.validate()?;
        Ok(policy)
    }
    pub fn bind_planning(
        &self,
        project: &str,
        setup_digest: &str,
        chief_model: &str,
    ) -> Result<BudgetPolicy, String> {
        let planning = PlanningBudgetScope {
            setup_digest: setup_digest.into(),
            chief_model: chief_model.into(),
        };
        let policy = BudgetPolicy {
            schema: self.schema,
            project_id: project.into(),
            revision_digest: planning.scope_digest(project)?,
            planning: Some(planning),
            limit_usd_nanos: self.limit_usd_nanos,
            quotes: self.quotes.clone(),
            scoped_quotes: self.scoped_quotes.clone(),
        };
        policy.validate()?;
        Ok(policy)
    }
}
fn model(s: &str) -> Result<(), String> {
    require(
        !s.is_empty()
            && s.len() <= 200
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b)),
        "invalid priced model",
    )
}
impl PriceQuote {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(count) = &self.count_price {
            count.validate()?;
            require(
                matches!(self.target, ApiTarget::OpenAi {}),
                "count price supports official OpenAI management only",
            )?;
        }
        require(self.schema == 1, "unsupported price quote")?;
        self.target.url()?;
        model(&self.requested_model)?;
        let mut seen = BTreeSet::new();
        require(
            !self.reported_models.is_empty() && self.reported_models.len() <= 16,
            "explicit reported model list required",
        )?;
        for m in &self.reported_models {
            model(m)?;
            require(seen.insert(m), "duplicate priced model")?;
        }
        self.rates.validate()?;
        timestamp(&self.valid_from)?;
        timestamp(&self.valid_until)?;
        require(
            chrono::DateTime::parse_from_rfc3339(&self.valid_from).map_err(|e| e.to_string())?
                < chrono::DateTime::parse_from_rfc3339(&self.valid_until)
                    .map_err(|e| e.to_string())?,
            "invalid price validity interval",
        )?;
        require(
            !self.source.trim().is_empty()
                && self.source.len() <= 1024
                && !self.source.chars().any(char::is_control),
            "price provenance required",
        )
    }
    pub fn valid_at(&self, at: &str) -> Result<(), String> {
        self.validate()?;
        let at = chrono::DateTime::parse_from_rfc3339(at)
            .map_err(|_| "invalid quote observation time")?;
        let from =
            chrono::DateTime::parse_from_rfc3339(&self.valid_from).map_err(|e| e.to_string())?;
        let until =
            chrono::DateTime::parse_from_rfc3339(&self.valid_until).map_err(|e| e.to_string())?;
        require(
            from <= at && at < until,
            "price quote not current; review a new policy",
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TokenCeiling {
    pub total_input: u64,
    pub output: u64,
    pub cache_read_allowed: bool,
    pub cache_write_allowed: bool,
}
impl TokenCeiling {
    pub fn validate(&self) -> Result<(), String> {
        require(
            self.total_input <= MAX_TOKENS && (1..=MAX_TOKENS).contains(&self.output),
            "invalid billable token ceiling",
        )
    }
    pub fn contains(&self, usage: &Usage) -> Result<bool, String> {
        self.validate()?;
        usage.validate()?;
        Ok(
            usage.input + usage.cache_read + usage.cache_write <= self.total_input
                && usage.output <= self.output
                && (self.cache_read_allowed || usage.cache_read == 0)
                && (self.cache_write_allowed || usage.cache_write == 0),
        )
    }
}
/// Complete nonoverlapping counters. Missing usage must not become this type
/// by filling unknown counters with zero. Adapters must normalize their own
/// inclusive/separate cache semantics before passing an observation here.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Usage {
    #[ts(type = "number")]
    pub input: u64,
    #[ts(type = "number")]
    pub output: u64,
    #[ts(type = "number")]
    pub cache_read: u64,
    #[ts(type = "number")]
    pub cache_write: u64,
}
impl Usage {
    pub fn validate(&self) -> Result<(), String> {
        require(
            [self.input, self.output, self.cache_read, self.cache_write]
                .into_iter()
                .all(|v| v <= MAX_TOKENS),
            "billable usage out of range",
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BudgetPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planning: Option<PlanningBudgetScope>,
    pub schema: u32,
    pub project_id: String,
    pub revision_digest: String,
    pub limit_usd_nanos: u64,
    pub quotes: Vec<PriceQuote>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scoped_quotes: Option<Vec<scoped::ScopedPriceQuote>>,
}
impl BudgetPolicy {
    pub fn validate(&self) -> Result<(), String> {
        require(matches!(self.schema, 1 | 2), "unsupported currency budget")?;
        identifier(&self.project_id)?;
        sha256(&self.revision_digest)?;
        if let Some(planning) = &self.planning {
            require(
                self.revision_digest == planning.scope_digest(&self.project_id)?,
                "planning permission scope differs from saved configuration",
            )?;
            require(
                match self.schema {
                    1 => self.quotes.iter().any(|q| {
                        matches!(q.target, ApiTarget::OpenAi {})
                            && q.requested_model == planning.chief_model
                    }),
                    2 => self.scoped_quotes.as_ref().is_some_and(|quotes| {
                        quotes.iter().any(|q| {
                            matches!(q.target, ApiTarget::OpenAi {})
                                && q.requested_model == planning.chief_model
                        })
                    }),
                    _ => false,
                },
                "planning permission requires an OpenAI chief quote",
            )?;
        }
        require(
            (1..=MAX_USD_NANOS).contains(&self.limit_usd_nanos),
            "invalid USD budget limit",
        )?;
        let mut seen = BTreeSet::new();
        if self.schema == 1 {
            require(
                self.scoped_quotes.is_none(),
                "legacy currency budget cannot contain scoped quotes",
            )?;
            require(
                !self.quotes.is_empty() && self.quotes.len() <= 64,
                "budget quotes required",
            )?;
            for q in &self.quotes {
                q.validate()?;
                require(seen.insert(digest(q)?), "duplicate budget quote")?;
            }
        } else {
            require(
                self.quotes.is_empty(),
                "scoped currency budget cannot contain legacy quotes",
            )?;
            let quotes = self
                .scoped_quotes
                .as_ref()
                .ok_or("scoped budget quotes required")?;
            require(
                !quotes.is_empty() && quotes.len() <= 64,
                "scoped budget quotes required and bounded",
            )?;
            for q in quotes {
                q.validate()?;
                require(seen.insert(digest(q)?), "duplicate scoped budget quote")?;
            }
        }
        Ok(())
    }
    pub fn quote(&self, hash: &str) -> Result<&PriceQuote, String> {
        require(
            self.schema == 1 && self.scoped_quotes.is_none(),
            "legacy quote lookup requires a legacy budget",
        )?;
        sha256(hash)?;
        self.quotes
            .iter()
            .find(|q| digest(q).is_ok_and(|v| v == hash))
            .ok_or("quote not in approved budget".into())
    }
    pub fn scoped_quote(&self, hash: &str) -> Result<&scoped::ScopedPriceQuote, String> {
        require(
            self.schema == 2 && self.quotes.is_empty(),
            "scoped quote lookup requires a scoped budget",
        )?;
        sha256(hash)?;
        self.scoped_quotes
            .as_ref()
            .ok_or("scoped budget quotes required")?
            .iter()
            .find(|q| digest(q).is_ok_and(|v| v == hash))
            .ok_or("scoped quote not in approved budget".into())
    }
}
/// A configuration-bound planning permission, never an execution-plan approval.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanningBudgetScope {
    pub setup_digest: String,
    pub chief_model: String,
}
impl PlanningBudgetScope {
    pub fn scope_digest(&self, project: &str) -> Result<String, String> {
        identifier(project)?;
        sha256(&self.setup_digest)?;
        model(&self.chief_model)?;
        digest(&(
            "planning-budget-permission-v1",
            project,
            &self.setup_digest,
            &self.chief_model,
        ))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChargeRole {
    Chief,
    Deputy,
    Worker,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Reservation {
    /// Explicit scoped request terms, not proof of counting or send authority.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pricing: Option<scoped::PricingRequest>,
    pub request_id: String,
    pub operation_id: String,
    pub role: ChargeRole,
    pub quote_digest: String,
    pub request_sha256: String,
    pub ceiling: TokenCeiling,
    /// Digest of the provider-specific preflight proving this ceiling. The
    /// caller must validate that proof; an arbitrary digest grants no send.
    pub ceiling_evidence_sha256: String,
}
impl Reservation {
    pub fn validate(&self) -> Result<(), String> {
        identifier(&self.request_id)?;
        identifier(&self.operation_id)?;
        for h in [
            &self.quote_digest,
            &self.request_sha256,
            &self.ceiling_evidence_sha256,
        ] {
            sha256(h)?;
        }
        self.ceiling.validate()?;
        if let Some(pricing) = &self.pricing {
            pricing.validate()?;
            require(
                pricing.input.through == self.ceiling.total_input
                    && pricing.output_ceiling == self.ceiling.output
                    && pricing.cache_read_allowed == self.ceiling.cache_read_allowed
                    && pricing.cache_write_allowed == self.ceiling.cache_write_allowed,
                "scoped pricing request differs from reserved token ceiling",
            )?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Settlement {
    /// Observed billing service; missing remains unknown and is never defaulted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual_service: Option<String>,
    pub request_id: String,
    pub response_id: String,
    pub request_sha256: String,
    pub actual_model: String,
    pub usage: Usage,
    pub evidence_sha256: String,
}
impl Settlement {
    pub fn validate(&self) -> Result<(), String> {
        identifier(&self.request_id)?;
        require(
            !self.response_id.is_empty()
                && self.response_id.len() <= 256
                && !self.response_id.chars().any(char::is_control),
            "invalid billing response id",
        )?;
        sha256(&self.request_sha256)?;
        sha256(&self.evidence_sha256)?;
        model(&self.actual_model)?;
        if let Some(service) = &self.actual_service {
            scoped::service(service)?;
        }
        self.usage.validate()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_rounding_and_cache_rates_do_not_under_reserve() {
        let rates = Rates {
            input: 2_500_000,
            output: 10_000_000,
            cache_read: Some(250_000),
            cache_write: Some(3_125_000),
        };
        let usage = Usage {
            input: 100,
            output: 10,
            cache_read: 200,
            cache_write: 20,
        };
        assert_eq!(rates.charge(&usage).unwrap(), 462_500);
        assert_eq!(
            rates
                .reserve(&TokenCeiling {
                    total_input: 320,
                    output: 10,
                    cache_read_allowed: true,
                    cache_write_allowed: true,
                })
                .unwrap(),
            1_100_000
        );
        let fractional = Rates {
            input: 1,
            output: 1,
            cache_read: Some(1),
            cache_write: Some(1),
        };
        assert_eq!(
            fractional
                .charge(&Usage {
                    input: 1,
                    output: 1,
                    cache_read: 1,
                    cache_write: 1
                })
                .unwrap(),
            1
        );
        assert!(Rates {
            cache_write: None,
            ..rates
        }
        .charge(&usage)
        .is_err());
    }
    #[test]
    fn invalid_or_overflowing_values_are_not_zero_cost() {
        let rates = Rates {
            input: MAX_RATE,
            output: MAX_RATE,
            cache_read: None,
            cache_write: None,
        };
        assert!(rates
            .charge(&Usage {
                input: MAX_TOKENS,
                output: MAX_TOKENS,
                cache_read: 0,
                cache_write: 0
            })
            .is_err());
        assert!(rates
            .reserve(&TokenCeiling {
                total_input: u64::MAX,
                output: 1,
                cache_read_allowed: false,
                cache_write_allowed: false,
            })
            .is_err());
        assert!(!TokenCeiling {
            total_input: 3,
            output: 1,
            cache_read_allowed: true,
            cache_write_allowed: false,
        }
        .contains(&Usage {
            input: 2,
            output: 1,
            cache_read: 2,
            cache_write: 0
        })
        .unwrap());
        assert!(serde_json::from_str::<Usage>(r#"{"input":0,"output":0,"cacheRead":0}"#).is_err());
    }
}
