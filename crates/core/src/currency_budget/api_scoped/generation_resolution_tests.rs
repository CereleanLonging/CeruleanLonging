use super::*;
use crate::api_generation_resolution::{ApiGenerationResolution, ApiGenerationResolutionReview};
use cerulean_domain::api_generation_resolution::ApiGenerationResolutionInput;
fn prepare(
    data: &Path,
    book: &mut BudgetLedger,
    attempt: &str,
) -> (CallIdentity, PreparedScopedApi) {
    let (run, call, draft) = make_draft(data, attempt, 1);
    let quote = book.policy().scoped_quotes.as_ref().unwrap()[0].clone();
    book.begin_api_count(
        &run,
        &call,
        &draft,
        &"c".repeat(64),
        &digest(&quote).unwrap(),
    )
    .unwrap();
    let counted = fixture_counted_api(draft, quote, 100).unwrap();
    book.record_api_count(&call.request_id, &counted).unwrap();
    let prepared = counted.prepare().unwrap();
    book.prepare_scoped_api(&call.request_id, &prepared)
        .unwrap();
    (call, prepared)
}
fn review(book: &BudgetLedger, id: &str, amount: u64) -> ApiGenerationResolution {
    let b = &book.state.bookings[id];
    ApiGenerationResolutionReview {
        input: ApiGenerationResolutionInput {
            project_id: "project".into(),
            request_id: id.into(),
            amount_usd_nanos: amount,
            billing_reference: "isolated supplier invoice".into(),
            explanation: "explicit user reviewed amount".into(),
        },
        source_digest: b.generation_source_digest().unwrap(),
        policy_digest: digest(book.policy()).unwrap(),
        previous_ledger_head: book.head().into(),
        maximum_usd_nanos: b.reserved_usd_nanos,
        original_outcome: b.generation_outcome().into(),
        count_digest: book.state.api_generation_count_digest(id).unwrap(),
    }
    .resolution(&"a".repeat(64))
}
fn unpriced(book: &mut BudgetLedger, id: &str) -> Settlement {
    let s = Settlement {
        request_id: id.into(),
        response_id: format!("resp_{id}"),
        request_sha256: book.state.bookings[id].reservation.request_sha256.clone(),
        actual_model: "unquoted-model".into(),
        actual_service: None,
        usage: cerulean_domain::currency_budget::Usage {
            input: 10,
            output: 2,
            cache_read: 0,
            cache_write: 0,
        },
        evidence_sha256: "f".repeat(64),
    };
    assert!(matches!(
        book.settle(s.clone()).unwrap(),
        Outcome::Unpriced { .. }
    ));
    s
}
#[test]
fn api_generation_resolution_original_evidence_count_separation_replay_and_guard() {
    for mode in ["unknown", "unpriced"] {
        let t = tempfile::tempdir().unwrap();
        let data = t.path().join("data");
        let p = policy();
        let b = book(&data, &p, true);
        drop(b);
        crate::currency_api_resolution_guard::tests::promote_fixture(&data);
        let mut b =
            BudgetLedger::open_existing(&data.join("currency-budgets/project"), "project").unwrap();
        let (call, prepared) = prepare(&data, &mut b, "attempt");
        let id = &call.request_id;
        assert!(b
            .record_api_generation_resolution(review(&b, id, 0))
            .is_err());
        if mode == "unknown" {
            b.unknown(id, &"e".repeat(64)).unwrap();
        } else {
            unpriced(&mut b, id);
        }
        let original = b.state.bookings[id].clone();
        let counts = b.state.api_counts.clone();
        let confirmed = review(&b, id, 0);
        let before = snapshot(&data);
        b.check_api_generation_resolution(&confirmed).unwrap();
        assert_eq!(snapshot(&data), before);
        b.record_api_generation_resolution(confirmed.clone())
            .unwrap();
        assert_eq!(b.state.reserved_usd_nanos, 17);
        assert_eq!(b.state.charged_usd_nanos, 0);
        assert_eq!(b.state.bookings[id].outcome, original.outcome);
        assert_eq!(b.state.api_counts, counts);
        assert_eq!(
            b.state.bookings[id].generation_source_digest().unwrap(),
            original.generation_source_digest().unwrap()
        );
        assert!(!b.state.blocked);
        assert!(b.api_preparation_permit(id, &prepared).is_err());
        let before = snapshot(&data);
        b.record_api_generation_resolution(confirmed.clone())
            .unwrap();
        let mut changed = confirmed.clone();
        changed.review.input.amount_usd_nanos = 1;
        assert!(b.record_api_generation_resolution(changed).is_err());
        assert_eq!(snapshot(&data), before);
        let state = b.state.clone();
        drop(b);
        let mut b =
            BudgetLedger::open_existing(&data.join("currency-budgets/project"), "project").unwrap();
        assert_eq!(b.state, state);
        b.record_api_generation_resolution(confirmed).unwrap();
        assert_eq!(snapshot(&data), before);
        drop(b);
        fs::remove_file(data.join(crate::currency_api_resolution_guard::MARKER)).unwrap();
        let before = snapshot(&data);
        assert!(crate::Store::open(&data)
            .err()
            .unwrap()
            .contains("reader12"));
        assert!(
            BudgetLedger::open_existing(&data.join("currency-budgets/project"), "project").is_err()
        );
        assert_eq!(snapshot(&data), before);
    }
}
#[test]
fn api_generation_resolution_keeps_other_unpriced_block_and_rejects_late_double_charge() {
    let t = tempfile::tempdir().unwrap();
    let data = t.path().join("data");
    let p = policy();
    let b = book(&data, &p, true);
    drop(b);
    crate::currency_api_resolution_guard::tests::promote_fixture(&data);
    let mut b =
        BudgetLedger::open_existing(&data.join("currency-budgets/project"), "project").unwrap();
    let (first, _) = prepare(&data, &mut b, "first");
    let (second, _) = prepare(&data, &mut b, "second");
    let s = unpriced(&mut b, &first.request_id);
    unpriced(&mut b, &second.request_id);
    let stale = review(&b, &second.request_id, 23);
    b.record_api_generation_resolution(review(&b, &first.request_id, 11))
        .unwrap();
    assert!(b.state.blocked);
    assert!(b.state.available_usd_nanos(&p).is_err());
    let before = snapshot(&data);
    assert!(b.record_api_generation_resolution(stale).is_err());
    assert!(b.settle(s).is_err());
    assert_eq!(snapshot(&data), before);
    b.record_api_generation_resolution(review(&b, &second.request_id, p.limit_usd_nanos))
        .unwrap();
    assert!(!b.state.blocked);
    assert_eq!(b.state.charged_usd_nanos, p.limit_usd_nanos + 11);
    assert_eq!(b.state.reserved_usd_nanos, 34);
    assert!(b.state.available_usd_nanos(&p).is_err());
}
#[test]
fn api_generation_resolution_rejects_changed_source_amount_and_missing_guard_before_write() {
    let t = tempfile::tempdir().unwrap();
    let data = t.path().join("data");
    let p = policy();
    let mut b = book(&data, &p, true);
    let (call, _) = prepare(&data, &mut b, "attempt");
    b.unknown(&call.request_id, &"f".repeat(64)).unwrap();
    let before = snapshot(&data);
    assert!(b
        .record_api_generation_resolution(review(&b, &call.request_id, 1))
        .unwrap_err()
        .contains("reader12"));
    assert_eq!(snapshot(&data), before);
    drop(b);
    crate::currency_api_resolution_guard::tests::promote_fixture(&data);
    let mut b =
        BudgetLedger::open_existing(&data.join("currency-budgets/project"), "project").unwrap();
    let original = review(&b, &call.request_id, 1);
    for mode in 0..8 {
        let mut bad = original.clone();
        match mode {
            0 => bad.review.source_digest = "0".repeat(64),
            1 => bad.review.count_digest = "0".repeat(64),
            2 => bad.review.policy_digest = "0".repeat(64),
            3 => bad.review.previous_ledger_head = "0".repeat(64),
            4 => bad.review.input.request_id = "different".into(),
            5 => bad.review.maximum_usd_nanos += 1,
            6 => bad.review.input.amount_usd_nanos = MAX_USD_NANOS + 1,
            _ => bad.review.input.billing_reference = " ".into(),
        }
        let before = snapshot(&data);
        assert!(b.record_api_generation_resolution(bad).is_err());
        assert_eq!(snapshot(&data), before);
    }
}
