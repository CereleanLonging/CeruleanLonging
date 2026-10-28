use super::*;
use cerulean_domain::api_worker::ApiTarget;
#[path = "historical_tests.rs"]
mod historical;

fn policy() -> BudgetPolicy {
    BudgetPolicy {
        scoped_quotes: None,
        planning: None,
        schema: 1,
        project_id: "project".into(),
        revision_digest: "a".repeat(64),
        limit_usd_nanos: 600_000,
        quotes: vec![PriceQuote {
            count_price: None,
            schema: 1,
            target: ApiTarget::OpenAi {},
            requested_model: "fixture-alias".into(),
            reported_models: vec!["fixture-snapshot".into()],
            rates: Rates {
                input: 2_500_000,
                output: 10_000_000,
                cache_read: Some(250_000),
                cache_write: None,
            },
            valid_from: "2020-01-01T00:00:00Z".into(),
            valid_until: "2100-01-01T00:00:00Z".into(),
            source: "Synthetic test rates; not supplier prices".into(),
        }],
    }
}
fn reservation(p: &BudgetPolicy, id: &str) -> Reservation {
    Reservation {
        pricing: None,
        request_id: id.into(),
        operation_id: "operation".into(),
        role: ChargeRole::Worker,
        quote_digest: digest(&p.quotes[0]).unwrap(),
        request_sha256: digest(&id).unwrap(),
        ceiling: TokenCeiling {
            total_input: 100,
            output: 10,
            cache_read_allowed: true,
            cache_write_allowed: false,
        },
        ceiling_evidence_sha256: "b".repeat(64),
    }
}

#[test]
fn potentially_billable_cache_categories_need_prices_before_reservation() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("budget");
    let p = policy();
    let mut book = BudgetLedger::open(&path, &p).unwrap();
    let mut r = reservation(&p, "unpriced-cache-write");
    r.ceiling.cache_write_allowed = true;
    let head = book.head().to_owned();
    assert!(book.reserve(r).is_err());
    assert_eq!(book.head(), head);
    assert!(book.state().bookings.is_empty());
    let mut r = reservation(&p, "no-cache-write");
    r.ceiling.cache_read_allowed = false;
    book.reserve(r.clone()).unwrap();
    let mut s = settlement(&r);
    s.usage.cache_read = 1;
    assert!(matches!(book.settle(s).unwrap(), Outcome::Unpriced { .. }));
    assert!(book.state().blocked);
}
fn settlement(r: &Reservation) -> Settlement {
    Settlement {
        actual_service: None,
        request_id: r.request_id.clone(),
        response_id: format!("response-{}", r.request_id),
        request_sha256: r.request_sha256.clone(),
        actual_model: "fixture-snapshot".into(),
        usage: Usage {
            input: 10,
            output: 1,
            cache_read: 0,
            cache_write: 0,
        },
        evidence_sha256: "c".repeat(64),
    }
}

#[test]
fn observation_batch_capacity_refuses_before_first_outcome() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("budget");
    let mut p = policy();
    p.limit_usd_nanos = 1_000_000;
    let mut book = BudgetLedger::open(&path, &p).unwrap();
    let a = reservation(&p, "batch-a");
    let b = reservation(&p, "batch-b");
    book.reserve(a.clone()).unwrap();
    book.reserve(b.clone()).unwrap();
    // Simulate the capacity boundary without generating thousands of records.
    book.count = MAX_RECORDS - 1;
    let state = book.state.clone();
    let head = book.head.clone();
    let files = fs::read_dir(&path).unwrap().count();
    let error = book
        .observe_events(
            vec![
                Event::Settle {
                    settlement: settlement(&a),
                },
                Event::Settle {
                    settlement: settlement(&b),
                },
            ],
            true,
        )
        .unwrap_err();
    assert!(error.contains("whole observation batch"), "{error}");
    assert_eq!(book.state, state);
    assert_eq!(book.head, head);
    assert_eq!(fs::read_dir(path).unwrap().count(), files);
    assert!(book.healthy());
}

