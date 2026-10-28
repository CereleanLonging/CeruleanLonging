//! Original approved workflow -> loopback HTTP -> journal -> reopen -> native export.
use super::*;
use crate::api_call_journal::measured_tests::{rejected_body, server};
use cerulean_adapters::api_generation::GatewayObserver;
use serde_json::json;

#[tokio::test]
async fn rejected_usage_original_workflow_http_stop_reopen_export_preserves_failure() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    drop(crate::Store::open(&data).unwrap());
    crate::rejected_usage_guard::tests::promote_fixture(&data, &temp.path().join("before17"));
    let (mut host, draft) = project_planning::tests::fixture(temp.path());
    host.configure_project(api_project_tests::draft_for(
        draft,
        ProjectWorkerAdapter::Openai,
        ApiTarget::OpenAi {},
    ))
    .unwrap();
    let mut proposal = project_planning::tests::proposal("tests");
    if let ManagementDecision::Plan { tasks, .. } = &mut proposal {
        tasks[0].worker_id = ProjectWorkerAdapter::Openai.worker_id();
    }
    project_planning::tests::complete(&mut host, proposal);
    let revision = host.projects["project"]
        .revision()
        .unwrap()
        .approval_digest()
        .unwrap();
    host.approve("project", &revision).unwrap();
    let (run, root) = assigned(&mut host, true);
    let head = host.projects["project"]
        .history()
        .last()
        .unwrap()
        .hash
        .clone();
    let client =
        ApiClient::new(SecretInput::new("synthetic-http-export-543".into()).unwrap()).unwrap();
    let prepared = client
        .prepare(ApiRequest {
            target: run.worker.target.clone(),
            model: run.worker.model.clone(),
            instructions: "Controlled rejected content".into(),
            messages: vec![ApiMessage::User("local only".into())],
            tools: vec![],
            max_output_tokens: run.worker.max_output_tokens,
            timeout_seconds: run.worker.request_timeout_seconds,
        })
        .unwrap();
    let id = crate::api_execution::call_identity(&run, 1, prepared.fingerprint().clone()).unwrap();
    let round = root.join("rounds/001");
    let mut journal = ApiCallJournal::open(&round, &id).unwrap();
    let (url, server) = server(rejected_body()).await;
    let (_tx, mut rx) = tokio::sync::oneshot::channel();
    let returned = journal
        .execute_reply_with_gateway(
            &id.fingerprint,
            &root,
            async |_: &mut GatewayObserver<'_>| {
                assert!(
                    round.join("00.json").exists(),
                    "HTTP must follow durable intent"
                );
                client.fixture_send_measured(prepared, &mut rx, &url).await
            },
        )
        .await
        .unwrap();
    server.await.unwrap();
    assert!(returned.is_err());
    assert!(!root.join("completion.json").exists());
    assert!(journal.state().remote_outcome_unknown());
    drop(journal);
    drop(host);
    let before = snapshot(&data);
    let export = crate::efficiency_evidence::read(&data, "project", &revision, &head).unwrap();
    assert_eq!(export["schema"], 2);
    assert_eq!(export["apiIntentCount"], 1);
    assert_eq!(export["apiReplies"], 0);
    assert_eq!(export["apiUncertainRemoteUsage"], 0);
    assert_eq!(
        export["apiCalls"][0]["outcome"],
        "rejected-with-observed-usage"
    );
    assert_eq!(export["apiCalls"][0]["usage"]["inputTokens"], 13);
    assert_eq!(export["apiCalls"][0]["usage"]["outputTokens"], 7);
    assert_eq!(export["taskStates"]["task"]["state"], "dispatching");
    assert_eq!(export["actualCost"], serde_json::Value::Null);
    assert_eq!(
        crate::efficiency_evidence::read(&data, "project", &revision, &head).unwrap(),
        export
    );
    assert!(snapshot(&data) == before, "export must not mutate records");
    if let Some(out) = std::env::var_os("CL543_EXPORT") {
        let file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(out)
            .unwrap();
        serde_json::to_writer_pretty(file, &export).unwrap();
    }
    // An edit to either response digest or request is detected by the original hash chain.
    let original = fs::read(round.join("01.json")).unwrap();
    let mut corrupted: serde_json::Value = serde_json::from_slice(&original).unwrap();
    corrupted["body"]["event"]["rejected_usage"]["responseJsonSha256"] = json!("f".repeat(64));
    fs::write(
        round.join("01.json"),
        serde_json::to_vec(&corrupted).unwrap(),
    )
    .unwrap();
    assert!(crate::efficiency_evidence::read(&data, "project", &revision, &head).is_err());
}
