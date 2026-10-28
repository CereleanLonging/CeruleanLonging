//! Explicit same-user, same-location recovery. A sibling guard survives the
//! interval where the original data directory is absent. Never starts a host.
use crate::backup::{self, Entry, Tree};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

type Result<T> = std::result::Result<T, String>;
fn io<T>(value: std::io::Result<T>) -> Result<T> {
    value.map_err(|e| e.to_string())
}
fn digest<T: Serialize>(value: &T) -> Result<String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).map_err(|e| e.to_string())?)
    ))
}
pub(crate) fn location(root: &Path) -> Result<PathBuf> {
    if io(root.try_exists())? {
        return io(fs::canonicalize(root));
    }
    let name = root
        .file_name()
        .ok_or("Recovery requires a named data directory")?;
    let parent = root
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    Ok(io(fs::canonicalize(parent))?.join(name))
}
pub(crate) fn guard(root: &Path) -> Result<PathBuf> {
    let root = location(root)?;
    let name = digest(&root.to_string_lossy().to_lowercase())?;
    Ok(root
        .parent()
        .ok_or("Data directory must have a parent")?
        .join(format!(".cerulean-recovery-{name}")))
}
pub(crate) fn ensure_not_recovering(root: &Path) -> Result<()> {
    crate::maintenance_interlock::ensure_not_reserved(root)?;
    ensure_no_recovery(root)
}
pub(crate) fn ensure_no_recovery(root: &Path) -> Result<()> {
    // A brand-new directory with no existing parent cannot have a recovery guard.
    if !io(root.try_exists())? {
        let parent = root
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        if !io(parent.try_exists())? {
            return Ok(());
        }
    }
    match fs::symlink_metadata(guard(root)?) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err("Data recovery is pending; inspect the original recovery transaction before startup or hold release".into()),
    }
}
fn entries(tree: &Tree) -> Vec<Entry> {
    tree.nodes.iter().map(|n| n.entry.clone()).collect()
}
fn matches(tree: &Tree, expected: &[Entry]) -> bool {
    tree.nodes.iter().map(|n| &n.entry).eq(expected.iter())
}
fn write_new(path: &Path, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("Recovery record exceeds limit".into());
    }
    let mut file = io(OpenOptions::new().write(true).create_new(true).open(path))?;
    #[cfg(test)]
    if path.file_name().is_some_and(|n| n == "plan.json")
        && path
            .parent()
            .is_some_and(|p| p.to_string_lossy().contains(".initializing-"))
    {
        tests::prepare_checkpoint("metadata-file-created");
    }
    io(file.write_all(&bytes).and_then(|_| file.sync_all()))
}
fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<(T, File)> {
    let mut pin = io(backup::open(path, false, false))?;
    let mut bytes = Vec::new();
    io((&mut pin)
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes))?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("Recovery record exceeds limit".into());
    }
    Ok((
        serde_json::from_slice(&bytes).map_err(|e| e.to_string())?,
        pin,
    ))
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PreviousHold {
    digest: String,
    released_record: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Review {
    pub digest: String,
    schema: u32,
    version: String,
    source_root: PathBuf,
    archive: PathBuf,
    transaction: PathBuf,
    manifest_digest: String,
    hold_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous_hold: Option<PreviousHold>,
    owner_sid: String,
    original_entries: Vec<Entry>,
    automatic_restart: bool,
}
struct Source {
    review: Review,
    original: Tree,
    backup: Tree,
    _archive: File,
    _manifest: File,
    _parent: File,
}
fn source(root: &Path, archive: &Path, manifest_digest: &str, hold_digest: &str) -> Result<Source> {
    source_with_previous(root, archive, manifest_digest, hold_digest, None)
}
fn source_with_previous(
    root: &Path,
    archive: &Path,
    manifest_digest: &str,
    hold_digest: &str,
    previous: Option<&str>,
) -> Result<Source> {
    let root = io(fs::canonicalize(root))?;
    let archive = io(fs::canonicalize(archive))?;
    let transaction = guard(&root)?;
    ensure_not_recovering(&root)?;
    let parent = io(backup::open(
        root.parent().ok_or("Missing parent")?,
        true,
        false,
    ))?;
    if archive.starts_with(&root) || root.starts_with(&archive) || archive.starts_with(&transaction)
    {
        return Err("Recovery backup and transaction must be outside original data".into());
    }
    let (manifest, verified, backup, archive_pin, manifest_pin) = io(backup::verified(&archive))?;
    let owner = io(crate::ipc::current_sid())?;
    if verified.manifest_digest != manifest_digest
        || manifest.source_root != root
        || manifest.owner_sid != owner
    {
        return Err(
            "Recovery requires the reviewed backup, original location and Windows user".into(),
        );
    }
    let original = io(backup::inventory(&root, true))?;
    ensure_not_recovering(&root)?;
    let hold = crate::upgrade::status(&root)?;
    let restore_hold = previous.unwrap_or(hold_digest);
    if !hold.valid_record
        || hold.hold_digest != hold_digest
        || !manifest
            .entries
            .iter()
            .any(|e| e.path == "upgrade-hold.json" && e.sha256 == restore_hold)
    {
        return Err(
            "Current hold or reviewed backup hold differs; use the explicit previous-backup review when holds differ".into(),
        );
    }
    let previous_hold = if let Some(previous) = previous {
        if previous == hold_digest || !valid_digest(previous) {
            return Err("Previous-backup recovery requires distinct complete current and backup hold digests".into());
        }
        let prefix = format!("upgrade-holds/released-{previous}-");
        let released = original
            .nodes
            .iter()
            .map(|n| &n.entry)
            .find(|entry| {
                entry.path.starts_with(&prefix)
                    && entry.path.ends_with(".json")
                    && entry.sha256 == previous
            })
            .ok_or(
                "Original release evidence for the backup hold is missing; do not recreate it",
            )?;
        let observed = crate::upgrade::inspect_preserved(&root, &root.join(&released.path))?;
        if !observed.valid_record || observed.hold_digest != previous {
            return Err("Preserved release hold is invalid for this original location/user".into());
        }
        Some(PreviousHold {
            digest: previous.into(),
            released_record: released.path.clone(),
        })
    } else {
        None
    };
    verify_backup_hold(&root, &archive, restore_hold)?;
    let mut review = Review {
        digest: String::new(),
        schema: if previous_hold.is_some() { 2 } else { 1 },
        version: env!("CARGO_PKG_VERSION").into(),
        source_root: root,
        archive,
        transaction,
        manifest_digest: manifest_digest.into(),
        hold_digest: hold_digest.into(),
        previous_hold,
        owner_sid: owner,
        original_entries: entries(&original),
        automatic_restart: false,
    };
    review.digest = digest(&review)?;
    Ok(Source {
        review,
        original,
        backup,
        _archive: archive_pin,
        _manifest: manifest_pin,
        _parent: parent,
    })
}
pub fn review(root: &Path, archive: &Path, manifest: &str, hold: &str) -> Result<Review> {
    Ok(source(root, archive, manifest, hold)?.review)
}
pub fn review_previous(
    root: &Path,
    archive: &Path,
    manifest: &str,
    current_hold: &str,
    backup_hold: &str,
) -> Result<Review> {
    Ok(source_with_previous(root, archive, manifest, current_hold, Some(backup_hold))?.review)
}
fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn verify_backup_hold(root: &Path, archive: &Path, expected: &str) -> Result<()> {
    let hold = crate::upgrade::inspect_preserved(root, &archive.join("payload/upgrade-hold.json"))?;
    if !hold.valid_record || hold.hold_digest != expected {
        return Err(
            "Backup hold does not prove the original location/user and reviewed quiescence".into(),
        );
    }
    Ok(())
}
impl Review {
    fn restored_hold(&self) -> &str {
        self.previous_hold
            .as_ref()
            .map_or(&self.hold_digest, |p| &p.digest)
    }
}
pub fn prepare(
    root: &Path,
    archive: &Path,
    manifest: &str,
    hold: &str,
    expected: &str,
) -> Result<Status> {
    prepare_source(root, source(root, archive, manifest, hold)?, expected)
}
pub fn prepare_previous(
    root: &Path,
    archive: &Path,
    manifest: &str,
    current_hold: &str,
    backup_hold: &str,
    expected: &str,
) -> Result<Status> {
    prepare_source(
        root,
        source_with_previous(root, archive, manifest, current_hold, Some(backup_hold))?,
        expected,
    )
}
fn prepare_source(root: &Path, mut context: Source, expected: &str) -> Result<Status> {
    if context.review.digest != expected {
        return Err("Recovery review changed; no preparation performed".into());
    }
    let transaction = &context.review.transaction;
    // Publish a usable plan and operation lock together. An interruption before
    // this rename leaves evidence outside the startup guard and original hold.
    let initializing = transaction.with_file_name(format!(
        "{}.initializing-{}-{}",
        transaction
            .file_name()
            .ok_or("Missing guard name")?
            .to_string_lossy(),
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    io(fs::create_dir(&initializing))?;
    write_new(&initializing.join("plan.json"), &context.review).map_err(|e| {
        format!(
            "Preparation metadata incomplete at {}; retain it: {e}",
            initializing.display()
        )
    })?;
    let operation = io(OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(initializing.join("operation.lock")))?;
    io(operation.sync_all())?;
    drop(operation);
    io(backup::publish(&initializing, transaction))?;
    #[cfg(test)]
    tests::prepare_checkpoint("guard-published");
    // Any incomplete payload or prepared receipt now has a valid reviewed plan
    // and can be explicitly abandoned while retaining its original bytes.
    let _operation = io(backup::open(
        &transaction.join("operation.lock"),
        false,
        true,
    ))?;
    io(backup::copy(
        &mut context.backup,
        &transaction.join("staged"),
    ))?;
    let check = io(backup::inventory(&transaction.join("staged"), false))?;
    if !matches(&check, &entries(&context.backup))
        || !matches(&context.original, &context.review.original_entries)
    {
        return Err("Recovery staging inventory mismatch; original data retained".into());
    }
    #[cfg(test)]
    tests::prepare_checkpoint("payload-copied");
    write_new(&transaction.join("prepared.json"), &context.review.digest)?;
    drop(check);
    drop(_operation);
    drop(context);
    status(root)
}

struct Transaction {
    plan: Review,
    expected: Vec<Entry>,
    _backup: Tree,
    _archive: File,
    _manifest: File,
    _parent: File,
    _directory: File,
    _plan: File,
    _operation: File,
}
fn validate_plan(
    plan: &mut Review,
    root: &Path,
    directory: &Path,
    expected: Option<&str>,
) -> Result<()> {
    let recorded_digest = std::mem::take(&mut plan.digest);
    let actual_digest = digest(&plan)?;
    plan.digest = recorded_digest;
    if !matches!((plan.schema, &plan.previous_hold), (1, None) | (2, Some(_)))
        || plan.automatic_restart
        || plan.source_root != root
        || plan.transaction != directory
        || plan.owner_sid != io(crate::ipc::current_sid())?
        || plan.digest != actual_digest
        || expected.is_some_and(|d| d != plan.digest)
    {
        return Err("Recovery transaction identity or review digest does not match".into());
    }
    if let Some(previous) = &plan.previous_hold {
        let prefix = format!("upgrade-holds/released-{}-", previous.digest);
        if !valid_digest(&previous.digest)
            || previous.digest == plan.hold_digest
            || !previous.released_record.starts_with(&prefix)
            || !previous.released_record.ends_with(".json")
            || previous.released_record[prefix.len()..].contains(['/', '\\'])
            || !plan
                .original_entries
                .iter()
                .any(|e| e.path == "upgrade-hold.json" && e.sha256 == plan.hold_digest)
            || !plan
                .original_entries
                .iter()
                .any(|e| e.path == previous.released_record && e.sha256 == previous.digest)
        {
            return Err("Previous backup release/current hold binding is inconsistent".into());
        }
    }
    Ok(())
}
fn transaction(root: &Path, expected: Option<&str>) -> Result<Transaction> {
    let root = location(root)?;
    let directory = guard(&root)?;
    let parent = io(backup::open(
        root.parent().ok_or("Missing parent")?,
        true,
        false,
    ))?;
    let pin = io(backup::open(&directory, true, false))?;
    let operation = io(backup::open(&directory.join("operation.lock"), false, true))?;
    let (mut plan, plan_pin): (Review, File) = read_json(&directory.join("plan.json"))?;
    validate_plan(&mut plan, &root, &directory, expected)?;
    let (manifest, review, backup, archive, manifest_pin) = io(backup::verified(&plan.archive))?;
    if manifest.source_root != root
        || manifest.owner_sid != plan.owner_sid
        || review.manifest_digest != plan.manifest_digest
        || !manifest
            .entries
            .iter()
            .any(|e| e.path == "upgrade-hold.json" && e.sha256 == plan.restored_hold())
    {
        return Err("Original recovery backup changed".into());
    }
    verify_backup_hold(&root, &plan.archive, plan.restored_hold())?;
    Ok(Transaction {
        plan,
        expected: manifest.entries,
        _backup: backup,
        _archive: archive,
        _manifest: manifest_pin,
        _parent: parent,
        _directory: pin,
        _plan: plan_pin,
        _operation: operation,
    })
}
fn tree_if_exists(path: &Path, cold: bool) -> Result<Option<Tree>> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
        Ok(_) => io(backup::inventory(path, cold)).map(Some),
    }
}
fn receipt(transaction: &Transaction, name: &str) -> Result<bool> {
    let path = transaction.plan.transaction.join(name);
    if !io(path.try_exists())? {
        return Ok(false);
    }
    let (value, _pin): (String, File) = read_json(&path)?;
    if value != transaction.plan.digest {
        return Err("Recovery receipt digest differs".into());
    }
    Ok(true)
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub review_digest: String,
    pub original_hold_digest: String,
    pub restored_hold_digest: String,
    pub original_state: String,
    pub preserved_original: bool,
    pub staged_backup: bool,
    pub move_intent: bool,
    pub restore_intent: bool,
    pub restoration_complete: bool,
    pub startup_blocked: bool,
    pub automatic_restart: bool,
}
fn observe(tx: &Transaction) -> Result<Status> {
    let original = tree_if_exists(&tx.plan.source_root, true)?;
    let preserved = tree_if_exists(&tx.plan.transaction.join("preserved"), true)?;
    let staged = tree_if_exists(&tx.plan.transaction.join("staged"), false)?;
    let preserved_original = preserved
        .as_ref()
        .is_some_and(|t| matches(t, &tx.plan.original_entries));
    let staged_backup = staged.as_ref().is_some_and(|t| matches(t, &tx.expected));
    let original_state = match original.as_ref() {
        None => "absent",
        Some(t) if preserved_original && matches(t, &tx.expected) => "restored",
        Some(t) if matches(t, &tx.plan.original_entries) => "original",
        Some(_) => "changed",
    };
    let move_intent = receipt(tx, "move-intent.json")?;
    let restore_intent = receipt(tx, "restore-intent.json")?;
    Ok(Status {
        review_digest: tx.plan.digest.clone(),
        original_hold_digest: tx.plan.hold_digest.clone(),
        restored_hold_digest: tx.plan.restored_hold().into(),
        original_state: original_state.into(),
        preserved_original,
        staged_backup,
        move_intent,
        restore_intent,
        restoration_complete: original_state == "restored"
            && staged.is_none()
            && move_intent
            && restore_intent,
        startup_blocked: true,
        automatic_restart: false,
    })
}
pub fn status(root: &Path) -> Result<Status> {
    observe(&transaction(root, None)?)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnosis {
    pub guard_present: bool,
    pub active: Option<Status>,
    pub issue: Option<String>,
    pub prepared_record: String,
    pub can_abandon_preparation: bool,
    pub unpublished_metadata: Vec<PathBuf>,
    pub next_action: String,
    pub automatic_actions: bool,
}
/// Guidance without deleting partial evidence, releasing holds, or inferring
/// that a process has stopped from a stale file or elapsed time.
pub fn diagnose(root: &Path) -> Result<Diagnosis> {
    let root = location(root)?;
    let directory = guard(&root)?;
    let parent = root.parent().ok_or("Missing parent")?;
    let _parent = io(backup::open(parent, true, false))?;
    let prefix = format!(
        "{}.initializing-",
        directory
            .file_name()
            .ok_or("Missing guard name")?
            .to_string_lossy()
    );
    let mut unpublished_metadata = Vec::new();
    for (count, item) in io(fs::read_dir(parent))?.enumerate() {
        if count >= 4096 {
            return Err("Recovery parent inventory exceeds the inspection limit".into());
        }
        let item = io(item)?;
        if item.file_name().to_string_lossy().starts_with(&prefix) {
            unpublished_metadata.push(item.path());
        }
    }
    unpublished_metadata.sort();
    let guard_present = match fs::symlink_metadata(&directory) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(e.to_string()),
        Ok(_) => true,
    };
    let mut result = Diagnosis {
        guard_present,
        active: None,
        issue: None,
        prepared_record: "unknown".into(),
        can_abandon_preparation: false,
        unpublished_metadata,
        next_action: "review-original-evidence".into(),
        automatic_actions: false,
    };
    if let Err(issue) = crate::maintenance_interlock::ensure_not_reserved(&root) {
        result.issue = Some(issue);
        result.next_action = "review-original-program-update".into();
        return Ok(result);
    }
    if !guard_present {
        let held = (|| -> Result<()> {
            let _writer = io(backup::open(&root.join("writer.lock"), false, true))?;
            if !crate::upgrade::status(&root)?.valid_record {
                return Err("Original maintenance hold is invalid".into());
            }
            Ok(())
        })();
        match held {
            Ok(()) => result.next_action = "review-new-preparation-or-explicit-hold-release".into(),
            Err(issue) => result.issue = Some(issue),
        }
        return Ok(result);
    }
    let inspected = (|| -> Result<()> {
        let tx = transaction(&root, None)?;
        let observed = observe(&tx)?;
        result.prepared_record = match receipt(&tx, "prepared.json") {
            Ok(true) => "valid",
            Ok(false) => "missing",
            Err(_) => "invalid",
        }
        .into();
        let finalization_present = io(directory.join("finalization.json").try_exists())?;
        let can_abandon = observed.original_state == "original"
            && !observed.move_intent
            && !observed.restore_intent
            && !io(directory.join("preserved").try_exists())?
            && !finalization_present;
        if can_abandon {
            record_files(&directory, 2)?;
        }
        result.can_abandon_preparation = can_abandon;
        result.next_action = if finalization_present {
            "inspect-original-finalization"
        } else if observed.restoration_complete {
            "review-and-finish"
        } else if can_abandon && (result.prepared_record != "valid" || !observed.staged_backup) {
            "review-and-abandon-preparation"
        } else if observed.staged_backup && result.prepared_record == "valid" {
            "review-and-apply-or-continue"
        } else {
            "review-original-evidence"
        }
        .into();
        result.active = Some(observed);
        Ok(())
    })();
    if let Err(issue) = inspected {
        result.issue = Some(issue);
    }
    Ok(result)
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Finalization {
    digest: String,
    schema: u32,
    review_digest: String,
    disposition: String,
    at_nanos: String,
    root_entries_digest: String,
    files: BTreeMap<String, String>,
    preserved: Option<Vec<Entry>>,
    staged: Option<Vec<Entry>>,
}
fn record_files(directory: &Path, schema: u32) -> Result<BTreeMap<String, String>> {
    let allowed: BTreeSet<_> = [
        "plan.json",
        "operation.lock",
        "prepared.json",
        "move-intent.json",
        "restore-intent.json",
        "staged",
        "preserved",
        "finalization.json",
    ]
    .into_iter()
    .collect();
    for entry in io(fs::read_dir(directory))? {
        let name = io(entry)?.file_name();
        if !name.to_str().is_some_and(|n| allowed.contains(n)) {
            return Err("Unexpected recovery transaction entry; preserve it for inspection".into());
        }
    }
    let mut files = BTreeMap::new();
    for name in [
        "plan.json",
        "prepared.json",
        "move-intent.json",
        "restore-intent.json",
    ] {
        let present = match fs::symlink_metadata(directory.join(name)) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(e) => return Err(e.to_string()),
            Ok(_) => true,
        };
        if present {
            let value = if schema == 1 {
                let (value, _pin): (serde_json::Value, File) = read_json(&directory.join(name))?;
                digest(&value)?
            } else {
                let mut pin = io(backup::open(&directory.join(name), false, false))?;
                let mut bytes = Vec::new();
                io((&mut pin)
                    .take(16 * 1024 * 1024 + 1)
                    .read_to_end(&mut bytes))?;
                if bytes.len() > 16 * 1024 * 1024 {
                    return Err("Recovery record exceeds limit".into());
                }
                format!("{:x}", Sha256::digest(bytes))
            };
            files.insert(name.into(), value);
        }
    }
    Ok(files)
}
fn archive_path(plan: &Review, record: &Finalization) -> PathBuf {
    plan.transaction.with_file_name(format!(
        ".cerulean-recovery-{}-{}-{}-{}",
        record.disposition, plan.digest, record.at_nanos, record.digest
    ))
}
fn validate_finalization(record: &mut Finalization, plan: &Review) -> Result<()> {
    let recorded = std::mem::take(&mut record.digest);
    let actual = digest(&record)?;
    record.digest = recorded;
    if !matches!(record.schema, 1 | 2)
        || record.review_digest != plan.digest
        || record.digest != actual
        || !matches!(record.disposition.as_str(), "completed" | "abandoned")
        || record.at_nanos.len() > 24
        || record.at_nanos.parse::<u128>().is_err()
    {
        return Err("Invalid recovery finalization record".into());
    }
    Ok(())
}
fn inspect_archive(root: &Path, directory: &Path, expected: &str) -> Result<String> {
    let _directory = io(backup::open(directory, true, false))?;
    let _operation = io(backup::open(&directory.join("operation.lock"), false, true))?;
    let (mut plan, _plan): (Review, File) = read_json(&directory.join("plan.json"))?;
    validate_plan(&mut plan, root, &guard(root)?, Some(expected))?;
    let (mut record, _record): (Finalization, File) =
        read_json(&directory.join("finalization.json"))?;
    validate_finalization(&mut record, &plan)?;
    if archive_path(&plan, &record) != directory
        || record_files(directory, record.schema)? != record.files
    {
        return Err("Recovery archive name or transaction receipts differ".into());
    }
    let (manifest, verified, _backup, _archive, _manifest) = io(backup::verified(&plan.archive))?;
    if verified.manifest_digest != plan.manifest_digest
        || manifest.source_root != root
        || manifest.owner_sid != plan.owner_sid
    {
        return Err("Archived recovery backup identity differs".into());
    }
    verify_backup_hold(root, &plan.archive, plan.restored_hold())?;
    let preserved = tree_if_exists(&directory.join("preserved"), true)?;
    let staged = tree_if_exists(&directory.join("staged"), false)?;
    if preserved.as_ref().map(entries) != record.preserved
        || staged.as_ref().map(entries) != record.staged
    {
        return Err("Archived recovery files changed".into());
    }
    let receipt_matches = |name: &str| -> bool {
        // Raw-byte sealing retains malformed preparation records on abandonment;
        // completed recovery still requires every original semantic receipt.
        read_json::<String>(&directory.join(name)).is_ok_and(|(value, _pin)| value == plan.digest)
    };
    if record.disposition == "completed" {
        if record.root_entries_digest != digest(&manifest.entries)?
            || record.preserved.as_ref() != Some(&plan.original_entries)
            || record.staged.is_some()
            || !receipt_matches("prepared.json")
            || !receipt_matches("move-intent.json")
            || !receipt_matches("restore-intent.json")
        {
            return Err("Completion evidence is inconsistent".into());
        }
    } else if record.root_entries_digest != digest(&plan.original_entries)?
        || record.preserved.is_some()
        || record.files.contains_key("move-intent.json")
        || record.files.contains_key("restore-intent.json")
    {
        return Err("Abandonment evidence is inconsistent".into());
    }
    Ok(record.disposition)
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchivedObservation {
    pub archive: PathBuf,
    pub verified: bool,
    pub disposition: Option<String>,
    pub issue: Option<String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inspection {
    pub review_digest: String,
    pub guard_present: bool,
    pub active: Option<Status>,
    pub active_issue: Option<String>,
    pub archives: Vec<ArchivedObservation>,
    pub current_data_verified: bool,
    pub automatic_replay: bool,
}
/// Read evidence after ACK loss. Historical completion never authorizes a new
/// transaction, release, retry or startup, even when the review digest repeats.
pub fn inspect(root: &Path, expected: &str) -> Result<Inspection> {
    if expected.len() != 64
        || !expected
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err("A complete reviewed digest is required".into());
    }
    let root = location(root)?;
    let parent = root.parent().ok_or("Missing parent")?;
    let _parent = io(backup::open(parent, true, false))?;
    let guard_present = match fs::symlink_metadata(guard(&root)?) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(e.to_string()),
        Ok(_) => true,
    };
    let (active, active_issue) = if guard_present {
        match status(&root) {
            Ok(s) => (Some(s), None),
            Err(e) => (None, Some(e)),
        }
    } else {
        (None, None)
    };
    let prefixes = [
        format!(".cerulean-recovery-completed-{expected}-"),
        format!(".cerulean-recovery-abandoned-{expected}-"),
    ];
    let mut paths = Vec::new();
    for (count, item) in io(fs::read_dir(parent))?.enumerate() {
        if count >= 4096 {
            return Err("Recovery parent inventory exceeds the inspection limit".into());
        }
        let item = io(item)?;
        if prefixes
            .iter()
            .any(|prefix| item.file_name().to_string_lossy().starts_with(prefix))
        {
            paths.push(item.path());
        }
    }
    paths.sort();
    let archives = paths
        .into_iter()
        .map(|archive| match inspect_archive(&root, &archive, expected) {
            Ok(disposition) => ArchivedObservation {
                archive,
                verified: true,
                disposition: Some(disposition),
                issue: None,
            },
            Err(issue) => ArchivedObservation {
                archive,
                verified: false,
                disposition: None,
                issue: Some(issue),
            },
        })
        .collect();
    Ok(Inspection {
        review_digest: expected.into(),
        guard_present,
        active,
        active_issue,
        archives,
        current_data_verified: false,
        automatic_replay: false,
    })
}
/// Explicit apply or explicit continuation after inspecting the same transaction.
/// Publication refuses existing targets; failed source/stage trees are retained.
pub fn apply(root: &Path, expected: &str) -> Result<Status> {
    crate::maintenance_interlock::ensure_not_reserved(root)?;
    let tx = transaction(root, Some(expected))?;
    if !receipt(&tx, "prepared.json")? {
        return Err("Recovery preparation incomplete".into());
    }
    let initial = observe(&tx)?;
    if initial.restoration_complete {
        return Ok(initial);
    }
    if initial.original_state == "original"
        && !io(tx.plan.transaction.join("preserved").try_exists())?
        && initial.staged_backup
        && !initial.restore_intent
    {
        let original = io(backup::inventory(&tx.plan.source_root, true))?;
        if !matches(&original, &tx.plan.original_entries) {
            return Err("Original data changed before preservation".into());
        }
        if !initial.move_intent {
            write_new(
                &tx.plan.transaction.join("move-intent.json"),
                &tx.plan.digest,
            )?;
        }
        // Windows directory rename cannot retain the descendant handles. The
        // sibling guard blocks this generation's startup during the name gap;
        // any competing target from older software is refused, never overwritten.
        drop(original);
        io(backup::publish(
            &tx.plan.source_root,
            &tx.plan.transaction.join("preserved"),
        ))?;
    }
    let observed = observe(&tx)?;
    if observed.original_state != "absent"
        || !observed.preserved_original
        || !observed.staged_backup
        || !observed.move_intent
    {
        return Err("Recovery state is not eligible for publication; inspect retained directories and original intent".into());
    }
    let preserved = io(backup::inventory(
        &tx.plan.transaction.join("preserved"),
        true,
    ))?;
    let staged = io(backup::inventory(
        &tx.plan.transaction.join("staged"),
        false,
    ))?;
    if !matches(&preserved, &tx.plan.original_entries) || !matches(&staged, &tx.expected) {
        return Err("Recovery files changed".into());
    }
    if !observed.restore_intent {
        write_new(
            &tx.plan.transaction.join("restore-intent.json"),
            &tx.plan.digest,
        )?;
    }
    drop(staged);
    io(backup::publish(
        &tx.plan.transaction.join("staged"),
        &tx.plan.source_root,
    ))?;
    drop(preserved);
    let result = observe(&tx)?;
    if !result.restoration_complete {
        return Err("Recovery publication needs inspection; startup remains blocked".into());
    }
    Ok(result)
}
/// Archive the guard only after exact verification, or explicitly abandon a
/// preparation which has not attempted to move data. Both paths keep evidence.
pub fn finish(root: &Path, expected: &str, abandon_preparation: bool) -> Result<PathBuf> {
    crate::maintenance_interlock::ensure_not_reserved(root)?;
    let tx = transaction(root, Some(expected))?;
    let observed = observe(&tx)?;
    if abandon_preparation {
        if observed.original_state != "original"
            || observed.move_intent
            || io(tx.plan.transaction.join("preserved").try_exists())?
        {
            return Err(
                "Only an unchanged preparation with no move intent may be abandoned".into(),
            );
        }
    } else if !observed.restoration_complete {
        return Err("Recovery is not complete; guard retained".into());
    }
    let original = io(backup::inventory(&tx.plan.source_root, true))?;
    let expected_entries = if abandon_preparation {
        &tx.plan.original_entries
    } else {
        &tx.expected
    };
    if !matches(&original, expected_entries) {
        return Err("Original location changed before finishing".into());
    }
    let hold = crate::upgrade::status(&tx.plan.source_root)?;
    let retained_hold = if abandon_preparation {
        &tx.plan.hold_digest
    } else {
        tx.plan.restored_hold()
    };
    if !hold.valid_record || hold.hold_digest != retained_hold {
        return Err("Restored hold must remain intact".into());
    }
    if !abandon_preparation {
        crate::data_format::status(&tx.plan.source_root)?;
    }
    let action = if abandon_preparation {
        "abandoned"
    } else {
        "completed"
    };
    let preserved = tree_if_exists(&tx.plan.transaction.join("preserved"), true)?;
    let staged = tree_if_exists(&tx.plan.transaction.join("staged"), false)?;
    let mut record = Finalization {
        digest: String::new(),
        schema: 2,
        review_digest: tx.plan.digest.clone(),
        disposition: action.into(),
        at_nanos: chrono::Utc::now()
            .timestamp_nanos_opt()
            .ok_or("Clock is out of range")?
            .to_string(),
        root_entries_digest: digest(expected_entries)?,
        files: record_files(&tx.plan.transaction, 2)?,
        preserved: preserved.as_ref().map(entries),
        staged: staged.as_ref().map(entries),
    };
    let record_path = tx.plan.transaction.join("finalization.json");
    if io(record_path.try_exists())? {
        let (mut existing, _pin): (Finalization, File) = read_json(&record_path)?;
        validate_finalization(&mut existing, &tx.plan)?;
        record.schema = existing.schema;
        record.files = record_files(&tx.plan.transaction, existing.schema)?;
        record.at_nanos.clone_from(&existing.at_nanos);
        record.digest = digest(&record)?;
        if record.digest != existing.digest {
            return Err("Finalization intent differs; preserve original evidence".into());
        }
        record = existing;
    } else {
        record.digest = digest(&record)?;
        write_new(&record_path, &record)?;
    }
    let destination = archive_path(&tx.plan, &record);
    let directory = tx.plan.transaction.clone();
    // Keep original writer and parent pinned through guard archival. Retained
    // evidence is never deleted, even if final publication fails or ACK is lost.
    let _parent = io(backup::open(
        tx.plan.source_root.parent().ok_or("Missing parent")?,
        true,
        false,
    ))?;
    let source_root = tx.plan.source_root.clone();
    let review_digest = tx.plan.digest.clone();
    drop(preserved);
    drop(staged);
    drop(tx);
    io(backup::publish(&directory, &destination))?;
    inspect_archive(&source_root, &destination, &review_digest)?;
    drop(original);
    Ok(destination)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{upgrade, Store};
    pub(super) fn prepare_checkpoint(phase: &str) {
        let Ok(path) = std::env::var("CERULEAN_PREPARATION_FAULT_CONFIG") else {
            return;
        };
        let config: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        if config["phase"].as_str() != Some(phase) {
            return;
        }
        write_new(Path::new(config["checkpoint"].as_str().unwrap()), &serde_json::json!({"phase":phase,"pid":std::process::id(),"token":config["token"],"testOnlyCheckpoint":true})).unwrap();
        loop {
            std::thread::park_timeout(std::time::Duration::from_millis(250));
        }
    }
    #[test]
    #[ignore = "Explicit owned subprocess preparation interruption driver"]
    fn preparation_interrupt_child() {
        let path = std::env::var("CERULEAN_PREPARATION_FAULT_CONFIG").unwrap();
        let config: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert!(matches!(
            config["phase"].as_str().unwrap(),
            "metadata-file-created" | "guard-published" | "payload-copied"
        ));
        let root = Path::new(config["data"].as_str().unwrap());
        let archive = Path::new(config["backup"].as_str().unwrap());
        let manifest = config["manifestDigest"].as_str().unwrap();
        let hold = config["holdDigest"].as_str().unwrap();
        let reviewed = config["reviewDigest"].as_str().unwrap();
        if let Some(previous) = config["backupHoldDigest"].as_str() {
            prepare_previous(root, archive, manifest, hold, previous, reviewed).unwrap();
        } else {
            prepare(root, archive, manifest, hold, reviewed).unwrap();
        }
        panic!("Requested preparation checkpoint was not reached");
    }
    struct Fixture {
        _temp: tempfile::TempDir,
        root: PathBuf,
        archive: PathBuf,
        manifest: String,
        hold: String,
        approved: serde_json::Value,
    }
    impl Fixture {
        fn new() -> Self {
            Self::in_directory(tempfile::tempdir().unwrap())
        }
        fn in_directory(temp: tempfile::TempDir) -> Self {
            let root = temp.path().join("故障数据 空格");
            let mut store = Store::open(&root).unwrap();
            let snapshot = store
                .execute(cerulean_domain::Command::CreatePlan {
                    title: "preserve recovery fixture".into(),
                    tasks: vec!["preserve approval".into()],
                })
                .unwrap();
            let plan = &snapshot.plans[0];
            let approved = store
                .execute(cerulean_domain::Command::ApprovePlan {
                    id: plan.id.clone(),
                    digest: plan.digest.clone(),
                })
                .unwrap();
            let approved = serde_json::to_value(approved.plans).unwrap();
            drop(store);
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
            fs::write(
                root.join("events/.format-migration-fixture.pending"),
                b"partial migration evidence",
            )
            .unwrap();
            Self {
                _temp: temp,
                root,
                archive,
                manifest,
                hold,
                approved,
            }
        }
        fn review(&self) -> Review {
            review(&self.root, &self.archive, &self.manifest, &self.hold).unwrap()
        }
        fn prepare(&self) -> Review {
            let review = self.review();
            let status = prepare(
                &self.root,
                &self.archive,
                &self.manifest,
                &self.hold,
                &review.digest,
            )
            .unwrap();
            assert_eq!(status.original_state, "original");
            assert!(status.staged_backup && status.startup_blocked && !status.restoration_complete);
            review
        }
        fn assert_usable(&self) {
            upgrade::release(&self.root, &self.hold).unwrap();
            let store = Store::open(&self.root).unwrap();
            assert_eq!(
                serde_json::to_value(store.snapshot().plans).unwrap(),
                self.approved
            );
        }
    }
    #[test]
    fn previous_backup_restores_after_promotion_release_and_new_hold_without_rewriting_evidence() {
        for schema in [1, 2] {
            let (f, current) = previous_fixture(schema);
            let original = entries(&backup::inventory(&f.root, true).unwrap());
            assert!(review(&f.root, &f.archive, &f.manifest, &current).is_err());
            let r = review_previous(&f.root, &f.archive, &f.manifest, &current, &f.hold).unwrap();
            assert_eq!(r.schema, 2);
            assert_ne!(r.hold_digest, r.restored_hold());
            assert_eq!(
                entries(&backup::inventory(&f.root, true).unwrap()),
                original
            );
            assert!(prepare_previous(
                &f.root,
                &f.archive,
                &f.manifest,
                &current,
                &f.hold,
                &"0".repeat(64)
            )
            .is_err());
            assert!(!guard(&f.root).unwrap().exists());
            prepare_previous(
                &f.root,
                &f.archive,
                &f.manifest,
                &current,
                &f.hold,
                &r.digest,
            )
            .unwrap();
            assert!(Store::open(&f.root).is_err());
            let result = apply(&f.root, &r.digest).unwrap();
            assert!(result.restoration_complete);
            assert_eq!(result.original_hold_digest, current);
            assert_eq!(result.restored_hold_digest, f.hold);
            // Reconcile the original transaction after a lost reply; no replay.
            assert!(status(&f.root).unwrap().restoration_complete);
            let history = finish(&f.root, &r.digest, false).unwrap();
            assert!(inspect(&f.root, &r.digest).unwrap().archives[0].verified);
            assert_eq!(
                entries(&backup::inventory(&history.join("preserved"), true).unwrap()),
                original
            );
            assert!(history
                .join("preserved")
                .join(crate::reader_generation::MARKER)
                .exists());
            assert!(!f.root.join(crate::reader_generation::MARKER).exists());
            assert_eq!(
                crate::data_format::status(&f.root)
                    .unwrap()
                    .minimum_reader_generation,
                schema
            );
            assert!(upgrade::release(&f.root, &current).is_err());
            f.assert_usable();
        }
    }
    #[test]
    fn reader4_previous_backup_restores_original_generation_and_preserves_both_guards() {
        for schema in [1, 2] {
            let f = promoted_fixture(Fixture::new(), schema);
            crate::currency_guard::tests::promote_fixture(&f.root);
            assert_eq!(
                crate::data_format::status(&f.root)
                    .unwrap()
                    .minimum_reader_generation,
                4
            );
            let current = fresh_hold(&f.root);
            let original = entries(&backup::inventory(&f.root, true).unwrap());
            assert!(review(&f.root, &f.archive, &f.manifest, &current).is_err());
            let r = review_previous(&f.root, &f.archive, &f.manifest, &current, &f.hold).unwrap();
            prepare_previous(
                &f.root,
                &f.archive,
                &f.manifest,
                &current,
                &f.hold,
                &r.digest,
            )
            .unwrap();
            assert!(Store::open(&f.root).is_err());
            assert!(apply(&f.root, &r.digest).unwrap().restoration_complete);
            assert!(status(&f.root).unwrap().restoration_complete);
            let history = finish(&f.root, &r.digest, false).unwrap();
            assert!(inspect(&f.root, &r.digest).unwrap().archives[0].verified);
            let preserved = history.join("preserved");
            assert_eq!(
                entries(&backup::inventory(&preserved, true).unwrap()),
                original
            );
            for marker in [
                crate::reader_generation::MARKER,
                crate::currency_guard::MARKER,
            ] {
                assert!(preserved.join(marker).exists());
                assert!(!f.root.join(marker).exists());
            }
            assert_eq!(
                crate::data_format::status(&f.root)
                    .unwrap()
                    .minimum_reader_generation,
                schema
            );
            assert!(upgrade::release(&f.root, &current).is_err());
            f.assert_usable();
        }
    }
    #[test]
    fn reader5_previous_backup_restores_original_generation_and_preserves_both_guards() {
        for schema in [1, 2] {
            let f = promoted_fixture(Fixture::new(), schema);
            crate::currency_guard::tests::promote_fixture(&f.root);
            crate::currency_execution_guard::tests::promote_fixture(&f.root);
            assert_eq!(
                crate::data_format::status(&f.root)
                    .unwrap()
                    .minimum_reader_generation,
                5
            );
            let current = fresh_hold(&f.root);
            let original = entries(&backup::inventory(&f.root, true).unwrap());
            assert!(review(&f.root, &f.archive, &f.manifest, &current).is_err());
            let r = review_previous(&f.root, &f.archive, &f.manifest, &current, &f.hold).unwrap();
            prepare_previous(
                &f.root,
                &f.archive,
                &f.manifest,
                &current,
                &f.hold,
                &r.digest,
            )
            .unwrap();
            assert!(Store::open(&f.root).is_err());
            assert!(apply(&f.root, &r.digest).unwrap().restoration_complete);
            assert!(status(&f.root).unwrap().restoration_complete);
            let history = finish(&f.root, &r.digest, false).unwrap();
            assert!(inspect(&f.root, &r.digest).unwrap().archives[0].verified);
            let preserved = history.join("preserved");
            assert_eq!(
                entries(&backup::inventory(&preserved, true).unwrap()),
                original
            );
            for marker in [
                crate::reader_generation::MARKER,
                crate::currency_guard::MARKER,
                crate::currency_execution_guard::MARKER,
            ] {
                assert!(preserved.join(marker).exists());
                assert!(!f.root.join(marker).exists());
            }
            assert_eq!(
                crate::data_format::status(&f.root)
                    .unwrap()
                    .minimum_reader_generation,
                schema
            );
            assert!(upgrade::release(&f.root, &current).is_err());
            f.assert_usable();
        }
    }
    #[test]
    fn reader6_previous_backup_restores_original_generation_and_preserves_all_guards() {
        for schema in [1, 2] {
            let f = promoted_fixture(Fixture::new(), schema);
            crate::currency_guard::tests::promote_fixture(&f.root);
            crate::currency_execution_guard::tests::promote_fixture(&f.root);
            crate::currency_count_guard::tests::promote_fixture(&f.root);
            assert_eq!(
                crate::data_format::status(&f.root)
                    .unwrap()
                    .minimum_reader_generation,
                6
            );
            let current = fresh_hold(&f.root);
            let original = entries(&backup::inventory(&f.root, true).unwrap());
            assert!(review(&f.root, &f.archive, &f.manifest, &current).is_err());
            let r = review_previous(&f.root, &f.archive, &f.manifest, &current, &f.hold).unwrap();
            prepare_previous(
                &f.root,
                &f.archive,
                &f.manifest,
                &current,
                &f.hold,
                &r.digest,
            )
            .unwrap();
            assert!(Store::open(&f.root).is_err());
            assert!(apply(&f.root, &r.digest).unwrap().restoration_complete);
            assert!(status(&f.root).unwrap().restoration_complete);
            let history = finish(&f.root, &r.digest, false).unwrap();
            assert!(inspect(&f.root, &r.digest).unwrap().archives[0].verified);
            let preserved = history.join("preserved");
            assert_eq!(
                entries(&backup::inventory(&preserved, true).unwrap()),
                original
            );
            for marker in [
                crate::reader_generation::MARKER,
                crate::currency_guard::MARKER,
                crate::currency_execution_guard::MARKER,
                crate::currency_count_guard::MARKER,
            ] {
                assert!(preserved.join(marker).exists());
                assert!(!f.root.join(marker).exists());
            }
            assert_eq!(
                crate::data_format::status(&f.root)
                    .unwrap()
                    .minimum_reader_generation,
                schema
            );
            assert!(upgrade::release(&f.root, &current).is_err());
            f.assert_usable();
        }
    }
    #[test]
    fn reader7_previous_backup_restores_original_generation_and_preserves_all_guards() {
        for schema in [1, 2] {
            let f = promoted_fixture(Fixture::new(), schema);
            crate::currency_guard::tests::promote_fixture(&f.root);
            crate::currency_execution_guard::tests::promote_fixture(&f.root);
            crate::currency_count_guard::tests::promote_fixture(&f.root);
            crate::currency_count_resolution_guard::tests::promote_fixture(&f.root);
            assert_eq!(
                crate::data_format::status(&f.root)
                    .unwrap()
                    .minimum_reader_generation,
                7
            );
            let current = fresh_hold(&f.root);
            let original = entries(&backup::inventory(&f.root, true).unwrap());
            assert!(review(&f.root, &f.archive, &f.manifest, &current).is_err());
            let r = review_previous(&f.root, &f.archive, &f.manifest, &current, &f.hold).unwrap();
            prepare_previous(
                &f.root,
                &f.archive,
                &f.manifest,
                &current,
                &f.hold,
                &r.digest,
            )
            .unwrap();
            assert!(Store::open(&f.root).is_err());
            assert!(apply(&f.root, &r.digest).unwrap().restoration_complete);
            assert!(status(&f.root).unwrap().restoration_complete);
            let history = finish(&f.root, &r.digest, false).unwrap();
            assert!(inspect(&f.root, &r.digest).unwrap().archives[0].verified);
            let preserved = history.join("preserved");
            assert_eq!(
                entries(&backup::inventory(&preserved, true).unwrap()),
                original
            );
            for marker in [
                crate::reader_generation::MARKER,
                crate::currency_guard::MARKER,
                crate::currency_execution_guard::MARKER,
                crate::currency_count_guard::MARKER,
                crate::currency_count_resolution_guard::MARKER,
            ] {
                assert!(preserved.join(marker).exists());
                assert!(!f.root.join(marker).exists());
            }
            assert_eq!(
                crate::data_format::status(&f.root)
                    .unwrap()
                    .minimum_reader_generation,
                schema
            );
            assert!(upgrade::release(&f.root, &current).is_err());
            f.assert_usable();
        }
    }
    #[test]
    fn reader8_previous_backup_restores_original_generation_and_preserves_all_guards() {
        for schema in [1, 2] {
            let f = promoted_fixture(Fixture::new(), schema);
            crate::currency_guard::tests::promote_fixture(&f.root);
            crate::currency_execution_guard::tests::promote_fixture(&f.root);
            crate::currency_count_guard::tests::promote_fixture(&f.root);
            crate::currency_count_resolution_guard::tests::promote_fixture(&f.root);
            crate::currency_scoped_guard::tests::promote_fixture(&f.root);
            assert_eq!(
                crate::data_format::status(&f.root)
                    .unwrap()
                    .minimum_reader_generation,
                8
            );
            let current = fresh_hold(&f.root);
            let original = entries(&backup::inventory(&f.root, true).unwrap());
            assert!(review(&f.root, &f.archive, &f.manifest, &current).is_err());
            let r = review_previous(&f.root, &f.archive, &f.manifest, &current, &f.hold).unwrap();
            prepare_previous(
                &f.root,
                &f.archive,
                &f.manifest,
                &current,
                &f.hold,
                &r.digest,
            )
            .unwrap();
            assert!(Store::open(&f.root).is_err());
            assert!(apply(&f.root, &r.digest).unwrap().restoration_complete);
            assert!(status(&f.root).unwrap().restoration_complete);
            let history = finish(&f.root, &r.digest, false).unwrap();
            assert!(inspect(&f.root, &r.digest).unwrap().archives[0].verified);
            let preserved = history.join("preserved");
            assert_eq!(
                entries(&backup::inventory(&preserved, true).unwrap()),
                original
            );
            for marker in [
                crate::reader_generation::MARKER,
                crate::currency_guard::MARKER,
                crate::currency_execution_guard::MARKER,
                crate::currency_count_guard::MARKER,
                crate::currency_count_resolution_guard::MARKER,
                crate::currency_scoped_guard::MARKER,
            ] {
                assert!(preserved.join(marker).exists());
                assert!(!f.root.join(marker).exists());
            }
            assert_eq!(
                crate::data_format::status(&f.root)
                    .unwrap()
                    .minimum_reader_generation,
                schema
            );
            assert!(upgrade::release(&f.root, &current).is_err());
            f.assert_usable();
        }
    }
    #[test]
    fn reader9_previous_backup_restores_original_generation_and_preserves_all_guards() {
        for schema in [1, 2] {
            let f = promoted_fixture(Fixture::new(), schema);
            crate::currency_guard::tests::promote_fixture(&f.root);
            crate::currency_execution_guard::tests::promote_fixture(&f.root);
            crate::currency_count_guard::tests::promote_fixture(&f.root);
            crate::currency_count_resolution_guard::tests::promote_fixture(&f.root);
            crate::currency_scoped_guard::tests::promote_fixture(&f.root);
            crate::currency_scoped_count_guard::tests::promote_fixture(&f.root);
            assert_eq!(
                crate::data_format::status(&f.root)
                    .unwrap()
                    .minimum_reader_generation,
                9
            );
            let current = fresh_hold(&f.root);
            let original = entries(&backup::inventory(&f.root, true).unwrap());
            assert!(review(&f.root, &f.archive, &f.manifest, &current).is_err());
            let r = review_previous(&f.root, &f.archive, &f.manifest, &current, &f.hold).unwrap();
            prepare_previous(
                &f.root,
                &f.archive,
                &f.manifest,
                &current,
                &f.hold,
                &r.digest,
            )
            .unwrap();
            assert!(Store::open(&f.root).is_err());
            assert!(apply(&f.root, &r.digest).unwrap().restoration_complete);
            assert!(status(&f.root).unwrap().restoration_complete);
            let history = finish(&f.root, &r.digest, false).unwrap();
            assert!(inspect(&f.root, &r.digest).unwrap().archives[0].verified);
            let preserved = history.join("preserved");
            assert_eq!(
                entries(&backup::inventory(&preserved, true).unwrap()),
                original
            );
            for marker in [
                crate::reader_generation::MARKER,
                crate::currency_guard::MARKER,
                crate::currency_execution_guard::MARKER,
                crate::currency_count_guard::MARKER,
                crate::currency_count_resolution_guard::MARKER,
                crate::currency_scoped_guard::MARKER,
                crate::currency_scoped_count_guard::MARKER,
            ] {
                assert!(preserved.join(marker).exists());
                assert!(!f.root.join(marker).exists());
            }
            assert_eq!(
                crate::data_format::status(&f.root)
                    .unwrap()
                    .minimum_reader_generation,
                schema
            );
            assert!(upgrade::release(&f.root, &current).is_err());
            f.assert_usable();
        }
    }
    #[test]
    fn reader10_previous_backup_restores_original_generation_and_preserves_all_guards() {
        for schema in [1, 2] {
            let f = promoted_fixture(Fixture::new(), schema);
            crate::currency_guard::tests::promote_fixture(&f.root);
            crate::currency_execution_guard::tests::promote_fixture(&f.root);
            crate::currency_count_guard::tests::promote_fixture(&f.root);
            crate::currency_count_resolution_guard::tests::promote_fixture(&f.root);
            crate::currency_scoped_guard::tests::promote_fixture(&f.root);
            crate::currency_scoped_count_guard::tests::promote_fixture(&f.root);
            crate::currency_scoped_execution_guard::tests::promote_fixture(&f.root);
            assert_eq!(
                crate::data_format::status(&f.root)
                    .unwrap()
                    .minimum_reader_generation,
                10
            );
            let current = fresh_hold(&f.root);
            let original = entries(&backup::inventory(&f.root, true).unwrap());
            assert!(review(&f.root, &f.archive, &f.manifest, &current).is_err());
            let r = review_previous(&f.root, &f.archive, &f.manifest, &current, &f.hold).unwrap();
            prepare_previous(
                &f.root,
                &f.archive,
                &f.manifest,
                &current,
                &f.hold,
                &r.digest,
            )
            .unwrap();
            assert!(Store::open(&f.root).is_err());
            assert!(apply(&f.root, &r.digest).unwrap().restoration_complete);
            assert!(status(&f.root).unwrap().restoration_complete);
            let history = finish(&f.root, &r.digest, false).unwrap();
            assert!(inspect(&f.root, &r.digest).unwrap().archives[0].verified);
            let preserved = history.join("preserved");
            assert_eq!(
                entries(&backup::inventory(&preserved, true).unwrap()),
                original
            );
            for marker in [
                crate::reader_generation::MARKER,
                crate::currency_guard::MARKER,
                crate::currency_execution_guard::MARKER,
                crate::currency_count_guard::MARKER,
                crate::currency_count_resolution_guard::MARKER,
                crate::currency_scoped_guard::MARKER,
                crate::currency_scoped_count_guard::MARKER,
                crate::currency_scoped_execution_guard::MARKER,
            ] {
                assert!(preserved.join(marker).exists());
                assert!(!f.root.join(marker).exists());
            }
            assert_eq!(
                crate::data_format::status(&f.root)
                    .unwrap()
                    .minimum_reader_generation,
                schema
            );
            assert!(upgrade::release(&f.root, &current).is_err());
            f.assert_usable();
        }
    }
    #[test]
    fn reader11_previous_backup_restores_original_generation_and_preserves_all_guards() {
        api_reader_previous_backup(11);
    }
    #[test]
    fn reader12_previous_backup_restores_original_generation_and_preserves_all_guards() {
        api_reader_previous_backup(12);
    }
    fn api_reader_previous_backup(generation: u32) {
        for schema in [1, 2] {
            let f = promoted_fixture(Fixture::new(), schema);
            crate::currency_guard::tests::promote_fixture(&f.root);
            crate::currency_execution_guard::tests::promote_fixture(&f.root);
            crate::currency_count_guard::tests::promote_fixture(&f.root);
            crate::currency_count_resolution_guard::tests::promote_fixture(&f.root);
            crate::currency_scoped_guard::tests::promote_fixture(&f.root);
            crate::currency_scoped_count_guard::tests::promote_fixture(&f.root);
            crate::currency_scoped_execution_guard::tests::promote_fixture(&f.root);
            crate::currency_api_guard::tests::promote_fixture(&f.root);
            if generation == 12 {
                crate::currency_api_resolution_guard::tests::promote_fixture(&f.root);
            }
            assert_eq!(
                crate::data_format::status(&f.root)
                    .unwrap()
                    .minimum_reader_generation,
                generation
            );
            let current = fresh_hold(&f.root);
            let original = entries(&backup::inventory(&f.root, true).unwrap());
            assert!(review(&f.root, &f.archive, &f.manifest, &current).is_err());
            let r = review_previous(&f.root, &f.archive, &f.manifest, &current, &f.hold).unwrap();
            prepare_previous(
                &f.root,
                &f.archive,
                &f.manifest,
                &current,
                &f.hold,
                &r.digest,
            )
            .unwrap();
            assert!(Store::open(&f.root).is_err());
            assert!(apply(&f.root, &r.digest).unwrap().restoration_complete);
            assert!(status(&f.root).unwrap().restoration_complete);
            let history = finish(&f.root, &r.digest, false).unwrap();
            assert!(inspect(&f.root, &r.digest).unwrap().archives[0].verified);
            let preserved = history.join("preserved");
            assert_eq!(
                entries(&backup::inventory(&preserved, true).unwrap()),
                original
            );
            for marker in [
                crate::reader_generation::MARKER,
                crate::currency_guard::MARKER,
                crate::currency_execution_guard::MARKER,
                crate::currency_count_guard::MARKER,
                crate::currency_count_resolution_guard::MARKER,
                crate::currency_scoped_guard::MARKER,
                crate::currency_scoped_count_guard::MARKER,
                crate::currency_scoped_execution_guard::MARKER,
                crate::currency_api_guard::MARKER,
            ] {
                assert!(preserved.join(marker).exists());
                assert!(!f.root.join(marker).exists());
            }
            assert_eq!(
                crate::data_format::status(&f.root)
                    .unwrap()
                    .minimum_reader_generation,
                schema
            );
            if generation == 12 {
                assert!(preserved
                    .join(crate::currency_api_resolution_guard::MARKER)
                    .exists());
                assert!(!f
                    .root
                    .join(crate::currency_api_resolution_guard::MARKER)
                    .exists());
            }
            assert!(upgrade::release(&f.root, &current).is_err());
            f.assert_usable();
        }
    }
    fn fresh_hold(root: &Path) -> String {
        let mut r = upgrade::UpgradeReview {
            digest: String::new(),
            ready: true,
            blockers: vec![],
            snapshot_digest: "c".repeat(64),
            core_version: env!("CARGO_PKG_VERSION").into(),
            core_sha256: "d".repeat(64),
            server_pid: std::process::id(),
        };
        r.digest = cerulean_domain::execution::digest(&r).unwrap();
        upgrade::prepare(root, r).unwrap().hold_digest
    }
    fn previous_fixture(schema: u32) -> (Fixture, String) {
        let f = promoted_fixture(Fixture::new(), schema);
        let current = fresh_hold(&f.root);
        (f, current)
    }
    fn promoted_fixture(mut f: Fixture, schema: u32) -> Fixture {
        // Remove only this test's synthetic incomplete-migration record so it
        // represents a successful promotion; production never repairs it.
        fs::remove_file(f.root.join("events/.format-migration-fixture.pending")).unwrap();
        if schema == 2 {
            let r = crate::data_format::review(&f.root, &f.archive, &f.manifest, &f.hold).unwrap();
            crate::data_format::migrate(&f.root, &f.archive, &f.manifest, &f.hold, &r.digest)
                .unwrap();
            upgrade::release(&f.root, &f.hold).unwrap();
            f.hold = fresh_hold(&f.root);
            f.archive = f._temp.path().join("backup2");
            f.manifest = backup::create_held(&f.root, &f.archive, &f.hold)
                .unwrap()
                .manifest_digest;
        }
        let r =
            crate::reader_generation::review(&f.root, &f.archive, &f.manifest, &f.hold).unwrap();
        crate::reader_generation::promote(&f.root, &f.archive, &f.manifest, &f.hold, &r.digest)
            .unwrap();
        upgrade::release(&f.root, &f.hold).unwrap();
        let mut store = Store::open(&f.root).unwrap();
        store
            .execute(cerulean_domain::Command::CreatePlan {
                title: "newer data retained on rollback".into(),
                tasks: vec!["do not replay".into()],
            })
            .unwrap();
        drop(store);
        f
    }
    #[test]
    #[ignore = "Create owned post-promotion fixtures for actual current-core quiescence and recovery CLI"]
    fn export_previous_recovery_cli_fixtures() {
        let run = PathBuf::from(std::env::var_os("CERULEAN_PREVIOUS_RECOVERY_DIR").unwrap());
        assert!(!run.exists(), "Preserve existing fixture");
        fs::create_dir(&run).unwrap();
        let mut cases = Vec::new();
        for (schema, mode) in [(1, "restore"), (2, "restore"), (1, "abandon")] {
            let dir = tempfile::Builder::new()
                .prefix(&format!("schema-{schema}-{mode}-"))
                .tempdir_in(&run)
                .unwrap();
            let f = promoted_fixture(Fixture::in_directory(dir), schema);
            cases.push(serde_json::json!({"schema":schema,"mode":mode,"data":f.root,"backup":f.archive,"manifestDigest":f.manifest,"backupHoldDigest":f.hold}));
            let _retained = f._temp.keep();
        }
        fs::write(
            run.join("cases.json"),
            serde_json::to_vec_pretty(&cases).unwrap(),
        )
        .unwrap();
    }
    #[test]
    #[ignore = "Owned previous-hold schema2 recovery fault fixtures; no user directory"]
    fn export_previous_recovery_fault_fixtures() {
        let run = PathBuf::from(std::env::var_os("CERULEAN_PREVIOUS_FAULT_DIR").unwrap());
        assert!(!run.exists(), "Preserve original fault fixtures");
        fs::create_dir(&run).unwrap();
        let mut cases = Vec::new();
        for schema in [1, 2] {
            for phase in [
                "metadata-file-created",
                "guard-published",
                "payload-copied",
                "share-denied",
            ] {
                let temp = tempfile::Builder::new()
                    .prefix(&format!("s{schema}-{phase}-"))
                    .tempdir_in(&run)
                    .unwrap();
                let f = promoted_fixture(Fixture::in_directory(temp), schema);
                let current = fresh_hold(&f.root);
                cases.push(serde_json::json!({"schema":schema,"phase":phase,"data":f.root,"backup":f.archive,"manifestDigest":f.manifest,"backupHoldDigest":f.hold,"holdDigest":current}));
                let _retained = f._temp.keep();
            }
        }
        fs::write(
            run.join("cases.json"),
            serde_json::to_vec_pretty(&cases).unwrap(),
        )
        .unwrap();
    }
    #[test]
    fn previous_backup_requires_release_proof_current_quiescence_and_fresh_review() {
        let (f, current) = previous_fixture(1);
        let r = review_previous(&f.root, &f.archive, &f.manifest, &current, &f.hold).unwrap();
        for (hold, previous) in [
            (&current, &current),
            (&f.hold, &current),
            (&"0".repeat(64), &f.hold),
            (&current, &"0".repeat(64)),
        ] {
            assert!(review_previous(&f.root, &f.archive, &f.manifest, hold, previous).is_err());
        }
        let lock = backup::open(&f.root.join("writer.lock"), false, true).unwrap();
        assert!(review_previous(&f.root, &f.archive, &f.manifest, &current, &f.hold).is_err());
        drop(lock);
        let released = f
            .root
            .join(&r.previous_hold.as_ref().unwrap().released_record);
        let held_bytes = fs::read(&released).unwrap();
        fs::write(&released, b"{}").unwrap();
        assert!(review_previous(&f.root, &f.archive, &f.manifest, &current, &f.hold).is_err());
        fs::write(&released, held_bytes).unwrap();
        fs::write(f.root.join("new-data"), b"review changed").unwrap();
        assert!(prepare_previous(
            &f.root,
            &f.archive,
            &f.manifest,
            &current,
            &f.hold,
            &r.digest
        )
        .is_err());
        assert!(!guard(&f.root).unwrap().exists());
    }
    #[test]
    fn previous_preparation_abandonment_keeps_current_hold_and_strict_schema_binding() {
        let (f, current) = previous_fixture(1);
        let r = review_previous(&f.root, &f.archive, &f.manifest, &current, &f.hold).unwrap();
        prepare_previous(
            &f.root,
            &f.archive,
            &f.manifest,
            &current,
            &f.hold,
            &r.digest,
        )
        .unwrap();
        let plan_path = guard(&f.root).unwrap().join("plan.json");
        let original = fs::read(&plan_path).unwrap();
        for mutation in ["schema", "hold", "release"] {
            let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
            match mutation {
                "schema" => value["schema"] = 1.into(),
                "hold" => value["previousHold"]["digest"] = current.clone().into(),
                _ => value["previousHold"]["releasedRecord"] = "../forged.json".into(),
            }
            let mut changed: Review = serde_json::from_value(value).unwrap();
            changed.digest.clear();
            changed.digest = digest(&changed).unwrap();
            fs::write(&plan_path, serde_json::to_vec(&changed).unwrap()).unwrap();
            assert!(status(&f.root).is_err());
            assert!(apply(&f.root, &changed.digest).is_err());
        }
        fs::write(&plan_path, original).unwrap();
        let archived = finish(&f.root, &r.digest, true).unwrap();
        assert!(archived.join("staged/upgrade-hold.json").exists());
        assert_eq!(upgrade::status(&f.root).unwrap().hold_digest, current);
        assert!(inspect(&f.root, &r.digest).unwrap().archives[0].verified);
        upgrade::release(&f.root, &current).unwrap();
        assert_eq!(Store::open(&f.root).unwrap().snapshot().plans.len(), 2);
    }

    #[test]
    fn reviewed_recovery_preserves_fault_and_exact_backup_before_explicit_release() {
        let f = Fixture::new();
        let review = f.prepare();
        assert!(upgrade::release(&f.root, &f.hold).is_err());
        assert!(Store::open(&f.root).is_err());
        assert!(finish(&f.root, &review.digest, false).is_err());
        assert!(apply(&f.root, &"0".repeat(64)).is_err());
        let result = apply(&f.root, &review.digest).unwrap();
        assert!(result.restoration_complete && result.preserved_original);
        assert_eq!(result.original_state, "restored");
        assert!(apply(&f.root, &review.digest).unwrap().restoration_complete);
        assert!(Store::open(&f.root).is_err());
        assert!(upgrade::release(&f.root, &f.hold).is_err());
        let history = finish(&f.root, &review.digest, false).unwrap();
        assert_eq!(
            fs::read(history.join("preserved/events/.format-migration-fixture.pending")).unwrap(),
            b"partial migration evidence"
        );
        assert!(f.root.join("upgrade-hold.json").exists());
        assert!(!f
            .root
            .join("events/.format-migration-fixture.pending")
            .exists());
        f.assert_usable();
    }
    #[test]
    fn stale_review_active_writer_wrong_backup_or_hold_cannot_prepare() {
        let f = Fixture::new();
        let review = f.review();
        assert!(prepare(&f.root, &f.archive, &f.manifest, &f.hold, &"0".repeat(64)).is_err());
        assert!(!review.transaction.exists());
        fs::write(f.root.join("new-evidence"), b"retain").unwrap();
        assert!(prepare(&f.root, &f.archive, &f.manifest, &f.hold, &review.digest).is_err());
        assert!(!review.transaction.exists());
        assert!(super::review(&f.root, &f.archive, &"0".repeat(64), &f.hold).is_err());
        assert!(super::review(&f.root, &f.archive, &f.manifest, &"0".repeat(64)).is_err());
        let lock = backup::open(&f.root.join("writer.lock"), false, true).unwrap();
        assert!(super::review(&f.root, &f.archive, &f.manifest, &f.hold).is_err());
        drop(lock);
    }
    #[test]
    fn real_rename_sharing_failure_retains_intent_then_explicit_apply_continues() {
        let f = Fixture::new();
        let review = f.prepare();
        let occupied = backup::open(
            &f.root.join("events/.format-migration-fixture.pending"),
            false,
            false,
        )
        .unwrap();
        assert!(apply(&f.root, &review.digest).is_err());
        let result = status(&f.root).unwrap();
        assert_eq!(result.original_state, "original");
        assert!(result.move_intent && !result.preserved_original && result.staged_backup);
        assert!(finish(&f.root, &review.digest, true).is_err());
        drop(occupied);
        assert!(apply(&f.root, &review.digest).unwrap().restoration_complete);
        finish(&f.root, &review.digest, false).unwrap();
        f.assert_usable();
    }
    fn moved_fixture(f: &Fixture, review: &Review) {
        write_new(&review.transaction.join("move-intent.json"), &review.digest).unwrap();
        backup::publish(&f.root, &review.transaction.join("preserved")).unwrap();
    }
    #[test]
    fn absent_original_after_move_is_guarded_and_can_be_explicitly_restored() {
        let f = Fixture::new();
        let review = f.prepare();
        moved_fixture(&f, &review);
        let before = fs::read(review.transaction.join("preserved/upgrade-hold.json")).unwrap();
        assert!(Store::open(&f.root).is_err());
        assert!(
            !f.root.exists(),
            "startup must not create an empty replacement directory"
        );
        assert!(crate::ipc::endpoint(&f.root).is_err());
        assert!(
            !f.root.exists(),
            "IPC endpoint lookup must not recreate original data"
        );
        assert_eq!(status(&f.root).unwrap().original_state, "absent");
        assert!(apply(&f.root, &review.digest).unwrap().restoration_complete);
        assert_eq!(fs::read(f.root.join("upgrade-hold.json")).unwrap(), before);
        finish(&f.root, &review.digest, false).unwrap();
        f.assert_usable();
    }
    #[test]
    fn competing_target_and_tampered_stage_are_never_overwritten() {
        let f = Fixture::new();
        let review = f.prepare();
        moved_fixture(&f, &review);
        fs::create_dir(&f.root).unwrap();
        fs::write(f.root.join("competing-data"), b"keep me").unwrap();
        assert!(apply(&f.root, &review.digest).is_err());
        assert!(finish(&f.root, &review.digest, false).is_err());
        assert_eq!(fs::read(f.root.join("competing-data")).unwrap(), b"keep me");
        assert!(review
            .transaction
            .join("preserved/events/.format-migration-fixture.pending")
            .exists());
        let other = Fixture::new();
        let r = other.prepare();
        fs::write(
            r.transaction.join("staged/events/00000001.json"),
            b"tampered",
        )
        .unwrap();
        assert!(apply(&other.root, &r.digest).is_err());
        assert!(!r.transaction.join("move-intent.json").exists());
        assert!(other
            .root
            .join("events/.format-migration-fixture.pending")
            .exists());
    }
    #[test]
    fn abandon_preparation_keeps_all_evidence_and_original_hold() {
        let f = Fixture::new();
        let review = f.prepare();
        fs::write(
            review.transaction.join("staged/unexpected"),
            b"partial staged evidence",
        )
        .unwrap();
        assert!(!status(&f.root).unwrap().staged_backup);
        let archived = finish(&f.root, &review.digest, true).unwrap();
        assert!(archived.join("staged/unexpected").exists());
        assert!(f
            .root
            .join("events/.format-migration-fixture.pending")
            .exists());
        assert!(f.root.join("upgrade-hold.json").exists());
        assert!(ensure_not_recovering(&f.root).is_ok());
        assert!(Store::open(&f.root).is_err());
    }
    #[test]
    fn invalid_guard_record_blocks_startup_and_operations_serialize() {
        let f = Fixture::new();
        let review = f.prepare();
        let operation =
            backup::open(&review.transaction.join("operation.lock"), false, true).unwrap();
        assert!(apply(&f.root, &review.digest).is_err());
        assert!(status(&f.root).is_err());
        drop(operation);
        fs::write(review.transaction.join("plan.json"), b"{").unwrap();
        assert!(status(&f.root).is_err());
        assert!(Store::open(&f.root).is_err());
        assert!(upgrade::release(&f.root, &f.hold).is_err());
        assert!(f
            .root
            .join("events/.format-migration-fixture.pending")
            .exists());
    }
    #[test]
    fn historical_completion_is_read_only_and_does_not_claim_current_data() {
        let f = Fixture::new();
        let review = f.prepare();
        apply(&f.root, &review.digest).unwrap();
        let archived = finish(&f.root, &review.digest, false).unwrap();
        f.assert_usable();
        let result = inspect(&f.root, &review.digest).unwrap();
        assert!(!result.guard_present && !result.current_data_verified && !result.automatic_replay);
        assert_eq!(result.archives.len(), 1);
        assert!(result.archives[0].verified);
        assert_eq!(result.archives[0].disposition.as_deref(), Some("completed"));
        assert!(finish(&f.root, &review.digest, false).is_err());
        fs::write(
            archived.join("preserved/events/.format-migration-fixture.pending"),
            b"tampered",
        )
        .unwrap();
        let changed = inspect(&f.root, &review.digest).unwrap();
        assert!(!changed.archives[0].verified && changed.archives[0].issue.is_some());
    }
    #[test]
    fn repeated_review_keeps_old_archives_separate_from_new_pending_transaction() {
        let f = Fixture::new();
        let first = f.prepare();
        let archived = finish(&f.root, &first.digest, true).unwrap();
        let second = f.prepare();
        assert_eq!(first.digest, second.digest);
        let pending = inspect(&f.root, &second.digest).unwrap();
        assert!(pending.guard_present && pending.active.is_some());
        assert!(!pending.active.unwrap().restoration_complete);
        assert_eq!(
            pending.archives[0].disposition.as_deref(),
            Some("abandoned")
        );
        assert!(pending.archives[0].verified);
        let next = finish(&f.root, &second.digest, true).unwrap();
        assert_ne!(next, archived);
        let result = inspect(&f.root, &second.digest).unwrap();
        assert_eq!(result.archives.len(), 2);
        assert!(result.archives.iter().all(|x| x.verified));
        fs::write(next.join("finalization.json"), b"{").unwrap();
        let result = inspect(&f.root, &second.digest).unwrap();
        assert_eq!(result.archives.iter().filter(|x| x.verified).count(), 1);
    }
    #[test]
    fn finalization_sharing_failure_keeps_original_intent_and_can_be_explicitly_finished() {
        let f = Fixture::new();
        let review = f.prepare();
        apply(&f.root, &review.digest).unwrap();
        let pin = backup::open(&review.transaction.join("plan.json"), false, false).unwrap();
        assert!(finish(&f.root, &review.digest, false).is_err());
        let bytes = fs::read(review.transaction.join("finalization.json")).unwrap();
        assert!(inspect(&f.root, &review.digest).unwrap().guard_present);
        drop(pin);
        let archived = finish(&f.root, &review.digest, false).unwrap();
        assert_eq!(fs::read(archived.join("finalization.json")).unwrap(), bytes);
        assert!(inspect(&f.root, &review.digest).unwrap().archives[0].verified);
    }
    #[test]
    fn incomplete_prepared_record_is_diagnosed_and_abandoned_without_rewriting_it() {
        for partial in [b"".as_slice(), b"{", b"\"wrong-review\""] {
            let f = Fixture::new();
            let review = f.prepare();
            fs::write(review.transaction.join("prepared.json"), partial).unwrap();
            let diagnosis = diagnose(&f.root).unwrap();
            assert!(diagnosis.can_abandon_preparation && !diagnosis.automatic_actions);
            assert_eq!(diagnosis.prepared_record, "invalid");
            assert_eq!(diagnosis.next_action, "review-and-abandon-preparation");
            assert!(apply(&f.root, &review.digest).is_err());
            let archived = finish(&f.root, &review.digest, true).unwrap();
            assert_eq!(fs::read(archived.join("prepared.json")).unwrap(), partial);
            assert!(inspect(&f.root, &review.digest).unwrap().archives[0].verified);
            assert!(f.root.join("upgrade-hold.json").exists());
            fs::write(archived.join("prepared.json"), b"different").unwrap();
            assert!(!inspect(&f.root, &review.digest).unwrap().archives[0].verified);
        }
    }
    #[test]
    fn incomplete_payload_and_missing_receipt_keep_evidence_until_explicit_abandon() {
        let f = Fixture::new();
        let review = f.prepare();
        fs::remove_file(review.transaction.join("prepared.json")).unwrap();
        fs::write(
            review.transaction.join("staged/events/00000001.json"),
            b"partial copy",
        )
        .unwrap();
        let diagnosis = diagnose(&f.root).unwrap();
        assert!(diagnosis.can_abandon_preparation);
        assert_eq!(diagnosis.prepared_record, "missing");
        assert_eq!(diagnosis.next_action, "review-and-abandon-preparation");
        let archive = finish(&f.root, &review.digest, true).unwrap();
        assert_eq!(
            fs::read(archive.join("staged/events/00000001.json")).unwrap(),
            b"partial copy"
        );
        assert!(inspect(&f.root, &review.digest).unwrap().archives[0].verified);
    }
    #[test]
    fn unpublished_metadata_does_not_become_a_guard_or_get_automatically_removed() {
        let f = Fixture::new();
        let review = f.review();
        let partial = review.transaction.with_file_name(format!(
            "{}.initializing-fixture",
            review.transaction.file_name().unwrap().to_string_lossy()
        ));
        fs::create_dir(&partial).unwrap();
        fs::write(partial.join("plan.json"), b"{").unwrap();
        let diagnosis = diagnose(&f.root).unwrap();
        assert!(!diagnosis.guard_present && !diagnosis.automatic_actions);
        assert_eq!(diagnosis.unpublished_metadata, vec![partial.clone()]);
        assert_eq!(
            diagnosis.next_action,
            "review-new-preparation-or-explicit-hold-release"
        );
        let next = f.prepare();
        assert_eq!(fs::read(partial.join("plan.json")).unwrap(), b"{");
        assert!(diagnose(&f.root).unwrap().guard_present);
        finish(&f.root, &next.digest, true).unwrap();
        assert!(partial.exists());
    }
    #[test]
    fn invalid_plan_and_move_intent_do_not_offer_preparation_abandonment() {
        let f = Fixture::new();
        let review = f.prepare();
        fs::write(review.transaction.join("plan.json"), b"{").unwrap();
        let diagnosis = diagnose(&f.root).unwrap();
        assert!(diagnosis.guard_present && diagnosis.issue.is_some());
        assert!(!diagnosis.can_abandon_preparation);
        let other = Fixture::new();
        let review = other.prepare();
        write_new(&review.transaction.join("move-intent.json"), &review.digest).unwrap();
        assert!(!diagnose(&other.root).unwrap().can_abandon_preparation);
        assert!(finish(&other.root, &review.digest, true).is_err());
    }
}
