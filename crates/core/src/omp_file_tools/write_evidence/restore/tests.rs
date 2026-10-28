use super::*;
use crate::omp_file_tools::tests::{dispatch_event, request};
use crate::{
    omp_execution::tests::assigned,
    scheduler::{CommandContext, SchedulerConfig},
};
use cerulean_adapters::omp_transport::worker::WorkerEvent;
const ORIGINAL: &[u8] = b"ORIGINAL-0123456789";
async fn fixture(root: &Path, existing: bool) -> (DurableWorkflow, DispatchIntent) {
    let (mut store, plan) = assigned(root, false);
    let (_, _, intent, _) = crate::omp_execution::scope(&store, "task", "attempt").unwrap();
    store
        .command(
            "fixture-dispatch",
            "core",
            WorkflowCommand::DispatchIntent {
                task: "task".into(),
                attempt: "attempt".into(),
            },
            CommandContext {
                thread_id: Some("session".into()),
                cwd: Some(plan.project.root.clone()),
                ..Default::default()
            },
            false,
        )
        .unwrap();
    let mut prompt =
        OmpJournal::open(&store.omp_attempt_path("attempt").unwrap(), &intent).unwrap();
    prompt
        .record(&WorkerEvent::DispatchIntent {
            intent: intent.clone(),
        })
        .unwrap();
    prompt
        .record(&WorkerEvent::PromptAcknowledged {
            request_id: "prompt-request".into(),
        })
        .unwrap();
    let mut service = OmpFileTools::create(
        &plan,
        store.approval().unwrap(),
        "user",
        1,
        &intent,
        &root.join("tools"),
    )
    .unwrap();
    if existing {
        fs::write(Path::new(&plan.project.root).join("result.txt"), ORIGINAL).unwrap();
    }
    let q = request(
        "1",
        "write_file",
        json!({"path":"result.txt","content":"replacement!","expectedSha256":if existing{hash(ORIGINAL)}else{"absent".into()}}),
    );
    service.record(&intent, &dispatch_event(&q)).unwrap();
    let (_tx, rx) = watch::channel(false);
    let result = service.start(q, rx).await.unwrap();
    assert!(!result.is_error);
    // The process disappeared after writing and before tool Result; preserve that gap.
    drop(service);
    drop(prompt);
    store
        .command(
            "fixture-unknown",
            "core",
            WorkflowCommand::RecoveryUnknown {
                task: "task".into(),
                attempt: "attempt".into(),
            },
            CommandContext::default(),
            false,
        )
        .unwrap();
    (store, intent)
}
#[tokio::test]
async fn restore_original_or_original_absence_keeps_workflow_unknown_and_cannot_repeat() {
    for existing in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let (store, intent) = fixture(temp.path(), existing).await;
        let root = temp.path().join("tools");
        let prior = store.history().len();
        let reviewed = review(&store, &root, "task", "attempt", "1").unwrap();
        let receipt =
            restore_original(&store, &root, "task", "attempt", "user", &reviewed).unwrap();
        assert_eq!(receipt.restored_sha256, existing.then(|| hash(ORIGINAL)));
        let path = Path::new(&intent.binding.workspace).join("result.txt");
        if existing {
            assert_eq!(fs::read(&path).unwrap(), ORIGINAL);
        } else {
            assert!(!path.exists());
        }
        assert_eq!(store.history().len(), prior);
        assert_eq!(store.state().tasks["task"].state, TaskState::Unknown);
        let checked = inspect_restore(&store, &root, "task", "attempt", "1").unwrap();
        assert!(checked.result_recorded && checked.current_matches_original);
        let images = root.join("repairs").join(hash(b"1"));
        assert_eq!(
            fs::read(images.join("current.bin")).unwrap(),
            b"replacement!"
        );
        let new_review = review(&store, &root, "task", "attempt", "1").unwrap();
        assert!(
            restore_original(&store, &root, "task", "attempt", "user", &new_review)
                .unwrap_err()
                .contains("do not retry")
        );
        fs::write(&path, "later user edit").unwrap();
        assert!(
            !inspect_restore(&store, &root, "task", "attempt", "1")
                .unwrap()
                .current_matches_original
        );
        assert_eq!(fs::read_to_string(path).unwrap(), "later user edit");
    }
}
#[tokio::test]
async fn stale_review_wrong_user_or_missing_original_evidence_never_mutates_file() {
    let temp = tempfile::tempdir().unwrap();
    let (store, intent) = fixture(temp.path(), true).await;
    let root = temp.path().join("tools");
    let reviewed = review(&store, &root, "task", "attempt", "1").unwrap();
    let path = Path::new(&intent.binding.workspace).join("result.txt");
    assert!(restore_original(&store, &root, "task", "attempt", "worker", &reviewed).is_err());
    assert!(!root.join("repairs").exists());
    fs::write(&path, "user version").unwrap();
    assert!(restore_original(&store, &root, "task", "attempt", "user", &reviewed).is_err());
    assert!(!root.join("repairs").exists());
    assert_eq!(fs::read_to_string(&path).unwrap(), "user version");
    let mut fresh = review(&store, &root, "task", "attempt", "1").unwrap();
    fresh.workflow_head = "a".repeat(64);
    assert!(restore_original(&store, &root, "task", "attempt", "user", &fresh).is_err());
    let fresh = review(&store, &root, "task", "attempt", "1").unwrap();
    fs::write(
        root.join("writes").join(hash(b"1")).join("before.bin"),
        b"corrupt",
    )
    .unwrap();
    assert!(restore_original(&store, &root, "task", "attempt", "user", &fresh).is_err());
    assert_eq!(fs::read_to_string(path).unwrap(), "user version");
}
#[tokio::test]
async fn interrupted_restore_retains_reviewed_bytes_and_requires_observation_not_replay() {
    let temp = tempfile::tempdir().unwrap();
    let (store, intent) = fixture(temp.path(), true).await;
    let root = temp.path().join("tools");
    let reviewed = review(&store, &root, "task", "attempt", "1").unwrap();
    assert!(
        restore_observed(&store, &root, "task", "attempt", "user", &reviewed, |at| {
            if at == "partial" {
                Err("fixture interruption".into())
            } else {
                Ok(())
            }
        })
        .is_err()
    );
    let checked = inspect_restore(&store, &root, "task", "attempt", "1").unwrap();
    assert!(!checked.result_recorded);
    assert!(!checked.current_matches_original);
    let path = Path::new(&intent.binding.workspace).join("result.txt");
    let partial = fs::read(&path).unwrap();
    let fresh = review(&store, &root, "task", "attempt", "1").unwrap();
    assert!(restore_original(&store, &root, "task", "attempt", "user", &fresh).is_err());
    assert_eq!(fs::read(path).unwrap(), partial);
    let repair = root.join("repairs").join(hash(b"1"));
    assert_eq!(
        fs::read(repair.join("current.bin")).unwrap(),
        b"replacement!"
    );
    assert_eq!(fs::read(repair.join("original.bin")).unwrap(), ORIGINAL);
}
#[tokio::test]
async fn missing_current_file_can_be_restored_and_forged_result_is_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let (store, intent) = fixture(temp.path(), true).await;
    let root = temp.path().join("tools");
    let path = Path::new(&intent.binding.workspace).join("result.txt");
    fs::remove_file(&path).unwrap();
    let reviewed = review(&store, &root, "task", "attempt", "1").unwrap();
    assert_eq!(reviewed.write.state, FileState::Missing);
    restore_original(&store, &root, "task", "attempt", "user", &reviewed).unwrap();
    assert_eq!(fs::read(path).unwrap(), ORIGINAL);
    let result_path = root.join("repairs").join(hash(b"1")).join("result.json");
    let mut result: RestoreReceipt =
        serde_json::from_slice(&fs::read(&result_path).unwrap()).unwrap();
    result.review_digest = "a".repeat(64);
    fs::write(result_path, serde_json::to_vec(&result).unwrap()).unwrap();
    assert!(inspect_restore(&store, &root, "task", "attempt", "1").is_err());
}
struct Owned(std::process::Child);
impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
#[ignore = "owned restore crash helper; isolated directory and boundary supplied by parent"]
fn crash_restore() {
    let root = PathBuf::from(std::env::var_os("CERULEAN_RESTORE_ROOT").unwrap());
    let point = std::env::var("CERULEAN_RESTORE_POINT").unwrap();
    let existing = std::env::var("CERULEAN_RESTORE_EXISTS").unwrap() == "yes";
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (store, _) = runtime.block_on(fixture(&root, existing));
    fs::write(
        root.join("config.json"),
        serde_json::to_vec(store.configuration()).unwrap(),
    )
    .unwrap();
    let service = root.join("tools");
    let reviewed = review(&store, &service, "task", "attempt", "1").unwrap();
    restore_observed(
        &store,
        &service,
        "task",
        "attempt",
        "user",
        &reviewed,
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
    )
    .unwrap();
}
#[test]
fn actual_restore_owner_termination_preserves_history_and_both_file_versions() {
    use std::{
        os::windows::process::CommandExt,
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    for existing in [false, true] {
        for point in ["prepared", "partial", "mutated", "recorded"] {
            let root = tempfile::tempdir().unwrap();
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--ignored",
                    "--exact",
                    "omp_file_tools::write_evidence::restore::tests::crash_restore",
                    "--nocapture",
                ])
                .env("CERULEAN_RESTORE_ROOT", root.path())
                .env("CERULEAN_RESTORE_POINT", point)
                .env(
                    "CERULEAN_RESTORE_EXISTS",
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
                    "restore exited early {existing}/{point}"
                );
                assert!(
                    Instant::now() < deadline,
                    "restore timed out {existing}/{point}"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            owner.0.kill().unwrap();
            owner.0.wait().unwrap();
            let config: SchedulerConfig =
                serde_json::from_slice(&fs::read(root.path().join("config.json")).unwrap())
                    .unwrap();
            let store = DurableWorkflow::open(&root.path().join("ledger"), config).unwrap();
            let service = root.path().join("tools");
            let checked = inspect_restore(&store, &service, "task", "attempt", "1").unwrap();
            assert_eq!(checked.result_recorded, point == "recorded");
            assert_eq!(
                checked.current_matches_original,
                matches!(point, "mutated" | "recorded") || (!existing && point == "partial")
            );
            assert_eq!(store.state().tasks["task"].state, TaskState::Unknown);
            let repair = service.join("repairs").join(hash(b"1"));
            assert_eq!(
                fs::read(repair.join("current.bin")).unwrap(),
                b"replacement!"
            );
            if existing {
                assert_eq!(fs::read(repair.join("original.bin")).unwrap(), ORIGINAL);
            }
            let reviewed = review(&store, &service, "task", "attempt", "1").unwrap();
            assert!(
                restore_original(&store, &service, "task", "attempt", "user", &reviewed).is_err()
            );
        }
    }
}

