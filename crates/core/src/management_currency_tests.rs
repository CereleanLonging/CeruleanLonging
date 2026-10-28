use super::*;
#[path = "api_inventory_tests.rs"]
mod api_inventory;
#[path = "api_currency_host_tests.rs"]
mod api_scoped_host;
#[path = "management_count_ledger_tests.rs"]
mod count_ledger;
#[path = "count_resolution_tests.rs"]
mod count_resolution;
#[path = "historical_confirmation_tests.rs"]
mod historical_confirmation;
#[path = "currency_history_tests.rs"]
mod history;
#[path = "management_preflight_host_tests.rs"]
mod host_preflight;
#[path = "observation_batch_tests.rs"]
mod observation_batch;
#[path = "management_scoped_count_tests.rs"]
mod scoped_count;
#[path = "management_scoped_decision_tests.rs"]
pub(super) mod scoped_decision;
#[path = "management_scoped_host_tests.rs"]
mod scoped_host;
use crate::{
    currency_budget::Outcome,
    management_execution::{DecisionIntent, UserResolution},
    reader_generation::tests::snapshot,
};
use cerulean_adapters::{
    api_generation::ApiUsage,
    openai::{DecisionObservation, DecisionResult, DecisionTransport, PreparedDecision},
};
use cerulean_domain::{
    api_worker::ApiTarget,
    currency_budget::*,
    management::{DecisionEnvelope, ManagementDecision, Purpose},
    project_setup::ProjectWorkerAdapter,
};

fn fixture(root: &Path, execution: bool) -> RuntimeHost {
    fixture_with_budget(root, execution, currency_product_tests::settings())
}
fn fixture_with_budget(root: &Path, execution: bool, settings: BudgetSettings) -> RuntimeHost {
    let (host, original) = project_planning::tests::fixture(root);
    drop(host);
    crate::currency_guard::tests::promote_fixture(&root.join("data"));
    let mut host = RuntimeHost::open(&root.join("data")).unwrap();
    let mut draft =
        api_project_tests::draft_for(original, ProjectWorkerAdapter::Openai, ApiTarget::OpenAi {});
    draft.currency_budget = Some(settings);
    host.configure_project(draft).unwrap();
    if execution {
        let mut proposal = project_planning::tests::proposal("tests");
        if let ManagementDecision::Plan { tasks, .. } = &mut proposal {
            tasks[0].worker_id = ProjectWorkerAdapter::Openai.worker_id();
        }
        project_planning::tests::complete(&mut host, proposal);
        let digest = host.projects["project"]
            .revision()
            .unwrap()
            .approval_digest()
            .unwrap();
        host.approve("project", &digest).unwrap();
    } else {
        let saved = host.project_setups["project"].digest.clone();
        host.authorize_planning_budget("project", &saved).unwrap();
    }
    host
}
fn intent(host: &RuntimeHost, id: &str, purpose: Purpose) -> DecisionIntent {
    let context = host
        .management_context("project", "Approved fixture scope")
        .unwrap();
    let setup = &host.project_setups["project"].setup;
    DecisionIntent {
        operation_id: id.into(),
        project_id: "project".into(),
        purpose,
        model: match purpose {
            Purpose::Plan | Purpose::Accept => setup.chief_model.clone(),
            _ => setup.deputy_model.clone(),
        },
        state_digest: digest(&context).unwrap(),
        context,
        created_at: chrono::Utc::now().to_rfc3339(),
    }
}
fn prepared(i: &DecisionIntent) -> PreparedDecision {
    PreparedDecision::new(i.purpose, &i.model, &i.state_digest, &i.context).unwrap()
}
fn reservation(host: &RuntimeHost, i: &DecisionIntent) -> Reservation {
    let quote = host.currency_accounts["project"]
        .policy()
        .quotes
        .iter()
        .find(|q| q.requested_model == i.model)
        .unwrap();
    Reservation {
        pricing: None,
        request_id: i.operation_id.clone(),
        operation_id: i.operation_id.clone(),
        role: match i.purpose {
            Purpose::Plan | Purpose::Accept => ChargeRole::Chief,
            _ => ChargeRole::Deputy,
        },
        quote_digest: digest(quote).unwrap(),
        request_sha256: prepared(i).fingerprint().body_sha256.clone(),
        ceiling: TokenCeiling {
            total_input: 100,
            output: 4096,
            cache_read_allowed: false,
            cache_write_allowed: true,
        },
        ceiling_evidence_sha256: "a".repeat(64),
    } // Explicitly synthetic ceiling; no provider call.
}
fn result(i: &DecisionIntent) -> DecisionResult {
    DecisionResult {
        observation: DecisionObservation {
            response_id: format!("resp_{}", i.operation_id.replace('-', "_")),
            requested_model: i.model.clone(),
            actual_model: i.model.clone(),
            input_tokens: 10,
            output_tokens: 2,
            envelope: DecisionEnvelope {
                state_digest: i.state_digest.clone(),
                decision: ManagementDecision::Wait {
                    reason: "Fixture decision".into(),
                },
            },
        },
        transport: Some(DecisionTransport {
            request: prepared(i).fingerprint().clone(),
            usage: ApiUsage {
                input_tokens: 10,
                output_tokens: 2,
                cache_read_tokens: Some(0),
                cache_write_tokens: Some(0),
            },
        }),
    }
}
fn begin(host: &mut RuntimeHost, i: &DecisionIntent) {
    let r = reservation(host, i);
    host.currency_accounts
        .get_mut("project")
        .unwrap()
        .reserve_management(i, &prepared(i), r)
        .unwrap();
    host.management
        .begin_with_request(i.clone(), Some(prepared(i).fingerprint().clone()))
        .unwrap();
}

