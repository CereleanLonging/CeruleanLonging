//! 326 prepare-only regressions. No credentials, transport, subprocess or model.
use super::*;
use crate::reader_generation::tests::snapshot;
use cerulean_domain::{api_worker::ApiTarget, credentials::ProviderEndpoint};

fn spec(root: &Path, relay: bool) -> RunSpec {
    let mut plan = crate::api_execution::tests::baseline_plan_fixture(root);
    plan.project.id = "eff-relay-prepare".into();
    plan.workflow.tasks[0].workspace_key = plan.project.id.clone();
    if relay {
        plan.execution[0].adapter_id = "a6api".into();
        plan.execution[0].api_worker.as_mut().unwrap().target = ApiTarget::Provider {
            endpoint: ProviderEndpoint::A6Api {},
        };
    }
    RunSpec {
        material: None,
        schema: 1,
        slot_id: "relay-prepare".into(),
        data_root: root.join("data").to_string_lossy().into(),
        material_manifest_sha256: "a".repeat(64),
        configuration_sha256: "b".repeat(64),
        seed: observe(
            Path::new(&plan.project.root),
            plan.snapshot_policy.as_ref().unwrap(),
        )
        .unwrap(),
        plan,
    }
}

#[test]
fn a6api_prepare_keeps_real_empty_cold_backup_and_unapproved_readback() {
    let temp = tempfile::tempdir().unwrap();
    let spec = spec(temp.path(), true);
    let data = PathBuf::from(&spec.data_root);
    let archive = relay_archive(&prospective(&data).unwrap(), true)
        .unwrap()
        .unwrap();
    let result = prepare(&data, spec.clone()).unwrap();
    assert_eq!(result["prepared"], true);
    assert_eq!(result["authorized"], false);
    assert_eq!(result["dispatched"], false);
    assert_eq!(
        crate::data_format::status(&data)
            .unwrap()
            .minimum_reader_generation,
        17
    );
    let backup = crate::backup::verify(&archive).unwrap();
    let guard_bytes = fs::read(data.join(crate::rejected_usage_guard::MARKER)).unwrap();
    let guard: Value = serde_json::from_slice(&guard_bytes).unwrap();
    assert_eq!(guard["sourceBackupDigest"], backup.manifest_digest);
    assert_eq!(guard["sourceReaderGeneration"], 1);
    assert!(guard["sourceReaderGuardDigest"].is_null());
    assert_eq!(guard["ownerSid"], crate::ipc::current_sid().unwrap());
    assert_eq!(
        Path::new(guard["root"].as_str().unwrap()),
        fs::canonicalize(&data).unwrap()
    );
    let manifest: Value =
        serde_json::from_slice(&fs::read(archive.join("manifest.json")).unwrap()).unwrap();
    let paths: Vec<_> = manifest["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["path"].as_str().unwrap())
        .collect();
    assert_eq!(paths, ["events", "writer.lock"]);
    assert!(!archive.join("payload/run-spec.json").exists());
    assert_eq!(
        fs::read_dir(archive.join("payload/events"))
            .unwrap()
            .count(),
        0
    );
    // The marker is deliberately not a legacy event. This checks its shape;
    // actual older-reader binaries remain a separate release acceptance check.
    assert!(serde_json::from_slice::<crate::Event>(&guard_bytes).is_err());
    let before = snapshot(temp.path());
    for _ in 0..2 {
        let readback = inspect(&data, &digest(&spec).unwrap()).unwrap();
        assert_eq!(readback["status"], "prepared");
        assert_eq!(readback["coreExport"]["apiIntentCount"], 0);
        assert_eq!(readback["managementModelRequests"], 0);
        assert!(readback["started"].is_null());
        assert!(prepare(&data, spec.clone()).is_err());
    }
    assert_eq!(snapshot(temp.path()), before);
}

#[test]
fn deepseek_prepare_creates_reader17_backup_and_preserves_unrelated_reader16_sibling() {
    let temp = tempfile::tempdir().unwrap();
    let spec = spec(temp.path(), false);
    let data = PathBuf::from(&spec.data_root);
    let sibling = temp.path().join("data.reader16-cold-backup");
    fs::write(&sibling, b"unrelated-original-file").unwrap();
    prepare(&data, spec.clone()).unwrap();
    assert_eq!(
        crate::data_format::status(&data)
            .unwrap()
            .minimum_reader_generation,
        17
    );
    assert!(data.join(crate::rejected_usage_guard::MARKER).exists());
    crate::backup::verify(&temp.path().join("data.reader17-cold-backup")).unwrap();
    assert_eq!(fs::read(sibling).unwrap(), b"unrelated-original-file");
    assert_eq!(
        inspect(&data, &digest(&spec).unwrap()).unwrap()["status"],
        "prepared"
    );
}

#[test]
fn relay_backup_collisions_are_rejected_before_creating_data() {
    for directory in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let spec = spec(temp.path(), true);
        let data = PathBuf::from(&spec.data_root);
        let archive = temp.path().join("data.reader17-cold-backup");
        if directory {
            fs::create_dir(&archive).unwrap();
            fs::write(archive.join("keep.txt"), b"preexisting").unwrap();
        } else {
            fs::write(&archive, b"preexisting").unwrap();
        }
        let before = snapshot(temp.path());
        let error = prepare(&data, spec).unwrap_err();
        assert!(
            error.contains("cold-backup destination already exists"),
            "{error}"
        );
        assert!(!data.exists());
        assert_eq!(snapshot(temp.path()), before);
    }
}

