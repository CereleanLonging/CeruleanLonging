use super::*;
#[path = "tool_supervision_tests.rs"]
mod supervision;
use cerulean_domain::execution::{ControllerRequestIdentity, IdentityStrength, SessionBinding};
const PROMPT: &str = "tool fixture";
fn intent() -> DispatchIntent {
    use sha2::{Digest, Sha256};
    DispatchIntent {
        version: 1,
        task_id: "task".into(),
        attempt_id: "attempt".into(),
        revision_digest: "a".repeat(64),
        fence: 1,
        binding: SessionBinding {
            adapter_id: "omp".into(),
            adapter_version: "18.4.4".into(),
            session_id: "session".into(),
            workspace: std::env::current_dir().unwrap().to_string_lossy().into(),
            identity_strength: IdentityStrength::ControllerRequest(ControllerRequestIdentity {
                connection_id: "connection".into(),
                request_id: "prompt-request".into(),
            }),
        },
        provider: "deepseek".into(),
        model: "fixture".into(),
        prompt_sha256: format!("{:x}", Sha256::digest(PROMPT.as_bytes())),
    }
}

fn dispatch() -> HostEvent {
    HostEvent::Dispatch {
        id: "1".into(),
        tool_call_id: "model-1".into(),
        tool_name: "cerulean_fixture".into(),
        arguments_sha256: "b".repeat(64),
    }
}
fn result() -> HostEvent {
    HostEvent::Result {
        id: "1".into(),
        result_sha256: "c".repeat(64),
        is_error: false,
    }
}
#[test]
fn bound_chain_reopens_only_for_inspection_and_never_replays_tools() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("tools");
    let mut journal = OmpToolJournal::create(&root, &intent()).unwrap();
    assert!(OmpToolJournal::open(&root, &intent()).is_err());
    assert!(journal.record(&intent(), &result()).is_err());
    journal.record(&intent(), &dispatch()).unwrap();
    assert_eq!(journal.observation().unresolved(), 1);
    assert!(journal.record(&intent(), &dispatch()).is_err());
    journal
        .record(&intent(), &HostEvent::Cancel { id: "1".into() })
        .unwrap();
    journal.record(&intent(), &result()).unwrap();
    assert_eq!(journal.observation().unresolved(), 0);
    let head = journal.head().to_owned();
    drop(journal);
    let mut reopened = OmpToolJournal::open(&root, &intent()).unwrap();
    assert_eq!(head, reopened.head());
    assert_eq!(reopened.observation().unresolved(), 0);
    assert!(reopened.record(&intent(), &dispatch()).is_err());
    drop(reopened);
    assert!(OmpToolJournal::create(&root, &intent()).is_err());
    let mut foreign = intent();
    foreign.fence += 1;
    assert!(OmpToolJournal::open(&root, &foreign).is_err());
    fs::write(root.join("001.json"), b"{}").unwrap();
    assert!(OmpToolJournal::open(&root, &intent()).is_err());
}
#[test]
fn interruption_retains_evidence_and_never_turns_cancellation_into_completion() {
    for stage in ["synced", "published", "committed"] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("tools");
        let mut journal = OmpToolJournal::create(&root, &intent()).unwrap();
        assert!(journal
            .publish(&dispatch(), |at| if at == stage {
                Err("interrupted".into())
            } else {
                Ok(())
            })
            .is_err());
        assert!(!journal.healthy());
        assert!(journal.record(&intent(), &dispatch()).is_err());
        drop(journal);
        let reopened = OmpToolJournal::open(&root, &intent());
        if stage == "committed" {
            let mut j = reopened.unwrap();
            assert_eq!(j.observation().unresolved(), 1);
            assert!(j.record(&intent(), &result()).is_err());
        } else {
            assert!(reopened.is_err());
            assert!(root.join("000.pending").exists());
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("tools");
    let mut j = OmpToolJournal::create(&root, &intent()).unwrap();
    j.record(&intent(), &dispatch()).unwrap();
    j.record(&intent(), &HostEvent::Cancel { id: "1".into() })
        .unwrap();
    drop(j);
    assert_eq!(
        OmpToolJournal::open(&root, &intent())
            .unwrap()
            .observation()
            .unresolved(),
        1
    );
}
#[test]
fn foreign_scope_and_missing_or_oversized_records_are_rejected() {
    for mode in ["scope", "gap", "oversize", "extra"] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("tools");
        let mut j = OmpToolJournal::create(&root, &intent()).unwrap();
        let mut foreign = intent();
        foreign.fence += 1;
        assert!(j.record(&foreign, &dispatch()).is_err());
        assert_eq!(j.observation().unresolved(), 0);
        j.record(&intent(), &dispatch()).unwrap();
        j.record(&intent(), &result()).unwrap();
        drop(j);
        match mode {
            "scope" => {
                let mut record: Record =
                    serde_json::from_slice(&fs::read(root.join("000.json")).unwrap()).unwrap();
                record.body.intent_digest = digest(&foreign).unwrap();
                record.hash = digest(&record.body).unwrap();
                fs::write(root.join("000.json"), serde_json::to_vec(&record).unwrap()).unwrap();
            }
            "gap" => {
                fs::remove_file(root.join("000.json")).unwrap();
            }
            "oversize" => {
                fs::write(root.join("000.json"), vec![b' '; MAX_BYTES + 1]).unwrap();
            }
            _ => {
                fs::write(root.join("extra"), b"unexpected").unwrap();
            }
        }
        assert!(OmpToolJournal::open(&root, &intent()).is_err(), "{mode}");
    }
}

