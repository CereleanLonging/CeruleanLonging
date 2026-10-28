use super::*;
use crate::{reader_generation::tests::snapshot, Store};

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    archive: PathBuf,
    to: PathBuf,
    manifest: String,
}
impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("data");
        drop(Store::open(&root).unwrap());
        let initial = temp.path().join("initial");
        let manifest = backup::create(&root, &initial).unwrap().manifest_digest;
        let r = crate::experiment_guard::review(&root, &initial, &manifest).unwrap();
        crate::experiment_guard::promote(&root, &initial, &manifest, &r.digest).unwrap();
        fs::write(
            root.join("team-material-import.json.pending"),
            b"original partial bytes",
        )
        .unwrap();
        let archive = temp.path().join("backup");
        let manifest = backup::create(&root, &archive).unwrap().manifest_digest;
        let to = temp.path().join("retained");
        Self {
            _temp: temp,
            root,
            archive,
            to,
            manifest,
        }
    }
    fn review(&self) -> Review {
        review(
            &self.root,
            &self.archive,
            &self.manifest,
            &self.to,
            "Preserve original failure",
        )
        .unwrap()
    }
    fn retain(&self, expected: &str) -> Result<Receipt, String> {
        retain(
            &self.root,
            &self.archive,
            &self.manifest,
            &self.to,
            "Preserve original failure",
            expected,
        )
    }
    fn inspect(&self, expected: &str) -> Result<Receipt, String> {
        inspect(
            &self.root,
            &self.archive,
            &self.manifest,
            &self.to,
            "Preserve original failure",
            expected,
        )
    }
}
#[test]
fn retention_keeps_original_data_and_backup_and_never_unlocks_or_replays() {
    let f = Fixture::new();
    let before = snapshot(&f.root);
    let saved = snapshot(&f.archive);
    let r = f.review();
    assert!(!f.to.exists());
    assert!(f.retain(&"0".repeat(64)).is_err());
    assert!(!f.to.exists());
    let recorded = f.retain(&r.digest).unwrap();
    assert_eq!(recorded.review.digest, r.digest);
    let receipt = snapshot(&f.to);
    assert!(f.retain(&r.digest).is_err());
    for _ in 0..2 {
        assert_eq!(f.inspect(&r.digest).unwrap().review.digest, r.digest);
    }
    let mut host = crate::diagnostic_service::ServiceHost::open(&f.root).unwrap();
    assert_eq!(host.snapshot().team_import.unwrap().state, "interrupted");
    let response = host.handle_command(cerulean_domain::Request {
        id: "blocked".into(),
        protocol_version: cerulean_domain::PROTOCOL_VERSION,
        command: cerulean_domain::Command::CreatePlan {
            title: "no execution unlock".into(),
            tasks: vec!["blocked".into()],
        },
    });
    assert!(response.error.is_some());
    drop(host);
    assert_eq!(before, snapshot(&f.root));
    assert_eq!(saved, snapshot(&f.archive));
    assert_eq!(receipt, snapshot(&f.to));
}
#[test]
fn changed_source_backup_reason_or_destination_invalidates_original_review() {
    for mode in ["source", "backup", "reason", "destination"] {
        let f = Fixture::new();
        let r = f.review();
        let result = match mode {
            "source" => {
                fs::write(f.root.join("changed.json"), b"new state").unwrap();
                f.retain(&r.digest)
            }
            "backup" => {
                fs::write(
                    f.archive.join("payload/team-material-import.json.pending"),
                    b"changed",
                )
                .unwrap();
                f.retain(&r.digest)
            }
            "reason" => retain(
                &f.root,
                &f.archive,
                &f.manifest,
                &f.to,
                "Different decision",
                &r.digest,
            ),
            _ => retain(
                &f.root,
                &f.archive,
                &f.manifest,
                &f.to.with_file_name("different"),
                "Preserve original failure",
                &r.digest,
            ),
        };
        assert!(result.is_err(), "{mode}");
        assert!(!f.to.exists());
        assert!(!f.to.with_file_name("different").exists());
    }
}
#[test]
fn live_core_or_hold_cannot_be_bypassed_for_retention() {
    let f = Fixture::new();
    let r = f.review();
    let host = crate::diagnostic_service::ServiceHost::open(&f.root).unwrap();
    assert!(review(
        &f.root,
        &f.archive,
        &f.manifest,
        &f.to,
        "Preserve original failure"
    )
    .is_err());
    assert!(f.retain(&r.digest).is_err());
    assert!(!f.to.exists());
    drop(host);
    fs::write(
        f.root.join("upgrade-hold.json"),
        b"unresolved original hold",
    )
    .unwrap();
    assert!(f.retain(&r.digest).is_err());
    assert!(!f.to.exists());
}
#[test]
fn partial_receipt_is_preserved_and_never_repaired_or_reported_complete() {
    for mode in ["empty", "pending", "malformed", "extra"] {
        let f = Fixture::new();
        let r = f.review();
        if mode == "extra" {
            f.retain(&r.digest).unwrap();
            fs::write(f.to.join("extra"), b"uncertain").unwrap();
        } else {
            fs::create_dir(&f.to).unwrap();
            if mode != "empty" {
                fs::write(
                    f.to.join(if mode == "pending" {
                        "retention.json.pending"
                    } else {
                        "retention.json"
                    }),
                    b"{partial",
                )
                .unwrap();
            }
        }
        let before = snapshot(&f.to);
        let source = snapshot(&f.root);
        assert!(f.inspect(&r.digest).is_err(), "{mode}");
        assert!(f.retain(&r.digest).is_err());
        assert_eq!(before, snapshot(&f.to));
        assert_eq!(source, snapshot(&f.root));
    }
}
#[test]
fn retention_refuses_destinations_inside_original_evidence_and_empty_import() {
    let f = Fixture::new();
    for to in [
        f.root.join("new-record"),
        f.archive.join("new-record"),
        PathBuf::from("relative"),
    ] {
        assert!(review(&f.root, &f.archive, &f.manifest, &to, "reason").is_err());
        assert!(!to.exists());
    }
    fs::remove_file(f.root.join("team-material-import.json.pending")).unwrap();
    let archive = f._temp.path().join("no-import-backup");
    let manifest = backup::create(&f.root, &archive).unwrap().manifest_digest;
    assert!(review(&f.root, &archive, &manifest, &f.to, "reason").is_err());
    assert!(!f.to.exists());
}