#[test]
fn shared_reservations_survive_restart_and_only_complete_usage_releases_difference() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("budget");
    let p = policy();
    let mut book = BudgetLedger::open(&path, &p).unwrap();
    let mut first = reservation(&p, "first");
    first.role = ChargeRole::Chief;
    assert_eq!(book.reserve(first.clone()).unwrap(), 350_000);
    let untouched = book.head().to_owned();
    assert!(book.reserve(reservation(&p, "too-much")).is_err());
    assert_eq!(book.head(), untouched);
    let mut second = reservation(&p, "second");
    second.role = ChargeRole::Deputy;
    second.ceiling.total_input = 50;
    assert_eq!(book.reserve(second.clone()).unwrap(), 225_000);
    book.unknown("first", &"d".repeat(64)).unwrap();
    assert_eq!(book.state().available_usd_nanos(&p).unwrap(), 25_000);
    assert!(book.not_sent("first", &"e".repeat(64)).is_err());
    let head = book.head().to_owned();
    let original = book.state().clone();
    drop(book);
    let mut book = BudgetLedger::open(&path, &p).unwrap();
    assert_eq!(book.head(), head);
    assert_eq!(book.state(), &original);
    assert!(book.reserve(first.clone()).is_err());
    assert!(matches!(
        book.settle(settlement(&first)).unwrap(),
        Outcome::Settled {
            charged_usd_nanos: 35_000,
            ..
        }
    ));
    assert_eq!(book.state().charged_usd_nanos, 35_000);
    assert_eq!(book.state().reserved_usd_nanos, 225_000);
    assert!(book.settle(settlement(&first)).is_err());
    book.not_sent("second", &"f".repeat(64)).unwrap();
    assert_eq!(book.state().available_usd_nanos(&p).unwrap(), 565_000);
    assert!(book.reserve(second).is_err());
    let expected = book.state().clone();
    drop(book);
    assert_eq!(BudgetLedger::open(&path, &p).unwrap().state(), &expected);
}

#[test]
fn unpriced_or_above_ceiling_usage_is_durable_and_blocks_further_reservations() {
    for case in ["model", "cache", "ceiling"] {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("budget");
        let p = policy();
        let mut book = BudgetLedger::open(&path, &p).unwrap();
        let r = reservation(&p, "first");
        book.reserve(r.clone()).unwrap();
        let mut observed = settlement(&r);
        match case {
            "model" => observed.actual_model = "unapproved-model".into(),
            "cache" => observed.usage.cache_write = 1,
            _ => observed.usage.output = 11,
        }
        assert!(
            matches!(book.settle(observed.clone()).unwrap(),Outcome::Unpriced{settlement,..} if settlement==observed)
        );
        assert_eq!(book.state().charged_usd_nanos, 0);
        assert_eq!(book.state().reserved_usd_nanos, 350_000);
        assert!(book.state().blocked);
        assert!(book.reserve(reservation(&p, "second")).is_err());
        let expected = book.state().clone();
        drop(book);
        let mut book = BudgetLedger::open(&path, &p).unwrap();
        assert_eq!(book.state(), &expected);
        assert!(book.reserve(reservation(&p, "second")).is_err());
    }
}

#[test]
fn stale_quotes_changed_approvals_and_mismatched_responses_do_not_release_credit() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("budget");
    let p = policy();
    let mut book = BudgetLedger::open(&path, &p).unwrap();
    let r = reservation(&p, "first");
    book.reserve(r.clone()).unwrap();
    let mut s = settlement(&r);
    s.request_sha256 = "0".repeat(64);
    let head = book.head().to_owned();
    assert!(book.settle(s).is_err());
    assert_eq!(head, book.head());
    drop(book);
    let mut changed = p.clone();
    changed.limit_usd_nanos += 1;
    assert!(BudgetLedger::open(&path, &changed).is_err());
    changed = p.clone();
    changed.quotes[0].rates.output += 1;
    assert!(BudgetLedger::open(&path, &changed).is_err());
    changed = p.clone();
    changed.revision_digest = "b".repeat(64);
    assert!(BudgetLedger::open(&path, &changed).is_err());
    let mut expired = p;
    expired.quotes[0].valid_until = "2021-01-01T00:00:00Z".into();
    let mut stale = BudgetLedger::open(&tmp.path().join("expired"), &expired).unwrap();
    assert!(stale.reserve(reservation(&expired, "first")).is_err());
    assert!(stale.state().bookings.is_empty());
}

