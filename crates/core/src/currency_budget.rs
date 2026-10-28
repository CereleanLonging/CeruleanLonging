//! Project-wide, single-writer USD reservations. No transport or tools run here.
//! The caller must authenticate approval and validate request/usage evidence.
//! Uncertain outcomes retain the entire reservation across process restarts.
use crate::{
    api_call_journal::{ApiCallJournal, CallIdentity, CallOutcome},
    host::ordinary,
};
use cerulean_domain::{
    currency_budget::*,
    execution::{digest, identifier, require, sha256, timestamp},
};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
const MAX_RECORD: usize = 128 * 1024;
const MAX_RECORDS: usize = 8192;
mod api_observations;
mod api_scoped;
pub use api_observations::{ApiFeePermit, ApiPreparationPermit};
pub use api_scoped::{ApiCountAttempt, ApiCountBooking, ApiCountOutcome, ScopedApiPreparation};
mod api_count_resolution;
mod api_generation_resolution;
mod count_resolution;
mod counts;
mod historical;
mod management;
mod observations;
mod preparation;
mod scoped;
mod scoped_counts;
mod scoped_preparation;
pub use cerulean_domain::historical_money::{HistoricalScope, HistoricalSource};
pub use counts::{CountBooking, CountIdentity, CountOutcome, ManagementCountAttempt};
pub use historical::HistoricalCharge;
pub use management::ManagementChargeIdentity;
pub use preparation::ManagementPreparation;
pub use scoped_preparation::ScopedManagementPreparation;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum Outcome {
    Reserved,
    Unknown {
        evidence_sha256: String,
    },
    Settled {
        settlement: Settlement,
        charged_usd_nanos: u64,
    },
    NotSent {
        evidence_sha256: String,
    },
    Unpriced {
        settlement: Settlement,
        reason: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Booking {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fee_resolution: Option<Box<crate::api_generation_resolution::ApiGenerationResolution>>,
    pub policy_digest: String,
    pub api_identity: Option<CallIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub management_identity: Option<ManagementChargeIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub management_preparation: Option<Box<ManagementPreparation>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scoped_management_preparation: Option<Box<ScopedManagementPreparation>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scoped_api_preparation: Option<Box<ScopedApiPreparation>>,
    pub reservation: Reservation,
    pub reserved_usd_nanos: u64,
    pub outcome: Outcome,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BudgetState {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub management_counts: BTreeMap<String, CountBooking>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub api_counts: BTreeMap<String, ApiCountBooking>,
    pub current_policy_digest: String,
    pub approval_links: BTreeMap<String, ApprovalLink>,
    /// Keep original quotes for late outcomes; a new approval never reprices
    /// requests already reserved under an earlier approval.
    pub policies: BTreeMap<String, BudgetPolicy>,
    pub charged_usd_nanos: u64,
    pub reserved_usd_nanos: u64,
    /// An unpriced or out-of-ceiling observation prevents further spending.
    pub blocked: bool,
    pub bookings: BTreeMap<String, Booking>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub historical_charges: BTreeMap<String, HistoricalCharge>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApprovalLink {
    pub previous_policy_digest: String,
    pub previous_ledger_head: String,
    pub approval_evidence_sha256: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PolicyAuthorization {
    pub previous_policy_digest: String,
    pub previous_ledger_head: String,
    pub policy: BudgetPolicy,
    /// Host-validated user approval evidence, not permission supplied by a model.
    pub approval_evidence_sha256: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
enum Event {
    ResolveApiGeneration {
        resolution: Box<crate::api_generation_resolution::ApiGenerationResolution>,
    },
    ResolveApiCount {
        resolution: Box<crate::api_count_resolution::ApiCountResolution>,
    },
    BeginApiCount {
        attempt: Box<ApiCountAttempt>,
    },
    RecordApiCount {
        request_id: String,
        receipt: Box<cerulean_adapters::api_generation::scoped::ScopedApiTokenCount>,
    },
    ApiCountUnknown {
        request_id: String,
        evidence_sha256: String,
    },
    ResolveManagementCount {
        resolution: Box<crate::count_resolution::ManagementCountResolution>,
    },
    BeginManagementCount {
        attempt: Box<ManagementCountAttempt>,
    },
    RecordManagementCount {
        operation: String,
        receipt: cerulean_adapters::openai::ManagementTokenCount,
    },
    RecordScopedManagementCount {
        operation: String,
        receipt: cerulean_adapters::openai::scoped_pricing::ScopedTokenCount,
    },
    ManagementCountUnknown {
        operation: String,
        evidence_sha256: String,
    },
    ReservePreparedManagement {
        preparation: Box<ManagementPreparation>,
    },
    Open {
        policy: BudgetPolicy,
    },
    BindInitialApproval {
        link: ApprovalLink,
    },
    Authorize {
        authorization: PolicyAuthorization,
    },
    Reserve {
        reservation: Reservation,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        api_identity: Option<Box<CallIdentity>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        management_identity: Option<Box<ManagementChargeIdentity>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scoped_preparation: Option<Box<ScopedManagementPreparation>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scoped_api_preparation: Option<Box<ScopedApiPreparation>>,
    },
    Unknown {
        request_id: String,
        evidence_sha256: String,
    },
    NotSent {
        request_id: String,
        evidence_sha256: String,
    },
    Settle {
        settlement: Settlement,
    },
    HistoricalCharge {
        charge: Box<HistoricalCharge>,
    },
}
impl BudgetState {
    pub fn available_usd_nanos(&self, policy: &BudgetPolicy) -> Result<u64, String> {
        require(
            digest(policy)? == self.current_policy_digest,
            "currency budget view uses an inactive approval",
        )?;
        require(!self.blocked, "currency budget requires reconciliation")?;
        policy
            .limit_usd_nanos
            .checked_sub(self.charged_usd_nanos)
            .and_then(|v| v.checked_sub(self.reserved_usd_nanos))
            .ok_or("currency budget accounting exceeds limit".into())
    }
    fn advance(
        &mut self,
        policy: &BudgetPolicy,
        event: &Event,
        at: &str,
        head: &str,
    ) -> Result<(), String> {
        match event {
            Event::ResolveApiGeneration { resolution } => {
                self.resolve_api_generation(policy, resolution, head)?
            }
            Event::ResolveApiCount { resolution } => {
                self.resolve_api_count(policy, resolution, head)?
            }
            Event::BeginApiCount { attempt } => self.begin_api_count(policy, attempt, at)?,
            Event::RecordApiCount {
                request_id,
                receipt,
            } => self.api_count_receipt(request_id, receipt)?,
            Event::ApiCountUnknown {
                request_id,
                evidence_sha256,
            } => self.api_count_unknown(request_id, evidence_sha256)?,
            Event::ResolveManagementCount { resolution } => {
                self.resolve_count(policy, resolution, head)?
            }
            Event::BeginManagementCount { attempt } => self.begin_count(policy, attempt, at)?,
            Event::RecordManagementCount { operation, receipt } => {
                self.count_receipt(operation, receipt)?
            }
            Event::RecordScopedManagementCount { operation, receipt } => {
                self.scoped_count_receipt(operation, receipt)?
            }
            Event::ManagementCountUnknown {
                operation,
                evidence_sha256,
            } => self.count_unknown(operation, evidence_sha256)?,
            Event::ReservePreparedManagement { preparation } => {
                preparation.validate(policy)?;
                self.counted_preparation(policy, preparation)?;
                self.advance(
                    policy,
                    &Event::Reserve {
                        reservation: preparation.reservation.clone(),
                        api_identity: None,
                        management_identity: Some(Box::new(preparation.identity.clone())),
                        scoped_preparation: None,
                        scoped_api_preparation: None,
                    },
                    at,
                    head,
                )?;
                self.bookings
                    .get_mut(&preparation.reservation.request_id)
                    .ok_or("prepared reservation missing")?
                    .management_preparation = Some(preparation.clone());
            }
            Event::Open { policy: actual } => {
                require(
                    actual == policy && self.policies.is_empty(),
                    "currency budget approval changed",
                )?;
                let hash = digest(actual)?;
                self.policies.insert(hash.clone(), actual.clone());
                self.current_policy_digest = hash;
            }
            Event::Authorize { authorization: a } => {
                a.policy.validate()?;
                sha256(&a.approval_evidence_sha256)?;
                sha256(&a.previous_policy_digest)?;
                sha256(&a.previous_ledger_head)?;
                require(
                    a.previous_policy_digest == digest(policy)?
                        && a.previous_policy_digest == self.current_policy_digest
                        && a.previous_ledger_head == head,
                    "currency authorization review is stale",
                )?;
                require(
                    a.policy.project_id == policy.project_id,
                    "currency authorization cannot change project",
                )?;
                require(
                    !self
                        .policies
                        .values()
                        .any(|p| p.revision_digest == a.policy.revision_digest),
                    "currency authorization requires a new approved plan revision",
                )?;
                let committed = self
                    .charged_usd_nanos
                    .checked_add(self.reserved_usd_nanos)
                    .ok_or("currency committed amount overflow")?;
                require(
                    a.policy.limit_usd_nanos >= committed,
                    "new currency limit is below existing charges and reservations",
                )?;
                let hash = digest(&a.policy)?;
                self.approval_links.insert(
                    hash.clone(),
                    ApprovalLink {
                        previous_policy_digest: a.previous_policy_digest.clone(),
                        previous_ledger_head: a.previous_ledger_head.clone(),
                        approval_evidence_sha256: a.approval_evidence_sha256.clone(),
                    },
                );
                self.policies.insert(hash.clone(), a.policy.clone());
                self.current_policy_digest = hash;
                // In particular, keep all request IDs, unknown reservations and
                // Unpriced blocks. A new plan is not a reconciliation action.
            }
            Event::BindInitialApproval { link } => {
                sha256(&link.approval_evidence_sha256)?;
                require(
                    self.policies.len() == 1
                        && self.bookings.is_empty()
                        && self.approval_links.is_empty()
                        && link.previous_policy_digest == self.current_policy_digest
                        && link.previous_ledger_head == head,
                    "initial currency approval link is stale or already bound",
                )?;
                self.approval_links
                    .insert(self.current_policy_digest.clone(), link.clone());
            }
            Event::Reserve {
                reservation: r,
                api_identity,
                management_identity,
                scoped_preparation,
                scoped_api_preparation,
            } => {
                require(
                    scoped_preparation.is_none() || scoped_api_preparation.is_none(),
                    "cannot mix management and API preparations",
                )?;
                if let Some(prepared) = scoped_api_preparation {
                    require(
                        management_identity.is_none()
                            && api_identity.as_deref() == Some(&prepared.attempt.call)
                            && &prepared.reservation == r,
                        "API preparation cannot mix identities",
                    )?;
                    self.api_counted_preparation(policy, prepared)?;
                } else {
                    require(
                        !self.api_counts.contains_key(&r.request_id)
                            && !(api_identity.is_some() && r.pricing.is_some()),
                        "scoped API reservation requires its original count and preparation",
                    )?;
                }
                if let Some(prepared) = scoped_preparation {
                    require(
                        api_identity.is_none()
                            && management_identity.is_none()
                            && &prepared.reservation == r,
                        "scoped preparation cannot mix wire identities",
                    )?;
                    self.scoped_counted_preparation(policy, prepared)?;
                } else {
                    self.counted_reservation(policy, r, management_identity.as_deref())?;
                }
                require(
                    api_identity.is_none() || management_identity.is_none(),
                    "request cannot bind API worker and management journals together",
                )?;
                if let Some(identity) = management_identity {
                    management::validate_binding(policy, r, identity)?;
                }
                r.validate()?;
                require(
                    !self
                        .historical_charges
                        .values()
                        .any(|h| h.scope.operation_id() == r.operation_id),
                    "historical scope was already accounted; cannot reserve it again",
                )?;
                if let Some(identity) = api_identity {
                    validate_api_binding(policy, r, identity)?;
                }
                require(
                    !self.bookings.contains_key(&r.request_id),
                    "currency request already reserved; never resend",
                )?;
                let (requested_model, target) = scoped::model_target(policy, r)?;
                if let Some(planning) = &policy.planning {
                    require(matches!(r.role, ChargeRole::Chief) && matches!(target, cerulean_domain::api_worker::ApiTarget::OpenAi {}) && requested_model == planning.chief_model, "planning budget permits only the configured chief; no worker or deputy execution")?;
                }
                require(
                    matches!(r.role, ChargeRole::Worker)
                        || matches!(target, cerulean_domain::api_worker::ApiTarget::OpenAi {}),
                    "management currency quote must target OpenAI",
                )?;
                let amount = scoped::reserve_amount(policy, r, at)?;
                require(
                    amount <= self.available_usd_nanos(policy)?,
                    "currency budget exhausted",
                )?;
                self.reserved_usd_nanos = self
                    .reserved_usd_nanos
                    .checked_add(amount)
                    .ok_or("currency reservation overflow")?;
                self.bookings.insert(
                    r.request_id.clone(),
                    Booking {
                        fee_resolution: None,
                        policy_digest: digest(policy)?,
                        api_identity: api_identity.as_deref().cloned(),
                        management_identity: management_identity.as_deref().cloned(),
                        management_preparation: None,
                        scoped_management_preparation: scoped_preparation.clone(),
                        scoped_api_preparation: scoped_api_preparation.clone(),
                        reservation: r.clone(),
                        reserved_usd_nanos: amount,
                        outcome: Outcome::Reserved,
                    },
                );
            }
            Event::Unknown {
                request_id,
                evidence_sha256,
            } => {
                sha256(evidence_sha256)?;
                let b = self
                    .bookings
                    .get_mut(request_id)
                    .ok_or("currency reservation missing")?;
                require(
                    matches!(b.outcome, Outcome::Reserved),
                    "currency unknown outcome out of order",
                )?;
                b.outcome = Outcome::Unknown {
                    evidence_sha256: evidence_sha256.clone(),
                };
            }
            Event::NotSent {
                request_id,
                evidence_sha256,
            } => {
                sha256(evidence_sha256)?;
                let b = self
                    .bookings
                    .get_mut(request_id)
                    .ok_or("currency reservation missing")?;
                require(
                    matches!(b.outcome, Outcome::Reserved),
                    "uncertainty cannot be released as not sent",
                )?;
                self.reserved_usd_nanos = self
                    .reserved_usd_nanos
                    .checked_sub(b.reserved_usd_nanos)
                    .ok_or("currency reservation underflow")?;
                b.outcome = Outcome::NotSent {
                    evidence_sha256: evidence_sha256.clone(),
                };
            }
            Event::HistoricalCharge { charge } => {
                self.advance_historical_charge(policy, charge, head)?;
            }
            Event::Settle { settlement: s } => {
                s.validate()?;
                let b = self
                    .bookings
                    .get(&s.request_id)
                    .ok_or("currency reservation missing")?;
                require(
                    b.fee_resolution.is_none()
                        && matches!(b.outcome, Outcome::Reserved | Outcome::Unknown { .. }),
                    "currency settlement already recorded",
                )?;
                require(
                    s.request_sha256 == b.reservation.request_sha256,
                    "currency response request mismatch",
                )?;
                // A response can only be charged once within its provider endpoint.
                let original_policy = self
                    .policies
                    .get(&b.policy_digest)
                    .ok_or("original currency approval missing")?;
                let (_, target) = scoped::model_target(original_policy, &b.reservation)?;
                for other in self.bookings.values() {
                    let prior = match &other.outcome {
                        Outcome::Settled { settlement, .. }
                        | Outcome::Unpriced { settlement, .. } => Some(settlement),
                        _ => None,
                    };
                    if prior.is_some_and(|p| p.response_id == s.response_id) {
                        let other_policy = self
                            .policies
                            .get(&other.policy_digest)
                            .ok_or("original currency approval missing")?;
                        require(
                            scoped::model_target(other_policy, &other.reservation)?.1 != target,
                            "billing response already attached to another request",
                        )?;
                    }
                }
                let priced = (|| {
                    let charged = scoped::charge_amount(original_policy, &b.reservation, s)?;
                    require(
                        charged <= b.reserved_usd_nanos,
                        "observed charge exceeds reservation",
                    )?;
                    Ok::<u64, String>(charged)
                })();
                let b = self
                    .bookings
                    .get_mut(&s.request_id)
                    .ok_or("currency reservation missing")?;
                b.outcome = match priced {
                    Ok(charged) => {
                        self.charged_usd_nanos = self
                            .charged_usd_nanos
                            .checked_add(charged)
                            .ok_or("currency charge overflow")?;
                        self.reserved_usd_nanos = self
                            .reserved_usd_nanos
                            .checked_sub(b.reserved_usd_nanos)
                            .ok_or("currency reservation underflow")?;
                        Outcome::Settled {
                            settlement: s.clone(),
                            charged_usd_nanos: charged,
                        }
                    }
                    Err(reason) => {
                        self.blocked = true;
                        Outcome::Unpriced {
                            settlement: s.clone(),
                            reason,
                        }
                    }
                };
            }
        }
        Ok(())
    }
}

fn validate_api_binding(
    policy: &BudgetPolicy,
    r: &Reservation,
    id: &CallIdentity,
) -> Result<(), String> {
    require(
        policy.planning.is_none(),
        "planning permission cannot authorize an API worker",
    )?;
    id.validate()?;
    let (requested_model, target) = scoped::model_target(policy, r)?;
    require(
        matches!(r.role, ChargeRole::Worker)
            && id.project_id == policy.project_id
            && id.revision_digest == policy.revision_digest
            && id.request_id == r.request_id
            && id.attempt_id == r.operation_id
            && id.fingerprint.body_sha256 == r.request_sha256
            && &id.fingerprint.target == target
            && id.fingerprint.requested_model == requested_model
            && u64::from(id.fingerprint.max_output_tokens) <= r.ceiling.output,
        "API currency request differs from approved reservation",
    )
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Body {
    schema: u32,
    sequence: usize,
    previous: String,
    policy_digest: String,
    at: String,
    event: Event,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    body: Body,
    hash: String,
}
pub struct BudgetLedger {
    root: PathBuf,
    _lock: File,
    policy: BudgetPolicy,
    policy_digest: String,
    count: usize,
    head: String,
    state: BudgetState,
    settlement_heads: BTreeMap<String, String>,
    management_head: String,
    poisoned: bool,
}
impl BudgetLedger {
    /// Reopen the same project ledger under its currently effective approval.
    /// Changing this argument does not reauthorize, reset or rewrite the ledger.
    pub fn open(root: &Path, policy: &BudgetPolicy) -> Result<Self, String> {
        Self::open_inner(root, policy, true, true)
    }
    /// Host recovery must never recreate a missing account or silently choose a new policy.
    pub(crate) fn open_existing(root: &Path, project: &str) -> Result<Self, String> {
        identifier(project)?;
        require(
            root.is_absolute()
                && !root
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir)),
            "currency ledger requires absolute ordinary path",
        )?;
        for parent in root.ancestors() {
            ordinary(parent, true)?;
        }
        let first = root.join("00000000.json");
        ordinary(&first, false)?;
        let mut bytes = Vec::new();
        File::open(&first)
            .map_err(|e| e.to_string())?
            .take((MAX_RECORD + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        require(
            bytes.len() <= MAX_RECORD,
            "currency initial record too large",
        )?;
        let record: Record =
            serde_json::from_slice(&bytes).map_err(|_| "currency initial record invalid")?;
        let Event::Open { policy } = record.body.event else {
            return Err("currency initial policy missing".into());
        };
        require(
            policy.project_id == project,
            "currency ledger belongs to another project",
        )?;
        // Every record, including the first, is read again under the writer lock.
        Self::open_inner(root, &policy, false, false)
    }
    fn open_inner(
        root: &Path,
        policy: &BudgetPolicy,
        create: bool,
        require_current: bool,
    ) -> Result<Self, String> {
        policy.validate()?;
        scoped::require_policy(root, policy)?;
        counts::require_policy(root, policy)?;
        require(
            root.is_absolute()
                && !root
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir)),
            "currency ledger requires absolute ordinary path",
        )?;
        for parent in root.ancestors() {
            match fs::symlink_metadata(parent) {
                Ok(_) => ordinary(parent, true)?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err("currency ledger path inaccessible".into()),
            }
        }
        if create {
            fs::create_dir_all(root).map_err(|_| "currency ledger directory unavailable")?;
        }
        ordinary(root, true)?;
        let lock_path = root.join("writer.lock");
        match fs::symlink_metadata(&lock_path) {
            Ok(_) => ordinary(&lock_path, false)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("currency ledger lock inaccessible".into()),
        }
        let lock = OpenOptions::new()
            .create(create)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)
            .map_err(|_| "currency ledger lock unavailable")?;
        require(
            lock.metadata().map_err(|e| e.to_string())?.len() == 0,
            "currency ledger lock has unexpected content",
        )?;
        lock.try_lock_exclusive()
            .map_err(|_| "currency ledger already owned")?;
        let mut files = fs::read_dir(root)
            .map_err(|e| e.to_string())?
            .take(MAX_RECORDS + 2)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        require(
            files.len() <= MAX_RECORDS + 1,
            "currency ledger has too many records",
        )?;
        files.sort_by_key(|f| f.file_name());
        let mut ledger = Self {
            root: root.into(),
            _lock: lock,
            policy: policy.clone(),
            policy_digest: digest(policy)?,
            count: 0,
            head: "genesis".into(),
            state: BudgetState::default(),
            settlement_heads: BTreeMap::new(),
            management_head: "genesis".into(),
            poisoned: false,
        };
        for f in files {
            if f.file_name() == "writer.lock" {
                continue;
            }
            ordinary(&f.path(), false)?;
            require(
                f.file_name() == format!("{:08}.json", ledger.count).as_str(),
                "currency ledger pending, missing or unexpected record",
            )?;
            let mut bytes = Vec::new();
            File::open(f.path())
                .map_err(|e| e.to_string())?
                .take((MAX_RECORD + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            require(
                bytes.len() <= MAX_RECORD,
                "currency ledger record too large",
            )?;
            let record: Record =
                serde_json::from_slice(&bytes).map_err(|_| "currency ledger record invalid")?;
            if ledger.count == 0 {
                if let Event::Open { policy: original } = &record.body.event {
                    original.validate()?;
                    require(
                        original.project_id == policy.project_id,
                        "currency ledger belongs to another project",
                    )?;
                    ledger.policy = original.clone();
                    ledger.policy_digest = digest(original)?;
                }
            }
            require(
                record.body.schema == 1
                    && record.body.sequence == ledger.count
                    && record.body.previous == ledger.head
                    && record.body.policy_digest == ledger.policy_digest
                    && digest(&record.body)? == record.hash,
                "currency ledger chain or approval mismatch",
            )?;
            timestamp(&record.body.at)?;
            scoped::require_event(root, &ledger.policy, &record.body.event)?;
            require(
                matches!(&record.body.event, Event::Open { .. }) == (ledger.count == 0),
                "currency ledger initialization order invalid",
            )?;
            if matches!(record.body.event, Event::ReservePreparedManagement { .. }) {
                ledger.require_preparation_guard()?;
            }
            if let Event::Authorize { authorization } = &record.body.event {
                counts::require_policy(root, &authorization.policy)?;
            }
            if matches!(
                record.body.event,
                Event::BeginManagementCount { .. }
                    | Event::RecordManagementCount { .. }
                    | Event::ManagementCountUnknown { .. }
            ) {
                counts::require_policy(root, &ledger.policy)?;
                ledger.require_count_guard()?;
            }
            if matches!(record.body.event, Event::ResolveManagementCount { .. }) {
                ledger.require_count_resolution_guard()?;
            }
            ledger.state.advance(
                &ledger.policy,
                &record.body.event,
                &record.body.at,
                &ledger.head,
            )?;
            if let Event::Authorize { authorization } = &record.body.event {
                ledger.policy = authorization.policy.clone();
                ledger.policy_digest = digest(&ledger.policy)?;
            }
            if let Event::Settle { settlement } = &record.body.event {
                ledger
                    .settlement_heads
                    .insert(settlement.request_id.clone(), record.hash.clone());
            }
            if !api_only_event(&record.body.event, &ledger.state) {
                ledger.management_head = record.hash.clone();
            }
            ledger.head = record.hash;
            ledger.count += 1;
        }
        if ledger.count == 0 {
            require(create, "currency account is missing; do not recreate")?;
            ledger.publish(Event::Open {
                policy: policy.clone(),
            })?;
        }
        require(
            !require_current || &ledger.policy == policy,
            "currency ledger current approval differs; no automatic authorization",
        )?;
        Ok(ledger)
    }
    pub fn policy(&self) -> &BudgetPolicy {
        &self.policy
    }
    /// The host must validate the actual user approval and journal linkage.
    /// This operation only advances the same ledger; it never resets spending.
    pub fn authorize(&mut self, authorization: PolicyAuthorization) -> Result<(), String> {
        self.publish(Event::Authorize { authorization })
    }
    pub(crate) fn check_authorization(
        &self,
        authorization: PolicyAuthorization,
    ) -> Result<(), String> {
        require(self.healthy(), "currency ledger requires reopen")?;
        scoped::require_history(&self.root, &self.state)?;
        scoped::require_policy(&self.root, &authorization.policy)?;
        require(
            self.count < MAX_RECORDS,
            "currency ledger cannot hold a new approval",
        )?;
        // Reserve the longest normal RFC3339 fractional form so the later user
        // event cannot commit an authorization too large for its ledger record.
        let at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, false);
        let event = Event::Authorize { authorization };
        self.state
            .clone()
            .advance(&self.policy, &event, &at, &self.head)?;
        encode_record(self.count, &self.head, &self.policy_digest, at, event)?;
        Ok(())
    }
    pub(crate) fn bind_initial_approval(&mut self, link: ApprovalLink) -> Result<(), String> {
        self.publish(Event::BindInitialApproval { link })
    }
    pub fn state(&self) -> &BudgetState {
        &self.state
    }
    pub fn healthy(&self) -> bool {
        !self.poisoned
    }
    pub fn head(&self) -> &str {
        &self.head
    }
    /// Last non-API event in the original ledger, derived again on replay.
    /// API charges can progress during a management count; current balance and
    /// blocked state are still rechecked before preparing its generation.
    pub(crate) fn management_head(&self) -> &str {
        &self.management_head
    }
    pub fn reserve(&mut self, reservation: Reservation) -> Result<u64, String> {
        let id = reservation.request_id.clone();
        self.publish(Event::Reserve {
            reservation,
            api_identity: None,
            management_identity: None,
            scoped_preparation: None,
            scoped_api_preparation: None,
        })?;
        Ok(self.state.bookings[&id].reserved_usd_nanos)
    }
    /// Bind the exact original API journal before its sole send. The caller
    /// still proves the input ceiling and authenticates the active plan.
    pub fn reserve_api(
        &mut self,
        journal: &ApiCallJournal,
        reservation: Reservation,
    ) -> Result<u64, String> {
        require(
            journal.healthy()
                && !journal.state().intent_recorded
                && journal.state().outcome.is_none(),
            "API currency reservation must precede the original send",
        )?;
        let id = reservation.request_id.clone();
        self.publish(Event::Reserve {
            reservation,
            api_identity: Some(Box::new(journal.identity().clone())),
            management_identity: None,
            scoped_preparation: None,
            scoped_api_preparation: None,
        })?;
        Ok(self.state.bookings[&id].reserved_usd_nanos)
    }
    /// Observe only the original durable call. Re-observing the exact same
    /// completed evidence is read-only and never grants permission to resend.
    pub fn observe_api(&mut self, journal: &ApiCallJournal) -> Result<Outcome, String> {
        if let Some(event) = self.api_observation(journal)? {
            self.publish(event)?;
        }
        Ok(self.state.bookings[&journal.identity().request_id]
            .outcome
            .clone())
    }
    fn api_observation(&self, journal: &ApiCallJournal) -> Result<Option<Event>, String> {
        if journal.state().scoped_request.is_some()
            || self
                .state
                .bookings
                .get(&journal.identity().request_id)
                .is_some_and(|b| b.scoped_api_preparation.is_some())
        {
            return self.scoped_api_observation(journal);
        }
        use cerulean_adapters::api_generation::ApiFailure;
        self.check_api(journal)?;
        let id = journal.identity();
        let prior = self.state.bookings[&id.request_id].outcome.clone();
        match &journal.state().outcome {
            Some(CallOutcome::Reply { observation }) => {
                if let Ok(usage) = observation
                    .usage
                    .billable_usage(id.fingerprint.target.provider())
                {
                    let settlement = Settlement {
                        actual_service: None,
                        request_id: id.request_id.clone(),
                        response_id: observation.response_id.clone(),
                        request_sha256: id.fingerprint.body_sha256.clone(),
                        actual_model: observation.actual_model.clone(),
                        usage,
                        evidence_sha256: journal.head().into(),
                    };
                    if matches!(&prior, Outcome::Settled{settlement: saved,..}|Outcome::Unpriced{settlement:saved,..} if saved == &settlement)
                    {
                        return Ok(None);
                    }
                    return Ok(Some(Event::Settle { settlement }));
                }
            }
            Some(CallOutcome::Failure {
                failure: ApiFailure::NotSent {},
            }) => {
                if matches!(&prior, Outcome::NotSent { evidence_sha256 } if evidence_sha256 == journal.head())
                {
                    return Ok(None);
                }
                return Ok(Some(Event::NotSent {
                    request_id: id.request_id.clone(),
                    evidence_sha256: journal.head().into(),
                }));
            }
            _ => {}
        }
        if matches!(prior, Outcome::Unknown { .. }) {
            return Ok(None);
        }
        Ok(Some(Event::Unknown {
            request_id: id.request_id.clone(),
            evidence_sha256: journal.head().into(),
        }))
    }
    pub(crate) fn check_api(&self, journal: &ApiCallJournal) -> Result<(), String> {
        if journal.state().scoped_request.is_some()
            || self
                .state
                .bookings
                .get(&journal.identity().request_id)
                .is_some_and(|b| b.scoped_api_preparation.is_some())
        {
            return self.check_scoped_api(journal);
        }
        require(
            journal.healthy() && journal.state().intent_recorded,
            "API currency observation lacks a healthy original intent",
        )?;
        let id = journal.identity();
        let booking = self
            .state
            .bookings
            .get(&id.request_id)
            .ok_or("API currency reservation missing")?;
        require(
            booking.api_identity.as_ref() == Some(id),
            "API currency journal identity changed",
        )?;
        let policy = self
            .state
            .policies
            .get(&booking.policy_digest)
            .ok_or("original currency approval missing")?;
        validate_api_binding(policy, &booking.reservation, id)
    }
    pub fn unknown(&mut self, request_id: &str, evidence_sha256: &str) -> Result<(), String> {
        identifier(request_id)?;
        self.publish(Event::Unknown {
            request_id: request_id.into(),
            evidence_sha256: evidence_sha256.into(),
        })
    }
    /// Caller must prove no transport send occurred; cancellation/HTTP errors
    /// or a local timeout are not proof and must use `unknown`.
    pub fn not_sent(&mut self, request_id: &str, evidence_sha256: &str) -> Result<(), String> {
        identifier(request_id)?;
        self.publish(Event::NotSent {
            request_id: request_id.into(),
            evidence_sha256: evidence_sha256.into(),
        })
    }
    /// A persisted Unpriced return is a blocking result, not a successful bill.
    pub fn settle(&mut self, settlement: Settlement) -> Result<Outcome, String> {
        let id = settlement.request_id.clone();
        self.publish(Event::Settle { settlement })?;
        Ok(self.state.bookings[&id].outcome.clone())
    }
    fn publish(&mut self, event: Event) -> Result<(), String> {
        self.publish_at(event, chrono::Utc::now().to_rfc3339())
    }
    fn publish_at(&mut self, event: Event, at: String) -> Result<(), String> {
        scoped::require_history(&self.root, &self.state)?;
        scoped::require_event(&self.root, &self.policy, &event)?;
        if matches!(event, Event::ResolveManagementCount { .. }) {
            self.require_count_resolution_guard()?;
        }
        counts::require_policy(&self.root, &self.policy)?;
        if let Event::Authorize { authorization } = &event {
            counts::require_policy(&self.root, &authorization.policy)?;
        }
        if matches!(
            event,
            Event::BeginManagementCount { .. }
                | Event::RecordManagementCount { .. }
                | Event::ManagementCountUnknown { .. }
        ) {
            self.require_count_guard()?;
        }
        if matches!(event, Event::ReservePreparedManagement { .. }) {
            self.require_preparation_guard()?;
        }
        require(
            !self.poisoned && self.count < MAX_RECORDS,
            "currency ledger unavailable",
        )?;
        let mut next = self.state.clone();
        next.advance(&self.policy, &event, &at, &self.head)?;
        let next_policy = match &event {
            Event::Authorize { authorization } => Some(authorization.policy.clone()),
            _ => None,
        };
        let next_policy_digest = next.current_policy_digest.clone();
        let settled_id = match &event {
            Event::Settle { settlement } => Some(settlement.request_id.clone()),
            _ => None,
        };
        let management_changed = !api_only_event(&event, &next);
        let (hash, bytes) = encode_record(self.count, &self.head, &self.policy_digest, at, event)?;
        self.poisoned = true;
        ordinary(&self.root, true)?;
        let pending = self.root.join(format!("{:08}.pending", self.count));
        let target = self.root.join(format!("{:08}.json", self.count));
        let mut f = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&pending)
            .map_err(|_| "currency pending record exists or unavailable")?;
        f.write_all(&bytes)
            .and_then(|_| f.sync_all())
            .map_err(|_| "currency record sync failed")?;
        drop(f);
        fs::hard_link(&pending, &target).map_err(|_| "currency immutable publication failed")?;
        fs::remove_file(&pending).map_err(|_| "currency publication incomplete")?;
        if let Some(id) = settled_id {
            self.settlement_heads.insert(id, hash.clone());
        }
        if management_changed {
            self.management_head = hash.clone();
        }
        self.head = hash;
        self.count += 1;
        self.state = next;
        if let Some(policy) = next_policy {
            self.policy = policy;
            self.policy_digest = next_policy_digest;
        }
        self.poisoned = false;
        Ok(())
    }
}

// Classification follows successful state validation; unknown/new event kinds
// remain management-affecting by default. This is no second ledger or balance.
fn api_only_event(event: &Event, state: &BudgetState) -> bool {
    match event {
        Event::BeginApiCount { .. }
        | Event::RecordApiCount { .. }
        | Event::ApiCountUnknown { .. } => true,
        Event::Reserve {
            api_identity: Some(_),
            management_identity: None,
            scoped_preparation: None,
            ..
        } => true,
        Event::Unknown { request_id, .. } | Event::NotSent { request_id, .. } => state
            .bookings
            .get(request_id)
            .is_some_and(|b| b.api_identity.is_some()),
        Event::Settle { settlement } => state
            .bookings
            .get(&settlement.request_id)
            .is_some_and(|b| b.api_identity.is_some()),
        _ => false,
    }
}

fn encode_record(
    sequence: usize,
    previous: &str,
    policy_digest: &str,
    at: String,
    event: Event,
) -> Result<(String, Vec<u8>), String> {
    let body = Body {
        schema: 1,
        sequence,
        previous: previous.into(),
        policy_digest: policy_digest.into(),
        at,
        event,
    };
    let hash = digest(&body)?;
    let bytes = serde_json::to_vec(&Record {
        body,
        hash: hash.clone(),
    })
    .map_err(|e| e.to_string())?;
    require(bytes.len() <= MAX_RECORD, "currency record too large")?;
    Ok((hash, bytes))
}

#[cfg(test)]
mod tests;

/// Tamper-test helper: keep the outer typed chain valid so recovery tests
/// exercise semantic evidence validation rather than failing on JSON key order.
#[cfg(test)]
pub(crate) fn fixture_rehash_record(value: serde_json::Value) -> Vec<u8> {
    let mut record: Record = serde_json::from_value(value).unwrap();
    record.hash = digest(&record.body).unwrap();
    serde_json::to_vec(&record).unwrap()
}
