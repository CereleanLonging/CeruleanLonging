//! Host authorization and cross-ledger proofs for the sole worker's advice transaction.
use super::supervision_requests::{binding, Binding};
use super::*;
use crate::{
    advice_delivery::*,
    management_execution::{ApplicationTarget, DecisionState},
    supervised_worker::{folder, AdviceUpdate, Prepared},
};
use cerulean_domain::{
    management::{ManagementDecision, Purpose},
    project_setup::{ProjectRun, RunState},
};
use std::sync::atomic::Ordering;

impl RuntimeHost {
    fn advice_authorized(&self, project: &str, operation: &str) -> Result<Binding, String> {
        let intent = self.management.intent(operation)?;
        let b = binding(intent)?.ok_or("missing advice binding")?;
        require(intent.project_id == project, "advice project mismatch")?;
        let active = self
            .codex_active
            .get(&(project.into(), b.attempt_id.clone()))
            .ok_or("advice worker ended")?;
        let view = self
            .supervision_advice_view(operation)?
            .ok_or("advice detail missing")?;
        let revision = active
            .snapshot
            .revision
            .as_ref()
            .ok_or("advice revision missing")?;
        let policy = revision
            .supervision_for(&b.task_id)?
            .ok_or("supervision not approved")?;
        let task = active
            .snapshot
            .state
            .tasks
            .get(&b.task_id)
            .ok_or("advice task missing")?;
        let elapsed = chrono::Utc::now().signed_duration_since(
            chrono::DateTime::parse_from_rfc3339(&active.job.started_at)
                .map_err(|_| "invalid start time")?,
        );
        require(
            self.runner_error.is_none()
                && self.runs.healthy()
                && view.current
                && policy.delivery_enabled
                && policy.max_advice_per_attempt > 0
                && active.job.attempt_id == b.attempt_id
                && active.job.task_id == b.task_id
                && active.advice_gate.load(Ordering::SeqCst) < 2
                && active.observation_saved.as_ref() == Some(&b.observation_digest)
                && active.observation_error.is_none()
                && task.state == TaskState::Running
                && task.attempt.as_deref() == Some(&b.attempt_id)
                && task.cancellation_requested != Some(true)
                && active.snapshot.state.revision_requested.is_none()
                && elapsed.num_milliseconds() >= 0
                && elapsed.num_seconds() < i64::from(revision.policy.max_seconds),
            "建议授权已失效、原执行已变化或未批准发送",
        )?;
        Ok(b)
    }
    pub(super) fn wake_ready_advice(&self, run: &ProjectRun) -> Result<bool, String> {
        if let Some(status) = self.management.statuses().into_iter().find(|s| {
            s.project_id == run.project_id
                && s.purpose == Purpose::Advise
                && s.state == DecisionState::Ready
                && self
                    .management
                    .intent(&s.operation_id)
                    .is_ok_and(|i| binding(i).is_ok_and(|b| b.is_some()))
        }) {
            if matches!(
                self.management
                    .original_envelope(&status.operation_id)?
                    .decision,
                ManagementDecision::Wait { .. }
            ) {
                return Err("副指挥请求补充信息，请审阅指挥记录".into());
            }
            self.management_fee_ack(&status.operation_id)?;
            let b = self.advice_authorized(&run.project_id, &status.operation_id)?;
            let active = &self.codex_active[&(run.project_id.clone(), b.attempt_id)];
            active
                .advice_wake
                .send(())
                .map_err(|_| "supervision control closed")?;
            return Ok(true);
        }
        Ok(false)
    }
    fn prepare_worker_advice(
        &mut self,
        project: &str,
        attempt: &str,
        snapshot: ExecutionSnapshot,
    ) -> Result<Option<Prepared>, String> {
        if self
            .codex_active
            .get(&(project.into(), attempt.into()))
            .is_none_or(|a| a.advice_gate.load(Ordering::SeqCst) == 3)
            || !self
                .runs
                .all()
                .iter()
                .any(|r| r.project_id == project && r.state == RunState::Active)
        {
            return Ok(None);
        }
        let Some(status) = self.management.statuses().into_iter().find(|s| {
            s.project_id == project
                && s.purpose == Purpose::Advise
                && s.state == DecisionState::Ready
                && self
                    .management
                    .intent(&s.operation_id)
                    .is_ok_and(|i| i.context["supervision"]["attemptId"] == attempt)
        }) else {
            return Ok(None);
        };
        let operation = status.operation_id;
        let b = self.advice_authorized(project, &operation)?;
        if b.version == 2 {
            self.validate_shared_advice(&operation, &b, &snapshot, None, true)?;
        }
        require(
            b.version == 2
                || digest(&snapshot)?
                    == digest(&self.codex_active[&(project.into(), attempt.into())].snapshot)?,
            "advice workflow changed before application",
        )?;
        let observation = self.management.original_envelope(&operation)?;
        let ManagementDecision::Advise {
            task_id,
            suggestion,
            ..
        } = &observation.decision
        else {
            return Ok(None);
        };
        let suggestion = suggestion.clone();
        require(
            suggestion.len() <= 16 * 1024,
            "advice too large for delivery",
        )?;
        let target = ApplicationTarget::Workflow {
            principal: "deputy".into(),
            command: WorkflowCommand::Advise {
                task: task_id.clone(),
                suggestion: suggestion.clone(),
            },
            context: Box::default(),
        };
        let state_digest = self.management.intent(&operation)?.state_digest.clone();
        let fee_ack = self.management_fee_ack(&operation)?;
        self.management
            .prepare_application_with_fee(&operation, target, &state_digest, fee_ack)?;
        self.codex_active[&(project.into(), attempt.into())]
            .advice_gate
            .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| "advice permission revoked")?;
        Ok(Some(Prepared {
            operation,
            suggestion,
        }))
    }
    fn commit_worker_advice(
        &mut self,
        project: &str,
        operation: &str,
        entry: crate::scheduler::Entry,
        snapshot: ExecutionSnapshot,
    ) -> Result<Option<DeliveryIntent>, String> {
        let authorized = self.advice_authorized(project, operation).is_ok();
        let b = binding(self.management.intent(operation)?)?.ok_or("advice binding missing")?;
        let key = (project.into(), b.attempt_id.clone());
        if b.version == 2 {
            self.validate_shared_advice(operation, &b, &snapshot, Some(&entry), false)?;
        }
        let active = &self.codex_active[&key];
        require(
            entry.body.operation_id == format!("mgmt-{operation}")
                && (b.version == 2 || entry.body.event.sequence == active.snapshot.sequence + 1)
                && digest(&entry.body)? == entry.hash,
            "advice workflow entry mismatch",
        )?;
        let (target, applied, _) = self
            .management
            .application(operation)
            .ok_or("advice application intent missing")?;
        let ApplicationTarget::Workflow {
            principal,
            command,
            context,
        } = target
        else {
            return Err("advice workflow target missing".into());
        };
        require(
            !applied
                && entry.body.event.principal == *principal
                && entry.body.event.command == *command
                && entry.body.context == **context,
            "advice event differs from prepared decision",
        )?;
        require(
            snapshot.project == active.snapshot.project
                && snapshot.revision_digest == active.snapshot.revision_digest
                && snapshot.approval == active.snapshot.approval
                && snapshot.sequence == entry.body.event.sequence
                && snapshot.state.tasks.get(&b.task_id).is_some_and(|t| {
                    t.state == TaskState::Running && t.attempt.as_deref() == Some(&b.attempt_id)
                }),
            "advice snapshot mismatch",
        )?;
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
            "advice event too large",
        )?;
        let saved: crate::scheduler::Entry =
            serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
                .map_err(|_| "invalid advice event")?;
        require(saved == entry, "advice was not durably published")?;
        let ManagementDecision::Advise { suggestion, .. } =
            &self.management.original_envelope(operation)?.decision
        else {
            return Err("advice response missing".into());
        };
        let intent = DeliveryIntent {
            delivery_id: DeliveryIntent::id_for(operation)?,
            management_operation: operation.into(),
            project_id: project.into(),
            task_id: b.task_id,
            attempt_id: b.attempt_id,
            thread_id: b.thread_id,
            turn_id: b.turn_id,
            incident_id: b.incident_id,
            revision_digest: active.job.revision_digest.clone(),
            run_digest: b.run_digest,
            observation_digest: b.observation_digest,
            management_response_digest: self.management.original_response_digest(operation)?,
            advice_event_hash: entry.hash,
            text_digest: digest(suggestion)?,
        };
        self.codex_active.get_mut(&key).unwrap().snapshot = snapshot;
        if !authorized {
            self.management
                .record_applied(operation, Some(intent.advice_event_hash))?;
            self.codex_active[&key]
                .advice_gate
                .store(3, Ordering::SeqCst);
            for run in self
                .runs
                .all()
                .into_iter()
                .filter(|r| r.project_id == project && r.state == RunState::Active)
            {
                self.set_run_outcome(
                    run,
                    RunState::Attention,
                    "建议已记录但发送授权失效，未发送；请审阅后处理".into(),
                )?;
            }
            return Ok(None);
        }
        Ok(Some(intent))
    }
    pub(super) fn delivery_for(&self, operation: &str) -> Result<Option<DeliveryRecord>, String> {
        let intent = self.management.intent(operation)?;
        let Some(b) = binding(intent)? else {
            return Ok(None);
        };
        let path = folder(&self.root, &intent.project_id, &b.attempt_id);
        if !path.exists() {
            return Ok(None);
        }
        let journal = DeliveryJournal::open(&path)?;
        Ok(journal.record(&DeliveryIntent::id_for(operation)?).cloned())
    }
    fn finish_worker_advice(&mut self, project: &str, operation: &str) -> Result<(), String> {
        let delivery = self
            .delivery_for(operation)?
            .ok_or("delivery evidence missing")?;
        require(
            delivery.intent.project_id == project,
            "delivery project mismatch",
        )?;
        self.validate_delivery_record(&delivery)?;
        self.management
            .record_applied(operation, Some(delivery.intent.advice_event_hash.clone()))?;
        if !delivery.acknowledged() {
            let runs = self.runs.all();
            for run in runs
                .into_iter()
                .filter(|r| r.project_id == project && r.state == RunState::Active)
            {
                self.set_run_outcome(
                    run,
                    RunState::Attention,
                    "建议送达未获可靠回执；不会重发，后续调度已停止".into(),
                )?;
            }
        }
        Ok(())
    }
    pub(super) fn handle_advice_update(
        &mut self,
        project: &str,
        attempt: &str,
        update: AdviceUpdate,
    ) {
        match update {
            AdviceUpdate::Prepare { snapshot, reply } => {
                let _ = reply.send(self.prepare_worker_advice(project, attempt, *snapshot));
            }
            AdviceUpdate::Commit {
                operation,
                entry,
                snapshot,
                reply,
            } => {
                #[cfg(test)]
                if self.management_fixture.is_some()
                    && std::env::var("CERULEAN_DAG_DELIVERY").as_deref() == Ok("PAUSE_COMMIT")
                {
                    for run in self
                        .runs
                        .all()
                        .into_iter()
                        .filter(|r| r.project_id == project && r.state == RunState::Active)
                    {
                        self.pause_project_run(&run.id).unwrap();
                    }
                }
                let _ = reply.send((|| {
                    require(
                        binding(self.management.intent(&operation)?)?
                            .is_some_and(|b| b.attempt_id == attempt),
                        "纠偏回执尝试不符",
                    )?;
                    self.commit_worker_advice(project, &operation, *entry, *snapshot)
                })());
            }
            AdviceUpdate::Finish { operation, reply } => {
                let _ = reply.send((|| {
                    require(
                        binding(self.management.intent(&operation)?)?
                            .is_some_and(|b| b.attempt_id == attempt),
                        "纠偏结束尝试不符",
                    )?;
                    self.finish_worker_advice(project, &operation)
                })());
            }
        }
    }
    pub(super) fn validate_delivery_record(&self, record: &DeliveryRecord) -> Result<(), String> {
        let d = &record.intent;
        let intent = self.management.intent(&d.management_operation)?;
        let b = binding(intent)?.ok_or("delivery without supervision request")?;
        let job = self
            .codex_jobs
            .get(&(intent.project_id.clone(), b.attempt_id.clone()))
            .ok_or("delivery job missing")?;
        let observation = self.management.original_envelope(&d.management_operation)?;
        let ManagementDecision::Advise {
            task_id,
            suggestion,
            ..
        } = &observation.decision
        else {
            return Err("delivery response is not advice".into());
        };
        let approved = super::supervision_requests::request_policy(intent)?.delivery_enabled;
        require(
            approved
                && d.project_id == intent.project_id
                && d.task_id == b.task_id
                && task_id == &d.task_id
                && d.attempt_id == b.attempt_id
                && d.thread_id == b.thread_id
                && d.turn_id == b.turn_id
                && d.incident_id == b.incident_id
                && d.revision_digest == job.revision_digest
                && d.run_digest == b.run_digest
                && d.observation_digest == b.observation_digest
                && d.management_response_digest
                    == self
                        .management
                        .original_response_digest(&d.management_operation)?
                && d.text_digest == digest(suggestion)?,
            "delivery scope or original permission mismatch",
        )?;
        let (target, _, _) = self
            .management
            .application(&d.management_operation)
            .ok_or("delivery application missing")?;
        let ApplicationTarget::Workflow {
            principal,
            command,
            context,
        } = target
        else {
            return Err("delivery workflow target missing".into());
        };
        let valid_entry = |e: &crate::scheduler::Entry| {
            e.hash == d.advice_event_hash
                && e.body.operation_id == format!("mgmt-{}", d.management_operation)
                && e.body.event.principal == *principal
                && e.body.event.command == *command
                && e.body.context == **context
        };
        if let Some(store) = self.projects.get(&d.project_id) {
            require(
                store.history().iter().any(valid_entry),
                "delivery lacks original advice event",
            )?;
        } else if self.api_writers.contains_key(&d.project_id) {
            let reader = self.codex_read(&d.project_id, &d.task_id, &d.attempt_id)?;
            require(
                reader.history().iter().any(valid_entry),
                "delivery lacks original shared advice event",
            )?;
        } else {
            let active = self
                .codex_active
                .get(&(d.project_id.clone(), d.attempt_id.clone()))
                .ok_or("delivery workflow unavailable")?;
            let path = self
                .root
                .join("workflows")
                .join(&d.project_id)
                .join("events")
                .join(format!(
                    "{:012}_mgmt-{}.json",
                    active.snapshot.sequence, d.management_operation
                ));
            ordinary(&path, false)?;
            require(
                fs::metadata(&path).map_err(|e| e.to_string())?.len() <= 256 * 1024,
                "delivery event too large",
            )?;
            let e: crate::scheduler::Entry =
                serde_json::from_slice(&fs::read(path).map_err(|e| e.to_string())?)
                    .map_err(|_| "invalid delivery event")?;
            require(
                digest(&e.body)? == e.hash && valid_entry(&e),
                "delivery advice event mismatch",
            )?;
        }
        Ok(())
    }
    pub(super) fn validate_supervision_deliveries(&self) -> Result<(), String> {
        for job in self.codex_jobs.values() {
            let path = folder(&self.root, &job.project_id, &job.attempt_id);
            if path.exists() {
                for record in DeliveryJournal::open(&path)?.records() {
                    require(
                        record.intent.project_id == job.project_id
                            && record.intent.attempt_id == job.attempt_id,
                        "delivery folder ownership mismatch",
                    )?;
                    self.validate_delivery_record(record)?;
                }
            }
        }
        for run in self
            .runs
            .all()
            .iter()
            .filter(|r| r.state == RunState::Completed)
        {
            self.require_resolved_advice_deliveries_for(
                &run.project_id,
                Some(&run.revision_digest),
            )?;
        }
        Ok(())
    }
    pub(super) fn require_resolved_advice_deliveries(&self, project: &str) -> Result<(), String> {
        self.validate_advice_closures()?;
        self.require_resolved_advice_deliveries_for(project, None)
    }
    fn require_resolved_advice_deliveries_for(
        &self,
        project: &str,
        revision: Option<&str>,
    ) -> Result<(), String> {
        for s in self.management.statuses().iter().filter(|s| {
            s.project_id == project
                && s.purpose == Purpose::Advise
                && s.state == DecisionState::Applied
        }) {
            if let Some(b) = binding(self.management.intent(&s.operation_id)?)? {
                if let Some(revision) = revision {
                    let job = self
                        .codex_jobs
                        .get(&(project.into(), b.attempt_id))
                        .ok_or("advice job missing")?;
                    if job.revision_digest != revision {
                        continue;
                    }
                }
                require(
                    self.delivery_for(&s.operation_id)?
                        .is_some_and(|d| d.acknowledged())
                        || self.advice_closures.get(&s.operation_id).is_some(),
                    "建议已应用但送达未知；请先核对，不会重发或推进后继任务",
                )?;
            }
        }
        Ok(())
    }
    pub(super) fn advice_delivered(&self, operation: &str) -> bool {
        self.delivery_for(operation)
            .ok()
            .flatten()
            .is_some_and(|d| d.acknowledged())
    }
    pub(super) fn advice_delivery_state(&self, operation: &str) -> &'static str {
        match self.delivery_for(operation) {
            Ok(Some(record)) if record.acknowledged() => "acknowledged",
            Ok(Some(_)) | Err(_) => "unknown",
            Ok(None) => {
                if self.management.application(operation).is_some() {
                    "unknown"
                } else {
                    "notAttempted"
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn copy_tree(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let kind = entry.file_type().unwrap();
            assert!(!kind.is_symlink());
            if kind.is_dir() {
                copy_tree(&entry.path(), &to.join(entry.file_name()));
            } else {
                fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
            }
        }
    }
    #[test]
    #[ignore = "requires explicit successful offline DAG evidence; never invokes models"]
    fn delivered_history_rejects_rehashed_scope_and_missing_ack_on_completed_run() {
        let source = PathBuf::from(std::env::var_os("CERULEAN_DELIVERY_SOURCE").unwrap())
            .canonicalize()
            .unwrap();
        let reports = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../reports")
            .canonicalize()
            .unwrap();
        assert!(source.starts_with(reports));
        let baseline = RuntimeHost::open(&source.join("data")).unwrap();
        let status = baseline
            .management
            .statuses()
            .into_iter()
            .find(|s| s.purpose == Purpose::Advise)
            .unwrap();
        let b = binding(baseline.management.intent(&status.operation_id).unwrap())
            .unwrap()
            .unwrap();
        assert!(baseline.advice_delivered(&status.operation_id));
        drop(baseline);
        for field in [
            "textDigest",
            "runDigest",
            "adviceEventHash",
            "managementResponseDigest",
            "observationDigest",
            "turnId",
            "missing-ack",
        ] {
            let temp = tempfile::tempdir().unwrap();
            let data = temp.path().join("data");
            copy_tree(&source.join("data"), &data);
            let path = folder(&data, "project", &b.attempt_id);
            let id = DeliveryIntent::id_for(&status.operation_id).unwrap();
            let first = path.join(format!("{id}.0.json"));
            let second = path.join(format!("{id}.1.json"));
            if field == "missing-ack" {
                fs::remove_file(&second).unwrap();
            } else {
                let mut intent: serde_json::Value =
                    serde_json::from_slice(&fs::read(&first).unwrap()).unwrap();
                intent["body"]["evidence"]["intent"][field] =
                    serde_json::json!(if field == "turnId" {
                        "foreign-turn".into()
                    } else {
                        "0".repeat(64)
                    });
                // Use typed digest ordering, as the real writer does.
                let body: crate::advice_delivery::Body =
                    serde_json::from_value(intent["body"].clone()).unwrap();
                intent["hash"] = serde_json::json!(digest(&body).unwrap());
                let mut outcome: serde_json::Value =
                    serde_json::from_slice(&fs::read(&second).unwrap()).unwrap();
                outcome["body"]["previous"] = intent["hash"].clone();
                if field == "turnId" {
                    outcome["body"]["evidence"]["outcome"]["ack"]["turnId"] =
                        serde_json::json!("foreign-turn");
                }
                let body: crate::advice_delivery::Body =
                    serde_json::from_value(outcome["body"].clone()).unwrap();
                outcome["hash"] = serde_json::json!(digest(&body).unwrap());
                fs::write(first, serde_json::to_vec(&intent).unwrap()).unwrap();
                fs::write(second, serde_json::to_vec(&outcome).unwrap()).unwrap();
            }
            match RuntimeHost::open(&data) {
                Ok(_) => panic!("forged delivery accepted: {field}"),
                Err(error) => assert!(
                    error.contains("delivery scope")
                        || error.contains("delivery lacks original advice")
                        || error.contains("送达未知"),
                    "{field}: {error}"
                ),
            }
        }
    }
}
