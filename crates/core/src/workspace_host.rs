use super::*;
#[path = "workspace_management_record_reviews.rs"]
mod management_record_reviews;
use cerulean_domain::workflow::{TaskRun, WorkflowState, WorkflowTask};
use cerulean_domain::workspace::*;

#[path = "workspace_api_publication_reviews.rs"]
pub(crate) mod api_publication_reviews;
#[cfg(all(test, windows))]
#[path = "workspace_api_publication_reviews_tests.rs"]
mod api_publication_reviews_tests;
#[path = "workspace_codex_cancellation.rs"]
pub(crate) mod codex_cancellation;
#[path = "workspace_jobs.rs"]
mod jobs;
#[path = "workspace_management_decisions.rs"]
pub(crate) mod management_decisions;
#[path = "workspace_project_runs.rs"]
pub(crate) mod project_runs;
#[cfg(all(test, windows))]
#[path = "workspace_project_runs_tests.rs"]
mod project_runs_tests;
#[path = "workspace_recovery.rs"]
mod recovery;
#[path = "workspace_revision_reviews.rs"]
pub(crate) mod revision_reviews;
#[cfg(all(test, windows))]
#[path = "workspace_revision_reviews_tests.rs"]
mod revision_reviews_tests;
#[path = "workspace_startup.rs"]
mod startup;
#[path = "workspace_unsent_assignments.rs"]
pub(crate) mod unsent_assignments;
#[path = "workspace_verification_rechecks.rs"]
mod verification_rechecks;

// Hash the selected original state without allocating an encoded full state or
// cloning its evidence. This is a query version, never an approval digest.
struct QueryHash(sha2::Sha256);
impl std::io::Write for QueryHash {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        use sha2::Digest;
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
struct ProjectView<'a> {
    project: &'a Project,
    plan_revision: Option<&'a PlanRevision>,
    approval: Option<&'a Approval>,
    revision_digest: Option<String>,
    sequence: u32,
    state: &'a WorkflowState,
}
impl ProjectView<'_> {
    fn execution_review(&self, expected: Option<&str>) -> Result<WorkspaceResult, String> {
        let original_digest = self
            .plan_revision
            .map(|p| p.approval_digest())
            .transpose()?;
        require(
            original_digest == self.revision_digest,
            "原执行方案摘要不一致，请保留记录核对",
        )?;
        if let Some(expected) = expected {
            cerulean_domain::execution::sha256(expected)?;
            require(
                original_digest.as_deref() == Some(expected),
                "原执行方案已变化，请重新读取并审阅",
            )?;
        }
        if let Some(approval) = self.approval {
            let revision = self.plan_revision.ok_or("批准记录缺少原执行方案")?;
            approval.validate(revision, "user", revision.policy.version)?;
        }
        if let Some(revision) = self.plan_revision {
            require(
                revision.project == *self.project
                    && self.state.plan.as_ref() == Some(&revision.workflow),
                "原执行方案与项目状态不符",
            )?;
        }
        crate::workspace_read::bounded(WorkspaceResult::ExecutionReview(Box::new(
            WorkspaceExecutionReview {
                project: self.project.clone(),
                revision: self.plan_revision.cloned(),
                revision_digest: original_digest,
                approval: self.approval.cloned(),
                revision_requested: self.state.revision_requested.clone(),
                sequence: self.sequence,
            },
        )))
    }
    fn version(&self) -> Result<String, String> {
        use sha2::Digest;
        let mut hash = QueryHash(sha2::Sha256::new());
        serde_json::to_writer(
            &mut hash,
            &(
                "workspace-project-tasks-v1",
                self.project,
                &self.revision_digest,
                self.sequence,
                self.state,
            ),
        )
        .map_err(|e| e.to_string())?;
        Ok(format!("{:x}", hash.0.finalize()))
    }
    fn tasks(&self) -> Result<Vec<(&WorkflowTask, &TaskRun)>, String> {
        let plan = self.state.plan.as_ref();
        require(
            plan.map_or(0, |p| p.tasks.len()) == self.state.tasks.len(),
            "原项目任务与方案不一致",
        )?;
        plan.into_iter()
            .flat_map(|p| &p.tasks)
            .map(|task| {
                self.state
                    .tasks
                    .get(&task.id)
                    .map(|run| (task, run))
                    .ok_or_else(|| "原任务状态不存在".into())
            })
            .collect()
    }
    fn query(&self, query: &WorkspaceQuery) -> Result<WorkspaceResult, String> {
        let version = self.version()?;
        let tasks = self.tasks()?;
        let result = match query {
            WorkspaceQuery::Tasks {
                offset, revision, ..
            } => {
                let total = tasks.len();
                let summaries = tasks.iter().map(|(t, r)| task_summary(t, r));
                let (entries, next_offset) = crate::workspace_read::page(
                    summaries,
                    total,
                    *offset,
                    revision.as_deref(),
                    &version,
                )?;
                WorkspaceResult::Tasks(WorkspaceTaskPage {
                    project: self.project.clone(),
                    revision: version,
                    revision_digest: self.revision_digest.clone(),
                    sequence: self.sequence,
                    approved: self.state.approved,
                    offset: *offset,
                    total: total as u32,
                    next_offset,
                    entries,
                })
            }
            WorkspaceQuery::Task {
                task_id, revision, ..
            }
            | WorkspaceQuery::TaskEvidence {
                task_id, revision, ..
            } => {
                cerulean_domain::execution::sha256(revision)?;
                require(revision == &version, "原项目任务状态已变化，请刷新任务首页")?;
                cerulean_domain::execution::identifier(task_id)?;
                let (task, run) = tasks
                    .into_iter()
                    .find(|(t, _)| &t.id == task_id)
                    .ok_or("原项目任务不存在")?;
                if let WorkspaceQuery::TaskEvidence { offset, .. } = query {
                    let (entries, next_offset) = crate::workspace_read::page(
                        run.evidence.iter(),
                        run.evidence.len(),
                        *offset,
                        Some(revision),
                        &version,
                    )?;
                    WorkspaceResult::TaskEvidence(WorkspaceTaskEvidencePage {
                        project_id: self.project.id.clone(),
                        task_id: task_id.clone(),
                        revision: version,
                        offset: *offset,
                        total: run.evidence.len() as u32,
                        next_offset,
                        entries: entries.into_iter().cloned().collect(),
                    })
                } else {
                    WorkspaceResult::Task(Box::new(WorkspaceTaskDetail {
                        project_id: self.project.id.clone(),
                        revision: version,
                        task: task.clone(),
                        run: task_summary(task, run),
                    }))
                }
            }
            _ => return Err("原项目任务查询类型不符".into()),
        };
        crate::workspace_read::bounded(result)
    }
}
fn task_summary(task: &WorkflowTask, run: &TaskRun) -> WorkspaceTaskSummary {
    WorkspaceTaskSummary {
        id: task.id.clone(),
        goal: task.goal.clone(),
        state: run.state,
        worker: run.worker.clone(),
        attempt: run.attempt.clone(),
        fence: run.fence,
        verified: run.verified,
        cancellation_requested: run.cancellation_requested,
        evidence_count: run.evidence.len() as u32,
    }
}

