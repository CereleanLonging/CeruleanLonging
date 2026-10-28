//! Real approved workflow, original worker channel and file tools; synthetic model transport.
use super::super::api_tool_checkpoint_host::tests::{next_checkpoint, response};
use super::*;
use cerulean_adapters::openai::DecisionObservation;
use cerulean_domain::{
    api_worker::ApiTarget, management::DecisionEnvelope, supervision::SupervisionPolicy,
};
use std::time::{Duration, Instant};

fn manager(intent: DecisionIntent) -> Result<DecisionObservation, String> {
    assert_eq!(intent.purpose, Purpose::Advise);
    assert_eq!(intent.model, "fixture-deputy");
    let bound = binding(&intent)?.ok_or("missing API binding")?;
    let original = evidence(&intent, &bound)?;
    assert_eq!(original.round(), 1);
    let details = &intent.context["supervisionFailureContext"];
    assert_eq!(details["schemaVersion"], 1);
    assert_eq!(details["incidentId"], bound.incident_id);
    let failures = details["failures"].as_array().unwrap();
    assert_eq!(failures.len(), 2);
    // Result order, not lexicographic RPC identity; no file/write contents.
    for (item, id) in failures.iter().zip(["10", "2"]) {
        assert_eq!(item["callId"], id);
        assert_eq!(item["toolName"], "cerulean_read_file");
        assert_eq!(item["path"], "missing.txt");
        assert_eq!(item["offset"], 0);
        assert_eq!(item["error"], "File unavailable or concurrently modified");
        assert!(item.get("content").is_none() && item.get("arguments").is_none());
    }
    Ok(DecisionObservation {
        response_id: "resp_api_advice".into(),
        requested_model: intent.model.clone(),
        actual_model: intent.model,
        input_tokens: 1,
        output_tokens: 1,
        envelope: DecisionEnvelope {
            state_digest: intent.state_digest,
            decision: ManagementDecision::Advise {
                task_id: bound.task_id,
                suggestion: "Stop repeating unavailable file reads; inspect the approved task."
                    .into(),
                rationale: "Two original tool failures.".into(),
            },
        },
    })
}
fn start(root: &Path) -> RuntimeHost {
    start_with_delivery(root, false)
}
fn start_with_delivery(root: &Path, delivery_enabled: bool) -> RuntimeHost {
    start_with_transport(
        root,
        delivery_enabled,
        if delivery_enabled {
            automatic_response
        } else {
            response
        },
    )
}
fn start_with_transport(
    root: &Path,
    delivery_enabled: bool,
    transport: crate::async_api::Fixture,
) -> RuntimeHost {
    let mut host = api_host_tests::fixture_with_configuration(
        root,
        project_setup::ProjectWorkerAdapter::Openai,
        ApiTarget::OpenAi {},
        true,
        |draft| {
            draft.worker_max_requests = 3;
            draft.management_max_requests = 5;
            draft.supervision = Some(SupervisionPolicy {
                schema_version: 1,
                repeated_failure_threshold: 2,
                max_advice_per_attempt: 1,
                delivery_enabled,
            });
        },
        |_, _| {},
    );
    host.set_openai_key(Some(
        secret::SecretInput::new("sk-synthetic-api-advice".into()).unwrap(),
    ))
    .unwrap();
    host.management_fixture = Some(manager);
    host.api_fixture = Some(transport);
    let revision = host.projects["project"]
        .revision()
        .unwrap()
        .approval_digest()
        .unwrap();
    let setup = host.project_setups["project"].digest.clone();
    host.start_managed_api("deputy-api", "attempt").unwrap();
    // This fixture already has a manual assignment. Authorize the run after
    // dispatch so polling does not mistake that Ready request for a run-owned one.
    host.start_project_run("project", &revision, &setup)
        .unwrap();
    host
}
fn wait_advice(host: &mut RuntimeHost, operation: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while host.management_active.contains_key(operation) {
        assert!(
            Instant::now() < deadline,
            "management fixture did not finish"
        );
        host.poll_management();
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn automatic_response(
    prepared: &cerulean_adapters::api_generation::PreparedRequest,
    round: u32,
) -> Result<
    cerulean_adapters::api_generation::ApiReply,
    cerulean_adapters::api_generation::ApiFailure,
> {
    use cerulean_adapters::api_generation::{fixture_messages, ApiMessage};
    if round >= 2 {
        assert_eq!(fixture_messages(prepared).iter().filter(|m| matches!(m, ApiMessage::User(s) if s.contains("Stop repeating unavailable file reads; inspect the approved task."))).count(), 1);
    }
    response(prepared, round)
}

#[test]
fn api_automatic_advice_enters_original_next_request_once_and_reopens_without_resend() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = start_with_delivery(temp.path(), true);
    // The real worker prepares this request after receiving the Host result.
    // Inspect the actual provider-bound conversation, not a parallel test copy.
    // The fixture is captured at launch, so start_with_delivery sets this sender.
    let first = next_checkpoint(&mut host);
    host.handle_api_tool_checkpoint("project", "attempt", first);
    assert!(host.api_active[&("project".into(), "attempt".into())]
        .advice_waiter
        .is_some());
    let operation = host
        .management
        .statuses()
        .into_iter()
        .find(|s| s.purpose == Purpose::Advise)
        .unwrap()
        .operation_id;
    wait_advice(&mut host, &operation);
    host.poll_project_runs();
    assert_eq!(host.runs.all()[0].state, RunState::Active);
    assert!(host.management.application(&operation).unwrap().1);
    let second = next_checkpoint(&mut host);
    let root = host
        .api_original_scope("project", "attempt", &second.checkpoint.run)
        .unwrap();
    let link: crate::api_advice_link::Link =
        crate::api_execution::load(&root, &format!("advice-{operation}.json")).unwrap();
    assert_eq!(link.next_call, second.checkpoint.call);
    assert_eq!(link.reference.previous_call.round, 1);
    assert_eq!(link.next_call.round, 2);
    host.handle_api_tool_checkpoint("project", "attempt", second);
    // Reuse the shared-acceptance fixture guard: retain an Active run while
    // preventing automatic verification from taking workflow ownership.
    host.orchestrating = true;
    api_host_tests::drain(&mut host);
    let intent = host.management.intent(&operation).unwrap().clone();
    assert_eq!(
        host.api_advice_delivery_state(&intent).unwrap(),
        "responseObserved"
    );
    assert_eq!(
        host.projects["project"]
            .history()
            .iter()
            .filter(|e| e.body.operation_id == format!("mgmt-{operation}"))
            .count(),
        1
    );
    host.validate_api_supervision_requests().unwrap();
    let operations = host.management_operation_ids("project");
    let original_link = fs::read(root.join(format!("advice-{operation}.json"))).unwrap();
    drop(host);
    let mut reopened = RuntimeHost::open(&temp.path().join("data")).unwrap();
    reopened.orchestrating = true;
    for _ in 0..3 {
        reopened.poll_verifications();
    }
    assert_eq!(reopened.management_operation_ids("project"), operations);
    assert_eq!(
        reopened.api_advice_delivery_state(&intent).unwrap(),
        "responseObserved"
    );
    assert_eq!(
        fs::read(root.join(format!("advice-{operation}.json"))).unwrap(),
        original_link
    );
    // Evidence mismatch must be diagnosed without manufacturing another send.
    let mut changed: serde_json::Value = serde_json::from_slice(&original_link).unwrap();
    changed["nextCall"]["fingerprint"]["bodySha256"] = serde_json::json!("0".repeat(64));
    fs::write(
        root.join(format!("advice-{operation}.json")),
        serde_json::to_vec(&changed).unwrap(),
    )
    .unwrap();
    assert!(reopened.api_advice_delivery_state(&intent).is_err());
    drop(reopened);
    assert!(RuntimeHost::open(&temp.path().join("data")).is_err());
}

#[test]
fn api_automatic_advice_stops_at_revoked_boundary_or_lost_application_ack() {
    for mode in ["pause", "cancel", "unknown", "lost-ack"] {
        let temp = tempfile::tempdir().unwrap();
        let mut host = start_with_delivery(temp.path(), true);
        if mode == "unknown" {
            host.management_fixture = Some(|_| Err("synthetic management response lost".into()));
        }
        let first = next_checkpoint(&mut host);
        let root = host
            .api_original_scope("project", "attempt", &first.checkpoint.run)
            .unwrap();
        host.handle_api_tool_checkpoint("project", "attempt", first);
        let operation = host
            .management
            .statuses()
            .into_iter()
            .find(|s| s.purpose == Purpose::Advise)
            .unwrap()
            .operation_id;
        wait_advice(&mut host, &operation);
        match mode {
            "pause" => {
                let run = host.runs.all()[0].clone();
                host.pause_project_run(&run.id).unwrap();
            }
            "cancel" => {
                host.api_active
                    .get_mut(&("project".into(), "attempt".into()))
                    .unwrap()
                    .cancel
                    .take()
                    .unwrap()
                    .send(())
                    .unwrap();
            }
            "lost-ack" => host.api_writers["project"]
                .owner
                .lose_next_acknowledgement(),
            _ => {}
        }
        assert!(
            host.poll_api_advice_delivery("project", "attempt").is_err(),
            "{mode}"
        );
        api_host_tests::drain(&mut host);
        assert!(!root.join("rounds/002").exists(), "{mode}");
        assert!(
            !root.join(format!("advice-{operation}.json")).exists(),
            "{mode}"
        );
        let events = host.projects["project"]
            .history()
            .iter()
            .filter(|e| e.body.operation_id == format!("mgmt-{operation}"))
            .count();
        assert_eq!(events, usize::from(mode == "lost-ack"));
        let operations = host.management_operation_ids("project");
        drop(host);
        let mut reopened = RuntimeHost::open(&temp.path().join("data")).unwrap();
        for _ in 0..3 {
            reopened.poll_verifications();
        }
        assert_eq!(reopened.management_operation_ids("project"), operations);
        assert_eq!(
            reopened.projects["project"]
                .history()
                .iter()
                .filter(|e| e.body.operation_id == format!("mgmt-{operation}"))
                .count(),
            events
        );
        assert!(!root.join("rounds/002").exists(), "{mode}");
    }
}

#[test]
fn api_automatic_advice_unknown_next_reply_preserves_original_link_without_resend() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = start_with_transport(temp.path(), true, |prepared, round| {
        if round == 2 {
            use cerulean_adapters::api_generation::{fixture_messages, ApiFailure, ApiMessage};
            assert!(
                matches!(fixture_messages(prepared).last(), Some(ApiMessage::User(s)) if s.contains("Deputy advice"))
            );
            Err(ApiFailure::Transport {})
        } else {
            response(prepared, round)
        }
    });
    let first = next_checkpoint(&mut host);
    let root = host
        .api_original_scope("project", "attempt", &first.checkpoint.run)
        .unwrap();
    host.handle_api_tool_checkpoint("project", "attempt", first);
    let operation = host
        .management
        .statuses()
        .into_iter()
        .find(|s| s.purpose == Purpose::Advise)
        .unwrap()
        .operation_id;
    wait_advice(&mut host, &operation);
    host.poll_project_runs();
    host.orchestrating = true;
    api_host_tests::drain(&mut host);
    let intent = host.management.intent(&operation).unwrap().clone();
    assert_eq!(
        host.api_advice_delivery_state(&intent).unwrap(),
        "requestOutcomeUnknown"
    );
    assert!(!host.api_supervision_advice_view(&intent).unwrap().delivered);
    let link = std::fs::read(root.join(format!("advice-{operation}.json"))).unwrap();
    assert!(root.join("rounds/002").exists());
    assert!(!root.join("rounds/003").exists());
    let operations = host.management_operation_ids("project");
    drop(host);
    let mut reopened = RuntimeHost::open(&temp.path().join("data")).unwrap();
    for _ in 0..3 {
        reopened.poll_verifications();
    }
    assert_eq!(reopened.management_operation_ids("project"), operations);
    assert_eq!(
        reopened.api_advice_delivery_state(&intent).unwrap(),
        "requestOutcomeUnknown"
    );
    assert_eq!(
        std::fs::read(root.join(format!("advice-{operation}.json"))).unwrap(),
        link
    );
    assert!(!root.join("rounds/003").exists());
}
#[test]
fn api_advice_owner_publication_keeps_worker_rights_and_recovers_lost_local_ack_once() {
    use crate::{
        management_execution::ApplicationTarget, workflow_access::WorkflowAccess,
        workflow_coordinator::Scope,
    };
    for lose_ack in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let mut host = start(temp.path());
        let first = next_checkpoint(&mut host);
        host.accept_api_tool_checkpoint("project", "attempt", first.checkpoint)
            .unwrap();
        assert!(host.request_api_supervision("project", "attempt").unwrap());
        let operation = host
            .management
            .statuses()
            .into_iter()
            .find(|s| s.purpose == Purpose::Advise)
            .unwrap()
            .operation_id;
        wait_advice(&mut host, &operation);
        let intent = host.management.intent(&operation).unwrap().clone();
        let original: ExecutionSnapshot =
            serde_json::from_value(intent.context["workflow"].clone()).unwrap();
        let ManagementDecision::Advise {
            task_id,
            suggestion,
            ..
        } = host
            .management
            .observation(&operation)
            .unwrap()
            .envelope
            .decision
            .clone()
        else {
            panic!("original deputy suggestion missing")
        };
        let command = WorkflowCommand::Advise {
            task: task_id,
            suggestion,
        };
        let local_operation = format!("mgmt-{operation}");
        let owner = &host.api_writers["project"].owner;
        let before = owner.snapshot().unwrap();
        let mut client = owner
            .client(Scope {
                project: "project".into(),
                task: "task".into(),
                attempt: "attempt".into(),
                revision: original.revision_digest.clone().unwrap(),
            })
            .unwrap();
        assert!(
            client
                .command(
                    &local_operation,
                    "deputy",
                    command.clone(),
                    CommandContext::default(),
                    true
                )
                .is_err(),
            "API worker client must never gain management publication rights"
        );
        drop(client);
        for mode in [
            "principal",
            "attempt",
            "snapshot",
            "task",
            "command",
            "context",
            "operation",
            "empty",
            "oversize",
        ] {
            let mut snapshot = original.clone();
            let mut principal = "deputy".to_owned();
            let mut attempt = "attempt".to_owned();
            let mut candidate = command.clone();
            let mut context = CommandContext::default();
            let mut op = local_operation.clone();
            match mode {
                "principal" => principal = "chief".into(),
                "attempt" => attempt = "other-attempt".into(),
                "snapshot" => snapshot.sequence += 1,
                "task" => {
                    candidate = WorkflowCommand::Advise {
                        task: "other-task".into(),
                        suggestion: "invalid".into(),
                    }
                }
                "command" => {
                    candidate = WorkflowCommand::Accept {
                        task: "task".into(),
                        attempt: "attempt".into(),
                    }
                }
                "context" => context.thread_id = Some("invented-provider-thread".into()),
                "operation" => op = "worker-invented".into(),
                "empty" => {
                    candidate = WorkflowCommand::Advise {
                        task: "task".into(),
                        suggestion: " ".into(),
                    }
                }
                _ => {
                    candidate = WorkflowCommand::Advise {
                        task: "task".into(),
                        suggestion: "x".repeat(16 * 1024 + 1),
                    }
                }
            }
            assert!(
                owner
                    .publish_management(snapshot, op, principal, attempt, candidate, context)
                    .is_err(),
                "{mode}"
            );
            assert_eq!(
                digest(&owner.snapshot().unwrap()).unwrap(),
                digest(&before).unwrap(),
                "{mode}"
            );
        }
        // Exercise the trusted local application layer using the original
        // saved response. This fixture does not authorize or simulate delivery.
        let target = ApplicationTarget::Workflow {
            principal: "deputy".into(),
            command: command.clone(),
            context: Box::default(),
        };
        let fee = host.management_fee_ack(&operation).unwrap();
        host.management
            .prepare_application_with_fee(&operation, target, &intent.state_digest, fee)
            .unwrap();
        let owner = &host.api_writers["project"].owner;
        if lose_ack {
            owner.lose_next_acknowledgement();
        }
        let result = owner.publish_management(
            original.clone(),
            local_operation.clone(),
            "deputy".into(),
            "attempt".into(),
            command.clone(),
            CommandContext::default(),
        );
        if lose_ack {
            assert!(result.is_err());
        } else {
            let entry = result.unwrap();
            host.management
                .record_applied(&operation, Some(entry.hash))
                .unwrap();
        }
        let writer = host.api_writers.get_mut("project").unwrap();
        writer.refresh().unwrap();
        assert_eq!(writer.snapshot.sequence, before.sequence + 1);
        assert!(
            writer
                .owner
                .publish_management(
                    original,
                    local_operation.clone(),
                    "deputy".into(),
                    "attempt".into(),
                    command,
                    CommandContext::default()
                )
                .is_err(),
            "never republish a stale application"
        );
        // End this local-publication fixture at its original boundary. It must
        // not pretend that recording Advise sent advice or permits a next call.
        first
            .reply
            .send(Err(
                "fixture stops after local application; no remote delivery".into(),
            ))
            .unwrap();
        api_host_tests::drain(&mut host);
        let history = host.projects["project"].history().to_vec();
        assert_eq!(
            history
                .iter()
                .filter(|e| e.body.operation_id == local_operation)
                .count(),
            1
        );
        let original_hash = history
            .iter()
            .find(|e| e.body.operation_id == local_operation)
            .unwrap()
            .hash
            .clone();
        assert_eq!(
            crate::api_execution::prior_requests(&host.projects["project"]).unwrap(),
            3,
            "original request reservation remains charged after stopping"
        );
        let original_root = host.projects["project"]
            .api_attempt_path("attempt")
            .unwrap();
        assert!(original_root.join("rounds/001").is_dir());
        assert!(
            !original_root.join("rounds/002").exists(),
            "no follow-up request was prepared or sent"
        );
        drop(host);
        let reopened = RuntimeHost::open(&temp.path().join("data")).unwrap();
        assert_eq!(reopened.projects["project"].history(), history.as_slice());
        let (_, applied, hash) = reopened.management.application(&operation).unwrap();
        assert!(applied);
        assert_eq!(hash, Some(original_hash.as_str()));
        assert!(
            !reopened
                .api_supervision_advice_view(reopened.management.intent(&operation).unwrap())
                .unwrap()
                .delivered
        );
    }
}

