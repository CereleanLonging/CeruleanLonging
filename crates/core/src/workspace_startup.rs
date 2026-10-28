//! Selected original startup evidence, proved by the current durable writer.
use super::*;
use cerulean_domain::execution::{identifier, require, sha256};

impl RuntimeHost {
    pub(super) fn workspace_startup(
        &self,
        query: &WorkspaceQuery,
    ) -> Result<WorkspaceResult, String> {
        let project = match query {
            WorkspaceQuery::CodexStartups { project_id, .. }
            | WorkspaceQuery::CodexStartup { project_id, .. } => project_id,
            _ => return Err("原启动查询类型不符".into()),
        };
        identifier(project)?;
        require(
            !self.base.poisoned
                && self.startups.healthy()
                && self.omp_startups.healthy()
                && self.management.healthy(),
            "原启动证据写入尚未核对，请保留记录并重开",
        )?;
        let records: Vec<_> = self
            .startups
            .all()
            .into_iter()
            .filter(|r| r.intent.project_id == *project)
            .cloned()
            .collect();
        let jobs: Vec<_> = self
            .codex_jobs
            .iter()
            .filter(|(key, job)| key.0 == *project || job.project_id == *project)
            .map(|(key, job)| {
                require(
                    key.0 == job.project_id && key.1 == job.attempt_id,
                    "原编程启动投影身份不一致",
                )?;
                Ok(job.clone())
            })
            .collect::<Result<_, String>>()?;
        let management = self.startup_management_proof(project, &records)?;
        #[cfg(windows)]
        require(self.api_fault.is_none(), "原项目协调器需要重开核对")?;
        #[cfg(windows)]
        let shared = self.api_writers.get(project);
        #[cfg(windows)]
        let mut basis = if let Some(writer) = shared {
            require(!writer.interrupted, "原项目协调器需要重开核对")?;
            writer.owner.workspace_startup_basis(
                records.clone(),
                jobs.clone(),
                management.clone(),
            )?
        } else {
            require(
                !self
                    .active
                    .values()
                    .any(|a| !a.shared && a.snapshot.project.id == *project)
                    && !self
                        .desktop_active
                        .iter()
                        .any(|a| a.snapshot.project.id == *project),
                "原项目由活动写者持有，请保留原记录",
            )?;
            let store = self
                .projects
                .get(project)
                .ok_or("原项目不存在，未创建替代记录")?;
            super::super::startup_recovery::inspect_workflow_startup(
                store,
                &records,
                &jobs,
                &management,
            )?
        };
        #[cfg(not(windows))]
        let mut basis = {
            let store = self
                .projects
                .get(project)
                .ok_or("原项目不存在，未创建替代记录")?;
            super::super::startup_recovery::inspect_workflow_startup(
                store,
                &records,
                &jobs,
                &management,
            )?
        };
        for detail in &mut basis.records {
            let record = records
                .iter()
                .find(|r| r.intent.attempt_id == detail.summary.attempt_id)
                .ok_or("原启动详情缺少不可变来源")?;
            let review = self.workspace_startup_review(
                record,
                &detail.summary.state,
                basis.sequence,
                basis.workflow_hash.as_deref(),
                basis.busy_tasks,
            );
            detail.evidence_digest = review.as_ref().ok().cloned();
            detail.blocked_reason = review.err();
        }
        let revision = crate::workspace_history::version(&(
            "workspace-codex-startups-v1",
            project,
            &basis,
            &records,
            &jobs,
            &management,
            self.startups
                .closure_reviews
                .iter()
                .filter(|r| r.project_id == *project)
                .collect::<Vec<_>>(),
            self.startups
                .reservations
                .iter()
                .filter(|r| r.project_id == *project)
                .collect::<Vec<_>>(),
        ))?;
        let result = match query {
            WorkspaceQuery::CodexStartups {
                offset,
                revision: expected,
                ..
            } => {
                let total = basis.records.len();
                let (entries, next_offset) = crate::workspace_read::page(
                    basis.records.into_iter().map(|view| view.summary),
                    total,
                    *offset,
                    expected.as_deref(),
                    &revision,
                )?;
                WorkspaceResult::CodexStartups(WorkspaceCodexStartupPage {
                    project_id: project.clone(),
                    revision,
                    offset: *offset,
                    total: total as u32,
                    next_offset,
                    entries,
                })
            }
            WorkspaceQuery::CodexStartup {
                attempt_id,
                revision: expected,
                ..
            } => {
                identifier(attempt_id)?;
                sha256(expected)?;
                require(expected == &revision, "原启动记录已变化，请刷新启动首页")?;
                let detail = basis
                    .records
                    .into_iter()
                    .find(|view| view.summary.attempt_id == *attempt_id)
                    .ok_or("原启动记录不存在，未创建替代记录")?;
                WorkspaceResult::CodexStartup(Box::new(WorkspaceCodexStartupDetail {
                    project_id: project.clone(),
                    revision,
                    detail,
                }))
            }
            _ => return Err("原启动查询类型不符".into()),
        };
        crate::workspace_read::bounded(result)
    }

