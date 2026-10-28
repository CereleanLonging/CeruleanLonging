//! Original resident worker/coordinator/ledger and file tools, synthetic transport.
#[path = "api_generation_resolution_tests.rs"]
mod generation_resolution;
#[path = "api_count_product_tests.rs"]
mod product;
use super::*;
use crate::api_currency::Fixture;
use cerulean_adapters::api_generation::{
    scoped::{fixture_counted_api, fixture_scoped_api_result, PreparedScopedApi},
    ApiFailure,
};
use cerulean_domain::{api_jobs::ApiJobState, credentials::CredentialProvider};
use serde_json::{json, Value};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

fn response(round: u32) -> Value {
    let output = if round == 1 {
        json!([{"type":"function_call","id":"fc_1","call_id":"call_1","name":"cerulean_write_file","status":"completed",
            "arguments":json!({"path":"sum.mjs","content":"export function sum(a,b) { return a+b; }\n","expectedSha256":"absent"}).to_string()}])
    } else {
        json!([{"type":"message","id":"msg_final","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Implementation ready for independent verification."}]}])
    };
    json!({"id":format!("resp_scoped_worker_{round}"),"model":"fixture-worker-snapshot","status":"completed","service_tier":"default", "output":output,
        "usage":{"input_tokens":10,"output_tokens":20,"input_tokens_details":{"cached_tokens":2,"cache_write_tokens":1}}})
}
fn fixture(root: &Path, transport: Fixture) -> RuntimeHost {
    fixture_with_limit(root, transport, 12_345_678_901)
}
fn fixture_with_limit(root: &Path, transport: Fixture, limit: u64) -> RuntimeHost {
    fixture_with_configuration(root, transport, limit, |_| {})
}
fn fixture_with_configuration(
    root: &Path,
    transport: Fixture,
    limit: u64,
    configure: impl FnOnce(&mut cerulean_domain::project_setup::ProjectSetupDraft),
) -> RuntimeHost {
    let mut settings = super::scoped_decision::settings();
    settings.limit_usd_nanos = limit;
    for q in settings.scoped_quotes.as_mut().unwrap() {
        if q.requested_model == "vendor/fixture-model" {
            q.reported_models = vec!["fixture-worker-snapshot".into()];
            for b in &mut q.bands {
                b.actual_model = "fixture-worker-snapshot".into();
            }
        }
    }
    let h = super::scoped_count::fixture_with_configuration(
        root, true, settings, true, None, configure,
    );
    drop(h);
    crate::currency_scoped_execution_guard::tests::promote_fixture(&root.join("data"));
    crate::currency_api_guard::tests::promote_fixture(&root.join("data"));
    let mut h = RuntimeHost::open(&root.join("data")).unwrap();
    let i = intent(&h, "scoped-api-deputy", Purpose::Assign);
    let mut observation = result(&i).observation;
    observation.envelope.decision = ManagementDecision::Assign {
        task_id: "task".into(),
        worker_id: "worker-openai".into(),
        rationale: "Controlled approved worker assignment".into(),
    };
    // Reuse the existing synthetic assignment, not a claim about management transport.
    h.management.begin(i.clone()).unwrap();
    h.management
        .finish(&i.operation_id, Ok(observation))
        .unwrap();
    crate::credential_vault::save_for(
        &h.root,
        CredentialProvider::OpenAi,
        &secret::SecretInput::new("sk-isolated-scoped-worker-fixture".into()).unwrap(),
    )
    .unwrap();
    h.api_scoped_fixture = Some(transport);
    h
}

