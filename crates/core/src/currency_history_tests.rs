use super::*;

#[test]
fn currency_historical_source_requires_closed_scope_and_preserves_original_files() {
    use crate::currency_budget::HistoricalScope;
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = fixture(temp.path(), false);
    let i = intent(&host, "source-review", Purpose::Plan);
    let scope = HistoricalScope::Management {
        operation_id: i.operation_id.clone(),
        original_project_id: Some("project".into()),
    };
    host.management.begin(i.clone()).unwrap();
    host.management
        .finish(&i.operation_id, Ok(result(&i).observation))
        .unwrap();
    assert!(host.historical_source_for_review(&scope).is_err());
    let head = host
        .management
        .evidence_digest(&i.operation_id)
        .unwrap()
        .to_owned();
    host.management
        .resolve(
            &i.operation_id,
            &head,
            UserResolution::Discard {
                reason: "Close local decision; historical fee remains unknown".into(),
            },
        )
        .unwrap();
    let before = snapshot(&data);
    let source = host.historical_source_for_review(&scope).unwrap();
    assert_eq!(source.scope, scope);
    assert_eq!(source.uncertain_outcomes, 1);
    assert_eq!(source.references.len(), 3);
    assert_eq!(host.historical_source_for_review(&scope).unwrap(), source);
    assert!(snapshot(&data) == before);
    let wrong = HistoricalScope::Management {
        operation_id: i.operation_id.clone(),
        original_project_id: None,
    };
    assert!(host.historical_source_for_review(&wrong).is_err());
    let (_tx, rx) = std::sync::mpsc::channel();
    host.management_active.insert(i.operation_id.clone(), rx);
    assert!(host.historical_source_for_review(&scope).is_err());
    host.management_active.remove(&i.operation_id);
    let path = data.join("management-decisions/source-review.0.json");
    let mut bytes = fs::read(&path).unwrap();
    bytes.push(b' ');
    fs::write(&path, bytes).unwrap();
    assert_ne!(
        host.historical_source_for_review(&scope).unwrap().digest,
        source.digest
    );
    fs::rename(
        data.join("management-decisions"),
        temp.path().join("original-management"),
    )
    .unwrap();
    assert!(host.historical_source_for_review(&scope).is_err());
    assert!(!data.join("management-decisions").exists());
}

#[test]
fn currency_history_legacy_discard_and_reopen_keep_unaccounted_cost_visible() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = fixture(temp.path(), false);
    assert!(host.snapshot().currency_budgets[0]
        .available_usd_nanos
        .is_some());
    let i = intent(&host, "old-planning", Purpose::Plan);
    host.management.begin(i.clone()).unwrap();
    host.management
        .finish(&i.operation_id, Ok(result(&i).observation))
        .unwrap();
    let head = host
        .management
        .response_head(&i.operation_id)
        .unwrap()
        .to_owned();
    host.management
        .resolve(
            &i.operation_id,
            &head,
            UserResolution::Discard {
                reason: "Local decision closed, not a billing refund".into(),
            },
        )
        .unwrap();
    let before = snapshot(&data);
    let view = host.snapshot();
    let gaps = view.currency_history.unwrap();
    assert_eq!(gaps.len(), 1);
    assert_eq!(
        gaps[0].reason,
        CurrencyHistoryReason::LegacyManagementRequest
    );
    assert_eq!(gaps[0].source_id, i.operation_id);
    assert_eq!(gaps[0].project_id.as_deref(), Some("project"));
    assert_eq!(view.currency_budgets[0].available_usd_nanos, None);
    assert_eq!(view.currency_budgets[0].charged_usd_nanos, 0);
    assert_eq!(view.currency_budgets[0].reserved_usd_nanos, 0);
    assert!(snapshot(&data) == before);
    drop(host);
    let host = RuntimeHost::open(&data).unwrap();
    assert_eq!(host.snapshot().currency_history, Some(gaps));
    assert!(snapshot(&data) == before);
}

#[test]
fn currency_history_wire_without_budget_has_history_but_no_invented_account() {
    let temp = tempfile::tempdir().unwrap();
    let (host, draft) = project_planning::tests::fixture(temp.path());
    drop(host);
    let data = temp.path().join("data");
    crate::currency_guard::tests::promote_fixture(&data);
    let mut host = RuntimeHost::open(&data).unwrap();
    host.configure_project(draft).unwrap();
    let i = intent(&host, "unbudgeted-planning", Purpose::Plan);
    host.management
        .begin_with_request(i.clone(), Some(prepared(&i).fingerprint().clone()))
        .unwrap();
    host.management
        .finish_result(&i.operation_id, Ok(result(&i)))
        .unwrap();
    let before = snapshot(&data);
    let view = host.snapshot();
    assert!(view.currency_budgets.is_empty());
    let gaps = view.currency_history.unwrap();
    assert_eq!(gaps.len(), 1);
    assert_eq!(
        gaps[0].reason,
        CurrencyHistoryReason::ManagementWithoutBudget
    );
    assert!(snapshot(&data) == before);
}

#[test]
fn currency_history_unattributed_blocks_availability_without_allocating_fees() {
    let temp = tempfile::tempdir().unwrap();
    let host = fixture(temp.path(), false);
    let before = snapshot(&temp.path().join("data"));
    let mut gaps = vec![CurrencyHistoryGap {
        project_id: None,
        source_id: "reviewed-original".into(),
        reason: CurrencyHistoryReason::ReviewedManagementRecord,
        reference: "management-decisions/reviewed-original.0.pending".into(),
    }];
    assert_eq!(
        host.currency_account_views(Some(&gaps))[0].available_usd_nanos,
        None
    );
    assert_eq!(
        host.currency_account_views(None)[0].available_usd_nanos,
        None
    );
    gaps[0].project_id = Some("another-project".into());
    assert!(host.currency_account_views(Some(&gaps))[0]
        .available_usd_nanos
        .is_some());
    assert!(snapshot(&temp.path().join("data")) == before);
    let mut old = serde_json::to_value(host.snapshot()).unwrap();
    old.as_object_mut().unwrap().remove("currencyHistory");
    let old: Snapshot = serde_json::from_value(old).unwrap();
    assert!(old.currency_history.is_none());
}

#[test]
fn historical_charge_host_requires_original_authenticated_confirmation_before_adoption() {
    use crate::currency_budget::{HistoricalCharge, HistoricalScope};
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = fixture(temp.path(), false);
    let book = host.currency_accounts.get_mut("project").unwrap();
    let charge = HistoricalCharge {
        project_id: "project".into(),
        scope: HistoricalScope::Management {
            operation_id: "unlinked-old-call".into(),
            original_project_id: Some("project".into()),
        },
        source_digest: "a".repeat(64),
        policy_digest: digest(book.policy()).unwrap(),
        previous_ledger_head: book.head().into(),
        amount_usd_nanos: 1,
        billing_reference: "Synthetic local fixture".into(),
        explanation: "No real user event; must not be adopted by host".into(),
        user_event_digest: "b".repeat(64),
    };
    book.record_historical_charge(charge).unwrap();
    drop(host);
    let before = snapshot(&data.join("currency-budgets"));
    let error = RuntimeHost::open(&data).err().unwrap();
    assert!(
        error.contains("original user-event reconciliation"),
        "{error}"
    );
    assert!(snapshot(&data.join("currency-budgets")) == before);
}
