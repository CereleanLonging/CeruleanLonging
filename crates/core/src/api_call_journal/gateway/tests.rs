use super::*;
use cerulean_adapters::api_generation::{
    fixture_reply,
    gateway::{GatewayHeaders, GatewayRequestId},
    ApiMessage, ApiRequest, ApiTarget,
};
use cerulean_domain::{credentials::ProviderEndpoint, secret::SecretInput};
use serde_json::json;

fn prepared() -> (ApiClient, PreparedRequest, CallIdentity) {
    let client = ApiClient::new(SecretInput::new("synthetic-gateway-324".into()).unwrap()).unwrap();
    let prepared = client
        .prepare(ApiRequest {
            target: ApiTarget::Provider {
                endpoint: ProviderEndpoint::A6Api {},
            },
            model: "gpt-6.1-sol".into(),
            instructions: "Reply only".into(),
            messages: vec![ApiMessage::User("A bounded synthetic check".into())],
            tools: vec![],
            max_output_tokens: 64,
            timeout_seconds: 10,
        })
        .unwrap();
    let identity = CallIdentity {
        project_id: "project".into(),
        task_id: "task".into(),
        attempt_id: "attempt".into(),
        revision_digest: "a".repeat(64),
        request_id: "local-324".into(),
        round: 1,
        fingerprint: prepared.fingerprint().clone(),
    };
    (client, prepared, identity)
}
fn headers() -> GatewayHeaders {
    GatewayHeaders {
        status: 200,
        request_id: GatewayRequestId::Accepted {
            value: "gateway-324".into(),
        },
    }
}
fn reply(prepared: &PreparedRequest) -> ApiReply {
    fixture_reply(prepared, json!({
        "id":"body-324", "model":"gpt-6.1-sol", "usage":{"prompt_tokens":2,"completion_tokens":1},
        "choices":[{"index":0,"finish_reason":"stop","message":{"role":"assistant","content":"done"}}]
    })).unwrap()
}
fn round(root: &Path) -> PathBuf {
    let path = root.join("rounds/001");
    fs::create_dir_all(&path).unwrap();
    path
}

#[tokio::test]
async fn gateway_324_api_success_and_http_failure_link_original_and_reopen_without_resend() {
    for failure in [false, true] {
        let base = tempfile::tempdir().unwrap();
        let path = round(base.path());
        let (_client, prepared, id) = prepared();
        let fp = prepared.fingerprint().clone();
        let mut journal = ApiCallJournal::open(&path, &id).unwrap();
        let result = journal
            .execute_reply_with_gateway(
                &fp,
                base.path(),
                async |observe: &mut GatewayObserver<'_>| {
                    assert!(path.join("00.json").is_file());
                    let mut h = headers();
                    if failure {
                        h.status = 429;
                    }
                    observe(&h);
                    assert!(
                        base.path().join("gateway-response-evidence").is_dir(),
                        "headers precede terminal result"
                    );
                    if failure {
                        Err(ApiFailure::Http { status: 429 })
                    } else {
                        Ok(reply(&prepared))
                    }
                },
            )
            .await
            .unwrap();
        assert_eq!(result.is_err(), failure);
        let review = journal.gateway_evidence(base.path()).unwrap().unwrap();
        assert_eq!(review.status, "linked");
        assert_eq!(review.local_id, "local-324");
        assert_eq!(review.gateway_request_id.as_deref(), Some("gateway-324"));
        assert_eq!(
            review.body_response_id.as_deref(),
            if failure { None } else { Some("body-324") }
        );
        assert_eq!(
            fs::read_dir(&path).unwrap().count(),
            3,
            "old strict round directory stays exact"
        );
        assert_eq!(fs::read_dir(base.path().join("rounds")).unwrap().count(), 1);
        let original = fs::read(path.join("01.json")).unwrap();
        let wire: serde_json::Value = serde_json::from_slice(&original).unwrap();
        let encoded = serde_json::to_vec(&wire).unwrap();
        assert!(
            !String::from_utf8(encoded).unwrap().contains("gateway"),
            "no new legacy fields"
        );
        drop(journal);
        let mut recovered = ApiCallJournal::read_existing(&path).unwrap();
        assert_eq!(recovered.state().remote_outcome_unknown(), failure);
        assert_eq!(
            recovered.gateway_evidence(base.path()).unwrap().unwrap(),
            review
        );
        assert!(recovered
            .execute_reply_with_gateway(
                &fp,
                base.path(),
                async |_observe: &mut GatewayObserver<'_>| {
                    panic!("read-only original request cannot resend")
                }
            )
            .await
            .is_err());
        assert_eq!(fs::read(path.join("01.json")).unwrap(), original);
    }
}