struct Owned(std::process::Child);
impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
#[ignore = "owned tool journal crash helper; parent supplies an isolated directory"]
fn crash_owner() {
    let directory = PathBuf::from(std::env::var_os("CERULEAN_OMP_TOOL_CHILD_ROOT").unwrap());
    let point = std::env::var("CERULEAN_OMP_TOOL_CHILD_POINT").unwrap();
    let phase = std::env::var("CERULEAN_OMP_TOOL_CHILD_PHASE").unwrap();
    let mut journal = OmpToolJournal::create(&directory.join("journal"), &intent()).unwrap();
    let event = if phase == "result" {
        journal.record(&intent(), &dispatch()).unwrap();
        result()
    } else {
        dispatch()
    };
    journal
        .publish(&event, |at| {
            if at == point {
                let mut ready = OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(directory.join("ready"))
                    .unwrap();
                ready.write_all(at.as_bytes()).unwrap();
                ready.sync_all().unwrap();
                loop {
                    std::thread::park_timeout(std::time::Duration::from_secs(1));
                }
            }
            Ok(())
        })
        .unwrap();
}
#[test]
fn actual_owner_exit_preserves_dispatch_and_result_boundary_evidence() {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    for phase in ["dispatch", "result"] {
        for point in ["synced", "published", "committed"] {
            let root = tempfile::tempdir().unwrap();
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--ignored",
                    "--exact",
                    "omp_tool_journal::tests::crash_owner",
                    "--nocapture",
                ])
                .env("CERULEAN_OMP_TOOL_CHILD_ROOT", root.path())
                .env("CERULEAN_OMP_TOOL_CHILD_POINT", point)
                .env("CERULEAN_OMP_TOOL_CHILD_PHASE", phase)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }
            let mut owner = Owned(command.spawn().unwrap());
            let deadline = Instant::now() + Duration::from_secs(20);
            while !root.path().join("ready").exists() {
                assert!(
                    owner.0.try_wait().unwrap().is_none(),
                    "owner exited early: {phase}/{point}"
                );
                assert!(
                    Instant::now() < deadline,
                    "owner marker timed out: {phase}/{point}"
                );
                std::thread::sleep(Duration::from_millis(10));
            }
            let files = fs::read_dir(root.path().join("journal"))
                .unwrap()
                .map(|e| e.unwrap().path())
                .filter(|p| p.file_name().unwrap() != "writer.lock")
                .map(|p| {
                    let bytes = fs::read(&p).unwrap();
                    (p, bytes)
                })
                .collect::<Vec<_>>();
            owner.0.kill().unwrap();
            owner.0.wait().unwrap();
            let reopened = OmpToolJournal::open(&root.path().join("journal"), &intent());
            if point == "committed" {
                let mut j = reopened.unwrap();
                assert_eq!(
                    j.observation().unresolved(),
                    usize::from(phase == "dispatch")
                );
                assert!(j.record(&intent(), &dispatch()).is_err());
                assert!(j.record(&intent(), &result()).is_err());
            } else {
                assert!(reopened.is_err());
            }
            for (p, bytes) in files {
                assert_eq!(
                    fs::read(p).unwrap(),
                    bytes,
                    "evidence mutated: {phase}/{point}"
                );
            }
        }
    }
}
