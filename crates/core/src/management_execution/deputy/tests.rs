use super::*;
use cerulean_adapters::{
    api_generation::ApiClient,
    deputy_management::{self, DeputyOutcome},
};
use cerulean_domain::{
    management_ingress::{ManagementHandoff, ManagementSource},
    secret::SecretInput,
};
use serde_json::json;
fn prepared(id: &str) -> (DecisionIntent, PreparedDeputy) {
    let context = json!({"requirement":"review only","supervision":{"taskId":"sum"}});
    let intent = DecisionIntent {
        operation_id: id.into(),
        project_id: "project".into(),
        purpose: Purpose::Advise,
        model: "deepseek-flash".into(),
        state_digest: digest(&context).unwrap(),
        context: context.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    let client =
        ApiClient::new(SecretInput::new("invalid-test-only-value".into()).unwrap()).unwrap();
    let p = deputy_management::prepare(
        &client,
        ManagementHandoff {
            operation_id: id.into(),
            project_id: "project".into(),
            purpose: intent.purpose,
            source: ManagementSource::DeepSeek {
                model: intent.model.clone(),
            },
            state_digest: intent.state_digest.clone(),
            context,
        },
    )
    .unwrap();
    (intent, p)
}
fn result(p: &PreparedDeputy, valid: bool) -> DeputyResult {
    let content = if valid {
        json!({"stateDigest":p.request().handoff.state_digest,"decision":{"action":"wait","reason":"need user-approved plan"}}).to_string()
    } else {
        "malformed decision".into()
    };
    deputy_management::fixture_result(
        p,
        json!({"id":"ds-real-shaped-reply","model":"deepseek-flash-version","usage":{"prompt_tokens":25,"completion_tokens":7,"prompt_cache_hit_tokens":3},"choices":[{"index":0,"finish_reason":"stop","message":{"role":"assistant","content":content}}]}),
    )
}
fn root() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    drop(crate::Store::open(&data).unwrap());
    crate::management_source_guard::tests::promote_fixture(&data);
    temp
}
#[test]
fn deputy_journal_keeps_provider_billing_and_wait_across_reopen_without_openai_receipts() {
    let tmp = root();
    let data = tmp.path().join("data");
    let (i, p) = prepared("deputy-wait");
    let r = result(&p, true);
    let mut j = DecisionJournal::open(&data).unwrap();
    j.begin_deputy(i.clone(), &p).unwrap();
    j.finish_deputy(&i.operation_id, r.clone()).unwrap();
    assert!(j.finish_deputy(&i.operation_id, r).is_err());
    assert!(j.billing_evidence(&i.operation_id).is_err());
    assert!(j.observation(&i.operation_id).is_err());
    j.prepare_application(&i.operation_id, ApplicationTarget::Wait, &i.state_digest)
        .unwrap();
    j.record_applied(&i.operation_id, None).unwrap();
    let old = j.evidence_digest(&i.operation_id).unwrap().to_owned();
    drop(j);
    let mut j = DecisionJournal::open(&data).unwrap();
    assert_eq!(j.statuses()[0].state, DecisionState::Applied);
    assert_eq!(j.deputy_evidence(&i.operation_id).unwrap().2, old);
    assert_eq!(
        j.deputy_evidence(&i.operation_id)
            .unwrap()
            .1
            .unwrap()
            .billing()
            .unwrap()
            .usage
            .input_tokens,
        25
    );
    assert!(j.begin_deputy(i.clone(), &p).is_err());
    let gaps = j.currency_history_gaps();
    assert_eq!(gaps.len(), 1);
    assert_eq!(
        j.historical_source(&i.operation_id, true)
            .unwrap()
            .uncertain_outcomes,
        0
    );
    let head = j.evidence_digest(&i.operation_id).unwrap().to_owned();
    j.resolve(
        &i.operation_id,
        &head,
        UserResolution::Reply {
            answer: "approval still separate".into(),
        },
    )
    .unwrap();
}
#[test]
fn deputy_invalid_decision_keeps_usage_but_cannot_apply_and_wrong_request_cannot_be_recorded() {
    let tmp = root();
    let data = tmp.path().join("data");
    let (i, p) = prepared("invalid");
    let mut j = DecisionJournal::open(&data).unwrap();
    j.begin_deputy(i.clone(), &p).unwrap();
    let (_, other) = prepared("other");
    assert!(j
        .finish_deputy(&i.operation_id, result(&other, true))
        .is_err());
    assert!(!data.join("management-decisions/invalid.1.json").exists());
    j.finish_deputy(&i.operation_id, result(&p, false)).unwrap();
    assert!(j.ready_envelope(&i.operation_id, &i.state_digest).is_err());
    assert_eq!(j.statuses()[0].state, DecisionState::Unknown);
    let head = j.evidence_digest(&i.operation_id).unwrap().to_owned();
    j.resolve(
        &i.operation_id,
        &head,
        UserResolution::Discard {
            reason: "invalid output; retain bill".into(),
        },
    )
    .unwrap();
    drop(j);
    let j = DecisionJournal::open(&data).unwrap();
    assert!(j
        .deputy_evidence(&i.operation_id)
        .unwrap()
        .1
        .unwrap()
        .billing()
        .is_some());
    assert_eq!(
        j.historical_source(&i.operation_id, true)
            .unwrap()
            .uncertain_outcomes,
        0
    );
}
#[test]
fn deputy_lost_response_stays_unknown_and_old_protocol_cannot_finish_it() {
    let tmp = root();
    let data = tmp.path().join("data");
    let (i, p) = prepared("lost");
    let mut j = DecisionJournal::open(&data).unwrap();
    j.begin_deputy(i.clone(), &p).unwrap();
    drop(j);
    let mut j = DecisionJournal::open(&data).unwrap();
    assert_eq!(j.statuses()[0].state, DecisionState::Unknown);
    assert!(j.begin_deputy(i.clone(), &p).is_err());
    assert!(j
        .finish_result(&i.operation_id, Err("wrong provider".into()))
        .is_err());
    assert!(j.deputy_evidence(&i.operation_id).unwrap().1.is_none());
    j.finish_deputy(
        &i.operation_id,
        DeputyResult {
            request: p.request(),
            decode_stage: None,
            outcome: DeputyOutcome::Failed {
                failure: cerulean_adapters::api_generation::ApiFailure::Transport {},
            },
        },
    )
    .unwrap();
    let head = j.evidence_digest(&i.operation_id).unwrap().to_owned();
    j.resolve(
        &i.operation_id,
        &head,
        UserResolution::Discard {
            reason: "original transport unknown".into(),
        },
    )
    .unwrap();
    assert_eq!(
        j.historical_source(&i.operation_id, true)
            .unwrap()
            .uncertain_outcomes,
        1
    );
}

