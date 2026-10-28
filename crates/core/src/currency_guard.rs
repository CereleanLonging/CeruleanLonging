//! Reader4 protects approved currency settings and their project-wide ledgers.
//! It layers over the unchanged container/reader3 records, never rewrites them.
use crate::data_format::Status;
use cerulean_domain::execution::{digest, require, sha256, timestamp};
use serde::{Deserialize, Serialize};
use std::{fs, io::Read, path::Path};
pub const GENERATION: u32 = 4;
pub const FILE_NAME: &str = "00000000.reader4.json";
pub const MARKER: &str = "events/00000000.reader4.json";

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
    let path = root.join(MARKER);
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
        Ok(_) => {}
    }
    #[cfg(windows)]
    let _pins = [root.to_path_buf(), root.join("events")]
        .iter()
        .map(|p| crate::backup::open(p, true, false).map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    #[cfg(windows)]
    let file = crate::backup::open(&path, false, false).map_err(|e| e.to_string())?;
    #[cfg(not(windows))]
    let file = fs::File::open(&path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    file.take(65537)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    require(bytes.len() <= 65536, "currency reader guard too large")?;
    let guard: Guard =
        serde_json::from_slice(&bytes).map_err(|_| "invalid currency reader guard")?;
    require(
        guard.schema == 1
            && guard.product == cerulean_domain::PRODUCT_NAME
            && guard.minimum_reader_generation == GENERATION
            && guard.data_schema == status.data_schema
            && guard.source_marker_digest == status.marker_digest
            && guard.source_reader_generation == status.minimum_reader_generation
            && guard.source_reader_guard_digest == status.reader_guard_digest
            && guard.source_reader_generation < GENERATION
            && Path::new(&guard.root) == fs::canonicalize(root).map_err(|e| e.to_string())?,
        "currency reader guard original data or reader changed",
    )?;
    sha256(&guard.source_backup_digest)?;
    timestamp(&guard.promoted_at)?;
    #[cfg(windows)]
    require(
        guard.owner_sid == crate::ipc::current_sid().map_err(|e| e.to_string())?,
        "currency reader guard belongs to another Windows user",
    )?;
    use sha2::Digest;
    status.minimum_reader_generation = GENERATION;
    status.reader_guard_digest = Some(format!("{:x}", sha2::Sha256::digest(bytes)));
    Ok(())
}
pub(crate) fn require_protected(root: &Path, present: bool) -> Result<(), String> {
    require(
        !present || crate::data_format::status(root)?.minimum_reader_generation >= GENERATION,
        "金额预算需要先停核心、锁定并备份原数据，再明确启用reader4保护",
    )
}

#[cfg(windows)]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PromotionReview {
    pub digest: String,
    source_root: String,
    backup: String,
    backup_digest: String,
    hold_digest: String,
    data_schema: u32,
    source_marker_digest: Option<String>,
    source_reader_guard_digest: Option<String>,
    from_reader_generation: u32,
    to_reader_generation: u32,
    migrator_version: &'static str,
    original_events_rewritten: bool,
    hold_released: bool,
}
#[cfg(windows)]
fn reviewed(
    root: &Path,
    archive: &Path,
    manifest: &str,
    hold: &str,
) -> Result<PromotionReview, String> {
    crate::maintenance_interlock::ensure_not_reserved(root)?;
    crate::reader_generation::reject_unprotected(root)?;
    sha256(manifest)?;
    sha256(hold)?;
    let status = crate::data_format::status(root)?;
    require(
        status.minimum_reader_generation < GENERATION,
        "currency reader protection already present; do not repeat promotion",
    )?;
    let mut review = PromotionReview {
        digest: String::new(),
        source_root: root.to_string_lossy().into_owned(),
        backup: fs::canonicalize(archive)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .into_owned(),
        backup_digest: manifest.into(),
        hold_digest: hold.into(),
        data_schema: status.data_schema,
        source_marker_digest: status.marker_digest,
        source_reader_guard_digest: status.reader_guard_digest,
        from_reader_generation: status.minimum_reader_generation,
        to_reader_generation: GENERATION,
        migrator_version: env!("CARGO_PKG_VERSION"),
        original_events_rewritten: false,
        hold_released: false,
    };
    review.digest = digest(&review)?;
    Ok(review)
}
#[cfg(windows)]
pub fn review(
    root: &Path,
    archive: &Path,
    manifest: &str,
    hold: &str,
) -> Result<PromotionReview, String> {
    crate::backup::with_original_held(root, archive, manifest, hold, |root| {
        reviewed(root, archive, manifest, hold)
    })
}
#[cfg(windows)]
pub fn promote(
    root: &Path,
    archive: &Path,
    manifest: &str,
    hold: &str,
    expected: &str,
) -> Result<Status, String> {
    use std::io::Write;
    crate::backup::with_original_held(root, archive, manifest, hold, |root| {
        let r = reviewed(root, archive, manifest, hold)?;
        require(
            r.digest == expected,
            "currency reader review changed; no write performed",
        )?;
        let guard = Guard {
            schema: 1,
            product: cerulean_domain::PRODUCT_NAME.into(),
            minimum_reader_generation: GENERATION,
            data_schema: r.data_schema,
            source_marker_digest: r.source_marker_digest,
            source_reader_generation: r.from_reader_generation,
            source_reader_guard_digest: r.source_reader_guard_digest,
            source_backup_digest: manifest.into(),
            root: root.to_string_lossy().into_owned(),
            owner_sid: crate::ipc::current_sid().map_err(|e| e.to_string())?,
            promoted_at: chrono::Utc::now().to_rfc3339(),
        };
        let events = root.join("events");
        let _pin = crate::backup::open(&events, true, false).map_err(|e| e.to_string())?;
        let pending = events.join(format!(
            ".format-migration-currency-{}-{}.pending",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&pending)
            .map_err(|e| e.to_string())?;
        file.write_all(&serde_json::to_vec_pretty(&guard).map_err(|e| e.to_string())?)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);
        crate::backup::publish(&pending, &root.join(MARKER)).map_err(|e| e.to_string())?;
        crate::data_format::status(root)
    })
}

