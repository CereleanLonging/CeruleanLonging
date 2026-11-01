//! Durable program-update reservation, mutually exclusive with data recovery.
//! A reservation outlives the controller process; only verified terminal or
//! unstarted preparation evidence permits explicit archival. Never starts a host.
use crate::{
    backup::{self, Entry, Tree},
    backup_recovery,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
type Result<T> = std::result::Result<T, String>;
fn io<T>(r: std::io::Result<T>) -> Result<T> {
    r.map_err(|e| e.to_string())
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn entries(tree: &Tree) -> Vec<Entry> {
    tree.nodes.iter().map(|n| n.entry.clone()).collect()
}
fn inventory_digest(items: &[Entry]) -> Result<String> {
    Ok(hash(&serde_json::to_vec(items).map_err(|e| e.to_string())?))
}
fn marker(root: &Path) -> Result<PathBuf> {
    let recovery = backup_recovery::guard(root)?;
    let name = recovery
        .file_name()
        .ok_or("Missing recovery guard name")?
        .to_string_lossy();
    Ok(recovery.with_file_name(format!(
        "{}.json",
        name.replacen(".cerulean-recovery-", ".cerulean-program-update-", 1)
    )))
}
pub(crate) fn ensure_not_reserved(root: &Path) -> Result<()> {
    if !io(root.try_exists())? {
        let parent = root
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        if !io(parent.try_exists())? {
            return Ok(());
        }
    }
    match fs::symlink_metadata(marker(root)?) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err("Program update owns this data directory; inspect and finish its original transaction before recovery or startup".into()),
    }
}
fn read(path: &Path) -> Result<(Vec<u8>, Value, File)> {
    let mut pin = io(backup::open(path, false, false))?;
    let mut bytes = Vec::new();
    io((&mut pin).take(4 * 1024 * 1024 + 1).read_to_end(&mut bytes))?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("Maintenance record exceeds limit".into());
    }
    let value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    Ok((bytes, value, pin))
}
fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("Missing maintenance field {key}"))
}
fn validate_intent(value: &Value, root: &Path, transaction: &Path, hold: &str) -> Result<()> {
    let schema_valid = (value["schema"] == 1 && value.get("controllerReaderGeneration").is_none())
        || (value["schema"] == 2
            && value["controllerReaderGeneration"]
                .as_u64()
                .is_some_and(|generation| {
                    (3..=u64::from(crate::DATA_READER_GENERATION)).contains(&generation)
                }));
    if !schema_valid
        || string(value, "ownerSid")? != io(crate::ipc::current_sid())?
        || io(fs::canonicalize(string(value, "data")?))? != root
        || io(fs::canonicalize(string(value, "transaction")?))? != transaction
        || string(value, "holdDigest")? != hold
    {
        return Err("Program transaction identity or hold differs".into());
    }
    Ok(())
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Reservation {
    schema: u32,
    root: PathBuf,
    owner_sid: String,
    transaction: PathBuf,
    intent_digest: String,
    hold_digest: String,
    original_data_digest: String,
    created_at: String,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Observation {
    pub state: String,
    pub guard_path: PathBuf,
    pub guard_digest: Option<String>,
    pub reservation: Option<Reservation>,
    pub current_data_verified: bool,
    pub automatic_actions: bool,
}
fn record(root: &Path, path: &Path) -> Result<(Reservation, String, File)> {
    let (bytes, value, pin) = read(path)?;
    let reservation: Reservation = serde_json::from_value(value).map_err(|e| e.to_string())?;
    if !matches!(reservation.schema, 1 | 2)
        || reservation.root != root
        || reservation.owner_sid != io(crate::ipc::current_sid())?
    {
        return Err("Maintenance reservation identity differs".into());
    }
    Ok((reservation, hash(&bytes), pin))
}
fn archived(root: &Path, digest: &str) -> Result<PathBuf> {
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("Invalid guard digest".into());
    }
    Ok(marker(root)?.with_file_name(format!(".cerulean-program-update-released-{digest}.json")))
}
pub fn status(root: &Path, expected: Option<&str>) -> Result<Observation> {
    let root = backup_recovery::location(root)?;
    let path = marker(&root)?;
    let active = match fs::symlink_metadata(&path) {
        Ok(_) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(e.to_string()),
    };
    let (state, selected) = if active {
        ("reserved", path.clone())
    } else if let Some(digest) = expected {
        ("released", archived(&root, digest)?)
    } else {
        return Ok(Observation {
            state: "unreserved".into(),
            guard_path: path,
            guard_digest: None,
            reservation: None,
            current_data_verified: false,
            automatic_actions: false,
        });
    };
    let (reservation, digest, _pin) = record(&root, &selected)?;
    if expected.is_some_and(|d| d != digest) {
        return Err("Original maintenance reservation digest differs".into());
    }
    Ok(Observation {
        state: state.into(),
        guard_path: selected,
        guard_digest: Some(digest),
        reservation: Some(reservation),
        current_data_verified: false,
        automatic_actions: false,
    })
}
pub fn reserve(
    root: &Path,
    transaction: &Path,
    intent_digest: &str,
    hold: &str,
) -> Result<Observation> {
    let root = io(fs::canonicalize(root))?;
    let transaction = io(fs::canonicalize(transaction))?;
    if transaction.starts_with(&root) || root.starts_with(&transaction) {
        return Err("Program transaction must be outside data".into());
    }
    let _parent = io(backup::open(
        root.parent().ok_or("Missing parent")?,
        true,
        false,
    ))?;
    let _transaction = io(backup::open(&transaction, true, false))?;
    let (bytes, intent, _intent) = read(&transaction.join("intent.json"))?;
    validate_intent(&intent, &root, &transaction, hold)?;
    if hash(&bytes) != intent_digest {
        return Err("Reviewed program intent digest differs".into());
    }
    // Preparing a reservation never accepts a transaction without the complete
    // original program inventory needed for safe preparation abandonment.
    let old_entries = program_entries(&intent, "oldEntries")?;
    if old_entries.is_empty() {
        return Err("Original program inventory is required in update intent".into());
    }
    let install = io(fs::canonicalize(string(&intent, "install")?))?;
    if install.starts_with(&root)
        || root.starts_with(&install)
        || install.starts_with(&transaction)
        || transaction.starts_with(&install)
    {
        return Err("Program, data and transaction paths must be disjoint".into());
    }
    use std::os::windows::fs::MetadataExt;
    let lock_metadata = io(fs::symlink_metadata(transaction.join("transaction.lock")))?;
    if !lock_metadata.is_file()
        || lock_metadata.len() != 0
        || lock_metadata.file_attributes() & 0x400 != 0
    {
        return Err("Original ordinary empty transaction lock is required".into());
    }
    let _program = program_matches(&install, &old_entries)?;
    let original = io(backup::inventory(&root, true))?;
    backup_recovery::ensure_no_recovery(&root)?;
    let path = marker(&root)?;
    if io(path.try_exists())? {
        let observed = status(&root, None)?;
        let existing = observed.reservation.as_ref().ok_or("Reservation missing")?;
        if existing.transaction != transaction
            || Some(existing.schema as u64) != intent["schema"].as_u64()
            || existing.intent_digest != intent_digest
            || existing.hold_digest != hold
        {
            return Err("Another program update already owns this data directory".into());
        }
        return Ok(observed);
    }
    let held = crate::upgrade::status(&root)?;
    if !held.valid_record || held.hold_digest != hold {
        return Err("Valid original maintenance hold required".into());
    }
    let reservation = Reservation {
        schema: intent["schema"]
            .as_u64()
            .ok_or("Missing validated update schema")? as u32,
        root: root.clone(),
        owner_sid: io(crate::ipc::current_sid())?,
        transaction,
        intent_digest: intent_digest.into(),
        hold_digest: hold.into(),
        original_data_digest: inventory_digest(&entries(&original))?,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    let bytes = serde_json::to_vec_pretty(&reservation).map_err(|e| e.to_string())?;
    let temporary = path.with_extension(format!(
        "pending-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let mut output = io(OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary))?;
    io(output.write_all(&bytes).and_then(|_| output.sync_all()))?;
    drop(output);
    io(backup::publish(&temporary, &path))?;
    status(&root, Some(&hash(&bytes)))
}
pub fn check(root: &Path, expected: &str) -> Result<Observation> {
    let root = io(fs::canonicalize(root))?;
    let original = io(backup::inventory(&root, true))?;
    backup_recovery::ensure_no_recovery(&root)?;
    let observed = status(&root, Some(expected))?;
    let reservation = observed.reservation.as_ref().ok_or("Reservation missing")?;
    if observed.state != "reserved"
        || inventory_digest(&entries(&original))? != reservation.original_data_digest
    {
        return Err("Reserved original data changed".into());
    }
    let (bytes, intent, _pin) = read(&reservation.transaction.join("intent.json"))?;
    validate_intent(
        &intent,
        &root,
        &reservation.transaction,
        &reservation.hold_digest,
    )?;
    if hash(&bytes) != reservation.intent_digest || intent["schema"] != reservation.schema {
        return Err("Reserved intent schema or bytes changed".into());
    }
    let held = crate::upgrade::status(&root)?;
    if !held.valid_record || held.hold_digest != reservation.hold_digest {
        return Err("Original maintenance hold changed".into());
    }
    Ok(observed)
}
fn program_entries(value: &Value, field: &str) -> Result<Vec<Entry>> {
    let mut array = value[field]
        .as_array()
        .ok_or("Program inventory missing")?
        .clone();
    for entry in &mut array {
        entry["path"] = Value::String(string(entry, "path")?.replace('\\', "/"));
    }
    let mut entries: Vec<Entry> =
        serde_json::from_value(Value::Array(array)).map_err(|e| e.to_string())?;
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}
fn program_matches(install: &Path, expected: &[Entry]) -> Result<Tree> {
    let tree = io(backup::inventory(install, false))?;
    if entries(&tree) != expected {
        return Err("Installed program differs from transaction evidence".into());
    }
    Ok(tree)
}
fn transition(from: &str, to: &str) -> bool {
    match from {
        "initial" => to == "prepared",
        "prepared" => matches!(to, "apply-intent" | "rollback-intent"),
        "apply-intent" => matches!(to, "old-preserved" | "rollback-intent"),
        "old-preserved" => matches!(to, "new-published" | "rollback-intent"),
        "new-published" => matches!(to, "installed-held" | "rollback-intent"),
        "installed-held" => matches!(to, "commit-intent" | "rollback-intent"),
        "rollback-intent" => matches!(to, "rollback-intent" | "rolled-back"),
        "commit-intent" => matches!(to, "committed" | "commit-abandoned"),
        "commit-abandoned" => to == "rollback-intent",
        _ => false,
    }
}
fn journal(transaction: &Path, plan_digest: &str, schema: u32) -> Result<String> {
    let mut paths = io(fs::read_dir(transaction))?
        .map(|p| p.map(|p| p.path()))
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    paths.retain(|p| {
        p.file_name().is_some_and(|n| {
            n.to_string_lossy().starts_with("event-") && n.to_string_lossy().ends_with(".json")
        })
    });
    paths.sort();
    if paths.is_empty() || paths.len() > 64 {
        return Err("Program journal is incomplete or exceeds limit".into());
    }
    let mut previous = plan_digest.to_owned();
    let mut state = "initial".to_owned();
    for (index, path) in paths.iter().enumerate() {
        let (bytes, value, _pin) = read(path)?;
        let kind = string(&value, "kind")?;
        if path.file_name().and_then(|n| n.to_str())
            != Some(format!("event-{:03}.json", index + 1).as_str())
            || value["schema"] != schema
            || value["sequence"].as_u64() != Some((index + 1) as u64)
            || string(&value, "previousDigest")? != previous
            || !transition(&state, kind)
        {
            return Err("Program journal chain or transition differs".into());
        }
        previous = hash(&bytes);
        state = kind.into();
    }
    Ok(state)
}
/// Invoked only after the controller has released its operation/data locks.
/// Neither the original program/data nor the update journal is modified here.
pub fn release(root: &Path, expected: &str, plan_digest: Option<&str>) -> Result<Observation> {
    let root = io(fs::canonicalize(root))?;
    let original = io(backup::inventory(&root, true))?;
    backup_recovery::ensure_no_recovery(&root)?;
    let path = marker(&root)?;
    let _parent = io(backup::open(
        root.parent().ok_or("Missing parent")?,
        true,
        false,
    ))?;
    let (reservation, actual, guard_pin) = record(&root, &path)?;
    if actual != expected {
        return Err("Reservation digest differs; no release performed".into());
    }
    let _transaction = io(backup::open(&reservation.transaction, true, false))?;
    let _operation = io(backup::open(
        &reservation.transaction.join("transaction.lock"),
        false,
        true,
    ))?;
    let (intent_bytes, intent, _intent) = read(&reservation.transaction.join("intent.json"))?;
    validate_intent(
        &intent,
        &root,
        &reservation.transaction,
        &reservation.hold_digest,
    )?;
    if hash(&intent_bytes) != reservation.intent_digest || intent["schema"] != reservation.schema {
        return Err("Reserved intent changed".into());
    }
    let install = PathBuf::from(string(&intent, "install")?);
    let _program;
    if let Some(expected_plan) = plan_digest {
        let (bytes, plan, _plan) = read(&reservation.transaction.join("plan.json"))?;
        if hash(&bytes) != expected_plan {
            return Err("Reviewed program plan changed".into());
        }
        validate_intent(
            &plan,
            &root,
            &reservation.transaction,
            &reservation.hold_digest,
        )?;
        for (key, value) in intent.as_object().ok_or("Intent must be an object")? {
            if key != "backupDigest" && key != "newEntries" && plan[key] != *value {
                return Err("Final program plan differs from reserved intent".into());
            }
        }
        let state = journal(&reservation.transaction, expected_plan, reservation.schema)?;
        if !matches!(state.as_str(), "committed" | "rolled-back") {
            return Err("Program update has not reached a terminal state".into());
        }
        _program = program_matches(
            &install,
            &program_entries(
                &plan,
                if state == "committed" {
                    "newEntries"
                } else {
                    "oldEntries"
                },
            )?,
        )?;
        let (manifest, review, _backup, _archive, _manifest) = io(backup::verified(
            &reservation.transaction.join("data-backup"),
        ))?;
        if review.manifest_digest != string(&plan, "backupDigest")?
            || manifest.source_root != root
            || manifest.owner_sid != reservation.owner_sid
            || inventory_digest(&manifest.entries)? != reservation.original_data_digest
        {
            return Err("Original reserved backup differs".into());
        }
        let mut actual_entries = entries(&original);
        let mut expected_entries = manifest.entries;
        if state == "committed" {
            let hold = expected_entries
                .iter()
                .find(|e| e.path == "upgrade-hold.json")
                .ok_or("Original hold absent from backup")?
                .clone();
            let added: Vec<_> = actual_entries
                .iter()
                .filter(|e| !expected_entries.iter().any(|b| b.path == e.path))
                .collect();
            let released: Vec<_> = added
                .iter()
                .filter(|e| {
                    !e.directory
                        && e.path.starts_with(&format!(
                            "upgrade-holds/released-{}-",
                            reservation.hold_digest
                        ))
                        && e.sha256 == reservation.hold_digest
                        && e.bytes == hold.bytes
                })
                .collect();
            if released.len() != 1 {
                return Err("Exactly one new original hold archive is required".into());
            }
            let released_path = released[0].path.clone();
            expected_entries.retain(|e| e.path != "upgrade-hold.json");
            actual_entries.retain(|e| e.path != released_path);
        } else {
            let hold = crate::upgrade::status(&root)?;
            if !hold.valid_record || hold.hold_digest != reservation.hold_digest {
                return Err("Rollback must retain the original hold".into());
            }
        }
        if !expected_entries.iter().any(|e| e.path == "upgrade-holds") {
            actual_entries.retain(|e| !(e.path == "upgrade-holds" && e.directory));
        }
        if actual_entries != expected_entries {
            return Err("Terminal data inventory differs from original evidence".into());
        }
    } else {
        for item in io(fs::read_dir(&reservation.transaction))? {
            let name = io(item)?.file_name();
            let name = name.to_string_lossy();
            if name.starts_with("event-")
                || matches!(name.as_ref(), "previous-program" | "failed-program")
            {
                return Err(
                    "Preparation has execution evidence; explicit terminal recovery required"
                        .into(),
                );
            }
        }
        _program = program_matches(&install, &program_entries(&intent, "oldEntries")?)?;
        if inventory_digest(&entries(&original))? != reservation.original_data_digest {
            return Err("Preparation data changed; original evidence review required".into());
        }
        let hold = crate::upgrade::status(&root)?;
        if !hold.valid_record || hold.hold_digest != reservation.hold_digest {
            return Err("Original hold must remain during abandonment".into());
        }
    }
    let destination = archived(&root, expected)?;
    drop(guard_pin);
    io(backup::publish(&path, &destination))?;
    status(&root, Some(expected))
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        _temp: tempfile::TempDir,
        data: PathBuf,
        tx: PathBuf,
        install: PathBuf,
        hold: String,
        intent: Value,
        intent_digest: String,
    }
    fn write(path: &Path, value: &Value) -> String {
        let bytes = serde_json::to_vec_pretty(value).unwrap();
        fs::write(path, &bytes).unwrap();
        hash(&bytes)
    }
    impl Fixture {
        fn new() -> Self {
            Self::in_directory(tempfile::tempdir().unwrap())
        }
        fn in_directory(temp: tempfile::TempDir) -> Self {
            let data = temp.path().join("数据");
            drop(crate::Store::open(&data).unwrap());
            let data = fs::canonicalize(data).unwrap();
            let mut review = crate::upgrade::UpgradeReview {
                digest: String::new(),
                ready: true,
                blockers: vec![],
                snapshot_digest: "a".repeat(64),
                core_version: env!("CARGO_PKG_VERSION").into(),
                core_sha256: "b".repeat(64),
                server_pid: std::process::id(),
            };
            review.digest = cerulean_domain::execution::digest(&review).unwrap();
            let hold = crate::upgrade::prepare(&data, review).unwrap().hold_digest;
            let install = temp.path().join("installed");
            fs::create_dir(&install).unwrap();
            fs::write(install.join("program.exe"), b"original fixture program").unwrap();
            let tx = temp.path().join("transaction");
            fs::create_dir(&tx).unwrap();
            fs::write(tx.join("transaction.lock"), []).unwrap();
            let original = backup::inventory(&install, false).unwrap();
            let intent = serde_json::json!({"schema":1,"ownerSid":crate::ipc::current_sid().unwrap(),"transaction":tx,"install":install,"data":data,"holdDigest":hold,"sourceVersion":"0.1.6","targetVersion":"0.1.7","sourceManifest":"a".repeat(64),"targetManifest":"b".repeat(64),"backupDigest":"","oldEntries":entries(&original),"newEntries":[]});
            drop(original);
            let intent_digest = write(&tx.join("intent.json"), &intent);
            Self {
                _temp: temp,
                data,
                tx,
                install,
                hold,
                intent,
                intent_digest,
            }
        }
        fn reserve(&self) -> String {
            reserve(&self.data, &self.tx, &self.intent_digest, &self.hold)
                .unwrap()
                .guard_digest
                .unwrap()
        }
        fn terminal(&self, committed: bool) -> String {
            let backup =
                backup::create_held(&self.data, &self.tx.join("data-backup"), &self.hold).unwrap();
            if committed {
                fs::write(
                    self.install.join("program.exe"),
                    b"candidate fixture program",
                )
                .unwrap();
            }
            let current = backup::inventory(&self.install, false).unwrap();
            let mut plan = self.intent.clone();
            plan["backupDigest"] = backup.manifest_digest.into();
            plan["newEntries"] = serde_json::to_value(entries(&current)).unwrap();
            drop(current);
            let plan_digest = write(&self.tx.join("plan.json"), &plan);
            let kinds: &[&str] = if committed {
                &[
                    "prepared",
                    "apply-intent",
                    "old-preserved",
                    "new-published",
                    "installed-held",
                    "commit-intent",
                    "committed",
                ]
            } else {
                &["prepared", "rollback-intent", "rolled-back"]
            };
            let mut previous = plan_digest.clone();
            for (index, kind) in kinds.iter().enumerate() {
                previous = write(
                    &self.tx.join(format!("event-{:03}.json", index + 1)),
                    &serde_json::json!({"schema":self.intent["schema"],"sequence":index+1,"previousDigest":previous,"kind":kind}),
                );
            }
            if committed {
                fs::create_dir(self.data.join("upgrade-holds")).unwrap();
                fs::rename(
                    self.data.join("upgrade-hold.json"),
                    self.data
                        .join(format!("upgrade-holds/released-{}-fixture.json", self.hold)),
                )
                .unwrap();
            }
            plan_digest
        }
    }
    #[test]
    fn reader_aware_update_schema_binds_reservation_intent_and_terminal_journal() {
        for (generation, committed) in [3, crate::currency_api_resolution_guard::GENERATION]
            .into_iter()
            .flat_map(|g| [false, true].map(|c| (g, c)))
        {
            let mut f = Fixture::new();
            f.intent["schema"] = 2.into();
            f.intent["controllerReaderGeneration"] = generation.into();
            f.intent_digest = write(&f.tx.join("intent.json"), &f.intent);
            let guard = f.reserve();
            assert_eq!(
                status(&f.data, Some(&guard))
                    .unwrap()
                    .reservation
                    .unwrap()
                    .schema,
                2
            );
            check(&f.data, &guard).unwrap();
            let plan = f.terminal(committed);
            let event = f.tx.join("event-001.json");
            let original = fs::read(&event).unwrap();
            let mut changed: Value = serde_json::from_slice(&original).unwrap();
            changed["schema"] = 1.into();
            write(&event, &changed);
            assert!(release(&f.data, &guard, Some(&plan)).is_err());
            assert!(marker(&f.data).unwrap().exists());
            fs::write(&event, original).unwrap();
            assert_eq!(
                release(&f.data, &guard, Some(&plan)).unwrap().state,
                "released"
            );
        }
    }
    #[test]
    fn unsupported_or_changed_controller_contract_never_adopts_a_reservation() {
        let mut f = Fixture::new();
        for (schema, reader) in [
            (1, Some(3)),
            (2, None),
            (2, Some(2)),
            (2, Some(crate::DATA_READER_GENERATION + 1)),
            (99, Some(3)),
        ] {
            let mut bad = f.intent.clone();
            bad["schema"] = schema.into();
            if let Some(reader) = reader {
                bad["controllerReaderGeneration"] = reader.into();
            }
            let digest = write(&f.tx.join("intent.json"), &bad);
            assert!(reserve(&f.data, &f.tx, &digest, &f.hold).is_err());
            assert!(!marker(&f.data).unwrap().exists());
        }
        f.intent["schema"] = 2.into();
        f.intent["controllerReaderGeneration"] = 3.into();
        f.intent_digest = write(&f.tx.join("intent.json"), &f.intent);
        let guard = f.reserve();
        let mut changed = f.intent.clone();
        changed["controllerReaderGeneration"] = 4.into();
        write(&f.tx.join("intent.json"), &changed);
        assert!(check(&f.data, &guard).is_err());
        assert!(release(&f.data, &guard, None).is_err());
        write(&f.tx.join("intent.json"), &f.intent);
        assert_eq!(release(&f.data, &guard, None).unwrap().state, "released");
    }
    #[test]
    fn reader4_reservation_preserves_original_contract_and_rejects_rebinding() {
        let mut f = Fixture::new();
        f.intent["schema"] = 2.into();
        for invalid in [
            serde_json::json!("4"),
            serde_json::json!(true),
            serde_json::json!(4.5),
        ] {
            f.intent["controllerReaderGeneration"] = invalid;
            let digest = write(&f.tx.join("intent.json"), &f.intent);
            assert!(reserve(&f.data, &f.tx, &digest, &f.hold).is_err());
            assert!(!marker(&f.data).unwrap().exists());
        }
        f.intent["controllerReaderGeneration"] = 4.into();
        f.intent_digest = write(&f.tx.join("intent.json"), &f.intent);
        let original = fs::read(f.tx.join("intent.json")).unwrap();
        let guard = f.reserve();
        assert_eq!(f.reserve(), guard);
        check(&f.data, &guard).unwrap();
        let mut changed = f.intent.clone();
        changed["controllerReaderGeneration"] = 3.into();
        write(&f.tx.join("intent.json"), &changed);
        assert!(check(&f.data, &guard).is_err());
        assert!(release(&f.data, &guard, None).is_err());
        fs::write(f.tx.join("intent.json"), &original).unwrap();
        assert_eq!(release(&f.data, &guard, None).unwrap().state, "released");
        assert_eq!(fs::read(f.tx.join("intent.json")).unwrap(), original);
    }
    #[test]
    fn reader5_reservation_preserves_contract_and_refuses_future_or_string_generation() {
        let mut f = Fixture::new();
        f.intent["schema"] = 2.into();
        for value in [
            serde_json::json!(crate::DATA_READER_GENERATION + 1),
            serde_json::json!("5"),
            serde_json::json!(5.5),
        ] {
            f.intent["controllerReaderGeneration"] = value;
            let hash = write(&f.tx.join("intent.json"), &f.intent);
            assert!(reserve(&f.data, &f.tx, &hash, &f.hold).is_err());
        }
        f.intent["controllerReaderGeneration"] = 5.into();
        f.intent_digest = write(&f.tx.join("intent.json"), &f.intent);
        let original = fs::read(f.tx.join("intent.json")).unwrap();
        let guard = f.reserve();
        check(&f.data, &guard).unwrap();
        f.intent["controllerReaderGeneration"] = 4.into();
        write(&f.tx.join("intent.json"), &f.intent);
        assert!(check(&f.data, &guard).is_err());
        fs::write(f.tx.join("intent.json"), original).unwrap();
        assert_eq!(release(&f.data, &guard, None).unwrap().state, "released");
    }
    #[test]
    fn reader6_reservation_preserves_contract_and_refuses_future_or_string_generation() {
        let mut f = Fixture::new();
        f.intent["schema"] = 2.into();
        for value in [
            serde_json::json!(crate::DATA_READER_GENERATION + 1),
            serde_json::json!("6"),
            serde_json::json!(6.5),
        ] {
            f.intent["controllerReaderGeneration"] = value;
            let hash = write(&f.tx.join("intent.json"), &f.intent);
            assert!(reserve(&f.data, &f.tx, &hash, &f.hold).is_err());
        }
        f.intent["controllerReaderGeneration"] = 6.into();
        f.intent_digest = write(&f.tx.join("intent.json"), &f.intent);
        let original = fs::read(f.tx.join("intent.json")).unwrap();
        let guard = f.reserve();
        check(&f.data, &guard).unwrap();
        f.intent["controllerReaderGeneration"] = 4.into();
        write(&f.tx.join("intent.json"), &f.intent);
        assert!(check(&f.data, &guard).is_err());
        fs::write(f.tx.join("intent.json"), original).unwrap();
        assert_eq!(release(&f.data, &guard, None).unwrap().state, "released");
    }
    #[test]
    fn reader7_reservation_preserves_contract_and_refuses_future_or_string_generation() {
        let mut f = Fixture::new();
        f.intent["schema"] = 2.into();
        for value in [
            serde_json::json!(crate::DATA_READER_GENERATION + 1),
            serde_json::json!("7"),
            serde_json::json!(7.5),
        ] {
            f.intent["controllerReaderGeneration"] = value;
            let hash = write(&f.tx.join("intent.json"), &f.intent);
            assert!(reserve(&f.data, &f.tx, &hash, &f.hold).is_err());
        }
        f.intent["controllerReaderGeneration"] = 7.into();
        f.intent_digest = write(&f.tx.join("intent.json"), &f.intent);
        let original = fs::read(f.tx.join("intent.json")).unwrap();
        let guard = f.reserve();
        check(&f.data, &guard).unwrap();
        f.intent["controllerReaderGeneration"] = 4.into();
        write(&f.tx.join("intent.json"), &f.intent);
        assert!(check(&f.data, &guard).is_err());
        fs::write(f.tx.join("intent.json"), original).unwrap();
        assert_eq!(release(&f.data, &guard, None).unwrap().state, "released");
    }
    #[test]
    fn reader8_reservation_preserves_contract_and_refuses_future_or_string_generation() {
        let mut f = Fixture::new();
        f.intent["schema"] = 2.into();
        for value in [
            serde_json::json!(crate::DATA_READER_GENERATION + 1),
            serde_json::json!("8"),
            serde_json::json!(8.5),
        ] {
            f.intent["controllerReaderGeneration"] = value;
            let hash = write(&f.tx.join("intent.json"), &f.intent);
            assert!(reserve(&f.data, &f.tx, &hash, &f.hold).is_err());
        }
        f.intent["controllerReaderGeneration"] = 8.into();
        f.intent_digest = write(&f.tx.join("intent.json"), &f.intent);
        let original = fs::read(f.tx.join("intent.json")).unwrap();
        let guard = f.reserve();
        check(&f.data, &guard).unwrap();
        f.intent["controllerReaderGeneration"] = 4.into();
        write(&f.tx.join("intent.json"), &f.intent);
        assert!(check(&f.data, &guard).is_err());
        fs::write(f.tx.join("intent.json"), original).unwrap();
        assert_eq!(release(&f.data, &guard, None).unwrap().state, "released");
    }
    #[test]
    fn reader9_reservation_preserves_contract_and_refuses_future_or_string_generation() {
        let mut f = Fixture::new();
        f.intent["schema"] = 2.into();
        for value in [
            serde_json::json!(crate::DATA_READER_GENERATION + 1),
            serde_json::json!("9"),
            serde_json::json!(9.5),
        ] {
            f.intent["controllerReaderGeneration"] = value;
            let hash = write(&f.tx.join("intent.json"), &f.intent);
            assert!(reserve(&f.data, &f.tx, &hash, &f.hold).is_err());
        }
        f.intent["controllerReaderGeneration"] = 9.into();
        f.intent_digest = write(&f.tx.join("intent.json"), &f.intent);
        let original = fs::read(f.tx.join("intent.json")).unwrap();
        let guard = f.reserve();
        check(&f.data, &guard).unwrap();
        f.intent["controllerReaderGeneration"] = 4.into();
        write(&f.tx.join("intent.json"), &f.intent);
        assert!(check(&f.data, &guard).is_err());
        fs::write(f.tx.join("intent.json"), original).unwrap();
        assert_eq!(release(&f.data, &guard, None).unwrap().state, "released");
    }
    #[test]
    fn reader10_reservation_preserves_contract_and_refuses_future_or_string_generation() {
        let mut f = Fixture::new();
        f.intent["schema"] = 2.into();
        for value in [
            serde_json::json!(crate::DATA_READER_GENERATION + 1),
            serde_json::json!("10"),
            serde_json::json!(10.5),
        ] {
            f.intent["controllerReaderGeneration"] = value;
            let hash = write(&f.tx.join("intent.json"), &f.intent);
            assert!(reserve(&f.data, &f.tx, &hash, &f.hold).is_err());
        }
        f.intent["controllerReaderGeneration"] = 10.into();
        f.intent_digest = write(&f.tx.join("intent.json"), &f.intent);
        let original = fs::read(f.tx.join("intent.json")).unwrap();
        let guard = f.reserve();
        check(&f.data, &guard).unwrap();
        f.intent["controllerReaderGeneration"] = 4.into();
        write(&f.tx.join("intent.json"), &f.intent);
        assert!(check(&f.data, &guard).is_err());
        fs::write(f.tx.join("intent.json"), original).unwrap();
        assert_eq!(release(&f.data, &guard, None).unwrap().state, "released");
    }
    #[test]
    fn reader11_reservation_preserves_contract_and_refuses_future_or_string_generation() {
        api_reader_reservation(11);
    }
    #[test]
    fn reader12_reservation_preserves_contract_and_refuses_future_or_string_generation() {
        api_reader_reservation(crate::currency_api_resolution_guard::GENERATION);
    }
    #[test]
    fn reader13_reservation_preserves_contract_and_refuses_future_or_string_generation() {
        api_reader_reservation(crate::management_source_guard::GENERATION);
    }
    #[test]
    fn reader14_reservation_preserves_contract_and_refuses_future_or_string_generation() {
        api_reader_reservation(crate::rejected_usage_guard::GENERATION);
    }
    #[test]
    fn reader18_reservation_preserves_contract_and_refuses_future_or_string_generation() {
        api_reader_reservation(crate::DATA_READER_GENERATION);
    }
    fn api_reader_reservation(generation: u32) {
        let mut f = Fixture::new();
        f.intent["schema"] = 2.into();
        for value in [
            serde_json::json!(crate::DATA_READER_GENERATION + 1),
            serde_json::json!(generation.to_string()),
            serde_json::json!(f64::from(generation) + 0.5),
        ] {
            f.intent["controllerReaderGeneration"] = value;
            let hash = write(&f.tx.join("intent.json"), &f.intent);
            assert!(reserve(&f.data, &f.tx, &hash, &f.hold).is_err());
        }
        f.intent["controllerReaderGeneration"] = generation.into();
        f.intent_digest = write(&f.tx.join("intent.json"), &f.intent);
        let original = fs::read(f.tx.join("intent.json")).unwrap();
        let guard = f.reserve();
        check(&f.data, &guard).unwrap();
        f.intent["controllerReaderGeneration"] = 4.into();
        write(&f.tx.join("intent.json"), &f.intent);
        assert!(check(&f.data, &guard).is_err());
        fs::write(f.tx.join("intent.json"), original).unwrap();
        assert_eq!(release(&f.data, &guard, None).unwrap().state, "released");
    }
    #[test]
    #[ignore = "Owned schema-2 controller review fixture; actual CLI owns reservation creation"]
    fn export_reader_update_fixture() {
        let run = PathBuf::from(std::env::var_os("CERULEAN_READER_UPDATE_DIR").unwrap());
        assert!(!run.exists(), "Preserve existing fixture");
        fs::create_dir(&run).unwrap();
        let temp = tempfile::Builder::new()
            .prefix("controller-")
            .tempdir_in(&run)
            .unwrap();
        let mut f = Fixture::in_directory(temp);
        f.intent["schema"] = 2.into();
        f.intent["controllerReaderGeneration"] = 3.into();
        f.intent["maintenanceInterlockSchema"] = 1.into();
        f.intent["maintenanceCore"] = std::env::var("CERULEAN_READER_UPDATE_CORE").unwrap().into();
        f.intent["maintenanceCoreSha256"] = std::env::var("CERULEAN_READER_UPDATE_CORE_SHA256")
            .unwrap()
            .into();
        f.intent["internal"] = true.into();
        f.intent["publisher"] = "".into();
        f.intent_digest = write(&f.tx.join("intent.json"), &f.intent);
        let value = serde_json::json!({"data":f.data,"transaction":f.tx,"install":f.install,"holdDigest":f.hold,"intentDigest":f.intent_digest});
        fs::write(
            run.join("fixture.json"),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
        let _retained = f._temp.keep();
    }
    #[test]
    fn reservation_outlives_calls_and_blocks_recovery_startup_and_hold_release() {
        let f = Fixture::new();
        let backup = f.tx.parent().unwrap().join("recovery-backup");
        let manifest = backup::create_held(&f.data, &backup, &f.hold)
            .unwrap()
            .manifest_digest;
        let review = backup_recovery::review(&f.data, &backup, &manifest, &f.hold).unwrap();
        let guard = f.reserve();
        assert_eq!(f.reserve(), guard);
        assert!(
            backup_recovery::prepare(&f.data, &backup, &manifest, &f.hold, &review.digest).is_err()
        );
        assert!(crate::Store::open(&f.data).is_err());
        assert!(crate::upgrade::release(&f.data, &f.hold).is_err());
        assert_eq!(
            backup_recovery::diagnose(&f.data).unwrap().next_action,
            "review-original-program-update"
        );
        assert_eq!(check(&f.data, &guard).unwrap().state, "reserved");
        assert_eq!(release(&f.data, &guard, None).unwrap().state, "released");
        assert!(f.data.join("upgrade-hold.json").exists());
        assert_eq!(status(&f.data, Some(&guard)).unwrap().state, "released");
        assert!(
            backup_recovery::prepare(&f.data, &backup, &manifest, &f.hold, &review.digest).is_ok()
        );
    }
    #[test]
    fn data_recovery_prevents_program_reservation_until_explicitly_finished() {
        let f = Fixture::new();
        let archive = f.tx.parent().unwrap().join("backup");
        let manifest = backup::create_held(&f.data, &archive, &f.hold)
            .unwrap()
            .manifest_digest;
        let review = backup_recovery::review(&f.data, &archive, &manifest, &f.hold).unwrap();
        backup_recovery::prepare(&f.data, &archive, &manifest, &f.hold, &review.digest).unwrap();
        assert!(reserve(&f.data, &f.tx, &f.intent_digest, &f.hold).is_err());
        assert_eq!(status(&f.data, None).unwrap().state, "unreserved");
        backup_recovery::finish(&f.data, &review.digest, true).unwrap();
        f.reserve();
    }
    #[test]
    fn changed_data_program_or_execution_evidence_cannot_abandon_preparation() {
        for mode in 0..3 {
            let f = Fixture::new();
            let guard = f.reserve();
            match mode {
                0 => fs::write(f.data.join("unexpected"), b"preserve").unwrap(),
                1 => fs::write(f.install.join("program.exe"), b"changed").unwrap(),
                _ => {
                    write(
                        &f.tx.join("event-001.json"),
                        &serde_json::json!({"kind":"prepared"}),
                    );
                }
            }
            assert!(release(&f.data, &guard, None).is_err());
            assert_eq!(status(&f.data, Some(&guard)).unwrap().state, "reserved");
            assert!(f.data.join("upgrade-hold.json").exists());
        }
    }
    #[test]
    fn terminal_commit_and_rollback_must_match_program_data_and_journal() {
        for committed in [false, true] {
            let f = Fixture::new();
            let guard = f.reserve();
            let plan = f.terminal(committed);
            assert!(crate::Store::open(&f.data).is_err());
            assert!(release(&f.data, &guard, Some(&"0".repeat(64))).is_err());
            assert_eq!(
                release(&f.data, &guard, Some(&plan)).unwrap().state,
                "released"
            );
            assert_eq!(f.data.join("upgrade-hold.json").exists(), !committed);
            if committed {
                drop(crate::Store::open(&f.data).unwrap());
            }
        }
    }
    #[test]
    fn invalid_journal_or_occupied_transaction_keeps_reservation() {
        let f = Fixture::new();
        let guard = f.reserve();
        let plan = f.terminal(false);
        let lock = backup::open(&f.tx.join("transaction.lock"), false, true).unwrap();
        assert!(release(&f.data, &guard, Some(&plan)).is_err());
        drop(lock);
        write(
            &f.tx.join("event-003.json"),
            &serde_json::json!({"schema":1,"sequence":3,"previousDigest":"wrong","kind":"rolled-back"}),
        );
        assert!(release(&f.data, &guard, Some(&plan)).is_err());
        assert_eq!(status(&f.data, Some(&guard)).unwrap().state, "reserved");
    }
    #[test]
    fn wrong_intent_and_live_writer_cannot_acquire_and_corrupt_marker_blocks_start() {
        let f = Fixture::new();
        assert!(reserve(&f.data, &f.tx, &"0".repeat(64), &f.hold).is_err());
        let lock = backup::open(&f.data.join("writer.lock"), false, true).unwrap();
        assert!(reserve(&f.data, &f.tx, &f.intent_digest, &f.hold).is_err());
        drop(lock);
        let guard = f.reserve();
        fs::write(marker(&f.data).unwrap(), b"{").unwrap();
        assert!(status(&f.data, Some(&guard)).is_err());
        assert!(crate::Store::open(&f.data).is_err());
        assert!(release(&f.data, &guard, None).is_err());
    }
}
