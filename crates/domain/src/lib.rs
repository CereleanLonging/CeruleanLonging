pub mod baseline_product;
pub mod blind_review;
pub mod omp_jobs;
pub mod omp_restore;
pub mod team_timing;
pub mod timezone;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;

pub mod api_count_resolution;
pub mod api_generation_resolution;
pub mod api_jobs;
pub mod api_worker;
pub mod command_result;
pub mod count_resolution;
pub mod credentials;
pub mod currency_budget;
pub mod desktop_sessions;
pub mod execution;
pub mod historical_money;
pub mod management;
pub mod management_ingress;
pub mod project_setup;
pub mod secret;
pub mod supervision;
pub mod verification_detail;
pub mod verification_recheck;
pub mod worker_observation;
pub mod workflow;
pub mod workflow_template;
pub mod workspace;

pub const PROTOCOL_VERSION: u32 = 1;
pub const PRODUCT_NAME: &str = "CeruleanLonging";

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub id: String,
    pub revision: u32,
    pub title: String,
    pub tasks: Vec<String>,
    pub digest: String,
    pub created_at: String,
    pub approved: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AdapterInfo {
    pub id: String,
    pub name: String,
    pub transport: String,
    pub status: String,
    pub note: String,
    pub capabilities: Vec<execution::AdapterCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EventSummary {
    pub sequence: u32,
    pub kind: String,
    pub at: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    #[serde(default)]
    pub blind_reviews: Vec<blind_review::BlindReviewView>,
    #[serde(default)]
    pub worker_account_setups: Vec<project_setup::WorkerAccountSetupView>,
    #[serde(default)]
    pub team_import: Option<team_timing::TeamImportView>,
    #[serde(default)]
    pub baselines: Vec<baseline_product::BaselineView>,
    #[serde(default)]
    pub baseline_review: Option<baseline_product::BaselineReview>,
    #[serde(default)]
    pub team_timelines: Vec<team_timing::TeamTimingView>,
    #[serde(default)]
    pub desktop_access: Option<desktop_sessions::DesktopAccess>,
    #[serde(default)]
    pub api_recovery: Option<api_jobs::ApiRecovery>,
    #[serde(default)]
    pub api_publication_reviews: Vec<ApiPublicationReview>,
    #[serde(default)]
    pub api_jobs: Vec<api_jobs::ApiJob>,
    #[serde(default)]
    pub management_record_reviews: Vec<ManagementRecordReview>,
    #[serde(default)]
    pub startup_closure_reviews: Vec<StartupClosureReview>,
    #[serde(default)]
    pub startup_reservations: Vec<StartupReservation>,
    #[serde(default)]
    pub core_diagnostic: Option<CoreDiagnostic>,
    pub product: String,
    pub protocol_version: u32,
    pub data_directory: String,
    pub plans: Vec<Plan>,
    pub events: Vec<EventSummary>,
    pub adapters: Vec<AdapterInfo>,
    #[serde(default)]
    pub workflows: Vec<execution::ExecutionSnapshot>,
    #[serde(default)]
    pub verification_jobs: Vec<execution::VerificationJob>,
    #[serde(default)]
    pub verification_detail: Option<verification_detail::VerificationDetail>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub verification_recheck_review: Option<verification_recheck::RecheckReview>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub verification_recheck_status: Option<verification_recheck::RecheckStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub verification_recheck_disposition_review:
        Option<verification_recheck::RecheckDispositionReview>,
    #[serde(default)]
    pub omp_files: Option<omp_restore::OmpFilesView>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub omp_restore: Option<omp_restore::OmpRestoreOutcome>,
    #[serde(default)]
    pub omp_jobs: Vec<omp_jobs::OmpJob>,
    #[serde(default)]
    pub omp_stop_issues: Vec<omp_jobs::OmpStopIssue>,
    #[serde(default)]
    pub worker_observation: Option<worker_observation::WorkerObservationView>,
    #[serde(default)]
    pub validators: Vec<execution::RegisteredValidator>,
    #[serde(default)]
    pub codex_connection: Option<CodexConnectionCheck>,
    #[serde(default)]
    pub codex_jobs: Vec<CodexJob>,
    #[serde(default)]
    pub codex_cancellations: Vec<CodexCancellationReview>,
    #[serde(default)]
    pub codex_startups: Vec<CodexStartupSummary>,
    #[serde(default)]
    pub codex_startup_detail: Option<CodexStartupView>,
    #[serde(default)]
    pub unsent_assignments: Vec<UnsentAssignmentReview>,
    #[serde(default)]
    pub openai_connection: OpenAiConnection,
    #[serde(default)]
    pub provider_credentials: Vec<credentials::ProviderCredentialStatus>,
    #[serde(default)]
    pub provider_connections: Vec<credentials::ProviderConnection>,
    #[serde(default)]
    pub project_setups: Vec<project_setup::SavedProjectSetup>,
    #[serde(default)]
    pub currency_budgets: Vec<currency_budget::ProjectCurrencyBudget>,
    /// None means the historical inventory is unavailable, not empty.
    #[serde(default)]
    pub currency_history: Option<Vec<currency_budget::CurrencyHistoryGap>>,
    #[serde(default)]
    pub currency_accounted: Option<Vec<historical_money::HistoricalAccounting>>,
    #[serde(default)]
    pub historical_charge_review: Option<historical_money::HistoricalChargeReview>,
    #[serde(default)]
    pub historical_sources: Option<historical_money::HistoricalSourcePage>,
    #[serde(default)]
    pub historical_accounting: Option<historical_money::HistoricalAccountingPage>,
    #[serde(default)]
    pub historical_confirmation: Option<historical_money::HistoricalConfirmationLookup>,
    #[serde(default)]
    pub management_decisions: Vec<project_setup::ManagementSummary>,
    #[serde(default)]
    pub management_history: project_setup::ManagementHistory,
    #[serde(default)]
    pub management_detail: Option<project_setup::ManagementView>,
    #[serde(default)]
    pub project_runs: Vec<project_setup::ProjectRun>,
    #[serde(default)]
    pub project_revisions: Vec<project_setup::ProjectRevisionReview>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CoreDiagnostic {
    #[serde(default)]
    pub desktop_files: Vec<DiagnosticFile>,
    #[serde(default)]
    pub api_files: Vec<DiagnosticFile>,
    pub message: String,
    pub detected_at: String,
    pub startup_files: Vec<DiagnosticFile>,
    #[serde(default)]
    pub management_files: Vec<DiagnosticFile>,
    pub inventory_limited: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ApiPublicationReview {
    pub relative_path: String,
    pub evidence_digest: String,
    pub reason: String,
    pub at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct ManagementRecordReview {
    pub relative_path: String,
    pub operation_id: String,
    pub sequence: u8,
    pub project_id: Option<String>,
    pub reason: String,
    pub at: String,
}
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticFile {
    #[serde(default)]
    pub review_kind: Option<String>,
    #[serde(default)]
    pub review_digest: Option<String>,
    #[serde(default)]
    pub reviewed: bool,
    pub relative_path: String,
    pub state: String,
    pub bytes: Option<u32>,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct StartupReservation {
    pub executor: String,
    pub project_id: String,
    pub attempt_id: String,
    pub reason: String,
    pub at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct StartupClosureReview {
    pub relative_path: String,
    pub executor: String,
    pub project_id: String,
    pub attempt_id: String,
    pub intent_hash: String,
    pub reason: String,
    pub at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct OpenAiConnection {
    pub key_configured: bool,
    pub state: String,
    pub checked_at: Option<String>,
    pub models: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodexJob {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub management_operation: Option<String>,
    pub project_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub revision_digest: String,
    pub worker: execution::WorkerSelection,
    pub state: CodexJobState,
    pub started_at: String,
    pub updated_at: String,
    pub message: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum CodexJobState {
    Starting,
    Executing,
    CancelRequested,
    Cancelling,
    Cancelled,
    Reconciling,
    Submitted,
    Unknown,
    Failed,
    Abandoned,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CodexCancellationReview {
    pub project_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub evidence_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UnsentAssignmentReview {
    pub project_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub evidence_digest: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CodexStartupSummary {
    pub project_id: String,
    pub task_id: String,
    pub attempt_id: String,
    pub management_operation: Option<String>,
    pub state: String,
    pub intent_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CodexStartupView {
    pub summary: CodexStartupSummary,
    pub workspace: String,
    pub revision_digest: String,
    pub worker_model: String,
    pub started_at: String,
    pub protocol: Option<String>,
    pub evidence_digest: Option<String>,
    pub blocked_reason: Option<String>,
    pub reason: Option<String>,
    pub workflow_sequence: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct CodexConnectionCheck {
    pub executable: String,
    pub state: String,
    pub checked_at: String,
    pub server_version: Option<String>,
    pub account_type: Option<String>,
    pub models: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Command {
    BlindReview {
        action: blind_review::BlindReviewAction,
    },
    Baseline {
        action: baseline_product::BaselineAction,
    },
    TeamTiming {
        action: team_timing::TeamTimingAction,
    },
    ReadApiGenerations {
        offset: u32,
        revision: Option<String>,
    },
    ReviewApiGenerationResolution {
        input: api_generation_resolution::ApiGenerationResolutionInput,
    },
    ConfirmApiGenerationResolution {
        input: api_generation_resolution::ApiGenerationResolutionInput,
        review_digest: String,
    },
    ReadApiGenerationConfirmation {
        project_id: String,
        request_id: String,
        review_digest: String,
    },
    ReadApiCounts {
        offset: u32,
        revision: Option<String>,
    },
    ReviewApiCountResolution {
        input: api_count_resolution::ApiCountResolutionInput,
    },
    ConfirmApiCountResolution {
        input: api_count_resolution::ApiCountResolutionInput,
        review_digest: String,
    },
    ReadApiCountConfirmation {
        project_id: String,
        request_id: String,
        review_digest: String,
    },
    ReadManagementCounts {
        offset: u32,
        revision: Option<String>,
    },
    ReviewManagementCountResolution {
        input: count_resolution::CountResolutionInput,
    },
    ConfirmManagementCountResolution {
        input: count_resolution::CountResolutionInput,
        review_digest: String,
    },
    ReadManagementCountConfirmation {
        project_id: String,
        operation_id: String,
        aspect: count_resolution::CountResolutionAspect,
        review_digest: String,
    },
    ReadHistoricalSources {
        offset: u32,
        revision: Option<String>,
    },
    ReadHistoricalAccounting {
        offset: u32,
        revision: Option<String>,
    },
    ReadHistoricalConfirmation {
        project_id: String,
        scope: historical_money::HistoricalScope,
        review_digest: String,
    },
    ReviewHistoricalCharge {
        input: historical_money::HistoricalChargeInput,
    },
    ConfirmHistoricalCharge {
        input: historical_money::HistoricalChargeInput,
        review_digest: String,
    },
    PrepareDesktopTask {
        project_id: String,
        task_id: String,
        attempt_id: String,
    },
    SendDesktopTask {
        review_digest: String,
    },
    CancelDesktopTask {
        project_id: String,
        attempt_id: String,
        review_digest: String,
        reason: String,
    },
    RetireUnknownDesktop {
        project_id: String,
        attempt_id: String,
        evidence_digest: String,
        reason: String,
    },
    ReadDesktopTask {
        project_id: String,
        attempt_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reader_sha256: Option<String>,
    },
    PrepareDesktopTrial {
        binding_id: String,
    },
    SendDesktopTrial {
        review_digest: String,
    },
    ReadDesktopTrial {
        trial_id: String,
        reader_sha256: String,
    },
    DiscoverDesktop,
    InspectDesktop {
        window: desktop_sessions::DesktopWindow,
    },
    BindDesktop {
        project_id: String,
        observation_digest: String,
        label: String,
    },
    ReviewManagementRecord {
        relative_path: String,
        evidence_digest: String,
        reason: String,
    },
    ReviewStartupClosure {
        relative_path: String,
        evidence_digest: String,
        reason: String,
    },
    ReviewStartupIntent {
        relative_path: String,
        evidence_digest: String,
        reason: String,
    },
    ReviewDesktopStopRecord {
        relative_path: String,
        evidence_digest: String,
        reason: String,
    },
    ReviewApiPublication {
        relative_path: String,
        evidence_digest: String,
        reason: String,
    },
    AcknowledgeOmpStopIssue {
        project_id: String,
        attempt_id: String,
        evidence_digest: String,
        reason: String,
    },
    CancelOmp {
        project_id: String,
        attempt_id: String,
        reason: String,
    },
    CancelApi {
        project_id: String,
        attempt_id: String,
        reason: String,
    },
    RecoverApi {
        project_id: String,
        attempt_id: String,
    },
    AbandonApiStartup {
        project_id: String,
        attempt_id: String,
        evidence_digest: String,
        reason: String,
    },
    RetireUnknownApi {
        project_id: String,
        attempt_id: String,
        evidence_digest: String,
        reason: String,
    },
    RecoverOmp {
        project_id: String,
        attempt_id: String,
    },
    AbandonOmpStartup {
        project_id: String,
        attempt_id: String,
        evidence_digest: String,
        reason: String,
    },
    ReadOmpFiles {
        project_id: String,
        task_id: String,
        attempt_id: String,
        offset: u32,
        call_id: Option<String>,
    },
    RestoreOmpFile {
        project_id: String,
        task_id: String,
        attempt_id: String,
        call_id: String,
        evidence_digest: String,
    },
    ReadWorkerObservation {
        project_id: String,
        attempt_id: String,
    },
    Snapshot,
    CancelCodex {
        project_id: String,
        attempt_id: String,
        evidence_digest: String,
        reason: String,
    },
    ReadCodexStartup {
        project_id: String,
        attempt_id: String,
    },
    AbandonCodexStartup {
        project_id: String,
        attempt_id: String,
        evidence_digest: String,
        reason: String,
    },
    ReadManagementPage {
        project_id: Option<String>,
        offset: u32,
        revision: Option<String>,
    },
    ReadManagementDetail {
        operation_id: String,
    },
    ReadVerificationDetail {
        project_id: String,
        operation_id: String,
    },
    ReviewVerificationRecheckDisposition {
        project_id: String,
        operation_id: String,
        reason: String,
    },
    RetireVerificationRecheck {
        review: Box<verification_recheck::RecheckDispositionReview>,
        event_operation_id: String,
    },
    ReviewVerificationRecheck {
        input: Box<verification_recheck::RecheckRequest>,
    },
    ReadVerificationRecheck {
        project_id: String,
        operation_id: String,
    },
    ApproveVerificationRecheck {
        review: Box<verification_recheck::RecheckReview>,
        approval_operation_id: String,
        operation_id: String,
    },
    StartVerificationRecheck {
        project_id: String,
        operation_id: String,
    },
    CancelVerificationRecheck {
        project_id: String,
        operation_id: String,
    },
    RecoverVerificationRecheck {
        project_id: String,
        operation_id: String,
    },
    RecoverVerification {
        project_id: String,
        operation_id: String,
        evidence_digest: String,
    },
    RequestProjectRevision {
        project_id: String,
        evidence_digest: String,
        reason: String,
    },
    RetireSubmittedPlan {
        project_id: String,
        evidence_digest: String,
        reason: String,
    },
    StartProjectRun {
        project_id: String,
        revision_digest: String,
        setup_digest: String,
    },
    PauseProjectRun {
        run_id: String,
    },
    DiscardManagementDecision {
        operation_id: String,
        evidence_digest: String,
        reason: String,
    },
    AnswerManagementWait {
        operation_id: String,
        evidence_digest: String,
        answer: String,
    },
    CloseAdviceTracking {
        operation_id: String,
        evidence_digest: String,
        reason: String,
    },
    AbandonManagementApplication {
        operation_id: String,
        evidence_digest: String,
        reason: String,
    },
    ConfigureProject {
        draft: Box<project_setup::ProjectSetupDraft>,
    },
    ProvisionWorkerAccounts {
        project_id: String,
        setup_digest: String,
    },
    OpenWorkerLogin {
        project_id: String,
        setup_digest: String,
        profile_id: Option<String>,
    },
    RequestDocumentRefinement {
        input: management::DocumentRefinementRequest,
    },
    RequestChiefPlan {
        project_id: String,
        setup_digest: String,
    },
    AuthorizePlanningBudget {
        project_id: String,
        setup_digest: String,
    },
    SetOpenAiKey {
        key: secret::SecretInput,
    },
    RemoveOpenAiKey,
    SetProviderKey {
        provider: credentials::CredentialProvider,
        key: secret::SecretInput,
    },
    RemoveProviderKey {
        provider: credentials::CredentialProvider,
    },
    CheckOpenAi,
    CheckProviderModels {
        target: credentials::ProviderEndpoint,
    },
    CheckCodex {
        executable: String,
    },
    ReconcileCodex {
        project_id: String,
        attempt_id: String,
    },
    AbandonUnsentAttempt {
        project_id: String,
        task_id: String,
        attempt_id: String,
        evidence_digest: String,
        reason: String,
    },
    RegisterValidator {
        id: String,
        name: String,
        executable: String,
        inputs: Vec<String>,
        environment: std::collections::BTreeMap<String, String>,
    },
    CreatePlan {
        title: String,
        tasks: Vec<String>,
    },
    ApprovePlan {
        id: String,
        digest: String,
    },
    RegisterProject {
        project: execution::Project,
    },
    ApproveRevision {
        project_id: String,
        digest: String,
    },
    CancelVerification {
        project_id: String,
        operation_id: String,
    },
    AbandonVerification {
        project_id: String,
        operation_id: String,
        digest: String,
        reason: String,
    },
}

#[derive(Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub id: String,
    pub protocol_version: u32,
    pub command: Command,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct ApiError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize, TS)]
pub struct Response {
    pub id: String,
    pub result: Option<Snapshot>,
    pub error: Option<ApiError>,
}

pub fn content_digest(title: &str, tasks: &[String]) -> String {
    let bytes = serde_json::to_vec(&(title, tasks)).expect("string serialization");
    format!("{:x}", Sha256::digest(bytes))
}

pub fn validate_plan(title: &str, tasks: &[String]) -> Result<(), String> {
    if title.trim().is_empty() || title.len() > 600 {
        return Err("计划标题不能为空，且不得超过 600 字节".into());
    }
    if tasks.is_empty()
        || tasks
            .iter()
            .any(|t| t.trim().is_empty() || t.contains('\0'))
    {
        return Err("计划须包含非空任务，且不能含 NUL 字符".into());
    }
    Ok(())
}
