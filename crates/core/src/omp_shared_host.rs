//! OMP attempts join the original resident-owned journal registry.
use super::*;
use crate::{omp_workflow_access::OmpWorkflowAccess, workflow_coordinator::WorkflowClient};

pub(super) enum OmpRead<'a> {
    Idle(&'a DurableWorkflow),
    Active(Box<WorkflowClient>),
}
impl std::ops::Deref for OmpRead<'_> {
    type Target = dyn OmpWorkflowAccess;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Idle(store) => *store,
            Self::Active(client) => client.as_ref(),
        }
    }
}
impl RuntimeHost {
    pub(super) fn shared_assignment_anchor(
        &self,
        project: &str,
        attempt: &str,
        adapter: &str,
    ) -> Result<Option<(u32, String)>, String> {
        if adapter == "codex" {
            // Legacy Codex managed records predate StartupJournal; their original
            // exact-snapshot bridge remains valid and never gains shared scope.
            let Some(record) = self.startups.get(project, attempt) else {
                return Ok(None);
            };
            if record.intent.schema_version == 2 {
                return Ok(Some((
                    record.intent.workflow_sequence,
                    record
                        .intent
                        .workflow_hash
                        .clone()
                        .ok_or("Codex历史锚点缺失")?,
                )));
            }
        } else if adapter == "omp" {
            let record = self
                .omp_startups
                .get(project, attempt)
                .ok_or("OMP启动证据缺失")?;
            if record.intent.schema_version == 2 {
                return Ok(Some((
                    record.intent.workflow_sequence,
                    record
                        .intent
                        .workflow_hash
                        .clone()
                        .ok_or("OMP历史锚点缺失")?,
                )));
            }
        } else if cerulean_domain::api_worker::is_api_adapter(adapter) {
            let startup = self
                .api_startups
                .get(&(project.into(), attempt.into()))
                .ok_or("API启动证据缺失")?;
            if startup.version == 2 {
                return Ok(Some((
                    startup.workflow_sequence,
                    startup.workflow_hash.clone(),
                )));
            }
        }
        Ok(None)
    }
    pub(super) fn has_omp_activity(&self, project: &str) -> bool {
        self.omp_active.keys().any(|(p, _)| p == project)
    }
    pub(super) fn omp_read(
        &self,
        project: &str,
        task: &str,
        attempt: &str,
    ) -> Result<OmpRead<'_>, String> {
        if let Some(writer) = self.api_writers.get(project) {
            return Ok(OmpRead::Active(Box::new(
                writer
                    .owner
                    .omp_client(crate::workflow_coordinator::Scope {
                        project: project.into(),
                        task: task.into(),
                        attempt: attempt.into(),
                        revision: writer
                            .snapshot
                            .revision_digest
                            .clone()
                            .ok_or("OMP批准摘要缺失")?,
                    })?,
            )));
        }
        let store = self.projects.get(project).ok_or("OMP项目不可用")?;
        require(!store.requires_reopen(), "OMP项目需要重开核对")?;
        Ok(OmpRead::Idle(store))
    }
    pub(super) fn finish_omp_owner(
        &mut self,
        project: &str,
        job: &mut omp_jobs::OmpJob,
        interrupted: bool,
        outcome: Result<(), String>,
    ) -> Result<(), String> {
        let writer = self.api_writers.get_mut(project).ok_or("OMP共享账本缺失")?;
        writer.interrupted |= interrupted;
        let remaining = self.api_active.keys().any(|(p, _)| p == project)
            || self.has_omp_activity(project)
            || self.has_codex_activity(project)
            || self.active.get(project).is_some_and(|a| a.shared);
        let record = self
            .omp_startups
            .get(project, &job.attempt_id)
            .ok_or("OMP启动证据缺失")?;
        let classify = |store: &dyn crate::workflow_access::WorkflowAccess| {
            if outcome.is_err()
                && !store
                    .history()
                    .iter()
                    .any(|e| e.body.event.command.attempt() == Some(job.attempt_id.as_str()))
            {
                omp_jobs::OmpJobState::Failed
            } else {
                crate::async_omp::observed_state(&record.intent, store)
            }
        };
        if remaining {
            let writer = self.api_writers.get_mut(project).ok_or("OMP共享账本缺失")?;
            writer.refresh()?;
            let reader = writer
                .owner
                .omp_client(crate::workflow_coordinator::Scope {
                    project: project.into(),
                    task: job.task_id.clone(),
                    attempt: job.attempt_id.clone(),
                    revision: job.revision_digest.clone(),
                })?;
            job.state = classify(&reader);
        } else {
            let mut writer = self.api_writers.remove(project).ok_or("OMP共享账本缺失")?;
            let store = writer.owner.finish(writer.interrupted)?;
            job.state = classify(&store);
            self.projects.insert(project.into(), store);
        }
        job.message = outcome.err();
        Ok(())
    }
}