#[test]
fn one_response_cannot_settle_two_same_provider_requests() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("budget");
    let mut p = policy();
    p.limit_usd_nanos = 1_000_000;
    let mut book = BudgetLedger::open(&path, &p).unwrap();
    let a = reservation(&p, "a");
    let b = reservation(&p, "b");
    book.reserve(a.clone()).unwrap();
    book.reserve(b.clone()).unwrap();
    book.settle(settlement(&a)).unwrap();
    let mut reused = settlement(&b);
    reused.response_id = settlement(&a).response_id;
    assert!(book.settle(reused).is_err());
    assert_eq!(book.state().reserved_usd_nanos, 350_000);
    book.settle(settlement(&b)).unwrap();
    assert_eq!(book.state().charged_usd_nanos, 70_000);
}

#[test]
fn writer_exclusion_and_interrupted_publication_cannot_create_spending_capacity() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("budget");
    let p = policy();
    let mut book = BudgetLedger::open(&path, &p).unwrap();
    assert!(BudgetLedger::open(&path, &p).is_err());
    // An unexpected final record causes immutable publication to fail. The
    // original pending bytes survive and the in-memory owner remains poisoned.
    fs::write(path.join("00000001.json"), b"occupied").unwrap();
    assert!(book.reserve(reservation(&p, "first")).is_err());
    assert!(!book.healthy());
    assert!(path.join("00000001.pending").is_file());
    assert_eq!(fs::read(path.join("00000001.json")).unwrap(), b"occupied");
    assert!(book.reserve(reservation(&p, "second")).is_err());
    drop(book);
    assert!(BudgetLedger::open(&path, &p).is_err());
}

#[test]
fn corrupt_records_and_missing_sequence_refuse_reopen_without_rewriting_evidence() {
    for mode in ["edited", "missing", "pending"] {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("budget");
        let p = policy();
        let mut book = BudgetLedger::open(&path, &p).unwrap();
        let r = reservation(&p, "first");
        book.reserve(r).unwrap();
        book.unknown("first", &"d".repeat(64)).unwrap();
        drop(book);
        match mode {
            "edited" => {
                let f = path.join("00000001.json");
                let mut v: serde_json::Value =
                    serde_json::from_slice(&fs::read(&f).unwrap()).unwrap();
                v["body"]["event"]["reservation"]["ceiling"]["output"] = 1.into();
                fs::write(f, serde_json::to_vec(&v).unwrap()).unwrap();
            }
            "missing" => {
                fs::rename(
                    path.join("00000001.json"),
                    tmp.path().join("preserved-original.json"),
                )
                .unwrap();
            }
            _ => {
                fs::write(path.join("00000003.pending"), b"unpublished").unwrap();
            }
        }
        let before: Vec<_> = fs::read_dir(&path)
            .unwrap()
            .map(|f| {
                let f = f.unwrap();
                (f.file_name(), fs::read(f.path()).unwrap())
            })
            .collect();
        assert!(BudgetLedger::open(&path, &p).is_err());
        for (name, bytes) in before {
            assert_eq!(fs::read(path.join(name)).unwrap(), bytes);
        }
    }
}

fn authorization(book: &BudgetLedger, next: BudgetPolicy) -> PolicyAuthorization {
    PolicyAuthorization {
        previous_policy_digest: digest(book.policy()).unwrap(),
        previous_ledger_head: book.head().into(),
        policy: next,
        approval_evidence_sha256: "d".repeat(64),
    }
}

