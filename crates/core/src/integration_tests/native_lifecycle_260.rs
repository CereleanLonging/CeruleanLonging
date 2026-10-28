use super::*;

#[tokio::test]
#[ignore = "260 native desktop owns explicit UI actions; synthetic cancellable transport, no provider I/O"]
async fn baseline_lifecycle_native_owned_service() {
    let out = PathBuf::from(std::env::var_os("CERULEAN_NATIVE_LIFECYCLE_260").unwrap());
    let root = out.join("data");
    assert!(
        !root.exists(),
        "fresh fixture only; never replay an old run"
    );
    drop(crate::Store::open(&root).unwrap());
    let backup = out.join("initial-cold-backup");
    let manifest = crate::backup::create(&root, &backup)
        .unwrap()
        .manifest_digest;
    let review = crate::experiment_guard::review(&root, &backup, &manifest).unwrap();
    crate::experiment_guard::promote(&root, &backup, &manifest, &review.digest).unwrap();
    let mut host = RuntimeHost::open(&root).unwrap();
    host.baseline_fixture = true;
    let owned = root.clone();
    let service = tokio::spawn(async move { crate::ipc::serve_host(&owned, host).await });
    let began = Instant::now();
    while crate::ipc::Client::connect(&root).await.is_err() {
        assert!(began.elapsed() < Duration::from_secs(10));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    crate::api_execution::publish(
        &out,
        "fixture-service-ready.json",
        &json!({
            "pid":std::process::id(),"dataRoot":root,"transport":"cfg(test) synthetic only",
            "delaySeconds":60,"realModelCalls":0,"userCredentialsRead":0
        }),
    )
    .unwrap();
    // Exit exclusively through the original authenticated Shutdown gate.
    service.await.unwrap().unwrap();
    let timeline = root.join("baseline-experiments/S01/timeline");
    if timeline.exists() {
        let start = crate::baseline_runner::timing::original_start_digest(&timeline).unwrap();
        let inspection = crate::baseline_runner::timing::inspect(&timeline, &start).unwrap();
        crate::api_execution::publish(&out, "fixture-final-inspection.json", &inspection).unwrap();
    }
    crate::api_execution::publish(
        &out,
        "fixture-service-stopped.json",
        &json!({
            "pid":std::process::id(),"normalShutdown":true,"realModelCalls":0
        }),
    )
    .unwrap();
}