#[path = "api_supervision_currency_tests.rs"]
mod supervision;
fn transport() -> Fixture {
    Fixture {
        count: Arc::new(|draft, quote| {
            fixture_counted_api(draft, quote, 100).map_err(|_| ApiFailure::InvalidResponse {})
        }),
        send: Arc::new(|prepared, round| {
            fixture_scoped_api_result(prepared, response(round))
                .map_err(|_| ApiFailure::InvalidResponse {})
        }),
    }
}
fn drain(h: &mut RuntimeHost) {
    let deadline = Instant::now() + Duration::from_secs(25);
    while !h.api_active.is_empty() && Instant::now() < deadline {
        h.poll_api();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(h.api_active.is_empty(), "scoped worker still active");
}
fn round_root(root: &Path, n: u32) -> PathBuf {
    root.join("data/workflows/project")
        .join(format!("api-attempt-{}", digest(&"attempt").unwrap()))
        .join(format!("rounds/{n:03}"))
}
fn assert_count_view(h: &RuntimeHost, counts: [u32; 4], reserved: u64) {
    let book = &h.currency_accounts["project"];
    let head = book.head().to_owned();
    let state = book.state().clone();
    let before = snapshot(&h.root.join("currency-budgets"));
    for _ in 0..2 {
        let views = h.currency_account_views(None);
        let view = views.iter().find(|v| v.project_id == "project").unwrap();
        assert_eq!(view.reserved_usd_nanos, state.reserved_usd_nanos);
        assert_eq!(view.charged_usd_nanos, state.charged_usd_nanos);
        assert_eq!(view.available_usd_nanos, None);
        assert_eq!(
            view.api_counts,
            Some(cerulean_domain::currency_budget::ApiCountSummary {
                pending: counts[0],
                counted: counts[1],
                unknown: counts[2],
                counted_without_generation: counts[3],
                reserved_usd_nanos: reserved,
            })
        );
    }
    assert_eq!(book.head(), head);
    assert_eq!(book.state(), &state);
    assert_eq!(snapshot(&h.root.join("currency-budgets")), before);
}
#[test]
fn api_count_resolution_host_original_user_confirmation_and_publication_gap_reopen_once() {
    use crate::api_count_resolution::APPROVAL_SOURCE;
    use cerulean_domain::api_count_resolution::ApiCountResolutionInput;
    for gap in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let mut h = fixture(temp.path(), transport());
        h.start_managed_api("scoped-api-deputy", "attempt").unwrap();
        drain(&mut h);
        drop(h);
        crate::currency_api_resolution_guard::tests::promote_fixture(&data);
        let mut h = RuntimeHost::open(&data).unwrap();
        let request = h.currency_accounts["project"]
            .state()
            .api_counts
            .keys()
            .next()
            .unwrap()
            .clone();
        let input = ApiCountResolutionInput {
            project_id: "project".into(),
            request_id: request.clone(),
            amount_usd_nanos: 17,
            billing_reference: "synthetic API count invoice".into(),
            explanation: "isolated user reviewed amount".into(),
        };
        let before = snapshot(&data);
        let review = h.review_api_count_resolution(&input).unwrap();
        assert_eq!(snapshot(&data), before);
        let command = WorkflowCommand::ConfirmApiCountResolution {
            review_digest: review.digest().unwrap(),
        };
        let context = CommandContext {
            api_count_resolution: Some(review.clone()),
            approval_source: Some(APPROVAL_SOURCE.into()),
            ..Default::default()
        };
        for (principal, ctx) in [
            ("deputy", context.clone()),
            ("user", CommandContext::default()),
        ] {
            assert!(h
                .projects
                .get_mut("project")
                .unwrap()
                .command("bad-api-fee", principal, command.clone(), ctx, false)
                .is_err());
            assert_eq!(snapshot(&data), before);
        }
        let generation = h.currency_accounts["project"].state().bookings.clone();
        let charged = h.currency_accounts["project"].state().charged_usd_nanos;
        if gap {
            h.projects
                .get_mut("project")
                .unwrap()
                .command("user-api-fee-gap", "user", command, context, false)
                .unwrap();
            assert!(h.currency_accounts["project"].state().api_counts[&request]
                .fee_resolution
                .is_none());
            drop(h);
            h = RuntimeHost::open(&data).unwrap();
        } else {
            h.confirm_api_count_resolution(&review, &review.digest().unwrap())
                .unwrap();
        }
        assert_eq!(
            h.currency_accounts["project"].state().reserved_usd_nanos,
            1000
        );
        assert_eq!(
            h.currency_accounts["project"].state().charged_usd_nanos,
            charged + 17
        );
        assert_eq!(h.currency_accounts["project"].state().bookings, generation);
        assert_count_view(&h, [0, 2, 0, 0], 1000);
        let before = snapshot(&data);
        h.confirm_api_count_resolution(&review, &review.digest().unwrap())
            .unwrap();
        assert_eq!(snapshot(&data), before);
        let events = h.projects["project"].history().len();
        let money = h.currency_accounts["project"].state().clone();
        drop(h);
        let mut h = RuntimeHost::open(&data).unwrap();
        h.confirm_api_count_resolution(&review, &review.digest().unwrap())
            .unwrap();
        assert_eq!(h.currency_accounts["project"].state(), &money);
        assert_eq!(h.projects["project"].history().len(), events);
        assert_eq!(snapshot(&data), before);
    }
}
#[test]
fn api_count_resolution_host_refuses_ledger_only_confirmation_and_stale_review() {
    use cerulean_domain::api_count_resolution::ApiCountResolutionInput;
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut h = fixture(temp.path(), transport());
    h.start_managed_api("scoped-api-deputy", "attempt").unwrap();
    drain(&mut h);
    drop(h);
    crate::currency_api_resolution_guard::tests::promote_fixture(&data);
    let mut h = RuntimeHost::open(&data).unwrap();
    let ids: Vec<_> = h.currency_accounts["project"]
        .state()
        .api_counts
        .keys()
        .cloned()
        .collect();
    let input = |request: String| ApiCountResolutionInput {
        project_id: "project".into(),
        request_id: request,
        amount_usd_nanos: 0,
        billing_reference: "synthetic explicit zero bill".into(),
        explanation: "zero supplied by user, not inferred".into(),
    };
    let first = h
        .review_api_count_resolution(&input(ids[0].clone()))
        .unwrap();
    let stale = h
        .review_api_count_resolution(&input(ids[1].clone()))
        .unwrap();
    h.confirm_api_count_resolution(&first, &first.digest().unwrap())
        .unwrap();
    let before = snapshot(&data);
    assert!(h
        .confirm_api_count_resolution(&stale, &stale.digest().unwrap())
        .is_err());
    assert_eq!(snapshot(&data), before);
    let fresh = h
        .review_api_count_resolution(&input(ids[1].clone()))
        .unwrap();
    h.currency_accounts
        .get_mut("project")
        .unwrap()
        .record_api_count_resolution(fresh.resolution(&"f".repeat(64)))
        .unwrap();
    drop(h);
    let before = snapshot(&data);
    assert!(RuntimeHost::open(&data)
        .err()
        .unwrap()
        .contains("original user"));
    assert_eq!(snapshot(&data), before);
}
#[test]
fn api_count_resolution_host_waits_for_count_worker_and_preserves_unknown_execution() {
    use cerulean_domain::api_count_resolution::ApiCountResolutionInput;
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    drop(fixture(temp.path(), transport()));
    crate::currency_api_resolution_guard::tests::promote_fixture(&data);
    let mut h = RuntimeHost::open(&data).unwrap();
    let (entered, receiver) = std::sync::mpsc::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let wait = Mutex::new(wait);
    h.api_scoped_fixture = Some(Fixture {
        count: Arc::new(move |_, _| {
            entered.send(()).unwrap();
            wait.lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(25))
                .unwrap();
            Err(ApiFailure::Transport {})
        }),
        send: Arc::new(|_, _| panic!("fee confirmation must never trigger generation")),
    });
    h.start_managed_api("scoped-api-deputy", "attempt").unwrap();
    wait_entered(&mut h, &receiver);
    let request = h.currency_accounts["project"]
        .state()
        .api_counts
        .keys()
        .next()
        .unwrap()
        .clone();
    let input = ApiCountResolutionInput {
        project_id: "project".into(),
        request_id: request.clone(),
        amount_usd_nanos: 13,
        billing_reference: "synthetic count bill".into(),
        explanation: "unknown count fee explicitly reviewed".into(),
    };
    let head = h.currency_accounts["project"].head().to_owned();
    let rejected = h.review_api_count_resolution(&input);
    release.send(()).unwrap();
    assert!(rejected.unwrap_err().contains("worker to stop"));
    assert_eq!(h.currency_accounts["project"].head(), head);
    drain(&mut h);
    let review = h.review_api_count_resolution(&input).unwrap();
    assert_eq!(review.original_outcome, "unknown");
    h.confirm_api_count_resolution(&review, &review.digest().unwrap())
        .unwrap();
    assert_eq!(
        h.api_jobs[&("project".into(), "attempt".into())].state,
        ApiJobState::Unknown
    );
    assert_eq!(h.currency_accounts["project"].state().reserved_usd_nanos, 0);
    assert_eq!(h.currency_accounts["project"].state().charged_usd_nanos, 13);
    assert!(h.currency_accounts["project"].state().bookings.is_empty());
    let money = h.currency_accounts["project"].state().clone();
    drop(h);
    let h = RuntimeHost::open(&data).unwrap();
    assert_eq!(h.currency_accounts["project"].state(), &money);
    assert!(matches!(
        money.api_counts[&request].outcome,
        crate::currency_budget::ApiCountOutcome::Unknown { .. }
    ));
}
#[test]
fn scoped_api_host_two_rounds_settle_before_tools_and_reopen_without_recount() {
    let temp = tempfile::tempdir().unwrap();
    let order = Arc::new(Mutex::new(vec![]));
    let count_n = Arc::new(AtomicUsize::new(0));
    let r = temp.path().to_path_buf();
    let c_order = order.clone();
    let c_n = count_n.clone();
    let c_root = r.clone();
    let s_order = order.clone();
    let s_root = r.clone();
    let transport = Fixture {
        count: Arc::new(move |draft, quote| {
            let n = c_n.fetch_add(1, Ordering::SeqCst) + 1;
            c_order.lock().unwrap().push(format!("count{n}"));
            if n == 2 {
                assert!(round_root(&c_root, 1).join("02.json").exists());
                assert!(c_root.join("project/sum.mjs").exists());
            }
            fixture_counted_api(draft, quote, 100).map_err(|_| ApiFailure::InvalidResponse {})
        }),
        send: Arc::new(move |prepared: &PreparedScopedApi, n| {
            s_order.lock().unwrap().push(format!("send{n}"));
            assert!(round_root(&s_root, n).join("00.json").exists());
            assert!(!round_root(&s_root, n).join("02.json").exists());
            if n == 1 {
                assert!(!s_root.join("project/sum.mjs").exists());
            }
            fixture_scoped_api_result(prepared, response(n))
                .map_err(|_| ApiFailure::InvalidResponse {})
        }),
    };
    let mut h = fixture(&r, transport);
    let empty = serde_json::to_value(&h.currency_account_views(None)[0]).unwrap();
    assert!(empty.get("apiCounts").is_none());
    h.start_managed_api("scoped-api-deputy", "attempt").unwrap();
    drain(&mut h);
    let job = &h.api_jobs[&("project".into(), "attempt".into())];
    assert_eq!(job.state, ApiJobState::Submitted, "{:?}", job.message);
    assert_eq!(
        *order.lock().unwrap(),
        vec!["count1", "send1", "count2", "send2"]
    );
    assert_eq!(count_n.load(Ordering::SeqCst), 2);
    let state = h.currency_accounts["project"].state().clone();
    assert_eq!(state.api_counts.len(), 2);
    assert_eq!(
        state
            .bookings
            .values()
            .filter(|b| matches!(b.outcome, Outcome::Settled { .. }))
            .count(),
        2
    );
    assert_eq!(state.reserved_usd_nanos, 2000);
    assert_count_view(&h, [0, 2, 0, 0], 2000);
    for n in [1, 2] {
        assert!(round_root(&r, n).join("02.json").exists());
    }
    assert!(fs::read_to_string(r.join("project/sum.mjs"))
        .unwrap()
        .contains("a+b"));
    drop(h);
    let before = snapshot(&r.join("data/currency-budgets"));
    let h = RuntimeHost::open(&r.join("data")).unwrap();
    assert_eq!(h.currency_accounts["project"].state(), &state);
    assert_eq!(snapshot(&r.join("data/currency-budgets")), before);
    assert_count_view(&h, [0, 2, 0, 0], 2000);
    assert_eq!(count_n.load(Ordering::SeqCst), 2);
}

