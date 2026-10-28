//! Actual owner termination between the two durable assignment acknowledgements.
//! The worker is a simulated thread using the production assignment exchange;
//! these cases do not claim a real provider process or an interrupted file write.
use super::*;
use std::{
    collections::BTreeMap,
    io::Write,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command as ProcessCommand, Stdio},
};

struct Owner(Child);
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(dir)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            (
                e.file_name().into_string().unwrap(),
                fs::read(e.path()).unwrap(),
            )
        })
        .collect()
}

fn pause(root: &Path, stage: &str, completed: usize) -> ! {
    let mut marker = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(root.join("ready.json"))
        .unwrap();
    writeln!(
        marker,
        "{}",
        serde_json::json!({"pid":std::process::id(),"stage":stage,"completed":completed})
    )
    .unwrap();
    marker.sync_all().unwrap();
    drop(marker);
    loop {
        std::thread::sleep(Duration::from_secs(1));
    }
}

#[test]
#[ignore = "owned assignment helper; only called by the isolated parent test"]
fn assignment_owner() {
    let root = PathBuf::from(std::env::var_os("CERULEAN_ASSIGNMENT_CASE").unwrap());
    let stage = std::env::var("CERULEAN_ASSIGNMENT_STAGE").unwrap();
    let mut host = fixture(&root, None);
    decision(&mut host);
    let (_thread, completed) = simulated(&mut host);
    let Assignment::Prepare {
        operation,
        attempt,
        snapshot,
        reply,
    } = next(&host)
    else {
        panic!("prepare expected")
    };
    if stage == "before-prepare" {
        pause(&root, &stage, completed.load(Ordering::SeqCst));
    }
    host.prepare_managed_assignment("project", &operation, *attempt, *snapshot, "omp")
        .unwrap();
    if stage == "prepared" {
        pause(&root, &stage, completed.load(Ordering::SeqCst));
    }
    reply.send(Ok(())).unwrap();
    let Assignment::Commit {
        operation,
        entry,
        snapshot,
        reply,
    } = next(&host)
    else {
        panic!("commit expected")
    };
    assert_eq!(completed.load(Ordering::SeqCst), 0);
    if stage == "assigned" {
        pause(&root, &stage, 0);
    }
    host.commit_managed_assignment("project", &operation, *entry, *snapshot, "omp")
        .unwrap();
    if stage == "applied" {
        pause(&root, &stage, completed.load(Ordering::SeqCst));
    }
    assert_eq!(stage, "acknowledged");
    reply.send(Ok(())).unwrap();
    // Wait for the production exchange to return. There is deliberately no
    // provider prompt after it in this fixture.
    let deadline = Instant::now() + Duration::from_secs(5);
    while completed.load(Ordering::SeqCst) == 0 {
        assert!(Instant::now() < deadline, "assignment did not finish");
        std::thread::sleep(Duration::from_millis(5));
    }
    pause(&root, &stage, completed.load(Ordering::SeqCst));
}