#[test]
fn api_advice_busy_management_revisits_original_incident_without_resend_or_paused_dispatch() {
    for paused in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let mut host = start(temp.path());
        let first = next_checkpoint(&mut host);
        host.accept_api_tool_checkpoint("project", "attempt", first.checkpoint)
            .unwrap();
        let (completed, probe) = std::sync::mpsc::channel();
        host.openai_probe = Some(probe);
        let original_ids = host.management_operation_ids("project");
        host.poll_project_runs();
        assert_eq!(host.management_operation_ids("project"), original_ids);
        if paused {
            let run = host.runs.all().remove(0);
            host.pause_project_run(&run.id).unwrap();
        }
        completed.send(Ok(vec!["fixture-deputy".into()])).unwrap();
        host.poll_verifications();
        assert!(host.openai_probe.is_none());
        if paused {
            assert_eq!(host.management_operation_ids("project"), original_ids);
            first
                .reply
                .send(Err("user paused while management unavailable".into()))
                .unwrap();
        } else {
            let operation = host
                .management
                .statuses()
                .into_iter()
                .find(|s| s.purpose == Purpose::Advise)
                .unwrap()
                .operation_id;
            wait_advice(&mut host, &operation);
            let ids = host.management_operation_ids("project");
            assert_eq!(ids.len(), original_ids.len() + 1);
            for _ in 0..3 {
                host.poll_project_runs();
            }
            assert_eq!(host.management_operation_ids("project"), ids);
            assert_eq!(host.runs.all()[0].state, RunState::Active);
            first.reply.send(Ok(None)).unwrap();
            let second = next_checkpoint(&mut host);
            host.handle_api_tool_checkpoint("project", "attempt", second);
        }
        api_host_tests::drain(&mut host);
        host.validate_api_supervision_requests().unwrap();
    }
}
#[test]
fn original_api_advice_is_requested_once_becomes_stale_and_reopens_without_delivery() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = start(temp.path());
    let first = next_checkpoint(&mut host);
    host.accept_api_tool_checkpoint("project", "attempt", first.checkpoint)
        .unwrap();
    assert!(host.request_api_supervision("project", "attempt").unwrap());
    let operation = host
        .management
        .statuses()
        .into_iter()
        .find(|s| s.purpose == Purpose::Advise)
        .unwrap()
        .operation_id;
    wait_advice(&mut host, &operation);
    let intent = host.management.intent(&operation).unwrap().clone();
    let view = host.api_supervision_advice_view(&intent).unwrap();
    assert!(view.current);
    assert!(!view.delivered);
    assert_eq!(view.delivery_state.as_deref(), Some("recordOnly"));
    assert_eq!(view.api.unwrap().round, 1);
    assert!(view.thread_id.is_empty() && view.turn_id.is_empty());
    assert!(view.suggestion.is_some());
    assert!(!host.request_api_supervision("project", "attempt").unwrap());
    // A ready record must wait without putting the running worker in Attention.
    host.poll_project_runs();
    assert_eq!(host.runs.all()[0].state, RunState::Active);
    first.reply.send(Ok(None)).unwrap();
    let second = next_checkpoint(&mut host);
    host.handle_api_tool_checkpoint("project", "attempt", second);
    api_host_tests::drain(&mut host);
    assert_eq!(
        host.api_jobs[&("project".into(), "attempt".into())].state,
        cerulean_domain::api_jobs::ApiJobState::Submitted
    );
    assert!(!host.api_supervision_advice_view(&intent).unwrap().current);
    assert!(host.management.application(&operation).is_none());
    let before = host.management_operation_ids("project");
    host.validate_api_supervision_requests().unwrap();
    let bound = binding(&intent).unwrap().unwrap();
    for (field, replacement) in [
        ("error", serde_json::json!("Invented failure")),
        ("path", serde_json::json!("unrelated.txt")),
        ("offset", serde_json::json!(99)),
        ("callId", serde_json::json!("write-result")),
        ("resultSequence", serde_json::json!(99)),
        ("resultSha256", serde_json::json!("0".repeat(64))),
    ] {
        let mut changed = intent.clone();
        changed.context["supervisionFailureContext"]["failures"][0][field] = replacement;
        changed.state_digest = digest(&changed.context).unwrap();
        assert!(
            host.validate_api_supervision_request(&changed, &bound)
                .is_err(),
            "{field}"
        );
    }
    // The prior request format is still understood; absence is not backfilled.
    let mut previous_format = intent.clone();
    previous_format
        .context
        .as_object_mut()
        .unwrap()
        .remove("supervisionFailureContext");
    previous_format.state_digest = digest(&previous_format.context).unwrap();
    host.validate_api_supervision_request(&previous_format, &bound)
        .unwrap();
    for field in ["runDigest", "incidentId", "observationDigest"] {
        let mut changed = intent.clone();
        changed.context["supervision"][field] = serde_json::json!("0".repeat(64));
        changed.state_digest = digest(&changed.context).unwrap();
        let altered = binding(&changed).unwrap().unwrap();
        assert!(
            host.validate_api_supervision_request(&changed, &altered)
                .is_err(),
            "{field}"
        );
    }
    let mut changed = intent.clone();
    changed.model = "another-model".into();
    assert!(host
        .validate_api_supervision_request(&changed, &bound)
        .is_err());
    let detail = host.management_detail(&operation).unwrap();
    assert!(detail.can_discard);
    host.resolve_management(
        &operation,
        &detail.evidence_digest,
        crate::management_execution::UserResolution::Discard {
            reason: "Reviewed record-only suggestion".into(),
        },
    )
    .unwrap();
    assert_eq!(host.management_operation_ids("project"), before);
    drop(host);
    let mut reopened = RuntimeHost::open(&temp.path().join("data")).unwrap();
    for _ in 0..3 {
        reopened.poll_verifications();
    }
    assert_eq!(reopened.management_operation_ids("project"), before);
    assert!(reopened.management.application(&operation).is_none());
    assert!(
        !reopened
            .api_supervision_advice_view(reopened.management.intent(&operation).unwrap())
            .unwrap()
            .delivered
    );
}

