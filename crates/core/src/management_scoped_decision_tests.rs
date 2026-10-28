use super::scoped_count as sc;
use super::*;
use crate::management_execution::ApplicationTarget;
use cerulean_adapters::openai::scoped_pricing::{fixture_scoped_result, PreparedScopedDecision};
use serde_json::{json, Value};

pub(in crate::host) fn settings() -> BudgetSettings {
    let mut s = sc::settings(Some(1000), 12_345_678_901, 256 * 1024);
    for q in s.scoped_quotes.as_mut().unwrap() {
        for b in &mut q.bands {
            b.rates.cache_read = Some(1);
            b.rates.cache_write = Some(2);
        }
    }
    s
}
fn fixture(root: &Path, purpose: Purpose) -> RuntimeHost {
    let h = sc::fixture(root, purpose != Purpose::Plan, settings(), true);
    drop(h);
    crate::currency_scoped_execution_guard::tests::promote_fixture(&root.join("data"));
    RuntimeHost::open(&root.join("data")).unwrap()
}
fn prepare(
    h: &mut RuntimeHost,
    id: &str,
    purpose: Purpose,
) -> (DecisionIntent, PreparedScopedDecision) {
    let i = intent(h, id, purpose);
    let c = sc::counted(h, &i);
    sc::begin(h, &i).unwrap();
    let b = h.currency_accounts.get_mut("project").unwrap();
    b.record_scoped_management_count(id, &c).unwrap();
    let p = c.prepare().unwrap();
    b.prepare_scoped_management(&i, &p).unwrap();
    (i, p)
}
pub(super) fn response(i: &DecisionIntent) -> Value {
    json!({"id":format!("resp_{}",i.operation_id.replace('-',"_")),"model":i.model,"status":"completed","service_tier":"default",
        "output":[{"type":"reasoning","summary":[{"text":"PRIVATE_SCOPED_FIXTURE"}]},{"type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":serde_json::to_string(&json!({"stateDigest":i.state_digest,"decision":{"action":"wait","reason":"bounded fixture"}})).unwrap()}]}],
        "usage":{"input_tokens":10,"output_tokens":20,"input_tokens_details":{"cached_tokens":3,"cache_write_tokens":2}}})
}
fn finish(h: &mut RuntimeHost, i: &DecisionIntent, p: &PreparedScopedDecision, value: Value) {
    h.management
        .finish_scoped_result(
            &i.operation_id,
            Ok(fixture_scoped_result(p, value).unwrap()),
        )
        .unwrap();
}
fn rehash(path: &Path, change: impl FnOnce(&mut Value)) {
    let mut v: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    change(&mut v);
    v["hash"] = json!(digest(&v["body"]).unwrap());
    std::fs::write(path, serde_json::to_vec(&v).unwrap()).unwrap();
}
#[test]
fn scoped_decision_four_purposes_share_original_fee_and_ack_before_application() {
    for purpose in [
        Purpose::Plan,
        Purpose::Assign,
        Purpose::Accept,
        Purpose::Advise,
    ] {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let mut h = fixture(temp.path(), purpose);
        let (i, p) = prepare(&mut h, "scoped", purpose);
        h.management.begin_scoped(i.clone(), &p).unwrap();
        assert!(h
            .management
            .finish_result(&i.operation_id, Err("legacy route".into()))
            .is_err());
        finish(&mut h, &i, &p, response(&i));
        assert!(h.management_fee_ack(&i.operation_id).is_err());
        assert!(h
            .management
            .prepare_application(&i.operation_id, ApplicationTarget::Wait, &i.state_digest)
            .is_err());
        h.reconcile_currency_account("project").unwrap();
        let ack = h.management_fee_ack(&i.operation_id).unwrap().unwrap();
        let b = &h.currency_accounts["project"];
        assert!(matches!(
            b.state().bookings[&i.operation_id].outcome,
            Outcome::Settled { .. }
        ));
        assert_eq!(b.state().reserved_usd_nanos, 1000);
        assert!(b.state().charged_usd_nanos > 0);
        let before = snapshot(&data);
        h.reconcile_currency_account("project").unwrap();
        assert_eq!(before, snapshot(&data));
        h.management
            .prepare_application_with_fee(
                &i.operation_id,
                ApplicationTarget::Wait,
                &i.state_digest,
                Some(ack.clone()),
            )
            .unwrap();
        h.management.record_applied(&i.operation_id, None).unwrap();
        let raw = std::fs::read_to_string(data.join("management-decisions/scoped.1.json")).unwrap();
        assert!(!raw.contains("PRIVATE_SCOPED_FIXTURE"));
        let before = snapshot(&data);
        drop(h);
        let h = RuntimeHost::open(&data).unwrap();
        assert_eq!(snapshot(&data), before);
        assert_eq!(h.management_fee_ack(&i.operation_id).unwrap(), Some(ack));
        assert!(h.management_active.is_empty());
    }
}
#[test]
fn scoped_decision_invalid_decision_still_charges_but_cannot_apply() {
    for mode in ["refusal", "bad-envelope", "tool", "mismatch"] {
        let temp = tempfile::tempdir().unwrap();
        let mut h = fixture(temp.path(), Purpose::Assign);
        let (i, p) = prepare(&mut h, "invalid", Purpose::Assign);
        h.management.begin_scoped(i.clone(), &p).unwrap();
        let mut v = response(&i);
        match mode {
            "refusal" => v["output"][1]["content"][0] = json!({"type":"refusal","refusal":"no"}),
            "bad-envelope" => v["output"][1]["content"][0]["text"] = json!("{}"),
            "tool" => v["output"]
                .as_array_mut()
                .unwrap()
                .push(json!({"type":"function_call","name":"shell"})),
            _ => {}
        }
        let mut r = fixture_scoped_result(&p, v).unwrap();
        if mode == "mismatch" {
            r.decision.as_mut().unwrap().input_tokens += 1;
        }
        h.management
            .finish_scoped_result(&i.operation_id, Ok(r))
            .unwrap();
        h.reconcile_currency_account("project").unwrap();
        assert!(matches!(
            h.currency_accounts["project"].state().bookings[&i.operation_id].outcome,
            Outcome::Settled { .. }
        ));
        assert!(h.management.observation(&i.operation_id).is_err());
        let ack = h.management_fee_ack(&i.operation_id).unwrap();
        assert!(h
            .management
            .prepare_application_with_fee(
                &i.operation_id,
                ApplicationTarget::Wait,
                &i.state_digest,
                ack
            )
            .is_err());
        drop(h);
        RuntimeHost::open(&temp.path().join("data")).unwrap();
    }
}
#[test]
fn scoped_decision_missing_usage_or_tier_retains_liability_and_denies_ack() {
    for mode in ["usage", "tier", "cache", "outside"] {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let mut h = fixture(temp.path(), Purpose::Assign);
        let (i, p) = prepare(&mut h, "unknown", Purpose::Assign);
        h.management.begin_scoped(i.clone(), &p).unwrap();
        let mut v = response(&i);
        match mode {
            "usage" => {
                v.as_object_mut().unwrap().remove("usage");
            }
            "tier" => {
                v.as_object_mut().unwrap().remove("service_tier");
            }
            "cache" => v["usage"]["input_tokens_details"] = json!({}),
            _ => v["usage"]["input_tokens"] = json!(101),
        }
        let reserved = h.currency_accounts["project"].state().reserved_usd_nanos;
        finish(&mut h, &i, &p, v);
        h.reconcile_currency_account("project").unwrap();
        assert!(h.management_fee_ack(&i.operation_id).is_err());
        assert_eq!(
            h.currency_accounts["project"].state().reserved_usd_nanos,
            reserved
        );
        assert!(matches!(
            h.currency_accounts["project"].state().bookings[&i.operation_id].outcome,
            Outcome::Unknown { .. } | Outcome::Unpriced { .. }
        ));
        let before = snapshot(&data);
        drop(h);
        let h = RuntimeHost::open(&data).unwrap();
        assert_eq!(snapshot(&data), before);
        assert!(h.management_active.is_empty());
    }
}
#[test]
fn scoped_decision_preparation_intent_and_response_gaps_never_refund_or_resend() {
    for phase in ["preparation", "intent", "response"] {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let mut h = fixture(temp.path(), Purpose::Assign);
        let (i, p) = prepare(&mut h, "interrupted", Purpose::Assign);
        if phase != "preparation" {
            h.management.begin_scoped(i.clone(), &p).unwrap();
        }
        if phase == "response" {
            finish(&mut h, &i, &p, response(&i));
        }
        drop(h);
        let h = RuntimeHost::open(&data).unwrap();
        let b = &h.currency_accounts["project"];
        if phase == "response" {
            assert!(matches!(
                b.state().bookings[&i.operation_id].outcome,
                Outcome::Settled { .. }
            ));
        } else {
            assert!(matches!(
                b.state().bookings[&i.operation_id].outcome,
                Outcome::Unknown { .. }
            ));
            assert!(b.state().reserved_usd_nanos > 1000);
        }
        assert!(h.management_active.is_empty());
        let before = snapshot(&data);
        drop(h);
        RuntimeHost::open(&data).unwrap();
        assert_eq!(before, snapshot(&data));
    }
}
#[test]
fn scoped_decision_fee_ack_uses_settlement_record_and_rejects_forged_restart() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut h = fixture(temp.path(), Purpose::Assign);
    let (i, p) = prepare(&mut h, "first", Purpose::Assign);
    h.management.begin_scoped(i.clone(), &p).unwrap();
    finish(&mut h, &i, &p, response(&i));
    h.reconcile_currency_account("project").unwrap();
    let ack = h.management_fee_ack(&i.operation_id).unwrap().unwrap();
    let j = intent(&h, "other", Purpose::Assign);
    sc::begin(&mut h, &j).unwrap();
    assert_ne!(h.currency_accounts["project"].head(), ack.settlement_head);
    assert_eq!(
        h.management_fee_ack(&i.operation_id).unwrap(),
        Some(ack.clone())
    );
    h.management
        .prepare_application_with_fee(
            &i.operation_id,
            ApplicationTarget::Wait,
            &i.state_digest,
            Some(ack),
        )
        .unwrap();
    drop(h);
    rehash(&data.join("management-decisions/first.2.json"), |v| {
        v["body"]["evidence"]["currencyAck"]["settlementHead"] = json!("a".repeat(64))
    });
    let before = snapshot(&data);
    assert!(RuntimeHost::open(&data).is_err());
    assert_eq!(before, snapshot(&data));
}
#[test]
fn scoped_decision_request_quote_and_preparation_tampering_refuse_without_writes() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut h = fixture(temp.path(), Purpose::Assign);
    let (i, p) = prepare(&mut h, "bound", Purpose::Assign);
    h.management.begin_scoped(i.clone(), &p).unwrap();
    for mode in [0, 1] {
        let mut r = fixture_scoped_result(&p, response(&i)).unwrap();
        if mode == 0 {
            r.request.body_sha256 = "a".repeat(64);
        } else {
            r.review_sha256 = "a".repeat(64);
        }
        let before = snapshot(&data);
        assert!(h
            .management
            .finish_scoped_result(&i.operation_id, Ok(r))
            .is_err());
        assert_eq!(before, snapshot(&data));
    }
    finish(&mut h, &i, &p, response(&i));
    h.reconcile_currency_account("project").unwrap();
    drop(h);
    let paths: Vec<_> = std::fs::read_dir(data.join("currency-budgets/project"))
        .unwrap()
        .map(|p| p.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    let path = paths
        .iter()
        .find(|p| {
            std::fs::read_to_string(p)
                .unwrap()
                .contains("scopedPreparation")
        })
        .unwrap();
    rehash(path, |v| {
        v["body"]["event"]["scopedPreparation"]["audit"]["reviewedAt"] =
            json!("2000-01-01T00:00:00Z")
    });
    let before = snapshot(&data);
    assert!(RuntimeHost::open(&data).is_err());
    assert_eq!(before, snapshot(&data));
}
#[test]
fn scoped_decision_reader9_refuses_new_writes_and_reader10_removal_refuses_recovery() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut h = sc::fixture(temp.path(), true, settings(), true);
    let i = intent(&h, "guarded", Purpose::Assign);
    let c = sc::counted(&h, &i);
    sc::begin(&mut h, &i).unwrap();
    h.currency_accounts
        .get_mut("project")
        .unwrap()
        .record_scoped_management_count(&i.operation_id, &c)
        .unwrap();
    let p = c.prepare().unwrap();
    let before = snapshot(&data);
    assert!(h
        .currency_accounts
        .get_mut("project")
        .unwrap()
        .prepare_scoped_management(&i, &p)
        .is_err());
    assert!(h.management.begin_scoped(i.clone(), &p).is_err());
    assert_eq!(before, snapshot(&data));
    drop(h);
    crate::currency_scoped_execution_guard::tests::promote_fixture(&data);
    let mut h = RuntimeHost::open(&data).unwrap();
    h.currency_accounts
        .get_mut("project")
        .unwrap()
        .prepare_scoped_management(&i, &p)
        .unwrap();
    h.management.begin_scoped(i, &p).unwrap();
    drop(h);
    std::fs::remove_file(data.join(crate::currency_scoped_execution_guard::MARKER)).unwrap();
    let before = snapshot(&data);
    assert!(RuntimeHost::open(&data).is_err());
    assert_eq!(before, snapshot(&data));
}