#[test]
#[ignore = "five actual owner kills; explicit fresh report required; no model calls"]
fn five_assignment_owner_crashes_preserve_evidence_and_never_resend() {
    let report = PathBuf::from(std::env::var_os("CERULEAN_ASSIGNMENT_REPORT").unwrap());
    assert!(report.is_absolute() && !report.exists());
    let mut cases = vec![];
    for stage in [
        "before-prepare",
        "prepared",
        "assigned",
        "applied",
        "acknowledged",
    ] {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path();
        let data = dir.join("data");
        let log = fs::File::create(dir.join("child.log")).unwrap();
        let mut owner = Owner(
            ProcessCommand::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "host::omp_management_bridge::tests::assignment_crash_tests::assignment_owner",
                    "--ignored",
                    "--nocapture",
                ])
                .env("CERULEAN_ASSIGNMENT_CASE", dir)
                .env("CERULEAN_ASSIGNMENT_STAGE", stage)
                .creation_flags(0x08000000)
                .stdin(Stdio::null())
                .stdout(Stdio::from(log.try_clone().unwrap()))
                .stderr(Stdio::from(log))
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(30);
        let marker: serde_json::Value = loop {
            if let Ok(raw) = fs::read(dir.join("ready.json")) {
                if let Ok(m) = serde_json::from_slice(&raw) {
                    break m;
                }
            }
            assert!(
                owner.0.try_wait().unwrap().is_none(),
                "{}",
                fs::read_to_string(dir.join("child.log")).unwrap_or_default()
            );
            assert!(Instant::now() < deadline, "assignment marker timeout");
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(marker["pid"].as_u64().unwrap(), u64::from(owner.0.id()));
        assert_eq!(marker["stage"], stage);
        assert_eq!(
            marker["completed"].as_u64().unwrap(),
            u64::from(stage == "acknowledged")
        );
        let before = files(&data.join("management-decisions"));
        let ledger = files(&data.join("workflows/project/events"));
        let startup = files(&data.join("omp-startups/project"));
        assert_eq!(
            before.len(),
            match stage {
                "before-prepare" => 2,
                "prepared" | "assigned" => 3,
                _ => 4,
            }
        );
        assert!(!before.keys().any(|name| name.ends_with(".pending")));
        owner.0.kill().unwrap();
        owner.0.wait().unwrap();
        let mut after_first_open = None;
        for _ in 0..2 {
            let mut host = RuntimeHost::open(&data).unwrap();
            host.poll_verifications();
            host.poll_project_runs();
            assert!(
                host.omp_active.is_empty()
                    && host.codex_active.is_empty()
                    && host.management_active.is_empty()
            );
            let assigned = !matches!(stage, "before-prepare" | "prepared");
            assert_eq!(
                host.projects["project"].state().tasks["task"].state,
                if assigned {
                    TaskState::Assigned
                } else {
                    TaskState::Pending
                }
            );
            assert!(!host.projects["project"].history().iter().any(|e| {
                matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. })
            }));
            if stage == "before-prepare" {
                assert!(host.management.application("deputy-omp").is_none());
                assert!(host.snapshot().omp_jobs[0].closure_digest.is_some());
            } else {
                let (_, applied, receipt) = host.management.application("deputy-omp").unwrap();
                assert_eq!(applied, assigned);
                assert_eq!(receipt.is_some(), assigned);
                if stage == "prepared" {
                    assert!(host.application_recovery_review("deputy-omp").is_ok());
                } else {
                    assert!(host.application_recovery_review("deputy-omp").is_err());
                }
            }
            assert_eq!(
                host.start_managed_omp("deputy-omp", "another-attempt")
                    .unwrap_err(),
                if stage == "before-prepare" {
                    "管理决定已经拥有启动尝试"
                } else {
                    "管理决定已准备或应用，不能重新启动"
                }
            );
            let management = files(&data.join("management-decisions"));
            for (name, bytes) in &before {
                assert_eq!(management.get(name), Some(bytes));
            }
            assert_eq!(
                management.len(),
                before.len() + usize::from(stage == "assigned")
            );
            if let Some(first) = &after_first_open {
                assert_eq!(first, &management);
            } else {
                after_first_open = Some(management);
            }
            assert_eq!(files(&data.join("workflows/project/events")), ledger);
            assert_eq!(files(&data.join("omp-startups/project")), startup);
        }
        cases.push(serde_json::json!({
            "stage":stage,"status":"passed","actualOwnerKill":true,
            "originalManagementBytesPreserved":true,"workflowUnchanged":true,
            "startupUnchanged":true,"coreReopens":2,
            "reconstructedReceipt":stage=="assigned","workerRestarts":0,
            "assignmentExchangeReturnedBeforeKill":stage=="acknowledged"
        }));
    }
    let mut output = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(report)
        .unwrap();
    output
        .write_all(
            &serde_json::to_vec_pretty(&serde_json::json!({
                "status":"passed","executor":"omp","realModelCalls":0,
                "worker":"simulated thread using production assignment exchange",
                "publicationInterrupted":false,"cases":cases
            }))
            .unwrap(),
        )
        .unwrap();
    output.sync_all().unwrap();
}