#[test]
fn existing_or_interrupted_roots_are_not_promoted_or_reused() {
    for interrupted in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let spec = spec(temp.path(), true);
        let data = PathBuf::from(&spec.data_root);
        fs::create_dir(&data).unwrap();
        if interrupted {
            fs::create_dir(data.join("events")).unwrap();
            drop(lock(&data, true).unwrap());
            crate::backup::create(&data, &temp.path().join("data.reader17-cold-backup")).unwrap();
        } else {
            fs::write(data.join("original.txt"), b"preexisting-data").unwrap();
        }
        let before = snapshot(temp.path());
        assert!(prepare(&data, spec).unwrap_err().contains("must be new"));
        assert_eq!(
            crate::data_format::status(&data)
                .unwrap()
                .minimum_reader_generation,
            1
        );
        assert!(!data.join("prepared.json").exists());
        assert_eq!(snapshot(temp.path()), before);
    }
}

#[test]
fn missing_guard_is_not_recreated_by_inspection_or_repreparation() {
    let temp = tempfile::tempdir().unwrap();
    let spec = spec(temp.path(), true);
    let data = PathBuf::from(&spec.data_root);
    prepare(&data, spec.clone()).unwrap();
    fs::remove_file(data.join(crate::rejected_usage_guard::MARKER)).unwrap();
    let before = snapshot(temp.path());
    assert!(inspect(&data, &digest(&spec).unwrap())
        .unwrap_err()
        .contains("reader16"));
    assert!(prepare(&data, spec).is_err());
    assert!(!data.join("started.json").exists());
    assert_eq!(snapshot(temp.path()), before);
}

// Deterministic same-thread interleaving only at this function's released locks.
// Each mutation takes the ordinary writer lock, as a competing writer would.
#[derive(Clone, Copy)]
enum Interference {
    BeforeBackupRoot,
    BeforeBackupEvent,
    AfterPromotionRoot,
    AfterPromotionEvent,
    AfterPromotionGuard,
}
thread_local! {
    static INTERFERENCE: std::cell::Cell<Option<Interference>> = const { std::cell::Cell::new(None) };
}

