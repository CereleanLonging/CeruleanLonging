//! Actual termination of the owned writer at production cancellation boundaries.
use super::*;
use crate::desktop_execution::{
    self as execution,
    cancellation::{self, StopObservation, StopProof, Transport},
};
use std::{
    io::Write,
    os::windows::process::CommandExt,
    process::{Child, Command as ProcessCommand, Stdio},
};

struct Owned(Child);
impl Drop for Owned {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
struct Wire;
static FORBIDDEN_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
impl Transport for Wire {
    fn prepare(&mut self, r: &execution::TaskReview) -> Result<StopObservation, String> {
        serde_json::from_value(super::cancellation_tests::running_observation(r))
            .map_err(|e| e.to_string())
    }
    fn stop(
        &mut self,
        r: &execution::TaskReview,
        o: &StopObservation,
    ) -> Result<StopProof, String> {
        let mut f = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(Path::new(&r.attempt.binding.workspace).join("stop-once"))
            .map_err(|e| e.to_string())?;
        f.write_all(b"one synthetic stop invocation")
            .and_then(|_| f.sync_all())
            .map_err(|e| e.to_string())?;
        self.read(r, o)
    }
    fn read(
        &mut self,
        r: &execution::TaskReview,
        o: &StopObservation,
    ) -> Result<StopProof, String> {
        require(
            Path::new(&r.attempt.binding.workspace)
                .join("stop-once")
                .exists(),
            "original fixture has not been stopped",
        )?;
        serde_json::from_value(super::cancellation_tests::cancelled_proof(
            &serde_json::to_value(o).unwrap(),
        ))
        .map_err(|e| e.to_string())
    }
    fn completed(&mut self, _: &execution::TaskReview) -> Result<execution::TaskProof, String> {
        Err("fixture completion unavailable".into())
    }
}
fn read_only(request: &ReadRequest) -> Result<ReadReply, String> {
    match request {
        ReadRequest::TaskStopRead {
            review,
            observation,
            ..
        } => {
            let mut wire = Wire;
            let o: StopObservation =
                serde_json::from_value(observation.clone()).map_err(|e| e.to_string())?;
            Ok(ReadReply {
                stop_proof: Some(serde_json::to_value(wire.read(review, &o)?).unwrap()),
                ..Default::default()
            })
        }
        ReadRequest::TaskRead { .. } => Err("fixture completion unavailable".into()),
        _ => {
            FORBIDDEN_CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            panic!("recovery must not prepare/send/stop: {request:?}");
        }
    }
}
fn source_tree(path: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut result = BTreeMap::new();
    for entry in fs::read_dir(path).unwrap() {
        let p = entry.unwrap().path();
        if p.is_dir() {
            result.extend(source_tree(&p));
        } else {
            result.insert(p.clone(), fs::read(p).unwrap());
        }
    }
    result
}
fn assert_preserved(before: &BTreeMap<PathBuf, Vec<u8>>) {
    for (p, b) in before {
        assert_eq!(fs::read(p).unwrap(), *b, "{}", p.display());
    }
}
fn checkpoint(
    root: &Path,
    wanted_record: &str,
    wanted_point: &str,
    record: &str,
    point: &str,
) -> Result<(), String> {
    if record == wanted_record && point == wanted_point {
        let mut marker = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join("ready.json"))
            .unwrap();
        marker.write_all(&serde_json::to_vec(&serde_json::json!({"ownerPid":std::process::id(),"record":record,"point":point})).unwrap()).unwrap();
        marker.sync_all().unwrap();
        drop(marker);
        loop {
            std::thread::park_timeout(Duration::from_secs(1));
        }
    }
    Ok(())
}
#[test]
#[ignore = "owned desktop stop publication writer; explicit fresh case and boundary required"]
fn desktop_stop_publication_owner() {
    assert_eq!(
        std::env::var("CERULEAN_NATIVE_OWNED_JOB").as_deref(),
        Ok("1")
    );
    let root = PathBuf::from(std::env::var_os("CERULEAN_DESKTOP_STOP_CASE").unwrap());
    let record = std::env::var("CERULEAN_DESKTOP_STOP_RECORD").unwrap();
    let point = std::env::var("CERULEAN_DESKTOP_STOP_POINT").unwrap();
    assert!(!root.join("data").exists());
    let (mut host, r) = super::cancellation_tests::unknown(&root);
    let data = host.root.clone();
    let permit = cancellation::request_observed(
        &data,
        &r,
        "user stop before owned writer termination",
        |r, p| checkpoint(&root, &record, &point, r, p),
    )
    .unwrap();
    cancellation::run_observed(
        &data,
        host.projects.get_mut("project").unwrap(),
        &r,
        Some(permit),
        &mut Wire,
        |r, p| checkpoint(&root, &record, &point, r, p),
    )
    .unwrap();
    panic!("requested publication point was not reached");
}
#[test]
#[ignore = "20 real owned desktop stop writer terminations; synthetic external transport, fresh reports required"]
fn twenty_desktop_stop_publication_terminations() {
    assert_eq!(
        std::env::var("CERULEAN_NATIVE_OWNED_JOB").as_deref(),
        Ok("1")
    );
    let root = fs::canonicalize(std::env::var_os("CERULEAN_DESKTOP_STOP_PUBLICATION_DIR").unwrap())
        .unwrap();
    let reports =
        fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports")).unwrap();
    assert!(root.starts_with(&reports));
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    let mut boundaries = Vec::new();
    for record in ["stop-request", "stop-intent", "stop-proof"] {
        for point in [
            "temp-created",
            "partial-written",
            "temp-synced",
            "published",
            "pending-removed",
        ] {
            boundaries.push((record, point));
        }
    }
    boundaries.extend([
        ("workflow", "cancel-recorded"),
        ("workflow", "unknown-recorded"),
        ("transport", "stop-returned"),
        ("transport", "proof-validated"),
        ("workflow", "reconciled"),
    ]);
    let mut cases = Vec::new();
    for (record, point) in boundaries {
        FORBIDDEN_CALLS.store(0, std::sync::atomic::Ordering::SeqCst);
        let case = root.join(format!("{record}-{point}"));
        fs::create_dir(&case).unwrap();
        let log = fs::File::create(case.join("owner.log")).unwrap();
        let mut owner=Owned(ProcessCommand::new(std::env::current_exe().unwrap()).args(["--exact","host::desktop_sessions::tests::stop_publication_tests::desktop_stop_publication_owner","--ignored","--nocapture"])
            .creation_flags(0x08000000).env("CERULEAN_DESKTOP_STOP_CASE",&case).env("CERULEAN_DESKTOP_STOP_RECORD",record).env("CERULEAN_DESKTOP_STOP_POINT",point)
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
                "owner exited at {record}:{point}; {}",
                fs::read_to_string(case.join("owner.log")).unwrap()
            );
            assert!(
                start.elapsed() < Duration::from_secs(30),
                "owner timeout: {record}:{point}"
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        assert_eq!(marker["ownerPid"], owner.0.id());
        assert_eq!(marker["record"], record);
        assert_eq!(marker["point"], point);
        owner.0.kill().unwrap();
        let exit = owner.0.wait().unwrap();
        assert!(!exit.success());
        let data = case.join("data");
        let original = source_tree(&data.join("desktop-task-dispatch-v1"));
        let original_events = source_tree(&data.join("workflows/project/events"));
        let stop_path = case.join("project/stop-once");
        let stop_before = stop_path.exists();
        let partial =
            record.starts_with("stop-") && ["temp-created", "partial-written"].contains(&point);
        let mut final_state = None;
        let mut final_events = 0;
        for reopen in 0..2 {
            if partial {
                assert!(
                    RuntimeHost::open(&data).is_err(),
                    "partial accepted at {record}:{point}"
                );
                let mut diagnostic = crate::diagnostic_service::ServiceHost::open(&data).unwrap();
                assert!(diagnostic.is_diagnostic());
                assert!(diagnostic
                    .handle(Request {
                        id: format!("reject-{reopen}"),
                        protocol_version: PROTOCOL_VERSION,
                        command: Command::CancelDesktopTask {
                            project_id: "project".into(),
                            attempt_id: "desktop-attempt".into(),
                            review_digest: "a".repeat(64),
                            reason: "must reject in diagnostics".into()
                        }
                    })
                    .error
                    .is_some());
                assert!(diagnostic.snapshot().core_diagnostic.is_some());
                assert_eq!(
                    source_tree(&data.join("workflows/project/events")),
                    original_events
                );
            } else {
                let mut host = RuntimeHost::open(&data).unwrap();
                host.desktop_fixture = Some(read_only);
                let (review, _) = execution::inspect(
                    &host.root,
                    &host.projects["project"],
                    "project",
                    "desktop-attempt",
                )
                .unwrap();
                assert!(host
                    .cancel_desktop_task("project", "desktop-attempt", &review.id, "never repeat")
                    .is_err());
                if host.projects["project"].state().tasks["task"].state == TaskState::Unknown {
                    host.read_desktop_task("project", "desktop-attempt")
                        .unwrap();
                    background_spin(&mut host);
                }
                let state = host.projects["project"].state().tasks["task"].state;
                assert_eq!(
                    state,
                    if stop_before {
                        TaskState::Cancelled
                    } else {
                        TaskState::Unknown
                    },
                    "{record}:{point} reopen {reopen}: {:?}",
                    host.desktop_access.error
                );
                assert!(!host.projects["project"].state().tasks["task"].verified);
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
                            WorkflowCommand::CancelIntent { .. }
                        ))
                        .count(),
                    1
                );
                assert!(!history.iter().any(|e| matches!(
                    e.body.event.command,
                    WorkflowCommand::Submit { .. }
                        | WorkflowCommand::Verify { .. }
                        | WorkflowCommand::Accept { .. }
                )));
                if reopen == 1 {
                    assert_eq!(
                        history.len(),
                        final_events,
                        "duplicate event on second reopen"
                    );
                }
                final_events = history.len();
                final_state = Some(state);
            }
            assert_preserved(&original);
            assert_eq!(
                FORBIDDEN_CALLS.load(std::sync::atomic::Ordering::SeqCst),
                0,
                "recovery tried a forbidden transport"
            );
            assert_eq!(stop_path.exists(), stop_before, "recovery invoked stop");
        }
        let item = serde_json::json!({"record":record,"point":point,"ownerPid":owner.0.id(),"actualTermination":true,"ownerExitCode":exit.code(),"diagnostic":partial,"reopens":2,"originalBytesPreserved":true,"stopInvocations":u8::from(stop_before),"recoveryStopInvocations":0,"finalState":final_state,"finalWorkflowEvents":final_events});
        fs::write(
            case.join("result.json"),
            serde_json::to_vec_pretty(&item).unwrap(),
        )
        .unwrap();
        cases.push(item);
    }
    fs::write(root.join("result.json"),serde_json::to_vec_pretty(&serde_json::json!({"status":"passed","actualTerminations":20,"scope":"actual owned RuntimeHost writer termination at 15 record publication and 5 workflow/transport boundaries; synthetic stop replies; partial records remain diagnostic, no repair implemented","realModelCalls":0,"externalMessages":0,"externalUserAppStops":0,"cases":cases})).unwrap()).unwrap();
}
