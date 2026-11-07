#![cfg(windows)]
use cerulean_core::{host::RuntimeHost, verifier::PinnedCheck};
use cerulean_domain::{execution::*, *};
use std::fs;

fn request(command: Command) -> Request {
    Request {
        id: "catalog-request".into(),
        protocol_version: PROTOCOL_VERSION,
        command,
    }
}
fn registration(id: &str, script: &std::path::Path) -> Command {
    Command::RegisterValidator {
        id: id.into(),
        name: "validation v1".into(),
        executable: std::env::current_exe().unwrap().to_string_lossy().into(),
        inputs: vec![script.to_string_lossy().into()],
        environment: [("CI".into(), "true".into())].into(),
    }
}

fn historical(root: &std::path::Path, id: &str, script: &std::path::Path) -> Vec<u8> {
    let tool = PinnedCheck::describe(
        &std::env::current_exe().unwrap(),
        &[script.into()],
        Default::default(),
    )
    .unwrap();
    let item = RegisteredValidator {
        id: id.into(),
        name: "Historical tool".into(),
        tool,
        registered_by: "user".into(),
        registered_at: "2026-11-04T00:00:00Z".into(),
    };
    let bytes = serde_json::to_vec(
        &serde_json::json!({"schemaVersion":1,"digest":digest(&item).unwrap(),"registration":item}),
    )
    .unwrap();
    fs::write(root.join("validators").join(format!("{id}.json")), &bytes).unwrap();
    bytes
}
#[test]
fn fixed_checker_rejects_registration_and_keeps_historical_bindings_readable() {
    let data = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let script = external.path().join("original.txt");
    fs::write(&script, "original").unwrap();
    let mut host = RuntimeHost::open(data.path()).unwrap();
    assert!(host
        .handle(request(registration("tool-1", &script)))
        .error
        .is_some());
    assert!(!data.path().join("validators/tool-1.json").exists());
    assert_eq!(host.snapshot().validators.len(), 1, "Build checker first");
    assert_eq!(
        host.snapshot().validators[0].id,
        cerulean_domain::objective_checker::ID
    );
    drop(host);
    let saved = historical(data.path(), "legacy-tool", &script);
    let mut host = RuntimeHost::open(data.path()).unwrap();
    let tool = host
        .registered_validator("legacy-tool")
        .unwrap()
        .tool
        .clone();
    let check = AcceptanceCheck {
        id: "old-check".into(),
        name: None,
        program: tool.executable.path.clone(),
        args: vec![],
        expected_assertions: vec![],
        timeout_seconds: 10,
        tool: Some(tool),
    };
    PinnedCheck::from_approved(&check).unwrap();
    fs::write(&script, "changed").unwrap();
    assert!(PinnedCheck::from_approved(&check).is_err());
    assert!(host
        .handle(request(registration("another-tool", &script)))
        .error
        .is_some());
    assert_eq!(host.snapshot().validators.len(), 1);
    assert_eq!(
        saved,
        fs::read(data.path().join("validators/legacy-tool.json")).unwrap()
    );
    drop(host);
    let mut record: serde_json::Value = serde_json::from_slice(&saved).unwrap();
    record["registration"]["name"] = "tampered".into();
    fs::write(
        data.path().join("validators/legacy-tool.json"),
        serde_json::to_vec(&record).unwrap(),
    )
    .unwrap();
    assert!(RuntimeHost::open(data.path()).is_err());
}
#[test]
fn fixed_checker_configuration_accepts_template_but_requires_test_before_execution() {
    use cerulean_domain::objective_checker::*;
    let data = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let chief = tempfile::tempdir().unwrap();
    let mut host = RuntimeHost::open(data.path()).unwrap();
    host.handle(request(Command::RegisterProject {
        project: Project {
            id: "project".into(),
            root: project.path().to_string_lossy().into(),
            timezone: "UTC".into(),
        },
    }))
    .result
    .unwrap();
    let value = serde_json::json!({"projectId":"project","requirement":"Implement and test sums","workerAdapter":"deepseek","workerModel":"fixture","chiefModel":"chief-fixture","deputyModel":"deputy-fixture","codexExecutable":"","chiefDirectory":chief.path().to_string_lossy(),"apiWorker":{"target":{"kind":"provider","endpoint":{"kind":"deepSeek"}},"model":"fixture","fileTools":["read-files","write-files"],"maxRounds":2,"maxToolCalls":4,"maxOutputTokens":1024,"maxTotalTokens":8000,"requestTimeoutSeconds":30},"validatorId":ID,"checkArgs":[],"artifactPaths":["sum.mjs"],"workerMaxRequests":3,"managementMaxRequests":10,"maxSeconds":60,"acceptance":{"schemaVersion":1,"stages":[{"id":"test","name":"Objective check","validatorId":ID,"args":[],"expectedAssertions":[],"timeoutSeconds":20}]}});
    let draft = serde_json::from_value(value).unwrap();
    let response = host.handle(request(Command::ConfigureProject { draft }));
    assert!(response.error.is_none(), "{:?}", response.error);
    let snapshot = host.snapshot();
    let setup = &snapshot.project_setups[0].setup;
    let mut task = cerulean_domain::management::ProposedTask {
        artifact_paths: None,
        objective_test: None,
        profile_id: None,
        id: "t".into(),
        goal: "sum".into(),
        dependencies: vec![],
        worker_id: "worker-deepseek".into(),
        validator_id: ID.into(),
    };
    assert!(bind_execution(setup, &task, &Default::default()).is_err());
    task.objective_test = Some(ScriptTest {
        language: Language::JavaScript,
        source: "reviewed test".into(),
        assertions: vec!["sum".into()],
    });
    assert!(bind_execution(setup, &task, &Default::default()).is_err());
    let runtimes = [(
        Language::JavaScript,
        ValidationFile {
            path: "D:\\tools\\node.exe".into(),
            sha256: "a".repeat(64),
        },
    )]
    .into_iter()
    .collect();
    let execution = bind_execution(setup, &task, &runtimes).unwrap();
    assert!(matches_setup(setup, &execution).unwrap());
    let mut altered = execution.clone();
    altered.artifact_paths = vec!["other.mjs".into()];
    assert!(!matches_setup(setup, &altered).unwrap());
    assert!(setup.supervision_for_execution(&execution).is_ok());
    drop(host);
    let restored = RuntimeHost::open(data.path()).unwrap();
    assert_eq!(
        restored.snapshot().project_setups[0].digest,
        snapshot.project_setups[0].digest
    );
    assert_eq!(
        cerulean_core::data_format::status(data.path())
            .unwrap()
            .minimum_reader_generation,
        22
    );
}