fn summary(
    project: &Project,
    revision_digest: Option<String>,
    sequence: u32,
    state: &cerulean_domain::workflow::WorkflowState,
) -> ProjectSummary {
    ProjectSummary {
        project: project.clone(),
        revision_digest,
        sequence,
        approved: state.approved,
        task_count: state.tasks.len() as u32,
        resource_holding_tasks: state
            .tasks
            .values()
            .filter(|t| t.state.holds_resources())
            .count() as u32,
        unknown_tasks: state
            .tasks
            .values()
            .filter(|t| t.state == cerulean_domain::workflow::TaskState::Unknown)
            .count() as u32,
    }
}
#[cfg(windows)]
fn execution_summary(s: &ExecutionSnapshot) -> ProjectSummary {
    summary(&s.project, s.revision_digest.clone(), s.sequence, &s.state)
}
impl RuntimeHost {
    fn workspace_project_view(&self, id: &str) -> Result<ProjectView<'_>, String> {
        cerulean_domain::execution::identifier(id)?;
        // Last live owner has the same precedence as workspace_projects. Shared
        // executor snapshots must not supersede their actual shared writer.
        #[cfg(windows)]
        {
            if let Some(writer) = self.api_writers.get(id) {
                require(
                    !writer.interrupted,
                    "原项目协调器需要重开核对，任务状态暂不可用",
                )?;
            }
            let current = self
                .active
                .values()
                .filter(|a| !a.shared)
                .map(|a| &a.snapshot)
                .chain(self.api_writers.values().map(|a| &a.snapshot))
                .chain(self.desktop_active.iter().map(|a| &a.snapshot))
                .rfind(|s| s.project.id == id);
            if let Some(s) = current {
                return Ok(ProjectView {
                    project: &s.project,
                    plan_revision: s.revision.as_ref(),
                    approval: s.approval.as_ref(),
                    revision_digest: s.revision_digest.clone(),
                    sequence: s.sequence,
                    state: &s.state,
                });
            }
        }
        let store = self
            .projects
            .get(id)
            .ok_or("原项目不存在，未创建替代记录")?;
        require(
            !store.requires_reopen(),
            "原项目写入尚未核对，请保留记录并重开",
        )?;
        let project = &store
            .configuration()
            .execution_scope
            .as_ref()
            .ok_or("原项目范围不存在")?
            .project;
        require(project.id == id, "原项目身份不一致")?;
        Ok(ProjectView {
            project,
            plan_revision: store.revision(),
            approval: store.approval(),
            revision_digest: store.revision().map(|r| r.approval_digest()).transpose()?,
            sequence: store.history().len() as u32,
            state: store.state(),
        })
    }
    fn workspace_projects(&self) -> Result<Vec<ProjectSummary>, String> {
        let mut entries = BTreeMap::new();
        for store in self.projects.values() {
            require(
                !store.requires_reopen(),
                "原项目写入尚未核对，请保留记录并重开",
            )?;
            let project = &store
                .configuration()
                .execution_scope
                .as_ref()
                .ok_or("原项目范围不存在")?
                .project;
            entries.insert(
                project.id.clone(),
                summary(
                    project,
                    store.revision().map(|r| r.approval_digest()).transpose()?,
                    store.history().len() as u32,
                    store.state(),
                ),
            );
        }
        // Same live owner precedence as the full snapshot, without copying its
        // revisions, tasks, evidence arrays or unrelated global collections.
        #[cfg(windows)]
        for s in self
            .active
            .values()
            .filter(|a| !a.shared)
            .map(|a| &a.snapshot)
            .chain(self.api_writers.values().map(|a| &a.snapshot))
            .chain(self.desktop_active.iter().map(|a| &a.snapshot))
        {
            entries.insert(s.project.id.clone(), execution_summary(s));
        }
        Ok(entries.into_values().collect())
    }
    fn workspace_configurations(&self, query: &WorkspaceQuery) -> Result<WorkspaceResult, String> {
        use sha2::Digest;
        require(!self.base.poisoned, "原写入尚未核对，请保留记录并重开")?;
        let mut hash = QueryHash(sha2::Sha256::new());
        serde_json::to_writer(&mut hash, &"workspace-configurations-v1")
            .map_err(|e| e.to_string())?;
        for (id, saved) in &self.project_setups {
            require(saved.setup.project_id == *id, "原配置项目身份不一致")?;
            let project = self.workspace_project_view(id)?;
            let mut setup_hash = QueryHash(sha2::Sha256::new());
            serde_json::to_writer(&mut setup_hash, &saved.setup).map_err(|e| e.to_string())?;
            require(
                format!("{:x}", setup_hash.0.finalize()) == saved.digest,
                "原项目配置摘要不一致",
            )?;
            serde_json::to_writer(&mut hash, &(id, saved, project.version()?))
                .map_err(|e| e.to_string())?;
        }
        let revision = format!("{:x}", hash.0.finalize());
        let result = match query {
            WorkspaceQuery::Configurations {
                offset,
                revision: expected,
            } => {
                let total = self.project_setups.len();
                let summaries =
                    self.project_setups
                        .iter()
                        .map(|(id, saved)| WorkspaceConfigurationSummary {
                            project_id: id.clone(),
                            digest: saved.digest.clone(),
                        });
                let (entries, next_offset) = crate::workspace_read::page(
                    summaries,
                    total,
                    *offset,
                    expected.as_deref(),
                    &revision,
                )?;
                WorkspaceResult::Configurations(WorkspaceConfigurationPage {
                    revision,
                    offset: *offset,
                    total: total as u32,
                    next_offset,
                    entries,
                })
            }
            WorkspaceQuery::Configuration {
                project_id,
                revision: expected,
            } => {
                cerulean_domain::execution::identifier(project_id)?;
                cerulean_domain::execution::sha256(expected)?;
                require(
                    expected == &revision,
                    "项目配置清单已变化，请刷新后读取原详情",
                )?;
                let saved = self
                    .project_setups
                    .get(project_id)
                    .ok_or("原项目配置不存在")?;
                WorkspaceResult::Configuration(Box::new(WorkspaceConfigurationDetail {
                    project_id: project_id.clone(),
                    revision,
                    saved: saved.clone(),
                }))
            }
            _ => return Err("配置查询类型不符".into()),
        };
        crate::workspace_read::bounded(result)
    }
    pub(crate) fn workspace_query(
        &self,
        query: &WorkspaceQuery,
    ) -> Result<WorkspaceResult, String> {
        let result = match query {
            WorkspaceQuery::VerificationRechecks { .. }
            | WorkspaceQuery::CurrentVerificationRecheck { .. }
            | WorkspaceQuery::VerificationRecheckInputs { .. }
            | WorkspaceQuery::RegisteredValidator { .. }
            | WorkspaceQuery::VerificationRecheck { .. }
            | WorkspaceQuery::VerificationRecheckDispositionReview { .. }
            | WorkspaceQuery::VerificationRecheckReview { .. } => {
                return self.workspace_verification_rechecks(query);
            }
            WorkspaceQuery::ManagementRecordReviews { .. }
            | WorkspaceQuery::ManagementRecordReview { .. } => {
                return self.workspace_management_record_reviews(query);
            }
            WorkspaceQuery::ApiPublicationReviews { .. }
            | WorkspaceQuery::ApiPublicationReview { .. } => {
                return self.workspace_api_publication_reviews(query);
            }
            WorkspaceQuery::ManagementGatewayEvidence {
                project_id,
                operation_id,
                revision,
            } => {
                return self.workspace_management_gateway_evidence(
                    project_id,
                    operation_id,
                    revision,
                );
            }
            WorkspaceQuery::ManagementDecisions { .. }
            | WorkspaceQuery::ManagementDecision { .. } => {
                return self.workspace_management_decisions(query);
            }
            WorkspaceQuery::History { project_id, .. }
            | WorkspaceQuery::HistoryEntry { project_id, .. } => {
                require(!self.base.poisoned, "原历史写入尚未核对，请保留记录并重开")?;
                if let Some(id) = project_id {
                    cerulean_domain::execution::identifier(id)?;
                    #[cfg(windows)]
                    {
                        require(self.api_fault.is_none(), "原历史协调器需要重开核对")?;
                        if let Some(writer) = self.api_writers.get(id) {
                            require(!writer.interrupted, "原历史协调器需要重开核对")?;
                            return writer.owner.workspace_history(query.clone());
                        }
                        require(
                            !self.active.values().any(|a| a.snapshot.project.id == *id)
                                && !self
                                    .desktop_active
                                    .iter()
                                    .any(|a| a.snapshot.project.id == *id),
                            "原历史由活动写者持有，暂不可用；请保留原记录",
                        )?;
                    }
                    let store = self
                        .projects
                        .get(id)
                        .ok_or("原项目不存在，未创建替代记录")?;
                    return crate::workspace_history::query(store, query);
                }
                return self.base.workspace_history(query);
            }
            WorkspaceQuery::Configurations { .. } | WorkspaceQuery::Configuration { .. } => {
                return self.workspace_configurations(query)
            }
            WorkspaceQuery::Recovery { project_id, .. }
            | WorkspaceQuery::RecoveryDetail { project_id, .. } => {
                let project = self.workspace_project_view(project_id)?;
                return self.workspace_recovery(query, &project.version()?);
            }
            WorkspaceQuery::ProjectRuns { .. } | WorkspaceQuery::ProjectRun { .. } => {
                return self.workspace_project_runs(query);
            }
            WorkspaceQuery::RevisionReviews { .. } | WorkspaceQuery::RevisionReview { .. } => {
                return self.workspace_revision_reviews(query);
            }
            WorkspaceQuery::UnsentAssignments { .. } | WorkspaceQuery::UnsentAssignment { .. } => {
                return self.workspace_unsent_assignments(query);
            }
            WorkspaceQuery::CodexCancellations { .. }
            | WorkspaceQuery::CodexCancellation { .. } => {
                return self.workspace_codex_cancellation(query);
            }
            WorkspaceQuery::CodexStartups { .. } | WorkspaceQuery::CodexStartup { .. } => {
                return self.workspace_startup(query);
            }
            WorkspaceQuery::Jobs { project_id, .. } | WorkspaceQuery::Job { project_id, .. } => {
                let project = self.workspace_project_view(project_id)?;
                return self.workspace_jobs(query, &project.version()?);
            }
            WorkspaceQuery::VerificationResult { .. }
            | WorkspaceQuery::VerificationResultArtifacts { .. }
            | WorkspaceQuery::VerificationResultStages { .. } => {
                return self.workspace_verification(query);
            }
            WorkspaceQuery::WorkerObservation { .. }
            | WorkspaceQuery::WorkerObservationEvents { .. }
            | WorkspaceQuery::WorkerObservationIncidents { .. } => {
                return self.workspace_observation(query);
            }
            WorkspaceQuery::ExecutionReview { project_id, digest } => {
                return self
                    .workspace_project_view(project_id)?
                    .execution_review(digest.as_deref());
            }
            WorkspaceQuery::Tasks { project_id, .. }
            | WorkspaceQuery::Task { project_id, .. }
            | WorkspaceQuery::TaskEvidence { project_id, .. } => {
                return self.workspace_project_view(project_id)?.query(query);
            }
            WorkspaceQuery::Overview => {
                let projects = self.workspace_projects();
                let needs_review = !self.omp_stop_issues.is_empty()
                    || self.base.poisoned
                    || !self.management.healthy()
                    || self.runner_error.is_some()
                    || projects.is_err();
                #[cfg(windows)]
                let needs_review = needs_review || self.api_fault.is_some();
                let degraded = self
                    .projects
                    .values()
                    .filter(|p| p.derived_diagnostics().error().is_some())
                    .count();
                #[cfg(windows)]
                let degraded = degraded
                    + self
                        .api_writers
                        .values()
                        .filter(|p| p.owner.derived_diagnostics.error().is_some())
                        .count()
                    + self
                        .active
                        .values()
                        .filter(|p| {
                            p.derived_diagnostics
                                .as_ref()
                                .is_some_and(|d| d.error().is_some())
                        })
                        .count();
                let mut message: Option<String> =
                    needs_review.then(|| "原停止或写入记录需要核对；请查看对应恢复记录".into());
                if degraded > 0 {
                    let warning = format!("{degraded} 个项目的日报、查询索引或恢复缓存更新失败，界面仍使用事件记录；已提交任务不会自动重发。请检查后台负载、磁盘空间和文件占用，恢复后重开项目重建派生文件。");
                    message = Some(
                        message.map_or_else(|| warning.clone(), |m| format!("{m}；{warning}")),
                    );
                }
                WorkspaceResult::Overview(self.base.workspace_overview(
                    if needs_review {
                        WorkspaceMode::ReviewRequired
                    } else {
                        WorkspaceMode::Ready
                    },
                    message,
                    projects.ok().map(|p| p.len() as u32),
                ))
            }
            WorkspaceQuery::Plans { .. } | WorkspaceQuery::Plan { .. } => {
                return self.base.workspace_plans(query)
            }
            WorkspaceQuery::Projects {
                offset,
                revision: expected,
            } => {
                let entries = self.workspace_projects()?;
                let revision = digest(&("workspace-projects-v1", &entries))?;
                let total = entries.len();
                let (entries, next_offset) = crate::workspace_read::page(
                    entries,
                    total,
                    *offset,
                    expected.as_deref(),
                    &revision,
                )?;
                WorkspaceResult::Projects(ProjectPage {
                    revision,
                    offset: *offset,
                    total: total as u32,
                    next_offset,
                    entries,
                })
            }
        };
        crate::workspace_read::bounded(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    fn settle_derived_views(host: &RuntimeHost) {
        // The readonly assertion starts after setup's independent publications.
        // Keep all files in the snapshot, including derived documents and SQLite.
        assert!(host.api_writers.is_empty() && host.active.is_empty());
        host.base
            .wait_for_index(std::time::Duration::from_secs(20))
            .unwrap();
        for store in host.projects.values() {
            store
                .wait_for_projections(std::time::Duration::from_secs(20))
                .unwrap();
            store
                .wait_for_index(std::time::Duration::from_secs(20))
                .unwrap();
        }
    }

    #[cfg(windows)]
    #[test]
    fn projection_failure_is_visible_with_direct_and_coordinator_ownership() {
        use std::time::Duration;
        let temp = tempfile::tempdir().unwrap();
        let (mut host, draft) = project_planning::tests::fixture(temp.path());
        host.configure_project(draft).unwrap();
        project_planning::tests::complete(&mut host, project_planning::tests::proposal("tests"));
        let store = host.projects.remove("project").unwrap();
        let config = store.configuration().clone();
        let path = host.root.join("workflows/project");
        drop(store);
        std::fs::remove_file(path.join("state.json")).unwrap();
        std::fs::create_dir(path.join("state.json")).unwrap();
        let store = crate::scheduler::DurableWorkflow::open(&path, config.clone()).unwrap();
        assert!(store.wait_for_projections(Duration::from_secs(20)).is_err());
        store.wait_for_index(Duration::from_secs(20)).unwrap();
        host.projects.insert("project".into(), store);
        let overview = host.workspace_query(&WorkspaceQuery::Overview).unwrap();
        let WorkspaceResult::Overview(value) = overview else {
            panic!("overview")
        };
        assert!(value
            .message
            .unwrap()
            .contains("日报、查询索引或恢复缓存更新失败"));
        let store = host.projects.remove("project").unwrap();
        let writer = crate::async_api::ApiWorkflow::start(store, path.clone()).unwrap();
        let before = serde_json::to_value(&writer.snapshot).unwrap();
        host.api_writers.insert("project".into(), writer);
        let WorkspaceResult::Overview(value) =
            host.workspace_query(&WorkspaceQuery::Overview).unwrap()
        else {
            panic!("overview")
        };
        assert!(value
            .message
            .unwrap()
            .contains("日报、查询索引或恢复缓存更新失败"));
        let mut writer = host.api_writers.remove("project").unwrap();
        writer.refresh().unwrap();
        assert_eq!(serde_json::to_value(&writer.snapshot).unwrap(), before);
        drop(writer.owner.finish(false).unwrap());
        std::fs::remove_dir(path.join("state.json")).unwrap();
        let store = crate::scheduler::DurableWorkflow::open(&path, config).unwrap();
        store.wait_for_projections(Duration::from_secs(20)).unwrap();
        host.projects.insert("project".into(), store);
        let WorkspaceResult::Overview(value) =
            host.workspace_query(&WorkspaceQuery::Overview).unwrap()
        else {
            panic!("overview")
        };
        assert!(!value
            .message
            .unwrap_or_default()
            .contains("日报、查询索引或恢复缓存更新失败"));
    }
    use cerulean_domain::workflow::{TaskRun, TaskState, WorkflowState};
    #[cfg(windows)]
    #[test]
    fn workspace_history_project_original_owner_handoff_and_refusal() {
        let temp = tempfile::tempdir().unwrap();
        let (mut host, draft) = project_planning::tests::fixture(temp.path());
        host.configure_project(draft).unwrap();
        project_planning::tests::complete(&mut host, project_planning::tests::proposal("tests"));
        settle_derived_views(&host);
        let before = crate::reader_generation::tests::snapshot(&host.root);
        let query = WorkspaceQuery::History {
            project_id: Some("project".into()),
            offset: 0,
            revision: None,
        };
        let original = host.workspace_query(&query).unwrap();
        let WorkspaceResult::History(page) = &original else {
            panic!("history")
        };
        assert!(!page.entries.is_empty());
        let detail_query = WorkspaceQuery::HistoryEntry {
            project_id: Some("project".into()),
            sequence: page.entries[0].sequence,
            revision: page.revision.clone(),
        };
        let detail = host.workspace_query(&detail_query).unwrap();
        let WorkspaceResult::HistoryEntry(entry) = &detail else {
            panic!("detail")
        };
        assert_eq!(
            entry.original_json,
            serde_json::to_string(&host.projects["project"].history()[0]).unwrap()
        );
        let store = host.projects.remove("project").unwrap();
        let writer =
            crate::async_api::ApiWorkflow::start(store, host.root.join("workflows/project"))
                .unwrap();
        host.api_writers.insert("project".into(), writer);
        assert_eq!(
            serde_json::to_value(host.workspace_query(&query).unwrap()).unwrap(),
            serde_json::to_value(&original).unwrap()
        );
        assert_eq!(
            serde_json::to_value(host.workspace_query(&detail_query).unwrap()).unwrap(),
            serde_json::to_value(&detail).unwrap()
        );
        host.api_writers.get_mut("project").unwrap().interrupted = true;
        assert!(host.workspace_query(&query).is_err());
        let mut writer = host.api_writers.remove("project").unwrap();
        let store = writer.owner.finish(false).unwrap();
        host.projects.insert("project".into(), store);
        assert_eq!(
            serde_json::to_value(host.workspace_query(&detail_query).unwrap()).unwrap(),
            serde_json::to_value(detail).unwrap()
        );
        assert!(host
            .workspace_query(&WorkspaceQuery::History {
                project_id: Some("missing".into()),
                offset: 0,
                revision: None
            })
            .is_err());
        assert_eq!(
            before,
            crate::reader_generation::tests::snapshot(&host.root)
        );
    }

    #[cfg(windows)]
    #[test]
    fn workspace_configuration_boundary_poisoned_original_project_writer() {
        let temp = tempfile::tempdir().unwrap();
        let (mut host, draft) = project_planning::tests::fixture(temp.path());
        host.configure_project(draft).unwrap();
        project_planning::tests::complete(&mut host, project_planning::tests::proposal("tests"));
        let approval_digest = host.projects["project"]
            .revision()
            .unwrap()
            .approval_digest()
            .unwrap();
        host.approve("project", &approval_digest).unwrap();
        let page = configuration_page(&host, 0, None);
        settle_derived_views(&host);
        let before = crate::reader_generation::tests::snapshot(&host.root);
        let history = host.projects["project"].history().to_vec();
        let state = host.projects["project"].state().clone();
        let workflow_digest = state.digest.clone().unwrap();
        // Interrupt the existing real writer only after all normal guards pass,
        // before any event bytes are written. No guard or writer is replaced.
        let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            host.projects.get_mut("project").unwrap().command_observed(
                "configuration-boundary-interruption",
                "user",
                WorkflowCommand::RequestRevision {
                    digest: workflow_digest,
                    reason: "controlled configuration read-boundary interruption".into(),
                },
                CommandContext {
                    approval_source: Some("authenticated local-user revision request".into()),
                    ..Default::default()
                },
                true,
                |point| {
                    if point == crate::scheduler::CommitPoint::BeforeEventWrite {
                        panic!("controlled interruption before event write");
                    }
                },
            )
        }));
        assert!(interrupted.is_err());
        assert!(host.projects["project"].requires_reopen());
        for query in [
            WorkspaceQuery::Configurations {
                offset: 0,
                revision: None,
            },
            WorkspaceQuery::Configuration {
                project_id: "project".into(),
                revision: page.revision,
            },
        ] {
            assert_eq!(
                host.workspace_query(&query).unwrap_err(),
                "原项目写入尚未核对，请保留记录并重开"
            );
        }
        assert_eq!(host.projects["project"].history(), history);
        assert_eq!(host.projects["project"].state(), &state);
        assert_eq!(
            before,
            crate::reader_generation::tests::snapshot(&host.root)
        );
    }

    #[cfg(windows)]
    fn configuration_page(
        host: &RuntimeHost,
        offset: u32,
        revision: Option<String>,
    ) -> WorkspaceConfigurationPage {
        let WorkspaceResult::Configurations(page) = host
            .workspace_query(&WorkspaceQuery::Configurations { offset, revision })
            .unwrap()
        else {
            panic!("configuration page")
        };
        page
    }

    #[cfg(windows)]
    #[test]
    fn workspace_configuration_boundary_pages_complete_setups_and_streamed_binding() {
        use sha2::Digest;
        let temp = tempfile::tempdir().unwrap();
        let (mut host, mut draft) = project_planning::tests::fixture(temp.path());
        // Valid individual drafts, but a controlled aggregate larger than one result.
        draft.requirement = "\u{0001}".repeat(7999);
        host.configure_project(draft.clone()).unwrap();
        for i in 0..40 {
            let id = format!("project-{i:02}");
            let root = temp.path().join(&id);
            fs::create_dir(&root).unwrap();
            host.register(Project {
                id: id.clone(),
                root: root.to_string_lossy().into(),
                timezone: "Asia/Shanghai".into(),
            })
            .unwrap();
            draft.project_id = id;
            host.configure_project(draft.clone()).unwrap();
        }
        let original = serde_json::to_vec(&host.project_setups).unwrap();
        assert!(original.len() > crate::workspace_read::RESULT_BYTES);
        settle_derived_views(&host);
        let before = crate::reader_generation::tests::snapshot(&host.root);
        let mut encoded_binding = serde_json::to_vec("workspace-configurations-v1").unwrap();
        for (id, saved) in &host.project_setups {
            assert_eq!(saved.digest, digest(&saved.setup).unwrap());
            encoded_binding.extend(
                serde_json::to_vec(&(
                    id,
                    saved,
                    host.workspace_project_view(id).unwrap().version().unwrap(),
                ))
                .unwrap(),
            );
        }
        let expected_revision = format!("{:x}", sha2::Sha256::digest(&encoded_binding));
        let mut ids = Vec::new();
        for (offset, count, next) in [(0, 20, Some(20)), (20, 20, Some(40)), (40, 1, None)] {
            let page = configuration_page(
                &host,
                offset,
                (offset != 0).then(|| expected_revision.clone()),
            );
            assert_eq!(page.revision, expected_revision);
            assert_eq!(page.offset, offset);
            assert_eq!(page.total, 41);
            assert_eq!(page.entries.len(), count);
            assert_eq!(page.next_offset, next);
            for entry in page.entries {
                let result = host
                    .workspace_query(&WorkspaceQuery::Configuration {
                        project_id: entry.project_id.clone(),
                        revision: page.revision.clone(),
                    })
                    .unwrap();
                assert!(
                    serde_json::to_vec(&result).unwrap().len()
                        <= crate::workspace_read::RESULT_BYTES
                );
                let WorkspaceResult::Configuration(detail) = result else {
                    panic!("configuration detail")
                };
                assert_eq!(detail.project_id, entry.project_id);
                assert_eq!(detail.revision, expected_revision);
                assert_eq!(detail.saved.digest, entry.digest);
                assert_eq!(
                    serde_json::to_value(&detail.saved).unwrap(),
                    serde_json::to_value(&host.project_setups[&entry.project_id]).unwrap()
                );
                ids.push(entry.project_id);
            }
        }
        assert_eq!(ids, host.project_setups.keys().cloned().collect::<Vec<_>>());
        assert_eq!(serde_json::to_vec(&host.project_setups).unwrap(), original);
        assert_eq!(
            before,
            crate::reader_generation::tests::snapshot(&host.root)
        );
    }

    #[cfg(windows)]
    #[test]
    fn workspace_configuration_boundary_preserves_large_encoded_document() {
        let temp = tempfile::tempdir().unwrap();
        let (mut host, draft) = project_planning::tests::fixture(temp.path());
        host.configure_project(draft).unwrap();
        settle_derived_views(&host);
        let before = crate::reader_generation::tests::snapshot(&host.root);
        // Synthetic JSON-escaping probe: document details can exceed the ordinary
        // evidence limit in 0.1.38. This does not exercise durable-event storage;
        // document_stream tests cover real service persistence and transport.
        let saved = host.project_setups.get_mut("project").unwrap();
        saved.setup.requirement = "\u{0001}".repeat(crate::workspace_read::RESULT_BYTES / 2);
        saved.digest = digest(&saved.setup).unwrap();
        let original = serde_json::to_vec(&host.project_setups).unwrap();
        let page = configuration_page(&host, 0, None);
        let candidate = WorkspaceResult::Configuration(Box::new(WorkspaceConfigurationDetail {
            project_id: "project".into(),
            revision: page.revision.clone(),
            saved: host.project_setups["project"].clone(),
        }));
        assert!(
            host.project_setups["project"].setup.requirement.len()
                < crate::workspace_read::RESULT_BYTES
        );
        assert!(
            serde_json::to_vec(&candidate).unwrap().len() > crate::workspace_read::RESULT_BYTES
        );
        let result = host
            .workspace_query(&WorkspaceQuery::Configuration {
                project_id: "project".into(),
                revision: page.revision,
            })
            .unwrap();
        assert!(
            serde_json::to_vec(&result).unwrap() == serde_json::to_vec(&candidate).unwrap(),
            "large document detail must remain complete and unchanged"
        );
        assert_eq!(serde_json::to_vec(&host.project_setups).unwrap(), original);
        assert_eq!(
            before,
            crate::reader_generation::tests::snapshot(&host.root)
        );
    }

    #[cfg(windows)]
    #[test]
    fn workspace_configuration_boundary_rejects_missing_identity_digest_and_poison() {
        let temp = tempfile::tempdir().unwrap();
        let (mut host, draft) = project_planning::tests::fixture(temp.path());
        host.configure_project(draft).unwrap();
        settle_derived_views(&host);
        let before = crate::reader_generation::tests::snapshot(&host.root);
        let original = host.project_setups["project"].clone();
        let page = configuration_page(&host, 0, None);
        let list = WorkspaceQuery::Configurations {
            offset: 0,
            revision: None,
        };
        let detail = |id: &str, revision: &str| WorkspaceQuery::Configuration {
            project_id: id.into(),
            revision: revision.into(),
        };
        assert_eq!(
            host.workspace_query(&detail("missing", &page.revision))
                .unwrap_err(),
            "原项目配置不存在"
        );
        assert!(host
            .workspace_query(&detail("project", &"0".repeat(64)))
            .unwrap_err()
            .contains("已变化"));
        for query in [
            WorkspaceQuery::Configurations {
                offset: 20,
                revision: None,
            },
            WorkspaceQuery::Configurations {
                offset: 2,
                revision: Some(page.revision.clone()),
            },
            detail("project", "not-a-digest"),
        ] {
            assert!(host.workspace_query(&query).is_err());
        }
        host.base.poisoned = true;
        for query in [&list, &detail("project", &page.revision)] {
            assert_eq!(
                host.workspace_query(query).unwrap_err(),
                "原写入尚未核对，请保留记录并重开"
            );
        }
        host.base.poisoned = false;
        host.project_setups
            .get_mut("project")
            .unwrap()
            .setup
            .project_id = "different".into();
        assert_eq!(
            host.workspace_query(&list).unwrap_err(),
            "原配置项目身份不一致"
        );
        host.project_setups
            .insert("project".into(), original.clone());
        host.project_setups.get_mut("project").unwrap().digest = "0".repeat(64);
        assert_eq!(
            host.workspace_query(&list).unwrap_err(),
            "原项目配置摘要不一致"
        );
        host.project_setups
            .insert("project".into(), original.clone());
        let store = host.projects.remove("project").unwrap();
        assert_eq!(
            host.workspace_query(&list).unwrap_err(),
            "原项目不存在，未创建替代记录"
        );
        host.projects.insert("different".into(), store);
        let saved = host.project_setups.remove("project").unwrap();
        let mut different = saved.clone();
        different.setup.project_id = "different".into();
        different.digest = digest(&different.setup).unwrap();
        host.project_setups.insert("different".into(), different);
        assert_eq!(host.workspace_query(&list).unwrap_err(), "原项目身份不一致");
        let store = host.projects.remove("different").unwrap();
        host.projects.insert("project".into(), store);
        host.project_setups.remove("different");
        host.project_setups.insert("project".into(), saved);
        assert_eq!(configuration_page(&host, 0, None).revision, page.revision);
        assert_eq!(
            before,
            crate::reader_generation::tests::snapshot(&host.root)
        );
    }

    #[cfg(windows)]
    #[test]
    fn workspace_configuration_boundary_real_shared_writer_handoff_and_same_sequence_changes() {
        let temp = tempfile::tempdir().unwrap();
        let (mut host, draft) = project_planning::tests::fixture(temp.path());
        host.configure_project(draft).unwrap();
        project_planning::tests::complete(&mut host, project_planning::tests::proposal("tests"));
        settle_derived_views(&host);
        let before = crate::reader_generation::tests::snapshot(&host.root);
        let page = configuration_page(&host, 0, None);
        let query = WorkspaceQuery::Configuration {
            project_id: "project".into(),
            revision: page.revision.clone(),
        };
        let original = serde_json::to_value(host.workspace_query(&query).unwrap()).unwrap();
        let job = VerificationJob {
            project_id: "project".into(),
            operation_id: "shared-check".into(),
            task_id: "historical-task".into(),
            attempt_id: "historical-attempt".into(),
            state: VerificationState::Unknown,
            started_at: "2026-10-14T00:00:00Z".into(),
            updated_at: "2026-10-14T00:00:00Z".into(),
            message: Some("original shared writer job".into()),
            reconciliation_digest: None,
        };
        host.jobs
            .insert(("project".into(), "shared-check".into()), job);
        let before_jobs = match host
            .workspace_query(&WorkspaceQuery::Jobs {
                project_id: "project".into(),
                offset: 0,
                revision: None,
            })
            .unwrap()
        {
            WorkspaceResult::Jobs(page) => page,
            _ => panic!("jobs"),
        };
        let job_query = |revision| WorkspaceQuery::Job {
            project_id: "project".into(),
            kind: WorkspaceJobKind::Verification,
            id: "shared-check".into(),
            revision,
        };
        let original_job = serde_json::to_value(
            host.workspace_query(&job_query(before_jobs.revision.clone()))
                .unwrap(),
        )
        .unwrap();
        let store = host.projects.remove("project").unwrap();
        let writer =
            crate::async_api::ApiWorkflow::start(store, host.root.join("workflows/project"))
                .unwrap();
        host.api_writers.insert("project".into(), writer);
        assert_eq!(
            serde_json::to_value(
                host.workspace_query(&job_query(before_jobs.revision.clone()))
                    .unwrap()
            )
            .unwrap(),
            original_job
        );
        assert_eq!(configuration_page(&host, 0, None).revision, page.revision);
        assert_eq!(
            serde_json::to_value(host.workspace_query(&query).unwrap()).unwrap(),
            original
        );
        let state = host.api_writers["project"].snapshot.state.clone();
        let sequence = host.api_writers["project"].snapshot.sequence;
        let task_id = state.tasks.keys().next().unwrap().clone();
        for evidence_change in [false, true] {
            let snapshot = &mut host.api_writers.get_mut("project").unwrap().snapshot;
            snapshot.state = state.clone();
            let run = snapshot.state.tasks.get_mut(&task_id).unwrap();
            if evidence_change {
                run.evidence
                    .push("controlled original evidence observation".into());
            } else {
                run.state = TaskState::Unknown;
            }
            assert_eq!(snapshot.sequence, sequence);
            assert!(host
                .workspace_query(&job_query(before_jobs.revision.clone()))
                .is_err());
            assert!(host
                .workspace_query(&WorkspaceQuery::Jobs {
                    project_id: "project".into(),
                    offset: 0,
                    revision: Some(before_jobs.revision.clone()),
                })
                .is_err());
            assert_ne!(configuration_page(&host, 0, None).revision, page.revision);
            assert!(host.workspace_query(&query).unwrap_err().contains("已变化"));
            assert!(host
                .workspace_query(&WorkspaceQuery::Configurations {
                    offset: 0,
                    revision: Some(page.revision.clone())
                })
                .unwrap_err()
                .contains("已变化"));
        }
        host.api_writers.get_mut("project").unwrap().snapshot.state = state;
        host.api_writers.get_mut("project").unwrap().interrupted = true;
        assert!(host
            .workspace_query(&job_query(before_jobs.revision.clone()))
            .unwrap_err()
            .contains("协调器"));
        assert!(host
            .workspace_query(&WorkspaceQuery::Jobs {
                project_id: "project".into(),
                offset: 0,
                revision: None,
            })
            .unwrap_err()
            .contains("协调器"));
        for query in [
            &query,
            &WorkspaceQuery::Configurations {
                offset: 0,
                revision: None,
            },
        ] {
            assert!(host.workspace_query(query).unwrap_err().contains("协调器"));
        }
        let mut writer = host.api_writers.remove("project").unwrap();
        let store = writer.owner.finish(false).unwrap();
        host.projects.insert("project".into(), store);
        assert_eq!(
            serde_json::to_value(
                host.workspace_query(&job_query(before_jobs.revision.clone()))
                    .unwrap()
            )
            .unwrap(),
            original_job
        );
        assert_eq!(configuration_page(&host, 0, None).revision, page.revision);
        assert_eq!(
            serde_json::to_value(host.workspace_query(&query).unwrap()).unwrap(),
            original
        );
        assert_eq!(
            before,
            crate::reader_generation::tests::snapshot(&host.root)
        );
    }

    #[test]
    fn workspace_execution_review_returns_whole_original_plan_and_exact_approval() {
        let temp = tempfile::tempdir().unwrap();
        let (mut host, draft) = project_planning::tests::fixture(temp.path());
        host.configure_project(draft).unwrap();
        let query = |digest| WorkspaceQuery::ExecutionReview {
            project_id: "project".into(),
            digest,
        };
        let WorkspaceResult::ExecutionReview(empty) = host.workspace_query(&query(None)).unwrap()
        else {
            panic!("review")
        };
        assert!(
            empty.revision.is_none() && empty.revision_digest.is_none() && empty.approval.is_none()
        );
        project_planning::tests::complete(&mut host, project_planning::tests::proposal("tests"));
        let original = host.projects["project"].revision().unwrap().clone();
        let digest = original.approval_digest().unwrap();
        settle_derived_views(&host);
        let before = crate::reader_generation::tests::snapshot(&host.root);
        let WorkspaceResult::ExecutionReview(review) =
            host.workspace_query(&query(Some(digest.clone()))).unwrap()
        else {
            panic!("review")
        };
        assert_eq!(review.revision.as_ref(), Some(&original));
        assert_eq!(review.revision_digest.as_ref(), Some(&digest));
        assert!(review.approval.is_none());
        assert!(host.workspace_query(&query(Some("0".repeat(64)))).is_err());
        assert!(before == crate::reader_generation::tests::snapshot(&host.root));
        host.approve("project", &digest).unwrap();
        settle_derived_views(&host);
        let after = crate::reader_generation::tests::snapshot(&host.root);
        let result = host.workspace_query(&query(Some(digest.clone()))).unwrap();
        assert!(serde_json::to_vec(&result).unwrap().len() < crate::workspace_read::RESULT_BYTES);
        let WorkspaceResult::ExecutionReview(approved) = result else {
            panic!("review")
        };
        assert_eq!(approved.revision.as_ref(), Some(&original));
        assert_eq!(
            approved.approval.as_ref(),
            host.projects["project"].approval()
        );
        assert_eq!(approved.approval.as_ref().unwrap().revision_digest, digest);
        let mut forged = approved.approval.unwrap();
        forged.user_id = "chief".into();
        let mut view = host.workspace_project_view("project").unwrap();
        view.approval = Some(&forged);
        assert!(view.execution_review(Some(&digest)).is_err());
        assert!(after == crate::reader_generation::tests::snapshot(&host.root));
    }
    #[cfg(windows)]
    #[test]
    fn workspace_tasks_follow_real_shared_writer_and_reject_interrupted_owner() {
        let temp = tempfile::tempdir().unwrap();
        let (mut host, draft) = project_planning::tests::fixture(temp.path());
        host.configure_project(draft).unwrap();
        project_planning::tests::complete(&mut host, project_planning::tests::proposal("tests"));
        let query = WorkspaceQuery::Tasks {
            project_id: "project".into(),
            offset: 0,
            revision: None,
        };
        settle_derived_views(&host);
        let before = crate::reader_generation::tests::snapshot(&host.root);
        let original = serde_json::to_value(host.workspace_query(&query).unwrap()).unwrap();
        let review_query = WorkspaceQuery::ExecutionReview {
            project_id: "project".into(),
            digest: None,
        };
        let original_review =
            serde_json::to_value(host.workspace_query(&review_query).unwrap()).unwrap();
        let store = host.projects.remove("project").unwrap();
        let writer =
            crate::async_api::ApiWorkflow::start(store, host.root.join("workflows/project"))
                .unwrap();
        host.api_writers.insert("project".into(), writer);
        assert!(!host.projects.contains_key("project"));
        assert_eq!(
            serde_json::to_value(host.workspace_query(&review_query).unwrap()).unwrap(),
            original_review
        );
        assert_eq!(
            serde_json::to_value(host.workspace_query(&query).unwrap()).unwrap(),
            original
        );
        host.api_writers.get_mut("project").unwrap().interrupted = true;
        assert!(host.workspace_query(&query).unwrap_err().contains("协调器"));
        let mut writer = host.api_writers.remove("project").unwrap();
        let store = writer.owner.finish(false).unwrap();
        host.projects.insert("project".into(), store);
        assert_eq!(
            serde_json::to_value(host.workspace_query(&query).unwrap()).unwrap(),
            original
        );
        assert!(host
            .workspace_query(&WorkspaceQuery::Tasks {
                project_id: "missing".into(),
                offset: 0,
                revision: None
            })
            .is_err());
        assert!(before == crate::reader_generation::tests::snapshot(&host.root));
    }
    fn task_state() -> WorkflowState {
        let mut state = WorkflowState::default();
        let mut tasks = vec![];
        for i in 0..41 {
            let id = format!("task-{i:02}");
            tasks.push(WorkflowTask {
                id: id.clone(),
                dependencies: vec![],
                workspace_key: "project".into(),
                goal: format!("original {i} {}", "\u{0001}".repeat(1000)),
                resource_keys: Some(vec!["shared".into()]),
            });
            state.tasks.insert(
                id,
                TaskRun {
                    state: TaskState::Unknown,
                    worker: Some("worker".into()),
                    attempt: Some(format!("attempt-{i}")),
                    fence: 7,
                    evidence: (0..43)
                        .map(|e| format!("original/{i}/{e}/{}", "e".repeat(3000)))
                        .collect(),
                    verified: false,
                    cancellation_requested: Some(true),
                },
            );
        }
        state.plan = Some(cerulean_domain::workflow::WorkflowPlan {
            id: "original".into(),
            version: 1,
            tasks,
        });
        state
    }
    #[test]
    fn workspace_tasks_pages_detail_and_evidence_preserve_complete_original_state() {
        let state = task_state();
        let project = Project {
            id: "project".into(),
            root: "D:/original".into(),
            timezone: "Asia/Shanghai".into(),
        };
        let view = ProjectView {
            project: &project,
            plan_revision: None,
            approval: None,
            revision_digest: Some("a".repeat(64)),
            sequence: 91,
            state: &state,
        };
        let original = serde_json::to_vec(&state).unwrap();
        assert!(original.len() > 4 * 1024 * 1024);
        let mut offset = 0;
        let mut revision = None;
        let mut ids = vec![];
        loop {
            let reply = view
                .query(&WorkspaceQuery::Tasks {
                    project_id: project.id.clone(),
                    offset,
                    revision: revision.clone(),
                })
                .unwrap();
            assert!(serde_json::to_vec(&reply).unwrap().len() < 65 * 1024);
            let WorkspaceResult::Tasks(page) = reply else {
                panic!("tasks")
            };
            assert_eq!(page.total, 41);
            assert!(page.entries.len() < 20);
            for task in &page.entries {
                assert_eq!(task.evidence_count, 43);
                assert_eq!(task.state, TaskState::Unknown);
                assert_eq!(task.fence, 7);
            }
            ids.extend(page.entries.into_iter().map(|t| t.id));
            revision = Some(page.revision);
            if let Some(next) = page.next_offset {
                offset = next;
            } else {
                break;
            }
        }
        assert_eq!(
            ids,
            state
                .plan
                .as_ref()
                .unwrap()
                .tasks
                .iter()
                .map(|t| t.id.clone())
                .collect::<Vec<_>>()
        );
        let revision = revision.unwrap();
        let task_id = "task-17".to_string();
        let WorkspaceResult::Task(detail) = view
            .query(&WorkspaceQuery::Task {
                project_id: project.id.clone(),
                task_id: task_id.clone(),
                revision: revision.clone(),
            })
            .unwrap()
        else {
            panic!("detail")
        };
        assert_eq!(detail.task, state.plan.as_ref().unwrap().tasks[17]);
        assert_eq!(detail.run.attempt.as_deref(), Some("attempt-17"));
        let mut offset = 0;
        let mut evidence = vec![];
        loop {
            let reply = view
                .query(&WorkspaceQuery::TaskEvidence {
                    project_id: project.id.clone(),
                    task_id: task_id.clone(),
                    offset,
                    revision: revision.clone(),
                })
                .unwrap();
            assert!(serde_json::to_vec(&reply).unwrap().len() < 65 * 1024);
            let WorkspaceResult::TaskEvidence(page) = reply else {
                panic!("evidence")
            };
            assert_eq!(page.total, 43);
            evidence.extend(page.entries);
            if let Some(next) = page.next_offset {
                offset = next;
            } else {
                break;
            }
        }
        assert_eq!(evidence, state.tasks[&task_id].evidence);
        assert_eq!(serde_json::to_vec(&state).unwrap(), original);
        assert_eq!(
            view.version().unwrap(),
            digest(&(
                "workspace-project-tasks-v1",
                &project,
                &view.revision_digest,
                91u32,
                &state
            ))
            .unwrap()
        );
    }
    #[test]
    fn workspace_tasks_reject_stale_state_cursor_and_missing_originals() {
        let mut state = task_state();
        let project = Project {
            id: "project".into(),
            root: "original".into(),
            timezone: "Asia/Shanghai".into(),
        };
        fn make<'a>(project: &'a Project, state: &'a WorkflowState) -> ProjectView<'a> {
            ProjectView {
                project,
                plan_revision: None,
                approval: None,
                revision_digest: None,
                sequence: 2,
                state,
            }
        }
        let revision = make(&project, &state).version().unwrap();
        for query in [
            WorkspaceQuery::Tasks {
                project_id: project.id.clone(),
                offset: 1,
                revision: None,
            },
            WorkspaceQuery::Tasks {
                project_id: project.id.clone(),
                offset: 42,
                revision: Some(revision.clone()),
            },
            WorkspaceQuery::Task {
                project_id: project.id.clone(),
                task_id: "missing".into(),
                revision: revision.clone(),
            },
            WorkspaceQuery::TaskEvidence {
                project_id: project.id.clone(),
                task_id: "task-00".into(),
                offset: 44,
                revision: revision.clone(),
            },
        ] {
            assert!(make(&project, &state).query(&query).is_err());
        }
        // Even a same-sequence in-memory observation change invalidates the page.
        state.tasks.get_mut("task-00").unwrap().evidence[0].push('x');
        assert!(make(&project, &state)
            .query(&WorkspaceQuery::Task {
                project_id: project.id.clone(),
                task_id: "task-00".into(),
                revision
            })
            .unwrap_err()
            .contains("已变化"));
        state.tasks.remove("task-00");
        assert!(make(&project, &state)
            .query(&WorkspaceQuery::Tasks {
                project_id: project.id.clone(),
                offset: 0,
                revision: None
            })
            .is_err());
        let empty = WorkflowState::default();
        let WorkspaceResult::Tasks(page) = make(&project, &empty)
            .query(&WorkspaceQuery::Tasks {
                project_id: project.id.clone(),
                offset: 0,
                revision: None,
            })
            .unwrap()
        else {
            panic!("empty")
        };
        assert_eq!(page.total, 0);
    }
    #[test]
    fn workspace_project_summary_keeps_unknown_resource_holders() {
        let mut state = WorkflowState::default();
        for (n, task_state) in [
            TaskState::Unknown,
            TaskState::Running,
            TaskState::Submitted,
            TaskState::Accepted,
            TaskState::Pending,
        ]
        .into_iter()
        .enumerate()
        {
            state.tasks.insert(
                format!("task-{n}"),
                TaskRun {
                    state: task_state,
                    worker: None,
                    attempt: None,
                    fence: 0,
                    evidence: vec!["original evidence remains elsewhere".into()],
                    verified: false,
                    cancellation_requested: None,
                },
            );
        }
        let project = Project {
            id: "project".into(),
            root: "D:/controlled".into(),
            timezone: "Asia/Shanghai".into(),
        };
        let value = summary(&project, Some("a".repeat(64)), 17, &state);
        assert_eq!(
            (
                value.task_count,
                value.resource_holding_tasks,
                value.unknown_tasks
            ),
            (5, 3, 1)
        );
        assert!(!value.approved);
        assert_eq!(value.sequence, 17);
        assert_eq!(state.tasks.len(), 5);
    }
}
