//! Real host reopen after interruption of the shared startup journal writer.
use super::*;
use crate::startup_journal::{PublishPoint, StartupClosure, StartupIntent, OMP_PROTOCOL};
use std::{
    io::Write,
    os::windows::process::CommandExt,
    process::{Child, Command as ProcessCommand, Stdio},
    time::{Duration, Instant},
};

pub(super) fn fixture(root: &Path, omp: bool) -> (RuntimeHost, StartupIntent) {
    let host = if omp {
        super::omp_host::tests::fixture(root, None)
    } else {
        let (mut host, draft) = super::project_planning::tests::fixture(root);
        host.configure_project(draft).unwrap();
        super::project_planning::tests::complete(
            &mut host,
            super::project_planning::tests::proposal("tests"),
        );
        let revision = host.projects["project"]
            .revision()
            .unwrap()
            .approval_digest()
            .unwrap();
        host.approve("project", &revision).unwrap();
        host
    };
    let store = &host.projects["project"];
    let plan = store.revision().unwrap();
    let intent = StartupIntent {
        schema_version: 1,
        protocol: if omp {
            OMP_PROTOCOL
        } else {
            "codex-durable-before-send-v1"
        }
        .into(),
        project_id: "project".into(),
        task_id: "task".into(),
        attempt_id: "attempt".into(),
        revision_digest: plan.approval_digest().unwrap(),
        worker: plan.task("task").unwrap().worker.clone().unwrap(),
        management_operation: None,
        management_hash: None,
        workflow_sequence: store.history().len() as u32,
        workflow_hash: store.history().last().map(|e| e.hash.clone()),
        started_at: chrono::Utc::now().to_rfc3339(),
    };
    (host, intent)
}
fn events(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fs::read_dir(root.join("data/workflows/project/events"))
        .unwrap()
        .map(|e| {
            let p = e.unwrap().path();
            let b = fs::read(&p).unwrap();
            (p, b)
        })
        .collect()
}
fn directory(omp: bool) -> &'static str {
    if omp {
        "omp-startups"
    } else {
        "codex-startups"
    }
}

#[test]
fn complete_pending_is_not_published_when_host_approval_or_management_binding_fails() {
    for omp in [false, true] {
        for mode in ["workflow-anchor", "management-link"] {
            let root = tempfile::tempdir().unwrap();
            let (mut host, mut intent) = fixture(root.path(), omp);
            if mode == "workflow-anchor" {
                intent.workflow_hash = Some("0".repeat(64));
            } else {
                intent.management_operation = Some("absent-decision".into());
                intent.management_hash = Some("0".repeat(64));
            }
            let journal = if omp {
                &mut host.omp_startups
            } else {
                &mut host.startups
            };
            assert!(journal
                .begin_observed(intent, |p| if p == PublishPoint::TempSynced {
                    Err("interrupted".into())
                } else {
                    Ok(())
                })
                .is_err());
            let pending = host
                .root
                .join(directory(omp))
                .join("project/attempt.0.pending");
            let before = fs::read(&pending).unwrap();
            let ledger = events(root.path());
            drop(host);
            assert!(
                RuntimeHost::open(&root.path().join("data")).is_err(),
                "{omp}:{mode}"
            );
            assert_eq!(fs::read(&pending).unwrap(), before);
            assert!(!pending.with_extension("json").exists());
            assert_eq!(events(root.path()), ledger);
        }
    }
}