#[cfg(all(test, windows))]
pub(crate) mod tests {
    use super::*;
    use crate::{
        reader_generation::tests::{held, snapshot},
        Store,
    };

    pub(crate) fn promote_fixture(root: &Path) {
        let archive = root.with_file_name("currency-cold-backup");
        let (manifest, hold) = held(root, &archive);
        let r = review(root, &archive, &manifest, &hold).unwrap();
        promote(root, &archive, &manifest, &hold, &r.digest).unwrap();
        crate::upgrade::release(root, &hold).unwrap();
    }

    #[test]
    fn currency_promotion_preserves_each_original_reader_and_requires_release() {
        for generation in [1, 2, 3] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("data");
            drop(Store::open(&root).unwrap());
            if generation == 2 {
                let archive = temp.path().join("schema-backup");
                let (manifest, hold) = held(&root, &archive);
                let r = crate::data_format::review(&root, &archive, &manifest, &hold).unwrap();
                crate::data_format::migrate(&root, &archive, &manifest, &hold, &r.digest).unwrap();
                crate::upgrade::release(&root, &hold).unwrap();
            } else if generation == 3 {
                crate::reader_generation::tests::promote_fixture(&root);
            }
            let archive = temp.path().join("currency-backup");
            let (manifest, hold) = held(&root, &archive);
            let original_status = crate::data_format::status(&root).unwrap();
            let before = snapshot(&root);
            let archived = snapshot(&archive);
            let r = review(&root, &archive, &manifest, &hold).unwrap();
            assert!(promote(&root, &archive, &manifest, &hold, &"0".repeat(64)).is_err());
            assert_eq!(snapshot(&root), before);
            let status = promote(&root, &archive, &manifest, &hold, &r.digest).unwrap();
            assert_eq!(status.minimum_reader_generation, 4);
            assert_eq!(status.marker_digest, original_status.marker_digest);
            let mut after = snapshot(&root);
            let guard = after.remove(&root.join(MARKER)).unwrap();
            assert_eq!(after, before);
            assert_eq!(snapshot(&archive), archived);
            let guard: Guard = serde_json::from_slice(&guard).unwrap();
            assert_eq!(guard.source_reader_generation, generation);
            assert_eq!(
                guard.source_reader_guard_digest,
                original_status.reader_guard_digest
            );
            assert!(Store::open(&root).is_err());
            assert!(review(&root, &archive, &manifest, &hold).is_err());
            crate::upgrade::release(&root, &hold).unwrap();
            drop(Store::open(&root).unwrap());
        }
    }

    #[test]
    fn currency_guard_binds_original_guard_and_refuses_incomplete_or_relocated_records() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("data");
        crate::reader_generation::tests::promote_fixture(&root);
        promote_fixture(&root);
        let bytes = fs::read(root.join(MARKER)).unwrap();
        for field in [
            "sourceReaderGeneration",
            "sourceReaderGuardDigest",
            "sourceBackupDigest",
            "root",
            "ownerSid",
        ] {
            let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            value[field] = serde_json::json!("changed");
            fs::write(root.join(MARKER), serde_json::to_vec(&value).unwrap()).unwrap();
            assert!(Store::open(&root).is_err(), "{field}");
        }
        fs::write(root.join(MARKER), b"{").unwrap();
        assert!(Store::open(&root).is_err());
        fs::write(root.join(MARKER), bytes).unwrap();
        let prior_guard = root.join(crate::reader_generation::MARKER);
        let mut original = fs::read(&prior_guard).unwrap();
        original.push(b' ');
        fs::write(prior_guard, original).unwrap();
        assert!(Store::open(&root).is_err());
    }

    #[test]
    fn currency_records_without_guard_are_rejected_before_loading_or_auto_promotion() {
        for (path, value) in [
            (
                "project-setups/setup.json",
                serde_json::json!({"setup":{"policy":{"currencyBudget":{}}}}),
            ),
            (
                "workflows/project/events/00000001.json",
                serde_json::json!({"body":{"context":{"execution":{"revision":{"policy":{"currencyBudget":{}}}}}}}),
            ),
            (
                "management-decisions/operation.json",
                serde_json::json!({"body":{"evidence":{"intent":{"context":{"executionSetup":{"setup":{"policy":{"currencyBudget":{}}}}}}}}}),
            ),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().join("data");
            crate::reader_generation::tests::promote_fixture(&root);
            let record = root.join(path);
            fs::create_dir_all(record.parent().unwrap()).unwrap();
            fs::write(&record, serde_json::to_vec(&value).unwrap()).unwrap();
            let before = snapshot(&root);
            assert!(crate::reader_generation::reject_unprotected(&root)
                .unwrap_err()
                .contains("currency"));
            assert!(Store::open(&root).is_err());
            assert_eq!(snapshot(&root), before);
        }
    }
}
