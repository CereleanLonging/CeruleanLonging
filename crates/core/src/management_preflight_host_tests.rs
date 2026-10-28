//! Actual Host dispatch/polling with synthetic count/send functions only.
use super::*;
use crate::currency_budget::CountOutcome;
use cerulean_adapters::openai::fixture_counted_decision;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc, Arc, Mutex,
};
use std::time::{Duration, Instant};

const COUNT_FEE: u64 = 1_000_000;
fn paid_fixture(root: &Path) -> RuntimeHost {
    count_ledger::count_fixture(root, COUNT_FEE, 256 * 1024, 12_345_678_901)
}
fn assert_preserved(before: &std::collections::BTreeMap<PathBuf, Vec<u8>>, root: &Path) {
    let after = snapshot(root);
    for (path, bytes) in before {
        assert_eq!(after.get(path), Some(bytes), "{}", path.display());
    }
}

struct Gate {
    entered: mpsc::Receiver<()>,
    release: mpsc::Sender<()>,
    sends: Arc<AtomicUsize>,
}
fn controlled(host: &mut RuntimeHost, tokens: u64, failure: u8) -> Gate {
    let (entered_tx, entered) = mpsc::channel();
    let (release, release_rx) = mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    let sends = Arc::new(AtomicUsize::new(0));
    let observed = sends.clone();
    host.openai_connection.key_configured = true; // No vault, credential or HTTP.
    host.management_count_fixture = Some(management_preflight_host::CountFixture {
        count: Arc::new(move |p| {
            entered_tx.send(()).unwrap();
            release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(10))
                .unwrap();
            match failure {
                1 => Err("synthetic count failure".into()),
                2 => panic!("synthetic count disconnect"),
                4 => cerulean_adapters::openai::fixture_expired_counted_decision(p, tokens),
                _ => fixture_counted_decision(p, tokens),
            }
        }),
        send: Arc::new(move |i| {
            observed.fetch_add(1, Ordering::SeqCst);
            if failure == 3 {
                return Err("synthetic uncertain generation".into());
            }
            Ok(result(&i))
        }),
        fail_spawn: false,
    });
    Gate {
        entered,
        release,
        sends,
    }
}
fn start(host: &mut RuntimeHost, gate: &Gate) -> String {
    let saved = host.project_setups["project"].digest.clone();
    host.request_chief_plan("project", &saved).unwrap();
    gate.entered.recv_timeout(Duration::from_secs(10)).unwrap();
    host.management_counts.keys().next().unwrap().clone()
}
fn drain(host: &mut RuntimeHost) {
    let until = Instant::now() + Duration::from_secs(10);
    while host.management_work_count() > 0 && Instant::now() < until {
        host.poll_management();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(host.management_work_count(), 0);
}

#[test]
fn counted_host_runs_async_blocks_mutation_and_settles_exactly_once() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = paid_fixture(temp.path());
    let gate = controlled(&mut host, 100, 0);
    let before = snapshot(&temp.path().join("data"));
    let operation = start(&mut host, &gate);
    assert_preserved(&before, &temp.path().join("data"));
    let state = host.currency_accounts["project"].state();
    assert_eq!(state.management_counts.len(), 1);
    assert_eq!(state.reserved_usd_nanos, COUNT_FEE);
    assert!(matches!(
        state.management_counts[&operation].outcome,
        CountOutcome::Pending
    ));
    let counts = host.currency_account_views(Some(&[]))[0]
        .management_counts
        .clone()
        .unwrap();
    assert_eq!((counts.pending, counts.counted, counts.unknown), (1, 0, 0));
    assert_eq!(counts.reserved_usd_nanos, COUNT_FEE);
    assert!(!counts.needs_review);
    assert!(host.management.statuses().is_empty());
    assert!(host.currency_accounts["project"]
        .state()
        .bookings
        .is_empty());
    assert!(!host.can_shutdown());
    assert!(host
        .set_openai_key(None)
        .unwrap_err()
        .contains("不能变更凭证"));
    assert!(host.check_openai().is_err());
    assert!(host
        .planning_idle("project")
        .unwrap_err()
        .contains("计数仍在运行"));
    let saved = host.project_setups["project"].digest.clone();
    assert!(host.request_chief_plan("project", &saved).is_err());
    assert!(host.authorize_planning_budget("project", &saved).is_err());
    assert_eq!(host.management_work_count(), 1);
    let mut keepalive = vec![];
    for n in 0..3 {
        let (tx, rx) = mpsc::channel();
        keepalive.push(tx);
        host.management_active.insert(format!("other-{n}"), rx);
    }
    assert_eq!(host.management_work_count(), 4);
    let i = intent_from_saved(&host, "fifth-management-request");
    assert!(host
        .start_counted_planning(i)
        .unwrap_err()
        .contains("concurrency limit"));
    host.management_active.clear();
    gate.release.send(()).unwrap();
    drain(&mut host);
    assert_eq!(gate.sends.load(Ordering::SeqCst), 1);
    assert!(
        host.management_count_errors.is_empty(),
        "{:?}",
        host.management_count_errors
    );
    let book = &host.currency_accounts["project"];
    assert!(matches!(
        book.state().bookings[&operation].outcome,
        Outcome::Settled { .. }
    ));
    assert!(book.state().bookings[&operation]
        .management_preparation
        .is_some());
    assert_eq!(book.state().reserved_usd_nanos, COUNT_FEE);
    let counts = host.currency_account_views(Some(&[]))[0]
        .management_counts
        .clone()
        .unwrap();
    assert_eq!(
        (counts.pending, counts.counted, counts.awaiting_generation),
        (0, 1, 0)
    );
    assert!(!counts.needs_review);
    assert!(book.state().charged_usd_nanos > 0);
    let after = snapshot(&temp.path().join("data"));
    for _ in 0..3 {
        host.poll_management();
    }
    assert_eq!(snapshot(&temp.path().join("data")), after);
    drop(host);
    let host = RuntimeHost::open(&temp.path().join("data")).unwrap();
    assert_eq!(snapshot(&temp.path().join("data")), after);
    assert_eq!(host.management_work_count(), 0);
    assert_eq!(gate.sends.load(Ordering::SeqCst), 1);
}

