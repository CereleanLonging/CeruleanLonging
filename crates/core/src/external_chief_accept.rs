//! Current-session acceptance shares the original workflow/run authority.
use super::*;
use cerulean_domain::{management::ManagementDecision, project_setup::ChiefSource};

impl RuntimeHost {
    pub(in crate::host) fn require_external_accept_run(
        &self,
        project: &str,
        operation: &str,
    ) -> Result<(), String> {
        require(
            self.runs.healthy(),
            "external acceptance run journal unresolved",
        )?;
        let run = self
            .runs
            .all()
            .into_iter()
            .find(|r| r.project_id == project && operation.starts_with(&format!("{}-", r.id)))
            .ok_or("external acceptance needs its original authorized run")?;
        self.ensure_run_dispatch_authorized(operation)?;
        self.run_scope(&run)
    }

    pub(in crate::host) fn persist_external_chief_request(
        &mut self,
        intent: DecisionIntent,
        session: &str,
    ) -> Result<ManagementHandoff, String> {
        identifier(session)?;
        crate::management_source_guard::require_protected(&self.root, true)?;
        self.require_currency_execution_available(&intent.project_id)?;
        let setup = self
            .project_setups
            .get(&intent.project_id)
            .ok_or("project setup missing")?;
        if let Some(sources) = &setup.setup.management_sources {
            require(
                matches!(&sources.chief, ChiefSource::CodexSession { session_id } if session_id == session),
                "external chief differs from configured session",
            )?;
        }
        require(
            self.management_operation_ids(&intent.project_id).len()
                < setup.setup.management_max_requests as usize,
            "management request limit reached",
        )?;
        require(
            self.management_work_count() < 4,
            "management concurrency limit reached",
        )?;
        let request = ManagementHandoff {
            operation_id: intent.operation_id.clone(),
            project_id: intent.project_id.clone(),
            purpose: intent.purpose,
            source: ManagementSource::CodexSession {
                session_id: session.into(),
            },
            state_digest: intent.state_digest.clone(),
            context: intent.context.clone(),
        };
        self.management.begin_external(intent, request.clone())?;
        Ok(request)
    }

    pub(in crate::host) fn prepare_external_chief_accept(
        &mut self,
        project: &str,
        operation: &str,
        model: &str,
        requirement: &str,
    ) -> Result<(), String> {
        self.require_external_accept_run(project, operation)?;
        let setup = self
            .project_setups
            .get(project)
            .ok_or("project setup missing")?;
        let Some(ChiefSource::CodexSession { session_id }) =
            setup.setup.management_sources.as_ref().map(|s| &s.chief)
        else {
            return Err(
                "external acceptance requires explicit current-session configuration".into(),
            );
        };
        require(
            model == "external-codex-session" && model == setup.setup.chief_model,
            "external chief model identity differs from configuration",
        )?;
        let session = session_id.clone();
        let context = self.role_context(
            self.management_context(project, requirement)?,
            Purpose::Accept,
            true,
        )?;
        let original: ExecutionSnapshot = serde_json::from_value(context["workflow"].clone())
            .map_err(|_| "acceptance workflow missing")?;
        require(
            original
                .state
                .tasks
                .values()
                .any(|t| t.state == TaskState::Submitted && t.verified && t.attempt.is_some()),
            "external acceptance needs an independently verified submission",
        )?;
        self.persist_external_chief_request(
            DecisionIntent {
                operation_id: operation.into(),
                project_id: project.into(),
                purpose: Purpose::Accept,
                model: model.into(),
                state_digest: digest(&context)?,
                context,
                created_at: chrono::Utc::now().to_rfc3339(),
            },
            &session,
        )?;
        Ok(())
    }

    pub(in crate::host) fn external_response_context(
        &self,
        operation: &str,
        response: &DecisionEnvelope,
        requirement: &str,
    ) -> Result<serde_json::Value, String> {
        let request = self.management.external_request(operation)?;
        if request.purpose == Purpose::Accept {
            if let ManagementDecision::Accept {
                task_id,
                attempt_id,
                ..
            } = &response.decision
            {
                let original: ExecutionSnapshot =
                    serde_json::from_value(request.context["workflow"].clone())
                        .map_err(|_| "acceptance workflow missing")?;
                require(
                    original.state.tasks.get(task_id).is_some_and(|t| {
                        t.state == TaskState::Submitted
                            && t.verified
                            && t.attempt.as_ref() == Some(attempt_id)
                    }),
                    "external reply does not name an originally verified submission",
                )?;
            }
            #[cfg(windows)]
            if self.scoped_acceptance_envelope(operation, response)? {
                // The helper proved the original journal prefix, unchanged
                // verified target, setup and approval against the live owner.
                return Ok(request.context.clone());
            }
        }
        self.match_original_catalog(
            &request.context,
            self.rebuild_role_context(
                &request.context,
                self.management_context(&request.project_id, requirement)?,
                request.purpose,
            )?,
        )
    }
}
