#[path = "api_generation_product_tests.rs"]
mod product;
use super::*;
use crate::api_generation_resolution::APPROVAL_SOURCE;
use cerulean_domain::api_generation_resolution::ApiGenerationResolutionInput;
fn failed_transport(unpriced: bool) -> Fixture {
    Fixture {
        count: transport().count,
        send: Arc::new(move |prepared, round| {
            if !unpriced {
                return Err(ApiFailure::Transport {});
            }
            let mut v = response(round);
            v.as_object_mut().unwrap().remove("service_tier");
            fixture_scoped_api_result(prepared, v).map_err(|_| ApiFailure::InvalidResponse {})
        }),
    }
}
fn ready(root: &Path, unpriced: bool) -> RuntimeHost {
    let mut h = fixture(root, failed_transport(unpriced));
    h.start_managed_api("scoped-api-deputy", "attempt").unwrap();
    drain(&mut h);
    drop(h);
    crate::currency_api_resolution_guard::tests::promote_fixture(&root.join("data"));
    RuntimeHost::open(&root.join("data")).unwrap()
}
fn input(h: &RuntimeHost, amount: u64) -> ApiGenerationResolutionInput {
    ApiGenerationResolutionInput {
        project_id: "project".into(),
        request_id: h.currency_accounts["project"]
            .state()
            .bookings
            .keys()
            .next()
            .unwrap()
            .clone(),
        amount_usd_nanos: amount,
        billing_reference: "isolated invoice".into(),
        explanation: "explicit user reviewed generation fee".into(),
    }
}
#[test]
fn api_generation_resolution_host_user_event_gap_and_reopen_preserve_unknown_tools_and_count() {
    for unpriced in [false, true] {
        for gap in [false, true] {
            let t = tempfile::tempdir().unwrap();
            let data = t.path().join("data");
            let mut h = ready(t.path(), unpriced);
            let i = input(&h, 17);
            let id = i.request_id.clone();
            let review = h.review_api_generation_resolution(&i).unwrap();
            let before = snapshot(&data);
            let command = WorkflowCommand::ConfirmApiGenerationResolution {
                review_digest: review.digest().unwrap(),
            };
            let context = CommandContext {
                api_generation_resolution: Some(review.clone()),
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
                    .command("bad-generation-fee", principal, command.clone(), ctx, false)
                    .is_err());
                assert_eq!(snapshot(&data), before);
            }
            let counts = h.currency_accounts["project"].state().api_counts.clone();
            let original = h.currency_accounts["project"].state().bookings[&id]
                .outcome
                .clone();
            let original_task = h.projects["project"].state().tasks["task"].clone();
            let original_round = snapshot(&round_root(t.path(), 1));
            if gap {
                h.projects
                    .get_mut("project")
                    .unwrap()
                    .command(
                        "original-generation-user-gap",
                        "user",
                        command,
                        context,
                        false,
                    )
                    .unwrap();
                assert!(h.currency_accounts["project"].state().bookings[&id]
                    .fee_resolution
                    .is_none());
                drop(h);
                h = RuntimeHost::open(&data).unwrap();
            } else {
                h.confirm_api_generation_resolution(&review, &review.digest().unwrap())
                    .unwrap();
            }
            let b = &h.currency_accounts["project"];
            assert_eq!(b.state().charged_usd_nanos, 17);
            assert_eq!(b.state().reserved_usd_nanos, 1000);
            assert!(!b.state().blocked);
            assert_eq!(b.state().bookings[&id].outcome, original);
            assert_eq!(b.state().api_counts, counts);
            assert_eq!(h.projects["project"].state().tasks["task"], original_task);
            assert_eq!(
                h.api_jobs[&("project".into(), "attempt".into())].state,
                ApiJobState::Unknown
            );
            assert!(!t.path().join("project/sum.mjs").exists());
            assert_eq!(snapshot(&round_root(t.path(), 1)), original_round);
            let journal =
                crate::api_call_journal::ApiCallJournal::read_existing(&round_root(t.path(), 1))
                    .unwrap();
            assert!(b.api_fee_permit(&journal).is_err());
            assert!(journal.require_fee_ack().is_err());
            drop(journal);
            let view = &h.currency_account_views(None)[0];
            assert_eq!(view.unknown_requests, 0);
            assert_eq!(view.unpriced_requests, 0);
            let before = snapshot(&data);
            h.confirm_api_generation_resolution(&review, &review.digest().unwrap())
                .unwrap();
            assert_eq!(snapshot(&data), before);
            drop(h);
            let mut h = RuntimeHost::open(&data).unwrap();
            assert_eq!(snapshot(&data), before);
            h.confirm_api_generation_resolution(&review, &review.digest().unwrap())
                .unwrap();
            assert_eq!(snapshot(&data), before);
            // Count billing remains a separate user decision, in either order of history.
            let count_input = cerulean_domain::api_count_resolution::ApiCountResolutionInput {
                project_id: "project".into(),
                request_id: id,
                amount_usd_nanos: 3,
                billing_reference: "count invoice".into(),
                explanation: "separate count fee".into(),
            };
            let count_review = h.review_api_count_resolution(&count_input).unwrap();
            h.confirm_api_count_resolution(&count_review, &count_review.digest().unwrap())
                .unwrap();
            assert_eq!(h.currency_accounts["project"].state().charged_usd_nanos, 20);
            assert_eq!(h.currency_accounts["project"].state().reserved_usd_nanos, 0);
            drop(h);
            let h = RuntimeHost::open(&data).unwrap();
            assert_eq!(h.currency_accounts["project"].state().charged_usd_nanos, 20);
        }
    }
}
#[test]
fn api_generation_resolution_host_rejects_ledger_only_and_stale_confirmation() {
    let t = tempfile::tempdir().unwrap();
    let data = t.path().join("data");
    let mut h = ready(t.path(), true);
    let i = input(&h, 11);
    let stale = h.review_api_generation_resolution(&i).unwrap();
    let count_input = cerulean_domain::api_count_resolution::ApiCountResolutionInput {
        project_id: "project".into(),
        request_id: i.request_id.clone(),
        amount_usd_nanos: 0,
        billing_reference: "explicit zero count invoice".into(),
        explanation: "count reviewed separately".into(),
    };
    let count = h.review_api_count_resolution(&count_input).unwrap();
    h.confirm_api_count_resolution(&count, &count.digest().unwrap())
        .unwrap();
    let before = snapshot(&data);
    assert!(h
        .confirm_api_generation_resolution(&stale, &stale.digest().unwrap())
        .is_err());
    assert_eq!(snapshot(&data), before);
    let fresh = h.review_api_generation_resolution(&i).unwrap();
    h.currency_accounts
        .get_mut("project")
        .unwrap()
        .record_api_generation_resolution(fresh.resolution(&"e".repeat(64)))
        .unwrap();
    drop(h);
    let before = snapshot(&data.join("currency-budgets"));
    assert!(RuntimeHost::open(&data)
        .err()
        .unwrap()
        .contains("original user"));
    assert_eq!(snapshot(&data.join("currency-budgets")), before);
}
#[test]
fn api_generation_resolution_host_rejects_live_worker_then_preserves_stopped_unknown() {
    let t = tempfile::tempdir().unwrap();
    let data = t.path().join("data");
    let h = fixture(t.path(), transport());
    drop(h);
    crate::currency_api_resolution_guard::tests::promote_fixture(&data);
    let mut h = RuntimeHost::open(&data).unwrap();
    let entered = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let e = entered.clone();
    let d = done.clone();
    h.api_scoped_fixture = Some(Fixture {
        count: transport().count,
        send: Arc::new(move |_, _| {
            e.store(true, Ordering::SeqCst);
            let deadline = Instant::now() + Duration::from_secs(20);
            while !d.load(Ordering::SeqCst) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(2));
            }
            Err(ApiFailure::Transport {})
        }),
    });
    h.start_managed_api("scoped-api-deputy", "attempt").unwrap();
    let deadline = Instant::now() + Duration::from_secs(20);
    while !entered.load(Ordering::SeqCst) && Instant::now() < deadline {
        h.poll_api();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(entered.load(Ordering::SeqCst));
    let i = input(&h, 0);
    let rejected = h.review_api_generation_resolution(&i);
    done.store(true, Ordering::SeqCst);
    drain(&mut h);
    assert!(rejected.unwrap_err().contains("worker to stop"));
    let r = h.review_api_generation_resolution(&i).unwrap();
    h.confirm_api_generation_resolution(&r, &r.digest().unwrap())
        .unwrap();
    assert_eq!(
        h.api_jobs[&("project".into(), "attempt".into())].state,
        ApiJobState::Unknown
    );
    assert!(!t.path().join("project/sum.mjs").exists());
}
#[test]
fn api_generation_resolution_reader_preflight_covers_workflow_and_pending_ledger() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("data");
    crate::currency_api_resolution_guard::tests::reader11(&root);
    for relative in ["currency-budgets/project", "workflows/project/events"] {
        let dir = root.join(relative);
        fs::create_dir_all(&dir).unwrap();
        let suffixes: &[&str] = if relative.ends_with("events") {
            &["json"]
        } else {
            &["json", "pending"]
        };
        for suffix in suffixes {
            let path = dir.join(format!("00.{suffix}"));
            for body in [
                json!({"event":{"kind":"resolveApiGeneration"}}),
                json!({"context":{"apiGenerationResolution":{}}}),
                json!({"event":{"command":{"kind":"confirm-api-generation-resolution"}}}),
            ] {
                fs::write(&path, serde_json::to_vec(&json!({"body":body})).unwrap()).unwrap();
                let before = snapshot(&root);
                assert!(crate::Store::open(&root)
                    .err()
                    .unwrap()
                    .contains("reader12"));
                assert_eq!(snapshot(&root), before);
            }
            fs::remove_file(path).unwrap();
        }
    }
}