fn prepared_fixture(root: &Path) -> RuntimeHost {
    let mut settings = currency_product_tests::settings();
    for quote in &mut settings.quotes {
        quote.rates.cache_read = Some(0);
    }
    drop(fixture_with_budget(root, false, settings));
    crate::currency_execution_guard::tests::promote_fixture(&root.join("data"));
    RuntimeHost::open(&root.join("data")).unwrap()
}
#[test]
#[ignore = "Owned reader5 maintenance fixtures; invokes no model or user desktop"]
fn export_reader5_preparation_fixture() {
    let root = PathBuf::from(std::env::var_os("CERULEAN_PREPARATION_FIXTURES").unwrap());
    assert!(!root.exists());
    fs::create_dir_all(&root).unwrap();
    let mut cases = vec![];
    for mode in ["cli", "window"] {
        let folder = root.join(mode);
        fs::create_dir(&folder).unwrap();
        let mut settings = currency_product_tests::settings();
        for quote in &mut settings.quotes {
            quote.rates.cache_read = Some(0);
        }
        drop(fixture_with_budget(&folder, false, settings));
        let data = folder.join("data");
        let archive = folder.join("cold-backup");
        let (manifest, hold) = crate::reader_generation::tests::held(&data, &archive);
        cases.push(serde_json::json!({"mode":mode,"data":data,"backup":archive,"manifest":manifest,"hold":hold}));
    }
    fs::write(
        root.join("cases.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}
fn prepare(host: &mut RuntimeHost, i: &DecisionIntent) -> u64 {
    let counted = cerulean_adapters::openai::fixture_counted_decision(prepared(i), 10).unwrap();
    let quote = host.currency_accounts["project"]
        .policy()
        .quotes
        .iter()
        .find(|q| q.requested_model == i.model)
        .unwrap();
    let quote = digest(quote).unwrap();
    host.currency_accounts
        .get_mut("project")
        .unwrap()
        .prepare_counted_management(i, &counted, &quote)
        .unwrap()
}

#[test]
fn prepared_management_recovers_reservation_before_original_intent_without_refund_or_resend() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = prepared_fixture(temp.path());
    let i = intent(&host, "prepared-before-intent", Purpose::Plan);
    let reserved = prepare(&mut host, &i);
    assert!(reserved > 0);
    assert!(host.management.intent(&i.operation_id).is_err());
    let original = snapshot(&data);
    drop(host);
    let mut host = RuntimeHost::open(&data).unwrap();
    let state = host.currency_accounts["project"].state();
    assert_eq!(state.reserved_usd_nanos, reserved);
    assert_eq!(state.charged_usd_nanos, 0);
    assert!(matches!(
        state.bookings[&i.operation_id].outcome,
        Outcome::Unknown { .. }
    ));
    assert!(state.bookings[&i.operation_id]
        .management_preparation
        .is_some());
    assert!(host.management.intent(&i.operation_id).is_err());
    assert!(host.management_active.is_empty());
    let after = snapshot(&data);
    for (p, b) in original {
        assert_eq!(after.get(&p), Some(&b));
    }
    let before = snapshot(&data);
    let counted = cerulean_adapters::openai::fixture_counted_decision(prepared(&i), 10).unwrap();
    let quote = digest(&host.currency_accounts["project"].policy().quotes[0]).unwrap();
    let book = host.currency_accounts.get_mut("project").unwrap();
    assert!(book
        .prepare_counted_management(&i, &counted, &quote)
        .is_err());
    assert!(book.not_sent(&i.operation_id, &"a".repeat(64)).is_err());
    assert_eq!(snapshot(&data), before);
    drop(host);
    let host = RuntimeHost::open(&data).unwrap();
    assert_eq!(snapshot(&data), before);
    assert!(host.management_active.is_empty());
}

#[test]
fn prepared_management_original_intent_and_reply_remain_authoritative() {
    for reply in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let mut host = prepared_fixture(temp.path());
        let i = intent(&host, "prepared-original", Purpose::Plan);
        let reserved = prepare(&mut host, &i);
        host.management
            .begin_with_request(i.clone(), Some(prepared(&i).fingerprint().clone()))
            .unwrap();
        if reply {
            host.management
                .finish_result(&i.operation_id, Ok(result(&i)))
                .unwrap();
        }
        drop(host);
        let host = RuntimeHost::open(&data).unwrap();
        let state = host.currency_accounts["project"].state();
        if reply {
            assert!(matches!(
                state.bookings[&i.operation_id].outcome,
                Outcome::Settled { .. }
            ));
            assert_eq!(state.reserved_usd_nanos, 0);
            assert!(state.charged_usd_nanos > 0);
        } else {
            assert!(matches!(
                state.bookings[&i.operation_id].outcome,
                Outcome::Unknown { .. }
            ));
            assert_eq!(state.reserved_usd_nanos, reserved);
        }
        drop(host);
        // Loss of previously observed originals must not be excused by the
        // earlier preparation or mistaken for the pre-intent interruption.
        for n in 0..=u8::from(reply) {
            fs::rename(
                data.join(format!(
                    "management-decisions/{}.{}.json",
                    i.operation_id, n
                )),
                temp.path().join(format!("retained-{n}")),
            )
            .unwrap();
        }
        let before = snapshot(&data);
        assert!(RuntimeHost::open(&data).is_err());
        assert_eq!(snapshot(&data), before);
    }
}

#[test]
fn prepared_management_requires_reader5_and_valid_original_count_receipt() {
    let temp = tempfile::tempdir().unwrap();
    let mut settings = currency_product_tests::settings();
    for quote in &mut settings.quotes {
        quote.rates.cache_read = Some(0);
    }
    let mut host = fixture_with_budget(temp.path(), false, settings);
    let data = temp.path().join("data");
    let i = intent(&host, "unprotected-preparation", Purpose::Plan);
    let counted = cerulean_adapters::openai::fixture_counted_decision(prepared(&i), 10).unwrap();
    let quote = digest(&host.currency_accounts["project"].policy().quotes[0]).unwrap();
    let before = snapshot(&data);
    assert!(host
        .currency_accounts
        .get_mut("project")
        .unwrap()
        .prepare_counted_management(&i, &counted, &quote)
        .unwrap_err()
        .contains("reader5"));
    assert_eq!(snapshot(&data), before);
    drop(host);
    crate::currency_execution_guard::tests::promote_fixture(&data);
    let mut host = RuntimeHost::open(&data).unwrap();
    prepare(&mut host, &i);
    drop(host);
    let path = data.join("currency-budgets/project/00000002.json");
    let original = fs::read(&path).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
    value["body"]["event"]["preparation"]["count"]["inputTokens"] = 11.into();
    fs::write(&path, crate::currency_budget::fixture_rehash_record(value)).unwrap();
    let before = snapshot(&data);
    assert!(RuntimeHost::open(&data)
        .err()
        .unwrap()
        .contains("invalid archived management token count"));
    assert_eq!(snapshot(&data), before);
    fs::write(path, original).unwrap();
    fs::rename(
        data.join(crate::currency_execution_guard::MARKER),
        temp.path().join("retained-reader5"),
    )
    .unwrap();
    let before = snapshot(&data);
    assert!(RuntimeHost::open(&data).err().unwrap().contains("reader5"));
    assert_eq!(snapshot(&data), before);
}

#[test]
fn counted_management_reservation_uses_provider_count_and_original_journal_after_reopen() {
    for execution in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let mut settings = currency_product_tests::settings();
        for quote in &mut settings.quotes {
            quote.rates.cache_read = Some(2_000_000);
        }
        let mut host = fixture_with_budget(temp.path(), execution, settings);
        let purpose = if execution {
            Purpose::Assign
        } else {
            Purpose::Plan
        };
        let i = intent(&host, "counted-1", purpose);
        let counted =
            cerulean_adapters::openai::fixture_counted_decision(prepared(&i), 10).unwrap();
        let quote = host.currency_accounts["project"]
            .policy()
            .quotes
            .iter()
            .find(|q| q.requested_model == i.model)
            .unwrap()
            .clone();
        let reserved = host
            .currency_accounts
            .get_mut("project")
            .unwrap()
            .reserve_counted_management(&i, &counted, &digest(&quote).unwrap())
            .unwrap();
        assert_eq!(
            reserved,
            quote.rates.reserve(&counted.ceiling().unwrap()).unwrap()
        );
        let booking = &host.currency_accounts["project"].state().bookings[&i.operation_id];
        assert_eq!(booking.reservation.ceiling.total_input, 10);
        assert_eq!(
            booking.reservation.ceiling_evidence_sha256,
            digest(counted.evidence()).unwrap()
        );
        assert!(host.management.intent(&i.operation_id).is_err());
        assert!(host.management_active.is_empty());
        host.management
            .begin_with_request(i.clone(), Some(counted.prepared().fingerprint().clone()))
            .unwrap();
        host.management
            .finish_result(&i.operation_id, Ok(result(&i)))
            .unwrap();
        drop(host);
        let host = RuntimeHost::open(&temp.path().join("data")).unwrap();
        let state = host.currency_accounts["project"].state();
        assert!(matches!(
            state.bookings[&i.operation_id].outcome,
            Outcome::Settled { .. }
        ));
        assert_eq!(state.reserved_usd_nanos, 0);
        assert!(state.charged_usd_nanos > 0);
        let before = snapshot(&temp.path().join("data"));
        drop(host);
        let host = RuntimeHost::open(&temp.path().join("data")).unwrap();
        assert_eq!(snapshot(&temp.path().join("data")), before);
        assert!(host.management_active.is_empty());
    }
}

