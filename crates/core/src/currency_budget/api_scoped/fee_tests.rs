use super::*;
use crate::api_call_journal::ApiCallJournal;
use cerulean_adapters::api_generation::{scoped::fixture_scoped_api_result, ApiFailure};
use serde_json::{json, Value};

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
fn path(data: &Path, call: &CallIdentity) -> PathBuf {
    data.join("workflows/project")
        .join(format!("api-attempt-{}", digest(&call.attempt_id).unwrap()))
        .join(format!("rounds/{:03}", call.round))
}
fn reply(id: &str) -> Value {
    json!({"id":id,"model":"fixture-actual","status":"completed","service_tier":"default",
        "output":[{"type":"message","id":"msg_fixture","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Done"}]}],
        "usage":{"input_tokens":12,"output_tokens":2,"input_tokens_details":{"cached_tokens":2,"cache_write_tokens":1}}})
}
#[tokio::test]
async fn scoped_api_fee_original_settlement_ack_releases_reply_once_and_is_stable() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let policy = policy();
    let mut book = book(&data, &policy, true);
    let (call, prepared) = prepare(&data, &mut book, "attempt");
    let root = path(&data, &call);
    let mut journal = ApiCallJournal::open(&root, &call).unwrap();
    let result = fixture_scoped_api_result(&prepared, reply("resp_original")).unwrap();
    let permit = book
        .api_preparation_permit(&call.request_id, &prepared)
        .unwrap();
    let pending = journal
        .execute_scoped_with(prepared, permit, |_| async { Ok(result) })
        .await
        .unwrap();
    assert!(journal.require_fee_ack().is_err());
    assert!(book.api_fee_permit(&journal).is_err());
    book.observe_api(&journal).unwrap();
    let settled = book.state.bookings[&call.request_id].outcome.clone();
    let amount = book.state.charged_usd_nanos;
    assert!(matches!(settled, Outcome::Settled { .. }));
    assert_eq!(book.state.reserved_usd_nanos, 17);
    let permit = book.api_fee_permit(&journal).unwrap();
    let proof = permit.proof().clone();
    let released = journal
        .release_scoped_reply(pending, permit)
        .unwrap()
        .unwrap();
    assert_eq!(released.observation().text, "Done");
    journal.require_fee_ack().unwrap();
    assert_eq!(journal.state().fee_ack.as_ref(), Some(&proof));
    // Later same-account activity cannot invalidate the already settled record hash.
    let _ = prepare(&data, &mut book, "other");
    assert_eq!(book.api_fee_permit(&journal).unwrap().proof(), &proof);
    let before = book.head().to_owned();
    book.observe_api(&journal).unwrap();
    assert_eq!(book.head(), before);
    assert_eq!(book.state.charged_usd_nanos, amount);
    drop(journal);
    drop(book);
    let mut book =
        BudgetLedger::open_existing(&data.join("currency-budgets/project"), "project").unwrap();
    let journal = ApiCallJournal::read_existing(&root).unwrap();
    journal.require_fee_ack().unwrap();
    let before = snapshot(&data);
    book.check_api(&journal).unwrap();
    book.observe_api(&journal).unwrap();
    assert_eq!(snapshot(&data), before);
}
#[tokio::test]
async fn scoped_api_fee_invalid_output_still_settles_but_unknown_terms_block_ack() {
    for mode in [
        "invalid-tools",
        "missing-tier",
        "missing-cache",
        "above-input",
        "unquoted-tier",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let policy = policy();
        let mut book = book(&data, &policy, true);
        let (call, prepared) = prepare(&data, &mut book, "attempt");
        let mut v = reply("resp_billing");
        match mode {
            "invalid-tools" => {
                v["output"] = json!([{"type":"function_call","id":"fc_bad","call_id":"call_bad","name":"unapproved","arguments":"{}","status":"completed"}])
            }
            "missing-tier" => {
                v.as_object_mut().unwrap().remove("service_tier");
            }
            "missing-cache" => {
                v["usage"]["input_tokens_details"]
                    .as_object_mut()
                    .unwrap()
                    .remove("cache_write_tokens");
            }
            "above-input" => v["usage"]["input_tokens"] = json!(101),
            "unquoted-tier" => v["service_tier"] = json!("priority"),
            _ => unreachable!(),
        }
        let result = fixture_scoped_api_result(&prepared, v).unwrap();
        let permit = book
            .api_preparation_permit(&call.request_id, &prepared)
            .unwrap();
        let mut journal = ApiCallJournal::open(&path(&data, &call), &call).unwrap();
        let pending = journal
            .execute_scoped_with(prepared, permit, |_| async { Ok(result) })
            .await
            .unwrap();
        let reserved = book.state.reserved_usd_nanos;
        book.observe_api(&journal).unwrap();
        if mode == "invalid-tools" {
            let permit = book.api_fee_permit(&journal).unwrap();
            assert!(journal
                .release_scoped_reply(pending, permit)
                .unwrap()
                .is_err());
            assert!(matches!(
                book.state.bookings[&call.request_id].outcome,
                Outcome::Settled { .. }
            ));
        } else {
            assert!(book.api_fee_permit(&journal).is_err(), "{mode}");
            assert!(journal.require_fee_ack().is_err());
            assert_eq!(book.state.reserved_usd_nanos, reserved);
            assert_eq!(book.state.charged_usd_nanos, 0);
        }
    }
}
#[tokio::test]
async fn scoped_api_fee_lost_ack_reopens_without_rebilling_or_reply_reconstruction() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let policy = policy();
    let mut book = book(&data, &policy, true);
    let (call, prepared) = prepare(&data, &mut book, "attempt");
    let root = path(&data, &call);
    let result = fixture_scoped_api_result(&prepared, reply("resp_lost_ack")).unwrap();
    let permit = book
        .api_preparation_permit(&call.request_id, &prepared)
        .unwrap();
    let mut journal = ApiCallJournal::open(&root, &call).unwrap();
    let pending = journal
        .execute_scoped_with(prepared, permit, |_| async { Ok(result) })
        .await
        .unwrap();
    book.observe_api(&journal).unwrap();
    let state = book.state.clone();
    drop(pending);
    drop(journal);
    drop(book);
    let mut book =
        BudgetLedger::open_existing(&data.join("currency-budgets/project"), "project").unwrap();
    let journal = ApiCallJournal::read_existing(&root).unwrap();
    let before = snapshot(&data);
    book.observe_api(&journal).unwrap();
    assert_eq!(book.state, state);
    assert_eq!(snapshot(&data), before);
    assert!(journal.require_fee_ack().is_err());
    assert!(book.api_fee_permit(&journal).is_ok());
    assert!(!root.join("02.json").exists());
}
#[tokio::test]
async fn scoped_api_fee_cancellation_retains_liability_and_not_sent_only_releases_generation() {
    for failure in [
        ApiFailure::LocalCancellation {},
        ApiFailure::Transport {},
        ApiFailure::NotSent {},
    ] {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let policy = policy();
        let mut book = book(&data, &policy, true);
        let (call, prepared) = prepare(&data, &mut book, "attempt");
        let permit = book
            .api_preparation_permit(&call.request_id, &prepared)
            .unwrap();
        let mut journal = ApiCallJournal::open(&path(&data, &call), &call).unwrap();
        let not_sent = matches!(failure, ApiFailure::NotSent {});
        let reserved = book.state.reserved_usd_nanos;
        let _pending = journal
            .execute_scoped_with(prepared, permit, |_| async { Err(failure) })
            .await
            .unwrap();
        book.observe_api(&journal).unwrap();
        assert!(book.api_fee_permit(&journal).is_err());
        assert!(journal.require_fee_ack().is_err());
        assert_eq!(
            book.state.reserved_usd_nanos,
            if not_sent { 17 } else { reserved }
        );
    }
}
#[test]
fn scoped_api_fee_prepared_only_journal_is_readonly_and_preserves_unknown_reservation() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let policy = policy();
    let mut book = book(&data, &policy, true);
    let (call, _) = prepare(&data, &mut book, "attempt");
    let root = path(&data, &call);
    drop(ApiCallJournal::open(&root, &call).unwrap());
    assert!(ApiCallJournal::read_existing(&root).is_err());
    let journal = ApiCallJournal::read_prepared_existing(&root, &call).unwrap();
    let reserved = book.state.reserved_usd_nanos;
    book.observe_api(&journal).unwrap();
    assert_eq!(book.state.reserved_usd_nanos, reserved);
    assert!(matches!(
        book.state.bookings[&call.request_id].outcome,
        Outcome::Unknown { .. }
    ));
    let before = snapshot(&data);
    book.observe_api(&journal).unwrap();
    assert_eq!(snapshot(&data), before);
    assert!(!root.join("00.json").exists());
}

