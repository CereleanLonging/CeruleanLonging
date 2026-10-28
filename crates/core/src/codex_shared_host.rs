//! Native Codex attempts use the resident registry shared with API/OMP/verifiers.
use super::*;
use crate::{codex_workflow_access::CodexWorkflowAccess, workflow_coordinator::WorkflowClient};

pub(super) enum CodexRead<'a> {
    Idle(&'a DurableWorkflow),
    Active(Box<WorkflowClient>),
}
impl std::ops::Deref for CodexRead<'_> {
    type Target = dyn CodexWorkflowAccess;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Idle(store) => *store,
            Self::Active(client) => client.as_ref(),
        }
    }
}
impl RuntimeHost {
    pub(super) fn validate_shared_advice(
        &self,
        operation: &str,
        b: &supervision_requests::Binding,
        snapshot: &ExecutionSnapshot,
        entry: Option<&crate::scheduler::Entry>,
        live: bool,
    ) -> Result<(), String> {
        let intent = self.management.intent(operation)?;
        let original: ExecutionSnapshot =
            serde_json::from_value(intent.context["workflow"].clone())
                .map_err(|_| "监督原快照无效")?;
        require(
            original.project.id == intent.project_id && snapshot.project == original.project,
            "监督项目不符",
        )?;
        if live {
            let requirement = intent.context["requirement"]
                .as_str()
                .ok_or("监督需求缺失")?;
            let mut rebuilt = self.match_original_catalog(
                &intent.context,
                self.context_for_snapshot(requirement, &original),
            )?;
            rebuilt["supervision"] = intent.context["supervision"].clone();
            rebuilt["supervisionEvidence"] = intent.context["supervisionEvidence"].clone();
            require(
                digest(&rebuilt)? == intent.state_digest,
                "监督配置或用户答复变化",
            )?;
        }
        let view = self.codex_read(&intent.project_id, &b.task_id, &b.attempt_id)?;
        let store = &*view;
        require(
            digest(&crate::workflow_prefix::prefix(store, snapshot.sequence)?)?
                == digest(snapshot)?,
            "监督执行快照不是原账本前缀",
        )?;
        let through = if let Some(e) = entry {
            require(
                snapshot.sequence == e.body.event.sequence,
                "监督快照与事件序号不符",
            )?;
            e.body
                .event
                .sequence
                .checked_sub(1)
                .ok_or("监督事件序号无效")?
        } else {
            snapshot.sequence
        };
        crate::supervision_scope::validate(
            store,
            &original,
            &b.task_id,
            &b.attempt_id,
            &b.thread_id,
            &b.turn_id,
            through,
        )?;
        if live {
            crate::supervision_scope::validate(
                store,
                &original,
                &b.task_id,
                &b.attempt_id,
                &b.thread_id,
                &b.turn_id,
                store.history().len() as u32,
            )?;
        }
        Ok(())
    }
    pub(super) fn has_codex_activity(&self, project: &str) -> bool {
        self.codex_active.keys().any(|(p, _)| p == project)
    }
    pub(super) fn codex_read(
        &self,
        project: &str,
        task: &str,
        attempt: &str,
    ) -> Result<CodexRead<'_>, String> {
        if let Some(writer) = self.api_writers.get(project) {
            return Ok(CodexRead::Active(Box::new(
                writer
                    .owner
                    .codex_client(crate::workflow_coordinator::Scope {
                        project: project.into(),
                        task: task.into(),
                        attempt: attempt.into(),
                        revision: writer
                            .snapshot
                            .revision_digest
                            .clone()
                            .ok_or("Codex批准摘要缺失")?,
                    })?,
            )));
        }
        let store = self.projects.get(project).ok_or("Codex项目不可用")?;
        require(!store.requires_reopen(), "Codex项目需要重开核对")?;
        Ok(CodexRead::Idle(store))
    }
    pub(super) fn finish_codex_owner(
        &mut self,
        project: &str,
        job: &mut CodexJob,
        interrupted: bool,
        outcome: Result<(), String>,
    ) -> Result<(), String> {
        self.api_writers
            .get_mut(project)
            .ok_or("Codex共享账本缺失")?
            .interrupted |= interrupted;
        let remaining = self.api_active.keys().any(|(p, _)| p == project)
            || self.has_omp_activity(project)
            || self.has_codex_activity(project)
            || self.active.get(project).is_some_and(|a| a.shared);
        let classify = |store: &dyn crate::workflow_access::WorkflowAccess| {
            if outcome.is_err()
                && store
                    .attempt_record(&job.task_id, &job.attempt_id)
                    .is_none()
            {
                CodexJobState::Failed
            } else {
                crate::async_codex::observed_state(job, store)
            }
        };
        if remaining {
            let writer = self
                .api_writers
                .get_mut(project)
                .ok_or("Codex共享账本缺失")?;
            writer.refresh()?;
            let reader = writer
                .owner
                .codex_client(crate::workflow_coordinator::Scope {
                    project: project.into(),
                    task: job.task_id.clone(),
                    attempt: job.attempt_id.clone(),
                    revision: job.revision_digest.clone(),
                })?;
            job.state = classify(&reader);
        } else {
            let mut writer = self
                .api_writers
                .remove(project)
                .ok_or("Codex共享账本缺失")?;
            let store = writer.owner.finish(writer.interrupted)?;
            job.state = classify(&store);
            self.projects.insert(project.into(), store);
        }
        job.message = outcome.err();
        Ok(())
    }
}
