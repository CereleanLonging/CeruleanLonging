use super::*;
use cerulean_domain::currency_budget::scoped::{
    BillingBasis, ContextMetric, InputRange, PriceBand, PricingRequest, RequestedService,
    ScopedPriceQuote,
};

fn base(root: &Path, scoped: bool) {
    drop(crate::Store::open(root).unwrap());
    crate::currency_guard::tests::promote_fixture(root);
    crate::currency_execution_guard::tests::promote_fixture(root);
    crate::currency_count_guard::tests::promote_fixture(root);
    crate::currency_count_resolution_guard::tests::promote_fixture(root);
    if scoped {
        crate::currency_scoped_guard::tests::promote_fixture(root);
    }
}
fn policy() -> BudgetPolicy {
    let rates = Rates {
        input: 1000,
        output: 2000,
        cache_read: Some(100),
        cache_write: Some(1500),
    };
    BudgetPolicy {
        schema: 2,
        planning: None,
        project_id: "project".into(),
        revision_digest: "a".repeat(64),
        limit_usd_nanos: 100_000,
        quotes: vec![],
        scoped_quotes: Some(vec![ScopedPriceQuote {
            schema: 1,
            count_price: None,
            target: ApiTarget::OpenAi {},
            requested_model: "fixture".into(),
            requested_service: RequestedService::Explicit {
                tier: "default".into(),
            },
            reported_models: vec!["fixture-actual".into()],
            reported_services: vec!["default".into(), "priority".into()],
            billing_basis: BillingBasis::PerRequest,
            context_metric: ContextMetric::TotalInputIncludingCache,
            bands: vec![
                PriceBand {
                    actual_model: "fixture-actual".into(),
                    actual_service: "default".into(),
                    input: InputRange {
                        from: 0,
                        through: 10,
                    },
                    rates: rates.clone(),
                },
                PriceBand {
                    actual_model: "fixture-actual".into(),
                    actual_service: "default".into(),
                    input: InputRange {
                        from: 11,
                        through: 100,
                    },
                    rates: Rates {
                        input: 2000,
                        output: 3000,
                        cache_read: Some(200),
                        cache_write: Some(3000),
                    },
                },
                PriceBand {
                    actual_model: "fixture-actual".into(),
                    actual_service: "priority".into(),
                    input: InputRange {
                        from: 0,
                        through: 100,
                    },
                    rates: Rates {
                        input: 4000,
                        output: 6000,
                        cache_read: Some(400),
                        cache_write: Some(6000),
                    },
                },
            ],
            valid_from: (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339(),
            valid_until: (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
            source: "synthetic scoped accounting fixture".into(),
        }]),
    }
}
fn legacy(p: &BudgetPolicy, revision: char) -> BudgetPolicy {
    let q = &p.scoped_quotes.as_ref().unwrap()[0];
    BudgetPolicy {
        schema: 1,
        scoped_quotes: None,
        planning: None,
        project_id: p.project_id.clone(),
        revision_digest: revision.to_string().repeat(64),
        limit_usd_nanos: p.limit_usd_nanos,
        quotes: vec![PriceQuote {
            schema: 1,
            count_price: None,
            target: q.target.clone(),
            requested_model: q.requested_model.clone(),
            reported_models: q.reported_models.clone(),
            rates: q.bands[0].rates.clone(),
            valid_from: q.valid_from.clone(),
            valid_until: q.valid_until.clone(),
            source: q.source.clone(),
        }],
    }
}
fn reservation(p: &BudgetPolicy, id: &str) -> Reservation {
    let pricing = (p.schema == 2).then(|| PricingRequest {
        target: ApiTarget::OpenAi {},
        requested_model: "fixture".into(),
        requested_service: RequestedService::Explicit {
            tier: "default".into(),
        },
        input: InputRange {
            from: 0,
            through: 100,
        },
        output_ceiling: 20,
        cache_read_allowed: true,
        cache_write_allowed: true,
    });
    Reservation {
        pricing,
        request_id: id.into(),
        operation_id: id.into(),
        role: ChargeRole::Worker,
        quote_digest: if p.schema == 2 {
            digest(&p.scoped_quotes.as_ref().unwrap()[0]).unwrap()
        } else {
            digest(&p.quotes[0]).unwrap()
        },
        request_sha256: "b".repeat(64),
        ceiling: TokenCeiling {
            total_input: 100,
            output: 20,
            cache_read_allowed: true,
            cache_write_allowed: true,
        },
        ceiling_evidence_sha256: "c".repeat(64),
    }
}
fn settlement(r: &Reservation, response: &str) -> Settlement {
    Settlement {
        actual_service: r.pricing.as_ref().map(|_| "default".into()),
        request_id: r.request_id.clone(),
        response_id: response.into(),
        request_sha256: r.request_sha256.clone(),
        actual_model: "fixture-actual".into(),
        usage: Usage {
            input: 3,
            output: 20,
            cache_read: 3,
            cache_write: 4,
        },
        evidence_sha256: "d".repeat(64),
    }
}
fn authorize(book: &mut BudgetLedger, next: BudgetPolicy) {
    book.authorize(PolicyAuthorization {
        previous_policy_digest: digest(book.policy()).unwrap(),
        previous_ledger_head: book.head().into(),
        policy: next,
        approval_evidence_sha256: "e".repeat(64),
    })
    .unwrap();
}

#[test]
fn scoped_ledger_requires_reader8_before_directory_or_approval_write() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    base(&data, false);
    let p = policy();
    let path = data.join("currency-budgets/project");
    let before = crate::reader_generation::tests::snapshot(&data);
    assert!(BudgetLedger::open(&path, &p).is_err());
    assert_eq!(before, crate::reader_generation::tests::snapshot(&data));
    let old = legacy(&p, 'f');
    let mut book = BudgetLedger::open(&path, &old).unwrap();
    let head = book.head().to_owned();
    let auth = PolicyAuthorization {
        previous_policy_digest: digest(&old).unwrap(),
        previous_ledger_head: head.clone(),
        policy: p.clone(),
        approval_evidence_sha256: "e".repeat(64),
    };
    assert!(book.check_authorization(auth.clone()).is_err());
    assert!(book.authorize(auth).is_err());
    assert_eq!(head, book.head());
    drop(book);
    crate::currency_scoped_guard::tests::promote_fixture(&data);
    let mut book = BudgetLedger::open(&path, &old).unwrap();
    authorize(&mut book, p.clone());
    drop(book);
    assert_eq!(BudgetLedger::open(&path, &p).unwrap().policy(), &p);
}

#[test]
fn scoped_ledger_reserves_all_bands_and_records_actual_service_across_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    base(&data, true);
    let p = policy();
    let path = data.join("currency-budgets/project");
    let mut book = BudgetLedger::open(&path, &p).unwrap();
    let r = reservation(&p, "one");
    assert_eq!(book.reserve(r.clone()).unwrap(), 720);
    let s = settlement(&r, "resp_one");
    assert!(matches!(
        book.settle(s.clone()).unwrap(),
        Outcome::Settled {
            charged_usd_nanos: 50,
            ..
        }
    ));
    let state = book.state().clone();
    let head = book.head().to_owned();
    drop(book);
    let mut reopened = BudgetLedger::open(&path, &p).unwrap();
    assert_eq!(reopened.state(), &state);
    assert_eq!(reopened.head(), head);
    assert!(reopened.settle(s).is_err());
    assert_eq!(reopened.head(), head);
    assert!(reopened.reserve(r).is_err());
    assert_eq!(reopened.state().charged_usd_nanos, 50);
}

