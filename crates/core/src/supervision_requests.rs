//! Dedicated no-tool advice request while the worker retains workflow ownership.
use super::*;
use crate::management_execution::DecisionIntent;
use cerulean_domain::{
    management::{ManagementDecision, Purpose},
    project_setup::{ProjectRun, RunState},
    supervision::SupervisionAdviceView,
    worker_observation::WorkerObservationView,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Binding {
    pub(super) version: u32,
    pub(super) run_id: String,
    pub(super) run_digest: String,
    pub(super) task_id: String,
    pub(super) attempt_id: String,
    pub(super) thread_id: String,
    pub(super) turn_id: String,
    pub(super) incident_id: String,
    pub(super) observation_digest: String,
}
pub(super) fn binding(intent: &DecisionIntent) -> Result<Option<Binding>, String> {
    #[cfg(windows)]
    if super::omp_supervision_requests::binding(intent)?.is_some() {
        return Ok(None);
    }
    if super::api_supervision_requests::binding(intent)?.is_some() {
        return Ok(None);
    }
    let Some(raw) = intent.context.get("supervision") else {
        return Ok(None);
    };
    let bound: Binding =
        serde_json::from_value(raw.clone()).map_err(|_| "invalid supervision request binding")?;
    require(
        matches!(bound.version, 1 | 2) && intent.purpose == Purpose::Advise,
        "supervision purpose mismatch",
    )?;
    for id in [
        &bound.run_id,
        &bound.task_id,
        &bound.attempt_id,
        &bound.incident_id,
    ] {
        identifier(id)?;
    }
    for hash in [&bound.run_digest, &bound.observation_digest] {
        sha256(hash)?;
    }
    require(
        !bound.thread_id.is_empty() && !bound.turn_id.is_empty(),
        "supervision thread/turn missing",
    )?;
    Ok(Some(bound))
}
/// Resolve permission from the immutable original request, never the current
/// configuration or a raw project-wide JSON flag. Old requests inherit exactly
/// their original global policy because their task overrides are absent.
pub(super) fn request_policy(
    intent: &DecisionIntent,
) -> Result<cerulean_domain::supervision::SupervisionPolicy, String> {
    let task = match super::api_supervision_requests::binding(intent)? {
        Some(bound) => bound.task_id,
        None => {
            #[cfg(windows)]
            if let Some(bound) = super::omp_supervision_requests::binding(intent)? {
                return request_policy_for_task(intent, &bound.task_id);
            }
            binding(intent)?.ok_or("缺少原监督请求")?.task_id
        }
    };
    request_policy_for_task(intent, &task)
}
fn request_policy_for_task(
    intent: &DecisionIntent,
    task: &str,
) -> Result<cerulean_domain::supervision::SupervisionPolicy, String> {
    let snapshot: ExecutionSnapshot =
        serde_json::from_value(intent.context["workflow"].clone()).map_err(|_| "监督原快照无效")?;
    let revision = snapshot.revision.as_ref().ok_or("监督原方案缺失")?;
    require(revision.project.id == intent.project_id, "监督原项目不符")?;
    let policy = revision
        .supervision_for(task)?
        .ok_or("原任务未批准异常监督")?;
    let setup: project_setup::SavedProjectSetup =
        serde_json::from_value(intent.context["executionSetup"].clone())
            .map_err(|_| "监督请求缺少原配置")?;
    let execution = revision.task(task)?;
    require(
        setup.digest == digest(&setup.setup)?
            && setup.setup.project_id == intent.project_id
            && setup.setup.policy == revision.policy
            && setup.setup.supervision_for_execution(execution)? == Some(policy),
        "监督原任务策略或配置不符",
    )?;
    Ok(policy.clone())
}
impl RuntimeHost {
    pub(super) fn revalidate_counted_supervision(
        &self,
        intent: &DecisionIntent,
    ) -> Result<(), String> {
        #[cfg(windows)]
        if super::omp_supervision_requests::binding(intent)?.is_some() {
            return self.revalidate_omp_supervision(intent);
        }
        if super::api_supervision_requests::binding(intent)?.is_some() {
            return self.revalidate_api_supervision(intent);
        }
        let b = binding(intent)?.ok_or("counted advice lacks original binding")?;
        let run = self
            .runs
            .all()
            .into_iter()
            .find(|r| r.id == b.run_id && r.project_id == intent.project_id)
            .ok_or("original advice run missing")?;
        require(
            digest(&run)? == b.run_digest,
            "original advice run authorization changed",
        )?;
        let observation: WorkerObservationView =
            serde_json::from_value(intent.context["supervisionEvidence"].clone())
                .map_err(|_| "original advice observation missing")?;
        let active = self
            .codex_active
            .get(&(intent.project_id.clone(), b.attempt_id.clone()))
            .ok_or("original advice worker missing")?;
        let current = crate::worker_observation::view(
            &active.job,
            active
                .observation
                .as_ref()
                .ok_or("original advice capture missing")?
                .snapshot(),
            active.observation_error.clone(),
            observation.supervision.clone(),
        )?;
        require(
            digest(&current)? == digest(&observation)?,
            "worker observation changed during count",
        )?;
        require(
            observation.task_id == b.task_id
                && observation.attempt_id == b.attempt_id
                && observation.evidence_digest == b.observation_digest
                && observation.trace.thread_id.as_deref() == Some(b.thread_id.as_str())
                && observation.trace.turn_id.as_deref() == Some(b.turn_id.as_str())
                && observation.incidents.iter().any(|i| {
                    i.id == b.incident_id
                        && intent.operation_id == format!("advice-{}", &i.evidence_digest[..48])
                }),
            "counted advice differs from its original incident",
        )?;
        let mut current =
            self.supervision_context_excluding(&run, &observation, Some(&intent.operation_id))?;
        current["supervision"] = intent.context["supervision"].clone();
        current["supervisionEvidence"] = intent.context["supervisionEvidence"].clone();
        require(
            digest(&self.match_original_catalog(&intent.context, current)?)? == intent.state_digest,
            "advice scope changed during counting",
        )
    }
    fn supervision_context(
        &self,
        run: &ProjectRun,
        observation: &WorkerObservationView,
    ) -> Result<serde_json::Value, String> {
        self.supervision_context_excluding(run, observation, None)
    }
    fn supervision_context_excluding(
        &self,
        run: &ProjectRun,
        observation: &WorkerObservationView,
        excluded: Option<&str>,
    ) -> Result<serde_json::Value, String> {
        require(
            self.runs.healthy() && self.runner_error.is_none(),
            "运行控制需恢复",
        )?;
        require(
            run.state == RunState::Active
                && self
                    .runs
                    .all()
                    .iter()
                    .any(|r| digest(r).ok() == digest(run).ok()),
            "监督需要当前用户运行授权",
        )?;
        let active = self
            .codex_active
            .get(&(run.project_id.clone(), observation.attempt_id.clone()))
            .ok_or("原执行线程已结束")?;
        let job = &active.job;
        require(
            !active.disconnected
                && active.cancel.is_some()
                && job.state == CodexJobState::Executing,
            "执行已停止或不可控制",
        )?;
        let current = self
            .api_writers
            .get(&run.project_id)
            .map(|w| w.owner.snapshot())
            .transpose()?;
        let state = current.as_ref().unwrap_or(&active.snapshot);
        let task = state
            .state
            .tasks
            .get(&job.task_id)
            .ok_or("监督任务不存在")?;
        require(
            task.state == TaskState::Running
                && task.attempt.as_deref() == Some(job.attempt_id.as_str())
                && task.cancellation_requested != Some(true)
                && state.state.revision_requested.is_none(),
            "原尝试不是可监督运行状态",
        )?;
        let revision = state.revision.as_ref().ok_or("监督方案缺失")?;
        let setup = self
            .project_setups
            .get(&job.project_id)
            .ok_or("监督配置缺失")?;
        require(
            revision.approval_digest()? == job.revision_digest
                && job.revision_digest == run.revision_digest
                && state
                    .approval
                    .as_ref()
                    .is_some_and(|a| a.revision_digest == run.revision_digest)
                && setup.digest == run.setup_digest
                && revision.policy == setup.setup.policy,
            "监督配置或批准不符",
        )?;
        let policy = revision
            .supervision_for(&job.task_id)?
            .ok_or("未批准异常监督")?;
        let execution = revision
            .execution
            .iter()
            .find(|e| e.task_id == job.task_id)
            .ok_or("监督执行任务缺失")?;
        require(
            setup.setup.supervision_for_execution(execution)? == Some(policy),
            "监督任务策略与配置不符",
        )?;
        policy.validate()?;
        require(
            policy.max_advice_per_attempt > 0,
            "本方案只记录异常，不允许请求建议",
        )?;
        let elapsed = chrono::Utc::now().signed_duration_since(
            chrono::DateTime::parse_from_rfc3339(&job.started_at)
                .map_err(|_| "invalid attempt start time")?,
        );
        require(
            elapsed.num_milliseconds() >= 0
                && elapsed.num_seconds() < i64::from(revision.policy.max_seconds),
            "原尝试已超过执行时限",
        )?;
        let trace = &observation.trace;
        require(
            observation.project_id == job.project_id
                && observation.task_id == job.task_id
                && observation.attempt_id == job.attempt_id
                && observation.revision_digest == job.revision_digest
                && observation.supervision.as_ref() == Some(policy)
                && observation.storage_error.is_none()
                && active.observation_error.is_none()
                && !trace.finished
                && trace.dropped == 0
                && trace.rejected == 0,
            "监督观察不完整或原执行已结束",
        )?;
        require(
            active.observation_saved.as_ref() == Some(&observation.evidence_digest),
            "新观察尚未可靠保存",
        )?;
        let mut original = self.management_original_intents(&job.project_id);
        if let Some(id) = excluded {
            original.remove(id);
        }
        let count = original.len();
        require(
            count < setup.setup.management_max_requests as usize,
            "项目管理请求额度已用完",
        )?;
        let advice_count = original
            .values()
            .filter(|i| {
                i.purpose == Purpose::Advise
                    && i.context["supervision"]["attemptId"] == job.attempt_id
            })
            .count();
        require(
            advice_count < policy.max_advice_per_attempt as usize,
            "本次尝试建议额度已用完",
        )?;
        Ok(self.context_for_snapshot(&setup.setup.requirement, state))
    }
    pub(super) fn poll_supervision_request(&mut self, run: &ProjectRun) -> Result<(), String> {
        if self.wake_ready_advice(run)? {
            return Ok(());
        }
        require(
            !self.management_decisions().iter().any(|s| {
                s.project_id == run.project_id
                    && s.purpose == Purpose::Advise
                    && s.state == crate::management_execution::DecisionState::Unknown
            }),
            "副指挥建议结果未知，已停止后续调度；不会重复请求",
        )?;
        let attempts: Vec<_> = self
            .codex_active
            .keys()
            .filter(|(p, _)| p == &run.project_id)
            .map(|(_, a)| a.clone())
            .collect();
        for attempt in attempts {
            self.poll_supervision_attempt(run, &attempt)?;
        }
        Ok(())
    }
    fn poll_supervision_attempt(&mut self, run: &ProjectRun, attempt: &str) -> Result<(), String> {
        let active = &self.codex_active[&(run.project_id.clone(), attempt.into())];
        let Some(policy) = active
            .snapshot
            .revision
            .as_ref()
            .ok_or("监督原方案缺失")?
            .supervision_for(&active.job.task_id)?
            .cloned()
        else {
            return Ok(());
        };
        if policy.max_advice_per_attempt == 0 {
            return Ok(());
        }
        // Pending management work remains exclusive and is never replayed.
        if self
            .management
            .statuses()
            .iter()
            .any(|s| s.project_id == run.project_id && !s.state.resolved())
            || self.counting_project(&run.project_id)
            || self.management_work_count() >= 4
            || self.management_probe_in_progress(&run.project_id, Purpose::Advise)
        {
            return Ok(());
        }
        let Some(capture) = &active.observation else {
            return Ok(());
        };
        let observation = crate::worker_observation::view(
            &active.job,
            capture.snapshot(),
            active.observation_error.clone(),
            Some(policy),
        )?;
        if observation.trace.finished
            || observation.trace.dropped > 0
            || observation.trace.rejected > 0
            || observation.incidents.is_empty()
        {
            return Ok(());
        }
        let prior_operations = self.management_operation_ids(&run.project_id);
        let incident = observation
            .incidents
            .iter()
            .find(|i| !prior_operations.contains(&format!("advice-{}", &i.evidence_digest[..48])));
        let Some(incident) = incident else {
            return Ok(());
        };
        if active.observation_saved.as_ref() != Some(&observation.evidence_digest) {
            return Ok(());
        }
        let mut context = self.supervision_context(run, &observation)?;
        self.require_management_connection(&run.project_id, Purpose::Advise)?;
        let bound = Binding {
            version: if api_assignment_scope::isolated(
                active.snapshot.revision.as_ref().ok_or("监督方案缺失")?,
                &active.job.task_id,
            )? {
                2
            } else {
                1
            },
            run_id: run.id.clone(),
            run_digest: digest(run)?,
            task_id: observation.task_id.clone(),
            attempt_id: observation.attempt_id.clone(),
            thread_id: observation.trace.thread_id.clone().ok_or("观察缺少会话")?,
            turn_id: observation.trace.turn_id.clone().ok_or("观察缺少轮次")?,
            incident_id: incident.id.clone(),
            observation_digest: observation.evidence_digest.clone(),
        };
        context["supervision"] = serde_json::to_value(bound).map_err(|e| e.to_string())?;
        context["supervisionEvidence"] =
            serde_json::to_value(&observation).map_err(|e| e.to_string())?;
        let intent = DecisionIntent {
            operation_id: format!("advice-{}", &incident.evidence_digest[..48]),
            project_id: run.project_id.clone(),
            purpose: Purpose::Advise,
            model: self.project_setups[&run.project_id]
                .setup
                .deputy_model
                .clone(),
            state_digest: digest(&context)?,
            context,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        self.launch_routed_management(intent)
    }
    pub(super) fn validate_supervision_requests(&self) -> Result<(), String> {
        for s in self.management.statuses() {
            let intent = self.management.intent(&s.operation_id)?;
            let Some(b) = binding(intent)? else { continue };
            let store = self
                .projects
                .get(&s.project_id)
                .ok_or("监督请求缺少项目账本")?;
            if b.version == 2 {
                let original: ExecutionSnapshot =
                    serde_json::from_value(intent.context["workflow"].clone())
                        .map_err(|_| "监督原快照无效")?;
                crate::supervision_scope::validate(
                    store,
                    &original,
                    &b.task_id,
                    &b.attempt_id,
                    &b.thread_id,
                    &b.turn_id,
                    original.sequence,
                )?;
                if let Some(entry) = store
                    .history()
                    .iter()
                    .find(|e| e.body.operation_id == format!("mgmt-{}", intent.operation_id))
                {
                    crate::supervision_scope::validate(
                        store,
                        &original,
                        &b.task_id,
                        &b.attempt_id,
                        &b.thread_id,
                        &b.turn_id,
                        entry
                            .body
                            .event
                            .sequence
                            .checked_sub(1)
                            .ok_or("监督事件序号无效")?,
                    )?;
                }
            }
            let observation: WorkerObservationView =
                serde_json::from_value(intent.context["supervisionEvidence"].clone())
                    .map_err(|_| "监督请求缺少观察证据")?;
            let job = self
                .codex_jobs
                .get(&(s.project_id.clone(), b.attempt_id.clone()))
                .ok_or("监督请求缺少原执行")?;
            require(
                job.task_id == b.task_id
                    && observation.project_id == job.project_id
                    && observation.task_id == job.task_id
                    && observation.attempt_id == job.attempt_id
                    && observation.revision_digest == job.revision_digest
                    && observation.evidence_digest == b.observation_digest,
                "监督请求关联不符",
            )?;
            let recomputed = crate::worker_observation::view(
                job,
                observation.trace.clone(),
                None,
                observation.supervision.clone(),
            )?;
            require(
                recomputed.evidence_digest == observation.evidence_digest
                    && recomputed.incidents == observation.incidents
                    && observation.storage_error.is_none()
                    && !observation.trace.finished
                    && observation.trace.dropped == 0
                    && observation.trace.rejected == 0
                    && observation.trace.thread_id.as_deref() == Some(&b.thread_id)
                    && observation.trace.turn_id.as_deref() == Some(&b.turn_id)
                    && observation.incidents.iter().any(|i| {
                        i.id == b.incident_id
                            && intent.operation_id == format!("advice-{}", &i.evidence_digest[..48])
                    }),
                "监督请求证据不符",
            )?;
            let original = store
                .history()
                .iter()
                .find_map(|e| match &e.body.context.execution {
                    Some(ExecutionEvidence::Revision { revision })
                        if revision
                            .approval_digest()
                            .is_ok_and(|d| d == job.revision_digest) =>
                    {
                        Some(revision)
                    }
                    _ => None,
                })
                .ok_or("监督策略缺少原方案证据")?;
            require(
                original.supervision_for(&b.task_id)? == observation.supervision.as_ref(),
                "监督策略缺少原方案证据",
            )?;
            let execution = original
                .execution
                .iter()
                .find(|e| e.task_id == b.task_id)
                .ok_or("监督原任务缺失")?;
            require(store.history().iter().any(|e|matches!(&e.body.event.command,WorkflowCommand::Started {task,attempt} if task==&b.task_id&&attempt==&b.attempt_id)&&e.body.context.thread_id.as_ref()==Some(&b.thread_id)&&e.body.context.turn_id.as_ref()==Some(&b.turn_id)),"监督请求缺少原Started证据")?;
            require(
                self.runs.has_authorization(&b.run_id, &b.run_digest)
                    && self.runs.all().iter().any(|r| {
                        r.id == b.run_id
                            && r.project_id == s.project_id
                            && r.revision_digest == job.revision_digest
                    }),
                "监督请求缺少运行授权",
            )?;
            let setup: project_setup::SavedProjectSetup =
                serde_json::from_value(intent.context["executionSetup"].clone())
                    .map_err(|_| "监督请求缺少原配置")?;
            require(
                setup.digest == digest(&setup.setup)?
                    && setup.setup.deputy_model == intent.model
                    && setup.setup.supervision_for_execution(execution)?
                        == observation.supervision.as_ref()
                    && setup
                        .setup
                        .supervision_for_execution(execution)?
                        .is_some_and(|p| p.max_advice_per_attempt > 0)
                    && self
                        .runs
                        .all()
                        .iter()
                        .any(|r| r.id == b.run_id && r.setup_digest == setup.digest),
                "监督请求配置或模型授权不符",
            )?;
        }
        Ok(())
    }
    pub(super) fn supervision_advice_view(
        &self,
        operation: &str,
    ) -> Result<Option<SupervisionAdviceView>, String> {
        let intent = self.management.intent(operation)?;
        #[cfg(windows)]
        if super::omp_supervision_requests::binding(intent)?.is_some() {
            return self.omp_supervision_advice_view(intent).map(Some);
        }
        if super::api_supervision_requests::binding(intent)?.is_some() {
            return self.api_supervision_advice_view(intent).map(Some);
        }
        let Some(b) = binding(intent)? else {
            return Ok(None);
        };
        let current = self
            .codex_active
            .get(&(intent.project_id.clone(), b.attempt_id.clone()))
            .is_some_and(|a| {
                !a.disconnected
                    && a.job.attempt_id == b.attempt_id
                    && a.job.state == CodexJobState::Executing
                    && a.cancel.is_some()
                    && a.observation.as_ref().is_some_and(|c| {
                        crate::worker_observation::view(
                            &a.job,
                            c.snapshot(),
                            a.observation_error.clone(),
                            a.snapshot.revision.as_ref().and_then(|r| {
                                r.supervision_for(&a.job.task_id).ok().flatten().cloned()
                            }),
                        )
                        .is_ok_and(|v| {
                            !v.trace.finished
                                && v.trace.dropped == 0
                                && v.trace.rejected == 0
                                && v.storage_error.is_none()
                                && v.evidence_digest == b.observation_digest
                        })
                    })
            })
            && self.runs.all().iter().any(|r| {
                r.id == b.run_id
                    && r.state == RunState::Active
                    && digest(r).is_ok_and(|d| d == b.run_digest)
            });
        let (suggestion, rationale) = match self
            .management
            .original_envelope(operation)
            .map(|o| &o.decision)
        {
            Ok(ManagementDecision::Advise {
                task_id,
                suggestion,
                rationale,
            }) if task_id == &b.task_id => (Some(suggestion.clone()), Some(rationale.clone())),
            _ => (None, None),
        };
        Ok(Some(SupervisionAdviceView {
            omp: None,
            api: None,
            incident_id: b.incident_id,
            task_id: b.task_id,
            attempt_id: b.attempt_id,
            thread_id: b.thread_id,
            turn_id: b.turn_id,
            observation_digest: b.observation_digest,
            suggestion,
            rationale,
            current,
            delivered: self.advice_delivered(operation),
            delivery_state: Some(self.advice_delivery_state(operation).into()),
        }))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::async_codex::ActiveCodex;
    use cerulean_domain::{supervision::SupervisionPolicy, worker_observation::*};
    #[test]
    fn scoped_host_advice_rechecks_original_live_observation_and_run() {
        for mode in 0..5 {
            let root = tempfile::tempdir().unwrap();
            let (mut host, _store, observation) = fixture(root.path(), 1, 6);
            let run = host.runs.all()[0].clone();
            let mut context = host.supervision_context(&run, &observation).unwrap();
            let incident = &observation.incidents[0];
            let bound = Binding {
                version: 1,
                run_id: run.id.clone(),
                run_digest: digest(&run).unwrap(),
                task_id: observation.task_id.clone(),
                attempt_id: observation.attempt_id.clone(),
                thread_id: observation.trace.thread_id.clone().unwrap(),
                turn_id: observation.trace.turn_id.clone().unwrap(),
                incident_id: incident.id.clone(),
                observation_digest: observation.evidence_digest.clone(),
            };
            context["supervision"] = serde_json::to_value(bound).unwrap();
            context["supervisionEvidence"] = serde_json::to_value(&observation).unwrap();
            let intent = DecisionIntent {
                operation_id: format!("advice-{}", &incident.evidence_digest[..48]),
                project_id: "project".into(),
                purpose: Purpose::Advise,
                model: host.project_setups["project"].setup.deputy_model.clone(),
                state_digest: digest(&context).unwrap(),
                context,
                created_at: chrono::Utc::now().to_rfc3339(),
            };
            let active = host
                .codex_active
                .get_mut(&("project".into(), "advice-attempt".into()))
                .unwrap();
            let mut trace = observation.trace.clone();
            if mode == 1 {
                trace.finished = true;
            }
            if mode == 2 {
                trace.turn_id = Some("changed-turn".into());
            }
            active.observation = Some(
                cerulean_adapters::codex_observation::ObservationCapture::fixture_trace(trace),
            );
            if mode == 3 {
                active.observation_saved = None;
            }
            if mode == 4 {
                host.pause_project_run(&run.id).unwrap();
            }
            assert_eq!(
                host.revalidate_counted_supervision(&intent).is_ok(),
                mode == 0,
                "mode {mode}"
            );
            assert_eq!(host.management_work_count(), 0);
        }
    }
    fn fixture(
        root: &Path,
        limit: u32,
        budget: u32,
    ) -> (RuntimeHost, DurableWorkflow, WorkerObservationView) {
        fixture_source(root, limit, budget, false)
    }
    fn fixture_source(
        root: &Path,
        limit: u32,
        budget: u32,
        deepseek: bool,
    ) -> (RuntimeHost, DurableWorkflow, WorkerObservationView) {
        if deepseek {
            drop(crate::Store::open(&root.join("data")).unwrap());
            crate::management_source_guard::tests::promote_fixture(&root.join("data"));
        }
        let (mut host, mut draft) = super::super::project_planning::tests::fixture(root);
        if deepseek {
            draft.management_sources = Some(project_setup::ManagementSources {
                chief: project_setup::ChiefSource::OpenAi {},
                deputy: project_setup::DeputySource::DeepSeek {},
            });
            draft.deputy_model = "deepseek-flash".into();
            host.set_openai_key(Some(
                secret::SecretInput::new("sk-synthetic-route-chief".into()).unwrap(),
            ))
            .unwrap();
            host.set_provider_key(
                credentials::CredentialProvider::DeepSeek,
                Some(secret::SecretInput::new("sk-synthetic-route-deputy".into()).unwrap()),
            )
            .unwrap();
        }
        draft.supervision = Some(SupervisionPolicy {
            schema_version: 1,
            repeated_failure_threshold: 2,
            max_advice_per_attempt: limit,
            delivery_enabled: false,
        });
        draft.management_max_requests = budget;
        super::super::project_planning::tests::isolate_draft(root, &mut draft);
        host.configure_project(draft).unwrap();
        super::super::project_planning::tests::complete(
            &mut host,
            super::super::project_planning::tests::proposal("tests"),
        );
        let revision = host.projects["project"]
            .revision()
            .unwrap()
            .approval_digest()
            .unwrap();
        host.approve("project", &revision).unwrap();
        let setup = host.project_setups["project"].digest.clone();
        host.start_project_run("project", &revision, &setup)
            .unwrap();
        let store = host.projects.remove("project").unwrap();
        let mut snapshot = crate::async_verification::snapshot(&store);
        // Synthetic ownership gate fixture; actual Started/transport are tested by the DAG peer.
        let task = snapshot.state.tasks.get_mut("task").unwrap();
        task.state = TaskState::Running;
        task.attempt = Some("advice-attempt".into());
        let now = chrono::Utc::now().to_rfc3339();
        let job = CodexJob {
            management_operation: None,
            project_id: "project".into(),
            task_id: "task".into(),
            attempt_id: "advice-attempt".into(),
            revision_digest: revision,
            worker: store
                .revision()
                .unwrap()
                .task("task")
                .unwrap()
                .worker
                .clone()
                .unwrap(),
            state: CodexJobState::Executing,
            started_at: now.clone(),
            updated_at: now,
            message: None,
        };
        let trace = WorkerObservationTrace {
            thread_id: Some("thread".into()),
            turn_id: Some("turn".into()),
            events: (1..=2)
                .map(|n| ToolObservation {
                    sequence: n,
                    received_at_ms: n as u64,
                    item_digest: format!("{n:064x}"),
                    kind: ObservedToolKind::Command,
                    status: ObservedToolStatus::Failed,
                    exit_code: Some(7),
                    action_digest: Some("a".repeat(64)),
                })
                .collect(),
            ..Default::default()
        };
        let view = crate::worker_observation::view(
            &job,
            trace,
            None,
            store.revision().unwrap().policy.supervision.clone(),
        )
        .unwrap();
        let (_, receiver) = std::sync::mpsc::channel();
        let (cancel, _) = tokio::sync::oneshot::channel();
        host.codex_active.insert(
            (job.project_id.clone(), job.attempt_id.clone()),
            ActiveCodex {
                preparation_cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                advice_wake: tokio::sync::watch::channel(()).0,
                advice_gate: std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0)),
                observation: None,
                observation_saved: Some(view.evidence_digest.clone()),
                observation_error: None,
                snapshot,
                job,
                receiver,
                disconnected: false,
                cancel: Some(cancel),
            },
        );
        (host, store, view)
    }
    #[test]
    fn deepseek_codex_advice_routes_once_and_preserves_original_authority() {
        for revoked in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let (mut host, _store, view) = fixture_source(root.path(), 1, 6, true);
            host.openai_connection.key_configured = false;
            host.management_fixture = Some(|_| panic!("DS advice must not call OpenAI"));
            host.deputy_management_fixture =
                Some(super::super::integration_tests::deepseek_routing::deputy);
            host.codex_active
                .get_mut(&("project".into(), "advice-attempt".into()))
                .unwrap()
                .observation = Some(
                cerulean_adapters::codex_observation::ObservationCapture::fixture_trace(
                    view.trace.clone(),
                ),
            );
            let run = host.runs.all()[0].clone();
            if revoked {
                host.pause_project_run(&run.id).unwrap();
                assert!(host.poll_supervision_request(&run).is_err());
                assert_eq!(host.management_operation_total(), 1);
                continue;
            }
            host.poll_supervision_request(&run).unwrap();
            let op = host
                .management
                .statuses()
                .into_iter()
                .find(|s| s.purpose == Purpose::Advise)
                .unwrap()
                .operation_id;
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while !host.management_active.is_empty() {
                assert!(std::time::Instant::now() < deadline);
                host.poll_management();
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            assert!(host.management.is_deputy(&op));
            assert_eq!(
                host.read_deepseek_management(&op)
                    .unwrap()
                    .1
                    .unwrap()
                    .billing()
                    .unwrap()
                    .usage
                    .input_tokens,
                17
            );
            let advice = host.supervision_advice_view(&op).unwrap().unwrap();
            assert!(advice.current && !advice.delivered && advice.suggestion.is_some());
            assert_eq!(advice.thread_id, "thread");
            assert_eq!(advice.turn_id, "turn");
            // Record-only policy retains the suggestion but cannot wake delivery.
            assert!(host
                .poll_supervision_request(&run)
                .unwrap_err()
                .contains("未批准发送"));
            assert_eq!(host.management_operation_total(), 2);
            let intent = host.management.intent(&op).unwrap().clone();
            host.pause_project_run(&run.id).unwrap();
            assert!(host.revalidate_counted_supervision(&intent).is_err());
        }
    }
    #[test]
    fn supervision_context_requires_live_authority_complete_durable_evidence_and_budget() {
        for case in 0..17 {
            let root = tempfile::tempdir().unwrap();
            let (mut host, _store, mut view) = fixture(
                root.path(),
                if case == 1 { 0 } else { 2 },
                if case == 2 { 1 } else { 6 },
            );
            let run = host.runs.all()[0].clone();
            match case {
                0 => {}
                1 | 2 => {}
                3 => host.pause_project_run(&run.id).unwrap(),
                4 => {
                    host.codex_active
                        .get_mut(&("project".into(), "advice-attempt".into()))
                        .unwrap()
                        .job
                        .state = CodexJobState::CancelRequested
                }
                5 => {
                    host.codex_active
                        .get_mut(&("project".into(), "advice-attempt".into()))
                        .unwrap()
                        .disconnected = true
                }
                6 => {
                    host.codex_active
                        .get_mut(&("project".into(), "advice-attempt".into()))
                        .unwrap()
                        .cancel
                        .take();
                }
                7 => {
                    host.codex_active
                        .get_mut(&("project".into(), "advice-attempt".into()))
                        .unwrap()
                        .snapshot
                        .approval = None
                }
                8 => view.trace.dropped = 1,
                9 => view.trace.rejected = 1,
                10 => view.trace.finished = true,
                11 => view.storage_error = Some("disk failed".into()),
                12 => {
                    host.codex_active
                        .get_mut(&("project".into(), "advice-attempt".into()))
                        .unwrap()
                        .observation_saved = None
                }
                13 => view.attempt_id = "other".into(),
                14 => {
                    host.codex_active
                        .get_mut(&("project".into(), "advice-attempt".into()))
                        .unwrap()
                        .snapshot
                        .state
                        .tasks
                        .get_mut("task")
                        .unwrap()
                        .cancellation_requested = Some(true)
                }
                16 => {
                    use std::sync::atomic::Ordering;
                    // Run authorization reads the original owner projection, not
                    // the synthetic per-worker Running observation above.
                    host.projects.insert("project".into(), _store);
                    host.pause_project_run(&run.id).unwrap();
                    assert_eq!(
                        host.codex_active[&("project".into(), "advice-attempt".into())]
                            .advice_gate
                            .load(Ordering::SeqCst),
                        3
                    );
                    host.start_project_run("project", &run.revision_digest, &run.setup_digest)
                        .unwrap();
                    assert_eq!(
                        host.codex_active[&("project".into(), "advice-attempt".into())]
                            .advice_gate
                            .load(Ordering::SeqCst),
                        0
                    );
                }
                _ => {
                    host.codex_active
                        .get_mut(&("project".into(), "advice-attempt".into()))
                        .unwrap()
                        .job
                        .started_at = "2020-01-01T00:00:00Z".into()
                }
            }
            assert_eq!(
                host.supervision_context(&run, &view).is_ok(),
                case == 0 || case == 16,
                "case {case}"
            );
            assert_eq!(host.management.statuses().len(), 1);
            assert!(host.management_active.is_empty());
        }
    }
    #[test]
    fn discarded_unknown_advice_still_consumes_attempt_budget_after_journal_reopen() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, _store, view) = fixture(root.path(), 2, 6);
        let run = host.runs.all()[0].clone();
        let base = host.supervision_context(&run, &view).unwrap();
        for n in 0..2 {
            let mut context = base.clone();
            context["supervision"] =
                serde_json::json!({"attemptId":"advice-attempt","taskId":"task"});
            let operation = format!("budget-advice-{n}");
            let intent = DecisionIntent {
                operation_id: operation.clone(),
                project_id: "project".into(),
                purpose: Purpose::Advise,
                model: "fixture-deputy".into(),
                state_digest: digest(&context).unwrap(),
                context,
                created_at: chrono::Utc::now().to_rfc3339(),
            };
            host.management.begin(intent).unwrap();
            host.management
                .finish(&operation, Err("unknown".into()))
                .unwrap();
            let evidence = host
                .management
                .evidence_digest(&operation)
                .unwrap()
                .to_owned();
            host.management
                .resolve(
                    &operation,
                    &evidence,
                    crate::management_execution::UserResolution::Discard {
                        reason: "fixture disposition".into(),
                    },
                )
                .unwrap();
        }
        host.management =
            crate::management_execution::DecisionJournal::open(&root.path().join("data")).unwrap();
        assert_eq!(host.management.statuses().len(), 3);
        assert!(host
            .supervision_context(&run, &view)
            .unwrap_err()
            .contains("本次尝试建议额度"));
        assert!(host.management_active.is_empty());
    }
}
