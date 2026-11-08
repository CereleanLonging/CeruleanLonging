//! Immutable management request evidence. A recovered intent never authorizes resend.
use crate::host::ordinary;
use crate::scheduler::CommandContext;
use cerulean_adapters::openai::{
    DecisionObservation, DecisionResult, DecisionTransport, ManagementRequestFingerprint,
    OpenAiClient, PreparedDecision,
};
use cerulean_domain::{
    execution::{
        digest, identifier, require, sha256, timestamp, ExecutionEvidence, RegisteredValidator,
    },
    management::{DecisionEnvelope, ManagementDecision, Purpose},
    management_ingress::{ExternalChiefDecision, ManagementHandoff},
    workflow::WorkflowCommand,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::Digest;
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

// An external planning record contains both the intent and handoff context.
// Keep enough bounded space for two 256 KiB contexts plus journal metadata.
pub(crate) const MAX_BYTES: usize = 1024 * 1024;
const MAX_OPERATIONS: usize = 1000;
mod decision_source;
mod deputy;
mod diagnostics;
#[cfg(windows)]
mod efficiency;
mod gateway;
pub(crate) mod omp;
pub(crate) use decision_source::StoredDecision;
mod external;
#[path = "management_historical_source.rs"]
mod historical_source;
mod scoped;
pub use scoped::{ScopedFeeAck, ScopedIntentBinding, ScopedResponseRecord};
#[cfg(test)]
pub(crate) mod read_metrics {
    use std::cell::Cell;
    #[derive(Clone, Copy, Debug, Default)]
    pub(crate) struct Counts {
        pub full_reads: usize,
        pub record_bytes: usize,
        pub raw_bytes: usize,
    }
    thread_local! { static COUNTS: Cell<Counts> = Cell::new(Counts::default()); }
    pub(crate) fn take() -> Counts {
        COUNTS.with(|s| s.replace(Counts::default()))
    }
    pub(super) fn full() {
        COUNTS.with(|s| {
            let mut c = s.get();
            c.full_reads += 1;
            s.set(c);
        });
    }
    pub(super) fn record(bytes: usize) {
        COUNTS.with(|s| {
            let mut c = s.get();
            c.record_bytes += bytes;
            s.set(c);
        });
    }
    pub(super) fn raw(bytes: usize) {
        COUNTS.with(|s| {
            let mut c = s.get();
            c.raw_bytes += bytes;
            s.set(c);
        });
    }
}
type DecisionReview = (String, Option<String>, Option<String>, Option<String>);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DecisionIntent {
    pub operation_id: String,
    pub project_id: String,
    pub purpose: Purpose,
    pub model: String,
    pub state_digest: String,
    pub context: Value,
    pub created_at: String,
}
impl DecisionIntent {
    pub(crate) fn validate(&self) -> Result<(), String> {
        identifier(&self.operation_id)?;
        identifier(&self.project_id)?;
        timestamp(&self.created_at)?;
        require(
            digest(&self.context)? == self.state_digest,
            "management context digest mismatch",
        )?;
        OpenAiClient::validate_request(self.purpose, &self.model, &self.state_digest, &self.context)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Evidence {
    OmpIntent {
        intent: DecisionIntent,
        request: omp::OmpRequest,
    },
    OmpResponse {
        response: omp::OmpResponse,
    },
    DeputyIntent {
        intent: DecisionIntent,
        request: cerulean_adapters::deputy_management::DeputyRequest,
    },
    DeputyResponse {
        response: cerulean_adapters::deputy_management::DeputyResult,
    },
    ExternalIntent {
        intent: DecisionIntent,
        #[serde(rename = "handoff")]
        request: ManagementHandoff,
    },
    ExternalResponse {
        response: ExternalChiefDecision,
    },
    Intent {
        intent: DecisionIntent,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        request: Option<ManagementRequestFingerprint>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        scoped: Option<ScopedIntentBinding>,
    },
    Response {
        observation: DecisionObservation,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        transport: Option<DecisionTransport>,
    },
    ScopedResponse {
        response: Box<ScopedResponseRecord>,
    },
    Unknown,
    ApplyIntent {
        target: Box<ApplicationTarget>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            rename = "currencyAck"
        )]
        currency_ack: Option<ScopedFeeAck>,
    },
    Applied {
        workflow_hash: Option<String>,
    },
    UserResolution {
        resolution: UserResolution,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "action",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum UserResolution {
    Discard {
        reason: String,
    },
    Reply {
        answer: String,
    },
    AbandonApplication {
        reason: String,
        workflow_sequence: u32,
        workflow_hash: Option<String>,
    },
    AbandonStartup {
        reason: String,
        attempt_id: String,
        startup_hash: String,
    },
}
#[derive(Debug, Clone)]
pub(crate) struct ApplicationClosure {
    pub reason: String,
    pub workflow_sequence: u32,
    pub workflow_hash: Option<String>,
}
#[derive(Debug, Clone)]
pub(crate) struct StartupResolution {
    pub reason: String,
    pub attempt_id: String,
    pub startup_hash: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum ApplicationTarget {
    Wait,
    DesktopHook {
        #[serde(rename = "taskId")]
        task_id: String,
        #[serde(rename = "attemptId")]
        attempt_id: String,
        #[serde(rename = "reviewDigest")]
        review_digest: String,
        #[serde(rename = "suggestionDigest")]
        suggestion_digest: String,
    },
    Workflow {
        principal: String,
        command: WorkflowCommand,
        context: Box<CommandContext>,
    },
}
impl ApplicationTarget {
    fn validate(
        &self,
        intent: &DecisionIntent,
        observation: &DecisionEnvelope,
    ) -> Result<(), String> {
        use ManagementDecision as D;
        use WorkflowCommand as C;
        if let Self::DesktopHook {
            task_id,
            attempt_id,
            review_digest,
            suggestion_digest,
        } = self
        {
            let D::Advise {
                task_id: proposed,
                suggestion,
                ..
            } = &observation.decision
            else {
                return Err("桌面Hook需要副指挥建议".into());
            };
            identifier(attempt_id)?;
            sha256(review_digest)?;
            sha256(suggestion_digest)?;
            return require(
                intent.purpose == Purpose::Advise
                    && intent.context["supervision"]["version"] == 5
                    && intent.context["supervision"]["kind"] == "desktop"
                    && intent.context["supervision"]["taskId"] == *task_id
                    && intent.context["supervision"]["attemptId"] == *attempt_id
                    && intent.context["supervision"]["reviewDigest"] == *review_digest
                    && task_id == proposed
                    && *suggestion_digest == digest(suggestion)?,
                "桌面Hook投送与原建议不符",
            );
        }
        let Self::Workflow {
            principal,
            command,
            context,
        } = self
        else {
            if let D::Refine { document } = &observation.decision {
                crate::host::document_refinement::validate_quotes(intent, document)?;
            }
            return require(
                matches!(observation.decision, D::Wait { .. } | D::Refine { .. }),
                "only wait or document refinement can resolve without workflow command",
            );
        };
        let mut expected = CommandContext::default();
        match (&observation.decision, command) {
            (D::Plan { tasks, .. }, C::Propose { plan }) => {
                require(principal == "chief", "chief identity required")?;
                let Some(ExecutionEvidence::Revision { revision }) = &context.execution else {
                    return Err("bound plan revision required".into());
                };
                require(
                    revision.project.id == intent.project_id
                        && revision.workflow == *plan
                        && tasks.len() == plan.tasks.len()
                        && tasks.len() == revision.execution.len()
                        && revision.snapshot_policy.is_some(),
                    "proposal differs from management decision",
                )?;
                let catalog: BTreeMap<String, RegisteredValidator> =
                    serde_json::from_value(intent.context["validators"].clone())
                        .map_err(|_| "validator catalog missing")?;
                for proposed in tasks {
                    let task = plan
                        .tasks
                        .iter()
                        .find(|t| t.id == proposed.id)
                        .ok_or("proposed task missing")?;
                    let spec = revision.task(&proposed.id)?;
                    let validator = catalog
                        .get(&proposed.validator_id)
                        .ok_or("proposed validator is not registered")?;
                    let configured = if intent.context["executionSetup"].is_object() {
                        let saved: cerulean_domain::project_setup::SavedProjectSetup =
                            serde_json::from_value(intent.context["executionSetup"].clone())
                                .map_err(|_| "invalid saved execution setup")?;
                        saved.setup.resources_for(proposed.profile_id.as_deref())?
                            == task.resource_keys
                            && cerulean_domain::objective_checker::bind_execution(
                                &saved.setup,
                                proposed,
                                &serde_json::from_value(
                                    intent
                                        .context
                                        .get("objectiveRuntimes")
                                        .cloned()
                                        .unwrap_or_else(|| serde_json::json!({})),
                                )
                                .map_err(|_| "客观测试运行时上下文无效")?,
                            )? == *spec
                            && saved
                                .setup
                                .validator_for(&proposed.id, proposed.profile_id.as_deref())?
                                == proposed.validator_id
                    } else {
                        spec.checks
                            .iter()
                            .all(|c| c.tool.as_ref() == Some(&validator.tool))
                    };
                    require(
                        task.goal == proposed.goal
                            && spec.profile_id == proposed.profile_id
                            && task.dependencies == proposed.dependencies
                            && proposed.worker_id == format!("worker-{}", spec.adapter_id)
                            && !spec.checks.is_empty()
                            && configured,
                        "proposal changed task, worker or validator",
                    )?;
                }
                expected.execution = context.execution.clone();
            }
            (
                D::Assign {
                    task_id, worker_id, ..
                },
                C::Assign {
                    task,
                    worker,
                    attempt,
                },
            ) => {
                require(
                    principal == "deputy" && task == task_id && worker == worker_id,
                    "assignment differs from deputy decision",
                )?;
                let Some(ExecutionEvidence::Attempt { attempt: bound }) = &context.execution else {
                    return Err("trusted session binding required".into());
                };
                require(
                    bound.id == *attempt
                        && bound.task_id == *task
                        && bound.worker_id == *worker
                        && worker == &format!("worker-{}", bound.binding.adapter_id),
                    "assignment binding mismatch",
                )?;
                expected.execution = context.execution.clone();
            }
            (
                D::Accept {
                    task_id,
                    attempt_id,
                    ..
                },
                C::Accept { task, attempt },
            ) => {
                require(
                    principal == "chief" && task == task_id && attempt == attempt_id,
                    "acceptance differs from chief decision",
                )?;
            }
            (
                D::Advise {
                    task_id,
                    suggestion: proposed,
                    ..
                },
                C::ReturnForRework {
                    task,
                    attempt,
                    verification_operation,
                    report_digest,
                    suggestion,
                },
            ) => {
                let feedback = &intent.context["rework"];
                require(
                    intent.purpose == Purpose::Advise
                        && principal == "deputy"
                        && feedback["requiresPlanRevision"] == false
                        && task == task_id
                        && suggestion == proposed
                        && feedback["taskId"] == *task
                        && feedback["attemptId"] == *attempt
                        && feedback["verificationOperation"] == *verification_operation
                        && feedback["reportDigest"] == *report_digest,
                    "rework differs from original deputy decision or checker feedback",
                )?;
            }
            (
                D::Advise {
                    task_id,
                    suggestion: proposed,
                    ..
                },
                C::Advise { task, suggestion },
            ) => {
                require(
                    principal == "deputy" && task == task_id && suggestion == proposed,
                    "advice differs from deputy decision",
                )?;
            }
            _ => return Err("management decision cannot grant this workflow command".into()),
        }
        require(
            **context == expected,
            "unexpected management command authority or evidence",
        )
    }
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Body {
    version: u32,
    operation_id: String,
    previous: String,
    at: String,
    evidence: Evidence,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    body: Body,
    hash: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DecisionState {
    Running,
    AwaitingExternal,
    Unknown,
    Ready,
    Applying,
    Applied,
    Discarded,
    Abandoned,
    StartupClosed,
}
impl DecisionState {
    pub(crate) fn resolved(&self) -> bool {
        matches!(
            self,
            Self::Applied | Self::Discarded | Self::Abandoned | Self::StartupClosed
        )
    }
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionStatus {
    pub operation_id: String,
    pub project_id: String,
    pub purpose: Purpose,
    pub state: DecisionState,
    pub state_digest: String,
}
struct Operation {
    omp_request: Option<omp::OmpRequest>,
    omp_response: Option<omp::OmpResponse>,
    deputy_request: Option<cerulean_adapters::deputy_management::DeputyRequest>,
    deputy_response: Option<cerulean_adapters::deputy_management::DeputyResult>,
    external_request: Option<ManagementHandoff>,
    external_response: Option<ExternalChiefDecision>,
    intent: DecisionIntent,
    intent_hash: String,
    request: Option<ManagementRequestFingerprint>,
    transport: Option<DecisionTransport>,
    scoped_request: Option<ScopedIntentBinding>,
    scoped_response: Option<ScopedResponseRecord>,
    fee_ack: Option<ScopedFeeAck>,
    chain_hash: String,
    response_hash: Option<String>,
    observation: Option<DecisionObservation>,
    finished: bool,
    application: Option<ApplicationTarget>,
    applied: bool,
    workflow_hash: Option<String>,
    resolution: Option<UserResolution>,
}
impl Operation {
    fn envelope(&self) -> Option<&DecisionEnvelope> {
        self.external_response
            .as_ref()
            .map(|r| &r.envelope)
            .or_else(|| self.omp_response.as_ref().and_then(|r| r.envelope.as_ref()))
            .or_else(|| self.deputy_response.as_ref().and_then(|r| r.envelope()))
            .or_else(|| self.observation.as_ref().map(|o| &o.envelope))
    }
}
pub(crate) struct DecisionBillingEvidence<'a> {
    pub intent: &'a DecisionIntent,
    pub request: Option<&'a ManagementRequestFingerprint>,
    pub transport: Option<&'a DecisionTransport>,
    pub observation: Option<&'a DecisionObservation>,
    pub head: &'a str,
    pub scoped_request: Option<&'a ScopedIntentBinding>,
    pub scoped_response: Option<&'a ScopedResponseRecord>,
}
/// Owned by RuntimeHost under its existing single-writer lock. Not public IPC.
pub(crate) struct DecisionJournal {
    root: PathBuf,
    operations: BTreeMap<String, Operation>,
    poisoned: bool,
    pending_publications: Vec<PendingPublication>,
    pub(crate) reviews: Vec<cerulean_domain::ManagementRecordReview>,
}
struct PendingPublication {
    pending: PathBuf,
    target: PathBuf,
    bytes: Vec<u8>,
    published: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PublishPoint {
    TempCreated,
    PartialWritten,
    TempSynced,
    Published,
    PendingRemoved,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecoveryPoint {
    Revalidated,
    Synced,
    FinalNamePresent,
    PendingRemoved,
    Finished,
}
fn absent(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err("management publication target already exists or cannot be inspected".into()),
    }
}
fn read(path: &Path) -> Result<(Record, Vec<u8>), String> {
    ordinary(path, false)?;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    require(bytes.len() <= MAX_BYTES, "management record too large")?;
    #[cfg(test)]
    read_metrics::record(bytes.len());
    let record: Record =
        serde_json::from_slice(&bytes).map_err(|_| "invalid management evidence")?;
    require(
        matches!(record.body.version, 1 | 2) && digest(&record.body)? == record.hash,
        "management evidence hash mismatch",
    )?;
    identifier(&record.body.operation_id)?;
    timestamp(&record.body.at)?;
    Ok((record, bytes))
}
impl DecisionJournal {
    pub(crate) fn open(root: &Path) -> Result<Self, String> {
        Self::open_inner(root, true)
    }
    fn read_existing(root: &Path) -> Result<Self, String> {
        #[cfg(test)]
        read_metrics::full();
        Self::open_inner(root, false)
    }
    fn open_inner(root: &Path, create: bool) -> Result<Self, String> {
        let reviews = crate::management_review::load(root)?;
        let root = root.join("management-decisions");
        if !root.exists() {
            require(
                create,
                "original management directory missing; do not recreate",
            )?;
            fs::create_dir(&root).map_err(|e| e.to_string())?;
        }
        ordinary(&root, true)?;
        let mut journal = Self {
            root,
            operations: BTreeMap::new(),
            poisoned: false,
            pending_publications: vec![],
            reviews,
        };
        let files = fs::read_dir(&journal.root)
            .map_err(|e| e.to_string())?
            .take(MAX_OPERATIONS * 10 + 1)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        require(
            files.len() <= MAX_OPERATIONS * 10,
            "management journal too large",
        )?;
        // Normalize complete pending names in memory only. Neither one journal's
        // local validity nor a local receipt proves the linked workflow exists.
        let mut publications: BTreeMap<String, (Option<PathBuf>, Option<PathBuf>)> =
            BTreeMap::new();
        for f in files {
            let name = f
                .file_name()
                .into_string()
                .map_err(|_| "invalid management filename")?;
            ordinary(&f.path(), false)?;
            let relative = format!("management-decisions/{name}");
            if journal.reviews.iter().any(|r| r.relative_path == relative) {
                continue;
            }
            if let Some(stem) = name.strip_suffix(".json") {
                publications.entry(stem.into()).or_default().0 = Some(f.path());
            } else if let Some(stem) = name.strip_suffix(".pending") {
                let (operation, sequence) = crate::management_review::parts(&relative)?;
                let logical = format!("{operation}.{sequence}");
                if stem != logical {
                    require(
                        journal
                            .reviews
                            .iter()
                            .any(|r| r.operation_id == operation && r.sequence == sequence),
                        "管理重试缺少原残片复核",
                    )?;
                }
                require(
                    publications
                        .entry(logical)
                        .or_default()
                        .1
                        .replace(f.path())
                        .is_none(),
                    "同一管理阶段存在多个未复核临时记录",
                )?;
            } else {
                return Err("unknown management evidence filename".into());
            }
        }
        require(
            publications.len() <= MAX_OPERATIONS * 5,
            "management logical history too large",
        )?;
        for (stem, (published, pending)) in publications {
            let name = format!("{stem}.json");
            let chosen = published.as_ref().or(pending.as_ref()).unwrap();
            let (record, bytes) = read(chosen)?;
            if let Some(pending) = pending {
                if let Some(hash) = pending
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.strip_prefix(&format!("{stem}.")))
                {
                    require(hash == record.hash, "管理重试文件名摘要不符")?;
                }
                require(
                    read(&pending)?.1 == bytes,
                    "conflicting management publication bytes",
                )?;
                journal.pending_publications.push(PendingPublication {
                    pending,
                    target: journal.root.join(&name),
                    bytes,
                    published: published.is_some(),
                });
            }
            journal.accept_record(&name, record)?;
        }
        require(
            journal.operations.len() + journal.reviews.iter().filter(|r| r.sequence == 0).count()
                <= MAX_OPERATIONS,
            "management journal full",
        )?;
        for review in &journal.reviews {
            let op = journal.operations.get(&review.operation_id);
            match review.sequence {
                0 => require(op.is_none(), "已复核的半写管理意图编号禁止复用")?,
                1 => require(
                    op.is_some_and(|o| {
                        !o.finished && o.envelope().is_none() && o.application.is_none()
                    }),
                    "半写管理响应复核与后续结果冲突",
                )?,
                2 => require(
                    op.is_some_and(|o| o.application.is_none()),
                    "半写应用准备复核与后续应用冲突",
                )?,
                _ => {}
            }
        }
        let mut reserved = std::collections::BTreeSet::new();
        for op in journal.operations.values() {
            if let Some(ApplicationTarget::Workflow {
                command: WorkflowCommand::Assign { attempt, .. },
                ..
            }) = &op.application
            {
                require(
                    reserved.insert((&op.intent.project_id, attempt)),
                    "management attempts have conflicting reservations",
                )?;
            }
        }
        Ok(journal)
    }
    /// Uses the same typed record/chain validator as normal loading; never
    /// publishes records and never reads fields from the truncated candidate.
    pub(crate) fn review_prefix(
        root: &Path,
        operation: &str,
        sequence: u8,
    ) -> Result<(String, Option<String>), String> {
        identifier(operation)?;
        require(sequence <= 4, "invalid management review sequence")?;
        let dir = root.join("management-decisions");
        ordinary(&dir, true)?;
        let mut journal = Self {
            root: dir.clone(),
            operations: BTreeMap::new(),
            poisoned: false,
            pending_publications: vec![],
            reviews: vec![],
        };
        let mut bound = vec![];
        for n in 0..sequence {
            let name = format!("{operation}.{n}.json");
            let path = dir.join(&name);
            match fs::symlink_metadata(&path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.to_string()),
                Ok(_) => {}
            }
            let (record, raw) = read(&path)?;
            journal.accept_record(&name, record)?;
            bound.push((name, format!("{:x}", sha2::Sha256::digest(&raw)), raw.len()));
        }
        let op = journal.operations.get(operation);
        let valid = match sequence {
            0 => true,
            1 => op.is_some_and(|o| !o.finished && o.resolution.is_none()),
            2 => op.is_some_and(|o| {
                o.envelope().is_some() && o.application.is_none() && o.resolution.is_none()
            }),
            3 => {
                op.is_some_and(|o| o.application.is_some() && !o.applied && o.resolution.is_none())
            }
            4 => op.is_some_and(|o| {
                o.resolution.is_none()
                    && (o.applied == matches!(o.application, Some(ApplicationTarget::Wait)))
            }),
            _ => false,
        };
        require(valid, "管理复核缺少有效的前置记录链")?;
        Ok((
            digest(&(operation, sequence, bound))?,
            op.map(|o| o.intent.project_id.clone()),
        ))
    }
    pub(crate) fn ensure_dispatchable(&self, id: &str) -> Result<(), String> {
        require(
            !self
                .reviews
                .iter()
                .any(|r| r.operation_id == id && r.sequence != 3),
            "管理记录已复核为未决状态，禁止重新派发；请核对原记录并明确处置",
        )
    }
    fn accept_record(&mut self, name: &str, record: Record) -> Result<(), String> {
        let id = &record.body.operation_id;
        let external = matches!(
            &record.body.evidence,
            Evidence::ExternalIntent { .. }
                | Evidence::DeputyIntent { .. }
                | Evidence::OmpIntent { .. }
        ) || self.operations.get(id).is_some_and(|op| {
            op.external_request.is_some() || op.deputy_request.is_some() || op.omp_request.is_some()
        });
        require(
            record.body.version == if external { 2 } else { 1 },
            "management evidence source/version mismatch",
        )?;
        match record.body.evidence {
            Evidence::OmpIntent { intent, request } => self.accept_omp_intent(
                (name, id, &record.body.previous),
                record.hash,
                intent,
                request,
            )?,
            Evidence::OmpResponse { response } => {
                self.accept_omp_response(name, id, &record.body.previous, record.hash, response)?
            }
            Evidence::DeputyIntent { intent, request } => self.accept_deputy_intent(
                (name, id, &record.body.previous),
                record.hash,
                intent,
                request,
            )?,
            Evidence::DeputyResponse { response } => {
                self.accept_deputy_response(name, id, &record.body.previous, record.hash, response)?
            }
            Evidence::ExternalIntent { intent, request } => {
                self.accept_external_intent(
                    (name, id, &record.body.previous),
                    record.hash,
                    intent,
                    request,
                )?;
            }
            Evidence::ExternalResponse { response } => {
                self.accept_external_response(
                    name,
                    id,
                    &record.body.previous,
                    record.hash,
                    response,
                )?;
            }
            Evidence::Intent {
                intent,
                request,
                scoped,
            } => {
                self.validate_scoped_intent(&intent, request.as_ref(), scoped.as_ref())?;
                intent.validate()?;
                self.validate_wire_intent(&intent, request.as_ref())?;
                require(
                    name == format!("{id}.0.json")
                        && record.body.previous == "genesis"
                        && intent.operation_id == *id,
                    "invalid management intent link",
                )?;
                require(
                    !self.operations.contains_key(id),
                    "duplicate management intent",
                )?;
                self.operations.insert(
                    id.clone(),
                    Operation {
                        omp_request: None,
                        omp_response: None,
                        deputy_request: None,
                        deputy_response: None,
                        external_request: None,
                        external_response: None,
                        intent,
                        intent_hash: record.hash.clone(),
                        request,
                        transport: None,
                        scoped_request: scoped,
                        scoped_response: None,
                        fee_ack: None,
                        chain_hash: record.hash,
                        response_hash: None,
                        observation: None,
                        finished: false,
                        application: None,
                        applied: false,
                        workflow_hash: None,
                        resolution: None,
                    },
                );
            }
            Evidence::UserResolution { resolution } => {
                let op = self
                    .operations
                    .get_mut(id)
                    .ok_or("resolution without request")?;
                require(
                    name == format!("{id}.4.json") && record.body.previous == op.chain_hash,
                    "invalid user resolution link",
                )?;
                Self::validate_resolution(op, &resolution)?;
                op.resolution = Some(resolution);
                op.chain_hash = record.hash;
            }
            Evidence::ApplyIntent {
                target,
                currency_ack,
            } => {
                let op = self
                    .operations
                    .get_mut(id)
                    .ok_or("application without request")?;
                require(
                    name == format!("{id}.2.json")
                        && op.finished
                        && op.application.is_none()
                        && record.body.previous == op.chain_hash,
                    "invalid application intent link",
                )?;
                target.validate(
                    &op.intent,
                    op.envelope().ok_or("application without response")?,
                )?;
                scoped::validate_ack(op, currency_ack.as_ref())?;
                op.fee_ack = currency_ack;
                op.application = Some(*target);
                op.chain_hash = record.hash;
            }
            Evidence::Applied { workflow_hash } => {
                let op = self
                    .operations
                    .get_mut(id)
                    .ok_or("application receipt without request")?;
                require(
                    name == format!("{id}.3.json")
                        && !op.applied
                        && record.body.previous == op.chain_hash,
                    "invalid application receipt link",
                )?;
                Self::validate_receipt(op, &workflow_hash)?;
                op.applied = true;
                op.workflow_hash = workflow_hash;
                op.chain_hash = record.hash;
            }
            evidence @ (Evidence::Response { .. }
            | Evidence::ScopedResponse { .. }
            | Evidence::Unknown) => {
                let op = self
                    .operations
                    .get_mut(id)
                    .ok_or("management outcome without intent")?;
                require(
                    name == format!("{id}.1.json")
                        && !op.finished
                        && op.external_request.is_none()
                        && op.deputy_request.is_none()
                        && op.omp_request.is_none()
                        && record.body.previous == op.chain_hash,
                    "invalid management outcome link",
                )?;
                if let Evidence::ScopedResponse { response } = &evidence {
                    response.validate(op)?;
                    op.observation = response.observation.clone();
                    op.scoped_response = Some(*response.clone());
                }
                if let Evidence::Response {
                    observation,
                    transport,
                } = evidence
                {
                    require(
                        op.scoped_request.is_none(),
                        "scoped intent cannot accept legacy response",
                    )?;
                    Self::validate_observation(&op.intent, &observation)?;
                    Self::validate_transport(op, &observation, transport.as_ref())?;
                    op.observation = Some(observation);
                    op.transport = transport;
                }
                op.finished = true;
                op.response_hash = Some(record.hash.clone());
                op.chain_hash = record.hash;
            }
        }
        Ok(())
    }
    /// Called by the host only after every cross-ledger validation succeeds.
    /// Restores existing evidence; never executes a missing workflow command.
    pub(crate) fn recover_publications(&mut self) -> Result<(), String> {
        self.recover_publications_observed(|_, _| Ok(()))
    }
    fn recover_publications_observed(
        &mut self,
        mut observe: impl FnMut(RecoveryPoint, usize) -> Result<(), String>,
    ) -> Result<(), String> {
        require(self.healthy(), "management journal requires reopen")?;
        if self.pending_publications.is_empty() {
            return Ok(());
        }
        // Any failed revalidation or partial batch completion requires reopen.
        self.poisoned = true;
        ordinary(&self.root, true)?;
        if self
            .operations
            .values()
            .any(|op| op.scoped_request.is_some())
        {
            scoped::require_guard(&self.root)?;
        }
        for item in &self.pending_publications {
            require(
                read(&item.pending)?.1 == item.bytes,
                "management pending changed after review",
            )?;
            if item.published {
                require(
                    read(&item.target)?.1 == item.bytes,
                    "management target changed after review",
                )?;
            } else {
                absent(&item.target)?;
            }
        }
        observe(RecoveryPoint::Revalidated, 0)?;
        for (index, item) in self.pending_publications.iter().enumerate() {
            if !item.published {
                OpenOptions::new()
                    .write(true)
                    .open(&item.pending)
                    .and_then(|f| f.sync_all())
                    .map_err(|e| e.to_string())?;
                observe(RecoveryPoint::Synced, index)?;
                fs::hard_link(&item.pending, &item.target).map_err(|e| e.to_string())?;
            }
            observe(RecoveryPoint::FinalNamePresent, index)?;
            fs::remove_file(&item.pending).map_err(|e| e.to_string())?;
            observe(RecoveryPoint::PendingRemoved, index)?;
        }
        observe(RecoveryPoint::Finished, 0)?;
        self.pending_publications.clear();
        self.poisoned = false;
        Ok(())
    }
    pub(crate) fn healthy(&self) -> bool {
        !self.poisoned
    }
    pub(crate) fn application_closure(&self, id: &str) -> Option<ApplicationClosure> {
        let op = self.operations.get(id)?;
        match &op.resolution {
            Some(UserResolution::AbandonApplication {
                reason,
                workflow_sequence,
                workflow_hash,
            }) => Some(ApplicationClosure {
                reason: reason.clone(),
                workflow_sequence: *workflow_sequence,
                workflow_hash: workflow_hash.clone(),
            }),
            _ => None,
        }
    }
    /// ApplyIntent reserves an ID even before Assign exists; a closed reservation never expires.
    pub(crate) fn check_attempt_reservation(
        &self,
        project: &str,
        attempt: &str,
        owner: Option<&str>,
    ) -> Result<(), String> {
        require(!self.poisoned, "management journal requires reopen")?;
        for op in self
            .operations
            .values()
            .filter(|op| op.intent.project_id == project)
        {
            if matches!(&op.application, Some(ApplicationTarget::Workflow {
                command: WorkflowCommand::Assign { attempt: reserved, .. }, ..
            }) if reserved == attempt)
            {
                require(
                    owner == Some(op.intent.operation_id.as_str()) && op.resolution.is_none(),
                    "attempt ID is reserved by an existing or closed management application",
                )?;
            }
        }
        Ok(())
    }
    fn publish(
        &mut self,
        id: &str,
        sequence: u8,
        previous: String,
        evidence: Evidence,
    ) -> Result<String, String> {
        self.publish_observed(id, sequence, previous, evidence, |_| Ok(()))
    }
    fn publish_observed(
        &mut self,
        id: &str,
        sequence: u8,
        previous: String,
        evidence: Evidence,
        mut observe: impl FnMut(PublishPoint) -> Result<(), String>,
    ) -> Result<String, String> {
        require(!self.poisoned, "management journal requires reopen")?;
        require(
            self.pending_publications.is_empty(),
            "management publication recovery required before writing",
        )?;
        if matches!(
            &evidence,
            Evidence::Intent {
                scoped: Some(_),
                ..
            } | Evidence::ScopedResponse { .. }
                | Evidence::ApplyIntent {
                    currency_ack: Some(_),
                    ..
                }
        ) || self.is_scoped(id)
        {
            scoped::require_guard(&self.root)?;
        }
        if sequence <= 2 {
            self.ensure_dispatchable(id)?;
        }
        let external = matches!(
            &evidence,
            Evidence::ExternalIntent { .. }
                | Evidence::DeputyIntent { .. }
                | Evidence::OmpIntent { .. }
        ) || self.operations.get(id).is_some_and(|op| {
            op.external_request.is_some() || op.deputy_request.is_some() || op.omp_request.is_some()
        });
        if external {
            crate::management_source_guard::require_protected(
                self.root.parent().ok_or("management root missing")?,
                true,
            )?;
        }
        let body = Body {
            version: if external { 2 } else { 1 },
            operation_id: id.into(),
            previous,
            at: chrono::Utc::now().to_rfc3339(),
            evidence,
        };
        let hash = digest(&body)?;
        let bytes = serde_json::to_vec(&Record {
            body,
            hash: hash.clone(),
        })
        .map_err(|e| e.to_string())?;
        require(bytes.len() <= MAX_BYTES, "management record too large")?;
        let target = self.root.join(format!("{id}.{sequence}.json"));
        let reviewed = self
            .reviews
            .iter()
            .any(|r| r.operation_id == id && r.sequence == sequence);
        let temp = self.root.join(if reviewed {
            format!("{id}.{sequence}.{hash}.pending")
        } else {
            format!("{id}.{sequence}.pending")
        });
        ordinary(&self.root, true)?;
        absent(&target)?;
        absent(&temp)?;
        // Once writing starts, any failure requires a fresh disk audit, never a resend.
        self.poisoned = true;
        let mut f = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        observe(PublishPoint::TempCreated)?;
        let middle = bytes.len() / 2;
        f.write_all(&bytes[..middle]).map_err(|e| e.to_string())?;
        observe(PublishPoint::PartialWritten)?;
        f.write_all(&bytes[middle..])
            .and_then(|_| f.sync_all())
            .map_err(|e| e.to_string())?;
        drop(f);
        observe(PublishPoint::TempSynced)?;
        fs::hard_link(&temp, &target).map_err(|e| e.to_string())?;
        observe(PublishPoint::Published)?;
        fs::remove_file(&temp).map_err(|e| e.to_string())?;
        observe(PublishPoint::PendingRemoved)?;
        self.poisoned = false;
        Ok(hash)
    }
    pub(crate) fn begin(&mut self, intent: DecisionIntent) -> Result<(), String> {
        self.begin_with_request(intent, None)
    }
    fn validate_wire_intent(
        &self,
        intent: &DecisionIntent,
        request: Option<&ManagementRequestFingerprint>,
    ) -> Result<(), String> {
        if let Some(request) = request {
            crate::currency_guard::require_protected(
                self.root.parent().ok_or("management root missing")?,
                true,
            )?;
            request.validate_for(intent.purpose, &intent.model, &intent.state_digest)?;
        }
        Ok(())
    }
    pub(crate) fn begin_with_request(
        &mut self,
        intent: DecisionIntent,
        request: Option<ManagementRequestFingerprint>,
    ) -> Result<(), String> {
        self.begin_with_binding(intent, request, None)
    }
    fn validate_scoped_intent(
        &self,
        intent: &DecisionIntent,
        request: Option<&ManagementRequestFingerprint>,
        scoped: Option<&ScopedIntentBinding>,
    ) -> Result<(), String> {
        if let Some(binding) = scoped {
            scoped::require_guard(&self.root)?;
            require(request.is_none(), "scoped intent cannot mix legacy request")?;
            binding.validate(intent)?;
        }
        Ok(())
    }
    pub(crate) fn require_intent_capacity(&self, pending_counts: usize) -> Result<(), String> {
        require(
            self.operations.len()
                + self.reviews.iter().filter(|r| r.sequence == 0).count()
                + pending_counts
                < MAX_OPERATIONS,
            "management journal full",
        )
    }
    fn begin_with_binding(
        &mut self,
        intent: DecisionIntent,
        request: Option<ManagementRequestFingerprint>,
        scoped: Option<ScopedIntentBinding>,
    ) -> Result<(), String> {
        intent.validate()?;
        self.validate_scoped_intent(&intent, request.as_ref(), scoped.as_ref())?;
        self.validate_wire_intent(&intent, request.as_ref())?;
        if let Some(request) = &request {
            require(
                request
                    == PreparedDecision::new(
                        intent.purpose,
                        &intent.model,
                        &intent.state_digest,
                        &intent.context,
                    )?
                    .fingerprint(),
                "management intent does not describe the prepared request",
            )?;
        }
        self.require_intent_capacity(0)?;
        require(
            !self.operations.contains_key(&intent.operation_id),
            "management operation already recorded; never resend",
        )?;
        require(
            !self.operations.values().any(|op| {
                op.intent.project_id == intent.project_id && !op.applied && op.resolution.is_none()
            }),
            "project management decision awaits reconciliation or application",
        )?;
        let id = intent.operation_id.clone();
        let hash = self.publish(
            &id,
            0,
            "genesis".into(),
            Evidence::Intent {
                intent: intent.clone(),
                request: request.clone(),
                scoped: scoped.clone(),
            },
        )?;
        self.operations.insert(
            id,
            Operation {
                omp_request: None,
                omp_response: None,
                deputy_request: None,
                deputy_response: None,
                external_request: None,
                external_response: None,
                intent,
                intent_hash: hash.clone(),
                request,
                transport: None,
                scoped_request: scoped,
                scoped_response: None,
                fee_ack: None,
                chain_hash: hash,
                response_hash: None,
                observation: None,
                finished: false,
                application: None,
                applied: false,
                workflow_hash: None,
                resolution: None,
            },
        );
        Ok(())
    }
    fn validate_observation(
        intent: &DecisionIntent,
        o: &DecisionObservation,
    ) -> Result<(), String> {
        if let ManagementDecision::Refine { document } = &o.envelope.decision {
            crate::host::document_refinement::validate_quotes(intent, document)?;
        }
        if let Some(supervision) = intent.context.get("supervision") {
            require(
                intent.purpose == Purpose::Advise,
                "supervision response purpose mismatch",
            )?;
            require(
                match &o.envelope.decision {
                    ManagementDecision::Advise { task_id, .. } => supervision["taskId"] == *task_id,
                    ManagementDecision::Wait { .. } => true,
                    _ => false,
                },
                "supervision response targets another task",
            )?;
        }
        require(
            o.requested_model == intent.model,
            "management response model mismatch",
        )?;
        require(
            o.response_id.starts_with("resp_")
                && o.response_id.len() <= 200
                && o.response_id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_'),
            "invalid management response ID",
        )?;
        OpenAiClient::validate_request(
            intent.purpose,
            &o.actual_model,
            &intent.state_digest,
            &intent.context,
        )?;
        o.envelope.validate(intent.purpose, &intent.state_digest)
    }
    pub(crate) fn finish(
        &mut self,
        id: &str,
        outcome: Result<DecisionObservation, String>,
    ) -> Result<(), String> {
        self.finish_result(id, outcome.map(DecisionResult::from))
    }
    fn validate_transport(
        op: &Operation,
        observation: &DecisionObservation,
        transport: Option<&DecisionTransport>,
    ) -> Result<(), String> {
        if op.scoped_request.is_some() {
            require(
                transport.is_none(),
                "scoped response cannot use legacy transport",
            )?;
            return scoped::validate_operation(op);
        }
        require(
            op.request.is_some() == transport.is_some(),
            "management transport evidence missing or unsolicited",
        )?;
        if let Some(transport) = transport {
            require(
                op.request.as_ref() == Some(&transport.request),
                "management response belongs to another wire request",
            )?;
            transport.validate(observation)?;
        }
        Ok(())
    }
    pub(crate) fn finish_result(
        &mut self,
        id: &str,
        outcome: Result<DecisionResult, String>,
    ) -> Result<(), String> {
        let op = self.operations.get(id).ok_or("management intent missing")?;
        require(
            !op.finished
                && op.resolution.is_none()
                && op.external_request.is_none()
                && op.deputy_request.is_none()
                && op.omp_request.is_none(),
            "management outcome already recorded, discarded, or belongs to external chief",
        )?;
        require(
            op.scoped_request.is_none(),
            "scoped outcome requires independent billing capture",
        )?;
        let result = outcome.ok().filter(|r| {
            Self::validate_observation(&op.intent, &r.observation).is_ok()
                && Self::validate_transport(op, &r.observation, r.transport.as_ref()).is_ok()
        });
        let evidence = result
            .clone()
            .map_or(Evidence::Unknown, |result| Evidence::Response {
                observation: result.observation,
                transport: result.transport,
            });
        let previous = op.chain_hash.clone();
        let hash = self.publish(id, 1, previous, evidence)?;
        let op = self.operations.get_mut(id).unwrap();
        if let Some(result) = result {
            op.observation = Some(result.observation);
            op.transport = result.transport;
        }
        op.finished = true;
        op.response_hash = Some(hash.clone());
        op.chain_hash = hash;
        Ok(())
    }
    pub(crate) fn statuses(&self) -> Vec<DecisionStatus> {
        self.operations
            .values()
            .map(|op| DecisionStatus {
                operation_id: op.intent.operation_id.clone(),
                project_id: op.intent.project_id.clone(),
                purpose: op.intent.purpose,
                state: if self.poisoned {
                    DecisionState::Unknown
                } else if matches!(op.resolution, Some(UserResolution::Discard { .. })) {
                    DecisionState::Discarded
                } else if matches!(op.resolution, Some(UserResolution::AbandonStartup { .. })) {
                    DecisionState::StartupClosed
                } else if matches!(
                    op.resolution,
                    Some(UserResolution::AbandonApplication { .. })
                ) {
                    DecisionState::Abandoned
                } else if op.applied {
                    DecisionState::Applied
                } else if op.application.is_some() {
                    DecisionState::Applying
                } else if op.envelope().is_some() {
                    DecisionState::Ready
                } else if op.external_request.is_some()
                    && !op.finished
                    && !self
                        .reviews
                        .iter()
                        .any(|r| r.operation_id == op.intent.operation_id && r.sequence <= 2)
                {
                    DecisionState::AwaitingExternal
                } else {
                    DecisionState::Unknown
                },
                state_digest: op.intent.state_digest.clone(),
            })
            .collect()
    }
    pub(crate) fn intent(&self, id: &str) -> Result<&DecisionIntent, String> {
        Ok(&self
            .operations
            .get(id)
            .ok_or("management intent missing")?
            .intent)
    }
    /// Original billing evidence is independent of local apply/discard state.
    pub(crate) fn billing_evidence(&self, id: &str) -> Result<DecisionBillingEvidence<'_>, String> {
        require(
            self.healthy(),
            "management billing evidence publication unresolved",
        )?;
        require(
            !self
                .reviews
                .iter()
                .any(|r| r.operation_id == id && r.sequence <= 1),
            "management billing evidence requires original record review",
        )?;
        let op = self
            .operations
            .get(id)
            .ok_or("original management intent missing; do not infer not-sent")?;
        require(
            op.external_request.is_none()
                && op.deputy_request.is_none()
                && op.omp_request.is_none(),
            "non-OpenAI management cannot use OpenAI billing evidence",
        )?;
        if op.scoped_request.is_some() {
            scoped::validate_operation(op)?;
        }
        if let Some(o) = &op.observation {
            Self::validate_transport(op, o, op.transport.as_ref())?;
        }
        Ok(DecisionBillingEvidence {
            intent: &op.intent,
            scoped_request: op.scoped_request.as_ref(),
            scoped_response: op.scoped_response.as_ref(),
            request: op.request.as_ref(),
            transport: op.transport.as_ref(),
            observation: op.observation.as_ref(),
            head: op.response_hash.as_deref().unwrap_or(&op.intent_hash),
        })
    }
    pub(crate) fn require_billing_published(&self) -> Result<(), String> {
        require(
            self.healthy() && self.pending_publications.is_empty(),
            "publish original management evidence before charging",
        )
    }
    /// A healthy complete directory can establish absence for a durable
    /// preparation. It is never evidence of a free or retryable request.
    pub(crate) fn absent_prepared_intent(&self, id: &str) -> Result<bool, String> {
        require(self.healthy(), "management journal requires reopen")?;
        require(
            !self.reviews.iter().any(|r| r.operation_id == id),
            "prepared management request has reviewed original fragments",
        )?;
        Ok(!self.operations.contains_key(id))
    }
    /// Include completed/discarded calls: local disposition never deletes cost.
    /// Legacy records without a wire fingerprint still need separate migration
    /// review before monetary execution can be enabled.
    pub(crate) fn currency_requests(&self, project: &str) -> Vec<&str> {
        self.operations
            .iter()
            .filter(|(_, op)| {
                op.intent.project_id == project
                    && (op.request.is_some() || op.scoped_request.is_some())
                    && [
                        "/executionSetup/setup/policy/currencyBudget",
                        "/workflow/revision/policy/currencyBudget",
                    ]
                    .iter()
                    .any(|p| op.intent.context.pointer(p).is_some_and(|v| !v.is_null()))
            })
            .map(|(id, _)| id.as_str())
            .collect()
    }
    /// Original history remains billable/uncertain after local apply or discard.
    pub(crate) fn currency_history_gaps(
        &self,
    ) -> Vec<cerulean_domain::currency_budget::CurrencyHistoryGap> {
        use cerulean_domain::currency_budget::{CurrencyHistoryGap, CurrencyHistoryReason};
        let mut gaps: Vec<_> = self
            .reviews
            .iter()
            .filter(|r| {
                r.sequence <= 1
                    && !self
                        .operations
                        .get(&r.operation_id)
                        .is_some_and(|op| op.external_request.is_some())
            })
            .map(|r| CurrencyHistoryGap {
                project_id: r.project_id.clone(),
                source_id: r.operation_id.clone(),
                reason: CurrencyHistoryReason::ReviewedManagementRecord,
                reference: r.relative_path.clone(),
            })
            .collect();
        for (id, op) in &self.operations {
            // External sessions have no product API call. Their subscription or
            // session usage is unknown, not a legacy OpenAI API charge or zero cost.
            if op.external_request.is_some() {
                continue;
            }
            if gaps.iter().any(|g| &g.source_id == id) {
                continue;
            }
            let reason = if op.deputy_request.is_some() || op.omp_request.is_some() {
                Some(CurrencyHistoryReason::ManagementWithoutBudget)
            } else if op.request.is_none() && op.scoped_request.is_none() {
                Some(CurrencyHistoryReason::LegacyManagementRequest)
            } else if ![
                "/executionSetup/setup/policy/currencyBudget",
                "/workflow/revision/policy/currencyBudget",
            ]
            .iter()
            .any(|p| op.intent.context.pointer(p).is_some_and(|v| !v.is_null()))
            {
                Some(CurrencyHistoryReason::ManagementWithoutBudget)
            } else {
                None
            };
            if let Some(reason) = reason {
                gaps.push(CurrencyHistoryGap {
                    project_id: Some(op.intent.project_id.clone()),
                    source_id: id.clone(),
                    reason,
                    reference: format!("management-decisions/{id}.0.json"),
                });
            }
        }
        gaps
    }
    pub(crate) fn workspace_basis(&self, project: &str) -> Result<String, String> {
        require(self.healthy(), "原指挥证据写入尚未核对，请保留记录并重开")?;
        struct Hash(sha2::Sha256);
        impl Write for Hash {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.update(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut hash = Hash(sha2::Sha256::new());
        serde_json::to_writer(&mut hash, &"workspace-management-evidence-v1")
            .map_err(|e| e.to_string())?;
        for (id, op) in &self.operations {
            if op.intent.project_id != project {
                continue;
            }
            serde_json::to_writer(
                &mut hash,
                &(
                    id,
                    &op.intent,
                    &op.intent_hash,
                    &op.request,
                    &op.transport,
                    &op.chain_hash,
                    &op.response_hash,
                    &op.observation,
                    op.finished,
                    &op.application,
                    op.applied,
                    &op.workflow_hash,
                    &op.resolution,
                ),
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(format!("{:x}", hash.0.finalize()))
    }
    pub(crate) fn evidence_digest(&self, id: &str) -> Result<&str, String> {
        Ok(&self
            .operations
            .get(id)
            .ok_or("management intent missing")?
            .chain_hash)
    }
    pub(crate) fn response_head(&self, id: &str) -> Result<&str, String> {
        self.operations
            .get(id)
            .and_then(|op| op.response_hash.as_deref())
            .ok_or_else(|| "management response evidence missing".into())
    }
    pub(crate) fn observation(&self, id: &str) -> Result<&DecisionObservation, String> {
        let op = self.operations.get(id).ok_or("management intent missing")?;
        let observation = op
            .observation
            .as_ref()
            .ok_or("management response missing")?;
        Self::validate_transport(op, observation, op.transport.as_ref())?;
        Ok(observation)
    }
    pub(crate) fn startup_closure(&self, id: &str) -> Option<StartupResolution> {
        match &self.operations.get(id)?.resolution {
            Some(UserResolution::AbandonStartup {
                reason,
                attempt_id,
                startup_hash,
            }) => Some(StartupResolution {
                reason: reason.clone(),
                attempt_id: attempt_id.clone(),
                startup_hash: startup_hash.clone(),
            }),
            _ => None,
        }
    }
    pub(crate) fn ready(
        &self,
        id: &str,
        current_digest: &str,
    ) -> Result<&DecisionObservation, String> {
        require(!self.poisoned, "management journal requires reopen")?;
        let op = self.operations.get(id).ok_or("management intent missing")?;
        require(
            !op.applied && op.resolution.is_none(),
            "management decision already applied or discarded",
        )?;
        require(
            op.intent.state_digest == current_digest,
            "management decision is stale",
        )?;
        op.observation
            .as_ref()
            .ok_or("management result unknown; do not resend".into())
    }
    #[cfg(test)]
    pub(crate) fn prepare_application(
        &mut self,
        id: &str,
        target: ApplicationTarget,
        current_digest: &str,
    ) -> Result<(), String> {
        self.prepare_application_with_fee(id, target, current_digest, None)
    }
    pub(crate) fn prepare_application_with_fee(
        &mut self,
        id: &str,
        target: ApplicationTarget,
        current_digest: &str,
        currency_ack: Option<ScopedFeeAck>,
    ) -> Result<(), String> {
        if let ApplicationTarget::Workflow { context, .. } = &target {
            if let Some(ExecutionEvidence::Revision { revision }) = &context.execution {
                crate::reader_generation::require_revision(
                    self.root.parent().ok_or("Missing management data root")?,
                    revision,
                )?;
            }
        }
        self.ensure_dispatchable(id)?;
        self.ready_envelope(id, current_digest)?;
        let op = self.operations.get(id).unwrap();
        scoped::validate_ack(op, currency_ack.as_ref())?;
        target.validate(&op.intent, op.envelope().unwrap())?;
        if let ApplicationTarget::Workflow {
            command: WorkflowCommand::Assign { attempt, .. },
            ..
        } = &target
        {
            self.check_attempt_reservation(&op.intent.project_id, attempt, Some(id))?;
        }
        if let Some(existing) = &op.application {
            return require(
                existing == &target && op.fee_ack == currency_ack,
                "management application target or fee ACK changed",
            );
        }
        let previous = op.chain_hash.clone();
        let hash = self.publish(
            id,
            2,
            previous,
            Evidence::ApplyIntent {
                target: Box::new(target.clone()),
                currency_ack: currency_ack.clone(),
            },
        )?;
        let op = self.operations.get_mut(id).unwrap();
        op.fee_ack = currency_ack;
        op.application = Some(target);
        op.chain_hash = hash;
        Ok(())
    }
    fn validate_receipt(op: &Operation, workflow_hash: &Option<String>) -> Result<(), String> {
        match op
            .application
            .as_ref()
            .ok_or("application intent missing")?
        {
            ApplicationTarget::Wait => {
                require(workflow_hash.is_none(), "wait has no workflow event")
            }
            ApplicationTarget::Workflow { .. } | ApplicationTarget::DesktopHook { .. } => sha256(
                workflow_hash
                    .as_deref()
                    .ok_or("workflow event hash required")?,
            ),
        }
    }
    pub(crate) fn record_applied(
        &mut self,
        id: &str,
        workflow_hash: Option<String>,
    ) -> Result<(), String> {
        let op = self.operations.get(id).ok_or("management intent missing")?;
        require(
            !matches!(
                op.resolution,
                Some(UserResolution::AbandonApplication { .. })
            ),
            "management application closed by user; late application receipt rejected",
        )?;
        Self::validate_receipt(op, &workflow_hash)?;
        if op.applied {
            return require(
                op.workflow_hash == workflow_hash,
                "application receipt conflict",
            );
        }
        let previous = op.chain_hash.clone();
        let hash = self.publish(
            id,
            3,
            previous,
            Evidence::Applied {
                workflow_hash: workflow_hash.clone(),
            },
        )?;
        let op = self.operations.get_mut(id).unwrap();
        op.applied = true;
        op.workflow_hash = workflow_hash;
        op.chain_hash = hash;
        Ok(())
    }
    pub(crate) fn application(&self, id: &str) -> Option<(&ApplicationTarget, bool, Option<&str>)> {
        self.operations.get(id).and_then(|o| {
            o.application
                .as_ref()
                .map(|t| (t, o.applied, o.workflow_hash.as_deref()))
        })
    }
    fn validate_resolution(op: &Operation, resolution: &UserResolution) -> Result<(), String> {
        require(
            op.resolution.is_none(),
            "management request already resolved by user",
        )?;
        let text = match resolution {
            UserResolution::AbandonStartup {
                reason,
                attempt_id,
                startup_hash,
            } => {
                require(
                    !op.applied
                        && op.application.is_none()
                        && op.intent.purpose == Purpose::Assign
                        && op.envelope().is_some_and(|o| {
                            matches!(o.decision, ManagementDecision::Assign { .. })
                        }),
                    "only an unapplied assignment response can close its startup",
                )?;
                identifier(attempt_id)?;
                sha256(startup_hash)?;
                reason
            }
            UserResolution::AbandonApplication {
                reason,
                workflow_sequence,
                workflow_hash,
            } => {
                require(
                    !op.applied
                        && matches!(op.application, Some(ApplicationTarget::Workflow { .. })),
                    "only an unapplied workflow application can be closed",
                )?;
                match (*workflow_sequence, workflow_hash) {
                    (0, None) => {}
                    (n, Some(hash)) if n > 0 => sha256(hash)?,
                    _ => return Err("invalid application closure workflow anchor".into()),
                }
                reason
            }
            UserResolution::Discard { reason } => {
                require(
                    !op.applied && op.application.is_none(),
                    "application must be reconciled before disposition",
                )?;
                reason
            }
            UserResolution::Reply { answer } => {
                require(
                    op.applied && matches!(op.application, Some(ApplicationTarget::Wait)),
                    "only applied Wait accepts an answer",
                )?;
                require(serde_json::to_vec(&serde_json::json!({"operationId":op.intent.operation_id,"question":op.envelope().map(|o| &o.decision),"answer":answer})).map_err(|e|e.to_string())?.len() <= 24 * 1024,
                    "答复转义后超出单次上下文容量，请缩短内容")?;
                answer
            }
        };
        require(
            !text.trim().is_empty() && text.len() <= 8000 && !text.contains('\0'),
            "invalid user resolution text",
        )
    }
    pub(crate) fn resolve(
        &mut self,
        id: &str,
        expected: &str,
        resolution: UserResolution,
    ) -> Result<(), String> {
        let op = self.operations.get(id).ok_or("management intent missing")?;
        require(op.chain_hash == expected, "management review is stale")?;
        Self::validate_resolution(op, &resolution)?;
        let hash = self.publish(
            id,
            4,
            expected.into(),
            Evidence::UserResolution {
                resolution: resolution.clone(),
            },
        )?;
        let op = self.operations.get_mut(id).unwrap();
        op.resolution = Some(resolution);
        op.chain_hash = hash;
        Ok(())
    }
    pub(crate) fn review(&self, id: &str) -> Result<DecisionReview, String> {
        let op = self.operations.get(id).ok_or("management intent missing")?;
        let wait = op.envelope().and_then(|o| match &o.decision {
            ManagementDecision::Wait { reason } => Some(reason.clone()),
            _ => None,
        });
        let (reply, discard) = match &op.resolution {
            Some(UserResolution::Reply { answer }) => (Some(answer.clone()), None),
            Some(UserResolution::Discard { reason }) => (None, Some(reason.clone())),
            Some(UserResolution::AbandonApplication { reason, .. }) => (None, Some(reason.clone())),
            Some(UserResolution::AbandonStartup { reason, .. }) => (None, Some(reason.clone())),
            None => (None, None),
        };
        Ok((op.chain_hash.clone(), wait, reply, discard))
    }
    pub(crate) fn user_replies(&self, project: &str, max_bytes: usize) -> Vec<Value> {
        let mut replies: Vec<_> = self.operations.values().filter_map(|op| {
            if op.intent.project_id != project { return None; }
            let Some(UserResolution::Reply { answer }) = &op.resolution else { return None; };
            let Some(DecisionEnvelope { decision: ManagementDecision::Wait { reason }, .. }) = op.envelope() else { return None; };
            Some((op.intent.created_at.as_str(), op.intent.operation_id.as_str(), serde_json::json!({"operationId":op.intent.operation_id,"question":reason,"answer":answer})))
        }).collect();
        replies.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)));
        let mut bytes = 2;
        let mut selected = Vec::new();
        for (_, _, value) in replies.into_iter().rev() {
            let size = serde_json::to_vec(&value).expect("JSON value").len() + 1;
            if bytes + size > max_bytes.min(24 * 1024) {
                break;
            }
            bytes += size;
            selected.push(value);
        }
        selected.reverse();
        selected
    }
    pub(crate) fn reply_count(&self, project: &str) -> usize {
        self.operations
            .values()
            .filter(|op| {
                op.intent.project_id == project
                    && matches!(op.resolution, Some(UserResolution::Reply { .. }))
            })
            .count()
    }
    pub(crate) fn unanswered_wait(&self, id: &str) -> bool {
        self.operations.get(id).is_some_and(|op| {
            op.applied
                && matches!(op.application, Some(ApplicationTarget::Wait))
                && op.resolution.is_none()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn journal_capacity_accounts_for_pending_counts_before_paid_preflight() {
        let dir = tempfile::tempdir().unwrap();
        let mut journal = DecisionJournal::open(dir.path()).unwrap();
        assert!(journal.require_intent_capacity(MAX_OPERATIONS - 1).is_ok());
        assert!(journal.require_intent_capacity(MAX_OPERATIONS).is_err());
        journal.begin(intent()).unwrap();
        assert!(journal.require_intent_capacity(MAX_OPERATIONS - 1).is_err());
        assert!(journal.require_intent_capacity(MAX_OPERATIONS - 2).is_ok());
    }
    use cerulean_domain::management::{DecisionEnvelope, ManagementDecision};
    #[test]
    fn startup_resolution_keeps_original_response_head_and_never_accepts_late_application() {
        let dir = tempfile::tempdir().unwrap();
        let mut journal = DecisionJournal::open(dir.path()).unwrap();
        let mut i = intent();
        i.purpose = Purpose::Assign;
        journal.begin(i.clone()).unwrap();
        let mut observation = response(&i);
        observation.envelope.decision = ManagementDecision::Assign {
            task_id: "task".into(),
            worker_id: "worker-codex".into(),
            rationale: "startup fixture".into(),
        };
        journal.finish(&i.operation_id, Ok(observation)).unwrap();
        let response_head = journal.response_head(&i.operation_id).unwrap().to_owned();
        journal
            .resolve(
                &i.operation_id,
                &response_head,
                UserResolution::AbandonStartup {
                    reason: "Close interrupted startup".into(),
                    attempt_id: "startup-attempt".into(),
                    startup_hash: "c".repeat(64),
                },
            )
            .unwrap();
        assert_eq!(journal.statuses()[0].state, DecisionState::StartupClosed);
        assert!(journal.statuses()[0].state.resolved());
        assert!(journal.ready(&i.operation_id, &i.state_digest).is_err());
        assert!(journal
            .prepare_application(&i.operation_id, ApplicationTarget::Wait, &i.state_digest)
            .is_err());
        assert!(journal.record_applied(&i.operation_id, None).is_err());
        assert!(journal
            .finish(&i.operation_id, Err("late completion".into()))
            .is_err());
        assert!(!journal.root.join("request-1.2.json").exists());
        assert!(!journal.root.join("request-1.3.json").exists());
        drop(journal);
        let reopened = DecisionJournal::open(dir.path()).unwrap();
        assert_eq!(
            reopened.response_head(&i.operation_id).unwrap(),
            response_head
        );
        assert_eq!(
            reopened
                .startup_closure(&i.operation_id)
                .unwrap()
                .attempt_id,
            "startup-attempt"
        );
        assert!(matches!(
            reopened
                .observation(&i.operation_id)
                .unwrap()
                .envelope
                .decision,
            ManagementDecision::Assign { .. }
        ));
        assert_eq!(reopened.statuses().len(), 1);
        let other = tempfile::tempdir().unwrap();
        let mut with_application = DecisionJournal::open(other.path()).unwrap();
        let (i, _) = prepared_assignment(&mut with_application);
        let head = with_application
            .evidence_digest(&i.operation_id)
            .unwrap()
            .to_owned();
        assert!(with_application
            .resolve(
                &i.operation_id,
                &head,
                UserResolution::AbandonStartup {
                    reason: "wrong stage".into(),
                    attempt_id: "reserved".into(),
                    startup_hash: "c".repeat(64),
                }
            )
            .is_err());
    }
    pub(super) fn prepared_assignment(
        journal: &mut DecisionJournal,
    ) -> (DecisionIntent, ApplicationTarget) {
        use cerulean_domain::execution::{Attempt, IdentityStrength, SessionBinding};
        let mut i = intent();
        i.purpose = Purpose::Assign;
        journal.begin(i.clone()).unwrap();
        journal
            .finish(
                &i.operation_id,
                Ok(DecisionObservation {
                    response_id: "resp_assignment_fixture".into(),
                    requested_model: i.model.clone(),
                    actual_model: i.model.clone(),
                    input_tokens: 0,
                    output_tokens: 0,
                    envelope: DecisionEnvelope {
                        state_digest: i.state_digest.clone(),
                        decision: ManagementDecision::Assign {
                            task_id: "task".into(),
                            worker_id: "worker-codex".into(),
                            rationale: "fixture".into(),
                        },
                    },
                }),
            )
            .unwrap();
        let target = ApplicationTarget::Workflow {
            principal: "deputy".into(),
            command: WorkflowCommand::Assign {
                task: "task".into(),
                worker: "worker-codex".into(),
                attempt: "reserved".into(),
            },
            context: Box::new(CommandContext {
                execution: Some(ExecutionEvidence::Attempt {
                    attempt: Attempt {
                        workspace_proof: None,
                        id: "reserved".into(),
                        task_id: "task".into(),
                        worker_id: "worker-codex".into(),
                        revision_digest: "a".repeat(64),
                        fence: 1,
                        binding: SessionBinding {
                            adapter_id: "codex".into(),
                            adapter_version: "fixture".into(),
                            session_id: "reserved-session".into(),
                            identity_strength: IdentityStrength::ProviderId,
                            workspace: "C:/fixture".into(),
                        },
                    },
                }),
                ..Default::default()
            }),
        };
        journal
            .prepare_application(&i.operation_id, target.clone(), &i.state_digest)
            .unwrap();
        (i, target)
    }
    #[test]
    fn closed_application_preserves_intent_and_reservation_and_rejects_late_receipts() {
        let dir = tempfile::tempdir().unwrap();
        let mut journal = DecisionJournal::open(dir.path()).unwrap();
        let (i, target) = prepared_assignment(&mut journal);
        let application_path = journal.root.join("request-1.2.json");
        let original = fs::read(&application_path).unwrap();
        let head = journal.evidence_digest(&i.operation_id).unwrap().to_owned();
        let resolution = UserResolution::AbandonApplication {
            reason: "Close local application".into(),
            workflow_sequence: 2,
            workflow_hash: Some("b".repeat(64)),
        };
        journal.resolve(&i.operation_id, &head, resolution).unwrap();
        assert_eq!(journal.statuses()[0].state, DecisionState::Abandoned);
        assert!(journal.statuses()[0].state.resolved());
        assert_eq!(journal.application(&i.operation_id).unwrap().0, &target);
        assert!(!journal.application(&i.operation_id).unwrap().1);
        assert!(journal
            .record_applied(&i.operation_id, Some("c".repeat(64)))
            .is_err());
        assert!(journal
            .prepare_application(&i.operation_id, target, &i.state_digest)
            .is_err());
        assert!(!journal.root.join("request-1.3.json").exists());
        assert_eq!(fs::read(application_path).unwrap(), original);
        drop(journal);
        let mut reopened = DecisionJournal::open(dir.path()).unwrap();
        let closed = reopened.application_closure(&i.operation_id).unwrap();
        assert_eq!(closed.reason, "Close local application");
        assert_eq!(closed.workflow_sequence, 2);
        assert_eq!(closed.workflow_hash, Some("b".repeat(64)));
        for owner in [None, Some("request-1"), Some("request-2")] {
            assert!(reopened
                .check_attempt_reservation("project", "reserved", owner)
                .is_err());
        }
        assert!(reopened
            .check_attempt_reservation("project", "fresh", Some("request-2"))
            .is_ok());
        let mut next = i;
        next.operation_id = "request-2".into();
        reopened.begin(next).unwrap();
        assert_eq!(
            reopened.statuses().len(),
            2,
            "closed requests still count against the request limit"
        );
    }
    #[test]
    fn application_closure_rejects_wait_and_invalid_anchor_without_mutation() {
        let dir = tempfile::tempdir().unwrap();
        let mut journal = DecisionJournal::open(dir.path()).unwrap();
        let (i, _) = prepared_assignment(&mut journal);
        let head = journal.evidence_digest(&i.operation_id).unwrap().to_owned();
        assert!(journal
            .resolve(
                &i.operation_id,
                &head,
                UserResolution::AbandonApplication {
                    reason: "invalid anchor".into(),
                    workflow_sequence: 0,
                    workflow_hash: Some("b".repeat(64)),
                }
            )
            .is_err());
        assert_eq!(journal.evidence_digest(&i.operation_id).unwrap(), head);
        let other = tempfile::tempdir().unwrap();
        let mut wait = DecisionJournal::open(other.path()).unwrap();
        let i = intent();
        wait.begin(i.clone()).unwrap();
        wait.finish(&i.operation_id, Ok(response(&i))).unwrap();
        wait.prepare_application(&i.operation_id, ApplicationTarget::Wait, &i.state_digest)
            .unwrap();
        let head = wait.evidence_digest(&i.operation_id).unwrap().to_owned();
        assert!(wait
            .resolve(
                &i.operation_id,
                &head,
                UserResolution::AbandonApplication {
                    reason: "Wait has no workflow event by design".into(),
                    workflow_sequence: 0,
                    workflow_hash: None,
                }
            )
            .is_err());
        wait.record_applied(&i.operation_id, None).unwrap();
        assert_eq!(wait.statuses()[0].state, DecisionState::Applied);
    }
    pub(super) fn intent() -> DecisionIntent {
        let context =
            serde_json::json!({"requirement":"implement feature","workflow":{"sequence":1}});
        DecisionIntent {
            operation_id: "request-1".into(),
            project_id: "project".into(),
            purpose: Purpose::Plan,
            model: "example-model".into(),
            state_digest: digest(&context).unwrap(),
            context,
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }
    #[test]
    fn discard_is_user_evidence_not_success_and_same_request_never_replays() {
        let dir = tempfile::tempdir().unwrap();
        let mut journal = DecisionJournal::open(dir.path()).unwrap();
        let i = intent();
        journal.begin(i.clone()).unwrap();
        let hash = journal.review(&i.operation_id).unwrap().0;
        let reason = UserResolution::Discard {
            reason: "Accept uncertain billing; close local no-tool request".into(),
        };
        assert!(journal
            .resolve(&i.operation_id, &"a".repeat(64), reason.clone())
            .is_err());
        journal.resolve(&i.operation_id, &hash, reason).unwrap();
        assert!(journal.finish(&i.operation_id, Ok(response(&i))).is_err());
        assert!(journal.ready(&i.operation_id, &i.state_digest).is_err());
        assert!(journal.begin(i.clone()).is_err());
        drop(journal);
        let mut journal = DecisionJournal::open(dir.path()).unwrap();
        assert_eq!(journal.statuses()[0].state, DecisionState::Discarded);
        assert!(journal.review(&i.operation_id).unwrap().3.is_some());
        let mut next = i;
        next.operation_id = "request-2".into();
        journal.begin(next).unwrap();
        assert_eq!(journal.statuses().len(), 2);
    }
    #[test]
    fn wait_reply_requires_consumed_wait_and_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let mut journal = DecisionJournal::open(dir.path()).unwrap();
        let i = intent();
        journal.begin(i.clone()).unwrap();
        journal.finish(&i.operation_id, Ok(response(&i))).unwrap();
        let reply = UserResolution::Reply {
            answer: "Use the already approved worker".into(),
        };
        assert!(journal
            .resolve(
                &i.operation_id,
                &journal.review(&i.operation_id).unwrap().0,
                reply.clone()
            )
            .is_err());
        journal
            .prepare_application(&i.operation_id, ApplicationTarget::Wait, &i.state_digest)
            .unwrap();
        assert!(journal
            .resolve(
                &i.operation_id,
                &journal.review(&i.operation_id).unwrap().0,
                UserResolution::Discard {
                    reason: "must not abandon application".into()
                }
            )
            .is_err());
        journal.record_applied(&i.operation_id, None).unwrap();
        assert!(journal.unanswered_wait(&i.operation_id));
        journal
            .resolve(
                &i.operation_id,
                &journal.review(&i.operation_id).unwrap().0,
                reply,
            )
            .unwrap();
        drop(journal);
        let journal = DecisionJournal::open(dir.path()).unwrap();
        assert!(!journal.unanswered_wait(&i.operation_id));
        assert_eq!(journal.statuses()[0].state, DecisionState::Applied);
        assert_eq!(
            journal.user_replies("project", 24 * 1024)[0]["answer"],
            "Use the already approved worker"
        );
        assert!(journal
            .user_replies("another-project", 24 * 1024)
            .is_empty());
    }
    #[test]
    fn long_reply_history_is_preserved_but_prompt_context_is_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let mut journal = DecisionJournal::open(dir.path()).unwrap();
        for number in 0..10 {
            let mut i = intent();
            i.operation_id = format!("reply-{number:02}");
            journal.begin(i.clone()).unwrap();
            journal.finish(&i.operation_id, Ok(response(&i))).unwrap();
            journal
                .prepare_application(&i.operation_id, ApplicationTarget::Wait, &i.state_digest)
                .unwrap();
            journal.record_applied(&i.operation_id, None).unwrap();
            let hash = journal.review(&i.operation_id).unwrap().0;
            if number == 0 {
                assert!(journal
                    .resolve(
                        &i.operation_id,
                        &hash,
                        UserResolution::Reply {
                            answer: "\u{1}".repeat(8000)
                        }
                    )
                    .is_err());
            }
            journal
                .resolve(
                    &i.operation_id,
                    &hash,
                    UserResolution::Reply {
                        answer: "x".repeat(8000),
                    },
                )
                .unwrap();
        }
        drop(journal);
        let journal = DecisionJournal::open(dir.path()).unwrap();
        assert_eq!(journal.reply_count("project"), 10);
        let recent = journal.user_replies("project", 24 * 1024);
        assert!(recent.len() < 10);
        assert_eq!(recent.last().unwrap()["operationId"], "reply-09");
        assert!(serde_json::to_vec(&recent).unwrap().len() <= 24 * 1024);
        assert_eq!(journal.review("reply-00").unwrap().2.unwrap().len(), 8000);
    }
    pub(super) fn response(i: &DecisionIntent) -> DecisionObservation {
        DecisionObservation {
            response_id: "resp_example".into(),
            requested_model: i.model.clone(),
            actual_model: i.model.clone(),
            input_tokens: 20,
            output_tokens: 10,
            envelope: DecisionEnvelope {
                state_digest: i.state_digest.clone(),
                decision: ManagementDecision::Wait {
                    reason: "Need worker configuration".into(),
                },
            },
        }
    }
    #[test]
    fn crash_after_intent_never_resends_even_with_new_operation_id() {
        let dir = tempfile::tempdir().unwrap();
        let i = intent();
        let mut journal = DecisionJournal::open(dir.path()).unwrap();
        journal.begin(i.clone()).unwrap();
        drop(journal);
        let mut journal = DecisionJournal::open(dir.path()).unwrap();
        assert_eq!(journal.statuses()[0].state, DecisionState::Unknown);
        assert!(journal.begin(i.clone()).is_err());
        let mut next = i;
        next.operation_id = "request-2".into();
        assert!(journal.begin(next).is_err());
    }
    #[test]
    fn saved_response_survives_reopen_but_stale_or_duplicate_cannot_apply() {
        let dir = tempfile::tempdir().unwrap();
        let i = intent();
        let mut journal = DecisionJournal::open(dir.path()).unwrap();
        journal.begin(i.clone()).unwrap();
        journal.finish(&i.operation_id, Ok(response(&i))).unwrap();
        assert!(journal.finish(&i.operation_id, Ok(response(&i))).is_err());
        drop(journal);
        let journal = DecisionJournal::open(dir.path()).unwrap();
        assert_eq!(
            journal
                .ready(&i.operation_id, &i.state_digest)
                .unwrap()
                .response_id,
            "resp_example"
        );
        assert!(journal.ready(&i.operation_id, &"b".repeat(64)).is_err());
    }
    #[test]
    fn invalid_or_failed_response_is_unknown_and_does_not_persist_error_body() {
        for bad_response in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let i = intent();
            let mut journal = DecisionJournal::open(dir.path()).unwrap();
            journal.begin(i.clone()).unwrap();
            let mut r = response(&i);
            r.requested_model = "wrong-model".into();
            journal
                .finish(
                    &i.operation_id,
                    if bad_response {
                        Ok(r)
                    } else {
                        Err("sensitive-provider-body".into())
                    },
                )
                .unwrap();
            drop(journal);
            let journal = DecisionJournal::open(dir.path()).unwrap();
            assert_eq!(journal.statuses()[0].state, DecisionState::Unknown);
            assert!(
                !fs::read_to_string(dir.path().join("management-decisions/request-1.1.json"))
                    .unwrap()
                    .contains("sensitive-provider-body")
            );
        }
    }
    #[test]
    fn corrupt_or_orphaned_publication_blocks_reopen_and_invalid_input_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let i = intent();
        let mut journal = DecisionJournal::open(dir.path()).unwrap();
        let mut bad = i.clone();
        bad.context = serde_json::json!({"changed":true});
        assert!(journal.begin(bad).is_err());
        assert_eq!(fs::read_dir(&journal.root).unwrap().count(), 0);
        journal.begin(i.clone()).unwrap();
        let path = journal.root.join("request-1.0.json");
        let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        record["body"]["evidence"]["intent"]["model"] = "tampered".into();
        fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
        assert!(DecisionJournal::open(dir.path()).is_err());
        let second = tempfile::tempdir().unwrap();
        let journal = DecisionJournal::open(second.path()).unwrap();
        fs::write(journal.root.join("request-1.0.pending"), b"partial").unwrap();
        assert!(DecisionJournal::open(second.path()).is_err());
    }
}

#[cfg(test)]
#[path = "management_publication_tests.rs"]
mod publication_tests;

#[cfg(test)]
#[path = "management_review_tests.rs"]
mod review_tests;

#[cfg(all(test, windows))]
#[path = "management_wire_tests.rs"]
mod wire_tests;

#[cfg(all(test, windows))]
#[path = "historical_readback_tests.rs"]
mod historical_readback_tests;
