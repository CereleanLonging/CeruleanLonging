//! Explicit data-container generation. The guard is deliberately an unknown
//! legacy event, so pre-guard readers fail before constructing an execution host.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path};

pub const MARKER: &str = "events/00000000.json";
const PENDING_PREFIX: &str = ".format-migration-";
pub const READABLE_SCHEMAS: &[u32] = &[1, 2];

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Marker {
    schema: u32,
    product: String,
    data_schema: u32,
    minimum_reader_generation: u32,
    event_schema: u32,
    source_schema: u32,
    source_backup_digest: String,
    root: String,
    owner_sid: String,
    migrated_at: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub data_schema: u32,
    pub minimum_reader_generation: u32,
    pub reader_guard_digest: Option<String>,
    pub marker_digest: Option<String>,
    pub source_backup_digest: Option<String>,
    pub automatic_migration: bool,
}
fn hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
pub fn status(root: &Path) -> Result<Status, String> {
    let mut status = container_status(root)?;
    crate::reader_generation::observe(root, &mut status)?;
    Ok(status)
}
fn container_status(root: &Path) -> Result<Status, String> {
    let events = root.join("events");
    match fs::read_dir(&events) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry.map_err(|e| e.to_string())?;
                if entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(PENDING_PREFIX)
                {
                    return Err("Data migration is incomplete; retain the hold and inspect the original backup".into());
                }
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }
    let path = root.join(MARKER);
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Status {
                data_schema: 1,
                minimum_reader_generation: 1,
                reader_guard_digest: None,
                marker_digest: None,
                source_backup_digest: None,
                automatic_migration: false,
            });
        }
        Err(e) => return Err(e.to_string()),
        Ok(_) => {}
    }
    let canonical = fs::canonicalize(root).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    let mut file = {
        let _root = crate::backup::open(root, true, false).map_err(|e| e.to_string())?;
        let _events = crate::backup::open(&events, true, false).map_err(|e| e.to_string())?;
        crate::backup::open(&path, false, false).map_err(|e| e.to_string())?
    };
    #[cfg(not(windows))]
    let mut file = fs::File::open(&path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(65537)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 65536 {
        return Err("Data format guard exceeds the supported limit".into());
    }
    let record: Marker =
        serde_json::from_slice(&bytes).map_err(|_| "Invalid data format guard; no data loaded")?;
    if record.schema != 1
        || record.product != cerulean_domain::PRODUCT_NAME
        || record.data_schema != 2
        || record.minimum_reader_generation != 2
        || record.event_schema != 1
        || record.source_schema != 1
        || !hex(&record.source_backup_digest)
        || Path::new(&record.root) != canonical
        || chrono::DateTime::parse_from_rfc3339(&record.migrated_at).is_err()
    {
        return Err(
            "Unsupported data format or different original location; no data loaded".into(),
        );
    }
    #[cfg(windows)]
    if record.owner_sid != crate::ipc::current_sid().map_err(|e| e.to_string())? {
        return Err("Data format belongs to another Windows user".into());
    }
    Ok(Status {
        data_schema: 2,
        minimum_reader_generation: 2,
        reader_guard_digest: None,
        marker_digest: Some(format!("{:x}", Sha256::digest(bytes))),
        source_backup_digest: Some(record.source_backup_digest),
        automatic_migration: false,
    })
}

