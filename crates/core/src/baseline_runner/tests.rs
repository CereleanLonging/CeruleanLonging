use super::*;
use crate::api_execution::tests::{baseline_plan_fixture, baseline_reply_fixture};
use cerulean_domain::secret::SecretInput;

fn spec(root: &Path) -> RunSpec {
    let mut plan = baseline_plan_fixture(root);
    plan.project.id = "eff-S01-r1-single".into();
    plan.workflow.tasks[0].workspace_key = plan.project.id.clone();
    RunSpec {
        material: None,
        schema: 1,
        slot_id: "S01-r1-single".into(),
        data_root: root.join("data").to_string_lossy().into(),
        material_manifest_sha256: "a".repeat(64),
        configuration_sha256: "b".repeat(64),
        seed: observe(
            Path::new(&plan.project.root),
            plan.snapshot_policy.as_ref().unwrap(),
        )
        .unwrap(),
        plan,
    }
}
fn approval(spec: &RunSpec) -> RunApproval {
    RunApproval {
        spec_digest: digest(spec).unwrap(),
        approval: Approval {
            plan_id: spec.plan.workflow.id.clone(),
            revision: spec.plan.workflow.version,
            policy_version: spec.plan.policy.version,
            revision_digest: spec.plan.approval_digest().unwrap(),
            user_id: "user".into(),
            approved_at: chrono::Utc::now().to_rfc3339(),
        },
    }
}
fn client(_: &RunSpec) -> Result<ApiClient, String> {
    ApiClient::new(SecretInput::new("synthetic-baseline-no-key".into()).unwrap())
}
fn prepared(root: &Path, spec: &RunSpec) -> PathBuf {
    let data = root.join("data");
    prepare(&data, spec.clone()).unwrap();
    data
}
fn original_evidence(mut value: Value) -> Value {
    // This is the time of this read, not a timestamp in the original ledger.
    for check in value["coreExport"]["verifications"].as_array_mut().unwrap() {
        check["detail"]
            .as_object_mut()
            .unwrap()
            .remove("observedAt");
    }
    value
}
async fn synthetic(data: &Path, spec: &RunSpec, mode: &str) -> (Result<Terminal, String>, usize) {
    let (_tx, mut rx) = oneshot::channel();
    let mut sends = 0;
    let result = execute(
        data,
        &digest(spec).unwrap(),
        approval(spec),
        "synthetic-fixture",
        client,
        &mut async |_: &ApiClient, request, _: &mut oneshot::Receiver<()>, _gateway: &mut cerulean_adapters::api_generation::GatewayObserver<'_>| {
            sends += 1;
            if mode == "failure" {
                Err(ApiFailure::InvalidResponse {})
            } else {
                baseline_reply_fixture(&request, sends)
            }
        },
        &mut rx,
        &AtomicBool::new(false),
    )
    .await;
    (result, sends)
}
#[test]
fn prepare_is_unapproved_immutable_and_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let s = spec(dir.path());
    let data = prepared(dir.path(), &s);
    let result = inspect(&data, &digest(&s).unwrap()).unwrap();
    assert_eq!(result["status"], "prepared");
    assert_eq!(result["coreExport"]["apiIntentCount"], 0);
    assert_eq!(result["managementModelRequests"], 0);
    assert!(result["started"].is_null());
    assert!(prepare(&data, s.clone()).is_err());
    assert!(prepare(&dir.path().join("other-data"), s.clone()).is_err());
    assert!(inspect(&data, &"0".repeat(64)).is_err());
}

#[cfg(windows)]
#[tokio::test]
async fn gateway_324_a6_prepared_fixture_execute_and_read_preserve_gateway_binding() {
    // Keep ordinary success cleanup; any assertion/unwrap failure retains the
    // exact fixture, including failures after terminal.accepted.
    let temp = Gateway324RetainOnFailure(Some(tempfile::tempdir().unwrap()));
    gateway_324_a6_fixture(temp.0.as_ref().unwrap().path()).await;
}

#[cfg(windows)]
struct Gateway324RetainOnFailure(Option<tempfile::TempDir>);
#[cfg(windows)]
impl Drop for Gateway324RetainOnFailure {
    fn drop(&mut self) {
        if std::thread::panicking() {
            if let Some(temp) = self.0.take() {
                eprintln!(
                    "gateway 324 original failed fixture retained at {:?}",
                    temp.keep()
                );
            }
        }
    }
}