#[test]
fn diagnostic_281_persists_rejection_and_missing_tampered_or_partial_sidecars_do_not_change_ledger()
{
    use cerulean_adapters::deputy_management::DecodeStage;
    for mode in ["valid", "missing", "tampered", "partial", "legacy"] {
        let tmp = root();
        let data = tmp.path().join("data");
        let (i, p) = prepared("diagnostic");
        let mut j = DecisionJournal::open(&data).unwrap();
        j.begin_deputy(i.clone(), &p).unwrap();
        let sidecar = data.join("management-response-diagnostics/diagnostic.json");
        if mode == "partial" {
            fs::create_dir(sidecar.parent().unwrap()).unwrap();
            fs::write(sidecar.with_extension("pending"), b"partial").unwrap();
        }
        let mut r = result(&p, false);
        if mode == "legacy" {
            r = serde_json::from_value(serde_json::to_value(r).unwrap()).unwrap();
        }
        j.finish_deputy(&i.operation_id, r.clone()).unwrap();
        let head = j.evidence_digest(&i.operation_id).unwrap().to_owned();
        let response = fs::read(data.join("management-decisions/diagnostic.1.json")).unwrap();
        match mode {
            "missing" => fs::remove_file(&sidecar).unwrap(),
            "tampered" => {
                let mut v: Value = serde_json::from_slice(&fs::read(&sidecar).unwrap()).unwrap();
                v["resultDigest"] = json!("0".repeat(64));
                fs::write(&sidecar, v.to_string()).unwrap();
            }
            _ => (),
        }
        drop(j);
        let mut j = DecisionJournal::open(&data).unwrap();
        match mode {
            "valid" => assert_eq!(
                j.read_decode_diagnostic("diagnostic").unwrap(),
                Some(DecodeStage::DecisionJson)
            ),
            "tampered" => assert!(j.read_decode_diagnostic("diagnostic").is_err()),
            _ => assert_eq!(j.read_decode_diagnostic("diagnostic").unwrap(), None),
        }
        let message = j.decode_diagnostic_message("diagnostic").unwrap();
        assert!(message.contains(match mode {
            "valid" => "DecisionJson",
            "tampered" => "不符",
            _ => "未记录",
        }));
        assert_eq!(j.evidence_digest(&i.operation_id).unwrap(), head);
        assert_eq!(j.statuses()[0].state, DecisionState::Unknown);
        assert!(j.ready_envelope(&i.operation_id, &i.state_digest).is_err());
        assert!(j.finish_deputy(&i.operation_id, r).is_err());
        assert!(j.begin_deputy(i.clone(), &p).is_err());
        assert_eq!(
            j.deputy_evidence(&i.operation_id)
                .unwrap()
                .1
                .unwrap()
                .billing()
                .unwrap()
                .usage
                .input_tokens,
            25
        );
        assert_eq!(
            fs::read(data.join("management-decisions/diagnostic.1.json")).unwrap(),
            response
        );
    }
}

