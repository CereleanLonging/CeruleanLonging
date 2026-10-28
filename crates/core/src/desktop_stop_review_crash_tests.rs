//! Kill the owned reviewer at each production receipt publication boundary.
use super::*;
use std::{
    io::Write,
    os::windows::process::CommandExt,
    process::{Child, Command as ProcessCommand, Stdio},
    sync::atomic::{AtomicUsize, Ordering},
};
static CALLS: AtomicUsize = AtomicUsize::new(0);
struct Owned(Child);
impl Drop for Owned {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
fn forbidden(_: &ReadRequest) -> Result<ReadReply, String> {
    CALLS.fetch_add(1, Ordering::SeqCst);
    Err("review cannot invoke desktop transport".into())
}
#[test]
#[ignore = "owned review receipt writer, requires explicit isolated case and boundary"]
fn desktop_stop_review_receipt_owner() {
    assert_eq!(
        std::env::var("CERULEAN_NATIVE_OWNED_JOB").as_deref(),
        Ok("1")
    );
    let root = fs::canonicalize(std::env::var_os("CERULEAN_STOP_REVIEW_CASE").unwrap()).unwrap();
    let reports =
        fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports")).unwrap();
    assert!(root.starts_with(reports));
    assert!(!root.join("data").exists());
    let stage = std::env::var("CERULEAN_STOP_REVIEW_STAGE").unwrap();
    let point = std::env::var("CERULEAN_STOP_REVIEW_POINT").unwrap();
    assert!(["stop-request", "stop-intent", "stop-proof"].contains(&stage.as_str()));
    assert!(["created", "partial", "synced", "published", "finished"].contains(&point.as_str()));
    let (data, r, rel) = fixture(&root, &stage, "partial-written");
    let (evidence, _) = review::inspect(&data, &rel).unwrap();
    // Match the service ownership used by the real review command.
    let service = crate::diagnostic_service::ServiceHost::open(&data).unwrap();
    assert!(service.is_diagnostic());
    review::acknowledge(&data, &rel, &evidence, "review before owned writer termination", |at| {
        if at == point {
            let bytes = serde_json::to_vec(&serde_json::json!({"ownerPid":std::process::id(), "stage":stage,"point":point,"relativePath":rel,"evidenceDigest":evidence,"reviewDigest":r.id,"attemptId":r.attempt.id})).unwrap();
            let mut marker = fs::OpenOptions::new().create_new(true).write(true).open(root.join("ready.json")).unwrap();
            marker.write_all(&bytes).unwrap(); marker.sync_all().unwrap(); drop(marker);
            loop { std::thread::park_timeout(Duration::from_secs(1)); }
        }
        Ok(())
    }).unwrap();
    panic!("requested boundary not reached");
}
#[test]
#[ignore = "15 actual owned reviewer terminations; explicit fresh report directory required"]
fn fifteen_desktop_stop_review_receipt_terminations() {
    assert_eq!(
        std::env::var("CERULEAN_NATIVE_OWNED_JOB").as_deref(),
        Ok("1")
    );
    let root =
        fs::canonicalize(std::env::var_os("CERULEAN_STOP_REVIEW_CRASH_DIR").unwrap()).unwrap();
    let reports =
        fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports")).unwrap();
    assert!(root.starts_with(reports));
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    let mut cases = vec![];
    for stage in ["stop-request", "stop-intent", "stop-proof"] {
        for point in ["created", "partial", "synced", "published", "finished"] {
            CALLS.store(0, Ordering::SeqCst);
            let case = root.join(format!("{stage}-{point}"));
            fs::create_dir(&case).unwrap();
            let log = fs::File::create(case.join("owner.log")).unwrap();
            let mut owner = Owned(ProcessCommand::new(std::env::current_exe().unwrap()).args(["--exact", "host::desktop_sessions::tests::stop_review_tests::crash::desktop_stop_review_receipt_owner", "--ignored", "--nocapture"])
                .creation_flags(0x08000000).env("CERULEAN_STOP_REVIEW_CASE", &case).env("CERULEAN_STOP_REVIEW_STAGE", stage).env("CERULEAN_STOP_REVIEW_POINT", point)
                .stdin(Stdio::null()).stdout(Stdio::from(log.try_clone().unwrap())).stderr(Stdio::from(log)).spawn().unwrap());
            let start = Instant::now();
            let marker: serde_json::Value = loop {
                if let Ok(bytes) = fs::read(case.join("ready.json")) {
                    if let Ok(value) = serde_json::from_slice(&bytes) {
                        break value;
                    }
                }
                assert!(
                    owner.0.try_wait().unwrap().is_none(),
                    "owner exited: {}",
                    fs::read_to_string(case.join("owner.log")).unwrap()
                );
                assert!(
                    start.elapsed() < Duration::from_secs(30),
                    "owner boundary timeout"
                );
                std::thread::sleep(Duration::from_millis(20));
            };
            assert_eq!(marker["ownerPid"], owner.0.id());
            assert_eq!(marker["stage"], stage);
            assert_eq!(marker["point"], point);
            owner.0.kill().unwrap();
            let exit = owner.0.wait().unwrap();
            assert!(!exit.success());
            let data = case.join("data");
            let originals = tree(&data.join("desktop-task-dispatch-v1"));
            let receipts = tree(&data.join("desktop-stop-reviews-v1"));
            let events = tree(&data.join("workflows/project/events"));
            let rel = marker["relativePath"].as_str().unwrap();
            let evidence = marker["evidenceDigest"].as_str().unwrap();
            let attempt = marker["attemptId"].as_str().unwrap();
            let original_digest = marker["reviewDigest"].as_str().unwrap();
            let published = matches!(point, "published" | "finished");
            for _ in 0..2 {
                if published {
                    let mut host = RuntimeHost::open(&data).unwrap();
                    host.desktop_fixture = Some(forbidden);
                    assert_eq!(
                        host.projects["project"].state().tasks["task"].state,
                        TaskState::Unknown
                    );
                    assert_eq!(
                        host.desktop_access.tasks[0].reviewed_stop_record.as_deref(),
                        Some(stage)
                    );
                    assert!(host
                        .cancel_desktop_task("project", attempt, original_digest, "must reject")
                        .is_err());
                    assert!(host.read_desktop_task("project", attempt).is_err());
                } else {
                    let mut service = crate::diagnostic_service::ServiceHost::open(&data).unwrap();
                    assert!(service.is_diagnostic());
                    let snap = service.snapshot();
                    let file = snap
                        .core_diagnostic
                        .unwrap()
                        .desktop_files
                        .into_iter()
                        .find(|f| f.relative_path == rel)
                        .unwrap();
                    assert!(!file.reviewed);
                    assert_eq!(file.review_digest.as_deref(), Some(evidence));
                    assert!(service
                        .handle(Request {
                            id: "reject".into(),
                            protocol_version: PROTOCOL_VERSION,
                            command: Command::ReadDesktopTask {
                                reader_sha256: None,
                                project_id: "project".into(),
                                attempt_id: attempt.into()
                            }
                        })
                        .error
                        .is_some());
                }
                assert_eq!(tree(&data.join("workflows/project/events")), events);
                check_tree(&originals);
                check_tree(&receipts);
            }
            {
                // Explicit user retry completes only publication; no workflow event.
                let _base = crate::Store::open(&data).unwrap();
                review::acknowledge(
                    &data,
                    rel,
                    evidence,
                    "explicit review after interrupted publication",
                    |_| Ok(()),
                )
                .unwrap();
            }
            let after_retry = tree(&data.join("desktop-stop-reviews-v1"));
            assert_eq!(
                after_retry
                    .keys()
                    .filter(|p| p.extension().is_some_and(|e| e == "json"))
                    .count(),
                1
            );
            for _ in 0..2 {
                let mut host = RuntimeHost::open(&data).unwrap();
                host.desktop_fixture = Some(forbidden);
                assert_eq!(
                    host.projects["project"].state().tasks["task"].state,
                    TaskState::Unknown
                );
                assert!(!host.projects["project"].state().tasks["task"].verified);
                assert!(host
                    .cancel_desktop_task("project", attempt, original_digest, "must reject")
                    .is_err());
                assert!(host.read_desktop_task("project", attempt).is_err());
                assert!(host.require_desktop_tasks_resolved().is_err());
                assert_eq!(tree(&data.join("workflows/project/events")), events);
            }
            {
                let mut host = RuntimeHost::open(&data).unwrap();
                host.desktop_fixture = Some(forbidden);
                let proof = host.desktop_retirement_digest("project", attempt).unwrap();
                host.retire_unknown_desktop(
                    "project",
                    attempt,
                    &proof,
                    "user handled external application; retire local tracking only",
                )
                .unwrap();
            }
            {
                let host = RuntimeHost::open(&data).unwrap();
                assert_eq!(
                    host.projects["project"].state().tasks["task"].state,
                    TaskState::Retired
                );
                let history = host.projects["project"].history();
                assert_eq!(
                    history
                        .iter()
                        .filter(|e| matches!(
                            e.body.event.command,
                            WorkflowCommand::DispatchIntent { .. }
                        ))
                        .count(),
                    1
                );
                assert_eq!(
                    history
                        .iter()
                        .filter(|e| matches!(
                            e.body.event.command,
                            WorkflowCommand::RetireUnknownDesktop { .. }
                        ))
                        .count(),
                    1
                );
                assert!(!history.iter().any(|e| matches!(
                    e.body.event.command,
                    WorkflowCommand::Submit { .. }
                        | WorkflowCommand::Verify { .. }
                        | WorkflowCommand::Accept { .. }
                        | WorkflowCommand::Reconcile {
                            outcome: ReconcileOutcome::Cancelled,
                            ..
                        }
                )));
            }
            check_tree(&originals);
            check_tree(&receipts);
            assert_eq!(tree(&data.join("desktop-stop-reviews-v1")), after_retry);
            assert_eq!(CALLS.load(Ordering::SeqCst), 0);
            let result = serde_json::json!({"stage":stage,"point":point,"ownerPid":owner.0.id(),"ownerExitCode":exit.code(),"actualTermination":true,"initialDiagnostic":!published,"preReviewReopens":2,"postReviewReopens":2,"retirementReopens":1,"originalBytesPreserved":true,"interruptedReceiptBytesPreserved":true,"reviewDoesNotChangeWorkflow":true,"transportCalls":0,"finalState":"retired","verified":false});
            fs::write(
                case.join("result.json"),
                serde_json::to_vec_pretty(&result).unwrap(),
            )
            .unwrap();
            cases.push(result);
        }
    }
    fs::write(root.join("result.json"),serde_json::to_vec_pretty(&serde_json::json!({"status":"passed","actualTerminations":15,"scope":"actual owned receipt reviewer termination; original stop transport synthetic and original stop fragment injected","cases":cases,"realModelCalls":0,"externalMessages":0,"externalAppStops":0})).unwrap()).unwrap();
}