#[test]
fn scoped_api_checkpoint_recovery_requires_original_fee_ack_and_never_recounts() {
    let temp = tempfile::tempdir().unwrap();
    let counts = Arc::new(AtomicUsize::new(0));
    let capture = counts.clone();
    let transport = Fixture {
        count: Arc::new(move |draft, quote| {
            capture.fetch_add(1, Ordering::SeqCst);
            fixture_counted_api(draft, quote, 100).map_err(|_| ApiFailure::InvalidResponse {})
        }),
        send: Arc::new(|prepared, n| {
            fixture_scoped_api_result(prepared, response(n))
                .map_err(|_| ApiFailure::InvalidResponse {})
        }),
    };
    let mut host = fixture(temp.path(), transport);
    host.start_managed_api("scoped-api-deputy", "attempt")
        .unwrap();
    let crate::api_tool_checkpoint::Request { checkpoint, reply } =
        crate::host::api_tool_checkpoint_host::tests::next_checkpoint(&mut host);
    host.accept_api_tool_checkpoint("project", "attempt", checkpoint)
        .unwrap();
    let evidence = host
        .api_checkpoint_evidence("project", "attempt")
        .unwrap()
        .unwrap();
    reply.send(Ok(None)).unwrap();
    drain(&mut host);
    assert_eq!(
        host.api_jobs[&("project".into(), "attempt".into())].state,
        ApiJobState::Submitted
    );
    let root = round_root(temp.path(), 1)
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let restored: crate::ApiToolEvidence =
        serde_json::from_slice(&serde_json::to_vec(&evidence).unwrap()).unwrap();
    restored.verify_original(&root).unwrap();
    assert_eq!(counts.load(Ordering::SeqCst), 2);
    let book = host.currency_accounts["project"].state().clone();
    restored.verify_original(&root).unwrap();
    assert_eq!(host.currency_accounts["project"].state(), &book);
    let ack = round_root(temp.path(), 1).join("02.json");
    fs::remove_file(&ack).unwrap();
    assert!(restored.verify_original(&root).is_err());
    assert!(
        !ack.exists(),
        "evidence inspection cannot repair a missing fee ACK"
    );
    assert_eq!(counts.load(Ordering::SeqCst), 2);
    assert_eq!(host.currency_accounts["project"].state(), &book);
    assert!(temp.path().join("project/sum.mjs").is_file());
}
#[test]
fn scoped_api_host_unpriced_or_invalid_response_stops_before_tools_or_next_count() {
    for mode in ["missing-tier", "invalid-tools", "transport"] {
        let temp = tempfile::tempdir().unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        let t = Fixture {
            count: Arc::new(move |draft, quote| {
                c.fetch_add(1, Ordering::SeqCst);
                fixture_counted_api(draft, quote, 100).map_err(|_| ApiFailure::InvalidResponse {})
            }),
            send: Arc::new(move |p, round| {
                if mode == "transport" {
                    return Err(ApiFailure::Transport {});
                }
                let mut value = response(round);
                if mode == "missing-tier" {
                    value.as_object_mut().unwrap().remove("service_tier");
                }
                if mode == "invalid-tools" {
                    value["output"][0]["name"] = json!("unapproved_tool");
                }
                fixture_scoped_api_result(p, value).map_err(|_| ApiFailure::InvalidResponse {})
            }),
        };
        let mut h = fixture(temp.path(), t);
        h.start_managed_api("scoped-api-deputy", "attempt").unwrap();
        drain(&mut h);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(
            h.api_jobs[&("project".into(), "attempt".into())].state,
            ApiJobState::Unknown
        );
        assert!(!temp.path().join("project/sum.mjs").exists());
        assert_eq!(h.currency_accounts["project"].state().bookings.len(), 1);
        let outcome = &h.currency_accounts["project"]
            .state()
            .bookings
            .values()
            .next()
            .unwrap()
            .outcome;
        match mode {
            "invalid-tools" => assert!(matches!(outcome, Outcome::Settled { .. }), "{outcome:?}"),
            "missing-tier" => assert!(matches!(outcome, Outcome::Unpriced { .. }), "{outcome:?}"),
            _ => assert!(matches!(outcome, Outcome::Unknown { .. }), "{outcome:?}"),
        }
    }
}
#[test]
fn scoped_api_host_production_gate_refuses_before_startup_or_count() {
    let temp = tempfile::tempdir().unwrap();
    let mut h = fixture(temp.path(), transport());
    h.api_scoped_fixture = None;
    let before = snapshot(&h.root);
    assert!(h
        .start_managed_api("scoped-api-deputy", "attempt")
        .unwrap_err()
        .contains("金额预算尚不可执行"));
    assert_eq!(snapshot(&h.root), before);
    assert!(h.api_active.is_empty());
    assert!(h.currency_accounts["project"].state().api_counts.is_empty());
}

