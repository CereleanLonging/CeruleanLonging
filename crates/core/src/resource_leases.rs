//! Cross-project exclusion from existing durable task and startup authorities.
//! No second lease journal or timeout can silently release an uncertain attempt.
use super::execution_resources::{task_resources, ResourceKey};
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ResourceLease {
    pub key: ResourceKey,
    pub project: String,
    pub task: String,
    pub attempt: String,
    pub starting: bool,
}

impl RuntimeHost {
    fn original_resources(
        &self,
        snapshots: &[ExecutionSnapshot],
        project: &str,
        task: &str,
        revision: &str,
        attempt: &str,
    ) -> Result<Vec<ResourceKey>, String> {
        let current = snapshots
            .iter()
            .find(|s| s.project.id == project)
            .and_then(|s| s.revision.as_ref())
            .filter(|p| p.approval_digest().is_ok_and(|d| d == revision));
        let plan = current
            .or_else(|| {
                self.projects.get(project).and_then(|s| {
                    s.history()
                        .iter()
                        .find_map(|e| match &e.body.context.execution {
                            Some(ExecutionEvidence::Revision { revision: p })
                                if p.approval_digest().is_ok_and(|d| d == revision) =>
                            {
                                Some(p)
                            }
                            _ => None,
                        })
                })
            })
            .ok_or("资源占用缺少原批准方案，须先核对原启动记录")?;
        task_resources(plan, task, attempt)
    }

    pub(super) fn resource_leases(&self) -> Result<Vec<ResourceLease>, String> {
        require(
            self.management.healthy()
                && self.startups.healthy()
                && self.omp_startups.healthy()
                && self.api_fault.is_none()
                && self.projects.values().all(|s| !s.requires_reopen()),
            "资源账本需要重开核对",
        )?;
        let snapshots = self.snapshot().workflows;
        let mut result = Vec::new();
        for snapshot in &snapshots {
            let Some(plan) = &snapshot.revision else {
                continue;
            };
            for task in &plan.workflow.tasks {
                if let Some(run) = snapshot
                    .state
                    .tasks
                    .get(&task.id)
                    .filter(|t| t.state.holds_resources())
                {
                    let attempt = run.attempt.clone().ok_or("持有资源的任务缺少尝试")?;
                    for key in task_resources(plan, &task.id, &attempt)? {
                        result.push(ResourceLease {
                            key,
                            project: snapshot.project.id.clone(),
                            task: task.id.clone(),
                            attempt: attempt.clone(),
                            starting: false,
                        });
                    }
                }
            }
        }
        let mut reserve =
            |project: &str, task: &str, attempt: &str, revision: &str| -> Result<(), String> {
                for key in self.original_resources(&snapshots, project, task, revision, attempt)? {
                    if !result
                        .iter()
                        .any(|l| l.project == project && l.attempt == attempt && l.key == key)
                    {
                        result.push(ResourceLease {
                            key,
                            project: project.into(),
                            task: task.into(),
                            attempt: attempt.into(),
                            starting: true,
                        });
                    }
                }
                Ok(())
            };
        for record in self.startups.all() {
            let i = &record.intent;
            let released = record.closure.is_some()
                || self
                    .codex_jobs
                    .get(&(i.project_id.clone(), i.attempt_id.clone()))
                    .is_some_and(|j| {
                        matches!(
                            j.state,
                            CodexJobState::Submitted
                                | CodexJobState::Cancelled
                                | CodexJobState::Abandoned
                        )
                    });
            if !released {
                reserve(&i.project_id, &i.task_id, &i.attempt_id, &i.revision_digest)?;
            }
        }
        for record in self.omp_startups.all() {
            let i = &record.intent;
            let released = record.closure.is_some()
                || self
                    .omp_jobs
                    .get(&(i.project_id.clone(), i.attempt_id.clone()))
                    .is_some_and(|j| {
                        matches!(
                            j.state,
                            omp_jobs::OmpJobState::Submitted
                                | omp_jobs::OmpJobState::Cancelled
                                | omp_jobs::OmpJobState::Abandoned
                        )
                    });
            if !released {
                reserve(&i.project_id, &i.task_id, &i.attempt_id, &i.revision_digest)?;
            }
        }
        for startup in self.api_startups.values() {
            let a = &startup.attempt;
            let released = self
                .api_jobs
                .get(&(startup.project_id.clone(), a.id.clone()))
                .is_some_and(|j| {
                    matches!(
                        j.state,
                        api_jobs::ApiJobState::Submitted
                            | api_jobs::ApiJobState::Abandoned
                            | api_jobs::ApiJobState::Retired
                    )
                });
            if !released {
                reserve(&startup.project_id, &a.task_id, &a.id, &a.revision_digest)?;
            }
        }
        for status in self.management.statuses() {
            if self
                .management
                .application_closure(&status.operation_id)
                .is_some()
            {
                continue;
            }
            let Some((
                crate::management_execution::ApplicationTarget::Workflow {
                    command: WorkflowCommand::Assign { task, attempt, .. },
                    context,
                    ..
                },
                false,
                _,
            )) = self.management.application(&status.operation_id)
            else {
                continue;
            };
            let Some(ExecutionEvidence::Attempt { attempt: bound }) = &context.execution else {
                return Err("待提交分配缺少原尝试，无法释放资源".into());
            };
            require(
                bound.id == *attempt && bound.task_id == *task,
                "待提交分配资源归属不符",
            )?;
            reserve(&status.project_id, task, attempt, &bound.revision_digest)?;
        }
        Ok(result)
    }