#[test]
fn unknown_api_advice_retains_original_request_and_pause_invalidates_current_evidence() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = start(temp.path());
    host.management_fixture = Some(|_| Err("synthetic reply lost".into()));
    let first = next_checkpoint(&mut host);
    host.accept_api_tool_checkpoint("project", "attempt", first.checkpoint)
        .unwrap();
    assert!(host.request_api_supervision("project", "attempt").unwrap());
    let operation = host
        .management
        .statuses()
        .into_iter()
        .find(|s| s.purpose == Purpose::Advise)
        .unwrap()
        .operation_id;
    wait_advice(&mut host, &operation);
    assert_eq!(
        host.management
            .statuses()
            .into_iter()
            .find(|s| s.operation_id == operation)
            .unwrap()
            .state,
        DecisionState::Unknown
    );
    assert!(!host.request_api_supervision("project", "attempt").unwrap());
    let intent = host.management.intent(&operation).unwrap().clone();
    assert!(host.revalidate_api_supervision(&intent).is_ok());
    let run = host.runs.all().remove(0);
    host.pause_project_run(&run.id).unwrap();
    assert!(host.revalidate_api_supervision(&intent).is_err());
    first.reply.send(Err("paused fixture".into())).unwrap();
    api_host_tests::drain(&mut host);
    let before = host.management_operation_ids("project");
    drop(host);
    let mut reopened = RuntimeHost::open(&temp.path().join("data")).unwrap();
    for _ in 0..3 {
        reopened.poll_verifications();
    }
    assert_eq!(reopened.management_operation_ids("project"), before);
    let detail = reopened.management_detail(&operation).unwrap();
    assert!(
        !detail.can_discard,
        "Unknown worker must be reconciled before management disposition"
    );
    assert!(detail.supervision_advice.unwrap().suggestion.is_none());
}

