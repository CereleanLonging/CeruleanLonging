use cerulean_domain::*;
mod blind_review;
mod blind_review_guard;
pub const DATA_READER_GENERATION: u32 = blind_review_guard::GENERATION;
#[cfg(windows)]
pub mod windows_worker_account;
#[cfg(all(test, windows))]
mod workload_fixture_252;
/// Explicitly reviewed schema-1 backup writers; never infer future compatibility.
pub const BACKUP_WRITER_VERSIONS: &[&str] = &[
    "0.1.0", "0.1.1", "0.1.2", "0.1.3", "0.1.4", "0.1.5", "0.1.6", "0.1.7", "0.1.8", "0.1.9",
    "0.1.10", "0.1.11", "0.1.12", "0.1.13", "0.1.14", "0.1.15", "0.1.16", "0.1.17", "0.1.18",
    "0.1.19", "0.1.20", "0.1.21", "0.1.22", "0.1.23", "0.1.24", "0.1.25", "0.1.26", "0.1.27",
    "0.1.28", "0.1.29", "0.1.30", "0.1.31", "0.1.32", "0.1.33", "0.1.34", "0.1.35",
];
#[cfg(windows)]
mod acceptance_scope;
mod advice_closure;
pub mod advice_delivery;
mod api_advice_link;
pub mod api_call_journal;
pub mod api_count_resolution;
#[cfg(windows)]
mod api_currency;
#[cfg(windows)]
pub mod api_execution;
pub mod api_generation_resolution;
#[cfg(windows)]
mod api_publication;
#[cfg(windows)]
mod api_startup;
#[cfg(windows)]
mod api_tool_checkpoint;
pub mod gateway_response_evidence;
#[cfg(windows)]
pub use api_tool_checkpoint::ApiToolEvidence;
pub mod artifacts;
#[cfg(windows)]
mod async_api;
#[cfg(windows)]
mod async_codex;
#[cfg(windows)]
mod async_omp;
#[cfg(windows)]
mod async_verification;
#[cfg(windows)]
mod async_verification_recheck;
#[cfg(windows)]
pub mod backup;
#[cfg(windows)]
pub mod backup_recovery;
#[cfg(windows)]
pub mod baseline_materials;
#[cfg(windows)]
pub mod baseline_runner;
#[cfg(windows)]
pub mod codex_execution;
mod codex_workflow_access;
pub mod count_resolution;
#[cfg(windows)]
mod credential_vault;
pub mod currency_api_guard;
pub mod currency_api_resolution_guard;
pub(crate) mod currency_approval;
pub mod currency_budget;
pub mod currency_count_guard;
pub mod currency_count_resolution_guard;
pub mod currency_execution_guard;
pub mod currency_guard;
pub mod currency_scoped_count_guard;
pub mod currency_scoped_execution_guard;
pub mod currency_scoped_guard;
pub mod data_format;
pub mod derived_index;
#[cfg(windows)]
pub mod desktop_execution;
#[cfg(windows)]
mod diagnostic_service;
mod directory_publication;
#[cfg(windows)]
pub mod efficiency_evidence;
pub mod experiment_guard;
#[cfg(windows)]
pub mod git_workspaces;
pub mod handoff;
pub mod historical_confirmation;
pub mod host;
#[cfg(windows)]
pub mod ipc;
#[cfg(windows)]
pub mod maintenance_interlock;
#[cfg(windows)]
mod managed_assignment;
pub mod management_execution;
mod management_review;
pub mod management_source_guard;
#[cfg(windows)]
mod omp_advice_journal;
pub mod omp_execution;
#[cfg(windows)]
pub mod omp_file_tools;
pub mod omp_journal;
#[cfg(windows)]
pub mod omp_process;
#[cfg(windows)]
mod omp_stop_review;
pub mod omp_tool_checkpoint;
pub mod omp_tool_journal;
mod omp_workflow_access;
#[cfg(windows)]
pub mod portable_evidence;
#[cfg(windows)]
pub mod preserve_shutdown;
mod project_setup;
pub mod reader_generation;
#[cfg(windows)]
mod recheck_disposition_evidence;
pub mod rejected_usage_guard;
pub mod relay_guard;
mod replay_checkpoint;
mod role_workspace;
mod run_control;
pub mod scheduler;
#[cfg(windows)]
pub mod source_snapshot;
mod startup_closure_review;
mod startup_journal;
mod startup_review;
#[cfg(windows)]
mod supervised_worker;
mod supervision_scope;
#[cfg(windows)]
pub mod team_import_retention;
#[cfg(windows)]
mod team_import_service;
#[cfg(windows)]
pub mod team_materials;
#[cfg(windows)]
pub mod upgrade;
mod validator_catalog;
mod verification_detail;
mod verification_recheck_evidence;
pub mod verification_recheck_guard;
#[cfg(windows)]
mod verification_reconciliation;
#[cfg(windows)]
pub mod verifier;
mod worker_directories;
#[cfg(windows)]
mod worker_observation;
#[cfg(windows)]
mod workflow_prefix;
mod workspace_history;
mod workspace_read;
#[cfg(all(test, windows))]
#[path = "git_workspaces/validation_fixture.rs"]
mod worktree_validation_fixture;
use chrono::Utc;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum Change {
    PlanCreated { plan: Plan },
    PlanApproved { id: String, digest: String },
}
#[derive(Debug, Serialize, Deserialize)]
struct Body {
    sequence: u32,
    previous_hash: String,
    at: String,
    change: Change,
}
#[derive(Debug, Serialize, Deserialize)]
struct Event {
    body: Body,
    hash: String,
}

