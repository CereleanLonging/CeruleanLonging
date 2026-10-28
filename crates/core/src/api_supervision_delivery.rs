//! Application and continuation share the original live API boundary. No
//! historical receipt can recreate a waiter, management request, or API call.
use super::*;
use crate::{
    api_advice_link::{AppliedAdvice, Link, Reference},
    api_call_journal::{ApiCallJournal, CallOutcome},
    management_execution::ApplicationTarget,
};

impl RuntimeHost {
    pub(in crate::host) fn api_advice_boundary_needed(
        &self,
        project: &str,
        attempt: &str,
    ) -> Result<bool, String> {
        let active = self
            .api_active
            .get(&(project.into(), attempt.into()))
            .ok_or("Original API owner missing")?;
        let Some(policy) = active
            .snapshot
            .revision
            .as_ref()
            .ok_or("API plan missing")?
            .supervision_for(&active.job.task_id)?
            .filter(|p| p.delivery_enabled)
        else {
            return Ok(false);
        };
        let checkpoint = active
            .tool_checkpoint
            .as_ref()
            .ok_or("Original API checkpoint missing")?;
        let operations = self.management_operation_ids(project);
        Ok(checkpoint
            .tools
            .supervision_incidents(policy)?
            .iter()
            .any(|i| !operations.contains(&format!("advice-{}", &i.evidence_digest[..48]))))
    }

    pub(in crate::host) fn poll_api_advice_delivery(
        &mut self,
        project: &str,
        attempt: &str,
    ) -> Result<(), String> {
        let key = (project.into(), attempt.into());
        if self
            .api_active
            .get(&key)
            .is_none_or(|a| a.advice_waiter.is_none())
        {
            return Ok(());
        }
        let result = self.prepare_api_advice_delivery(project, attempt);
        match result {
            Ok(None) => Ok(()),
            Ok(Some(advice)) => self
                .api_active
                .get_mut(&key)
                .and_then(|a| a.advice_waiter.take())
                .ok_or("Original API advice waiter missing")?
                .send(Ok(Some(advice)))
                .map_err(|_| "Original API advice waiter closed; never resend".into()),
            Err(error) => {
                if let Some(reply) = self
                    .api_active
                    .get_mut(&key)
                    .and_then(|a| a.advice_waiter.take())
                {
                    let _ = reply.send(Err(error.clone()));
                }
                Err(error)
            }
        }
    }

    fn prepare_api_advice_delivery(
        &mut self,
        project: &str,
        attempt: &str,
    ) -> Result<Option<AppliedAdvice>, String> {
        let active = self
            .api_active
            .get(&(project.into(), attempt.into()))
            .ok_or("Original API owner missing")?;
        require(
            active
                .advice_waiter
                .as_ref()
                .is_some_and(|w| !w.is_closed()),
            "Original API advice boundary expired; never resend",
        )?;
        let checkpoint = active
            .tool_checkpoint
            .as_ref()
            .ok_or("API advice checkpoint missing")?;
        let original_evidence = checkpoint.evidence();
        // The durable journal has no Running state. Only the original Host
        // knows whether its request is still owned by a live background call.
        let Some(status) = self.management_decisions().into_iter().find(|s| {
            s.project_id == project
                && self.management.intent(&s.operation_id).is_ok_and(|i| {
                    binding(i).is_ok_and(|b| {
                        b.is_some_and(|b| {
                            b.attempt_id == attempt
                                && b.observation_digest
                                    == digest(&original_evidence).unwrap_or_default()
                        })
                    })
                })
        }) else {
            return Ok(None);
        };
        if status.state == DecisionState::Running {
            return Ok(None);
        }
        require(
            status.state == DecisionState::Ready,
            "API advice is unresolved or already applied; never resend",
        )?;
        let operation = status.operation_id;
        let intent = self.management.intent(&operation)?.clone();
        self.revalidate_api_supervision(&intent)?;
        require(
            super::super::supervision_requests::request_policy(&intent)?.delivery_enabled,
            "API advice delivery not approved",
        )?;
        let observation = self.management.original_envelope(&operation)?.clone();
        let ManagementDecision::Advise {
            task_id,
            suggestion,
            ..
        } = &observation.decision
        else {
            return Err("API deputy requested review instead of advice".into());
        };
        require(
            task_id == &original_evidence.run().task_id
                && !suggestion.trim().is_empty()
                && suggestion.len() <= 16 * 1024,
            "API advice target or size invalid",
        )?;
        let original: ExecutionSnapshot =
            serde_json::from_value(intent.context["workflow"].clone())
                .map_err(|_| "Original API advice workflow missing")?;
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
        // The original publisher independently checks the exact snapshot. An
        // unrelated workflow change cannot turn this suggestion into a new grant.
        self.revalidate_api_supervision(&intent)?;
        let entry = self
            .api_writers
            .get(project)
            .ok_or("Original API coordinator missing")?
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
            .record_applied(&operation, Some(entry.hash.clone()))?;
        self.api_writers.get_mut(project).unwrap().refresh()?;
        let checkpoint = self.api_active[&(project.into(), attempt.into())]
            .tool_checkpoint
            .as_ref()
            .unwrap();
        let response_digest = self.management.original_response_digest(&operation)?;
        Ok(Some(AppliedAdvice {
            reference: Reference {
                operation,
                request_digest: intent.state_digest,
                response_digest,
                workflow_hash: entry.hash,
                previous_call: checkpoint.call.clone(),
                previous_head: checkpoint.call_head.clone(),
                suggestion_digest: digest(suggestion)?,
            },
            suggestion: suggestion.clone(),
        }))
    }

