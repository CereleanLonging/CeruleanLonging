//! Actual termination while an already validated host restores publication.
use super::*;
use crate::startup_journal::{PublishPoint, RecoveryPoint, StartupClosure, StartupJournal};
use std::{
    io::Write,
    os::windows::process::CommandExt,
    process::{Child, Command as ProcessCommand, Stdio},
    time::{Duration, Instant},
};
fn journal(h: &mut RuntimeHost, omp: bool) -> &mut StartupJournal {
    if omp {
        &mut h.omp_startups
    } else {
        &mut h.startups
    }
}
fn folder(omp: bool) -> &'static str {
    if omp {
        "omp-startups"
    } else {
        "codex-startups"
    }
}
fn events(data: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fs::read_dir(data.join("workflows/project/events"))
        .unwrap()
        .map(|e| {
            let p = e.unwrap().path();
            let b = fs::read(&p).unwrap();
            (p, b)
        })
        .collect()
}
struct Owner(Child);
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "owned recovery publication helper; isolated parent case only"]
fn recovery_owner() {
    let root = PathBuf::from(std::env::var_os("CERULEAN_RECOVERY_CASE").unwrap());
    let omp = std::env::var("CERULEAN_RECOVERY_EXECUTOR").unwrap() == "omp";
    let kind = std::env::var("CERULEAN_RECOVERY_KIND").unwrap();
    let duplicate = std::env::var("CERULEAN_RECOVERY_DUPLICATE").unwrap() == "true";
    let point = std::env::var("CERULEAN_RECOVERY_POINT").unwrap();
    let (mut host, intent) = super::startup_publication_tests::fixture(&root, omp);
    let data = host.root.clone();
    let prepared = if duplicate {
        PublishPoint::Published
    } else {
        PublishPoint::TempSynced
    };
    if kind == "intent" {
        assert!(journal(&mut host, omp)
            .begin_observed(intent, |p| if p == prepared {
                Err("prepared complete publication".into())
            } else {
                Ok(())
            })
            .is_err());
    } else {
        journal(&mut host, omp).begin(intent).unwrap();
        if kind == "retry" {
            let r = journal(&mut host, omp)
                .get("project", "attempt")
                .unwrap()
                .clone();
            assert!(journal(&mut host, omp)
                .close_observed(
                    "project",
                    "attempt",
                    &r.head,
                    StartupClosure {
                        reason: "old incomplete close".into(),
                        workflow_sequence: r.intent.workflow_sequence,
                        workflow_hash: r.intent.workflow_hash
                    },
                    |p| if p == PublishPoint::PartialWritten {
                        Err("fixture partial".into())
                    } else {
                        Ok(())
                    }
                )
                .is_err());
            drop(host);
            let relative = format!("{}/project/attempt.1.pending", folder(omp));
            let proof = crate::startup_closure_review::inspect(&data, &relative)
                .unwrap()
                .0;
            crate::startup_closure_review::acknowledge(
                &data,
                &relative,
                &proof,
                "reviewed before retry",
                |_| Ok(()),
            )
            .unwrap();
            host = RuntimeHost::open(&data).unwrap();
        }
        let r = journal(&mut host, omp)
            .get("project", "attempt")
            .unwrap()
            .clone();
        assert!(journal(&mut host, omp)
            .close_observed(
                "project",
                "attempt",
                &r.head,
                StartupClosure {
                    reason: "original complete closure".into(),
                    workflow_sequence: r.intent.workflow_sequence,
                    workflow_hash: r.intent.workflow_hash
                },
                |p| if p == prepared {
                    Err("prepared complete publication".into())
                } else {
                    Ok(())
                }
            )
            .is_err());
    }
    drop(host);
    let dir = data.join(folder(omp)).join("project");
    let mut expected: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    for entry in fs::read_dir(&dir).unwrap() {
        let p = entry.unwrap().path();
        let name = p.file_name().unwrap().to_str().unwrap();
        let raw = fs::read(&p).unwrap();
        if kind == "retry" && name == "attempt.1.pending" {
            expected.insert(name.into(), raw);
            continue;
        }
        let target = if name.starts_with("attempt.0.") {
            "attempt.0.json"
        } else {
            "attempt.1.json"
        };
        if let Some(previous) = expected.insert(target.into(), raw.clone()) {
            assert_eq!(previous, raw);
        }
    }
    fs::write(
        root.join("expected.json"),
        serde_json::to_vec(&expected).unwrap(),
    )
    .unwrap();
    let _host = RuntimeHost::open_recovering(&data, |host| {
        let pause = |p: RecoveryPoint, _index: usize| -> Result<(), String> {
            if format!("{p:?}") == point {
                let mut marker = fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(root.join("ready.json"))
                    .unwrap();
                write!(
                    marker,
                    "{}",
                    serde_json::json!({"pid":std::process::id(),"point":point})
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
        if omp {
            host.startups.recover_publications()?;
            host.omp_startups.recover_publications_observed(pause)
        } else {
            host.startups.recover_publications_observed(pause)?;
            host.omp_startups.recover_publications()
        }
    })
    .unwrap();
    panic!("recovery point not reached");
}

#[test]
#[ignore = "54 actual validated recovery publication interruptions; fresh explicit report, no worker"]
fn fifty_four_recovery_interruptions_preserve_original_and_do_not_dispatch() {
    let report = PathBuf::from(std::env::var_os("CERULEAN_RECOVERY_REPORT").unwrap());
    assert!(report.is_absolute() && !report.exists());
    let mut cases = vec![];
    for omp in [false, true] {
        for kind in ["intent", "closure", "retry"] {
            for duplicate in [false, true] {
                for point in [
                    RecoveryPoint::Revalidated,
                    RecoveryPoint::Synced,
                    RecoveryPoint::FinalNamePresent,
                    RecoveryPoint::PendingRemoved,
                    RecoveryPoint::Finished,
                ] {
                    if duplicate && point == RecoveryPoint::Synced {
                        continue;
                    }
                    let root = tempfile::tempdir().unwrap();
                    let dir = root.path();
                    let data = dir.join("data");
                    let log = fs::File::create(dir.join("child.log")).unwrap();
                    let mut owner = Owner(
                        ProcessCommand::new(std::env::current_exe().unwrap())
                            .args([
                                "--exact",
                                "host::startup_recovery_crash_tests::recovery_owner",
                                "--ignored",
                                "--nocapture",
                            ])
                            .env("CERULEAN_RECOVERY_CASE", dir)
                            .env(
                                "CERULEAN_RECOVERY_EXECUTOR",
                                if omp { "omp" } else { "codex" },
                            )
                            .env("CERULEAN_RECOVERY_KIND", kind)
                            .env("CERULEAN_RECOVERY_DUPLICATE", duplicate.to_string())
                            .env("CERULEAN_RECOVERY_POINT", format!("{point:?}"))
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
                        assert!(Instant::now() < deadline, "recovery marker timeout");
                        std::thread::sleep(Duration::from_millis(10));
                    };
                    assert_eq!(marker["pid"].as_u64().unwrap(), u64::from(owner.0.id()));
                    let expected: BTreeMap<String, Vec<u8>> =
                        serde_json::from_slice(&fs::read(dir.join("expected.json")).unwrap())
                            .unwrap();
                    let ledger = events(&data);
                    owner.0.kill().unwrap();
                    owner.0.wait().unwrap();
                    for _ in 0..2 {
                        let mut host = RuntimeHost::open(&data).unwrap();
                        host.poll_verifications();
                        host.poll_project_runs();
                        let j = journal(&mut host, omp);
                        let r = j.get("project", "attempt").unwrap();
                        assert_eq!(r.closure.is_some(), kind != "intent");
                        assert!(j.check_reservation("project", "attempt", None).is_err());
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
                    for (name, raw) in &expected {
                        assert_eq!(
                            fs::read(data.join(folder(omp)).join("project").join(name)).unwrap(),
                            *raw
                        );
                    }
                    let remaining = fs::read_dir(data.join(folder(omp)).join("project"))
                        .unwrap()
                        .count();
                    assert_eq!(remaining, expected.len());
                    assert_eq!(events(&data), ledger);
                    cases.push(serde_json::json!({"executor":if omp{"omp"}else{"codex"},"kind":kind,"duplicateBeforeRecovery":duplicate,"point":format!("{point:?}"),"originalBytesPreserved":true,"workflowUnchanged":true,"workerStarts":0,"task":"pending"}));
                }
            }
        }
    }
    assert_eq!(cases.len(), 54);
    fs::write(
        report,
        serde_json::to_vec_pretty(
            &serde_json::json!({"status":"passed","realModelCalls":0,"cases":cases}),
        )
        .unwrap(),
    )
    .unwrap();
}
