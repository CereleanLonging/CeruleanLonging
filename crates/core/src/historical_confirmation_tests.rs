//! Original production journals with controlled local records; no supplier calls.
use super::*;
use crate::{
    currency_budget::HistoricalScope,
    historical_confirmation::{HistoricalReview, APPROVAL_SOURCE},
};
#[cfg(windows)]
#[path = "historical_ipc_tests.rs"]
mod native_ipc;
#[path = "historical_paging_tests.rs"]
mod paging;
#[path = "historical_product_tests.rs"]
mod product;

fn closed(host: &mut RuntimeHost, id: &str) -> HistoricalScope {
    let i = intent(host, id, Purpose::Plan);
    host.management.begin(i.clone()).unwrap();
    host.management
        .finish(id, Ok(result(&i).observation))
        .unwrap();
    let head = host.management.evidence_digest(id).unwrap().to_owned();
    host.management
        .resolve(
            id,
            &head,
            UserResolution::Discard {
                reason: "Close original local decision; fee remains uncertain".into(),
            },
        )
        .unwrap();
    HistoricalScope::Management {
        operation_id: id.into(),
        original_project_id: Some("project".into()),
    }
}
fn review(host: &RuntimeHost, scope: &HistoricalScope) -> HistoricalReview {
    host.review_historical_charge(
        "project",
        scope,
        17,
        "controlled statement reference",
        "Explicit local test amount, not a supplier invoice",
    )
    .unwrap()
}
fn commit(
    host: &mut RuntimeHost,
    r: &HistoricalReview,
    principal: &str,
    context: CommandContext,
) -> Result<(), String> {
    host.projects
        .get_mut(&r.project_id)
        .unwrap()
        .command(
            &format!("history-fixture-{}", r.digest()?),
            principal,
            WorkflowCommand::ConfirmHistoricalCharge {
                review_digest: r.digest()?,
            },
            context,
            false,
        )
        .map(|_| ())
}
fn context(r: &HistoricalReview) -> CommandContext {
    CommandContext {
        historical_charge: Some(r.clone()),
        approval_source: Some(APPROVAL_SOURCE.into()),
        ..Default::default()
    }
}

#[test]
fn currency_historical_confirmation_exact_once_same_account_and_original_user_event() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = fixture(temp.path(), false);
    let scope = closed(&mut host, "closed-old");
    let before = snapshot(&data);
    let r = review(&host, &scope);
    assert!(snapshot(&data) == before);
    let policy = host.currency_accounts["project"].policy().clone();
    let original = host
        .management
        .evidence_digest("closed-old")
        .unwrap()
        .to_owned();
    host.confirm_historical_charge(&r, &r.digest().unwrap())
        .unwrap();
    let entry = host.projects["project"].history().last().unwrap();
    assert_eq!(entry.body.event.principal, "user");
    assert_eq!(entry.body.context.historical_charge.as_ref(), Some(&r));
    let book = &host.currency_accounts["project"];
    assert_eq!(
        book.state().historical_charges[&scope.key().unwrap()],
        r.charge(&entry.hash)
    );
    assert_eq!(book.state().charged_usd_nanos, 17);
    assert_eq!(book.policy(), &policy);
    assert_eq!(
        host.management.evidence_digest("closed-old").unwrap(),
        original
    );
    // Raw history remains visible; UI projection of accounted records is a separate step.
    assert_eq!(host.snapshot().currency_history.unwrap().len(), 1);
    let after = snapshot(&data);
    host.confirm_historical_charge(&r, &r.digest().unwrap())
        .unwrap();
    assert!(snapshot(&data) == after);
    drop(host);
    let mut host = RuntimeHost::open(&data).unwrap();
    assert!(snapshot(&data) == after);
    host.confirm_historical_charge(&r, &r.digest().unwrap())
        .unwrap();
    assert!(snapshot(&data) == after);
}

#[test]
fn currency_historical_confirmation_only_user_plain_context_and_reader4() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = fixture(temp.path(), false);
    let scope = closed(&mut host, "role-old");
    let r = review(&host, &scope);
    let before = snapshot(&data);
    for principal in ["chief", "deputy", "core", "worker-openai"] {
        assert!(commit(&mut host, &r, principal, context(&r)).is_err());
    }
    for mode in ["missing", "source", "extra", "different-review"] {
        let mut c = context(&r);
        match mode {
            "missing" => c.historical_charge = None,
            "source" => c.approval_source = Some("model assertion".into()),
            "extra" => c.cwd = Some("unexpected".into()),
            _ => c.historical_charge.as_mut().unwrap().amount_usd_nanos += 1,
        }
        assert!(commit(&mut host, &r, "user", c).is_err(), "{mode}");
    }
    assert!(snapshot(&data) == before);
    let marker = data.join(crate::currency_guard::MARKER);
    fs::rename(&marker, temp.path().join("reader-guard-preserved")).unwrap();
    assert!(commit(&mut host, &r, "user", context(&r)).is_err());
    assert!(crate::reader_generation::reject_unprotected(&data).is_err());
}

