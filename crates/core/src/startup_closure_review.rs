//! Review incomplete closure bytes without inventing a completed closure.
//! A later close still goes through the original host review and ledger checks.
use crate::{
    host::ordinary,
    startup_review::{bytes, reason},
};
use cerulean_domain::{
    execution::{artifact_path, digest, identifier, require, sha256, timestamp},
    StartupClosureReview,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
const FOLDER: &str = "startup-closure-reviews";
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

pub(crate) fn parts(relative: &str) -> Result<(&str, &str, &str), String> {
    let p: Vec<_> = relative.split('/').collect();
    require(
        p.len() == 3 && matches!(p[0], "codex-startups" | "omp-startups"),
        "关闭复核路径无效",
    )?;
    let stem = p[2]
        .strip_suffix(".pending")
        .ok_or("关闭复核必须指向未发布文件")?;
    let stem = if let Some((base, hash)) = stem.rsplit_once('.') {
        if hash.len() == 64 {
            sha256(hash)?;
            base
        } else {
            stem
        }
    } else {
        stem
    };
    let attempt = stem.strip_suffix(".1").ok_or("仅支持未完成关闭记录")?;
    for id in [p[1], attempt] {
        identifier(id)?;
        artifact_path(id)?;
    }
    Ok((p[0], p[1], attempt))
}
fn path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    parts(relative)?;
    let folder = root.join(FOLDER);
    match fs::symlink_metadata(&folder) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
        Ok(_) => ordinary(&folder, true)?,
    };
    Ok(folder.join(format!("{}.json", digest(&relative)?)))
}
fn fingerprint(root: &Path, relative: &str) -> Result<(String, String), String> {
    let (folder, project, attempt) = parts(relative)?;
    ordinary(root, true)?;
    let dir = root.join(folder);
    ordinary(&dir, true)?;
    require(
        fs::read_dir(&dir)
            .map_err(|e| e.to_string())?
            .any(|e| e.is_ok_and(|e| e.file_name() == project)),
        "关闭项目大小写不符",
    )?;
    let dir = dir.join(project);
    ordinary(&dir, true)?;
    let filename = relative.rsplit('/').next().unwrap();
    require(
        fs::read_dir(&dir)
            .map_err(|e| e.to_string())?
            .any(|e| e.is_ok_and(|e| e.file_name() == filename)),
        "关闭文件大小写不符",
    )?;
    let raw = bytes(&root.join(relative))?.ok_or("未完成关闭原文缺失")?;
    require(
        serde_json::from_slice::<serde_json::Value>(&raw).is_err_and(|e| e.is_eof()),
        "只能复核空或未写完整的关闭记录",
    )?;
    let head = crate::startup_journal::review_intent_head(root, folder, project, attempt)?;
    let evidence = digest(&(
        1,
        relative,
        &head,
        format!("{:x}", Sha256::digest(&raw)),
        raw.len(),
    ))?;
    Ok((head, evidence))
}
fn unpublished(root: &Path, relative: &str) -> Result<(), String> {
    let (folder, project, attempt) = parts(relative)?;
    require(
        bytes(
            &root
                .join(folder)
                .join(project)
                .join(format!("{attempt}.1.json")),
        )?
        .is_none(),
        "正式关闭证据已存在，不能确认新的中断关闭",
    )
}
fn verify(root: &Path, r: &Receipt) -> Result<StartupClosureReview, String> {
    let (folder, project, attempt) = parts(&r.relative_path)?;
    reason(&r.reason)?;
    timestamp(&r.at)?;
    let (head, evidence) = fingerprint(root, &r.relative_path)?;
    require(
        r.version == 1
            && r.evidence_digest == evidence
            && r.hash
                == digest(&(
                    r.version,
                    &r.relative_path,
                    &r.evidence_digest,
                    &r.reason,
                    &r.at,
                ))?,
        "关闭复核凭据与原证据不符",
    )?;
    Ok(StartupClosureReview {
        relative_path: r.relative_path.clone(),
        executor: folder.strip_suffix("-startups").unwrap().into(),
        project_id: project.into(),
        attempt_id: attempt.into(),
        intent_hash: head,
        reason: r.reason.clone(),
        at: r.at.clone(),
    })
}
pub(crate) fn inspect(root: &Path, relative: &str) -> Result<(String, bool), String> {
    let (_, evidence) = fingerprint(root, relative)?;
    if let Some(raw) = bytes(&path(root, relative)?)? {
        let r: Receipt = serde_json::from_slice(&raw).map_err(|_| "关闭复核凭据损坏")?;
        require(r.relative_path == relative, "关闭复核路径不符")?;
        verify(root, &r)?;
        Ok((evidence, true))
    } else {
        unpublished(root, relative)?;
        Ok((evidence, false))
    }
}
pub(crate) fn load(root: &Path) -> Result<Vec<StartupClosureReview>, String> {
    let folder = root.join(FOLDER);
    match fs::symlink_metadata(&folder) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.to_string()),
        Ok(_) => ordinary(&folder, true)?,
    };
    let entries = fs::read_dir(&folder)
        .map_err(|e| e.to_string())?
        .take(10001)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    require(entries.len() <= 10000, "关闭复核目录已满")?;
    let mut result = vec![];
    for entry in entries {
        ordinary(&entry.path(), false)?;
        if entry.path().extension().is_some_and(|e| e == "pending") {
            continue;
        }
        require(
            entry.path().extension().is_some_and(|e| e == "json"),
            "未知关闭复核文件",
        )?;
        let r: Receipt = serde_json::from_slice(&bytes(&entry.path())?.ok_or("关闭复核消失")?)
            .map_err(|_| "关闭复核凭据损坏")?;
        require(
            entry.path() == path(root, &r.relative_path)?,
            "关闭复核名称不符",
        )?;
        result.push(verify(root, &r)?);
    }
    result.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(result)
}
pub(crate) fn acknowledge(
    root: &Path,
    relative: &str,
    expected: &str,
    text: &str,
    mut observe: impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    reason(text)?;
    let (evidence, reviewed) = inspect(root, relative)?;
    require(evidence == expected, "关闭原证据已变化，请重新读取")?;
    if reviewed {
        return Ok(());
    }
    let mut r = Receipt {
        version: 1,
        relative_path: relative.into(),
        evidence_digest: expected.into(),
        reason: text.into(),
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
    require(raw.len() <= 65536, "关闭复核记录过大")?;
    let target = path(root, relative)?;
    let folder = target.parent().unwrap();
    fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    ordinary(folder, true)?;
    require(
        fs::read_dir(folder)
            .map_err(|e| e.to_string())?
            .take(10000)
            .count()
            < 10000,
        "关闭复核目录已满",
    )?;
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
        fingerprint(root, relative)?.1 == expected,
        "发布前关闭证据已变化",
    )?;
    unpublished(root, relative)?;
    fs::hard_link(&pending, &target).map_err(|e| e.to_string())?;
    observe("published")?;
    fs::remove_file(pending).map_err(|e| e.to_string())?;
    observe("finished")
}
