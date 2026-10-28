use super::*;

#[test]
fn observation_batch_conflicting_management_replies_preserve_all_reservations() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = fixture(temp.path(), false);
    for id in ["batch-a", "batch-b"] {
        let i = intent(&host, id, Purpose::Plan);
        begin(&mut host, &i);
        let mut reply = result(&i);
        reply.observation.response_id = "resp_same_original".into();
        host.management.finish_result(id, Ok(reply)).unwrap();
        let head = host.management.response_head(id).unwrap().to_owned();
        host.management
            .resolve(
                id,
                &head,
                UserResolution::Discard {
                    reason: "Close local decision; retain billable response".into(),
                },
            )
            .unwrap();
    }
    let state = host.currency_accounts["project"].state().clone();
    let before = snapshot(&data.join("currency-budgets"));
    let error = host.reconcile_currency_account("project").unwrap_err();
    assert!(
        error.contains("billing response already attached"),
        "{error}"
    );
    assert_eq!(host.currency_accounts["project"].state(), &state);
    assert!(
        snapshot(&data.join("currency-budgets")) == before,
        "partial charge persisted"
    );
    drop(host);
    assert!(RuntimeHost::open(&data).is_err());
    assert!(
        snapshot(&data.join("currency-budgets")) == before,
        "reopen persisted a partial charge"
    );
}

#[test]
fn observation_batch_readonly_preflight_and_idempotent_mixed_outcomes() {
    for mode in ["settled", "unknown", "unpriced"] {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let mut host = fixture(temp.path(), false);
        let ids = vec!["batch-a".to_owned(), "batch-b".to_owned()];
        for (index, id) in ids.iter().enumerate() {
            let i = intent(&host, id, Purpose::Plan);
            begin(&mut host, &i);
            let mut reply = result(&i);
            if index == 1 && mode == "unknown" {
                reply.transport.as_mut().unwrap().usage.cache_write_tokens = None;
            }
            if index == 1 && mode == "unpriced" {
                reply.observation.actual_model = "unquoted-model".into();
            }
            host.management.finish_result(id, Ok(reply)).unwrap();
            let head = host.management.response_head(id).unwrap().to_owned();
            host.management
                .resolve(
                    id,
                    &head,
                    UserResolution::Discard {
                        reason: "Close local decision; preserve original usage".into(),
                    },
                )
                .unwrap();
        }
        let before = snapshot(&data.join("currency-budgets"));
        let book = host.currency_accounts.get_mut("project").unwrap();
        let state = book.state().clone();
        book.observe_original_calls(
            &host.management,
            &ids,
            std::iter::empty(),
            std::iter::empty(),
            false,
        )
        .unwrap();
        assert_eq!(book.state(), &state);
        assert!(snapshot(&data.join("currency-budgets")) == before);
        host.reconcile_currency_account("project").unwrap();
        let book = &host.currency_accounts["project"];
        assert!(matches!(
            book.state().bookings["batch-a"].outcome,
            Outcome::Settled { .. }
        ));
        let b = &book.state().bookings["batch-b"].outcome;
        assert!(
            match mode {
                "settled" => matches!(b, Outcome::Settled { .. }),
                "unknown" => matches!(b, Outcome::Unknown { .. }),
                _ => matches!(b, Outcome::Unpriced { .. }),
            },
            "unexpected {mode}: {b:?}"
        );
        let state = book.state().clone();
        let before = snapshot(&data.join("currency-budgets"));
        host.reconcile_currency_account("project").unwrap();
        assert_eq!(host.currency_accounts["project"].state(), &state);
        assert!(snapshot(&data.join("currency-budgets")) == before);
        drop(host);
        let host = RuntimeHost::open(&data).unwrap();
        assert_eq!(host.currency_accounts["project"].state(), &state);
        assert!(snapshot(&data.join("currency-budgets")) == before);
    }
}
