use super::*;
use crate::count_resolution::{CountResolutionReview, APPROVAL_SOURCE};
use cerulean_adapters::openai::fixture_counted_decision;
use cerulean_domain::count_resolution::{CountResolutionAction as Action, CountResolutionInput};

fn fixture(root: &Path) -> RuntimeHost {
    let host = count_ledger::count_fixture(root, 1000, 256 * 1024, 12_345_678_901);
    drop(host);
    crate::currency_count_resolution_guard::tests::promote_fixture(&root.join("data"));
    RuntimeHost::open(&root.join("data")).unwrap()
}
fn original(
    host: &mut RuntimeHost,
    id: &str,
    counted: bool,
) -> (DecisionIntent, cerulean_adapters::openai::CountedDecision) {
    // Synthetic extra records exercise stale heads without invoking a new dispatch.
    let i = if let Some(c) = host.currency_accounts["project"]
        .state()
        .management_counts
        .values()
        .next()
    {
        let mut i = c.attempt.intent.clone();
        i.operation_id = id.into();
        i
    } else {
        intent(host, id, Purpose::Plan)
    };
    let c = fixture_counted_decision(prepared(&i), 100).unwrap();
    let book = host.currency_accounts.get_mut("project").unwrap();
    let quote = digest(&book.policy().quotes[0]).unwrap();
    book.begin_management_count(&i, &prepared(&i), &quote)
        .unwrap();
    if counted {
        book.record_management_count(id, &c).unwrap();
    } else {
        book.mark_management_count_unknown(id, &"a".repeat(64))
            .unwrap();
    }
    host.refresh_management_count_attention("project");
    (i, c)
}
fn input(id: &str, amount: Option<u64>) -> CountResolutionInput {
    CountResolutionInput {
        project_id: "project".into(),
        operation_id: id.into(),
        action: match amount {
            Some(amount_usd_nanos) => Action::SettleFee {
                amount_usd_nanos,
                billing_reference: "synthetic bill".into(),
                explanation: "controlled local invoice amount; no supplier".into(),
            },
            None => Action::RetireContinuation {
                reason: "explicitly retire this original operation".into(),
            },
        },
    }
}
fn review(host: &RuntimeHost, id: &str, amount: Option<u64>) -> CountResolutionReview {
    host.review_management_count_resolution(&input(id, amount))
        .unwrap()
}
fn commit(
    host: &mut RuntimeHost,
    r: &CountResolutionReview,
    principal: &str,
    context: CommandContext,
) -> Result<(), String> {
    host.projects
        .get_mut("project")
        .unwrap()
        .command(
            &format!("count-user-{}", r.digest()?),
            principal,
            WorkflowCommand::ConfirmManagementCountResolution {
                review_digest: r.digest()?,
            },
            context,
            false,
        )
        .map(|_| ())
}
fn context(r: &CountResolutionReview) -> CommandContext {
    CommandContext {
        count_resolution: Some(r.clone()),
        approval_source: Some(APPROVAL_SOURCE.into()),
        ..Default::default()
    }
}

