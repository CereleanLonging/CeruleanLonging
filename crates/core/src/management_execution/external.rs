//! External chief evidence shares the existing publication/application journal.
//! Version 2 deliberately fails the previous reader's version-1 requirement;
//! legacy records and their hashes stay byte-for-byte unchanged.
use super::*;
use cerulean_domain::management_ingress::ManagementSource;

fn validate_intent(intent: &DecisionIntent, request: &ManagementHandoff) -> Result<(), String> {
    intent.validate()?;
    request.validate()?;
    require(
        matches!(request.source, ManagementSource::CodexSession { .. })
            && intent.model == "external-codex-session"
            && request.operation_id == intent.operation_id
            && request.project_id == intent.project_id
            && request.purpose == intent.purpose
            && request.state_digest == intent.state_digest
            && request.context == intent.context,
        "external chief intent/source mismatch",
    )
}
fn validate_response(op: &Operation, response: &ExternalChiefDecision) -> Result<(), String> {
    let request = op
        .external_request
        .as_ref()
        .ok_or("external chief request missing")?;
    let ManagementSource::CodexSession { session_id } = &request.source else {
        return Err("external chief source missing".into());
    };
    response.validate_for(request, &request.context, session_id)
}

impl DecisionJournal {
    pub(crate) fn is_external(&self, id: &str) -> bool {
        self.operations
            .get(id)
            .is_some_and(|op| op.external_request.is_some())
    }
    pub(super) fn accept_external_intent(
        &mut self,
        link: (&str, &str, &str),
        hash: String,
        intent: DecisionIntent,
        request: ManagementHandoff,
    ) -> Result<(), String> {
        let (name, id, previous) = link;
        crate::management_source_guard::require_protected(
            self.root.parent().ok_or("management root missing")?,
            true,
        )?;
        validate_intent(&intent, &request)?;
        require(
            name == format!("{id}.0.json") && previous == "genesis" && intent.operation_id == id,
            "invalid external chief intent link",
        )?;
        require(
            !self.operations.contains_key(id),
            "duplicate external chief intent",
        )?;
        self.operations.insert(
            id.into(),
            Operation {
                omp_request: None,
                omp_response: None,
                deputy_request: None,
                deputy_response: None,
                external_request: Some(request),
                external_response: None,
                intent,
                intent_hash: hash.clone(),
                request: None,
                transport: None,
                scoped_request: None,
                scoped_response: None,
                fee_ack: None,
                chain_hash: hash,
                response_hash: None,
                observation: None,
                finished: false,
                application: None,
                applied: false,
                workflow_hash: None,
                resolution: None,
            },
        );
        Ok(())
    }
    pub(super) fn accept_external_response(
        &mut self,
        name: &str,
        id: &str,
        previous: &str,
        hash: String,
        response: ExternalChiefDecision,
    ) -> Result<(), String> {
        let op = self
            .operations
            .get_mut(id)
            .ok_or("external response without intent")?;
        require(
            name == format!("{id}.1.json")
                && previous == op.chain_hash
                && !op.finished
                && op.resolution.is_none(),
            "invalid external response link",
        )?;
        validate_response(op, &response)?;
        op.external_response = Some(response);
        op.finished = true;
        op.response_hash = Some(hash.clone());
        op.chain_hash = hash;
        Ok(())
    }
    pub(crate) fn begin_external(
        &mut self,
        intent: DecisionIntent,
        request: ManagementHandoff,
    ) -> Result<(), String> {
        crate::management_source_guard::require_protected(
            self.root.parent().ok_or("management root missing")?,
            true,
        )?;
        validate_intent(&intent, &request)?;
        self.require_intent_capacity(0)?;
        require(
            !self.operations.contains_key(&intent.operation_id),
            "external chief operation already recorded; never resend",
        )?;
        require(
            !self.operations.values().any(|op| {
                op.intent.project_id == intent.project_id && !op.applied && op.resolution.is_none()
            }),
            "project management decision awaits reconciliation or application",
        )?;
        let id = intent.operation_id.clone();
        let hash = self.publish(
            &id,
            0,
            "genesis".into(),
            Evidence::ExternalIntent {
                intent: intent.clone(),
                request: request.clone(),
            },
        )?;
        self.accept_external_intent(
            (&format!("{id}.0.json"), &id, "genesis"),
            hash,
            intent,
            request,
        )
    }
    pub(crate) fn external_request(&self, id: &str) -> Result<&ManagementHandoff, String> {
        self.ensure_dispatchable(id)?;
        self.operations
            .get(id)
            .and_then(|op| op.external_request.as_ref())
            .ok_or("external chief request missing".into())
    }
    pub(crate) fn finish_external(
        &mut self,
        id: &str,
        response: ExternalChiefDecision,
        current: &Value,
        session: &str,
    ) -> Result<(), String> {
        self.ensure_dispatchable(id)?;
        let op = self
            .operations
            .get(id)
            .ok_or("external chief request missing")?;
        require(
            !op.finished && op.resolution.is_none(),
            "external chief response already recorded or discarded",
        )?;
        response.validate_for(self.external_request(id)?, current, session)?;
        let previous = op.chain_hash.clone();
        let hash = self.publish(
            id,
            1,
            previous.clone(),
            Evidence::ExternalResponse {
                response: response.clone(),
            },
        )?;
        self.accept_external_response(&format!("{id}.1.json"), id, &previous, hash, response)
    }
    pub(crate) fn ready_envelope(
        &self,
        id: &str,
        current_digest: &str,
    ) -> Result<&DecisionEnvelope, String> {
        require(!self.poisoned, "management journal requires reopen")?;
        let op = self.operations.get(id).ok_or("management intent missing")?;
        require(
            !op.applied && op.resolution.is_none(),
            "management decision already applied or discarded",
        )?;
        require(
            op.intent.state_digest == current_digest,
            "management decision is stale",
        )?;
        self.original_envelope(id)
    }
    pub(crate) fn original_envelope(&self, id: &str) -> Result<&DecisionEnvelope, String> {
        require(self.healthy(), "management source publication unresolved")?;
        // Readback of a reviewed original response is needed to recover/close
        // its startup. Dispatch/application entry points enforce their own
        // review gates; reading historical evidence must not grant dispatch.
        let op = self.operations.get(id).ok_or("management intent missing")?;
        let envelope = op
            .envelope()
            .ok_or("management result pending or unknown; do not resend")?;
        envelope.validate(op.intent.purpose, &op.intent.state_digest)?;
        if let Some(response) = &op.omp_response {
            response.validate(&op.intent, op.omp_request.as_ref().ok_or("OMP 原请求缺失")?)?;
        } else if let Some(response) = &op.external_response {
            validate_response(op, response)?;
        } else if let Some(response) = &op.deputy_response {
            response.validate(op.deputy_request.as_ref().ok_or("deputy request missing")?)?;
        } else if let Some(observation) = &op.observation {
            Self::validate_transport(op, observation, op.transport.as_ref())?;
        }
        Ok(envelope)
    }
}