#[test]
fn plan_revision_preserves_spending_unknowns_and_original_prices_for_late_responses() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("budget");
    let mut first = policy();
    first.limit_usd_nanos = 1_000_000;
    let mut book = BudgetLedger::open(&path, &first).unwrap();
    let paid = reservation(&first, "paid");
    book.reserve(paid.clone()).unwrap();
    book.settle(settlement(&paid)).unwrap();
    let late = reservation(&first, "late");
    book.reserve(late.clone()).unwrap();
    book.unknown("late", &"e".repeat(64)).unwrap();
    let original_files: Vec<_> = fs::read_dir(&path)
        .unwrap()
        .filter(|p| p.as_ref().is_ok_and(|p| p.file_name() != "writer.lock"))
        .map(|p| {
            let p = p.unwrap();
            (p.file_name(), fs::read(p.path()).unwrap())
        })
        .collect();
    let mut next = first.clone();
    next.revision_digest = "2".repeat(64);
    next.limit_usd_nanos = 1_500_000;
    next.quotes[0].rates.input = 5_000_000;
    next.quotes[0].rates.output = 20_000_000;
    book.authorize(authorization(&book, next.clone())).unwrap();
    assert_eq!(book.state().charged_usd_nanos, 35_000);
    assert_eq!(book.state().reserved_usd_nanos, 350_000);
    assert_eq!(book.state().available_usd_nanos(&next).unwrap(), 1_115_000);
    assert!(book.state().available_usd_nanos(&first).is_err());
    assert!(book.reserve(late.clone()).is_err());
    for (name, bytes) in original_files {
        assert_eq!(fs::read(path.join(name)).unwrap(), bytes);
    }
    let head = book.head().to_owned();
    drop(book);
    assert!(BudgetLedger::open(&path, &first).is_err());
    let mut book = BudgetLedger::open(&path, &next).unwrap();
    assert_eq!(book.head(), head);
    assert!(matches!(
        book.settle(settlement(&late)).unwrap(),
        Outcome::Settled {
            charged_usd_nanos: 35_000,
            ..
        }
    ));
    let fresh = reservation(&next, "fresh");
    assert_eq!(book.reserve(fresh.clone()).unwrap(), 700_000);
    assert!(matches!(
        book.settle(settlement(&fresh)).unwrap(),
        Outcome::Settled {
            charged_usd_nanos: 70_000,
            ..
        }
    ));
    assert_eq!(book.state().charged_usd_nanos, 140_000);
    assert_eq!(book.state().reserved_usd_nanos, 0);
    let expected = book.state().clone();
    drop(book);
    assert_eq!(BudgetLedger::open(&path, &next).unwrap().state(), &expected);
}

#[test]
fn authorization_rejects_stale_heads_insufficient_limits_cross_project_and_revision_reuse() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("budget");
    let p = policy();
    let mut book = BudgetLedger::open(&path, &p).unwrap();
    let mut next = p.clone();
    next.revision_digest = "2".repeat(64);
    let stale = authorization(&book, next.clone());
    book.reserve(reservation(&p, "first")).unwrap();
    assert!(book.authorize(stale).is_err());
    for mode in ["limit", "project", "revision", "policy"] {
        let mut a = authorization(&book, next.clone());
        match mode {
            "limit" => a.policy.limit_usd_nanos = 349_999,
            "project" => a.policy.project_id = "other".into(),
            "revision" => a.policy.revision_digest = p.revision_digest.clone(),
            _ => a.previous_policy_digest = "0".repeat(64),
        }
        let head = book.head().to_owned();
        assert!(book.authorize(a).is_err());
        assert_eq!(head, book.head());
    }
    next.limit_usd_nanos = 350_000;
    book.authorize(authorization(&book, next.clone())).unwrap();
    assert_eq!(book.state().available_usd_nanos(&next).unwrap(), 0);
    assert!(book.authorize(authorization(&book, p)).is_err());
    assert!(book.reserve(reservation(&next, "second")).is_err());
}

