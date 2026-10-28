//! Reader15 adds background import ownership; reader14 experiment data remains readable. Promotion
//! only appends a reader marker to a cold, byte-identical reviewed backup source.
use crate::data_format::Status;
use cerulean_domain::execution::{digest, require, sha256, timestamp};
use serde::{Deserialize, Serialize};
use std::{fs, io::Read, path::Path};
pub const GENERATION: u32 = 15;
pub const LEGACY_FILE_NAME: &str = "00000000.reader14.json";
pub const FILE_NAME: &str = "00000000.reader15.json";
pub const MARKER: &str = "events/00000000.reader15.json";
const OWNED: &[&str] = &[
    "baseline-experiments",
    "team-material-bindings",
    "team-material-import.json",
    "team-material-import.json.pending",
    "team-observations",
    "team-timelines",
];
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
    observe_generation(root, status, LEGACY_FILE_NAME, 14)?;
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
    require(bytes.len() <= 65536, "experiment reader guard too large")?;
    let g: Guard = serde_json::from_slice(&bytes).map_err(|_| "invalid experiment reader guard")?;
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
        "experiment reader guard original data or reader changed",
    )?;
    sha256(&g.source_backup_digest)?;
    timestamp(&g.promoted_at)?;
    #[cfg(windows)]
    require(
        g.owner_sid == crate::ipc::current_sid().map_err(|e| e.to_string())?,
        "experiment reader guard belongs to another Windows user",
    )?;
    use sha2::Digest;
    status.minimum_reader_generation = generation;
    status.reader_guard_digest = Some(format!("{:x}", sha2::Sha256::digest(bytes)));
    Ok(())
}
pub(crate) fn require_protected(root: &Path) -> Result<(), String> {
    require(
        crate::data_format::status(root)?.minimum_reader_generation >= 14,
        "实验写入需要reader14保护；停止核心并创建原数据冷备份，再审阅启用实验保护。不要重派原任务",
    )
}
pub(crate) fn reject_unprotected(root: &Path) -> Result<(), String> {
    let generation = crate::data_format::status(root)?.minimum_reader_generation;
    for name in [
        "team-import-operation.json",
        "team-import-operation.json.pending",
        "team-import-outcome.json",
        "team-import-outcome.json.pending",
    ] {
        match fs::symlink_metadata(root.join(name)) {
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>(), Err(e)=>return Err(e.to_string()),
            Ok(_) if generation < GENERATION => return Err("Background team import requires reader15; preserve original records and review a cold backup".into()),
            Ok(_)=>(),
        }
    }
    if generation >= 14 {
        return Ok(());
    }
    for name in OWNED {
        match fs::symlink_metadata(root.join(name)){
            Err(e) if e.kind()==std::io::ErrorKind::NotFound=>(),Err(e)=>return Err(e.to_string()),
            Ok(_)=>return Err("Existing experiment data requires reader14; retain original records and explicitly review a cold backup before promotion".into()),
        }
    }
    Ok(())
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
    // Explicit promotion may protect original pre-reader14 experiments, including
    // unresolved ones. It cannot bypass any of the prior reader requirements.
    crate::reader_generation::reject_legacy_unprotected(root)?;
    sha256(manifest)?;
    let s = crate::data_format::status(root)?;
    require(
        s.minimum_reader_generation < GENERATION,
        "experiment protection already present; do not repeat promotion",
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
            "experiment reader review changed; no write performed",
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
            ".format-migration-experiments-{}-{}.pending",
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
mod tests;
