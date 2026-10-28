//! Reader16 protects separately credentialed A6API records.
use crate::data_format::Status;
use cerulean_domain::execution::{digest, require, sha256, timestamp};
use serde::{Deserialize, Serialize};
use std::{fs, io::Read, path::Path};
pub const GENERATION: u32 = 16;
pub const FILE_NAME: &str = "00000000.reader16.json";
pub const MARKER: &str = "events/00000000.reader16.json";
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Guard {
    schema: u32,
    product: String,
    minimum_reader_generation: u32,
    data_schema: u32,
    source_marker_digest: Option<String>,
    source_reader_generation: u32,
    source_reader_guard_digest: Option<String>,
    source_backup_digest: String,
    root: String,
    owner_sid: String,
    promoted_at: String,
}
pub(crate) fn observe(root: &Path, status: &mut Status) -> Result<(), String> {
    observe_generation(root, status, FILE_NAME, GENERATION)
}
fn observe_generation(
    root: &Path,
    status: &mut Status,
    name: &str,
    generation: u32,
) -> Result<(), String> {
    let path = root.join("events").join(name);
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
        Ok(_) => (),
    }
    #[cfg(windows)]
    let _pins = [root.to_owned(), root.join("events")]
        .iter()
        .map(|p| crate::backup::open(p, true, false).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    #[cfg(windows)]
    let file = crate::backup::open(&path, false, false).map_err(|e| e.to_string())?;
    #[cfg(not(windows))]
    let file = fs::File::open(&path).map_err(|e| e.to_string())?;
    let mut bytes = vec![];
    file.take(65537)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    require(bytes.len() <= 65536, "relay reader guard too large")?;
    let g: Guard = serde_json::from_slice(&bytes).map_err(|_| "invalid relay reader guard")?;
    require(
        g.schema == 1
            && g.product == cerulean_domain::PRODUCT_NAME
            && g.minimum_reader_generation == generation
            && g.data_schema == status.data_schema
            && g.source_marker_digest == status.marker_digest
            && g.source_reader_generation == status.minimum_reader_generation
            && g.source_reader_guard_digest == status.reader_guard_digest
            && g.source_reader_generation < generation
            && Path::new(&g.root) == fs::canonicalize(root).map_err(|e| e.to_string())?,
        "relay reader guard original data or reader changed",
    )?;
    sha256(&g.source_backup_digest)?;
    timestamp(&g.promoted_at)?;
    #[cfg(windows)]
    require(
        g.owner_sid == crate::ipc::current_sid().map_err(|e| e.to_string())?,
        "relay reader guard belongs to another Windows user",
    )?;
    use sha2::Digest;
    status.minimum_reader_generation = generation;
    status.reader_guard_digest = Some(format!("{:x}", sha2::Sha256::digest(bytes)));
    Ok(())
}
pub(crate) fn require_protected(root: &Path, present: bool) -> Result<(), String> {
    require(
        !present || crate::data_format::status(root)?.minimum_reader_generation >= GENERATION,
        "中转及可选指挥来源需要 reader16 保护；停止核心并创建原数据冷备份，再审阅启用中转保护",
    )
}
pub(crate) fn revision_uses_relay(plan: &cerulean_domain::execution::PlanRevision) -> bool {
    plan.execution.iter().any(|e| {
        e.adapter_id == "a6api"
            || e.api_worker.as_ref().is_some_and(|w| {
                w.target.provider() == cerulean_domain::credentials::CredentialProvider::A6Api
            })
    })
}
// Only protocol-owned selections, not task text or arbitrary tool JSON.
pub(crate) fn record_uses_relay(value: &serde_json::Value) -> bool {
    fn worker(v: &serde_json::Value) -> bool {
        v["workerAdapter"] == "a6api"
            || v["adapterId"] == "a6api"
            || v.pointer("/apiWorker/target/endpoint/kind")
                .is_some_and(|s| s == "a6Api")
    }
    fn sources(v: &serde_json::Value) -> bool {
        v["managementSources"]["chief"]["kind"] == "provider"
            || v["managementSources"]["deputy"]["kind"] == "provider"
    }
    if sources(&value["setup"])
        || [
            "/body/evidence/intent/context/executionSetup/setup",
            "/body/evidence/request/handoff/context/executionSetup/setup",
        ]
        .iter()
        .any(|p| value.pointer(p).is_some_and(sources))
        || [
            "/body/evidence/request/handoff/source/kind",
            "/body/evidence/response/request/handoff/source/kind",
        ]
        .iter()
        .any(|p| value.pointer(p).is_some_and(|v| v == "provider"))
    {
        return true;
    }
    let setup = &value["setup"];
    if worker(setup)
        || setup["profiles"]
            .as_array()
            .is_some_and(|p| p.iter().any(worker))
    {
        return true;
    }
    value
        .pointer("/body/context/execution/revision/execution")
        .and_then(|p| p.as_array())
        .is_some_and(|p| p.iter().any(worker))
}
#[cfg(windows)]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromotionReview {
    pub digest: String,
    source_root: String,
    backup: String,
    backup_digest: String,
    data_schema: u32,
    source_marker_digest: Option<String>,
    source_reader_guard_digest: Option<String>,
    from_reader_generation: u32,
    to_reader_generation: u32,
    migrator_version: &'static str,
    original_events_rewritten: bool,
    execution_resumed: bool,
}
#[cfg(windows)]
fn reviewed(root: &Path, archive: &Path, manifest: &str) -> Result<PromotionReview, String> {
    // Promotion never repairs existing unprotected relay records or rewrites history.
    crate::reader_generation::reject_unprotected(root)?;
    sha256(manifest)?;
    let s = crate::data_format::status(root)?;
    require(
        s.minimum_reader_generation < GENERATION,
        "relay protection already present; do not repeat promotion",
    )?;
    let mut r = PromotionReview {
        digest: String::new(),
        source_root: root.to_string_lossy().into(),
        backup: fs::canonicalize(archive)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .into(),
        backup_digest: manifest.into(),
        data_schema: s.data_schema,
        source_marker_digest: s.marker_digest,
        source_reader_guard_digest: s.reader_guard_digest,
        from_reader_generation: s.minimum_reader_generation,
        to_reader_generation: GENERATION,
        migrator_version: env!("CARGO_PKG_VERSION"),
        original_events_rewritten: false,
        execution_resumed: false,
    };
    r.digest = digest(&r)?;
    Ok(r)
}
#[cfg(windows)]
pub fn review(root: &Path, archive: &Path, manifest: &str) -> Result<PromotionReview, String> {
    crate::backup::with_original_cold(root, archive, manifest, |root| {
        reviewed(root, archive, manifest)
    })
}
#[cfg(windows)]
pub fn promote(
    root: &Path,
    archive: &Path,
    manifest: &str,
    expected: &str,
) -> Result<Status, String> {
    use std::io::Write;
    crate::backup::with_original_cold(root, archive, manifest, |root| {
        let r = reviewed(root, archive, manifest)?;
        require(
            r.digest == expected,
            "relay reader review changed; no write performed",
        )?;
        let g = Guard {
            schema: 1,
            product: cerulean_domain::PRODUCT_NAME.into(),
            minimum_reader_generation: GENERATION,
            data_schema: r.data_schema,
            source_marker_digest: r.source_marker_digest,
            source_reader_generation: r.from_reader_generation,
            source_reader_guard_digest: r.source_reader_guard_digest,
            source_backup_digest: manifest.into(),
            root: root.to_string_lossy().into(),
            owner_sid: crate::ipc::current_sid().map_err(|e| e.to_string())?,
            promoted_at: chrono::Utc::now().to_rfc3339(),
        };
        let events = root.join("events");
        let _pin = crate::backup::open(&events, true, false).map_err(|e| e.to_string())?;
        let pending = events.join(format!(
            ".format-migration-relays-{}-{}.pending",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let mut f = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&pending)
            .map_err(|e| e.to_string())?;
        f.write_all(&serde_json::to_vec_pretty(&g).map_err(|e| e.to_string())?)
            .and_then(|_| f.sync_all())
            .map_err(|e| e.to_string())?;
        drop(f);
        crate::backup::publish(&pending, &root.join(MARKER)).map_err(|e| e.to_string())?;
        crate::data_format::status(root)
    })
}

#[cfg(all(test, windows))]
pub(crate) mod tests {
    use super::*;
    use crate::{reader_generation::tests::snapshot, Store};

    pub(crate) fn promote_fixture(root: &Path, archive: &Path) {
        let manifest = crate::backup::create(root, archive)
            .unwrap()
            .manifest_digest;
        let reviewed = review(root, archive, &manifest).unwrap();
        promote(root, archive, &manifest, &reviewed.digest).unwrap();
    }

    #[test]
    fn relay_promotion_requires_cold_backup_and_preserves_reader15_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("data");
        drop(Store::open(&root).unwrap());
        let old_archive = temp.path().join("before15");
        let old_manifest = crate::backup::create(&root, &old_archive)
            .unwrap()
            .manifest_digest;
        let old_review =
            crate::experiment_guard::review(&root, &old_archive, &old_manifest).unwrap();
        crate::experiment_guard::promote(&root, &old_archive, &old_manifest, &old_review.digest)
            .unwrap();
        let archive = temp.path().join("before16");
        let manifest = crate::backup::create(&root, &archive)
            .unwrap()
            .manifest_digest;
        let before = snapshot(&root);
        let saved = snapshot(&archive);
        let reviewed = review(&root, &archive, &manifest).unwrap();
        assert!(promote(&root, &archive, &manifest, &"0".repeat(64)).is_err());
        assert_eq!(snapshot(&root), before);
        let owner = Store::open(&root).unwrap();
        assert!(promote(&root, &archive, &manifest, &reviewed.digest).is_err());
        drop(owner);
        assert_eq!(
            promote(&root, &archive, &manifest, &reviewed.digest)
                .unwrap()
                .minimum_reader_generation,
            16
        );
        let mut after = snapshot(&root);
        let bytes = after.remove(&root.join(MARKER)).unwrap();
        let guard: Guard = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(guard.source_reader_generation, 15);
        assert_eq!(after, before);
        assert_eq!(snapshot(&archive), saved);
        assert!(promote(&root, &archive, &manifest, &reviewed.digest).is_err());
        drop(Store::open(&root).unwrap());
        assert_eq!(
            crate::data_format::status(&root)
                .unwrap()
                .minimum_reader_generation,
            16
        );
    }

    #[test]
    fn relay_preflight_checks_protocol_fields_without_interpreting_task_text() {
        use serde_json::json;
        for value in [
            json!({"setup":{"workerAdapter":"a6api"}}),
            json!({"setup":{"managementSources":{"chief":{"kind":"provider","endpoint":{"kind":"deepSeek"}}}}}),
            json!({"body":{"evidence":{"request":{"handoff":{"source":{"kind":"provider","endpoint":{"kind":"a6Api"}}}}}}}),
            json!({"body":{"evidence":{"response":{"request":{"handoff":{"source":{"kind":"provider","endpoint":{"kind":"anthropic"}}}}}}}}),
            json!({"setup":{"profiles":[{"workerAdapter":"a6api"}]}}),
            json!({"body":{"context":{"execution":{"revision":{"execution":[{"adapterId":"a6api"}]}}}}}),
        ] {
            assert!(record_uses_relay(&value));
        }
        assert!(!record_uses_relay(
            &json!({"setup":{"requirement":"a6api", "notes":{"workerAdapter":"a6api"}}})
        ));
        assert!(!record_uses_relay(
            &json!({"notes":{"managementSources":{"chief":{"kind":"provider"}}}})
        ));
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("data");
        drop(Store::open(&root).unwrap());
        fs::create_dir(root.join("project-setups")).unwrap();
        fs::write(
            root.join("project-setups/relay.json"),
            json!({"setup":{"workerAdapter":"a6api"}}).to_string(),
        )
        .unwrap();
        let before = snapshot(&root);
        let error = match Store::open(&root) {
            Err(e) => e,
            Ok(_) => panic!("unprotected relay was loaded"),
        };
        assert!(error.contains("reader16"), "{error}");
        assert_eq!(snapshot(&root), before);
    }

    #[test]
    fn relay_vault_separates_official_credentials_and_rejects_unprotected_save() {
        use cerulean_domain::{credentials::CredentialProvider as P, secret::SecretInput};
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("data");
        drop(Store::open(&root).unwrap());
        let relay = SecretInput::new("synthetic-a6api-vault-sentinel".into()).unwrap();
        let official = SecretInput::new("synthetic-official-vault-sentinel".into()).unwrap();
        let before = snapshot(&root);
        assert!(crate::credential_vault::save_for(&root, P::A6Api, &relay)
            .unwrap_err()
            .contains("reader16"));
        assert_eq!(snapshot(&root), before);
        promote_fixture(&root, &temp.path().join("cold"));
        crate::credential_vault::save_for(&root, P::A6Api, &relay).unwrap();
        crate::credential_vault::save_for(&root, P::OpenAi, &official).unwrap();
        assert_eq!(
            crate::credential_vault::load_for(&root, P::A6Api)
                .unwrap()
                .expose(),
            relay.expose()
        );
        assert_eq!(
            crate::credential_vault::load_for(&root, P::OpenAi)
                .unwrap()
                .expose(),
            official.expose()
        );
        let openai_path = root.join("credentials/openai.dpapi");
        let official_bytes = fs::read(&openai_path).unwrap();
        let relay_bytes = fs::read(root.join("credentials/a6api.dpapi")).unwrap();
        assert!(!relay_bytes
            .windows(relay.expose().len())
            .any(|w| w == relay.expose().as_bytes()));
        fs::write(&openai_path, &relay_bytes).unwrap();
        assert!(crate::credential_vault::load_for(&root, P::OpenAi).is_err());
        fs::write(&openai_path, &official_bytes).unwrap();
        crate::credential_vault::remove_for(&root, P::A6Api).unwrap();
        assert!(!crate::credential_vault::exists_for(&root, P::A6Api).unwrap());
        assert_eq!(fs::read(&openai_path).unwrap(), official_bytes);
    }
}