    pub(super) fn require_task_resources(
        &self,
        project: &str,
        task: &str,
        attempt: &str,
    ) -> Result<(), String> {
        let snapshots = self.snapshot().workflows;
        let plan = snapshots
            .iter()
            .find(|s| s.project.id == project)
            .and_then(|s| s.revision.as_ref())
            .ok_or("执行边界缺少批准方案")?;
        if let Some(message) = self.task_resource_conflict(project, task, attempt)? {
            return Err(message);
        }
        crate::worker_boundaries::validate(plan, false)
    }

    // New execution needs a bound directory. Reading retained resource leases
    // and replaying an original assignment must remain possible without it.
    pub(super) fn require_execution_boundaries(&self, project: &str) -> Result<(), String> {
        let snapshots = self.snapshot().workflows;
        let plan = snapshots
            .iter()
            .find(|s| s.project.id == project)
            .and_then(|s| s.revision.as_ref())
            .ok_or("执行边界缺少批准方案")?;
        crate::worker_boundaries::validate(plan, true)
    }
    // A normal occupied resource is a scheduling wait. Corrupt authority remains
    // an error and must never be swallowed as ordinary resource contention.
    pub(super) fn task_resource_conflict(
        &self,
        project: &str,
        task: &str,
        attempt: &str,
    ) -> Result<Option<String>, String> {
        let snapshots = self.snapshot().workflows;
        let plan = snapshots
            .iter()
            .find(|s| s.project.id == project)
            .and_then(|s| s.revision.as_ref())
            .ok_or("资源检查缺少项目方案")?;
        let keys = task_resources(plan, task, attempt)?;
        for lease in self.resource_leases()? {
            if lease.project == project && lease.task == task && lease.attempt == attempt {
                continue;
            }
            // A stale preassignment for the same task cannot be bypassed by
            // choosing a new attempt/worktree even when its paths are disjoint.
            if (lease.project == project && lease.task == task)
                || keys.iter().any(|key| key.conflicts(&lease.key))
            {
                return Ok(Some(format!(
                    "共享资源 {} 由项目 {} 的任务 {} / 尝试 {} 持有；先等待验收或核对原结果",
                    lease.key.label(),
                    lease.project,
                    lease.task,
                    lease.attempt
                )));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::startup_journal::StartupIntent;
    pub(super) fn fixture(root: &Path) -> RuntimeHost {
        let (mut host, draft) = project_planning::tests::fixture(root);
        host.configure_project(draft).unwrap();
        project_planning::tests::complete(&mut host, project_planning::tests::proposal("tests"));
        let mut plan = host.projects["project"].revision().unwrap().clone();
        plan.workflow.version += 1;
        plan.workflow.tasks[0].resource_keys = Some(vec!["tcp-4000".into(), "db-fixture".into()]);
        host.propose("project", "resources-proposal", plan.clone())
            .unwrap();
        host.approve("project", &plan.approval_digest().unwrap())
            .unwrap();
        fs::create_dir(root.join("other")).unwrap();
        host.register(Project {
            id: "other".into(),
            root: root.join("other").to_string_lossy().into(),
            timezone: "Asia/Shanghai".into(),
        })
        .unwrap();
        plan.project = host.projects["other"]
            .configuration()
            .execution_scope
            .as_ref()
            .unwrap()
            .project
            .clone();
        plan.workflow.version = 1;
        plan.workflow.tasks[0].workspace_key = "other".into();
        host.propose("other", "other-proposal", plan.clone())
            .unwrap();
        host.approve("other", &plan.approval_digest().unwrap())
            .unwrap();
        host
    }
    pub(super) fn reserve(host: &mut RuntimeHost) {
        let store = &host.projects["project"];
        let plan = store.revision().unwrap();
        host.startups
            .begin(StartupIntent {
                schema_version: 1,
                protocol: "codex-durable-before-send-v1".into(),
                project_id: "project".into(),
                task_id: "task".into(),
                attempt_id: "resource-owner".into(),
                revision_digest: plan.approval_digest().unwrap(),
                worker: plan.task("task").unwrap().worker.clone().unwrap(),
                management_operation: None,
                management_hash: None,
                workflow_sequence: store.history().len() as u32,
                workflow_hash: store.history().last().map(|e| e.hash.clone()),
                started_at: chrono::Utc::now().to_rfc3339(),
            })
            .unwrap();
    }
    #[test]
    fn durable_preassignment_reservation_blocks_other_project_until_explicit_closure() {
        let root = tempfile::tempdir().unwrap();
        let mut host = fixture(root.path());
        assert!(host
            .require_task_resources("other", "task", "resource-candidate")
            .is_ok());
        reserve(&mut host);
        assert_eq!(
            host.projects["project"].state().tasks["task"].state,
            TaskState::Pending
        );
        let leases = host.resource_leases().unwrap();
        assert_eq!(leases.len(), 3);
        assert!(leases
            .iter()
            .all(|l| l.starting && l.attempt == "resource-owner"));
        assert!(host
            .require_task_resources("other", "task", "resource-candidate")
            .unwrap_err()
            .contains("tcp-4000"));
        let revision = host.projects["other"]
            .revision()
            .unwrap()
            .approval_digest()
            .unwrap();
        assert!(host
            .start_codex("other", "task", "blocked-codex", &revision)
            .unwrap_err()
            .contains("共享资源"));
        assert!(host
            .start_omp("other", "task", "blocked-omp", &revision)
            .unwrap_err()
            .contains("共享资源"));
        assert!(host.startups.get("other", "blocked-codex").is_none());
        assert!(host.omp_startups.get("other", "blocked-omp").is_none());
        let history = host.projects["project"].history().to_vec();
        drop(host);
        let mut reopened = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(reopened.projects["project"].history(), history);
        assert_eq!(reopened.resource_leases().unwrap(), leases);
        assert!(reopened
            .require_task_resources("other", "task", "resource-candidate")
            .is_err());
        let review = reopened
            .startup_detail("project", "resource-owner")
            .unwrap();
        reopened
            .abandon_codex_startup(
                "project",
                "resource-owner",
                review.evidence_digest.as_deref().unwrap(),
                "Explicitly close this synthetic reservation",
            )
            .unwrap();
        assert!(reopened.resource_leases().unwrap().is_empty());
        assert!(reopened
            .require_task_resources("other", "task", "resource-candidate")
            .is_ok());
        drop(reopened);
        let reopened = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert!(reopened
            .require_task_resources("other", "task", "resource-candidate")
            .is_ok());
    }

    #[test]
    fn active_ledger_and_unknown_replay_preserve_cross_project_exclusion() {
        let root = tempfile::tempdir().unwrap();
        let mut host = fixture(root.path());
        let mut store = host.projects.remove("project").unwrap();
        let plan = store.revision().unwrap().clone();
        let attempt = Attempt {
            workspace_proof: None,
            id: "assigned-owner".into(),
            task_id: "task".into(),
            worker_id: "worker-codex".into(),
            revision_digest: plan.approval_digest().unwrap(),
            fence: 1,
            binding: SessionBinding {
                adapter_id: "codex".into(),
                adapter_version: "fixture".into(),
                session_id: "fixture-session".into(),
                identity_strength: IdentityStrength::ProviderId,
                workspace: plan.project.root.clone(),
            },
        };
        store
            .command(
                "assign-resource",
                "deputy",
                WorkflowCommand::Assign {
                    task: "task".into(),
                    worker: "worker-codex".into(),
                    attempt: attempt.id.clone(),
                },
                CommandContext {
                    execution: Some(ExecutionEvidence::Attempt {
                        attempt: attempt.clone(),
                    }),
                    ..Default::default()
                },
                true,
            )
            .unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        let (_, receiver) = std::sync::mpsc::channel();
        let (cancel, _) = tokio::sync::oneshot::channel();
        host.codex_active.insert(
            ("project".into(), attempt.id.clone()),
            crate::async_codex::ActiveCodex {
                preparation_cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                advice_wake: tokio::sync::watch::channel(()).0,
                advice_gate: std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0)),
                observation: None,
                observation_saved: None,
                observation_error: None,
                snapshot: crate::async_verification::snapshot(&store),
                job: CodexJob {
                    management_operation: None,
                    project_id: "project".into(),
                    task_id: "task".into(),
                    attempt_id: attempt.id.clone(),
                    revision_digest: attempt.revision_digest.clone(),
                    worker: plan.task("task").unwrap().worker.clone().unwrap(),
                    state: CodexJobState::Executing,
                    started_at: now.clone(),
                    updated_at: now,
                    message: None,
                },
                receiver,
                disconnected: false,
                cancel: Some(cancel),
            },
        );
        assert!(!host.projects.contains_key("project"));
        let writer =
            crate::async_api::ApiWorkflow::start(store, host.root.join("workflows/project"))
                .unwrap();
        host.api_writers.insert("project".into(), writer);
        let held = host.resource_leases().unwrap();
        assert_eq!(held.len(), 3);
        assert!(held.iter().all(|l| !l.starting));
        assert!(host
            .require_task_resources("other", "task", "resource-candidate")
            .is_err());
        host.codex_active
            .remove(&("project".into(), attempt.id.clone()));
        let mut store = host
            .api_writers
            .remove("project")
            .unwrap()
            .owner
            .finish(false)
            .unwrap();
        store
            .command(
                "dispatch-resource",
                "core",
                WorkflowCommand::DispatchIntent {
                    task: "task".into(),
                    attempt: attempt.id.clone(),
                },
                CommandContext {
                    thread_id: Some(attempt.binding.session_id.clone()),
                    cwd: Some(plan.project.root),
                    ..Default::default()
                },
                true,
            )
            .unwrap();
        store
            .command(
                "unknown-resource",
                "core",
                WorkflowCommand::RecoveryUnknown {
                    task: "task".into(),
                    attempt: attempt.id,
                },
                CommandContext::default(),
                true,
            )
            .unwrap();
        host.projects.insert("project".into(), store);
        assert_eq!(
            host.projects["project"].state().tasks["task"].state,
            TaskState::Unknown
        );
        assert_eq!(host.resource_leases().unwrap(), held);
        let history = host.projects["project"].history().to_vec();
        drop(host);
        let host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(host.projects["project"].history(), history);
        assert_eq!(host.resource_leases().unwrap(), held);
        assert!(host
            .require_task_resources("other", "task", "resource-candidate")
            .is_err());
    }

    #[test]
    fn api_conflict_precedes_startup_and_distinct_keys_remain_available() {
        use cerulean_domain::{api_worker::*, credentials::ProviderEndpoint, management::*};
        let root = tempfile::tempdir().unwrap();
        let mut host = fixture(root.path());
        let mut plan = host.projects["other"].revision().unwrap().clone();
        plan.workflow.version += 1;
        plan.execution[0].adapter_id = "deepseek".into();
        plan.execution[0].worker = None;
        plan.execution[0].api_worker = Some(ApiWorkerSelection {
            write_scope: None,
            target: ApiTarget::Provider {
                endpoint: ProviderEndpoint::DeepSeek {},
            },
            model: "fixture".into(),
            file_tools: vec![ToolPermission::ReadFiles, ToolPermission::WriteFiles],
            max_rounds: 3,
            max_tool_calls: 8,
            max_output_tokens: 512,
            max_total_tokens: 4096,
            request_timeout_seconds: 30,
        });
        host.propose("other", "api-proposal", plan.clone()).unwrap();
        host.approve("other", &plan.approval_digest().unwrap())
            .unwrap();
        reserve(&mut host);
        let context = host
            .management_context("other", "Use approved resources")
            .unwrap();
        let digest = digest(&context).unwrap();
        host.management
            .begin(crate::management_execution::DecisionIntent {
                operation_id: "resource-api-assign".into(),
                project_id: "other".into(),
                purpose: Purpose::Assign,
                model: "fixture-deputy".into(),
                state_digest: digest.clone(),
                context,
                created_at: chrono::Utc::now().to_rfc3339(),
            })
            .unwrap();
        host.management
            .finish(
                "resource-api-assign",
                Ok(cerulean_adapters::openai::DecisionObservation {
                    response_id: "resp_resource_fixture".into(),
                    requested_model: "fixture-deputy".into(),
                    actual_model: "fixture-deputy".into(),
                    input_tokens: 1,
                    output_tokens: 1,
                    envelope: DecisionEnvelope {
                        state_digest: digest,
                        decision: ManagementDecision::Assign {
                            task_id: "task".into(),
                            worker_id: "worker-deepseek".into(),
                            rationale: "Approved task".into(),
                        },
                    },
                }),
            )
            .unwrap();
        let history = host.projects["other"].history().to_vec();
        assert!(host
            .start_managed_api("resource-api-assign", "blocked-api")
            .unwrap_err()
            .contains("共享资源"));
        assert!(host.api_startups.is_empty() && host.api_jobs.is_empty());
        assert!(host.management.application("resource-api-assign").is_none());
        assert_eq!(host.projects["other"].history(), history);
        // A different named resource does not inherit an unrelated reservation.
        plan.workflow.version += 1;
        plan.workflow.tasks[0].resource_keys = Some(vec!["tcp-4001".into()]);
        host.propose("other", "disjoint-proposal", plan.clone())
            .unwrap();
        host.approve("other", &plan.approval_digest().unwrap())
            .unwrap();
        assert!(host
            .require_task_resources("other", "task", "resource-candidate")
            .is_ok());
        assert_eq!(host.resource_leases().unwrap().len(), 3);
    }

    #[test]
    fn prepared_assignment_retains_resources_across_reopen_without_workflow_event() {
        use crate::management_execution::ApplicationTarget;
        use cerulean_domain::management::{ManagementDecision, Purpose};
        let root = tempfile::tempdir().unwrap();
        let mut host = fixture(root.path());
        let plan = host.projects["project"].revision().unwrap().clone();
        let context = host
            .management_context("project", "Reserve approved task")
            .unwrap();
        let state = digest(&context).unwrap();
        host.management
            .begin(crate::management_execution::DecisionIntent {
                operation_id: "prepared-resource".into(),
                project_id: "project".into(),
                purpose: Purpose::Assign,
                model: "fixture-deputy".into(),
                state_digest: state.clone(),
                context,
                created_at: chrono::Utc::now().to_rfc3339(),
            })
            .unwrap();
        host.management
            .finish(
                "prepared-resource",
                Ok(cerulean_adapters::openai::DecisionObservation {
                    response_id: "resp_prepared_fixture".into(),
                    requested_model: "fixture-deputy".into(),
                    actual_model: "fixture-deputy".into(),
                    input_tokens: 1,
                    output_tokens: 1,
                    envelope: cerulean_domain::management::DecisionEnvelope {
                        state_digest: state.clone(),
                        decision: ManagementDecision::Assign {
                            task_id: "task".into(),
                            worker_id: "worker-codex".into(),
                            rationale: "Reserve approved task".into(),
                        },
                    },
                }),
            )
            .unwrap();
        let attempt = Attempt {
            workspace_proof: None,
            id: "prepared-owner".into(),
            task_id: "task".into(),
            worker_id: "worker-codex".into(),
            revision_digest: plan.approval_digest().unwrap(),
            fence: 1,
            binding: SessionBinding {
                adapter_id: "codex".into(),
                adapter_version: "fixture".into(),
                session_id: "fixture-session".into(),
                identity_strength: IdentityStrength::ProviderId,
                workspace: plan.project.root,
            },
        };
        host.management
            .prepare_application(
                "prepared-resource",
                ApplicationTarget::Workflow {
                    principal: "deputy".into(),
                    command: WorkflowCommand::Assign {
                        task: "task".into(),
                        worker: "worker-codex".into(),
                        attempt: attempt.id.clone(),
                    },
                    context: Box::new(CommandContext {
                        execution: Some(ExecutionEvidence::Attempt { attempt }),
                        ..Default::default()
                    }),
                },
                &state,
            )
            .unwrap();
        assert!(host.startups.all().is_empty());
        assert_eq!(
            host.projects["project"].state().tasks["task"].state,
            TaskState::Pending
        );
        let leases = host.resource_leases().unwrap();
        assert_eq!(leases.len(), 3);
        assert!(host
            .require_task_resources("other", "task", "resource-candidate")
            .is_err());
        let history = host.projects["project"].history().to_vec();
        drop(host);
        let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
        host.poll_verifications();
        assert_eq!(host.projects["project"].history(), history);
        assert_eq!(host.resource_leases().unwrap(), leases);
        assert!(host
            .require_task_resources("other", "task", "resource-candidate")
            .is_err());
    }

    #[test]
    fn published_assignment_with_stale_projection_cannot_release_resources() {
        let root = tempfile::tempdir().unwrap();
        let mut host = fixture(root.path());
        let plan = host.projects["project"].revision().unwrap().clone();
        let attempt = Attempt {
            workspace_proof: None,
            id: "published-owner".into(),
            task_id: "task".into(),
            worker_id: "worker-codex".into(),
            revision_digest: plan.approval_digest().unwrap(),
            fence: 1,
            binding: SessionBinding {
                adapter_id: "codex".into(),
                adapter_version: "fixture".into(),
                session_id: "fixture-session".into(),
                identity_strength: IdentityStrength::ProviderId,
                workspace: plan.project.root,
            },
        };
        let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = host.projects.get_mut("project").unwrap().command_observed(
                "publish-resource",
                "deputy",
                WorkflowCommand::Assign {
                    task: "task".into(),
                    worker: "worker-codex".into(),
                    attempt: attempt.id.clone(),
                },
                CommandContext {
                    execution: Some(ExecutionEvidence::Attempt { attempt }),
                    ..Default::default()
                },
                true,
                |point| {
                    if point == crate::scheduler::CommitPoint::EventPublished {
                        panic!("synthetic interruption after resource assignment publication");
                    }
                },
            );
        }));
        assert!(interrupted.is_err());
        assert_eq!(
            host.projects["project"].state().tasks["task"].state,
            TaskState::Pending
        );
        assert!(host
            .require_task_resources("other", "task", "resource-candidate")
            .unwrap_err()
            .contains("资源账本需要重开核对"));
        drop(host);
        let host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(
            host.projects["project"].state().tasks["task"].state,
            TaskState::Assigned
        );
        assert_eq!(host.resource_leases().unwrap().len(), 3);
        assert!(host
            .require_task_resources("other", "task", "resource-candidate")
            .unwrap_err()
            .contains("共享资源"));
    }
}

#[cfg(test)]
#[path = "execution_resource_tests.rs"]
mod execution_tests;
