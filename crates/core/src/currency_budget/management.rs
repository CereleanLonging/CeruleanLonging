//! Bind management spending to the original purpose/configuration and journal.
//! Host approval reconciliation and a trusted ceiling are still prerequisites.
use super::*;
use crate::management_execution::{DecisionIntent, DecisionJournal};
use cerulean_adapters::openai::{ManagementRequestFingerprint, PreparedDecision};
use cerulean_domain::{
    api_worker::ApiTarget, credentials::CredentialProvider, execution::ExecutionSnapshot,
    management::Purpose, project_setup::SavedProjectSetup,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManagementChargeIdentity<R = ManagementRequestFingerprint> {
    pub project_id: String,
    pub operation_id: String,
    pub intent_digest: String,
    pub setup_digest: String,
    pub revision_digest: Option<String>,
    pub policy_digest: String,
    pub request: R,
}
impl ManagementChargeIdentity {
    pub(super) fn from_original(
        intent: &DecisionIntent,
        request: &ManagementRequestFingerprint,
        policy: &BudgetPolicy,
    ) -> Result<Self, String> {
        intent.validate()?;
        request.validate_for(intent.purpose, &intent.model, &intent.state_digest)?;
        Self::from_validated(intent, request, policy)
    }
}
impl
    ManagementChargeIdentity<cerulean_adapters::openai::scoped_pricing::ScopedDecisionFingerprint>
{
    pub(super) fn from_scoped_original(
        intent: &DecisionIntent,
        request: &cerulean_adapters::openai::scoped_pricing::ScopedDecisionFingerprint,
        policy: &BudgetPolicy,
    ) -> Result<Self, String> {
        intent.validate()?;
        request.validate()?;
        require(
            request.purpose == intent.purpose
                && request.requested_model == intent.model
                && request.state_digest == intent.state_digest,
            "scoped count differs from original management scope",
        )?;
        Self::from_validated(intent, request, policy)
    }
}
impl<R: Clone> ManagementChargeIdentity<R> {
    // Share original role/configuration/approval validation without inventing a
    // legacy wire fingerprint for a different request protocol.
    fn from_validated(
        intent: &DecisionIntent,
        request: &R,
        policy: &BudgetPolicy,
    ) -> Result<Self, String> {
        let saved: SavedProjectSetup =
            serde_json::from_value(intent.context["executionSetup"].clone())
                .map_err(|_| "management billing lacks original saved configuration")?;
        crate::project_setup::validate(&saved)?;
        require(
            saved.setup.project_id == intent.project_id && intent.project_id == policy.project_id,
            "management billing project mismatch",
        )?;
        let configured = match intent.purpose {
            Purpose::Plan | Purpose::Refine | Purpose::Accept => &saved.setup.chief_model,
            _ => &saved.setup.deputy_model,
        };
        require(
            configured == &intent.model,
            "management billing model differs from original role configuration",
        )?;
        let revision_digest = if intent.purpose == Purpose::Plan {
            let settings = saved
                .setup
                .policy
                .currency_budget
                .as_ref()
                .ok_or("original planning configuration has no money budget")?;
            require(
                &settings.bind_planning(
                    &intent.project_id,
                    &saved.digest,
                    &saved.setup.chief_model,
                )? == policy,
                "management planning request differs from original budget permission",
            )?;
            None
        } else {
            let workflow: ExecutionSnapshot =
                serde_json::from_value(intent.context["workflow"].clone())
                    .map_err(|_| "management billing lacks original execution context")?;
            let revision = workflow
                .revision
                .as_ref()
                .ok_or("management billing revision missing")?;
            revision.validate()?;
            let approval = workflow
                .approval
                .as_ref()
                .ok_or("management billing requires an approved execution plan")?;
            approval.validate(revision, &approval.user_id, revision.policy.version)?;
            let expected = revision.approval_digest()?;
            let settings = revision
                .policy
                .currency_budget
                .as_ref()
                .ok_or("approved plan has no money budget")?;
            require(
                workflow.project.id == intent.project_id
                    && revision.project.id == intent.project_id
                    && workflow.state.approved
                    && workflow.revision_digest.as_ref() == Some(&expected)
                    && &settings.bind(&intent.project_id, &expected)? == policy,
                "management execution request differs from original approved plan",
            )?;
            Some(expected)
        };
        Ok(Self {
            project_id: intent.project_id.clone(),
            operation_id: intent.operation_id.clone(),
            intent_digest: digest(intent)?,
            setup_digest: saved.digest,
            revision_digest,
            policy_digest: digest(policy)?,
            request: request.clone(),
        })
    }
}

pub(super) fn validate_binding(
    policy: &BudgetPolicy,
    r: &Reservation,
    id: &ManagementChargeIdentity,
) -> Result<(), String> {
    identifier(&id.project_id)?;
    identifier(&id.operation_id)?;
    sha256(&id.intent_digest)?;
    sha256(&id.setup_digest)?;
    id.request.validate_for(
        id.request.purpose,
        &id.request.requested_model,
        &id.request.state_digest,
    )?;
    let quote = policy.quote(&r.quote_digest)?;
    let expected_role = match id.request.purpose {
        Purpose::Plan | Purpose::Refine | Purpose::Accept => ChargeRole::Chief,
        _ => ChargeRole::Deputy,
    };
    require(
        id.project_id == policy.project_id
            && id.policy_digest == digest(policy)?
            && r.role == expected_role
            && r.operation_id == id.operation_id
            && r.request_id == id.operation_id
            && r.request_sha256 == id.request.body_sha256
            && quote.target == ApiTarget::OpenAi {}
            && quote.requested_model == id.request.requested_model
            && u64::from(id.request.max_output_tokens) <= r.ceiling.output,
        "management request differs from its currency reservation",
    )?;
    if let Some(planning) = &policy.planning {
        require(
            id.request.purpose == Purpose::Plan
                && id.revision_digest.is_none()
                && id.setup_digest == planning.setup_digest
                && id.request.requested_model == planning.chief_model,
            "planning permission only covers its original chief planning request",
        )
    } else {
        require(
            id.request.purpose != Purpose::Plan
                && id.revision_digest.as_ref() == Some(&policy.revision_digest),
            "execution approval cannot pay for an unapproved planning scope",
        )
    }
}

impl BudgetLedger {
    /// Bridge for a live, opaque provider count. The host must first reconcile
    /// original user approval and persist the count receipt. This method grants
    /// no send and does not close the reserve -> intent crash window.
    pub fn reserve_counted_management(
        &mut self,
        intent: &DecisionIntent,
        counted: &cerulean_adapters::openai::CountedDecision,
        quote_digest: &str,
    ) -> Result<u64, String> {
        let ceiling = counted.ceiling()?;
        let reservation = Reservation {
            pricing: None,
            request_id: intent.operation_id.clone(),
            operation_id: intent.operation_id.clone(),
            role: match intent.purpose {
                Purpose::Plan | Purpose::Refine | Purpose::Accept => ChargeRole::Chief,
                _ => ChargeRole::Deputy,
            },
            quote_digest: quote_digest.into(),
            request_sha256: counted.prepared().fingerprint().body_sha256.clone(),
            ceiling,
            ceiling_evidence_sha256: digest(counted.evidence())?,
        };
        self.reserve_management(intent, counted.prepared(), reservation)
    }
    /// Only reserves. The host must verify original user authorization and the
    /// real preflight ceiling before durably recording the intent and sending.
    pub fn reserve_management(
        &mut self,
        intent: &DecisionIntent,
        prepared: &PreparedDecision,
        reservation: Reservation,
    ) -> Result<u64, String> {
        require(
            PreparedDecision::new(
                intent.purpose,
                &intent.model,
                &intent.state_digest,
                &intent.context,
            )?
            .fingerprint()
                == prepared.fingerprint(),
            "management prepared body differs from original intent",
        )?;
        let identity =
            ManagementChargeIdentity::from_original(intent, prepared.fingerprint(), &self.policy)?;
        let id = reservation.request_id.clone();
        self.publish(Event::Reserve {
            reservation,
            api_identity: None,
            management_identity: Some(Box::new(identity)),
            scoped_preparation: None,
            scoped_api_preparation: None,
        })?;
        Ok(self.state.bookings[&id].reserved_usd_nanos)
    }
    /// Uses only the original journal, including after local discard/apply.
    /// It never treats missing evidence or an HTTP failure as a free request.
    pub(super) fn management_observation(
        &self,
        journal: &DecisionJournal,
        id: &str,
    ) -> Result<Option<Event>, String> {
        if self
            .state
            .bookings
            .get(id)
            .is_some_and(|b| b.scoped_management_preparation.is_some())
        {
            return self.scoped_management_observation(journal, id);
        }
        if let Some(event) = self.prepared_without_intent(journal, id)? {
            return Ok(event);
        }
        let evidence = self.checked_management(journal, id)?;
        let request = evidence
            .request
            .ok_or("original management wire identity missing")?;
        let prior = self.state.bookings[id].outcome.clone();
        if let (Some(observation), Some(transport)) = (evidence.observation, evidence.transport) {
            if let Ok(usage) = transport.usage.billable_usage(CredentialProvider::OpenAi) {
                let settlement = Settlement {
                    actual_service: None,
                    request_id: id.into(),
                    response_id: observation.response_id.clone(),
                    request_sha256: request.body_sha256.clone(),
                    actual_model: observation.actual_model.clone(),
                    usage,
                    evidence_sha256: evidence.head.into(),
                };
                if matches!(&prior,Outcome::Settled{settlement:s,..}|Outcome::Unpriced{settlement:s,..} if s==&settlement)
                {
                    return Ok(None);
                }
                return Ok(Some(Event::Settle { settlement }));
            }
        }
        if matches!(prior, Outcome::Unknown { .. }) {
            return Ok(None);
        }
        Ok(Some(Event::Unknown {
            request_id: id.into(),
            evidence_sha256: evidence.head.into(),
        }))
    }
    fn checked_management<'a>(
        &self,
        journal: &'a DecisionJournal,
        id: &str,
    ) -> Result<crate::management_execution::DecisionBillingEvidence<'a>, String> {
        let booking = self
            .state
            .bookings
            .get(id)
            .ok_or("management reservation missing")?;
        let policy = self
            .state
            .policies
            .get(&booking.policy_digest)
            .ok_or("original management currency policy missing")?;
        let evidence = journal.billing_evidence(id)?;
        let request = evidence
            .request
            .ok_or("legacy management request has no billable wire identity")?;
        let identity = ManagementChargeIdentity::from_original(evidence.intent, request, policy)?;
        require(
            booking.management_identity.as_ref() == Some(&identity),
            "management journal differs from original reserved request",
        )?;
        validate_binding(policy, &booking.reservation, &identity)?;
        Ok(evidence)
    }
}
