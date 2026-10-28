use super::*;
use crate::startup_closure_review as review;
use crate::startup_journal::{PublishPoint, StartupClosure, StartupJournal};
use std::{
    io::Write,
    os::windows::process::CommandExt,
    process::{Child, Command as ProcessCommand, Stdio},
    time::{Duration, Instant},
};
fn journal(host: &mut RuntimeHost, omp: bool) -> &mut StartupJournal {
    if omp {
        &mut host.omp_startups
    } else {
        &mut host.startups
    }
}
fn folder(omp: bool) -> &'static str {
    if omp {
        "omp-startups"
    } else {
        "codex-startups"
    }
}
pub(super) fn fixture(root: &Path, omp: bool, point: PublishPoint) -> (PathBuf, String) {
    let (mut host, intent) = super::startup_publication_tests::fixture(root, omp);
    journal(&mut host, omp).begin(intent.clone()).unwrap();
    let head = journal(&mut host, omp)
        .get("project", "attempt")
        .unwrap()
        .head
        .clone();
    let close = StartupClosure {
        reason: "interrupted old reason".into(),
        workflow_sequence: intent.workflow_sequence,
        workflow_hash: intent.workflow_hash,
    };
    assert!(journal(&mut host, omp)
        .close_observed("project", "attempt", &head, close, |p| if p == point {
            Err("interrupted".into())
        } else {
            Ok(())
        })
        .is_err());
    (
        host.root.clone(),
        format!("{}/project/attempt.1.pending", folder(omp)),
    )
}
pub(super) fn acknowledge(data: &Path, relative: &str) {
    let proof = review::inspect(data, relative).unwrap().0;
    review::acknowledge(
        data,
        relative,
        &proof,
        "preserve and re-review closure",
        |_| Ok(()),
    )
    .unwrap();
}
pub(super) fn close(host: &mut RuntimeHost, omp: bool) {
    let command = if omp {
        Command::AbandonOmpStartup {
            project_id: "project".into(),
            attempt_id: "attempt".into(),
            evidence_digest: host.snapshot().omp_jobs[0].closure_digest.clone().unwrap(),
            reason: "new explicit closure".into(),
        }
    } else {
        Command::AbandonCodexStartup {
            project_id: "project".into(),
            attempt_id: "attempt".into(),
            evidence_digest: host
                .startup_detail("project", "attempt")
                .unwrap()
                .evidence_digest
                .unwrap(),
            reason: "new explicit closure".into(),
        }
    };
    let response = host.handle(Request {
        id: "close".into(),
        protocol_version: PROTOCOL_VERSION,
        command,
    });
    assert!(response.error.is_none(), "{:?}", response.error);
}
fn events(data: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fs::read_dir(data.join("workflows/project/events"))
        .unwrap()
        .map(|e| {
            let path = e.unwrap().path();
            let raw = fs::read(&path).unwrap();
            (path, raw)
        })
        .collect()
}

#[test]
fn closure_review_preserves_original_identity_and_requires_new_original_host_review() {
    for omp in [false, true] {
        for point in [PublishPoint::TempCreated, PublishPoint::PartialWritten] {
            let root = tempfile::tempdir().unwrap();
            let (data, relative) = fixture(root.path(), omp, point);
            let original = fs::read(data.join(&relative)).unwrap();
            let ledger = events(&data);
            let intent_path = data.join(folder(omp)).join("project/attempt.0.json");
            let intent_bytes = fs::read(&intent_path).unwrap();
            assert!(RuntimeHost::open(&data).is_err());
            let proof = review::inspect(&data, &relative).unwrap().0;
            assert!(
                review::acknowledge(&data, &relative, &"0".repeat(64), "stale", |_| Ok(()))
                    .is_err()
            );
            review::acknowledge(&data, &relative, &proof, "review only", |_| Ok(())).unwrap();
            let mut host = RuntimeHost::open(&data).unwrap();
            assert!(journal(&mut host, omp)
                .get("project", "attempt")
                .unwrap()
                .closure
                .is_none());
            assert_eq!(host.snapshot().startup_closure_reviews.len(), 1);
            assert!(journal(&mut host, omp)
                .check_reservation("project", "attempt", None)
                .is_err());
            close(&mut host, omp);
            drop(host);
            for _ in 0..2 {
                let mut host = RuntimeHost::open(&data).unwrap();
                assert_eq!(
                    journal(&mut host, omp)
                        .get("project", "attempt")
                        .unwrap()
                        .closure
                        .as_ref()
                        .unwrap()
                        .reason,
                    "new explicit closure"
                );
                assert!(host.codex_active.is_empty() && host.omp_active.is_empty());
                assert_eq!(
                    host.projects["project"].state().tasks["task"].state,
                    TaskState::Pending
                );
            }
            assert_eq!(fs::read(data.join(&relative)).unwrap(), original);
            assert_eq!(fs::read(intent_path).unwrap(), intent_bytes);
            assert_eq!(events(&data), ledger);
        }
    }
}

