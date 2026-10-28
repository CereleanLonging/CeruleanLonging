use super::super::tests::{dispatch_event, fixture, request, run};
use super::*;
#[tokio::test]
async fn preserved_images_are_bound_and_inspection_never_rewrites_user_changes() {
    for exists in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let (mut service, intent, plan, _) = fixture(temp.path());
        let path = Path::new(&plan.project.root).join("result.txt");
        if exists {
            fs::write(&path, "original data").unwrap();
        }
        let q = request(
            "1",
            "write_file",
            json!({"path":"result.txt","content":"replacement","expectedSha256":if exists{hash(b"original data")}else{"absent".into()}}),
        );
        let output = run(&mut service, &intent, q, false).await;
        assert!(!output.is_error, "{}", output.text);
        assert!(inspect(&temp.path().join("tools"), &intent, "1").is_err());
        drop(service);
        let evidence = temp.path().join("tools/writes").join(hash(b"1"));
        assert_eq!(
            fs::read(evidence.join("after.bin")).unwrap(),
            b"replacement"
        );
        if exists {
            assert_eq!(
                fs::read(evidence.join("before.bin")).unwrap(),
                b"original data"
            );
        } else {
            assert!(!evidence.join("before.bin").exists());
        }
        let inspection = inspect(&temp.path().join("tools"), &intent, "1").unwrap();
        assert_eq!(inspection.state, FileState::Intended);
        assert!(inspection.applied_recorded);
        assert!(inspection.tool_result_recorded);
        fs::write(&path, "later user edit").unwrap();
        assert_eq!(
            inspect(&temp.path().join("tools"), &intent, "1")
                .unwrap()
                .state,
            FileState::Other
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "later user edit");
        let mut foreign = intent.clone();
        foreign.fence += 1;
        assert!(inspect(&temp.path().join("tools"), &foreign, "1").is_err());
        fs::write(evidence.join("after.bin"), b"tampered").unwrap();
        assert!(inspect(&temp.path().join("tools"), &intent, "1").is_err());
    }
}
#[tokio::test]
async fn failed_image_preparation_cannot_modify_existing_file() {
    let temp = tempfile::tempdir().unwrap();
    let (mut service, intent, plan, _) = fixture(temp.path());
    let path = Path::new(&plan.project.root).join("result.txt");
    fs::write(&path, "original").unwrap();
    fs::create_dir(temp.path().join("tools/writes").join(hash(b"1"))).unwrap();
    let q = request(
        "1",
        "write_file",
        json!({"path":"result.txt","content":"replacement","expectedSha256":hash(b"original")}),
    );
    service.record(&intent, &dispatch_event(&q)).unwrap();
    let (_tx, rx) = watch::channel(false);
    assert!(service.start(q, rx).await.is_err());
    assert_eq!(fs::read_to_string(path).unwrap(), "original");
    assert_eq!(service.observation().unresolved(), 1);
}
#[test]
fn preparation_and_applied_records_are_non_replacing_and_hash_checked() {
    let temp = tempfile::tempdir().unwrap();
    let (service, intent, plan, _) = fixture(temp.path());
    let q = request(
        "1",
        "write_file",
        json!({"path":"result.txt","content":"replacement","expectedSha256":"absent"}),
    );
    let digest = digest(&dispatch_event(&q)).unwrap();
    let write = PreparedWrite::prepare(
        &service.root.writes,
        &service.root.intent_digest,
        &digest,
        "1",
        "result.txt",
        None,
        b"replacement",
    )
    .unwrap();
    assert!(PreparedWrite::prepare(
        &service.root.writes,
        &service.root.intent_digest,
        &digest,
        "1",
        "result.txt",
        None,
        b"different"
    )
    .is_err());
    write.applied().unwrap();
    assert!(write.applied().is_err());
    assert!(!Path::new(&plan.project.root).join("result.txt").exists());
    drop(write);
    drop(service);
    // Images alone cannot manufacture the missing original dispatch.
    assert!(inspect(&temp.path().join("tools"), &intent, "1").is_err());
}
struct Owned(std::process::Child);
impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
#[ignore = "owned file-write crash helper; isolated directory and boundary supplied by parent"]
fn crash_writer() {
    let root = PathBuf::from(std::env::var_os("CERULEAN_FILE_CRASH_ROOT").unwrap());
    let point = std::env::var("CERULEAN_FILE_CRASH_POINT").unwrap();
    let existing = std::env::var("CERULEAN_FILE_CRASH_EXISTS").unwrap() == "yes";
    let (mut service, intent, plan, _) = fixture(&root);
    let path = Path::new(&plan.project.root).join("result.txt");
    if existing {
        fs::write(path, b"original data").unwrap();
    }
    fs::write(
        root.join("intent.json"),
        serde_json::to_vec(&intent).unwrap(),
    )
    .unwrap();
    let args = json!({"path":"result.txt","content":"replacement!","expectedSha256":if existing{hash(b"original data")}else{"absent".into()}});
    let q = request("1", "write_file", args.clone());
    let event = dispatch_event(&q);
    service.record(&intent, &event).unwrap();
    let _result = write_file_observed(
        &service.root,
        "1",
        &digest(&event).unwrap(),
        Ok(serde_json::from_value(args).unwrap()),
        |at| {
            if at == point {
                let mut ready = OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(root.join("ready"))
                    .unwrap();
                ready.write_all(at.as_bytes()).unwrap();
                ready.sync_all().unwrap();
                loop {
                    std::thread::park_timeout(std::time::Duration::from_secs(1));
                }
            }
            Ok(())
        },
    );
}
#[test]
fn actual_writer_termination_preserves_original_and_target_at_four_boundaries() {
    use std::{
        os::windows::process::CommandExt,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    for existing in [false, true] {
        for point in ["prepared", "partial", "synced", "recorded"] {
            let root = tempfile::tempdir().unwrap();
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--ignored",
                    "--exact",
                    "omp_file_tools::write_evidence::tests::crash_writer",
                    "--nocapture",
                ])
                .env("CERULEAN_FILE_CRASH_ROOT", root.path())
                .env("CERULEAN_FILE_CRASH_POINT", point)
                .env(
                    "CERULEAN_FILE_CRASH_EXISTS",
                    if existing { "yes" } else { "no" },
                )
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(0x08000000);
            let mut owner = Owned(command.spawn().unwrap());
            let deadline = Instant::now() + Duration::from_secs(20);
            while !root.path().join("ready").exists() {
                assert!(
                    owner.0.try_wait().unwrap().is_none(),
                    "writer exited before {existing}/{point}"
                );
                assert!(Instant::now() < deadline, "writer marker timed out");
                std::thread::sleep(Duration::from_millis(10));
            }
            owner.0.kill().unwrap();
            owner.0.wait().unwrap();
            let intent: DispatchIntent =
                serde_json::from_slice(&fs::read(root.path().join("intent.json")).unwrap())
                    .unwrap();
            let inspection = inspect(&root.path().join("tools"), &intent, "1").unwrap();
            assert_eq!(
                inspection.state,
                match point {
                    "prepared" => FileState::Original,
                    "partial" => FileState::Other,
                    _ => FileState::Intended,
                }
            );
            assert_eq!(inspection.applied_recorded, point == "recorded");
            assert!(!inspection.tool_result_recorded);
            let images = root.path().join("tools/writes").join(hash(b"1"));
            if existing {
                assert_eq!(
                    fs::read(images.join("before.bin")).unwrap(),
                    b"original data"
                );
            }
            assert_eq!(fs::read(images.join("after.bin")).unwrap(), b"replacement!");
            let current = Path::new(&intent.binding.workspace).join("result.txt");
            let before = fs::read(&current).ok();
            inspect(&root.path().join("tools"), &intent, "1").unwrap();
            assert_eq!(fs::read(&current).ok(), before);
        }
    }
}