#[test]
fn scoped_ledger_original_policy_prices_late_response_after_revision() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    base(&data, true);
    let p = policy();
    let path = data.join("currency-budgets/project");
    let mut book = BudgetLedger::open(&path, &p).unwrap();
    let r = reservation(&p, "late");
    book.reserve(r.clone()).unwrap();
    book.unknown("late", &"e".repeat(64)).unwrap();
    let mut next = p.clone();
    next.revision_digest = "f".repeat(64);
    for b in &mut next.scoped_quotes.as_mut().unwrap()[0].bands {
        b.rates.input *= 5;
        b.rates.output *= 5;
    }
    authorize(&mut book, next.clone());
    let s = settlement(&r, "resp_late");
    assert!(matches!(
        book.settle(s).unwrap(),
        Outcome::Settled {
            charged_usd_nanos: 50,
            ..
        }
    ));
    drop(book);
    let reopened = BudgetLedger::open(&path, &next).unwrap();
    assert_eq!(reopened.state().charged_usd_nanos, 50);
    assert_eq!(reopened.state().reserved_usd_nanos, 0);
}

#[test]
fn scoped_ledger_response_deduplication_covers_both_legacy_directions() {
    for scoped_first in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        base(&data, true);
        let modern = policy();
        let old = legacy(&modern, 'f');
        let (first, second) = if scoped_first {
            (modern, old)
        } else {
            (old, modern)
        };
        let path = data.join("currency-budgets/project");
        let mut book = BudgetLedger::open(&path, &first).unwrap();
        let r1 = reservation(&first, "first");
        book.reserve(r1.clone()).unwrap();
        book.settle(settlement(&r1, "resp_shared")).unwrap();
        authorize(&mut book, second.clone());
        let r2 = reservation(&second, "second");
        book.reserve(r2.clone()).unwrap();
        let before = book.state().clone();
        let head = book.head().to_owned();
        assert!(book.settle(settlement(&r2, "resp_shared")).is_err());
        assert_eq!(book.state(), &before);
        assert_eq!(book.head(), head);
        drop(book);
        let reopened = BudgetLedger::open(&path, &second).unwrap();
        assert_eq!(reopened.state(), &before);
    }
}