#[test]
fn count_resolution_fee_and_retirement_are_independent_exact_once_and_restart_stable() {
    for retire_first in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let mut h = fixture(temp.path());
        let (i, c) = original(&mut h, "count-once", true);
        let source = h.currency_accounts["project"].state().management_counts["count-once"].clone();
        let before = snapshot(&data);
        let r = review(&h, "count-once", if retire_first { None } else { Some(17) });
        assert_eq!(before, snapshot(&data));
        h.confirm_management_count_resolution(&r, &r.digest().unwrap())
            .unwrap();
        assert_eq!(
            h.currency_accounts["project"].state().reserved_usd_nanos,
            if retire_first { 1000 } else { 0 }
        );
        assert_eq!(
            h.management_count_errors.contains_key("project"),
            !retire_first
        );
        let other = review(&h, "count-once", if retire_first { Some(17) } else { None });
        h.confirm_management_count_resolution(&other, &other.digest().unwrap())
            .unwrap();
        let b = &h.currency_accounts["project"];
        assert_eq!(b.state().charged_usd_nanos, 17);
        assert_eq!(b.state().reserved_usd_nanos, 0);
        assert_eq!(b.state().management_counts.len(), 1);
        assert_eq!(
            b.state().management_counts["count-once"].attempt,
            source.attempt
        );
        assert_eq!(
            b.state().management_counts["count-once"].outcome,
            source.outcome
        );
        assert!(!h.management_count_errors.contains_key("project"));
        let before = snapshot(&data);
        h.confirm_management_count_resolution(&r, &r.digest().unwrap())
            .unwrap();
        h.confirm_management_count_resolution(&other, &other.digest().unwrap())
            .unwrap();
        assert_eq!(before, snapshot(&data));
        let quote = digest(&h.currency_accounts["project"].policy().quotes[0]).unwrap();
        let b = h.currency_accounts.get_mut("project").unwrap();
        assert!(b.begin_management_count(&i, &prepared(&i), &quote).is_err());
        assert!(b.prepare_counted_management(&i, &c, &quote).is_err());
        let s = h.currency_account_views(Some(&[]))[0]
            .management_counts
            .clone()
            .unwrap();
        assert_eq!(s.reserved_usd_nanos, 0);
        assert_eq!(s.awaiting_generation, 0);
        assert!(!s.needs_review);
        drop(h);
        let mut h = RuntimeHost::open(&data).unwrap();
        assert_eq!(before, snapshot(&data));
        h.confirm_management_count_resolution(&r, &r.digest().unwrap())
            .unwrap();
        assert_eq!(before, snapshot(&data));
        assert!(!h.management_count_errors.contains_key("project"));
        assert!(h.management_active.is_empty());
        assert!(h.management.statuses().is_empty());
    }
}
#[test]
fn count_resolution_unknown_zero_and_excess_bill_preserve_actual_liability() {
    for amount in [0, 12_345_678_902] {
        let t = tempfile::tempdir().unwrap();
        let mut h = fixture(t.path());
        original(&mut h, "unknown-bill", false);
        let r = review(&h, "unknown-bill", Some(amount));
        h.confirm_management_count_resolution(&r, &r.digest().unwrap())
            .unwrap();
        let b = &h.currency_accounts["project"];
        assert_eq!(b.state().charged_usd_nanos, amount);
        assert_eq!(b.state().reserved_usd_nanos, 0);
        assert_eq!(
            b.state().available_usd_nanos(b.policy()).is_err(),
            amount > 0
        );
        assert!(h.management_count_errors.contains_key("project"));
        let r = review(&h, "unknown-bill", None);
        h.confirm_management_count_resolution(&r, &r.digest().unwrap())
            .unwrap();
        assert!(!h.management_count_errors.contains_key("project"));
        assert!(matches!(
            h.currency_accounts["project"].state().management_counts["unknown-bill"].outcome,
            crate::currency_budget::CountOutcome::Unknown { .. }
        ));
    }
}
#[test]
fn count_resolution_authentication_plain_context_and_reader7_required() {
    let t = tempfile::tempdir().unwrap();
    let data = t.path().join("data");
    let mut h = fixture(t.path());
    original(&mut h, "auth-count", false);
    let r = review(&h, "auth-count", Some(17));
    let before = snapshot(&data);
    for principal in ["chief", "deputy", "core", "worker-openai"] {
        assert!(commit(&mut h, &r, principal, context(&r)).is_err());
    }
    for mode in 0..4 {
        let mut c = context(&r);
        match mode {
            0 => c.count_resolution = None,
            1 => c.approval_source = Some("model assertion".into()),
            2 => c.cwd = Some("extra".into()),
            _ => c.count_resolution.as_mut().unwrap().maximum_usd_nanos += 1,
        };
        assert!(commit(&mut h, &r, "user", c).is_err());
    }
    assert_eq!(before, snapshot(&data));
    fs::rename(
        data.join(crate::currency_count_resolution_guard::MARKER),
        t.path().join("saved-marker"),
    )
    .unwrap();
    assert!(commit(&mut h, &r, "user", context(&r)).is_err());
    assert!(h
        .confirm_management_count_resolution(&r, &r.digest().unwrap())
        .is_err());
}
#[test]
fn count_resolution_recovers_only_original_user_event_and_rejects_unlinked_ledger() {
    for linked in [true, false] {
        let t = tempfile::tempdir().unwrap();
        let data = t.path().join("data");
        let mut h = fixture(t.path());
        original(&mut h, "recover-count", false);
        let r = review(&h, "recover-count", Some(17));
        if linked {
            commit(&mut h, &r, "user", context(&r)).unwrap();
        } else {
            h.currency_accounts
                .get_mut("project")
                .unwrap()
                .record_management_count_resolution(r.resolution(&"f".repeat(64)))
                .unwrap();
        }
        let history = h.projects["project"].history().to_vec();
        drop(h);
        if linked {
            let h = RuntimeHost::open(&data).unwrap();
            assert_eq!(h.projects["project"].history(), history);
            assert_eq!(h.currency_accounts["project"].state().charged_usd_nanos, 17);
            let before = snapshot(&data);
            drop(h);
            drop(RuntimeHost::open(&data).unwrap());
            assert_eq!(before, snapshot(&data));
        } else {
            assert!(RuntimeHost::open(&data).is_err());
        }
    }
}
#[test]
fn count_resolution_stale_conflicting_or_pending_reviews_do_not_write() {
    let t = tempfile::tempdir().unwrap();
    let data = t.path().join("data");
    let mut h = fixture(t.path());
    original(&mut h, "stale-count", false);
    let old = review(&h, "stale-count", Some(17));
    original(&mut h, "other-count", false);
    let before = snapshot(&data);
    assert!(h
        .confirm_management_count_resolution(&old, &old.digest().unwrap())
        .is_err());
    assert_eq!(before, snapshot(&data));
    let r = review(&h, "stale-count", Some(17));
    h.confirm_management_count_resolution(&r, &r.digest().unwrap())
        .unwrap();
    let mut changed = r.clone();
    changed.input.action = input("stale-count", Some(18)).action;
    let before = snapshot(&data);
    assert!(h
        .confirm_management_count_resolution(&changed, &changed.digest().unwrap())
        .is_err());
    assert_eq!(before, snapshot(&data));
    let mut i = h.currency_accounts["project"].state().management_counts["stale-count"]
        .attempt
        .intent
        .clone();
    i.operation_id = "pending-count".into();
    let b = h.currency_accounts.get_mut("project").unwrap();
    let q = digest(&b.policy().quotes[0]).unwrap();
    b.begin_management_count(&i, &prepared(&i), &q).unwrap();
    let before = snapshot(&data);
    assert!(h
        .review_management_count_resolution(&input("pending-count", Some(0)))
        .is_err());
    assert!(h
        .review_management_count_resolution(&input("pending-count", None))
        .is_err());
    assert_eq!(before, snapshot(&data));
}
#[test]
fn count_resolution_failed_ledger_publication_preserves_user_event_and_never_resends() {
    let t = tempfile::tempdir().unwrap();
    let data = t.path().join("data");
    let mut h = fixture(t.path());
    original(&mut h, "io-count", false);
    let r = review(&h, "io-count", Some(17));
    let account = data.join("currency-budgets/project");
    let seq = fs::read_dir(&account)
        .unwrap()
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|s| s == "json"))
        .count();
    fs::write(
        account.join(format!("{seq:08}.pending")),
        b"preserve collision",
    )
    .unwrap();
    let before = snapshot(&account);
    let events = h.projects["project"].history().len();
    assert!(h
        .confirm_management_count_resolution(&r, &r.digest().unwrap())
        .is_err());
    assert_eq!(events + 1, h.projects["project"].history().len());
    assert_eq!(before, snapshot(&account));
    assert!(!h.currency_accounts["project"].healthy());
    let after = snapshot(&data);
    assert!(h
        .confirm_management_count_resolution(&r, &r.digest().unwrap())
        .is_err());
    assert_eq!(after, snapshot(&data));
    drop(h);
    assert!(RuntimeHost::open(&data).is_err());
    assert_eq!(after, snapshot(&data));
}
#[test]
fn count_resolution_preserves_generation_reservation_and_requires_generation_closure() {
    let t = tempfile::tempdir().unwrap();
    let mut h = fixture(t.path());
    let (i, c) = original(&mut h, "generation-count", true);
    let b = h.currency_accounts.get_mut("project").unwrap();
    let q = digest(&b.policy().quotes[0]).unwrap();
    let held = b.prepare_counted_management(&i, &c, &q).unwrap();
    assert!(h
        .review_management_count_resolution(&input("generation-count", None))
        .is_err());
    h.reconcile_currency_account("project").unwrap();
    let r = review(&h, "generation-count", Some(17));
    h.confirm_management_count_resolution(&r, &r.digest().unwrap())
        .unwrap();
    assert_eq!(
        h.currency_accounts["project"].state().reserved_usd_nanos,
        held
    );
    assert!(h
        .review_management_count_resolution(&input("generation-count", None))
        .is_err());
    h.management
        .begin_with_request(i.clone(), Some(prepared(&i).fingerprint().clone()))
        .unwrap();
    h.management
        .finish_result(&i.operation_id, Ok(result(&i)))
        .unwrap();
    assert!(h.reconcile_currency_account("project").is_err());
    // A generation appearing after count confirmation violates the original
    // frozen source. It must never be silently adopted on recovery.
    assert!(h.count_resolution_confirmations().is_err());
}
#[test]
fn count_resolution_product_list_review_confirm_lookup_and_route_separation() {
    let t = tempfile::tempdir().unwrap();
    let data = t.path().join("data");
    let mut h = fixture(t.path());
    original(&mut h, "product-count", false);
    let before = snapshot(&data);
    let p = h
        .count_resolution_result(&Command::ReadManagementCounts {
            offset: 0,
            revision: None,
        })
        .unwrap()
        .counts
        .unwrap();
    assert_eq!(p.total, 1);
    assert_eq!(p.entries[0].original_outcome, "unknown");
    assert_eq!(before, snapshot(&data));
    assert!(h.count_resolution_result(&Command::Snapshot).is_err());
    assert!(h
        .count_resolution_result(&Command::ReadManagementCounts {
            offset: 1,
            revision: None
        })
        .is_err());
    assert!(h
        .count_resolution_result(&Command::ReadManagementCounts {
            offset: 0,
            revision: Some("wrong".into())
        })
        .is_err());
    let input = input("product-count", Some(17));
    let r = h
        .count_resolution_result(&Command::ReviewManagementCountResolution {
            input: input.clone(),
        })
        .unwrap()
        .review
        .unwrap();
    let cmd = Command::ConfirmManagementCountResolution {
        input,
        review_digest: r.review_digest.clone(),
    };
    let out = h.count_resolution_result(&cmd).unwrap();
    assert_eq!(
        out.confirmation.unwrap().record.unwrap().state,
        cerulean_domain::count_resolution::CountResolutionState::Recorded
    );
    let before = snapshot(&data);
    h.count_resolution_result(&cmd).unwrap();
    assert_eq!(before, snapshot(&data));
    let request = Request {
        id: "wrong-route".into(),
        protocol_version: cerulean_domain::PROTOCOL_VERSION,
        command: cmd,
    };
    assert!(h.handle(request).error.is_some());
    assert_eq!(before, snapshot(&data));
}

