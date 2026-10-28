//! Reader12 protects explicit local-user API fee reconciliation.
//! It layers over the original reader11 guard; promotion requires a cold backup.
use crate::data_format::Status;
use cerulean_domain::execution::{digest, require, sha256, timestamp};
use serde::{Deserialize, Serialize};
use std::{fs, io::Read, path::Path};
pub const GENERATION: u32 = 12;
pub const FILE_NAME: &str = "00000000.reader12.json";
pub const MARKER: &str = "events/00000000.reader12.json";
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
    require(
        bytes.len() <= 65536,
        "API fee resolution reader guard too large",
    )?;
    let guard: Guard =
        serde_json::from_slice(&bytes).map_err(|_| "invalid API fee resolution reader guard")?;
    require(
        guard.schema == 1
            && guard.product == cerulean_domain::PRODUCT_NAME
            && guard.minimum_reader_generation == GENERATION
            && guard.data_schema == status.data_schema
            && guard.source_marker_digest == status.marker_digest
            && guard.source_reader_generation == status.minimum_reader_generation
            && guard.source_reader_guard_digest == status.reader_guard_digest
            && guard.source_reader_generation == crate::currency_api_guard::GENERATION
            && Path::new(&guard.root) == fs::canonicalize(root).map_err(|e| e.to_string())?,
        "API fee resolution reader guard original data or reader changed",
    )?;
    sha256(&guard.source_backup_digest)?;
    timestamp(&guard.promoted_at)?;
    #[cfg(windows)]
    require(
        guard.owner_sid == crate::ipc::current_sid().map_err(|e| e.to_string())?,
        "API fee resolution reader guard belongs to another Windows user",
    )?;
    use sha2::Digest;
    status.minimum_reader_generation = GENERATION;
    status.reader_guard_digest = Some(format!("{:x}", sha2::Sha256::digest(bytes)));
    Ok(())
}
pub(crate) fn require_protected(root: &Path, present: bool) -> Result<(), String> {
    require(
        !present || crate::data_format::status(root)?.minimum_reader_generation >= GENERATION,
        "金额预算需要先停核心、锁定并备份原数据，再明确启用reader12保护",
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
        status.minimum_reader_generation == crate::currency_api_guard::GENERATION,
        "API fee resolution requires original reader11 data; do not repeat promotion",
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
            "API fee resolution reader review changed; no write performed",
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
            ".format-migration-api-fee-resolution-{}-{}.pending",
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
pub(crate) mod tests;