#[test]
fn scoped_ledger_unknown_tier_and_usage_keep_hold_and_block_new_spending() {
    for mode in 0..4 {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        base(&data, true);
        let p = policy();
        let path = data.join("currency-budgets/project");
        let mut book = BudgetLedger::open(&path, &p).unwrap();
        let r = reservation(&p, "one");
        book.reserve(r.clone()).unwrap();
        let mut s = settlement(&r, "resp_unknown");
        match mode {
            0 => s.actual_service = None,
            1 => s.actual_service = Some("unquoted".into()),
            2 => s.usage.input = 101,
            _ => s.actual_model = "unquoted-model".into(),
        }
        assert!(matches!(
            book.settle(s.clone()).unwrap(),
            Outcome::Unpriced { .. }
        ));
        assert_eq!(book.state().reserved_usd_nanos, 720);
        assert_eq!(book.state().charged_usd_nanos, 0);
        assert!(book.state().blocked);
        drop(book);
        let mut reopened = BudgetLedger::open(&path, &p).unwrap();
        assert!(reopened.reserve(reservation(&p, "next")).is_err());
        assert!(
            matches!(&reopened.state().bookings["one"].outcome,Outcome::Unpriced{settlement,..} if settlement==&s)
        );
    }
}

#[test]
fn scoped_ledger_changed_request_expired_terms_or_exhaustion_publish_nothing() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    base(&data, true);
    let p = policy();
    let path = data.join("currency-budgets/project");
    let mut book = BudgetLedger::open(&path, &p).unwrap();
    let before = book.head().to_owned();
    for mode in 0..6 {
        let mut r = reservation(&p, "invalid");
        match mode {
            0 => r.pricing = None,
            1 => r.pricing.as_mut().unwrap().requested_service = RequestedService::Omitted,
            2 => r.pricing.as_mut().unwrap().input.through = 101,
            3 => r.pricing.as_mut().unwrap().requested_model = "other".into(),
            4 => r.quote_digest = "f".repeat(64),
            _ => r.ceiling.cache_write_allowed = false,
        };
        assert!(book.reserve(r).is_err());
        assert_eq!(book.head(), before);
    }
    let mut expired = p.clone();
    expired.revision_digest = "e".repeat(64);
    expired.scoped_quotes.as_mut().unwrap()[0].valid_until =
        (chrono::Utc::now() - chrono::Duration::minutes(1)).to_rfc3339();
    authorize(&mut book, expired.clone());
    let before = book.head().to_owned();
    assert!(book.reserve(reservation(&expired, "expired")).is_err());
    assert_eq!(book.head(), before);
    let mut small = p.clone();
    small.revision_digest = "f".repeat(64);
    small.limit_usd_nanos = 719;
    authorize(&mut book, small.clone());
    let before = book.head().to_owned();
    assert!(book.reserve(reservation(&small, "too-much")).is_err());
    assert_eq!(book.head(), before);
}

#[test]
fn scoped_ledger_pending_publication_and_removed_guard_preserve_liability() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    base(&data, true);
    let p = policy();
    let path = data.join("currency-budgets/project");
    let mut book = BudgetLedger::open(&path, &p).unwrap();
    let r = reservation(&p, "one");
    book.reserve(r).unwrap();
    let old = legacy(&p, 'f');
    authorize(&mut book, old);
    let head = book.head().to_owned();
    let marker = data.join(crate::currency_scoped_guard::MARKER);
    let marker_bytes = fs::read(&marker).unwrap();
    fs::remove_file(&marker).unwrap();
    assert!(book.unknown("one", &"e".repeat(64)).is_err());
    assert_eq!(head, book.head());
    fs::write(&marker, marker_bytes).unwrap();
    let pending = path.join(format!("{:08}.pending", book.count));
    fs::write(&pending, b"owned-collision").unwrap();
    assert!(book.unknown("one", &"e".repeat(64)).is_err());
    assert!(!book.healthy());
    assert_eq!(fs::read(&pending).unwrap(), b"owned-collision");
    drop(book);
    assert!(BudgetLedger::open_existing(&path, "project").is_err());
    assert_eq!(fs::read(pending).unwrap(), b"owned-collision");
}

