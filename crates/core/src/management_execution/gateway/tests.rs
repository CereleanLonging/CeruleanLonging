use super::*;
use cerulean_adapters::{
    api_generation::{
        gateway::{GatewayHeaders, GatewayRequestId},
        ApiClient, ApiFailure,
    },
    deputy_management::{self, DeputyResult, PreparedDeputy},
};
use cerulean_domain::{
    credentials::ProviderEndpoint,
    management_ingress::{ManagementHandoff, ManagementRole, ManagementSource},
    secret::SecretInput,
};
use serde_json::json;

fn fixture() -> (
    tempfile::TempDir,
    DecisionJournal,
    DecisionIntent,
    PreparedDeputy,
) {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    drop(crate::Store::open(&data).unwrap());
    crate::management_source_guard::tests::promote_fixture(&data);
    crate::relay_guard::tests::promote_fixture(&data, &temp.path().join("before16"));
    let endpoint = ProviderEndpoint::A6Api {};
    let context = json!({"requirement":"bounded synthetic review", "executionSetup":{"setup":{
        "deputyModel":"gpt-6.1-sol", "managementSources":{"deputy":{"kind":"provider","endpoint":endpoint}}
    }}});
    let intent = DecisionIntent {
        operation_id: "management-local-324".into(),
        project_id: "project".into(),
        purpose: Purpose::Advise,
        model: "gpt-6.1-sol".into(),
        state_digest: digest(&context).unwrap(),
        context: context.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    let client =
        ApiClient::new(SecretInput::new("synthetic-management-324".into()).unwrap()).unwrap();
    let prepared = deputy_management::prepare(
        &client,
        ManagementHandoff {
            operation_id: intent.operation_id.clone(),
            project_id: intent.project_id.clone(),
            purpose: intent.purpose,
            source: ManagementSource::Provider {
                endpoint,
                model: intent.model.clone(),
                role: ManagementRole::Deputy,
            },
            state_digest: intent.state_digest.clone(),
            context,
        },
    )
    .unwrap();
    let journal = DecisionJournal::open(&data).unwrap();
    (temp, journal, intent, prepared)
}
fn headers() -> GatewayHeaders {
    GatewayHeaders {
        status: 200,
        request_id: GatewayRequestId::Accepted {
            value: "management-gateway-324".into(),
        },
    }
}
fn result(prepared: &PreparedDeputy, malformed: bool) -> DeputyResult {
    let content = if malformed {
        "unusable decision".into()
    } else {
        json!({"stateDigest":prepared.request().handoff.state_digest,"decision":{"action":"wait","reason":"need approval"}}).to_string()
    };
    deputy_management::fixture_result(
        prepared,
        json!({
            "id":"management-body-324", "model":"gpt-6.1-sol", "usage":{"prompt_tokens":3,"completion_tokens":2},
            "choices":[{"index":0,"finish_reason":"stop","message":{"role":"assistant","content":content}}]
        }),
    )
}

#[test]
fn gateway_324_management_reply_rejection_and_http_failure_keep_original_bytes_on_reopen() {
    for mode in ["accepted", "rejected", "http"] {
        let (temp, mut journal, intent, prepared) = fixture();
        journal.begin_deputy(intent.clone(), &prepared).unwrap();
        let mut capture = journal.gateway_capture(&intent.operation_id).unwrap();
        let mut h = headers();
        if mode == "http" {
            h.status = 429;
        }
        capture.headers(&h);
        assert_eq!(
            journal
                .gateway_evidence(&intent.operation_id)
                .unwrap()
                .unwrap()
                .status,
            "headerOnly"
        );
        let response = if mode == "http" {
            DeputyResult {
                request: prepared.request(),
                decode_stage: None,
                outcome: DeputyOutcome::Failed {
                    failure: ApiFailure::Http { status: 429 },
                },
            }
        } else {
            result(&prepared, mode == "rejected")
        };
        let original_shape = serde_json::to_vec(&response).unwrap();
        journal
            .finish_deputy(&intent.operation_id, response)
            .unwrap();
        let file = temp
            .path()
            .join("data/management-decisions/management-local-324.1.json");
        let original_record = fs::read(&file).unwrap();
        journal.finish_gateway(&intent.operation_id, capture);
        assert_eq!(
            fs::read(&file).unwrap(),
            original_record,
            "sidecar must not edit original response"
        );
        let first = journal
            .gateway_evidence(&intent.operation_id)
            .unwrap()
            .unwrap();
        assert_eq!(first.status, "linked");
        assert_eq!(
            first.gateway_request_id.as_deref(),
            Some("management-gateway-324")
        );
        assert_eq!(
            first.body_response_id.as_deref(),
            if mode == "http" {
                None
            } else {
                Some("management-body-324")
            }
        );
        let head = journal
            .evidence_digest(&intent.operation_id)
            .unwrap()
            .to_owned();
        drop(journal);
        let mut recovered = DecisionJournal::open(&temp.path().join("data")).unwrap();
        assert_eq!(
            recovered.evidence_digest(&intent.operation_id).unwrap(),
            head
        );
        assert_eq!(
            recovered
                .gateway_evidence(&intent.operation_id)
                .unwrap()
                .unwrap(),
            first
        );
        let saved = recovered
            .deputy_evidence(&intent.operation_id)
            .unwrap()
            .1
            .unwrap();
        assert_eq!(serde_json::to_vec(&saved).unwrap(), original_shape);
        assert!(recovered.begin_deputy(intent.clone(), &prepared).is_err());
        assert_eq!(fs::read(file).unwrap(), original_record);
        assert_eq!(
            crate::data_format::status(&temp.path().join("data"))
                .unwrap()
                .minimum_reader_generation,
            16
        );
    }
}

#[test]
fn gateway_324_management_header_without_completion_does_not_resolve_unknown() {
    let (temp, mut journal, intent, prepared) = fixture();
    journal.begin_deputy(intent.clone(), &prepared).unwrap();
    journal
        .gateway_capture(&intent.operation_id)
        .unwrap()
        .headers(&headers());
    let original = fs::read(
        temp.path()
            .join("data/management-decisions/management-local-324.0.json"),
    )
    .unwrap();
    drop(journal);
    let mut recovered = DecisionJournal::open(&temp.path().join("data")).unwrap();
    assert!(recovered
        .deputy_evidence(&intent.operation_id)
        .unwrap()
        .1
        .is_none());
    assert_eq!(
        recovered
            .gateway_evidence(&intent.operation_id)
            .unwrap()
            .unwrap()
            .status,
        "headerOnly"
    );
    assert_eq!(recovered.statuses()[0].state, DecisionState::Unknown);
    assert!(recovered.begin_deputy(intent.clone(), &prepared).is_err());
    assert_eq!(
        fs::read(
            temp.path()
                .join("data/management-decisions/management-local-324.0.json")
        )
        .unwrap(),
        original
    );
}

#[test]
fn gateway_324_management_diagnostic_failure_cannot_invalidate_received_decision() {
    let (temp, mut journal, intent, prepared) = fixture();
    journal.begin_deputy(intent.clone(), &prepared).unwrap();
    fs::write(
        temp.path().join("data/gateway-response-evidence"),
        b"blocking ordinary file",
    )
    .unwrap();
    let mut capture = journal.gateway_capture(&intent.operation_id).unwrap();
    capture.headers(&headers());
    journal
        .finish_deputy(&intent.operation_id, result(&prepared, false))
        .unwrap();
    journal.finish_gateway(&intent.operation_id, capture);
    assert_eq!(journal.statuses()[0].state, DecisionState::Ready);
    assert!(journal.gateway_evidence(&intent.operation_id).is_err());
    let original = journal
        .evidence_digest(&intent.operation_id)
        .unwrap()
        .to_owned();
    drop(journal);
    let recovered = DecisionJournal::open(&temp.path().join("data")).unwrap();
    assert_eq!(
        recovered.evidence_digest(&intent.operation_id).unwrap(),
        original
    );
    assert_eq!(recovered.statuses()[0].state, DecisionState::Ready);
}
