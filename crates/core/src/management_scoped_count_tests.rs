use super::*;
use crate::currency_budget::{CountIdentity, CountOutcome};
use cerulean_adapters::openai::scoped_pricing::{
    fixture_counted_scoped_decision, CountedScopedDecision, ScopedDecisionDraft,
};
use cerulean_domain::count_resolution::{CountResolutionAction, CountResolutionInput};
use cerulean_domain::currency_budget::scoped::{
    BillingBasis, ContextMetric, InputRange, PriceBand, RequestedService, ScopedPriceQuote,
};

pub(super) fn settings(fee: Option<u64>, budget: u64, body_limit: u32) -> BudgetSettings {
    let original = currency_product_tests::settings();
    BudgetSettings {
        schema: 2,
        limit_usd_nanos: budget,
        quotes: vec![],
        scoped_quotes: Some(
            original
                .quotes
                .into_iter()
                .map(|q| ScopedPriceQuote {
                    schema: 1,
                    target: q.target,
                    requested_model: q.requested_model,
                    requested_service: RequestedService::Explicit {
                        tier: "default".into(),
                    },
                    reported_models: q.reported_models.clone(),
                    reported_services: vec!["default".into()],
                    billing_basis: BillingBasis::PerRequest,
                    context_metric: ContextMetric::TotalInputIncludingCache,
                    bands: vec![PriceBand {
                        actual_model: q.reported_models[0].clone(),
                        actual_service: "default".into(),
                        input: InputRange {
                            from: 0,
                            through: MAX_TOKENS,
                        },
                        rates: q.rates,
                    }],
                    count_price: fee.map(|maximum_usd_nanos| ManagementCountPrice {
                        schema: 1,
                        maximum_usd_nanos,
                        maximum_body_bytes: body_limit,
                        source: "Synthetic scoped count fee; no supplier guarantee".into(),
                    }),
                    valid_from: q.valid_from,
                    valid_until: q.valid_until,
                    source: q.source,
                })
                .collect(),
        ),
    }
}
pub(super) fn fixture(
    root: &Path,
    execution: bool,
    settings: BudgetSettings,
    reader9: bool,
) -> RuntimeHost {
    fixture_with_limit(root, execution, settings, reader9, None)
}
pub(super) fn fixture_with_limit(
    root: &Path,
    execution: bool,
    settings: BudgetSettings,
    reader9: bool,
    limit: Option<u32>,
) -> RuntimeHost {
    fixture_with_configuration(root, execution, settings, reader9, limit, |_| {})
}
pub(super) fn fixture_with_configuration(
    root: &Path,
    execution: bool,
    settings: BudgetSettings,
    reader9: bool,
    limit: Option<u32>,
    configure: impl FnOnce(&mut cerulean_domain::project_setup::ProjectSetupDraft),
) -> RuntimeHost {
    let (host, original) = project_planning::tests::fixture(root);
    drop(host);
    let data = root.join("data");
    crate::currency_guard::tests::promote_fixture(&data);
    crate::currency_execution_guard::tests::promote_fixture(&data);
    crate::currency_count_guard::tests::promote_fixture(&data);
    crate::currency_count_resolution_guard::tests::promote_fixture(&data);
    crate::currency_scoped_guard::tests::promote_fixture(&data);
    if reader9 {
        crate::currency_scoped_count_guard::tests::promote_fixture(&data);
    }
    let mut h = RuntimeHost::open(&data).unwrap();
    let mut draft =
        api_project_tests::draft_for(original, ProjectWorkerAdapter::Openai, ApiTarget::OpenAi {});
    draft.currency_budget = Some(settings);
    if let Some(limit) = limit {
        draft.management_max_requests = limit;
    }
    configure(&mut draft);
    h.configure_project(draft).unwrap();
    if execution {
        let mut proposal = project_planning::tests::proposal("tests");
        if let ManagementDecision::Plan { tasks, .. } = &mut proposal {
            tasks[0].worker_id = ProjectWorkerAdapter::Openai.worker_id();
        }
        project_planning::tests::complete(&mut h, proposal);
        let digest = h.projects["project"]
            .revision()
            .unwrap()
            .approval_digest()
            .unwrap();
        h.approve("project", &digest).unwrap();
    } else {
        let setup = h.project_setups["project"].digest.clone();
        h.authorize_planning_budget("project", &setup).unwrap();
    }
    h
}
pub(super) fn draft(i: &DecisionIntent) -> ScopedDecisionDraft {
    ScopedDecisionDraft::standard(i.purpose, &i.model, &i.state_digest, &i.context).unwrap()
}
pub(super) fn quote(h: &RuntimeHost, i: &DecisionIntent) -> ScopedPriceQuote {
    h.currency_accounts["project"]
        .policy()
        .scoped_quotes
        .as_ref()
        .unwrap()
        .iter()
        .find(|q| q.requested_model == i.model)
        .unwrap()
        .clone()
}
pub(super) fn begin(h: &mut RuntimeHost, i: &DecisionIntent) -> Result<u64, String> {
    let q = digest(&quote(h, i))?;
    h.currency_accounts
        .get_mut("project")
        .unwrap()
        .begin_scoped_management_count(i, &draft(i), &q)
}
pub(super) fn counted(h: &RuntimeHost, i: &DecisionIntent) -> CountedScopedDecision {
    fixture_counted_scoped_decision(draft(i), quote(h, i), 100).unwrap()
}

