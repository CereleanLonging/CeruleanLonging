//! Trusted service host. User IPC cannot impersonate execution roles.
#[cfg(all(test, windows))]
#[path = "advice_crash_tests.rs"]
mod advice_crash_tests;
#[path = "advice_recovery.rs"]
mod advice_recovery;
#[cfg(windows)]
#[path = "api_assignment_scope.rs"]
mod api_assignment_scope;
#[cfg(windows)]
#[path = "api_controls.rs"]
mod api_controls;
#[cfg(all(test, windows))]
#[path = "api_controls_tests.rs"]
mod api_controls_tests;
#[path = "api_count_resolution_host.rs"]
mod api_count_resolution_host;
#[path = "api_count_resolution_product.rs"]
mod api_count_resolution_product;
#[cfg(windows)]
#[path = "api_currency_host.rs"]
mod api_currency_host;
#[path = "api_generation_resolution_host.rs"]
mod api_generation_resolution_host;
#[path = "api_generation_resolution_product.rs"]
mod api_generation_resolution_product;
#[cfg(windows)]
#[path = "api_host.rs"]
mod api_host;
#[cfg(all(test, windows))]
#[path = "api_host_compatibility_tests.rs"]
mod api_host_compatibility_tests;
#[cfg(all(test, windows))]
#[path = "api_host_tests.rs"]
mod api_host_tests;
#[cfg(all(test, windows))]
#[path = "api_project_tests.rs"]
mod api_project_tests;
#[cfg(all(test, windows))]
#[path = "api_publication_tests.rs"]
mod api_publication_tests;
#[cfg(all(test, windows))]
#[path = "api_run_tests.rs"]
mod api_run_tests;
#[cfg(windows)]
#[path = "api_supervision_requests.rs"]
mod api_supervision_requests;
#[cfg(windows)]
#[path = "api_tool_checkpoint_host.rs"]
mod api_tool_checkpoint_host;
#[path = "application_recovery.rs"]
mod application_recovery;
#[cfg(windows)]
#[path = "baseline_product.rs"]
mod baseline_product;
#[path = "blind_review_host.rs"]
mod blind_review_host;
#[cfg(all(test, windows))]
#[path = "codex_cancel_live.rs"]
mod codex_cancel_live;
#[cfg(all(test, windows))]
#[path = "codex_cancel_native.rs"]
mod codex_cancel_native;
#[path = "codex_cancellation.rs"]
mod codex_cancellation;
#[cfg(all(test, windows))]
#[path = "codex_early_cancel_live.rs"]
mod codex_early_cancel_live;
#[cfg(windows)]
#[path = "codex_management_bridge.rs"]
mod codex_management_bridge;
#[cfg(windows)]
#[path = "codex_shared_host.rs"]
mod codex_shared_host;
#[cfg(all(test, windows))]
#[path = "codex_worktree_tests.rs"]
mod codex_worktree_tests;
#[path = "count_resolution_host.rs"]
mod count_resolution_host;
#[path = "count_resolution_product.rs"]
mod count_resolution_product;
#[path = "currency_history.rs"]
mod currency_history;
#[path = "currency_host.rs"]
mod currency_host;
#[cfg(all(test, windows))]
#[path = "currency_host_tests.rs"]
mod currency_host_tests;
#[cfg(all(test, windows))]
#[path = "currency_product_tests.rs"]
mod currency_product_tests;
#[cfg(all(test, windows))]
#[path = "dependency_host_fixture.rs"]
mod dependency_host_fixture;
#[path = "deputy_management_host.rs"]
mod deputy_management_host;
#[cfg(windows)]
#[path = "desktop_supervision.rs"]
mod desktop_supervision;
#[path = "document_refinement.rs"]
pub(crate) mod document_refinement;
#[cfg(windows)]
#[path = "execution_resources.rs"]
mod execution_resources;
#[path = "external_chief.rs"]
mod external_chief;
#[cfg(windows)]
#[path = "functional_test_host.rs"]
mod functional_test_host;
#[path = "historical_charge_host.rs"]
mod historical_charge_host;
#[path = "historical_paging.rs"]
mod historical_paging;
#[path = "historical_product.rs"]
mod historical_product;
#[cfg(all(test, windows))]
#[path = "integration_tests.rs"]
mod integration_tests;
#[cfg(all(test, windows))]
#[path = "legacy_scheduler_tests.rs"]
mod legacy_scheduler_tests;
#[cfg(windows)]
#[path = "local_review.rs"]
mod local_review;
#[path = "management_application.rs"]
mod management_application;
#[path = "management_assignments.rs"]
mod management_assignments;
#[cfg(all(test, windows))]
#[path = "management_configuration_tests.rs"]
mod management_configuration_tests;
#[cfg(all(test, windows))]
#[path = "management_currency_tests.rs"]
mod management_currency_tests;
#[path = "management_history.rs"]
mod management_history;
#[path = "management_preflight_host.rs"]
mod management_preflight_host;
#[path = "management_resolution.rs"]
mod management_resolution;
#[path = "management_review_host.rs"]
mod management_review_host;
#[path = "management_routing.rs"]
mod management_routing;
#[path = "management_scoped_host.rs"]
mod management_scoped_host;
#[cfg(all(test, windows))]
#[path = "management_source_bridge_tests.rs"]
mod management_source_bridge_tests;
#[cfg(all(test, windows))]
#[path = "omp_cancel_crash_tests.rs"]
mod omp_cancel_crash_tests;
#[cfg(all(test, windows))]
#[path = "omp_controls_native.rs"]
mod omp_controls_native;
#[cfg(windows)]
#[path = "omp_host.rs"]
mod omp_host;
#[cfg(windows)]
#[path = "omp_management_bridge.rs"]
mod omp_management_bridge;
#[cfg(all(test, windows))]
#[path = "omp_project_tests.rs"]
mod omp_project_tests;
#[path = "omp_restore_host.rs"]
mod omp_restore_host;
#[cfg(windows)]
#[path = "omp_shared_host.rs"]
mod omp_shared_host;
#[cfg(all(test, windows))]
#[path = "omp_stop_review_tests.rs"]
mod omp_stop_review_tests;
#[cfg(windows)]
#[path = "omp_supervision_requests.rs"]
mod omp_supervision_requests;
#[cfg(windows)]
#[path = "omp_tool_checkpoint_host.rs"]
mod omp_tool_checkpoint_host;
#[cfg(all(test, windows))]
#[path = "planning_budget_tests.rs"]
mod planning_budget_tests;
#[cfg(windows)]
#[path = "preserve_shutdown_host.rs"]
pub(crate) mod preserve_shutdown_host;
#[cfg(all(test, windows))]
#[path = "project_dag_tests.rs"]
mod project_dag_tests;
#[path = "project_planning.rs"]
mod project_planning;
#[path = "project_revision.rs"]
mod project_revision;
#[path = "project_rework.rs"]
mod project_rework;
#[path = "project_runner.rs"]
mod project_runner;
#[path = "provider_connections.rs"]
mod provider_connections;
#[cfg(all(test, windows))]
#[path = "relay_host_tests.rs"]
mod relay_host_tests;
#[cfg(all(test, windows))]
#[path = "resource_config_tests.rs"]
mod resource_config_tests;
#[cfg(windows)]
#[path = "resource_leases.rs"]
mod resource_leases;
#[cfg(windows)]
#[path = "shared_acceptance_host.rs"]
mod shared_acceptance_host;
#[cfg(windows)]
#[path = "shared_verification_host.rs"]
mod shared_verification_host;
#[cfg(all(test, windows))]
#[path = "startup_closure_tests.rs"]
mod startup_closure_tests;
#[path = "startup_management_proof.rs"]
pub(crate) mod startup_management_proof;
#[cfg(all(test, windows))]
#[path = "startup_order_tests.rs"]
mod startup_order_tests;
#[cfg(all(test, windows))]
#[path = "startup_publication_tests.rs"]
mod startup_publication_tests;
#[path = "startup_recovery.rs"]
pub(crate) mod startup_recovery;
#[cfg(all(test, windows))]
#[path = "startup_recovery_crash_tests.rs"]
mod startup_recovery_crash_tests;
#[cfg(windows)]
#[path = "supervision_delivery.rs"]
mod supervision_delivery;
#[cfg(windows)]
#[path = "supervision_requests.rs"]
mod supervision_requests;
#[cfg(windows)]
#[path = "team_observation.rs"]
pub mod team_observation;
#[path = "team_timing_product.rs"]
mod team_timing_product;
#[path = "unsent_assignment.rs"]
mod unsent_assignment;
#[path = "verification_recheck.rs"]
mod verification_recheck;
#[cfg(windows)]
#[path = "verification_recheck_host.rs"]
mod verification_recheck_host;
#[path = "verification_recovery.rs"]
mod verification_recovery;
#[path = "verification_start_retry.rs"]
mod verification_start_retry;
#[path = "worker_account_authorization.rs"]
mod worker_account_authorization;
#[cfg(all(test, windows))]
#[path = "worker_boundaries_host_tests.rs"]
mod worker_boundaries_host_tests;
#[path = "workspace_host.rs"]
pub(crate) mod workspace_host;
#[cfg(all(test, windows))]
#[path = "workspace_management_decisions_tests.rs"]
mod workspace_management_decisions_tests;
#[path = "workspace_observation.rs"]
pub(crate) mod workspace_observation;
#[cfg(all(test, windows))]
#[path = "workspace_startup_tests.rs"]
mod workspace_startup_tests;
#[path = "workspace_verification.rs"]
pub(crate) mod workspace_verification;
#[cfg(windows)]
#[path = "worktree_configuration.rs"]
mod worktree_configuration;
use crate::{
    scheduler::{CommandContext, DurableWorkflow, SchedulerConfig},
    Store,
};
#[cfg(windows)]
#[path = "desktop_disposition.rs"]
mod desktop_disposition;
#[cfg(windows)]
#[path = "desktop_management.rs"]
mod desktop_management;
#[cfg(windows)]
#[path = "desktop_sessions.rs"]
pub(crate) mod desktop_sessions;
use cerulean_domain::{execution::*, workflow::*, *};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub struct RuntimeHost {
    blind_reviews: crate::blind_review::Ledger,
    #[cfg(windows)]
    blind_active: Option<blind_review_host::ActiveReview>,
    #[cfg(windows)]
    functional_active: Option<functional_test_host::ActiveFunctionalTest>,
    #[cfg(all(test, windows))]
    blind_fixture: Option<blind_review_host::Fixture>,
    #[cfg(windows)]
    baselines: baseline_product::Controller,
    #[cfg(all(test, windows))]
    baseline_fixture: bool,
    #[cfg(windows)]
    timed_teams: BTreeMap<String, team_observation::timing::TimedTeam>,
    #[cfg(windows)]
    timing_errors: BTreeMap<String, String>,
    #[cfg(windows)]
    desktop_access: cerulean_domain::desktop_sessions::DesktopAccess,
    #[cfg(windows)]
    desktop_probe: Option<desktop_sessions::Probe>,
    #[cfg(windows)]
    desktop_active: Option<desktop_sessions::desktop_background::Active>,
    #[cfg(all(windows, test))]
    desktop_fixture: Option<desktop_sessions::Fixture>,
    #[cfg(windows)]
    api_startups: crate::api_startup::Startups,
    #[cfg(windows)]
    api_publication_reviews: Vec<ApiPublicationReview>,
    #[cfg(windows)]
    api_jobs: BTreeMap<(String, String), api_jobs::ApiJob>,
    #[cfg(windows)]
    api_active: BTreeMap<(String, String), crate::async_api::ActiveApi>,
    #[cfg(windows)]
    api_writers: BTreeMap<String, crate::async_api::ApiWorkflow>,
    #[cfg(windows)]
    api_fault: Option<String>,
    #[cfg(all(test, windows))]
    api_fixture: Option<crate::async_api::Fixture>,
    #[cfg(all(test, windows))]
    api_gateway_fixture: Option<crate::async_api::GatewayFixture>,
    #[cfg(all(test, windows))]
    api_scoped_fixture: Option<crate::api_currency::Fixture>,
    #[cfg(all(test, windows))]
    api_prepare_gate: Option<crate::async_api::PreparationGate>,
    #[cfg(all(test, windows))]
    omp_prepare_gate: Option<crate::async_omp::PreparationGate>,
    #[cfg(all(test, windows))]
    codex_prepare_gate: Option<crate::async_codex::PreparationGate>,
    base: Store,
    root: PathBuf,
    projects: BTreeMap<String, DurableWorkflow>,
    jobs: BTreeMap<(String, String), VerificationJob>,
    #[cfg(windows)]
    recheck_jobs: std::collections::BTreeSet<(String, String)>,
    validators: BTreeMap<String, RegisteredValidator>,
    codex_jobs: BTreeMap<(String, String), CodexJob>,
    omp_jobs: BTreeMap<(String, String), omp_jobs::OmpJob>,
    omp_stop_issues: Vec<omp_jobs::OmpStopIssue>,
    omp_startups: crate::startup_journal::StartupJournal,
    #[cfg(windows)]
    omp_active: BTreeMap<(String, String), crate::async_omp::ActiveOmp>,
    #[cfg(windows)]
    codex_active: BTreeMap<(String, String), crate::async_codex::ActiveCodex>,
    codex_connection: Option<CodexConnectionCheck>,
    openai_connection: OpenAiConnection,
    deepseek_key_configured: bool,
    provider_connections:
        BTreeMap<credentials::CredentialProvider, credentials::ProviderConnection>,
    provider_probes: BTreeMap<
        credentials::CredentialProvider,
        std::sync::mpsc::Receiver<Result<cerulean_adapters::provider_models::ModelList, String>>,
    >,
    openai_probe: Option<std::sync::mpsc::Receiver<Result<Vec<String>, String>>>,
    management: crate::management_execution::DecisionJournal,
    startups: crate::startup_journal::StartupJournal,
    advice_closures: crate::advice_closure::ClosureJournal,
    project_setups: BTreeMap<String, project_setup::SavedProjectSetup>,
    currency_accounts: BTreeMap<String, crate::currency_budget::BudgetLedger>,
    planning_errors: BTreeMap<String, String>,
    runs: crate::run_control::RunLedger,
    orchestrating: bool,
    runner_error: Option<String>,
    #[cfg(test)]
    management_fixture: Option<
        fn(
            crate::management_execution::DecisionIntent,
        ) -> Result<cerulean_adapters::openai::DecisionObservation, String>,
    >,
    management_active: BTreeMap<
        String,
        std::sync::mpsc::Receiver<Result<management_scoped_host::ManagementCompletion, String>>,
    >,
    #[cfg(test)]
    deputy_management_fixture: Option<
        fn(
            &cerulean_adapters::deputy_management::PreparedDeputy,
        ) -> cerulean_adapters::deputy_management::DeputyResult,
    >,
    management_counts: BTreeMap<String, management_preflight_host::PendingCount>,
    management_count_errors: BTreeMap<String, String>,
    #[cfg(test)]
    management_count_fixture: Option<management_preflight_host::CountFixture>,
    #[cfg(test)]
    management_scoped_fixture: Option<management_scoped_host::ScopedFixture>,
    codex_probe:
        Option<std::sync::mpsc::Receiver<Result<cerulean_adapters::codex::Connection, String>>>,
    #[cfg(windows)]
    active: BTreeMap<String, crate::async_verification::ActiveVerification>,
}
pub(crate) fn ordinary(path: &Path, directory: bool) -> Result<(), String> {
    let m = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    require(
        !m.file_type().is_symlink() && if directory { m.is_dir() } else { m.is_file() },
        "linked or invalid host path",
    )?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        require(m.file_attributes() & 0x400 == 0, "reparse host path")?;
    }
    Ok(())
}
pub(crate) fn config(project: Project) -> SchedulerConfig {
    config_with_catalog(project, cerulean_adapters::current_catalog())
}
fn legacy_config(project: Project) -> SchedulerConfig {
    config_with_catalog(project, cerulean_adapters::catalog())
}
fn config_with_catalog(project: Project, catalog: Vec<AdapterInfo>) -> SchedulerConfig {
    let mut principals = vec![
        Principal {
            id: "user".into(),
            role: Role::User,
            provider: None,
        },
        Principal {
            id: "chief".into(),
            role: Role::Chief,
            provider: Some("openai".into()),
        },
        Principal {
            id: "deputy".into(),
            role: Role::Deputy,
            provider: Some("openai".into()),
        },
        Principal {
            id: "core".into(),
            role: Role::Core,
            provider: None,
        },
    ];
    principals.extend(catalog.into_iter().map(|a| Principal {
        id: format!("worker-{}", a.id),
        role: Role::Worker,
        provider: None,
    }));
    SchedulerConfig {
        // Old readers must not rewrite these projections using fixed UTC+8.
        schema_version: if project.timezone == cerulean_domain::timezone::DEFAULT {
            1
        } else {
            2
        },
        core_id: "core".into(),
        principals,
        execution_scope: Some(ExecutionScope {
            project,
            policy_version: 1,
        }),
    }
}
pub(crate) fn matches_config(saved: &SchedulerConfig, project: &Project) -> bool {
    *saved == config(project.clone()) || *saved == legacy_config(project.clone())
}
pub(crate) fn require_bound_worker(
    config: &SchedulerConfig,
    adapter_id: &str,
) -> Result<(), String> {
    require(
        config
            .principals
            .iter()
            .any(|p| p.id == format!("worker-{adapter_id}") && p.role == Role::Worker),
        "执行者未绑定到此项目的原角色表；请创建新项目",
    )
}
#[path = "role_workspace_host.rs"]
mod role_workspace_host;

