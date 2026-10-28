//! Product controls delegate to the original Host and timing token. No new scheduler.
use super::*;
use cerulean_domain::team_timing::{TeamTimingAction as Action, TeamTimingView};
#[cfg(windows)]
use sha2::Digest;
#[cfg(windows)]
use std::io::Read;
#[cfg(windows)]
use team_observation::timing::{self, TimedTeam};

#[cfg(windows)]
impl RuntimeHost {
    fn timing_exists(&self, project: &str) -> bool {
        self.root.join("team-timelines").join(project).exists()
    }
    fn timing_bound(&self, project: &str) -> bool {
        self.timing_exists(project)
            || self
                .root
                .join("team-material-bindings")
                .join(project)
                .exists()
    }
    pub(crate) fn timing_command_guard(&self, command: &Command) -> Result<(), String> {
        let project = match command {
            Command::ConfigureProject { draft } => Some(draft.project_id.as_str()),
            Command::RequestProjectRevision { project_id, .. }
            | Command::RetireSubmittedPlan { project_id, .. } => Some(project_id.as_str()),
            _ => None,
        };
        if let Some(project) = project {
            identifier(project)?;
            require(
                !self.timing_bound(project),
                "固定实验材料已绑定，保留原方案；新实验请使用新的隔离目录",
            )?;
        }
        Ok(())
    }
    fn use_timing(
        &mut self,
        project: &str,
        selected: Option<&str>,
        f: impl FnOnce(&mut TimedTeam, &mut Self) -> Result<(), String>,
    ) -> Result<(), String> {
        identifier(project)?;
        let mut timeline = self
            .timed_teams
            .remove(project)
            .ok_or("原计时时钟不在本进程；请核对原记录，不重发任务")?;
        let outcome = if selected.is_some_and(|s| s != timeline.start_digest()) {
            Err("计时选择已变化，请重新读取".into())
        } else {
            f(&mut timeline, self)
        };
        self.timed_teams.insert(project.into(), timeline);
        outcome
    }
    pub(crate) fn team_timing_action(&mut self, action: Action) -> Result<(), String> {
        crate::experiment_guard::require_protected(&self.root)?;
        match action {
            Action::Import { path, sha256 } => self.import_timed_team(&path, &sha256),
            Action::Begin {
                project_id,
                setup_digest,
            } => {
                require(
                    !self.timed_teams.contains_key(&project_id),
                    "原计时已经开始，不重复规划",
                )?;
                let token = self.begin_timed_team_planning(&project_id, &setup_digest, true)?;
                self.timed_teams.insert(project_id, token);
                Ok(())
            }
            Action::Human {
                project_id,
                start_digest,
                active,
            } => self.use_timing(&project_id, Some(&start_digest), |t, h| {
                t.set_human_active(h, active)
            }),
            Action::Finish {
                project_id,
                start_digest,
            } => {
                self.use_timing(&project_id, Some(&start_digest), |t, h| {
                    t.finish(h).map(|_| ())
                })?;
                self.timed_teams.remove(&project_id);
                Ok(())
            }
            Action::CloseInterrupted {
                project_id,
                start_digest,
            } => {
                require(
                    !self.timed_teams.contains_key(&project_id),
                    "原时钟仍在运行；本操作只用于重开后的原记录",
                )?;
                self.close_interrupted_team_timeline(&project_id, &start_digest)
                    .map(|_| ())
            }
        }
    }
    pub(crate) fn prepare_team_import(
        &self,
        path: &str,
        selected: &str,
    ) -> Result<crate::team_materials::TeamRequest, String> {
        require(
            self.omp_stop_issues.is_empty(),
            "停止记录需要复核，诊断模式不能导入团队材料",
        )?;
        crate::experiment_guard::require_protected(&self.root)?;
        self.baseline_guard(&Command::TeamTiming {
            action: Action::Import {
                path: path.into(),
                sha256: selected.into(),
            },
        })?;
        require(
            self.projects.is_empty()
                && self.management_work_count() == 0
                && self.runs.all().is_empty(),
            "效率验收需使用空工作台数据目录；已有项目与记录保留",
        )?;
        require(
            selected.len() == 64 && selected.bytes().all(|v| v.is_ascii_hexdigit()),
            "材料SHA256无效",
        )?;
        let path = Path::new(path);
        require(path.is_absolute(), "材料路径须为绝对路径")?;
        ordinary(path, false)?;
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|e| e.to_string())?
            .take(65537)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        require(
            bytes.len() <= 65536
                && format!("{:x}", sha2::Sha256::digest(&bytes)) == selected.to_lowercase(),
            "材料大小超限或摘要变化",
        )?;
        let request: crate::team_materials::TeamRequest =
            serde_json::from_slice(&bytes).map_err(|_| "团队材料格式无效")?;
        require(
            std::fs::canonicalize(&request.material.data_root).map_err(|e| e.to_string())?
                == self.root,
            "材料工作台目录与当前核心不同",
        )?;
        require(
            !self
                .root
                .join("team-material-import.json")
                .try_exists()
                .map_err(|e| e.to_string())?
                && !self
                    .root
                    .join("team-material-import.json.pending")
                    .try_exists()
                    .map_err(|e| e.to_string())?,
            "已有材料导入意图；请核对原记录",
        )?;
        Ok(request)
    }
    pub(crate) fn import_timed_team(&mut self, path: &str, selected: &str) -> Result<(), String> {
        let request = self.prepare_team_import(path, selected)?;
        let b = crate::team_materials::compose(&request)?;
        // Publish before any setup mutation. A partial import must be inspected, never retried.
        let intent = self.root.join("team-material-import.json");
        require(
            !intent.exists() && !self.root.join("team-material-import.json.pending").exists(),
            "已有材料导入意图；请核对原记录",
        )?;
        crate::api_execution::publish(
            &self.root,
            "team-material-import.json",
            &serde_json::json!({"requestPath":path,"sha256":selected.to_lowercase(),"bindingDigest":cerulean_domain::execution::digest(&b)?}),
        )?;
        self.register(b.plan.project.clone())?;
        #[cfg(test)]
        crate::team_import_service::partial_fault_tests::after_registration(self);
        let s = &b.setup.setup;
        let mut validators = vec![(
            s.validator_id.clone(),
            s.check.tool.clone().ok_or("材料验收工具缺失")?,
        )];
        for profile in &s.profiles {
            for stage in &profile.acceptance.stages {
                validators.push((
                    stage.validator_id.clone(),
                    stage.check.tool.clone().ok_or("材料验收工具缺失")?,
                ));
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        for (id, tool) in validators {
            if seen.insert(id.clone()) {
                self.register_validator(
                    id.clone(),
                    id,
                    tool.executable.path,
                    tool.inputs.into_iter().map(|v| v.path).collect(),
                    tool.environment,
                )?;
            }
        }
        self.configure_project(b.configuration_draft())?;
        let project = &b.plan.project.id;
        let initial = self.project_setups[project].digest.clone();
        self.prepare_team_experiment_inputs(project, &initial)?;
        self.bind_team_experiment_materials(&request).map(|_| ())
    }
    pub(crate) fn poll_team_timing_proposals(&mut self) {
        let projects = self.timed_teams.keys().cloned().collect::<Vec<_>>();
        for project in projects {
            if self.timing_errors.contains_key(&project) {
                continue;
            }
            let ready = self.timed_teams[&project].planning();
            let outcome = match ready {
                Ok(true)
                    if self.management_work_count() == 0
                        && self
                            .projects
                            .get(&project)
                            .is_some_and(|s| s.revision().is_some()) =>
                {
                    self.use_timing(&project, None, |t, h| t.proposal_ready(h))
                }
                Err(e) => Err(e),
                _ => Ok(()),
            };
            if let Err(e) = outcome {
                self.timing_errors.insert(project, e);
            }
        }
    }
    pub(crate) fn request_chief_plan_without_timing(
        &mut self,
        project: &str,
        setup: &str,
    ) -> Result<(), String> {
        identifier(project)?;
        require(
            !self.timing_bound(project),
            "固定材料已绑定；请使用计时规划入口，已有任务不重复请求",
        )?;
        self.request_chief_plan(project, setup)
    }
    pub(crate) fn approve_with_timing(
        &mut self,
        project: &str,
        revision: &str,
    ) -> Result<(), String> {
        identifier(project)?;
        if self.timing_bound(project) {
            self.use_timing(project, None, |t, h| t.approve_plan(h, revision))
        } else {
            self.approve(project, revision)
        }
    }
    pub(crate) fn start_project_run_with_timing(
        &mut self,
        project: &str,
        revision: &str,
        setup: &str,
    ) -> Result<(), String> {
        identifier(project)?;
        if self.timing_bound(project) {
            require(
                self.project_setups
                    .get(project)
                    .is_some_and(|s| s.digest == setup)
                    && self
                        .projects
                        .get(project)
                        .and_then(|s| s.revision())
                        .is_some_and(|p| p.approval_digest().is_ok_and(|v| v == revision)),
                "原方案或配置已变化",
            )?;
            self.use_timing(project, None, |t, h| t.start_approved_run(h))
        } else {
            self.start_project_run(project, revision, setup)
        }
    }
    pub(crate) fn team_timing_views(&self) -> Vec<TeamTimingView> {
        self.project_setups.keys().filter(|p|self.timing_bound(p)).map(|project|{
            if !self.timing_exists(project){
                let mut v=empty_view(project,"prepared");
                let binding=crate::api_execution::load::<crate::team_materials::TeamBlueprint>(
                    &self.root.join("team-material-bindings").join(project),"binding.json");
                match binding{
                    Ok(b) if self.project_setups.get(project).is_some_and(|s|s.digest==b.setup.digest)=>(),
                    _=>{v.phase="error".into();v.error=Some("材料绑定缺失、不完整或与当前配置不同".into());}
                }
                return v;
            }
            let view=timing::product_view(self,project,self.timed_teams.get(project));
            match view{
                Ok(mut v)=>{if let Some(e)=self.timing_errors.get(project){v.error=Some(e.clone());}v},
                Err(error)=>TeamTimingView{project_id:project.clone(),start_digest:None,phase:"error".into(),clock_live:false,human_active:false,
                    elapsed_ms:None,planning_ms:None,approval_wait_ms:None,execution_and_closure_ms:None,human_active_ms:None,accepted:None,error:Some(error)}
            }
        }).collect()
    }
}
#[cfg(all(test, windows))]
mod import_diagnostic_tests {
    use super::*;

    #[test]
    fn shutdown_safe_diagnostic_host_cannot_begin_background_import() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("data");
        drop(crate::Store::open(&root).unwrap());
        let backup = temp.path().join("backup");
        let manifest = crate::backup::create(&root, &backup)
            .unwrap()
            .manifest_digest;
        let review = crate::experiment_guard::review(&root, &backup, &manifest).unwrap();
        crate::experiment_guard::promote(&root, &backup, &manifest, &review.digest).unwrap();
        let mut host = RuntimeHost::open(&root).unwrap();
        host.omp_stop_issues.push(omp_jobs::OmpStopIssue {
            project_id: "original-project".into(),
            task_id: "original-task".into(),
            attempt_id: "original-attempt".into(),
            message: "stop review required".into(),
            pending_path: "original.pending".into(),
            pending_sha256: None,
            pending_bytes: None,
            evidence_digest: None,
            acknowledged: false,
        });
        assert!(host.can_shutdown());
        let before = crate::reader_generation::tests::snapshot(&root);
        let mut service = crate::diagnostic_service::ServiceHost::Ready(Box::new(host));
        let response = service.handle_command(Request {
            id: "blocked-import".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::TeamTiming {
                action: Action::Import {
                    path: root.join("source.json").to_string_lossy().into(),
                    sha256: "a".repeat(64),
                },
            },
        });
        let error = response.error.unwrap();
        assert_eq!(error.code, "teamImportRejected");
        assert!(error.message.contains("诊断模式"));
        assert!(service.snapshot().team_import.is_none());
        assert_eq!(service.snapshot().omp_stop_issues.len(), 1);
        assert_eq!(before, crate::reader_generation::tests::snapshot(&root));
    }
}
#[cfg(windows)]
fn empty_view(project: &str, phase: &str) -> TeamTimingView {
    TeamTimingView {
        project_id: project.into(),
        start_digest: None,
        phase: phase.into(),
        clock_live: false,
        human_active: false,
        elapsed_ms: None,
        planning_ms: None,
        approval_wait_ms: None,
        execution_and_closure_ms: None,
        human_active_ms: None,
        accepted: None,
        error: None,
    }
}
#[cfg(not(windows))]
impl RuntimeHost {
    pub(crate) fn timing_command_guard(&self, _: &Command) -> Result<(), String> {
        Ok(())
    }
    pub(crate) fn team_timing_action(&mut self, _: Action) -> Result<(), String> {
        Err("团队效率计时需要Windows核心".into())
    }
    pub(crate) fn team_timing_views(&self) -> Vec<TeamTimingView> {
        vec![]
    }
    pub(crate) fn poll_team_timing_proposals(&mut self) {}
    pub(crate) fn request_chief_plan_without_timing(
        &mut self,
        p: &str,
        s: &str,
    ) -> Result<(), String> {
        self.request_chief_plan(p, s)
    }
    pub(crate) fn approve_with_timing(&mut self, p: &str, r: &str) -> Result<(), String> {
        self.approve(p, r)
    }
    pub(crate) fn start_project_run_with_timing(
        &mut self,
        p: &str,
        r: &str,
        s: &str,
    ) -> Result<(), String> {
        self.start_project_run(p, r, s)
    }
}