#[test]
fn counted_host_rechecks_configuration_original_approval_and_history() {
    for change in 0..3 {
        let temp = tempfile::tempdir().unwrap();
        let mut host = paid_fixture(temp.path());
        let gate = controlled(&mut host, 100, 0);
        start(&mut host, &gate);
        match change {
            0 => {
                host.project_setups
                    .get_mut("project")
                    .unwrap()
                    .setup
                    .requirement
                    .push_str(" changed");
            }
            1 => {
                host.currency_accounts.remove("project");
            }
            _ => {
                // A new legacy request introduces unresolved money history.
                let mut i = intent_from_saved(&host, "legacy-during-count");
                i.context.as_object_mut().unwrap().remove("executionSetup");
                i.state_digest = digest(&i.context).unwrap();
                host.management.begin(i).unwrap();
            }
        }
        let before = snapshot(&temp.path().join("data"));
        gate.release.send(()).unwrap();
        drain(&mut host);
        assert_eq!(gate.sends.load(Ordering::SeqCst), 0);
        assert!(host.management_count_errors.contains_key("project"));
        assert_preserved(&before, &temp.path().join("data"));
    }
}
fn intent_from_saved(host: &RuntimeHost, operation: &str) -> DecisionIntent {
    // Captures the existing pending intent for controlled concurrent mutations.
    let setup = &host.project_setups["project"];
    let workflow = host
        .snapshot()
        .workflows
        .into_iter()
        .find(|w| w.project.id == "project")
        .unwrap();
    let context = host.context_for_snapshot(&setup.setup.requirement, &workflow);
    DecisionIntent {
        operation_id: operation.into(),
        project_id: "project".into(),
        purpose: Purpose::Plan,
        model: setup.setup.chief_model.clone(),
        state_digest: digest(&context).unwrap(),
        context,
        created_at: chrono::Utc::now().to_rfc3339(),
    }
}

