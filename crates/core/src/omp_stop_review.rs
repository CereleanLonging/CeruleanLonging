//! User acknowledgements of a truncated, unpublished stop request. Original
//! bytes stay in place; this is not evidence that a worker stopped or completed.
use crate::{
    async_omp::{cancel_path, reason, MAX_CANCEL_BYTES},
    host::ordinary,
    startup_journal::StartupIntent,
};
use cerulean_domain::{
    execution::{digest, require, timestamp},
    omp_jobs::OmpStopIssue,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipt {
    version: u32,
    startup_head: String,
    evidence_digest: String,
    reason: String,
    at: String,
    hash: String,
}
fn optional_bytes(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("停止复核文件不可读".into()),
        Ok(_) => ordinary(path, false)?,
    }
    let mut bytes = vec![];
    fs::File::open(path)
        .map_err(|_| "停止复核文件不可读")?
        .take((MAX_CANCEL_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "停止复核读取失败")?;
    require(bytes.len() <= MAX_CANCEL_BYTES, "停止复核文件过大")?;
    Ok(Some(bytes))
}
fn review_path(root: &Path, intent: &StartupIntent) -> Result<PathBuf, String> {
    let folder = root.join("omp-stop-reviews");
    match fs::symlink_metadata(&folder) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("停止复核目录不可读".into()),
        Ok(_) => ordinary(&folder, true)?,
    }
    Ok(folder.join(
        cancel_path(root, intent)?
            .file_name()
            .ok_or("停止记录名称缺失")?,
    ))
}
// Only EOF-truncated JSON with no committed name can be acknowledged here.
// Complete invalid, foreign, conflicting, linked or oversized files need repair
// from trusted backups; a user acknowledgement must not bless their contents.
fn fingerprint(
    root: &Path,
    intent: &StartupIntent,
    head: &str,
) -> Result<(String, u32, String), String> {
    let path = cancel_path(root, intent)?;
    ordinary(path.parent().unwrap(), true)?;
    require(
        optional_bytes(&path)?.is_none(),
        "已有正式停止记录，不能按未发布中断处置",
    )?;
    let bytes = optional_bytes(&path.with_extension("pending"))?.ok_or("中断停止记录缺失")?;
    require(
        serde_json::from_slice::<serde_json::Value>(&bytes).is_err_and(|e| e.is_eof()),
        "仅允许复核未写完整的停止记录",
    )?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let size = bytes.len() as u32;
    let evidence = digest(&(
        1,
        &intent.project_id,
        &intent.attempt_id,
        head,
        &hash,
        size,
        "committed-absent",
    ))?;
    Ok((hash, size, evidence))
}
pub(crate) fn accepted(
    root: &Path,
    intent: &StartupIntent,
    head: &str,
) -> Result<Option<String>, String> {
    let Some(bytes) = optional_bytes(&review_path(root, intent)?)? else {
        return Ok(None);
    };
    let receipt: Receipt = serde_json::from_slice(&bytes).map_err(|_| "停止复核凭据无效")?;
    let (_, _, evidence) = fingerprint(root, intent, head)?;
    require(
        receipt.version == 1
            && receipt.startup_head == head
            && receipt.evidence_digest == evidence
            && receipt.hash
                == digest(&(
                    receipt.version,
                    &receipt.startup_head,
                    &receipt.evidence_digest,
                    &receipt.reason,
                    &receipt.at,
                ))?,
        "停止复核凭据与原始证据不符",
    )?;
    reason(&receipt.reason)?;
    timestamp(&receipt.at)?;
    Ok(Some(receipt.reason))
}
pub(crate) fn inspect(
    root: &Path,
    intent: &StartupIntent,
    head: &str,
    message: String,
) -> OmpStopIssue {
    let mut issue = OmpStopIssue {
        project_id: intent.project_id.clone(),
        task_id: intent.task_id.clone(),
        attempt_id: intent.attempt_id.clone(),
        message,
        pending_path: cancel_path(root, intent)
            .map(|p| p.with_extension("pending").display().to_string())
            .unwrap_or_default(),
        pending_sha256: None,
        pending_bytes: None,
        evidence_digest: None,
        acknowledged: false,
    };
    if let Ok((hash, size, evidence)) = fingerprint(root, intent, head) {
        issue.pending_sha256 = Some(hash);
        issue.pending_bytes = Some(size);
        // A damaged acknowledgement never gains an overwrite/repair shortcut.
        if matches!(accepted(root, intent, head), Ok(None)) {
            issue.evidence_digest = Some(evidence);
        }
    }
    issue
}
pub(crate) fn validate_review(
    root: &Path,
    intent: &StartupIntent,
    head: &str,
    expected: &str,
    text: &str,
) -> Result<(), String> {
    reason(text)?;
    require(
        fingerprint(root, intent, head)?.2 == expected,
        "停止复核证据已变化，请重新读取",
    )?;
    accepted(root, intent, head)?;
    Ok(())
}
pub(crate) fn acknowledge_observed(
    root: &Path,
    intent: &StartupIntent,
    head: &str,
    expected: &str,
    text: &str,
    mut observe: impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    reason(text)?;
    require(
        fingerprint(root, intent, head)?.2 == expected,
        "停止复核证据已变化，请重新读取",
    )?;
    if accepted(root, intent, head)?.is_some() {
        return Ok(());
    }
    let mut r = Receipt {
        version: 1,
        startup_head: head.into(),
        evidence_digest: expected.into(),
        reason: text.into(),
        at: chrono::Utc::now().to_rfc3339(),
        hash: String::new(),
    };
    r.hash = digest(&(
        r.version,
        &r.startup_head,
        &r.evidence_digest,
        &r.reason,
        &r.at,
    ))?;
    let bytes = serde_json::to_vec(&r).map_err(|_| "停止复核编码失败")?;
    require(bytes.len() <= MAX_CANCEL_BYTES, "停止复核记录过大")?;
    let path = review_path(root, intent)?;
    let parent = path.parent().unwrap();
    fs::create_dir_all(parent).map_err(|_| "停止复核目录创建失败")?;
    ordinary(parent, true)?;
    require(
        fs::read_dir(parent)
            .map_err(|_| "停止复核目录不可读")?
            .take(10001)
            .count()
            < 10000,
        "停止复核记录已满",
    )?;
    let pending = path.with_extension(format!("{}.pending", r.hash));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)
        .map_err(|_| "停止复核临时记录创建失败")?;
    observe("created")?;
    let mid = bytes.len() / 2;
    file.write_all(&bytes[..mid])
        .map_err(|_| "停止复核记录写入失败")?;
    observe("partial")?;
    file.write_all(&bytes[mid..])
        .and_then(|_| file.sync_all())
        .map_err(|_| "停止复核记录同步失败")?;
    drop(file);
    observe("synced")?;
    require(
        fingerprint(root, intent, head)?.2 == expected,
        "停止复核发布前原始证据已变化",
    )?;
    fs::hard_link(&pending, &path).map_err(|_| "停止复核发布失败")?;
    observe("published")?;
    fs::remove_file(&pending).map_err(|_| "停止复核收尾失败")?;
    observe("finished")
}
