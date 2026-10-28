//! Read-only recovery of complete API publications and explicit review of truncated bytes.
//! Reviews never manufacture a startup identity or a remote cancellation receipt.
use crate::{host::ordinary, startup_review::bytes};
use cerulean_domain::{execution::*, ApiPublicationReview};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::Path,
};

const REVIEWS: &str = "api-publication-reviews";
type Pair = (Option<Vec<u8>>, Option<Vec<u8>>);
fn key(value: &str) -> Result<(), String> {
    require(
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "API记录名称无效",
    )
}
fn folder(value: &str) -> Result<(), String> {
    require(
        matches!(
            value,
            "api-startups" | "api-cancellations" | "api-workspace-bindings"
        ),
        "不支持的API记录目录",
    )
}
fn parts(relative: &str) -> Result<(&str, &str), String> {
    let (dir, name) = relative.split_once('/').ok_or("API复核路径无效")?;
    folder(dir)?;
    let stem = name.strip_suffix(".pending").ok_or("仅复核API未发布记录")?;
    key(stem)?;
    Ok((dir, stem))
}
pub(crate) fn pairs(root: &Path, dir: &str) -> Result<BTreeMap<String, Pair>, String> {
    folder(dir)?;
    ordinary(root, true)?;
    let directory = root.join(dir);
    if matches!(fs::symlink_metadata(&directory),Err(e) if e.kind()==std::io::ErrorKind::NotFound) {
        fs::create_dir(&directory).map_err(|_| "API记录目录不可创建")?;
    }
    ordinary(&directory, true)?;
    let mut result = BTreeMap::<String, Pair>::new();
    for (count, entry) in fs::read_dir(&directory)
        .map_err(|_| "API记录目录不可读")?
        .enumerate()
    {
        require(count < 2000, "API记录目录过大")?;
        let entry = entry.map_err(|_| "API记录不可读")?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "API文件名无效")?;
        let (stem, ext) = name.rsplit_once('.').ok_or("API记录扩展名缺失")?;
        key(stem)?;
        require(matches!(ext, "json" | "pending"), "API记录扩展名无效")?;
        let raw = bytes(&entry.path())?.ok_or("API原文缺失")?;
        let pair = result.entry(stem.into()).or_default();
        let slot = if ext == "json" {
            &mut pair.0
        } else {
            &mut pair.1
        };
        require(slot.is_none(), "API记录重复")?;
        *slot = Some(raw);
    }
    require(result.len() <= 1000, "API记录历史过大")?;
    Ok(result)
}
pub(crate) fn complete(pair: &Pair) -> Result<Option<&[u8]>, String> {
    match (&pair.0, &pair.1) {
        (Some(final_bytes), Some(pending)) => {
            require(final_bytes == pending, "API正式与待发布记录冲突")?;
            Ok(Some(final_bytes))
        }
        (Some(raw), None) => Ok(Some(raw)),
        (None, Some(raw))
            if serde_json::from_slice::<serde_json::Value>(raw).is_err_and(|e| e.is_eof()) =>
        {
            Ok(None)
        }
        (None, Some(raw)) => Ok(Some(raw)),
        (None, None) => Err("API记录缺失".into()),
    }
}
pub(crate) fn records(
    root: &Path,
    dir: &str,
    reviewed: &BTreeSet<String>,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    pairs(root, dir)?
        .into_iter()
        .filter_map(|(stem, pair)| match complete(&pair) {
            Ok(Some(raw)) => Some(Ok((stem, raw.to_vec()))),
            Ok(None) if reviewed.contains(&format!("{dir}/{stem}.pending")) => None,
            Ok(None) => Some(Err("API记录只写入一部分，请保留原文并明确复核".into())),
            Err(error) => Some(Err(error)),
        })
        .collect()
}
fn fingerprint(root: &Path, relative: &str) -> Result<String, String> {
    let (dir, stem) = parts(relative)?;
    ordinary(root, true)?;
    ordinary(&root.join(dir), true)?;
    let name = format!("{stem}.pending");
    require(
        fs::read_dir(root.join(dir))
            .map_err(|_| "API目录不可读")?
            .any(|e| e.is_ok_and(|e| e.file_name() == name.as_str())),
        "API文件大小写不符",
    )?;
    require(
        bytes(&root.join(dir).join(format!("{stem}.json")))?.is_none(),
        "正式API记录已经存在，不能按半写记录复核",
    )?;
    let raw = bytes(&root.join(relative))?.ok_or("API半写原文缺失")?;
    require(
        serde_json::from_slice::<serde_json::Value>(&raw).is_err_and(|e| e.is_eof()),
        "仅允许空或未写完整的API原文",
    )?;
    let startup_hash = if matches!(dir, "api-cancellations" | "api-workspace-bindings") {
        Some(digest(&crate::api_startup::original(root, stem)?)?)
    } else {
        None
    };
    digest(&(
        "api-partial-publication-review-v1",
        relative,
        format!("{:x}", Sha256::digest(&raw)),
        raw.len(),
        startup_hash,
    ))
}
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
impl Receipt {
    fn hash(&self) -> Result<String, String> {
        digest(&(
            self.version,
            &self.relative_path,
            &self.evidence_digest,
            &self.reason,
            &self.at,
        ))
    }
    fn verify(&self, root: &Path) -> Result<ApiPublicationReview, String> {
        crate::startup_review::reason(&self.reason)?;
        timestamp(&self.at)?;
        require(
            self.version == 1
                && self.hash == self.hash()?
                && self.evidence_digest == fingerprint(root, &self.relative_path)?,
            "API复核与原证据不符",
        )?;
        Ok(ApiPublicationReview {
            relative_path: self.relative_path.clone(),
            evidence_digest: self.evidence_digest.clone(),
            reason: self.reason.clone(),
            at: self.at.clone(),
        })
    }
}
fn receipt_path(root: &Path, relative: &str) -> Result<std::path::PathBuf, String> {
    parts(relative)?;
    let dir = root.join(REVIEWS);
    match fs::symlink_metadata(&dir) {
        Ok(_) => ordinary(&dir, true)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("API复核目录不可读".into()),
    }
    Ok(dir.join(format!("{}.json", digest(&relative)?)))
}
pub(crate) fn inspect(root: &Path, relative: &str) -> Result<(String, bool), String> {
    let head = fingerprint(root, relative)?;
    let reviewed = if let Some(raw) = bytes(&receipt_path(root, relative)?)? {
        let r: Receipt = serde_json::from_slice(&raw).map_err(|_| "API复核损坏")?;
        require(r.relative_path == relative, "API复核目标不符")?;
        r.verify(root)?;
        true
    } else {
        false
    };
    Ok((head, reviewed))
}
pub(crate) fn reviews(root: &Path) -> Result<Vec<ApiPublicationReview>, String> {
    let dir = root.join(REVIEWS);
    match fs::symlink_metadata(&dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(_) => return Err("API复核目录不可读".into()),
        Ok(_) => ordinary(&dir, true)?,
    }
    let mut result = vec![];
    for (count, file) in fs::read_dir(&dir)
        .map_err(|_| "API复核目录不可读")?
        .enumerate()
    {
        require(count < 10000, "API复核目录已满")?;
        let file = file.map_err(|_| "API复核文件不可读")?;
        ordinary(&file.path(), false)?;
        let name = file
            .file_name()
            .into_string()
            .map_err(|_| "API复核名称无效")?;
        let p = name.split('.').collect::<Vec<_>>();
        key(p[0])?;
        if p.len() == 3 && p[2] == "pending" {
            key(p[1])?;
            // An interrupted review is not user confirmation. Retain it and allow explicit retry.
            bytes(&file.path())?;
            continue;
        }
        require(p.len() == 2 && p[1] == "json", "API复核扩展名无效")?;
        let r: Receipt = serde_json::from_slice(&bytes(&file.path())?.ok_or("API复核缺失")?)
            .map_err(|_| "API复核损坏")?;
        require(
            file.path() == receipt_path(root, &r.relative_path)?,
            "API复核文件名不符",
        )?;
        result.push(r.verify(root)?);
    }
    result.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(result)
}
pub(crate) fn acknowledge(
    root: &Path,
    relative: &str,
    expected: &str,
    reason: &str,
    mut observe: impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    crate::startup_review::reason(reason)?;
    sha256(expected)?;
    let (current, reviewed) = inspect(root, relative)?;
    require(current == expected, "API原文已变化，请刷新后重新复核")?;
    if reviewed {
        return Ok(());
    }
    let mut receipt = Receipt {
        version: 1,
        relative_path: relative.into(),
        evidence_digest: expected.into(),
        reason: reason.into(),
        at: chrono::Utc::now().to_rfc3339(),
        hash: String::new(),
    };
    receipt.hash = receipt.hash()?;
    let raw = serde_json::to_vec(&receipt).map_err(|_| "API复核编码失败")?;
    let final_path = receipt_path(root, relative)?;
    fs::create_dir_all(final_path.parent().unwrap()).map_err(|_| "API复核目录不可创建")?;
    ordinary(final_path.parent().unwrap(), true)?;
    require(
        fs::read_dir(final_path.parent().unwrap())
            .map_err(|_| "API复核目录不可读")?
            .take(10000)
            .count()
            < 10000,
        "API复核目录已满",
    )?;
    let pending = final_path.with_extension(format!("{}.pending", receipt.hash));
    let mut f = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&pending)
        .map_err(|_| "API复核预留失败")?;
    observe("created")?;
    f.write_all(&raw[..raw.len() / 2])
        .map_err(|_| "API复核写入失败")?;
    observe("partial")?;
    f.write_all(&raw[raw.len() / 2..])
        .and_then(|_| f.sync_all())
        .map_err(|_| "API复核同步失败")?;
    drop(f);
    observe("synced")?;
    require(
        fingerprint(root, relative)? == expected,
        "发布前API原文已变化",
    )?;
    fs::hard_link(&pending, final_path).map_err(|_| "API复核发布失败")?;
    observe("published")?;
    fs::remove_file(pending).map_err(|_| "API复核清理失败")?;
    observe("finished")
}
