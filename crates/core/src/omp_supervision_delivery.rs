//! Native OMP advice follows the original held tool result and management fee ACK.
//! Only this live Host can create a steering permit; recovery is inspection only.
use super::*;
use crate::{
    management_execution::ApplicationTarget,
    omp_advice_journal::{self, Reference},
};
use cerulean_adapters::omp_transport::worker::advice::SteerAdvice;

impl RuntimeHost {
    pub(in crate::host) fn revoke_omp_advice(&mut self, project: &str) {
        for active in self
            .omp_active
            .values_mut()
            .filter(|a| a.job.project_id == project)
        {
            if let Some(gate) = &active.advice_gate {
                gate.store(2, std::sync::atomic::Ordering::SeqCst);
            }
            if let Some(reply) = active.advice_waiter.take() {
                // Pausing scheduling withdraws an unsent suggestion, not the
                // already approved original task or its held tool result.
                let _ = reply.send(Ok(None));
            }
        }
    }
    pub(in crate::host) fn handle_omp_advice_boundary(
        &mut self,
        project: &str,
        attempt: &str,
        reply: Option<crate::async_omp::AdviceReply>,
    ) -> Result<(), String> {
        let Some(reply) = reply else { return Ok(()) };
        let key = (project.into(), attempt.into());
        let result: Result<bool, String> = (|| {
            let active = self
                .omp_active
                .get(&key)
                .ok_or("Original OMP owner missing")?;
            require(
                active.advice_waiter.is_none(),
                "Original OMP advice boundary already pending",
            )?;
            let policy = active
                .snapshot
                .revision
                .as_ref()
                .ok_or("OMP plan missing")?
                .supervision_for(&active.job.task_id)?
                .ok_or("OMP advice policy missing")?;
            require(
                policy.delivery_enabled,
                "OMP advice delivery was not approved",
            )?;
            let checkpoint = active
                .tool_checkpoint
                .as_ref()
                .ok_or("OMP original tool checkpoint missing")?;
            let operations = self.management_operation_ids(project);
            let fresh = checkpoint
                .tools
                .supervision_incidents(policy)?
                .iter()
                .any(|i| !operations.contains(&format!("advice-{}", &i.evidence_digest[..48])));
            Ok(fresh
                && self
                    .runs
                    .all()
                    .iter()
                    .any(|r| r.project_id == project && r.state == RunState::Active))
        })();
        match result {
            Ok(true) => {
                self.omp_active.get_mut(&key).unwrap().advice_waiter = Some(reply);
                Ok(())
            }
            Ok(false) => reply
                .send(Ok(None))
                .map_err(|_| "Original OMP boundary ended; never resend".into()),
            Err(error) => {
                let _ = reply.send(Err(error.clone()));
                Err(error)
            }
        }
    }
    pub(in crate::host) fn poll_omp_advice_delivery(
        &mut self,
        project: &str,
        attempt: &str,
    ) -> Result<(), String> {
        let key = (project.into(), attempt.into());
        if self
            .omp_active
            .get(&key)
            .is_none_or(|a| a.advice_waiter.is_none())
        {
            return Ok(());
        }
        match self.prepare_omp_advice_delivery(project, attempt) {
            Ok(None) => Ok(()),
            Ok(Some(advice)) => self
                .omp_active
                .get_mut(&key)
                .and_then(|a| a.advice_waiter.take())
                .ok_or("Original OMP waiter missing")?
                .send(Ok(Some(advice)))
                .map_err(|_| "Original OMP waiter closed; advice not resent".into()),
            Err(error) => {
                if let Some(reply) = self
                    .omp_active
                    .get_mut(&key)
                    .and_then(|a| a.advice_waiter.take())
                {
                    let _ = reply.send(Err(error.clone()));
                }
                Err(error)
            }
        }
    }
    fn prepare_omp_advice_delivery(
        &mut self,
        project: &str,
        attempt: &str,
    ) -> Result<Option<SteerAdvice>, String> {
        let active = self
            .omp_active
            .get(&(project.into(), attempt.into()))
            .ok_or("Original OMP owner missing")?;
        require(
            active
                .advice_waiter
                .as_ref()
                .is_some_and(|w| !w.is_closed()),
            "OMP original held result expired",
        )?;
        let Some(status) = self.management_decisions().into_iter().find(|s| {
            s.project_id == project
                && s.purpose == Purpose::Advise
                && s.state == DecisionState::Ready
                && self.management.intent(&s.operation_id).is_ok_and(|i| {
                    binding(i)
                        .ok()
                        .flatten()
                        .is_some_and(|b| b.attempt_id == attempt)
                })
        }) else {
            return Ok(None);
        };
        let operation = status.operation_id;
        let intent = self.management.intent(&operation)?.clone();
        self.revalidate_omp_supervision(&intent)?;
        require(
            super::super::supervision_requests::request_policy(&intent)?.delivery_enabled,
            "OMP native advice delivery not approved",
        )?;
        let observation = self.management.original_envelope(&operation)?.clone();
        let ManagementDecision::Advise {
            task_id,
            suggestion,
            ..
        } = &observation.decision
        else {
            return Err("OMP deputy requires review instead of advice".into());
        };
        let bound = binding(&intent)?.ok_or("OMP advice binding missing")?;
        require(task_id == &bound.task_id, "OMP advice target changed")?;
        omp_advice_journal::message(&operation, suggestion)?;
        let original: ExecutionSnapshot =
            serde_json::from_value(intent.context["workflow"].clone())
                .map_err(|_| "OMP original advice workflow missing")?;
        let command = WorkflowCommand::Advise {
            task: task_id.clone(),
            suggestion: suggestion.clone(),
        };
        let fee = self.management_fee_ack(&operation)?;
        self.management.prepare_application_with_fee(
            &operation,
            ApplicationTarget::Workflow {
                principal: "deputy".into(),
                command: command.clone(),
                context: Box::default(),
            },
            &intent.state_digest,
            fee,
        )?;
        self.revalidate_omp_supervision(&intent)?;
        let entry = self
            .api_writers
            .get(project)
            .ok_or("OMP original coordinator missing")?
            .owner
            .publish_management(
                original,
                format!("mgmt-{operation}"),
                "deputy".into(),
                attempt.into(),
                command,
                CommandContext::default(),
            )?;
        self.management
            .record_applied(&operation, Some(entry.hash))?;
        self.api_writers.get_mut(project).unwrap().refresh()?;
        let (root, reference) = self.omp_advice_reference(&intent)?;
        let mut advice = omp_advice_journal::prepare(&root, reference, suggestion.clone())?;
        let gate = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0));
        let active = self
            .omp_active
            .get_mut(&(project.into(), attempt.into()))
            .unwrap();
        active.advice_gate = Some(gate.clone());
        let cancelled = active.preparation_cancel.clone();
        // This one-shot CAS reserves sending against pause. Once reserved,
        // persistence must still succeed before a byte is sent. Revocation
        // before reservation releases the original task without a steer.
        advice.may_send = Box::new(move || {
            !cancelled.load(std::sync::atomic::Ordering::Acquire)
                && gate
                    .compare_exchange(
                        0,
                        1,
                        std::sync::atomic::Ordering::SeqCst,
                        std::sync::atomic::Ordering::SeqCst,
                    )
                    .is_ok()
        });
        Ok(Some(advice))
    }
    pub(in crate::host) fn omp_advice_reference(
        &self,
        intent: &DecisionIntent,
    ) -> Result<(PathBuf, Reference), String> {
        let bound = binding(intent)?.ok_or("OMP advice binding missing")?;
        let e = evidence(intent, &bound)?;
        let store = self.omp_read(&intent.project_id, &bound.task_id, &bound.attempt_id)?;
        let root = store.omp_attempt_path(&bound.attempt_id)?;
        let observation = self.management.original_envelope(&intent.operation_id)?;
        let ManagementDecision::Advise {
            task_id,
            suggestion,
            ..
        } = &observation.decision
        else {
            return Err("OMP original advice response missing".into());
        };
        let (target, applied, workflow_hash) = self
            .management
            .application(&intent.operation_id)
            .ok_or("OMP original application missing")?;
        require(
            applied
                && task_id == &bound.task_id
                && *target
                    == ApplicationTarget::Workflow {
                        principal: "deputy".into(),
                        command: WorkflowCommand::Advise {
                            task: task_id.clone(),
                            suggestion: suggestion.clone(),
                        },
                        context: Box::default(),
                    },
            "OMP original advice application changed",
        )?;
        self.management_fee_ack(&intent.operation_id)?;
        let hash = workflow_hash.ok_or("OMP original advice workflow hash missing")?;
        require(
            store.history().iter().any(|entry| {
                entry.hash == hash
                    && entry.body.operation_id == format!("mgmt-{}", intent.operation_id)
                    && crate::host::management_application::matches_target(entry, target)
            }),
            "OMP advice lacks original workflow publication",
        )?;
        let reference = Reference {
            version: 1,
            operation: intent.operation_id.clone(),
            dispatch: e.intent().clone(),
            request_digest: intent.state_digest.clone(),
            response_digest: self
                .management
                .original_response_digest(&intent.operation_id)?,
            workflow_hash: hash.into(),
            tool_head: e.tool_head().into(),
            held_result_sha256: e.held_result_sha256().into(),
            suggestion_digest: digest(suggestion)?,
            message_sha256: omp_advice_journal::text_hash(&omp_advice_journal::message(
                &intent.operation_id,
                suggestion,
            )?),
        };
        Ok((root, reference))
    }
    pub(in crate::host) fn omp_advice_delivery_state(
        &self,
        intent: &DecisionIntent,
    ) -> Result<&'static str, String> {
        if !super::super::supervision_requests::request_policy(intent)?.delivery_enabled {
            return Ok("recordOnly");
        }
        let bound = binding(intent)?.ok_or("OMP advice binding missing")?;
        let store = self.omp_read(&intent.project_id, &bound.task_id, &bound.attempt_id)?;
        let path = store
            .omp_attempt_path(&bound.attempt_id)?
            .join(format!("advice-{}", intent.operation_id));
        let Some((_, applied, _)) = self.management.application(&intent.operation_id) else {
            require(
                !path.try_exists().map_err(|e| e.to_string())?,
                "OMP advice journal lacks management application",
            )?;
            return Ok("awaitingApplication");
        };
        if !applied {
            require(
                !path.try_exists().map_err(|e| e.to_string())?,
                "OMP advice send lacks application ACK",
            )?;
            return Ok("applicationUnknown");
        }
        let (root, reference) = self.omp_advice_reference(intent)?;
        Ok(omp_advice_journal::inspect(&root, &reference)?.map_or("appliedLocally", |s| s.label()))
    }
    pub(in crate::host) fn require_resolved_omp_advice(&self, project: &str) -> Result<(), String> {
        for status in self.management.statuses().into_iter().filter(|s| {
            s.project_id == project
                && s.purpose == Purpose::Advise
                && s.state == DecisionState::Applied
        }) {
            let intent = self.management.intent(&status.operation_id)?;
            if binding(intent)?.is_none() {
                continue;
            }
            if self.validate_omp_advice_closure(&status.operation_id)? {
                continue;
            }
            require(
                matches!(
                    self.omp_advice_delivery_state(intent)?,
                    "messageObserved" | "notSent"
                ),
                "OMP建议已经应用但原任务接收未确认；保留原记录，不重发、不推进后继任务",
            )?;
        }
        Ok(())
    }
}