#[test]
fn counted_host_count_failure_or_disconnect_stops_without_send_or_retry() {
    for failure in [1, 2] {
        let temp = tempfile::tempdir().unwrap();
        let mut host = paid_fixture(temp.path());
        let gate = controlled(&mut host, 100, failure);
        let before = snapshot(&temp.path().join("data"));
        start(&mut host, &gate);
        gate.release.send(()).unwrap();
        drain(&mut host);
        assert_eq!(gate.sends.load(Ordering::SeqCst), 0);
        assert!(host.management_count_errors.contains_key("project"));
        assert!(host.currency_accounts["project"]
            .state()
            .bookings
            .is_empty());
        let saved = host.project_setups["project"].digest.clone();
        assert!(host.request_chief_plan("project", &saved).is_err());
        assert_eq!(
            host.currency_accounts["project"].state().reserved_usd_nanos,
            COUNT_FEE
        );
        assert!(host.currency_accounts["project"]
            .state()
            .management_counts
            .values()
            .all(|b| matches!(b.outcome, CountOutcome::Unknown { .. })));
        assert_preserved(&before, &temp.path().join("data"));
        let after = snapshot(&temp.path().join("data"));
        for _ in 0..3 {
            host.poll_management();
        }
        assert_eq!(snapshot(&temp.path().join("data")), after);
        drop(host);
        let mut host = RuntimeHost::open(&temp.path().join("data")).unwrap();
        let _gate = controlled(&mut host, 100, 0);
        assert!(host.management_count_errors.contains_key("project"));
        assert!(host.request_chief_plan("project", &saved).is_err());
        assert_eq!(snapshot(&temp.path().join("data")), after);
    }
}

#[test]
fn counted_host_generation_failure_keeps_full_reservation_across_restart() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = paid_fixture(temp.path());
    let gate = controlled(&mut host, 100, 3);
    let operation = start(&mut host, &gate);
    gate.release.send(()).unwrap();
    drain(&mut host);
    let state = host.currency_accounts["project"].state();
    assert!(matches!(
        state.bookings[&operation].outcome,
        Outcome::Unknown { .. }
    ));
    let amount = state.reserved_usd_nanos;
    assert!(amount > 0);
    assert_eq!(state.charged_usd_nanos, 0);
    assert_eq!(gate.sends.load(Ordering::SeqCst), 1);
    drop(host);
    let mut host = RuntimeHost::open(&temp.path().join("data")).unwrap();
    host.poll_management();
    assert_eq!(
        host.currency_accounts["project"].state().reserved_usd_nanos,
        amount
    );
    assert!(host
        .currency_accounts
        .get_mut("project")
        .unwrap()
        .not_sent(&operation, &"a".repeat(64))
        .is_err());
    assert_eq!(gate.sends.load(Ordering::SeqCst), 1);
}

#[test]
fn counted_host_live_economics_gate_refuses_before_vault_count_or_publication() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = paid_fixture(temp.path());
    host.openai_connection.key_configured = true;
    let before = snapshot(&temp.path().join("data"));
    let saved = host.project_setups["project"].digest.clone();
    assert!(host
        .request_chief_plan("project", &saved)
        .unwrap_err()
        .contains("计数费用"));
    assert!(host
        .require_currency_execution_available("project")
        .is_err());
    assert_eq!(host.management_work_count(), 0);
    assert_eq!(snapshot(&temp.path().join("data")), before);
}

#[test]
fn counted_host_reserves_exact_count_and_rejects_exhaustion_without_intent() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = paid_fixture(temp.path());
    let gate = controlled(&mut host, MAX_TOKENS, 0);
    let before = snapshot(&temp.path().join("data"));
    start(&mut host, &gate);
    gate.release.send(()).unwrap();
    drain(&mut host);
    assert_eq!(gate.sends.load(Ordering::SeqCst), 0);
    assert!(host.management_count_errors["project"].contains("exhausted"));
    assert!(host.currency_accounts["project"]
        .state()
        .bookings
        .is_empty());
    assert!(host.management.statuses().is_empty());
    assert_preserved(&before, &temp.path().join("data"));
    assert_eq!(
        host.currency_accounts["project"].state().reserved_usd_nanos,
        COUNT_FEE
    );
    assert!(host.currency_accounts["project"]
        .state()
        .management_counts
        .values()
        .all(|b| matches!(b.outcome, CountOutcome::Counted { .. })));
}