pub struct Store {
    root: PathBuf,
    _lock: File,
    plans: Vec<Plan>,
    events: Vec<EventSummary>,
    last_hash: String,
    poisoned: bool,
    index: derived_index::Cache,
    indexed_events: Vec<derived_index::IndexedEvent>,
    checkpoint: replay_checkpoint::Tracker,
}
impl Drop for Store {
    fn drop(&mut self) {
        // Join cache I/O while the authoritative writer lock is still held.
        self.index.shutdown();
    }
}
fn hash(body: &Body) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(body).map_err(|e| e.to_string())?)
    ))
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, String> {
        #[cfg(windows)]
        crate::backup_recovery::ensure_not_recovering(path)?;
        #[cfg(windows)]
        crate::upgrade::ensure_not_held(path)?;
        data_format::status(path)?;
        reader_generation::reject_unprotected(path)?;
        fs::create_dir_all(path.join("events")).map_err(|e| e.to_string())?;
        let root = fs::canonicalize(path).map_err(|e| e.to_string())?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join("writer.lock"))
            .map_err(|e| e.to_string())?;
        lock.try_lock_exclusive()
            .map_err(|_| "数据目录已被另一个核心进程占用".to_string())?;
        #[cfg(windows)]
        crate::backup_recovery::ensure_not_recovering(&root)?;
        #[cfg(windows)]
        crate::upgrade::ensure_not_held(&root)?;
        data_format::status(&root)?;
        reader_generation::reject_unprotected(&root)?;
        let mut store = Self {
            root,
            _lock: lock,
            plans: vec![],
            events: vec![],
            last_hash: "genesis".into(),
            poisoned: false,
            index: derived_index::Cache::default(),
            indexed_events: vec![],
            checkpoint: replay_checkpoint::Tracker::default(),
        };
        let mut files = fs::read_dir(store.root.join("events"))
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        files.sort_by_key(|e| e.file_name());
        let mut paths = Vec::new();
        for f in files {
            if f.file_name() == "00000000.json"
                || f.file_name() == reader_generation::FILE_NAME
                || f.file_name() == currency_guard::FILE_NAME
                || f.file_name() == currency_execution_guard::FILE_NAME
                || f.file_name() == currency_count_guard::FILE_NAME
                || f.file_name() == currency_count_resolution_guard::FILE_NAME
                || f.file_name() == currency_scoped_guard::FILE_NAME
                || f.file_name() == currency_scoped_count_guard::FILE_NAME
                || f.file_name() == currency_scoped_execution_guard::FILE_NAME
                || f.file_name() == currency_api_guard::FILE_NAME
                || f.file_name() == currency_api_resolution_guard::FILE_NAME
                || f.file_name() == management_source_guard::FILE_NAME
                || f.file_name() == relay_guard::FILE_NAME
                || f.file_name() == rejected_usage_guard::FILE_NAME
                || f.file_name() == verification_recheck_guard::FILE_NAME
                || f.file_name() == blind_review_guard::FILE_NAME
                || f.file_name() == experiment_guard::FILE_NAME
                || f.file_name() == experiment_guard::LEGACY_FILE_NAME
            {
                // Validated under the authoritative writer lock above.
                continue;
            }
            if f.path().extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            paths.push(f.path());
        }
        if let Some(cached) = replay_checkpoint::load(&store.root, "store", "", &paths) {
            if let replay_checkpoint::Payload::Store {
                plans,
                events,
                indexed,
                last_hash,
            } = cached.payload
            {
                if events.len() == cached.proofs.len() && indexed.len() == events.len() {
                    store.plans = plans;
                    store.events = events;
                    store.indexed_events = indexed;
                    store.last_hash = last_hash;
                    store.checkpoint.restore(cached.proofs);
                }
            }
        }
        for path in paths.into_iter().skip(store.checkpoint.restored) {
            let bytes = fs::read(&path).map_err(|e| e.to_string())?;
            let e: Event = serde_json::from_slice(&bytes).map_err(|e| format!("事件损坏: {e}"))?;
            if path.file_name().unwrap_or_default().to_string_lossy()
                != format!("{:08}.json", e.body.sequence)
                || e.body.sequence != store.events.len() as u32 + 1
                || e.body.previous_hash != store.last_hash
                || hash(&e.body)? != e.hash
            {
                return Err("事件序列或哈希链不匹配，已停止加载".into());
            }
            store.apply(&e.body)?;
            store.indexed_events.push(Self::index_event(&e));
            store
                .checkpoint
                .record(format!("{:08}.json", e.body.sequence), &bytes);
            store.last_hash = e.hash;
        }
        store.refresh_index();
        Ok(store)
    }
    fn index_event(event: &Event) -> derived_index::IndexedEvent {
        derived_index::IndexedEvent {
            sequence: event.body.sequence as u64,
            operation_id: event.body.sequence.to_string(),
            kind: match event.body.change {
                Change::PlanCreated { .. } => "planCreated",
                Change::PlanApproved { .. } => "planApproved",
            }
            .into(),
            received_at: event.body.at.clone(),
            hash: event.hash.clone(),
            task: None,
            attempt: None,
        }
    }
    fn index_head(&self) -> derived_index::Head {
        derived_index::Head {
            ledger: "store".into(),
            count: self.events.len(),
            hash: self.last_hash.clone(),
            bindings: String::new(),
        }
    }
    fn refresh_index(&mut self) {
        self.index.prepare_refresh();
        if let Some(proofs) = self.checkpoint.due() {
            self.index.checkpoint(&replay_checkpoint::PublicationRef {
                kind: "store",
                bindings: "",
                proofs: &proofs,
                payload: replay_checkpoint::PayloadRef::Store {
                    plans: &self.plans,
                    events: &self.events,
                    indexed: &self.indexed_events,
                    last_hash: &self.last_hash,
                },
            });
        }
        let head = self.index_head();
        let data = derived_index::DataRef {
            head: &head,
            events: &self.indexed_events,
            plans: &self.plans,
            summaries: &self.events,
            tasks: &[],
            daily: &[],
        };
        self.index.refresh(&self.root, &data);
    }
    /// Cache diagnostics do not change the success of a committed command.
    pub fn index_error(&self) -> Option<String> {
        self.index
            .read(&self.root, &self.index_head())
            .err()
            .or_else(|| self.index.error())
    }
    /// Optional readiness barrier for maintenance/diagnostics, never for dispatch.
    pub fn wait_for_index(&self, timeout: std::time::Duration) -> Result<(), String> {
        self.index.wait(timeout)
    }
    /// Diagnostic only: events restored from an authenticated, byte-verified prefix.
    pub fn checkpoint_events(&self) -> usize {
        self.checkpoint.restored
    }
    fn apply(&mut self, body: &Body) -> Result<(), String> {
        let (kind, summary) = match &body.change {
            Change::PlanCreated { plan } => {
                validate_plan(&plan.title, &plan.tasks)?;
                if plan.digest != content_digest(&plan.title, &plan.tasks)
                    || plan.approved
                    || self.plans.iter().any(|p| p.id == plan.id)
                {
                    return Err("计划记录无效".into());
                }
                self.plans.push(plan.clone());
                ("planCreated", format!("保存计划：{}", plan.title))
            }
            Change::PlanApproved { id, digest } => {
                let p = self
                    .plans
                    .iter_mut()
                    .find(|p| p.id == *id)
                    .ok_or("计划不存在")?;
                if p.digest != *digest || p.approved {
                    return Err("批准与计划不匹配或重复".into());
                }
                p.approved = true;
                ("planApproved", format!("用户批准：{}", p.title))
            }
        };
        self.events.push(EventSummary {
            sequence: body.sequence,
            kind: kind.into(),
            at: body.at.clone(),
            summary,
        });
        Ok(())
    }
    fn append(&mut self, change: Change) -> Result<(), String> {
        if self.poisoned {
            return Err("上次持久化结果不确定，请重启核心核对账本".into());
        }
        let body = Body {
            sequence: self.events.len() as u32 + 1,
            previous_hash: self.last_hash.clone(),
            at: Utc::now().to_rfc3339(),
            change,
        };
        let event = Event {
            hash: hash(&body)?,
            body,
        };
        let dir = self.root.join("events");
        let dest = dir.join(format!("{:08}.json", event.body.sequence));
        let temp = dir.join(format!(
            "{:08}-{}.tmp",
            event.body.sequence,
            std::process::id()
        ));
        // Exclusive writer + create_new temporary; never overwrite an existing committed event.
        if dest.exists() {
            return Err("事件目标已存在".into());
        }
        let bytes = serde_json::to_vec_pretty(&event).map_err(|e| e.to_string())?;
        self.poisoned = true;
        let mut f = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        f.write_all(&bytes)
            .and_then(|_| f.sync_all())
            .map_err(|e| e.to_string())?;
        drop(f);
        fs::rename(&temp, &dest).map_err(|e| e.to_string())?;
        self.apply(&event.body)?;
        self.indexed_events.push(Self::index_event(&event));
        self.checkpoint
            .record(format!("{:08}.json", event.body.sequence), &bytes);
        self.last_hash = event.hash;
        self.poisoned = false;
        self.refresh_index();
        Ok(())
    }
    pub fn snapshot(&self) -> Snapshot {
        let cached = self.index.read(&self.root, &self.index_head()).ok();
        Snapshot {
            blind_reviews: vec![],
            worker_account_setups: vec![],
            team_import: None,
            baselines: vec![],
            baseline_review: None,
            team_timelines: vec![],
            api_jobs: vec![],
            desktop_access: None,
            api_recovery: None,
            api_publication_reviews: vec![],
            core_diagnostic: None,
            startup_reservations: vec![],
            management_record_reviews: vec![],
            startup_closure_reviews: vec![],
            product: PRODUCT_NAME.into(),
            protocol_version: PROTOCOL_VERSION,
            data_directory: self.root.display().to_string(),
            plans: cached
                .as_ref()
                .map(|data| data.plans.clone())
                .unwrap_or_else(|| self.plans.clone()),
            events: cached
                .as_ref()
                .map(|data| data.summaries.clone())
                .unwrap_or_else(|| self.events.clone()),
            adapters: cerulean_adapters::display_catalog(),
            workflows: vec![],
            verification_jobs: vec![],
            verification_detail: None,
            verification_recheck_review: None,
            verification_recheck_status: None,
            verification_recheck_disposition_review: None,
            omp_files: None,
            omp_restore: None,
            omp_jobs: vec![],
            omp_stop_issues: vec![],
            worker_observation: None,
            codex_jobs: vec![],
            codex_cancellations: vec![],
            unsent_assignments: vec![],
            validators: vec![],
            codex_connection: None,
            openai_connection: OpenAiConnection::default(),
            provider_credentials: vec![],
            provider_connections: vec![],
            project_setups: vec![],
            currency_budgets: vec![],
            currency_history: None,
            currency_accounted: None,
            historical_charge_review: None,
            historical_sources: None,
            historical_accounting: None,
            historical_confirmation: None,
            management_decisions: vec![],
            management_history: Default::default(),
            management_detail: None,
            codex_startups: vec![],
            codex_startup_detail: None,
            project_runs: vec![],
            project_revisions: vec![],
        }
    }
    pub fn execute(&mut self, command: Command) -> Result<Snapshot, String> {
        self.execute_command(command)?;
        Ok(self.snapshot())
    }
    pub(crate) fn execute_command(&mut self, command: Command) -> Result<(), String> {
        match command {
            Command::BlindReview { .. }
            | Command::Baseline { .. }
            | Command::TeamTiming { .. }
            | Command::DiscoverDesktop
            | Command::InspectDesktop { .. }
            | Command::BindDesktop { .. }
            | Command::PrepareDesktopTrial { .. }
            | Command::PrepareDesktopTask { .. }
            | Command::SendDesktopTask { .. }
            | Command::ReadDesktopTask { .. }
            | Command::CancelDesktopTask { .. }
            | Command::RetireUnknownDesktop { .. }
            | Command::SendDesktopTrial { .. }
            | Command::ReadDesktopTrial { .. }
            | Command::ReviewStartupIntent { .. }
            | Command::ReviewDesktopStopRecord { .. }
            | Command::ReviewApiPublication { .. }
            | Command::ReviewManagementRecord { .. }
            | Command::ReviewStartupClosure { .. }
            | Command::AcknowledgeOmpStopIssue { .. }
            | Command::CancelOmp { .. }
            | Command::CancelApi { .. }
            | Command::RecoverApi { .. }
            | Command::AbandonApiStartup { .. }
            | Command::RetireUnknownApi { .. }
            | Command::RecoverOmp { .. }
            | Command::AbandonOmpStartup { .. }
            | Command::ReadOmpFiles { .. }
            | Command::RestoreOmpFile { .. }
            | Command::RegisterProject { .. }
            | Command::StartProjectRun { .. }
            | Command::ReadManagementPage { .. }
            | Command::ReadManagementDetail { .. }
            | Command::ReadVerificationDetail { .. }
            | Command::ReviewVerificationRecheckDisposition { .. }
            | Command::RetireVerificationRecheck { .. }
            | Command::ReviewVerificationRecheck { .. }
            | Command::ReadVerificationRecheck { .. }
            | Command::ApproveVerificationRecheck { .. }
            | Command::StartVerificationRecheck { .. }
            | Command::CancelVerificationRecheck { .. }
            | Command::RecoverVerificationRecheck { .. }
            | Command::ReadWorkerObservation { .. }
            | Command::RecoverVerification { .. }
            | Command::RequestProjectRevision { .. }
            | Command::RetireSubmittedPlan { .. }
            | Command::PauseProjectRun { .. }
            | Command::DiscardManagementDecision { .. }
            | Command::AnswerManagementWait { .. }
            | Command::AbandonManagementApplication { .. }
            | Command::CloseAdviceTracking { .. }
            | Command::ReadCodexStartup { .. }
            | Command::AbandonCodexStartup { .. }
            | Command::ConfigureProject { .. }
            | Command::ProvisionWorkerAccounts { .. }
            | Command::OpenWorkerLogin { .. }
            | Command::RequestChiefPlan { .. }
            | Command::AuthorizePlanningBudget { .. }
            | Command::ReviewHistoricalCharge { .. }
            | Command::ReadHistoricalSources { .. }
            | Command::ReadHistoricalAccounting { .. }
            | Command::ReadHistoricalConfirmation { .. }
            | Command::ConfirmHistoricalCharge { .. }
            | Command::ReadManagementCounts { .. }
            | Command::ReadApiGenerations { .. }
            | Command::ReviewApiGenerationResolution { .. }
            | Command::ConfirmApiGenerationResolution { .. }
            | Command::ReadApiGenerationConfirmation { .. }
            | Command::ReadApiCounts { .. }
            | Command::ReviewApiCountResolution { .. }
            | Command::ConfirmApiCountResolution { .. }
            | Command::ReadApiCountConfirmation { .. }
            | Command::ReviewManagementCountResolution { .. }
            | Command::ConfirmManagementCountResolution { .. }
            | Command::ReadManagementCountConfirmation { .. }
            | Command::CheckCodex { .. }
            | Command::SetOpenAiKey { .. }
            | Command::RemoveOpenAiKey
            | Command::SetProviderKey { .. }
            | Command::RemoveProviderKey { .. }
            | Command::CheckOpenAi
            | Command::CheckProviderModels { .. }
            | Command::ReconcileCodex { .. }
            | Command::CancelCodex { .. }
            | Command::AbandonUnsentAttempt { .. }
            | Command::RegisterValidator { .. }
            | Command::ApproveRevision { .. }
            | Command::CancelVerification { .. }
            | Command::AbandonVerification { .. } => return Err("完整项目命令需要常驻宿主".into()),
            Command::Snapshot => {}
            Command::CreatePlan { title, tasks } => {
                let title = title.trim().to_string();
                let tasks = tasks
                    .into_iter()
                    .map(|s| s.trim().to_string())
                    .collect::<Vec<_>>();
                validate_plan(&title, &tasks)?;
                let plan = Plan {
                    id: format!("plan-{:08}", self.events.len() + 1),
                    revision: 1,
                    digest: content_digest(&title, &tasks),
                    title,
                    tasks,
                    created_at: Utc::now().to_rfc3339(),
                    approved: false,
                };
                self.append(Change::PlanCreated { plan })?;
            }
            Command::ApprovePlan { id, digest } => {
                let p = self.plans.iter().find(|p| p.id == id).ok_or("计划不存在")?;
                if p.digest != digest {
                    return Err("计划内容已变化，请重新审阅".into());
                }
                if !p.approved {
                    self.append(Change::PlanApproved { id, digest })?;
                }
            }
        }
        Ok(())
    }
    pub fn handle(&mut self, request: Request) -> Response {
        if request.protocol_version != PROTOCOL_VERSION {
            return Response {
                id: request.id,
                result: None,
                error: Some(ApiError {
                    code: "protocolMismatch".into(),
                    message: "核心协议版本不匹配".into(),
                }),
            };
        }
        match self.execute(request.command) {
            Ok(s) => Response {
                id: request.id,
                result: Some(s),
                error: None,
            },
            Err(message) => Response {
                id: request.id,
                result: None,
                error: Some(ApiError {
                    code: "commandRejected".into(),
                    message,
                }),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn approve_requires_exact_digest_and_survives_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path()).unwrap();
        let p = s
            .execute(Command::CreatePlan {
                title: "真实需求".into(),
                tasks: vec!["编程任务".into()],
            })
            .unwrap()
            .plans
            .remove(0);
        assert!(s
            .execute(Command::ApprovePlan {
                id: p.id.clone(),
                digest: "stale".into()
            })
            .is_err());
        assert_eq!(s.events.len(), 1);
        s.execute(Command::ApprovePlan {
            id: p.id.clone(),
            digest: p.digest.clone(),
        })
        .unwrap();
        s.execute(Command::ApprovePlan {
            id: p.id,
            digest: p.digest,
        })
        .unwrap();
        assert_eq!(s.events.len(), 2);
        drop(s);
        let s = Store::open(dir.path()).unwrap();
        assert!(s.plans[0].approved);
        assert_eq!(s.events.len(), 2);
    }
    #[test]
    fn refuses_concurrent_writer_and_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path()).unwrap();
        assert!(Store::open(dir.path()).is_err());
        s.execute(Command::CreatePlan {
            title: "test".into(),
            tasks: vec!["task".into()],
        })
        .unwrap();
        drop(s);
        fs::write(dir.path().join("events/00000001.json"), b"{}").unwrap();
        assert!(Store::open(dir.path()).is_err());
    }
    #[test]
    fn protocol_and_invalid_plan_cannot_mutate() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::open(dir.path()).unwrap();
        assert!(s
            .handle(Request {
                id: "x".into(),
                protocol_version: 99,
                command: Command::CreatePlan {
                    title: "a".into(),
                    tasks: vec!["b".into()]
                }
            })
            .error
            .is_some());
        assert!(s
            .execute(Command::CreatePlan {
                title: " ".into(),
                tasks: vec![]
            })
            .is_err());
        assert!(s.events.is_empty());
    }
}

mod workflow_access;
#[cfg(windows)]
mod workflow_coordinator;
