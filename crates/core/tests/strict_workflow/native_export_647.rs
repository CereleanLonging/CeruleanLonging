//! Explicit native fixture exporter. Synthetic worker receipts, one real local
//! failed checker, no provider, no copied or rewritten protection records.
use super::*;

#[test]
#[ignore = "explicit owned native catalog fixture, must run as ordinary test account"]
fn export_final_catalog() {
    let catalog = std::path::PathBuf::from(std::env::var_os("CERULEAN_NATIVE647_CATALOG").unwrap());
    assert_eq!(
        catalog,
        Path::new(
            r"C:\Users\clacceptHost452\AppData\Roaming\local.ceruleanlonging.workbench\experiment-workbenches-v1\exp-recheck-647"
        )
    );
    assert_eq!(
        std::env::var("USERNAME").unwrap().to_lowercase(),
        "claccepthost452"
    );
    let ready = fs::read(catalog.join("ready.json")).unwrap();
    let intent = fs::read(catalog.join("intent.json")).unwrap();
    let marker = fs::read(catalog.join("data/events/00000000.reader18.json")).unwrap();
    let mut claim = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(catalog.join("synthetic647-intent.json"))
        .unwrap();
    use std::io::Write;
    claim.write_all(br#"{"syntheticWorker":true,"providerRequests":0,"localOriginalChecks":1,"recheckExecutions":0}"#).unwrap();
    claim.sync_all().unwrap();
    let mode = "detail-first-fail";
    let project = catalog.join("synthetic-project");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("result.txt"), "abc").unwrap();
    fs::write(project.join("validator-mode"), mode).unwrap();
    let data = catalog.join("data");
    let mut host = RuntimeHost::open(&data).unwrap();
    let mut p = plan(&project);
    let snap = host
        .handle(Request {
            id: "register".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::RegisterProject {
                project: p.project.clone(),
            },
        })
        .result
        .unwrap();
    p.project = snap.workflows[0].project.clone();
    p.snapshot_policy = Some(SnapshotPolicy {
        exclude: vec![],
        max_files: 100,
        max_bytes: 1024 * 1024,
    });
    p.execution[0].checks[0].args = vec![
        "--ignored".into(),
        "--exact".into(),
        "real_validator_fixture".into(),
        "--nocapture".into(),
    ];
    p.execution[0].checks[0].timeout_seconds = 30;
    if mode.starts_with("detail-") {
        p.execution[0].checks[0].name = Some("语法检查".into());
        p.execution[0].checks[0].timeout_seconds = if mode == "detail-timeout" { 1 } else { 5 };
        let mut second = p.execution[0].checks[0].clone();
        second.id = "second".into();
        second.name = Some("单元测试".into());
        second.timeout_seconds = 30;
        p.execution[0].checks.push(second);
    }
    host.propose("project", "chief", p.clone()).unwrap();
    host.handle(Request {
        id: "approve".into(),
        protocol_version: PROTOCOL_VERSION,
        command: Command::ApproveRevision {
            project_id: "project".into(),
            digest: p.approval_digest().unwrap(),
        },
    })
    .result
    .unwrap();
    drop(host);
    let ledger = data.join("workflows/project");
    let cfg = serde_json::from_slice(&fs::read(ledger.join("bindings.json")).unwrap()).unwrap();
    let mut store = DurableWorkflow::open(&ledger, cfg).unwrap();
    let attempt = assign(&mut store, &p);
    dispatch(&mut store, &p);
    apply(
        &mut store,
        "core",
        WorkflowCommand::Started {
            task: "task".into(),
            attempt: attempt.id.clone(),
        },
        ctx(receipt(&attempt, ReceiptStatus::Running), None),
    )
    .unwrap();
    submit(&mut store, &p, &attempt);
    drop(store);
    let pins: Vec<_> = p.execution[0]
        .checks
        .iter()
        .map(|check| {
            cerulean_core::verifier::PinnedCheck::new(check, &std::env::current_exe().unwrap(), &[])
                .unwrap()
        })
        .collect();

    let mut host = RuntimeHost::open(&data).unwrap();
    host.start_verification("project", "stage-check", "task", "attempt", pins)
        .unwrap();
    complete_detail_verification(&mut host);
    let snapshot = host.snapshot();
    assert_eq!(snapshot.verification_jobs.len(), 1);
    assert_eq!(
        snapshot.verification_jobs[0].state,
        VerificationState::Failed
    );
    let mut checks = p.execution[0].checks.clone();
    checks[0].args.push("--test-threads=1".into());
    let request = cerulean_domain::verification_recheck::RecheckRequest {
        project_id: "project".into(),
        original_operation_id: "stage-check".into(),
        replacement_checks: checks,
        reason: "Synthetic native647: retire an approved but unstarted recheck; no provider".into(),
    };
    drop(host);
    assert_eq!(fs::read(catalog.join("ready.json")).unwrap(), ready);
    assert_eq!(fs::read(catalog.join("intent.json")).unwrap(), intent);
    assert_eq!(
        fs::read(catalog.join("data/events/00000000.reader18.json")).unwrap(),
        marker
    );
    let value = serde_json::json!({"passed":true,"syntheticWorker":true,"modelCalls":0,"originalCheckerJobs":1,"originalFailed":true,"recheckExecutions":0,"dataDirectory":data,"request":request});
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(catalog.join("synthetic647-result.json"))
        .unwrap();
    f.write_all(&serde_json::to_vec_pretty(&value).unwrap())
        .unwrap();
    f.sync_all().unwrap();
}