#[test]
fn count_resolution_settled_generation_keeps_separate_charges_and_original_response() {
    let t = tempfile::tempdir().unwrap();
    let data = t.path().join("data");
    let mut h = fixture(t.path());
    let (i, c) = original(&mut h, "closed-generation", true);
    let b = h.currency_accounts.get_mut("project").unwrap();
    let q = digest(&b.policy().quotes[0]).unwrap();
    b.prepare_counted_management(&i, &c, &q).unwrap();
    h.management
        .begin_with_request(i.clone(), Some(prepared(&i).fingerprint().clone()))
        .unwrap();
    h.management
        .finish_result(&i.operation_id, Ok(result(&i)))
        .unwrap();
    h.reconcile_currency_account("project").unwrap();
    let generation = h.currency_accounts["project"].state().charged_usd_nanos;
    let original_booking = h.currency_accounts["project"].state().bookings[&i.operation_id].clone();
    assert!(h
        .review_management_count_resolution(&input(&i.operation_id, None))
        .is_err());
    let head = h
        .management
        .evidence_digest(&i.operation_id)
        .unwrap()
        .to_owned();
    h.management
        .resolve(
            &i.operation_id,
            &head,
            UserResolution::Discard {
                reason: "explicit local closure".into(),
            },
        )
        .unwrap();
    let r = review(&h, &i.operation_id, Some(17));
    h.confirm_management_count_resolution(&r, &r.digest().unwrap())
        .unwrap();
    let retire = review(&h, &i.operation_id, None);
    h.confirm_management_count_resolution(&retire, &retire.digest().unwrap())
        .unwrap();
    assert_eq!(
        h.currency_accounts["project"].state().charged_usd_nanos,
        generation + 17
    );
    assert_eq!(
        h.currency_accounts["project"].state().bookings[&i.operation_id],
        original_booking
    );
    assert_eq!(h.management.response_head(&i.operation_id).unwrap(), head);
    let before = snapshot(&data);
    drop(h);
    drop(RuntimeHost::open(&data).unwrap());
    assert_eq!(before, snapshot(&data));
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "Owned actual Windows named-pipe count resolution; requires normal host permissions"]
async fn count_resolution_native_original_confirmation_and_restart() {
    use crate::ipc::{self, Client, UserOperation};
    use std::time::Duration;
    struct Service(tokio::task::JoinHandle<Result<(), String>>);
    impl Drop for Service {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    async fn client(data: &Path) -> Client {
        for _ in 0..100 {
            if let Ok(c) = Client::connect(data).await {
                return c;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("owned service not ready");
    }
    async fn query(
        data: &Path,
        command: Command,
    ) -> cerulean_domain::count_resolution::CountResolutionResult {
        let reply = client(data)
            .await
            .call(UserOperation::CountResolution {
                request: Request {
                    id: "owned-count".into(),
                    protocol_version: cerulean_domain::PROTOCOL_VERSION,
                    command,
                },
            })
            .await
            .unwrap();
        assert!(reply.response.error.is_none(), "{:?}", reply.response.error);
        assert!(reply.response.result.is_none());
        reply.count_resolution.unwrap()
    }
    async fn stop(data: &Path, mut service: Service) {
        let r = client(data)
            .await
            .call(UserOperation::Shutdown {
                id: "owned-count-stop".into(),
                protocol_version: cerulean_domain::PROTOCOL_VERSION,
            })
            .await
            .unwrap();
        assert!(r.stopping);
        tokio::time::timeout(Duration::from_secs(20), &mut service.0)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
    let t = tempfile::tempdir().unwrap();
    let data = t.path().join("data");
    let mut h = fixture(t.path());
    original(&mut h, "native-count", false);
    let d = data.clone();
    let service = Service(tokio::spawn(async move { ipc::serve_host(&d, h).await }));
    let before = snapshot(&data);
    let p = query(
        &data,
        Command::ReadManagementCounts {
            offset: 0,
            revision: None,
        },
    )
    .await
    .counts
    .unwrap();
    assert_eq!(p.total, 1);
    assert_eq!(before, snapshot(&data));
    let input = input("native-count", Some(0));
    let r = query(
        &data,
        Command::ReviewManagementCountResolution {
            input: input.clone(),
        },
    )
    .await
    .review
    .unwrap();
    let cmd = Command::ConfirmManagementCountResolution {
        input,
        review_digest: r.review_digest.clone(),
    };
    let record = query(&data, cmd.clone())
        .await
        .confirmation
        .unwrap()
        .record
        .unwrap();
    assert_eq!(record.review, r);
    let after = snapshot(&data);
    query(&data, cmd).await;
    assert_eq!(after, snapshot(&data));
    stop(&data, service).await;
    let h = RuntimeHost::open(&data).unwrap();
    let d = data.clone();
    let service = Service(tokio::spawn(async move { ipc::serve_host(&d, h).await }));
    let lookup = Command::ReadManagementCountConfirmation {
        project_id: "project".into(),
        operation_id: "native-count".into(),
        aspect: cerulean_domain::count_resolution::CountResolutionAspect::SettleFee,
        review_digest: r.review_digest,
    };
    let read = query(&data, lookup)
        .await
        .confirmation
        .unwrap()
        .record
        .unwrap();
    assert_eq!(read, record);
    stop(&data, service).await;
}
