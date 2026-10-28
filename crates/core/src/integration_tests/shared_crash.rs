//! Real owner termination while independent native/API attempts share one ledger.
//! Only provider replies are controlled; original processes, files and recovery run.
use super::*;
use std::{
    io::Write,
    os::windows::{
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::CommandExt,
    },
    process::{Child, Command as ProcessCommand, Stdio},
};
use windows_sys::Win32::{
    Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
};

const CASES: [&str; 8] = [
    "codex-api-running",
    "codex-omp-running",
    "two-codex-running",
    "accepted-api-codex",
    "before-prepare",
    "prepared",
    "assigned",
    "applied",
];

struct Owner(Child);
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn json_file(path: &Path, value: &impl serde::Serialize) {
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    f.write_all(&serde_json::to_vec_pretty(value).unwrap())
        .unwrap();
    f.sync_all().unwrap();
    // This is a fixture notification, never a product journal. The parent uses
    // the established assignment-crash protocol: parse the complete JSON before
    // termination, not mere existence. Original journals were synced earlier.
}
fn records(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut r = BTreeMap::new();
    if dir.exists() {
        for item in fs::read_dir(dir).unwrap() {
            let p = item.unwrap().path();
            assert!(!fs::symlink_metadata(&p).unwrap().file_type().is_symlink());
            if p.is_dir() {
                r.extend(records(&p));
            } else if p.extension().is_some_and(|x| x == "json") {
                r.insert(p.clone(), fs::read(p).unwrap());
            }
        }
    }
    r
}
fn wire(root: &Path) -> Vec<serde_json::Value> {
    fs::read_to_string(root.join("bin/protocol.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}
fn wire_count(root: &Path, method: &str) -> usize {
    wire(root).iter().filter(|v| v["method"] == method).count()
}

fn explicit_recovery(host: &mut RuntimeHost, root: &Path, stage: &str) -> serde_json::Value {
    let before = host.projects["project"].history().len();
    let original_wire = wire(root);
    let run = host.runs.all()[0].clone();
    host.pause_project_run(&run.id).unwrap();
    if stage == "two-codex-running" {
        // Only read the two original sessions. Every model/manager callback in
        // the subsequently approved continuation remains an explicit fixture.
        host.management_fixture = Some(mixed::mixed_manager);
        host.api_fixture = Some(worker);
        let jobs: Vec<_> = host.codex_jobs.values().cloned().collect();
        assert_eq!(jobs.len(), 2);
        for (n, job) in jobs.iter().enumerate() {
            assert_eq!(job.state, CodexJobState::Unknown);
            host.reconcile_codex("project", &job.attempt_id).unwrap();
            spin(host, |h| h.codex_active.is_empty());
            assert_eq!(
                host.codex_jobs[&("project".into(), job.attempt_id.clone())].state,
                CodexJobState::Submitted
            );
            if n == 0 {
                let other = &jobs[1];
                assert_eq!(
                    host.projects["project"].state().tasks[&other.task_id].state,
                    TaskState::Unknown
                );
                assert!(host
                    .task_resource_conflict("project", &other.task_id, "replacement")
                    .unwrap()
                    .is_some());
            }
        }
        assert!(host.jobs.is_empty());
        assert_eq!(host.runs.all()[0].state, RunState::Paused);
        assert_eq!(wire_count(root, "turn/start"), 2);
        assert_eq!(wire_count(root, "thread/start"), 2);
        assert_eq!(wire_count(root, "turn/steer"), 0);
        assert!(wire_count(root, "thread/read") >= 2);
        host.start_project_run("project", &run.revision_digest, &run.setup_digest)
            .unwrap();
        spin(host, |h| {
            assert!(
                matches!(
                    h.runs.all()[0].state,
                    RunState::Active | RunState::Completed
                ),
                "{:?}",
                h.runs.all()
            );
            h.runs.all()[0].state == RunState::Completed
        });
        assert!(host.projects["project"]
            .state()
            .tasks
            .values()
            .all(|t| t.state == TaskState::Accepted && t.verified));
        assert_eq!(host.jobs.len(), 3);
        assert_eq!(host.management.statuses().len(), 7);
        assert_eq!(
            fs::read_to_string(
                dependency_host_fixture::task_path(host, "integration").join("bundle.txt")
            )
            .unwrap(),
            "AB"
        );
        for task in ["a", "b", "integration"] {
            assert_eq!(host.projects["project"].history().iter().filter(|e|matches!(&e.body.event.command,WorkflowCommand::DispatchIntent{task:t,..}if t==task)).count(),1);
        }
        assert_eq!(wire_count(root, "turn/start"), 2);
    } else {
        match stage {
            "before-prepare" => {
                let review = host.startup_detail("project", "codex-attempt").unwrap();
                host.abandon_codex_startup(
                    "project",
                    "codex-attempt",
                    review.evidence_digest.as_deref().unwrap(),
                    "owned test user closes an unsent startup after confirmed process exit",
                )
                .unwrap();
            }
            "prepared" => {
                let review = host.application_recovery_review("shared-codex").unwrap();
                host.abandon_management_application(
                    "shared-codex",
                    &review.evidence_digest,
                    "owned test user closes a prepared application without an Assign event",
                )
                .unwrap();
            }
            "assigned" | "applied" => {
                let review = host
                    .unsent_assignments()
                    .into_iter()
                    .find(|r| r.attempt_id == "codex-attempt")
                    .unwrap();
                host.abandon_unsent_attempt(
                    "project",
                    "b",
                    "codex-attempt",
                    &review.evidence_digest,
                    "owned test user abandons the original Assigned but never dispatched attempt",
                )
                .unwrap();
            }
            _ => panic!("unexpected explicit recovery case"),
        }
        assert_eq!(
            host.projects["project"].state().tasks["b"].state,
            TaskState::Pending
        );
        assert_eq!(
            host.projects["project"].state().tasks["a"].state,
            TaskState::Submitted
        );
        assert!(host
            .task_resource_conflict("project", "b", "replacement")
            .unwrap()
            .is_none());
        assert!(host
            .start_managed_codex("shared-codex", "replacement")
            .is_err());
        assert_eq!(wire(root), original_wire);
        assert_eq!(host.runs.all()[0].state, RunState::Paused);
    }
    let after = host.projects["project"].history().len();
    json!({"stage":stage,"explicitUserAction":true,"newEvents":after-before,"originalPromptResends":0,
        "result":if stage=="two-codex-running"{"completed-with-original-attempts"}else{"closed-unsent-no-dispatch"},
        "readOnlyNativeRecoveries":if stage=="two-codex-running"{2}else{0},
        "newIntegrationTasks":usize::from(stage=="two-codex-running"),"realModelCalls":0})
}
fn checkpoint(host: &RuntimeHost, root: &Path, stage: &str) -> ! {
    let mut peers = Vec::new();
    if stage.contains("running") || stage == "accepted-api-codex" {
        for a in host.codex_active.values() {
            let t = fs::read_to_string(root.join("bin").join(format!("gated-{}", a.job.task_id)))
                .unwrap();
            let pid = t
                .strip_prefix("fixture-thread-")
                .unwrap()
                .parse::<u32>()
                .unwrap();
            peers.push(json!({"adapter":"codex","task":a.job.task_id,"attempt":a.job.attempt_id,"pid":pid}));
        }
    } else {
        let pid = fs::read_to_string(root.join("bin/app-server-pid"))
            .unwrap()
            .parse::<u32>()
            .unwrap();
        peers.push(json!({"adapter":"codex","task":"b","attempt":"codex-attempt","pid":pid}));
    }
    if let Some(profile) = omp_parallel::profile(host) {
        let ready: serde_json::Value =
            serde_json::from_slice(&fs::read(profile.join("fixture-ready.json")).unwrap()).unwrap();
        let a = host.omp_active.values().next().unwrap();
        peers.push(json!({"adapter":"omp","task":a.job.task_id,"attempt":a.job.attempt_id,"pid":ready["pid"]}));
    }
    assert_eq!(host.api_writers.len(), 1);
    assert!(!host.projects.contains_key("project"));
    let snapshot = host.api_writers["project"].owner.snapshot().unwrap();
    json_file(
        &root.join("ready.json"),
        &json!({"stage":stage,"ownerPid":std::process::id(),"peers":peers,
        "sharedWriters":host.api_writers.len(),"nativeActive":host.codex_active.len()+host.omp_active.len(),
        "apiActive":host.api_active.len(),"snapshot":snapshot,"run":host.runs.all()[0],
        "codexJobs":host.snapshot().codex_jobs,"ompJobs":host.snapshot().omp_jobs}),
    );
    loop {
        std::thread::park_timeout(Duration::from_secs(1));
    }
}

#[test]
#[ignore = "owned shared-ledger helper; only launched by the explicit isolated parent test"]
fn shared_owner() {
    use ProjectWorkerAdapter::{Codex, Deepseek, Omp};
    let root = PathBuf::from(std::env::var_os("CERULEAN_SHARED_CRASH_CASE").unwrap())
        .canonicalize()
        .unwrap();
    let reports = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../reports")
        .canonicalize()
        .unwrap();
    assert!(root.starts_with(&reports) && root != reports);
    let stage = std::env::var("CERULEAN_SHARED_CRASH_STAGE").unwrap();
    assert!(CASES.contains(&stage.as_str()));
    let order = match stage.as_str() {
        "codex-api-running" => [Codex, Deepseek, Deepseek],
        "codex-omp-running" => [Codex, Omp, Deepseek],
        "two-codex-running" => [Codex, Codex, Deepseek],
        _ => [Deepseek, Codex, Deepseek],
    };
    let runtime = omp_parallel::runtime();
    let mut host = mixed::mixed_host_mode(&root, runtime.path(), order, true);
    fs::write(root.join("bin/gate-codex"), "controlled peer only").unwrap();
    host.api_fixture = Some(parallel::gated);
    let plan = planned(&mut host);
    let revision = plan.approval_digest().unwrap();
    host.approve("project", &revision).unwrap();
    let setup = host.project_setups["project"].digest.clone();
    if stage.contains("running") || stage == "accepted-api-codex" {
        let gate = order[..2]
            .iter()
            .position(|a| a.is_api())
            .map(|n| parallel::gate(if n == 0 { "a" } else { "b" }));
        host.start_project_run("project", &revision, &setup)
            .unwrap();
        if let Some((wait, _)) = &gate {
            spin(&mut host, |_| wait.try_recv().is_ok());
        }
        spin(&mut host, |h| codex_parallel::all_running(h, &root, &order));
        assert_eq!(
            host.codex_active.len() + host.omp_active.len() + host.api_active.len(),
            2
        );
        if stage == "accepted-api-codex" {
            gate.as_ref().unwrap().1.send(()).unwrap();
            spin(&mut host, |h| {
                h.snapshot().workflows[0].state.tasks["a"].state == TaskState::Accepted
            });
            assert_eq!(host.codex_active.len(), 1);
            assert!(host.api_active.is_empty());
        }
        checkpoint(&host, &root, &stage);
    }
    // The native worker waits for original Prepare/Commit ACKs while API A
    // completes. The parent kills this owner before the withheld ACK can return.
    host.orchestrating = true;
    host.start_project_run("project", &revision, &setup)
        .unwrap();
    let (wait, release) = parallel::gate("a");
    omp_parallel::decision(&mut host, "shared-api", "a", "worker-deepseek");
    host.start_managed_api("shared-api", "api-attempt").unwrap();
    spin(&mut host, |_| wait.try_recv().is_ok());
    omp_parallel::decision(&mut host, "shared-codex", "b", "worker-codex");
    host.start_managed_codex("shared-codex", "codex-attempt")
        .unwrap();
    let key = ("project".into(), "codex-attempt".into());
    use crate::async_codex::Update;
    let Update::PrepareAssignment {
        operation,
        attempt,
        snapshot,
        reply,
    } = host.codex_active[&key]
        .receiver
        .recv_timeout(Duration::from_secs(30))
        .unwrap()
    else {
        panic!("prepare required")
    };
    omp_parallel::finish_api_only(&mut host, &release);
    if stage == "before-prepare" {
        checkpoint(&host, &root, &stage);
    }
    host.prepare_managed_codex_assignment("project", &operation, *attempt, *snapshot)
        .unwrap();
    if stage == "prepared" {
        checkpoint(&host, &root, &stage);
    }
    reply.send(Ok(())).unwrap();
    let Update::CommitAssignment {
        operation,
        entry,
        snapshot,
        reply: _withheld,
    } = host.codex_active[&key]
        .receiver
        .recv_timeout(Duration::from_secs(15))
        .unwrap()
    else {
        panic!("commit required")
    };
    if stage == "assigned" {
        checkpoint(&host, &root, &stage);
    }
    host.commit_managed_codex_assignment("project", &operation, *entry, *snapshot)
        .unwrap();
    checkpoint(&host, &root, &stage);
}

#[test]
#[ignore = "real owner kills with Codex/API, Codex/OMP, two Codex and scoped assignment windows; explicit new evidence directory"]
fn shared_owner_termination_preserves_original_attempts_resources_and_no_resend() {
    let outer = PathBuf::from(
        std::env::var_os("CERULEAN_SHARED_CRASH_DIR").expect("fresh evidence directory"),
    )
    .canonicalize()
    .unwrap();
    let reports = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../reports")
        .canonicalize()
        .unwrap();
    assert!(outer.starts_with(&reports) && outer != reports);
    let explicit = std::env::var("CERULEAN_SHARED_CRASH_RECOVER").as_deref() == Ok("1");
    let mut cases = Vec::new();
    for stage in CASES.into_iter().filter(|s| {
        !explicit
            || matches!(
                *s,
                "two-codex-running" | "before-prepare" | "prepared" | "assigned" | "applied"
            )
    }) {
        let root = outer.join(stage);
        fs::create_dir(&root).unwrap();
        let mut owner = Owner(
            ProcessCommand::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "host::integration_tests::shared_crash::shared_owner",
                    "--nocapture",
                ])
                .env("CERULEAN_SHARED_CRASH_CASE", &root)
                .env("CERULEAN_SHARED_CRASH_STAGE", stage)
                .creation_flags(0x08000000)
                .stdin(Stdio::null())
                .stdout(fs::File::create(root.join("owner.stdout")).unwrap())
                .stderr(fs::File::create(root.join("owner.stderr")).unwrap())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(60);
        let marked: serde_json::Value = loop {
            if let Ok(raw) = fs::read(root.join("ready.json")) {
                if let Ok(value) = serde_json::from_slice(&raw) {
                    break value;
                }
            }
            assert!(
                owner.0.try_wait().unwrap().is_none(),
                "owner exited before {stage}; {}",
                root.display()
            );
            assert!(
                Instant::now() < deadline,
                "no shared checkpoint {stage}; {}",
                root.display()
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(marked["stage"], stage);
        assert_eq!(marked["ownerPid"], owner.0.id());
        assert_eq!(marked["sharedWriters"], 1);
        let before: ExecutionSnapshot = serde_json::from_value(marked["snapshot"].clone()).unwrap();
        let data = root.join("data");
        let mut original = BTreeMap::new();
        for folder in [
            "management-decisions",
            "workflows/project/events",
            "codex-startups",
            "omp-startups",
        ] {
            original.extend(records(&data.join(folder)));
        }
        let peers: Vec<OwnedHandle> = marked["peers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| {
                let raw = unsafe {
                    OpenProcess(PROCESS_SYNCHRONIZE, 0, p["pid"].as_u64().unwrap() as u32)
                };
                assert!(!raw.is_null(), "original peer is not live at {stage}");
                let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
                assert_eq!(
                    unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) },
                    WAIT_TIMEOUT
                );
                handle
            })
            .collect();
        let before_wire = wire(&root);
        let manifests:Vec<_>=original.iter().map(|(p,b)|json!({"path":p.strip_prefix(&root).unwrap(),"sha256":format!("{:x}",<sha2::Sha256 as sha2::Digest>::digest(b))})).collect();
        json_file(&root.join("original-records.json"), &manifests);
        owner.0.kill().unwrap();
        assert!(!owner.0.wait().unwrap().success());
        for peer in &peers {
            assert_eq!(
                unsafe { WaitForSingleObject(peer.as_raw_handle(), 5000) },
                WAIT_OBJECT_0,
                "original child survived {stage}"
            );
        }
        let mut host = RuntimeHost::open(&data).unwrap_or_else(|e| panic!("{stage}: {e}"));
        let recovered = host.projects["project"].history().to_vec();
        for (path, bytes) in &original {
            assert_eq!(&fs::read(path).unwrap(), bytes, "{}", path.display());
        }
        assert_eq!(wire(&root), before_wire);
        assert_eq!(host.snapshot().workflows.len(), 1);
        let assigned = matches!(stage, "assigned" | "applied");
        let startup_window = matches!(
            stage,
            "before-prepare" | "prepared" | "assigned" | "applied"
        );
        for task in ["a", "b"] {
            let old = &before.state.tasks[task];
            let expected = if matches!(old.state, TaskState::Running | TaskState::Dispatching) {
                TaskState::Unknown
            } else {
                old.state
            };
            assert_eq!(
                host.projects["project"].state().tasks[task].state,
                expected,
                "{stage}/{task}"
            );
            assert_eq!(
                host.projects["project"].state().tasks[task].attempt,
                old.attempt
            );
            if matches!(expected, TaskState::Unknown | TaskState::Assigned)
                || (startup_window && task == "b")
            {
                assert!(
                    host.task_resource_conflict("project", task, "different-attempt")
                        .unwrap()
                        .is_some(),
                    "lost unknown/startup lease {stage}/{task}"
                );
            }
        }
        assert_eq!(
            host.projects["project"].state().tasks["integration"].state,
            TaskState::Pending
        );
        if startup_window {
            assert_eq!(
                host.startups
                    .get("project", "codex-attempt")
                    .unwrap()
                    .intent
                    .schema_version,
                2
            );
            assert_eq!(
                host.projects["project"].state().tasks["b"].state,
                if assigned {
                    TaskState::Assigned
                } else {
                    TaskState::Pending
                }
            );
            if stage == "before-prepare" {
                assert!(host.management.application("shared-codex").is_none());
            } else {
                assert_eq!(
                    host.management.application("shared-codex").unwrap().1,
                    assigned
                );
            }
            assert_eq!(wire_count(&root, "turn/start"), 0);
            assert!(host
                .start_managed_codex("shared-codex", "replacement")
                .is_err());
            if stage == "before-prepare" {
                assert!(host
                    .startup_detail("project", "codex-attempt")
                    .unwrap()
                    .evidence_digest
                    .is_some());
            } else if stage == "prepared" {
                assert!(host.application_recovery_review("shared-codex").is_ok());
            } else {
                assert!(host
                    .unsent_assignments()
                    .iter()
                    .any(|r| r.attempt_id == "codex-attempt"));
            }
        }
        for _ in 0..3 {
            host.poll_verifications();
        }
        assert_eq!(host.projects["project"].history(), recovered);
        assert_eq!(wire(&root), before_wire);
        assert!(
            host.codex_active.is_empty()
                && host.omp_active.is_empty()
                && host.api_active.is_empty()
                && host.api_writers.is_empty()
        );
        assert_ne!(host.runs.all()[0].state, RunState::Active);
        let management = records(&data.join("management-decisions"));
        json_file(&root.join("recovered-snapshot.json"), &host.snapshot());
        drop(host);
        let mut host = RuntimeHost::open(&data).unwrap();
        host.poll_verifications();
        assert_eq!(host.projects["project"].history(), recovered);
        assert_eq!(records(&data.join("management-decisions")), management);
        assert_eq!(wire(&root), before_wire);
        assert_eq!(host.snapshot().workflows.len(), 1);
        let resolution = if explicit {
            let result = explicit_recovery(&mut host, &root, stage);
            json_file(&root.join("resolution.json"), &result);
            let final_history = host.projects["project"].history().to_vec();
            let final_management = records(&data.join("management-decisions"));
            let final_wire = wire(&root);
            json_file(&root.join("final-snapshot.json"), &host.snapshot());
            drop(host);
            let mut reopened = RuntimeHost::open(&data).unwrap();
            reopened.poll_verifications();
            assert_eq!(reopened.projects["project"].history(), final_history);
            assert_eq!(
                records(&data.join("management-decisions")),
                final_management
            );
            assert_eq!(wire(&root), final_wire);
            for (path, bytes) in &original {
                assert_eq!(&fs::read(path).unwrap(), bytes);
            }
            Some(result)
        } else {
            None
        };
        cases.push(json!({"stage":stage,"status":"passed","actualOwnerKill":true,"peerExits":peers.len(),"peers":marked["peers"],
            "originalRecords":original.len(),"originalRecordsPreserved":true,"reopenCount":2,"workerRestarts":0,"promptResends":0,
            "beforeSequence":before.sequence,"recoveredSequence":recovered.len(),"firstReopenNewEvents":recovered.len()-before.sequence as usize,
            "secondReopenNewEvents":0,"unknownResourcesHeld":true,"explicitResolution":resolution,"realModelCalls":0}));
        println!("Shared actual kill {stage}: {} peers exited, original identities/resources held, reopen2 events 0, resend 0",peers.len());
    }
    json_file(
        &outer.join("result.json"),
        &json!({"status":"passed","cases":cases,"realModelCalls":0,"userDesktopActions":0}),
    );
}