#[test]
fn new_approval_cannot_clear_an_unpriced_block_and_failed_publication_cannot_enable_new_limit() {
    for interrupt in [false, true] {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("budget");
        let p = policy();
        let mut book = BudgetLedger::open(&path, &p).unwrap();
        let r = reservation(&p, "first");
        book.reserve(r.clone()).unwrap();
        let mut s = settlement(&r);
        s.actual_model = "unknown-model".into();
        book.settle(s).unwrap();
        let old = book.state().clone();
        let mut next = p.clone();
        next.revision_digest = "2".repeat(64);
        next.limit_usd_nanos *= 2;
        if interrupt {
            fs::write(
                path.join(format!("{:08}.pending", book.count)),
                b"preserved interruption",
            )
            .unwrap();
        }
        let result = book.authorize(authorization(&book, next.clone()));
        if interrupt {
            assert!(result.is_err());
            assert!(!book.healthy());
            assert_eq!(book.state(), &old);
            assert_eq!(book.policy(), &p);
            drop(book);
            assert!(BudgetLedger::open(&path, &next).is_err());
        } else {
            result.unwrap();
            assert!(book.state().blocked);
            assert_eq!(book.state().reserved_usd_nanos, 350_000);
            assert!(book.reserve(reservation(&next, "second")).is_err());
            drop(book);
            assert!(BudgetLedger::open(&path, &next).unwrap().state().blocked);
        }
    }
}

fn api_fixture(
    root: &Path,
    p: &BudgetPolicy,
) -> (
    cerulean_adapters::api_generation::PreparedRequest,
    CallIdentity,
    ApiCallJournal,
    Reservation,
) {
    use cerulean_adapters::api_generation::{ApiClient, ApiMessage, ApiRequest};
    use cerulean_domain::secret::SecretInput;
    let client =
        ApiClient::new(SecretInput::new("sk-synthetic-budget-fixture".into()).unwrap()).unwrap();
    let request = client
        .prepare(ApiRequest {
            target: ApiTarget::OpenAi {},
            model: "fixture-alias".into(),
            instructions: "Approved synthetic scope".into(),
            messages: vec![ApiMessage::User("Return a marker.".into())],
            tools: vec![],
            max_output_tokens: 10,
            timeout_seconds: 10,
        })
        .unwrap();
    let id = CallIdentity {
        project_id: p.project_id.clone(),
        task_id: "task".into(),
        attempt_id: "operation".into(),
        revision_digest: p.revision_digest.clone(),
        request_id: "api-request".into(),
        round: 1,
        fingerprint: request.fingerprint().clone(),
    };
    let journal = ApiCallJournal::open(&root.join("api-call"), &id).unwrap();
    let mut r = reservation(p, &id.request_id);
    r.request_sha256 = id.fingerprint.body_sha256.clone();
    (request, id, journal, r)
}