#[test]
fn planned_deliverables_require_chief_evidence_and_reader23() {
    use cerulean_domain::objective_checker::*;
    let data = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let chief = tempfile::tempdir().unwrap();
    let mut host = RuntimeHost::open(data.path()).unwrap();
    host.handle(request(Command::RegisterProject {
        project: Project {
            id: "project".into(),
            root: project.path().to_string_lossy().into(),
            timezone: "UTC".into(),
        },
    }))
    .result
    .unwrap();
    let value = serde_json::json!({"projectId":"project","requirement":"Implement and test sums","workerAdapter":"deepseek","workerModel":"fixture","chiefModel":"chief-fixture","deputyModel":"deputy-fixture","codexExecutable":"","chiefDirectory":chief.path().to_string_lossy(),"apiWorker":{"target":{"kind":"provider","endpoint":{"kind":"deepSeek"}},"model":"fixture","fileTools":["read-files","write-files"],"maxRounds":2,"maxToolCalls":4,"maxOutputTokens":1024,"maxTotalTokens":8000,"requestTimeoutSeconds":30},"validatorId":ID,"checkArgs":[],"artifactPaths":[],"workerMaxRequests":3,"managementMaxRequests":10,"maxSeconds":60,"acceptance":{"schemaVersion":1,"stages":[{"id":"test","name":"Objective check","validatorId":ID,"args":[],"expectedAssertions":[],"timeoutSeconds":20}]}});
    let mut value = value;
    value["profiles"] = serde_json::json!([{
        "id":"extra", "description":"Chosen by the approved plan", "workerAdapter":"deepseek",
        "workerModel":"fixture-extra", "codexExecutable":"", "artifactPaths":[],
        "apiWorker":value["apiWorker"].clone(), "acceptance":value["acceptance"].clone()
    }]);
    value["profiles"][0]["apiWorker"]["model"] = "fixture-extra".into();
    let draft = serde_json::from_value(value).unwrap();
    let response = host.handle(request(Command::ConfigureProject { draft }));
    assert!(response.error.is_none(), "{:?}", response.error);
    let snapshot = host.snapshot();
    let setup = &snapshot.project_setups[0].setup;
    let mut task = cerulean_domain::management::ProposedTask {
        artifact_paths: None,
        objective_test: None,
        profile_id: None,
        id: "t".into(),
        goal: "sum".into(),
        dependencies: vec![],
        worker_id: "worker-deepseek".into(),
        validator_id: ID.into(),
    };
    assert!(bind_execution(setup, &task, &Default::default()).is_err());
    task.objective_test = Some(ScriptTest {
        language: Language::JavaScript,
        source: "reviewed test".into(),
        assertions: vec!["sum".into()],
    });
    assert!(bind_execution(setup, &task, &Default::default()).is_err());
    let runtimes = [(
        Language::JavaScript,
        ValidationFile {
            path: "D:\\tools\\node.exe".into(),
            sha256: "a".repeat(64),
        },
    )]
    .into_iter()
    .collect();
    assert!(bind_execution(setup, &task, &runtimes).is_err());
    for invalid in [
        vec![],
        vec!["../escape".into()],
        vec!["a.mjs".into(), "A.mjs".into()],
        vec!["x".into(); 65],
    ] {
        task.artifact_paths = Some(invalid);
        assert!(bind_execution(setup, &task, &runtimes).is_err());
    }
    task.artifact_paths = Some(vec!["sum.mjs".into()]);
    let execution = bind_execution(setup, &task, &runtimes).unwrap();
    assert_eq!(execution.artifact_paths, vec!["sum.mjs"]);
    let original = digest(&execution).unwrap();
    task.profile_id = Some("extra".into());
    let extra = bind_execution(setup, &task, &runtimes).unwrap();
    assert_eq!(extra.api_worker.unwrap().model, "fixture-extra");
    assert_eq!(extra.artifact_paths, vec!["sum.mjs"]);
    task.profile_id = None;
    task.artifact_paths = Some(vec!["changed.mjs".into()]);
    assert_ne!(
        original,
        digest(&bind_execution(setup, &task, &runtimes).unwrap()).unwrap()
    );
    assert!(matches_setup(setup, &execution).unwrap());
    let mut altered = execution.clone();
    altered.artifact_paths = vec!["other.mjs".into()];
    assert!(matches_setup(setup, &altered).unwrap()); // template comparison only
    altered.artifact_paths = vec![];
    assert!(matches_setup(setup, &altered).is_err());
    assert!(setup.supervision_for_execution(&execution).is_ok());
    drop(host);
    let restored = RuntimeHost::open(data.path()).unwrap();
    assert_eq!(
        restored.snapshot().project_setups[0].digest,
        snapshot.project_setups[0].digest
    );
    assert_eq!(
        cerulean_core::data_format::status(data.path())
            .unwrap()
            .minimum_reader_generation,
        23
    );
    drop(restored);
    fs::remove_file(data.path().join("events/00000000.reader23.json")).unwrap();
    assert!(
        RuntimeHost::open(data.path()).is_err(),
        "empty configured paths require reader23 on reopen"
    );
}