#[test]
fn scoped_decision_pending_original_response_and_application_recover_once() {
    for phase in ["response", "application"] {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let mut h = fixture(temp.path(), Purpose::Assign);
        let (i, p) = prepare(&mut h, "pending", Purpose::Assign);
        h.management.begin_scoped(i.clone(), &p).unwrap();
        finish(&mut h, &i, &p, response(&i));
        let sequence = if phase == "application" {
            h.reconcile_currency_account("project").unwrap();
            let ack = h.management_fee_ack(&i.operation_id).unwrap();
            h.management
                .prepare_application_with_fee(
                    &i.operation_id,
                    ApplicationTarget::Wait,
                    &i.state_digest,
                    ack,
                )
                .unwrap();
            2
        } else {
            1
        };
        drop(h);
        let dir = data.join("management-decisions");
        std::fs::rename(
            dir.join(format!("pending.{sequence}.json")),
            dir.join(format!("pending.{sequence}.pending")),
        )
        .unwrap();
        let h = RuntimeHost::open(&data).unwrap();
        assert!(dir.join(format!("pending.{sequence}.json")).is_file());
        assert!(!dir.join(format!("pending.{sequence}.pending")).exists());
        assert!(matches!(
            h.currency_accounts["project"].state().bookings[&i.operation_id].outcome,
            Outcome::Settled { .. }
        ));
        assert!(h.management_fee_ack(&i.operation_id).is_ok());
        let before = snapshot(&data);
        drop(h);
        RuntimeHost::open(&data).unwrap();
        assert_eq!(before, snapshot(&data));
    }
}
#[test]
fn scoped_decision_duplicate_provider_response_refuses_second_charge() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut h = fixture(temp.path(), Purpose::Assign);
    let (i, p) = prepare(&mut h, "first", Purpose::Assign);
    h.management.begin_scoped(i.clone(), &p).unwrap();
    finish(&mut h, &i, &p, response(&i));
    h.reconcile_currency_account("project").unwrap();
    let ack = h.management_fee_ack(&i.operation_id).unwrap();
    h.management
        .prepare_application_with_fee(
            &i.operation_id,
            ApplicationTarget::Wait,
            &i.state_digest,
            ack,
        )
        .unwrap();
    h.management.record_applied(&i.operation_id, None).unwrap();
    let (j, p) = prepare(&mut h, "second", Purpose::Assign);
    h.management.begin_scoped(j.clone(), &p).unwrap();
    let mut value = response(&j);
    value["id"] = response(&i)["id"].clone();
    finish(&mut h, &j, &p, value);
    let before = snapshot(&data);
    assert!(h.reconcile_currency_account("project").is_err());
    assert_eq!(before, snapshot(&data));
    assert!(h.management_fee_ack(&j.operation_id).is_err());
}