#[tokio::test]
async fn scoped_api_fee_forged_ack_and_failed_ack_publication_never_release_reply() {
    for mode in ["forged", "pending"] {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let policy = policy();
        let mut book = book(&data, &policy, true);
        let (call, prepared) = prepare(&data, &mut book, "attempt");
        let root = path(&data, &call);
        let result = fixture_scoped_api_result(&prepared, reply("resp_ack")).unwrap();
        let permit = book
            .api_preparation_permit(&call.request_id, &prepared)
            .unwrap();
        let mut journal = ApiCallJournal::open(&root, &call).unwrap();
        let pending = journal
            .execute_scoped_with(prepared, permit, |_| async { Ok(result) })
            .await
            .unwrap();
        book.observe_api(&journal).unwrap();
        let state = book.state.clone();
        let permit = book.api_fee_permit(&journal).unwrap();
        if mode == "pending" {
            fs::write(root.join("02.pending"), b"owned interrupted ACK").unwrap();
            assert!(journal.release_scoped_reply(pending, permit).is_err());
            assert!(!journal.healthy());
            assert_eq!(book.state, state);
            assert!(!root.join("02.json").exists());
            drop(journal);
            let before = snapshot(&data);
            assert!(ApiCallJournal::read_existing(&root).is_err());
            assert_eq!(snapshot(&data), before);
        } else {
            journal
                .release_scoped_reply(pending, permit)
                .unwrap()
                .unwrap();
            drop(journal);
            let p = root.join("02.json");
            let mut v: Value = serde_json::from_slice(&fs::read(&p).unwrap()).unwrap();
            v["body"]["event"]["ack"]["settlementHead"] = json!("f".repeat(64));
            fs::write(&p, crate::api_call_journal::fixture_rehash_record(v)).unwrap();
            let journal = ApiCallJournal::read_existing(&root).unwrap();
            let before = snapshot(&data);
            assert!(book
                .check_api(&journal)
                .unwrap_err()
                .contains("ACK differs"));
            assert_eq!(book.state, state);
            assert_eq!(snapshot(&data), before);
        }
    }
}

#[tokio::test]
async fn scoped_api_fee_duplicate_response_is_rejected_without_second_charge() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let policy = policy();
    let mut book = book(&data, &policy, true);
    for attempt in ["first", "second"] {
        let (call, prepared) = prepare(&data, &mut book, attempt);
        let result = fixture_scoped_api_result(&prepared, reply("resp_same_invoice")).unwrap();
        let permit = book
            .api_preparation_permit(&call.request_id, &prepared)
            .unwrap();
        let mut journal = ApiCallJournal::open(&path(&data, &call), &call).unwrap();
        let _pending = journal
            .execute_scoped_with(prepared, permit, |_| async { Ok(result) })
            .await
            .unwrap();
        if attempt == "first" {
            book.observe_api(&journal).unwrap();
        } else {
            let state = book.state.clone();
            let before = snapshot(&data);
            assert!(book.observe_api(&journal).is_err());
            assert_eq!(book.state, state);
            assert_eq!(snapshot(&data), before);
            assert!(book.api_fee_permit(&journal).is_err());
        }
    }
}
