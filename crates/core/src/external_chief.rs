//! Trusted host entry for a locally bound, externally running Codex chief.
//! No provider call, new Codex session, or automatic user approval is performed.
use super::*;
use crate::management_execution::DecisionIntent;
use cerulean_domain::{
    management::{DecisionEnvelope, Purpose},
    management_ingress::{ExternalChiefDecision, ManagementHandoff, ManagementSource},
};

impl RuntimeHost {
    pub(crate) fn external_chief_operation(
        &mut self,
        operation: cerulean_domain::management_ingress::ExternalChiefOperation,
    ) -> Result<cerulean_domain::management_ingress::ExternalChiefResult, String> {
        use cerulean_domain::management_ingress::{
            ExternalChiefOperation as O, ExternalChiefResult,
        };
        let handoff = match operation {
            O::PreparePlan {
                project_id,
                setup_digest,
                session_id,
            } => self.prepare_external_chief_plan(&project_id, &setup_digest, &session_id)?,
            O::Read { operation_id } => self.read_external_chief_request(&operation_id)?,
            O::Submit {
                operation_id,
                session_id,
                decision,
            } => {
                let handoff = self.read_external_chief_request(&operation_id)?;
                self.submit_external_chief_decision(&operation_id, &session_id, decision)?;
                handoff
            }
        };
        let status = self
            .management_decisions()
            .into_iter()
            .find(|s| s.operation_id == handoff.operation_id)
            .ok_or("external chief status missing")?;
        Ok(ExternalChiefResult {
            operation_id: handoff.operation_id.clone(),
            request_digest: digest(&handoff)?,
            state: serde_json::to_value(status.state)
                .map_err(|_| "status unavailable")?
                .as_str()
                .ok_or("status invalid")?
                .into(),
            message: self.planning_errors.get(&handoff.operation_id).cloned(),
            handoff,
        })
    }
    /// The local caller supplies session correlation, not remote attestation.
    /// Selection is explicit for this operation; existing API routes are unchanged.
    pub fn prepare_external_chief_plan(
        &mut self,
        project: &str,
        setup_digest: &str,
        session: &str,
    ) -> Result<ManagementHandoff, String> {
        self.require_review_dispatch_idle()?;
        identifier(session)?;
        crate::management_source_guard::require_protected(&self.root, true)?;
        self.require_currency_execution_available(project)?;
        self.poll_verifications();
        self.planning_idle(project)?;
        let setup = self
            .project_setups
            .get(project)
            .ok_or("project setup missing")?
            .clone();
        require(setup.digest == setup_digest, "project setup changed")?;
        setup.setup.require_available_adapters()?;
        let count = self.management_operation_ids(project).len();
        let context = self.role_context(
            self.management_context(project, &setup.setup.requirement)?,
            Purpose::Plan,
            true,
        )?;
        let operation = format!("ui-plan-{}-{}", &digest(&project)?[..32], count + 1);
        let context = crate::worker_boundaries::annotate(context, Purpose::Plan)?;
        let context = self.planning_replies(context)?;
        let intent = DecisionIntent {
            operation_id: operation,
            project_id: project.into(),
            purpose: Purpose::Plan,
            model: "external-codex-session".into(),
            state_digest: digest(&context)?,
            context,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        self.persist_external_chief_request(intent, session)
    }
    pub fn read_external_chief_request(
        &self,
        operation: &str,
    ) -> Result<ManagementHandoff, String> {
        let request = self.management.external_request(operation)?;
        if request.purpose == Purpose::Accept {
            self.require_available_inner(&request.project_id, true)?;
        } else {
            self.require_available(&request.project_id)?;
        }
        Ok(request.clone())
    }
    pub fn submit_external_chief_decision(
        &mut self,
        operation: &str,
        session: &str,
        response: ExternalChiefDecision,
    ) -> Result<(), String> {
        self.require_review_dispatch_idle()?;
        self.poll_verifications();
        let request = self.management.external_request(operation)?.clone();
        self.require_currency_execution_available(&request.project_id)?;
        response.validate_for(&request, &request.context, session)?;
        if request.purpose == Purpose::Accept {
            self.require_external_accept_run(&request.project_id, operation)?;
        }
        let requirement = request.context["requirement"]
            .as_str()
            .ok_or("management requirement missing")?;
        let current = self.external_response_context(operation, &response.envelope, requirement)?;
        self.management
            .finish_external(operation, response, &current, session)?;
        // Existing plan materialization proposes the revision; Approve remains a
        // separate user command against that exact revision and digest.
        self.poll_planning_results();
        Ok(())
    }
    pub(super) fn management_envelope(&self, operation: &str) -> Result<&DecisionEnvelope, String> {
        self.management.ensure_dispatchable(operation)?;
        let intent = self.management.intent(operation)?;
        if intent.purpose == Purpose::Accept && self.management.is_external(operation) {
            self.require_external_accept_run(&intent.project_id, operation)?;
        }
        #[cfg(windows)]
        if self.scoped_api_decision(operation)? || self.scoped_acceptance_decision(operation)? {
            return self
                .management
                .ready_envelope(operation, &intent.state_digest);
        }
        let requirement = intent.context["requirement"]
            .as_str()
            .ok_or("management requirement missing")?;
        let current = self.rebuild_management_context(
            intent,
            self.management_context(&intent.project_id, requirement)?,
        )?;
        self.management
            .ready_envelope(operation, &digest(&current)?)
    }
}

#[path = "external_chief_accept.rs"]
mod accept;

#[cfg(all(test, windows))]
#[path = "external_chief/tests.rs"]
mod tests;
