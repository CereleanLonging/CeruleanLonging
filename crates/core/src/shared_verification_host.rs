//! Independent checks can share one journal owner with isolated API attempts.
use super::*;

impl RuntimeHost {
    pub(super) fn advance_shared_verification(
        &mut self,
        run: &cerulean_domain::project_setup::ProjectRun,
        snapshot: &ExecutionSnapshot,
    ) -> Result<bool, String> {
        if self.active.contains_key(&run.project_id)
            || self.active.len()
                + self.api_active.len()
                + self.codex_active.len()
                + self.omp_active.len()
                + usize::from(self.desktop_active.is_some())
                >= 4
        {
            return Ok(false);
        }
        for (task, state) in &snapshot.state.tasks {
            let Some(attempt) = state.attempt.as_deref() else {
                continue;
            };
            if state.state != TaskState::Submitted
                || state.verified
                || self
                    .api_active
                    .contains_key(&(run.project_id.clone(), attempt.into()))
                || self
                    .omp_active
                    .contains_key(&(run.project_id.clone(), attempt.into()))
                || self
                    .codex_active
                    .contains_key(&(run.project_id.clone(), attempt.into()))
            {
                continue;
            }
            require(
                !self
                    .jobs
                    .values()
                    .any(|j| j.project_id == run.project_id && j.attempt_id == attempt),
                "本次独立验收已有记录，不会自动重跑",
            )?;
            let pins = snapshot
                .revision
                .as_ref()
                .ok_or("原验收方案缺失")?
                .task(task)?
                .checks
                .iter()
                .map(crate::verifier::PinnedCheck::from_approved)
                .collect::<Result<Vec<_>, _>>()?;
            let operation = format!("verify-{}", &digest(&(run.id.as_str(), attempt))?[..32]);
            self.start_verification(&run.project_id, &operation, task, attempt, pins)?;
            return Ok(true);
        }
        Ok(false)
    }
    pub(super) fn require_shared_verification_scope(
        &self,
        project: &str,
        task: &str,
        attempt: &str,
    ) -> Result<(), String> {
        require(
            !self
                .api_active
                .contains_key(&(project.into(), attempt.into()))
                && !self
                    .omp_active
                    .contains_key(&(project.into(), attempt.into()))
                && !self
                    .codex_active
                    .contains_key(&(project.into(), attempt.into())),
            "原执行线程尚未退出，暂不能开始验收",
        )?;
        let snapshot = self
            .api_writers
            .get(project)
            .ok_or("共享账本缺失")?
            .owner
            .snapshot()?;
        let plan = snapshot.revision.as_ref().ok_or("原批准方案缺失")?;
        require(
            plan.worktree.is_some()
                && plan.workflow.tasks.iter().any(|t| {
                    t.id == task
                        && isolated_task_key(project, task).is_ok_and(|key| key == t.workspace_key)
                }),
            "并行验收需要批准的独立任务目录",
        )
    }
    pub(super) fn launch_shared_verification(
        &mut self,
        job: VerificationJob,
        pins: Vec<crate::verifier::PinnedCheck>,
    ) -> Result<(), String> {
        let project = job.project_id.clone();
        let launched = (|| {
            let writer = self.api_writers.get(&project).ok_or("共享账本缺失")?;
            let client = writer.owner.verification_client(
                crate::workflow_coordinator::Scope {
                    project: project.clone(),
                    task: job.task_id.clone(),
                    attempt: job.attempt_id.clone(),
                    revision: writer
                        .snapshot
                        .revision_digest
                        .clone()
                        .ok_or("原批准摘要缺失")?,
                },
                job.clone(),
            )?;
            crate::async_verification::launch_shared(&self.root, client, job.clone(), pins)
        })();
        match launched {
            Ok(active) => {
                self.active.insert(project, active);
                Ok(())
            }
            Err(error) => {
                let mut job = job;
                job.state = VerificationState::Unknown;
                job.message = Some(error.clone());
                if let Err(save) = crate::async_verification::save(&self.root, &job) {
                    self.api_fault = Some(format!("独立验收启动失败，状态保存也失败: {save}"));
                }
                self.jobs.insert((project, job.operation_id.clone()), job);
                Err(error)
            }
        }
    }
    pub(super) fn finish_shared_verification(
        &mut self,
        project: &str,
        active: crate::async_verification::ActiveVerification,
        completion: crate::async_verification::Completion,
    ) {
        let mut job = active.job.clone();
        job.updated_at = chrono::Utc::now().to_rfc3339();
        job.state = if completion.uncertain {
            VerificationState::Unknown
        } else {
            match &completion.outcome {
                Ok(true) => VerificationState::Succeeded,
                Ok(false) | Err(_) => VerificationState::Failed,
            }
        };
        job.message = completion.outcome.err();
        let owner = (|| {
            let remaining = self.api_active.keys().any(|(p, _)| p == project)
                || self.has_omp_activity(project)
                || self.has_codex_activity(project);
            let writer = self
                .api_writers
                .get_mut(project)
                .ok_or("独立验收失去共享账本")?;
            writer.interrupted |= completion.uncertain;
            if remaining {
                writer.refresh()?;
            } else {
                let mut writer = self.api_writers.remove(project).ok_or("共享账本缺失")?;
                let store = writer.owner.finish(writer.interrupted)?;
                self.projects.insert(project.into(), store);
            }
            Ok::<_, String>(())
        })();
        if let Err(error) = owner {
            self.api_fault = Some(error.clone());
            job.state = VerificationState::Unknown;
            job.message = Some(error);
        }
        if let Err(error) = crate::async_verification::save(&self.root, &job) {
            job.state = VerificationState::Unknown;
            job.message = Some(format!("验收状态保存失败，需要对账: {error}"));
        }
        self.jobs
            .insert((project.into(), job.operation_id.clone()), job);
    }
}
