//! Opt-in complete baseline timing. Original preparation, approval, once-only
//! execution and verification remain in baseline_runner; no management calls.
use super::*;
use crate::baseline_materials::{self, ComposeRequest};
use crate::host::team_observation::timing::{
    read_marks, replay_for, summary_for, Kind, Mark, Replay,
};
use std::sync::{Arc, Mutex, MutexGuard};

const FILES: &[&str] = &[
    "start.json",
    "writer.lock",
    "planning-attempt.json",
    "planning-error.json",
    "proposal.json",
    "approval.json",
    "run-intent.json",
    "run-return.json",
    "terminal.json",
];
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Start {
    schema: u32,
    timeline_root: String,
    data_root: String,
    request: ComposeRequest,
    human_tracking: bool,
    started_at: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Proposal {
    spec_digest: String,
    prepared_head: String,
}
struct Inner {
    path: PathBuf,
    start: Start,
    selected: String,
    clock: Option<Instant>,
    _lock: File,
}
impl Inner {
    fn read(&self) -> Result<Vec<Mark>, String> {
        let original: Start = load(&self.path, "start.json")?;
        require(
            digest(&original)? == self.selected,
            "baseline timeline changed",
        )?;
        read_marks(&self.path, &self.selected, self.start.human_tracking, FILES)
    }
    fn state(&self) -> Result<Replay, String> {
        replay_for(self.start.human_tracking, &self.read()?)
    }
    fn proposal(&self) -> Result<Proposal, String> {
        load(&self.path, "proposal.json")
    }
    fn append(&mut self, kind: Kind) -> Result<(), String> {
        require(
            !self.path.join("terminal.json").exists()
                && !self.path.join("terminal.json.pending").exists(),
            "timeline already closed",
        )?;
        let mut records = self.read()?;
        require(records.len() < 512, "baseline timeline mark limit")?;
        let anchor = if self.path.join("proposal.json").exists() {
            Some(self.proposal()?)
        } else {
            None
        };
        let mark = Mark {
            sequence: records.len() + 1,
            previous: records
                .last()
                .map(digest)
                .transpose()?
                .unwrap_or_else(|| self.selected.clone()),
            offset_ms: self
                .clock
                .map(|c| u64::try_from(c.elapsed().as_millis()).map_err(|_| "clock overflow"))
                .transpose()?,
            kind,
            observed_at: chrono::Utc::now().to_rfc3339(),
            history_length: usize::from(anchor.is_some()),
            ledger_head: anchor.map(|p| p.prepared_head),
        };
        records.push(mark.clone());
        replay_for(self.start.human_tracking, &records)?;
        publish(
            &self.path,
            &format!("mark-{:04}.json", mark.sequence),
            &mark,
        )
    }
    fn close(&mut self, kind: Kind) -> Result<Value, String> {
        let before = inspect_sources(&self.start, &self.path, &self.read()?)?;
        if matches!(kind, Kind::Finished) {
            require(
                before["baseline"]["status"] == "terminal",
                "original baseline terminal required",
            )?;
        } else {
            require(
                before["baseline"].is_null()
                    || before["baseline"]["status"] == "prepared"
                    || (matches!(kind, Kind::Interrupted)
                        && before["baseline"]["status"] == "terminal"),
                "original execution is unresolved; inspect without retry",
            )?;
        }
        self.append(kind)?;
        let value = terminal(&self.start, &self.selected, &self.read()?, &before)?;
        publish(&self.path, "terminal.json", &value)?;
        Ok(value)
    }
}
fn guard(handle: &Arc<Mutex<Inner>>) -> Result<MutexGuard<'_, Inner>, String> {
    handle
        .lock()
        .map_err(|_| "timeline owner failed; inspect original records".into())
}
/// Keeps the original monotonic clock alive. Its methods only record explicit
/// local-user activity, never approve or control execution.
#[derive(Clone)]
pub struct ActivityHandle {
    inner: Arc<Mutex<Inner>>,
}
impl ActivityHandle {
    /// UI polling must not wait behind material validation or closure inspection.
    pub(crate) fn product_view(
        &self,
        id: &str,
    ) -> Result<Option<cerulean_domain::team_timing::TeamTimingView>, String> {
        let i = match self.inner.try_lock() {
            Ok(i) => i,
            Err(std::sync::TryLockError::WouldBlock) => return Ok(None),
            Err(_) => return Err("timeline owner failed; inspect original records".into()),
        };
        let state = i.state()?;
        let end: Option<Value> = optional_record(&i.path, "terminal.json")?;
        let elapsed = if let Some(v) = &end {
            v["elapsedMs"].as_u64()
        } else {
            i.clock.map(|c| c.elapsed().as_millis() as u64)
        };
        Ok(Some(cerulean_domain::team_timing::TeamTimingView {
            project_id: id.into(),
            start_digest: Some(i.selected.clone()),
            phase: if state.phase == 4 {
                if state.lost {
                    "interrupted"
                } else {
                    "finished"
                }
            } else {
                match state.phase {
                    0 => "planning",
                    1 => "review",
                    2 => "approved",
                    _ => "executing",
                }
            }
            .into(),
            clock_live: state.phase < 4 && i.clock.is_some(),
            human_active: state.human.is_some(),
            elapsed_ms: elapsed,
            planning_ms: elapsed.and(state.proposal.or(state.end)),
            approval_wait_ms: elapsed.and(
                state
                    .approved
                    .or(elapsed)
                    .zip(state.proposal)
                    .map(|(a, p)| a - p),
            ),
            execution_and_closure_ms: elapsed.zip(state.approved).map(|(a, p)| a - p),
            human_active_ms: elapsed
                .map(|e| state.human_ms + state.human.map(|h| e - h).unwrap_or(0)),
            accepted: end.as_ref().and_then(|v| v["accepted"].as_bool()),
            error: None,
        }))
    }
    pub(crate) fn try_set_active(&self, active: bool) -> Result<(), String> {
        // Wait only for the short original append critical section. This never
        // retries an append, records a duplicate toggle, or waits behind a job
        // indefinitely. Product controls reject long approval/closure jobs.
        let started = Instant::now();
        let mut i = loop {
            match self.inner.try_lock() {
                Ok(i) => break i,
                Err(std::sync::TryLockError::WouldBlock)
                    if started.elapsed() < std::time::Duration::from_millis(750) =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(5))
                }
                Err(_) => return Err("计时记录忙或已中断，请等待当前核对结束".into()),
            }
        };
        i.append(if active {
            Kind::HumanStarted
        } else {
            Kind::HumanStopped
        })
    }
    pub fn set_active(&self, active: bool) -> Result<(), String> {
        guard(&self.inner)?.append(if active {
            Kind::HumanStarted
        } else {
            Kind::HumanStopped
        })
    }
    fn started(&self, selected: &str) -> Result<(), String> {
        guard(&self.inner)?.append(Kind::Execution {
            start_digest: selected.into(),
        })
    }
}
pub struct TimedBaseline {
    inner: Arc<Mutex<Inner>>,
}
impl TimedBaseline {
    #[cfg(test)]
    pub(crate) async fn run_product_fixture(
        &mut self,
        cancel: &mut oneshot::Receiver<()>,
        verification_cancel: &AtomicBool,
    ) -> Result<Terminal, String> {
        let mut calls = 0;
        // Only the ignored native lifecycle fixture selects this bounded delay.
        // This entire transport is cfg(test); no production protocol gains a gate.
        let delay = if std::env::var_os("CERULEAN_NATIVE_LIFECYCLE_260").is_some() {
            std::time::Duration::from_secs(60)
        } else {
            std::time::Duration::from_millis(300)
        };
        self.execute(
            "synthetic-fixture",
            |_| {
                ApiClient::new(cerulean_domain::secret::SecretInput::new(
                    "synthetic-256-not-a-key".into(),
                )?)
            },
            &mut async |_: &ApiClient, request, cancel: &mut oneshot::Receiver<()>, _gateway: &mut cerulean_adapters::api_generation::GatewayObserver<'_>| {
                calls += 1;
                // Bounded delay exposes the real background control window without network.
                tokio::select! {
                    _= &mut *cancel => Err(ApiFailure::LocalCancellation{}),
                    _=tokio::time::sleep(delay)=>crate::workload_fixture_252::worker(&request,calls)
                }
            },
            cancel,
            verification_cancel,
        )
        .await
    }
    /// Begins before actual composition/preparation, after path eligibility only.
    /// The timeline root must be a new sibling, separate from all execution inputs.
    pub fn begin(
        timeline_root: &Path,
        request: ComposeRequest,
        human_tracking: bool,
    ) -> Result<Self, String> {
        sha256(&request.manifest_sha256)?;
        let timeline = prospective(timeline_root)?;
        let data = prospective(Path::new(&request.data_root))?;
        require(
            !timeline.try_exists().map_err(|e| e.to_string())?
                && !data.try_exists().map_err(|e| e.to_string())?,
            "timeline and baseline data must both be new; do not resume an intent",
        )?;
        for other in [
            &data,
            &root_path(&request.project_root)?,
            &root_path(&request.bundle)?,
            &root_path(&request.worktree.pool_root)?,
        ] {
            require(
                !contains_path(&timeline, other) && !contains_path(other, &timeline),
                "timeline overlaps execution inputs",
            )?;
        }
        fs::create_dir(&timeline).map_err(|e| e.to_string())?;
        let lock = super::lock(&timeline, true)?;
        let clock = Instant::now();
        let start = Start {
            schema: 1,
            timeline_root: timeline.to_string_lossy().into(),
            data_root: data.to_string_lossy().into(),
            request,
            human_tracking,
            started_at: chrono::Utc::now().to_rfc3339(),
        };
        let selected = digest(&start)?;
        publish(&timeline, "start.json", &start)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(Inner {
                path: timeline,
                start,
                selected,
                clock: Some(clock),
                _lock: lock,
            })),
        })
    }
    pub fn start_digest(&self) -> Result<String, String> {
        Ok(guard(&self.inner)?.selected.clone())
    }
    pub fn activity(&self) -> ActivityHandle {
        ActivityHandle {
            inner: self.inner.clone(),
        }
    }
    /// Construct and propose once. A failure record is retained; no implicit retry.
    pub fn prepare(&mut self) -> Result<Value, String> {
        let (request, path, selected) = {
            let i = guard(&self.inner)?;
            require(i.state()?.phase == 0, "planning already finished")?;
            new_record(&i.path, "planning-attempt.json")?;
            publish(
                &i.path,
                "planning-attempt.json",
                &json!({"startDigest":i.selected}),
            )?;
            (i.start.request.clone(), i.path.clone(), i.selected.clone())
        };
        let outcome: Result<Value, String> = (|| {
            let (spec, _) = baseline_materials::compose(&request)?;
            let spec_digest = digest(&spec)?;
            let ready = super::prepare(Path::new(&request.data_root), spec)?;
            let prepared: Prepared = load(Path::new(&request.data_root), "prepared.json")?;
            let proposal = Proposal {
                spec_digest,
                prepared_head: prepared.ledger_head,
            };
            let mut i = guard(&self.inner)?;
            publish(&i.path, "proposal.json", &proposal)?;
            i.append(Kind::Proposal)?;
            Ok(ready)
        })();
        if outcome.is_err() {
            let _owner = guard(&self.inner)?;
            publish(
                &path,
                "planning-error.json",
                &json!({"startDigest":selected,"result":"composition-or-preparation-failed"}),
            )?;
        }
        outcome
    }
    /// Explicit user approval only; the original runner validates it again.
    pub fn approve(&mut self, approval: RunApproval) -> Result<(), String> {
        let mut i = guard(&self.inner)?;
        let state = i.state()?;
        require(
            state.phase == 1 && state.human.is_none(),
            "review incomplete or human activity open",
        )?;
        let proposal = i.proposal()?;
        require(
            approval.spec_digest == proposal.spec_digest,
            "approval selects another spec",
        )?;
        let session = Session::open(Path::new(&i.start.data_root), &proposal.spec_digest)?;
        session.fresh()?;
        approval
            .approval
            .validate(&session.spec.plan, "user", session.spec.plan.policy.version)?;
        new_record(&i.path, "approval.json")?;
        publish(&i.path, "approval.json", &approval)?;
        i.append(Kind::Approved)
    }
    /// Uses the explicitly selected credential root, without copying credentials.
    pub async fn run_from_vault(
        &mut self,
        credential_root: &Path,
        cancel: &mut oneshot::Receiver<()>,
        verification_cancel: &AtomicBool,
    ) -> Result<Terminal, String> {
        self
            .execute(
                "production-api",
                |spec| {
                    ApiClient::new(crate::credential_vault::load_for(
                        credential_root,
                        spec.plan.execution[0]
                            .api_worker
                            .as_ref()
                            .unwrap()
                            .target
                            .provider(),
                    )?)
                },
                &mut async |client: &ApiClient,
                            prepared,
                            cancel: &mut oneshot::Receiver<()>,
                            gateway: &mut cerulean_adapters::api_generation::GatewayObserver<
                    '_,
                >| {
                    client.send_with_gateway(prepared, cancel, gateway).await
                },
                cancel,
                verification_cancel,
            )
            .await
    }
    async fn execute(
        &mut self,
        transport: &str,
        make_client: impl FnOnce(&RunSpec) -> Result<ApiClient, String>,
        send: &mut impl AsyncFnMut(
            &ApiClient,
            PreparedRequest,
            &mut oneshot::Receiver<()>,
            &mut cerulean_adapters::api_generation::GatewayObserver<'_>,
        ) -> Result<ApiReply, ApiFailure>,
        cancel: &mut oneshot::Receiver<()>,
        verification_cancel: &AtomicBool,
    ) -> Result<Terminal, String> {
        let (root, selected, approval, path) = {
            let i = guard(&self.inner)?;
            require(i.state()?.phase == 2, "timed user approval required")?;
            let proposal = i.proposal()?;
            let approval: RunApproval = load(&i.path, "approval.json")?;
            new_record(&i.path, "run-intent.json")?;
            publish(
                &i.path,
                "run-intent.json",
                &json!({"startDigest":i.selected,"specDigest":proposal.spec_digest,
                "approvalDigest":digest(&approval)?}),
            )?;
            (
                PathBuf::from(&i.start.data_root),
                proposal.spec_digest,
                approval,
                i.path.clone(),
            )
        };
        let handle = self.activity();
        let outcome = super::execute_observed(
            &root,
            &selected,
            approval,
            transport,
            make_client,
            send,
            cancel,
            verification_cancel,
            move |start| handle.started(start),
        )
        .await;
        let _owner = guard(&self.inner)?;
        publish(
            &path,
            "run-return.json",
            &json!({"specDigest":selected,"returnedTerminal":outcome.is_ok()}),
        )?;
        outcome
    }
    /// User may finish activity first. The native terminal is not rewritten.
    pub fn finish(&mut self) -> Result<Value, String> {
        let mut i = guard(&self.inner)?;
        require(
            i.state()?.phase == 3 && i.state()?.human.is_none(),
            "execution or human activity not closed",
        )?;
        let returned: Value = load(&i.path, "run-return.json")?;
        require(
            returned["returnedTerminal"] == true,
            "original call has no terminal return",
        )?;
        i.close(Kind::Finished)
    }
    /// Known stop before native execution, including rejected/cancelled preparation.
    pub fn abort_before_execution(&mut self) -> Result<Value, String> {
        let mut i = guard(&self.inner)?;
        require(
            i.state()?.phase < 3 && i.state()?.human.is_none(),
            "original execution or human activity remains",
        )?;
        i.close(Kind::Aborted)
    }
}
fn validate_start(start: &Start, path: &Path, selected: &str) -> Result<(), String> {
    require(
        start.schema == 1
            && digest(start)? == selected
            && Path::new(&start.timeline_root) == path
            && prospective(Path::new(&start.request.data_root))? == Path::new(&start.data_root),
        "baseline timeline scope changed",
    )?;
    chrono::DateTime::parse_from_rfc3339(&start.started_at)
        .map_err(|_| "invalid timeline start label")?;
    Ok(())
}
fn inspect_sources(start: &Start, path: &Path, records: &[Mark]) -> Result<Value, String> {
    let state = replay_for(start.human_tracking, records)?;
    let data = Path::new(&start.data_root);
    let selected = digest(start)?;
    let attempt = optional_record(path, "planning-attempt.json")?;
    let error = optional_record(path, "planning-error.json")?;
    let intent = optional_record(path, "run-intent.json")?;
    let returned = optional_record(path, "run-return.json")?;
    if let Some(v) = &attempt {
        require(
            *v == json!({"startDigest":selected}),
            "planning intent changed",
        )?;
    }
    if let Some(v) = &error {
        require(
            attempt.is_some()
                && *v
                    == json!({"startDigest":selected,"result":"composition-or-preparation-failed"}),
            "planning failure unlinked",
        )?;
    }
    require(
        path.join("approval.json").exists() == state.approved.is_some(),
        "approval publication lacks its original mark",
    )?;
    if !path.join("proposal.json").exists() {
        require(
            intent.is_none() && returned.is_none(),
            "run records lack proposal",
        )?;
        require(
            !data.try_exists().map_err(|e| e.to_string())?,
            "partial original preparation exists; inspect it without restarting",
        )?;
        require(
            records
                .iter()
                .all(|m| m.history_length == 0 && m.ledger_head.is_none()),
            "timeline claims a missing proposal",
        )?;
        require(
            state.proposal.is_none() && state.execution.is_none() && state.approved.is_none(),
            "timeline activity lacks proposal",
        )?;
        return Ok(json!({"baseline":null,"originalTerminalDigest":null}));
    }
    require(
        attempt.is_some() && error.is_none() && state.proposal.is_some(),
        "proposal incomplete or planning failed; inspect without retry",
    )?;
    let proposal: Proposal = load(path, "proposal.json")?;
    let report = super::inspect(data, &proposal.spec_digest)?;
    let spec: RunSpec = load(data, "run-spec.json")?;
    require(
        digest(&spec.material)? == digest(&Some(start.request.clone()))?,
        "original baseline material differs from enrolled request",
    )?;
    let prepared: Prepared = load(data, "prepared.json")?;
    require(
        prepared.ledger_head == proposal.prepared_head
            && prepared.spec_digest == proposal.spec_digest,
        "timeline proposal differs from original prepared record",
    )?;
    let mut proposed = false;
    for mark in records {
        proposed |= matches!(mark.kind, Kind::Proposal);
        require(
            if proposed {
                mark.history_length == 1
                    && mark.ledger_head.as_deref() == Some(proposal.prepared_head.as_str())
            } else {
                mark.history_length == 0 && mark.ledger_head.is_none()
            },
            "timeline original proposal anchor changed",
        )?;
        if matches!(
            mark.kind,
            Kind::Proposal | Kind::Approved | Kind::Execution { .. } | Kind::Finished
        ) {
            require(
                mark.history_length == 1,
                "phase has no original proposal anchor",
            )?;
        }
    }
    if state.approved.is_some() {
        let approval: RunApproval = load(path, "approval.json")?;
        require(
            approval.spec_digest == proposal.spec_digest,
            "timeline approval changed",
        )?;
        approval
            .approval
            .validate(&spec.plan, "user", spec.plan.policy.version)?;
        if let Some(v) = &intent {
            require(
                *v == json!({"startDigest":selected,"specDigest":proposal.spec_digest,"approvalDigest":digest(&approval)?}),
                "run intent changed",
            )?;
        }
        if report["started"].is_object() {
            require(
                serde_json::to_value(&approval.approval).map_err(|e| e.to_string())?
                    == report["started"]["approval"],
                "native approval differs from timeline user approval",
            )?;
        }
    } else {
        require(
            intent.is_none() && returned.is_none(),
            "run records lack user approval",
        )?;
    }
    if let Some(v) = &returned {
        require(
            intent.is_some()
                && (*v == json!({"specDigest":proposal.spec_digest,"returnedTerminal":true})
                    || *v == json!({"specDigest":proposal.spec_digest,"returnedTerminal":false})),
            "run return unlinked or malformed",
        )?;
        if v["returnedTerminal"] == true {
            require(
                report["terminal"].is_object(),
                "returned terminal is missing",
            )?;
        }
    }
    if let Some(selected) = state.execution {
        require(intent.is_some(), "native start lacks timed run intent")?;
        let native: super::Started = load(data, "started.json")?;
        require(
            digest(&native)? == selected,
            "timeline execution link differs from original intent",
        )?;
    } else {
        require(
            report["started"].is_null(),
            "native start has no timeline linkage; no retry",
        )?;
    }
    let original = if report["terminal"].is_object() {
        Some(digest(&load::<super::Terminal>(data, "terminal.json")?)?)
    } else {
        None
    };
    if matches!(records.last().map(|m| &m.kind), Some(Kind::Finished)) {
        require(
            original.is_some()
                && returned
                    .as_ref()
                    .is_some_and(|v| v["returnedTerminal"] == true),
            "finished timing lacks original return",
        )?;
    }
    if state.phase == 4 {
        require(
            report["status"] != "started-without-terminal",
            "terminal timing cannot close unresolved execution",
        )?;
    }
    Ok(json!({"baseline":report,"originalTerminalDigest":original}))
}
fn optional_record(path: &Path, name: &str) -> Result<Option<Value>, String> {
    if path.join(name).try_exists().map_err(|e| e.to_string())? {
        Ok(Some(load(path, name)?))
    } else {
        Ok(None)
    }
}
fn new_record(path: &Path, name: &str) -> Result<(), String> {
    require(
        !path.join(name).try_exists().map_err(|e| e.to_string())?
            && !path
                .join(format!("{name}.pending"))
                .try_exists()
                .map_err(|e| e.to_string())?,
        "original intent already exists; inspect it without retry",
    )
}
fn terminal(
    start: &Start,
    selected: &str,
    marks: &[Mark],
    sources: &Value,
) -> Result<Value, String> {
    let mut result = summary_for(
        start.human_tracking,
        selected,
        marks,
        sources["baseline"]["terminal"]["accepted"] == true,
        "baseline-compose-and-prepare-through-original-terminal-observation",
    )?;
    result["originalTerminalDigest"] = sources["originalTerminalDigest"].clone();
    result["managementModelRequests"] = json!(0);
    result["originalStatus"] = sources["baseline"]["status"].clone();
    Ok(result)
}
/// Read-only after the live owner and all activity handles have gone away.
pub(crate) fn original_start_digest(path: &Path) -> Result<String, String> {
    digest(&load::<Start>(path, "start.json")?)
}
pub fn inspect(path: &Path, selected: &str) -> Result<Value, String> {
    sha256(selected)?;
    let path = root_path(path.to_str().ok_or("invalid timeline path")?)?;
    let _lock = super::lock(&path, false)?;
    let start: Start = load(&path, "start.json")?;
    validate_start(&start, &path, selected)?;
    let marks = read_marks(&path, selected, start.human_tracking, FILES)?;
    let sources = inspect_sources(&start, &path, &marks)?;
    let end = if path.join("terminal.json").exists() || path.join("terminal.json.pending").exists()
    {
        let end: Value = load(&path, "terminal.json")?;
        require(
            end == terminal(&start, selected, &marks, &sources)?,
            "baseline timing terminal changed",
        )?;
        Some(end)
    } else {
        None
    };
    Ok(
        json!({"start":start,"startDigest":selected,"marks":marks,"terminal":end,
        "sources":sources,"sourceValidated":true,"releaseAccepted":false}),
    )
}
/// Never reopens a live clock or retries a native call. An original started
/// execution without a terminal remains unresolved and cannot be closed here.
pub fn close_interrupted(path: &Path, selected: &str) -> Result<Value, String> {
    sha256(selected)?;
    let path = root_path(path.to_str().ok_or("invalid timeline path")?)?;
    let lock = super::lock(&path, false)?;
    let start: Start = load(&path, "start.json")?;
    validate_start(&start, &path, selected)?;
    let mut inner = Inner {
        path,
        start,
        selected: selected.into(),
        clock: None,
        _lock: lock,
    };
    inner.close(Kind::Interrupted)
}

#[cfg(test)]
mod tests;
