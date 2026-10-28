//! OMP tool evidence uses the original management/counting path. This stage
//! observes and applies advice; historical observations never authorize steering.
use super::*;
use crate::{
    management_execution::{DecisionIntent, DecisionState},
    omp_tool_checkpoint::{OmpSupervisionReference, OmpToolEvidence},
};
use cerulean_domain::{
    management::{ManagementDecision, Purpose},
    project_setup::{ProjectRun, RunState},
    supervision::{OmpAdviceReference, SupervisionAdviceView},
};
use serde::{Deserialize, Serialize};
#[path = "omp_supervision_delivery.rs"]
mod delivery;
#[cfg(test)]
#[path = "omp_supervision_tests.rs"]
mod tests;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Binding {
    version: u32,
    kind: String,
    run_id: String,
    run_digest: String,
    pub(super) task_id: String,
    attempt_id: String,
    incident_id: String,
    observation_digest: String,
}
pub(super) fn binding(intent: &DecisionIntent) -> Result<Option<Binding>, String> {
    let Some(raw) = intent.context.get("supervision") else {
        return Ok(None);
    };
    if raw["version"] != 4 {
        return Ok(None);
    }
    let b: Binding =
        serde_json::from_value(raw.clone()).map_err(|_| "Invalid OMP supervision binding")?;
    require(
        b.kind == "omp" && b.version == 4 && intent.purpose == Purpose::Advise,
        "OMP supervision purpose changed",
    )?;
    for id in [&b.run_id, &b.task_id, &b.attempt_id, &b.incident_id] {
        identifier(id)?;
    }
    sha256(&b.run_digest)?;
    sha256(&b.observation_digest)?;
    Ok(Some(b))
}
fn evidence(intent: &DecisionIntent, b: &Binding) -> Result<OmpSupervisionReference, String> {
    let e: OmpSupervisionReference =
        serde_json::from_value(intent.context["supervisionEvidence"].clone())
            .map_err(|_| "Original OMP tool evidence missing")?;
    require(
        e.intent().task_id == b.task_id
            && e.intent().attempt_id == b.attempt_id
            && digest(&e)? == b.observation_digest,
        "OMP supervision evidence changed",
    )?;
    Ok(e)
}
impl RuntimeHost {
    pub(super) fn poll_omp_supervision(&mut self, run: &ProjectRun) -> Result<(), String> {
        let attempts: Vec<_> = self
            .omp_active
            .iter()
            .filter(|((p, _), a)| {
                p == &run.project_id
                    && a.job.state == omp_jobs::OmpJobState::Executing
                    && a.tool_checkpoint.is_some()
            })
            .map(|((_, a), _)| a.clone())
            .collect();
        for attempt in attempts {
            self.request_omp_supervision(&run.project_id, &attempt)?;
            self.poll_omp_advice_delivery(&run.project_id, &attempt)?;
        }
        Ok(())
    }
    fn omp_supervision_context(
        &self,
        run: &ProjectRun,
        e: &OmpToolEvidence,
        excluded: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        self.run_scope(run)?;
        require(
            run.state == RunState::Active && self.runs.all().iter().any(|r| r == run),
            "Original OMP run is not active",
        )?;
        let i = e.intent();
        let active = self
            .omp_active
            .get(&(run.project_id.clone(), i.attempt_id.clone()))
            .ok_or("Original OMP owner missing")?;
        require(
            !active.disconnected
                && active.cancel.is_some()
                && active.job.state == omp_jobs::OmpJobState::Executing
                && !active
                    .preparation_cancel
                    .load(std::sync::atomic::Ordering::Acquire)
                && active
                    .tool_checkpoint
                    .as_ref()
                    .is_some_and(|c| c.evidence() == *e),
            "Original OMP observation no longer active",
        )?;
        let store = self.omp_read(&run.project_id, &i.task_id, &i.attempt_id)?;
        let plan = store.revision().ok_or("Original OMP plan missing")?;
        e.validate_plan(plan)?;
        let attempt = store
            .attempt_record(&i.task_id, &i.attempt_id)
            .ok_or("Original OMP attempt missing")?;
        let state = store
            .state()
            .tasks
            .get(&i.task_id)
            .ok_or("Original OMP task missing")?;
        require(
            store.state().approved
                && store.state().revision_requested.is_none()
                && state.state == TaskState::Running
                && state.attempt.as_deref() == Some(&i.attempt_id)
                && state.cancellation_requested != Some(true)
                && attempt.binding == i.binding
                && attempt.fence == i.fence
                && attempt.revision_digest == i.revision_digest
                && run.revision_digest == i.revision_digest,
            "OMP original execution scope changed",
        )?;
        let approval = store.approval().ok_or("OMP approval missing")?;
        approval.validate(plan, &approval.user_id, plan.policy.version)?;
        let saved = self
            .project_setups
            .get(&run.project_id)
            .ok_or("Original OMP setup missing")?;
        let policy = plan
            .supervision_for(&i.task_id)?
            .ok_or("OMP supervision not approved")?;
        require(
            policy.max_advice_per_attempt > 0
                && saved.digest == run.setup_digest
                && saved.setup.policy == plan.policy
                && saved
                    .setup
                    .supervision_for_execution(plan.task(&i.task_id)?)?
                    == Some(policy),
            "OMP advice recording was not approved",
        )?;
        require(
            !policy.delivery_enabled
                || active
                    .advice_waiter
                    .as_ref()
                    .is_some_and(|w| !w.is_closed()),
            "OMP advice requires original live held tool result",
        )?;
        let elapsed = chrono::Utc::now().signed_duration_since(
            chrono::DateTime::parse_from_rfc3339(&active.job.started_at)
                .map_err(|_| "Invalid OMP start time")?,
        );
        require(
            elapsed.num_milliseconds() >= 0
                && elapsed.num_seconds() < i64::from(plan.policy.max_seconds),
            "OMP supervision expired",
        )?;
        let mut intents = self.management_original_intents(&run.project_id);
        if let Some(id) = excluded {
            intents.remove(id);
        }
        require(
            intents.len() < saved.setup.management_max_requests as usize,
            "OMP management request budget exhausted",
        )?;
        require(
            intents
                .values()
                .filter(|r| {
                    r.purpose == Purpose::Advise
                        && r.context["supervision"]["attemptId"] == i.attempt_id
                })
                .count()
                < policy.max_advice_per_attempt as usize,
            "OMP original attempt advice budget exhausted",
        )?;
        let snapshot = self
            .api_writers
            .get(&run.project_id)
            .ok_or("Original OMP coordinator missing")?
            .owner
            .snapshot()?;
        Ok(self.context_for_snapshot(&saved.setup.requirement, &snapshot))
    }
    pub(super) fn request_omp_supervision(
        &mut self,
        project: &str,
        attempt: &str,
    ) -> Result<bool, String> {
        let active = self
            .omp_active
            .get(&(project.into(), attempt.into()))
            .ok_or("Original OMP owner missing")?;
        let plan = active
            .snapshot
            .revision
            .as_ref()
            .ok_or("OMP plan missing")?;
        let Some(policy) = plan
            .supervision_for(&active.job.task_id)?
            .filter(|p| p.max_advice_per_attempt > 0)
        else {
            return Ok(false);
        };

        let Some(checkpoint) = active.tool_checkpoint.as_ref() else {
            return Ok(false);
        };
        let incidents = checkpoint.tools.supervision_incidents(policy)?;
        let operations = self.management_operation_ids(project);
        let Some(incident) = incidents
            .iter()
            .find(|i| !operations.contains(&format!("advice-{}", &i.evidence_digest[..48])))
        else {
            return Ok(false);
        };
        if self
            .management
            .statuses()
            .iter()
            .any(|s| s.project_id == project && !s.state.resolved())
            || self.counting_project(project)
            || self.management_work_count() >= 4
            || self.management_probe_in_progress(project, Purpose::Advise)
        {
            return Ok(false);
        }
        let Some(run) = self
            .runs
            .all()
            .into_iter()
            .find(|r| r.project_id == project && r.state == RunState::Active)
        else {
            return Ok(false);
        };
        let e = checkpoint.evidence();
        let mut context = self.omp_supervision_context(&run, &e, None)?;
        self.require_management_connection(project, Purpose::Advise)?;
        let reference = OmpSupervisionReference::from_evidence(&e, policy, &incident.id)?;
        let b = Binding {
            version: 4,
            kind: "omp".into(),
            run_id: run.id.clone(),
            run_digest: digest(&run)?,
            task_id: e.intent().task_id.clone(),
            attempt_id: attempt.into(),
            incident_id: incident.id.clone(),
            observation_digest: digest(&reference)?,
        };
        context["supervision"] = serde_json::to_value(&b).map_err(|e| e.to_string())?;
        context["supervisionEvidence"] =
            serde_json::to_value(reference).map_err(|e| e.to_string())?;
        self.launch_routed_management(DecisionIntent {
            operation_id: format!("advice-{}", &incident.evidence_digest[..48]),
            project_id: project.into(),
            purpose: Purpose::Advise,
            model: self.project_setups[project].setup.deputy_model.clone(),
            state_digest: digest(&context)?,
            context,
            created_at: chrono::Utc::now().to_rfc3339(),
        })?;
        Ok(true)
    }
    pub(super) fn revalidate_omp_supervision(&self, intent: &DecisionIntent) -> Result<(), String> {
        intent.validate()?;
        let b = binding(intent)?.ok_or("OMP advice binding missing")?;
        let e = evidence(intent, &b)?;
        let run = self
            .runs
            .all()
            .into_iter()
            .find(|r| r.id == b.run_id && r.project_id == intent.project_id)
            .ok_or("Original OMP advice run missing")?;
        require(
            digest(&run)? == b.run_digest,
            "Original OMP run authority changed",
        )?;
        let policy = super::supervision_requests::request_policy(intent)?;
        let current = self
            .omp_active
            .get(&(intent.project_id.clone(), b.attempt_id.clone()))
            .and_then(|a| a.tool_checkpoint.as_ref())
            .ok_or("Original OMP checkpoint missing")?;
        e.validate_live(&current.evidence(), &policy, &b.incident_id)?;
        require(
            current
                .tools
                .supervision_incidents(&policy)?
                .iter()
                .any(|i| {
                    i.id == b.incident_id
                        && intent.operation_id == format!("advice-{}", &i.evidence_digest[..48])
                }),
            "OMP advice incident changed",
        )?;
        let mut context =
            self.omp_supervision_context(&run, &current.evidence(), Some(&intent.operation_id))?;
        context["supervision"] = intent.context["supervision"].clone();
        context["supervisionEvidence"] = intent.context["supervisionEvidence"].clone();
        require(
            digest(&self.match_original_catalog(&intent.context, context)?)? == intent.state_digest
                && self.project_setups[&intent.project_id].setup.deputy_model == intent.model,
            "OMP advice context changed during counting",
        )
    }
    pub(super) fn validate_omp_supervision_requests(&self) -> Result<(), String> {
        for project in self.projects.keys() {
            for intent in self.management_original_intents(project).values() {
                let Some(b) = binding(intent)? else { continue };
                intent.validate()?;
                let e = evidence(intent, &b)?;
                let store = &self.projects[project];
                let original: ExecutionSnapshot =
                    serde_json::from_value(intent.context["workflow"].clone())
                        .map_err(|_| "OMP original snapshot missing")?;
                require(
                    digest(&crate::workflow_prefix::prefix(store, original.sequence)?)?
                        == digest(&original)?,
                    "OMP advice snapshot is not original history",
                )?;
                let plan = original
                    .revision
                    .as_ref()
                    .ok_or("OMP original plan missing")?;
                e.validate_plan(plan)?;
                let approval = original
                    .approval
                    .as_ref()
                    .ok_or("OMP original approval missing")?;
                approval.validate(plan, &approval.user_id, plan.policy.version)?;
                let task = original
                    .state
                    .tasks
                    .get(&b.task_id)
                    .ok_or("OMP original task missing")?;
                require(
                    original.project.id == *project
                        && original.state.approved
                        && original.state.revision_requested.is_none()
                        && task.state == TaskState::Running
                        && task.attempt.as_deref() == Some(&b.attempt_id)
                        && task.cancellation_requested != Some(true),
                    "OMP original advice not running",
                )?;
                let attempt = store
                    .attempt_record(&b.task_id, &b.attempt_id)
                    .ok_or("OMP original attempt missing")?;
                require(store.history().iter().take(original.sequence as usize).any(|entry|
                    matches!(&entry.body.event.command, WorkflowCommand::Started { task, attempt } if task == &b.task_id && attempt == &b.attempt_id)
                    && entry.body.context.turn_id.is_none()
                    && entry.body.context.thread_id.as_deref() == Some(&e.intent().binding.session_id)),
                    "Original OMP Started evidence missing")?;
                require(
                    attempt.binding == e.intent().binding
                        && attempt.fence == e.intent().fence
                        && attempt.revision_digest == e.intent().revision_digest,
                    "OMP historical advice scope changed",
                )?;
                let policy = super::supervision_requests::request_policy(intent)?;
                let setup: project_setup::SavedProjectSetup =
                    serde_json::from_value(intent.context["executionSetup"].clone())
                        .map_err(|_| "OMP original setup missing")?;
                require(
                    policy.max_advice_per_attempt > 0
                        && setup.setup.deputy_model == intent.model
                        && self.runs.has_authorization(&b.run_id, &b.run_digest)
                        && self.runs.all().iter().any(|r| {
                            r.id == b.run_id
                                && r.project_id == *project
                                && r.setup_digest == setup.digest
                                && r.revision_digest == e.intent().revision_digest
                        }),
                    "OMP original run authorization missing",
                )?;
                let tools = e.verify_original(
                    &store.omp_file_service_path(&b.attempt_id)?,
                    plan,
                    &policy,
                    &b.incident_id,
                )?;
                self.omp_advice_delivery_state(intent)?;
                require(
                    tools.incidents(&policy)?.iter().any(|i| {
                        i.id == b.incident_id
                            && intent.operation_id == format!("advice-{}", &i.evidence_digest[..48])
                    }),
                    "OMP historical incident missing",
                )?;
            }
        }
        Ok(())
    }
    pub(super) fn omp_supervision_pending(&self, run: &ProjectRun) -> Result<bool, String> {
        for status in self
            .management_decisions()
            .into_iter()
            .filter(|s| s.project_id == run.project_id && !s.state.resolved())
        {
            if binding(self.management.intent(&status.operation_id)?)?.is_some() {
                if self.has_omp_activity(&run.project_id) {
                    return Ok(true);
                }
                require(
                    status.state == DecisionState::Running,
                    "OMP监督建议已保存或结果待核对；请审阅原指挥记录，不会自动投送或重复请求",
                )?;
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub(super) fn omp_supervision_advice_view(
        &self,
        intent: &DecisionIntent,
    ) -> Result<SupervisionAdviceView, String> {
        let b = binding(intent)?.ok_or("OMP advice binding missing")?;
        let e = evidence(intent, &b)?;
        let (suggestion, rationale) = match self
            .management
            .original_envelope(&intent.operation_id)
            .map(|o| &o.decision)
        {
            Ok(ManagementDecision::Advise {
                task_id,
                suggestion,
                rationale,
            }) if task_id == &b.task_id => (Some(suggestion.clone()), Some(rationale.clone())),
            _ => (None, None),
        };
        Ok(SupervisionAdviceView {
            api: None,
            omp: Some(OmpAdviceReference {
                request_id: e.intent().validate_metadata()?.1,
                tool_head: e.tool_head().into(),
            }),
            incident_id: b.incident_id,
            task_id: b.task_id,
            attempt_id: b.attempt_id,
            thread_id: String::new(),
            turn_id: String::new(),
            observation_digest: b.observation_digest,
            suggestion,
            rationale,
            current: self.revalidate_omp_supervision(intent).is_ok(),
            delivered: self.omp_advice_delivery_state(intent)? == "messageObserved",
            delivery_state: Some(self.omp_advice_delivery_state(intent)?.into()),
        })
    }
}