#[test]
fn counted_host_changed_ledger_head_retains_other_preparation_and_stops() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = paid_fixture(temp.path());
    let gate = controlled(&mut host, 100, 0);
    let operation = start(&mut host, &gate);
    let other = intent_from_saved(&host, "other-preparation");
    let counted = fixture_counted_decision(prepared(&other), 10).unwrap();
    let book = host.currency_accounts.get_mut("project").unwrap();
    let quote = digest(&book.policy().quotes[0]).unwrap();
    book.begin_management_count(&other, counted.prepared(), &quote)
        .unwrap();
    book.record_management_count(&other.operation_id, &counted)
        .unwrap();
    let held = book
        .prepare_counted_management(&other, &counted, &quote)
        .unwrap()
        + 2 * COUNT_FEE;
    gate.release.send(()).unwrap();
    drain(&mut host);
    assert!(host.management_count_errors["project"].contains("ledger changed"));
    let state = host.currency_accounts["project"].state();
    assert!(!state.bookings.contains_key(&operation));
    assert!(matches!(
        state.bookings[&other.operation_id].outcome,
        Outcome::Unknown { .. }
    ));
    assert_eq!(state.reserved_usd_nanos, held);
    assert_eq!(gate.sends.load(Ordering::SeqCst), 0);
    drop(host);
    let host = RuntimeHost::open(&temp.path().join("data")).unwrap();
    assert_eq!(
        host.currency_accounts["project"].state().reserved_usd_nanos,
        held
    );
}

#[test]
fn counted_host_intent_publication_collision_keeps_preparation_and_never_sends() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = paid_fixture(temp.path());
    let gate = controlled(&mut host, 100, 0);
    let operation = start(&mut host, &gate);
    let collision = temp
        .path()
        .join("data/management-decisions")
        .join(format!("{operation}.0.json"));
    fs::write(&collision, b"owned synthetic publication collision").unwrap();
    gate.release.send(()).unwrap();
    drain(&mut host);
    assert_eq!(gate.sends.load(Ordering::SeqCst), 0);
    assert!(host.management_count_errors.contains_key("project"));
    let state = host.currency_accounts["project"].state();
    assert!(state.reserved_usd_nanos > 0);
    assert!(matches!(
        state.bookings[&operation].outcome,
        Outcome::Unknown { .. }
    ));
    assert!(state.bookings[&operation].management_preparation.is_some());
    assert!(host.management.intent(&operation).is_err());
    let after = snapshot(&temp.path().join("data"));
    for _ in 0..3 {
        host.poll_management();
    }
    assert_eq!(snapshot(&temp.path().join("data")), after);
    drop(host);
    // Preserve the unexpected artifact for review; don't delete it to force recovery.
    assert!(RuntimeHost::open(&temp.path().join("data")).is_err());
    assert_eq!(
        fs::read(&collision).unwrap(),
        b"owned synthetic publication collision"
    );
    assert_eq!(snapshot(&temp.path().join("data")), after);
}

#[test]
fn counted_host_known_insufficiency_or_missing_cache_price_refuses_before_count() {
    for missing_price in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let mut settings = currency_product_tests::settings();
        if !missing_price {
            settings.limit_usd_nanos = 1;
            for quote in &mut settings.quotes {
                quote.rates.cache_read = Some(0);
            }
        }
        for q in &mut settings.quotes {
            q.count_price = Some(ManagementCountPrice {
                schema: 1,
                maximum_usd_nanos: COUNT_FEE,
                maximum_body_bytes: 256 * 1024,
                source: "Synthetic fee".into(),
            });
        }
        let mut host = count_ledger::count_fixture_with_settings(temp.path(), settings);
        let gate = controlled(&mut host, 100, 0);
        let before = snapshot(&temp.path().join("data"));
        let saved = host.project_setups["project"].digest.clone();
        let error = host.request_chief_plan("project", &saved).unwrap_err();
        assert!(
            error.contains(if missing_price {
                "cache-read"
            } else {
                "exhausted"
            }),
            "{error}"
        );
        assert!(gate.entered.try_recv().is_err());
        assert_eq!(gate.sends.load(Ordering::SeqCst), 0);
        assert_eq!(host.management_work_count(), 0);
        assert_eq!(snapshot(&temp.path().join("data")), before);
    }
}

