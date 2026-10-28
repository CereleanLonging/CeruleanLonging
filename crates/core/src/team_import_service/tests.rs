use super::*;
use crate::{diagnostic_service::ServiceHost, Store};
use cerulean_domain::{team_timing::TeamTimingAction, Command, Request, PROTOCOL_VERSION};
use serde_json::json;
use std::{
    fs,
    time::{Duration, Instant},
};

fn protected(temp: &Path) -> PathBuf {
    let root = temp.join("data");
    drop(Store::open(&root).unwrap());
    let archive = temp.join("backup");
    let manifest = crate::backup::create(&root, &archive)
        .unwrap()
        .manifest_digest;
    let review = crate::experiment_guard::review(&root, &archive, &manifest).unwrap();
    crate::experiment_guard::promote(&root, &archive, &manifest, &review.digest).unwrap();
    fs::canonicalize(root).unwrap()
}
fn request(id: &str, command: Command) -> Request {
    Request {
        id: id.into(),
        protocol_version: PROTOCOL_VERSION,
        command,
    }
}
fn operation(root: &Path) -> Operation {
    Operation {
        schema: 1,
        root: root.to_string_lossy().into(),
        request_id: "original".into(),
        source_path: root.join("source.json").to_string_lossy().into(),
        source_sha256: "a".repeat(64),
    }
}
#[test]
fn cold_partial_import_fences_mutation_and_upgrade_without_replay() {
    for mode in [
        "intent",
        "pending-intent",
        "pending-outcome",
        "bad-outcome",
        "forged-completed",
        "orphan-outcome",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let root = protected(temp.path());
        let op = operation(&root);
        if mode == "pending-intent" {
            fs::write(root.join(format!("{OPERATION}.pending")), b"{partial").unwrap();
        } else if mode != "orphan-outcome" {
            publish(&root, OPERATION, &op).unwrap();
        }
        match mode {
            "pending-outcome" => {
                fs::write(root.join(format!("{OUTCOME}.pending")), b"{partial").unwrap()
            }
            "bad-outcome" | "orphan-outcome" => fs::write(root.join(OUTCOME), b"{bad").unwrap(),
            "forged-completed" => publish(
                &root,
                OUTCOME,
                &Outcome {
                    operation_digest: digest(&op).unwrap(),
                    completed: true,
                    binding: None,
                    message: None,
                },
            )
            .unwrap(),
            _ => (),
        }
        let before = crate::reader_generation::tests::snapshot(&root);
        for _ in 0..2 {
            let mut host = ServiceHost::open(&root).unwrap();
            assert_eq!(
                host.snapshot().team_import.unwrap().state,
                "interrupted",
                "{mode}"
            );
            assert!(host
                .handle_command(request(
                    "write",
                    Command::CreatePlan {
                        title: "must not write".into(),
                        tasks: vec!["one".into()]
                    }
                ))
                .error
                .is_some());
            assert!(host
                .handle_command(request(
                    "retry",
                    Command::TeamTiming {
                        action: TeamTimingAction::Import {
                            path: "missing".into(),
                            sha256: "a".repeat(64)
                        }
                    }
                ))
                .error
                .is_some());
            assert!(host.can_shutdown());
            assert!(!crate::upgrade::review(&mut host).unwrap().0.ready);
        }
        assert_eq!(
            before,
            crate::reader_generation::tests::snapshot(&root),
            "{mode}"
        );
    }
}
#[test]
fn accepted_background_failure_retains_original_intent_and_reopens_without_retry() {
    let temp = tempfile::tempdir().unwrap();
    let root = protected(temp.path());
    let missing = temp.path().join("missing");
    let file = temp.path().join("request.json");
    let material = json!({"material":{"bundle":missing,"manifestSha256":"a".repeat(64),"schedule":missing,"launcher":missing,"checker":missing,
        "node":{"path":missing,"sha256":"a".repeat(64)},"projectRoot":missing,"dataRoot":root,
        "worktree":{"schemaVersion":1,"baseCommit":"a".repeat(40),"poolRoot":missing,"git":{"path":missing,"sha256":"a".repeat(64)},"input":"committed-raw-blobs"}},
        "workloads":missing,"branchSpecs":{},"chiefSessionId":null});
    let bytes = serde_json::to_vec(&material).unwrap();
    fs::write(&file, &bytes).unwrap();
    use sha2::Digest;
    let command = Command::TeamTiming {
        action: TeamTimingAction::Import {
            path: file.to_string_lossy().into(),
            sha256: format!("{:x}", sha2::Sha256::digest(&bytes)),
        },
    };
    let mut host = ServiceHost::open(&root).unwrap();
    let accepted = host.handle_command(request("original", command.clone()));
    assert!(accepted.error.is_none(), "{:?}", accepted.error);
    assert!(host
        .handle_command(request("duplicate", command.clone()))
        .error
        .is_some());
    let until = Instant::now() + Duration::from_secs(10);
    while matches!(host, ServiceHost::Importing(_)) {
        assert!(Instant::now() < until);
        host.poll_import();
        std::thread::sleep(Duration::from_millis(10));
    }
    let view = host.snapshot().team_import.unwrap();
    assert_eq!(view.state, "failed");
    assert_eq!(view.request_id, "original");
    assert!(host.can_shutdown());
    drop(host);
    let before = crate::reader_generation::tests::snapshot(&root);
    let mut reopened = ServiceHost::open(&root).unwrap();
    assert_eq!(reopened.snapshot().team_import.unwrap().state, "failed");
    assert!(reopened
        .handle_command(request("again", command))
        .error
        .is_some());
    assert!(reopened.snapshot().workflows.is_empty());
    assert_eq!(before, crate::reader_generation::tests::snapshot(&root));
}
#[test]
fn reader14_remains_readable_and_requires_reviewed_promotion_for_new_imports() {
    let temp = tempfile::tempdir().unwrap();
    let root = protected(temp.path());
    let marker = root.join(crate::experiment_guard::MARKER);
    let mut v: serde_json::Value = serde_json::from_slice(&fs::read(&marker).unwrap()).unwrap();
    v["minimumReaderGeneration"] = json!(14);
    fs::remove_file(&marker).unwrap();
    let legacy = root
        .join("events")
        .join(crate::experiment_guard::LEGACY_FILE_NAME);
    fs::write(&legacy, serde_json::to_vec(&v).unwrap()).unwrap();
    let original = fs::read(&legacy).unwrap();
    let mut host = ServiceHost::open(&root).unwrap();
    assert!(host.snapshot().team_import.is_none());
    let rejected = host.handle_command(request(
        "old",
        Command::TeamTiming {
            action: TeamTimingAction::Import {
                path: "missing".into(),
                sha256: "a".repeat(64),
            },
        },
    ));
    assert!(rejected.error.unwrap().message.contains("reader15"));
    drop(host);
    assert!(!root.join(OPERATION).exists());
    let backup = temp.path().join("upgrade-backup");
    let manifest = crate::backup::create(&root, &backup)
        .unwrap()
        .manifest_digest;
    let review = crate::experiment_guard::review(&root, &backup, &manifest).unwrap();
    crate::experiment_guard::promote(&root, &backup, &manifest, &review.digest).unwrap();
    assert_eq!(fs::read(legacy).unwrap(), original);
    assert_eq!(
        crate::data_format::status(&root)
            .unwrap()
            .minimum_reader_generation,
        15
    );
    drop(ServiceHost::open(&root).unwrap());
}

#[test]
fn original_legacy_partial_import_is_read_only_without_new_operation_records() {
    for pending in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let root = protected(temp.path());
        let path = root.join(if pending {
            "team-material-import.json.pending"
        } else {
            "team-material-import.json"
        });
        fs::write(&path,serde_json::to_vec(&json!({"requestPath":root.join("original.json"),"sha256":"a".repeat(64),"bindingDigest":"b".repeat(64)})).unwrap()).unwrap();
        let before = crate::reader_generation::tests::snapshot(&root);
        let mut host = ServiceHost::open(&root).unwrap();
        let status = host.snapshot().team_import.unwrap();
        assert_eq!(status.request_id, "legacy-import");
        assert_eq!(status.state, "interrupted");
        assert!(host
            .handle_command(request(
                "write",
                Command::CreatePlan {
                    title: "blocked".into(),
                    tasks: vec!["blocked".into()]
                }
            ))
            .error
            .is_some());
        assert!(!crate::upgrade::review(&mut host).unwrap().0.ready);
        drop(host);
        assert_eq!(before, crate::reader_generation::tests::snapshot(&root));
        assert!(!root.join(OPERATION).exists());
    }
}