#[test]
fn counted_management_rejects_changed_request_wrong_quote_unknown_cache_and_exhausted_budget() {
    for mode in ["context", "quote", "cache", "budget"] {
        let temp = tempfile::tempdir().unwrap();
        let mut settings = currency_product_tests::settings();
        if mode != "cache" {
            for quote in &mut settings.quotes {
                quote.rates.cache_read = Some(0);
            }
        }
        if mode == "budget" {
            settings.limit_usd_nanos = 1;
        }
        let mut host = fixture_with_budget(temp.path(), false, settings);
        let i = intent(&host, "counted-refused", Purpose::Plan);
        let p = if mode == "context" {
            let mut changed = i.clone();
            changed.context["requirement"] = serde_json::json!("changed after counting");
            changed.state_digest = digest(&changed.context).unwrap();
            prepared(&changed)
        } else {
            prepared(&i)
        };
        let counted = cerulean_adapters::openai::fixture_counted_decision(p, 10).unwrap();
        let quote = host.currency_accounts["project"]
            .policy()
            .quotes
            .iter()
            .find(|q| {
                if mode == "quote" {
                    q.requested_model != i.model
                } else {
                    q.requested_model == i.model
                }
            })
            .unwrap();
        let quote_digest = digest(quote).unwrap();
        let before = snapshot(&temp.path().join("data"));
        let head = host.currency_accounts["project"].head().to_owned();
        assert!(
            host.currency_accounts
                .get_mut("project")
                .unwrap()
                .reserve_counted_management(&i, &counted, &quote_digest)
                .is_err(),
            "{mode}"
        );
        assert_eq!(host.currency_accounts["project"].head(), head);
        assert_eq!(snapshot(&temp.path().join("data")), before);
        assert!(host.management.intent(&i.operation_id).is_err());
        assert!(host.management_active.is_empty());
    }
}

