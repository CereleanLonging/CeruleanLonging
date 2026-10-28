use super::*;

fn linked(root: &Path, p: &BudgetPolicy) -> BudgetLedger {
    let mut book = BudgetLedger::open(root, p).unwrap();
    book.bind_initial_approval(ApprovalLink {
        previous_policy_digest: digest(p).unwrap(),
        previous_ledger_head: book.head().into(),
        approval_evidence_sha256: "1".repeat(64),
    })
    .unwrap();
    book
}
fn charge(book: &BudgetLedger, id: &str, amount: u64) -> HistoricalCharge {
    HistoricalCharge {
        project_id: book.policy().project_id.clone(),
        scope: HistoricalScope::Management {
            operation_id: id.into(),
            original_project_id: Some("project".into()),
        },
        source_digest: "2".repeat(64),
        policy_digest: digest(book.policy()).unwrap(),
        previous_ledger_head: book.head().into(),
        amount_usd_nanos: amount,
        billing_reference: "Synthetic invoice reference, not an actual supplier statement".into(),
        explanation: "Controlled user-event fixture; no external authentication proven".into(),
        user_event_digest: "3".repeat(64),
    }
}
fn files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(root)
        .unwrap()
        .map(|e| {
            let p = e.unwrap().path();
            (
                p.file_name().unwrap().to_string_lossy().into(),
                if p.file_name().unwrap() == "writer.lock" {
                    assert_eq!(fs::metadata(&p).unwrap().len(), 0);
                    vec![]
                } else {
                    fs::read(p).unwrap()
                },
            )
        })
        .collect()
}

#[test]
fn historical_charge_preserves_unknown_reservations_and_replays_once() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("book");
    let p = policy();
    let mut book = linked(&root, &p);
    book.reserve(reservation(&p, "original-unknown")).unwrap();
    book.unknown("original-unknown", &"a".repeat(64)).unwrap();
    let held = book.state.reserved_usd_nanos;
    let c = charge(&book, "old-request", 80_000);
    let before = files(&root);
    book.check_historical_charge(&c).unwrap();
    assert_eq!(files(&root), before);
    book.record_historical_charge(c.clone()).unwrap();
    assert_eq!(book.state.charged_usd_nanos, 80_000);
    assert_eq!(book.state.reserved_usd_nanos, held);
    assert_eq!(
        book.state.available_usd_nanos(&p).unwrap(),
        p.limit_usd_nanos - held - 80_000
    );
    let before = files(&root);
    book.record_historical_charge(c.clone()).unwrap();
    assert_eq!(files(&root), before);
    drop(book);
    let mut book = BudgetLedger::open(&root, &p).unwrap();
    assert_eq!(
        book.state.historical_charges.get(&c.scope.key().unwrap()),
        Some(&c)
    );
    book.record_historical_charge(c).unwrap();
    assert_eq!(files(&root), before);
}

#[test]
fn historical_charge_altered_confirmation_or_stale_review_never_rewrites() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("book");
    let mut book = linked(&root, &policy());
    let c = charge(&book, "old-request", 1);
    book.record_historical_charge(c.clone()).unwrap();
    let before = files(&root);
    for mode in ["amount", "source", "approval", "reference", "head"] {
        let mut changed = c.clone();
        match mode {
            "amount" => changed.amount_usd_nanos += 1,
            "source" => changed.source_digest = "4".repeat(64),
            "approval" => changed.user_event_digest = "5".repeat(64),
            "reference" => changed.billing_reference.push_str(" changed"),
            _ => changed.previous_ledger_head = book.head().into(),
        }
        assert!(book.record_historical_charge(changed).is_err(), "{mode}");
        assert_eq!(files(&root), before);
    }
    let mut stale = charge(&book, "different-request", 1);
    stale.previous_ledger_head = c.previous_ledger_head;
    assert!(book.record_historical_charge(stale).is_err());
    assert_eq!(files(&root), before);
}

#[test]
fn historical_charge_known_project_and_logical_scope_cannot_be_reassigned() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("book");
    let mut book = linked(&root, &policy());
    for scope in [
        HistoricalScope::Management {
            operation_id: "old".into(),
            original_project_id: Some("another".into()),
        },
        HistoricalScope::Api {
            project_id: "another".into(),
            attempt_id: "old".into(),
        },
    ] {
        let mut c = charge(&book, "old", 1);
        c.scope = scope;
        assert!(book.record_historical_charge(c).is_err());
    }
    let mut c = charge(&book, "old", 1);
    c.scope = HistoricalScope::Management {
        operation_id: "old".into(),
        original_project_id: None,
    };
    let key = c.scope.key().unwrap();
    book.record_historical_charge(c.clone()).unwrap();
    c.scope = HistoricalScope::Management {
        operation_id: "old".into(),
        original_project_id: Some("project".into()),
    };
    assert_eq!(c.scope.key().unwrap(), key);
    assert!(book.record_historical_charge(c).is_err());
    assert_eq!(book.state.historical_charges.len(), 1);
}

