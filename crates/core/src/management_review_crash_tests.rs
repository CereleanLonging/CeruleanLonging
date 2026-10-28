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
#[ignore = "owned management review child; isolated parent only"]
fn review_owner() {
    let root = PathBuf::from(std::env::var_os("CERULEAN_MANAGEMENT_REVIEW_CASE").unwrap());
    let kind = std::env::var("CERULEAN_MANAGEMENT_REVIEW_KIND").unwrap();
    let point = std::env::var("CERULEAN_MANAGEMENT_REVIEW_POINT").unwrap();
    let (_, relative, _, _) = damaged(&root, &kind, false);
    let evidence = inspect(&root, &relative).unwrap().0;
    acknowledge(&root,&relative,&evidence,"first confirmation",|p|{
        if p==point {
            let mut marker=OpenOptions::new().create_new(true).write(true).open(root.join("ready.json")).unwrap();
            writeln!(marker,"{}",serde_json::json!({"pid":std::process::id(),"relative":relative,"point":point,"evidence":evidence})).unwrap();marker.sync_all().unwrap();drop(marker);
            loop{std::thread::sleep(Duration::from_secs(1));}
        }
        Ok(())
    }).unwrap();
    panic!("review point not reached");
}
#[test]
#[ignore = "25 actual review publication terminations; fresh explicit report; no provider"]
fn twenty_five_review_terminations_keep_original_and_require_confirmation() {
    let report = PathBuf::from(std::env::var_os("CERULEAN_MANAGEMENT_REVIEW_REPORT").unwrap());
    assert!(report.is_absolute() && !report.exists());
    let mut cases = vec![];
    for kind in ["intent", "response", "apply", "applied", "discard"] {
        for point in ["created", "partial", "synced", "published", "finished"] {
            let root = tempfile::tempdir().unwrap();
            let dir = root.path();
            let log = fs::File::create(dir.join("child.log")).unwrap();
            let mut owner = Owner(
                Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "management_execution::review_tests::crash_tests::review_owner",
                        "--ignored",
                        "--nocapture",
                    ])
                    .env("CERULEAN_MANAGEMENT_REVIEW_CASE", dir)
                    .env("CERULEAN_MANAGEMENT_REVIEW_KIND", kind)
                    .env("CERULEAN_MANAGEMENT_REVIEW_POINT", point)
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
                    if let Ok(v) = serde_json::from_slice(&raw) {
                        break v;
                    }
                }
                assert!(
                    owner.0.try_wait().unwrap().is_none(),
                    "{}",
                    fs::read_to_string(dir.join("child.log")).unwrap_or_default()
                );
                assert!(Instant::now() < deadline, "review marker timeout");
                std::thread::sleep(Duration::from_millis(10));
            };
            assert_eq!(marker["pid"].as_u64().unwrap(), u64::from(owner.0.id()));
            assert_eq!(marker["point"], point);
            let relative = marker["relative"].as_str().unwrap();
            let evidence = marker["evidence"].as_str().unwrap();
            let original = fs::read(dir.join(relative)).unwrap();
            let before: BTreeMap<String, Vec<u8>> = fs::read_dir(dir.join("management-decisions"))
                .unwrap()
                .map(|e| {
                    let e = e.unwrap();
                    (
                        e.file_name().into_string().unwrap(),
                        fs::read(e.path()).unwrap(),
                    )
                })
                .collect();
            owner.0.kill().unwrap();
            owner.0.wait().unwrap();
            let published = matches!(point, "published" | "finished");
            assert_eq!(load(dir).unwrap().len(), usize::from(published));
            assert_eq!(DecisionJournal::open(dir).is_ok(), published);
            acknowledge(dir, relative, evidence, "new explicit confirmation", |_| {
                Ok(())
            })
            .unwrap();
            for _ in 0..2 {
                let journal = DecisionJournal::open(dir).unwrap();
                assert_eq!(journal.reviews.len(), 1);
                assert_eq!(
                    journal.reviews[0].reason,
                    if published {
                        "first confirmation"
                    } else {
                        "new explicit confirmation"
                    }
                );
                assert_eq!(fs::read(dir.join(relative)).unwrap(), original);
                for (name, raw) in &before {
                    assert_eq!(
                        fs::read(dir.join("management-decisions").join(name)).unwrap(),
                        *raw
                    );
                }
            }
            cases.push(serde_json::json!({"kind":kind,"point":point,"actualOwnerKill":true,"originalBytesPreserved":true,"prefixUnchanged":true,"newConfirmationRequired":!published}));
        }
    }
    let mut f = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(report)
        .unwrap();
    f.write_all(&serde_json::to_vec_pretty(&serde_json::json!({"status":"passed","scope":"management review receipt publication","realModelCalls":0,"cases":cases})).unwrap()).unwrap();
    f.sync_all().unwrap();
}
