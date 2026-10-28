use super::*;
#[path = "fee_tests.rs"]
mod fee;
#[path = "generation_resolution_tests.rs"]
mod generation_resolution;
#[path = "resolution_tests.rs"]
mod resolution;
use crate::reader_generation::tests::snapshot;
use cerulean_adapters::api_generation::{
    scoped::fixture_counted_api, ApiClient, ApiMessage, ApiRequest,
};
use cerulean_domain::{
    api_worker::{ApiTarget, ApiWorkerSelection},
    currency_budget::scoped::{
        BillingBasis, ContextMetric, PriceBand, RequestedService, ScopedPriceQuote,
    },
    execution::*,
    secret::SecretInput,
};

fn policy() -> BudgetPolicy {
    let now = chrono::Utc::now();
    BudgetPolicy {
        schema: 2,
        planning: None,
        project_id: "project".into(),
        revision_digest: "a".repeat(64),
        limit_usd_nanos: 1_000_000,
        quotes: vec![],
        scoped_quotes: Some(vec![ScopedPriceQuote {
            schema: 1,
            target: ApiTarget::OpenAi {},
            requested_model: "fixture".into(),
            requested_service: RequestedService::Explicit {
                tier: "default".into(),
            },
            reported_models: vec!["fixture-actual".into()],
            reported_services: vec!["default".into()],
            billing_basis: BillingBasis::PerRequest,
            context_metric: ContextMetric::TotalInputIncludingCache,
            bands: vec![PriceBand {
                actual_model: "fixture-actual".into(),
                actual_service: "default".into(),
                input: InputRange {
                    from: 0,
                    through: 1000,
                },
                rates: Rates {
                    input: 1000,
                    output: 2000,
                    cache_read: Some(100),
                    cache_write: Some(1500),
                },
            }],
            count_price: Some(ManagementCountPrice {
                schema: 1,
                maximum_usd_nanos: 17,
                maximum_body_bytes: 262144,
                source: "Synthetic explicit count fee".into(),
            }),
            valid_from: (now - chrono::Duration::hours(1)).to_rfc3339(),
            valid_until: (now + chrono::Duration::hours(1)).to_rfc3339(),
            source: "Synthetic original scoped quote".into(),
        }]),
    }
}
fn book(data: &Path, policy: &BudgetPolicy, guarded: bool) -> BudgetLedger {
    crate::currency_api_guard::tests::reader10(data);
    if guarded {
        crate::currency_api_guard::tests::promote_fixture(data);
    }
    let mut book = BudgetLedger::open(&data.join("currency-budgets/project"), policy).unwrap();
    book.bind_initial_approval(ApprovalLink {
        previous_policy_digest: digest(policy).unwrap(),
        previous_ledger_head: book.head().into(),
        approval_evidence_sha256: "b".repeat(64),
    })
    .unwrap();
    book
}
fn make_draft(
    data: &Path,
    attempt: &str,
    round: u32,
) -> (ApiRunIdentity, CallIdentity, ScopedApiDraft) {
    let draft =
        ApiClient::new(SecretInput::new("sk-isolated-api-currency-fixture".into()).unwrap())
            .unwrap()
            .prepare_scoped(ApiRequest {
                target: ApiTarget::OpenAi {},
                model: "fixture".into(),
                instructions: "approved fixture".into(),
                messages: vec![ApiMessage::User(format!("synthetic round {round}"))],
                tools: vec![],
                max_output_tokens: 32,
                timeout_seconds: 10,
            })
            .unwrap();
    let run = ApiRunIdentity {
        version: 1,
        project_id: "project".into(),
        task_id: "task".into(),
        attempt_id: attempt.into(),
        revision_digest: "a".repeat(64),
        fence: 1,
        prompt_sha256: "b".repeat(64),
        binding: SessionBinding {
            adapter_id: "openai".into(),
            adapter_version: "api-worker-v1".into(),
            session_id: "fixture-session".into(),
            workspace: data.to_string_lossy().into_owned(),
            identity_strength: IdentityStrength::ControllerRequest(ControllerRequestIdentity {
                connection_id: "fixture-connection".into(),
                request_id: attempt.into(),
            }),
        },
        worker: ApiWorkerSelection {
            write_scope: None,
            target: ApiTarget::OpenAi {},
            model: "fixture".into(),
            file_tools: vec![ToolPermission::ReadFiles],
            max_rounds: 3,
            max_tool_calls: 3,
            max_output_tokens: 32,
            max_total_tokens: 1000,
            request_timeout_seconds: 10,
        },
    };
    let call = CallIdentity {
        project_id: run.project_id.clone(),
        task_id: run.task_id.clone(),
        attempt_id: attempt.into(),
        revision_digest: run.revision_digest.clone(),
        request_id: format!("api-{}-{round}", &digest(&attempt).unwrap()[..24]),
        round,
        fingerprint: draft.fingerprint().request.clone(),
    };
    (run, call, draft)
}
#[test]
fn scoped_api_currency_count_and_preparation_are_atomic_in_original_balance() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let policy = policy();
    let mut book = book(&data, &policy, true);
    let quote = policy.scoped_quotes.as_ref().unwrap()[0].clone();
    let (run, call, draft) = make_draft(&data, "attempt", 1);
    assert_eq!(
        book.begin_api_count(
            &run,
            &call,
            &draft,
            &"c".repeat(64),
            &digest(&quote).unwrap()
        )
        .unwrap(),
        17
    );
    assert_eq!(book.state.reserved_usd_nanos, 17);
    let counted = fixture_counted_api(draft, quote, 100).unwrap();
    book.record_api_count(&call.request_id, &counted).unwrap();
    let head = book.head().to_owned();
    book.record_api_count(&call.request_id, &counted).unwrap();
    assert_eq!(book.head(), head);
    let prepared = counted.prepare().unwrap();
    let amount = book
        .prepare_scoped_api(&call.request_id, &prepared)
        .unwrap();
    assert_eq!(book.state.reserved_usd_nanos, 17 + amount);
    let before = snapshot(&data);
    assert!(book
        .prepare_scoped_api(&call.request_id, &prepared)
        .is_err());
    assert_eq!(snapshot(&data), before);
    let original = book.state.clone();
    drop(book);
    let reopened =
        BudgetLedger::open_existing(&data.join("currency-budgets/project"), "project").unwrap();
    assert_eq!(reopened.state(), &original);
    assert_eq!(snapshot(&data), before);
    let encoded = serde_json::to_string(&original).unwrap();
    assert!(!encoded.contains("approved fixture"));
    assert!(!encoded.contains("sk-isolated"));
}
#[test]
fn scoped_api_currency_count_failure_and_price_gap_preserve_explicit_fee() {
    for mode in ["interrupted", "price-gap"] {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let policy = policy();
        let mut book = book(&data, &policy, true);
        let quote = policy.scoped_quotes.as_ref().unwrap()[0].clone();
        let (run, call, draft) = make_draft(&data, "attempt", 1);
        book.begin_api_count(
            &run,
            &call,
            &draft,
            &"c".repeat(64),
            &digest(&quote).unwrap(),
        )
        .unwrap();
        if mode == "price-gap" {
            let counted = fixture_counted_api(draft, quote, 1001).unwrap();
            book.record_api_count(&call.request_id, &counted).unwrap();
            assert!(counted.prepare().is_err());
        }
        drop(book);
        let mut book =
            BudgetLedger::open_existing(&data.join("currency-budgets/project"), "project").unwrap();
        book.recover_api_counts().unwrap();
        let head = book.head().to_owned();
        book.recover_api_counts().unwrap();
        assert_eq!(book.head(), head);
        assert_eq!(book.state.reserved_usd_nanos, 17);
        assert_eq!(book.state.charged_usd_nanos, 0);
        assert!(book.state.bookings.is_empty());
        if mode == "interrupted" {
            assert!(matches!(
                book.state.api_counts[&call.request_id].outcome,
                ApiCountOutcome::Unknown { .. }
            ));
        }
        let (run, call, draft) = make_draft(&data, "attempt", 1);
        assert!(book
            .begin_api_count(
                &run,
                &call,
                &draft,
                &"c".repeat(64),
                &digest(&policy.scoped_quotes.as_ref().unwrap()[0]).unwrap()
            )
            .is_err());
    }
}
#[test]
fn scoped_api_currency_reader10_and_round_mismatch_refuse_before_write() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let policy = policy();
    let mut book = book(&data, &policy, false);
    let q = digest(&policy.scoped_quotes.as_ref().unwrap()[0]).unwrap();
    let (run, call, draft) = make_draft(&data, "attempt", 1);
    let before = snapshot(&data);
    assert!(book
        .begin_api_count(&run, &call, &draft, &"c".repeat(64), &q)
        .is_err());
    assert_eq!(snapshot(&data), before);
    drop(book);
    crate::currency_api_guard::tests::promote_fixture(&data);
    let mut book =
        BudgetLedger::open_existing(&data.join("currency-budgets/project"), "project").unwrap();
    let mut wrong = call.clone();
    wrong.round = 2;
    let before = snapshot(&data);
    assert!(book
        .begin_api_count(&run, &wrong, &draft, &"c".repeat(64), &q)
        .is_err());
    assert_eq!(snapshot(&data), before);
    book.begin_api_count(&run, &call, &draft, &"c".repeat(64), &q)
        .unwrap();
    drop(book);
    fs::remove_file(data.join(crate::currency_api_guard::MARKER)).unwrap();
    let before = snapshot(&data);
    assert!(crate::Store::open(&data).is_err());
    assert_eq!(snapshot(&data), before);
}

