use super::*;
use crate::api_count_resolution::{ApiCountResolution, ApiCountResolutionReview};
use cerulean_domain::api_count_resolution::ApiCountResolutionInput;

fn review(book: &BudgetLedger, request: &str, amount: u64) -> ApiCountResolution {
    let count = &book.state().api_counts[request];
    ApiCountResolution {
        review: ApiCountResolutionReview {
            input: ApiCountResolutionInput {
                project_id: "project".into(),
                request_id: request.into(),
                amount_usd_nanos: amount,
                billing_reference: "isolated invoice".into(),
                explanation: "fixture user confirmation".into(),
            },
            source_digest: count.source_digest().unwrap(),
            policy_digest: digest(book.policy()).unwrap(),
            previous_ledger_head: book.head().into(),
            maximum_usd_nanos: count.attempt.maximum_usd_nanos,
            original_outcome: count.original_outcome().into(),
            generation_digest: book.state().api_count_generation_digest(request).unwrap(),
        },
        user_event_digest: "e".repeat(64),
    }
}
#[test]
fn api_count_resolution_retains_generation_and_replays_exact_confirmation_once() {
    for generation in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let policy = policy();
        let book = book(&data, &policy, true);
        drop(book);
        crate::currency_api_resolution_guard::tests::promote_fixture(&data);
        let mut book =
            BudgetLedger::open_existing(&data.join("currency-budgets/project"), "project").unwrap();
        let (run, call, draft) = make_draft(&data, "attempt", 1);
        let quote = policy.scoped_quotes.as_ref().unwrap()[0].clone();
        book.begin_api_count(
            &run,
            &call,
            &draft,
            &"c".repeat(64),
            &digest(&quote).unwrap(),
        )
        .unwrap();
        let before = snapshot(&data);
        assert!(book
            .check_api_count_resolution(&review(&book, &call.request_id, 0))
            .is_err());
        assert_eq!(snapshot(&data), before);
        let counted = fixture_counted_api(draft, quote, 100).unwrap();
        book.record_api_count(&call.request_id, &counted).unwrap();
        let prepared = counted.prepare().unwrap();
        let amount = if generation {
            let value = book
                .prepare_scoped_api(&call.request_id, &prepared)
                .unwrap();
            book.unknown(&call.request_id, &"f".repeat(64)).unwrap();
            value
        } else {
            0
        };
        let original = book.state().api_counts[&call.request_id].clone();
        let generation_before = book.state().bookings.clone();
        let confirmation = review(&book, &call.request_id, 11);
        let before = snapshot(&data);
        book.check_api_count_resolution(&confirmation).unwrap();
        assert_eq!(snapshot(&data), before);
        book.record_api_count_resolution(confirmation.clone())
            .unwrap();
        assert_eq!(book.state().reserved_usd_nanos, amount);
        assert_eq!(book.state().charged_usd_nanos, 11);
        assert_eq!(book.state().bookings, generation_before);
        assert_eq!(
            book.state().api_counts[&call.request_id].attempt,
            original.attempt
        );
        assert_eq!(
            book.state().api_counts[&call.request_id].outcome,
            original.outcome
        );
        let before = snapshot(&data);
        book.record_api_count_resolution(confirmation.clone())
            .unwrap();
        assert!(book
            .prepare_scoped_api(&call.request_id, &prepared)
            .is_err());
        let mut changed = confirmation.clone();
        changed.review.input.amount_usd_nanos += 1;
        assert!(book.record_api_count_resolution(changed).is_err());
        assert_eq!(snapshot(&data), before);
        let state = book.state().clone();
        drop(book);
        let mut reopened =
            BudgetLedger::open_existing(&data.join("currency-budgets/project"), "project").unwrap();
        assert_eq!(reopened.state(), &state);
        reopened.record_api_count_resolution(confirmation).unwrap();
        assert_eq!(snapshot(&data), before);
        drop(reopened);
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
fn api_count_resolution_unknown_fee_guard_staleness_and_actual_overage() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let policy = policy();
    let mut book = book(&data, &policy, true);
    let (run, call, draft) = make_draft(&data, "attempt", 1);
    let quote = policy.scoped_quotes.as_ref().unwrap()[0].clone();
    book.begin_api_count(
        &run,
        &call,
        &draft,
        &"c".repeat(64),
        &digest(&quote).unwrap(),
    )
    .unwrap();
    book.mark_api_count_unknown(&call.request_id, &"f".repeat(64))
        .unwrap();
    let resolution = review(&book, &call.request_id, 0);
    let before = snapshot(&data);
    assert!(book
        .record_api_count_resolution(resolution)
        .unwrap_err()
        .contains("reader12"));
    assert_eq!(snapshot(&data), before);
    assert!(!serde_json::to_string(book.state())
        .unwrap()
        .contains("feeResolution"));
    drop(book);
    crate::currency_api_resolution_guard::tests::promote_fixture(&data);
    let mut book =
        BudgetLedger::open_existing(&data.join("currency-budgets/project"), "project").unwrap();
    let original = review(&book, &call.request_id, policy.limit_usd_nanos + 1);
    for mode in 0..7 {
        let mut changed = original.clone();
        match mode {
            0 => changed.review.source_digest = "0".repeat(64),
            1 => changed.review.previous_ledger_head = "0".repeat(64),
            2 => changed.review.generation_digest = "0".repeat(64),
            3 => changed.review.input.request_id = "other-round".into(),
            4 => changed.review.input.billing_reference = " ".into(),
            5 => changed.review.maximum_usd_nanos += 1,
            _ => changed.review.input.amount_usd_nanos = MAX_USD_NANOS + 1,
        }
        let before = snapshot(&data);
        assert!(book.record_api_count_resolution(changed).is_err());
        assert_eq!(snapshot(&data), before);
    }
    book.record_api_count_resolution(original).unwrap();
    assert_eq!(book.state().charged_usd_nanos, policy.limit_usd_nanos + 1);
    assert_eq!(book.state().reserved_usd_nanos, 0);
    assert!(book.state().available_usd_nanos(&policy).is_err());
    assert!(matches!(
        book.state().api_counts[&call.request_id].outcome,
        ApiCountOutcome::Unknown { .. }
    ));
    let (run, call, draft) = make_draft(&data, "next", 1);
    let before = snapshot(&data);
    assert!(book
        .begin_api_count(
            &run,
            &call,
            &draft,
            &"c".repeat(64),
            &digest(&quote).unwrap()
        )
        .is_err());
    assert_eq!(snapshot(&data), before);
}