#[test]
fn scoped_count_ledger_four_purposes_retain_fee_and_original_scope_after_reopen() {
    for purpose in [
        Purpose::Plan,
        Purpose::Assign,
        Purpose::Accept,
        Purpose::Advise,
    ] {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let mut h = fixture(
            temp.path(),
            purpose != Purpose::Plan,
            settings(Some(1000), 12_345_678_901, 256 * 1024),
            true,
        );
        let i = intent(&h, "original-scoped-count", purpose);
        let c = counted(&h, &i);
        assert_eq!(begin(&mut h, &i).unwrap(), 1000);
        let b = h.currency_accounts.get_mut("project").unwrap();
        assert_eq!(b.state().reserved_usd_nanos, 1000);
        assert!(matches!(
            b.state().management_counts[&i.operation_id]
                .attempt
                .identity,
            CountIdentity::Scoped(_)
        ));
        b.record_scoped_management_count(&i.operation_id, &c)
            .unwrap();
        let before = snapshot(&data);
        b.record_scoped_management_count(&i.operation_id, &c)
            .unwrap();
        assert_eq!(snapshot(&data), before);
        assert!(b.state().bookings.is_empty());
        assert!(h.management_active.is_empty());
        drop(h);
        let h = RuntimeHost::open(&data).unwrap();
        assert_eq!(snapshot(&data), before);
        let b = &h.currency_accounts["project"];
        assert_eq!(b.state().reserved_usd_nanos, 1000);
        assert!(matches!(
            b.state().management_counts[&i.operation_id].outcome,
            CountOutcome::ScopedCounted { .. }
        ));
        let view = h.currency_account_views(Some(&[]))[0]
            .management_counts
            .clone()
            .unwrap();
        assert_eq!(view.awaiting_generation, 1);
        assert_eq!(view.reserved_usd_nanos, 1000);
        assert!(view.needs_review);
    }
}
#[test]
fn scoped_count_ledger_restart_keeps_unknown_liability_and_never_recounts() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut h = fixture(
        temp.path(),
        false,
        settings(Some(1000), 12_345_678_901, 256 * 1024),
        true,
    );
    let i = intent(&h, "interrupted", Purpose::Plan);
    let c = counted(&h, &i);
    begin(&mut h, &i).unwrap();
    drop(h);
    let mut h = RuntimeHost::open(&data).unwrap();
    let before = snapshot(&data);
    assert!(matches!(
        h.currency_accounts["project"].state().management_counts[&i.operation_id].outcome,
        CountOutcome::Unknown { .. }
    ));
    assert!(begin(&mut h, &i).is_err());
    let b = h.currency_accounts.get_mut("project").unwrap();
    assert!(b
        .record_scoped_management_count(&i.operation_id, &c)
        .is_err());
    assert!(b.not_sent(&i.operation_id, &"a".repeat(64)).is_err());
    assert_eq!(b.state().reserved_usd_nanos, 1000);
    assert_eq!(snapshot(&data), before);
    drop(h);
    drop(RuntimeHost::open(&data).unwrap());
    assert_eq!(snapshot(&data), before);
}
#[test]
fn scoped_count_ledger_guard_precedes_write_and_removed_marker_refuses_startup() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut h = fixture(
        temp.path(),
        false,
        settings(Some(1000), 12_345_678_901, 256 * 1024),
        false,
    );
    let i = intent(&h, "guarded-count", Purpose::Plan);
    let before = snapshot(&data);
    assert!(begin(&mut h, &i).unwrap_err().contains("reader9"));
    assert_eq!(snapshot(&data), before);
    drop(h);
    crate::currency_scoped_count_guard::tests::promote_fixture(&data);
    let mut h = RuntimeHost::open(&data).unwrap();
    begin(&mut h, &i).unwrap();
    drop(h);
    let marker = data.join(crate::currency_scoped_count_guard::MARKER);
    let moved = temp.path().join("owned-guard.json");
    fs::rename(&marker, &moved).unwrap();
    let before = snapshot(&data);
    assert!(RuntimeHost::open(&data)
        .err()
        .unwrap()
        .contains("no reader9 guard"));
    assert_eq!(snapshot(&data), before);
    fs::rename(moved, marker).unwrap();
    drop(RuntimeHost::open(&data).unwrap());
}
#[test]
fn scoped_count_ledger_refuses_unknown_fee_body_overflow_budget_and_wrong_identity_without_write() {
    for terms in [
        settings(None, 12_345_678_901, 256 * 1024),
        settings(Some(1000), 12_345_678_901, 1),
        settings(Some(1001), 1000, 256 * 1024),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let mut h = fixture(temp.path(), false, terms, true);
        let i = intent(&h, "refused-count", Purpose::Plan);
        let before = snapshot(&temp.path().join("data"));
        assert!(begin(&mut h, &i).is_err());
        assert_eq!(snapshot(&temp.path().join("data")), before);
    }
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut h = fixture(
        temp.path(),
        false,
        settings(Some(0), 1000, 256 * 1024),
        true,
    );
    let i = intent(&h, "zero-explicit", Purpose::Plan);
    assert_eq!(begin(&mut h, &i).unwrap(), 0);
    let mut other = i.clone();
    other.operation_id = "other".into();
    other.context["requirement"] = serde_json::json!("changed");
    other.state_digest = digest(&other.context).unwrap();
    let changed = counted(&h, &other);
    let q = quote(&h, &i);
    let before = snapshot(&data);
    let b = h.currency_accounts.get_mut("project").unwrap();
    assert!(b
        .record_scoped_management_count(&i.operation_id, &changed)
        .is_err());
    assert!(b
        .begin_scoped_management_count(&i, &draft(&other), &digest(&q).unwrap())
        .is_err());
    assert!(b
        .begin_scoped_management_count(&other, &draft(&other), &"a".repeat(64))
        .is_err());
    assert_eq!(snapshot(&data), before);
}
#[test]
fn scoped_count_ledger_rejects_cross_wire_receipt_and_preserves_legacy_serialization() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut h = fixture(
        temp.path(),
        false,
        settings(Some(1000), 12_345_678_901, 256 * 1024),
        true,
    );
    let i = intent(&h, "separate-wire", Purpose::Plan);
    begin(&mut h, &i).unwrap();
    let old = cerulean_adapters::openai::fixture_counted_decision(prepared(&i), 100).unwrap();
    let before = snapshot(&data);
    let b = h.currency_accounts.get_mut("project").unwrap();
    assert!(b.record_management_count(&i.operation_id, &old).is_err());
    assert_eq!(snapshot(&data), before);
    drop(h);
    let temp = tempfile::tempdir().unwrap();
    let mut h = count_ledger::count_fixture(temp.path(), 1000, 256 * 1024, 12_345_678_901);
    let i = intent(&h, "legacy-wire", Purpose::Plan);
    let q = digest(&h.currency_accounts["project"].policy().quotes[0]).unwrap();
    let b = h.currency_accounts.get_mut("project").unwrap();
    b.begin_management_count(&i, &prepared(&i), &q).unwrap();
    let id = &b.state().management_counts[&i.operation_id]
        .attempt
        .identity;
    let CountIdentity::Legacy(legacy) = id else {
        panic!("legacy identity changed")
    };
    assert_eq!(
        serde_json::to_vec(id).unwrap(),
        serde_json::to_vec(legacy).unwrap()
    );
    assert_eq!(
        serde_json::from_slice::<CountIdentity>(&serde_json::to_vec(legacy).unwrap()).unwrap(),
        *id
    );
}
#[test]
fn scoped_count_ledger_original_fee_resolution_and_retirement_reuse_user_workflow() {
    for received in [false, true] {
        for retire_first in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let data = temp.path().join("data");
            let mut h = fixture(
                temp.path(),
                false,
                settings(Some(1000), 12_345_678_901, 256 * 1024),
                true,
            );
            let i = intent(&h, "resolve-scoped", Purpose::Plan);
            let c = counted(&h, &i);
            begin(&mut h, &i).unwrap();
            let b = h.currency_accounts.get_mut("project").unwrap();
            if received {
                b.record_scoped_management_count(&i.operation_id, &c)
                    .unwrap();
            } else {
                b.mark_management_count_unknown(&i.operation_id, &"a".repeat(64))
                    .unwrap();
            }
            let source = b.state().management_counts[&i.operation_id]
                .source_digest()
                .unwrap();
            for retire in [retire_first, !retire_first] {
                let input = CountResolutionInput {
                    project_id: "project".into(),
                    operation_id: i.operation_id.clone(),
                    action: if retire {
                        CountResolutionAction::RetireContinuation {
                            reason: "Synthetic explicit retirement".into(),
                        }
                    } else {
                        CountResolutionAction::SettleFee {
                            amount_usd_nanos: 17,
                            billing_reference: "Synthetic invoice".into(),
                            explanation: "Controlled test only".into(),
                        }
                    },
                };
                let review = h.review_management_count_resolution(&input).unwrap();
                h.confirm_management_count_resolution(&review, &review.digest().unwrap())
                    .unwrap();
                let before = snapshot(&data);
                h.confirm_management_count_resolution(&review, &review.digest().unwrap())
                    .unwrap();
                assert_eq!(snapshot(&data), before);
            }
            assert!(begin(&mut h, &i).is_err());
            let state = h.currency_accounts["project"].state();
            assert_eq!(
                state.management_counts[&i.operation_id]
                    .source_digest()
                    .unwrap(),
                source
            );
            assert_eq!(state.reserved_usd_nanos, 0);
            assert_eq!(state.charged_usd_nanos, 17);
            assert!(!h.management_count_errors.contains_key("project"));
            let view = h.currency_account_views(Some(&[]))[0]
                .management_counts
                .clone()
                .unwrap();
            assert!(!view.needs_review);
            assert_eq!(view.reserved_usd_nanos, 0);
            let before = snapshot(&data);
            drop(h);
            drop(RuntimeHost::open(&data).unwrap());
            assert_eq!(snapshot(&data), before);
        }
    }
}
#[test]
fn scoped_count_ledger_shared_balance_and_rehashed_tamper_cannot_change_fee_or_quote() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut h = fixture(
        temp.path(),
        false,
        settings(Some(1000), 1000, 256 * 1024),
        true,
    );
    let i = intent(&h, "full-budget", Purpose::Plan);
    begin(&mut h, &i).unwrap();
    let mut other = i.clone();
    other.operation_id = "excess-count".into();
    let before = snapshot(&data);
    assert!(begin(&mut h, &other).unwrap_err().contains("exhausted"));
    assert_eq!(snapshot(&data), before);
    drop(h);
    let path = data.join("currency-budgets/project/00000002.json");
    let original = fs::read(&path).unwrap();
    for (field, value) in [
        ("maximumUsdNanos", serde_json::json!(1)),
        ("quoteDigest", serde_json::json!("a".repeat(64))),
        ("bodySha256", serde_json::json!("b".repeat(64))),
        ("schema", serde_json::json!(1)),
    ] {
        let mut event: serde_json::Value = serde_json::from_slice(&original).unwrap();
        event["body"]["event"]["attempt"][field] = value;
        fs::write(&path, crate::currency_budget::fixture_rehash_record(event)).unwrap();
        assert!(RuntimeHost::open(&data).is_err());
    }
    fs::write(path, original).unwrap();
    drop(RuntimeHost::open(&data).unwrap());
}

