use super::*;
use crate::currency_budget::CountOutcome;
use cerulean_adapters::openai::fixture_counted_decision;

pub(super) fn count_fixture(
    root: &Path,
    maximum: u64,
    body_limit: u32,
    budget: u64,
) -> RuntimeHost {
    let mut settings = currency_product_tests::settings();
    settings.limit_usd_nanos = budget;
    for quote in &mut settings.quotes {
        quote.rates.cache_read = Some(0);
        quote.count_price = Some(ManagementCountPrice {
            schema: 1,
            maximum_usd_nanos: maximum,
            maximum_body_bytes: body_limit,
            source: "Synthetic count fee maximum; not a supplier price".into(),
        });
    }
    count_fixture_with_settings(root, settings)
}
pub(super) fn count_fixture_with_settings(root: &Path, settings: BudgetSettings) -> RuntimeHost {
    let (host, original) = project_planning::tests::fixture(root);
    drop(host);
    crate::currency_guard::tests::promote_fixture(&root.join("data"));
    crate::currency_execution_guard::tests::promote_fixture(&root.join("data"));
    crate::currency_count_guard::tests::promote_fixture(&root.join("data"));
    let mut host = RuntimeHost::open(&root.join("data")).unwrap();
    let mut draft =
        api_project_tests::draft_for(original, ProjectWorkerAdapter::Openai, ApiTarget::OpenAi {});
    draft.currency_budget = Some(settings);
    host.configure_project(draft).unwrap();
    let saved = host.project_setups["project"].digest.clone();
    host.authorize_planning_budget("project", &saved).unwrap();
    host
}
fn begin_count(host: &mut RuntimeHost, i: &DecisionIntent) -> Result<u64, String> {
    let quote = digest(&host.currency_accounts["project"].policy().quotes[0]).unwrap();
    host.currency_accounts
        .get_mut("project")
        .unwrap()
        .begin_management_count(i, &prepared(i), &quote)
}
#[test]
fn paid_count_preserves_fee_before_network_and_after_successful_generation_settlement() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = count_fixture(temp.path(), 1_000_000, 256 * 1024, 12_345_678_901);
    let i = intent(&host, "count-generation", Purpose::Plan);
    let amount = begin_count(&mut host, &i).unwrap();
    assert_eq!(amount, 1_000_000);
    assert!(host.management.statuses().is_empty());
    assert!(host.management_active.is_empty());
    let counted = fixture_counted_decision(prepared(&i), 100).unwrap();
    let quote = digest(&host.currency_accounts["project"].policy().quotes[0]).unwrap();
    let book = host.currency_accounts.get_mut("project").unwrap();
    let before = snapshot(&data);
    assert!(book
        .prepare_counted_management(&i, &counted, &quote)
        .is_err());
    assert_eq!(snapshot(&data), before);
    book.record_management_count(&i.operation_id, &counted)
        .unwrap();
    let before = snapshot(&data);
    book.record_management_count(&i.operation_id, &counted)
        .unwrap();
    assert_eq!(snapshot(&data), before);
    let generated = book
        .prepare_counted_management(&i, &counted, &quote)
        .unwrap();
    assert_eq!(book.state().reserved_usd_nanos, amount + generated);
    host.management
        .begin_with_request(i.clone(), Some(prepared(&i).fingerprint().clone()))
        .unwrap();
    host.management
        .finish_result(&i.operation_id, Ok(result(&i)))
        .unwrap();
    host.reconcile_currency_account("project").unwrap();
    let state = host.currency_accounts["project"].state();
    assert_eq!(state.reserved_usd_nanos, amount);
    assert!(state.charged_usd_nanos > 0);
    assert!(matches!(
        state.management_counts[&i.operation_id].outcome,
        CountOutcome::Counted { .. }
    ));
    let before = snapshot(&data);
    drop(host);
    let host = RuntimeHost::open(&data).unwrap();
    assert_eq!(snapshot(&data), before);
    assert_eq!(
        host.currency_accounts["project"].state().reserved_usd_nanos,
        amount
    );
}
#[test]
fn paid_count_restart_retains_unknown_fee_and_cannot_recount_generate_or_refund() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = count_fixture(temp.path(), 1_000_000, 256 * 1024, 12_345_678_901);
    let i = intent(&host, "interrupted-count", Purpose::Plan);
    begin_count(&mut host, &i).unwrap();
    drop(host);
    let mut host = RuntimeHost::open(&data).unwrap();
    let before = snapshot(&data);
    let book = host.currency_accounts.get_mut("project").unwrap();
    assert_eq!(book.state().reserved_usd_nanos, 1_000_000);
    assert!(matches!(
        book.state().management_counts[&i.operation_id].outcome,
        CountOutcome::Unknown { .. }
    ));
    let quote = digest(&book.policy().quotes[0]).unwrap();
    let counted = fixture_counted_decision(prepared(&i), 100).unwrap();
    assert!(book
        .begin_management_count(&i, &prepared(&i), &quote)
        .is_err());
    assert!(book
        .record_management_count(&i.operation_id, &counted)
        .is_err());
    assert!(book
        .prepare_counted_management(&i, &counted, &quote)
        .is_err());
    assert!(book.not_sent(&i.operation_id, &"a".repeat(64)).is_err());
    assert_eq!(snapshot(&data), before);
    drop(host);
    let host = RuntimeHost::open(&data).unwrap();
    assert_eq!(snapshot(&data), before);
    assert!(host.management.statuses().is_empty());
}
#[test]
fn paid_count_exact_original_receipt_required_and_failure_is_idempotent() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = count_fixture(temp.path(), 1000, 256 * 1024, 12_345_678_901);
    let i = intent(&host, "failed-count", Purpose::Plan);
    begin_count(&mut host, &i).unwrap();
    let mut other = i.clone();
    other.context["requirement"] = serde_json::json!("different request");
    other.state_digest = digest(&other.context).unwrap();
    let counted = fixture_counted_decision(prepared(&other), 100).unwrap();
    let book = host.currency_accounts.get_mut("project").unwrap();
    let before = snapshot(&data);
    assert!(book
        .record_management_count(&i.operation_id, &counted)
        .is_err());
    assert_eq!(snapshot(&data), before);
    book.mark_management_count_unknown(&i.operation_id, &"b".repeat(64))
        .unwrap();
    let before = snapshot(&data);
    book.mark_management_count_unknown(&i.operation_id, &"b".repeat(64))
        .unwrap();
    assert!(book
        .mark_management_count_unknown(&i.operation_id, &"c".repeat(64))
        .is_err());
    assert_eq!(snapshot(&data), before);
    assert_eq!(book.state().reserved_usd_nanos, 1000);
}
#[test]
fn paid_count_scope_and_available_budget_are_checked_before_any_publication() {
    for (fee, body, budget) in [(1_000_000, 1, 12_345_678_901), (1001, 256 * 1024, 1000)] {
        let temp = tempfile::tempdir().unwrap();
        let mut host = count_fixture(temp.path(), fee, body, budget);
        let i = intent(&host, "refused-count", Purpose::Plan);
        let before = snapshot(&temp.path().join("data"));
        assert!(begin_count(&mut host, &i).is_err());
        assert_eq!(snapshot(&temp.path().join("data")), before);
        assert!(host.currency_accounts["project"]
            .state()
            .management_counts
            .is_empty());
    }
}
#[test]
fn paid_count_fee_is_shared_with_generation_budget_and_zero_is_only_explicit() {
    for fee in [0, 1000] {
        let temp = tempfile::tempdir().unwrap();
        let mut host = count_fixture(temp.path(), fee, 256 * 1024, 1000);
        let i = intent(&host, "fee-shared", Purpose::Plan);
        assert_eq!(begin_count(&mut host, &i).unwrap(), fee);
        let counted = fixture_counted_decision(prepared(&i), 100).unwrap();
        let book = host.currency_accounts.get_mut("project").unwrap();
        book.record_management_count(&i.operation_id, &counted)
            .unwrap();
        let quote = digest(&book.policy().quotes[0]).unwrap();
        assert!(book
            .prepare_counted_management(&i, &counted, &quote)
            .unwrap_err()
            .contains("exhausted"));
        assert_eq!(book.state().reserved_usd_nanos, fee);
        assert!(book.state().bookings.is_empty());
    }
    let temp = tempfile::tempdir().unwrap();
    let mut host = prepared_fixture(temp.path());
    let i = intent(&host, "unknown-fee", Purpose::Plan);
    let before = snapshot(&temp.path().join("data"));
    assert!(begin_count(&mut host, &i)
        .unwrap_err()
        .contains("unknown is not free"));
    assert_eq!(snapshot(&temp.path().join("data")), before);
}
#[test]
fn paid_count_rehashed_tamper_is_rejected_by_inner_body_and_original_fee_binding() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = count_fixture(temp.path(), 1000, 256 * 1024, 12_345_678_901);
    let i = intent(&host, "tamper-count", Purpose::Plan);
    begin_count(&mut host, &i).unwrap();
    drop(host);
    let path = data.join("currency-budgets/project/00000002.json");
    let original = fs::read(&path).unwrap();
    for field in ["maximumUsdNanos", "bodyBytes", "bodySha256"] {
        let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
        value["body"]["event"]["attempt"][field] = if field == "bodySha256" {
            serde_json::json!("a".repeat(64))
        } else {
            serde_json::json!(999)
        };
        fs::write(&path, crate::currency_budget::fixture_rehash_record(value)).unwrap();
        let error = RuntimeHost::open(&data)
            .err()
            .expect("tampered count accepted");
        assert!(error.contains("count request or fee ceiling"), "{error}");
    }
    fs::write(path, original).unwrap();
    drop(RuntimeHost::open(&data).unwrap());
}
#[test]
fn paid_count_price_requires_reader6_before_config_write_and_missing_guard_cannot_be_adopted() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let (host, original) = project_planning::tests::fixture(temp.path());
    drop(host);
    crate::currency_guard::tests::promote_fixture(&data);
    crate::currency_execution_guard::tests::promote_fixture(&data);
    let mut host = RuntimeHost::open(&data).unwrap();
    let mut draft =
        api_project_tests::draft_for(original, ProjectWorkerAdapter::Openai, ApiTarget::OpenAi {});
    let mut settings = currency_product_tests::settings();
    settings.quotes[0].count_price = Some(ManagementCountPrice {
        schema: 1,
        maximum_usd_nanos: 1000,
        maximum_body_bytes: 65536,
        source: "Synthetic fee evidence".into(),
    });
    draft.currency_budget = Some(settings);
    let before = snapshot(&data);
    assert!(host
        .configure_project(draft.clone())
        .unwrap_err()
        .contains("reader6"));
    assert_eq!(snapshot(&data), before);
    drop(host);
    crate::currency_count_guard::tests::promote_fixture(&data);
    let mut host = RuntimeHost::open(&data).unwrap();
    host.configure_project(draft).unwrap();
    drop(host);
    let marker = data.join(crate::currency_count_guard::MARKER);
    let hidden = temp.path().join("owned-reader6-marker.json");
    fs::rename(&marker, &hidden).unwrap();
    // With only the valid older guard left, protocol-owned price fields must
    // themselves trigger refusal; arbitrary business text is not inspected.
    assert!(RuntimeHost::open(&data)
        .err()
        .unwrap()
        .contains("count price has no reader6"));
    fs::rename(hidden, marker).unwrap();
    drop(RuntimeHost::open(&data).unwrap());
}

#[test]
#[ignore = "Exports owned reader6 count data for real binary compatibility probes; no provider calls"]
fn export_reader6_count_fixture() {
    let root = PathBuf::from(std::env::var_os("CERULEAN_COUNT_FIXTURES").unwrap());
    assert!(!root.exists());
    fs::create_dir_all(&root).unwrap();
    let mut host = count_fixture(&root, 1000, 256 * 1024, 12_345_678_901);
    let i = intent(&host, "pending-count", Purpose::Plan);
    begin_count(&mut host, &i).unwrap();
    drop(host);
}
