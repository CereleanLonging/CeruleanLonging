//! Reuse the saved chief response and original management application transaction.
use super::*;
use crate::management_execution::ApplicationTarget;

impl RuntimeHost {
    fn acceptance_original(&self, operation: &str) -> Result<ExecutionSnapshot, String> {
        let intent = self.management.intent(operation)?;
        require(
            intent.purpose == management::Purpose::Accept,
            "共享收口用途不符",
        )?;
        let original: ExecutionSnapshot =
            serde_json::from_value(intent.context["workflow"].clone())
                .map_err(|_| "主指挥原管理快照无效")?;
        require(
            original.project.id == intent.project_id,
            "主指挥管理项目身份不符",
        )?;
        let requirement = intent.context["requirement"]
            .as_str()
            .ok_or("管理需求缺失")?;
        require(
            digest(&self.match_original_catalog(
                &intent.context,
                self.context_for_snapshot(requirement, &original),
            )?)? == intent.state_digest,
            "主指挥管理配置或用户答复已变化",
        )?;
        Ok(original)
    }
    pub(super) fn scoped_acceptance_decision(&self, operation: &str) -> Result<bool, String> {
        let intent = self.management.intent(operation)?;
        if intent.purpose != management::Purpose::Accept {
            return Ok(false);
        }
        self.scoped_acceptance_envelope(operation, self.management.original_envelope(operation)?)
    }
    /// Check an incoming external response before publishing it. Reuse the
    /// same original-prefix proof as application of an already stored response.
    pub(super) fn scoped_acceptance_envelope(
        &self,
        operation: &str,
        envelope: &management::DecisionEnvelope,
    ) -> Result<bool, String> {
        let intent = self.management.intent(operation)?;
        if intent.purpose != management::Purpose::Accept {
            return Ok(false);
        }
        let management::ManagementDecision::Accept {
            task_id,
            attempt_id,
            ..
        } = &envelope.decision
        else {
            return Ok(false);
        };
        let reader = self.api_read(&intent.project_id, task_id, attempt_id)?;
        if !crate::acceptance_scope::isolated(reader.revision().ok_or("收口方案缺失")?, task_id)?
        {
            return Ok(false);
        }
        let original = self.acceptance_original(operation)?;
        crate::acceptance_scope::validate(&*reader, &original, task_id, attempt_id)?;
        Ok(true)
    }
    pub(super) fn apply_shared_acceptance(
        &mut self,
        operation: &str,
        target: ApplicationTarget,
    ) -> Result<(), String> {
        let ApplicationTarget::Workflow {
            principal,
            command: WorkflowCommand::Accept { task, attempt },
            context,
        } = &target
        else {
            return Err("共享账本仅允许此主指挥收口事务".into());
        };
        let project = self.management.intent(operation)?.project_id.clone();
        self.require_available_inner(&project, true)?;
        // If publication acknowledgement was lost, only read the original event.
        // Missing evidence never authorizes a second send of this application.
        if let Some((prior, applied, saved_hash)) = self.management.application(operation) {
            require(prior == &target, "原主指挥应用目标不同")?;
            let reader = self.api_read(&project, task, attempt)?;
            let event = reader
                .history()
                .iter()
                .find(|e| e.body.operation_id == format!("mgmt-{operation}"))
                .ok_or("原主指挥应用结果未知，禁止重发；请核对原事务")?;
            require(
                management_application::matches_target(event, &target),
                "原主指挥事件与应用意图不同",
            )?;
            let hash = event.hash.clone();
            if applied {
                return require(
                    saved_hash == Some(hash.as_str()),
                    "主指挥应用回执与账本不符",
                );
            }
            drop(reader);
            if let Some(writer) = self.api_writers.get_mut(&project) {
                writer.refresh()?;
            }
            return self.management.record_applied(operation, Some(hash));
        }
        self.management_envelope(operation)?;
        let original = self.acceptance_original(operation)?;
        let digest = self.management.intent(operation)?.state_digest.clone();
        let fee_ack = self.management_fee_ack(operation)?;
        self.management.prepare_application_with_fee(
            operation,
            target.clone(),
            &digest,
            fee_ack,
        )?;
        let writer = self
            .api_writers
            .get_mut(&project)
            .ok_or("主指挥共享账本缺失")?;
        let outcome = writer.owner.accept(
            original,
            format!("mgmt-{operation}"),
            principal.clone(),
            task.clone(),
            attempt.clone(),
            (**context).clone(),
        );
        // An ACK can be lost after Accept is durable. Refresh only the original
        // authoritative projection, leaving the management application unresolved.
        if let Err(error) = writer.refresh() {
            let message = format!("主指挥收口后账本不可读，需要核对: {error}");
            self.api_fault = Some(message.clone());
            return Err(message);
        }
        let entry = outcome?;
        self.management.record_applied(operation, Some(entry.hash))
    }
}
