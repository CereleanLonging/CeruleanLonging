//! Original management usage/source export under the caller's data writer lock.
use super::*;
use crate::scheduler::Entry;
use serde_json::json;
use std::collections::BTreeSet;

impl DecisionJournal {
    pub(crate) fn efficiency_evidence(
        root: &Path,
        project: &str,
        history: &[Entry],
    ) -> Result<Value, String> {
        let journal = Self::read_existing(root)?;
        journal.efficiency_snapshot(project, history)
    }

    /// The live RuntimeHost already owns the root writer and this journal.
    pub(crate) fn efficiency_snapshot(
        &self,
        project: &str,
        history: &[Entry],
    ) -> Result<Value, String> {
        let journal = self;
        journal.require_billing_published()?;
        let mut operations = Vec::new();
        let mut selected = BTreeSet::new();
        let mut response_ids = BTreeSet::new();
        let mut linked = 0;
        let statuses = journal.statuses();
        for (id, op) in &journal.operations {
            if op.intent.project_id != project {
                continue;
            }
            selected.insert(id.as_str());
            let event = history
                .iter()
                .find(|e| e.body.operation_id == format!("mgmt-{id}"));
            if let Some(ApplicationTarget::Workflow {
                principal,
                command,
                context,
            }) = &op.application
            {
                if let Some(event) = event {
                    require(
                        event.body.event.principal == *principal
                            && event.body.event.command == *command
                            && event.body.context == **context,
                        "management application differs from original workflow event",
                    )?;
                    if op.applied {
                        require(
                            op.workflow_hash.as_ref() == Some(&event.hash),
                            "management application receipt links another workflow event",
                        )?;
                    }
                    linked += 1;
                } else {
                    require(
                        !op.applied,
                        "applied management decision has no original workflow event",
                    )?;
                }
            } else {
                require(
                    event.is_none(),
                    "workflow event has no matching management application",
                )?;
            }
            let (source, usage_basis, usage, actual_model, product_api_intent) = match journal
                .stored_decision(id)?
            {
                StoredDecision::Omp { .. } => (
                    "omp",
                    "native-process-usage-unknown",
                    Value::Null,
                    None,
                    false,
                ),
                StoredDecision::CodexSession { .. } => (
                    "codex-session",
                    "external-session-usage-unknown",
                    Value::Null,
                    None,
                    false,
                ),
                StoredDecision::DeepSeek { request, response }
                | StoredDecision::Provider { request, response } => {
                    let billing = response.as_ref().and_then(|r| r.billing());
                    (
                        request.wire.target.provider().id(),
                        "provider-api-observation",
                        billing.map_or(Value::Null, |b| json!(b.usage)),
                        billing.map(|b| b.actual_model.clone()),
                        true,
                    )
                }
                StoredDecision::OpenAi { observation } => {
                    let bill = journal.billing_evidence(id)?;
                    if let Some(scoped) = bill.scoped_response.and_then(|r| r.billing.as_ref()) {
                        (
                            "openai",
                            "scoped-normalized-usage",
                            json!(scoped.usage),
                            Some(scoped.actual_model.clone()),
                            true,
                        )
                    } else if let Some(transport) = bill.transport {
                        (
                            "openai",
                            "provider-api-observation",
                            json!(transport.usage),
                            observation.as_ref().map(|o| o.actual_model.clone()),
                            true,
                        )
                    } else {
                        (
                            "openai",
                            "legacy-observation-without-wire-fingerprint",
                            observation.as_ref().map_or(Value::Null, |o| {
                                json!({
                                    "inputTokens":o.input_tokens, "outputTokens":o.output_tokens,
                                    "cacheReadTokens":null, "cacheWriteTokens":null
                                })
                            }),
                            observation.as_ref().map(|o| o.actual_model.clone()),
                            true,
                        )
                    }
                }
            };
            let response_id = op
                .deputy_response
                .as_ref()
                .and_then(|r| r.billing())
                .map(|b| &b.response_id)
                .or_else(|| {
                    op.scoped_response
                        .as_ref()?
                        .billing
                        .as_ref()
                        .map(|b| &b.response_id)
                })
                .or_else(|| op.observation.as_ref().map(|o| &o.response_id));
            let response_id_digest = response_id.map(digest).transpose()?;
            if let Some(hash) = &response_id_digest {
                require(
                    response_ids.insert((source, hash.clone())),
                    "management response identity reused by another operation",
                )?;
            }
            let status = statuses.iter().find(|s| s.operation_id == *id).unwrap();
            operations.push(json!({
                "operationId":id, "purpose":op.intent.purpose, "state":status.state,
                "source":source, "productApiIntent":product_api_intent,
                "requestedModel":op.intent.model, "actualModel":actual_model,
                "usageBasis":usage_basis, "usage":usage, "actualCost":null,
                "intentHash":op.intent_hash, "responseHash":op.response_hash,
                "responseIdDigest":response_id_digest,
                "chainHead":op.chain_hash, "createdAt":op.intent.created_at,
                "stateDigest":op.intent.state_digest, "decisionUsable":op.envelope().is_some(),
                "appliedAcknowledged":op.applied,
                "workflowEventHash":event.map(|e| &e.hash),
                "resolutionPresent":op.resolution.is_some(),
                "responseReceivedOrUnknownRecorded":op.finished,
            }));
        }
        // Reverse linkage catches a removed whole operation, even if the
        // remaining management directory consists entirely of valid chains.
        for event in history {
            if let Some(id) = event.body.operation_id.strip_prefix("mgmt-") {
                require(
                    selected.contains(id),
                    "workflow management event has no original management source",
                )?;
            }
        }
        let reviews: Vec<_> = journal
            .reviews
            .iter()
            .filter(|r| r.project_id.as_deref().is_none_or(|p| p == project))
            .map(|r| {
                json!({
                    "operationId":r.operation_id, "originalProjectId":r.project_id,
                    "sequence":r.sequence, "relativePath":r.relative_path,
                    "usage":null, "actualCost":null
                })
            })
            .collect();
        Ok(json!({
            "projectId":project, "basisDigest":journal.workspace_basis(project)?,
            "operations":operations, "linkedWorkflowEvents":linked,
            "reviewedFragments":reviews, "managementChainsVerified":true,
            "workflowApplicationsCrossChecked":true, "usageComplete":false,
            "actualCost":null, "managementRequestMs":null,
            "limitations":[
                "All recorded operations are retained, including failed, discarded or unapplied ones",
                "External Codex session tokens and subscription cost remain unknown",
                "Intent presence does not prove a transmitted or billable API request",
                "Local record integrity does not attest real model execution",
                "Provider-specific usage shapes require explicit normalization",
                "No provider bill or complete timing evidence is supplied"
            ]
        }))
    }
}
