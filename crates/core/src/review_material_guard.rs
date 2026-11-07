//! Reader24 protects the independent-check summary attached to blind-review material.
use cerulean_domain::execution::{digest, require};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};
pub const GENERATION: u32 = 24;
pub const FILE_NAME: &str = "00000000.reader24.json";
pub const API_LIMITS_GENERATION: u32 = 25;
pub const API_LIMITS_FILE_NAME: &str = "00000000.reader25.json";
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Guard {
    schema: u32,
    product: String,
    generation: u32,
    root: String,
    owner: String,
}
pub(crate) fn observe(root: &Path, status: &mut crate::data_format::Status) -> Result<(), String> {
    observe_guard(root, status, FILE_NAME, GENERATION)?;
    observe_guard(root, status, API_LIMITS_FILE_NAME, API_LIMITS_GENERATION)
}
fn observe_guard(
    root: &Path,
    status: &mut crate::data_format::Status,
    name: &str,
    generation: u32,
) -> Result<(), String> {
    let path = root.join("events").join(name);
    if !path.exists() {
        return Ok(());
    }
    crate::host::ordinary(&path, false)?;
    let mut bytes = vec![];
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(8193)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    require(bytes.len() <= 8192, "检查材料摘要阅读器保护超限")?;
    let g: Guard = serde_json::from_slice(&bytes).map_err(|_| "检查材料摘要阅读器保护损坏")?;
    require(
        g.schema == 1
            && g.product == cerulean_domain::PRODUCT_NAME
            && g.generation == generation
            && Path::new(&g.root) == fs::canonicalize(root).map_err(|e| e.to_string())?,
        "检查材料摘要阅读器或数据目录不符",
    )?;
    #[cfg(windows)]
    require(
        g.owner == crate::ipc::current_sid().map_err(|e| e.to_string())?,
        "检查材料摘要数据用户不符",
    )?;
    status.minimum_reader_generation = status.minimum_reader_generation.max(generation);
    status.reader_guard_digest = Some(digest(&g)?);
    Ok(())
}
pub(crate) fn enable(root: &Path) -> Result<(), String> {
    enable_guard(root, FILE_NAME, GENERATION)
}
pub(crate) fn enable_api_limits(root: &Path) -> Result<(), String> {
    enable_guard(root, API_LIMITS_FILE_NAME, API_LIMITS_GENERATION)
}
fn enable_guard(root: &Path, name: &str, generation: u32) -> Result<(), String> {
    let mut status = crate::data_format::status(root)?;
    let path = root.join("events").join(name);
    if path.exists() {
        return observe_guard(root, &mut status, name, generation);
    }
    #[cfg(windows)]
    let owner = crate::ipc::current_sid().map_err(|e| e.to_string())?;
    #[cfg(not(windows))]
    let owner = String::new();
    let g = Guard {
        schema: 1,
        product: cerulean_domain::PRODUCT_NAME.into(),
        generation,
        root: fs::canonicalize(root)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .into(),
        owner,
    };
    let bytes = serde_json::to_vec(&g).map_err(|e| e.to_string())?;
    let mut f = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    f.write_all(&bytes)
        .and_then(|_| f.sync_all())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reader25_limits_preserve_reader24_and_reject_corrupt_budget_guard() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        drop(crate::Store::open(root).unwrap());
        enable(root).unwrap();
        let old = fs::read(root.join("events").join(FILE_NAME)).unwrap();
        enable_api_limits(root).unwrap();
        enable_api_limits(root).unwrap();
        assert_eq!(fs::read(root.join("events").join(FILE_NAME)).unwrap(), old);
        assert_eq!(
            crate::data_format::status(root)
                .unwrap()
                .minimum_reader_generation,
            API_LIMITS_GENERATION
        );
        drop(crate::Store::open(root).unwrap());
        let marker = root.join("events").join(API_LIMITS_FILE_NAME);
        let original = fs::read(&marker).unwrap();
        let mut guard: Guard = serde_json::from_slice(&original).unwrap();
        guard.generation = GENERATION;
        fs::write(&marker, serde_json::to_vec(&guard).unwrap()).unwrap();
        assert!(crate::Store::open(root).is_err());
        fs::write(&marker, original).unwrap();
        drop(crate::Store::open(root).unwrap());
    }
    #[test]
    fn reader24_is_idempotent_and_rejects_foreign_and_corrupt_markers() {
        assert!(crate::DATA_READER_GENERATION >= GENERATION);
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        drop(crate::Store::open(root).unwrap());
        enable(root).unwrap();
        let marker = root.join("events").join(FILE_NAME);
        let original = fs::read(&marker).unwrap();
        enable(root).unwrap();
        assert_eq!(fs::read(&marker).unwrap(), original);
        assert_eq!(
            crate::data_format::status(root)
                .unwrap()
                .minimum_reader_generation,
            GENERATION
        );
        let mut foreign: Guard = serde_json::from_slice(&original).unwrap();
        foreign.root = root.join("foreign").to_string_lossy().into();
        fs::write(&marker, serde_json::to_vec(&foreign).unwrap()).unwrap();
        assert!(crate::Store::open(root).is_err());
        fs::write(&marker, b"corrupt").unwrap();
        assert!(crate::Store::open(root).is_err());
        fs::write(&marker, original).unwrap();
        drop(crate::Store::open(root).unwrap());
    }
}
