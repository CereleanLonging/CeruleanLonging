//! Explicitly retain an unresolved import, without changing its source or backup.
//! The separate receipt records a user's disposition; it never unlocks execution.
use crate::{backup, team_import_service};
use cerulean_domain::{
    execution::{digest, identifier, require, sha256, timestamp},
    team_timing::TeamImportView,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Review {
    pub digest: String,
    source_root: PathBuf,
    backup: PathBuf,
    manifest_digest: String,
    receipt_directory: PathBuf,
    reason: String,
    import: TeamImportView,
    minimum_reader: u32,
    execution_unlocked: bool,
    source_modified: bool,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Receipt {
    schema: u32,
    recorded_at: String,
    pub review: Review,
}
fn destination(path: &Path, source: &Path, archive: &Path) -> Result<(PathBuf, fs::File), String> {
    require(path.is_absolute(), "保留决定目录须为绝对路径")?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("保留决定目录名无效")?;
    identifier(name)?;
    let parent = fs::canonicalize(path.parent().ok_or("保留决定目录缺少父目录")?)
        .map_err(|e| e.to_string())?;
    let pin = backup::open(&parent, true, false).map_err(|e| e.to_string())?;
    let result = parent.join(name);
    require(
        !result.starts_with(source) && !result.starts_with(archive),
        "保留决定须保存到原数据和原备份以外的新目录",
    )?;
    Ok((result, pin))
}
fn reviewed(
    root: &Path,
    archive: &Path,
    manifest: &str,
    to: &Path,
    reason: &str,
) -> Result<Review, String> {
    sha256(manifest)?;
    require(
        !reason.trim().is_empty()
            && reason.len() <= 2000
            && !reason.chars().any(|c| c.is_control()),
        "请填写保留原因，不超过2000字节且不含控制字符",
    )?;
    let status = crate::data_format::status(root)?;
    require(
        status.minimum_reader_generation >= 14,
        "原实验数据保护不足；先按原冷备份审阅保护，不重派任务",
    )?;
    let import = team_import_service::view(root).ok_or("没有需要保留处置的原团队导入")?;
    require(
        matches!(import.state.as_str(), "failed" | "interrupted"),
        "仅可处置未完成或中断的原导入；运行中和已完成导入不适用",
    )?;
    let archive = fs::canonicalize(archive).map_err(|e| e.to_string())?;
    let (receipt_directory, _pin) = destination(to, root, &archive)?;
    let mut review = Review {
        digest: String::new(),
        source_root: root.to_owned(),
        backup: archive,
        manifest_digest: manifest.into(),
        receipt_directory,
        reason: reason.trim().into(),
        import,
        minimum_reader: status.minimum_reader_generation,
        execution_unlocked: false,
        source_modified: false,
    };
    review.digest = digest(&review)?;
    Ok(review)
}
pub fn review(
    root: &Path,
    archive: &Path,
    manifest: &str,
    to: &Path,
    reason: &str,
) -> Result<Review, String> {
    backup::with_original_cold(root, archive, manifest, |root| {
        reviewed(root, archive, manifest, to, reason)
    })
}
pub fn retain(
    root: &Path,
    archive: &Path,
    manifest: &str,
    to: &Path,
    reason: &str,
    expected: &str,
) -> Result<Receipt, String> {
    backup::with_original_cold(root, archive, manifest, |root| {
        let review = reviewed(root, archive, manifest, to, reason)?;
        require(
            review.digest == expected,
            "原导入、备份、原因或保存位置已变化，请重新审阅",
        )?;
        let (dest, _parent) = destination(to, root, &review.backup)?;
        let receipt = Receipt {
            schema: 1,
            recorded_at: chrono::Utc::now().to_rfc3339(),
            review,
        };
        let bytes = serde_json::to_vec(&receipt).map_err(|e| e.to_string())?;
        require(bytes.len() <= 65536, "保留决定记录过大")?;
        fs::create_dir(&dest)
            .map_err(|e| format!("保留决定目录已存在或不可创建；先核对原记录，不重复执行：{e}"))?;
        let _directory = backup::open(&dest, true, false).map_err(|e| e.to_string())?;
        let pending = dest.join("retention.json.pending");
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&pending)
            .map_err(|e| e.to_string())?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);
        backup::publish(&pending, &dest.join("retention.json")).map_err(|e| e.to_string())?;
        Ok(receipt)
    })
}
pub fn inspect(
    root: &Path,
    archive: &Path,
    manifest: &str,
    to: &Path,
    reason: &str,
    expected: &str,
) -> Result<Receipt, String> {
    backup::with_original_cold(root, archive, manifest, |root| {
        let review = reviewed(root, archive, manifest, to, reason)?;
        require(
            review.digest == expected,
            "当前原数据与已审阅保留决定不一致",
        )?;
        let (dest, _parent) = destination(to, root, &review.backup)?;
        let _dir = backup::open(&dest, true, false).map_err(|e| e.to_string())?;
        let names = fs::read_dir(&dest)
            .map_err(|e| e.to_string())?
            .map(|e| e.map(|v| v.file_name()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        require(
            names.len() == 1 && names[0] == "retention.json",
            "保留决定发布不完整或含其他记录；保留现场，不覆盖或自动重试",
        )?;
        let file =
            backup::open(&dest.join("retention.json"), false, false).map_err(|e| e.to_string())?;
        let mut bytes = vec![];
        file.take(65537)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        require(bytes.len() <= 65536, "保留决定记录过大")?;
        let receipt: Receipt = serde_json::from_slice(&bytes).map_err(|_| "保留决定记录无效")?;
        timestamp(&receipt.recorded_at)?;
        require(
            receipt.schema == 1
                && serde_json::to_value(&receipt.review).map_err(|e| e.to_string())?
                    == serde_json::to_value(&review).map_err(|e| e.to_string())?,
            "保留决定与当前原记录不一致",
        )?;
        Ok(receipt)
    })
}
#[cfg(test)]
mod tests;
