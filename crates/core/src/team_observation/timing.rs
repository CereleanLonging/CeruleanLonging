//! Full team timeline, started before the original chief planning request.
//! UTC timestamps are labels only; elapsed and human spans require one live clock.
use super::*;
use fs2::FileExt;
use std::fs::{File, OpenOptions};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Start {
    schema: u32,
    data_root: String,
    project: String,
    setup_digest: String,
    binding_digest: String,
    human_tracking: bool,
    started_at: String,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) enum Kind {
    Proposal,
    HumanStarted,
    HumanStopped,
    Approved,
    Execution { start_digest: String },
    Finished,
    Interrupted,
    Aborted,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Mark {
    pub(crate) sequence: usize,
    pub(crate) previous: String,
    pub(crate) kind: Kind,
    pub(crate) offset_ms: Option<u64>,
    pub(crate) observed_at: String,
    pub(crate) history_length: usize,
    pub(crate) ledger_head: Option<String>,
}
/// Trusted local-user entry; approval is an explicit separate user action.
pub struct TimedTeam {
    root: PathBuf,
    start: Start,
    selected: String,
    clock: Instant,
    _lock: File,
    run: Option<ObservedTeamRun>,
}
fn folder(root: &Path, project: &str) -> Result<PathBuf, String> {
    identifier(project)?;
    Ok(root.join("team-timelines").join(project))
}
fn lock(path: &Path, create: bool) -> Result<File, String> {
    root_path(path.to_str().ok_or("invalid timeline path")?)?;
    if !create {
        ordinary(&path.join("writer.lock"), false)?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(create)
        .open(path.join("writer.lock"))
        .map_err(|e| e.to_string())?;
    file.try_lock_exclusive()
        .map_err(|_| "timeline writer is active")?;
    Ok(file)
}
fn marks(path: &Path, start: &Start) -> Result<Vec<Mark>, String> {
    let records = read_marks(
        path,
        &digest(start)?,
        start.human_tracking,
        &["start.json", "writer.lock", "terminal.json"],
    )?;
    require(
        records.iter().all(|m| m.kind != Kind::Aborted),
        "team timeline does not support baseline abort",
    )?;
    Ok(records)
}
pub(crate) fn read_marks(
    path: &Path,
    start_digest: &str,
    human_tracking: bool,
    allowed: &[&str],
) -> Result<Vec<Mark>, String> {
    ordinary(path, true)?;
    let mut names = fs::read_dir(path)
        .map_err(|e| e.to_string())?
        .map(|e| {
            e.map(|e| e.file_name().to_string_lossy().into_owned())
                .map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    names.sort();
    require(names.len() <= 512 + allowed.len(), "timeline size limit")?;
    let mut records = vec![];
    for name in names {
        if allowed.contains(&name.as_str()) {
            continue;
        }
        let expected = format!("mark-{:04}.json", records.len() + 1);
        require(
            name == expected,
            "timeline gap, unexpected file, or incomplete publication",
        )?;
        let mark: Mark = load(path, &name)?;
        require(
            mark.sequence == records.len() + 1
                && mark.previous
                    == if records.is_empty() {
                        start_digest.into()
                    } else {
                        digest(records.last().unwrap())?
                    },
            "timeline chain changed",
        )?;
        chrono::DateTime::parse_from_rfc3339(&mark.observed_at)
            .map_err(|_| "timeline UTC label invalid")?;
        records.push(mark);
    }
    replay_for(human_tracking, &records)?;
    Ok(records)
}
#[derive(Default)]
pub(crate) struct Replay {
    pub(crate) phase: u8,
    pub(crate) human: Option<u64>,
    pub(crate) human_ms: u64,
    pub(crate) proposal: Option<u64>,
    pub(crate) approved: Option<u64>,
    pub(crate) end: Option<u64>,
    pub(crate) lost: bool,
    pub(crate) execution: Option<String>,
}
fn replay(start: &Start, records: &[Mark]) -> Result<Replay, String> {
    replay_for(start.human_tracking, records)
}
pub(crate) fn replay_for(human_tracking: bool, records: &[Mark]) -> Result<Replay, String> {
    let mut state = Replay::default();
    let mut last = 0;
    for m in records {
        require(state.phase < 4, "timeline already terminal")?;
        if m.kind == Kind::Interrupted {
            require(
                m.offset_ms.is_none(),
                "lost clock must not supply elapsed time",
            )?;
            state.phase = 4;
            state.lost = true;
            continue;
        }
        let offset = m.offset_ms.ok_or("live mark has no clock")?;
        require(offset >= last, "timeline monotonic clock regressed")?;
        last = offset;
        match &m.kind {
            Kind::Proposal => {
                require(state.phase == 0, "proposal out of order")?;
                state.phase = 1;
                state.proposal = Some(offset);
            }
            Kind::HumanStarted => {
                require(
                    human_tracking && state.phase <= 3 && state.human.is_none(),
                    "human span unavailable or overlapping",
                )?;
                state.human = Some(offset);
            }
            Kind::HumanStopped => {
                let began = state.human.take().ok_or("human span is not open")?;
                state.human_ms = state
                    .human_ms
                    .checked_add(offset - began)
                    .ok_or("human time overflow")?;
            }
            Kind::Approved => {
                require(
                    state.phase == 1 && state.human.is_none(),
                    "approval out of order or human span still active",
                )?;
                state.phase = 2;
                state.approved = Some(offset);
            }
            Kind::Execution { start_digest } => {
                require(state.phase == 2, "execution out of order")?;
                sha256(start_digest)?;
                state.phase = 3;
                state.execution = Some(start_digest.clone());
            }
            Kind::Finished => {
                require(
                    state.phase == 3 && state.human.is_none(),
                    "finish out of order",
                )?;
                state.phase = 4;
                state.end = Some(offset);
            }
            Kind::Aborted => {
                require(
                    state.phase < 3 && state.human.is_none(),
                    "execution or human span still open at abort",
                )?;
                state.phase = 4;
                state.end = Some(offset);
            }
            Kind::Interrupted => unreachable!(),
        }
    }
    Ok(state)
}
fn scope(host: &RuntimeHost, start: &Start) -> Result<(), String> {
    require(
        start.schema == 1
            && host.root == Path::new(&start.data_root)
            && host
                .project_setups
                .get(&start.project)
                .is_some_and(|s| s.digest == start.setup_digest),
        "timeline Host or setup differs",
    )?;
    let binding =
        read_binding(&host.root, &start.project)?.ok_or("timeline material binding missing")?;
    require(
        digest(&binding)? == start.binding_digest,
        "timeline material binding changed",
    )?;
    Ok(())
}
impl RuntimeHost {
    /// Shutdown recognition only. Original closure, ledger and run must agree.
    pub(in crate::host) fn closed_failed_team_preserved(
        &self,
        project: &str,
    ) -> Result<(), String> {
        crate::experiment_guard::require_protected(&self.root)?;
        require(
            !self.timed_teams.contains_key(project) && !self.timing_errors.contains_key(project),
            "experiment timing still active or unresolved",
        )?;
        let path = folder(&self.root, project)?;
        let _lock = lock(&path, false)?;
        let start: Start = load(&path, "start.json")?;
        require(start.project == project, "experiment project changed")?;
        scope(self, &start)?;
        let records = marks(&path, &start)?;
        let state = replay(&start, &records)?;
        require(state.phase == 4, "experiment timeline is not closed")?;
        let execution = state
            .execution
            .as_ref()
            .ok_or("experiment execution missing")?;
        let store = self
            .projects
            .get(project)
            .ok_or("experiment project missing")?;
        for mark in &records {
            require(
                mark.history_length <= store.history().len()
                    && mark.ledger_head
                        == mark
                            .history_length
                            .checked_sub(1)
                            .and_then(|i| store.history().get(i))
                            .map(|e| e.hash.clone()),
                "experiment timeline ledger prefix changed",
            )?;
        }
        require(
            records
                .last()
                .is_some_and(|m| m.history_length == store.history().len()),
            "workflow advanced after experiment closure",
        )?;
        self.failed_team_terminal_preserved(project, execution)?;
        let terminal: Value = load(&path, "terminal.json")?;
        require(
            terminal == summary(&start, &digest(&start)?, &records, false)?,
            "failed experiment timing terminal changed",
        )
    }
}

fn append(
    host: &RuntimeHost,
    start: &Start,
    selected: &str,
    clock: Option<Instant>,
    kind: Kind,
) -> Result<(), String> {
    crate::experiment_guard::require_protected(&host.root)?;
    scope(host, start)?;
    let path = folder(&host.root, &start.project)?;
    let original: Start = load(&path, "start.json")?;
    require(
        digest(&original)? == selected && digest(start)? == selected,
        "timeline start changed",
    )?;
    absent(&path, "terminal.json")?;
    let mut records = marks(&path, start)?;
    require(records.len() < 512, "timeline mark limit")?;
    let (history_length, ledger_head) = if let Some(store) = host.projects.get(&start.project) {
        (
            store.history().len(),
            store.history().last().map(|e| e.hash.clone()),
        )
    } else if let Some(writer) = host.api_writers.get(&start.project) {
        writer.owner.timing_prefix()?
    } else {
        return Err("原计时账本由其他未提供原子前缀的执行器持有；操作未写入".into());
    };
    let mark = Mark {
        sequence: records.len() + 1,
        previous: records
            .last()
            .map(digest)
            .transpose()?
            .unwrap_or(selected.into()),
        kind,
        offset_ms: clock
            .map(|c| u64::try_from(c.elapsed().as_millis()).map_err(|_| "timeline overflow"))
            .transpose()?,
        observed_at: chrono::Utc::now().to_rfc3339(),
        history_length,
        ledger_head,
    };
    records.push(mark.clone());
    replay(start, &records)?;
    publish(&path, &format!("mark-{:04}.json", mark.sequence), &mark)
}
impl RuntimeHost {
    /// Enroll a fresh material-bound project, persist the clock boundary, then
    /// invoke the original chief planner. This request may use the configured model.
    pub fn begin_timed_team_planning(
        &mut self,
        project: &str,
        setup: &str,
        human_tracking: bool,
    ) -> Result<TimedTeam, String> {
        crate::experiment_guard::require_protected(&self.root)?;
        self.observation_quiet()?;
        self.planning_idle(project)?;
        let binding =
            read_binding(&self.root, project)?.ok_or("bind fixed team materials before timing")?;
        crate::team_materials::verify(&binding)?;
        require(
            self.projects.len() == 1
                && self.projects[project].history().is_empty()
                && self.management.statuses().is_empty()
                && self.runs.all().is_empty()
                && self.project_setups[project].digest == setup
                && binding.setup.digest == setup,
            "timing must precede all original planning and approval",
        )?;
        // Like the execution observer, synthetic materials must not reach production.
        require(
            Path::new(&binding.request.material.data_root)
                .canonicalize()
                .map_err(|e| e.to_string())?
                == self.root,
            "timeline binding selects another data root",
        )?;
        let synthetic = {
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
            binding.proof.declared_execution_kind == "real-attempt" || synthetic,
            "synthetic timing cannot start production planning",
        )?;
        let parent = self.root.join("team-timelines");
        if !parent.exists() {
            fs::create_dir(&parent).map_err(|e| e.to_string())?;
        }
        ordinary(&parent, true)?;
        let path = folder(&self.root, project)?;
        fs::create_dir(&path).map_err(|_| "timeline exists or is incomplete; do not restart")?;
        let lock = lock(&path, true)?;
        let clock = Instant::now();
        let start = Start {
            schema: 1,
            data_root: self.root.to_string_lossy().into(),
            project: project.into(),
            setup_digest: setup.into(),
            binding_digest: digest(&binding)?,
            human_tracking,
            started_at: chrono::Utc::now().to_rfc3339(),
        };
        let selected = digest(&start)?;
        publish(&path, "start.json", &start)?;
        self.request_chief_plan(project, setup)?;
        Ok(TimedTeam {
            root: self.root.clone(),
            start,
            selected,
            clock,
            _lock: lock,
            run: None,
        })
    }
    /// Close a lost-clock timeline only after original work is quiescent.
    /// Does not resume planning, approve, dispatch or manufacture elapsed time.
    pub fn close_interrupted_team_timeline(
        &self,
        project: &str,
        selected: &str,
    ) -> Result<Value, String> {
        self.observation_quiet()?;
        require(
            self.runs.all().iter().all(|r| r.state != RunState::Active),
            "original run remains active",
        )?;
        let path = folder(&self.root, project)?;
        let _lock = lock(&path, false)?;
        let start: Start = load(&path, "start.json")?;
        require(
            start.project == project && digest(&start)? == selected,
            "timeline selection changed",
        )?;
        let records = marks(&path, &start)?;
        let state = replay(&start, &records)?;
        if state.execution.is_none() {
            require(self.runs.all().is_empty() && !directory(&self.root, project)?.try_exists().map_err(|e|e.to_string())?,
                "execution may have started without timeline linkage; inspect original work, do not restart")?;
        }
        if let Some(execution) = state.execution {
            let original = directory(&self.root, project)?;
            if !original.join("terminal.json").exists() {
                self.close_interrupted_team_observation(project, &execution)?;
            }
        }
        append(self, &start, selected, None, Kind::Interrupted)?;
        finalize(self, &start, selected)
    }
}
impl TimedTeam {
    pub(crate) fn planning(&self) -> Result<bool, String> {
        Ok(replay(
            &self.start,
            &marks(&folder(&self.root, &self.start.project)?, &self.start)?,
        )?
        .phase
            == 0)
    }
    pub fn start_digest(&self) -> &str {
        &self.selected
    }
    fn check(&self, host: &RuntimeHost) -> Result<(), String> {
        require(self.root == host.root, "timeline belongs to another Host")?;
        scope(host, &self.start)
    }
    /// Observe the original proposal before user approval, not a retrospective timestamp.
    pub fn proposal_ready(&mut self, host: &RuntimeHost) -> Result<(), String> {
        self.check(host)?;
        host.observation_quiet()?;
        let store = &host.projects[&self.start.project];
        let binding = read_binding(&host.root, &self.start.project)?.ok_or("material missing")?;
        require(
            store.history().len() == 1
                && store.approval().is_none()
                && store.revision() == Some(&binding.plan),
            "timed proposal must be the original unapproved bound plan",
        )?;
        append(
            host,
            &self.start,
            &self.selected,
            Some(self.clock),
            Kind::Proposal,
        )
    }
    /// Explicit user activity toggles; this is self-reported activity, not inferred focus.
    pub fn set_human_active(&mut self, host: &RuntimeHost, active: bool) -> Result<(), String> {
        self.check(host)?;
        append(
            host,
            &self.start,
            &self.selected,
            Some(self.clock),
            if active {
                Kind::HumanStarted
            } else {
                Kind::HumanStopped
            },
        )
    }
    /// Explicit user approval through the original validator. Never call automatically.
    pub fn approve_plan(&mut self, host: &mut RuntimeHost, expected: &str) -> Result<(), String> {
        self.approve_plan_scoped(host, expected, None)
    }
    pub(crate) fn approve_plan_scoped(
        &mut self,
        host: &mut RuntimeHost,
        expected: &str,
        scope: Option<cerulean_domain::execution::WorkerAccountAuthorization>,
    ) -> Result<(), String> {
        self.check(host)?;
        host.observation_quiet()?;
        let state = replay(
            &self.start,
            &marks(&folder(&self.root, &self.start.project)?, &self.start)?,
        )?;
        require(
            state.phase == 1 && state.human.is_none(),
            "review incomplete or human span still active",
        )?;
        let store = &host.projects[&self.start.project];
        let binding = read_binding(&host.root, &self.start.project)?.ok_or("material missing")?;
        require(
            store.history().len() == 1
                && store.approval().is_none()
                && store.revision() == Some(&binding.plan),
            "unapproved plan changed",
        )?;
        host.approve_scoped(&self.start.project, expected, scope)?;
        append(
            host,
            &self.start,
            &self.selected,
            Some(self.clock),
            Kind::Approved,
        )
    }
    /// Start only the plan approved through this timeline's explicit user action.
    pub fn start_approved_run(&mut self, host: &mut RuntimeHost) -> Result<(), String> {
        self.check(host)?;
        host.observation_quiet()?;
        require(self.run.is_none(), "original run already linked")?;
        require(
            replay(
                &self.start,
                &marks(&folder(&self.root, &self.start.project)?, &self.start)?,
            )?
            .phase
                == 2,
            "timed user approval required",
        )?;
        let store = &host.projects[&self.start.project];
        let binding = read_binding(&host.root, &self.start.project)?.ok_or("material missing")?;
        require(
            store.history().len() == 2 && store.revision() == Some(&binding.plan),
            "approved plan changed",
        )?;
        let plan = store.revision().ok_or("proposal missing")?;
        store.approval().ok_or("user approval required")?.validate(
            plan,
            "user",
            plan.policy.version,
        )?;
        let revision = plan.approval_digest()?;
        let run =
            host.start_observed_team_run(&self.start.project, &revision, &self.start.setup_digest)?;
        // The original once-only start remains authoritative even if this publication fails.
        append(
            host,
            &self.start,
            &self.selected,
            Some(self.clock),
            Kind::Execution {
                start_digest: run.start_digest().into(),
            },
        )?;
        self.run = Some(run);
        Ok(())
    }
    pub fn finish(&mut self, host: &RuntimeHost) -> Result<Value, String> {
        self.check(host)?;
        let path = folder(&self.root, &self.start.project)?;
        let state = replay(&self.start, &marks(&path, &self.start)?)?;
        require(
            state.phase == 3 && state.human.is_none(),
            "timeline not executing or human span still active",
        )?;
        self.run
            .as_ref()
            .ok_or("execution token lost")?
            .finish(host)?;
        append(
            host,
            &self.start,
            &self.selected,
            Some(self.clock),
            Kind::Finished,
        )?;
        finalize(host, &self.start, &self.selected)
    }
}

/// Bounded live projection. This is a display of the original timing journal;
/// complete source verification remains the offline inspect entry below.
pub(crate) fn product_view(
    host: &RuntimeHost,
    project: &str,
    live: Option<&TimedTeam>,
) -> Result<cerulean_domain::team_timing::TeamTimingView, String> {
    use cerulean_domain::team_timing::TeamTimingView;
    let path = folder(&host.root, project)?;
    let start: Start = load(&path, "start.json")?;
    require(
        start.project == project && Path::new(&start.data_root) == host.root,
        "timeline project changed",
    )?;
    scope(host, &start)?;
    let selected = digest(&start)?;
    if let Some(owner) = live {
        require(
            owner.selected == selected,
            "live timeline selection changed",
        )?;
    }
    let records = marks(&path, &start)?;
    let state = replay(&start, &records)?;
    let end = if path.join("terminal.json").exists() {
        let v: Value = load(&path, "terminal.json")?;
        let accepted = if let Some(execution) = &state.execution {
            let original: Terminal = load(&directory(&host.root, project)?, "terminal.json")?;
            require(
                &original.start_digest == execution,
                "original execution terminal changed",
            )?;
            original.accepted
        } else {
            false
        };
        require(
            v == summary(&start, &selected, &records, accepted)?,
            "timing terminal changed",
        )?;
        Some(v)
    } else {
        None
    };
    require(
        state.phase != 4 || end.is_some(),
        "timing terminal publication incomplete",
    )?;
    let elapsed = if let Some(v) = &end {
        v["elapsedMs"].as_u64()
    } else {
        live.map(|o| u64::try_from(o.clock.elapsed().as_millis()).map_err(|_| "clock overflow"))
            .transpose()?
    };
    let continuous = elapsed.is_some();
    Ok(TeamTimingView {
        project_id: project.into(),
        start_digest: Some(selected),
        phase: if state.phase == 4 {
            if state.lost {
                "interrupted"
            } else {
                "finished"
            }
        } else if live.is_none() {
            "clockLost"
        } else {
            match state.phase {
                0 => "planning",
                1 => "review",
                2 => "approved",
                _ => "executing",
            }
        }
        .into(),
        clock_live: live.is_some() && state.phase < 4,
        human_active: live.is_some() && state.human.is_some(),
        elapsed_ms: elapsed,
        planning_ms: continuous.then_some(state.proposal).flatten(),
        approval_wait_ms: if continuous {
            state
                .approved
                .or(elapsed)
                .zip(state.proposal)
                .map(|(a, p)| a - p)
        } else {
            None
        },
        execution_and_closure_ms: elapsed.zip(state.approved).map(|(e, a)| e - a),
        human_active_ms: if continuous && start.human_tracking {
            Some(state.human_ms + state.human.zip(elapsed).map(|(a, e)| e - a).unwrap_or(0))
        } else {
            None
        },
        accepted: end.as_ref().and_then(|v| v["accepted"].as_bool()),
        error: None,
    })
}
fn summary(
    start: &Start,
    selected: &str,
    records: &[Mark],
    accepted: bool,
) -> Result<Value, String> {
    summary_for(
        start.human_tracking,
        selected,
        records,
        accepted,
        "chief-planning-request-through-original-terminal-observation",
    )
}
pub(crate) fn summary_for(
    human_tracking: bool,
    selected: &str,
    records: &[Mark],
    accepted: bool,
    timing_scope: &str,
) -> Result<Value, String> {
    let state = replay_for(human_tracking, records)?;
    require(state.phase == 4, "timeline not terminal")?;
    let total = if state.lost { None } else { state.end };
    let human = if state.lost || !human_tracking {
        None
    } else {
        Some(state.human_ms)
    };
    Ok(
        json!({"schema":1,"startDigest":selected,"chainHead":records.last().map(digest).transpose()?,
        "marks":records.len(),"clockContinuity":!state.lost,"elapsedMs":total,
        "planningMs":if state.lost{None}else{state.proposal.or(state.end)},
        "approvalWaitMs":if state.lost{None}else{state.approved.or(state.end).zip(state.proposal).map(|(a,p)|a-p)},
        "executionAndClosureMs":total.zip(state.approved).map(|(e,a)|e-a),"humanActiveMs":human,
        "humanTimeBasis":if human_tracking{"explicit-user-toggles-monotonic"}else{"not-tracked"},
        "accepted":accepted,"interrupted":state.lost,"timingScope":timing_scope,
        "includesPollingDelay":true,"realEfficiencyRecordReady":false}),
    )
}
fn finalize(host: &RuntimeHost, start: &Start, selected: &str) -> Result<Value, String> {
    let path = folder(&host.root, &start.project)?;
    let records = marks(&path, start)?;
    let state = replay(start, &records)?;
    let accepted = if let Some(execution) = state.execution {
        let end: Terminal = load(&directory(&host.root, &start.project)?, "terminal.json")?;
        require(end.start_digest == execution, "original execution differs")?;
        end.accepted
    } else {
        false
    };
    let result = summary(start, selected, &records, accepted)?;
    publish(&path, "terminal.json", &result)?;
    Ok(result)
}
/// Offline reader validates the timeline against the original materials, ledger
/// and execution reader; does not open RuntimeHost or start work.
pub fn inspect(root: &Path, project: &str, selected: &str) -> Result<Value, String> {
    sha256(selected)?;
    let root = root_path(root.to_str().ok_or("invalid root")?)?;
    ordinary(&root.join("writer.lock"), false)?;
    let host_lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(root.join("writer.lock"))
        .map_err(|e| e.to_string())?;
    host_lock
        .try_lock_exclusive()
        .map_err(|_| "Host writer active")?;
    let path = folder(&root, project)?;
    let _lock = lock(&path, false)?;
    let start: Start = load(&path, "start.json")?;
    require(
        start.schema == 1
            && start.project == project
            && Path::new(&start.data_root) == root
            && digest(&start)? == selected,
        "timeline start identity changed",
    )?;
    chrono::DateTime::parse_from_rfc3339(&start.started_at)
        .map_err(|_| "invalid start UTC label")?;
    let binding = read_binding(&root, project)?.ok_or("timeline material missing")?;
    crate::team_materials::verify(&binding)?;
    require(
        digest(&binding)? == start.binding_digest && binding.setup.digest == start.setup_digest,
        "timeline binding changed",
    )?;
    require(
        Path::new(&binding.request.material.data_root)
            .canonicalize()
            .map_err(|e| e.to_string())?
            == root,
        "timeline material selects another data root",
    )?;
    let records = marks(&path, &start)?;
    let state = replay(&start, &records)?;
    let setups = crate::project_setup::load(&root)?;
    if state.execution.is_none() {
        require(
            crate::run_control::RunLedger::read_existing(&root)?
                .all()
                .is_empty()
                && !directory(&root, project)?
                    .try_exists()
                    .map_err(|e| e.to_string())?,
            "original execution exists without timeline linkage",
        )?;
    }
    require(
        setups
            .get(project)
            .is_some_and(|s| s.digest == start.setup_digest),
        "original timeline setup changed",
    )?;
    DurableWorkflow::inspect_existing(&root.join("workflows").join(project), |store| {
        require(
            store.revision().is_none_or(|p| p == &binding.plan),
            "timeline proposal changed",
        )?;
        for mark in &records {
            require(
                mark.history_length <= store.history().len()
                    && mark.ledger_head
                        == mark
                            .history_length
                            .checked_sub(1)
                            .and_then(|i| store.history().get(i))
                            .map(|e| e.hash.clone()),
                "timeline ledger prefix changed",
            )?;
            match mark.kind {
                Kind::Proposal => require(mark.history_length == 1, "proposal boundary changed")?,
                Kind::Approved | Kind::Execution { .. } => {
                    require(mark.history_length == 2, "approval boundary changed")?
                }
                _ => (),
            }
        }
        if state.phase == 4 {
            require(
                records
                    .last()
                    .is_some_and(|m| m.history_length == store.history().len()),
                "workflow advanced after timeline closure",
            )?;
        }
        Ok(())
    })?;
    let execution = state
        .execution
        .as_ref()
        .map(|id| inspect_locked(&root, project, id))
        .transpose()?;
    if state.phase == 4 && state.execution.is_some() {
        require(
            execution
                .as_ref()
                .is_some_and(|v| v["terminal"].is_object()),
            "timeline closure lacks original execution terminal",
        )?;
    }
    let accepted = execution
        .as_ref()
        .is_some_and(|v| v["terminal"]["accepted"] == true);
    let terminal =
        if path.join("terminal.json").exists() || path.join("terminal.json.pending").exists() {
            let saved: Value = load(&path, "terminal.json")?;
            require(
                saved == summary(&start, selected, &records, accepted)?,
                "timeline terminal changed",
            )?;
            Some(saved)
        } else {
            None
        };
    Ok(
        json!({"start":start,"startDigest":selected,"marks":records,"terminal":terminal,
        "execution":execution,"sourceValidated":true,"releaseAccepted":false}),
    )
}

#[cfg(test)]
#[path = "timing/tests.rs"]
mod tests;
