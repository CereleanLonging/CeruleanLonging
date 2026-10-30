//! Explicit local shutdown with unresolved work preserved, never cancellation.
use crate::diagnostic_service::ServiceHost;
use cerulean_domain::execution::{digest, require};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PreservationReview {
    pub digest: String,
    pub reviewed_at: String,
    pub snapshot_digest: String,
    pub evidence_digest: String,
    pub core_version: String,
    pub core_sha256: String,
    pub server_pid: u32,
    pub data_root: String,
    pub owner_sid: String,
    pub unknown_tasks: usize,
    pub submitted_tasks: usize,
    pub pending_external_decisions: usize,
    pub requires_reconciliation: bool,
}

pub(crate) fn review(
    host: &ServiceHost,
    reviewed_at: String,
) -> Result<PreservationReview, String> {
    let ServiceHost::Ready(host) = host else {
        return Err("当前核心处于导入或诊断状态，不能使用保存未决工作入口".into());
    };
    // This path is read-only. In particular it must not poll a runnable scheduler
    // in order to make a shutdown request appear quiescent.
    let (snapshot, evidence_digest) = host.preserved_shutdown_evidence()?;
    let tasks = snapshot
        .workflows
        .iter()
        .flat_map(|w| w.state.tasks.values());
    use cerulean_domain::workflow::TaskState;
    let unknown_tasks = tasks
        .clone()
        .filter(|t| t.state == TaskState::Unknown)
        .count();
    let submitted_tasks = tasks.filter(|t| t.state == TaskState::Submitted).count();
    let mut result = PreservationReview {
        digest: String::new(),
        reviewed_at,
        snapshot_digest: digest(&snapshot)?,
        evidence_digest,
        core_version: env!("CARGO_PKG_VERSION").into(),
        core_sha256: crate::upgrade::runtime_hash()?,
        server_pid: std::process::id(),
        data_root: host.directory().to_string_lossy().into_owned(),
        owner_sid: crate::ipc::current_sid().map_err(|e| e.to_string())?,
        unknown_tasks,
        submitted_tasks,
        pending_external_decisions: snapshot
            .management_decisions
            .iter()
            .filter(|d| d.state == "awaitingExternal")
            .count(),
        requires_reconciliation: true,
    };
    require(
        unknown_tasks + submitted_tasks > 0,
        "没有未决编程或提交记录，请使用正常关闭",
    )?;
    result.digest = digest(&result)?;
    Ok(result)
}
