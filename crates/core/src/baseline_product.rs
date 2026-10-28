//! Product controller only: all work and timing use the original baseline runner.
use super::*;
use crate::{
    api_execution::{load, publish},
    artifacts::{contains_path, root_path},
    baseline_materials::ComposeRequest,
    baseline_runner::{
        timing::{self, ActivityHandle, TimedBaseline},
        RunApproval, RunSpec,
    },
};
use cerulean_domain::{
    baseline_product::{BaselineAction as Action, BaselineReview, BaselineView},
    team_timing::TeamTimingView,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::JoinHandle,
};
use tokio::sync::oneshot;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Binding {
    schema: u32,
    id: String,
    source_path: String,
    source_sha256: String,
    source_body: String,
    request: ComposeRequest,
}
struct Entry {
    binding: Binding,
    selected: String,
    path: PathBuf,
    token: Option<TimedBaseline>,
    activity: Option<ActivityHandle>,
    view: BaselineView,
    spec: Option<RunSpec>,
}
struct Completion {
    token: Option<TimedBaseline>,
    result: Result<Option<serde_json::Value>, String>,
}
struct Job {
    id: String,
    kind: &'static str,
    thread: JoinHandle<Completion>,
    cancel: Option<oneshot::Sender<()>>,
    verification: Arc<AtomicBool>,
}
pub(super) struct Controller {
    entries: BTreeMap<String, Entry>,
    job: Option<Job>,
    review: Option<BaselineReview>,
}
fn separate(a: &Path, b: &Path) -> Result<(), String> {
    require(
        !contains_path(a, b) && !contains_path(b, a),
        "实验目录与既有项目或材料重叠",
    )
}
fn paths(b: &Binding) -> Result<Vec<PathBuf>, String> {
    [
        &b.request.project_root,
        &b.request.bundle,
        &b.request.worktree.pool_root,
    ]
    .iter()
    .map(|p| root_path(p))
    .collect()
}
fn intent(path: &Path, name: &str, value: &impl Serialize) -> Result<(), String> {
    require(
        !path.join(name).try_exists().map_err(|e| e.to_string())?
            && !path
                .join(format!("{name}.pending"))
                .try_exists()
                .map_err(|e| e.to_string())?,
        "已有原操作意图；只读核对，不重复执行",
    )?;
    publish(path, name, value)
}
fn initial(id: &str) -> BaselineView {
    BaselineView {
        id: id.into(),
        binding_digest: None,
        phase: "imported".into(),
        busy: false,
        cancel_requested: false,
        spec_digest: None,
        timing: None,
        error: None,
    }
}
impl Controller {
    pub(super) fn open(root: &Path) -> Result<Self, String> {
        let mut this = Self {
            entries: BTreeMap::new(),
            job: None,
            review: None,
        };
        let folder = root.join("baseline-experiments");
        if !folder.try_exists().map_err(|e| e.to_string())? {
            return Ok(this);
        }
        ordinary(&folder, true)?;
        for item in fs::read_dir(&folder).map_err(|e| e.to_string())? {
            require(this.entries.len() < 64, "单执行者实验登记超过64项")?;
            let item = item.map_err(|e| e.to_string())?;
            let path = item.path();
            ordinary(&path, true)?;
            let id = item
                .file_name()
                .into_string()
                .map_err(|_| "实验名称非Unicode")?;
            identifier(&id)?;
            let b: Binding = load(&path, "binding.json")?;
            require(
                b.schema == 1
                    && b.id == id
                    && format!("{:x}", Sha256::digest(b.source_body.as_bytes())) == b.source_sha256,
                "实验材料导入记录变化",
            )?;
            let mut derived: ComposeRequest =
                serde_json::from_str(&b.source_body).map_err(|_| "原材料无效")?;
            derived.data_root = path.join("data").to_string_lossy().into();
            require(
                digest(&derived)? == digest(&b.request)?,
                "实验数据目录派生与原材料不一致",
            )?;
            let selected = digest(&b)?;
            let mut view = initial(&id);
            view.binding_digest = Some(selected.clone());
            view.phase = "needsRead".into();
            view.cancel_requested =
                path.join("cancel.json").exists() || path.join("cancel.json.pending").exists();
            this.entries.insert(
                id,
                Entry {
                    binding: b,
                    selected,
                    path,
                    token: None,
                    activity: None,
                    view,
                    spec: None,
                },
            );
        }
        Ok(this)
    }
    pub(super) fn busy(&self) -> bool {
        self.job.is_some()
    }
    pub(super) fn views(&self) -> Vec<BaselineView> {
        self.entries.values().map(|e| e.view.clone()).collect()
    }
    pub(super) fn review(&self) -> Option<BaselineReview> {
        self.review.clone()
    }
    fn scope(&self, path: &Path) -> Result<(), String> {
        for e in self.entries.values() {
            for reserved in paths(&e.binding)? {
                separate(path, &reserved)?;
            }
        }
        Ok(())
    }
    fn selected(&mut self, id: &str, start: &str) -> Result<&mut Entry, String> {
        identifier(id)?;
        sha256(start)?;
        let e = self.entries.get_mut(id).ok_or("原实验不存在")?;
        require(
            e.view
                .timing
                .as_ref()
                .and_then(|v| v.start_digest.as_deref())
                == Some(start),
            "原计时引用已变化或尚未核对",
        )?;
        Ok(e)
    }
    fn refresh_views(&mut self) {
        for e in self.entries.values_mut() {
            if let Some(a) = &e.activity {
                match a.product_view(&e.binding.id) {
                    Ok(Some(v)) => e.view.timing = Some(v),
                    Ok(None) => (),
                    Err(err) => e.view.error = Some(err),
                }
            }
        }
    }
    pub(super) fn poll(&mut self) {
        self.refresh_views();
        if !self.job.as_ref().is_some_and(|j| j.thread.is_finished()) {
            return;
        }
        let job = self.job.take().unwrap();
        let e = self.entries.get_mut(&job.id).unwrap();
        e.view.busy = false;
        match job.thread.join() {
            Err(_) => {
                e.token = None;
                e.activity = None;
                e.view.phase = "unknown".into();
                e.view.error = Some("后台操作中断；核对原记录，不重试".into());
                if let Some(t) = &mut e.view.timing {
                    t.clock_live = false;
                    t.elapsed_ms = None;
                    t.planning_ms = None;
                    t.approval_wait_ms = None;
                    t.execution_and_closure_ms = None;
                    t.human_active_ms = None;
                }
            }
            Ok(done) => {
                e.token = done.token;
                match done.result {
                    Err(err) => {
                        e.view.error = Some(err);
                        e.view.phase = "error".into();
                    }
                    Ok(value) => {
                        e.view.error = None;
                        e.view.phase = match job.kind {
                            "prepare" => "review",
                            "approve" => "approved",
                            "start" => "returned",
                            "finish" | "abort" | "close" => "closed",
                            _ => "clockLost",
                        }
                        .into();
                        if job.kind == "read" {
                            if let Some(report) = &value {
                                e.view.timing = Some(offline_view(&job.id, report));
                                e.view.phase = if report["terminal"].is_object() {
                                    "closed"
                                } else {
                                    "clockLost"
                                }
                                .into();
                            } else {
                                e.view.phase = "imported".into();
                            }
                        }
                        if matches!(job.kind, "prepare" | "read")
                            && e.path.join("data/run-spec.json").exists()
                        {
                            match load::<RunSpec>(&e.path.join("data"), "run-spec.json") {
                                Ok(spec) => match digest(&spec) {
                                    Ok(d) => {
                                        e.view.spec_digest = Some(d);
                                        e.spec = Some(spec)
                                    }
                                    Err(err) => e.view.error = Some(err),
                                },
                                Err(err) => e.view.error = Some(err),
                            }
                        }
                        if matches!(job.kind, "finish" | "abort" | "close") {
                            if let Some(v) = value {
                                if let Some(t) = &mut e.view.timing {
                                    apply_terminal(t, &v);
                                }
                            }
                            e.activity = None;
                            e.token = None;
                        }
                    }
                }
            }
        }
        self.refresh_views();
        let e = &self.entries[&job.id];
        if matches!(job.kind, "prepare" | "read") {
            self.review = e
                .spec
                .as_ref()
                .zip(e.view.timing.as_ref().and_then(|v| v.start_digest.as_ref()))
                .map(|(s, start)| BaselineReview {
                    id: job.id,
                    start_digest: start.clone(),
                    spec_digest: e.view.spec_digest.clone().unwrap(),
                    plan: s.plan.clone(),
                });
        }
    }
    fn launch(
        &mut self,
        id: &str,
        kind: &'static str,
        credential_root: &Path,
        #[cfg(test)] fixture: bool,
    ) -> Result<(), String> {
        require(self.job.is_none(), "另一个基线操作仍在后台；请等待完成")?;
        let e = self.entries.get_mut(id).ok_or("实验不存在")?;
        let path = e.path.clone();
        let credential_root = credential_root.to_owned();
        let binding_digest = e.selected.clone();
        let bound_request = serde_json::to_value(&e.binding.request).map_err(|e| e.to_string())?;
        let selected = e.view.timing.as_ref().and_then(|v| v.start_digest.clone());
        let approval = e
            .spec
            .as_ref()
            .map(|s| {
                Ok::<_, String>(RunApproval {
                    spec_digest: e.view.spec_digest.clone().ok_or("原方案摘要缺失")?,
                    approval: Approval {
                        plan_id: s.plan.workflow.id.clone(),
                        revision: s.plan.workflow.version,
                        policy_version: s.plan.policy.version,
                        revision_digest: s.plan.approval_digest()?,
                        user_id: "user".into(),
                        approved_at: chrono::Utc::now().to_rfc3339(),
                    },
                })
            })
            .transpose()?;
        let mut token = e.token.take();
        let (tx, mut rx) = oneshot::channel();
        let verification = Arc::new(AtomicBool::new(false));
        let check = verification.clone();
        e.view.busy = true;
        e.view.phase = format!("{kind}Pending");
        let spawned = std::thread::Builder::new()
            .name(format!("baseline-{kind}"))
            .spawn(move || {
                let result = (|| match kind {
                    "prepare" => token.as_mut().ok_or("原时钟丢失")?.prepare().map(Some),
                    "approve" => token
                        .as_mut()
                        .ok_or("原时钟丢失")?
                        .approve(approval.ok_or("原批准方案缺失")?)
                        .map(|_| None),
                    "start" => {
                        let runtime = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .map_err(|e| e.to_string())?;
                        let t = token.as_mut().ok_or("原时钟丢失")?;
                        #[cfg(test)]
                        if fixture {
                            return runtime
                                .block_on(t.run_product_fixture(&mut rx, &check))
                                .map(|_| None);
                        }
                        runtime
                            .block_on(t.run_from_vault(&credential_root, &mut rx, &check))
                            .map(|_| None)
                    }
                    "finish" => token.as_mut().ok_or("原时钟丢失")?.finish().map(Some),
                    "abort" => token
                        .as_mut()
                        .ok_or("原时钟丢失")?
                        .abort_before_execution()
                        .map(Some),
                    "close" => timing::close_interrupted(
                        &path.join("timeline"),
                        selected.as_deref().ok_or("计时引用缺失")?,
                    )
                    .map(Some),
                    "read" => {
                        if !path.join("timeline").exists() {
                            require(
                                !path.join("prepare.json").exists()
                                    && !path.join("prepare.json.pending").exists(),
                                "准备意图存在但时间线缺失；不重试",
                            )?;
                            return Ok(None);
                        }
                        let prepared: serde_json::Value = load(&path, "prepare.json")?;
                        require(
                            prepared == serde_json::json!({"bindingDigest":binding_digest}),
                            "准备操作与材料不符",
                        )?;
                        let timeline = path.join("timeline");
                        let report =
                            timing::inspect(&timeline, &timing::original_start_digest(&timeline)?)?;
                        require(
                            report["start"]["request"] == bound_request,
                            "原计时与导入材料不符",
                        )?;
                        Ok(Some(report))
                    }
                    _ => Err("无效后台操作".into()),
                })();
                Completion { token, result }
            });
        match spawned {
            Ok(thread) => {
                self.job = Some(Job {
                    id: id.into(),
                    kind,
                    thread,
                    cancel: Some(tx),
                    verification,
                });
                Ok(())
            }
            Err(err) => {
                e.activity = None;
                e.view.busy = false;
                e.view.phase = "unknown".into();
                e.view.error = Some(err.to_string());
                if let Some(t) = &mut e.view.timing {
                    t.clock_live = false;
                    t.elapsed_ms = None;
                    t.planning_ms = None;
                    t.approval_wait_ms = None;
                    t.execution_and_closure_ms = None;
                    t.human_active_ms = None;
                }
                Err("后台线程未建立；原意图保留，不重复执行".into())
            }
        }
    }
}
fn apply_terminal(t: &mut TeamTimingView, v: &serde_json::Value) {
    t.clock_live = false;
    t.human_active = false;
    t.phase = if v["interrupted"] == true {
        "interrupted"
    } else {
        "finished"
    }
    .into();
    t.elapsed_ms = v["elapsedMs"].as_u64();
    t.planning_ms = v["planningMs"].as_u64();
    t.approval_wait_ms = v["approvalWaitMs"].as_u64();
    t.execution_and_closure_ms = v["executionAndClosureMs"].as_u64();
    t.human_active_ms = v["humanActiveMs"].as_u64();
    t.accepted = v["accepted"].as_bool();
}
fn offline_view(id: &str, r: &serde_json::Value) -> TeamTimingView {
    let mut t = TeamTimingView {
        project_id: id.into(),
        start_digest: r["startDigest"].as_str().map(str::to_owned),
        phase: "clockLost".into(),
        clock_live: false,
        human_active: false,
        elapsed_ms: None,
        planning_ms: None,
        approval_wait_ms: None,
        execution_and_closure_ms: None,
        human_active_ms: None,
        accepted: None,
        error: None,
    };
    if r["terminal"].is_object() {
        apply_terminal(&mut t, &r["terminal"])
    }
    t
}
impl RuntimeHost {
    pub(crate) fn baseline_guard(&self, c: &Command) -> Result<(), String> {
        // New normal-project configuration is frozen while experiments are enrolled.
        // Existing disjoint projects can still run; no baseline gets their resources.
        if !self.baselines.entries.is_empty()
            && matches!(
                c,
                Command::RegisterProject { .. }
                    | Command::ConfigureProject { .. }
                    | Command::CreatePlan { .. }
                    | Command::TeamTiming {
                        action: cerulean_domain::team_timing::TeamTimingAction::Import { .. }
                    }
            )
        {
            return Err("已有单执行者实验登记；请在独立工作台配置新项目，保留原实验目录".into());
        }
        Ok(())
    }
    pub(crate) fn baseline_action(&mut self, a: Action) -> Result<(), String> {
        crate::experiment_guard::require_protected(&self.root)?;
        match &a {
            Action::Approve {
                id,
                start_digest,
                spec_digest,
            } => return self.baseline_authorize(id, start_digest, spec_digest, "approve"),
            Action::Start {
                id,
                start_digest,
                spec_digest,
            } => return self.baseline_authorize(id, start_digest, spec_digest, "start"),
            _ => (),
        }
        if let Action::Import {
            id,
            path,
            sha256: selected,
        } = a
        {
            return self.import_baseline(&id, &path, &selected);
        }
        self.baselines.poll();
        match a {
            Action::Prepare { id, binding_digest } => {
                require(!self.baselines.busy(), "已有基线后台操作")?;
                let e = self.baselines.entries.get_mut(&id).ok_or("实验不存在")?;
                require(
                    e.selected == binding_digest
                        && e.view.phase == "imported"
                        && !e.view.cancel_requested,
                    "材料选择变化或已有准备/停止意图",
                )?;
                intent(
                    &e.path,
                    "prepare.json",
                    &serde_json::json!({"bindingDigest":binding_digest}),
                )?;
                let token = TimedBaseline::begin(
                    &e.path.join("timeline"),
                    e.binding.request.clone(),
                    true,
                )?;
                e.view.timing = token.activity().product_view(&id)?;
                e.activity = Some(token.activity());
                e.token = Some(token);
                self.launch_baseline(&id, "prepare")
            }
            Action::Read { id, binding_digest } => {
                require(!self.baselines.busy(), "已有基线后台操作")?;
                let e = self.baselines.entries.get(&id).ok_or("实验不存在")?;
                require(e.selected == binding_digest, "材料引用变化")?;
                if e.activity.is_some() {
                    self.baselines.review = e
                        .spec
                        .as_ref()
                        .zip(e.view.timing.as_ref().and_then(|v| v.start_digest.as_ref()))
                        .map(|(s, d)| BaselineReview {
                            id: id.clone(),
                            start_digest: d.clone(),
                            spec_digest: e.view.spec_digest.clone().unwrap(),
                            plan: s.plan.clone(),
                        });
                    return Ok(());
                }
                self.baselines.review = None;
                self.launch_baseline(&id, "read")
            }
            Action::Human {
                id,
                start_digest,
                active,
            } => {
                require(
                    !self
                        .baselines
                        .job
                        .as_ref()
                        .is_some_and(|j| j.id == id && !matches!(j.kind, "prepare" | "start")),
                    "正在核对批准或收口，请等待完成",
                )?;
                self.baselines
                    .selected(&id, &start_digest)?
                    .activity
                    .as_ref()
                    .ok_or("原时钟已丢失")?
                    .try_set_active(active)
            }
            Action::Cancel { id, start_digest } => {
                let e = self.baselines.selected(&id, &start_digest)?;
                require(
                    e.activity.is_some() && !e.view.cancel_requested && e.view.phase != "closed",
                    "原任务不接受重复停止或已结束",
                )?;
                intent(
                    &e.path,
                    "cancel.json",
                    &serde_json::json!({"startDigest":start_digest,"remoteCancellationConfirmed":false}),
                )?;
                e.view.cancel_requested = true;
                if let Some(job) = &mut self.baselines.job {
                    if job.id == id {
                        job.verification.store(true, Ordering::SeqCst);
                        if let Some(tx) = job.cancel.take() {
                            let _ = tx.send(());
                        }
                    }
                }
                Ok(())
            }
            Action::Approve {
                id,
                start_digest,
                spec_digest,
            }
            | Action::Start {
                id,
                start_digest,
                spec_digest,
            } => {
                let _ = (id, start_digest, spec_digest);
                unreachable!()
            }
            Action::Finish { id, start_digest } => {
                self.baseline_control(&id, &start_digest, "finish")
            }
            Action::AbortBeforeExecution { id, start_digest } => {
                self.baseline_control(&id, &start_digest, "abort")
            }
            Action::CloseInterrupted { id, start_digest } => {
                self.baseline_control(&id, &start_digest, "close")
            }
            Action::Import { .. } => unreachable!(),
        }
    }
    fn launch_baseline(&mut self, id: &str, kind: &'static str) -> Result<(), String> {
        self.baselines.launch(
            id,
            kind,
            &self.root,
            #[cfg(test)]
            self.baseline_fixture,
        )
    }
    fn baseline_authorize(
        &mut self,
        id: &str,
        start: &str,
        spec: &str,
        kind: &'static str,
    ) -> Result<(), String> {
        self.baselines.poll();
        require(!self.baselines.busy(), "已有基线后台操作")?;
        require(
            self.baselines
                .review
                .as_ref()
                .is_some_and(|r| r.id == id && r.start_digest == start && r.spec_digest == spec),
            "请先审阅原固定方案",
        )?;
        let e = self.baselines.selected(id, start)?;
        require(
            e.token.is_some()
                && !e.view.cancel_requested
                && e.view.spec_digest.as_deref() == Some(spec)
                && e.view.error.is_none(),
            "原批准或计时不可继续",
        )?;
        require(
            e.view.phase
                == if kind == "approve" {
                    "review"
                } else {
                    "approved"
                },
            "原任务阶段不接受此操作",
        )?;
        require(
            !e.view.timing.as_ref().is_some_and(|t| t.human_active),
            "请先结束人工参与",
        )?;
        intent(
            &e.path,
            &format!("{kind}.json"),
            &serde_json::json!({"startDigest":start,"specDigest":spec}),
        )?;
        self.launch_baseline(id, kind)
    }
    fn baseline_control(
        &mut self,
        id: &str,
        start: &str,
        kind: &'static str,
    ) -> Result<(), String> {
        require(!self.baselines.busy(), "已有基线后台操作")?;
        let e = self.baselines.selected(id, start)?;
        require(
            !e.view.timing.as_ref().is_some_and(|t| t.human_active),
            "请先结束人工参与",
        )?;
        if kind == "close" {
            require(
                e.activity.is_none() && e.view.phase == "clockLost",
                "只能收口核对后的失钟记录",
            )?;
        } else {
            require(e.token.is_some(), "原时钟丢失，不恢复执行")?;
            require(
                if kind == "finish" {
                    e.view.phase == "returned"
                } else {
                    e.view.timing.as_ref().is_some_and(|t| {
                        matches!(t.phase.as_str(), "planning" | "review" | "approved")
                    })
                },
                "原任务未到可收口阶段",
            )?;
        }
        intent(
            &e.path,
            &format!("{kind}.json"),
            &serde_json::json!({"startDigest":start}),
        )?;
        self.launch_baseline(id, kind)
    }
    fn import_baseline(&mut self, id: &str, path: &str, selected: &str) -> Result<(), String> {
        identifier(id)?;
        sha256(selected)?;
        require(
            !self.baselines.busy() && self.baselines.entries.len() < 64,
            "已有后台操作或实验数量达到上限",
        )?;
        require(
            !self
                .baselines
                .entries
                .keys()
                .any(|k| k.eq_ignore_ascii_case(id)),
            "实验名称已使用",
        )?;
        let body = crate::baseline_materials::bytes(Path::new(path), &selected.to_lowercase())?;
        require(
            body.len() <= 128 * 1024,
            "产品导入请求不得超过128KiB，以保证持久登记可完整回读",
        )?;
        let mut request: ComposeRequest =
            serde_json::from_slice(&body).map_err(|_| "材料请求格式无效")?;
        let folder = self.root.join("baseline-experiments");
        let b = Binding {
            schema: 1,
            id: id.into(),
            source_path: path.into(),
            source_sha256: selected.to_lowercase(),
            source_body: String::from_utf8(body).map_err(|_| "材料不是UTF-8")?,
            request: request.clone(),
        };
        let reserved = paths(&b)?;
        for p in &reserved {
            separate(&self.root, p)?;
            self.baselines.scope(p)?;
            for s in self.snapshot().workflows {
                separate(p, &root_path(&s.project.root)?)?;
                if let Some(w) = s.revision.as_ref().and_then(|r| r.worktree.as_ref()) {
                    separate(p, &root_path(&w.pool_root)?)?;
                }
            }
            for s in self.project_setups.values() {
                if let Some(w) = &s.setup.worktree {
                    separate(p, &root_path(&w.pool_root)?)?;
                }
                if let Some(parent) = s
                    .setup
                    .worker
                    .as_ref()
                    .and_then(|w| w.omp_runtime_parent.as_ref())
                {
                    separate(p, &root_path(parent)?)?;
                }
            }
        }
        if !folder.exists() {
            fs::create_dir(&folder).map_err(|e| e.to_string())?;
        }
        ordinary(&folder, true)?;
        let folder = root_path(folder.to_str().ok_or("目录非Unicode")?)?;
        let area = folder.join(id);
        request.data_root = area.join("data").to_string_lossy().into();
        fs::create_dir(&area).map_err(|e| e.to_string())?;
        let binding = Binding { request, ..b };
        publish(&area, "binding.json", &binding)?;
        let selected = digest(&binding)?;
        let mut view = initial(id);
        view.binding_digest = Some(selected.clone());
        self.baselines.entries.insert(
            id.into(),
            Entry {
                binding,
                selected,
                path: area,
                token: None,
                activity: None,
                view,
                spec: None,
            },
        );
        Ok(())
    }
}