#[test]
fn reviewed_incomplete_intents_reopen_without_fabrication_or_id_reuse() {
    use crate::diagnostic_service::ServiceHost;
    for omp in [false, true] {
        for point in [PublishPoint::TempCreated, PublishPoint::PartialWritten] {
            let root = tempfile::tempdir().unwrap();
            let (mut host, intent) = fixture(root.path(), omp);
            let wanted = intent.revision_digest.clone();
            let journal = if omp {
                &mut host.omp_startups
            } else {
                &mut host.startups
            };
            assert!(journal
                .begin_observed(intent, |p| if p == point {
                    Err("crash".into())
                } else {
                    Ok(())
                })
                .is_err());
            let path = host
                .root
                .join(directory(omp))
                .join("project/attempt.0.pending");
            let raw = fs::read(&path).unwrap();
            let ledger = events(root.path());
            drop(host);
            let data = root.path().join("data");
            let mut service = ServiceHost::open(&data).unwrap();
            let file = service
                .snapshot()
                .core_diagnostic
                .unwrap()
                .startup_files
                .remove(0);
            let result = service.handle(Request {
                id: "review".into(),
                protocol_version: PROTOCOL_VERSION,
                command: Command::ReviewStartupIntent {
                    relative_path: file.relative_path,
                    evidence_digest: file.review_digest.unwrap(),
                    reason: "keep original; reserve old ID".into(),
                },
            });
            assert!(result.error.is_none());
            assert!(service.is_diagnostic());
            drop(service);
            let mut host = RuntimeHost::open(&data).unwrap();
            assert_eq!(host.snapshot().startup_reservations.len(), 1);
            assert!(host.startups.all().is_empty() && host.omp_startups.all().is_empty());
            for journal in [&host.startups, &host.omp_startups] {
                if !journal.reservations.is_empty() {
                    assert!(journal
                        .check_reservation("project", "attempt", Some("new-owner"))
                        .is_err());
                }
            }
            // Both production entry points must reject before authentication or launch.
            assert!(host
                .start_codex("project", "task", "attempt", &wanted)
                .unwrap_err()
                .contains("reserved"));
            assert!(host
                .start_omp("project", "task", "attempt", &wanted)
                .unwrap_err()
                .contains("reserved"));
            assert!(host.codex_active.is_empty() && host.omp_active.is_empty());
            assert_eq!(
                host.projects["project"].state().tasks["task"].state,
                TaskState::Pending
            );
            assert_eq!(fs::read(&path).unwrap(), raw);
            assert_eq!(events(root.path()), ledger);
            drop(host);
            let host = RuntimeHost::open(&data).unwrap();
            assert_eq!(host.snapshot().startup_reservations.len(), 1);
        }
    }
}

#[test]
fn startup_review_cannot_bypass_other_executor_or_orphan_execution_evidence() {
    for mode in ["other-executor", "omp-attempt", "omp-files", "assigned"] {
        let root = tempfile::tempdir().unwrap();
        let (mut host, intent) = fixture(root.path(), false);
        if mode == "other-executor" {
            host.startups.begin(intent).unwrap();
        } else if mode == "assigned" {
            let store = host.projects.get_mut("project").unwrap();
            let binding = SessionBinding {
                adapter_id: "codex".into(),
                adapter_version: "fixture".into(),
                session_id: "synthetic-reviewed-conflict".into(),
                identity_strength: IdentityStrength::ProviderId,
                workspace: store.revision().unwrap().project.root.clone(),
            };
            store
                .command(
                    "prior-assignment",
                    "deputy",
                    WorkflowCommand::Assign {
                        task: "task".into(),
                        worker: "worker-codex".into(),
                        attempt: "attempt".into(),
                    },
                    CommandContext {
                        execution: Some(ExecutionEvidence::Attempt {
                            attempt: Attempt {
                                workspace_proof: None,
                                id: "attempt".into(),
                                task_id: "task".into(),
                                worker_id: "worker-codex".into(),
                                revision_digest: intent.revision_digest,
                                fence: 1,
                                binding,
                            },
                        }),
                        ..Default::default()
                    },
                    false,
                )
                .unwrap();
        } else {
            let store = &host.projects["project"];
            let path = if mode == "omp-attempt" {
                store.omp_attempt_path("attempt").unwrap()
            } else {
                store.omp_file_service_path("attempt").unwrap()
            };
            fs::create_dir_all(path).unwrap();
        }
        let relative = "omp-startups/project/attempt.0.pending";
        let path = host.root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"{").unwrap();
        let evidence = crate::startup_review::inspect(&host.root, relative)
            .unwrap()
            .0;
        crate::startup_review::acknowledge(
            &host.root,
            relative,
            &evidence,
            "review is not proof of no execution",
            |_| Ok(()),
        )
        .unwrap();
        let ledger = events(root.path());
        drop(host);
        let result = RuntimeHost::open(&root.path().join("data"));
        assert!(result.is_err(), "{mode}");
        assert_eq!(events(root.path()), ledger);
        assert_eq!(fs::read(path).unwrap(), b"{");
    }
}

