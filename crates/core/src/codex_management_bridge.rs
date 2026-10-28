//! The service owns management evidence; the worker owns the project ledger.
//! Two acknowledged phases bind those owners without giving the worker a journal writer.
use super::*;
use crate::{management_execution::ApplicationTarget, scheduler::Entry};
use cerulean_domain::management::{ManagementDecision, Purpose};

fn assignment_target(attempt: Attempt) -> ApplicationTarget {
    ApplicationTarget::Workflow {
        principal: "deputy".into(),
        command: WorkflowCommand::Assign {
            task: attempt.task_id.clone(),
            worker: attempt.worker_id.clone(),
            attempt: attempt.id.clone(),
        },
        context: Box::new(CommandContext {
            execution: Some(ExecutionEvidence::Attempt { attempt }),
            ..Default::default()
        }),
    }
}
struct BridgeScope {
    task_id: String,
    attempt_id: String,
    revision_digest: String,
    snapshot: ExecutionSnapshot,
}
fn valid_identity(adapter: &str, identity: &IdentityStrength) -> bool {
    matches!(
        (adapter, identity),
        ("codex", IdentityStrength::ProviderId) | ("omp", IdentityStrength::ControllerRequest(_))
    ) || (cerulean_domain::api_worker::is_api_adapter(adapter)
        && matches!(identity, IdentityStrength::ControllerRequest(_)))
}
impl RuntimeHost {
    /// Production orchestration entry: task and worker come from the saved deputy
    /// response. Caller chooses only a fresh local attempt identifier.
    pub fn start_managed_codex(&mut self, operation: &str, attempt: &str) -> Result<(), String> {
        self.poll_verifications();
        require(
            self.management.application(operation).is_none(),
            "management decision already applying or applied",
        )?;
        require(
            !self
                .codex_jobs
                .values()
                .any(|j| j.management_operation.as_deref() == Some(operation)),
            "management decision already owns a Codex attempt; reconcile before retry",
        )?;
        let project = self.management.intent(operation)?.project_id.clone();
        let observation = self.management_envelope(operation)?;
        let ManagementDecision::Assign {
            task_id, worker_id, ..
        } = &observation.decision
        else {
            return Err("deputy assignment required".into());
        };
        require(
            worker_id == "worker-codex",
            "decision selects a different worker",
        )?;
        let task = task_id.clone();
        let revision_digest = self
            .codex_read(&project, &task, attempt)?
            .revision()
            .ok_or("approved revision missing")?
            .approval_digest()?;
        self.start_codex_inner(
            &project,
            &task,
            attempt,
            &revision_digest,
            Some(operation.into()),
        )
    }
    fn bridge_scope(
        &self,
        project: &str,
        operation: &str,
        adapter: &str,
    ) -> Result<BridgeScope, String> {
        require(
            self.management.application_closure(operation).is_none()
                && self.management.startup_closure(operation).is_none(),
            "应用事务已关闭，迟到确认被拒绝",
        )?;
        self.ensure_run_dispatch_authorized(operation)?;
        let (owner, task, attempt, revision, snapshot) = match adapter {
            "codex" => {
                let a = self
                    .codex_active
                    .values()
                    .find(|a| {
                        a.job.project_id == project
                            && a.job.management_operation.as_deref() == Some(operation)
                    })
                    .ok_or("managed Codex worker missing")?;
                (
                    &a.job.management_operation,
                    &a.job.task_id,
                    &a.job.attempt_id,
                    &a.job.revision_digest,
                    &a.snapshot,
                )
            }
            "omp" => {
                let a = self
                    .omp_active
                    .values()
                    .find(|a| {
                        a.job.project_id == project
                            && a.job.management_operation.as_deref() == Some(operation)
                    })
                    .ok_or("managed OMP worker missing")?;
                (
                    &a.job.management_operation,
                    &a.job.task_id,
                    &a.job.attempt_id,
                    &a.job.revision_digest,
                    &a.snapshot,
                )
            }
            api if cerulean_domain::api_worker::is_api_adapter(api) => {
                let a = self
                    .api_active
                    .values()
                    .find(|a| {
                        a.job.project_id == project
                            && a.job.management_operation.as_deref() == Some(operation)
                    })
                    .ok_or("managed API worker missing")?;
                require(
                    a.job.worker.target.provider().id() == api,
                    "API active adapter mismatch",
                )?;
                (
                    &a.job.management_operation,
                    &a.job.task_id,
                    &a.job.attempt_id,
                    &a.job.revision_digest,
                    &a.snapshot,
                )
            }
            _ => return Err("unsupported managed worker".into()),
        };
        require(
            owner.as_deref() == Some(operation),
            "worker management operation mismatch",
        )?;
        let intent = self.management.intent(operation)?;
        require(
            intent.project_id == project && intent.purpose == Purpose::Assign,
            "management role or project mismatch",
        )?;
        Ok(BridgeScope {
            task_id: task.clone(),
            attempt_id: attempt.clone(),
            revision_digest: revision.clone(),
            snapshot: snapshot.clone(),
        })
    }
    pub(super) fn prepare_managed_codex_assignment(
        &mut self,
        project: &str,
        operation: &str,
        attempt: Attempt,
        snapshot: ExecutionSnapshot,
    ) -> Result<(), String> {
        self.prepare_managed_assignment(project, operation, attempt, snapshot, "codex")
    }
    pub(super) fn commit_managed_codex_assignment(
        &mut self,
        project: &str,
        operation: &str,
        entry: Entry,
        snapshot: ExecutionSnapshot,
    ) -> Result<(), String> {
        self.commit_managed_assignment(project, operation, entry, snapshot, "codex")
    }
    pub(super) fn prepare_managed_assignment(
        &mut self,
        project: &str,
        operation: &str,
        attempt: Attempt,
        snapshot: ExecutionSnapshot,
        adapter: &str,
    ) -> Result<(), String> {
        self.require_task_resources(project, &attempt.task_id, &attempt.id)?;
        self.management
            .check_attempt_reservation(project, &attempt.id, Some(operation))?;
        if cerulean_domain::api_worker::is_api_adapter(adapter) {
            let startup = self
                .api_startups
                .get(&(project.into(), attempt.id.clone()))
                .ok_or("API durable startup missing")?;
            require(
                startup.attempt == attempt && startup.management_operation == operation,
                "API startup scope mismatch",
            )?;
        }
        let startups = if adapter == "omp" {
            &self.omp_startups
        } else {
            &self.startups
        };
        startups.check_reservation(project, &attempt.id, Some(operation))?;
        if adapter == "omp" {
            require(
                startups.get(project, &attempt.id).is_some(),
                "OMP durable startup missing",
            )?;
        }
        let active = self.bridge_scope(project, operation, adapter)?;
        let scoped_anchor = self.shared_assignment_anchor(project, &attempt.id, adapter)?;
        let scoped_api = scoped_anchor.is_some();
        if scoped_api {
            let store = self.api_read(project, &attempt.task_id, &attempt.id)?;
            let original = self.validate_api_assignment_scope(
                operation,
                &*store,
                &attempt.task_id,
                snapshot.sequence,
                true,
            )?;
            let (anchor_sequence, anchor_hash) = scoped_anchor.as_ref().unwrap();
            require(
                *anchor_sequence == original.sequence
                    && store
                        .history()
                        .get(original.sequence as usize - 1)
                        .is_some_and(|e| &e.hash == anchor_hash)
                    && digest(&api_assignment_scope::prefix(&*store, snapshot.sequence)?)?
                        == digest(&snapshot)?,
                "API准备回执不属于原授权历史",
            )?;
        } else {
            require(
                digest(&snapshot)? == digest(&active.snapshot)?,
                "worker state changed before managed assignment",
            )?;
        }
        let revision = snapshot
            .revision
            .as_ref()
            .ok_or("managed revision missing")?;
        revision.validate_attempt_workspace(&attempt)?;
        #[cfg(windows)]
        crate::git_workspaces::require_prepared(revision, &attempt)?;
        require(
            attempt.id == active.attempt_id
                && attempt.task_id == active.task_id
                && attempt.worker_id == format!("worker-{adapter}")
                && attempt.revision_digest == active.revision_digest
                && attempt.binding.adapter_id == adapter
                && valid_identity(adapter, &attempt.binding.identity_strength),
            "managed session binding mismatch",
        )?;
        let task = snapshot
            .state
            .tasks
            .get(&attempt.task_id)
            .ok_or("managed task missing")?;
        require(
            snapshot.state.approved
                && task.state == TaskState::Pending
                && task.attempt.is_none()
                && attempt.fence == task.fence.checked_add(1).ok_or("fence overflow")?
                && !attempt.binding.session_id.trim().is_empty()
                && !attempt.binding.adapter_version.trim().is_empty(),
            "managed assignment is not the next approved attempt",
        )?;
        if adapter == "omp" {
            let startup = self
                .omp_startups
                .get(project, &attempt.id)
                .ok_or("OMP startup missing")?;
            require(
                startup.intent.management_operation.as_deref() == Some(operation)
                    && startup.intent.task_id == attempt.task_id
                    && startup.intent.revision_digest == attempt.revision_digest,
                "OMP startup scope mismatch",
            )?;
            cerulean_adapters::omp_transport::worker::DispatchIntent {
                version: 1,
                task_id: attempt.task_id.clone(),
                attempt_id: attempt.id.clone(),
                revision_digest: attempt.revision_digest.clone(),
                fence: attempt.fence,
                binding: attempt.binding.clone(),
                provider: startup.intent.worker.provider.clone(),
                model: startup.intent.worker.model.clone(),
                prompt_sha256: "0".repeat(64),
            }
            .validate_metadata()?;
        }
        let intent = self.management.intent(operation)?;
        let requirement = intent.context["requirement"]
            .as_str()
            .ok_or("management requirement missing")?;
        let current = self.rebuild_management_context(
            intent,
            self.context_for_snapshot(requirement, &snapshot),
        )?;
        let current_digest = if scoped_api {
            intent.state_digest.clone()
        } else {
            digest(&current)?
        };
        let fee_ack = self.management_fee_ack(operation)?;
        self.management.prepare_application_with_fee(
            operation,
            assignment_target(attempt),
            &current_digest,
            fee_ack,
        )
    }
    pub(super) fn commit_managed_assignment(
        &mut self,
        project: &str,
        operation: &str,
        entry: Entry,
        snapshot: ExecutionSnapshot,
        adapter: &str,
    ) -> Result<(), String> {
        let active = self.bridge_scope(project, operation, adapter)?;
        let scoped_anchor = self.shared_assignment_anchor(project, &active.attempt_id, adapter)?;
        let scoped_api = scoped_anchor.is_some();
        if scoped_api {
            let store = self.api_read(project, &active.task_id, &active.attempt_id)?;
            let original = self.validate_api_assignment_scope(
                operation,
                &*store,
                &active.task_id,
                entry
                    .body
                    .event
                    .sequence
                    .checked_sub(1)
                    .ok_or("API分配事件序号无效")?,
                false,
            )?;
            let (anchor_sequence, anchor_hash) = scoped_anchor.as_ref().unwrap();
            require(
                *anchor_sequence == original.sequence
                    && store
                        .history()
                        .get(original.sequence as usize - 1)
                        .is_some_and(|e| &e.hash == anchor_hash)
                    && digest(&api_assignment_scope::prefix(&*store, snapshot.sequence)?)?
                        == digest(&snapshot)?,
                "API分配回执不属于原授权历史",
            )?;
        } else {
            require(
                entry.body.event.sequence
                    == active
                        .snapshot
                        .sequence
                        .checked_add(1)
                        .ok_or("workflow sequence overflow")?,
                "managed assignment event mismatch",
            )?;
        }
        require(
            entry.body.operation_id == format!("mgmt-{operation}")
                && digest(&entry.body)? == entry.hash,
            "managed assignment event mismatch",
        )?;
        let (target, applied, _) = self
            .management
            .application(operation)
            .ok_or("managed application intent missing")?;
        require(!applied, "managed assignment already acknowledged")?;
        let ApplicationTarget::Workflow {
            principal,
            command,
            context,
        } = target
        else {
            return Err("assignment command required".into());
        };
        require(
            entry.body.event.principal == *principal
                && entry.body.event.command == *command
                && entry.body.context == **context,
            "assignment differs from approved application target",
        )?;
        require(
            snapshot.project == active.snapshot.project
                && snapshot.revision_digest == active.snapshot.revision_digest
                && snapshot.approval == active.snapshot.approval
                && snapshot.sequence == entry.body.event.sequence
                && snapshot.state.tasks.get(&active.task_id).is_some_and(|t| {
                    t.state == TaskState::Assigned
                        && t.attempt.as_deref() == Some(active.attempt_id.as_str())
                }),
            "invalid assigned snapshot",
        )?;
        // Confirm the event returned by the worker is the actual immutable disk event.
        let path = self
            .root
            .join("workflows")
            .join(project)
            .join("events")
            .join(format!(
                "{:012}_{}.json",
                entry.body.event.sequence, entry.body.operation_id
            ));
        ordinary(&path, false)?;
        require(
            fs::metadata(&path).map_err(|e| e.to_string())?.len() <= 256 * 1024,
            "assignment evidence too large",
        )?;
        let saved: Entry = serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|_| "invalid assignment evidence")?;
        require(saved == entry, "assignment event was not durably published")?;
        self.management
            .record_applied(operation, Some(entry.hash))?;
        match adapter {
            "codex" => {
                self.codex_active
                    .get_mut(&(project.into(), active.attempt_id.clone()))
                    .unwrap()
                    .snapshot = snapshot
            }
            "omp" => {
                self.omp_active
                    .get_mut(&(project.into(), active.attempt_id.clone()))
                    .ok_or("managed OMP attempt missing")?
                    .snapshot = snapshot
            }
            api if cerulean_domain::api_worker::is_api_adapter(api) => {
                self.api_active
                    .get_mut(&(project.into(), active.attempt_id.clone()))
                    .ok_or("managed API attempt missing")?
                    .snapshot = snapshot
            }
            _ => return Err("unsupported managed worker".into()),
        }
        Ok(())
    }
    pub(super) fn validate_managed_codex_links(&self) -> Result<(), String> {
        let mut linked = std::collections::BTreeSet::new();
        for job in self.codex_jobs.values() {
            let store = self
                .projects
                .get(&job.project_id)
                .ok_or("managed job project missing")?;
            if let Some(entry) = store.history().iter().find(|e| matches!(&e.body.event.command, WorkflowCommand::Assign { attempt, .. } if attempt == &job.attempt_id)) {
                if let Some(operation) = entry.body.operation_id.strip_prefix("mgmt-") {
                    require(job.management_operation.as_deref() == Some(operation), "Codex job removed or changed its management link")?;
                }
            }
            let Some(operation) = &job.management_operation else {
                continue;
            };
            require(
                linked.insert(operation),
                "management decision linked to multiple Codex attempts",
            )?;
            let intent = self.management.intent(operation)?;
            require(
                intent.project_id == job.project_id && intent.purpose == Purpose::Assign,
                "Codex management link differs from request",
            )?;
            if let Some(closed) = self.management.startup_closure(operation) {
                let record = self
                    .startups
                    .get(&job.project_id, &job.attempt_id)
                    .ok_or("关闭的管理启动缺少原意图")?;
                require(
                    record
                        .closure
                        .as_ref()
                        .is_some_and(|c| c.reason == closed.reason)
                        && record.head == closed.startup_hash
                        && closed.attempt_id == job.attempt_id
                        && record.intent.management_operation.as_deref() == Some(operation)
                        && self.management.application(operation).is_none(),
                    "启动关闭与管理关联冲突",
                )?;
                continue;
            }
            if let Some((target, _, _)) = self.management.application(operation) {
                let ApplicationTarget::Workflow {
                    command:
                        WorkflowCommand::Assign {
                            task,
                            worker,
                            attempt,
                        },
                    ..
                } = target
                else {
                    return Err("Codex management link is not assignment".into());
                };
                require(
                    task == &job.task_id && attempt == &job.attempt_id && worker == "worker-codex",
                    "Codex management link differs from attempt",
                )?;
            } else {
                let observation = self
                    .management
                    .ready_envelope(operation, &intent.state_digest)?;
                require(
                    matches!(&observation.decision,ManagementDecision::Assign { task_id, worker_id, .. } if task_id == &job.task_id && worker_id == "worker-codex"),
                    "Codex management response mismatch",
                )?;
            }
        }
        Ok(())
    }
}