impl RuntimeHost {
    #[cfg(windows)]
    pub fn describe_codex_worker(
        executable: &Path,
        model: &str,
    ) -> Result<WorkerSelection, String> {
        crate::async_codex::describe(executable, model)
    }
    pub(crate) fn directory(&self) -> &Path {
        &self.root
    }
    fn management_context(
        &self,
        project: &str,
        requirement: &str,
    ) -> Result<serde_json::Value, String> {
        require(
            !requirement.trim().is_empty() && !requirement.contains('\0'),
            "invalid management requirement",
        )?;
        #[cfg(windows)]
        if let Some(writer) = self.api_writers.get(project) {
            self.require_available_inner(project, true)?;
            let snapshot = writer.owner.snapshot()?;
            require(
                snapshot
                    .revision
                    .as_ref()
                    .is_some_and(|r| r.worktree.is_some()),
                "API并行上下文需要独立工作目录",
            )?;
            return Ok(self.context_for_snapshot(requirement, &snapshot));
        }
        self.require_available(project)?;
        let workflow = self
            .snapshot()
            .workflows
            .into_iter()
            .find(|w| w.project.id == project)
            .ok_or("project not registered")?;
        Ok(self.context_for_snapshot(requirement, &workflow))
    }
    fn context_for_snapshot(
        &self,
        requirement: &str,
        workflow: &ExecutionSnapshot,
    ) -> serde_json::Value {
        let binding = self
            .projects
            .get(&workflow.project.id)
            .map(|store| store.configuration());
        #[cfg(windows)]
        let binding = binding.or_else(|| {
            self.api_writers
                .get(&workflow.project.id)
                .map(|writer| writer.owner.configuration())
        });
        let workers: Vec<_> = cerulean_adapters::supported_catalog()
            .into_iter()
            .filter(|adapter| {
                binding.is_none_or(|config| require_bound_worker(config, &adapter.id).is_ok())
            })
            .collect();
        let mut context = serde_json::json!({"requirement":requirement,"workflow":workflow,"validators":self.validators,"workers":workers});
        #[cfg(windows)]
        if self
            .project_setups
            .get(&workflow.project.id)
            .is_some_and(|s| s.setup.validator_id == cerulean_domain::objective_checker::ID)
        {
            context["validators"] = serde_json::to_value(
                self.validators
                    .iter()
                    .filter(|(id, _)| id.as_str() == cerulean_domain::objective_checker::ID)
                    .collect::<BTreeMap<_, _>>(),
            )
            .expect("catalog");
            let runtimes = crate::objective_checker::runtimes()
                .into_iter()
                .filter(|(_, file)| {
                    !self.projects.values().any(|p| {
                        p.configuration().execution_scope.as_ref().is_some_and(|s| {
                            crate::artifacts::contains_path(
                                Path::new(&s.project.root),
                                Path::new(&file.path),
                            )
                        })
                    })
                })
                .collect::<BTreeMap<_, _>>();
            context["objectiveRuntimes"] = serde_json::to_value(runtimes).expect("runtimes");
            context["objectiveTestInstructions"] =
                cerulean_domain::objective_checker::PLANNING_INSTRUCTIONS.into();
        }
        if let Some(setup) = self.project_setups.get(&workflow.project.id) {
            context["executionSetup"] = serde_json::to_value(setup).expect("serializable setup");
        }
        let base_bytes = serde_json::to_vec(&context).expect("JSON context").len();
        let replies = self.management.user_replies(
            &workflow.project.id,
            (64 * 1024usize).saturating_sub(base_bytes + 128),
        );
        if self.management.reply_count(&workflow.project.id) > 0 {
            context["userResponsesOmitted"] =
                (self.management.reply_count(&workflow.project.id) - replies.len()).into();
            context["userResponses"] = serde_json::Value::Array(replies);
        }
        context
    }
    /// Trusted orchestration entry only. Not exposed as an arbitrary user IPC call.
    /// A fresh intent is durable before the single provider call is scheduled.
    #[cfg(windows)]
    pub fn start_management_decision(
        &mut self,
        project: &str,
        operation: &str,
        purpose: management::Purpose,
        model: &str,
        requirement: &str,
    ) -> Result<(), String> {
        self.require_review_dispatch_idle()?;
        self.require_management_dispatch_available(project, purpose)?;
        self.poll_verifications();
        if purpose == management::Purpose::Accept
            && self
                .project_setups
                .get(project)
                .and_then(|s| s.setup.management_sources.as_ref())
                .is_some_and(|s| matches!(s.chief, project_setup::ChiefSource::CodexSession { .. }))
        {
            return self.prepare_external_chief_accept(project, operation, model, requirement);
        }
        if let Some(setup) = self.project_setups.get(project) {
            require(
                self.management_operation_ids(project).len()
                    < setup.setup.management_max_requests as usize,
                "项目主副指挥请求上限已用完",
            )?;
        }
        require(
            self.management_work_count() < 4,
            "management concurrency limit reached",
        )?;
        self.require_management_connection(project, purpose)?;
        if self.has_api_activity(project) {
            require(
                matches!(
                    purpose,
                    management::Purpose::Assign | management::Purpose::Accept
                ),
                "运行中仅允许独立任务分配与主指挥收口",
            )?;
        }
        let context = self.management_dispatch_context(project, purpose, requirement)?;
        let intent = crate::management_execution::DecisionIntent {
            operation_id: operation.into(),
            project_id: project.into(),
            purpose,
            model: model.into(),
            state_digest: digest(&context)?,
            context,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        self.launch_routed_management(intent)
    }
    pub(super) fn require_currency_execution_available(&self, project: &str) -> Result<(), String> {
        require(
            !self.currency_accounts.contains_key(project)
                && self
                    .project_setups
                    .get(project)
                    .is_none_or(|s| s.setup.policy.currency_budget.is_none())
                && self
                    .projects
                    .get(project)
                    .and_then(|p| p.revision())
                    .is_none_or(|r| r.policy.currency_budget.is_none()),
            "此项目的金额预算尚不可执行；未发送模型请求",
        )
    }
    #[cfg(windows)]
    fn launch_authenticated_management(
        &mut self,
        intent: crate::management_execution::DecisionIntent,
    ) -> Result<(), String> {
        if let Some(sources) = self
            .project_setups
            .get(&intent.project_id)
            .and_then(|s| s.setup.management_sources.as_ref())
        {
            use cerulean_domain::project_setup::{ChiefSource, DeputySource};
            let openai = match intent.purpose {
                management::Purpose::Plan
                | management::Purpose::Refine
                | management::Purpose::Accept => {
                    matches!(sources.chief, ChiefSource::OpenAi {})
                }
                management::Purpose::Assign | management::Purpose::Advise => {
                    matches!(sources.deputy, DeputySource::OpenAi {})
                }
            };
            require(
                openai,
                "当前配置的管理来源不是OpenAI，不允许回退到OpenAI调用",
            )?;
        }
        if self.scoped_management_configured(&intent.project_id) {
            return self.start_scoped_management(intent);
        }
        if intent.purpose == management::Purpose::Plan
            && self
                .require_currency_execution_available(&intent.project_id)
                .is_err()
        {
            return self.start_counted_planning(intent);
        }
        self.require_currency_execution_available(&intent.project_id)?;
        #[cfg(test)]
        if let Some(send) = self.management_fixture {
            return self.launch_management(intent, send);
        }
        // Local prerequisites fail before any billable intent is recorded.
        let client = cerulean_adapters::openai::OpenAiClient::new(crate::credential_vault::load(
            &self.root,
        )?)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "management runtime unavailable")?;
        let prepared = cerulean_adapters::openai::PreparedDecision::new(
            intent.purpose,
            &intent.model,
            &intent.state_digest,
            &intent.context,
        )?;
        let record_wire = crate::data_format::status(&self.root)?.minimum_reader_generation
            >= crate::currency_guard::GENERATION;
        let fingerprint = record_wire.then(|| prepared.fingerprint().clone());
        self.launch_management_evidenced(intent, fingerprint, move |_| {
            runtime
                .block_on(client.decide_prepared(prepared))
                .map(|mut r| {
                    // Reader1–3 retain their old on-disk contract. Monetary projects
                    // require reader4 and remain gated before reaching this outlet.
                    if !record_wire {
                        r.transport = None;
                    }
                    r
                })
        })
    }
    #[cfg(all(windows, test))]
    fn launch_management(
        &mut self,
        intent: crate::management_execution::DecisionIntent,
        send_once: impl FnOnce(
                crate::management_execution::DecisionIntent,
            ) -> Result<cerulean_adapters::openai::DecisionObservation, String>
            + Send
            + 'static,
    ) -> Result<(), String> {
        self.launch_management_evidenced(intent, None, move |i| send_once(i).map(Into::into))
    }
    #[cfg(windows)]
    fn launch_management_evidenced(
        &mut self,
        intent: crate::management_execution::DecisionIntent,
        fingerprint: Option<cerulean_adapters::openai::ManagementRequestFingerprint>,
        send_once: impl FnOnce(
                crate::management_execution::DecisionIntent,
            ) -> Result<cerulean_adapters::openai::DecisionResult, String>
            + Send
            + 'static,
    ) -> Result<(), String> {
        let operation = intent.operation_id.clone();
        if let Some(fingerprint) = fingerprint {
            self.management
                .begin_with_request(intent.clone(), Some(fingerprint))?;
        } else {
            self.management.begin(intent.clone())?;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        let spawn = std::thread::Builder::new()
            .name("openai-management".into())
            .spawn(move || {
                let result = send_once(intent)
                    .map(|r| management_scoped_host::ManagementCompletion::Legacy(Box::new(r)));
                let _ = sender.send(result);
            });
        match spawn {
            Ok(_) => {
                self.management_active.insert(operation, receiver);
                Ok(())
            }
            Err(_) => {
                self.management
                    .finish(&operation, Err("management worker not started".into()))?;
                Err("management worker not started; intent retained for review".into())
            }
        }
    }
    fn poll_management(&mut self) {
        #[cfg(windows)]
        self.poll_management_counts();
        let completed: Vec<_> = self
            .management_active
            .iter()
            .filter_map(|(id, receiver)| match receiver.try_recv() {
                Ok(result) => Some((id.clone(), result)),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Some((id.clone(), Err("management worker disconnected".into())))
                }
            })
            .collect();
        for (id, outcome) in completed {
            self.management_active.remove(&id);
            // On publication failure the journal is poisoned and the intent remains unknown.
            // Never rerun a request to repair local persistence.
            let publication = self.record_management_completion(&id, outcome);
            if let Ok(intent) = self.management.intent(&id) {
                let project = intent.project_id.clone();
                if let Err(error) = publication {
                    self.management_count_errors.insert(project.clone(), error);
                }
                if self.currency_accounts.contains_key(&project) {
                    if let Err(error) = self.reconcile_currency_account(&project) {
                        self.management_count_errors.insert(project, error);
                    }
                }
            }
        }
        self.poll_planning_results();
    }
    pub fn management_decisions(&self) -> Vec<crate::management_execution::DecisionStatus> {
        let mut statuses = self.management.statuses();
        for status in &mut statuses {
            if self.management_active.contains_key(&status.operation_id) {
                status.state = crate::management_execution::DecisionState::Running;
            }
        }
        statuses
    }
    /// Read a stored suggestion only against the current core-derived context.
    /// This does not grant user approval, apply a command, or consume the decision.
    pub fn management_decision(
        &self,
        operation: &str,
    ) -> Result<&cerulean_adapters::openai::DecisionObservation, String> {
        self.management.ensure_dispatchable(operation)?;
        let intent = self.management.intent(operation)?;
        #[cfg(windows)]
        if self.scoped_api_decision(operation)? || self.scoped_acceptance_decision(operation)? {
            return self.management.ready(operation, &intent.state_digest);
        }
        let requirement = intent.context["requirement"]
            .as_str()
            .ok_or("management requirement missing")?;
        let current = self.rebuild_management_context(
            intent,
            self.management_context(&intent.project_id, requirement)?,
        )?;
        self.management.ready(operation, &digest(&current)?)
    }
    pub fn open(root: &Path) -> Result<Self, String> {
        Self::open_recovering(root, |host| {
            host.management.recover_publications()?;
            host.startups.recover_publications()?;
            host.omp_startups.recover_publications()
        })
    }
    // The public constructor always uses the production recovery above. This
    // private seam lets owned tests stop recovery after all host validation.
    fn open_recovering(
        root: &Path,
        recover: impl FnOnce(&mut Self) -> Result<(), String>,
    ) -> Result<Self, String> {
        let base = Store::open(root)?;
        let blind_reviews = crate::blind_review::Ledger::open(root)?;
        let root = fs::canonicalize(root).map_err(|e| e.to_string())?;
        let mut openai_connection = OpenAiConnection {
            state: "notConfigured".into(),
            ..Default::default()
        };
        #[cfg(windows)]
        {
            openai_connection.key_configured = crate::credential_vault::exists(&root)?;
            if openai_connection.key_configured {
                openai_connection.state = "unchecked".into();
            }
        }
        #[cfg(windows)]
        let deepseek_key_configured =
            crate::credential_vault::exists_for(&root, credentials::CredentialProvider::DeepSeek)?;
        #[cfg(not(windows))]
        let deepseek_key_configured = false;
        let mut validators = crate::validator_catalog::load(&root)?;
        #[cfg(windows)]
        if let Ok(checker) = crate::objective_checker::registration() {
            validators.insert(checker.id.clone(), checker);
        }
        let management = crate::management_execution::DecisionJournal::open(&root)?;
        let startups = crate::startup_journal::StartupJournal::open(&root)?;
        let omp_startups = crate::startup_journal::StartupJournal::open_omp(&root)?;
        let project_setups = crate::project_setup::load(&root)?;
        let runs = crate::run_control::RunLedger::open(&root)?;
        let advice_closures = crate::advice_closure::ClosureJournal::open(&root)?;
        let folder = root.join("workflows");
        if !folder.exists() {
            fs::create_dir(&folder).map_err(|e| e.to_string())?;
        }
        ordinary(&folder, true)?;
        let mut projects = BTreeMap::new();
        let mut jobs = BTreeMap::new();
        #[cfg(windows)]
        let mut recheck_jobs = std::collections::BTreeSet::new();
        let mut codex_jobs = BTreeMap::new();
        for entry in fs::read_dir(&folder).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let id = entry
                .file_name()
                .into_string()
                .map_err(|_| "invalid project directory")?;
            identifier(&id)?;
            ordinary(&entry.path(), true)?;
            let path = entry.path().join("bindings.json");
            ordinary(&path, false)?;
            require(
                fs::metadata(&path).map_err(|e| e.to_string())?.len() <= 256 * 1024,
                "host config too large",
            )?;
            let saved: SchedulerConfig =
                serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            let scope = saved
                .execution_scope
                .as_ref()
                .ok_or("unscoped workflow cannot enter service")?;
            require(
                scope.project.id == id && matches_config(&saved, &scope.project),
                "host role or scope configuration mismatch",
            )?;
            let store = DurableWorkflow::open(&entry.path(), saved)?;
            #[cfg(windows)]
            for job in crate::async_codex::load(&root, &id, &store)? {
                codex_jobs.insert((id.clone(), job.attempt_id.clone()), job);
            }
            #[cfg(windows)]
            for job in crate::async_verification::load(&root, &id, &store)? {
                jobs.insert((id.clone(), job.operation_id.clone()), job);
            }
            #[cfg(windows)]
            for job in crate::async_verification_recheck::load(&root, &id, &store)? {
                let key = (id.clone(), job.operation_id.clone());
                require(!jobs.contains_key(&key), "原验收与复验操作编号冲突")?;
                recheck_jobs.insert(key.clone());
                jobs.insert(key, job);
            }
            projects.insert(id, store);
        }
        let provider_connections = provider_connections::load_statuses(&root)?;
        #[cfg(windows)]
        let desktop_access = {
            let bindings = desktop_sessions::load(&root)?;
            let trials = desktop_sessions::load_trials(&root)?;
            require(
                trials
                    .iter()
                    .all(|t| bindings.iter().any(|b| b.id == t.review.binding_id)),
                "桌面验证关联缺失",
            )?;
            require(
                bindings
                    .iter()
                    .all(|binding| projects.contains_key(&binding.project_id)),
                "桌面关联项目缺失",
            )?;
            cerulean_domain::desktop_sessions::DesktopAccess {
                bridge_sha256: desktop_sessions::fingerprint(),
                bindings,
                trials,
                ..Default::default()
            }
        };
        let mut host = Self {
            #[cfg(windows)]
            baselines: baseline_product::Controller::open(&root)?,
            #[cfg(all(test, windows))]
            baseline_fixture: false,
            #[cfg(windows)]
            timed_teams: BTreeMap::new(),
            #[cfg(windows)]
            timing_errors: BTreeMap::new(),
            #[cfg(windows)]
            api_startups: crate::api_startup::load(&root)?,
            #[cfg(windows)]
            api_publication_reviews: crate::api_publication::reviews(&root)?,
            #[cfg(windows)]
            api_jobs: BTreeMap::new(),
            #[cfg(windows)]
            api_active: BTreeMap::new(),
            #[cfg(windows)]
            api_writers: BTreeMap::new(),
            #[cfg(windows)]
            api_fault: None,
            #[cfg(all(test, windows))]
            api_fixture: None,
            #[cfg(all(test, windows))]
            api_gateway_fixture: None,
            #[cfg(all(test, windows))]
            api_scoped_fixture: None,
            #[cfg(all(test, windows))]
            api_prepare_gate: None,
            #[cfg(all(test, windows))]
            omp_prepare_gate: None,
            #[cfg(all(test, windows))]
            codex_prepare_gate: None,
            base,
            blind_reviews,
            #[cfg(windows)]
            blind_active: None,
            #[cfg(windows)]
            functional_active: None,
            #[cfg(all(test, windows))]
            blind_fixture: None,
            root,
            projects,
            jobs,
            #[cfg(windows)]
            recheck_jobs,
            validators,
            codex_jobs,
            omp_jobs: BTreeMap::new(),
            omp_stop_issues: vec![],
            omp_startups,
            #[cfg(windows)]
            omp_active: BTreeMap::new(),
            #[cfg(windows)]
            codex_active: BTreeMap::new(),
            codex_connection: None,
            openai_connection,
            deepseek_key_configured,
            provider_connections,
            #[cfg(windows)]
            desktop_access,
            #[cfg(windows)]
            desktop_probe: None,
            #[cfg(windows)]
            desktop_active: None,
            #[cfg(all(windows, test))]
            desktop_fixture: None,
            provider_probes: BTreeMap::new(),
            openai_probe: None,
            management,
            startups,
            advice_closures,
            project_setups,
            currency_accounts: BTreeMap::new(),
            planning_errors: BTreeMap::new(),
            runs,
            orchestrating: false,
            runner_error: None,
            #[cfg(test)]
            management_fixture: None,
            management_active: BTreeMap::new(),
            #[cfg(test)]
            deputy_management_fixture: None,
            management_counts: BTreeMap::new(),
            management_count_errors: BTreeMap::new(),
            #[cfg(test)]
            management_count_fixture: None,
            #[cfg(test)]
            management_scoped_fixture: None,
            codex_probe: None,
            #[cfg(windows)]
            active: BTreeMap::new(),
        };
        #[cfg(windows)]
        host.load_omp_jobs()?;
        #[cfg(windows)]
        host.load_api_jobs()?;
        #[cfg(windows)]
        host.validate_omp_managed_links()?;
        host.validate_startups()?;
        host.validate_application_closures()?;
        host.validate_management_applications()?;
        host.validate_project_setups()?;
        #[cfg(windows)]
        host.validate_desktop_dispatch_records()?;
        #[cfg(windows)]
        host.refresh_desktop_tasks()?;
        host.validate_runs()?;
        #[cfg(windows)]
        host.validate_supervision_requests()?;
        #[cfg(windows)]
        host.validate_supervision_deliveries()?;
        #[cfg(windows)]
        host.validate_managed_codex_links()?;
        host.validate_startup_reservations()?;
        host.validate_management_reviews()?;
        host.load_currency_accounts()?;
        #[cfg(windows)]
        host.validate_api_supervision_requests()?;
        #[cfg(windows)]
        host.validate_desktop_supervision_requests()?;
        #[cfg(windows)]
        host.validate_omp_supervision_requests()?;
        host.validate_advice_closures()?;
        recover(&mut host)?;
        for project in host.projects.keys().cloned().collect::<Vec<_>>() {
            host.reconcile_currency_account(&project)?;
        }
        if !host.omp_stop_issues.is_empty() {
            // All identity/ledger validation above still applies. Do not drive
            // management or startup projections while a stop record needs review.
            return Ok(host);
        }
        host.recover_revision_runs()?;
        host.reconcile_management_applications()?;
        host.recover_startups()?;
        #[cfg(windows)]
        host.recover_omp_startups()?;
        host.repair_closed_application_jobs()?;
        Ok(host)
    }
    pub fn snapshot(&self) -> Snapshot {
        let mut snapshot = self.base.snapshot();
        snapshot.blind_reviews = self.blind_review_views();
        #[cfg(windows)]
        {
            snapshot.worker_account_setups = crate::windows_worker_account::views(&self.root);
        }
        snapshot.team_timelines = self.team_timing_views();
        #[cfg(windows)]
        {
            snapshot.team_import = crate::team_import_service::view(&self.root);
        }
        #[cfg(windows)]
        {
            snapshot.baselines = self.baselines.views();
            snapshot.baseline_review = self.baselines.review();
        }
        #[cfg(windows)]
        {
            let mut access = self.desktop_access.clone();
            if let Some(active) = &self.desktop_active {
                access.checking = true;
                access
                    .tasks
                    .retain(|t| t.review.id != active.status.review.id);
                access.tasks.push(active.status.clone());
            }
            for task in &mut access.tasks {
                task.retirement_digest = if !task.active && !task.submitted && !task.retired {
                    self.desktop_retirement_digest(&task.review.project_id, &task.review.attempt.id)
                        .ok()
                } else {
                    None
                };
            }
            snapshot.desktop_access = Some(access);
        }
        #[cfg(windows)]
        {
            snapshot.api_jobs = self.api_summaries();
            snapshot.api_recovery = self
                .api_fault
                .as_ref()
                .map(|message| api_jobs::ApiRecovery {
                    message: message.clone(),
                    can_restart: self.can_restart_api_fault(),
                });
            snapshot.api_publication_reviews = self.api_publication_reviews.clone();
        }
        snapshot.management_record_reviews = self.management.reviews.clone();
        snapshot.startup_closure_reviews = self
            .startups
            .closure_reviews
            .iter()
            .chain(&self.omp_startups.closure_reviews)
            .cloned()
            .collect();
        snapshot.startup_reservations = self
            .startups
            .reservations
            .iter()
            .chain(&self.omp_startups.reservations)
            .cloned()
            .collect();
        snapshot.omp_stop_issues = self.omp_stop_issues.clone();
        snapshot.workflows = self
            .projects
            .values()
            .map(|store| ExecutionSnapshot {
                project: store
                    .configuration()
                    .execution_scope
                    .as_ref()
                    .expect("validated scope")
                    .project
                    .clone(),
                revision: store.revision().cloned(),
                revision_digest: store
                    .revision()
                    .map(|r| r.approval_digest().expect("validated revision")),
                approval: store.approval().cloned(),
                state: store.state().clone(),
                sequence: store.history().len() as u32,
            })
            .collect();
        #[cfg(windows)]
        snapshot.workflows.extend(
            self.active
                .values()
                .filter(|a| !a.shared)
                .map(|a| a.snapshot.clone()),
        );
        snapshot
            .workflows
            .sort_by(|a, b| a.project.id.cmp(&b.project.id));
        #[cfg(windows)]
        snapshot
            .workflows
            .extend(self.api_writers.values().map(|a| a.snapshot.clone()));
        #[cfg(windows)]
        snapshot
            .workflows
            .extend(self.desktop_active.iter().map(|a| a.snapshot.clone()));
        #[cfg(windows)]
        {
            snapshot.omp_jobs = self.omp_summaries();
        }
        #[cfg(not(windows))]
        {
            snapshot.omp_jobs = self.omp_jobs.values().cloned().collect();
        }
        snapshot
            .workflows
            .sort_by(|a, b| a.project.id.cmp(&b.project.id));
        snapshot.verification_jobs = self
            .jobs
            .values()
            .map(|job| self.verification_job_view(job))
            .collect();
        snapshot.validators = self
            .validators
            .values()
            .filter(|v| v.id == cerulean_domain::objective_checker::ID)
            .cloned()
            .collect();
        snapshot.codex_connection = self.codex_connection.clone();
        snapshot.openai_connection = self.openai_connection.clone();
        snapshot.provider_credentials = vec![
            credentials::ProviderCredentialStatus {
                provider: credentials::CredentialProvider::OpenAi,
                key_configured: self.openai_connection.key_configured,
            },
            credentials::ProviderCredentialStatus {
                provider: credentials::CredentialProvider::DeepSeek,
                key_configured: self.deepseek_key_configured,
            },
        ];
        snapshot.provider_credentials.extend(
            self.provider_connections
                .values()
                .filter(|s| s.provider != credentials::CredentialProvider::DeepSeek)
                .map(|s| credentials::ProviderCredentialStatus {
                    provider: s.provider,
                    key_configured: s.key_configured,
                }),
        );
        snapshot.provider_connections = self
            .provider_connections
            .values()
            .cloned()
            .map(|mut s| {
                if s.provider == credentials::CredentialProvider::DeepSeek {
                    s.key_configured = self.deepseek_key_configured;
                }
                s
            })
            .collect();
        snapshot.project_setups = self.project_setups.values().cloned().collect();
        let history = self.currency_history_gaps();
        // Account availability always uses all original records, never a page
        // or the bounded compatibility lists sent to an older UI.
        snapshot.currency_budgets = self.currency_account_views(history.as_deref());
        self.project_historical_pages(&mut snapshot, history.as_deref());
        snapshot.project_runs = self.runs.all();
        snapshot.project_revisions = self.project_revision_candidates();
        if let Some(error) = &self.runner_error {
            for run in &mut snapshot.project_runs {
                if run.state == project_setup::RunState::Active {
                    run.state = project_setup::RunState::Attention;
                    run.message = error.clone();
                }
            }
        }
        let (decisions, history) = self
            .management_page(None, 0, None)
            .expect("default management page");
        snapshot.management_decisions = decisions;
        snapshot.management_history = history;
        snapshot.codex_jobs = self.codex_jobs.values().cloned().collect();
        snapshot.codex_cancellations = self.codex_cancellations();
        snapshot.codex_startups = self.startup_summaries();
        snapshot.unsent_assignments = self.unsent_assignments();
        snapshot
    }
    fn verification_job_view(&self, original: &VerificationJob) -> VerificationJob {
        #[allow(unused_mut)]
        let mut job = original.clone();
        #[cfg(windows)]
        {
            job.reconciliation_digest = None;
            if job.state == VerificationState::Unknown
                && !self.active.contains_key(&job.project_id)
                && !self.is_verification_recheck(&job.project_id, &job.operation_id)
            {
                if let Some(store) = self.projects.get(&job.project_id) {
                    job.reconciliation_digest =
                        crate::verification_reconciliation::review_digest(&self.root, &job, store)
                            .ok();
                }
            }
        }
        job
    }
    fn set_openai_key(&mut self, key: Option<secret::SecretInput>) -> Result<(), String> {
        self.require_provider_idle(credentials::CredentialProvider::OpenAi)?;
        require(self.openai_probe.is_none(), "OpenAI 检查中，请等待结束")?;
        require(
            self.management_work_count() == 0,
            "指挥请求进行中，不能变更凭证",
        )?;
        #[cfg(windows)]
        {
            if let Some(secret) = &key {
                crate::credential_vault::save(&self.root, secret)?;
            } else {
                crate::credential_vault::remove(&self.root)?;
            }
            self.openai_connection = OpenAiConnection {
                key_configured: key.is_some(),
                state: if key.is_some() {
                    "unchecked"
                } else {
                    "notConfigured"
                }
                .into(),
                ..Default::default()
            };
            Ok(())
        }
        #[cfg(not(windows))]
        {
            let _ = key;
            Err("凭证存储需要 Windows".into())
        }
    }
    fn set_provider_key(
        &mut self,
        provider: credentials::CredentialProvider,
        key: Option<secret::SecretInput>,
    ) -> Result<(), String> {
        if provider == credentials::CredentialProvider::OpenAi {
            return self.set_openai_key(key);
        }
        self.require_provider_idle(provider)?;
        #[cfg(windows)]
        {
            if let Some(secret) = &key {
                crate::credential_vault::save_for(&self.root, provider, secret)?;
            } else {
                crate::credential_vault::remove_for(&self.root, provider)?;
            }
            if provider == credentials::CredentialProvider::DeepSeek {
                self.deepseek_key_configured = key.is_some();
            }
            self.provider_connections.insert(
                provider,
                credentials::ProviderConnection::unchecked(provider, key.is_some()),
            );
            Ok(())
        }
        #[cfg(not(windows))]
        {
            let _ = key;
            Err("本机凭证存储当前仅支持Windows".into())
        }
    }
    fn check_openai(&mut self) -> Result<(), String> {
        require(
            self.management_work_count() == 0,
            "指挥请求进行中，请等待结束再检查连接",
        )?;
        require(self.openai_probe.is_none(), "OpenAI 检查正在进行")?;
        require(
            self.openai_connection.key_configured,
            "请先在本机配置 OpenAI API Key",
        )?;
        #[cfg(windows)]
        {
            let root = self.root.clone();
            let (sender, receiver) = std::sync::mpsc::channel();
            std::thread::Builder::new()
                .name("openai-connection-check".into())
                .spawn(move || {
                    let result = (|| {
                        let key = crate::credential_vault::load(&root)?;
                        let runtime = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .map_err(|_| "OpenAI runtime unavailable")?;
                        runtime.block_on(async {
                            cerulean_adapters::openai::OpenAiClient::new(key)?
                                .models()
                                .await
                        })
                    })();
                    let _ = sender.send(result);
                })
                .map_err(|_| "无法启动 OpenAI 检查")?;
            self.openai_probe = Some(receiver);
            self.openai_connection.state = "checking".into();
            self.openai_connection.error = None;
            self.openai_connection.models.clear();
            Ok(())
        }
        #[cfg(not(windows))]
        {
            Err("凭证存储需要 Windows".into())
        }
    }
    fn check_codex(&mut self, executable: String) -> Result<(), String> {
        require(self.codex_probe.is_none(), "Codex 连接检查正在进行")?;
        crate::codex_discovery::validate_executable(Path::new(&executable))?;
        let workspace = self.root.join("codex-connection-check");
        if !workspace.exists() {
            fs::create_dir(&workspace).map_err(|e| e.to_string())?;
        }
        ordinary(&workspace, true)?;
        let (sender, receiver) = std::sync::mpsc::channel();
        let path = executable.clone();
        std::thread::Builder::new()
            .name("codex-connection-check".into())
            .spawn(move || {
                let result = (|| {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|_| "Codex check runtime unavailable")?;
                    runtime.block_on(async {
                        let mut client = cerulean_adapters::codex::CodexClient::launch(
                            Path::new(&path),
                            &workspace,
                        )?;
                        let result = client.connect().await;
                        client.close().await?;
                        result
                    })
                })();
                let _ = sender.send(result);
            })
            .map_err(|e| e.to_string())?;
        self.codex_probe = Some(receiver);
        self.codex_connection = Some(CodexConnectionCheck {
            executable,
            state: "checking".into(),
            checked_at: chrono::Utc::now().to_rfc3339(),
            server_version: None,
            account_type: None,
            models: vec![],
            error: None,
        });
        Ok(())
    }
    fn register_validator(
        &mut self,
        id: String,
        name: String,
        executable: String,
        inputs: Vec<String>,
        environment: BTreeMap<String, String>,
    ) -> Result<(), String> {
        #[cfg(windows)]
        require(
            !self
                .codex_active
                .values()
                .any(|a| a.job.management_operation.is_some())
                && !self
                    .omp_active
                    .values()
                    .any(|a| a.job.management_operation.is_some())
                && self.api_active.is_empty()
                && self.desktop_active.is_none(),
            "指挥分派执行中，暂不能变更验收目录",
        )?;
        #[cfg(windows)]
        {
            identifier(&id)?;
            require(
                !name.trim().is_empty() && name.len() <= 200 && inputs.len() <= 16,
                "登记名称或固定文件数量不合法",
            )?;
            let mut total = 0u64;
            for path in std::iter::once(&executable).chain(&inputs) {
                require(
                    path.len() <= 4096 && Path::new(path).is_absolute(),
                    "请输入文件绝对路径",
                )?;
                total = total
                    .checked_add(fs::metadata(path).map_err(|e| e.to_string())?.len())
                    .ok_or("validator size overflow")?;
                require(
                    total <= 512 * 1024 * 1024,
                    "验收工具与固定文件合计不得超过 512 MiB",
                )?;
            }
            let tool = crate::verifier::PinnedCheck::describe(
                Path::new(&executable),
                &inputs.iter().map(PathBuf::from).collect::<Vec<_>>(),
                environment,
            )?;
            for workflow in self.snapshot().workflows {
                for file in std::iter::once(&tool.executable).chain(&tool.inputs) {
                    require(
                        !crate::artifacts::contains_path(
                            Path::new(&workflow.project.root),
                            Path::new(&file.path),
                        ),
                        "验收工具须位于已登记项目之外",
                    )?;
                }
            }
            crate::validator_catalog::save(
                &self.root,
                &mut self.validators,
                RegisteredValidator {
                    id,
                    name: name.trim().into(),
                    tool,
                    registered_by: "user".into(),
                    registered_at: chrono::Utc::now().to_rfc3339(),
                },
            )
        }
        #[cfg(not(windows))]
        {
            let _ = (id, name, executable, inputs, environment);
            Err("validator registration requires Windows".into())
        }
    }
    pub fn registered_validator(&self, id: &str) -> Result<&RegisteredValidator, String> {
        self.validators
            .get(id)
            .ok_or_else(|| "验收工具未登记".into())
    }
    fn register(&mut self, mut project: Project) -> Result<(), String> {
        identifier(&project.id)?;
        cerulean_domain::timezone::parse(&project.timezone)?;
        project.root = crate::artifacts::root_path(&project.root)?
            .to_string_lossy()
            .into();
        for parent in self
            .project_setups
            .values()
            .filter_map(|s| {
                s.setup
                    .worker
                    .as_ref()
                    .and_then(|w| w.omp_runtime_parent.as_deref())
            })
            .chain(
                self.omp_startups
                    .all()
                    .into_iter()
                    .filter_map(|s| s.intent.worker.omp_runtime_parent.as_deref()),
            )
        {
            require(
                !crate::artifacts::contains_path(Path::new(parent), Path::new(&project.root))
                    && !crate::artifacts::contains_path(
                        Path::new(&project.root),
                        Path::new(parent),
                    ),
                "项目不能与已配置或历史OMP运行目录重叠",
            )?;
        }
        #[cfg(windows)]
        for snapshot in self
            .active
            .values()
            .map(|a| &a.snapshot)
            .chain(self.codex_active.values().map(|a| &a.snapshot))
            .chain(self.omp_active.values().map(|a| &a.snapshot))
            .chain(self.api_writers.values().map(|a| &a.snapshot))
            .chain(self.desktop_active.iter().map(|a| &a.snapshot))
        {
            require(
                snapshot.project.id != project.id,
                "project currently verifying",
            )?;
            require(
                !crate::artifacts::contains_path(
                    Path::new(&snapshot.project.root),
                    Path::new(&project.root),
                ) && !crate::artifacts::contains_path(
                    Path::new(&project.root),
                    Path::new(&snapshot.project.root),
                ),
                "workspace overlaps verifying project",
            )?;
        }
        if let Some(store) = self.projects.get(&project.id) {
            return require(
                store
                    .configuration()
                    .execution_scope
                    .as_ref()
                    .unwrap()
                    .project
                    == project,
                "project ID already binds a different scope",
            );
        }
        for store in self.projects.values() {
            let registered = &store
                .configuration()
                .execution_scope
                .as_ref()
                .unwrap()
                .project
                .root;
            require(
                !crate::artifacts::contains_path(Path::new(registered), Path::new(&project.root))
                    && !crate::artifacts::contains_path(
                        Path::new(&project.root),
                        Path::new(registered),
                    ),
                "workspace overlaps registered project",
            )?;
            require(
                store
                    .configuration()
                    .execution_scope
                    .as_ref()
                    .unwrap()
                    .project
                    .root
                    .to_lowercase()
                    != project.root.to_lowercase(),
                "workspace already registered",
            )?;
        }
        let id = project.id.clone();
        let store = DurableWorkflow::open(&self.root.join("workflows").join(&id), config(project))?;
        self.projects.insert(id, store);
        Ok(())
    }
    /// Internal chief callback only; absent from the user IPC enum.
    pub fn propose(
        &mut self,
        project_id: &str,
        operation_id: &str,
        revision: PlanRevision,
    ) -> Result<(), String> {
        crate::reader_generation::require_revision(&self.root, &revision)?;
        crate::worker_boundaries::validate(&revision, false)?;
        self.poll_verifications();
        self.require_available(project_id)?;
        require(
            revision.snapshot_policy.is_some(),
            "new execution proposals require a snapshot policy",
        )?;
        require(
            revision
                .execution
                .iter()
                .all(|t| t.checks.iter().all(|c| c.tool.is_some())),
            "新执行方案必须绑定实际验收程序及依赖摘要",
        )?;
        require(
            revision.project.id == project_id,
            "proposal project differs from callback scope",
        )?;
        let store = self
            .projects
            .get_mut(project_id)
            .ok_or("project not registered")?;
        for execution in &revision.execution {
            require_bound_worker(store.configuration(), &execution.adapter_id)?;
        }
        store.command(
            operation_id,
            "chief",
            WorkflowCommand::Propose {
                plan: revision.workflow.clone(),
            },
            CommandContext {
                execution: Some(ExecutionEvidence::Revision { revision }),
                ..Default::default()
            },
            false,
        )?;
        Ok(())
    }
    fn approve(&mut self, project: &str, wanted: &str) -> Result<(), String> {
        self.approve_scoped(project, wanted, None)
    }
    fn approve_scoped(
        &mut self,
        project: &str,
        wanted: &str,
        worker_account_setup: Option<WorkerAccountAuthorization>,
    ) -> Result<(), String> {
        let store = self.projects.get(project).ok_or("project not registered")?;
        let revision = store.revision().ok_or("chief proposal missing")?;
        require(
            revision.approval_digest()? == wanted,
            "计划内容或权限已变化，请重新审阅",
        )?;
        if let Some(prior) = store.approval() {
            require(
                worker_account_setup.is_none()
                    || prior.worker_account_setup == worker_account_setup,
                "原批准不包含这次管理授权；请重新审阅新方案，不能追加或扩大原授权",
            )?;
            return self.reconcile_currency_account(project);
        }
        crate::worker_boundaries::validate(revision, false)?;
        let currency_approval = self.prepare_currency_approval(project)?;
        if worker_account_setup.is_some() {
            crate::worker_account_guard::enable(&self.root)?;
        }
        let store = self
            .projects
            .get_mut(project)
            .ok_or("project not registered")?;
        let revision = store.revision().ok_or("chief proposal missing")?;
        let approval = Approval {
            worker_account_setup,
            plan_id: revision.workflow.id.clone(),
            revision: revision.workflow.version,
            policy_version: revision.policy.version,
            revision_digest: wanted.into(),
            user_id: "user".into(),
            approved_at: chrono::Utc::now().to_rfc3339(),
        };
        store.command(
            &format!("user-approve-{}", store.history().len() + 1),
            "user",
            WorkflowCommand::Approve {
                digest: store
                    .state()
                    .digest
                    .clone()
                    .ok_or("workflow digest missing")?,
            },
            CommandContext {
                currency_approval,
                approval_source: Some("authenticated local-user service request".into()),
                execution: Some(ExecutionEvidence::Approval { approval }),
                ..Default::default()
            },
            false,
        )?;
        self.reconcile_currency_account(project)
    }
    fn require_available(&self, project: &str) -> Result<(), String> {
        self.require_available_inner(project, false)
    }
    fn require_available_inner(&self, project: &str, allow_api: bool) -> Result<(), String> {
        self.require_available_for_recheck(project, allow_api, None)
    }
    fn require_available_for_recheck(
        &self,
        project: &str,
        allow_api: bool,
        recheck: Option<&str>,
    ) -> Result<(), String> {
        #[cfg(windows)]
        require(
            !crate::windows_worker_account::views(&self.root)
                .iter()
                .any(|v| v.project_id == project && v.state == "pending"),
            "Windows账户设置仍在进行或结果未明，暂不修改或启动项目",
        )?;
        require(
            match recheck {
                None => self
                    .projects
                    .get(project)
                    .is_none_or(|s| s.unresolved_verification_recheck().is_none()),
                Some(operation) => self
                    .projects
                    .get(project)
                    .and_then(|s| s.unresolved_verification_recheck())
                    .and_then(|e| e.body.context.verification_recheck.as_ref())
                    .is_some_and(|a| a.operation_id == operation),
            },
            "项目仍有未决复验批准，请核对原操作；禁止重新派发",
        )?;
        require(
            !self.counting_project(project),
            "项目指挥计数仍在运行，请等待原操作",
        )?;
        if let Some(error) = self.management_count_errors.get(project) {
            return Err(format!("管理预检需要审阅，未自动重试：{error}"));
        }
        #[cfg(windows)]
        require(
            self.desktop_active
                .as_ref()
                .is_none_or(|a| a.snapshot.project.id != project),
            "桌面任务执行或回读中，请等待原操作",
        )?;
        #[cfg(windows)]
        require(
            self.api_fault.is_none()
                && self
                    .desktop_active
                    .as_ref()
                    .is_none_or(|a| a.snapshot.project.id != project)
                && (allow_api || !self.has_api_activity(project))
                && !self
                    .api_jobs
                    .values()
                    .any(|j| j.project_id == project && j.state == api_jobs::ApiJobState::Unknown),
            "API尝试执行中或需要核对；禁止重派",
        )?;
        require(self.advice_closures.healthy(), "建议处置账本需要重开恢复")?;
        require(
            self.startups.healthy() && self.omp_startups.healthy(),
            "启动账本需要重开恢复",
        )?;
        require(
            !self.omp_jobs.values().any(|j| {
                j.project_id == project
                    && matches!(
                        j.state,
                        omp_jobs::OmpJobState::Unknown | omp_jobs::OmpJobState::Failed
                    )
            }),
            "OMP尝试需要核对或关闭，禁止重派",
        )?;
        require(
            !self.startups.all().iter().any(|record| {
                record.intent.project_id == project
                    && record.closure.is_none()
                    && !self
                        .codex_jobs
                        .contains_key(&(project.into(), record.intent.attempt_id.clone()))
                    && record
                        .intent
                        .management_operation
                        .as_ref()
                        .is_none_or(|op| self.management.application(op).is_none())
                    && self.projects.get(project).is_none_or(|store| {
                        !store.history().iter().any(|entry| {
                            entry.body.event.command.attempt()
                                == Some(record.intent.attempt_id.as_str())
                        })
                    })
            }),
            "存在未完成的启动意图，请先审阅启动记录",
        )?;
        #[cfg(windows)]
        require(
            (!self.active.contains_key(project)
                || (allow_api
                    && self.active.get(project).is_some_and(|a| a.shared)
                    && self.api_writers.contains_key(project)))
                && (!self.has_codex_activity(project) || allow_api)
                && (allow_api || !self.has_omp_activity(project))
                && self
                    .desktop_active
                    .as_ref()
                    .is_none_or(|a| a.snapshot.project.id != project),
            "项目执行或验收中，请等待当前操作",
        )?;
        require(
            !self
                .codex_jobs
                .values()
                .any(|j| j.project_id == project && j.state == CodexJobState::Unknown),
            "项目执行结果待对账，禁止重派",
        )?;
        require(
            !self.jobs.values().any(|j| {
                j.project_id == project
                    && j.state == VerificationState::Unknown
                    && Some(j.operation_id.as_str()) != recheck
            }),
            "项目存在待对账验收，禁止自动重跑",
        )
    }
    #[cfg(windows)]
    pub fn start_verification(
        &mut self,
        project: &str,
        operation: &str,
        task: &str,
        attempt: &str,
        pins: Vec<crate::verifier::PinnedCheck>,
    ) -> Result<(), String> {
        self.poll_verifications();
        let shared = self.api_writers.contains_key(project);
        self.require_available_inner(project, shared)?;
        require(
            !self.active.contains_key(project),
            "本项目已有独立验收运行中",
        )?;
        require(
            self.active.len()
                + self.codex_active.len()
                + self.omp_active.len()
                + self.api_active.len()
                + usize::from(self.desktop_active.is_some())
                < 4,
            "最多同时运行 4 个后台项目操作",
        )?;
        require(
            self.jobs
                .values()
                .filter(|job| job.project_id == project)
                .count()
                < 1000,
            "验收记录达到当前版本上限，需归档功能后继续",
        )?;
        for id in [project, operation, task, attempt] {
            identifier(id)?;
        }
        require(
            !self.jobs.contains_key(&(project.into(), operation.into())),
            "verification operation already used",
        )?;
        let state = if let Some(writer) = self.api_writers.get(project) {
            self.require_shared_verification_scope(project, task, attempt)?;
            writer.owner.snapshot()?.state
        } else {
            self.projects
                .get(project)
                .ok_or("project not registered")?
                .state()
                .clone()
        };
        let run = state.tasks.get(task).ok_or("task missing")?;
        require(
            state.revision_requested.is_none()
                && run.state == TaskState::Submitted
                && run.attempt.as_deref() == Some(attempt)
                && !run.verified,
            "submitted unverified attempt required",
        )?;
        let now = chrono::Utc::now().to_rfc3339();
        let job = VerificationJob {
            project_id: project.into(),
            operation_id: operation.into(),
            task_id: task.into(),
            attempt_id: attempt.into(),
            state: VerificationState::Running,
            started_at: now.clone(),
            updated_at: now,
            message: None,
            reconciliation_digest: None,
        };
        self.save_verification_job(&job)?;
        self.jobs
            .insert((project.into(), operation.into()), job.clone());
        if shared {
            return self.launch_shared_verification(job, pins);
        }
        let store = self.projects.remove(project).expect("checked project");
        let config = store.configuration().clone();
        match crate::async_verification::launch(&self.root, store, job.clone(), pins) {
            Ok(active) => {
                self.active.insert(project.into(), active);
                Ok(())
            }
            Err(error) => {
                let mut job = job;
                job.state = VerificationState::Unknown;
                job.message = Some(error.clone());
                self.jobs
                    .insert((project.into(), operation.into()), job.clone());
                let _ = self.save_verification_job(&job);
                let store =
                    DurableWorkflow::open(&self.root.join("workflows").join(project), config)?;
                self.projects.insert(project.into(), store);
                Err(error)
            }
        }
    }
    /// Trusted deputy callback only. UI callers cannot supply roles or executable commands.
    #[cfg(windows)]
    pub fn start_codex(
        &mut self,
        project: &str,
        task: &str,
        attempt: &str,
        revision_digest: &str,
    ) -> Result<(), String> {
        self.start_codex_inner(project, task, attempt, revision_digest, None)
    }
    #[cfg(windows)]
    fn start_codex_inner(
        &mut self,
        project: &str,
        task: &str,
        attempt: &str,
        revision_digest: &str,
        management_operation: Option<String>,
    ) -> Result<(), String> {
        self.poll_verifications();
        self.require_available_inner(project, true)?;
        self.require_task_resources(project, task, attempt)?;
        self.require_execution_boundaries(project)?;
        self.management.check_attempt_reservation(
            project,
            attempt,
            management_operation.as_deref(),
        )?;
        require(
            self.startups.get(project, attempt).is_none()
                && self.omp_startups.get(project, attempt).is_none(),
            "启动尝试ID已永久保留，不能重用",
        )?;
        self.startups
            .check_reservation(project, attempt, management_operation.as_deref())?;
        self.omp_startups
            .check_reservation(project, attempt, management_operation.as_deref())?;
        require(
            self.codex_active.len()
                + self.active.len()
                + self.omp_active.len()
                + self.api_active.len()
                + usize::from(self.desktop_active.is_some())
                < 4,
            "后台执行数量达到上限",
        )?;
        require(
            !self
                .codex_jobs
                .contains_key(&(project.into(), attempt.into())),
            "Codex attempt already used; never resend",
        )?;
        self.require_worker_slot()?;
        require(
            self.codex_jobs
                .values()
                .filter(|j| j.project_id == project)
                .count()
                < 1000,
            "Codex job history limit",
        )?;
        let view = self.codex_read(project, task, attempt)?;
        let store = &*view;
        let worker = crate::async_codex::preflight(store, task, attempt, revision_digest)?;
        let scoped = management_operation.is_some()
            && api_assignment_scope::isolated(store.revision().ok_or("Codex方案缺失")?, task)?;
        if self.api_writers.contains_key(project) {
            require(scoped, "Codex并行启动需要批准的独立目录与原管理决定")?;
        }
        let anchor = if scoped {
            self.validate_api_assignment_scope(
                management_operation.as_deref().unwrap(),
                store,
                task,
                store.history().len() as u32,
                true,
            )?
            .sequence
        } else {
            store.history().len() as u32
        };
        let now = chrono::Utc::now().to_rfc3339();
        let job = CodexJob {
            management_operation,
            project_id: project.into(),
            task_id: task.into(),
            attempt_id: attempt.into(),
            revision_digest: revision_digest.into(),
            worker,
            state: CodexJobState::Starting,
            started_at: now.clone(),
            updated_at: now,
            message: None,
        };
        let management_hash = if let Some(operation) = &job.management_operation {
            let intent = self.management.intent(operation)?;
            require(
                intent.project_id == project && self.management.application(operation).is_none(),
                "启动管理归属不符或已有应用",
            )?;
            let observation = self.management_envelope(operation)?;
            require(
                matches!(&observation.decision,
                cerulean_domain::management::ManagementDecision::Assign { task_id, worker_id, .. }
                    if task_id == task && worker_id == "worker-codex"),
                "启动与副指挥分配不符",
            )?;
            Some(self.management.response_head(operation)?.to_owned())
        } else {
            None
        };
        let intent = crate::startup_journal::StartupIntent {
            schema_version: if scoped { 2 } else { 1 },
            protocol: "codex-durable-before-send-v1".into(),
            project_id: project.into(),
            task_id: task.into(),
            attempt_id: attempt.into(),
            revision_digest: revision_digest.into(),
            worker: job.worker.clone(),
            management_operation: job.management_operation.clone(),
            management_hash,
            workflow_sequence: anchor,
            workflow_hash: store
                .history()
                .get(anchor.checked_sub(1).ok_or("Codex批准锚点缺失")? as usize)
                .map(|e| e.hash.clone()),
            started_at: job.started_at.clone(),
        };
        drop(view);
        self.startups.begin(intent)?;
        self.launch_codex_job(job, false)
    }
    fn reconcile_codex(&mut self, project: &str, attempt: &str) -> Result<(), String> {
        #[cfg(windows)]
        {
            identifier(project)?;
            identifier(attempt)?;
            require(
                !self.active.contains_key(project)
                    && !self.has_codex_activity(project)
                    && !self.has_omp_activity(project)
                    && self
                        .desktop_active
                        .as_ref()
                        .is_none_or(|a| a.snapshot.project.id != project),
                "项目仍有后台操作",
            )?;
            require(
                self.codex_active.len()
                    + self.active.len()
                    + self.omp_active.len()
                    + self.api_active.len()
                    + usize::from(self.desktop_active.is_some())
                    < 4,
                "后台执行数量达到上限",
            )?;
            let mut job = self
                .codex_jobs
                .get(&(project.into(), attempt.into()))
                .ok_or("Codex job missing")?
                .clone();
            require(job.state == CodexJobState::Unknown, "仅不确定执行可以对账")?;
            let store = self
                .projects
                .get(project)
                .ok_or("project ledger unavailable")?;
            require(
                store.state().tasks.get(&job.task_id).is_some_and(|t| {
                    t.state == TaskState::Unknown && t.attempt.as_deref() == Some(attempt)
                }),
                "没有可读回的原始尝试；需检查启动失败证据",
            )?;
            job.state = CodexJobState::Reconciling;
            job.message = None;
            job.updated_at = chrono::Utc::now().to_rfc3339();
            self.launch_codex_job(job, true)
        }
        #[cfg(not(windows))]
        {
            let _ = (project, attempt);
            Err("Codex execution requires Windows".into())
        }
    }
    #[cfg(windows)]
    fn launch_codex_job(&mut self, job: CodexJob, recovery: bool) -> Result<(), String> {
        let project = job.project_id.clone();
        let key = (project.clone(), job.attempt_id.clone());
        crate::async_codex::save(&self.root, &job)?;
        self.codex_jobs.insert(key.clone(), job.clone());
        let mut new_config = None;
        let launched = (|| {
            if !self.api_writers.contains_key(&project) {
                let store = self.projects.remove(&project).ok_or("Codex项目不可用")?;
                new_config = Some(store.configuration().clone());
                let writer = crate::async_api::ApiWorkflow::start(
                    store,
                    self.root.join("workflows").join(&project),
                )?;
                self.api_writers.insert(project.clone(), writer);
            }
            let client = self.api_writers[&project].owner.codex_client(
                crate::workflow_coordinator::Scope {
                    project: project.clone(),
                    task: job.task_id.clone(),
                    attempt: job.attempt_id.clone(),
                    revision: job.revision_digest.clone(),
                },
            )?;
            crate::async_codex::launch(
                &self.root,
                client,
                job.clone(),
                recovery,
                #[cfg(test)]
                self.codex_prepare_gate.take(),
            )
        })();
        match launched {
            Ok(active) => {
                self.codex_active.insert(key, active);
                Ok(())
            }
            Err(error) => {
                if let Some(config) = new_config {
                    let store = match self.api_writers.remove(&project) {
                        Some(mut writer) => writer.owner.finish(false),
                        None => DurableWorkflow::open(
                            &self.root.join("workflows").join(&project),
                            config,
                        ),
                    }?;
                    self.projects.insert(project, store);
                }
                let mut failed = job;
                failed.state = CodexJobState::Failed;
                failed.message = Some(error.clone());
                self.codex_jobs.insert(key, failed.clone());
                let _ = crate::async_codex::save(&self.root, &failed);
                Err(error)
            }
        }
    }
    #[cfg(windows)]
    fn poll_codex_workers(&mut self) {
        use crate::async_codex::Update;
        let mut ready = vec![];
        let mut assignments = vec![];
        for ((project, attempt), active) in &mut self.codex_active {
            crate::worker_observation::persist(&self.root, active);
            if active.disconnected {
                continue;
            }
            loop {
                match active.receiver.try_recv() {
                    Ok(
                        update @ (Update::PrepareAssignment { .. }
                        | Update::CommitAssignment { .. }
                        | Update::Advice(_)),
                    ) => {
                        assignments.push((project.clone(), attempt.clone(), update));
                        break;
                    }
                    Ok(Update::Progress(snapshot)) => {
                        if let Some(writer) = self.api_writers.get(project) {
                            if let Ok(authoritative) = writer.owner.snapshot() {
                                active.snapshot = authoritative;
                            }
                        } else if snapshot.sequence >= active.snapshot.sequence {
                            active.snapshot = *snapshot;
                        }
                        active.job.state = if active
                            .snapshot
                            .state
                            .tasks
                            .get(&active.job.task_id)
                            .is_some_and(|task| task.state == TaskState::Cancelling)
                        {
                            CodexJobState::Cancelling
                        } else if matches!(
                            active.job.state,
                            CodexJobState::CancelRequested | CodexJobState::Cancelling
                        ) {
                            active.job.state.clone()
                        } else {
                            CodexJobState::Executing
                        };
                        if active.job.state == CodexJobState::Cancelling {
                            active.job.message =
                                Some("停止意图已保存，等待执行器终态核对；后续调度已暂停".into());
                        }
                        active.job.updated_at = chrono::Utc::now().to_rfc3339();
                        self.codex_jobs.insert(
                            (project.clone(), active.job.attempt_id.clone()),
                            active.job.clone(),
                        );
                    }
                    Ok(Update::Done {
                        interrupted,
                        cleaned,
                        outcome,
                    }) => {
                        ready.push((
                            project.clone(),
                            attempt.clone(),
                            interrupted,
                            cleaned,
                            outcome,
                        ));
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        active.disconnected = true;
                        active.job.state = CodexJobState::Unknown;
                        active.job.message = Some("Codex后台通道中断，需要重开核查".into());
                        self.codex_jobs
                            .insert((project.clone(), attempt.clone()), active.job.clone());
                        break;
                    }
                }
            }
        }
        for (project, attempt, update) in assignments {
            match update {
                Update::Advice(update) => self.handle_advice_update(&project, &attempt, update),
                Update::PrepareAssignment {
                    operation,
                    attempt,
                    snapshot,
                    reply,
                } => {
                    let result = self.prepare_managed_codex_assignment(
                        &project, &operation, *attempt, *snapshot,
                    );
                    let _ = reply.send(result);
                }
                Update::CommitAssignment {
                    operation,
                    entry,
                    snapshot,
                    reply,
                } => {
                    let result = self
                        .commit_managed_codex_assignment(&project, &operation, *entry, *snapshot);
                    let _ = reply.send(result);
                }
                _ => unreachable!(),
            }
        }
        for (project, attempt, interrupted, cleaned, outcome) in ready {
            let key = (project.clone(), attempt.clone());
            let mut active = self.codex_active.remove(&key).expect("active Codex worker");
            crate::worker_observation::persist(&self.root, &mut active);
            let mut job = active.job.clone();
            job.updated_at = chrono::Utc::now().to_rfc3339();
            let stopping = matches!(
                job.state,
                CodexJobState::CancelRequested | CodexJobState::Cancelling
            );
            if !cleaned {
                job.state = CodexJobState::Unknown;
                job.message = Some("Codex进程退出尚未确认，保留原活动与账本所有者".into());
                if let Some(writer) = self.api_writers.get_mut(&project) {
                    writer.interrupted = true;
                }
                active.disconnected = true;
                active.job = job.clone();
                self.codex_active.insert(key.clone(), active);
            } else if let Err(error) =
                self.finish_codex_owner(&project, &mut job, interrupted, outcome)
            {
                job.state = CodexJobState::Unknown;
                job.message = Some(error.clone());
                self.api_fault = Some(error);
                active.disconnected = true;
                active.job = job.clone();
                self.codex_active.insert(key.clone(), active);
            } else if job.state == CodexJobState::Cancelled {
                job.message = Some("原会话已确认本次执行停止；已有修改保留，后续调度仍暂停".into());
            } else if stopping && job.state == CodexJobState::Submitted {
                job.message = Some("任务已先完成，结果已保留并等待独立验收；后续调度仍暂停".into());
            }
            if let Err(error) = crate::async_codex::save(&self.root, &job) {
                self.api_fault = Some(error);
                job.state = CodexJobState::Unknown;
                job.message = Some("执行状态保存失败，请重开核心对账".into());
            }
            self.codex_jobs.insert(key, job);
        }
    }
    pub fn poll_verifications(&mut self) {
        #[cfg(windows)]
        {
            self.poll_functional_test();
            if self.functional_active.is_some() {
                return;
            }
        }
        #[cfg(windows)]
        self.poll_blind_review();
        if self
            .blind_reviews
            .rounds
            .values()
            .any(|r| r.phase == cerulean_domain::blind_review::ReviewPhase::Collecting)
        {
            return;
        }
        #[cfg(windows)]
        self.baselines.poll();
        if !self.omp_stop_issues.is_empty() {
            return;
        }
        self.poll_management();
        self.poll_provider_models();
        #[cfg(windows)]
        self.poll_desktop();
        if let Some(receiver) = &self.openai_probe {
            let outcome = match receiver.try_recv() {
                Ok(r) => Some(r),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Some(Err("OpenAI 检查中断".into()))
                }
            };
            if let Some(outcome) = outcome {
                self.openai_probe = None;
                self.openai_connection.checked_at = Some(chrono::Utc::now().to_rfc3339());
                match outcome {
                    Ok(models) => {
                        self.openai_connection.state = "available".into();
                        self.openai_connection.models = models;
                        self.openai_connection.error = None;
                    }
                    Err(error) => {
                        self.openai_connection.state = "failed".into();
                        self.openai_connection.error = Some(error);
                    }
                }
            }
        }
        #[cfg(windows)]
        self.poll_codex_workers();
        #[cfg(windows)]
        self.poll_omp();
        #[cfg(windows)]
        self.poll_api();
        if let Some(receiver) = &self.codex_probe {
            let completed = match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Codex connection check interrupted".into()))
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
            };
            if let Some(result) = completed {
                self.codex_probe = None;
                if let Some(status) = &mut self.codex_connection {
                    status.checked_at = chrono::Utc::now().to_rfc3339();
                    match result {
                        Ok(connection) => {
                            status.state = if connection.account_type.is_some() {
                                "available"
                            } else {
                                "loginRequired"
                            }
                            .into();
                            status.server_version = Some(connection.server_version);
                            status.account_type = connection.account_type;
                            status.models =
                                connection.models.into_iter().map(|m| m.model).collect();
                        }
                        Err(error) => {
                            status.state = "failed".into();
                            status.error = Some(error);
                        }
                    }
                }
            }
        }
        #[cfg(windows)]
        {
            let ready: Vec<_> = self
                .active
                .iter()
                .filter_map(|(project, active)| match active.receiver.try_recv() {
                    Ok(result) => Some((project.clone(), result)),
                    Err(std::sync::mpsc::TryRecvError::Empty) => None,
                    Err(std::sync::mpsc::TryRecvError::Disconnected)
                        if active.job.state != VerificationState::Unknown =>
                    {
                        Some((
                            project.clone(),
                            crate::async_verification::Completion {
                                store: if active.shared {
                                    None
                                } else {
                                    Some(Err(
                                        "verification worker disappeared; reconciliation required"
                                            .into(),
                                    ))
                                },
                                outcome: Err("verification result missing".into()),
                                uncertain: true,
                            },
                        ))
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => None,
                })
                .collect();
            for (project, completion) in ready {
                let mut active = self.active.remove(&project).expect("active verification");
                if active.shared {
                    self.finish_shared_verification(&project, active, completion);
                    continue;
                }
                let mut job = active.job.clone();
                job.updated_at = chrono::Utc::now().to_rfc3339();
                match completion
                    .store
                    .unwrap_or_else(|| Err("owned verification lost its ledger".into()))
                {
                    Ok(store) => {
                        job.state = if completion.uncertain {
                            VerificationState::Unknown
                        } else {
                            match &completion.outcome {
                                Ok(true) => VerificationState::Succeeded,
                                Ok(false) => VerificationState::Failed,
                                Err(message) if message.contains("panicked") => {
                                    VerificationState::Unknown
                                }
                                Err(_) => VerificationState::Failed,
                            }
                        };
                        job.message = completion.outcome.err();
                        self.projects.insert(project.clone(), store);
                    }
                    Err(error) => {
                        job.state = VerificationState::Unknown;
                        job.message = Some(error);
                        active.job = job.clone();
                        self.active.insert(project.clone(), active);
                    }
                }
                if let Err(error) = self.save_verification_job(&job) {
                    job.state = VerificationState::Unknown;
                    job.message = Some(format!("验收状态保存失败，需要对账: {error}"));
                }
                self.jobs.insert((project, job.operation_id.clone()), job);
            }
        }
        #[cfg(windows)]
        self.poll_project_runs();
        self.poll_team_timing_proposals();
    }
    fn abandon_verification(
        &mut self,
        project: &str,
        operation: &str,
        wanted: &str,
        reason: &str,
    ) -> Result<(), String> {
        #[cfg(windows)]
        {
            require(
                !self.active.contains_key(project),
                "验收线程尚未结束，不能放弃结果",
            )?;
            let key = (project.to_string(), operation.to_string());
            let job = self.jobs.get(&key).ok_or("验收记录不存在")?.clone();
            require(
                !self.is_verification_recheck(project, operation),
                "复验须核对原批准，不可使用旧验收放弃路径",
            )?;
            let store = self
                .projects
                .get(project)
                .ok_or("项目账本尚未恢复，请先重开核心")?;
            require(
                !store
                    .history()
                    .iter()
                    .any(|e| e.body.operation_id == operation),
                "已有账本验收结果，不能放弃为未知结果",
            )?;
            if job.state == VerificationState::Abandoned {
                require(
                    crate::verification_reconciliation::applies(&self.root, &job)?,
                    "放弃记录缺失",
                )?;
                return Ok(());
            }
            require(
                job.state == VerificationState::Unknown,
                "仅待对账验收可以放弃",
            )?;
            crate::verification_reconciliation::abandon(&self.root, &job, store, wanted, reason)?;
            let mut job = job;
            job.state = VerificationState::Abandoned;
            job.updated_at = chrono::Utc::now().to_rfc3339();
            job.reconciliation_digest = None;
            job.message = Some("用户已放弃本次验收；保留证据，任务未通过。".into());
            self.jobs.insert(key, job.clone());
            self.save_verification_job(&job)
        }
        #[cfg(not(windows))]
        {
            let _ = (project, operation, wanted, reason);
            Err("verification unsupported on this platform".into())
        }
    }
    fn cancel_verification(&mut self, project: &str, operation: &str) -> Result<(), String> {
        #[cfg(windows)]
        {
            let active = self.active.get(project).ok_or("此项目没有运行中的验收")?;
            require(
                active.job.operation_id == operation
                    && matches!(
                        active.job.state,
                        VerificationState::Running | VerificationState::Cancelling
                    ),
                "验收操作不匹配或已结束",
            )?;
            let mut job = active.job.clone();
            job.state = VerificationState::Cancelling;
            job.updated_at = chrono::Utc::now().to_rfc3339();
            self.save_verification_job(&job)?;
            let active = self.active.get_mut(project).expect("original verification");
            active
                .cancel
                .store(true, std::sync::atomic::Ordering::Release);
            active.job = job.clone();
            self.jobs.insert((project.into(), operation.into()), job);
            Ok(())
        }
        #[cfg(not(windows))]
        {
            let _ = (project, operation);
            Err("verification unsupported on this platform".into())
        }
    }
    pub fn can_shutdown(&mut self) -> bool {
        if !self.blind_reviews.healthy() {
            return false;
        }
        #[cfg(windows)]
        if self.blind_active.is_some() || self.functional_active.is_some() {
            return false;
        }
        #[cfg(windows)]
        {
            self.baselines.poll();
            if self.baselines.busy() {
                return false;
            }
        }
        #[cfg(windows)]
        if self.desktop_probe.is_some() || self.desktop_active.is_some() {
            return false;
        }
        #[cfg(windows)]
        if self.api_fault.is_some()
            || !self.api_active.is_empty()
            || self
                .api_jobs
                .values()
                .any(|j| j.state == api_jobs::ApiJobState::Unknown)
        {
            return false;
        }
        if !self.omp_stop_issues.is_empty() {
            // Diagnostic mode only starts on reopening, with no live children.
            return true;
        }
        self.poll_verifications();
        if self
            .runs
            .all()
            .iter()
            .any(|r| r.state == project_setup::RunState::Active)
        {
            return false;
        }
        if self.codex_probe.is_some()
            || self.openai_probe.is_some()
            || !self.provider_probes.is_empty()
            || self.management_work_count() != 0
        {
            return false;
        }
        #[cfg(windows)]
        if !self.active.is_empty()
            || !self.codex_active.is_empty()
            || !self.omp_active.is_empty()
            || self.omp_jobs.values().any(|j| {
                matches!(
                    j.state,
                    omp_jobs::OmpJobState::Unknown | omp_jobs::OmpJobState::Failed
                )
            })
            || self
                .codex_jobs
                .values()
                .any(|j| j.state == CodexJobState::Unknown)
        {
            return false;
        }
        if self
            .jobs
            .values()
            .any(|j| j.state == VerificationState::Unknown)
        {
            return false;
        }
        self.projects.iter().all(|(project, s)| {
            // Retirement deliberately preserves the original Submitted state
            // and verification evidence. Only its authenticated, current-plan
            // disposition permits shutdown; a plain pause/revision is insufficient.
            let submitted_retired = s.state().revision_requested.as_ref().is_some_and(|wanted| {
                s.state().digest.as_ref() == Some(wanted)
                    && s.history().iter().any(|entry| {
                        entry.body.event.principal == "user"
                            && entry.body.context.approval_source.as_deref()
                                == Some("authenticated local-user submitted-plan retirement")
                            && matches!(&entry.body.event.command,
                                WorkflowCommand::RetireSubmittedPlan { digest, .. } if digest == wanted)
                    })
            });
            // A sealed failed experiment cannot retire its immutable plan.
            // Recognize its unchanged closure solely for normal shutdown.
            #[cfg(windows)]
            let experiment_preserved = s.state().tasks.values()
                .any(|t| t.state == TaskState::Submitted)
                && self.closed_failed_team_preserved(project).is_ok();
            #[cfg(not(windows))]
            let experiment_preserved = { let _ = project; false };
            s.state().tasks.values().all(|t| {
                matches!(
                    t.state,
                    TaskState::Pending
                        | TaskState::Accepted
                        | TaskState::Cancelled
                        | TaskState::Retired
                ) || ((submitted_retired || experiment_preserved) && t.state == TaskState::Submitted)
            })
        })
    }
    pub fn can_restart_reviewed_stops(&self) -> bool {
        #[cfg(windows)]
        if self.baselines.busy() {
            return false;
        }
        !self.omp_stop_issues.is_empty() && self.omp_stop_issues.iter().all(|i| i.acknowledged)
    }
    pub fn handle(&mut self, request: Request) -> Response {
        self.handle_with_snapshot(request, true)
    }
    pub(crate) fn handle_command(&mut self, request: Request) -> Response {
        if !request.command.supports_receipt() {
            return Response {
                id: request.id,
                result: None,
                error: Some(ApiError {
                    code: "receiptUnsupported".into(),
                    message: "此操作需要原查询/详情接口，未执行".into(),
                }),
            };
        }
        self.handle_with_snapshot(request, false)
    }
    fn handle_with_snapshot(&mut self, request: Request, include_snapshot: bool) -> Response {
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
        #[cfg(windows)]
        if (self.functional_active.is_some()
            || self
                .blind_reviews
                .rounds
                .values()
                .any(|r| r.phase == cerulean_domain::blind_review::ReviewPhase::Collecting))
            && !matches!(
                &request.command,
                Command::Snapshot
                    | Command::BlindReview {
                        action: cerulean_domain::blind_review::BlindReviewAction::Cancel { .. }
                    }
            )
        {
            return Response {
                id: request.id,
                result: None,
                error: Some(ApiError {
                    code: "functionalTestActive".into(),
                    message: "主指挥整体功能测试进行中，请等待或取消原操作后修改配置及派发任务"
                        .into(),
                }),
            };
        }
        #[cfg(windows)]
        if !matches!(request.command, Command::Snapshot)
            && crate::team_import_service::unresolved(&self.root)
        {
            return Response {
                id: request.id,
                result: None,
                error: Some(ApiError {
                    code: "teamImportUnresolved".into(),
                    message: "原团队材料导入尚未完成核对；只读保留原记录，不重复导入或派发".into(),
                }),
            };
        }
        if let Command::AcknowledgeOmpStopIssue {
            project_id,
            attempt_id,
            evidence_digest,
            reason,
        } = &request.command
        {
            #[cfg(windows)]
            let result =
                self.acknowledge_omp_stop_issue(project_id, attempt_id, evidence_digest, reason);
            #[cfg(not(windows))]
            let result: Result<(), String> = {
                let _ = (project_id, attempt_id, evidence_digest, reason);
                Err("OMP恢复需要Windows".into())
            };
            return match result {
                Ok(()) => Response {
                    id: request.id,
                    result: include_snapshot.then(|| self.snapshot()),
                    error: None,
                },
                Err(message) => Response {
                    id: request.id,
                    result: None,
                    error: Some(ApiError {
                        code: "stopReviewRejected".into(),
                        message,
                    }),
                },
            };
        }
        if !self.omp_stop_issues.is_empty() {
            return if matches!(request.command, Command::Snapshot) {
                Response {
                    id: request.id,
                    result: include_snapshot.then(|| self.snapshot()),
                    error: None,
                }
            } else {
                Response {
                    id: request.id,
                    result: None,
                    error: Some(ApiError {
                        code: "diagnosticMode".into(),
                        message:
                            "停止记录需要复核，核心处于诊断模式；当前仅允许读取快照与确认中断记录"
                                .into(),
                    }),
                }
            };
        }
        if matches!(
            request.command,
            Command::CancelApi { .. }
                | Command::RecoverApi { .. }
                | Command::AbandonApiStartup { .. }
                | Command::RetireUnknownApi { .. }
        ) {
            #[cfg(windows)]
            let result = match &request.command {
                Command::RetireUnknownApi {
                    project_id,
                    attempt_id,
                    evidence_digest,
                    reason,
                } => self.retire_unknown_api(project_id, attempt_id, evidence_digest, reason),
                Command::CancelApi {
                    project_id,
                    attempt_id,
                    reason,
                } => self.cancel_api(project_id, attempt_id, reason),
                Command::RecoverApi {
                    project_id,
                    attempt_id,
                } => self.recover_api(project_id, attempt_id),
                Command::AbandonApiStartup {
                    project_id,
                    attempt_id,
                    evidence_digest,
                    reason,
                } => self.abandon_api_startup(project_id, attempt_id, evidence_digest, reason),
                _ => unreachable!(),
            };
            #[cfg(not(windows))]
            let result: Result<(), String> = Err("API后台执行需要Windows".into());
            return match result {
                Ok(()) => Response {
                    id: request.id,
                    result: include_snapshot.then(|| self.snapshot()),
                    error: None,
                },
                Err(message) => Response {
                    id: request.id,
                    result: None,
                    error: Some(ApiError {
                        code: "apiControlRejected".into(),
                        message,
                    }),
                },
            };
        }
        if matches!(
            request.command,
            Command::CancelOmp { .. }
                | Command::RecoverOmp { .. }
                | Command::AbandonOmpStartup { .. }
        ) {
            #[cfg(windows)]
            let result = match &request.command {
                Command::CancelOmp {
                    project_id,
                    attempt_id,
                    reason,
                } => self.cancel_omp(project_id, attempt_id, reason),
                Command::RecoverOmp {
                    project_id,
                    attempt_id,
                } => self.recover_omp(project_id, attempt_id),
                Command::AbandonOmpStartup {
                    project_id,
                    attempt_id,
                    evidence_digest,
                    reason,
                } => self.abandon_omp_startup(project_id, attempt_id, evidence_digest, reason),
                _ => unreachable!(),
            };
            #[cfg(not(windows))]
            let result: Result<(), String> = Err("OMP后台执行需要Windows".into());
            return match result {
                Ok(()) => Response {
                    id: request.id,
                    result: include_snapshot.then(|| self.snapshot()),
                    error: None,
                },
                Err(message) => Response {
                    id: request.id,
                    result: None,
                    error: Some(ApiError {
                        code: "ompControlRejected".into(),
                        message,
                    }),
                },
            };
        }
        if request.command.is_count_resolution() {
            return Response {
                id: request.id,
                result: None,
                error: Some(ApiError {
                    code: "narrowCountInterfaceRequired".into(),
                    message: "请使用独立计数核对入口".into(),
                }),
            };
        }
        // Review reads and explicit file restoration never drive scheduling.
        // Historical reads do not consume decisions or change a shared cursor.
        if let Some(result) = self
            .omp_restore_request(&request.command)
            .or_else(|| self.management_read(&request.command))
            .or_else(|| self.historical_currency_request(&request.command))
        {
            return match result {
                Ok(snapshot) => Response {
                    id: request.id,
                    result: Some(snapshot),
                    error: None,
                },
                Err(message) => Response {
                    id: request.id,
                    result: None,
                    error: Some(ApiError {
                        code: "queryRejected".into(),
                        message,
                    }),
                },
            };
        }
        let scope_guard = self.timing_command_guard(&request.command);
        #[cfg(windows)]
        let scope_guard = scope_guard.and_then(|_| self.baseline_guard(&request.command));
        if let Err(message) = scope_guard {
            return Response {
                id: request.id,
                result: None,
                error: Some(ApiError {
                    code: "timingScopeLocked".into(),
                    message,
                }),
            };
        }
        // A pause must be persisted before any new scheduling tick.
        if !matches!(
            &request.command,
            Command::PauseProjectRun { .. }
                | Command::RequestProjectRevision { .. }
                | Command::RetireSubmittedPlan { .. }
                | Command::RecoverVerification { .. }
                | Command::RetryVerificationStart { .. }
                | Command::RetireVerificationRecheck { .. }
                | Command::ApproveVerificationRecheck { .. }
                | Command::StartVerificationRecheck { .. }
                | Command::CancelVerificationRecheck { .. }
                | Command::RecoverVerificationRecheck { .. }
                | Command::DiscardManagementDecision { .. }
                | Command::AnswerManagementWait { .. }
                | Command::AbandonUnsentAttempt { .. }
                | Command::AbandonManagementApplication { .. }
                | Command::CloseAdviceTracking { .. }
                | Command::AbandonCodexStartup { .. }
                | Command::CancelCodex { .. }
                | Command::RetireUnknownDesktop { .. }
                | Command::CancelDesktopTask { .. }
        ) {
            self.poll_verifications();
        }
        let result = match request.command {
            Command::BlindReview { action } => self.blind_review_action(action),
            Command::Baseline { action } => {
                #[cfg(windows)]
                {
                    self.baseline_action(action)
                }
                #[cfg(not(windows))]
                {
                    let _ = action;
                    Err("单执行者实验需要Windows核心".into())
                }
            }
            Command::TeamTiming { action } => self.team_timing_action(action),
            Command::CancelDesktopTask {
                project_id,
                attempt_id,
                review_digest,
                reason,
            } => {
                #[cfg(windows)]
                {
                    self.cancel_desktop_task(&project_id, &attempt_id, &review_digest, &reason)
                }
                #[cfg(not(windows))]
                {
                    let _ = (project_id, attempt_id, review_digest, reason);
                    Err("桌面任务仅支持Windows".into())
                }
            }
            Command::PrepareDesktopTask {
                project_id,
                task_id,
                attempt_id,
            } => {
                #[cfg(windows)]
                {
                    self.prepare_desktop_task(&project_id, &task_id, &attempt_id)
                }
                #[cfg(not(windows))]
                {
                    let _ = (project_id, task_id, attempt_id);
                    Err("桌面任务仅支持Windows".into())
                }
            }
            Command::SendDesktopTask { review_digest } => {
                #[cfg(windows)]
                {
                    self.send_desktop_task(&review_digest)
                }
                #[cfg(not(windows))]
                {
                    let _ = review_digest;
                    Err("桌面任务仅支持Windows".into())
                }
            }
            Command::RetireUnknownDesktop {
                project_id,
                attempt_id,
                evidence_digest,
                reason,
            } => {
                #[cfg(windows)]
                {
                    self.retire_unknown_desktop(&project_id, &attempt_id, &evidence_digest, &reason)
                }
                #[cfg(not(windows))]
                {
                    let _ = (project_id, attempt_id, evidence_digest, reason);
                    Err("桌面任务仅支持Windows".into())
                }
            }
            Command::ReadDesktopTask {
                project_id,
                attempt_id,
                reader_sha256,
            } => {
                #[cfg(windows)]
                {
                    match reader_sha256.as_deref() {
                        None => self.read_desktop_task(&project_id, &attempt_id),
                        reader => {
                            self.read_desktop_task_with_reader(&project_id, &attempt_id, reader)
                        }
                    }
                }
                #[cfg(not(windows))]
                {
                    let _ = (project_id, attempt_id, reader_sha256);
                    Err("桌面任务仅支持Windows".into())
                }
            }
            Command::PrepareDesktopTrial { binding_id } => {
                #[cfg(windows)]
                {
                    self.prepare_desktop_trial(&binding_id)
                }
                #[cfg(not(windows))]
                {
                    let _ = binding_id;
                    Err("桌面验证仅支持Windows".into())
                }
            }
            Command::SendDesktopTrial { review_digest } => {
                #[cfg(windows)]
                {
                    self.send_desktop_trial(&review_digest)
                }
                #[cfg(not(windows))]
                {
                    let _ = review_digest;
                    Err("桌面验证仅支持Windows".into())
                }
            }
            Command::ReadDesktopTrial {
                trial_id,
                reader_sha256,
            } => {
                #[cfg(windows)]
                {
                    self.read_desktop_trial(&trial_id, &reader_sha256)
                }
                #[cfg(not(windows))]
                {
                    let _ = (trial_id, reader_sha256);
                    Err("桌面验证仅支持Windows".into())
                }
            }
            Command::DiscoverDesktop => {
                #[cfg(windows)]
                {
                    self.discover_desktop()
                }
                #[cfg(not(windows))]
                {
                    Err("桌面观察仅支持Windows".into())
                }
            }
            Command::BindDesktopSession {
                project_id,
                session_id,
            } => {
                #[cfg(windows)]
                {
                    self.bind_desktop_session(&project_id, &session_id)
                }
                #[cfg(not(windows))]
                {
                    let _ = (project_id, session_id);
                    Err("桌面会话绑定仅支持Windows".into())
                }
            }
            Command::InspectDesktop { window } => {
                #[cfg(windows)]
                {
                    self.inspect_desktop(window)
                }
                #[cfg(not(windows))]
                {
                    let _ = window;
                    Err("桌面观察仅支持Windows".into())
                }
            }
            Command::BindDesktop {
                project_id,
                observation_digest,
                label,
            } => {
                #[cfg(windows)]
                {
                    self.bind_desktop(&project_id, &observation_digest, &label)
                }
                #[cfg(not(windows))]
                {
                    let _ = (project_id, observation_digest, label);
                    Err("桌面观察仅支持Windows".into())
                }
            }
            Command::RetireSubmittedPlan {
                project_id,
                evidence_digest,
                reason,
            } => self.retire_submitted_plan(&project_id, &evidence_digest, &reason),
            Command::RequestProjectRevision {
                project_id,
                evidence_digest,
                reason,
            } => self.request_project_revision(&project_id, &evidence_digest, &reason),
            Command::CancelCodex {
                project_id,
                attempt_id,
                evidence_digest,
                reason,
            } => self.cancel_codex(&project_id, &attempt_id, &evidence_digest, &reason),
            Command::AbandonCodexStartup {
                project_id,
                attempt_id,
                evidence_digest,
                reason,
            } => self.abandon_codex_startup(&project_id, &attempt_id, &evidence_digest, &reason),
            Command::StartProjectRun {
                project_id,
                revision_digest,
                setup_digest,
            } => self.start_project_run_with_timing(&project_id, &revision_digest, &setup_digest),
            Command::PauseProjectRun { run_id } => self.pause_project_run(&run_id),
            Command::AbandonManagementApplication {
                operation_id,
                evidence_digest,
                reason,
            } => self.abandon_management_application(&operation_id, &evidence_digest, &reason),
            Command::CloseAdviceTracking {
                operation_id,
                evidence_digest,
                reason,
            } => self.close_advice_tracking(&operation_id, &evidence_digest, &reason),
            Command::DiscardManagementDecision {
                operation_id,
                evidence_digest,
                reason,
            } => self.resolve_management(
                &operation_id,
                &evidence_digest,
                crate::management_execution::UserResolution::Discard { reason },
            ),
            Command::AnswerManagementWait {
                operation_id,
                evidence_digest,
                answer,
            } => self.resolve_management(
                &operation_id,
                &evidence_digest,
                crate::management_execution::UserResolution::Reply { answer },
            ),
            Command::ConfigureProject { draft } => self.configure_project(*draft),
            Command::ProvisionWorkerAccounts {
                project_id,
                setup_digest,
            } => {
                #[cfg(windows)]
                {
                    (|| -> Result<(), String> {
                        if self.projects.get(&project_id).is_some_and(|s| {
                            s.approval().is_some() && s.state().revision_requested.is_none()
                        }) {
                            let plan = self
                                .prepare_authorized_worker_accounts(&project_id, &setup_digest)?;
                            return crate::windows_worker_account::start(plan);
                        }
                        self.planning_idle(&project_id)?;
                        let saved = self
                            .project_setups
                            .get(&project_id)
                            .ok_or("请先保存项目配置")?;
                        require(saved.digest == setup_digest, "项目配置已变化")?;
                        saved.setup.require_available_adapters()?;
                        require(
                            !crate::windows_worker_account::views(&self.root)
                                .iter()
                                .any(|v| v.project_id == project_id && v.state == "pending"),
                            "原账户设置尚未核对，不会重复提权",
                        )?;
                        let chief = saved
                            .setup
                            .chief_directory
                            .as_deref()
                            .ok_or("项目尚未启用目录隔离")?;
                        let project = &self.projects[&project_id]
                            .configuration()
                            .execution_scope
                            .as_ref()
                            .ok_or("项目范围缺失")?
                            .project;
                        let plan = crate::windows_worker_account::prepare(
                            &self.root,
                            project,
                            chief,
                            &saved.setup.worker_directories,
                        )?;
                        crate::windows_worker_account::start(plan)
                    })()
                }
                #[cfg(not(windows))]
                {
                    let _ = (project_id, setup_digest);
                    Err("执行账户隔离需要Windows".into())
                }
            }
            Command::OpenWorkerLogin {
                project_id,
                setup_digest,
                profile_id,
            } => {
                #[cfg(windows)]
                {
                    (|| -> Result<(), String> {
                        self.worker_account_idle(&project_id)?;
                        let saved = self.project_setups.get(&project_id).ok_or("项目配置缺失")?;
                        require(saved.digest == setup_digest, "项目配置已变化")?;
                        let task = saved.setup.execution_for("login", profile_id.as_deref())?;
                        let project = &self.projects[&project_id]
                            .configuration()
                            .execution_scope
                            .as_ref()
                            .ok_or("项目范围缺失")?
                            .project;
                        crate::windows_worker_account::open_login(&self.root, project, &task)
                    })()
                }
                #[cfg(not(windows))]
                {
                    let _ = (project_id, setup_digest, profile_id);
                    Err("执行账户隔离需要Windows".into())
                }
            }
            Command::RequestDocumentRefinement { input } => self.request_document_refinement(input),
            Command::RequestChiefPlan {
                project_id,
                setup_digest,
            } => self.request_chief_plan_without_timing(&project_id, &setup_digest),
            Command::ReviewHistoricalCharge { .. }
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
            | Command::ReadHistoricalSources { .. }
            | Command::ReadHistoricalAccounting { .. }
            | Command::ReadHistoricalConfirmation { .. } => {
                unreachable!("handled before scheduling")
            }
            Command::AuthorizePlanningBudget {
                project_id,
                setup_digest,
            } => self.authorize_planning_budget(&project_id, &setup_digest),
            Command::SetOpenAiKey { key } => self.set_openai_key(Some(key)),
            Command::RemoveOpenAiKey => self.set_openai_key(None),
            Command::SetProviderKey { provider, key } => self.set_provider_key(provider, Some(key)),
            Command::RemoveProviderKey { provider } => self.set_provider_key(provider, None),
            Command::CheckOpenAi => self.check_openai(),
            Command::CheckProviderModels { target } => self.check_provider_models(target),
            Command::CheckCodex { executable } => self.check_codex(executable),
            Command::ReconcileCodex {
                project_id,
                attempt_id,
            } => self.reconcile_codex(&project_id, &attempt_id),
            Command::AbandonUnsentAttempt {
                project_id,
                task_id,
                attempt_id,
                evidence_digest,
                reason,
            } => self.abandon_unsent_attempt(
                &project_id,
                &task_id,
                &attempt_id,
                &evidence_digest,
                &reason,
            ),
            Command::RegisterValidator {
                id,
                name,
                executable,
                inputs,
                environment,
            } => {
                let _ = (id, name, executable, inputs, environment);
                Err("独立检查工具固定随工作台提供，不再开放新增或替换；历史登记仅保留回读".into())
            }
            Command::RegisterProject { project } => self.register(project),
            Command::ApproveRevision { project_id, digest } => {
                self.approve_with_timing(&project_id, &digest)
            }
            Command::ApproveRevisionWithWorkerAccounts {
                project_id,
                digest,
                setup_digest,
            } => self.approve_with_worker_accounts(&project_id, &digest, &setup_digest),
            Command::CancelVerification {
                project_id,
                operation_id,
            } => self.cancel_verification(&project_id, &operation_id),
            Command::AbandonVerification {
                project_id,
                operation_id,
                digest,
                reason,
            } => self.abandon_verification(&project_id, &operation_id, &digest, &reason),
            Command::RetryVerificationStart {
                project_id,
                original_operation_id,
                evidence_digest,
                reason,
            } => self.retry_verification_start(
                &project_id,
                &original_operation_id,
                &evidence_digest,
                &reason,
            ),
            Command::RecoverVerification {
                project_id,
                operation_id,
                evidence_digest,
            } => self.recover_verification(&project_id, &operation_id, &evidence_digest),
            Command::ApproveVerificationRecheck {
                review,
                approval_operation_id,
                operation_id,
            } => self
                .approve_verification_recheck(&review, &approval_operation_id, &operation_id)
                .map(|_| ()),
            command @ (Command::RetireVerificationRecheck { .. }
            | Command::StartVerificationRecheck { .. }
            | Command::CancelVerificationRecheck { .. }
            | Command::RecoverVerificationRecheck { .. }) => {
                self.verification_recheck_command(command)
            }
            command => self.base.execute_command(command),
        };
        match result {
            Ok(()) => Response {
                id: request.id,
                result: include_snapshot.then(|| self.snapshot()),
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

#[cfg(all(test, windows))]
mod management_host_tests {
    use super::*;
    use crate::management_execution::{DecisionIntent, DecisionState};
    use cerulean_adapters::openai::DecisionObservation;
    use cerulean_domain::management::{DecisionEnvelope, ManagementDecision, Purpose};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc,
    };
    use std::time::{Duration, Instant};

    fn intent(host: &RuntimeHost, id: &str) -> DecisionIntent {
        let context = host
            .management_context("project", "Implement a tested sum function")
            .unwrap();
        DecisionIntent {
            operation_id: id.into(),
            project_id: "project".into(),
            purpose: Purpose::Plan,
            model: "test-model".into(),
            state_digest: digest(&context).unwrap(),
            context,
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }
    fn observation(i: &DecisionIntent) -> DecisionObservation {
        DecisionObservation {
            response_id: "resp_host_test".into(),
            requested_model: i.model.clone(),
            actual_model: i.model.clone(),
            input_tokens: 20,
            output_tokens: 10,
            envelope: DecisionEnvelope {
                state_digest: i.state_digest.clone(),
                decision: ManagementDecision::Wait {
                    reason: "Await configured worker".into(),
                },
            },
        }
    }
    fn poll_done(host: &mut RuntimeHost) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !host.management_active.is_empty() && Instant::now() < deadline {
            host.poll_verifications();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(host.management_active.is_empty());
    }
    #[test]
    fn management_wire_host_persists_original_request_before_async_send_and_reopens_readonly() {
        use cerulean_adapters::{
            api_generation::ApiUsage,
            openai::{DecisionResult, DecisionTransport, PreparedDecision},
        };
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let project = tempfile::tempdir().unwrap();
        let mut host = RuntimeHost::open(&data).unwrap();
        host.register(Project {
            id: "project".into(),
            root: project.path().to_string_lossy().into(),
            timezone: "Asia/Shanghai".into(),
        })
        .unwrap();
        drop(host);
        crate::currency_guard::tests::promote_fixture(&data);
        let mut host = RuntimeHost::open(&data).unwrap();
        let i = intent(&host, "wire-host-1");
        let request = PreparedDecision::new(i.purpose, &i.model, &i.state_digest, &i.context)
            .unwrap()
            .fingerprint()
            .clone();
        let sends = Arc::new(AtomicUsize::new(0));
        let counter = sends.clone();
        let path = data.join("management-decisions/wire-host-1.0.json");
        let expected = request.clone();
        host.launch_management_evidenced(i.clone(), Some(request.clone()), move |i| {
            let record: serde_json::Value =
                serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
            assert_eq!(
                record["body"]["evidence"]["request"],
                serde_json::to_value(&expected).unwrap()
            );
            counter.fetch_add(1, Ordering::SeqCst);
            let observation = observation(&i);
            Ok(DecisionResult {
                transport: Some(DecisionTransport {
                    request: expected,
                    usage: ApiUsage {
                        input_tokens: observation.input_tokens,
                        output_tokens: observation.output_tokens,
                        cache_read_tokens: Some(3),
                        cache_write_tokens: Some(4),
                    },
                }),
                observation,
            })
        })
        .unwrap();
        poll_done(&mut host);
        assert!(host.management_decision("wire-host-1").is_ok());
        let before = crate::reader_generation::tests::snapshot(&data);
        assert!(host
            .launch_management_evidenced(i.clone(), Some(request.clone()), |_| panic!(
                "duplicate send"
            ))
            .is_err());
        drop(host);
        let mut host = RuntimeHost::open(&data).unwrap();
        assert!(host.management_decision("wire-host-1").is_ok());
        assert!(host
            .launch_management_evidenced(i, Some(request), |_| panic!("restart send"))
            .is_err());
        assert_eq!(sends.load(Ordering::SeqCst), 1);
        assert_eq!(crate::reader_generation::tests::snapshot(&data), before);
    }
    #[test]
    fn host_persists_before_send_and_restart_reads_without_resend() {
        let data = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        let mut host = RuntimeHost::open(data.path()).unwrap();
        host.register(Project {
            id: "project".into(),
            root: project.path().to_string_lossy().into(),
            timezone: "Asia/Shanghai".into(),
        })
        .unwrap();
        let i = intent(&host, "decision-1");
        let evidence = data.path().join("management-decisions/decision-1.0.json");
        let (began, started) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let sends = Arc::new(AtomicUsize::new(0));
        let counter = sends.clone();
        host.launch_management(i.clone(), move |i| {
            assert!(evidence.is_file());
            let record: serde_json::Value =
                serde_json::from_slice(&fs::read(evidence).unwrap()).unwrap();
            assert_eq!(
                record["body"]["evidence"]["intent"]["stateDigest"],
                i.state_digest
            );
            counter.fetch_add(1, Ordering::SeqCst);
            began.send(()).unwrap();
            gate.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(observation(&i))
        })
        .unwrap();
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(host.management_decisions()[0].state, DecisionState::Running);
        assert!(!host.can_shutdown());
        assert!(host.set_openai_key(None).is_err());
        assert!(host
            .launch_management(i.clone(), |_| panic!("duplicate send"))
            .is_err());
        assert!(host
            .launch_management(intent(&host, "decision-2"), |_| panic!("second send"))
            .is_err());
        release.send(()).unwrap();
        poll_done(&mut host);
        assert!(host.management_decision("decision-1").is_ok());
        assert!(host.snapshot().workflows[0].revision.is_none()); // Suggestions never impersonate approval or proposal.
        drop(host);
        let mut host = RuntimeHost::open(data.path()).unwrap();
        assert!(host.management_decision("decision-1").is_ok());
        assert_eq!(host.management_decisions()[0].state, DecisionState::Ready);
        assert!(host
            .launch_management(i, |_| panic!("restart resend"))
            .is_err());
        assert_eq!(sends.load(Ordering::SeqCst), 1);
        // A changed authoritative context invalidates a saved decision.
        host.validators.insert(
            "changed-catalog".into(),
            RegisteredValidator {
                id: "changed-catalog".into(),
                name: "changed catalog fixture".into(),
                registered_by: "user".into(),
                registered_at: chrono::Utc::now().to_rfc3339(),
                tool: ValidationTool {
                    executable: ValidationFile {
                        path: "unused".into(),
                        sha256: "a".repeat(64),
                    },
                    inputs: vec![],
                    environment: Default::default(),
                },
            },
        );
        assert!(host
            .management_decision("decision-1")
            .unwrap_err()
            .contains("stale"));
    }
    #[test]
    fn disconnected_worker_and_failed_publication_remain_unknown() {
        for failure in ["panic", "storage"] {
            let data = tempfile::tempdir().unwrap();
            let project = tempfile::tempdir().unwrap();
            let mut host = RuntimeHost::open(data.path()).unwrap();
            host.register(Project {
                id: "project".into(),
                root: project.path().to_string_lossy().into(),
                timezone: "Asia/Shanghai".into(),
            })
            .unwrap();
            let i = intent(&host, "decision-1");
            let retry_intent = intent(&host, "decision-2");
            let target = data
                .path()
                .join("management-decisions/decision-1.1.pending");
            host.launch_management(i, move |i| {
                if failure == "panic" {
                    panic!("injected worker crash after intent");
                }
                fs::write(target, b"injected partial publication").unwrap();
                Ok(observation(&i))
            })
            .unwrap();
            poll_done(&mut host);
            assert_eq!(host.management_decisions()[0].state, DecisionState::Unknown);
            assert!(host.management_decision("decision-1").is_err());
            if failure == "storage" {
                // A new context is already refused by publication preflight.
                // Also check the journal gate with an intent prepared before
                // the fault; the helper must not unwrap the expected refusal.
                assert!(host
                    .management_context("project", "Implement a tested sum function")
                    .unwrap_err()
                    .contains("management publication target already exists"));
            }
            assert!(host
                .launch_management(retry_intent, |_| panic!("must not resend"))
                .is_err());
            assert_eq!(host.management_decisions().len(), 1);
        }
    }
}

#[cfg(all(test, windows))]
#[path = "workflow_coordinator_tests.rs"]
mod workflow_coordinator_tests;