#[test]
fn scoped_count_ledger_late_receipt_uses_original_quote_and_history_guard_survives_downgrade() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut h = fixture(
        temp.path(),
        false,
        settings(Some(1000), 12_345_678_901, 256 * 1024),
        true,
    );
    let i = intent(&h, "late-count", Purpose::Plan);
    let c = counted(&h, &i);
    begin(&mut h, &i).unwrap();
    // Controlled low-level policy transition; not a fabricated Host user event.
    let b = h.currency_accounts.get_mut("project").unwrap();
    let legacy = currency_product_tests::settings()
        .bind("project", &"a".repeat(64))
        .unwrap();
    b.authorize(crate::currency_budget::PolicyAuthorization {
        previous_policy_digest: digest(b.policy()).unwrap(),
        previous_ledger_head: b.head().into(),
        policy: legacy.clone(),
        approval_evidence_sha256: "b".repeat(64),
    })
    .unwrap();
    b.record_scoped_management_count(&i.operation_id, &c)
        .unwrap();
    assert_eq!(b.state().reserved_usd_nanos, 1000);
    let marker = data.join(crate::currency_scoped_count_guard::MARKER);
    let bytes = fs::read(&marker).unwrap();
    fs::remove_file(&marker).unwrap();
    let before = snapshot(&data);
    assert!(b
        .record_scoped_management_count(&i.operation_id, &c)
        .unwrap_err()
        .contains("reader9"));
    assert!(b
        .mark_management_count_unknown(&i.operation_id, &"c".repeat(64))
        .unwrap_err()
        .contains("reader9"));
    assert_eq!(snapshot(&data), before);
    fs::write(&marker, bytes).unwrap();
    let expected = b.state().clone();
    drop(h);
    let reopened =
        crate::currency_budget::BudgetLedger::open(&data.join("currency-budgets/project"), &legacy)
            .unwrap();
    assert_eq!(reopened.state(), &expected);
}