#[cfg(windows)]
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationReview {
    pub digest: String,
    source_root: String,
    backup: String,
    backup_digest: String,
    hold_digest: String,
    from_schema: u32,
    to_schema: u32,
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
) -> Result<MigrationReview, String> {
    let current = status(root)?;
    if !hex(manifest)
        || !hex(hold)
        || current.data_schema != 1
        || current.reader_guard_digest.is_some()
    {
        return Err(
            "Only an unmigrated schema-1 directory with explicit reviewed digests is eligible"
                .into(),
        );
    }
    let mut value = MigrationReview {
        digest: String::new(),
        source_root: root.to_string_lossy().into_owned(),
        backup: fs::canonicalize(archive)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .into_owned(),
        backup_digest: manifest.into(),
        hold_digest: hold.into(),
        from_schema: 1,
        to_schema: 2,
        migrator_version: env!("CARGO_PKG_VERSION"),
        original_events_rewritten: false,
        hold_released: false,
    };
    value.digest = cerulean_domain::execution::digest(&value)?;
    Ok(value)
}
#[cfg(windows)]
pub fn review(
    root: &Path,
    archive: &Path,
    manifest: &str,
    hold: &str,
) -> Result<MigrationReview, String> {
    crate::backup::with_original_held(root, archive, manifest, hold, |root| {
        reviewed(root, archive, manifest, hold)
    })
}
#[cfg(windows)]
pub fn migrate(
    root: &Path,
    archive: &Path,
    manifest: &str,
    hold: &str,
    expected: &str,
) -> Result<Status, String> {
    use std::io::Write;
    crate::backup::with_original_held(root, archive, manifest, hold, |root| {
        let review = reviewed(root, archive, manifest, hold)?;
        if review.digest != expected {
            return Err("Migration review changed; no migration performed".into());
        }
        let record = Marker {
            schema: 1,
            product: cerulean_domain::PRODUCT_NAME.into(),
            data_schema: 2,
            minimum_reader_generation: 2,
            event_schema: 1,
            source_schema: 1,
            source_backup_digest: manifest.into(),
            root: root.to_string_lossy().into_owned(),
            owner_sid: crate::ipc::current_sid().map_err(|e| e.to_string())?,
            migrated_at: chrono::Utc::now().to_rfc3339(),
        };
        let events = root.join("events");
        let _events = crate::backup::open(&events, true, false).map_err(|e| e.to_string())?;
        let temporary = events.join(format!(
            "{PENDING_PREFIX}{}-{}.pending",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        #[cfg(test)]
        tests::interrupt_checkpoint("before-create");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        #[cfg(test)]
        tests::interrupt_checkpoint("pending-created");
        let bytes = serde_json::to_vec_pretty(&record).map_err(|e| e.to_string())?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        #[cfg(test)]
        tests::interrupt_checkpoint("pending-synced");
        drop(file);
        crate::backup::publish(&temporary, &root.join(MARKER)).map_err(|e| e.to_string())?;
        #[cfg(test)]
        tests::interrupt_checkpoint("marker-published");
        status(root)
    })
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::{backup, upgrade, Store};
    // Only the ignored, explicitly launched test child sets this environment.
    // These checkpoints and configuration readers do not exist in product builds.
    pub(super) fn interrupt_checkpoint(phase: &str) {
        use std::io::Write;
        let Ok(config) = std::env::var("CERULEAN_MIGRATION_FAULT_CONFIG") else {
            return;
        };
        let config: serde_json::Value = serde_json::from_slice(&fs::read(config).unwrap()).unwrap();
        if config["phase"].as_str() != Some(phase) {
            return;
        }
        let checkpoint = Path::new(config["checkpoint"].as_str().unwrap());
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(checkpoint)
            .unwrap();
        let receipt = serde_json::json!({
            "phase": phase, "pid": std::process::id(), "token": config["token"],
            "testOnlyCheckpoint": true
        });
        output
            .write_all(&serde_json::to_vec(&receipt).unwrap())
            .unwrap();
        output.sync_all().unwrap();
        drop(output);
        loop {
            if let Some(resume) = config["resume"].as_str() {
                if fs::read_to_string(resume).ok().as_deref() == config["token"].as_str() {
                    return;
                }
            }
            std::thread::park_timeout(std::time::Duration::from_millis(250));
        }
    }
    #[test]
    #[ignore = "Explicit subprocess file-lock driver; requires an isolated reviewed backup"]
    fn migration_io_failure_child() {
        let config = std::env::var("CERULEAN_MIGRATION_FAULT_CONFIG").unwrap();
        let config: serde_json::Value = serde_json::from_slice(&fs::read(config).unwrap()).unwrap();
        assert!(matches!(
            config["phase"].as_str().unwrap(),
            "pending-created" | "pending-synced"
        ));
        let error = migrate(
            Path::new(config["data"].as_str().unwrap()),
            Path::new(config["backup"].as_str().unwrap()),
            config["manifestDigest"].as_str().unwrap(),
            config["holdDigest"].as_str().unwrap(),
            config["reviewDigest"].as_str().unwrap(),
        )
        .expect_err("The external OS file lock must prevent migration");
        use std::io::Write;
        let mut result = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(config["outcome"].as_str().unwrap())
            .unwrap();
        result.write_all(error.as_bytes()).unwrap();
        result.sync_all().unwrap();
    }
    #[test]
    #[ignore = "Explicit subprocess fault driver; requires an isolated reviewed backup"]
    fn migration_interrupt_child() {
        let config = std::env::var("CERULEAN_MIGRATION_FAULT_CONFIG").unwrap();
        let config: serde_json::Value = serde_json::from_slice(&fs::read(config).unwrap()).unwrap();
        assert!(matches!(
            config["phase"].as_str().unwrap(),
            "before-create" | "pending-created" | "pending-synced" | "marker-published"
        ));
        migrate(
            Path::new(config["data"].as_str().unwrap()),
            Path::new(config["backup"].as_str().unwrap()),
            config["manifestDigest"].as_str().unwrap(),
            config["holdDigest"].as_str().unwrap(),
            config["reviewDigest"].as_str().unwrap(),
        )
        .unwrap();
        panic!("Requested interruption checkpoint was not reached");
    }
    fn fixture() -> (
        tempfile::TempDir,
        std::path::PathBuf,
        std::path::PathBuf,
        String,
        String,
    ) {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("data");
        drop(Store::open(&root).unwrap());
        let root = fs::canonicalize(root).unwrap();
        let mut review = upgrade::UpgradeReview {
            digest: String::new(),
            ready: true,
            blockers: vec![],
            snapshot_digest: "a".repeat(64),
            core_version: env!("CARGO_PKG_VERSION").into(),
            core_sha256: "b".repeat(64),
            server_pid: std::process::id(),
        };
        review.digest = cerulean_domain::execution::digest(&review).unwrap();
        let hold = upgrade::prepare(&root, review).unwrap().hold_digest;
        let archive = temp.path().join("backup");
        let manifest = backup::create_held(&root, &archive, &hold)
            .unwrap()
            .manifest_digest;
        (temp, root, archive, manifest, hold)
    }
    #[test]
    fn migration_is_explicit_backup_bound_and_preserves_original_bytes() {
        let (_temp, root, archive, manifest, hold) = fixture();
        let original = fs::read(root.join("upgrade-hold.json")).unwrap();
        let r = review(&root, &archive, &manifest, &hold).unwrap();
        assert_eq!(status(&root).unwrap().data_schema, 1);
        assert!(migrate(&root, &archive, &manifest, &hold, &"0".repeat(64)).is_err());
        assert!(!root.join(MARKER).exists());
        let migrated = migrate(&root, &archive, &manifest, &hold, &r.digest).unwrap();
        assert_eq!(migrated.data_schema, 2);
        assert_eq!(fs::read(root.join("upgrade-hold.json")).unwrap(), original);
        assert!(migrate(&root, &archive, &manifest, &hold, &r.digest).is_err());
        assert!(Store::open(&root).is_err());
        upgrade::release(&root, &hold).unwrap();
        drop(Store::open(&root).unwrap());
    }
    #[test]
    fn changed_data_backup_identity_and_active_writer_prevent_migration() {
        let (_temp, root, archive, manifest, hold) = fixture();
        assert!(review(&root, &archive, &"0".repeat(64), &hold).is_err());
        assert!(review(&root, &archive, &manifest, &"0".repeat(64)).is_err());
        let lock = backup::open(&root.join("writer.lock"), false, true).unwrap();
        assert!(review(&root, &archive, &manifest, &hold).is_err());
        drop(lock);
        fs::write(root.join("unexpected"), b"preserve").unwrap();
        assert!(review(&root, &archive, &manifest, &hold).is_err());
        assert!(!root.join(MARKER).exists());
    }
    #[test]
    fn unknown_invalid_or_partial_format_blocks_store_before_loading() {
        let (_temp, root, archive, manifest, hold) = fixture();
        let r = review(&root, &archive, &manifest, &hold).unwrap();
        migrate(&root, &archive, &manifest, &hold, &r.digest).unwrap();
        upgrade::release(&root, &hold).unwrap();
        let original = fs::read(root.join(MARKER)).unwrap();
        for field in [
            "dataSchema",
            "minimumReaderGeneration",
            "eventSchema",
            "schema",
        ] {
            let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
            value[field] = serde_json::json!(99);
            fs::write(root.join(MARKER), serde_json::to_vec(&value).unwrap()).unwrap();
            assert!(Store::open(&root).is_err());
        }
        fs::write(root.join(MARKER), b"{").unwrap();
        assert!(Store::open(&root).is_err());
        fs::write(root.join(MARKER), original).unwrap();
        fs::write(
            root.join("events/.format-migration-interrupted.pending"),
            b"partial",
        )
        .unwrap();
        assert!(Store::open(&root).is_err());
    }
}
