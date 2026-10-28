use super::*;
use crate::api_call_journal::measured_tests::server;

#[tokio::test]
async fn rejected_usage_full_dispatch_stops_before_tools_and_exports_failure_after_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let s = spec(dir.path());
    let data = prepared(dir.path(), &s);
    assert_eq!(
        crate::data_format::status(&data)
            .unwrap()
            .minimum_reader_generation,
        17
    );
    assert_eq!(
        s.plan.execution[0]
            .api_worker
            .as_ref()
            .unwrap()
            .target
            .provider(),
        cerulean_domain::credentials::CredentialProvider::DeepSeek
    );
    let body = json!({"object":"chat.completion","id":"baseline-rejected-543","model":"fixture",
        "choices":[{"index":0,"finish_reason":"length","message":{"role":"assistant","content":null,"tool_calls":[{"id":"tool-1","type":"function","function":{"name":"cerulean_write","arguments":"{truncated"}}]}}],
        "usage":{"prompt_tokens":13,"completion_tokens":7,"total_tokens":20,"prompt_cache_hit_tokens":2,"prompt_cache_miss_tokens":11}});
    let (url, server) = server(body).await;
    let (_tx, mut rx) = oneshot::channel();
    let mut requests = 0;
    let end = execute(
        &data,
        &digest(&s).unwrap(),
        approval(&s),
        "controlled-local-http",
        client,
        &mut async |client: &ApiClient,
                    request,
                    cancel: &mut oneshot::Receiver<()>,
                    _: &mut cerulean_adapters::api_generation::GatewayObserver<'_>| {
            requests += 1;
            client.fixture_send_measured(request, cancel, &url).await
        },
        &mut rx,
        &AtomicBool::new(false),
    )
    .await
    .unwrap();
    server.await.unwrap();
    assert_eq!(requests, 1);
    assert!(!end.accepted);
    assert_eq!(end.task_state, TaskState::Unknown);
    assert!(end.error.as_ref().unwrap().contains("no automatic retry"));
    assert_eq!(
        observe(
            Path::new(&s.plan.project.root),
            s.plan.snapshot_policy.as_ref().unwrap()
        )
        .unwrap(),
        s.seed,
        "rejected tool arguments must not change project files"
    );
    let exported = inspect(&data, &digest(&s).unwrap()).unwrap();
    assert_eq!(exported["coreExport"]["apiIntentCount"], 1);
    assert_eq!(exported["coreExport"]["apiReplies"], 0);
    assert_eq!(exported["coreExport"]["apiUncertainRemoteUsage"], 0);
    assert_eq!(
        exported["coreExport"]["apiCalls"][0]["usage"]["outputTokens"],
        7
    );
    assert_eq!(
        exported["coreExport"]["apiCalls"][0]["outcome"],
        "rejected-with-observed-usage"
    );
    assert!(exported["coreExport"]["verifications"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(inspect(&data, &digest(&s).unwrap()).unwrap(), exported);
    let (again, sends) = synthetic(&data, &s, "ok").await;
    assert!(again.is_err());
    assert_eq!(sends, 0);
    assert_eq!(inspect(&data, &digest(&s).unwrap()).unwrap(), exported);
    if let Some(out) = std::env::var_os("CL543_BASELINE_EXPORT") {
        let file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(out)
            .unwrap();
        serde_json::to_writer_pretty(file, &exported).unwrap();
    }
}
