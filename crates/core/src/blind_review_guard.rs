//! Unknown reader filename makes older cores reject, rather than ignore ballots.
use cerulean_domain::execution::{digest, require};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};
pub const GENERATION: u32 = 19;
pub const FILE_NAME: &str = "00000000.reader19.json";
pub(crate) fn require_isolated_execution(root: &Path, bound: bool) -> Result<(), String> {
    require(
        bound || !root.join("events").join(FILE_NAME).exists(),
        "此数据目录已启用私有盲评；旧宿主权限执行被拒绝，请在隔离项目中重新批准执行",
    )
}
pub(crate) fn require_management_source(
    _root: &Path,
    sources: Option<&cerulean_domain::project_setup::ManagementSources>,
) -> Result<(), String> {
    if let Some(cerulean_domain::project_setup::ManagementSources {
        chief: cerulean_domain::project_setup::ChiefSource::CodexSession { session_id },
        ..
    }) = sources
    {
        cerulean_domain::execution::identifier(session_id)?;
        require(session_id.len() <= 80, "主指挥会话标识超限")?;
    }
    // Actual scoring uses the tool-restricted session route. The host blocks
    // ordinary management dispatch throughout collection and functional tests.
    Ok(())
}
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
        if let Ok(mut entries) = fs::read_dir(root.join("blind-review")) {
            require(entries.next().is_none(), "盲评数据缺少阅读器保护，禁止载入")?;
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
    require(bytes.len() <= 8192, "盲评阅读器保护超限")?;
    let g: Guard = serde_json::from_slice(&bytes).map_err(|_| "盲评阅读器保护损坏")?;
    require(
        g.schema == 1
            && g.product == cerulean_domain::PRODUCT_NAME
            && g.generation == GENERATION
            && Path::new(&g.root) == fs::canonicalize(root).map_err(|e| e.to_string())?,
        "盲评阅读器或数据目录不符",
    )?;
    #[cfg(windows)]
    require(
        g.owner == crate::ipc::current_sid().map_err(|e| e.to_string())?,
        "盲评数据用户不符",
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
    fn blind_review_guard_prevents_missing_marker_and_keeps_original_events() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let store = crate::Store::open(root).unwrap();
        drop(store);
        let before = fs::read_dir(root.join("events")).unwrap().count();
        use cerulean_domain::project_setup::{ChiefSource, DeputySource, ManagementSources};
        let external = ManagementSources {
            chief_reasoning_effort: None,
            deputy_reasoning_effort: None,
            chief: ChiefSource::CodexSession {
                session_id: "session-fixture".into(),
            },
            deputy: DeputySource::OpenAi {},
        };
        assert!(require_isolated_execution(root, false).is_ok());
        assert!(require_management_source(root, Some(&external)).is_ok());
        enable(root).unwrap();
        assert!(require_isolated_execution(root, false).is_err());
        assert!(require_isolated_execution(root, true).is_ok());
        assert!(require_management_source(root, Some(&external)).is_ok());
        assert!(require_management_source(root, None).is_ok());
        assert_eq!(
            crate::data_format::status(root)
                .unwrap()
                .minimum_reader_generation,
            19
        );
        let reopened = crate::Store::open(root).unwrap();
        drop(reopened);
        assert_eq!(
            fs::read_dir(root.join("events")).unwrap().count(),
            before + 1
        );
        fs::create_dir(root.join("blind-review")).unwrap();
        fs::write(root.join("blind-review/00000001.pending"), b"partial").unwrap();
        fs::remove_file(root.join("events").join(FILE_NAME)).unwrap();
        assert!(crate::Store::open(root).is_err());
    }
}