#[test]
fn historical_charge_cannot_double_count_existing_or_future_reservation() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("book");
    let p = policy();
    let mut book = linked(&root, &p);
    let mut r = reservation(&p, "existing");
    r.operation_id = "same-operation".into();
    book.reserve(r).unwrap();
    let c = charge(&book, "same-operation", 1);
    assert!(book.record_historical_charge(c).is_err());
    let c = charge(&book, "historical-operation", 1);
    book.record_historical_charge(c).unwrap();
    let mut r = reservation(&p, "future");
    r.operation_id = "historical-operation".into();
    assert!(book.reserve(r).unwrap_err().contains("already accounted"));
    assert_eq!(book.state.bookings.len(), 1);
    assert_eq!(book.state.charged_usd_nanos, 1);
}

#[test]
fn historical_charge_over_limit_remains_until_new_explicit_capacity_covers_it() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("book");
    let p = policy();
    let mut book = linked(&root, &p);
    let c = charge(&book, "over-limit", 700_000);
    book.record_historical_charge(c.clone()).unwrap();
    assert!(book.state.available_usd_nanos(&p).is_err());
    assert!(book.reserve(reservation(&p, "new")).is_err());
    let mut next = p.clone();
    next.revision_digest = "b".repeat(64);
    next.limit_usd_nanos = 800_000;
    book.authorize(PolicyAuthorization {
        previous_policy_digest: digest(&p).unwrap(),
        previous_ledger_head: book.head().into(),
        policy: next.clone(),
        approval_evidence_sha256: "6".repeat(64),
    })
    .unwrap();
    assert_eq!(book.state.available_usd_nanos(&next).unwrap(), 100_000);
    assert_eq!(book.state.historical_charges[&c.scope.key().unwrap()], c);
    drop(book);
    let book = BudgetLedger::open(&root, &next).unwrap();
    assert_eq!(book.state.charged_usd_nanos, 700_000);
}

#[test]
fn historical_charge_exact_integer_limits_and_explicit_zero_evidence_are_required() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("book");
    let mut p = policy();
    p.limit_usd_nanos = MAX_USD_NANOS;
    let mut book = linked(&root, &p);
    let mut zero = charge(&book, "zero", 0);
    zero.billing_reference.clear();
    assert!(book.record_historical_charge(zero).is_err());
    let zero = charge(&book, "zero", 0);
    book.record_historical_charge(zero).unwrap();
    assert_eq!(book.state.historical_charges.len(), 1);
    let too_big = charge(&book, "too-big", MAX_USD_NANOS + 1);
    assert!(book.record_historical_charge(too_big).is_err());
    book.reserve(reservation(&p, "held")).unwrap();
    let held_overflow = charge(&book, "held-overflow", MAX_USD_NANOS);
    assert!(book
        .record_historical_charge(held_overflow)
        .unwrap_err()
        .contains("outstanding reservations"));
    book.not_sent("held", &"f".repeat(64)).unwrap();
    let max = charge(&book, "max", MAX_USD_NANOS);
    book.record_historical_charge(max).unwrap();
    let before = files(&root);
    let more = charge(&book, "overflow", 1);
    assert!(book.record_historical_charge(more).is_err());
    assert_eq!(files(&root), before);
}

#[test]
fn historical_charge_requires_linked_approval_and_retains_failed_publication() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("book");
    let p = policy();
    let mut book = BudgetLedger::open(&root, &p).unwrap();
    let c = charge(&book, "old", 1);
    assert!(book.record_historical_charge(c).is_err());
    drop(book);
    let mut book = linked(&root, &p);
    let c = charge(&book, "old", 1);
    let pending = root.join(format!("{:08}.pending", book.count));
    fs::write(&pending, b"retained original failure").unwrap();
    let before = files(&root);
    assert!(book.record_historical_charge(c).is_err());
    assert!(!book.healthy());
    assert_eq!(book.state.charged_usd_nanos, 0);
    assert_eq!(files(&root), before);
    drop(book);
    assert!(BudgetLedger::open(&root, &p).is_err());
    assert_eq!(files(&root), before);
}
