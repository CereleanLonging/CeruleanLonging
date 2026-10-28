use super::*;
use cerulean_domain::historical_money::HistoricalAccountingState;
fn command(host: &mut RuntimeHost, command: Command) -> Response {
    host.handle(Request {
        id: "history-product".into(),
        protocol_version: PROTOCOL_VERSION,
        command,
    })
}

#[test]
fn currency_historical_product_review_is_readonly_and_confirm_uses_same_user_service() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = fixture(temp.path(), false);
    let scope = closed(&mut host, "product-old");
    let input = review(&host, &scope).input();
    let before = snapshot(&data);
    let response = command(
        &mut host,
        Command::ReviewHistoricalCharge {
            input: input.clone(),
        },
    );
    assert!(response.error.is_none(), "{:?}", response.error);
    let snapshot_view = response.result.unwrap();
    let reviewed = snapshot_view.historical_charge_review.unwrap();
    assert_eq!(reviewed.input, input);
    assert_eq!(snapshot_view.currency_budgets[0].available_usd_nanos, None);
    assert!(snapshot(&data) == before);
    assert!(host.snapshot().historical_charge_review.is_none());
    let response = command(
        &mut host,
        Command::ConfirmHistoricalCharge {
            input: input.clone(),
            review_digest: reviewed.review_digest.clone(),
        },
    );
    assert!(response.error.is_none(), "{:?}", response.error);
    let view = response.result.unwrap();
    assert!(view.historical_charge_review.is_none());
    assert_eq!(view.currency_history.unwrap().len(), 1);
    let records = view.currency_accounted.unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].state, HistoricalAccountingState::Accounted);
    assert_eq!(records[0].review, reviewed);
    assert!(view.currency_budgets[0].available_usd_nanos.is_some());
    assert_eq!(view.currency_budgets[0].charged_usd_nanos, 17);
    let after = snapshot(&data);
    assert!(command(
        &mut host,
        Command::ConfirmHistoricalCharge {
            input,
            review_digest: reviewed.review_digest
        }
    )
    .error
    .is_none());
    assert!(snapshot(&data) == after);
    drop(host);
    let host = RuntimeHost::open(&data).unwrap();
    assert_eq!(host.snapshot().currency_accounted, Some(records));
    assert!(snapshot(&data) == after);
}

#[test]
fn currency_historical_product_rejects_changed_amount_source_and_review_without_writes() {
    for mode in ["amount", "digest", "source", "account"] {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let mut host = fixture(temp.path(), false);
        let scope = closed(&mut host, "product-stale");
        let mut input = review(&host, &scope).input();
        let mut view = command(
            &mut host,
            Command::ReviewHistoricalCharge {
                input: input.clone(),
            },
        )
        .result
        .unwrap()
        .historical_charge_review
        .unwrap();
        match mode {
            "amount" => input.amount_usd_nanos += 1,
            "digest" => view.review_digest = "a".repeat(64),
            "account" => input.project_id = "missing".into(),
            _ => {
                let p = data.join("management-decisions/product-stale.0.json");
                let mut b = fs::read(&p).unwrap();
                b.push(b' ');
                fs::write(p, b).unwrap();
            }
        }
        let before = snapshot(&data);
        let response = command(
            &mut host,
            Command::ConfirmHistoricalCharge {
                input,
                review_digest: view.review_digest,
            },
        );
        assert!(response.error.is_some(), "{mode}");
        assert!(snapshot(&data) == before, "{mode}");
        assert_eq!(
            host.currency_accounts["project"].state().charged_usd_nanos,
            0
        );
    }
}

#[test]
fn currency_historical_product_pending_confirmation_does_not_resolve_other_history() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = fixture(temp.path(), false);
    let first = closed(&mut host, "product-first");
    closed(&mut host, "product-second");
    let r = review(&host, &first);
    commit(&mut host, &r, "user", context(&r)).unwrap();
    let before = snapshot(&data);
    let view = host.snapshot();
    assert_eq!(
        view.currency_accounted.unwrap()[0].state,
        HistoricalAccountingState::ConfirmedPending
    );
    assert!(view.currency_budgets[0].available_usd_nanos.is_none());
    assert!(snapshot(&data) == before);
    host.reconcile_currency_account("project").unwrap();
    let view = host.snapshot();
    assert_eq!(
        view.currency_accounted.unwrap()[0].state,
        HistoricalAccountingState::Accounted
    );
    assert_eq!(view.currency_history.unwrap().len(), 2);
    assert!(view.currency_budgets[0].available_usd_nanos.is_none());
}

#[test]
fn currency_historical_product_legacy_snapshot_missing_fields_stays_unavailable() {
    let temp = tempfile::tempdir().unwrap();
    let host = fixture(temp.path(), false);
    let mut value = serde_json::to_value(host.snapshot()).unwrap();
    value.as_object_mut().unwrap().remove("currencyAccounted");
    value
        .as_object_mut()
        .unwrap()
        .remove("historicalChargeReview");
    let old: Snapshot = serde_json::from_value(value).unwrap();
    assert!(old.currency_accounted.is_none());
    assert!(old.historical_charge_review.is_none());
}