#[test]
fn currency_historical_confirmation_recovers_published_user_event_without_duplicate_workflow() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = fixture(temp.path(), false);
    let scope = closed(&mut host, "gap-old");
    let r = review(&host, &scope);
    commit(&mut host, &r, "user", context(&r)).unwrap();
    assert_eq!(
        host.currency_accounts["project"].state().charged_usd_nanos,
        0
    );
    let workflow = host.projects["project"].history().to_vec();
    drop(host);
    let host = RuntimeHost::open(&data).unwrap();
    assert_eq!(host.projects["project"].history(), workflow);
    assert_eq!(
        host.currency_accounts["project"].state().charged_usd_nanos,
        17
    );
    let after = snapshot(&data);
    drop(host);
    RuntimeHost::open(&data).unwrap();
    assert!(snapshot(&data) == after);
}

#[test]
fn currency_historical_confirmation_changed_review_or_source_is_not_silently_adopted() {
    for mode in [
        "amount",
        "head",
        "source",
        "missing-account",
        "changed-after-confirm",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let mut host = fixture(temp.path(), false);
        let scope = closed(&mut host, "stale-old");
        let mut r = review(&host, &scope);
        let expected = r.digest().unwrap();
        if mode == "missing-account" || mode == "changed-after-confirm" {
            commit(&mut host, &r, "user", context(&r)).unwrap();
            drop(host);
            if mode == "missing-account" {
                fs::rename(
                    data.join("currency-budgets/project"),
                    temp.path().join("preserved-account"),
                )
                .unwrap();
            } else {
                let p = data.join("management-decisions/stale-old.0.json");
                let mut bytes = fs::read(&p).unwrap();
                bytes.push(b' ');
                fs::write(p, bytes).unwrap();
            }
            let before = snapshot(&data);
            assert!(RuntimeHost::open(&data).is_err(), "{mode}");
            assert!(snapshot(&data) == before, "{mode}");
            continue;
        }
        if mode == "amount" {
            r.amount_usd_nanos += 1;
        }
        if mode == "head" {
            r.previous_ledger_head = "f".repeat(64);
        }
        if mode == "source" {
            let p = data.join("management-decisions/stale-old.0.json");
            let mut bytes = fs::read(&p).unwrap();
            bytes.push(b' ');
            fs::write(p, bytes).unwrap();
        }
        let before = snapshot(&data);
        assert!(
            host.confirm_historical_charge(&r, &expected).is_err(),
            "{mode}"
        );
        assert!(snapshot(&data) == before);
    }
}

#[test]
fn currency_historical_confirmation_preserves_existing_unknown_reservations() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), false);
    let scope = closed(&mut host, "different-old");
    let i = intent(&host, "pending-money", Purpose::Plan);
    begin(&mut host, &i);
    host.reconcile_currency_account("project").unwrap();
    let r = review(&host, &scope);
    let reserved = host.currency_accounts["project"].state().reserved_usd_nanos;
    host.confirm_historical_charge(&r, &r.digest().unwrap())
        .unwrap();
    assert_eq!(
        host.currency_accounts["project"].state().reserved_usd_nanos,
        reserved
    );
    assert_eq!(
        host.currency_accounts["project"].state().charged_usd_nanos,
        17
    );
    assert!(matches!(
        host.currency_accounts["project"].state().bookings["pending-money"].outcome,
        Outcome::Unknown { .. }
    ));
}

