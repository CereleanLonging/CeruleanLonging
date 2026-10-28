use super::*;
use cerulean_domain::{
    execution::{digest, Project},
    management::{DecisionEnvelope, ManagementDecision, ProposedTask},
    management_ingress::{ExternalChiefDecision, ExternalChiefOperation as O},
};

fn setup(root: &Path) -> (RuntimeHost, String) {
    let data = root.join("data");
    drop(crate::Store::open(&data).unwrap());
    crate::management_source_guard::tests::promote_fixture(&data);
    let mut host = RuntimeHost::open(&data).unwrap();
    let project = root.join("project");
    std::fs::create_dir(&project).unwrap();
    let worker = root.join("codex.exe");
    std::fs::write(&worker, b"never executed in this test").unwrap();
    for (index, command) in [
        Command::RegisterProject {
            project: Project {
                id: "project".into(),
                root: project.to_string_lossy().into(),
                timezone: "Asia/Shanghai".into(),
            },
        },
        Command::RegisterValidator {
            id: "tests".into(),
            name: "never launched".into(),
            executable: std::env::current_exe().unwrap().to_string_lossy().into(),
            inputs: vec![],
            environment: Default::default(),
        },
    ]
    .into_iter()
    .enumerate()
    {
        let reply = host.handle(Request {
            id: format!("setup-{index}"),
            protocol_version: PROTOCOL_VERSION,
            command,
        });
        assert!(reply.error.is_none(), "{:?}", reply.error);
    }
    let draft=serde_json::from_value(serde_json::json!({"projectId":"project","requirement":"Implement sum","codexExecutable":worker,"workerModel":"fixture-worker","chiefModel":"fixture-chief","deputyModel":"fixture-deputy","validatorId":"tests","checkArgs":[],"artifactPaths":["sum.mjs"],"workerMaxRequests":1,"managementMaxRequests":4,"maxSeconds":60})).unwrap();
    let response = host.handle(Request {
        id: "configure".into(),
        protocol_version: PROTOCOL_VERSION,
        command: Command::ConfigureProject {
            draft: Box::new(draft),
        },
    });
    assert!(response.error.is_none(), "{:?}", response.error);
    let snapshot = serde_json::to_value(response.result.unwrap()).unwrap();
    // The configuration is read back through the public snapshot contract.
    let digest = snapshot["projectSetups"][0]["digest"]
        .as_str()
        .unwrap()
        .to_owned();
    (host, digest)
}
#[tokio::test]
async fn external_chief_ipc_returns_real_handoff_and_never_approves_or_resends() {
    let root = tempfile::tempdir().unwrap();
    let (host, setup_digest) = setup(root.path());
    let data = root.path().join("data");
    let owned = data.clone();
    let service = tokio::spawn(async move { serve_host(&owned, host).await });
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if Client::connect(&data).await.is_ok() {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let call = |id: &str, version, operation| UserOperation::ExternalChief {
        id: id.into(),
        protocol_version: version,
        operation,
    };
    let prepare = O::PreparePlan {
        project_id: "project".into(),
        setup_digest,
        session_id: "current-session".into(),
    };
    let wrong = Client::connect(&data)
        .await
        .unwrap()
        .call(call("bad", 999, prepare.clone()))
        .await
        .unwrap();
    assert_eq!(wrong.response.error.unwrap().code, "protocolMismatch");
    let reply = Client::connect(&data)
        .await
        .unwrap()
        .call(call("prepare", PROTOCOL_VERSION, prepare.clone()))
        .await
        .unwrap();
    assert!(reply.response.error.is_none(), "{:?}", reply.response.error);
    let result = reply.external_chief.unwrap();
    assert_eq!(result.state, "awaitingExternal");
    assert_eq!(result.request_digest, digest(&result.handoff).unwrap());
    let duplicate = Client::connect(&data)
        .await
        .unwrap()
        .call(call("duplicate", PROTOCOL_VERSION, prepare))
        .await
        .unwrap();
    assert!(duplicate.response.error.is_some());
    let decision = ExternalChiefDecision {
        request_digest: result.request_digest.clone(),
        session_id: "current-session".into(),
        envelope: DecisionEnvelope {
            state_digest: result.handoff.state_digest.clone(),
            decision: ManagementDecision::Plan {
                tasks: vec![ProposedTask {
                    profile_id: None,
                    id: "sum".into(),
                    goal: "Implement sum".into(),
                    dependencies: vec![],
                    worker_id: "worker-codex".into(),
                    validator_id: "tests".into(),
                }],
                rationale: "one task".into(),
            },
        },
    };
    let submit = O::Submit {
        operation_id: result.operation_id.clone(),
        session_id: "current-session".into(),
        decision,
    };
    let reply = Client::connect(&data)
        .await
        .unwrap()
        .call(call("submit", PROTOCOL_VERSION, submit.clone()))
        .await
        .unwrap();
    assert!(reply.response.error.is_none(), "{:?}", reply.response.error);
    assert_eq!(reply.external_chief.unwrap().state, "applied");
    let duplicate = Client::connect(&data)
        .await
        .unwrap()
        .call(call("submit-again", PROTOCOL_VERSION, submit))
        .await
        .unwrap();
    assert!(duplicate.response.error.is_some());
    let read = Client::connect(&data)
        .await
        .unwrap()
        .call(call(
            "read",
            PROTOCOL_VERSION,
            O::Read {
                operation_id: result.operation_id,
            },
        ))
        .await
        .unwrap();
    assert_eq!(read.external_chief.unwrap().state, "applied");
    let snapshot = Client::snapshot(&data).await.unwrap();
    assert!(snapshot.workflows[0].approval.is_none());
    let stopped = Client::connect(&data)
        .await
        .unwrap()
        .call(UserOperation::Shutdown {
            id: "stop".into(),
            protocol_version: PROTOCOL_VERSION,
        })
        .await
        .unwrap();
    assert!(stopped.stopping);
    tokio::time::timeout(Duration::from_secs(10), service)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}
