//! Explicit review retains truncated bytes and binds the valid preceding chain.
//! It grants no workflow command, model resend, or task completion.
use crate::{host::ordinary, management_execution::DecisionJournal};
use cerulean_domain::{
    execution::{artifact_path, digest, identifier, require, sha256, timestamp},
    ManagementRecordReview,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
const LIMIT: usize = crate::management_execution::MAX_BYTES;
const FOLDER: &str = "management-record-reviews";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipt {
    version: u32,
    relative_path: String,
    evidence_digest: String,
    reason: String,
    at: String,
    hash: String,
}
pub(crate) fn parts(relative: &str) -> Result<(&str, u8), String> {
    let name = relative
        .strip_prefix("management-decisions/")
        .ok_or("管理复核路径无效")?;
    artifact_path(name)?;
    let stem = name
        .strip_suffix(".pending")
        .ok_or("只能复核未发布管理记录")?;
    let components: Vec<_> = stem.split('.').collect();
    require(matches!(components.len(), 2 | 3), "管理复核文件名无效")?;
    identifier(components[0])?;
    let sequence = match components[1] {
        "0" => 0,
        "1" => 1,
        "2" => 2,
        "3" => 3,
        "4" => 4,
        _ => return Err("管理复核阶段无效".into()),
    };
    if components.len() == 3 {
        require(sequence >= 3, "此阶段不允许管理写入重试")?;
        sha256(components[2])?;
    }
    Ok((components[0], sequence))
}
fn bytes(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
        Ok(_) => ordinary(path, false)?,
    }
    let mut raw = vec![];
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut raw)
        .map_err(|e| e.to_string())?;
    require(raw.len() <= LIMIT, "管理复核文件过大")?;
    Ok(Some(raw))
}
fn entries(dir: &Path) -> Result<Vec<fs::DirEntry>, String> {
    ordinary(dir, true)?;
    let items = fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .take(10001)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    require(items.len() <= 10000, "管理复核目录已满")?;
    Ok(items)
}
fn receipt_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    parts(relative)?;
    let folder = root.join(FOLDER);
    match fs::symlink_metadata(&folder) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
        Ok(_) => ordinary(&folder, true)?,
    }
    Ok(folder.join(format!("{}.json", digest(&relative)?)))
}
fn fingerprint(root: &Path, relative: &str) -> Result<(String, Option<String>), String> {
    let (operation, sequence) = parts(relative)?;
    ordinary(root, true)?;
    let dir = root.join("management-decisions");
    let name = relative.strip_prefix("management-decisions/").unwrap();
    require(
        entries(&dir)?.iter().any(|e| e.file_name() == name),
        "管理文件名称或大小写不符",
    )?;
    let raw = bytes(&root.join(relative))?.ok_or("管理原始残片缺失")?;
    require(
        serde_json::from_slice::<serde_json::Value>(&raw).is_err_and(|e| e.is_eof()),
        "只能复核空或未写完整的管理记录",
    )?;
    let (prefix, project) = DecisionJournal::review_prefix(root, operation, sequence)?;
    Ok((
        digest(&(
            1,
            relative,
            format!("{:x}", Sha256::digest(&raw)),
            raw.len(),
            prefix,
        ))?,
        project,
    ))
}
fn candidate(root: &Path, relative: &str) -> Result<(), String> {
    let (operation, sequence) = parts(relative)?;
    let dir = root.join("management-decisions");
    require(
        bytes(&dir.join(format!("{operation}.{sequence}.json")))?.is_none(),
        "管理正式记录已存在，不能新增残片复核",
    )?;
    // Later evidence cannot be silently hidden by reviewing an earlier write.
    for entry in entries(&dir)? {
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "管理文件名无效")?;
        if let Some(rest) = name.strip_prefix(&format!("{operation}.")) {
            if let Some(stage) = rest.split('.').next().and_then(|s| s.parse::<u8>().ok()) {
                require(stage <= sequence, "已有后续管理证据，需先核对冲突")?;
            }
        }
    }
    Ok(())
}
fn valid_reason(reason: &str) -> Result<(), String> {
    require(
        !reason.trim().is_empty() && reason.len() <= 8000 && !reason.contains('\0'),
        "请填写有效的管理复核说明",
    )
}
fn verify(root: &Path, r: &Receipt) -> Result<ManagementRecordReview, String> {
    let (operation, sequence) = parts(&r.relative_path)?;
    valid_reason(&r.reason)?;
    timestamp(&r.at)?;
    let (evidence, project) = fingerprint(root, &r.relative_path)?;
    require(
        r.version == 1
            && evidence == r.evidence_digest
            && r.hash
                == digest(&(
                    r.version,
                    &r.relative_path,
                    &r.evidence_digest,
                    &r.reason,
                    &r.at,
                ))?,
        "管理复核凭据与原文或前置链不符",
    )?;
    Ok(ManagementRecordReview {
        relative_path: r.relative_path.clone(),
        operation_id: operation.into(),
        sequence,
        project_id: project,
        reason: r.reason.clone(),
        at: r.at.clone(),
    })
}
pub(crate) fn inspect(root: &Path, relative: &str) -> Result<(String, bool), String> {
    let (evidence, _) = fingerprint(root, relative)?;
    if let Some(raw) = bytes(&receipt_path(root, relative)?)? {
        let r: Receipt = serde_json::from_slice(&raw).map_err(|_| "管理复核凭据损坏")?;
        require(r.relative_path == relative, "管理复核路径不符")?;
        verify(root, &r)?;
        Ok((evidence, true))
    } else {
        candidate(root, relative)?;
        Ok((evidence, false))
    }
}
pub(crate) fn load(root: &Path) -> Result<Vec<ManagementRecordReview>, String> {
    let dir = root.join(FOLDER);
    match fs::symlink_metadata(&dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.to_string()),
        Ok(_) => ordinary(&dir, true)?,
    }
    let mut reviews = vec![];
    for file in entries(&dir)? {
        ordinary(&file.path(), false)?;
        if file.path().extension().is_some_and(|e| e == "pending") {
            continue;
        }
        require(
            file.path().extension().is_some_and(|e| e == "json"),
            "未知管理复核文件",
        )?;
        let r: Receipt = serde_json::from_slice(&bytes(&file.path())?.ok_or("管理复核凭据缺失")?)
            .map_err(|_| "管理复核凭据损坏")?;
        require(
            file.path() == receipt_path(root, &r.relative_path)?,
            "管理复核凭据名称不符",
        )?;
        reviews.push(verify(root, &r)?);
    }
    reviews.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(reviews)
}
pub(crate) fn acknowledge(
    root: &Path,
    relative: &str,
    expected: &str,
    reason: &str,
    mut observe: impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    valid_reason(reason)?;
    let (current, reviewed) = inspect(root, relative)?;
    require(current == expected, "管理证据已变化，请刷新后重新复核")?;
    if reviewed {
        return Ok(());
    }
    let mut r = Receipt {
        version: 1,
        relative_path: relative.into(),
        evidence_digest: expected.into(),
        reason: reason.into(),
        at: chrono::Utc::now().to_rfc3339(),
        hash: String::new(),
    };
    r.hash = digest(&(
        r.version,
        &r.relative_path,
        &r.evidence_digest,
        &r.reason,
        &r.at,
    ))?;
    let raw = serde_json::to_vec(&r).map_err(|e| e.to_string())?;
    require(raw.len() <= LIMIT, "管理复核凭据过大")?;
    let target = receipt_path(root, relative)?;
    let folder = target.parent().unwrap();
    fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    require(entries(folder)?.len() < 10000, "管理复核目录已满")?;
    let pending = target.with_extension(format!("{}.pending", r.hash));
    let mut f = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&pending)
        .map_err(|e| e.to_string())?;
    observe("created")?;
    f.write_all(&raw[..raw.len() / 2])
        .map_err(|e| e.to_string())?;
    observe("partial")?;
    f.write_all(&raw[raw.len() / 2..])
        .and_then(|_| f.sync_all())
        .map_err(|e| e.to_string())?;
    drop(f);
    observe("synced")?;
    require(
        fingerprint(root, relative)?.0 == expected,
        "发布前管理证据已变化",
    )?;
    candidate(root, relative)?;
    fs::hard_link(&pending, &target).map_err(|e| e.to_string())?;
    observe("published")?;
    fs::remove_file(pending).map_err(|e| e.to_string())?;
    observe("finished")
}
