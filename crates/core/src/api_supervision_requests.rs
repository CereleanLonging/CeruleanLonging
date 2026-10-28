//! API-specific identity, original tool evidence, shared management transport and budget.
//! Recording advice never authorizes its application or delivery to the worker.
use super::*;
use crate::{
    management_execution::{DecisionIntent, DecisionState},
    ApiToolEvidence,
};
use cerulean_domain::{
    management::{ManagementDecision, Purpose},
    project_setup::{ProjectRun, RunState},
    supervision::{ApiAdviceReference, SupervisionAdviceView},
};
use serde::{Deserialize, Serialize};

#[path = "api_supervision_delivery.rs"]
mod delivery;
#[cfg(test)]
#[path = "api_supervision_tests.rs"]
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
    if raw["version"] != 3 {
        return Ok(None);
    }
    let b: Binding =
        serde_json::from_value(raw.clone()).map_err(|_| "Invalid API supervision binding")?;
    require(
        b.version == 3 && b.kind == "api" && intent.purpose == Purpose::Advise,
        "API supervision purpose changed",
    )?;
    for id in [&b.run_id, &b.task_id, &b.attempt_id, &b.incident_id] {
        identifier(id)?;
    }
    sha256(&b.run_digest)?;
    sha256(&b.observation_digest)?;
    Ok(Some(b))
}
fn evidence(intent: &DecisionIntent, bound: &Binding) -> Result<ApiToolEvidence, String> {
    let evidence: ApiToolEvidence =
        serde_json::from_value(intent.context["supervisionEvidence"].clone())
            .map_err(|_| "Original API supervision evidence missing")?;
    require(
        evidence.run().project_id == intent.project_id
            && evidence.run().task_id == bound.task_id
            && evidence.run().attempt_id == bound.attempt_id
            && digest(&evidence)? == bound.observation_digest,
        "API supervision evidence scope changed",
    )?;
    Ok(evidence)
}
impl RuntimeHost {
    pub(super) fn poll_api_supervision(&mut self, run: &ProjectRun) -> Result<(), String> {
        let attempts: Vec<_> = self
            .api_active
            .iter()
            .filter(|((project, _), active)| {
                project == &run.project_id
                    && active.job.state == cerulean_domain::api_jobs::ApiJobState::Executing
                    && active.tool_checkpoint.is_some()
            })
            .map(|((_, attempt), _)| attempt.clone())
            .collect();
        for attempt in attempts {
            // Revisit only local eligibility. Original operation IDs, including
            // count-only and discarded records, still forbid a second request.
            self.request_api_supervision(&run.project_id, &attempt)?;
            self.poll_api_advice_delivery(&run.project_id, &attempt)?;
        }
        Ok(())
    }
    fn api_supervision_context(
        &self,
        run: &ProjectRun,
        evidence: &ApiToolEvidence,
        excluded: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        self.run_scope(run)?;
        require(
            run.state == RunState::Active && self.runs.all().iter().any(|r| r == run),
            "Original API run is not active",
        )?;
        let identity = evidence.run();
        let plan = self.api_live_scope(&run.project_id, &identity.attempt_id, identity, false)?;
        let current = self
            .api_checkpoint_evidence(&run.project_id, &identity.attempt_id)?
            .ok_or("Original API checkpoint missing")?;
        require(
            &current == evidence && run.revision_digest == identity.revision_digest,
            "API tool observation changed",
        )?;
        let setup = self
            .project_setups
            .get(&run.project_id)
            .ok_or("Original API setup missing")?;
        require(
            setup.digest == run.setup_digest && setup.setup.policy == plan.policy,
            "API supervision approval or setup changed",
        )?;
        let policy = plan
            .supervision_for(&identity.task_id)?
            .ok_or("API supervision not approved")?;
        require(
            policy.max_advice_per_attempt > 0
                && setup
                    .setup
                    .supervision_for_execution(plan.task(&identity.task_id)?)?
                    == Some(policy),
            "API advice recording was not approved",
        )?;
        let active = &self.api_active[&(run.project_id.clone(), identity.attempt_id.clone())];
        require(
            !policy.delivery_enabled
                || active
                    .advice_waiter
                    .as_ref()
                    .is_some_and(|w| !w.is_closed()),
            "API advice requires the original live tool boundary",
        )?;
        let elapsed = chrono::Utc::now().signed_duration_since(
            chrono::DateTime::parse_from_rfc3339(&active.job.started_at)
                .map_err(|_| "Invalid API start time")?,
        );
        require(
            elapsed.num_milliseconds() >= 0
                && elapsed.num_seconds() < i64::from(plan.policy.max_seconds),
            "API supervision expired",
        )?;
        let mut originals = self.management_original_intents(&run.project_id);
        if let Some(operation) = excluded {
            originals.remove(operation);
        }
        require(
            originals.len() < setup.setup.management_max_requests as usize,
            "Project management request budget exhausted",
        )?;
        require(
            originals
                .values()
                .filter(|i| {
                    i.purpose == Purpose::Advise
                        && i.context["supervision"]["attemptId"] == identity.attempt_id
                })
                .count()
                < policy.max_advice_per_attempt as usize,
            "Original attempt advice budget exhausted",
        )?;
        let snapshot = self
            .api_writers
            .get(&run.project_id)
            .ok_or("Original API coordinator missing")?
            .owner
            .snapshot()?;
        Ok(self.context_for_snapshot(&setup.setup.requirement, &snapshot))
    }
    pub(super) fn request_api_supervision(
        &mut self,
        project: &str,
        attempt: &str,
    ) -> Result<bool, String> {
        let active = self
            .api_active
            .get(&(project.into(), attempt.into()))
            .ok_or("Original API owner missing")?;
        let policy = active
            .snapshot
            .revision
            .as_ref()
            .ok_or("API plan missing")?
            .supervision_for(&active.job.task_id)?;
        let Some(policy) = policy.filter(|p| p.max_advice_per_attempt > 0) else {
            return Ok(false);
        };
        let Some(checkpoint) = active.tool_checkpoint.as_ref() else {
            return Ok(false);
        };
        if policy.delivery_enabled && active.advice_waiter.is_none() {
            return Ok(false);
        }
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
        let evidence = checkpoint.evidence();
        let mut context = self.api_supervision_context(&run, &evidence, None)?;
        let root = self.api_original_scope(project, attempt, evidence.run())?;
        context["supervisionFailureContext"] = serde_json::to_value(evidence.failure_context(
            &root,
            policy,
            &incident.id,
            &checkpoint.errors,
        )?)
        .map_err(|e| e.to_string())?;
        self.require_management_connection(project, Purpose::Advise)?;
        let bound = Binding {
            version: 3,
            kind: "api".into(),
            run_id: run.id.clone(),
            run_digest: digest(&run)?,
            task_id: evidence.run().task_id.clone(),
            attempt_id: attempt.into(),
            incident_id: incident.id.clone(),
            observation_digest: digest(&evidence)?,
        };
        context["supervision"] = serde_json::to_value(&bound).map_err(|e| e.to_string())?;
        context["supervisionEvidence"] =
            serde_json::to_value(evidence).map_err(|e| e.to_string())?;
        let intent = DecisionIntent {
            operation_id: format!("advice-{}", &incident.evidence_digest[..48]),
            project_id: project.into(),
            purpose: Purpose::Advise,
            model: self.project_setups[project].setup.deputy_model.clone(),
            state_digest: digest(&context)?,
            context,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        self.launch_routed_management(intent)?;
        Ok(true)
    }
    pub(super) fn revalidate_api_supervision(&self, intent: &DecisionIntent) -> Result<(), String> {
        intent.validate()?;
        let bound = binding(intent)?.ok_or("API advice binding missing")?;
        let original = evidence(intent, &bound)?;
        let run = self
            .runs
            .all()
            .into_iter()
            .find(|r| r.id == bound.run_id && r.project_id == intent.project_id)
            .ok_or("Original API advice run missing")?;
        require(
            digest(&run)? == bound.run_digest,
            "Original API run authorization changed",
        )?;
        let policy = super::supervision_requests::request_policy(intent)?;
        let current = self
            .api_active
            .get(&(intent.project_id.clone(), bound.attempt_id.clone()))
            .and_then(|a| a.tool_checkpoint.as_ref())
            .ok_or("Original API checkpoint missing")?;
        require(
            current.evidence() == original
                && current
                    .tools
                    .supervision_incidents(&policy)?
                    .iter()
                    .any(|i| {
                        i.id == bound.incident_id
                            && intent.operation_id == format!("advice-{}", &i.evidence_digest[..48])
                    }),
            "API advice incident changed",
        )?;
        let mut context =
            self.api_supervision_context(&run, &original, Some(&intent.operation_id))?;
        context["supervision"] = intent.context["supervision"].clone();
        context["supervisionEvidence"] = intent.context["supervisionEvidence"].clone();
        // Older saved requests have no diagnostic projection. Preserve their
        // original digest rather than manufacturing new historical context.
        if intent.context.get("supervisionFailureContext").is_some() {
            let root =
                self.api_original_scope(&intent.project_id, &bound.attempt_id, original.run())?;
            context["supervisionFailureContext"] = serde_json::to_value(original.failure_context(
                &root,
                &policy,
                &bound.incident_id,
                &current.errors,
            )?)
            .map_err(|e| e.to_string())?;
        }
        require(
            digest(&self.match_original_catalog(&intent.context, context)?)? == intent.state_digest
                && self.project_setups[&intent.project_id].setup.deputy_model == intent.model,
            "API advice context changed during counting",
        )
    }
    pub(super) fn validate_api_supervision_requests(&self) -> Result<(), String> {
        for project in self.projects.keys() {
            for intent in self.management_original_intents(project).values() {
                if let Some(bound) = binding(intent)? {
                    self.validate_api_supervision_request(intent, &bound)?;
                    self.api_advice_delivery_state(intent)?;
                }
            }
        }
        Ok(())
    }
    fn validate_api_supervision_request(
        &self,
        intent: &DecisionIntent,
        bound: &Binding,
    ) -> Result<(), String> {
        intent.validate()?;
        let observation = evidence(intent, bound)?;
        let store = self
            .projects
            .get(&intent.project_id)
            .ok_or("Original API workflow missing")?;
        let original: ExecutionSnapshot =
            serde_json::from_value(intent.context["workflow"].clone())
                .map_err(|_| "API advice workflow missing")?;
        require(
            digest(&crate::workflow_prefix::prefix(store, original.sequence)?)?
                == digest(&original)?,
            "API advice snapshot is not original history",
        )?;
        let revision = original
            .revision
            .as_ref()
            .ok_or("Original API advice plan missing")?;
        observation.run().validate_plan(revision)?;
        let approval = original
            .approval
            .as_ref()
            .ok_or("Original API advice approval missing")?;
        approval.validate(revision, &approval.user_id, revision.policy.version)?;
        let state = original
            .state
            .tasks
            .get(&bound.task_id)
            .ok_or("Original API advice task missing")?;
        require(
            original.state.approved
                && original.state.revision_requested.is_none()
                && state.state == TaskState::Running
                && state.attempt.as_deref() == Some(&bound.attempt_id)
                && state.cancellation_requested != Some(true),
            "Original API advice was not running",
        )?;
        let attempt = store
            .attempt_record(&bound.task_id, &bound.attempt_id)
            .ok_or("Original API advice attempt missing")?;
        require(
            attempt.binding == observation.run().binding
                && attempt.fence == observation.run().fence
                && attempt.revision_digest == observation.run().revision_digest,
            "API advice attempt changed",
        )?;
        require(store.history().iter().take(original.sequence as usize).any(|e|
            matches!(&e.body.event.command, WorkflowCommand::Started { task, attempt } if task == &bound.task_id && attempt == &bound.attempt_id)
            && e.body.context.turn_id.is_none() && e.body.context.thread_id.as_deref() == Some(&observation.run().binding.session_id)),
            "Original API Started evidence missing")?;
        let setup: project_setup::SavedProjectSetup =
            serde_json::from_value(intent.context["executionSetup"].clone())
                .map_err(|_| "API advice setup missing")?;
        let policy = super::supervision_requests::request_policy(intent)?;
        require(
            policy.max_advice_per_attempt > 0
                && setup.setup.deputy_model == intent.model
                && self
                    .runs
                    .has_authorization(&bound.run_id, &bound.run_digest)
                && self.runs.all().iter().any(|r| {
                    r.id == bound.run_id
                        && r.project_id == intent.project_id
                        && r.revision_digest == observation.run().revision_digest
                        && r.setup_digest == setup.digest
                }),
            "API advice original authorization missing",
        )?;
        let root = store.api_attempt_path(&bound.attempt_id)?;
        let incidents = observation.verify_incidents(&root, &policy)?;
        require(
            incidents.iter().any(|i| {
                i.id == bound.incident_id
                    && intent.operation_id == format!("advice-{}", &i.evidence_digest[..48])
            }),
            "Original API advice incident missing",
        )?;
        if let Some(raw) = intent.context.get("supervisionFailureContext") {
            let details: crate::api_tool_checkpoint::FailureContext =
                serde_json::from_value(raw.clone())
                    .map_err(|_| "Invalid original API failure context")?;
            details.verify(&observation, &root, &policy, &bound.incident_id)?;
        }
        Ok(())
    }
    pub(super) fn api_supervision_pending(&self, run: &ProjectRun) -> Result<bool, String> {
        for status in self
            .management_decisions()
            .into_iter()
            .filter(|s| s.project_id == run.project_id && !s.state.resolved())
        {
            if binding(self.management.intent(&status.operation_id)?)?.is_some() {
                // Record-only advice must not change the current worker's run
                // authority merely because its response finished first. Hold
                // new scheduling until original execution has returned ownership.
                if self.has_api_activity(&run.project_id) {
                    return Ok(true);
                }
                require(
                    status.state == DecisionState::Running,
                    "API监督建议已保存或结果待核对，请审阅指挥记录；不会自动投送或重复请求",
                )?;
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub(super) fn api_supervision_advice_view(
        &self,
        intent: &DecisionIntent,
    ) -> Result<SupervisionAdviceView, String> {
        let bound = binding(intent)?.ok_or("API advice binding missing")?;
        let evidence = evidence(intent, &bound)?;
        let (suggestion, rationale) = match self
            .management
            .original_envelope(&intent.operation_id)
            .map(|o| &o.decision)
        {
            Ok(ManagementDecision::Advise {
                task_id,
                suggestion,
                rationale,
            }) if task_id == &bound.task_id => (Some(suggestion.clone()), Some(rationale.clone())),
            _ => (None, None),
        };
        let delivery_state = self.api_advice_delivery_state(intent)?;
        Ok(SupervisionAdviceView {
            omp: None,
            api: Some(ApiAdviceReference {
                request_id: evidence.request_id().into(),
                round: evidence.round(),
                tool_head: evidence.tool_evidence().head().into(),
            }),
            incident_id: bound.incident_id,
            task_id: bound.task_id,
            attempt_id: bound.attempt_id,
            thread_id: String::new(),
            turn_id: String::new(),
            observation_digest: bound.observation_digest,
            suggestion,
            rationale,
            current: self.revalidate_api_supervision(intent).is_ok(),
            delivered: delivery_state == "responseObserved",
            delivery_state: Some(delivery_state.into()),
        })
    }
}
