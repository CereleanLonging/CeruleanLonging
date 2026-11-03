//! DeepSeek uses the same management transaction chain, with its own provenance.
use super::*;
use cerulean_adapters::deputy_management::{DeputyRequest, DeputyResult, PreparedDeputy};

fn validate(intent: &DecisionIntent, request: &DeputyRequest) -> Result<(), String> {
    intent.validate()?;
    request.validate()?;
    if matches!(
        request.handoff.source,
        cerulean_domain::management_ingress::ManagementSource::Provider { .. }
    ) {
        let refinement = serde_json::json!({"chiefModel":intent.context["chiefModel"],"managementSources":{"chief":intent.context["chiefSource"]}});
        let saved = if intent.purpose == Purpose::Refine {
            &refinement
        } else {
            &intent.context["executionSetup"]["setup"]
        };
        let role = match intent.purpose {
            Purpose::Plan | Purpose::Refine | Purpose::Accept => "chief",
            _ => "deputy",
        };
        let model = if role == "chief" {
            "chiefModel"
        } else {
            "deputyModel"
        };
        let source = &saved["managementSources"][role];
        let (endpoint, _) = request.handoff.source.api_selection()?;
        require(
            source["kind"] == "provider"
                && source["endpoint"]
                    == serde_json::to_value(endpoint).map_err(|e| e.to_string())?
                && saved[model] == intent.model,
            "management request differs from original saved role selection",
        )?;
    }
    let h = &request.handoff;
    require(
        intent.operation_id == h.operation_id
            && intent.project_id == h.project_id
            && intent.purpose == h.purpose
            && intent.model == request.wire.requested_model
            && intent.state_digest == h.state_digest
            && intent.context == h.context,
        "deputy intent differs from provider request",
    )?;
    require(
        ![
            "/executionSetup/setup/policy/currencyBudget",
            "/workflow/revision/policy/currencyBudget",
        ]
        .iter()
        .any(|p| intent.context.pointer(p).is_some_and(|v| !v.is_null())),
        "DeepSeek monetary execution needs its own pricing contract",
    )
}
impl DecisionJournal {
    pub(super) fn accept_deputy_intent(
        &mut self,
        link: (&str, &str, &str),
        hash: String,
        intent: DecisionIntent,
        request: DeputyRequest,
    ) -> Result<(), String> {
        let (name, id, previous) = link;
        crate::management_source_guard::require_protected(
            self.root.parent().ok_or("management root missing")?,
            true,
        )?;
        crate::relay_guard::require_protected(
            self.root.parent().ok_or("management root missing")?,
            matches!(
                request.handoff.source,
                cerulean_domain::management_ingress::ManagementSource::Provider { .. }
            ),
        )?;
        validate(&intent, &request)?;
        require(
            name == format!("{id}.0.json")
                && previous == "genesis"
                && intent.operation_id == id
                && !self.operations.contains_key(id),
            "invalid or duplicate deputy intent link",
        )?;
        self.operations.insert(
            id.into(),
            Operation {
                deputy_request: Some(request),
                deputy_response: None,
                external_request: None,
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
    pub(super) fn accept_deputy_response(
        &mut self,
        name: &str,
        id: &str,
        previous: &str,
        hash: String,
        response: DeputyResult,
    ) -> Result<(), String> {
        let op = self.operations.get_mut(id).ok_or("deputy intent missing")?;
        require(
            name == format!("{id}.1.json")
                && previous == op.chain_hash
                && !op.finished
                && op.resolution.is_none(),
            "invalid deputy response link",
        )?;
        response.validate(
            op.deputy_request
                .as_ref()
                .ok_or("non-deputy request cannot accept DeepSeek response")?,
        )?;
        if let Some(envelope) = response.envelope() {
            if op.intent.purpose == Purpose::Advise && op.intent.context["supervision"].is_object()
            {
                let expected = op.intent.context["supervision"]["taskId"]
                    .as_str()
                    .ok_or("supervision task missing")?;
                require(
                    matches!(&envelope.decision,ManagementDecision::Advise{task_id,..} if task_id==expected)
                        || matches!(envelope.decision, ManagementDecision::Wait { .. }),
                    "deputy response targets another supervised task",
                )?;
            }
        }
        op.deputy_response = Some(response);
        op.finished = true;
        op.response_hash = Some(hash.clone());
        op.chain_hash = hash;
        Ok(())
    }
    pub(crate) fn begin_deputy(
        &mut self,
        intent: DecisionIntent,
        prepared: &PreparedDeputy,
    ) -> Result<(), String> {
        let request = prepared.request();
        crate::relay_guard::require_protected(
            self.root.parent().ok_or("management root missing")?,
            matches!(
                request.handoff.source,
                cerulean_domain::management_ingress::ManagementSource::Provider { .. }
            ),
        )?;
        validate(&intent, &request)?;
        self.require_intent_capacity(0)?;
        require(
            !self.operations.contains_key(&intent.operation_id),
            "deputy request already recorded; never resend",
        )?;
        require(
            !self.operations.values().any(|o| {
                o.intent.project_id == intent.project_id && !o.applied && o.resolution.is_none()
            }),
            "project management decision awaits reconciliation or application",
        )?;
        let id = intent.operation_id.clone();
        let hash = self.publish(
            &id,
            0,
            "genesis".into(),
            Evidence::DeputyIntent {
                intent: intent.clone(),
                request: request.clone(),
            },
        )?;
        self.accept_deputy_intent(
            (&format!("{id}.0.json"), &id, "genesis"),
            hash,
            intent,
            request,
        )
    }
    pub(crate) fn finish_deputy(&mut self, id: &str, response: DeputyResult) -> Result<(), String> {
        self.ensure_dispatchable(id)?;
        let op = self.operations.get(id).ok_or("deputy request missing")?;
        require(
            !op.finished && op.resolution.is_none(),
            "deputy outcome already recorded; never resend",
        )?;
        response.validate(op.deputy_request.as_ref().ok_or("deputy request missing")?)?;
        let previous = op.chain_hash.clone();
        let hash = self.publish(
            id,
            1,
            previous.clone(),
            Evidence::DeputyResponse {
                response: response.clone(),
            },
        )?;
        self.accept_deputy_response(
            &format!("{id}.1.json"),
            id,
            &previous,
            hash,
            response.clone(),
        )?;
        // The original response/billing transaction owns recovery. Diagnostic
        // I/O must never turn a received response into a retryable failure.
        let _ = self.save_decode_diagnostic(id, &response);
        Ok(())
    }
    pub(crate) fn deputy_evidence(
        &self,
        id: &str,
    ) -> Result<(DeputyRequest, Option<DeputyResult>, String), String> {
        require(self.healthy(), "management evidence publication unresolved")?;
        self.ensure_dispatchable(id)?;
        let op = self.operations.get(id).ok_or("deputy request missing")?;
        Ok((
            op.deputy_request.clone().ok_or("not a DeepSeek request")?,
            op.deputy_response.clone(),
            op.chain_hash.clone(),
        ))
    }
    pub(crate) fn is_deputy(&self, id: &str) -> bool {
        self.operations
            .get(id)
            .is_some_and(|o| o.deputy_request.is_some())
    }
}

#[cfg(all(test, windows))]
mod tests;
