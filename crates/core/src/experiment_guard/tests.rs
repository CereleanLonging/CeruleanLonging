use super::*;
use crate::{reader_generation::tests::snapshot, Store};
use cerulean_domain::{Command, Request, PROTOCOL_VERSION};
fn promote_cold(root: &Path, archive: &Path) {
    let manifest = crate::backup::create(root, archive)
        .unwrap()
        .manifest_digest;
    let r = review(root, archive, &manifest).unwrap();
    promote(root, archive, &manifest, &r.digest).unwrap();
}
#[test]
fn experiment_protection_preserves_existing_bytes_and_legacy_guard_chain() {
    for generation in [1, 3, 13] {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("data");
        drop(Store::open(&root).unwrap());
        if generation == 3 {
            crate::reader_generation::tests::promote_fixture(&root);
        }
        if generation == 13 {
            crate::management_source_guard::tests::promote_fixture(&root);
        }
        fs::create_dir(root.join("team-timelines")).unwrap();
        fs::write(
            root.join("team-timelines/original.pending"),
            b"preserve unresolved original bytes",
        )
        .unwrap();
        assert!(Store::open(&root).unwrap_err_string().contains("reader14"));
        let archive = temp.path().join("cold");
        let manifest = crate::backup::create(&root, &archive)
            .unwrap()
            .manifest_digest;
        let before = snapshot(&root);
        let saved = snapshot(&archive);
        let r = review(&root, &archive, &manifest).unwrap();
        assert!(promote(&root, &archive, &manifest, &"0".repeat(64)).is_err());
        assert_eq!(before, snapshot(&root));
        assert_eq!(
            promote(&root, &archive, &manifest, &r.digest)
                .unwrap()
                .minimum_reader_generation,
            GENERATION
        );
        let mut after = snapshot(&root);
        let guard = after.remove(&root.join(MARKER)).unwrap();
        assert_eq!(before, after);
        assert_eq!(saved, snapshot(&archive));
        let g: Guard = serde_json::from_slice(&guard).unwrap();
        assert_eq!(g.source_reader_generation, generation);
        assert!(promote(&root, &archive, &manifest, &r.digest).is_err());
        drop(Store::open(&root).unwrap());
    }
}
// Store intentionally has no Debug implementation, so do not require it merely
// to report a rejected open in a test.
trait OpenError {
    fn unwrap_err_string(self) -> String;
}
impl OpenError for Result<Store, String> {
    fn unwrap_err_string(self) -> String {
        match self {
            Err(e) => e,
            Ok(_) => panic!("unexpectedly opened unprotected data"),
        }
    }
}
#[test]
fn experiment_guard_rejects_live_changed_and_wrong_backup_sources() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data");
    drop(Store::open(&root).unwrap());
    let archive = temp.path().join("cold");
    let manifest = crate::backup::create(&root, &archive)
        .unwrap()
        .manifest_digest;
    let before = snapshot(&root);
    let active = Store::open(&root).unwrap();
    assert!(review(&root, &archive, &manifest).is_err());
    drop(active);
    assert!(review(&root, &archive, &"0".repeat(64)).is_err());
    assert_eq!(before, snapshot(&root));
    fs::write(root.join("late.json"), b"changed since backup").unwrap();
    let changed = snapshot(&root);
    assert!(review(&root, &archive, &manifest).is_err());
    assert_eq!(changed, snapshot(&root));
    let nested = root.join("nested");
    let nested_writer = Store::open(&nested).unwrap();
    assert!(crate::backup::create(&root, &temp.path().join("blocked-backup")).is_err());
    drop(nested_writer);
    let other = temp.path().join("other");
    drop(Store::open(&other).unwrap());
    assert!(review(&other, &archive, &manifest).is_err());
    assert!(!root.join(MARKER).exists());
}
#[test]
fn experiment_cold_promotion_never_bypasses_existing_upgrade_hold() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data");
    drop(Store::open(&root).unwrap());
    let archive = temp.path().join("held-cold");
    let (manifest, _hold) = crate::reader_generation::tests::held(&root, &archive);
    let before = snapshot(&root);
    assert!(review(&root, &archive, &manifest).is_err());
    assert_eq!(before, snapshot(&root));
}
#[test]
fn experiment_guard_rejects_missing_mutated_future_and_pending_records() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data");
    drop(Store::open(&root).unwrap());
    assert!(require_protected(&root).is_err());
    promote_cold(&root, &temp.path().join("cold"));
    let original = fs::read(root.join(MARKER)).unwrap();
    for field in [
        "sourceReaderGeneration",
        "sourceReaderGuardDigest",
        "sourceBackupDigest",
        "minimumReaderGeneration",
        "dataSchema",
        "root",
        "ownerSid",
    ] {
        let mut v: serde_json::Value = serde_json::from_slice(&original).unwrap();
        v[field] = serde_json::json!("changed");
        fs::write(root.join(MARKER), serde_json::to_vec(&v).unwrap()).unwrap();
        assert!(Store::open(&root).is_err(), "{field}");
    }
    fs::write(root.join(MARKER), &original).unwrap();
    fs::write(root.join("events/00000000.reader16.json"), b"{}").unwrap();
    assert!(Store::open(&root).is_err());
    fs::remove_file(root.join("events/00000000.reader16.json")).unwrap();
    fs::write(
        root.join("events/.format-migration-experiments-partial.pending"),
        b"partial",
    )
    .unwrap();
    assert!(Store::open(&root).is_err());
    fs::remove_file(root.join("events/.format-migration-experiments-partial.pending")).unwrap();
    fs::remove_file(root.join(MARKER)).unwrap();
    for name in OWNED {
        fs::write(root.join(name), b"retained").unwrap();
        let before = snapshot(&root);
        assert!(Store::open(&root).is_err(), "{name}");
        assert_eq!(before, snapshot(&root));
        fs::remove_file(root.join(name)).unwrap();
    }
}
#[test]
fn experiment_product_rejects_unprotected_before_any_import_write() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data");
    let mut h = crate::host::RuntimeHost::open(&root).unwrap();
    let before = snapshot(&root);
    let commands = [
        Command::Baseline {
            action: cerulean_domain::baseline_product::BaselineAction::Import {
                id: "trial".into(),
                path: "missing".into(),
                sha256: "a".repeat(64),
            },
        },
        Command::TeamTiming {
            action: cerulean_domain::team_timing::TeamTimingAction::Import {
                path: "missing".into(),
                sha256: "a".repeat(64),
            },
        },
    ];
    for command in commands {
        let r = h.handle(Request {
            id: "guard-test".into(),
            protocol_version: PROTOCOL_VERSION,
            command,
        });
        assert!(r.error.unwrap().message.contains("reader14"));
        assert_eq!(snapshot(&root), before);
    }
}
#[test]
#[ignore = "257 fresh protected Host imports only; no planning, approval, dispatch or credentials"]
fn export_experiment_reader_cli_fixtures() {
    let out = std::path::PathBuf::from(std::env::var_os("CERULEAN_READER_257_DIR").unwrap());
    let repo = out.parent().unwrap().parent().unwrap();
    let mut cases = vec![];
    for kind in ["baseline", "team", "legacy"] {
        let area = out.join(kind);
        fs::create_dir(&area).unwrap();
        let root = area.join("data");
        drop(Store::open(&root).unwrap());
        if kind != "legacy" {
            promote_cold(&root, &area.join("cold-before-import"));
        }
        let mut h = crate::host::RuntimeHost::open(&root).unwrap();
        let command = if kind == "baseline" {
            let path =
                repo.join("reports/baseline-product-256/fixtures/S03-r1-single/request.json");
            use sha2::Digest;
            Command::Baseline {
                action: cerulean_domain::baseline_product::BaselineAction::Import {
                    id: "sample257".into(),
                    path: path.to_string_lossy().into(),
                    sha256: format!("{:x}", sha2::Sha256::digest(fs::read(&path).unwrap())),
                },
            }
        } else if kind == "team" {
            let mut req: crate::team_materials::TeamRequest = serde_json::from_slice(
                &fs::read(
                    repo.join("reports/team-timing-product-255/fixtures/S05-r1-team/request.json"),
                )
                .unwrap(),
            )
            .unwrap();
            req.material.data_root = root.to_string_lossy().into();
            let bytes = serde_json::to_vec(&req).unwrap();
            let path = area.join("request.json");
            fs::write(&path, &bytes).unwrap();
            use sha2::Digest;
            Command::TeamTiming {
                action: cerulean_domain::team_timing::TeamTimingAction::Import {
                    path: path.to_string_lossy().into(),
                    sha256: format!("{:x}", sha2::Sha256::digest(bytes)),
                },
            }
        } else {
            Command::CreatePlan {
                title: "legacy read control".into(),
                tasks: vec!["never dispatch".into()],
            }
        };
        let reply = h.handle(Request {
            id: "guarded-import".into(),
            protocol_version: PROTOCOL_VERSION,
            command,
        });
        assert!(reply.error.is_none(), "{:?}", reply.error);
        assert!(h.can_shutdown());
        let status = crate::data_format::status(&root).unwrap();
        drop(h);
        cases.push(serde_json::json!({"kind":kind,"root":root,"generation":status.minimum_reader_generation,"modelCalls":0}));
    }
    let area = out.join("pending");
    fs::create_dir(&area).unwrap();
    let root = area.join("data");
    drop(Store::open(&root).unwrap());
    fs::create_dir(root.join("baseline-experiments")).unwrap();
    fs::write(
        root.join("baseline-experiments/original.pending"),
        b"preserve original uncertain import",
    )
    .unwrap();
    let backup = area.join("cold");
    let manifest = crate::backup::create(&root, &backup)
        .unwrap()
        .manifest_digest;
    cases.push(serde_json::json!({"kind":"pending","root":root,"backup":backup,"manifestDigest":manifest,"generation":1,"modelCalls":0}));
    crate::api_execution::publish(&out, "cases.json", &cases).unwrap();
}