    fn workspace_startup_review(
        &self,
        record: &crate::startup_journal::StartupRecord,
        state: &str,
        sequence: u32,
        workflow_hash: Option<&str>,
        busy_tasks: bool,
    ) -> Result<String, String> {
        require(record.closure.is_none(), "启动记录已经关闭")?;
        require(
            state != "advanced",
            "已进入应用或分配阶段，请使用相应恢复入口",
        )?;
        let intent = &record.intent;
        let project = &intent.project_id;
        #[cfg(windows)]
        require(
            !self.has_codex_activity(project) && !self.active.contains_key(project),
            "执行或验收线程仍持有项目",
        )?;
        require(
            self.runs.healthy() && self.runner_error.is_none(),
            "项目运行控制需要重开恢复",
        )?;
        let runs: Vec<_> = self
            .runs
            .all()
            .into_iter()
            .filter(|r| &r.project_id == project)
            .collect();
        require(
            !runs
                .iter()
                .any(|r| r.state == project_setup::RunState::Active),
            "请先暂停项目调度",
        )?;
        require(
            !self.management_active.keys().any(|op| {
                self.management
                    .intent(op)
                    .is_ok_and(|i| &i.project_id == project)
            }),
            "项目指挥请求仍在运行",
        )?;
        let statuses = self.management.statuses();
        require(
            !statuses.iter().any(|s| {
                &s.project_id == project
                    && Some(s.operation_id.as_str()) != intent.management_operation.as_deref()
                    && !s.state.resolved()
            }),
            "项目存在另一待处置管理请求",
        )?;
        require(!busy_tasks, "项目存在执行中或待对账任务")?;
        require(
            !self.jobs.values().any(|j| {
                &j.project_id == project
                    && matches!(
                        j.state,
                        VerificationState::Running
                            | VerificationState::Cancelling
                            | VerificationState::Unknown
                    )
            }),
            "项目存在待对账验收",
        )?;
        let job = self
            .codex_jobs
            .get(&(project.clone(), intent.attempt_id.clone()));
        require(
            !self.codex_jobs.values().any(|j| {
                &j.project_id == project
                    && j.attempt_id != intent.attempt_id
                    && matches!(
                        j.state,
                        CodexJobState::Starting
                            | CodexJobState::Executing
                            | CodexJobState::Reconciling
                            | CodexJobState::Unknown
                    )
            }),
            "项目存在另一待对账编程记录",
        )?;
        if let Some(job) = job {
            require(
                matches!(
                    job.state,
                    CodexJobState::Unknown | CodexJobState::Failed | CodexJobState::Starting
                ),
                "编程投影与未分配启动不符",
            )?;
        }
        let heads = statuses
            .iter()
            .filter(|s| &s.project_id == project)
            .map(|s| {
                self.management
                    .evidence_digest(&s.operation_id)
                    .map(|h| (s.operation_id.as_str(), h))
            })
            .collect::<Result<Vec<_>, _>>()?;
        digest(
            &serde_json::json!({"purpose":"close-codex-startup-v1", "intent":intent,
            "startupHead":record.head, "workflowSequence":sequence, "workflowHash":workflow_hash,
            "management":heads, "job":job, "runs":runs}),
        )
    }
}