#[test]
fn currency_historical_confirmation_unknown_project_is_global_and_cannot_charge_twice() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = fixture(temp.path(), false);
    let other = tempfile::tempdir().unwrap();
    let (other_host, mut draft) = project_planning::tests::fixture(other.path());
    drop(other_host);
    host.register(Project {
        id: "second".into(),
        root: other.path().join("project").to_string_lossy().into(),
        timezone: "Asia/Shanghai".into(),
    })
    .unwrap();
    draft = api_project_tests::draft_for(draft, ProjectWorkerAdapter::Openai, ApiTarget::OpenAi {});
    draft.project_id = "second".into();
    draft.currency_budget = Some(currency_product_tests::settings());
    host.configure_project(draft).unwrap();
    let saved = host.project_setups["second"].digest.clone();
    host.authorize_planning_budget("second", &saved).unwrap();
    let i = intent(&host, "orphan-history", Purpose::Plan);
    host.management.begin(i).unwrap();
    let original = data.join("management-decisions/orphan-history.0.json");
    let bytes = fs::read(&original).unwrap();
    let relative = "management-decisions/orphan-history.0.pending";
    fs::rename(original, data.join(relative)).unwrap();
    fs::write(data.join(relative), &bytes[..bytes.len() / 2]).unwrap();
    let digest = crate::management_review::inspect(&data, relative)
        .unwrap()
        .0;
    crate::management_review::acknowledge(
        &data,
        relative,
        &digest,
        "Original uncertain intent retained",
        |_| Ok(()),
    )
    .unwrap();
    host.management = crate::management_execution::DecisionJournal::open(&data).unwrap();
    let scope = HistoricalScope::Management {
        operation_id: "orphan-history".into(),
        original_project_id: None,
    };
    let first = review(&host, &scope);
    let second = host
        .review_historical_charge(
            "second",
            &scope,
            17,
            "controlled statement reference",
            "Explicit local test amount, not a supplier invoice",
        )
        .unwrap();
    host.confirm_historical_charge(&first, &first.digest().unwrap())
        .unwrap();
    let before = snapshot(&data);
    assert!(host
        .confirm_historical_charge(&second, &second.digest().unwrap())
        .is_err());
    assert!(snapshot(&data) == before);
    let mut changed = first.clone();
    changed
        .source
        .references
        .push("invented-display-reference".into());
    assert!(host
        .confirm_historical_charge(&changed, &changed.digest().unwrap())
        .is_err());
    assert!(snapshot(&data) == before);
    assert_eq!(
        host.currency_accounts["project"].state().charged_usd_nanos,
        17
    );
    assert_eq!(
        host.currency_accounts["second"].state().charged_usd_nanos,
        0
    );
    // The low-level trusted journal alone cannot enforce cross-project ownership.
    // Startup must reject this inconsistent original user record before any new charge.
    commit(&mut host, &second, "user", context(&second)).unwrap();
    drop(host);
    let before = snapshot(&data);
    let error = RuntimeHost::open(&data).err().unwrap();
    assert!(error.contains("more than one project"), "{error}");
    assert!(snapshot(&data) == before);
}

#[test]
fn currency_historical_confirmation_rejects_unlinked_policy_and_mismatched_ledger_charge() {
    for mode in ["wrong-policy", "wrong-amount", "missing-user-event"] {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let mut host = fixture(temp.path(), false);
        let scope = closed(&mut host, "proof-old");
        let mut r = review(&host, &scope);
        if mode == "wrong-policy" {
            r.policy_digest = "e".repeat(64);
            commit(&mut host, &r, "user", context(&r)).unwrap();
        } else {
            let event = if mode == "missing-user-event" {
                "f".repeat(64)
            } else {
                commit(&mut host, &r, "user", context(&r)).unwrap();
                host.projects["project"]
                    .history()
                    .last()
                    .unwrap()
                    .hash
                    .clone()
            };
            let mut charge = r.charge(&event);
            charge.amount_usd_nanos += 1;
            host.currency_accounts
                .get_mut("project")
                .unwrap()
                .record_historical_charge(charge)
                .unwrap();
        }
        drop(host);
        let before = snapshot(&data);
        assert!(RuntimeHost::open(&data).is_err(), "{mode}");
        assert!(snapshot(&data) == before, "{mode}");
    }
}

#[test]
fn currency_historical_confirmation_failed_money_publication_preserves_original_user_event() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = fixture(temp.path(), false);
    let scope = closed(&mut host, "io-old");
    let r = review(&host, &scope);
    let account = data.join("currency-budgets/project");
    let sequence = fs::read_dir(&account)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|s| s == "json"))
        .count();
    let pending = account.join(format!("{sequence:08}.pending"));
    fs::write(&pending, b"preserved interrupted publication").unwrap();
    let original = snapshot(&account);
    let events = host.projects["project"].history().len();
    assert!(host
        .confirm_historical_charge(&r, &r.digest().unwrap())
        .is_err());
    assert_eq!(host.projects["project"].history().len(), events + 1);
    assert_eq!(
        host.currency_accounts["project"].state().charged_usd_nanos,
        0
    );
    assert!(!host.currency_accounts["project"].healthy());
    assert!(snapshot(&account) == original);
    let after = snapshot(&data);
    assert!(host
        .confirm_historical_charge(&r, &r.digest().unwrap())
        .is_err());
    assert!(snapshot(&data) == after);
    drop(host);
    assert!(RuntimeHost::open(&data).is_err());
    assert!(snapshot(&data) == after);
}

#[test]
fn currency_historical_confirmation_multiple_missing_publications_are_not_reconstructed() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = fixture(temp.path(), false);
    let one = closed(&mut host, "gap-one");
    let two = closed(&mut host, "gap-two");
    let one = review(&host, &one);
    let two = review(&host, &two);
    commit(&mut host, &one, "user", context(&one)).unwrap();
    commit(&mut host, &two, "user", context(&two)).unwrap();
    drop(host);
    let before = snapshot(&data);
    let error = RuntimeHost::open(&data).err().unwrap();
    assert!(
        error.contains("multiple original historical publications"),
        "{error}"
    );
    assert!(snapshot(&data) == before);
}