#[test]
fn counted_host_spawn_failure_retains_original_fee_and_blocks_after_restart() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = paid_fixture(temp.path());
    let gate = controlled(&mut host, 100, 0);
    host.management_count_fixture.as_mut().unwrap().fail_spawn = true;
    let saved = host.project_setups["project"].digest.clone();
    assert!(host
        .request_chief_plan("project", &saved)
        .unwrap_err()
        .contains("worker not started"));
    assert!(gate.entered.try_recv().is_err());
    assert_eq!(gate.sends.load(Ordering::SeqCst), 0);
    assert_eq!(host.management_work_count(), 0);
    let state = host.currency_accounts["project"].state();
    assert_eq!(state.reserved_usd_nanos, COUNT_FEE);
    assert!(state.bookings.is_empty());
    assert!(state
        .management_counts
        .values()
        .all(|b| matches!(b.outcome, CountOutcome::Unknown { .. })));
    let after = snapshot(&data);
    drop(host);
    let mut host = RuntimeHost::open(&data).unwrap();
    let _gate = controlled(&mut host, 100, 0);
    assert!(host
        .request_chief_plan("project", &saved)
        .unwrap_err()
        .contains("审阅"));
    assert_eq!(snapshot(&data), after);
}

#[test]
fn counted_host_expired_receipt_is_preserved_without_generation_or_restart_retry() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = paid_fixture(temp.path());
    let gate = controlled(&mut host, 100, 4);
    let id = start(&mut host, &gate);
    gate.release.send(()).unwrap();
    drain(&mut host);
    assert!(host.management_count_errors["project"].contains("expired"));
    assert_eq!(gate.sends.load(Ordering::SeqCst), 0);
    let state = host.currency_accounts["project"].state();
    assert!(matches!(
        state.management_counts[&id].outcome,
        CountOutcome::Counted { .. }
    ));
    assert!(state.bookings.is_empty());
    assert_eq!(state.reserved_usd_nanos, COUNT_FEE);
    let counts = host.currency_account_views(Some(&[]))[0]
        .management_counts
        .clone()
        .unwrap();
    assert_eq!((counts.counted, counts.awaiting_generation), (1, 1));
    assert!(counts.needs_review);
    let after = snapshot(&data);
    drop(host);
    let mut host = RuntimeHost::open(&data).unwrap();
    let _gate = controlled(&mut host, 100, 0);
    assert_eq!(
        host.currency_account_views(Some(&[]))[0]
            .management_counts
            .as_ref(),
        Some(&counts)
    );
    let saved = host.project_setups["project"].digest.clone();
    assert!(host
        .request_chief_plan("project", &saved)
        .unwrap_err()
        .contains("审阅"));
    host.poll_management();
    assert_eq!(snapshot(&data), after);
}

#[test]
fn counted_host_interrupted_pending_restores_attention_without_live_authority() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = paid_fixture(temp.path());
    // The exact durable boundary used by Host, with no worker started.
    let i = intent_from_saved(&host, "interrupted-durable-count");
    let book = host.currency_accounts.get_mut("project").unwrap();
    let quote = digest(&book.policy().quotes[0]).unwrap();
    book.begin_management_count(&i, &prepared(&i), &quote)
        .unwrap();
    let before = snapshot(&data);
    drop(host);
    let mut host = RuntimeHost::open(&data).unwrap();
    assert_preserved(&before, &data);
    let gate = controlled(&mut host, 100, 0);
    let saved = host.project_setups["project"].digest.clone();
    assert!(host
        .request_chief_plan("project", &saved)
        .unwrap_err()
        .contains("审阅"));
    assert!(gate.entered.try_recv().is_err());
    assert!(matches!(
        host.currency_accounts["project"].state().management_counts[&i.operation_id].outcome,
        CountOutcome::Unknown { .. }
    ));
    assert_eq!(
        host.currency_accounts["project"].state().reserved_usd_nanos,
        COUNT_FEE
    );
    assert_eq!(host.management_work_count(), 0);
    let counts = host.currency_account_views(Some(&[]))[0]
        .management_counts
        .clone()
        .unwrap();
    assert_eq!((counts.pending, counts.unknown), (0, 1));
    assert!(counts.needs_review);
    let after = snapshot(&data);
    drop(host);
    let _host = RuntimeHost::open(&data).unwrap();
    assert_eq!(snapshot(&data), after);
}

