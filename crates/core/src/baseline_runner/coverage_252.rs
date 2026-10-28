use super::*;
use crate::workload_fixture_252 as fixture;
use cerulean_domain::secret::SecretInput;

fn authorization(spec: &RunSpec) -> RunApproval {
    RunApproval {
        spec_digest: digest(spec).unwrap(),
        approval: Approval {
            plan_id: spec.plan.workflow.id.clone(),
            revision: 1,
            policy_version: 1,
            revision_digest: spec.plan.approval_digest().unwrap(),
            user_id: "user".into(),
            approved_at: chrono::Utc::now().to_rfc3339(),
        },
    }
}
#[tokio::test]
#[ignore = "252 explicit isolated seed-only slot; synthetic transport, real Git/files/checker"]
async fn missing_single_workload_accepts_and_reopens() {
    assert!(fixture::slot().ends_with("-single"));
    let req =
        serde_json::from_slice(&fs::read(fixture::area().join("request.json")).unwrap()).unwrap();
    let (spec, _) = crate::baseline_materials::compose(&req).unwrap();
    assert!(spec.plan.execution[0]
        .api_worker
        .as_ref()
        .unwrap()
        .write_scope
        .is_some());
    let data = Path::new(&spec.data_root);
    let selected = digest(&spec).unwrap();
    if std::env::var_os("CERULEAN_COVERAGE_252_RESUME_PREPARED").is_some() {
        assert_eq!(fixture::slot(), "S02-r1-single");
        assert!(!data.join("started.json").exists());
        assert_eq!(inspect(data, &selected).unwrap()["status"], "prepared");
        let original: serde_json::Value =
            serde_json::from_slice(&fs::read(fixture::area().join("selection.json")).unwrap())
                .unwrap();
        assert_eq!(original["specDigest"], selected);
    } else {
        prepare(data, spec.clone()).unwrap();
        fixture::save(
            "selection.json",
            &json!({"dataRoot":data,"specDigest":selected}),
        );
    }
    let (_tx, mut rx) = oneshot::channel();
    let mut sends = 0;
    let end = execute(
        data,
        &selected,
        authorization(&spec),
        "synthetic-252",
        |_| ApiClient::new(SecretInput::new("synthetic-no-user-key".into()).unwrap()),
        &mut async |_: &ApiClient, prepared, _: &mut oneshot::Receiver<()>, _gateway: &mut cerulean_adapters::api_generation::GatewayObserver<'_>| {
            sends += 1;
            fixture::worker(&prepared, sends)
        },
        &mut rx,
        &AtomicBool::new(false),
    )
    .await
    .unwrap();
    fixture::save("terminal.json", &end);
    assert!(end.accepted, "{:?}", end.error);
    assert_eq!(
        sends,
        if fixture::slot().starts_with('P') {
            4
        } else {
            2
        }
    );
    let report = inspect(data, &selected).unwrap();
    assert_eq!(report["taskState"], "accepted");
    assert_eq!(report["managementModelRequests"], 0);
    assert_eq!(report["coreExport"]["apiReplies"], sends);
    fixture::save("inspection.json", &report);
    let terminal = fs::read(data.join("terminal.json")).unwrap();
    for _ in 0..2 {
        assert_eq!(inspect(data, &selected).unwrap()["taskState"], "accepted");
        let (_tx, mut cancel) = oneshot::channel();
        assert!(
            execute(
                data,
                &selected,
                authorization(&spec),
                "synthetic-252",
                |_| panic!("no credentials on repeat"),
                &mut async |_: &ApiClient,
                            _,
                            _: &mut oneshot::Receiver<()>,
                            _gateway: &mut cerulean_adapters::api_generation::GatewayObserver<
                    '_,
                >| panic!("no repeat request"),
                &mut cancel,
                &AtomicBool::new(false)
            )
            .await
            .is_err()
        );
    }
    assert_eq!(fs::read(data.join("terminal.json")).unwrap(), terminal);
    fixture::save(
        "accepted-result.json",
        &json!({"accepted":true,"slot":fixture::slot(),
        "workerReplies":sends,"managementOperations":0,"independentChecks":1,
        "reopensWithoutRedispatch":2,"realModelCalls":0,"writeScope":"declared-artifacts"}),
    );
}