#[tokio::test]
async fn gateway_324_api_dropped_future_keeps_original_unknown_and_header_only() {
    let base = tempfile::tempdir().unwrap();
    let path = round(base.path());
    let (_client, prepared, id) = prepared();
    let fp = prepared.fingerprint().clone();
    let mut journal = ApiCallJournal::open(&path, &id).unwrap();
    let captured = std::cell::Cell::new(false);
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(20),
        journal.execute_reply_with_gateway(
            &fp,
            base.path(),
            async |observe: &mut GatewayObserver<'_>| {
                observe(&headers());
                captured.set(true);
                std::future::pending::<Result<ApiReply, ApiFailure>>().await
            },
        ),
    )
    .await;
    assert!(result.is_err());
    assert!(
        captured.get(),
        "test must reach headers before dropping future"
    );
    drop(journal);
    let recovered = ApiCallJournal::read_existing(&path).unwrap();
    assert!(recovered.state().remote_outcome_unknown());
    assert!(recovered.state().outcome.is_none());
    assert!(!path.join("01.json").exists());
    assert_eq!(
        recovered
            .gateway_evidence(base.path())
            .unwrap()
            .unwrap()
            .status,
        "headerOnly"
    );
}

#[tokio::test]
async fn gateway_324_auxiliary_write_failure_does_not_change_original_reply_or_failure() {
    for failure in [false, true] {
        let base = tempfile::tempdir().unwrap();
        let path = round(base.path());
        fs::write(
            base.path().join("gateway-response-evidence"),
            b"blocking ordinary file",
        )
        .unwrap();
        let (_client, prepared, id) = prepared();
        let fp = prepared.fingerprint().clone();
        let mut journal = ApiCallJournal::open(&path, &id).unwrap();
        let result = journal
            .execute_reply_with_gateway(
                &fp,
                base.path(),
                async |observe: &mut GatewayObserver<'_>| {
                    observe(&headers());
                    if failure {
                        Err(ApiFailure::LocalCancellation {})
                    } else {
                        Ok(reply(&prepared))
                    }
                },
            )
            .await
            .unwrap();
        assert_eq!(result.is_err(), failure);
        assert!(journal.healthy());
        assert_eq!(journal.state().remote_outcome_unknown(), failure);
        assert!(journal.gateway_evidence(base.path()).is_err());
        let original = journal.head().to_owned();
        drop(journal);
        let recovered = ApiCallJournal::read_existing(&path).unwrap();
        assert_eq!(recovered.head(), original);
    }
}

#[tokio::test]
async fn gateway_324_outcome_publication_failure_preserves_header_and_withholds_reply() {
    let base = tempfile::tempdir().unwrap();
    let path = round(base.path());
    let (_client, prepared, id) = prepared();
    let fp = prepared.fingerprint().clone();
    let mut journal = ApiCallJournal::open(&path, &id).unwrap();
    let result = journal
        .execute_reply_with_gateway(
            &fp,
            base.path(),
            async |observe: &mut GatewayObserver<'_>| {
                observe(&headers());
                fs::write(
                    path.join("01.pending"),
                    b"simulate original publication conflict",
                )
                .unwrap();
                Ok(reply(&prepared))
            },
        )
        .await;
    assert!(result.is_err());
    assert!(!journal.healthy());
    let binding = Binding {
        lane: Lane::Worker,
        project_id: id.project_id.clone(),
        local_id: id.request_id.clone(),
        request_digest: digest(&id).unwrap(),
        intent_hash: journal.intent_head.clone().unwrap(),
    };
    assert_eq!(
        crate::gateway_response_evidence::read(base.path(), &binding, None)
            .unwrap()
            .status,
        "headerOnly"
    );
    drop(journal);
    assert!(ApiCallJournal::read_existing(&path).is_err());
    assert!(!path.join("01.json").exists());
}
