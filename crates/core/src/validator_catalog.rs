//! Immutable user registrations. A registration is not execution approval.
use crate::host::ordinary;
use cerulean_domain::execution::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
};

const MAX_RECORD: usize = 64 * 1024;
const MAX_TOOLS: usize = 32;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    schema_version: u32,
    registration: RegisteredValidator,
    digest: String,
}

pub(crate) fn load(root: &Path) -> Result<BTreeMap<String, RegisteredValidator>, String> {
    let dir = root.join("validators");
    if !dir.exists() {
        fs::create_dir(&dir).map_err(|e| e.to_string())?;
    }
    ordinary(&dir, true)?;
    let mut registrations = BTreeMap::new();
    for entry in fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_some_and(|ext| ext == "tmp") {
            continue;
        }
        require(
            path.extension().is_some_and(|ext| ext == "json"),
            "invalid validator catalog entry",
        )?;
        ordinary(&path, false)?;
        let mut bytes = Vec::new();
        fs::File::open(&path)
            .map_err(|e| e.to_string())?
            .take(MAX_RECORD as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        require(bytes.len() <= MAX_RECORD, "validator record too large")?;
        let record: Record = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        record.registration.validate()?;
        require(
            record.schema_version == 1
                && record.digest == digest(&record.registration)?
                && path.file_stem().and_then(|s| s.to_str())
                    == Some(record.registration.id.as_str()),
            "validator registration evidence mismatch",
        )?;
        require(
            registrations
                .insert(record.registration.id.clone(), record.registration)
                .is_none(),
            "duplicate validator registration",
        )?;
        require(registrations.len() <= MAX_TOOLS, "validator catalog full")?;
    }
    Ok(registrations)
}

pub(crate) fn save(
    root: &Path,
    registrations: &mut BTreeMap<String, RegisteredValidator>,
    item: RegisteredValidator,
) -> Result<(), String> {
    item.validate()?;
    if let Some(previous) = registrations.get(&item.id) {
        return require(
            previous.name == item.name && previous.tool == item.tool,
            "工具登记已存在；变更请使用新登记，不覆盖历史",
        );
    }
    require(registrations.len() < MAX_TOOLS, "最多登记 32 个工具版本")?;
    let dir = root.join("validators");
    ordinary(&dir, true)?;
    let dest = dir.join(format!("{}.json", item.id));
    require(
        !dest.exists(),
        "validator registration already on disk; reopen core",
    )?;
    let record = Record {
        schema_version: 1,
        digest: digest(&item)?,
        registration: item.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&record).map_err(|e| e.to_string())?;
    require(bytes.len() <= MAX_RECORD, "validator record too large")?;
    let temp = dir.join(format!(
        "{}.{}.tmp",
        item.id,
        chrono::Utc::now()
            .timestamp_nanos_opt()
            .ok_or("clock out of range")?
    ));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    drop(file);
    fs::rename(temp, dest).map_err(|e| e.to_string())?;
    registrations.insert(item.id.clone(), item);
    Ok(())
}