    /// Read only; startup never fills a missing link or restarts an application.
    pub(in crate::host) fn api_advice_delivery_state(
        &self,
        intent: &DecisionIntent,
    ) -> Result<&'static str, String> {
        if !super::super::supervision_requests::request_policy(intent)?.delivery_enabled {
            return Ok("recordOnly");
        }
        let bound = binding(intent)?.ok_or("API advice binding missing")?;
        let original = evidence(intent, &bound)?;
        let active = self
            .api_active
            .contains_key(&(intent.project_id.clone(), bound.attempt_id.clone()));
        let root = if active {
            self.api_original_scope(&intent.project_id, &bound.attempt_id, original.run())?
        } else {
            self.projects
                .get(&intent.project_id)
                .ok_or("Original API workflow missing")?
                .api_attempt_path(&bound.attempt_id)?
        };
        crate::api_execution::require_original_identity(&root, original.run())?;
        let name = format!("advice-{}.json", intent.operation_id);
        // A pending link is unresolved, never evidence of a request or permission.
        if root
            .join(format!("{name}.pending"))
            .try_exists()
            .map_err(|e| e.to_string())?
        {
            return Ok("linkPublicationUnknown");
        }
        let Some((target, applied, workflow_hash)) =
            self.management.application(&intent.operation_id)
        else {
            require(
                !root.join(&name).try_exists().map_err(|e| e.to_string())?,
                "API advice link has no application",
            )?;
            return Ok("awaitingApplication");
        };
        if !root.join(&name).try_exists().map_err(|e| e.to_string())? {
            require(
                !applied
                    || !root
                        .join("rounds")
                        .join(format!("{:03}", original.round() + 1))
                        .try_exists()
                        .map_err(|e| e.to_string())?,
                "API continued after advice but original request link is missing",
            )?;
            return Ok(if applied {
                "appliedLocally"
            } else {
                "applicationUnknown"
            });
        }
        require(
            applied,
            "API advice link has no original application receipt",
        )?;
        let observation = self.management.original_envelope(&intent.operation_id)?;
        let ManagementDecision::Advise {
            task_id,
            suggestion,
            ..
        } = &observation.decision
        else {
            return Err("API link decision is not advice".into());
        };
        require(
            *target
                == ApplicationTarget::Workflow {
                    principal: "deputy".into(),
                    command: WorkflowCommand::Advise {
                        task: task_id.clone(),
                        suggestion: suggestion.clone(),
                    },
                    context: Box::default(),
                },
            "API advice application changed",
        )?;
        self.management_fee_ack(&intent.operation_id)?;
        let link: Link = crate::api_execution::load(&root, &name)?;
        let previous = ApiCallJournal::read_existing(
            &root.join("rounds").join(format!("{:03}", original.round())),
        )?;
        let expected = Reference {
            operation: intent.operation_id.clone(),
            request_digest: intent.state_digest.clone(),
            response_digest: self
                .management
                .original_response_digest(&intent.operation_id)?,
            workflow_hash: workflow_hash.ok_or("API advice event hash missing")?.into(),
            previous_call: previous.identity().clone(),
            previous_head: previous.head().into(),
            suggestion_digest: digest(suggestion)?,
        };
        expected.validate(original.run())?;
        require(
            link.version == 1 && link.reference == expected,
            "API advice link differs from original management application",
        )?;
        crate::api_execution::check_round_scope(
            original.run(),
            &link.next_call,
            original.round() + 1,
        )?;
        let path = root
            .join("rounds")
            .join(format!("{:03}", link.next_call.round));
        if !path.try_exists().map_err(|e| e.to_string())? {
            return Ok("awaitingNextRequest");
        }
        // The original request may still own its journal lock. Do not compete
        // with it or treat a cached link as a provider acknowledgement.
        if active {
            return Ok("nextRequestActive");
        }
        let next = if previous.state().scoped_request.is_some() {
            // A crash can leave only the original monetary preparation. The
            // existing read-only path preserves that liability without a send.
            ApiCallJournal::read_prepared_existing(&path, &link.next_call)?
        } else {
            ApiCallJournal::read_existing(&path)?
        };
        require(
            next.identity() == &link.next_call,
            "API advice next request changed",
        )?;
        if !next.healthy() {
            return Ok("requestOutcomeUnknown");
        }
        match &next.state().outcome {
            Some(CallOutcome::Reply { .. }) => {
                if next.state().scoped_request.is_some() && next.require_fee_ack().is_err() {
                    return Ok("requestOutcomeUnknown");
                }
                Ok("responseObserved")
            }
            _ => Ok("requestOutcomeUnknown"),
        }
    }
}