#[test]
fn scoped_api_currency_competing_counts_and_generation_share_last_balance() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut policy = policy();
    policy.limit_usd_nanos = 17;
    let mut book = book(&data, &policy, true);
    let quote = policy.scoped_quotes.as_ref().unwrap()[0].clone();
    let (run, call, draft) = make_draft(&data, "first", 1);
    book.begin_api_count(
        &run,
        &call,
        &draft,
        &"c".repeat(64),
        &digest(&quote).unwrap(),
    )
    .unwrap();
    let before = snapshot(&data);
    let (other, second, second_draft) = make_draft(&data, "second", 1);
    assert!(book
        .begin_api_count(
            &other,
            &second,
            &second_draft,
            &"d".repeat(64),
            &digest(&quote).unwrap()
        )
        .is_err());
    assert_eq!(snapshot(&data), before);
    let counted = fixture_counted_api(draft, quote, 100).unwrap();
    book.record_api_count(&call.request_id, &counted).unwrap();
    let prepared = counted.prepare().unwrap();
    let before = snapshot(&data);
    assert!(book
        .prepare_scoped_api(&call.request_id, &prepared)
        .is_err());
    assert_eq!(snapshot(&data), before);
    assert_eq!(book.state.reserved_usd_nanos, 17);
    assert_eq!(book.state.api_counts.len(), 1);
    assert!(book.state.bookings.is_empty());
}

