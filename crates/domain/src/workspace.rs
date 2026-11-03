//! Explicit workspace read models. A page is never a partial Snapshot.
use crate::{execution::Project, Plan};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum WorkspaceQuery {
    CurrentVerificationRecheck {
        project_id: String,
    },
    VerificationRecheckInputs {
        project_id: String,
        original_operation_id: String,
    },
    RegisteredValidator {
        id: String,
        digest: String,
    },
    VerificationRechecks {
        project_id: String,
        offset: u32,
        revision: Option<String>,
    },
    VerificationRecheck {
        project_id: String,
        operation_id: String,
    },
    VerificationRecheckDispositionReview {
        project_id: String,
        operation_id: String,
        reason: String,
    },
    VerificationRecheckReview {
        input: Box<crate::verification_recheck::RecheckRequest>,
    },
    Overview,
    History {
        project_id: Option<String>,
        offset: u32,
        revision: Option<String>,
    },
    ManagementDecisions {
        project_id: String,
        offset: u32,
        revision: Option<String>,
    },
    ManagementDecision {
        project_id: String,
        operation_id: String,
        revision: String,
    },
    ManagementGatewayEvidence {
        project_id: String,
        operation_id: String,
        revision: String,
    },
    HistoryEntry {
        project_id: Option<String>,
        sequence: u32,
        revision: String,
    },
    Recovery {
        project_id: String,
        offset: u32,
        revision: Option<String>,
    },
    RecoveryDetail {
        project_id: String,
        kind: WorkspaceRecoveryKind,
        id: String,
        revision: String,
    },
    Jobs {
        project_id: String,
        offset: u32,
        revision: Option<String>,
    },
    Job {
        project_id: String,
        kind: WorkspaceJobKind,
        id: String,
        revision: String,
    },
    CodexStartups {
        project_id: String,
        offset: u32,
        revision: Option<String>,
    },
    CodexStartup {
        project_id: String,
        attempt_id: String,
        revision: String,
    },
    ProjectRuns {
        project_id: String,
        offset: u32,
        revision: Option<String>,
    },
    ProjectRun {
        project_id: String,
        run_id: String,
        revision: String,
    },
    RevisionReviews {
        project_id: String,
        offset: u32,
        revision: Option<String>,
    },
    RevisionReview {
        project_id: String,
        revision: String,
    },
    VerificationResult {
        project_id: String,
        operation_id: String,
        revision: Option<String>,
    },
    VerificationResultArtifacts {
        project_id: String,
        operation_id: String,
        offset: u32,
        revision: String,
    },
    VerificationResultStages {
        project_id: String,
        operation_id: String,
        offset: u32,
        revision: String,
    },
    WorkerObservation {
        project_id: String,
        attempt_id: String,
        revision: Option<String>,
    },
    WorkerObservationEvents {
        project_id: String,
        attempt_id: String,
        offset: u32,
        revision: String,
    },
    WorkerObservationIncidents {
        project_id: String,
        attempt_id: String,
        offset: u32,
        revision: String,
    },
    Plans {
        offset: u32,
        revision: Option<String>,
    },
    Plan {
        id: String,
        revision: String,
    },
    Projects {
        offset: u32,
        revision: Option<String>,
    },
    Configurations {
        offset: u32,
        revision: Option<String>,
    },
    Configuration {
        project_id: String,
        revision: String,
    },
    Tasks {
        project_id: String,
        offset: u32,
        revision: Option<String>,
    },
    Task {
        project_id: String,
        task_id: String,
        revision: String,
    },
    TaskEvidence {
        project_id: String,
        task_id: String,
        offset: u32,
        revision: String,
    },
    ExecutionReview {
        project_id: String,
        digest: Option<String>,
    },
    UnsentAssignments {
        project_id: String,
        offset: u32,
        revision: Option<String>,
    },
    UnsentAssignment {
        project_id: String,
        task_id: String,
        attempt_id: String,
        revision: String,
    },
    CodexCancellations {
        project_id: String,
        offset: u32,
        revision: Option<String>,
    },
    CodexCancellation {
        project_id: String,
        attempt_id: String,
        revision: String,
    },
    ManagementRecordReviews {
        offset: u32,
        revision: Option<String>,
    },
    ManagementRecordReview {
        relative_path: String,
        revision: String,
    },
    ApiPublicationReviews {
        offset: u32,
        revision: Option<String>,
    },
    ApiPublicationReview {
        relative_path: String,
        revision: String,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceHistorySummary {
    pub sequence: u32,
    pub at: String,
    pub kind: String,
    pub principal: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceHistoryPage {
    pub project_id: Option<String>,
    pub revision: String,
    pub offset: u32,
    pub total: u32,
    pub next_offset: Option<u32>,
    pub entries: Vec<WorkspaceHistorySummary>,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceHistoryDetail {
    pub project_id: Option<String>,
    pub revision: String,
    pub sequence: u32,
    pub original_json: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceConfigurationSummary {
    pub project_id: String,
    pub digest: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceConfigurationPage {
    pub revision: String,
    pub offset: u32,
    pub total: u32,
    pub next_offset: Option<u32>,
    pub entries: Vec<WorkspaceConfigurationSummary>,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceConfigurationDetail {
    pub project_id: String,
    pub revision: String,
    pub saved: crate::project_setup::SavedProjectSetup,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceManagementDecisionPage {
    pub project_id: String,
    pub revision: String,
    pub offset: u32,
    pub next_offset: Option<u32>,
    pub total: u32,
    pub entries: Vec<crate::project_setup::ManagementSummary>,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceManagementDecisionDetail {
    pub project_id: String,
    pub revision: String,
    pub record: crate::project_setup::ManagementView,
}
/// Optional diagnostics for one original management operation. `revision` pins
/// only the management record; `observation` is a separate read-time snapshot.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceManagementGatewayEvidenceDetail {
    pub project_id: String,
    pub operation_id: String,
    pub revision: String,
    pub evidence_digest: String,
    /// None means the original operation has no A6API management route.
    /// Reader errors are returned as errors, never as None or Missing.
    pub observation: Option<WorkspaceGatewayEvidence>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceGatewayEvidence {
    pub provider: WorkspaceGatewayProvider,
    pub local_id: String,
    pub body_response_id: Option<String>,
    pub status: WorkspaceGatewayEvidenceStatus,
    pub headers: Option<WorkspaceGatewayHeaders>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceGatewayProvider {
    A6api,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceGatewayEvidenceStatus {
    Linked,
    HeaderOnly,
    Missing,
    NoHeadersObserved,
    Unavailable,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceGatewayHeaders {
    pub status: u16,
    pub request_id: WorkspaceGatewayRequestId,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum WorkspaceGatewayRequestId {
    Missing {},
    Accepted {
        value: String,
    },
    Rejected {
        reason: WorkspaceGatewayRequestIdRejection,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceGatewayRequestIdRejection {
    InvalidLength,
    InvalidCharacters,
    RepeatedHeader,
    CredentialReflection,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceMode {
    /// Host loading/review state only; never an execution authorization.
    Ready,
    ReviewRequired,
    Diagnostic,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceOverview {
    pub product: String,
    pub protocol_version: u32,
    pub data_directory: String,
    pub mode: WorkspaceMode,
    pub message: Option<String>,
    pub legacy_plan_count: Option<u32>,
    pub pending_legacy_plan_count: Option<u32>,
    /// Unavailable in data diagnostic mode, never inferred as zero.
    pub project_count: Option<u32>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanSummary {
    pub id: String,
    pub revision: u32,
    pub title: String,
    pub digest: String,
    pub created_at: String,
    pub approved: bool,
    pub task_count: u32,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectSummary {
    pub project: Project,
    pub revision_digest: Option<String>,
    pub sequence: u32,
    pub approved: bool,
    pub task_count: u32,
    pub resource_holding_tasks: u32,
    pub unknown_tasks: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PlanPage {
    pub revision: String,
    pub offset: u32,
    pub total: u32,
    pub next_offset: Option<u32>,
    pub entries: Vec<PlanSummary>,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectPage {
    pub revision: String,
    pub offset: u32,
    pub total: u32,
    pub next_offset: Option<u32>,
    pub entries: Vec<ProjectSummary>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceTaskSummary {
    pub id: String,
    pub goal: String,
    pub state: crate::workflow::TaskState,
    pub worker: Option<String>,
    pub attempt: Option<String>,
    pub fence: u32,
    pub verified: bool,
    pub cancellation_requested: Option<bool>,
    pub evidence_count: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceTaskPage {
    pub project: Project,
    pub revision: String,
    pub revision_digest: Option<String>,
    pub sequence: u32,
    pub approved: bool,
    pub offset: u32,
    pub total: u32,
    pub next_offset: Option<u32>,
    pub entries: Vec<WorkspaceTaskSummary>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceTaskDetail {
    pub project_id: String,
    pub revision: String,
    pub task: crate::workflow::WorkflowTask,
    pub run: WorkspaceTaskSummary,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceTaskEvidencePage {
    pub project_id: String,
    pub task_id: String,
    pub revision: String,
    pub offset: u32,
    pub total: u32,
    pub next_offset: Option<u32>,
    pub entries: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceExecutionReview {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_account_setup_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub worker_account_directories: Vec<String>,
    pub project: Project,
    /// Complete original authorization content, never a task-summary substitute.
    pub revision: Option<crate::execution::PlanRevision>,
    pub revision_digest: Option<String>,
    pub approval: Option<crate::execution::Approval>,
    pub revision_requested: Option<String>,
    pub sequence: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceJobKind {
    Codex,
    Omp,
    Api,
    Verification,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum WorkspaceJobStatus {
    Codex(crate::CodexJobState),
    Omp(crate::omp_jobs::OmpJobState),
    Api(crate::api_jobs::ApiJobState),
    Verification(crate::execution::VerificationState),
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceJobSummary {
    /// Attempt id for executors; operation id for independent verification.
    pub id: String,
    pub project_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub status: WorkspaceJobStatus,
    pub started_at: String,
    pub updated_at: Option<String>,
    pub has_message: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum WorkspaceJobRecord {
    Codex(crate::CodexJob),
    Omp(crate::omp_jobs::OmpJob),
    Api(crate::api_jobs::ApiJob),
    Verification(crate::execution::VerificationJob),
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceJobPage {
    pub project_id: String,
    pub revision: String,
    pub offset: u32,
    pub total: u32,
    pub next_offset: Option<u32>,
    pub entries: Vec<WorkspaceJobSummary>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceJobDetail {
    pub project_id: String,
    pub revision: String,
    pub record: WorkspaceJobRecord,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub enum WorkspaceRecoveryKind {
    ManagementRecord,
    StartupClosure,
    StartupReservation,
    OmpStop,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceRecoverySummary {
    pub kind: WorkspaceRecoveryKind,
    pub id: String,
    pub project_id: String,
    pub task_id: Option<String>,
    pub attempt_id: Option<String>,
    pub has_message: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum WorkspaceRecoveryRecord {
    ManagementRecord(crate::ManagementRecordReview),
    StartupClosure(crate::StartupClosureReview),
    StartupReservation(crate::StartupReservation),
    OmpStop(crate::omp_jobs::OmpStopIssue),
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceRecoveryPage {
    pub project_id: String,
    pub revision: String,
    pub offset: u32,
    pub total: u32,
    pub next_offset: Option<u32>,
    pub entries: Vec<WorkspaceRecoverySummary>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceRecoveryDetail {
    pub project_id: String,
    pub revision: String,
    pub record: WorkspaceRecoveryRecord,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceObservationDetail {
    pub project_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub revision: String,
    pub revision_digest: String,
    pub evidence_digest: String,
    pub storage_error: Option<String>,
    pub supervision: Option<crate::supervision::SupervisionPolicy>,
    pub thread_id: Option<String>,
    pub turn_id: Option<String>,
    pub dropped: u32,
    pub rejected: u32,
    pub duplicates: u32,
    pub finished: bool,
    pub event_count: u32,
    pub incident_count: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceObservationEventPage {
    pub project_id: String,
    pub attempt_id: String,
    pub revision: String,
    pub offset: u32,
    pub total: u32,
    pub next_offset: Option<u32>,
    pub entries: Vec<crate::worker_observation::ToolObservation>,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceObservationIncidentPage {
    pub project_id: String,
    pub attempt_id: String,
    pub revision: String,
    pub offset: u32,
    pub total: u32,
    pub next_offset: Option<u32>,
    pub entries: Vec<crate::supervision::SupervisionIncident>,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceVerificationResultDetail {
    pub project_id: String,
    pub operation_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub state: crate::execution::VerificationState,
    pub revision_digest: String,
    pub manifest_digest: String,
    pub snapshot_digest: Option<String>,
    pub ledger_recorded: bool,
    pub evidence_digest: String,
    pub observed_at: String,
    pub message: Option<String>,
    pub recovery: Option<crate::verification_detail::VerificationRecoveryReview>,
    pub recovery_error: Option<String>,
    pub revision: String,
    pub artifact_count: u32,
    pub stage_count: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceVerificationArtifactPage {
    pub project_id: String,
    pub operation_id: String,
    pub revision: String,
    pub offset: u32,
    pub total: u32,
    pub next_offset: Option<u32>,
    pub entries: Vec<crate::execution::Artifact>,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceVerificationStagePage {
    pub project_id: String,
    pub operation_id: String,
    pub revision: String,
    pub offset: u32,
    pub total: u32,
    pub next_offset: Option<u32>,
    pub entries: Vec<crate::verification_detail::VerificationStageView>,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceCodexStartupPage {
    pub project_id: String,
    pub revision: String,
    pub offset: u32,
    pub total: u32,
    pub next_offset: Option<u32>,
    pub entries: Vec<crate::CodexStartupSummary>,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceCodexStartupDetail {
    pub project_id: String,
    pub revision: String,
    pub detail: crate::CodexStartupView,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceProjectRunSummary {
    pub id: String,
    pub project_id: String,
    pub revision_digest: String,
    pub setup_digest: String,
    pub state: crate::project_setup::RunState,
    pub has_message: bool,
    pub acknowledged_decisions: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceProjectRunPage {
    pub project_id: String,
    pub revision: String,
    pub offset: u32,
    pub total: u32,
    pub next_offset: Option<u32>,
    pub active_count: u32,
    pub entries: Vec<WorkspaceProjectRunSummary>,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceProjectRunDetail {
    pub project_id: String,
    pub revision: String,
    pub record: crate::project_setup::ProjectRun,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceRevisionReviewPage {
    pub project_id: String,
    pub revision: String,
    pub offset: u32,
    pub total: u32,
    pub next_offset: Option<u32>,
    pub entries: Vec<crate::project_setup::ProjectRevisionReview>,
    pub unavailable_reason: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceRevisionReviewDetail {
    pub project_id: String,
    pub revision: String,
    pub record: crate::project_setup::ProjectRevisionReview,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceUnsentAssignmentPage {
    pub project_id: String,
    pub revision: String,
    pub offset: u32,
    pub total: u32,
    pub next_offset: Option<u32>,
    pub entries: Vec<crate::UnsentAssignmentReview>,
    pub unavailable_reason: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceUnsentAssignmentDetail {
    pub project_id: String,
    pub revision: String,
    pub record: crate::UnsentAssignmentReview,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceCodexCancellationPage {
    pub project_id: String,
    pub revision: String,
    pub offset: u32,
    pub total: u32,
    pub next_offset: Option<u32>,
    pub entries: Vec<crate::CodexCancellationReview>,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceCodexCancellationDetail {
    pub project_id: String,
    pub revision: String,
    pub review: crate::CodexCancellationReview,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceApiPublicationReviewPage {
    pub revision: String,
    pub offset: u32,
    pub total: u32,
    pub next_offset: Option<u32>,
    pub entries: Vec<crate::ApiPublicationReview>,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceApiPublicationReviewDetail {
    pub revision: String,
    pub review: crate::ApiPublicationReview,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum WorkspaceResult {
    CurrentVerificationRecheck(Option<Box<crate::verification_recheck::RecheckStatus>>),
    VerificationRecheckInputs(Box<WorkspaceRecheckInputs>),
    RegisteredValidator(Box<crate::execution::RegisteredValidator>),
    VerificationRechecks(WorkspaceRecheckPage),
    VerificationRecheck(Box<crate::verification_recheck::RecheckStatus>),
    VerificationRecheckDispositionReview(
        Box<crate::verification_recheck::RecheckDispositionReview>,
    ),
    VerificationRecheckReview(Box<crate::verification_recheck::RecheckReview>),
    ManagementRecordReviews(WorkspaceManagementRecordReviewPage),
    ManagementRecordReview(Box<WorkspaceManagementRecordReviewDetail>),
    Overview(WorkspaceOverview),
    History(WorkspaceHistoryPage),
    HistoryEntry(Box<WorkspaceHistoryDetail>),
    Recovery(WorkspaceRecoveryPage),
    RecoveryDetail(Box<WorkspaceRecoveryDetail>),
    Jobs(WorkspaceJobPage),
    Job(Box<WorkspaceJobDetail>),
    CodexStartups(WorkspaceCodexStartupPage),
    CodexStartup(Box<WorkspaceCodexStartupDetail>),
    ProjectRuns(WorkspaceProjectRunPage),
    ProjectRun(Box<WorkspaceProjectRunDetail>),
    RevisionReviews(WorkspaceRevisionReviewPage),
    RevisionReview(Box<WorkspaceRevisionReviewDetail>),
    UnsentAssignments(WorkspaceUnsentAssignmentPage),
    UnsentAssignment(Box<WorkspaceUnsentAssignmentDetail>),
    CodexCancellations(WorkspaceCodexCancellationPage),
    CodexCancellation(Box<WorkspaceCodexCancellationDetail>),
    ApiPublicationReviews(WorkspaceApiPublicationReviewPage),
    ApiPublicationReview(Box<WorkspaceApiPublicationReviewDetail>),
    VerificationResult(Box<WorkspaceVerificationResultDetail>),
    VerificationResultArtifacts(WorkspaceVerificationArtifactPage),
    VerificationResultStages(WorkspaceVerificationStagePage),
    ManagementDecisions(WorkspaceManagementDecisionPage),
    ManagementDecision(Box<WorkspaceManagementDecisionDetail>),
    ManagementGatewayEvidence(Box<WorkspaceManagementGatewayEvidenceDetail>),
    WorkerObservation(Box<WorkspaceObservationDetail>),
    WorkerObservationEvents(WorkspaceObservationEventPage),
    WorkerObservationIncidents(WorkspaceObservationIncidentPage),
    Plans(PlanPage),
    Plan(Plan),
    Projects(ProjectPage),
    Configurations(WorkspaceConfigurationPage),
    Configuration(Box<WorkspaceConfigurationDetail>),
    Tasks(WorkspaceTaskPage),
    Task(Box<WorkspaceTaskDetail>),
    TaskEvidence(WorkspaceTaskEvidencePage),
    ExecutionReview(Box<WorkspaceExecutionReview>),
}

/// Original approval events, including approvals that have no executor job yet.
/// Pagination revisions bind the original event stream, not a live job status.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceRecheckSummary {
    pub operation_id: String,
    pub original_operation_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub approval_event: crate::execution::EventAnchor,
    pub review_digest: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceRecheckPage {
    pub project_id: String,
    pub revision: String,
    pub offset: u32,
    pub total: u32,
    pub next_offset: Option<u32>,
    pub entries: Vec<WorkspaceRecheckSummary>,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceValidatorSummary {
    pub id: String,
    pub name: String,
    pub digest: String,
}

/// Form inputs only. Original logs and source pins are verified by the separate
/// review query, then verified again when the user approves and starts.
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceRecheckInputs {
    pub original_job: crate::execution::VerificationJob,
    pub revision_digest: String,
    pub original_checks_digest: String,
    pub checks: Vec<crate::execution::AcceptanceCheck>,
    pub max_timeout_seconds: u32,
    pub validators: Vec<WorkspaceValidatorSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceManagementRecordReviewPage {
    pub revision: String,
    pub offset: u32,
    pub total: u32,
    pub next_offset: Option<u32>,
    pub entries: Vec<crate::ManagementRecordReview>,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceManagementRecordReviewDetail {
    pub revision: String,
    pub review: crate::ManagementRecordReview,
}

#[cfg(test)]
mod management_gateway_329_contract_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn workspace_management_gateway_329_query_is_strict_and_separate_from_original_detail() {
        let query = json!({"type":"managementGatewayEvidence","projectId":"project","operationId":"operation","revision":"a".repeat(64)});
        let decoded: WorkspaceQuery = serde_json::from_value(query.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), query);
        let mut extra = query.clone();
        extra["approve"] = json!(true);
        assert!(serde_json::from_value::<WorkspaceQuery>(extra).is_err());
        let mut missing = query;
        missing.as_object_mut().unwrap().remove("revision");
        assert!(serde_json::from_value::<WorkspaceQuery>(missing).is_err());
    }

    #[test]
    fn workspace_management_gateway_329_serializes_finite_safe_observations() {
        let evidence = json!({
            "provider":"a6api", "localId":"operation", "bodyResponseId":"body-id", "status":"linked",
            "headers":{"status":429,"requestId":{"kind":"accepted","value":"gateway-id"}}
        });
        let decoded: WorkspaceGatewayEvidence = serde_json::from_value(evidence.clone()).unwrap();
        assert_eq!(serde_json::to_value(decoded).unwrap(), evidence);
        for (name, value) in [
            ("status", json!("billed")),
            ("provider", json!("untrusted")),
        ] {
            let mut changed = evidence.clone();
            changed[name] = value;
            assert!(serde_json::from_value::<WorkspaceGatewayEvidence>(changed).is_err());
        }
        let mut changed = evidence;
        changed["headers"]["requestId"] =
            json!({"kind":"rejected","reason":"credentialReflection","raw":"must-not-escape"});
        assert!(serde_json::from_value::<WorkspaceGatewayEvidence>(changed).is_err());
        let non_api = json!({"projectId":"project","operationId":"operation","revision":"a".repeat(64),"evidenceDigest":"b".repeat(64),"observation":null});
        let decoded: WorkspaceManagementGatewayEvidenceDetail =
            serde_json::from_value(non_api.clone()).unwrap();
        assert!(decoded.observation.is_none());
        assert_eq!(serde_json::to_value(decoded).unwrap(), non_api);
    }
}

#[cfg(test)]
#[path = "workspace_gateway_legacy_330_tests.rs"]
mod management_gateway_330_legacy_tests;
