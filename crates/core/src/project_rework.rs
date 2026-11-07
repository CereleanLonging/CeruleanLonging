//! Return original checker feedback to the configured deputy before redispatch.
use super::*;
use crate::management_execution::ApplicationTarget;
use cerulean_domain::management::{ManagementDecision, Purpose};

impl RuntimeHost {
    pub(super) fn rework_feedback(
        &self,
        project: &str,
    ) -> Result<Option<serde_json::Value>, String> {
        let store = self.projects.get(project).ok_or("返工项目账本暂不可用")?;
        if store.state().tasks.values().any(|t| {
            matches!(
                t.state,
                TaskState::Assigned
                    | TaskState::Dispatching
                    | TaskState::Running
                    | TaskState::Cancelling
                    | TaskState::Unknown
            )
        }) {
            return Ok(None);
        }
        for (task, state) in &store.state().tasks {
            if state.state != TaskState::Submitted || state.verified {
                continue;
            }
            let attempt = state.attempt.as_deref().ok_or("返工尝试缺失")?;
            let Some(entry) = store.history().iter().rev().find(|e| {
                matches!(
                    &e.body.event.command, WorkflowCommand::Verify { task: t, attempt: a, .. }
                        if t == task && a == attempt
                )
            }) else {
                continue;
            };
            require(
                matches!(
                    entry.body.event.command,
                    WorkflowCommand::Verify { passed: false, .. }
                ),
                "返工检查状态不符",
            )?;
            let job = self
                .jobs
                .get(&(project.into(), entry.body.operation_id.clone()))
                .ok_or("原独立检查记录缺失")?;
            require(
                job.state == VerificationState::Failed,
                "原检查仍需对账，暂不能返工",
            )?;
            let basis = crate::verification_detail::basis(store, job)?;
            let (detail, report) = crate::verification_detail::inspect(&self.root, job, &basis)?;
            require(detail.ledger_recorded, "原失败检查尚未入账")?;
            let report = report.ok_or("原检查缺少终态报告，暂不能返工")?;
            let check = cerulean_domain::independent_checks::inspect(
                project,
                &job.operation_id,
                &basis.manifest,
                basis.approved_checks(task)?,
                &report,
            )?;
            let missing = basis
                .plan
                .task(task)?
                .artifact_paths
                .iter()
                .filter(|path| !basis.manifest.artifacts.iter().any(|a| &a.path == *path))
                .cloned()
                .collect::<Vec<_>>();
            require(
                !missing.is_empty() || check.checks.iter().any(|c| !c.passed),
                "返工缺少具体失败依据",
            )?;
            let requires_revision = basis.approved_checks(task)? != basis.plan.task(task)?.checks;
            return Ok(Some(serde_json::json!({
                "taskId":task,"attemptId":attempt,"verificationOperation":entry.body.operation_id,
                "requiresPlanRevision":requires_revision,
                "reportDigest":digest(&report)?,"missingArtifacts":missing,"report":report,"checks":check.checks,
                "rule":"Return Advise for this exact task with concrete source corrections under the approved task and directory. A new attempt will be assigned and independently checked. Never change tests, acceptance criteria, permissions or scope. If requiresPlanRevision is true or the checker or plan needs revision, return Wait explaining the issue for chief/user review."
            })));
        }
        Ok(None)
    }

    pub(super) fn add_rework_context(
        &self,
        mut context: serde_json::Value,
        purpose: Purpose,
    ) -> Result<serde_json::Value, String> {
        if purpose == Purpose::Advise {
            let project = context["workflow"]["project"]["id"]
                .as_str()
                .ok_or("返工项目缺失")?;
            if let Some(feedback) = self.rework_feedback(project)? {
                context["rework"] = feedback;
            }
        }
        Ok(context)
    }

    pub(super) fn apply_rework_decision(
        &mut self,
        operation: &str,
        decision: ManagementDecision,
    ) -> Result<(), String> {
        let ManagementDecision::Advise {
            task_id,
            suggestion,
            ..
        } = decision
        else {
            return Err("返工需要副指挥的具体修正决定".into());
        };
        let intent = self.management.intent(operation)?;
        let feedback = self
            .rework_feedback(&intent.project_id)?
            .ok_or("返工依据已变化")?;
        require(
            intent.context.get("rework") == Some(&feedback) && feedback["taskId"] == task_id,
            "副指挥返工决定与原检查反馈不符",
        )?;
        require(
            feedback["requiresPlanRevision"] == false,
            "检查标准已复验修订，请先由主指挥修订方案再返工",
        )?;
        let field = |name: &str| {
            feedback[name]
                .as_str()
                .map(str::to_owned)
                .ok_or("返工关联缺失".to_string())
        };
        self.apply_management_decision_inner(
            operation,
            ApplicationTarget::Workflow {
                principal: "deputy".into(),
                command: WorkflowCommand::ReturnForRework {
                    task: task_id,
                    attempt: field("attemptId")?,
                    verification_operation: field("verificationOperation")?,
                    report_digest: field("reportDigest")?,
                    suggestion,
                },
                context: Box::default(),
            },
        )
    }
}
