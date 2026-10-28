//! Actual process termination at production advice boundaries, not copied crash prefixes.
use super::*;
use cerulean_domain::{management::Purpose, project_setup::RunState};
use std::{
    os::windows::{
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::CommandExt,
    },
    process::{Child, Command as ProcessCommand, Stdio},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::WAIT_OBJECT_0,
    System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
};
struct Owner(Child);
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn records(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut result = BTreeMap::new();
    if dir.exists() {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            assert!(!fs::symlink_metadata(&path)
                .unwrap()
                .file_type()
                .is_symlink());
            if path.is_dir() {
                result.extend(records(&path));
            } else if path.extension().is_some_and(|e| e == "json") {
                result.insert(path.clone(), fs::read(path).unwrap());
            }
        }
    }
    result
}
fn counts(bin: &Path) -> (usize, usize, usize) {
    let events: Vec<serde_json::Value> = fs::read_to_string(bin.join("protocol.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let count = |method: &str| events.iter().filter(|v| v["method"] == method).count();
    (
        count("turn/start"),
        count("turn/steer"),
        count("thread/read"),
    )
}
fn settled(host: &RuntimeHost) -> bool {
    host.codex_active.is_empty() && host.active.is_empty() && host.management_active.is_empty()
}
fn await_reconciliation(host: &mut RuntimeHost) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !settled(host) {
        host.poll_verifications();
        assert!(
            Instant::now() < deadline,
            "read-only recovery timeout: {:?}",
            host.snapshot()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
#[test]
#[ignore = "explicit fresh reports directory and built offline peer; kills only owned test processes"]
fn five_advice_boundaries_survive_real_owner_termination_without_resend() {
    let outer = PathBuf::from(std::env::var_os("CERULEAN_ADVICE_CRASH_DIR").unwrap())
        .canonicalize()
        .unwrap();
    let reports = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../reports")
        .canonicalize()
        .unwrap();
    assert!(outer.starts_with(&reports) && outer != reports);
    let peer = PathBuf::from(std::env::var_os("CERULEAN_DAG_EXE").unwrap())
        .canonicalize()
        .unwrap();
    let mut results = vec![];
    for point in ["prepared", "advised", "intent", "received", "acknowledged"] {
        let case = outer.join(point);
        fs::create_dir(&case).unwrap();
        let data = case.join("data");
        let bin = case.join("bin");
        let mut owner = Owner(ProcessCommand::new(std::env::current_exe().unwrap())
            .args(["--ignored", "--exact", "host::project_dag_tests::two_task_production_pipeline_with_multichecks_and_restarts", "--nocapture"])
            .env("CERULEAN_DAG_DIR", &case).env("CERULEAN_DAG_MODE", "success")
            .env("CERULEAN_DAG_DELIVERY", "ACK").env("CERULEAN_DAG_OBSERVATIONS", "1")
            .env("CERULEAN_DAG_SUPERVISION", "1").env("CERULEAN_DAG_OBSERVATIONS_CLEAN", "1")
            .env("CERULEAN_DAG_EXE", &peer).env("CERULEAN_ADVICE_CRASH_POINT", point)
            .env_remove("CERULEAN_DAG_ADVICE")
            .creation_flags(0x08000000).stdin(Stdio::null())
            .stdout(fs::File::create(case.join("owner.stdout")).unwrap())
            .stderr(fs::File::create(case.join("owner.stderr")).unwrap())
            .spawn().unwrap());
        let deadline = Instant::now() + Duration::from_secs(30);
        let marker = data.join("advice-crash-marker.json");
        while !marker.exists() {
            assert!(
                owner.0.try_wait().unwrap().is_none(),
                "owner exited before {point}; inspect {}",
                case.display()
            );
            assert!(Instant::now() < deadline, "no boundary {point}");
            std::thread::sleep(Duration::from_millis(10));
        }
        let marked: serde_json::Value = serde_json::from_slice(&fs::read(marker).unwrap()).unwrap();
        assert_eq!(marked["point"], point);
        assert_eq!(marked["ownerPid"], owner.0.id());
        let peer_pid: u32 = fs::read_to_string(bin.join("app-server-pid"))
            .unwrap()
            .parse()
            .unwrap();
        let raw = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, peer_pid) };
        assert!(
            !raw.is_null(),
            "owned protocol peer is not live at boundary"
        );
        let process = unsafe { OwnedHandle::from_raw_handle(raw) };
        let mut original = records(&data.join("management-decisions"));
        original.extend(records(&data.join("workflows/project/events")));
        original.extend(records(&data.join("advice-deliveries")));
        let before_wire = counts(&bin);
        assert_eq!(
            before_wire,
            (
                1,
                usize::from(matches!(point, "received" | "acknowledged")),
                0
            )
        );
        owner.0.kill().unwrap();
        let exit = owner.0.wait().unwrap();
        assert!(!exit.success());
        assert_eq!(
            unsafe { WaitForSingleObject(process.as_raw_handle(), 5000) },
            WAIT_OBJECT_0,
            "owned peer survived EOF after host termination"
        );
        let mut host = RuntimeHost::open(&data).unwrap();
        super::project_dag_tests::enable_offline_management(&mut host);
        for (path, bytes) in &original {
            assert_eq!(
                fs::read(path).unwrap(),
                *bytes,
                "original file changed: {}",
                path.display()
            );
        }
        assert_eq!(counts(&bin), before_wire);
        let status = host
            .management
            .statuses()
            .into_iter()
            .find(|s| s.purpose == Purpose::Advise)
            .unwrap();
        let operation = status.operation_id;
        let prepared = point == "prepared";
        assert_eq!(
            host.management.application(&operation).unwrap().1,
            !prepared
        );
        assert_eq!(
            host.projects["project"]
                .history()
                .iter()
                .filter(|e| matches!(e.body.event.command, WorkflowCommand::Advise { .. }))
                .count(),
            usize::from(!prepared)
        );
        assert_eq!(host.advice_delivered(&operation), point == "acknowledged");
        assert_eq!(
            host.codex_jobs.values().next().unwrap().state,
            CodexJobState::Unknown
        );
        let recovered_sequence = host.projects["project"].history().len();
        for _ in 0..4 {
            host.poll_verifications();
        }
        assert_eq!(host.runs.all()[0].state, RunState::Attention);
        assert_eq!(host.projects["project"].history().len(), recovered_sequence);
        assert_eq!(counts(&bin), before_wire);
        let run = host.runs.all()[0].clone();
        host.pause_project_run(&run.id).unwrap();
        let attempt = host.codex_jobs.values().next().unwrap().attempt_id.clone();
        host.reconcile_codex("project", &attempt).unwrap();
        await_reconciliation(&mut host);
        assert_eq!(
            host.codex_jobs.values().next().unwrap().state,
            CodexJobState::Submitted,
            "{}",
            serde_json::to_string(&host.snapshot()).unwrap()
        );
        assert_eq!(host.runs.all()[0].state, RunState::Paused);
        let read_wire = counts(&bin);
        assert_eq!((read_wire.0, read_wire.1), (before_wire.0, before_wire.1));
        assert!(read_wire.2 > before_wire.2);
        if prepared {
            let review = host.application_recovery_review(&operation).unwrap();
            host.abandon_management_application(
                &operation,
                &review.evidence_digest,
                "Owner terminated before applying advice",
            )
            .unwrap();
        } else if point != "acknowledged" {
            let review = host.advice_tracking_review(&operation).unwrap();
            host.close_advice_tracking(
                &operation,
                &review.evidence_digest,
                "Owner terminated; retain unknown advice delivery",
            )
            .unwrap();
        } else {
            assert!(host.advice_tracking_review(&operation).is_err());
        }
        let held = host.projects["project"].history().len();
        drop(host);
        host = RuntimeHost::open(&data).unwrap();
        super::project_dag_tests::enable_offline_management(&mut host);
        for _ in 0..4 {
            host.poll_verifications();
        }
        assert_eq!(host.projects["project"].history().len(), held);
        assert_eq!(counts(&bin), read_wire);
        host.start_project_run("project", &run.revision_digest, &run.setup_digest)
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            host.poll_verifications();
            let state = host.runs.all()[0].state.clone();
            if state == RunState::Completed || state == RunState::Attention {
                break;
            }
            assert!(Instant::now() < deadline, "resume timeout {point}");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            host.runs.all()[0].state,
            RunState::Completed,
            "{:?}",
            host.snapshot()
        );
        assert!(settled(&host));
        assert_eq!(host.jobs.len(), 2);
        assert!(host.projects["project"]
            .state()
            .tasks
            .values()
            .all(|t| t.state == TaskState::Accepted));
        let wire = counts(&bin);
        assert_eq!(wire.0, 2);
        assert_eq!(
            wire.1,
            if matches!(point, "received" | "acknowledged") {
                2
            } else {
                1
            }
        );
        assert_eq!(host.advice_delivered(&operation), point == "acknowledged");
        let snapshot = host.snapshot();
        fs::write(
            case.join("snapshot.json"),
            serde_json::to_vec_pretty(&snapshot).unwrap(),
        )
        .unwrap();
        drop(host);
        host = RuntimeHost::open(&data).unwrap();
        for _ in 0..4 {
            host.poll_verifications();
        }
        assert_eq!(
            digest(&host.snapshot().workflows).unwrap(),
            digest(&snapshot.workflows).unwrap()
        );
        assert_eq!(counts(&bin), wire);
        results.push(serde_json::json!({"point":point,"ownerPid":marked["ownerPid"],"peerPid":peer_pid,"realTermination":true,"peerExited":true,"originalFilesUnchanged":original.len(),"beforeWire":before_wire,"finalWire":wire,"afterExplicitResume":"completed","realModelCalls":0}));
    }
    fs::write(
        outer.join("crash-result.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"status":"passed","cases":results,"realModelCalls":0}),
        )
        .unwrap(),
    )
    .unwrap();
}