#[test]
fn counted_host_count_publication_collisions_never_send_or_erase_evidence() {
    for after_begin in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let mut host = paid_fixture(temp.path());
        let gate = controlled(&mut host, 100, 0);
        if after_begin {
            start(&mut host, &gate);
        }
        let ledger = data.join("currency-budgets/project");
        let next = fs::read_dir(&ledger)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|e| e == "json"))
            .count();
        let collision = ledger.join(format!("{next:08}.pending"));
        fs::write(&collision, b"owned incomplete count publication").unwrap();
        if after_begin {
            gate.release.send(()).unwrap();
            drain(&mut host);
        } else {
            let saved = host.project_setups["project"].digest.clone();
            assert!(host.request_chief_plan("project", &saved).is_err());
            assert!(gate.entered.try_recv().is_err());
        }
        assert_eq!(gate.sends.load(Ordering::SeqCst), 0);
        assert!(!host.currency_accounts["project"].healthy());
        assert!(host.currency_accounts["project"]
            .state()
            .bookings
            .is_empty());
        assert_eq!(
            host.currency_accounts["project"].state().reserved_usd_nanos,
            if after_begin { COUNT_FEE } else { 0 }
        );
        let after = snapshot(&data);
        host.poll_management();
        let saved = host.project_setups["project"].digest.clone();
        assert!(host.request_chief_plan("project", &saved).is_err());
        assert_eq!(snapshot(&data), after);
        drop(host);
        assert!(RuntimeHost::open(&data).is_err());
        assert_eq!(snapshot(&data), after);
    }
}

#[test]
fn counted_host_failed_count_consumes_request_limit_even_without_memory_error() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = paid_fixture(temp.path());
    let gate = controlled(&mut host, 100, 1);
    let operation = start(&mut host, &gate);
    gate.release.send(()).unwrap();
    drain(&mut host);
    // Exercise the persisted request limit independently of the stronger
    // attention guard; product exposes no clearing command.
    host.management_count_errors.clear();
    let same = intent_from_saved(&host, &operation);
    assert!(host
        .start_counted_planning(same)
        .unwrap_err()
        .contains("never recount"));
    let different = intent_from_saved(&host, "new-id-cannot-reset-limit");
    let before = snapshot(&temp.path().join("data"));
    assert!(host
        .start_counted_planning(different)
        .unwrap_err()
        .contains("上限"));
    assert!(gate.entered.try_recv().is_err());
    assert_eq!(snapshot(&temp.path().join("data")), before);
}

#[test]
fn counted_host_fee_plus_minimum_generation_must_fit_before_count() {
    let temp = tempfile::tempdir().unwrap();
    let settings = currency_product_tests::settings();
    let min = settings.quotes[0]
        .rates
        .reserve(&TokenCeiling {
            total_input: 1,
            output: 4096,
            cache_read_allowed: false,
            cache_write_allowed: true,
        })
        .unwrap();
    let mut host =
        count_ledger::count_fixture(temp.path(), COUNT_FEE, 256 * 1024, min + COUNT_FEE - 1);
    let gate = controlled(&mut host, 100, 0);
    let before = snapshot(&temp.path().join("data"));
    let saved = host.project_setups["project"].digest.clone();
    assert!(host
        .request_chief_plan("project", &saved)
        .unwrap_err()
        .contains("exhausted"));
    assert!(gate.entered.try_recv().is_err());
    assert_eq!(snapshot(&temp.path().join("data")), before);
}
