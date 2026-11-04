//! Reader21 protects desktop hook supervision and weighted independent checks.
use cerulean_domain::execution::{digest, require};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};
pub const GENERATION: u32 = 21;
pub const FILE_NAME: &str = "00000000.reader21.json";
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
    let path = root.join("events").join(FILE_NAME);
    if !path.exists() {
        for folder in ["independent-check-archive", "desktop-supervision-v1"] {
            if let Ok(mut entries) = fs::read_dir(root.join(folder)) {
                require(
                    entries.next().is_none(),
                    "独立检查或监督数据缺少阅读器保护，禁止载入",
                )?;
            }
        }
        return Ok(());
    }
    crate::host::ordinary(&path, false)?;
    let mut bytes = vec![];
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(8193)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    require(bytes.len() <= 8192, "独立检查与监督阅读器保护超限")?;
    let g: Guard = serde_json::from_slice(&bytes).map_err(|_| "独立检查与监督阅读器保护损坏")?;
    require(
        g.schema == 1
            && g.product == cerulean_domain::PRODUCT_NAME
            && g.generation == GENERATION
            && Path::new(&g.root) == fs::canonicalize(root).map_err(|e| e.to_string())?,
        "独立检查与监督阅读器或数据目录不符",
    )?;
    #[cfg(windows)]
    require(
        g.owner == crate::ipc::current_sid().map_err(|e| e.to_string())?,
        "独立检查与监督数据用户不符",
    )?;
    status.minimum_reader_generation = status.minimum_reader_generation.max(GENERATION);
    status.reader_guard_digest = Some(digest(&g)?);
    Ok(())
}
pub(crate) fn enable(root: &Path) -> Result<(), String> {
    let mut status = crate::data_format::status(root)?;
    let path = root.join("events").join(FILE_NAME);
    if path.exists() {
        return observe(root, &mut status);
    }
    #[cfg(windows)]
    let owner = crate::ipc::current_sid().map_err(|e| e.to_string())?;
    #[cfg(not(windows))]
    let owner = String::new();
    let g = Guard {
        schema: 1,
        product: cerulean_domain::PRODUCT_NAME.into(),
        generation: GENERATION,
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
    fn reader21_preserves_history_and_rejects_missing_or_foreign_guard() {
        assert_eq!(crate::DATA_READER_GENERATION, GENERATION);
        for folder in ["independent-check-archive", "desktop-supervision-v1"] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path();
            drop(crate::Store::open(root).unwrap());
            let events = fs::read_dir(root.join("events")).unwrap().count();
            enable(root).unwrap();
            enable(root).unwrap();
            assert_eq!(
                fs::read_dir(root.join("events")).unwrap().count(),
                events + 1
            );
            assert_eq!(
                crate::data_format::status(root)
                    .unwrap()
                    .minimum_reader_generation,
                GENERATION
            );
            drop(crate::Store::open(root).unwrap());
            let marker = root.join("events").join(FILE_NAME);
            let original = fs::read(&marker).unwrap();
            let mut foreign: Guard = serde_json::from_slice(&original).unwrap();
            foreign.root = root.join("another-root").to_string_lossy().into();
            fs::write(&marker, serde_json::to_vec(&foreign).unwrap()).unwrap();
            assert!(crate::Store::open(root).is_err());
            fs::write(&marker, &original).unwrap();
            fs::create_dir(root.join(folder)).unwrap();
            fs::write(
                root.join(folder).join("original.json"),
                b"retained evidence",
            )
            .unwrap();
            fs::remove_file(&marker).unwrap();
            assert!(crate::Store::open(root).is_err());
            assert_eq!(
                fs::read(root.join(folder).join("original.json")).unwrap(),
                b"retained evidence"
            );
        }
    }
}
