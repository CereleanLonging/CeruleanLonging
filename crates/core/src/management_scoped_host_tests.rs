//! Host queues and original durable boundaries, with in-process transport only.
use super::*;
use super::{scoped_count as sc, scoped_decision as sd};
use crate::currency_budget::CountOutcome;
use crate::management_execution::ApplicationTarget;
use cerulean_adapters::openai::scoped_pricing::{
    fixture_counted_scoped_decision, fixture_expired_scoped_count, fixture_scoped_result,
    ScopedDecisionDraft,
};
use cerulean_domain::project_setup::{ProjectRun, RunState};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc, Arc, Mutex,
};
use std::time::{Duration, Instant};

fn fixture(root: &Path, purpose: Purpose, limit: u32) -> RuntimeHost {
    let h = sc::fixture_with_limit(
        root,
        purpose != Purpose::Plan,
        sd::settings(),
        true,
        Some(limit),
    );
    drop(h);
    crate::currency_scoped_execution_guard::tests::promote_fixture(&root.join("data"));
    let mut h = RuntimeHost::open(&root.join("data")).unwrap();
    if purpose != Purpose::Plan {
        let scope = crate::currency_budget::HistoricalScope::Management {
            operation_id: "ui-plan-fixture-1".into(),
            original_project_id: Some("project".into()),
        };
        let review = h
            .review_historical_charge(
                "project",
                &scope,
                17,
                "synthetic historical planning reference",
                "fixture only; no supplier invoice",
            )
            .unwrap();
        h.confirm_historical_charge(&review, &review.digest().unwrap())
            .unwrap();
    }
    h
}
struct Gate {
    count_entered: mpsc::Receiver<()>,
    count_release: mpsc::Sender<()>,
    send_entered: mpsc::Receiver<()>,
    send_release: mpsc::Sender<()>,
    counts: Arc<AtomicUsize>,
    sends: Arc<AtomicUsize>,
}
fn controlled(h: &mut RuntimeHost, mode: u8) -> Gate {
    let (ct, count_entered) = mpsc::channel();
    let (count_release, cr) = mpsc::channel();
    let cr = Mutex::new(cr);
    let (st, send_entered) = mpsc::channel();
    let (send_release, sr) = mpsc::channel();
    let sr = Mutex::new(sr);
    let counts = Arc::new(AtomicUsize::new(0));
    let sends = Arc::new(AtomicUsize::new(0));
    let cc = counts.clone();
    let ss = sends.clone();
    let root = h.root.clone();
    let generation_root = root.clone();
    h.openai_connection.key_configured = true;
    h.management_scoped_fixture = Some(management_scoped_host::ScopedFixture {
        count: Arc::new(move |draft, quote| {
            let records: Vec<serde_json::Value> =
                std::fs::read_dir(root.join("currency-budgets/project"))
                    .unwrap()
                    .map(|p| p.unwrap().path())
                    .filter(|p| p.extension().is_some_and(|e| e == "json"))
                    .map(|p| serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap())
                    .collect();
            assert!(records
                .iter()
                .any(|v| v.pointer("/body/event/attempt/bodySha256")
                    == Some(&serde_json::json!(draft.fingerprint().count_body_sha256))));
            cc.fetch_add(1, Ordering::SeqCst);
            ct.send(()).unwrap();
            cr.lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(30))
                .unwrap();
            match mode {
                1 => Err("controlled count failure".into()),
                2 => panic!("controlled disconnected count"),
                3 => fixture_expired_scoped_count(draft, quote, 100),
                4 => fixture_counted_scoped_decision(draft, quote, MAX_TOKENS),
                _ => fixture_counted_scoped_decision(draft, quote, 100),
            }
        }),
        send: Arc::new(move |i, prepared| {
            let raw = std::fs::read(
                generation_root.join(format!("management-decisions/{}.0.json", i.operation_id)),
            )
            .unwrap();
            let intent: serde_json::Value = serde_json::from_slice(&raw).unwrap();
            assert_eq!(
                intent["body"]["evidence"]["scoped"]["request"],
                serde_json::to_value(&prepared.audit().request).unwrap()
            );
            let prepared_on_disk =
                std::fs::read_dir(generation_root.join("currency-budgets/project"))
                    .unwrap()
                    .map(|p| p.unwrap().path())
                    .filter(|p| p.extension().is_some_and(|e| e == "json"))
                    .any(|p| {
                        let v: serde_json::Value =
                            serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap();
                        v.pointer("/body/event/scopedPreparation/intent/operationId")
                            == Some(&serde_json::json!(i.operation_id))
                    });
            assert!(prepared_on_disk);
            ss.fetch_add(1, Ordering::SeqCst);
            st.send(()).unwrap();
            sr.lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(30))
                .unwrap();
            if mode == 5 {
                return Err("controlled uncertain generation".into());
            }
            let mut value = sd::response(&i);
            if mode == 6 {
                value.as_object_mut().unwrap().remove("service_tier");
            }
            if mode == 7 {
                value["output"][1]["content"][0]["text"] = serde_json::json!("{}");
            }
            fixture_scoped_result(&prepared, value)
        }),
        fail_count_spawn: mode == 8,
        fail_generation_spawn: mode == 9,
    });
    Gate {
        count_entered,
        count_release,
        send_entered,
        send_release,
        counts,
        sends,
    }
}
fn scoped_intent(h: &RuntimeHost, id: &str, purpose: Purpose) -> DecisionIntent {
    let mut i = intent(h, id, purpose);
    i.context = h
        .management_context("project", &h.project_setups["project"].setup.requirement)
        .unwrap();
    i.state_digest = digest(&i.context).unwrap();
    i
}
fn start(h: &mut RuntimeHost, g: &Gate, id: &str, purpose: Purpose) {
    let setup = h.project_setups["project"].setup.clone();
    let model = if matches!(purpose, Purpose::Plan | Purpose::Accept) {
        setup.chief_model
    } else {
        setup.deputy_model
    };
    h.start_management_decision("project", id, purpose, &model, &setup.requirement)
        .unwrap();
    g.count_entered
        .recv_timeout(Duration::from_secs(30))
        .unwrap();
}
fn wait_send(h: &mut RuntimeHost, g: &Gate) {
    let until = Instant::now() + Duration::from_secs(30);
    loop {
        h.poll_management();
        match g.send_entered.try_recv() {
            Ok(()) => return,
            Err(mpsc::TryRecvError::Empty) => {}
            Err(e) => panic!("send disconnected: {e:?} / {:?}", h.management_count_errors),
        }
        assert!(Instant::now() < until, "{:?}", h.management_count_errors);
        std::thread::sleep(Duration::from_millis(2));
    }
}
fn drain(h: &mut RuntimeHost) {
    let until = Instant::now() + Duration::from_secs(30);
    while h.management_work_count() > 0 && Instant::now() < until {
        h.poll_management();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(h.management_work_count(), 0);
}

#[test]
fn scoped_host_four_purposes_dispatch_in_original_order_and_apply_after_fee() {
    for purpose in [
        Purpose::Plan,
        Purpose::Assign,
        Purpose::Accept,
        Purpose::Advise,
    ] {
        let t = tempfile::tempdir().unwrap();
        let data = t.path().join("data");
        let mut h = fixture(t.path(), purpose, 10);
        let g = controlled(&mut h, 0);
        start(&mut h, &g, "scoped-host", purpose);
        assert!(matches!(
            h.currency_accounts["project"].state().management_counts["scoped-host"].outcome,
            CountOutcome::Pending
        ));
        assert!(h.management.absent_prepared_intent("scoped-host").unwrap());
        assert!(h.currency_accounts["project"].state().bookings.is_empty());
        assert_eq!(h.management_work_count(), 1);
        assert!(h.set_openai_key(None).is_err());
        assert!(h.check_openai().is_err());
        assert!(h.require_available("project").is_err());
        g.count_release.send(()).unwrap();
        wait_send(&mut h, &g);
        assert!(h.management.is_scoped("scoped-host"));
        assert!(h.management_fee_ack("scoped-host").is_err());
        assert!(h
            .apply_management_decision("scoped-host", ApplicationTarget::Wait)
            .is_err());
        g.send_release.send(()).unwrap();
        drain(&mut h);
        assert!(
            h.management_count_errors.is_empty(),
            "{:?}",
            h.management_count_errors
        );
        let ack = h.management_fee_ack("scoped-host").unwrap().unwrap();
        h.apply_management_decision("scoped-host", ApplicationTarget::Wait)
            .unwrap();
        assert_eq!(
            h.management.scoped_application_ack("scoped-host"),
            Some(&ack)
        );
        assert!(matches!(
            h.currency_accounts["project"].state().bookings["scoped-host"].outcome,
            Outcome::Settled { .. }
        ));
        assert_eq!(
            h.currency_accounts["project"].state().reserved_usd_nanos,
            1000
        );
        assert_eq!(g.counts.load(Ordering::SeqCst), 1);
        assert_eq!(g.sends.load(Ordering::SeqCst), 1);
        let before = snapshot(&data);
        drop(h);
        let h = RuntimeHost::open(&data).unwrap();
        assert_eq!(before, snapshot(&data));
        assert_eq!(h.management_work_count(), 0);
        assert_eq!(h.management_fee_ack("scoped-host").unwrap(), Some(ack));
    }
}
#[test]
fn scoped_host_returns_count_receipt_before_scope_head_or_freshness_failure() {
    for mode in [0, 3, 4, 10] {
        let t = tempfile::tempdir().unwrap();
        let data = t.path().join("data");
        let mut h = fixture(t.path(), Purpose::Assign, 10);
        let g = controlled(&mut h, mode);
        start(&mut h, &g, "preflight", Purpose::Assign);
        if mode == 0 {
            h.project_setups
                .get_mut("project")
                .unwrap()
                .setup
                .requirement
                .push_str(" changed");
        }
        if mode == 10 {
            let mut i = h.management_counts["preflight"].intent.clone();
            i.operation_id = "other-count".into();
            let draft =
                ScopedDecisionDraft::standard(i.purpose, &i.model, &i.state_digest, &i.context)
                    .unwrap();
            let q = h.management_counts["preflight"].basis.quote.clone();
            h.currency_accounts
                .get_mut("project")
                .unwrap()
                .begin_scoped_management_count(&i, &draft, &q)
                .unwrap();
        }
        g.count_release.send(()).unwrap();
        drain(&mut h);
        assert_eq!(g.sends.load(Ordering::SeqCst), 0);
        assert!(matches!(
            h.currency_accounts["project"].state().management_counts["preflight"].outcome,
            CountOutcome::ScopedCounted { .. }
        ));
        assert!(h.currency_accounts["project"].state().bookings.is_empty());
        assert!(h.management.absent_prepared_intent("preflight").unwrap());
        assert!(h.management_count_errors.contains_key("project"));
        drop(h);
        let h = RuntimeHost::open(&data).unwrap();
        assert_eq!(h.management_work_count(), 0);
        assert!(h.currency_accounts["project"].state().reserved_usd_nanos >= 1000);
    }
}
#[test]
fn scoped_host_count_errors_disconnect_and_spawn_failures_retain_liability() {
    for mode in [1, 2, 8, 9] {
        let t = tempfile::tempdir().unwrap();
        let data = t.path().join("data");
        let mut h = fixture(t.path(), Purpose::Assign, 10);
        let g = controlled(&mut h, mode);
        if mode == 8 {
            let i = scoped_intent(&h, "failed", Purpose::Assign);
            assert!(h.start_scoped_management(i).is_err());
        } else {
            start(&mut h, &g, "failed", Purpose::Assign);
            g.count_release.send(()).unwrap();
            drain(&mut h);
        }
        assert_eq!(g.sends.load(Ordering::SeqCst), 0);
        assert!(h.management_count_errors.contains_key("project"));
        let b = &h.currency_accounts["project"];
        if mode == 9 {
            assert!(matches!(
                b.state().bookings["failed"].outcome,
                Outcome::Unknown { .. }
            ));
        } else {
            assert!(matches!(
                b.state().management_counts["failed"].outcome,
                CountOutcome::Unknown { .. }
            ));
        }
        drop(h);
        let h = RuntimeHost::open(&data).unwrap();
        assert_eq!(h.management_work_count(), 0);
        assert!(h.currency_accounts["project"].state().reserved_usd_nanos >= 1000);
    }
}
#[test]
fn scoped_host_invalid_decision_and_unknown_fee_remain_distinct_and_block_application() {
    for mode in [5, 6, 7] {
        let t = tempfile::tempdir().unwrap();
        let mut h = fixture(t.path(), Purpose::Plan, 10);
        let g = controlled(&mut h, mode);
        start(&mut h, &g, "billable", Purpose::Plan);
        g.count_release.send(()).unwrap();
        wait_send(&mut h, &g);
        g.send_release.send(()).unwrap();
        drain(&mut h);
        assert!(h.management.application("billable").is_none());
        assert!(h
            .apply_management_decision("billable", ApplicationTarget::Wait)
            .is_err());
        let outcome = &h.currency_accounts["project"].state().bookings["billable"].outcome;
        match mode {
            5 => assert!(matches!(outcome, Outcome::Unknown { .. })),
            6 => assert!(matches!(outcome, Outcome::Unpriced { .. })),
            _ => assert!(matches!(outcome, Outcome::Settled { .. })),
        }
        let before = snapshot(&t.path().join("data"));
        drop(h);
        let h = RuntimeHost::open(&t.path().join("data")).unwrap();
        assert_eq!(before, snapshot(&t.path().join("data")));
        assert_eq!(h.management_work_count(), 0);
    }
}
#[test]
fn scoped_host_ledger_count_ids_survive_retirement_and_new_user_planning_does_not_reuse_them() {
    let t = tempfile::tempdir().unwrap();
    let data = t.path().join("data");
    let mut h = fixture(t.path(), Purpose::Plan, 3);
    let g = controlled(&mut h, 1);
    let setup = h.project_setups["project"].digest.clone();
    h.request_chief_plan("project", &setup).unwrap();
    g.count_entered
        .recv_timeout(Duration::from_secs(30))
        .unwrap();
    let first = h.management_counts.keys().next().unwrap().clone();
    g.count_release.send(()).unwrap();
    drain(&mut h);
    assert!(h.management.statuses().is_empty());
    assert_eq!(h.management_operation_ids("project").len(), 1);
    assert_eq!(h.management_operation_total(), 1);
    use cerulean_domain::count_resolution::{CountResolutionAction, CountResolutionInput};
    for action in [
        CountResolutionAction::SettleFee {
            amount_usd_nanos: 17,
            billing_reference: "controlled count invoice".into(),
            explanation: "fixture only".into(),
        },
        CountResolutionAction::RetireContinuation {
            reason: "user stops original operation".into(),
        },
    ] {
        let review = h
            .review_management_count_resolution(&CountResolutionInput {
                project_id: "project".into(),
                operation_id: first.clone(),
                action,
            })
            .unwrap();
        h.confirm_management_count_resolution(&review, &review.digest().unwrap())
            .unwrap();
    }
    let second_gate = controlled(&mut h, 0);
    h.request_chief_plan("project", &setup).unwrap();
    second_gate
        .count_entered
        .recv_timeout(Duration::from_secs(30))
        .unwrap();
    let second = h.management_counts.keys().next().unwrap().clone();
    assert_ne!(first, second);
    assert!(second.ends_with("-2"));
    second_gate.count_release.send(()).unwrap();
    wait_send(&mut h, &second_gate);
    second_gate.send_release.send(()).unwrap();
    drain(&mut h);
    assert_eq!(h.management_operation_total(), 2);
    assert_eq!(
        h.currency_accounts["project"]
            .state()
            .management_counts
            .len(),
        2
    );
    let before = snapshot(&data);
    drop(h);
    RuntimeHost::open(&data).unwrap();
    assert_eq!(before, snapshot(&data));
}
#[test]
fn scoped_host_live_gate_and_request_limits_refuse_before_count_or_vault() {
    for purpose in [
        Purpose::Plan,
        Purpose::Assign,
        Purpose::Accept,
        Purpose::Advise,
    ] {
        let t = tempfile::tempdir().unwrap();
        let mut h = fixture(t.path(), purpose, 10);
        h.openai_connection.key_configured = true;
        let i = scoped_intent(&h, "no-live-call", purpose);
        let before = snapshot(&t.path().join("data"));
        assert!(h.start_scoped_management(i.clone()).is_err());
        assert!(h
            .start_management_decision(
                "project",
                &i.operation_id,
                purpose,
                &i.model,
                &h.project_setups["project"].setup.requirement.clone()
            )
            .is_err());
        assert_eq!(before, snapshot(&t.path().join("data")));
        assert_eq!(h.management_work_count(), 0);
    }
    let t = tempfile::tempdir().unwrap();
    let mut h = fixture(t.path(), Purpose::Assign, 1);
    let g = controlled(&mut h, 0);
    let i = scoped_intent(&h, "over-limit", Purpose::Assign);
    let before = snapshot(&t.path().join("data"));
    assert!(h
        .start_scoped_management(i)
        .unwrap_err()
        .contains("请求上限"));
    assert_eq!(before, snapshot(&t.path().join("data")));
    assert_eq!(g.counts.load(Ordering::SeqCst), 0);
}
#[test]
fn scoped_host_pause_resume_and_run_recovery_invalidate_count_authority() {
    for mode in 0..4 {
        let t = tempfile::tempdir().unwrap();
        let mut h = fixture(t.path(), Purpose::Assign, 10);
        let revision = h.projects["project"]
            .revision()
            .unwrap()
            .approval_digest()
            .unwrap();
        let mut run = ProjectRun {
            id: format!("run-{}", &digest(&("project", &revision)).unwrap()[..32]),
            project_id: "project".into(),
            revision_digest: revision,
            setup_digest: h.project_setups["project"].digest.clone(),
            state: RunState::Active,
            message: "Synthetic user authorization".into(),
            acknowledged_decisions: h.management_operation_total() as u32,
        };
        h.runs.save(run.clone(), "user").unwrap();
        let original_head = h.runs.project_evidence_digest("project").to_owned();
        let g = controlled(&mut h, 0);
        // Exercise the original async dispatch boundary directly: normal runner
        // polling intentionally still refuses monetary worker execution.
        let i = scoped_intent(&h, "run-authority", Purpose::Assign);
        h.start_scoped_management(i).unwrap();
        g.count_entered
            .recv_timeout(Duration::from_secs(30))
            .unwrap();
        if mode < 2 {
            run.state = RunState::Paused;
            h.runs.save(run.clone(), "user").unwrap();
            if mode == 1 {
                run.state = RunState::Active;
                h.runs.save(run, "user").unwrap();
            }
            assert_ne!(original_head, h.runs.project_evidence_digest("project"));
        } else if mode == 2 {
            h.runs.poison_for_test();
        } else {
            h.runner_error = Some("synthetic run recovery failure".into());
        }
        g.count_release.send(()).unwrap();
        drain(&mut h);
        assert_eq!(g.sends.load(Ordering::SeqCst), 0);
        assert!(matches!(
            h.currency_accounts["project"].state().management_counts["run-authority"].outcome,
            CountOutcome::ScopedCounted { .. }
        ));
        assert!(h.currency_accounts["project"].state().bookings.is_empty());
        assert!(h.management_count_errors.contains_key("project"));
        let head = h.runs.project_evidence_digest("project").to_owned();
        drop(h);
        let h = RuntimeHost::open(&t.path().join("data")).unwrap();
        assert_eq!(h.runs.project_evidence_digest("project"), head);
        assert_eq!(h.management_work_count(), 0);
    }
}
#[test]
fn scoped_host_generation_response_publication_failure_never_applies_or_resends() {
    let t = tempfile::tempdir().unwrap();
    let data = t.path().join("data");
    let mut h = fixture(t.path(), Purpose::Assign, 10);
    let g = controlled(&mut h, 0);
    start(&mut h, &g, "disk-failure", Purpose::Assign);
    g.count_release.send(()).unwrap();
    wait_send(&mut h, &g);
    let pending = data.join("management-decisions/disk-failure.1.pending");
    std::fs::write(&pending, b"original incomplete publication").unwrap();
    g.send_release.send(()).unwrap();
    drain(&mut h);
    assert!(h.management.application("disk-failure").is_none());
    assert!(h.management_fee_ack("disk-failure").is_err());
    assert_eq!(g.sends.load(Ordering::SeqCst), 1);
    assert_eq!(
        std::fs::read(pending).unwrap(),
        b"original incomplete publication"
    );
    let before = snapshot(&data);
    drop(h);
    assert!(RuntimeHost::open(&data).is_err());
    assert_eq!(before, snapshot(&data));
}