#[cfg(windows)]
async fn gateway_324_a6_fixture(root: &Path) -> (PathBuf, RunSpec, Value) {
    use cerulean_adapters::api_generation::{
        gateway::{GatewayHeaders, GatewayRequestId},
        ApiTarget, GatewayObserver,
    };
    use cerulean_domain::credentials::ProviderEndpoint;
    let mut s = spec(root);
    s.plan.execution[0].adapter_id = "a6api".into();
    s.plan.execution[0].api_worker.as_mut().unwrap().target = ApiTarget::Provider {
        endpoint: ProviderEndpoint::A6Api {},
    };
    s.validate().unwrap();
    // Reuse the real prepare entry. Requires 326's new-root reader16 fix.
    let data = prepared(root, &s);
    let workflow = data.join("workflows").join(&s.plan.project.id);
    let (_keep, mut cancel) = oneshot::channel();
    let mut sends = 0;
    let terminal = execute(
        &data,
        &digest(&s).unwrap(),
        approval(&s),
        "synthetic-fixture",
        client,
        &mut async |_: &ApiClient,
                    request,
                    _: &mut oneshot::Receiver<()>,
                    gateway: &mut GatewayObserver<'_>| {
            sends += 1;
            gateway(&GatewayHeaders {
                status: 200,
                request_id: GatewayRequestId::Accepted {
                    value: format!("gateway-baseline-{sends}"),
                },
            });
            baseline_reply_fixture(&request, sends)
        },
        &mut cancel,
        &AtomicBool::new(false),
    )
    .await
    .unwrap();
    if !terminal.accepted {
        let observed = inspect(&data, &digest(&s).unwrap());
        // The ordinary test's guard retains its TempDir on panic; the fixed
        // opt-in case is never automatically deleted. Preserve both paths.
        panic!("synthetic baseline failed: {terminal:?}; readback={observed:?}; retained={root:?}");
    }
    assert_eq!(sends, 2);
    let report = inspect(&data, &digest(&s).unwrap()).unwrap();
    assert_eq!(report["managementModelRequests"], 0);
    assert_eq!(report["coreExport"]["apiReplies"], 2);
    let original = fs::read(data.join("terminal.json")).unwrap();
    let store = DurableWorkflow::open(&workflow, s.config()).unwrap();
    let attempt = store.api_attempt_path("baseline-attempt").unwrap();
    crate::api_execution::CallInventory::read(&store)
        .unwrap()
        .check(None)
        .unwrap();
    for round in 1..=2 {
        let journal = crate::api_call_journal::ApiCallJournal::read_existing(
            &attempt.join(format!("rounds/{round:03}")),
        )
        .unwrap();
        let review = journal.gateway_evidence(&attempt).unwrap().unwrap();
        assert_eq!(review.status, "linked");
        assert_eq!(
            review.gateway_request_id,
            Some(format!("gateway-baseline-{round}"))
        );
        assert_eq!(review.local_id, journal.identity().request_id);
        assert_ne!(review.gateway_request_id, review.body_response_id);
    }
    drop(store);
    for _ in 0..2 {
        assert_eq!(
            original_evidence(inspect(&data, &digest(&s).unwrap()).unwrap()),
            original_evidence(report.clone())
        );
        let (again, calls) = synthetic(&data, &s, "ok").await;
        assert!(again.is_err());
        assert_eq!(calls, 0);
        assert_eq!(fs::read(data.join("terminal.json")).unwrap(), original);
    }
    assert_eq!(
        crate::data_format::status(&data)
            .unwrap()
            .minimum_reader_generation,
        16
    );
    let expected = original_evidence(inspect(&data, &digest(&s).unwrap()).unwrap());
    (data, s, expected)
}
#[tokio::test]
async fn full_run_binds_sources_and_timing_and_never_reexecutes() {
    let temp = tempfile::tempdir().unwrap();
    let retained = std::env::var_os("CERULEAN_BASELINE_247_DIR").map(PathBuf::from);
    let root = retained.as_deref().unwrap_or(temp.path());
    if retained.is_some() {
        fs::create_dir(root).unwrap();
    }
    let s = spec(root);
    let data = prepared(root, &s);
    let (result, sends) = synthetic(&data, &s, "ok").await;
    let terminal = result.unwrap();
    assert!(terminal.accepted);
    assert_eq!(sends, 2);
    assert!(terminal.elapsed_ms > 0);
    let report = inspect(&data, &digest(&s).unwrap()).unwrap();
    assert_eq!(report["coreExport"]["apiReplies"], 2);
    assert_eq!(
        report["coreExport"]["verifications"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(report["coreExport"]["managementUsageIncluded"], true);
    assert_eq!(report["started"]["transport"], "synthetic-fixture");
    assert_eq!(report["realityAttested"], false);
    assert_eq!(report["experimentRecordReady"], false);
    let original = fs::read(data.join("terminal.json")).unwrap();
    for _ in 0..2 {
        assert_eq!(
            original_evidence(inspect(&data, &digest(&s).unwrap()).unwrap()),
            original_evidence(report.clone())
        );
        let (again, calls) = synthetic(&data, &s, "ok").await;
        assert!(again.is_err());
        assert_eq!(calls, 0);
        assert_eq!(fs::read(data.join("terminal.json")).unwrap(), original);
    }
    if retained.is_some() {
        publish(root, "inspection.json", &report).unwrap();
        publish(root,"selection.json",&json!({"data":data,"specDigest":digest(&s).unwrap(),
            "project":s.plan.project.id,"revision":terminal.revision_digest,"head":terminal.ledger_head})).unwrap();
    }
}

#[tokio::test]
#[ignore = "247 original completed run, read-only inspection and rejected repeat calls only"]
async fn recover_original_completed_without_dispatch() {
    let root = PathBuf::from(std::env::var_os("CERULEAN_BASELINE_247_DIR").unwrap());
    let data = root.join("data");
    let s: RunSpec = load(&data, "run-spec.json").unwrap();
    let end: Terminal = load(&data, "terminal.json").unwrap();
    assert!(end.accepted);
    let report = inspect(&data, &digest(&s).unwrap()).unwrap();
    assert_eq!(report["coreExport"]["apiReplies"], 2);
    assert_eq!(
        report["coreExport"]["verifications"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    for _ in 0..2 {
        assert_eq!(
            original_evidence(inspect(&data, &digest(&s).unwrap()).unwrap()),
            original_evidence(report.clone())
        );
        let (result, sends) = synthetic(&data, &s, "ok").await;
        assert!(result.is_err());
        assert_eq!(sends, 0);
    }
    publish(&root, "inspection.json", &report).unwrap();
    publish(
        &root,
        "selection.json",
        &json!({"data":data,"specDigest":digest(&s).unwrap(),
        "project":s.plan.project.id,"revision":end.revision_digest,"head":end.ledger_head}),
    )
    .unwrap();
}
#[tokio::test]
async fn seed_change_blocks_before_credentials_intent_and_requests() {
    let dir = tempfile::tempdir().unwrap();
    let s = spec(dir.path());
    let data = prepared(dir.path(), &s);
    fs::write(
        Path::new(&s.plan.project.root).join("unexpected.txt"),
        "changed",
    )
    .unwrap();
    let (_tx, mut rx) = oneshot::channel();
    let result = execute(
        &data,
        &digest(&s).unwrap(),
        approval(&s),
        "synthetic",
        |_| panic!("credentials must not be read"),
        &mut async |_: &ApiClient, _, _: &mut oneshot::Receiver<()>, _gateway: &mut cerulean_adapters::api_generation::GatewayObserver<'_>| panic!("must not send"),
        &mut rx,
        &AtomicBool::new(false),
    )
    .await;
    assert!(result.unwrap_err().contains("seed"));
    assert!(!data.join("started.json").exists());
}
#[tokio::test]
async fn wrong_approval_and_active_lock_cannot_start() {
    let dir = tempfile::tempdir().unwrap();
    let s = spec(dir.path());
    let data = prepared(dir.path(), &s);
    let mut bad = approval(&s);
    bad.spec_digest = "0".repeat(64);
    let (_tx, mut rx) = oneshot::channel();
    assert!(execute(
        &data,
        &digest(&s).unwrap(),
        bad,
        "synthetic",
        |_| panic!("no credentials"),
        &mut async |_: &ApiClient, _, _: &mut oneshot::Receiver<()>, _gateway: &mut cerulean_adapters::api_generation::GatewayObserver<'_>| panic!("no request"),
        &mut rx,
        &AtomicBool::new(false)
    )
    .await
    .is_err());
    assert!(!data.join("started.json").exists());
    let held = lock(&data, false).unwrap();
    assert!(inspect(&data, &digest(&s).unwrap()).is_err());
    assert!(synthetic(&data, &s, "ok").await.0.is_err());
    drop(held);
    assert_eq!(
        inspect(&data, &digest(&s).unwrap()).unwrap()["status"],
        "prepared"
    );
}
#[tokio::test]
async fn partial_intent_is_retained_and_never_retried() {
    let dir = tempfile::tempdir().unwrap();
    let s = spec(dir.path());
    let data = prepared(dir.path(), &s);
    fs::write(data.join("started.json.pending"), b"{").unwrap();
    let (result, calls) = synthetic(&data, &s, "ok").await;
    assert!(result.is_err());
    assert_eq!(calls, 0);
    assert!(inspect(&data, &digest(&s).unwrap()).is_err());
    assert_eq!(fs::read(data.join("started.json.pending")).unwrap(), b"{");
}
#[tokio::test]
async fn credential_failure_has_terminal_without_send_and_blocks_retry() {
    let dir = tempfile::tempdir().unwrap();
    let s = spec(dir.path());
    let data = prepared(dir.path(), &s);
    let (_tx, mut rx) = oneshot::channel();
    let end = execute(
        &data,
        &digest(&s).unwrap(),
        approval(&s),
        "synthetic",
        |_| Err("synthetic credential unavailable".into()),
        &mut async |_: &ApiClient, _, _: &mut oneshot::Receiver<()>, _gateway: &mut cerulean_adapters::api_generation::GatewayObserver<'_>| panic!("must not send"),
        &mut rx,
        &AtomicBool::new(false),
    )
    .await
    .unwrap();
    assert!(!end.accepted);
    assert_eq!(end.task_state, TaskState::Pending);
    assert_eq!(
        inspect(&data, &digest(&s).unwrap()).unwrap()["coreExport"]["apiIntentCount"],
        0
    );
    assert!(synthetic(&data, &s, "ok").await.0.is_err());
}
#[tokio::test]
async fn provider_failure_is_read_only_and_retains_unknown_usage() {
    let dir = tempfile::tempdir().unwrap();
    let s = spec(dir.path());
    let data = prepared(dir.path(), &s);
    let (result, calls) = synthetic(&data, &s, "failure").await;
    assert!(!result.unwrap().accepted);
    assert_eq!(calls, 1);
    let before = fs::read(data.join("terminal.json")).unwrap();
    let report = inspect(&data, &digest(&s).unwrap()).unwrap();
    assert_eq!(report["coreExport"]["apiUncertainRemoteUsage"], 1);
    assert!(report["coreExport"]["apiCalls"][0]["usage"].is_null());
    let (retry, calls) = synthetic(&data, &s, "ok").await;
    assert!(retry.is_err());
    assert_eq!(calls, 0);
    assert_eq!(fs::read(data.join("terminal.json")).unwrap(), before);
}
#[tokio::test]
async fn altered_terminal_and_missing_management_inventory_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let s = spec(dir.path());
    let data = prepared(dir.path(), &s);
    synthetic(&data, &s, "ok").await.0.unwrap();
    let mut end: Terminal = load(&data, "terminal.json").unwrap();
    end.start_digest = "0".repeat(64);
    fs::write(
        data.join("terminal.json"),
        serde_json::to_vec(&end).unwrap(),
    )
    .unwrap();
    assert!(inspect(&data, &digest(&s).unwrap()).is_err());
    let other = tempfile::tempdir().unwrap();
    let s = spec(other.path());
    let data = prepared(other.path(), &s);
    fs::rename(
        data.join("management-decisions"),
        data.join("management-held"),
    )
    .unwrap();
    assert!(inspect(&data, &digest(&s).unwrap()).is_err());
}
#[test]
fn overlapping_root_and_extra_snapshot_exclusion_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = spec(dir.path());
    assert!(prepare(&Path::new(&s.plan.project.root).join("data"), s.clone()).is_err());
    s.plan
        .snapshot_policy
        .as_mut()
        .unwrap()
        .exclude
        .push("hidden".into());
    assert!(prepare(&dir.path().join("data"), s).is_err());
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "explicit fixed-path compatibility export; never automatic"]
async fn gateway_324_export_api_compatibility_fixture() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let parent = repository.join("reports/gateway-id-324/compatibility");
    for ancestor in parent.ancestors() {
        crate::host::ordinary(ancestor, true).unwrap();
    }
    let case = fs::canonicalize(parent).unwrap().join("case-api");
    fs::create_dir(&case).unwrap();
    let (data, spec, expected) = gateway_324_a6_fixture(&case).await;
    let archive = crate::backup::create(&data, &case.join("current-backup")).unwrap();
    crate::api_execution::publish(&case, "export.json", &serde_json::json!({
        "schema":1,"lane":"api","synthetic":true,"realModelCalls":0,
        "dataRoot":fs::canonicalize(&data).unwrap(),"ownerSid":crate::ipc::current_sid().unwrap(),
        "specDigest":digest(&spec).unwrap(),"projectId":spec.plan.project.id,
        "readerGeneration":16,"backupDigest":archive.manifest_digest,
        "expectedOriginalInspection":expected,
        "currentTestExecutable":std::env::current_exe().unwrap()
    })).unwrap();
}

#[cfg(windows)]
#[test]
#[ignore = "explicit current-reader check after fixed-path old-reader cold restore"]
fn gateway_324_check_api_compatibility_restore() {
    let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let case = repository.join("reports/gateway-id-324/compatibility/case-api");
    for ancestor in case.ancestors() {
        crate::host::ordinary(ancestor, true).unwrap();
    }
    let case = fs::canonicalize(case).unwrap();
    let export: Value = crate::api_execution::load(&case, "export.json").unwrap();
    assert_eq!(export["synthetic"], true);
    let data = case.join("data");
    assert_eq!(
        export["dataRoot"],
        serde_json::to_value(fs::canonicalize(&data).unwrap()).unwrap()
    );
    assert_eq!(export["ownerSid"], crate::ipc::current_sid().unwrap());
    let old: Value = crate::api_execution::load(&case, "old-core-result.json").unwrap();
    assert_eq!(old["coldRestoreByteIdentical"], true);
    let expected = export["specDigest"].as_str().unwrap();
    assert_eq!(
        original_evidence(inspect(&data, expected).unwrap()),
        export["expectedOriginalInspection"]
    );
    let spec: RunSpec = load(&data, "run-spec.json").unwrap();
    let store = DurableWorkflow::open(
        &data.join("workflows").join(&spec.plan.project.id),
        spec.config(),
    )
    .unwrap();
    let attempt = store.api_attempt_path("baseline-attempt").unwrap();
    crate::api_execution::CallInventory::read(&store)
        .unwrap()
        .check(None)
        .unwrap();
    for round in 1..=2 {
        let journal = crate::api_call_journal::ApiCallJournal::read_existing(
            &attempt.join(format!("rounds/{round:03}")),
        )
        .unwrap();
        let linked = journal.gateway_evidence(&attempt).unwrap().unwrap();
        assert_eq!(linked.status, "linked");
        assert_eq!(
            linked.gateway_request_id,
            Some(format!("gateway-baseline-{round}"))
        );
        assert_eq!(linked.local_id, journal.identity().request_id);
        assert_ne!(linked.gateway_request_id, linked.body_response_id);
    }
    drop(store);
    assert_eq!(
        crate::data_format::status(&data)
            .unwrap()
            .minimum_reader_generation,
        16
    );
    crate::api_execution::publish(
        &case,
        "current-restored-check.json",
        &serde_json::json!({
            "status":"passed","lane":"api","synthetic":true,"realModelCalls":0,
            "originalInspectionEqual":true,"linkedGatewayRecords":2,"readerGeneration":16,
            "oldApiSemanticReadProven":false
        }),
    )
    .unwrap();
}