#[test]
fn scoped_count_ledger_receipt_replay_rejects_rehashed_tamper_and_oversize_is_prepublication() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut h = fixture(
        temp.path(),
        false,
        settings(Some(1000), 12_345_678_901, 256 * 1024),
        true,
    );
    let i = intent(&h, "receipt-tamper", Purpose::Plan);
    let mut oversized = i.clone();
    oversized.context["requirement"] = serde_json::json!("x".repeat(140 * 1024));
    oversized.state_digest = digest(&oversized.context).unwrap();
    let before = snapshot(&data);
    assert!(ScopedDecisionDraft::standard(
        oversized.purpose,
        &oversized.model,
        &oversized.state_digest,
        &oversized.context,
    )
    .is_err());
    assert_eq!(snapshot(&data), before);
    begin(&mut h, &i).unwrap();
    let c = counted(&h, &i);
    h.currency_accounts
        .get_mut("project")
        .unwrap()
        .record_scoped_management_count(&i.operation_id, &c)
        .unwrap();
    drop(h);
    let path = data.join("currency-budgets/project/00000003.json");
    let original = fs::read(&path).unwrap();
    for (pointer, value) in [
        (
            "/body/event/receipt/quoteSha256",
            serde_json::json!("a".repeat(64)),
        ),
        ("/body/event/receipt/inputTokens", serde_json::json!(101)),
        (
            "/body/event/receipt/request/bodySha256",
            serde_json::json!("b".repeat(64)),
        ),
        (
            "/body/event/receipt/request/countBodyBytes",
            serde_json::json!(1),
        ),
    ] {
        let mut value_record: serde_json::Value = serde_json::from_slice(&original).unwrap();
        *value_record.pointer_mut(pointer).unwrap() = value;
        fs::write(
            &path,
            crate::currency_budget::fixture_rehash_record(value_record),
        )
        .unwrap();
        assert!(RuntimeHost::open(&data).is_err());
    }
    fs::write(path, original).unwrap();
    drop(RuntimeHost::open(&data).unwrap());
}