fn wait_entered(h: &mut RuntimeHost, entered: &std::sync::mpsc::Receiver<()>) {
    let until = Instant::now() + Duration::from_secs(25);
    loop {
        match entered.try_recv() {
            Ok(()) => return,
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(e) => panic!("fixture gate disconnected: {e}"),
        }
        assert!(
            Instant::now() < until,
            "fixture gate was never reached: {:?}",
            h.api_jobs
        );
        h.poll_api();
        std::thread::sleep(Duration::from_millis(2));
    }
}
#[test]
fn scoped_api_host_cancelled_count_or_generation_retains_original_fee_without_tools() {
    for at_count in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let (entered, receiver) = std::sync::mpsc::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let wait = Arc::new(Mutex::new(wait));
        let sends = Arc::new(AtomicUsize::new(0));
        let sent = sends.clone();
        let ce = entered.clone();
        let cw = wait.clone();
        let t = Fixture {
            count: Arc::new(move |d, q| {
                if at_count {
                    ce.send(()).unwrap();
                    cw.lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(25))
                        .unwrap();
                }
                fixture_counted_api(d, q, 100).map_err(|_| ApiFailure::InvalidResponse {})
            }),
            send: Arc::new(move |p, n| {
                sent.fetch_add(1, Ordering::SeqCst);
                if !at_count {
                    entered.send(()).unwrap();
                    wait.lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(25))
                        .unwrap();
                }
                fixture_scoped_api_result(p, response(n))
                    .map_err(|_| ApiFailure::InvalidResponse {})
            }),
        };
        let mut h = fixture(temp.path(), t);
        h.start_managed_api("scoped-api-deputy", "attempt").unwrap();
        wait_entered(&mut h, &receiver);
        assert_count_view(&h, [u32::from(at_count), u32::from(!at_count), 0, 0], 1000);
        h.cancel_api(
            "project",
            "attempt",
            "controlled cancellation during original request",
        )
        .unwrap();
        release.send(()).unwrap();
        drain(&mut h);
        assert_eq!(sends.load(Ordering::SeqCst), usize::from(!at_count));
        let state = h.currency_accounts["project"].state().clone();
        assert_eq!(state.api_counts.len(), 1);
        assert!(matches!(
            state.api_counts.values().next().unwrap().outcome,
            crate::currency_budget::ApiCountOutcome::Counted { .. }
        ));
        assert_eq!(state.bookings.len(), usize::from(!at_count));
        if !at_count {
            assert!(matches!(
                state.bookings.values().next().unwrap().outcome,
                Outcome::Settled { .. }
            ));
        }
        assert_eq!(state.reserved_usd_nanos, 1000);
        assert_count_view(&h, [0, 1, 0, u32::from(at_count)], 1000);
        assert!(!temp.path().join("project/sum.mjs").exists());
        assert!(!round_root(temp.path(), 1).join("02.json").exists());
        drop(h);
        let h = RuntimeHost::open(&temp.path().join("data")).unwrap();
        assert_eq!(h.currency_accounts["project"].state(), &state);
        assert_count_view(&h, [0, 1, 0, u32::from(at_count)], 1000);
    }
}
#[test]
fn scoped_api_host_count_failure_or_insufficient_generation_balance_never_generates() {
    for failure in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let t = Fixture {
            count: Arc::new(move |d, q| {
                if failure {
                    Err(ApiFailure::Transport {})
                } else {
                    fixture_counted_api(d, q, 100).map_err(|_| ApiFailure::InvalidResponse {})
                }
            }),
            send: Arc::new(|_, _| panic!("generation must not run")),
        };
        let mut h = fixture_with_limit(temp.path(), t, 1001);
        h.start_managed_api("scoped-api-deputy", "attempt").unwrap();
        drain(&mut h);
        let state = h.currency_accounts["project"].state().clone();
        assert_eq!(state.reserved_usd_nanos, 1000);
        assert!(state.bookings.is_empty());
        assert_eq!(state.api_counts.len(), 1);
        assert_eq!(
            matches!(
                state.api_counts.values().next().unwrap().outcome,
                crate::currency_budget::ApiCountOutcome::Unknown { .. }
            ),
            failure
        );
        assert_count_view(
            &h,
            [
                0,
                u32::from(!failure),
                u32::from(failure),
                u32::from(!failure),
            ],
            1000,
        );
        assert!(!temp.path().join("project/sum.mjs").exists());
        drop(h);
        let h = RuntimeHost::open(&temp.path().join("data")).unwrap();
        assert_eq!(h.currency_accounts["project"].state(), &state);
        assert_count_view(
            &h,
            [
                0,
                u32::from(!failure),
                u32::from(failure),
                u32::from(!failure),
            ],
            1000,
        );
    }
}
#[test]
fn scoped_api_host_pause_resume_during_count_invalidates_generation_authority() {
    use cerulean_domain::project_setup::{ProjectRun, RunState};
    let temp = tempfile::tempdir().unwrap();
    let (entered, receiver) = std::sync::mpsc::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let wait = Mutex::new(wait);
    let t = Fixture {
        count: Arc::new(move |d, q| {
            entered.send(()).unwrap();
            wait.lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(25))
                .unwrap();
            fixture_counted_api(d, q, 100).map_err(|_| ApiFailure::InvalidResponse {})
        }),
        send: Arc::new(|_, _| panic!("count from before pause must not generate")),
    };
    let mut h = fixture(temp.path(), t);
    let revision = h.projects["project"]
        .revision()
        .unwrap()
        .approval_digest()
        .unwrap();
    h.start_managed_api("scoped-api-deputy", "attempt").unwrap();
    // Install the synthetic run before servicing the dispatch request. Poll only
    // the API owner; the normal project runner's economic gate remains closed.
    let mut run = ProjectRun {
        id: format!("run-{}", &digest(&("project", &revision)).unwrap()[..32]),
        project_id: "project".into(),
        revision_digest: revision,
        setup_digest: h.project_setups["project"].digest.clone(),
        state: RunState::Active,
        message: "Controlled original run authorization".into(),
        acknowledged_decisions: h.management_operation_total() as u32,
    };
    h.runs.save(run.clone(), "user").unwrap();
    wait_entered(&mut h, &receiver);
    run.state = RunState::Paused;
    h.runs.save(run.clone(), "user").unwrap();
    run.state = RunState::Active;
    h.runs.save(run, "user").unwrap();
    release.send(()).unwrap();
    drain(&mut h);
    let state = h.currency_accounts["project"].state();
    assert_eq!(state.api_counts.len(), 1);
    assert!(matches!(
        state.api_counts.values().next().unwrap().outcome,
        crate::currency_budget::ApiCountOutcome::Counted { .. }
    ));
    assert!(state.bookings.is_empty());
    assert_eq!(state.reserved_usd_nanos, 1000);
    assert!(!temp.path().join("project/sum.mjs").exists());
}

