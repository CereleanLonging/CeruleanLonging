//! Advice uses the original count liability even if authorization changes before generation.
use super::*;
use cerulean_adapters::openai::scoped_pricing::{
    fixture_counted_scoped_decision, fixture_scoped_result,
};
use cerulean_domain::supervision::SupervisionPolicy;

#[test]
fn api_advice_pause_during_count_preserves_liability_and_reopens_count_only_intent() {
    exercise(true, false);
}
#[test]
fn api_advice_scoped_generation_saves_original_fee_ack_and_reopens_without_delivery() {
    exercise(false, false);
}
#[test]
fn api_automatic_advice_scoped_link_precedes_count_and_original_fee_ack() {
    exercise(false, true);
}
fn exercise(pause: bool, delivery: bool) {
    let temp = tempfile::tempdir().unwrap();
    let evidence_root = temp
        .path()
        .join("data/workflows/project")
        .join(format!("api-attempt-{}", digest(&"attempt").unwrap()));
    let count_index = Arc::new(AtomicUsize::new(0));
    let counter = count_index.clone();
    let transport = Fixture {
        count: Arc::new(move |draft, quote| {
            let n = counter.fetch_add(1, Ordering::SeqCst);
            if delivery && n == 1 {
                let path = std::fs::read_dir(&evidence_root)
                    .unwrap()
                    .map(|e| e.unwrap().path())
                    .find(|p| {
                        p.file_name()
                            .unwrap()
                            .to_string_lossy()
                            .starts_with("advice-")
                            && p.extension().is_some_and(|e| e == "json")
                    })
                    .unwrap();
                let link: crate::api_advice_link::Link =
                    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
                assert_eq!(link.next_call.fingerprint, draft.fingerprint().request);
                assert!(
                    !evidence_root.join("rounds/002").exists(),
                    "link must precede generation journal and count transport"
                );
            }
            fixture_counted_api(draft, quote, 100).map_err(|_| ApiFailure::InvalidResponse {})
        }),
        send: Arc::new(move |prepared, round| {
            if delivery && round == 2 {
                use cerulean_adapters::api_generation::{
                    scoped::fixture_scoped_api_messages, ApiMessage,
                };
                assert!(
                    matches!(fixture_scoped_api_messages(prepared).last(), Some(ApiMessage::User(s)) if s.contains("Stop repeating unavailable file reads."))
                );
            }
            let mut value = response(round);
            if round == 1 {
                value["output"] = json!(["first", "second"].map(|id| json!({"type":"function_call","id":format!("fc_{id}"),"call_id":id,
                    "name":"cerulean_read_file","status":"completed","arguments":json!({"path":"missing.txt","offset":0}).to_string()})));
            }
            fixture_scoped_api_result(prepared, value).map_err(|_| ApiFailure::InvalidResponse {})
        }),
    };
    let mut host = fixture_with_configuration(temp.path(), transport, 12_345_678_901, |draft| {
        draft.management_max_requests = 5;
        draft.supervision = Some(SupervisionPolicy {
            schema_version: 1,
            repeated_failure_threshold: 2,
            max_advice_per_attempt: 1,
            delivery_enabled: delivery,
        });
    });
    // The inherited assignment has no scoped fee receipt. Explicitly close and
    // account for it, then request a fresh assignment through the real scoped path.
    let original = host.management_detail("scoped-api-deputy").unwrap();
    host.resolve_management(
        "scoped-api-deputy",
        &original.evidence_digest,
        UserResolution::Discard {
            reason: "Use scoped fixture assignment".into(),
        },
    )
    .unwrap();
    for operation in ["ui-plan-fixture-1", "scoped-api-deputy"] {
        let scope = crate::currency_budget::HistoricalScope::Management {
            operation_id: operation.into(),
            original_project_id: Some("project".into()),
        };
        let review = host
            .review_historical_charge(
                "project",
                &scope,
                17,
                "synthetic original management bill",
                "isolated fixture only",
            )
            .unwrap();
        host.confirm_historical_charge(&review, &review.digest().unwrap())
            .unwrap();
    }
    let (entered, receiver) = std::sync::mpsc::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let wait = Mutex::new(wait);
    let counts = Arc::new(AtomicUsize::new(0));
    let recorded = counts.clone();
    host.openai_connection.key_configured = true;
    host.management_scoped_fixture = Some(management_scoped_host::ScopedFixture {
        count: Arc::new(move |draft, quote| {
            if draft.fingerprint().purpose == Purpose::Advise {
                recorded.fetch_add(1, Ordering::SeqCst);
                entered.send(()).unwrap();
                wait.lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(20))
                    .unwrap();
            }
            fixture_counted_scoped_decision(draft, quote, 100)
        }),
        send: Arc::new(move |i, prepared| {
            assert!(
                i.purpose == Purpose::Assign || (!pause && i.purpose == Purpose::Advise),
                "paused advice must not generate"
            );
            let mut value = super::super::scoped_decision::response(&i);
            let decision = if i.purpose == Purpose::Assign {
                json!({"action":"assign","taskId":"task","workerId":"worker-openai","rationale":"original scoped assignment"})
            } else {
                json!({"action":"advise","taskId":"task","suggestion":"Stop repeating unavailable file reads.","rationale":"Original API tool failures."})
            };
            value["output"][1]["content"][0]["text"] =
                json!(json!({"stateDigest":i.state_digest,"decision":decision}).to_string());
            fixture_scoped_result(&prepared, value)
        }),
        fail_count_spawn: false,
        fail_generation_spawn: false,
    });
    let mut assignment = intent(&host, "scoped-original-assignment", Purpose::Assign);
    assignment.context = host
        .management_context("project", &host.project_setups["project"].setup.requirement)
        .unwrap();
    assignment.state_digest = digest(&assignment.context).unwrap();
    host.start_scoped_management(assignment).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while host.counting_project("project") || !host.management_active.is_empty() {
        assert!(Instant::now() < deadline);
        host.poll_management();
        std::thread::sleep(Duration::from_millis(2));
    }
    host.start_managed_api("scoped-original-assignment", "attempt")
        .unwrap();
    let revision = host.project_setups["project"].digest.clone();
    let approved = host.api_writers["project"]
        .owner
        .snapshot()
        .unwrap()
        .revision_digest
        .unwrap();
    host.start_project_run("project", &approved, &revision)
        .unwrap();
    let first = crate::host::api_tool_checkpoint_host::tests::next_checkpoint(&mut host);
    let reply = if delivery {
        host.handle_api_tool_checkpoint("project", "attempt", first);
        None
    } else {
        host.accept_api_tool_checkpoint("project", "attempt", first.checkpoint)
            .unwrap();
        assert!(host.request_api_supervision("project", "attempt").unwrap());
        Some(first.reply)
    };
    receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    let operation = host
        .management_original_intents("project")
        .values()
        .find(|i| i.purpose == Purpose::Advise)
        .unwrap()
        .operation_id
        .clone();
    let reserved = host.currency_accounts["project"].state().reserved_usd_nanos;
    let run = host.runs.all().remove(0);
    if pause {
        host.pause_project_run(&run.id).unwrap();
    }
    release.send(()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while host.counting_project("project") || !host.management_active.is_empty() {
        assert!(Instant::now() < deadline);
        host.poll_management();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(counts.load(Ordering::SeqCst), 1);
    if pause {
        assert!(host.management.intent(&operation).is_err());
        assert_eq!(
            host.currency_accounts["project"].state().reserved_usd_nanos,
            reserved
        );
    } else {
        host.management_fee_ack(&operation).unwrap();
        let detail = host.management_detail(&operation).unwrap();
        let advice = detail.supervision_advice.unwrap();
        assert_eq!(
            advice.suggestion.as_deref(),
            Some("Stop repeating unavailable file reads.")
        );
        assert!(advice.current && !advice.delivered);
        assert!(host.management.application(&operation).is_none());
    }
    assert!(matches!(
        host.currency_accounts["project"].state().management_counts[&operation].outcome,
        crate::currency_budget::CountOutcome::ScopedCounted { .. }
    ));
    assert!(!host.request_api_supervision("project", "attempt").unwrap());
    if let Some(reply) = reply {
        reply
            .send(if pause {
                Err("original user pause".into())
            } else {
                Ok(None)
            })
            .unwrap();
    } else {
        host.poll_api_advice_delivery("project", "attempt").unwrap();
    }
    drain(&mut host);
    if delivery {
        assert_eq!(count_index.load(Ordering::SeqCst), 2);
        assert_eq!(
            host.api_advice_delivery_state(host.management.intent(&operation).unwrap())
                .unwrap(),
            "responseObserved"
        );
    }
    host.validate_api_supervision_requests().unwrap();
    let before = host.currency_accounts["project"].state().clone();
    drop(host);
    let mut reopened = RuntimeHost::open(&temp.path().join("data")).unwrap();
    reopened.orchestrating = true;
    assert_eq!(reopened.currency_accounts["project"].state(), &before);
    assert!(reopened
        .management_original_intents("project")
        .contains_key(&operation));
    for _ in 0..3 {
        reopened.poll_verifications();
    }
    assert_eq!(reopened.currency_accounts["project"].state(), &before);
    if pause {
        assert!(reopened.management.intent(&operation).is_err());
    } else {
        reopened.management_fee_ack(&operation).unwrap();
        let detail = reopened.management_detail(&operation).unwrap();
        let advice = detail.supervision_advice.unwrap();
        assert!(!advice.current);
        assert_eq!(advice.delivered, delivery);
        assert_eq!(
            advice.suggestion.as_deref(),
            Some("Stop repeating unavailable file reads.")
        );
        assert_eq!(
            reopened.management.application(&operation).is_some(),
            delivery
        );
    }
}