#[test]
fn transport_285_diagnostics_preserve_unknown_liability_and_legacy_response_bytes_after_reopen() {
    use cerulean_adapters::{api_generation::ApiFailure, deputy_management::DecodeStage};
    for stage in [
        DecodeStage::TransportConnect,
        DecodeStage::TransportConnectTimeout,
        DecodeStage::TransportRequestSend,
        DecodeStage::TransportRequestTimeout,
        DecodeStage::TransportBodyRead,
        DecodeStage::TransportBodyTimeout,
    ] {
        let tmp = root();
        let data = tmp.path().join("data");
        let (i, p) = prepared("transport285");
        let mut j = DecisionJournal::open(&data).unwrap();
        j.begin_deputy(i.clone(), &p).unwrap();
        let r = DeputyResult {
            request: p.request(),
            decode_stage: Some(stage),
            outcome: DeputyOutcome::Failed {
                failure: ApiFailure::Transport {},
            },
        };
        r.validate(&p.request()).unwrap();
        let legacy: DeputyResult =
            serde_json::from_value(serde_json::to_value(&r).unwrap()).unwrap();
        assert!(legacy.decode_stage.is_none());
        assert_eq!(digest(&legacy).unwrap(), digest(&r).unwrap());
        let mut wrong = r.clone();
        wrong.outcome = DeputyOutcome::Failed {
            failure: ApiFailure::NotSent {},
        };
        assert!(wrong.validate(&p.request()).is_err());
        assert!(j.finish_deputy(&i.operation_id, wrong).is_err());
        j.finish_deputy(&i.operation_id, r.clone()).unwrap();
        let original = fs::read(data.join("management-decisions/transport285.1.json")).unwrap();
        drop(j);
        let mut j = DecisionJournal::open(&data).unwrap();
        assert_eq!(
            j.read_decode_diagnostic(&i.operation_id).unwrap(),
            Some(stage)
        );
        assert!(j
            .decode_diagnostic_message(&i.operation_id)
            .unwrap()
            .contains("计费仍未知"));
        assert!(j.historical_source(&i.operation_id, true).is_err());
        assert_eq!(j.statuses()[0].state, DecisionState::Unknown);
        assert!(j.begin_deputy(i.clone(), &p).is_err());
        assert!(j.finish_deputy(&i.operation_id, r).is_err());
        let head = j.evidence_digest(&i.operation_id).unwrap().to_owned();
        j.resolve(
            &i.operation_id,
            &head,
            UserResolution::Discard {
                reason:
                    "fixture closes original transport failure without resolving unknown billing"
                        .into(),
            },
        )
        .unwrap();
        assert_eq!(
            j.historical_source(&i.operation_id, true)
                .unwrap()
                .uncertain_outcomes,
            1
        );
        assert_eq!(
            fs::read(data.join("management-decisions/transport285.1.json")).unwrap(),
            original
        );
    }
}
