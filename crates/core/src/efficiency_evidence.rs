//! Offline evidence export, using the original ledger/round/verifier readers.
//! No model, credentials, recovery, billing settlement, or scheduling.
use crate::{api_call_journal::CallOutcome, api_execution::CallInventory, host::ordinary};
use cerulean_domain::{execution::*, workflow::WorkflowCommand};
use serde_json::{json, Value};
use std::path::Path;

/// Include management records while excluding an active RuntimeHost writer.
/// Only existing lock files are opened; no root, lock, or journal is created.
pub fn read_with_management(
    data_root: &Path,
    project: &str,
    expected_revision: &str,
    expected_head: &str,
) -> Result<Value, String> {
    use fs2::FileExt;
    validate_root(data_root)?;
    let path = data_root.join("writer.lock");
    ordinary(&path, false)?;
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    lock.try_lock_exclusive()
        .map_err(|_| "data writer is active; offline management export refused")?;
    read_inner(data_root, project, expected_revision, expected_head, true)
}

/// The caller supplies the expected original revision and ledger head. This
/// detects a stale selection, not provider authenticity or experiment reality.
/// Monetary runs require their account reader and are deliberately unsupported
/// here; this function never bypasses that check or settles unknown charges.
pub fn read(
    data_root: &Path,
    project: &str,
    expected_revision: &str,
    expected_head: &str,
) -> Result<Value, String> {
    read_inner(data_root, project, expected_revision, expected_head, false)
}

fn validate_root(data_root: &Path) -> Result<(), String> {
    require(
        data_root.is_absolute()
            && !data_root
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir)),
        "evidence root must be absolute without parent traversal",
    )?;
    for parent in data_root.ancestors() {
        ordinary(parent, true)?;
    }
    Ok(())
}

