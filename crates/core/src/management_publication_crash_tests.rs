//! Actual writer-process termination. This tests the journal, not a provider.
use super::*;
use std::{
    os::windows::process::CommandExt,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
struct Owner(Child);
impl Drop for Owner {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
#[ignore = "owned management publication child; isolated parent test only"]
fn publication_owner() {
    let root = PathBuf::from(std::env::var_os("CERULEAN_MANAGEMENT_CASE").unwrap());
    let kind = std::env::var("CERULEAN_MANAGEMENT_KIND").unwrap();
    let point = std::env::var("CERULEAN_MANAGEMENT_POINT").unwrap();
    let (sequence, expected) = seed(&root, &kind);
    let (target, _) = paths(&root, sequence);
    let (record, _) = read(&target).unwrap();
    // Keep the valid prefix; republish only this final fixture record through
    // the production writer and actually terminate inside that write.
    fs::remove_file(&target).unwrap();
    let mut journal = DecisionJournal::open(&root).unwrap();
    journal
        .publish_observed(
            &record.body.operation_id,
            sequence,
            record.body.previous,
            record.body.evidence,
            |p| {
                if format!("{p:?}") == point {
                    let mut marker = OpenOptions::new()
                        .create_new(true)
                        .write(true)
                        .open(root.join("ready.json"))
                        .unwrap();
                    writeln!(
                        marker,
                        "{}",
                        serde_json::json!({
                            "pid":std::process::id(),"kind":kind,"point":point,
                            "sequence":sequence,"expected":expected
                        })
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
    panic!("publication boundary was not reached");
}

#[test]
#[ignore = "45 actual management writer terminations; fresh explicit report, no provider"]
fn forty_five_actual_publication_crashes_preserve_evidence() {
    let report = PathBuf::from(std::env::var_os("CERULEAN_MANAGEMENT_REPORT").unwrap());
    assert!(report.is_absolute() && !report.exists());
    let mut cases = vec![];
    for kind in KINDS {
        for point in [
            PublishPoint::TempCreated,
            PublishPoint::PartialWritten,
            PublishPoint::TempSynced,
            PublishPoint::Published,
            PublishPoint::PendingRemoved,
        ] {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path();
            let log = fs::File::create(dir.join("child.log")).unwrap();
            let mut owner = Owner(
                Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "management_execution::publication_tests::crash_tests::publication_owner",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("CERULEAN_MANAGEMENT_CASE", dir)
                    .env("CERULEAN_MANAGEMENT_KIND", kind)
                    .env("CERULEAN_MANAGEMENT_POINT", format!("{point:?}"))
                    .creation_flags(0x08000000)
                    .stdin(Stdio::null())
                    .stdout(Stdio::from(log.try_clone().unwrap()))
                    .stderr(Stdio::from(log))
                    .spawn()
                    .unwrap(),
            );
            let deadline = Instant::now() + Duration::from_secs(30);
            let marker: Value = loop {
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
                assert!(
                    Instant::now() < deadline,
                    "management writer marker timeout"
                );
                std::thread::sleep(Duration::from_millis(10));
            };
            assert_eq!(marker["pid"].as_u64().unwrap(), u64::from(owner.0.id()));
            assert_eq!(marker["point"], format!("{point:?}"));
            assert_eq!(marker["kind"], kind);
            let (target, pending) = paths(dir, marker["sequence"].as_u64().unwrap() as u8);
            let before = inventory(dir);
            owner.0.kill().unwrap();
            owner.0.wait().unwrap();
            let complete = !matches!(
                point,
                PublishPoint::TempCreated | PublishPoint::PartialWritten
            );
            if !complete {
                for _ in 0..2 {
                    assert!(DecisionJournal::open(dir).is_err());
                    assert_eq!(inventory(dir), before);
                }
                assert!(!target.exists());
            } else {
                let expected = if pending.exists() {
                    fs::read(&pending).unwrap()
                } else {
                    fs::read(&target).unwrap()
                };
                for _ in 0..2 {
                    let mut journal = DecisionJournal::open(dir).unwrap();
                    journal.recover_publications().unwrap();
                    assert_eq!(
                        serde_json::to_value(&journal.statuses()[0].state).unwrap(),
                        marker["expected"]
                    );
                    assert_eq!(fs::read(&target).unwrap(), expected);
                    assert!(!pending.exists());
                    let after = inventory(dir);
                    for (name, bytes) in &before {
                        if !name.ends_with(".pending") {
                            assert_eq!(after.get(name), Some(bytes));
                        }
                    }
                    assert_eq!(
                        after.len(),
                        before.len() - usize::from(point == PublishPoint::Published)
                    );
                }
            }
            cases.push(serde_json::json!({"kind":kind,"point":format!("{point:?}"),
                "actualOwnerKill":true,"reopens":2,"originalBytesPreserved":true,
                "disposition":if complete {"complete-or-recovered"} else {"preserved-and-refused"}}));
        }
    }
    assert_eq!(cases.len(), 45);
    let mut output = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(report)
        .unwrap();
    output
        .write_all(
            &serde_json::to_vec_pretty(&serde_json::json!({
                "status":"passed","realModelCalls":0,"scope":"management journal publication",
                "recoveryActualCrashTested":false,"cases":cases
            }))
            .unwrap(),
        )
        .unwrap();
    output.sync_all().unwrap();
}