#[tokio::test]
async fn api_original_journal_controls_settlement_unknown_and_known_not_sent_without_resend() {
    use cerulean_adapters::api_generation::{fixture_reply, ApiClient, ApiFailure};
    use cerulean_domain::secret::SecretInput;
    use serde_json::json;
    for outcome in [
        "reply",
        "cache-unknown",
        "cache-write-unknown",
        "transport",
        "not-sent",
        "inflight",
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let p = policy();
        let path = tmp.path().join("budget");
        let mut book = BudgetLedger::open(&path, &p).unwrap();
        let (request, id, mut journal, r) = api_fixture(tmp.path(), &p);
        assert!(book.observe_api(&journal).is_err());
        book.reserve_api(&journal, r.clone()).unwrap();
        let mut sends = 0;
        if outcome == "not-sent" {
            // Actual production transport entry, cancelled before any HTTP.
            let client =
                ApiClient::new(SecretInput::new("sk-synthetic-budget-fixture".into()).unwrap())
                    .unwrap();
            let (tx, mut rx) = tokio::sync::oneshot::channel();
            tx.send(()).unwrap();
            assert_eq!(
                journal
                    .execute_once(&client, request, &mut rx)
                    .await
                    .unwrap()
                    .unwrap_err(),
                ApiFailure::NotSent {}
            );
        } else if outcome == "inflight" {
            let pending = journal.execute_reply(&id.fingerprint, || async {
                sends += 1;
                std::future::pending::<
                    Result<cerulean_adapters::api_generation::ApiReply, ApiFailure>,
                >()
                .await
            });
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(20), pending)
                    .await
                    .is_err()
            );
        } else {
            let mut value = json!({"id":"response-api","model":"fixture-snapshot","status":"completed","output":[{"type":"message","id":"msg-1","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Done."}]}],"usage":{"input_tokens":10,"output_tokens":1,"input_tokens_details":{"cached_tokens":0,"cache_write_tokens":0}}});
            if outcome == "cache-write-unknown" {
                value["usage"]["input_tokens_details"]
                    .as_object_mut()
                    .unwrap()
                    .remove("cache_write_tokens");
            }
            if outcome == "cache-unknown" {
                value["usage"]
                    .as_object_mut()
                    .unwrap()
                    .remove("input_tokens_details");
            }
            let reply = fixture_reply(&request, value).unwrap();
            journal
                .execute_reply(&id.fingerprint, || async {
                    assert!(path.join("00000001.json").is_file());
                    sends += 1;
                    if outcome == "transport" {
                        Err(ApiFailure::Transport {})
                    } else {
                        Ok(reply)
                    }
                })
                .await
                .unwrap()
                .ok();
        }
        let observed = book.observe_api(&journal).unwrap();
        match outcome {
            "reply" => {
                assert!(matches!(
                    observed,
                    Outcome::Settled {
                        charged_usd_nanos: 35_000,
                        ..
                    }
                ));
                assert_eq!(book.state().reserved_usd_nanos, 0);
            }
            "not-sent" => {
                assert!(matches!(observed, Outcome::NotSent { .. }));
                assert_eq!(book.state().reserved_usd_nanos, 0);
            }
            _ => {
                assert!(matches!(observed, Outcome::Unknown { .. }));
                assert_eq!(book.state().reserved_usd_nanos, 350_000);
            }
        }
        let head = book.head().to_owned();
        assert_eq!(book.observe_api(&journal).unwrap(), observed);
        assert_eq!(book.head(), head);
        drop(book);
        drop(journal);
        let mut book = BudgetLedger::open(&path, &p).unwrap();
        let mut journal = ApiCallJournal::open(&tmp.path().join("api-call"), &id).unwrap();
        assert_eq!(book.observe_api(&journal).unwrap(), observed);
        assert_eq!(book.head(), head);
        assert!(book.reserve_api(&journal, r).is_err());
        assert!(journal
            .execute_reply(&id.fingerprint, || async {
                sends += 1;
                Err(ApiFailure::Transport {})
            })
            .await
            .is_err());
        assert_eq!(sends, if outcome == "not-sent" { 0 } else { 1 });
    }
}

#[tokio::test]
async fn api_reservation_binds_approved_model_request_and_task_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let p = policy();
    let mut book = BudgetLedger::open(&tmp.path().join("budget"), &p).unwrap();
    let (_request, id, journal, r) = api_fixture(tmp.path(), &p);
    for mode in ["role", "body", "output", "attempt", "quote"] {
        let mut wrong = r.clone();
        match mode {
            "role" => wrong.role = ChargeRole::Chief,
            "body" => wrong.request_sha256 = "0".repeat(64),
            "output" => wrong.ceiling.output = 1,
            "attempt" => wrong.operation_id = "other".into(),
            _ => wrong.quote_digest = "0".repeat(64),
        }
        let head = book.head().to_owned();
        assert!(book.reserve_api(&journal, wrong).is_err());
        assert_eq!(book.head(), head);
    }
    book.reserve_api(&journal, r).unwrap();
    assert_eq!(
        book.state().bookings[&id.request_id].api_identity.as_ref(),
        Some(&id)
    );
    let mut different = id.clone();
    different.task_id = "other".into();
    let mut other = ApiCallJournal::open(&tmp.path().join("other-call"), &different).unwrap();
    other
        .execute_reply(&different.fingerprint, || async {
            Err(cerulean_adapters::api_generation::ApiFailure::Transport {})
        })
        .await
        .unwrap()
        .unwrap_err();
    let head = book.head().to_owned();
    assert!(book.observe_api(&other).is_err());
    assert_eq!(head, book.head());
}
