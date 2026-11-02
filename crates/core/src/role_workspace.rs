//! Chief-owned immutable material, separate from the deputy/worker write root.
//! These are scoped application permissions, not an OS sandbox for native tools.
use crate::{
    artifacts::{contains_path, root_path},
    host::ordinary,
};
use cerulean_domain::{execution::*, project_setup::SavedProjectSetup};
use serde_json::{json, Value};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const LIMIT: usize = 64 * 1024;

pub(crate) fn prepare(
    chief: &str,
    shared: &str,
    pool: Option<&str>,
    data: &Path,
) -> Result<String, String> {
    let path = Path::new(chief);
    require(
        path.is_absolute()
            && !chief.chars().any(char::is_control)
            && !path.components().any(|c| {
                matches!(
                    c,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            }),
        "主指挥目录须使用规范绝对路径",
    )?;
    // Only create the final component under an already-existing ordinary parent.
    let parent = root_path(
        path.parent()
            .and_then(Path::to_str)
            .ok_or("主指挥目录缺少父目录")?,
    )?;
    let candidate = parent.join(path.file_name().ok_or("无效主指挥目录")?);
    for other in std::iter::once(shared).chain(pool).chain(data.to_str()) {
        let other = root_path(other)?;
        require(
            !contains_path(&other, &candidate) && !contains_path(&candidate, &other),
            "主指挥目录必须独立于执行目录、目录池和核心数据目录",
        )?;
    }
    if !candidate.exists() {
        fs::create_dir(&candidate).map_err(|e| e.to_string())?;
    }
    ordinary(&candidate, true)?;
    Ok(root_path(candidate.to_str().ok_or("目录编码无效")?)?
        .to_string_lossy()
        .into())
}

fn location(chief: &str, kind: &str, key: &str) -> Result<PathBuf, String> {
    sha256(key)?;
    require(
        matches!(kind, "requirements" | "plans" | "reviews"),
        "unsupported chief material",
    )?;
    let base = root_path(chief)?;
    Ok(base.join(kind).join(format!("{key}.json")))
}

pub(crate) fn read(chief: &str, kind: &str, key: &str) -> Result<Value, String> {
    let path = location(chief, kind, key)?;
    root_path(
        path.parent()
            .unwrap()
            .to_str()
            .ok_or("invalid material directory")?,
    )?;
    ordinary(&path, false)?;
    #[cfg(windows)]
    let mut file = crate::backup::open(&path, false, false).map_err(|e| e.to_string())?;
    #[cfg(not(windows))]
    let mut file = fs::File::open(&path).map_err(|e| e.to_string())?;
    let mut bytes = vec![];
    (&mut file)
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    require(bytes.len() <= LIMIT, "chief material exceeds read limit")?;
    serde_json::from_slice(&bytes).map_err(|_| "主指挥资料无效，请核对原版本；不会覆盖".into())
}

pub(crate) fn publish(
    chief: &str,
    kind: &str,
    key: &str,
    value: &Value,
) -> Result<PathBuf, String> {
    let file = location(chief, kind, key)?;
    let parent = file.parent().unwrap();
    if !parent.exists() {
        fs::create_dir(parent).map_err(|e| e.to_string())?;
    }
    ordinary(parent, true)?;
    if file.exists() {
        require(
            read(chief, kind, key)? == *value,
            "主指挥资料已变化，禁止覆盖",
        )?;
        return Ok(file);
    }
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    require(bytes.len() <= LIMIT, "chief material exceeds write limit")?;
    let pending = file.with_extension("pending");
    let mut stream = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&pending)
        .map_err(|e| e.to_string())?;
    stream
        .write_all(&bytes)
        .and_then(|_| stream.sync_all())
        .map_err(|e| e.to_string())?;
    drop(stream);
    fs::rename(&pending, &file).map_err(|e| e.to_string())?;
    Ok(file)
}

fn requirement(saved: &SavedProjectSetup) -> Value {
    json!({"schemaVersion":1,"projectId":saved.setup.project_id,"setupDigest":saved.digest,"requirement":saved.setup.requirement})
}

pub(crate) fn save_requirements(saved: &SavedProjectSetup) -> Result<(), String> {
    if let Some(chief) = &saved.setup.chief_directory {
        publish(chief, "requirements", &saved.digest, &requirement(saved))?;
    }
    Ok(())
}

pub(crate) fn read_requirements(saved: &SavedProjectSetup, shared: &str) -> Result<Value, String> {
    let chief = saved
        .setup
        .chief_directory
        .as_deref()
        .ok_or("chief workspace not configured")?;
    let content = read(chief, "requirements", &saved.digest)?;
    require(
        content == requirement(saved),
        "主指挥需求与已保存配置不一致，请重新核对",
    )?;
    let workers = saved
        .setup
        .worker_directories
        .iter()
        .map(|(identity, binding)| Ok((identity.clone(), binding.root(shared)?)))
        .collect::<Result<std::collections::BTreeMap<_, _>, String>>()?;
    Ok(
        json!({"schemaVersion":1,"chiefDirectory":chief,"deputyDirectory":shared,"workerDirectories":workers,
        "deputyChiefAccess":"read-only","materialPath":location(chief,"requirements",&saved.digest)?,
        "materialDigest":digest(&content)?,"material":content}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chief_material_is_scoped_immutable_and_detects_tampering() {
        let dir = tempfile::tempdir().unwrap();
        let chief = dir.path().to_str().unwrap();
        let value = json!({"requirement":"keep the original requirement"});
        let key = digest(&value).unwrap();
        let file = publish(chief, "requirements", &key, &value).unwrap();
        assert_eq!(read(chief, "requirements", &key).unwrap(), value);
        assert!(read(chief, "../other", &key).is_err());
        assert!(read(chief, "requirements", "../escape").is_err());
        assert!(publish(chief, "requirements", &key, &json!({"changed":true})).is_err());
        fs::write(file, b"not json").unwrap();
        assert!(read(chief, "requirements", &key).is_err());
    }
    #[test]
    fn chief_must_be_outside_shared_worker_and_data_roots() {
        let dir = tempfile::tempdir().unwrap();
        let shared = dir.path().join("shared");
        let data = dir.path().join("data");
        fs::create_dir(&shared).unwrap();
        fs::create_dir(&data).unwrap();
        assert!(prepare(
            shared.join("chief").to_str().unwrap(),
            shared.to_str().unwrap(),
            None,
            &data
        )
        .is_err());
        let chief = dir.path().join("chief");
        prepare(
            chief.to_str().unwrap(),
            shared.to_str().unwrap(),
            None,
            &data,
        )
        .unwrap();
        assert!(chief.is_dir());
    }
}