#[test]
#[ignore = "startup publication crash helper; parent supplies fresh test case"]
fn startup_publication_owner() {
    let root = PathBuf::from(std::env::var_os("CERULEAN_STARTUP_CRASH_CASE").unwrap());
    let omp = std::env::var("CERULEAN_STARTUP_CRASH_PROTOCOL").unwrap() == "omp";
    let closing = std::env::var("CERULEAN_STARTUP_CRASH_CLOSE").unwrap() == "true";
    let target = std::env::var("CERULEAN_STARTUP_CRASH_POINT").unwrap();
    let (mut host, intent) = fixture(&root, omp);
    let journal = if omp {
        &mut host.omp_startups
    } else {
        &mut host.startups
    };
    let observer = |p: PublishPoint| -> Result<(), String> {
        if format!("{p:?}") == target {
            let mut marker = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(root.join("ready.json"))
                .unwrap();
            write!(
                marker,
                "{}",
                serde_json::json!({"point":target,"ownerPid":std::process::id()})
            )
            .unwrap();
            marker.sync_all().unwrap();
            drop(marker);
            loop {
                std::thread::park_timeout(Duration::from_secs(1));
            }
        }
        Ok(())
    };
    if closing {
        journal.begin(intent.clone()).unwrap();
        let head = journal.get("project", "attempt").unwrap().head.clone();
        journal
            .close_observed(
                "project",
                "attempt",
                &head,
                StartupClosure {
                    reason: "original explicit startup closure".into(),
                    workflow_sequence: intent.workflow_sequence,
                    workflow_hash: intent.workflow_hash,
                },
                observer,
            )
            .unwrap();
    } else {
        journal.begin_observed(intent, observer).unwrap();
    }
    panic!("startup publication point not reached");
}
struct Owner(Child);
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "owned startup review publication helper; parent supplies isolated case"]
fn startup_review_owner() {
    let root = PathBuf::from(std::env::var_os("CERULEAN_STARTUP_CRASH_CASE").unwrap());
    let omp = std::env::var("CERULEAN_STARTUP_CRASH_PROTOCOL").unwrap() == "omp";
    let point = std::env::var("CERULEAN_STARTUP_REVIEW_POINT").unwrap();
    let (mut host, intent) = fixture(&root, omp);
    let journal = if omp {
        &mut host.omp_startups
    } else {
        &mut host.startups
    };
    assert!(journal
        .begin_observed(intent, |p| if p == PublishPoint::PartialWritten {
            Err("fixture interrupted intent".into())
        } else {
            Ok(())
        })
        .is_err());
    let relative = format!("{}/project/attempt.0.pending", directory(omp));
    let evidence = crate::startup_review::inspect(&host.root, &relative)
        .unwrap()
        .0;
    crate::startup_review::acknowledge(
        &host.root,
        &relative,
        &evidence,
        "first confirmed reason",
        |p| {
            if p == point {
                let mut marker = fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(root.join("ready.json"))
                    .unwrap();
                write!(
                    marker,
                    "{}",
                    serde_json::json!({"ownerPid":std::process::id(),"point":p})
                )
                .unwrap();
                marker.sync_all().unwrap();
                drop(marker);
                loop {
                    std::thread::sleep(Duration::from_secs(1));
                }
            }
            Ok(())
        },
    )
    .unwrap();
    panic!("review boundary not reached");
}