// Crate callers including the baseline runner hold the original root writer
// lock themselves when including management. Public entry points acquire it.
pub(crate) fn read_inner(
    data_root: &Path,
    project: &str,
    expected_revision: &str,
    expected_head: &str,
    include_management: bool,
) -> Result<Value, String> {
    identifier(project)?;
    sha256(expected_revision)?;
    sha256(expected_head)?;
    validate_root(data_root)?;
    ordinary(&data_root.join("workflows"), true)?;
    let workflow = data_root.join("workflows").join(project);
    CallInventory::inspect_existing(&workflow, |store, inventory| {
        let history = store.history();
        let last = history.last().ok_or("original ledger is empty")?;
        let revision = store.revision().ok_or("original revision is missing")?;
        require(
            revision.project.id == project
                && revision.approval_digest()? == expected_revision
                && last.hash == expected_head,
            "original project/revision/head selection changed",
        )?;
        // Inventory replays original approved prefixes, identities, round chains,
        // tool completion seals and orphan detection, while keeping call locks.
        let mut calls = Vec::new();
        let mut reply_count = 0;
        let mut not_sent = 0;
        let mut uncertain = 0;
        let mut rejected_measured = 0;
        let mut response_ids = std::collections::BTreeSet::new();
        for journal in inventory.journals() {
            let id = journal.identity();
            let (outcome, usage, actual_model, response_digest) = match &journal.state().outcome {
                Some(CallOutcome::Reply { observation }) => {
                    observation
                        .usage
                        .total_tokens(id.fingerprint.target.provider())?;
                    reply_count += 1;
                    (
                        "reply",
                        Some(observation.usage.clone()),
                        Some(observation.actual_model.clone()),
                        Some(digest(&observation.response_id)?),
                    )
                }
                Some(CallOutcome::Failure {
                    failure: cerulean_adapters::api_generation::ApiFailure::NotSent {},
                }) => {
                    not_sent += 1;
                    ("not-sent", None, None, None)
                }
                Some(CallOutcome::Failure { .. }) if journal.state().rejected_usage.is_some() => {
                    let measured = journal.state().rejected_usage.as_ref().unwrap();
                    measured.validate(&id.fingerprint)?;
                    rejected_measured += 1;
                    (
                        "rejected-with-observed-usage",
                        Some(measured.usage.clone()),
                        Some(measured.actual_model.clone()),
                        Some(digest(&measured.response_id)?),
                    )
                }
                Some(CallOutcome::Failure { .. }) => {
                    uncertain += 1;
                    ("failure-remote-usage-unknown", None, None, None)
                }
                None => {
                    uncertain += 1;
                    ("intent-without-outcome", None, None, None)
                }
            };
            if let Some(response) = &response_digest {
                require(
                    response_ids.insert((
                        id.fingerprint.target.provider().id().to_owned(),
                        response.clone(),
                    )),
                    "duplicate API response identity; usage cannot be summed",
                )?;
            }
            calls.push(json!({
                "identity":id, "journalHead":journal.head(), "outcome":outcome,
                "usage":usage, "actualModel":actual_model,
                "responseIdDigest":response_digest, "actualCost":null,
            }));
        }
        // Hash-bound verification stages and logs are checked against the actual
        // original Verify event. No checks are launched and no state reconciled.
        let mut verifications = Vec::new();
        for entry in history {
            let WorkflowCommand::Verify {
                task,
                attempt,
                passed,
                ..
            } = &entry.body.event.command
            else {
                continue;
            };
            let job = VerificationJob {
                project_id: project.into(),
                operation_id: entry.body.operation_id.clone(),
                task_id: task.clone(),
                attempt_id: attempt.clone(),
                state: if *passed {
                    VerificationState::Succeeded
                } else {
                    VerificationState::Failed
                },
                started_at: entry.body.received_at.clone(),
                updated_at: entry.body.received_at.clone(),
                message: None,
                reconciliation_digest: None,
            };
            let basis = crate::verification_detail::basis(store, &job)?;
            let detail = crate::verification_detail::read(data_root, &job, &basis)?;
            verifications.push(json!({
                "ledgerHash":entry.hash, "passed":passed,
                "statusSource":"original-Verify-event-not-job-status-cache",
                "detail":detail,
            }));
        }
        let management = include_management
            .then(|| {
                crate::management_execution::DecisionJournal::efficiency_evidence(
                    data_root, project, history,
                )
            })
            .transpose()?;
        Ok(json!({
            "schema":if rejected_measured > 0 { 2 } else { 1 }, "kind":"original-worker-and-verification-evidence",
            "projectId":project, "revisionDigest":expected_revision,
            "ledgerHead":expected_head, "ledgerEventCount":history.len(),
            "firstLedgerEventAt":history.first().map(|e| &e.body.received_at),
            "lastLedgerEventAt":last.body.received_at,
            "taskStates":store.state().tasks, "apiCalls":calls,
            "apiIntentCount":reply_count+not_sent+uncertain+rejected_measured,
            "apiReplies":reply_count, "apiNotSent":not_sent,
            "apiUncertainRemoteUsage":uncertain, "verifications":verifications,
            "ledgerAndApiChainsVerified":true, "recordedVerificationFilesVerified":true,
            "realityAttested":false, "managementUsageIncluded":include_management,
            "management":management,
            "wallMs":null, "humanActiveMs":null, "actualCost":null,
            "experimentContentFullyVerified":false, "efficiencyGainProven":false,
            "releaseAccepted":false,
            "limitations":[
                "Local integrity does not prove a real provider call; synthetic logs can be valid",
                "API intent count is not a billable request count",
                "Usage is the original provider-specific observation; missing values remain unknown",
                if include_management {
                    "Recorded management observations are included; session usage, billing and complete timing remain unknown"
                } else {
                    "Management usage and billing are not included"
                },
                "Only ledger-recorded checks are inspected; pending jobs are not declared complete",
                "Ledger timestamps are not a full experimental wall-clock measurement",
                "Task suite, seed, configuration and authorization still need experiment binding"
            ]
        }))
    })
}

#[cfg(test)]
mod management_tests;
#[cfg(test)]
mod tests;
