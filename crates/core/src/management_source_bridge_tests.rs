//! No model or worker process. Real management/startup ledgers and Host reopen.
use super::*;
use crate::management_execution::{DecisionIntent, StoredDecision};
use crate::startup_journal::StartupIntent;
use cerulean_adapters::{api_generation::ApiClient, deputy_management};
use cerulean_domain::{
    management::{DecisionEnvelope, ManagementDecision, Purpose},
    management_ingress::{ExternalChiefDecision, ManagementHandoff, ManagementSource},
    secret::SecretInput,
};
use serde_json::json;

fn external_plan(root: &Path) -> RuntimeHost {
    let (mut host, draft) = super::project_planning::tests::fixture(root);
    host.configure_project(draft).unwrap();
    drop(host);
    crate::management_source_guard::tests::promote_fixture(&root.join("data"));
    let mut host = RuntimeHost::open(&root.join("data")).unwrap();
    let setup = host.project_setups["project"].digest.clone();
    let request = host
        .prepare_external_chief_plan("project", &setup, "current-session")
        .unwrap();
    host.submit_external_chief_decision(
        &request.operation_id,
        "current-session",
        ExternalChiefDecision {
            request_digest: digest(&request).unwrap(),
            session_id: "current-session".into(),
            envelope: DecisionEnvelope {
                state_digest: request.state_digest,
                decision: super::project_planning::tests::proposal("tests"),
            },
        },
    )
    .unwrap();
    assert!(host.projects["project"].approval().is_none());
    host
}
fn deputy(host: &mut RuntimeHost, id: &str, worker: &str, valid: bool) {
    let context = host
        .management_context("project", "Implement a sum function")
        .unwrap();
    let state_digest = digest(&context).unwrap();
    let intent = DecisionIntent {
        operation_id: id.into(),
        project_id: "project".into(),
        purpose: Purpose::Assign,
        model: "deepseek-flash".into(),
        state_digest: state_digest.clone(),
        context: context.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    let p = deputy_management::prepare(
        &ApiClient::new(SecretInput::new("invalid-fixture-value".into()).unwrap()).unwrap(),
        ManagementHandoff {
            operation_id: id.into(),
            project_id: "project".into(),
            purpose: Purpose::Assign,
            source: ManagementSource::DeepSeek {
                model: "deepseek-flash".into(),
            },
            state_digest: state_digest.clone(),
            context,
        },
    )
    .unwrap();
    host.management.begin_deputy(intent, &p).unwrap();
    let content = if valid {
        json!({"stateDigest":state_digest,"decision":{"action":"assign","taskId":"task","workerId":worker,"rationale":"approved fixture task"}}).to_string()
    } else {
        "invalid decision".into()
    };
    host.management.finish_deputy(id,deputy_management::fixture_result(&p,json!({"id":"ds-assignment","model":"deepseek-flash","usage":{"prompt_tokens":20,"completion_tokens":6},"choices":[{"index":0,"finish_reason":"stop","message":{"role":"assistant","content":content}}]}))).unwrap();
}
fn reserve(host: &mut RuntimeHost, id: &str) -> StartupIntent {
    let store = &host.projects["project"];
    let revision = store.revision().unwrap();
    let intent = StartupIntent {
        schema_version: 1,
        protocol: "codex-durable-before-send-v1".into(),
        project_id: "project".into(),
        task_id: "task".into(),
        attempt_id: "attempt".into(),
        revision_digest: revision.approval_digest().unwrap(),
        worker: revision.task("task").unwrap().worker.clone().unwrap(),
        management_operation: Some(id.into()),
        management_hash: Some(host.management.response_head(id).unwrap().into()),
        workflow_sequence: store.history().len() as u32,
        workflow_hash: store.history().last().map(|e| e.hash.clone()),
        started_at: chrono::Utc::now().to_rfc3339(),
    };
    host.startups.begin(intent.clone()).unwrap();
    intent
}
#[test]
fn provider_source_bridge_external_plan_and_deputy_startup_survive_reopen_without_openai_receipts()
{
    let root = tempfile::tempdir().unwrap();
    let mut host = external_plan(root.path());
    let digest = host.projects["project"]
        .revision()
        .unwrap()
        .approval_digest()
        .unwrap();
    // Explicit simulated user approval; no automatic approval by either model.
    host.approve("project", &digest).unwrap();
    deputy(&mut host, "ds-assign", "worker-codex", true);
    assert!(matches!(
        host.management_envelope("ds-assign").unwrap().decision,
        ManagementDecision::Assign { .. }
    ));
    assert!(host.management.observation("ds-assign").is_err());
    reserve(&mut host, "ds-assign");
    let records = host.startups.all().into_iter().cloned().collect::<Vec<_>>();
    let proof = host.startup_management_proof("project", &records).unwrap();
    assert_eq!(proof.operations.len(), 2);
    assert!(proof.operations.iter().all(|o| o.observation.is_none()));
    assert!(proof
        .operations
        .iter()
        .any(|o| matches!(o.source_decision, Some(StoredDecision::CodexSession { .. }))));
    assert!(proof
        .operations
        .iter()
        .any(|o| matches!(o.source_decision, Some(StoredDecision::DeepSeek { .. }))));
    let original = host
        .management
        .original_response_digest("ds-assign")
        .unwrap();
    drop(host);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(
        host.management
            .original_response_digest("ds-assign")
            .unwrap(),
        original
    );
    assert!(host.codex_active.is_empty() && host.management_active.is_empty());
    let review = host.startup_detail("project", "attempt").unwrap();
    assert!(review.evidence_digest.is_some());
    let reply = host.handle(Request {
        id: "close-original-startup".into(),
        protocol_version: PROTOCOL_VERSION,
        command: Command::AbandonCodexStartup {
            project_id: "project".into(),
            attempt_id: "attempt".into(),
            evidence_digest: review.evidence_digest.unwrap(),
            reason: "owned fixture: no worker was started".into(),
        },
    });
    assert!(reply.error.is_none(), "{:?}", reply.error);
    drop(host);
    let host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert!(host
        .startups
        .get("project", "attempt")
        .unwrap()
        .closure
        .is_some());
    assert!(host.management.startup_closure("ds-assign").is_some());
}
#[test]
fn provider_source_bridge_invalid_or_wrong_worker_cannot_authorize_startup() {
    for (valid, worker) in [(false, "worker-codex"), (true, "worker-omp")] {
        let root = tempfile::tempdir().unwrap();
        let mut host = external_plan(root.path());
        let revision = host.projects["project"]
            .revision()
            .unwrap()
            .approval_digest()
            .unwrap();
        host.approve("project", &revision).unwrap();
        deputy(&mut host, "ds-assign", worker, valid);
        reserve(&mut host, "ds-assign");
        let records = host.startups.all().into_iter().cloned().collect::<Vec<_>>();
        assert!(host.startup_management_proof("project", &records).is_err());
        assert!(host.codex_active.is_empty());
        drop(host);
        assert!(RuntimeHost::open(&root.path().join("data")).is_err());
    }
}
#[test]
fn provider_source_bridge_preserves_legacy_observation_digest_and_serialized_startup_shape() {
    let root = tempfile::tempdir().unwrap();
    let (host, _) = super::startup_publication_tests::fixture(root.path(), false);
    let statuses = host.management.statuses();
    assert!(!statuses.is_empty());
    for status in statuses {
        assert_eq!(
            host.management
                .original_response_digest(&status.operation_id)
                .unwrap(),
            digest(host.management.observation(&status.operation_id).unwrap()).unwrap()
        );
    }
    let proof = host.startup_management_proof("project", &[]).unwrap();
    assert!(proof
        .operations
        .iter()
        .all(|o| o.observation.is_some() && o.source_decision.is_none()));
    let json = serde_json::to_value(proof).unwrap();
    assert!(json["operations"]
        .as_array()
        .unwrap()
        .iter()
        .all(|o| o.get("source_decision").is_none()));
}
