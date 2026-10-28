//! Cold, byte-preserving backup. Never starts a host or replays its ledger.
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    os::windows::{ffi::OsStrExt, fs::OpenOptionsExt, io::AsRawHandle},
    path::{Path, PathBuf},
};
use windows_sys::Win32::Storage::FileSystem::{
    GetFileInformationByHandle, MoveFileExW, BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_SHARE_READ, FILE_SHARE_WRITE,
};

const MAX_ENTRIES: usize = 16_384;
const MAX_BYTES: u64 = 16 * 1024 * 1024 * 1024;
const MANIFEST_LIMIT: u64 = 16 * 1024 * 1024;
fn invalid(message: &str) -> io::Error {
    io::Error::other(message)
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Entry {
    pub(crate) path: String,
    pub(crate) directory: bool,
    pub(crate) bytes: u64,
    pub(crate) sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Manifest {
    schema: u32,
    version: String,
    created_at: String,
    pub(crate) source_root: PathBuf,
    pub(crate) owner_sid: String,
    pub(crate) entries: Vec<Entry>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub manifest_digest: String,
    source_root: PathBuf,
    entries: usize,
    bytes: u64,
    scope: &'static str,
}
pub(crate) struct Pinned {
    pub(crate) entry: Entry,
    file: File,
}
pub(crate) struct Tree {
    _root: File,
    pub(crate) nodes: Vec<Pinned>,
}

// Pin names against replacement. Data handles also exclude concurrent writers.
pub fn open(path: &Path, directory: bool, lock: bool) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(lock)
        .share_mode(
            FILE_SHARE_READ
                | if directory || lock {
                    FILE_SHARE_WRITE
                } else {
                    0
                },
        )
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || (info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directory
        || (!directory && info.nNumberOfLinks != 1)
    {
        return Err(invalid(
            "backup refuses links, reparse points and invalid file types",
        ));
    }
    if lock {
        if file.metadata()?.len() != 0 {
            return Err(invalid("nonempty writer.lock is not supported"));
        }
        file.try_lock_exclusive()
            .map_err(|_| invalid("core or another writer is active; stop it explicitly first"))?;
    }
    Ok(file)
}
fn digest(file: &mut File) -> io::Result<(u64, String)> {
    file.seek(SeekFrom::Start(0))?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    let mut size = 0;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        size += count as u64;
        if size > MAX_BYTES {
            return Err(invalid("backup size limit exceeded"));
        }
        hash.update(&buffer[..count]);
    }
    Ok((size, format!("{:x}", hash.finalize())))
}
fn safe_name(name: &str) -> bool {
    let base = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    let numbered_device = (base.starts_with("COM") || base.starts_with("LPT"))
        && base.chars().count() == 4
        && base
            .chars()
            .nth(3)
            .is_some_and(|c| "123456789¹²³".contains(c));
    !name.is_empty()
        && !name.ends_with([' ', '.'])
        && !name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
        && !matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$")
        && !numbered_device
}
pub(crate) fn inventory(root: &Path, cold: bool) -> io::Result<Tree> {
    let mut tree = Tree {
        _root: open(root, true, false)?,
        nodes: Vec::new(),
    };
    // Acquire the existing authoritative lock before reading any data; never create it.
    if cold {
        tree.nodes.push(Pinned {
            entry: Entry {
                path: "writer.lock".into(),
                directory: false,
                bytes: 0,
                sha256: format!("{:x}", Sha256::digest([])),
            },
            file: open(&root.join("writer.lock"), false, true)?,
        });
    }
    let mut pending = vec![PathBuf::new()];
    let mut names = BTreeSet::new();
    let mut total = 0;
    while let Some(relative) = pending.pop() {
        for item in fs::read_dir(root.join(&relative))? {
            let item = item?;
            let name = item
                .file_name()
                .into_string()
                .map_err(|_| invalid("non-Unicode filename"))?;
            if !safe_name(&name) {
                return Err(invalid("unsupported filename"));
            }
            let rel = relative.join(&name);
            let path = rel
                .to_str()
                .ok_or_else(|| invalid("invalid path"))?
                .replace('\\', "/");
            if !names.insert(path.to_lowercase()) {
                return Err(invalid("case-colliding paths"));
            }
            if cold && path == "writer.lock" {
                continue;
            }
            let directory = item.file_type()?.is_dir();
            let mut file = open(
                &item.path(),
                directory,
                cold && !directory && name == "writer.lock",
            )?;
            let (bytes, sha256) = if directory {
                (0, String::new())
            } else {
                digest(&mut file)?
            };
            total += bytes;
            if total > MAX_BYTES || tree.nodes.len() >= MAX_ENTRIES {
                return Err(invalid("backup capacity exceeded"));
            }
            tree.nodes.push(Pinned {
                entry: Entry {
                    path,
                    directory,
                    bytes,
                    sha256,
                },
                file,
            });
            if directory {
                pending.push(rel);
            }
        }
    }
    tree.nodes.sort_by(|a, b| a.entry.path.cmp(&b.entry.path));
    Ok(tree)
}
pub(crate) fn copy(tree: &mut Tree, destination: &Path) -> io::Result<()> {
    fs::create_dir(destination)?;
    for node in &mut tree.nodes {
        let path = destination.join(&node.entry.path);
        if node.entry.directory {
            fs::create_dir(&path)?;
        } else {
            let mut output = OpenOptions::new().write(true).create_new(true).open(path)?;
            node.file.seek(SeekFrom::Start(0))?;
            // Windows byte-range locks prohibit reading the locked zero-byte lock region.
            if node.entry.bytes > 0 {
                io::copy(&mut node.file, &mut output)?;
            }
            output.sync_all()?;
        }
    }
    Ok(())
}
fn target(path: &Path) -> io::Result<(PathBuf, File)> {
    let name = path
        .file_name()
        .and_then(|x| x.to_str())
        .filter(|s| safe_name(s))
        .ok_or_else(|| invalid("destination must be a new named directory"))?;
    let parent = fs::canonicalize(
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?;
    let pin = open(&parent, true, false)?;
    let result = parent.join(name);
    if result.try_exists()? {
        return Err(invalid("destination already exists; nothing overwritten"));
    }
    Ok((result, pin))
}
fn stage(target: &Path) -> PathBuf {
    target.with_file_name(format!(
        ".cerulean-pending-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ))
}
pub(crate) fn publish(source: &Path, destination: &Path) -> io::Result<()> {
    let wide = |path: &Path| -> io::Result<Vec<u16>> {
        let mut text: Vec<_> = path.as_os_str().encode_wide().collect();
        if text.contains(&0) {
            return Err(invalid("NUL in publication path"));
        }
        text.push(0);
        Ok(text)
    };
    let source = wide(source)?;
    let destination = wide(destination)?;
    // No MOVEFILE_REPLACE_EXISTING and no cross-volume copy fallback.
    if unsafe { MoveFileExW(source.as_ptr(), destination.as_ptr(), 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
fn review(manifest: &Manifest, bytes: &[u8]) -> Review {
    Review { manifest_digest: format!("{:x}", Sha256::digest(bytes)), source_root: manifest.source_root.clone(), entries: manifest.entries.len(), bytes: manifest.entries.iter().map(|e| e.bytes).sum(), scope: "core-data-only; encrypted credentials; no external projects; no automatic restart; same-user original-location restore" }
}
pub(crate) fn verified(archive: &Path) -> io::Result<(Manifest, Review, Tree, File, File)> {
    let archive_pin = open(archive, true, false)?;
    let children: BTreeSet<_> = fs::read_dir(archive)?
        .map(|e| e.map(|e| e.file_name()))
        .collect::<io::Result<_>>()?;
    if children
        != ["manifest.json".into(), "payload".into()]
            .into_iter()
            .collect()
    {
        return Err(invalid("incomplete backup or unexpected archive entries"));
    }
    let mut manifest_pin = open(&archive.join("manifest.json"), false, false)?;
    let mut bytes = Vec::new();
    (&mut manifest_pin)
        .take(MANIFEST_LIMIT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MANIFEST_LIMIT {
        return Err(invalid("manifest too large"));
    }
    let manifest: Manifest =
        serde_json::from_slice(&bytes).map_err(|_| invalid("invalid manifest"))?;
    if manifest.schema != 1 || !crate::BACKUP_WRITER_VERSIONS.contains(&manifest.version.as_str()) {
        return Err(invalid(
            "unsupported backup format/version; no conversion performed",
        ));
    }
    let tree = inventory(&archive.join("payload"), false)?;
    if !tree
        .nodes
        .iter()
        .map(|n| &n.entry)
        .eq(manifest.entries.iter())
    {
        return Err(invalid("backup inventory/hash mismatch"));
    }
    let result = review(&manifest, &bytes);
    Ok((manifest, result, tree, archive_pin, manifest_pin))
}
pub fn verify(archive: &Path) -> Result<Review, String> {
    verified(archive)
        .map(|(_, r, _, _, _)| r)
        .map_err(|e| e.to_string())
}
pub fn create(source: &Path, destination: &Path) -> Result<Review, String> {
    create_inner(source, destination, None).map_err(|e| e.to_string())
}
pub fn create_held(source: &Path, destination: &Path, hold_digest: &str) -> Result<Review, String> {
    create_inner(source, destination, Some(hold_digest)).map_err(|e| e.to_string())
}
/// Explicit metadata-only promotion of data which may no longer open in an
/// execution host. Pin the cold source, all nested writers and exact backup;
/// no upgrade hold is invented, released, or bypassed by this entry point.
pub(crate) fn with_original_cold<T>(
    source: &Path,
    archive: &Path,
    expected: &str,
    action: impl FnOnce(&Path) -> Result<T, String>,
) -> Result<T, String> {
    crate::backup_recovery::ensure_not_recovering(source)?;
    crate::maintenance_interlock::ensure_not_reserved(source)?;
    crate::upgrade::ensure_not_held(source)?;
    let source = fs::canonicalize(source).map_err(|e| e.to_string())?;
    let (manifest, review, _backup_tree, _archive_pin, _manifest_pin) =
        verified(archive).map_err(|e| e.to_string())?;
    if review.manifest_digest != expected
        || manifest.source_root != source
        || manifest.owner_sid != crate::ipc::current_sid().map_err(|e| e.to_string())?
    {
        return Err("Promotion requires this user's reviewed original-location cold backup".into());
    }
    let original = inventory(&source, true).map_err(|e| e.to_string())?;
    if !original
        .nodes
        .iter()
        .map(|n| &n.entry)
        .eq(manifest.entries.iter())
    {
        return Err("Promotion source differs from the reviewed backup".into());
    }
    crate::backup_recovery::ensure_not_recovering(&source)?;
    crate::maintenance_interlock::ensure_not_reserved(&source)?;
    crate::upgrade::ensure_not_held(&source)?;
    action(&source)
}
/// Run a metadata migration only while the reviewed backup, original data,
/// authoritative writer lock, and every existing data file remain pinned.
pub(crate) fn with_original_held<T>(
    source: &Path,
    archive: &Path,
    expected_manifest: &str,
    expected_hold: &str,
    action: impl FnOnce(&Path) -> Result<T, String>,
) -> Result<T, String> {
    crate::backup_recovery::ensure_not_recovering(source)?;
    let source = fs::canonicalize(source).map_err(|e| e.to_string())?;
    let (manifest, review, _backup_tree, _archive_pin, _manifest_pin) =
        verified(archive).map_err(|e| e.to_string())?;
    if review.manifest_digest != expected_manifest
        || manifest.source_root != source
        || manifest.owner_sid != crate::ipc::current_sid().map_err(|e| e.to_string())?
    {
        return Err(
            "Migration requires the reviewed backup of this user and original location".into(),
        );
    }
    let original = inventory(&source, true).map_err(|e| e.to_string())?;
    if !original
        .nodes
        .iter()
        .map(|n| &n.entry)
        .eq(manifest.entries.iter())
    {
        return Err("Migration source differs from the reviewed backup".into());
    }
    let hold = crate::upgrade::status(&source)?;
    if !hold.valid_record || hold.hold_digest != expected_hold {
        return Err("Migration requires the exact valid upgrade hold".into());
    }
    action(&source)
}
fn create_inner(
    source: &Path,
    destination: &Path,
    hold_digest: Option<&str>,
) -> io::Result<Review> {
    let source = fs::canonicalize(source)?;
    let (destination, _parent) = target(destination)?;
    if destination.starts_with(&source) {
        return Err(invalid("backup must be outside the source data directory"));
    }
    let mut tree = inventory(&source, true)?;
    if let Some(expected) = hold_digest {
        let hold = crate::upgrade::status(&source).map_err(|e| invalid(&e))?;
        if !hold.valid_record || hold.hold_digest != expected {
            return Err(invalid(
                "upgrade hold is invalid or differs from reviewed digest",
            ));
        }
    }
    let temporary = stage(&destination);
    fs::create_dir(&temporary)?;
    copy(&mut tree, &temporary.join("payload"))?;
    let manifest = Manifest {
        schema: 1,
        version: env!("CARGO_PKG_VERSION").into(),
        created_at: chrono::Utc::now().to_rfc3339(),
        source_root: source,
        owner_sid: crate::ipc::current_sid()?,
        entries: tree
            .nodes
            .iter()
            .map(|n| Entry {
                path: n.entry.path.clone(),
                directory: n.entry.directory,
                bytes: n.entry.bytes,
                sha256: n.entry.sha256.clone(),
            })
            .collect(),
    };
    let bytes = serde_json::to_vec_pretty(&manifest)?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temporary.join("manifest.json"))?;
    output.write_all(&bytes)?;
    output.sync_all()?;
    drop(output);
    let result = verify(&temporary).map_err(|e| invalid(&e))?;
    publish(&temporary, &destination)?;
    Ok(result)
}
pub fn restore(archive: &Path, destination: &Path, expected: &str) -> Result<Review, String> {
    crate::backup_recovery::ensure_not_recovering(destination)?;
    restore_inner(archive, destination, expected).map_err(|e| e.to_string())
}
fn restore_inner(archive: &Path, destination: &Path, expected: &str) -> io::Result<Review> {
    let (manifest, result, mut tree, _archive_pin, _manifest_pin) = verified(archive)?;
    let (destination, _parent) = target(destination)?;
    if result.manifest_digest != expected {
        return Err(invalid("reviewed manifest digest does not match"));
    }
    if manifest.owner_sid != crate::ipc::current_sid()? || manifest.source_root != destination {
        return Err(invalid(
            "restore requires the original Windows user and canonical data location",
        ));
    }
    let temporary = stage(&destination);
    copy(&mut tree, &temporary)?;
    let check = inventory(&temporary, false)?;
    if !check
        .nodes
        .iter()
        .map(|n| &n.entry)
        .eq(manifest.entries.iter())
    {
        return Err(invalid("restored inventory mismatch"));
    }
    drop(check);
    publish(&temporary, &destination)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("数据 空格");
        drop(crate::Store::open(&data).unwrap());
        fs::create_dir(data.join("empty")).unwrap();
        fs::write(
            data.join("opaque-journal.json"),
            br#"{"state":"unknown","attempt":"original"}"#,
        )
        .unwrap();
        let archive = root.path().join("backup");
        (root, data, archive)
    }
    fn alter_manifest(archive: &Path, change: impl FnOnce(&mut serde_json::Value)) -> String {
        let path = archive.join("manifest.json");
        let mut value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        change(&mut value);
        let bytes = serde_json::to_vec(&value).unwrap();
        fs::write(path, &bytes).unwrap();
        format!("{:x}", Sha256::digest(bytes))
    }
    #[test]
    fn original_location_restores_approval_unknown_bytes_empty_dirs_and_dpapi() {
        use cerulean_domain::{secret::SecretInput, Command};
        let (root, data, archive) = fixture();
        let mut store = crate::Store::open(&data).unwrap();
        let snapshot = store
            .execute(Command::CreatePlan {
                title: "offline backup fixture".into(),
                tasks: vec!["keep original approval".into()],
            })
            .unwrap();
        let plan = &snapshot.plans[0];
        let snapshot = store
            .execute(Command::ApprovePlan {
                id: plan.id.clone(),
                digest: plan.digest.clone(),
            })
            .unwrap();
        let before = serde_json::to_value(&snapshot.plans).unwrap();
        drop(store);
        crate::credential_vault::save(
            &data,
            &SecretInput::new("synthetic-offline-backup-test-key".into()).unwrap(),
        )
        .unwrap();
        let result = create(&data, &archive).unwrap();
        fs::rename(&data, root.path().join("original-preserved")).unwrap();
        restore(&archive, &data, &result.manifest_digest).unwrap();
        assert!(data.join("empty").is_dir());
        assert_eq!(
            fs::read(data.join("opaque-journal.json")).unwrap(),
            fs::read(root.path().join("original-preserved/opaque-journal.json")).unwrap()
        );
        assert!(crate::credential_vault::load(&data).is_ok());
        let restored = crate::Store::open(&data).unwrap();
        assert_eq!(
            serde_json::to_value(restored.snapshot().plans).unwrap(),
            before
        );
        assert_eq!(
            verify(&archive).unwrap().manifest_digest,
            result.manifest_digest
        );
    }
    #[test]
    fn active_core_and_nested_writer_are_rejected_without_destination() {
        let (_root, data, archive) = fixture();
        let core = crate::Store::open(&data).unwrap();
        assert!(create(&data, &archive).is_err());
        assert!(!archive.exists());
        drop(core);
        let nested = crate::Store::open(&data.join("nested")).unwrap();
        assert!(create(&data, &archive).is_err());
        assert!(!archive.exists());
        drop(nested);
        create(&data, &archive).unwrap();
    }
    #[test]
    fn tampering_missing_extra_and_incomplete_archives_fail() {
        for mode in 0..4 {
            let (_root, data, archive) = fixture();
            create(&data, &archive).unwrap();
            match mode {
                0 => fs::write(archive.join("payload/opaque-journal.json"), b"tampered").unwrap(),
                1 => fs::remove_file(archive.join("payload/opaque-journal.json")).unwrap(),
                2 => fs::write(archive.join("payload/extra"), b"extra").unwrap(),
                _ => fs::remove_file(archive.join("manifest.json")).unwrap(),
            }
            assert!(verify(&archive).is_err());
        }
    }
    #[test]
    fn untrusted_manifest_paths_duplicates_and_schema_never_escape() {
        for mode in 0..3 {
            let (root, data, archive) = fixture();
            create(&data, &archive).unwrap();
            alter_manifest(&archive, |v| match mode {
                0 => v["entries"][0]["path"] = "../../outside".into(),
                1 => {
                    let entry = v["entries"][0].clone();
                    v["entries"].as_array_mut().unwrap().push(entry);
                }
                _ => v["schema"] = 99.into(),
            });
            assert!(verify(&archive).is_err());
            assert!(!root.path().join("outside").exists());
        }
    }
    #[test]
    fn existing_destination_wrong_root_sid_and_digest_do_not_restore() {
        let (root, data, archive) = fixture();
        let result = create(&data, &archive).unwrap();
        assert!(restore(&archive, &data, &result.manifest_digest).is_err());
        let other = root.path().join("other");
        assert!(restore(&archive, &other, &result.manifest_digest).is_err());
        fs::rename(&data, root.path().join("preserved")).unwrap();
        assert!(restore(&archive, &data, &"0".repeat(64)).is_err());
        let changed = alter_manifest(&archive, |v| v["ownerSid"] = "S-1-0-0".into());
        assert!(restore(&archive, &data, &changed).is_err());
        assert!(!data.exists());
        assert!(!other.exists());
    }
    #[test]
    fn hard_links_and_nested_destination_are_rejected() {
        let (root, data, archive) = fixture();
        assert!(create(&data, &data.join("nested-backup")).is_err());
        fs::hard_link(
            data.join("opaque-journal.json"),
            root.path().join("external-link"),
        )
        .unwrap();
        assert!(create(&data, &archive).is_err());
        assert!(!archive.exists());
    }
    #[test]
    fn pinned_inventory_excludes_file_writes_and_replacements() {
        let (_root, data, _archive) = fixture();
        let tree = inventory(&data, true).unwrap();
        assert!(fs::write(data.join("opaque-journal.json"), b"changed").is_err());
        assert!(fs::rename(data.join("empty"), data.join("moved")).is_err());
        drop(tree);
        fs::write(data.join("opaque-journal.json"), b"allowed after release").unwrap();
    }
    #[test]
    fn publication_refuses_existing_directory_and_preserves_both() {
        let root = tempfile::tempdir().unwrap();
        let pending = root.path().join("pending");
        let target = root.path().join("target");
        fs::create_dir(&pending).unwrap();
        fs::write(pending.join("evidence"), "preserved").unwrap();
        fs::create_dir(&target).unwrap();
        assert!(publish(&pending, &target).is_err());
        assert!(pending.join("evidence").exists());
        assert!(target.is_dir());
    }
}
