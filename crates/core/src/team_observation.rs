//! Opt-in experiment observations around the existing Host runner. This is not
//! a scheduler, a material authenticator, or an authorization bypass.
use super::*;
use crate::{
    api_execution::{load, publish},
    artifacts::root_path,
    source_snapshot::{observe, Tree},
};
use cerulean_domain::project_setup::RunState;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Instant;
#[path = "team_observation/timing.rs"]
pub mod timing;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Started {
    schema: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    material_binding_digest: Option<String>,
    data_root: String,
    project: String,
    revision_digest: String,
    setup_digest: String,
    plan: PlanRevision,
    approval: Approval,
    seed: Tree,
    workflow_head: String,
    history_length: usize,
    management: Value,
    started_at: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Launch {
    start_digest: String,
    run_id: String,
    error: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Terminal {
    start_digest: String,
    launch_digest: String,
    ledger_head: String,
    run_state: Option<RunState>,
    task_states: Value,
    management: Value,
    run_control_head: String,
    elapsed_ms: Option<u64>,
    clock_continuity: bool,
    finished_at: String,
    accepted: bool,
}

/// Only created after the start observation is persisted. Retaining this token
/// keeps monotonic clock continuity; it contains no credential or model client.
pub struct ObservedTeamRun {
    root: PathBuf,
    project: String,
    start_digest: String,
    clock: Instant,
    launch_error: Option<String>,
}
impl ObservedTeamRun {
    pub fn start_digest(&self) -> &str {
        &self.start_digest
    }
    pub fn launch_error(&self) -> Option<&str> {
        self.launch_error.as_deref()
    }
    /// Capture only after the original runner and its children have stopped.
    pub fn finish(&self, host: &RuntimeHost) -> Result<Value, String> {
        require(
            host.root == self.root,
            "observation belongs to another Host",
        )?;
        host.finish_team_observation(&self.project, &self.start_digest, Some(self.clock))
    }
}
fn directory(root: &Path, project: &str) -> Result<PathBuf, String> {
    identifier(project)?;
    Ok(root.join("team-observations").join(project))
}
fn absent(path: &Path, name: &str) -> Result<(), String> {
    require(
        !path.join(name).try_exists().map_err(|e| e.to_string())?
            && !path
                .join(format!("{name}.pending"))
                .try_exists()
                .map_err(|e| e.to_string())?,
        "observation already exists; inspect original run without restarting",
    )
}
fn initial_still_present(initial: &Value, current: &Value) -> Result<(), String> {
    let before = initial["operations"]
        .as_array()
        .ok_or("initial management inventory missing")?;
    let after = current["operations"]
        .as_array()
        .ok_or("current management inventory missing")?;
    for operation in before {
        require(
            after.iter().any(|next| next == operation),
            "pre-run management source changed or disappeared",
        )?;
    }
    require(
        initial["reviewedFragments"] == current["reviewedFragments"],
        "management reviewed fragments changed during experiment",
    )
}
fn validate_launch(launch: &Launch, start: &Started, expected: &str) -> Result<(), String> {
    require(
        launch.start_digest == expected
            && launch.run_id
                == format!(
                    "run-{}",
                    &digest(&(start.project.as_str(), start.revision_digest.as_str()))?[..32]
                ),
        "launch scope changed",
    )
}
impl RuntimeHost {
    /// Check a sealed failure against the live owner without taking a second
    /// workflow lock or changing submitted tasks and the original run.
    fn failed_team_terminal_preserved(&self, project: &str, expected: &str) -> Result<(), String> {
        self.observation_quiet()?;
        require(
            !self
                .management
                .statuses()
                .iter()
                .any(|s| s.project_id == project && !s.state.resolved()),
            "failed experiment has unresolved management decisions",
        )?;
        let path = directory(&self.root, project)?;
        let start: Started = load(&path, "started.json")?;
        let launch: Launch = load(&path, "launch.json")?;
        let end: Terminal = load(&path, "terminal.json")?;
        let store = self
            .projects
            .get(project)
            .ok_or("experiment project missing")?;
        validate_start(&start, expected, &self.root, project, store)?;
        validate_launch(&launch, &start, expected)?;
        let binding = read_binding(&self.root, project)?.ok_or("experiment material missing")?;
        crate::team_materials::verify(&binding)?;
        require(
            start.schema == 2
                && start.material_binding_digest.as_ref() == Some(&digest(&binding)?)
                && start.plan == binding.plan
                && start.setup_digest == binding.setup.digest
                && start.seed == binding.seed,
            "failed experiment material changed",
        )?;
        let run = self
            .runs
            .all()
            .into_iter()
            .find(|r| r.id == launch.run_id)
            .ok_or("failed experiment run missing")?;
        require(
            matches!(run.state, RunState::Attention | RunState::Paused)
                && run.project_id == project
                && run.revision_digest == start.revision_digest
                && run.setup_digest == start.setup_digest
                && end.run_state.as_ref() == Some(&run.state)
                && end.run_control_head == self.runs.evidence_digest()
                && end.start_digest == expected
                && end.launch_digest == digest(&launch)?
                && !end.accepted
                && end.clock_continuity == end.elapsed_ms.is_some()
                && end.ledger_head == store.history().last().ok_or("ledger missing")?.hash
                && end.task_states
                    == serde_json::to_value(&store.state().tasks).map_err(|e| e.to_string())?,
            "failed experiment terminal differs from original workflow or run",
        )?;
        let management = self
            .management
            .efficiency_snapshot(project, store.history())?;
        initial_still_present(&start.management, &management)?;
        require(
            end.management == management,
            "failed experiment management changed",
        )
    }

    /// Freeze the material-derived expectation before the original chief plans.
    /// Does not alter setup, create a proposal, approve, or access credentials.
    pub fn bind_team_experiment_materials(
        &self,
        request: &crate::team_materials::TeamRequest,
    ) -> Result<String, String> {
        crate::experiment_guard::require_protected(&self.root)?;
        self.observation_quiet()?;
        let blueprint = crate::team_materials::compose(request)?;
        let project = &blueprint.plan.project.id;
        self.planning_idle(project)?;
        require(
            self.projects.len() == 1
                && self.projects[project].history().is_empty()
                && self.management.statuses().is_empty()
                && self.runs.all().is_empty()
                && self.root
                    == Path::new(&blueprint.request.material.data_root)
                        .canonicalize()
                        .map_err(|e| e.to_string())?
                && self
                    .project_setups
                    .get(project)
                    .is_some_and(|s| s.digest == blueprint.setup.digest)
                && self.projects[project]
                    .configuration()
                    .execution_scope
                    .as_ref()
                    .is_some_and(|s| s.project == blueprint.plan.project),
            "material binding requires the fresh exact configured project before planning",
        )?;
        let parent = self.root.join("team-material-bindings");
        if !parent.exists() {
            fs::create_dir(&parent).map_err(|e| e.to_string())?;
        }
        ordinary(&parent, true)?;
        let path = parent.join(project);
        fs::create_dir(&path)
            .map_err(|_| "material binding exists or is incomplete; do not replace it")?;
        publish(&path, "binding.json", &blueprint)?;
        digest(&blueprint)
    }

    /// Explicit pre-planning configuration revision. Ordinary project defaults
    /// stay unchanged; an already proposed/approved plan is never rewritten.
    pub fn prepare_team_experiment_inputs(
        &mut self,
        project: &str,
        expected_setup: &str,
    ) -> Result<String, String> {
        crate::experiment_guard::require_protected(&self.root)?;
        identifier(project)?;
        sha256(expected_setup)?;
        self.observation_quiet()?;
        self.planning_idle(project)?;
        require(
            self.projects[project].history().is_empty()
                && self.management.statuses().is_empty()
                && self.runs.all().is_empty(),
            "experiment snapshot must be selected before any planning",
        )?;
        let mut saved = self
            .project_setups
            .get(project)
            .ok_or("experiment setup missing")?
            .clone();
        require(saved.digest == expected_setup, "experiment setup changed")?;
        let policy = SnapshotPolicy {
            exclude: vec![".git".into()],
            max_files: 100,
            max_bytes: 1024 * 1024,
        };
        if saved.setup.snapshot_policy == policy {
            return Ok(saved.digest);
        }
        saved.setup.version = saved
            .setup
            .version
            .checked_add(1)
            .ok_or("setup version overflow")?;
        saved.setup.snapshot_policy = policy;
        saved.digest = digest(&saved.setup)?;
        crate::project_setup::save(&self.root, &saved)?;
        let selected = saved.digest.clone();
        self.project_setups.insert(project.into(), saved);
        Ok(selected)
    }

    fn observation_quiet(&self) -> Result<(), String> {
        require(
            self.api_active.is_empty()
                && self.api_writers.is_empty()
                && self.codex_active.is_empty()
                && self.omp_active.is_empty()
                && self.active.is_empty()
                && self.desktop_active.is_none()
                && self.desktop_probe.is_none()
                && self.codex_probe.is_none()
                && self.openai_probe.is_none()
                && self.provider_probes.is_empty()
                && self.management_work_count() == 0,
            "experiment observation requires quiescent owned work",
        )
    }

    /// Trusted local-user entry. The original approved setup and all production
    /// credential, budget and dispatch guards still run in start_project_run.
    /// No UI/IPC command is added by this development API.
    pub fn start_observed_team_run(
        &mut self,
        project: &str,
        revision: &str,
        setup: &str,
    ) -> Result<ObservedTeamRun, String> {
        crate::experiment_guard::require_protected(&self.root)?;
        identifier(project)?;
        sha256(revision)?;
        sha256(setup)?;
        self.observation_quiet()?;
        require(
            self.runs.all().is_empty(),
            "team experiment requires an unused isolated Host",
        )?;
        require(
            self.api_startups.is_empty()
                && self.api_jobs.is_empty()
                && self.jobs.is_empty()
                && self.codex_jobs.is_empty()
                && self.omp_jobs.is_empty(),
            "experiment has earlier execution records",
        )?;
        let store = self
            .projects
            .get(project)
            .ok_or("experiment project missing")?;
        let plan = store.revision().ok_or("experiment plan missing")?;
        let approval = store.approval().ok_or("experiment approval missing")?;
        let saved = self
            .project_setups
            .get(project)
            .ok_or("experiment setup missing")?;
        require(
            self.projects.len() == 1
                && saved.digest == setup
                && plan.approval_digest()? == revision
                && store.history().len() == 2
                && store
                    .state()
                    .tasks
                    .values()
                    .all(|s| s.state == TaskState::Pending && s.attempt.is_none())
                && plan.execution.iter().all(|e| e.api_worker.is_some())
                && plan.worktree.is_some()
                && plan.policy.currency_budget.is_none(),
            "team observation requires a fresh approved API/worktree plan",
        )?;
        approval.validate(plan, "user", plan.policy.version)?;
        require(
            self.management
                .statuses()
                .iter()
                .all(|s| s.project_id == project && s.state.resolved()),
            "pre-run management operations must be resolved and scoped",
        )?;
        let policy = plan
            .snapshot_policy
            .as_ref()
            .ok_or("experiment input snapshot required")?;
        require(
            policy.exclude == vec![".git"],
            "experiment may only exclude Git metadata",
        )?;
        let seed = observe(Path::new(&plan.project.root), policy)?;
        let worktree = plan.worktree.as_ref().unwrap();
        crate::git_workspaces::GitWorkspaces::inspect_committed_seed(
            Path::new(&worktree.pool_root),
            &worktree.git,
            Path::new(&plan.project.root),
            &worktree.base_commit,
            &seed,
        )?;
        let management = self
            .management
            .efficiency_snapshot(project, store.history())?;
        let material = read_binding(&self.root, project)?;
        if let Some(binding) = &material {
            crate::team_materials::verify(binding)?;
            require(
                binding.plan == *plan
                    && binding.setup.digest == saved.digest
                    && digest(&binding.setup.setup)? == digest(&saved.setup)?
                    && binding.seed == seed,
                "actual team setup or approved plan differs from frozen materials",
            )?;
            let synthetic_transport = {
                #[cfg(test)]
                {
                    self.api_fixture.is_some() && self.management_fixture.is_some()
                }
                #[cfg(not(test))]
                {
                    false
                }
            };
            require(
                binding.proof.declared_execution_kind == "real-attempt" || synthetic_transport,
                "synthetic team materials cannot launch production transports",
            )?;
        }
        let folder = self.root.join("team-observations");
        if !folder.exists() {
            fs::create_dir(&folder).map_err(|e| e.to_string())?;
        }
        ordinary(&folder, true)?;
        let path = directory(&self.root, project)?;
        // The entire directory is one intent. A partial preparation is never reused.
        fs::create_dir(&path)
            .map_err(|_| "team observation directory already exists or cannot be created")?;
        let clock = Instant::now();
        let start = Started {
            schema: if material.is_some() { 2 } else { 1 },
            material_binding_digest: material.as_ref().map(digest).transpose()?,
            data_root: self.root.to_string_lossy().into(),
            project: project.into(),
            revision_digest: revision.into(),
            setup_digest: setup.into(),
            plan: plan.clone(),
            approval: approval.clone(),
            seed,
            workflow_head: store.history().last().ok_or("ledger missing")?.hash.clone(),
            history_length: store.history().len(),
            management,
            started_at: chrono::Utc::now().to_rfc3339(),
        };
        let start_digest = digest(&start)?;
        publish(&path, "started.json", &start)?;
        let error = self.start_project_run(project, revision, setup).err();
        let launch = Launch {
            start_digest: start_digest.clone(),
            run_id: format!("run-{}", &digest(&(project, revision))?[..32]),
            error: error.clone(),
        };
        if let Err(error) = publish(&path, "launch.json", &launch) {
            // An uncertain observation must not trigger later automatic scheduling.
            self.runner_error =
                Some("team observation publication uncertain; inspect original records".into());
            return Err(error);
        }
        Ok(ObservedTeamRun {
            root: self.root.clone(),
            project: project.into(),
            start_digest,
            clock,
            launch_error: error,
        })
    }
    /// Close original stopped work after losing the in-memory clock. Never
    /// reconstructs elapsed time from wall timestamps or resumes scheduling.
    pub fn close_interrupted_team_observation(
        &self,
        project: &str,
        expected: &str,
    ) -> Result<Value, String> {
        self.finish_team_observation(project, expected, None)
    }
    fn finish_team_observation(
        &self,
        project: &str,
        expected: &str,
        clock: Option<Instant>,
    ) -> Result<Value, String> {
        crate::experiment_guard::require_protected(&self.root)?;
        sha256(expected)?;
        self.observation_quiet()?;
        let end = clock.map(|start| start.elapsed());
        let path = directory(&self.root, project)?;
        absent(&path, "terminal.json")?;
        let start: Started = load(&path, "started.json")?;
        let launch: Launch = load(&path, "launch.json")?;
        let store = self
            .projects
            .get(project)
            .ok_or("experiment project missing")?;
        validate_start(&start, expected, &self.root, project, store)?;
        validate_launch(&launch, &start, expected)?;
        let run = self.runs.all().into_iter().find(|r| r.id == launch.run_id);
        require(
            run.as_ref().is_none_or(|r| r.state != RunState::Active)
                && (run.is_some() || launch.error.is_some()),
            "original experiment is still active or launch record missing",
        )?;
        if let Some(run) = &run {
            require(
                run.revision_digest == start.revision_digest
                    && run.setup_digest == start.setup_digest,
                "observed run scope changed",
            )?;
        }
        let management = self
            .management
            .efficiency_snapshot(project, store.history())?;
        initial_still_present(&start.management, &management)?;
        let accepted = run.as_ref().is_some_and(|r| r.state == RunState::Completed)
            && store
                .state()
                .tasks
                .values()
                .all(|s| s.state == TaskState::Accepted && s.verified);
        let terminal = Terminal {
            start_digest: expected.into(),
            launch_digest: digest(&launch)?,
            ledger_head: store.history().last().ok_or("ledger missing")?.hash.clone(),
            run_state: run.map(|r| r.state),
            task_states: serde_json::to_value(&store.state().tasks).map_err(|e| e.to_string())?,
            management,
            run_control_head: self.runs.evidence_digest().into(),
            elapsed_ms: end
                .map(|d| u64::try_from(d.as_millis()).map_err(|_| "experiment timing overflow"))
                .transpose()?,
            clock_continuity: clock.is_some(),
            finished_at: chrono::Utc::now().to_rfc3339(),
            accepted,
        };
        publish(&path, "terminal.json", &terminal)?;
        serde_json::to_value(terminal).map_err(|e| e.to_string())
    }
}
fn validate_start(
    start: &Started,
    expected: &str,
    root: &Path,
    project: &str,
    store: &DurableWorkflow,
) -> Result<(), String> {
    require(
        ((start.schema == 1 && start.material_binding_digest.is_none())
            || (start.schema == 2 && start.material_binding_digest.is_some()))
            && digest(start)? == expected
            && Path::new(&start.data_root) == root
            && start.project == project
            && start.plan.project.id == project
            && start.plan.approval_digest()? == start.revision_digest
            && store.revision() == Some(&start.plan)
            && store.approval() == Some(&start.approval)
            && start.history_length == 2
            && store
                .history()
                .get(start.history_length - 1)
                .is_some_and(|e| e.hash == start.workflow_head),
        "original observation differs from selected approved workflow",
    )?;
    start
        .approval
        .validate(&start.plan, "user", start.plan.policy.version)
}

/// Offline original evidence only. No RuntimeHost opening, recovery or dispatch.
pub fn inspect(root: &Path, project: &str, expected: &str) -> Result<Value, String> {
    use fs2::FileExt;
    sha256(expected)?;
    let root = root_path(root.to_str().ok_or("non-Unicode data root")?)?;
    ordinary(&root.join("writer.lock"), false)?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(root.join("writer.lock"))
        .map_err(|e| e.to_string())?;
    lock.try_lock_exclusive()
        .map_err(|_| "Host writer is active")?;
    inspect_locked(&root, project, expected)
}

fn inspect_locked(root: &Path, project: &str, expected: &str) -> Result<Value, String> {
    let path = directory(root, project)?;
    root_path(path.to_str().ok_or("invalid observation directory")?)?;
    let start: Started = load(&path, "started.json")?;
    let material = read_binding(root, project)?;
    require(
        material.as_ref().map(digest).transpose()? == start.material_binding_digest,
        "original material binding missing or changed",
    )?;
    if let Some(binding) = &material {
        crate::team_materials::verify(binding)?;
        require(
            binding.plan == start.plan
                && binding.setup.digest == start.setup_digest
                && binding.seed == start.seed,
            "observed setup or plan differs from material binding",
        )?;
        ordinary(&root.join("project-setups"), true)?;
        let saved = crate::project_setup::load(root)?;
        require(
            saved
                .get(project)
                .is_some_and(|s| s.digest == binding.setup.digest),
            "original setup differs from material binding",
        )?;
    }
    let launch = if path.join("launch.json").exists() || path.join("launch.json.pending").exists() {
        Some(load::<Launch>(&path, "launch.json")?)
    } else {
        None
    };
    let terminal =
        if path.join("terminal.json").exists() || path.join("terminal.json.pending").exists() {
            Some(load::<Terminal>(&path, "terminal.json")?)
        } else {
            None
        };
    let controls = crate::run_control::RunLedger::read_existing(root)?;
    if let Some(end) = &terminal {
        let launch = launch.as_ref().ok_or("terminal lacks launch")?;
        let run = controls.all().into_iter().find(|r| r.id == launch.run_id);
        require(
            end.run_control_head == controls.evidence_digest()
                && end.run_state == run.as_ref().map(|r| r.state.clone())
                && run.as_ref().is_none_or(|r| {
                    r.project_id == project
                        && r.revision_digest == start.revision_digest
                        && r.setup_digest == start.setup_digest
                })
                && (run.is_some() || launch.error.is_some()),
            "terminal differs from original run control",
        )?;
    }
    let head = DurableWorkflow::inspect_existing(&root.join("workflows").join(project), |store| {
        validate_start(&start, expected, root, project, store)?;
        let head = store.history().last().ok_or("ledger missing")?.hash.clone();
        if let Some(launch) = &launch {
            validate_launch(launch, &start, expected)?;
        }
        if let Some(end) = &terminal {
            require(
                end.start_digest == expected
                    && end.launch_digest
                        == digest(launch.as_ref().ok_or("terminal lacks launch")?)?
                    && end.ledger_head == head
                    && end.task_states
                        == serde_json::to_value(&store.state().tasks).map_err(|e| e.to_string())?
                    && end.clock_continuity == end.elapsed_ms.is_some()
                    && end.accepted
                        == (end.run_state == Some(RunState::Completed)
                            && store
                                .state()
                                .tasks
                                .values()
                                .all(|s| s.state == TaskState::Accepted && s.verified)),
                "observation terminal differs from original workflow",
            )?;
        }
        Ok(head)
    })?;
    let evidence =
        crate::efficiency_evidence::read_inner(root, project, &start.revision_digest, &head, true)?;
    initial_still_present(&start.management, &evidence["management"])?;
    if let Some(end) = &terminal {
        require(
            end.management == evidence["management"],
            "terminal management inventory changed",
        )?;
    }
    Ok(
        json!({"schema":1,"kind":"team-original-run-observation","startDigest":expected,
        "started":start,"launch":launch,"terminal":terminal,"coreExport":evidence,
        "status":if terminal.is_some(){"terminal"}else{"started-without-terminal"},
        "elapsedMs":terminal.as_ref().and_then(|t|t.elapsed_ms),"humanActiveMs":null,
        "materialContentBound":material.is_some(),"materialBinding":material,"realityAttested":false,"experimentRecordReady":false,"releaseAccepted":false}),
    )
}

fn read_binding(
    root: &Path,
    project: &str,
) -> Result<Option<crate::team_materials::TeamBlueprint>, String> {
    let path = root.join("team-material-bindings").join(project);
    if !path.try_exists().map_err(|e| e.to_string())? {
        return Ok(None);
    }
    root_path(path.to_str().ok_or("invalid binding path")?)?;
    load(&path, "binding.json").map(Some)
}
