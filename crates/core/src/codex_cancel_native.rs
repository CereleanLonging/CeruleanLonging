//! Ignored native fixture: production worker/process/IPC, synthetic protocol peer.
use super::*;
use cerulean_domain::project_setup::{ProjectSetupDraft, RunState};
use std::{io::Write, time::Duration};

fn create_json(path: &Path, value: serde_json::Value) {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    file.write_all(&serde_json::to_vec_pretty(&value).unwrap())
        .unwrap();
    file.sync_all().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "native cancellation through production worker and IPC; synthetic peer, no provider calls"]
async fn native_codex_cancel_service_fixture() {
    let outer = PathBuf::from(
        std::env::var_os("CERULEAN_CANCEL_NATIVE_DIR").expect("fresh native fixture directory"),
    )
    .canonicalize()
    .unwrap();
    let reports = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../reports")
        .canonicalize()
        .unwrap();
    assert!(outer.starts_with(&reports) && outer != reports);
    ordinary(&outer, true).unwrap();
    let mode = std::env::var("CERULEAN_CANCEL_MODE").unwrap_or_else(|_| "interrupted".into());
    assert!(matches!(
        mode.as_str(),
        "interrupted" | "completed" | "ack-lost" | "empty-interrupted" | "empty-ack-lost"
    ));
    let helper = outer.join("bin/codex.exe");
    ordinary(helper.parent().unwrap(), true).unwrap();
    ordinary(&helper, false).unwrap();
    assert!(!outer.join("data").exists() && !outer.join("project").exists());
    let project = outer.join("project");
    fs::create_dir(&project).unwrap();
    create_json(
        &outer.join("bin/fixture-config.json"),
        serde_json::json!({"version":1,"mode":mode,"workspace":project}),
    );
    let data = outer.join("data");
    let mut host = RuntimeHost::open(&data).unwrap();
    assert!(!host.openai_connection.key_configured);
    host.register(Project {
        id: "project".into(),
        root: project.to_string_lossy().into_owned(),
        timezone: "Asia/Shanghai".into(),
    })
    .unwrap();
    host.register_validator(
        "tests".into(),
        "Unused synthetic cancellation validator".into(),
        std::env::current_exe()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        vec![],
        Default::default(),
    )
    .unwrap();
    host.configure_project(ProjectSetupDraft {
        chief_directory: None,
        management_sources: None,
        resource_keys: vec![],
        profiles: vec![],
        worktree: None,
        desktop_worker: None,
        api_worker: None,
        omp_runtime_parent: None,
        worker_adapter: Default::default(),
        supervision: None,
        project_id: "project".into(),
        requirement: "Implement a sum function".into(),
        codex_executable: helper.to_string_lossy().into_owned(),
        worker_model: "fixture-worker".into(),
        chief_model: "fixture-chief".into(),
        deputy_model: "fixture-deputy".into(),
        validator_id: "tests".into(),
        check_args: vec!["--exact".into(), "unused-native-cancel-validator".into()],
        acceptance: None,
        artifact_paths: vec!["sum.mjs".into()],
        currency_budget: None,
        worker_max_requests: 2,
        management_max_requests: 1,
        max_seconds: 180,
    })
    .unwrap();
    super::project_planning::tests::complete(
        &mut host,
        super::project_planning::tests::proposal("tests"),
    );
    let revision = host.projects["project"]
        .revision()
        .unwrap()
        .approval_digest()
        .unwrap();
    host.approve("project", &revision).unwrap();
    let setup = host.project_setups["project"].digest.clone();
    host.start_project_run("project", &revision, &setup)
        .unwrap();
    // Match the production runner's reentrancy guard while invoking its trusted
    // worker entry; no management API request is needed by this protocol test.
    host.orchestrating = true;
    let started = host.start_codex("project", "task", "cancel-native", &revision);
    host.orchestrating = false;
    started.unwrap();
    let mut running = false;
    for _ in 0..800 {
        host.poll_verifications();
        let snapshot = host.snapshot();
        if snapshot.workflows.iter().any(|workflow| {
            workflow.project.id == "project"
                && workflow.state.tasks.get("task").is_some_and(|task| {
                    task.state == TaskState::Running
                        && task.attempt.as_deref() == Some("cancel-native")
                })
        }) {
            running = true;
            break;
        }
        assert!(
            host.has_codex_activity("project"),
            "fixture worker ended before Running"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    assert!(running, "synthetic peer never produced a running turn");
    assert_eq!(host.management.statuses().len(), 1);
    assert!(host.management_active.is_empty());
    let run = host.runs.all().into_iter().next().unwrap();
    assert_eq!(run.state, RunState::Active);
    let run_id = run.id;
    assert_eq!(host.codex_cancellations().len(), 1);
    let service_dir = data.clone();
    let service = tokio::spawn(async move { crate::ipc::serve_host(&service_dir, host).await });
    let mut ready = false;
    for _ in 0..100 {
        if crate::ipc::Client::connect(&data).await.is_ok() {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(ready, "native fixture pipe was not ready");
    create_json(
        &outer.join("fixture-service.json"),
        serde_json::json!({
            "dataDirectory":data,"ownerPid":std::process::id(),
            "projectId":"project","taskId":"task","attemptId":"cancel-native",
            "runId":run_id,"mode":mode,"protocolLog":outer.join("bin/protocol-log.jsonl"),
            "privateState":outer.join("bin/.fixture-state.json"),
            "helperExecutable":helper,"realModelCalls":0,
            "note":"Production async worker and IPC with a synthetic protocol peer; not a real Codex cancellation claim"
        }),
    );
    service.await.unwrap().unwrap();
}
