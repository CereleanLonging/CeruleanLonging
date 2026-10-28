//! Durable count attempt and fee ceiling, before any provider counting request.
//! A count response has no invoice: successful or uncertain counts retain their
//! entire approved fee ceiling. Neither outcome is proof of zero billing.
use super::*;
use crate::management_execution::DecisionIntent;
use cerulean_adapters::openai::scoped_pricing::ScopedDecisionFingerprint;
use cerulean_adapters::openai::{CountedDecision, ManagementTokenCount, PreparedDecision};

/// Untagged only to preserve the exact legacy encoding. Both request types deny
/// unknown fields and have distinct required fields; attempt schema binds kind.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CountIdentity {
    Legacy(ManagementChargeIdentity),
    Scoped(ManagementChargeIdentity<ScopedDecisionFingerprint>),
}
impl CountIdentity {
    pub(super) fn legacy(&self) -> Option<&ManagementChargeIdentity> {
        match self {
            Self::Legacy(v) => Some(v),
            Self::Scoped(_) => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManagementCountAttempt {
    pub schema: u32,
    pub intent: DecisionIntent,
    pub identity: CountIdentity,
    pub quote_digest: String,
    pub endpoint: String,
    pub body_sha256: String,
    pub body_bytes: usize,
    pub maximum_usd_nanos: u64,
}
impl ManagementCountAttempt {
    fn validate(&self, policy: &BudgetPolicy) -> Result<(), String> {
        if self.schema == 2 {
            return self.validate_scoped(policy);
        }
        require(self.schema == 1, "unsupported management count attempt")?;
        let prepared = PreparedDecision::new(
            self.intent.purpose,
            &self.intent.model,
            &self.intent.state_digest,
            &self.intent.context,
        )?;
        require(
            ManagementChargeIdentity::from_original(&self.intent, prepared.fingerprint(), policy)?
                == *self
                    .identity
                    .legacy()
                    .ok_or("legacy count identity required")?,
            "count attempt differs from its original management request",
        )?;
        let quote = policy.quote(&self.quote_digest)?;
        let price = quote
            .count_price
            .as_ref()
            .ok_or("explicit count fee ceiling required; unknown is not free")?;
        require(
            quote.target == cerulean_domain::api_worker::ApiTarget::OpenAi {}
                && quote.requested_model == self.intent.model,
            "count quote differs from original endpoint or model",
        )?;
        let identity = prepared.count_request_identity()?;
        require(
            identity
                == (
                    self.endpoint.clone(),
                    self.body_sha256.clone(),
                    self.body_bytes,
                )
                && self.body_bytes <= price.maximum_body_bytes as usize
                && self.maximum_usd_nanos == price.maximum_usd_nanos,
            "count request or fee ceiling differs from approved scope",
        )
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CountOutcome {
    Pending,
    Counted {
        receipt: Box<ManagementTokenCount>,
    },
    ScopedCounted {
        receipt: Box<cerulean_adapters::openai::scoped_pricing::ScopedTokenCount>,
    },
    Unknown {
        evidence_sha256: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CountBooking {
    pub attempt: ManagementCountAttempt,
    pub outcome: CountOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fee_resolution: Option<Box<crate::count_resolution::ManagementCountResolution>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retirement: Option<Box<crate::count_resolution::ManagementCountResolution>>,
}
pub(super) fn require_policy(root: &Path, policy: &BudgetPolicy) -> Result<(), String> {
    if policy.quotes.iter().any(|q| q.count_price.is_some()) {
        let data = root
            .parent()
            .filter(|p| p.file_name().is_some_and(|n| n == "currency-budgets"))
            .and_then(Path::parent)
            .ok_or("count fee policies require a guarded host directory")?;
        crate::currency_count_guard::require_protected(data, true)?;
    }
    Ok(())
}
impl BudgetState {
    pub(super) fn begin_count(
        &mut self,
        policy: &BudgetPolicy,
        attempt: &ManagementCountAttempt,
        at: &str,
    ) -> Result<(), String> {
        attempt.validate(policy)?;
        require(
            self.approval_links
                .contains_key(&self.current_policy_digest),
            "count attempt requires original linked user approval",
        )?;
        let id = &attempt.intent.operation_id;
        require(
            !self.api_counts.contains_key(id)
                && !self.management_counts.contains_key(id)
                && !self.bookings.contains_key(id)
                && !self
                    .historical_charges
                    .values()
                    .any(|h| h.scope.operation_id() == *id),
            "count operation already accounted; never recount",
        )?;
        if attempt.schema == 2 {
            policy.scoped_quote(&attempt.quote_digest)?.valid_at(at)?;
        } else {
            policy.quote(&attempt.quote_digest)?.valid_at(at)?;
        }
        require(
            attempt.maximum_usd_nanos <= self.available_usd_nanos(policy)?,
            "currency budget exhausted before count",
        )?;
        self.reserved_usd_nanos = self
            .reserved_usd_nanos
            .checked_add(attempt.maximum_usd_nanos)
            .ok_or("count fee overflow")?;
        self.management_counts.insert(
            id.clone(),
            CountBooking {
                attempt: attempt.clone(),
                outcome: CountOutcome::Pending,
                fee_resolution: None,
                retirement: None,
            },
        );
        Ok(())
    }
    pub(super) fn count_receipt(
        &mut self,
        id: &str,
        receipt: &ManagementTokenCount,
    ) -> Result<(), String> {
        let booking = self
            .management_counts
            .get_mut(id)
            .ok_or("original count attempt missing")?;
        require(
            matches!(booking.outcome, CountOutcome::Pending),
            "count outcome already recorded; do not replace",
        )?;
        receipt.validate(
            &booking
                .attempt
                .identity
                .legacy()
                .ok_or("legacy count identity required")?
                .request,
        )?;
        require(
            receipt.endpoint == booking.attempt.endpoint
                && receipt.count_body_sha256 == booking.attempt.body_sha256,
            "count receipt differs from original counting body",
        )?;
        booking.outcome = CountOutcome::Counted {
            receipt: Box::new(receipt.clone()),
        };
        Ok(())
    }
    pub(super) fn count_unknown(&mut self, id: &str, evidence: &str) -> Result<(), String> {
        sha256(evidence)?;
        let booking = self
            .management_counts
            .get_mut(id)
            .ok_or("original count attempt missing")?;
        require(
            matches!(booking.outcome, CountOutcome::Pending),
            "count outcome already recorded; do not replace",
        )?;
        booking.outcome = CountOutcome::Unknown {
            evidence_sha256: evidence.into(),
        };
        Ok(())
    }
    pub(super) fn counted_preparation(
        &self,
        policy: &BudgetPolicy,
        preparation: &ManagementPreparation,
    ) -> Result<(), String> {
        if policy
            .quote(&preparation.reservation.quote_digest)?
            .count_price
            .is_some()
        {
            let count = self
                .management_counts
                .get(&preparation.intent.operation_id)
                .ok_or("original paid count attempt missing")?;
            require(
                count.retirement.is_none(),
                "count continuation permanently retired",
            )?;
            require(
                count.attempt.intent == preparation.intent
                    && count.attempt.identity.legacy() == Some(&preparation.identity)
                    && count.attempt.quote_digest == preparation.reservation.quote_digest
                    && matches!(&count.outcome,CountOutcome::Counted{receipt} if receipt.as_ref()==&preparation.count),
                "prepared generation differs from original paid count",
            )?;
        }
        Ok(())
    }
    pub(super) fn counted_reservation(
        &self,
        policy: &BudgetPolicy,
        r: &Reservation,
        identity: Option<&ManagementChargeIdentity>,
    ) -> Result<(), String> {
        if r.role != ChargeRole::Worker && policy.quote(&r.quote_digest)?.count_price.is_some() {
            let count = self
                .management_counts
                .get(&r.operation_id)
                .ok_or("original paid count missing before management reserve")?;
            require(
                count.retirement.is_none(),
                "count continuation permanently retired",
            )?;
            let CountOutcome::Counted { receipt } = &count.outcome else {
                return Err("count outcome unknown; cannot generate".into());
            };
            require(
                identity.is_some()
                    && identity == count.attempt.identity.legacy()
                    && r.request_id == r.operation_id
                    && count.attempt.quote_digest == r.quote_digest
                    && r.request_sha256 == receipt.request.body_sha256
                    && r.ceiling.total_input == receipt.input_tokens
                    && r.ceiling.output == u64::from(receipt.request.max_output_tokens),
                "management reserve differs from paid count",
            )?;
        }
        Ok(())
    }
}
impl BudgetLedger {
    pub(super) fn require_count_guard(&self) -> Result<(), String> {
        let data = self
            .root
            .parent()
            .filter(|p| p.file_name().is_some_and(|n| n == "currency-budgets"))
            .and_then(Path::parent)
            .ok_or("count attempts require a guarded host directory")?;
        crate::currency_count_guard::require_protected(data, true)
    }
    /// Caller first authenticates the original approval and current Host scope.
    /// The complete attempt and fee ceiling are one immutable ledger entry.
    pub fn begin_management_count(
        &mut self,
        intent: &DecisionIntent,
        prepared: &PreparedDecision,
        quote: &str,
    ) -> Result<u64, String> {
        let original = PreparedDecision::new(
            intent.purpose,
            &intent.model,
            &intent.state_digest,
            &intent.context,
        )?;
        require(
            original.fingerprint() == prepared.fingerprint(),
            "count body differs from original intent",
        )?;
        let price = self
            .policy
            .quote(quote)?
            .count_price
            .as_ref()
            .ok_or("explicit count fee ceiling required; unknown is not free")?;
        let (endpoint, body_sha256, body_bytes) = prepared.count_request_identity()?;
        let attempt = ManagementCountAttempt {
            schema: 1,
            intent: intent.clone(),
            identity: CountIdentity::Legacy(ManagementChargeIdentity::from_original(
                intent,
                prepared.fingerprint(),
                &self.policy,
            )?),
            quote_digest: quote.into(),
            endpoint,
            body_sha256,
            body_bytes,
            maximum_usd_nanos: price.maximum_usd_nanos,
        };
        let maximum = attempt.maximum_usd_nanos;
        self.publish(Event::BeginManagementCount {
            attempt: Box::new(attempt),
        })?;
        Ok(maximum)
    }
    pub fn record_management_count(
        &mut self,
        operation: &str,
        counted: &CountedDecision,
    ) -> Result<(), String> {
        // Preserve a received audit receipt even when its live send authority
        // expired while queued. Freshness is checked before preparation/send.
        require(self.healthy(), "currency ledger requires reopen")?;
        let booking = self
            .state
            .management_counts
            .get(operation)
            .ok_or("original count attempt missing")?;
        if matches!(&booking.outcome,CountOutcome::Counted{receipt} if receipt.as_ref()==counted.evidence())
        {
            return Ok(());
        }
        self.publish(Event::RecordManagementCount {
            operation: operation.into(),
            receipt: counted.evidence().clone(),
        })
    }
    pub fn mark_management_count_unknown(
        &mut self,
        operation: &str,
        evidence: &str,
    ) -> Result<(), String> {
        require(self.healthy(), "currency ledger requires reopen")?;
        let booking = self
            .state
            .management_counts
            .get(operation)
            .ok_or("original count attempt missing")?;
        if matches!(&booking.outcome,CountOutcome::Unknown{evidence_sha256} if evidence_sha256==evidence)
        {
            return Ok(());
        }
        self.publish(Event::ManagementCountUnknown {
            operation: operation.into(),
            evidence_sha256: evidence.into(),
        })
    }
    /// A pending receipt after restart is uncertainty, never permission to send.
    /// No recorded count is reconstructed as an opaque live CountedDecision.
    pub(crate) fn recover_management_counts(&mut self) -> Result<(), String> {
        let pending: Vec<_> = self
            .state
            .management_counts
            .iter()
            .filter(|(_, b)| matches!(b.outcome, CountOutcome::Pending))
            .map(|(id, b)| Ok((id.clone(), digest(&("count-interrupted-v1", &b.attempt))?)))
            .collect::<Result<_, String>>()?;
        for (id, hash) in pending {
            self.mark_management_count_unknown(&id, &hash)?;
        }
        Ok(())
    }
}