#[test]
fn call_recovery_management_settles_original_reply_once_even_after_local_discard() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = fixture(temp.path(), false);
    let i = intent(&host, "billing-1", Purpose::Plan);
    begin(&mut host, &i);
    host.management
        .finish_result(&i.operation_id, Ok(result(&i)))
        .unwrap();
    let response = host
        .management
        .response_head(&i.operation_id)
        .unwrap()
        .to_owned();
    host.management
        .resolve(
            &i.operation_id,
            &response,
            UserResolution::Discard {
                reason: "Close decision only".into(),
            },
        )
        .unwrap();
    assert_eq!(
        host.currency_accounts["project"].state().charged_usd_nanos,
        0
    );
    drop(host);
    let host = RuntimeHost::open(&data).unwrap();
    let state = host.currency_accounts["project"].state();
    assert!(
        matches!(&state.bookings[&i.operation_id].outcome,Outcome::Settled{settlement,..} if settlement.evidence_sha256==response)
    );
    assert!(state.charged_usd_nanos > 0);
    assert_eq!(state.reserved_usd_nanos, 0);
    let charged = state.charged_usd_nanos;
    let before = snapshot(&data);
    drop(host);
    let host = RuntimeHost::open(&data).unwrap();
    assert_eq!(
        host.currency_accounts["project"].state().charged_usd_nanos,
        charged
    );
    assert_eq!(snapshot(&data), before);
    assert!(host.management_active.is_empty());
}
#[test]
fn call_recovery_management_missing_usage_or_unknown_never_releases_reservation() {
    for mode in ["usage", "failure", "inflight"] {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let mut host = fixture(temp.path(), false);
        let i = intent(&host, "billing-unknown", Purpose::Plan);
        begin(&mut host, &i);
        let reserved = host.currency_accounts["project"].state().reserved_usd_nanos;
        if mode == "usage" {
            let mut r = result(&i);
            r.transport.as_mut().unwrap().usage.cache_write_tokens = None;
            host.management
                .finish_result(&i.operation_id, Ok(r))
                .unwrap();
        }
        if mode == "failure" {
            host.management
                .finish_result(
                    &i.operation_id,
                    Err("untrusted error must not escape".into()),
                )
                .unwrap();
        }
        drop(host);
        let host = RuntimeHost::open(&data).unwrap();
        let state = host.currency_accounts["project"].state();
        assert_eq!(state.reserved_usd_nanos, reserved);
        assert_eq!(state.charged_usd_nanos, 0);
        assert!(matches!(
            state.bookings[&i.operation_id].outcome,
            Outcome::Unknown { .. }
        ));
        let before = snapshot(&data);
        drop(host);
        RuntimeHost::open(&data).unwrap();
        assert_eq!(snapshot(&data), before);
    }
}
#[test]
fn call_recovery_management_permission_rejects_other_purpose_role_model_and_prepared_body() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), false);
    let i = intent(&host, "billing-scope", Purpose::Plan);
    let before = snapshot(&temp.path().join("data"));
    let mut role = reservation(&host, &i);
    role.role = ChargeRole::Worker;
    assert!(host
        .currency_accounts
        .get_mut("project")
        .unwrap()
        .reserve_management(&i, &prepared(&i), role)
        .is_err());
    for purpose in [Purpose::Assign, Purpose::Accept, Purpose::Advise] {
        let changed = intent(&host, "wrong-purpose", purpose);
        let r = reservation(&host, &changed);
        assert!(host
            .currency_accounts
            .get_mut("project")
            .unwrap()
            .reserve_management(&changed, &prepared(&changed), r)
            .is_err());
    }
    let mut changed = i.clone();
    changed.model = "fixture-deputy".into();
    let r = reservation(&host, &changed);
    assert!(host
        .currency_accounts
        .get_mut("project")
        .unwrap()
        .reserve_management(&changed, &prepared(&changed), r)
        .is_err());
    let mut other = i.clone();
    other.context["requirement"] = "changed".into();
    other.state_digest = digest(&other.context).unwrap();
    let r = reservation(&host, &i);
    assert!(host
        .currency_accounts
        .get_mut("project")
        .unwrap()
        .reserve_management(&i, &prepared(&other), r)
        .is_err());
    assert_eq!(snapshot(&temp.path().join("data")), before);
}
#[test]
fn call_recovery_management_execution_uses_original_approval_and_configured_role_model() {
    for purpose in [Purpose::Assign, Purpose::Accept, Purpose::Advise] {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let mut host = fixture(temp.path(), true);
        let i = intent(&host, "execution-billing", purpose);
        begin(&mut host, &i);
        host.management
            .finish_result(&i.operation_id, Ok(result(&i)))
            .unwrap();
        drop(host);
        let host = RuntimeHost::open(&data).unwrap();
        assert!(host.currency_accounts["project"].state().charged_usd_nanos > 0);
        let id = host.currency_accounts["project"].state().bookings[&i.operation_id]
            .management_identity
            .as_ref()
            .unwrap();
        assert_eq!(
            id.revision_digest.as_ref(),
            Some(&host.currency_accounts["project"].policy().revision_digest)
        );
    }
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), true);
    let i = intent(&host, "wrong-plan", Purpose::Plan);
    let r = reservation(&host, &i);
    assert!(host
        .currency_accounts
        .get_mut("project")
        .unwrap()
        .reserve_management(&i, &prepared(&i), r)
        .is_err());
}
#[test]
fn call_recovery_management_missing_original_request_keeps_account_and_refuses_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = fixture(temp.path(), false);
    let i = intent(&host, "missing-intent", Purpose::Plan);
    let r = reservation(&host, &i);
    host.currency_accounts
        .get_mut("project")
        .unwrap()
        .reserve_management(&i, &prepared(&i), r)
        .unwrap();
    drop(host);
    let before = snapshot(&data);
    assert!(RuntimeHost::open(&data).is_err());
    assert_eq!(snapshot(&data), before);
    // Preserve the reservation-first interruption. Recovery is not authorization
    // to recreate the absent intent, to refund, or to send the request.
}
#[test]
fn call_recovery_management_journal_identity_mismatch_cannot_charge_or_clear_reservation() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), false);
    let i = intent(&host, "different-intent", Purpose::Plan);
    let r = reservation(&host, &i);
    host.currency_accounts
        .get_mut("project")
        .unwrap()
        .reserve_management(&i, &prepared(&i), r)
        .unwrap();
    let mut changed = i;
    changed.context["requirement"] = "changed".into();
    changed.state_digest = digest(&changed.context).unwrap();
    host.management
        .begin_with_request(
            changed.clone(),
            Some(prepared(&changed).fingerprint().clone()),
        )
        .unwrap();
    host.management
        .finish_result(&changed.operation_id, Ok(result(&changed)))
        .unwrap();
    let data = temp.path().join("data");
    let before = snapshot(&data);
    drop(host);
    assert!(RuntimeHost::open(&data).is_err());
    assert_eq!(snapshot(&data), before);
}