/// Produces isolated, stopped, credential-free data for testing an actual frozen reader.
#[test]
#[ignore = "requires an explicit new fixture directory"]
fn export_original_advice_for_frozen_reader() {
    let root = PathBuf::from(std::env::var("API_SUPERVISION_FIXTURE_ROOT").unwrap());
    fs::create_dir(&root).unwrap();
    let mut host = start(&root);
    let first = next_checkpoint(&mut host);
    host.handle_api_tool_checkpoint("project", "attempt", first);
    let second = next_checkpoint(&mut host);
    host.handle_api_tool_checkpoint("project", "attempt", second);
    api_host_tests::drain(&mut host);
    let operation = host
        .management
        .statuses()
        .into_iter()
        .find(|s| s.purpose == Purpose::Advise)
        .unwrap()
        .operation_id;
    wait_advice(&mut host, &operation);
    host.poll_project_runs();
    assert_eq!(host.runs.all()[0].state, RunState::Attention);
    host.set_openai_key(None).unwrap();
    host.validate_api_supervision_requests().unwrap();
    drop(host);
    let reopened = RuntimeHost::open(&root.join("data")).unwrap();
    assert!(!reopened.openai_connection.key_configured);
    assert!(reopened
        .api_supervision_advice_view(reopened.management.intent(&operation).unwrap())
        .unwrap()
        .suggestion
        .is_some());
}