#[test]
fn scoped_api_currency_late_original_count_survives_new_approval_but_cannot_generate() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let policy = policy();
    let mut book = book(&data, &policy, true);
    let quote = policy.scoped_quotes.as_ref().unwrap()[0].clone();
    let (run, call, draft) = make_draft(&data, "attempt", 1);
    book.begin_api_count(
        &run,
        &call,
        &draft,
        &"c".repeat(64),
        &digest(&quote).unwrap(),
    )
    .unwrap();
    let counted = fixture_counted_api(draft, quote, 100).unwrap();
    let mut next = policy.clone();
    next.revision_digest = "d".repeat(64);
    next.scoped_quotes.as_mut().unwrap()[0].source = "new approved quote".into();
    book.authorize(PolicyAuthorization {
        previous_policy_digest: digest(book.policy()).unwrap(),
        previous_ledger_head: book.head().into(),
        policy: next,
        approval_evidence_sha256: "e".repeat(64),
    })
    .unwrap();
    book.record_api_count(&call.request_id, &counted).unwrap();
    assert_eq!(book.state.reserved_usd_nanos, 17);
    let prepared = counted.prepare().unwrap();
    let before = snapshot(&data);
    assert!(book
        .prepare_scoped_api(&call.request_id, &prepared)
        .is_err());
    assert_eq!(snapshot(&data), before);
    assert!(matches!(
        book.state.api_counts[&call.request_id].outcome,
        ApiCountOutcome::Counted { .. }
    ));
}
