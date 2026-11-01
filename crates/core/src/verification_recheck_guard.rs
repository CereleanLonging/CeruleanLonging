//! Reader18 protects explicit corrected-checker approval and verification evidence.
use crate::data_format::Status;
use cerulean_domain::execution::{digest, require, sha256, timestamp};
use serde::{Deserialize, Serialize};
use std::{fs, io::Read, path::Path};
pub const GENERATION: u32 = 18;
pub const FILE_NAME: &str = "00000000.reader18.json";
pub const MARKER: &str = "events/00000000.reader18.json";
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
    require(
        bytes.len() <= 65536,
        "verification recheck reader guard too large",
    )?;
    let g: Guard =
        serde_json::from_slice(&bytes).map_err(|_| "invalid verification recheck reader guard")?;
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
        "verification recheck reader guard original data or reader changed",
    )?;
    sha256(&g.source_backup_digest)?;
    timestamp(&g.promoted_at)?;
    #[cfg(windows)]
    require(
        g.owner_sid == crate::ipc::current_sid().map_err(|e| e.to_string())?,
        "verification recheck reader guard belongs to another Windows user",
    )?;
    use sha2::Digest;
    status.minimum_reader_generation = generation;
    status.reader_guard_digest = Some(format!("{:x}", sha2::Sha256::digest(bytes)));
    Ok(())
}
pub fn require_protected(root: &Path, present: bool) -> Result<(), String> {
    require(
        !present || crate::data_format::status(root)?.minimum_reader_generation >= GENERATION,
        "检查器复验需要 reader18 保护；停止核心并创建原数据冷备份，再审阅启用复验保护",
    )
}

fn entries(path: &Path, count: &mut usize) -> Result<Vec<std::path::PathBuf>, String> {
    let metadata = match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        other => other.map_err(|e| e.to_string())?,
    };
    require(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "recheck preflight directory is not ordinary",
    )?;
    #[cfg(windows)]
    let _pin = crate::backup::open(path, true, false).map_err(|e| e.to_string())?;
    let mut result = vec![];
    for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
        *count += 1;
        require(*count <= 100_000, "recheck preflight history exceeds limit")?;
        let path = entry.map_err(|e| e.to_string())?.path();
        let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        require(
            !meta.file_type().is_symlink(),
            "recheck preflight reparse refused",
        )?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            require(
                meta.file_attributes() & 0x400 == 0,
                "recheck preflight reparse refused",
            )?;
        }
        result.push(path);
    }
    Ok(result)
}
// Only owned workflow protocol and recheck job storage, never business JSON.
pub(crate) fn record_uses_recheck(value: &serde_json::Value) -> bool {
    value
        .pointer("/body/event/command/kind")
        .is_some_and(|v| v == "approve-verification-recheck")
        || value
            .pointer("/body/event/command/kind")
            .is_some_and(|v| v == "retire-verification-recheck")
        || value
            .pointer("/body/context/recheckDisposition")
            .is_some_and(|v| !v.is_null())
        || value
            .pointer("/body/context/verificationRecheck")
            .is_some_and(|v| !v.is_null())
        || value
            .pointer("/body/context/execution/kind")
            .is_some_and(|v| v == "verification-recheck")
}
pub(crate) fn reject_unprotected(root: &Path) -> Result<(), String> {
    if crate::data_format::status(root)?.minimum_reader_generation >= GENERATION {
        return Ok(());
    }
    match fs::symlink_metadata(root.join("verification-recheck-jobs")) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.to_string()),
        Ok(_) => {
            return Err(
                "Existing recheck jobs have no reader18 protection; preserve original records"
                    .into(),
            )
        }
    }
    let mut count = 0;
    for project in entries(&root.join("workflows"), &mut count)? {
        if !project.is_dir() {
            continue;
        }
        for path in entries(&project.join("events"), &mut count)? {
            if !path
                .extension()
                .is_some_and(|e| e == "json" || e == "pending")
            {
                continue;
            }
            #[cfg(windows)]
            let file = crate::backup::open(&path, false, false).map_err(|e| e.to_string())?;
            #[cfg(not(windows))]
            let file = fs::File::open(&path).map_err(|e| e.to_string())?;
            let mut bytes = vec![];
            file.take(2 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            require(
                bytes.len() <= 2 * 1024 * 1024,
                "recheck preflight record exceeds limit",
            )?;
            let value: serde_json::Value = serde_json::from_slice(&bytes)
                .map_err(|_| "recheck preflight record incomplete; preserve original evidence")?;
            require(!record_uses_recheck(&value),
                "Existing corrected verification has no reader18 protection; preserve original records")?;
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
    #[serde(skip_serializing_if = "Option::is_none")]
    source_hold_digest: Option<String>,
    migrator_version: &'static str,
    original_events_rewritten: bool,
    execution_resumed: bool,
}
#[cfg(windows)]
fn reviewed(
    root: &Path,
    archive: &Path,
    manifest: &str,
    hold: Option<&str>,
) -> Result<PromotionReview, String> {
    // Promotion never repairs existing unprotected verification recheck records or rewrites history.
    crate::reader_generation::reject_unprotected(root)?;
    sha256(manifest)?;
    if let Some(hold) = hold {
        sha256(hold)?;
    }
    let s = crate::data_format::status(root)?;
    require(
        s.minimum_reader_generation < GENERATION,
        "verification recheck protection already present; do not repeat promotion",
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
        source_hold_digest: hold.map(str::to_owned),
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
        reviewed(root, archive, manifest, None)
    })
}
#[cfg(windows)]
pub fn promote(
    root: &Path,
    archive: &Path,
    manifest: &str,
    expected: &str,
) -> Result<Status, String> {
    crate::backup::with_original_cold(root, archive, manifest, |root| {
        promote_original(root, archive, manifest, expected, None)
    })
}
#[cfg(windows)]
pub fn review_held(
    root: &Path,
    archive: &Path,
    manifest: &str,
    hold: &str,
) -> Result<PromotionReview, String> {
    crate::maintenance_interlock::ensure_not_reserved(root)?;
    crate::backup::with_original_held(root, archive, manifest, hold, |root| {
        crate::maintenance_interlock::ensure_not_reserved(root)?;
        reviewed(root, archive, manifest, Some(hold))
    })
}
#[cfg(windows)]
pub fn promote_held(
    root: &Path,
    archive: &Path,
    manifest: &str,
    hold: &str,
    expected: &str,
) -> Result<Status, String> {
    crate::maintenance_interlock::ensure_not_reserved(root)?;
    crate::backup::with_original_held(root, archive, manifest, hold, |root| {
        crate::maintenance_interlock::ensure_not_reserved(root)?;
        promote_original(root, archive, manifest, expected, Some(hold))
    })
}
#[cfg(windows)]
fn promote_original(
    root: &Path,
    archive: &Path,
    manifest: &str,
    expected: &str,
    hold: Option<&str>,
) -> Result<Status, String> {
    use std::io::Write;
    let r = reviewed(root, archive, manifest, hold)?;
    require(
        r.digest == expected,
        "verification recheck reader review changed; no write performed",
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
        ".format-migration-verification-recheck-{}-{}.pending",
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
}

#[cfg(all(test, windows))]
pub(crate) mod tests;