#[tokio::test]
async fn running_scope_and_completed_prompt_refuse_but_error_or_abort_can_be_reviewed() {
    use cerulean_adapters::omp_protocol::{PromptReceipt, PromptStatus};
    let temp = tempfile::tempdir().unwrap();
    let (store, _) = assigned(temp.path(), false);
    assert!(review(&store, &temp.path().join("tools"), "task", "attempt", "1").is_err());
    for status in [
        PromptStatus::Completed,
        PromptStatus::Error,
        PromptStatus::Aborted,
    ] {
        let temp = tempfile::tempdir().unwrap();
        let (store, intent) = fixture(temp.path(), true).await;
        let mut journal =
            OmpJournal::open(&store.omp_attempt_path("attempt").unwrap(), &intent).unwrap();
        let (identity, request_id) = intent.validate_metadata().unwrap();
        journal
            .record(&WorkerEvent::Terminal {
                receipt: PromptReceipt {
                    identity,
                    request_id,
                    correlation_origin: "controller-rpc-request-id".into(),
                    status,
                },
                abort_requested: false,
                abort_acknowledged: false,
            })
            .unwrap();
        drop(journal);
        let r = review(&store, &temp.path().join("tools"), "task", "attempt", "1");
        if status == PromptStatus::Completed {
            assert!(r.is_err());
        } else {
            let reviewed = r.unwrap();
            restore_original(
                &store,
                &temp.path().join("tools"),
                "task",
                "attempt",
                "user",
                &reviewed,
            )
            .unwrap();
        }
    }
}