#[test]
#[ignore = "10 actual review publication terminations; fresh explicit report, no worker or provider"]
fn ten_startup_review_terminations_preserve_original_and_reservations() {
    let report = PathBuf::from(std::env::var_os("CERULEAN_STARTUP_REVIEW_REPORT").unwrap());
    assert!(report.is_absolute() && !report.exists());
    let mut cases = vec![];
    for omp in [false, true] {
        for point in ["created", "partial", "synced", "published", "finished"] {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path();
            let log = fs::File::create(dir.join("owner.log")).unwrap();
            let mut owner = Owner(
                ProcessCommand::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "host::startup_publication_tests::startup_review_owner",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("CERULEAN_STARTUP_CRASH_CASE", dir)
                    .env(
                        "CERULEAN_STARTUP_CRASH_PROTOCOL",
                        if omp { "omp" } else { "codex" },
                    )
                    .env("CERULEAN_STARTUP_REVIEW_POINT", point)
                    .creation_flags(0x08000000)
                    .stdin(Stdio::null())
                    .stdout(Stdio::from(log.try_clone().unwrap()))
                    .stderr(Stdio::from(log))
                    .spawn()
                    .unwrap(),
            );
            let deadline = Instant::now() + Duration::from_secs(25);
            let marker: serde_json::Value = loop {
                if let Ok(raw) = fs::read(dir.join("ready.json")) {
                    if let Ok(m) = serde_json::from_slice(&raw) {
                        break m;
                    }
                }
                assert!(
                    owner.0.try_wait().unwrap().is_none(),
                    "{}",
                    fs::read_to_string(dir.join("owner.log")).unwrap_or_default()
                );
                assert!(Instant::now() < deadline, "review marker timeout");
                std::thread::sleep(Duration::from_millis(10));
            };
            assert_eq!(
                marker["ownerPid"].as_u64().unwrap(),
                u64::from(owner.0.id())
            );
            let data = dir.join("data");
            let relative = format!("{}/project/attempt.0.pending", directory(omp));
            let original = fs::read(data.join(&relative)).unwrap();
            let ledger = events(dir);
            let review_files: BTreeMap<_, _> = fs::read_dir(data.join("startup-intent-reviews"))
                .unwrap()
                .map(|e| {
                    let p = e.unwrap().path();
                    let b = fs::read(&p).unwrap();
                    (p, b)
                })
                .collect();
            owner.0.kill().unwrap();
            owner.0.wait().unwrap();
            let committed = matches!(point, "published" | "finished");
            if !committed {
                assert!(RuntimeHost::open(&data).is_err());
                let proof = crate::startup_review::inspect(&data, &relative).unwrap().0;
                crate::startup_review::acknowledge(
                    &data,
                    &relative,
                    &proof,
                    "explicit retry after interruption",
                    |_| Ok(()),
                )
                .unwrap();
            }
            for _ in 0..2 {
                let mut host = RuntimeHost::open(&data).unwrap();
                host.poll_verifications();
                host.poll_project_runs();
                let snapshot = host.snapshot();
                assert_eq!(snapshot.startup_reservations.len(), 1);
                assert_eq!(
                    snapshot.startup_reservations[0].reason,
                    if committed {
                        "first confirmed reason"
                    } else {
                        "explicit retry after interruption"
                    }
                );
                let j = if omp {
                    &host.omp_startups
                } else {
                    &host.startups
                };
                assert!(j
                    .check_reservation("project", "attempt", Some("replacement"))
                    .is_err());
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
            assert_eq!(events(dir), ledger);
            assert_eq!(fs::read(data.join(&relative)).unwrap(), original);
            for (path, raw) in review_files {
                assert_eq!(fs::read(path).unwrap(), raw);
            }
            cases.push(serde_json::json!({"executor":if omp{"omp"}else{"codex"},"point":point,"committedBeforeKill":committed,"originalPreserved":true,"idReserved":true,"task":"pending","workflowBytesUnchanged":true,"workerStarts":0}));
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

#[test]
#[ignore = "20 real startup/closure publication interruptions; explicit fresh report; no model or worker process"]
fn twenty_real_startup_interruptions_preserve_identity_and_never_start_worker() {
    let report = PathBuf::from(std::env::var_os("CERULEAN_STARTUP_CRASH_REPORT").unwrap());
    assert!(report.is_absolute() && !report.exists());
    let mut results = vec![];
    for omp in [false, true] {
        for closing in [false, true] {
            for point in [
                PublishPoint::TempCreated,
                PublishPoint::PartialWritten,
                PublishPoint::TempSynced,
                PublishPoint::Published,
                PublishPoint::PendingRemoved,
            ] {
                let root = tempfile::tempdir().unwrap();
                let dir = root.path();
                let log = fs::File::create(dir.join("owner.log")).unwrap();
                let mut owner = Owner(
                    ProcessCommand::new(std::env::current_exe().unwrap())
                        .args([
                            "--exact",
                            "host::startup_publication_tests::startup_publication_owner",
                            "--ignored",
                            "--nocapture",
                        ])
                        .env("CERULEAN_STARTUP_CRASH_CASE", dir)
                        .env(
                            "CERULEAN_STARTUP_CRASH_PROTOCOL",
                            if omp { "omp" } else { "codex" },
                        )
                        .env("CERULEAN_STARTUP_CRASH_CLOSE", closing.to_string())
                        .env("CERULEAN_STARTUP_CRASH_POINT", format!("{point:?}"))
                        .creation_flags(0x08000000)
                        .stdin(Stdio::null())
                        .stdout(Stdio::from(log.try_clone().unwrap()))
                        .stderr(Stdio::from(log))
                        .spawn()
                        .unwrap(),
                );
                let deadline = Instant::now() + Duration::from_secs(25);
                let marker: serde_json::Value = loop {
                    if let Ok(bytes) = fs::read(dir.join("ready.json")) {
                        if let Ok(value) = serde_json::from_slice(&bytes) {
                            break value;
                        }
                    }
                    assert!(
                        owner.0.try_wait().unwrap().is_none(),
                        "owner exited: {}",
                        fs::read_to_string(dir.join("owner.log")).unwrap_or_default()
                    );
                    assert!(Instant::now() < deadline, "startup marker timeout");
                    std::thread::sleep(Duration::from_millis(10));
                };
                assert_eq!(
                    marker["ownerPid"].as_u64().unwrap(),
                    u64::from(owner.0.id())
                );
                let folder = dir.join("data").join(directory(omp)).join("project");
                let target = folder.join(format!("attempt.{}.json", u8::from(closing)));
                let pending = target.with_extension("pending");
                let bytes = fs::read(if pending.exists() { &pending } else { &target }).unwrap();
                let ledger = events(dir);
                let old_intent = closing.then(|| fs::read(folder.join("attempt.0.json")).unwrap());
                owner.0.kill().unwrap();
                owner.0.wait().unwrap();
                let incomplete = matches!(
                    point,
                    PublishPoint::TempCreated | PublishPoint::PartialWritten
                );
                if incomplete {
                    assert!(RuntimeHost::open(&dir.join("data")).is_err());
                    assert!(RuntimeHost::open(&dir.join("data")).is_err());
                    assert_eq!(fs::read(&pending).unwrap(), bytes);
                    assert!(!target.exists());
                } else {
                    let mut host = RuntimeHost::open(&dir.join("data")).unwrap();
                    host.poll_verifications();
                    host.poll_project_runs();
                    let journal = if omp {
                        &host.omp_startups
                    } else {
                        &host.startups
                    };
                    let record = journal.get("project", "attempt").unwrap();
                    assert_eq!(record.closure.is_some(), closing);
                    assert!(journal
                        .check_reservation("project", "attempt", None)
                        .is_err());
                    assert_eq!(fs::read(&target).unwrap(), bytes);
                    assert!(!pending.exists());
                    assert!(
                        host.codex_active.is_empty()
                            && host.omp_active.is_empty()
                            && host.management_active.is_empty()
                    );
                    assert_eq!(
                        host.projects["project"].state().tasks["task"].state,
                        TaskState::Pending
                    );
                    if omp {
                        assert_eq!(
                            host.snapshot().omp_jobs[0].state,
                            if closing {
                                omp_jobs::OmpJobState::Abandoned
                            } else {
                                omp_jobs::OmpJobState::Unknown
                            }
                        );
                    }
                    drop(host);
                    let host = RuntimeHost::open(&dir.join("data")).unwrap();
                    assert!(host.codex_active.is_empty() && host.omp_active.is_empty());
                }
                assert_eq!(events(dir), ledger);
                if let Some(original) = old_intent {
                    assert_eq!(fs::read(folder.join("attempt.0.json")).unwrap(), original);
                }
                results.push(serde_json::json!({"protocol":if omp{"omp"}else{"codex"},"closing":closing,"point":format!("{point:?}"),"passed":true,"outcome":if incomplete{"refused-incomplete-evidence"}else{"validated-and-recovered"},"workflowBytesUnchanged":true,"originalBytesPreserved":true,"workerStarts":0}));
            }
        }
    }
    fs::write(
        report,
        serde_json::to_vec_pretty(
            &serde_json::json!({"version":1,"status":"passed","realModelCalls":0,"cases":results}),
        )
        .unwrap(),
    )
    .unwrap();
}