#[test]
fn call_recovery_management_intent_detects_missing_reservation_even_with_valid_account_prefix() {
    for truncate in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let mut host = fixture(temp.path(), false);
        let i = intent(&host, "unreserved-original", Purpose::Plan);
        if truncate {
            let r = reservation(&host, &i);
            host.currency_accounts
                .get_mut("project")
                .unwrap()
                .reserve_management(&i, &prepared(&i), r)
                .unwrap();
        }
        host.management
            .begin_with_request(i.clone(), Some(prepared(&i).fingerprint().clone()))
            .unwrap();
        drop(host);
        if truncate {
            fs::rename(
                data.join("currency-budgets/project/00000002.json"),
                temp.path().join("retained-reservation"),
            )
            .unwrap();
        }
        let before = snapshot(&data);
        let error = RuntimeHost::open(&data)
            .err()
            .expect("unreserved original request must block recovery");
        assert!(error.contains("no reservation"), "{error}");
        assert_eq!(snapshot(&data), before);
    }
}
#[test]
fn call_recovery_management_unpriced_model_or_cache_keeps_original_charge_evidence_and_blocks() {
    for bad_model in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let mut host = fixture(temp.path(), false);
        let i = intent(&host, "unpriced-original", Purpose::Plan);
        begin(&mut host, &i);
        let reserved = host.currency_accounts["project"].state().reserved_usd_nanos;
        let mut r = result(&i);
        if bad_model {
            r.observation.actual_model = "unpriced-model".into();
        } else {
            r.transport.as_mut().unwrap().usage.cache_read_tokens = Some(3);
        }
        host.management
            .finish_result(&i.operation_id, Ok(r))
            .unwrap();
        drop(host);
        let host = RuntimeHost::open(&data).unwrap();
        let state = host.currency_accounts["project"].state();
        assert!(state.blocked);
        assert_eq!(state.reserved_usd_nanos, reserved);
        assert!(matches!(
            state.bookings[&i.operation_id].outcome,
            Outcome::Unpriced { .. }
        ));
        let before = snapshot(&data);
        drop(host);
        RuntimeHost::open(&data).unwrap();
        assert_eq!(snapshot(&data), before);
    }
}
#[test]
fn call_recovery_management_complete_pending_response_publishes_before_currency_settlement() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = fixture(temp.path(), false);
    let i = intent(&host, "pending-original", Purpose::Plan);
    begin(&mut host, &i);
    host.management
        .finish_result(&i.operation_id, Ok(result(&i)))
        .unwrap();
    drop(host);
    let file = data.join("management-decisions/pending-original.1.json");
    let pending = data.join("management-decisions/pending-original.1.pending");
    let original = fs::read(&file).unwrap();
    fs::rename(&file, &pending).unwrap();
    let host = RuntimeHost::open(&data).unwrap();
    assert_eq!(fs::read(&file).unwrap(), original);
    assert!(!pending.exists());
    assert!(matches!(
        host.currency_accounts["project"].state().bookings[&i.operation_id].outcome,
        Outcome::Settled { .. }
    ));
    let before = snapshot(&data);
    drop(host);
    RuntimeHost::open(&data).unwrap();
    assert_eq!(snapshot(&data), before);
}