#[test]
fn closure_retry_interruption_never_overwrites_residue_or_invents_completion() {
    for omp in [false, true] {
        for point in [
            PublishPoint::TempCreated,
            PublishPoint::PartialWritten,
            PublishPoint::TempSynced,
            PublishPoint::Published,
            PublishPoint::PendingRemoved,
        ] {
            let root = tempfile::tempdir().unwrap();
            let (data, relative) = fixture(root.path(), omp, PublishPoint::PartialWritten);
            acknowledge(&data, &relative);
            let old = fs::read(data.join(&relative)).unwrap();
            let ledger = events(&data);
            let mut host = RuntimeHost::open(&data).unwrap();
            let j = journal(&mut host, omp);
            let record = j.get("project", "attempt").unwrap().clone();
            let closure = StartupClosure {
                reason: "new user close after review".into(),
                workflow_sequence: record.intent.workflow_sequence,
                workflow_hash: record.intent.workflow_hash,
            };
            assert!(j
                .close_observed("project", "attempt", &record.head, closure, |p| {
                    if p == point {
                        Err("again".into())
                    } else {
                        Ok(())
                    }
                })
                .is_err());
            drop(host);
            if matches!(
                point,
                PublishPoint::TempCreated | PublishPoint::PartialWritten
            ) {
                assert!(RuntimeHost::open(&data).is_err());
                let next = fs::read_dir(data.join(folder(omp)).join("project"))
                    .unwrap()
                    .map(|e| e.unwrap().path())
                    .find(|p| {
                        p.extension().is_some_and(|e| e == "pending") && p != &data.join(&relative)
                    })
                    .unwrap();
                let retry_original = fs::read(&next).unwrap();
                let next_relative = next
                    .strip_prefix(&data)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                acknowledge(&data, &next_relative);
                let mut host = RuntimeHost::open(&data).unwrap();
                assert!(journal(&mut host, omp)
                    .get("project", "attempt")
                    .unwrap()
                    .closure
                    .is_none());
                close(&mut host, omp);
                drop(host);
                assert_eq!(fs::read(next).unwrap(), retry_original);
            }
            let mut host = RuntimeHost::open(&data).unwrap();
            assert!(journal(&mut host, omp)
                .get("project", "attempt")
                .unwrap()
                .closure
                .is_some());
            assert_eq!(fs::read(data.join(&relative)).unwrap(), old);
            assert_eq!(events(&data), ledger);
        }
    }
}

