//! Only the original OMP owner can publish a live observation. No action permit.
use super::*;
use crate::omp_tool_checkpoint::OmpToolCheckpoint;
use cerulean_domain::omp_jobs::OmpJobState;

impl RuntimeHost {
    pub(super) fn accept_omp_tool_checkpoint(
        &mut self,
        project: &str,
        attempt: &str,
        checkpoint: OmpToolCheckpoint,
    ) -> Result<(), String> {
        let active = self
            .omp_active
            .get(&(project.into(), attempt.into()))
            .ok_or("Original OMP owner missing")?;
        let intent = &checkpoint.intent;
        require(
            !active.disconnected
                && active.job.state == OmpJobState::Executing
                && active.cancel.is_some()
                && !active
                    .preparation_cancel
                    .load(std::sync::atomic::Ordering::Acquire)
                && active.job.project_id == project
                && active.job.task_id == intent.task_id
                && active.job.attempt_id == attempt
                && intent.attempt_id == attempt,
            "OMP tool checkpoint has no original active owner",
        )?;
        checkpoint.tools.matches_scope(intent)?;
        let store = self.omp_read(project, &intent.task_id, attempt)?;
        let plan = store.revision().ok_or("Original OMP plan missing")?;
        checkpoint.evidence().validate_plan(plan)?;
        let approval = store.approval().ok_or("Original OMP approval missing")?;
        approval.validate(plan, &approval.user_id, plan.policy.version)?;
        let original = store
            .attempt_record(&intent.task_id, attempt)
            .ok_or("Original OMP attempt missing")?;
        let state = store
            .state()
            .tasks
            .get(&intent.task_id)
            .ok_or("Original OMP task missing")?;
        require(
            original.binding == intent.binding
                && original.fence == intent.fence
                && original.revision_digest == intent.revision_digest
                && store.state().approved
                && store.state().revision_requested.is_none()
                && state.state == TaskState::Running
                && state.attempt.as_deref() == Some(attempt)
                && state.cancellation_requested != Some(true),
            "OMP observation scope no longer current",
        )?;
        if let Some(previous) = &active.tool_checkpoint {
            require(
                previous.intent == *intent
                    && previous.tools.observation().calls.iter().all(|(id, call)| {
                        checkpoint.tools.observation().calls.get(id) == Some(call)
                    }),
                "OMP completed tool prefix changed",
            )?;
        }
        drop(store);
        self.omp_active
            .get_mut(&(project.into(), attempt.into()))
            .unwrap()
            .tool_checkpoint = Some(checkpoint);
        Ok(())
    }
}
