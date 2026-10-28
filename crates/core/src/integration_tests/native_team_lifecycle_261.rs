//! Native UI owns the original import/approval/run; only transport is synthetic.
use super::*;
use std::sync::OnceLock;

static BLUEPRINT: OnceLock<crate::team_materials::TeamBlueprint> = OnceLock::new();

fn outer() -> PathBuf {
    PathBuf::from(std::env::var_os("CERULEAN_NATIVE_TEAM_261").unwrap())
}

fn manager_fixture(
    intent: crate::management_execution::DecisionIntent,
) -> Result<DecisionObservation, String> {
    let mut result = manager(intent)?;
    if let ManagementDecision::Plan { tasks, .. } = &mut result.envelope.decision {
        *tasks = BLUEPRINT.get().unwrap().tasks.clone();
    }
    Ok(result)
}

fn worker_fixture(request: &PreparedRequest, round: u32) -> Result<ApiReply, ApiFailure> {
    let messages = fixture_messages(request);
    let ApiMessage::User(prompt) = &messages[0] else {
        panic!("original worker prompt missing")
    };
    let branch = if prompt.contains("Fixed task: left.") {
        Some("left")
    } else if prompt.contains("Fixed task: right.") {
        Some("right")
    } else {
        None
    };
    if let Some(branch) = branch.filter(|_| round == 1) {
        let out = outer();
        crate::api_execution::publish(
            &out,
            &format!("worker-{branch}-entered.json"),
            &json!({"branch":branch,"round":round,"pid":std::process::id(),"synthetic":true}),
        )
        .unwrap();
        // This callback runs on the existing dedicated API worker thread, never
        // the IPC thread. The harness releases the original response after UI
        // reconnect; an expired observation fails once and cannot replay it.
        let began = Instant::now();
        while !out.join("release-original-workers.json").exists() {
            if began.elapsed() >= Duration::from_secs(50) {
                return Err(ApiFailure::InvalidResponse {});
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        crate::api_execution::publish(
            &out,
            &format!("worker-{branch}-released.json"),
            &json!({"branch":branch,"heldMs":began.elapsed().as_millis(),"synthetic":true}),
        )
        .unwrap();
    }
    crate::workload_fixture_252::worker(request, round)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "261 native UI team lifecycle, fresh synthetic fixture only; no provider I/O"]
async fn team_lifecycle_native_owned_service() {
    let out = outer();
    let slot = std::env::var("CERULEAN_COVERAGE_252_SLOT").unwrap();
    let project = format!("eff-{slot}");
    let existing_materials = std::env::var_os("CERULEAN_NATIVE_TEAM_261_MATERIALS");
    let materials = existing_materials
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| out.clone());
    let request = serde_json::from_slice(
        &fs::read(materials.join("fixtures").join(slot).join("request.json")).unwrap(),
    )
    .unwrap();
    let blueprint = crate::team_materials::compose(&request).unwrap();
    let root = PathBuf::from(&blueprint.request.material.data_root);
    assert!(BLUEPRINT.set(blueprint).is_ok());
    if existing_materials.is_none() {
        assert!(
            !root.exists(),
            "fresh fixture only; no original task replay"
        );
        drop(crate::Store::open(&root).unwrap());
        let backup = out.join("initial-cold-backup");
        let manifest = crate::backup::create(&root, &backup)
            .unwrap()
            .manifest_digest;
        let review = crate::experiment_guard::review(&root, &backup, &manifest).unwrap();
        crate::experiment_guard::promote(&root, &backup, &manifest, &review.digest).unwrap();
    }
    let mut host = RuntimeHost::open(&root).unwrap();
    if existing_materials.is_some() {
        // Continue only the already imported, never-planned original project.
        // Native import timed out; replaying it or any task is forbidden.
        let snapshot = host.snapshot();
        assert_eq!(snapshot.team_timelines.len(), 1);
        assert_eq!(snapshot.team_timelines[0].phase, "prepared");
        assert!(snapshot.team_timelines[0].start_digest.is_none());
        assert!(snapshot.project_runs.is_empty());
        assert_eq!(snapshot.workflows.len(), 1);
        assert!(snapshot.workflows[0].revision.is_none());
        assert!(host.management.statuses().is_empty());
        assert!(snapshot.api_jobs.is_empty());
        crate::api_execution::publish(&out, "original-prepared-snapshot.json", &snapshot).unwrap();
    }
    host.management_fixture = Some(manager_fixture);
    host.api_fixture = Some(worker_fixture);
    // Clearly invalid test values, confined to this new Host. Native driver
    // removes them through original commands before authenticated shutdown.
    team_materials_250::keys(&mut host, true);
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
        &json!({"pid":std::process::id(),"dataRoot":root,"syntheticOnly":true,"userCredentialsRead":0}),
    )
    .unwrap();
    service.await.unwrap().unwrap();
    let inspection = crate::host::team_observation::timing::inspect(
        &root,
        &project,
        &fs::read_to_string(out.join("original-start-digest.txt")).unwrap(),
    )
    .unwrap();
    crate::api_execution::publish(&out, "fixture-final-inspection.json", &inspection).unwrap();
    crate::api_execution::publish(
        &out,
        "fixture-service-stopped.json",
        &json!({"pid":std::process::id(),"normalShutdown":true,"realModelCalls":0}),
    )
    .unwrap();
}
