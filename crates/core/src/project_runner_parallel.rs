//! Admission while a project's API writer is owned by running attempts.
//! Existing management and workflow journals remain the only authorities.
use super::*;

impl RuntimeHost {
    pub(super) fn advance_parallel_api_run(&mut self, run: &ProjectRun) -> Result<(), String> {
        let project = &run.project_id;
        self.require_available_inner(project, true)?;
        let snapshot = self
            .api_writers
            .get(project)
            .ok_or("API项目写入者缺失")?
            .owner
            .snapshot()?;
        require(
            !snapshot.state.tasks.values().any(|t| {
                matches!(
                    t.state,
                    TaskState::Unknown
                        | TaskState::Cancelled
                        | TaskState::Retired
                        | TaskState::Cancelling
                )
            }),
            "原任务中断或取消，停止新调度并核对原尝试",
        )?;
        let statuses = self.management.statuses();
        if self.jobs.values().any(|j| {
            j.project_id == *project
                    // Historical failures remain in the journal after an
                    // approved revision or successful corrected check. Only
                    // the current, still-unverified attempt blocks admission.
                    // Unknown jobs remain globally blocked by require_available_inner.
                    && snapshot.state.tasks.get(&j.task_id).is_some_and(|t| {
                        t.attempt.as_deref() == Some(j.attempt_id.as_str()) && !t.verified
                    })
                    && !matches!(
                        j.state,
                        VerificationState::Running | VerificationState::Succeeded
                    )
        }) {
            // Let already dispatched peers reach their terminal state. The
            // exclusive path then sends the failed report to the deputy.
            return Ok(());
        }
        if let Some(pending) = statuses
            .iter()
            .find(|s| s.project_id == *project && !s.state.resolved())
        {
            require(
                pending.operation_id.starts_with(&format!("{}-", run.id)),
                "项目存在运行之外的待处理指挥请求",
            )?;
            if pending.state == DecisionState::AwaitingExternal {
                require(
                    pending.purpose == Purpose::Accept,
                    "unexpected external management purpose in run",
                )?;
                self.management.external_request(&pending.operation_id)?;
                self.require_external_accept_run(project, &pending.operation_id)?;
                return Ok(());
            }
            // Prepare/Commit belongs to an already running startup. Let its two
            // original ACKs resolve; never create another decision around it.
            if self.api_active.values().any(|a| {
                a.job.project_id == *project
                    && a.job.management_operation.as_deref() == Some(&pending.operation_id)
            }) || self.omp_active.values().any(|a| {
                a.job.project_id == *project
                    && a.job.management_operation.as_deref() == Some(&pending.operation_id)
            }) || self.codex_active.values().any(|a| {
                a.job.project_id == *project
                    && a.job.management_operation.as_deref() == Some(&pending.operation_id)
            }) {
                return Ok(());
            }
            if pending.purpose == Purpose::Accept {
                require(
                    matches!(
                        pending.state,
                        DecisionState::Ready | DecisionState::Applying
                    ),
                    "主指挥收口结果未知，不会重发",
                )?;
                let response = self.management.original_envelope(&pending.operation_id)?;
                let ManagementDecision::Accept {
                    task_id,
                    attempt_id,
                    ..
                } = &response.decision
                else {
                    return Err("主指挥未给出收口决定，请审阅原响应；不会重发".into());
                };
                return self.apply_management_decision_inner(
                    &pending.operation_id,
                    ApplicationTarget::Workflow {
                        principal: "chief".into(),
                        command: WorkflowCommand::Accept {
                            task: task_id.clone(),
                            attempt: attempt_id.clone(),
                        },
                        context: Box::default(),
                    },
                );
            }
            require(
                pending.state == DecisionState::Ready && pending.purpose == Purpose::Assign,
                "并行分配结果或应用待核对，不会重发",
            )?;
            let response = self.management.original_envelope(&pending.operation_id)?;
            let ManagementDecision::Assign { task_id, .. } = &response.decision else {
                return Err("并行副指挥未给出任务分配，请审阅原响应后处理".into());
            };
            let plan = snapshot.revision.as_ref().ok_or("方案缺失")?;
            require(
                api_assignment_scope::isolated(plan, task_id)?,
                "并行分配必须选择批准的独立API、OMP或Codex任务",
            )?;
            self.management_envelope(&pending.operation_id)?;
            let attempt = format!("attempt-{}", &digest(&pending.operation_id)?[..32]);
            if !self.worker_slot_available()
                || self
                    .task_resource_conflict(project, task_id, &attempt)?
                    .is_some()
            {
                return Ok(());
            }
            return if plan.task(task_id)?.adapter_id == "omp" {
                self.start_managed_omp(&pending.operation_id, &attempt)
            } else if plan.task(task_id)?.adapter_id == "codex" {
                self.start_managed_codex(&pending.operation_id, &attempt)
            } else {
                self.start_managed_api(&pending.operation_id, &attempt)
            };
        }
        if self.advance_shared_verification(run, &snapshot)? {
            return Ok(());
        }
        if snapshot.state.tasks.iter().any(|(task, state)| {
            state.state == TaskState::Submitted
                && state.verified
                && snapshot
                    .revision
                    .as_ref()
                    .is_some_and(|p| crate::acceptance_scope::isolated(p, task).unwrap_or(false))
        }) {
            let setup = &self.project_setups[project].setup;
            let model = setup.chief_model.clone();
            let requirement = setup.requirement.clone();
            let operation = format!("{}-{}", run.id, self.management_operation_total() + 1);
            return self.start_management_decision(
                project,
                &operation,
                Purpose::Accept,
                &model,
                &requirement,
            );
        }
        if !self.worker_slot_available() {
            return Ok(());
        }
        let plan = snapshot.revision.as_ref().ok_or("方案缺失")?;
        let operation = format!("{}-{}", run.id, self.management_operation_total() + 1);
        let attempt = format!("attempt-{}", &digest(&operation)?[..32]);
        let mut candidates = Vec::new();
        for task in &plan.workflow.tasks {
            if snapshot
                .state
                .tasks
                .get(&task.id)
                .is_some_and(|t| t.state == TaskState::Pending && t.attempt.is_none())
                && api_assignment_scope::isolated(plan, &task.id)?
                && task.dependencies.iter().all(|d| {
                    snapshot
                        .state
                        .tasks
                        .get(d)
                        .is_some_and(|t| t.state == TaskState::Accepted)
                })
                && self
                    .task_resource_conflict(project, &task.id, &attempt)?
                    .is_none()
            {
                candidates.push(task.id.clone());
            }
        }
        if candidates.is_empty() {
            return Ok(());
        }
        let reader = self.api_read(project, &candidates[0], &attempt)?;
        if crate::api_execution::prior_requests(&*reader)? >= plan.policy.max_requests {
            // A running attempt may still seal fewer rounds than its reservation.
            // Wait for authoritative completion instead of billing a futile decision.
            return Ok(());
        }
        drop(reader);
        self.api_run_credentials(project)?;
        let setup = self.project_setups[project].setup.clone();
        let requirement = serde_json::json!({
            "parallelAssignment": true,
            "taskCandidates": candidates,
            "instruction": "仅分配候选中的一个任务；原始需求见executionSetup.setup.requirement，遵循已批准方案。其他任务仍在执行；不要重复分配、验收或修改方案。"
        }).to_string();
        self.start_management_decision(
            project,
            &operation,
            Purpose::Assign,
            &setup.deputy_model,
            &requirement,
        )
    }
}
