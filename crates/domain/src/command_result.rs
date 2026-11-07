//! Successful command acknowledgement is independent of a view refresh.
use crate::{Command, Snapshot};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandReceipt {
    /// The correlated local command returned successfully. This is not proof
    /// that any background worker finished, nor a durable request receipt log.
    pub request_id: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum CommandResult {
    Receipt(CommandReceipt),
    Snapshot(Box<Snapshot>),
}
impl Command {
    pub fn supports_receipt(&self) -> bool {
        match self {
            Self::ReadManagementCounts { .. }
            | Self::ReadApiGenerations { .. }
            | Self::ReviewApiGenerationResolution { .. }
            | Self::ConfirmApiGenerationResolution { .. }
            | Self::ReadApiGenerationConfirmation { .. }
            | Self::ReadApiCounts { .. }
            | Self::ReviewApiCountResolution { .. }
            | Self::ConfirmApiCountResolution { .. }
            | Self::ReadApiCountConfirmation { .. }
            | Self::ReviewManagementCountResolution { .. }
            | Self::ConfirmManagementCountResolution { .. }
            | Self::ReadManagementCountConfirmation { .. }
            | Self::ReadHistoricalSources { .. }
            | Self::ReadHistoricalAccounting { .. }
            | Self::ReadHistoricalConfirmation { .. }
            | Self::ReviewHistoricalCharge { .. }
            | Self::ConfirmHistoricalCharge { .. }
            | Self::ReadOmpFiles { .. }
            | Self::RestoreOmpFile { .. }
            | Self::ReadWorkerObservation { .. }
            | Self::Snapshot
            | Self::ReadCodexStartup { .. }
            | Self::ReadManagementPage { .. }
            | Self::ReadManagementDetail { .. }
            | Self::ReadVerificationDetail { .. } => false,
            Self::ReviewVerificationRecheck { .. }
            | Self::ReadVerificationRecheck { .. }
            | Self::ReviewVerificationRecheckDisposition { .. } => false,
            Self::BlindReview { .. }
            | Self::Baseline { .. }
            | Self::TeamTiming { .. }
            | Self::PrepareDesktopTask { .. }
            | Self::SendDesktopTask { .. }
            | Self::CancelDesktopTask { .. }
            | Self::RetireUnknownDesktop { .. }
            | Self::ReadDesktopTask { .. }
            | Self::PrepareDesktopTrial { .. }
            | Self::SendDesktopTrial { .. }
            | Self::ReadDesktopTrial { .. }
            | Self::DiscoverDesktop
            | Self::BindDesktopSession { .. }
            | Self::InspectDesktop { .. }
            | Self::BindDesktop { .. }
            | Self::ReviewManagementRecord { .. }
            | Self::ReviewStartupClosure { .. }
            | Self::ReviewStartupIntent { .. }
            | Self::ReviewDesktopStopRecord { .. }
            | Self::ReviewApiPublication { .. }
            | Self::AcknowledgeOmpStopIssue { .. }
            | Self::CancelOmp { .. }
            | Self::CancelApi { .. }
            | Self::RecoverApi { .. }
            | Self::AbandonApiStartup { .. }
            | Self::RetireUnknownApi { .. }
            | Self::RecoverOmp { .. }
            | Self::AbandonOmpStartup { .. }
            | Self::CancelCodex { .. }
            | Self::AbandonCodexStartup { .. }
            | Self::RecoverVerification { .. }
            | Self::RetryVerificationStart { .. }
            | Self::RetireVerificationRecheck { .. }
            | Self::ApproveVerificationRecheck { .. }
            | Self::StartVerificationRecheck { .. }
            | Self::CancelVerificationRecheck { .. }
            | Self::RecoverVerificationRecheck { .. }
            | Self::RequestProjectRevision { .. }
            | Self::RetireSubmittedPlan { .. }
            | Self::StartProjectRun { .. }
            | Self::PauseProjectRun { .. }
            | Self::DiscardManagementDecision { .. }
            | Self::AnswerManagementWait { .. }
            | Self::CloseAdviceTracking { .. }
            | Self::AbandonManagementApplication { .. }
            | Self::ConfigureProject { .. }
            | Self::ProvisionWorkerAccounts { .. }
            | Self::OpenWorkerLogin { .. }
            | Self::RequestChiefPlan { .. }
            | Self::RequestDocumentRefinement { .. }
            | Self::AuthorizePlanningBudget { .. }
            | Self::SetOpenAiKey { .. }
            | Self::RemoveOpenAiKey
            | Self::SetProviderKey { .. }
            | Self::RemoveProviderKey { .. }
            | Self::CheckOpenAi
            | Self::CheckProviderModels { .. }
            | Self::CheckCodex { .. }
            | Self::ReconcileCodex { .. }
            | Self::AbandonUnsentAttempt { .. }
            | Self::RegisterValidator { .. }
            | Self::CreatePlan { .. }
            | Self::ApprovePlan { .. }
            | Self::RegisterProject { .. }
            | Self::ApproveRevision { .. }
            | Self::ApproveRevisionWithWorkerAccounts { .. }
            | Self::CancelVerification { .. }
            | Self::AbandonVerification { .. } => true,
        }
    }
}