pub(super) fn checkpoint(root: &Path, phase: &str) -> Result<(), String> {
    let selected = INTERFERENCE.get();
    let applies = matches!(
        (selected, phase),
        (
            Some(Interference::BeforeBackupRoot | Interference::BeforeBackupEvent),
            "before-backup"
        ) | (
            Some(
                Interference::AfterPromotionRoot
                    | Interference::AfterPromotionEvent
                    | Interference::AfterPromotionGuard
            ),
            "after-promotion"
        )
    );
    if !applies {
        return Ok(());
    }
    INTERFERENCE.set(None);
    let _writer = lock(root, false)?;
    match selected.unwrap() {
        Interference::BeforeBackupRoot | Interference::AfterPromotionRoot => {
            fs::write(root.join("other-writer.txt"), b"preserve-other-writer-data")
                .map_err(|e| e.to_string())?;
        }
        Interference::BeforeBackupEvent | Interference::AfterPromotionEvent => {
            fs::write(root.join("events/other-writer.json"), b"{}").map_err(|e| e.to_string())?;
        }
        Interference::AfterPromotionGuard => {
            let path = root.join(crate::rejected_usage_guard::MARKER);
            let mut guard: Value =
                serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            // Still structurally valid for reader17, but no longer the exact
            // guard returned by this operation's promotion of this cold backup.
            guard["sourceBackupDigest"] = "c".repeat(64).into();
            fs::write(path, serde_json::to_vec(&guard).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn prepare_with_interference(
    data: &Path,
    spec: RunSpec,
    selected: Interference,
) -> Result<Value, String> {
    assert!(INTERFERENCE.replace(Some(selected)).is_none());
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            INTERFERENCE.set(None);
        }
    }
    let _reset = Reset;
    let result = prepare(data, spec);
    assert!(
        INTERFERENCE.get().is_none(),
        "test interleaving was not reached"
    );
    result
}

#[test]
fn relay_new_root_changes_before_backup_are_preserved_without_promotion() {
    for selected in [
        Interference::BeforeBackupRoot,
        Interference::BeforeBackupEvent,
    ] {
        let temp = tempfile::tempdir().unwrap();
        let spec = spec(temp.path(), true);
        let data = PathBuf::from(&spec.data_root);
        let error = prepare_with_interference(&data, spec.clone(), selected).unwrap_err();
        assert!(
            error.contains("cold backup is no longer the empty new root"),
            "{error}"
        );
        let archive = temp.path().join("data.reader17-cold-backup");
        crate::backup::verify(&archive).unwrap();
        let relative = match selected {
            Interference::BeforeBackupRoot => "other-writer.txt",
            Interference::BeforeBackupEvent => "events/other-writer.json",
            _ => unreachable!(),
        };
        assert_eq!(
            fs::read(data.join(relative)).unwrap(),
            fs::read(archive.join("payload").join(relative)).unwrap()
        );
        assert_eq!(
            crate::data_format::status(&data)
                .unwrap()
                .minimum_reader_generation,
            1
        );
        assert!(!data.join(crate::rejected_usage_guard::MARKER).exists());
        assert!(!data.join("run-spec.json").exists());
        assert!(!data.join("prepared.json").exists());
        let before = snapshot(temp.path());
        assert!(prepare(&data, spec).is_err());
        assert_eq!(snapshot(temp.path()), before);
    }
}

#[test]
fn relay_new_root_changes_after_promotion_are_preserved_without_preparation() {
    for selected in [
        Interference::AfterPromotionRoot,
        Interference::AfterPromotionEvent,
        Interference::AfterPromotionGuard,
    ] {
        let temp = tempfile::tempdir().unwrap();
        let spec = spec(temp.path(), true);
        let data = PathBuf::from(&spec.data_root);
        let error = prepare_with_interference(&data, spec.clone(), selected).unwrap_err();
        assert!(
            error.contains("new root changed")
                || error.contains("guard or event inventory changed"),
            "{error}"
        );
        let archive = temp.path().join("data.reader17-cold-backup");
        let backup = crate::backup::verify(&archive).unwrap();
        require_empty_relay_backup(&archive, &backup.manifest_digest).unwrap();
        // reader17 alone is insufficient: these interrupted roots must never
        // receive RunSpec/Prepared or an automatic retry after an intervening write.
        assert_eq!(
            crate::data_format::status(&data)
                .unwrap()
                .minimum_reader_generation,
            17
        );
        match selected {
            Interference::AfterPromotionRoot => assert_eq!(
                fs::read(data.join("other-writer.txt")).unwrap(),
                b"preserve-other-writer-data"
            ),
            Interference::AfterPromotionEvent => assert_eq!(
                fs::read(data.join("events/other-writer.json")).unwrap(),
                b"{}"
            ),
            Interference::AfterPromotionGuard => {
                let guard: Value = serde_json::from_slice(
                    &fs::read(data.join(crate::rejected_usage_guard::MARKER)).unwrap(),
                )
                .unwrap();
                assert_eq!(guard["sourceBackupDigest"], "c".repeat(64));
                assert_ne!(guard["sourceBackupDigest"], backup.manifest_digest);
            }
            _ => unreachable!(),
        }
        assert!(!data.join("run-spec.json").exists());
        assert!(!data.join("prepared.json").exists());
        assert!(!data.join("started.json").exists());
        let before = snapshot(temp.path());
        assert!(prepare(&data, spec).is_err());
        assert_eq!(snapshot(temp.path()), before);
    }
}