#[test]
fn scoped_api_host_lost_prepare_or_fee_ack_keeps_original_liability_and_never_runs_tools() {
    use crate::{api_currency::Request, async_api::Update};
    for at_prepare in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let (entered, receiver) = std::sync::mpsc::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let wait = Arc::new(Mutex::new(wait));
        let ce = entered.clone();
        let cw = wait.clone();
        let sends = Arc::new(AtomicUsize::new(0));
        let sent = sends.clone();
        let t = Fixture {
            count: Arc::new(move |d, q| {
                if at_prepare {
                    ce.send(()).unwrap();
                    cw.lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(25))
                        .unwrap();
                }
                fixture_counted_api(d, q, 100).map_err(|_| ApiFailure::InvalidResponse {})
            }),
            send: Arc::new(move |p, n| {
                sent.fetch_add(1, Ordering::SeqCst);
                if !at_prepare {
                    entered.send(()).unwrap();
                    wait.lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(25))
                        .unwrap();
                }
                fixture_scoped_api_result(p, response(n))
                    .map_err(|_| ApiFailure::InvalidResponse {})
            }),
        };
        let mut h = fixture(temp.path(), t);
        h.start_managed_api("scoped-api-deputy", "attempt").unwrap();
        wait_entered(&mut h, &receiver);
        release.send(()).unwrap();
        let update = h.api_active[&("project".into(), "attempt".into())]
            .receiver
            .recv_timeout(Duration::from_secs(25))
            .unwrap();
        let Update::Currency(mut request) = update else {
            panic!("expected original money request")
        };
        let original_reply: Box<dyn std::any::Any> = match request.as_mut() {
            Request::Counted { reply, .. } if at_prepare => {
                let (replacement, lost) = tokio::sync::oneshot::channel();
                let original = std::mem::replace(reply, replacement);
                drop(lost);
                Box::new(original)
            }
            Request::Settle { reply, .. } if !at_prepare => {
                let (replacement, lost) = tokio::sync::oneshot::channel();
                let original = std::mem::replace(reply, replacement);
                drop(lost);
                Box::new(original)
            }
            _ => panic!("unexpected money request phase"),
        };
        h.handle_api_currency("project", "attempt", *request);
        // Lose acknowledgement AFTER the original Host publication, rather
        // than racing an earlier worker stop against the authorization check.
        drop(original_reply);
        drain(&mut h);
        assert_eq!(sends.load(Ordering::SeqCst), usize::from(!at_prepare));
        assert!(!temp.path().join("project/sum.mjs").exists());
        assert!(!round_root(temp.path(), 1).join("02.json").exists());
        let reserved = h.currency_accounts["project"].state().reserved_usd_nanos;
        let outcome = &h.currency_accounts["project"]
            .state()
            .bookings
            .values()
            .next()
            .unwrap()
            .outcome;
        if at_prepare {
            assert!(matches!(outcome, Outcome::Reserved));
        } else {
            assert!(matches!(outcome, Outcome::Settled { .. }));
        }
        drop(h);
        let h = RuntimeHost::open(&temp.path().join("data")).unwrap();
        assert_eq!(
            h.currency_accounts["project"].state().reserved_usd_nanos,
            reserved
        );
        let outcome = &h.currency_accounts["project"]
            .state()
            .bookings
            .values()
            .next()
            .unwrap()
            .outcome;
        if at_prepare {
            assert!(matches!(outcome, Outcome::Unknown { .. }));
        } else {
            assert!(matches!(outcome, Outcome::Settled { .. }));
        }
        drop(h);
        let before = snapshot(&temp.path().join("data"));
        drop(RuntimeHost::open(&temp.path().join("data")).unwrap());
        assert_eq!(snapshot(&temp.path().join("data")), before);
    }
}
#[test]
fn scoped_api_host_response_publication_failure_preserves_reserve_without_tools_or_resend() {
    let temp = tempfile::tempdir().unwrap();
    let r = temp.path().to_path_buf();
    let sends = Arc::new(AtomicUsize::new(0));
    let sent = sends.clone();
    let mut t = transport();
    t.send = Arc::new(move |p, n| {
        sent.fetch_add(1, Ordering::SeqCst);
        fs::write(
            round_root(&r, n).join("01.pending"),
            b"owned interrupted response publication",
        )
        .unwrap();
        fixture_scoped_api_result(p, response(n)).map_err(|_| ApiFailure::InvalidResponse {})
    });
    let mut h = fixture(temp.path(), t);
    h.start_managed_api("scoped-api-deputy", "attempt").unwrap();
    drain(&mut h);
    assert_eq!(sends.load(Ordering::SeqCst), 1);
    assert!(!temp.path().join("project/sum.mjs").exists());
    assert!(h.currency_accounts["project"].state().reserved_usd_nanos > 1000);
    assert!(!round_root(temp.path(), 1).join("01.json").exists());
    drop(h);
    let before = snapshot(&temp.path().join("data"));
    assert!(RuntimeHost::open(&temp.path().join("data")).is_err());
    assert_eq!(snapshot(&temp.path().join("data")), before);
}