#[test]
fn closure_review_rejects_changed_original_identity_invalid_json_and_execution_evidence() {
    for mode in [
        "raw-changed",
        "intent-changed",
        "complete-invalid",
        "execution",
    ] {
        let root = tempfile::tempdir().unwrap();
        let (data, relative) = fixture(root.path(), true, PublishPoint::PartialWritten);
        if mode == "complete-invalid" {
            fs::write(data.join(&relative), b"{}").unwrap();
            assert!(review::inspect(&data, &relative).is_err());
            continue;
        }
        acknowledge(&data, &relative);
        match mode {
            "raw-changed" => fs::write(data.join(&relative), b"{").unwrap(),
            "intent-changed" => {
                fs::write(data.join("omp-startups/project/attempt.0.json"), b"{}").unwrap()
            }
            "execution" => {
                let host = RuntimeHost::open(&data).unwrap();
                fs::create_dir_all(
                    host.projects["project"]
                        .omp_attempt_path("attempt")
                        .unwrap(),
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        assert!(RuntimeHost::open(&data).is_err(), "{mode}");
    }
}

struct Owner(Child);
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
#[ignore = "owned closure review/retry writer; isolated parent case only"]
fn closure_owner() {
    let root = PathBuf::from(std::env::var_os("CERULEAN_CLOSURE_CASE").unwrap());
    let omp = std::env::var("CERULEAN_CLOSURE_EXECUTOR").unwrap() == "omp";
    let phase = std::env::var("CERULEAN_CLOSURE_PHASE").unwrap();
    let point = std::env::var("CERULEAN_CLOSURE_POINT").unwrap();
    let (data, relative) = fixture(&root, omp, PublishPoint::PartialWritten);
    let pause = |p: &str| -> Result<(), String> {
        if p == point {
            let mut marker = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(root.join("ready.json"))
                .unwrap();
            write!(
                marker,
                "{}",
                serde_json::json!({"pid":std::process::id(),"phase":phase,"point":point})
            )
            .unwrap();
            marker.sync_all().unwrap();
            drop(marker);
            loop {
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        Ok(())
    };
    if phase == "review" {
        let _service = crate::diagnostic_service::ServiceHost::open(&data).unwrap();
        let proof = review::inspect(&data, &relative).unwrap().0;
        review::acknowledge(&data, &relative, &proof, "first closure review", pause).unwrap();
    } else {
        acknowledge(&data, &relative);
        let mut host = RuntimeHost::open(&data).unwrap();
        let j = journal(&mut host, omp);
        let r = j.get("project", "attempt").unwrap().clone();
        j.close_observed(
            "project",
            "attempt",
            &r.head,
            StartupClosure {
                reason: "reviewed retry close".into(),
                workflow_sequence: r.intent.workflow_sequence,
                workflow_hash: r.intent.workflow_hash,
            },
            |p| pause(&format!("{p:?}")),
        )
        .unwrap();
    }
    panic!("closure boundary not reached");
}
#[test]
#[ignore = "20 actual closure review/retry writer terminations; explicit fresh report, no workers"]
fn twenty_actual_closure_review_and_retry_interruptions() {
    let report = PathBuf::from(std::env::var_os("CERULEAN_CLOSURE_REPORT").unwrap());
    assert!(report.is_absolute() && !report.exists());
    let mut cases = vec![];
    for omp in [false, true] {
        for phase in ["review", "retry"] {
            for point in if phase == "review" {
                ["created", "partial", "synced", "published", "finished"]
            } else {
                [
                    "TempCreated",
                    "PartialWritten",
                    "TempSynced",
                    "Published",
                    "PendingRemoved",
                ]
            } {
                let root = tempfile::tempdir().unwrap();
                let dir = root.path();
                let data = dir.join("data");
                let log = fs::File::create(dir.join("child.log")).unwrap();
                let mut child = Owner(
                    ProcessCommand::new(std::env::current_exe().unwrap())
                        .args([
                            "--exact",
                            "host::startup_closure_tests::closure_owner",
                            "--ignored",
                            "--nocapture",
                        ])
                        .env("CERULEAN_CLOSURE_CASE", dir)
                        .env(
                            "CERULEAN_CLOSURE_EXECUTOR",
                            if omp { "omp" } else { "codex" },
                        )
                        .env("CERULEAN_CLOSURE_PHASE", phase)
                        .env("CERULEAN_CLOSURE_POINT", point)
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
                        child.0.try_wait().unwrap().is_none(),
                        "{}",
                        fs::read_to_string(dir.join("child.log")).unwrap_or_default()
                    );
                    assert!(Instant::now() < deadline, "closure marker timeout");
                    std::thread::sleep(Duration::from_millis(10));
                };
                assert_eq!(marker["pid"].as_u64().unwrap(), u64::from(child.0.id()));
                let relative = format!("{}/project/attempt.1.pending", folder(omp));
                let old = fs::read(data.join(&relative)).unwrap();
                let intent_path = data.join(folder(omp)).join("project/attempt.0.json");
                let intent = fs::read(&intent_path).unwrap();
                let ledger = events(&data);
                let review_files: BTreeMap<_, _> =
                    fs::read_dir(data.join("startup-closure-reviews"))
                        .unwrap()
                        .map(|e| {
                            let p = e.unwrap().path();
                            let raw = fs::read(&p).unwrap();
                            (p, raw)
                        })
                        .collect();
                child.0.kill().unwrap();
                child.0.wait().unwrap();
                let requires_confirmation = if phase == "review" {
                    matches!(point, "created" | "partial" | "synced")
                } else {
                    matches!(point, "TempCreated" | "PartialWritten")
                };
                if requires_confirmation {
                    assert!(RuntimeHost::open(&data).is_err());
                    let target = if phase == "review" {
                        relative.clone()
                    } else {
                        fs::read_dir(data.join(folder(omp)).join("project"))
                            .unwrap()
                            .map(|e| e.unwrap().path())
                            .find(|p| {
                                p.extension().is_some_and(|s| s == "pending")
                                    && p != &data.join(&relative)
                            })
                            .unwrap()
                            .strip_prefix(&data)
                            .unwrap()
                            .to_string_lossy()
                            .replace('\\', "/")
                    };
                    acknowledge(&data, &target);
                }
                let mut host = RuntimeHost::open(&data).unwrap();
                if phase == "review" || requires_confirmation {
                    assert!(journal(&mut host, omp)
                        .get("project", "attempt")
                        .unwrap()
                        .closure
                        .is_none());
                    close(&mut host, omp);
                }
                drop(host);
                for _ in 0..2 {
                    let mut host = RuntimeHost::open(&data).unwrap();
                    host.poll_verifications();
                    host.poll_project_runs();
                    assert!(journal(&mut host, omp)
                        .get("project", "attempt")
                        .unwrap()
                        .closure
                        .is_some());
                    assert!(
                        host.codex_active.is_empty()
                            && host.omp_active.is_empty()
                            && host.management_active.is_empty()
                    );
                    assert_eq!(
                        host.projects["project"].state().tasks["task"].state,
                        TaskState::Pending
                    );
                }
                assert_eq!(fs::read(data.join(&relative)).unwrap(), old);
                assert_eq!(fs::read(intent_path).unwrap(), intent);
                assert_eq!(events(&data), ledger);
                for (p, raw) in review_files {
                    assert_eq!(fs::read(p).unwrap(), raw);
                }
                cases.push(serde_json::json!({"executor":if omp{"omp"}else{"codex"},"phase":phase,"point":point,"requiredNewConfirmation":requires_confirmation,"intentAndOriginalClosurePreserved":true,"workflowUnchanged":true,"task":"pending","workerStarts":0}));
            }
        }
    }
    fs::write(
        report,
        serde_json::to_vec_pretty(
            &serde_json::json!({"status":"passed","realModelCalls":0,"cases":cases}),
        )
        .unwrap(),
    )
    .unwrap();
}