#[test]
fn scoped_ledger_legacy_service_evidence_still_requires_reader8_for_later_writes() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    base(&data, true);
    let p = legacy(&policy(), 'f');
    let path = data.join("currency-budgets/project");
    let mut book = BudgetLedger::open(&path, &p).unwrap();
    for id in ["service-evidence", "unknown-later", "not-sent-later"] {
        book.reserve(reservation(&p, id)).unwrap();
    }
    let mut response = settlement(&reservation(&p, "service-evidence"), "resp_legacy_service");
    response.actual_service = Some("default".into());
    assert!(matches!(
        book.settle(response.clone()).unwrap(),
        Outcome::Unpriced { .. }
    ));
    assert!(book
        .state()
        .policies
        .values()
        .all(|p| p.schema == 1 && p.scoped_quotes.is_none()));
    assert!(book
        .state()
        .bookings
        .values()
        .all(|b| b.reservation.pricing.is_none()));
    let state = book.state().clone();
    let head = book.head().to_owned();
    let marker = data.join(crate::currency_scoped_guard::MARKER);
    let marker_bytes = fs::read(&marker).unwrap();
    fs::remove_file(&marker).unwrap();
    let original_files = crate::reader_generation::tests::snapshot(&data);

    assert!(book
        .unknown("unknown-later", &"e".repeat(64))
        .unwrap_err()
        .contains("reader8"));
    assert!(book
        .not_sent("not-sent-later", &"e".repeat(64))
        .unwrap_err()
        .contains("reader8"));
    assert_eq!(book.head(), head);
    assert_eq!(book.state(), &state);
    assert!(book.healthy());
    assert_eq!(
        crate::reader_generation::tests::snapshot(&data),
        original_files
    );

    // Restoring the exact owned marker proves the failures were caused by the
    // guard, not by the existing Unpriced block or either request's state.
    fs::write(&marker, marker_bytes).unwrap();
    book.unknown("unknown-later", &"e".repeat(64)).unwrap();
    book.not_sent("not-sent-later", &"e".repeat(64)).unwrap();
    assert!(matches!(&book.state().bookings["service-evidence"].outcome,
        Outcome::Unpriced { settlement, .. } if settlement == &response));
    let final_state = book.state().clone();
    drop(book);
    assert_eq!(BudgetLedger::open(&path, &p).unwrap().state(), &final_state);
}

#[test]
fn scoped_ledger_authorization_preflights_record_size_and_remaining_capacity() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    base(&data, true);
    let p = legacy(&policy(), 'f');
    let path = data.join("currency-budgets/project");
    let mut book = BudgetLedger::open(&path, &p).unwrap();
    let mut oversized = policy();
    let mut quote = oversized.scoped_quotes.as_ref().unwrap()[0].clone();
    let rates = quote.bands[0].rates.clone();
    quote.bands.clear();
    for actual_service in ["default", "priority"] {
        for index in 0..256 {
            quote.bands.push(PriceBand {
                actual_model: "fixture-actual".into(),
                actual_service: actual_service.into(),
                input: InputRange {
                    from: index * 2,
                    through: index * 2 + 1,
                },
                rates: rates.clone(),
            });
        }
    }
    let mut second = quote.clone();
    second.requested_model = "fixture-second".into();
    oversized.scoped_quotes = Some(vec![quote, second]);
    oversized.validate().unwrap();
    assert!(serde_json::to_vec(&oversized).unwrap().len() > MAX_RECORD);
    let authorization = |next: BudgetPolicy| PolicyAuthorization {
        previous_policy_digest: digest(&p).unwrap(),
        previous_ledger_head: book.head().into(),
        policy: next,
        approval_evidence_sha256: "e".repeat(64),
    };
    let too_large = authorization(oversized);
    let ordinary = authorization(policy());
    let head = book.head().to_owned();
    let state = book.state().clone();
    let original_files = crate::reader_generation::tests::snapshot(&data);
    assert!(book
        .check_authorization(too_large.clone())
        .unwrap_err()
        .contains("currency record too large"));
    assert!(book
        .authorize(too_large)
        .unwrap_err()
        .contains("currency record too large"));
    assert_eq!(book.head(), head);
    assert_eq!(book.state(), &state);
    assert!(book.healthy());
    assert_eq!(
        crate::reader_generation::tests::snapshot(&data),
        original_files
    );

    // Controlled boundary injection avoids writing 8,192 unrelated records.
    // Both preflights are read-only; restore the true sequence before reopen.
    let count = book.count;
    book.count = MAX_RECORDS - 1;
    book.check_authorization(ordinary.clone()).unwrap();
    book.count = MAX_RECORDS;
    assert!(book
        .check_authorization(ordinary)
        .unwrap_err()
        .contains("cannot hold a new approval"));
    assert_eq!(book.head(), head);
    assert_eq!(book.state(), &state);
    assert!(book.healthy());
    assert_eq!(
        crate::reader_generation::tests::snapshot(&data),
        original_files
    );
    book.count = count;
    drop(book);
    assert_eq!(BudgetLedger::open(&path, &p).unwrap().state(), &state);
}