#[tokio::test]
async fn rehashed_path_payload_and_foreign_tool_cannot_rebind_write_images() {
    for mode in ["path", "payload", "incomplete"] {
        let temp = tempfile::tempdir().unwrap();
        let (mut service, intent, _, _) = fixture(temp.path());
        let q = request(
            "1",
            "write_file",
            json!({"path":"result.txt","content":"replacement","expectedSha256":"absent"}),
        );
        let output = run(&mut service, &intent, q, false).await;
        assert!(!output.is_error);
        drop(service);
        let images = temp.path().join("tools/writes").join(hash(b"1"));
        let mut record: Record =
            serde_json::from_slice(&fs::read(images.join("prepared.json")).unwrap()).unwrap();
        match mode {
            "path" => record.body.path = "other.txt".into(),
            "payload" => {
                fs::write(images.join("after.bin"), b"changed").unwrap();
                record.body.after_sha256 = hash(b"changed");
                record.body.after_bytes = 7;
            }
            _ => {
                fs::write(images.join("prepared.json"), b"{partial").unwrap();
                assert!(inspect(&temp.path().join("tools"), &intent, "1").is_err());
                continue;
            }
        }
        record.hash = digest(&record.body).unwrap();
        fs::write(
            images.join("prepared.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        fs::write(
            images.join("applied.json"),
            serde_json::to_vec(&Applied {
                version: 1,
                write_digest: record.hash,
            })
            .unwrap(),
        )
        .unwrap();
        assert!(
            inspect(&temp.path().join("tools"), &intent, "1").is_err(),
            "{mode}"
        );
    }
    let temp = tempfile::tempdir().unwrap();
    let (mut service, intent, _, _) = fixture(temp.path());
    let q = request("read", "read_file", json!({"path":"result.txt","offset":0}));
    let event = dispatch_event(&q);
    service.record(&intent, &event).unwrap();
    let prepared = PreparedWrite::prepare(
        &service.root.writes,
        &service.root.intent_digest,
        &digest(&event).unwrap(),
        "read",
        "result.txt",
        None,
        b"replacement",
    )
    .unwrap();
    drop(prepared);
    drop(service);
    assert!(inspect(&temp.path().join("tools"), &intent, "read")
        .unwrap_err()
        .contains("another tool"));
}
