//! An explicit user decision retires uncertainty, never certifies task success.
use crate::{artifacts::root_path, host::ordinary, scheduler::DurableWorkflow};
use cerulean_domain::execution::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Abandonment {
    schema_version: u32,
    project_id: String,
    operation_id: String,
    task_id: String,
    attempt_id: String,
    started_at: String,
    user_id: String,
    at: String,
    reason: String,
    review_digest: String,
    evidence_digest: String,
}
pub(crate) fn directory(root: &Path, job: &VerificationJob) -> PathBuf {
    root.join("workflows")
        .join(&job.project_id)
        .join("verification")
        .join(&job.operation_id)
}
fn bytes(path: &Path, limit: u64) -> Result<Option<Vec<u8>>, String> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
        Ok(_) => {}
    }
    ordinary(path, false)?;
    let mut value = vec![];
    File::open(path)
        .map_err(|e| e.to_string())?
        .take(limit + 1)
        .read_to_end(&mut value)
        .map_err(|e| e.to_string())?;
    require(
        value.len() as u64 <= limit,
        "reconciliation evidence too large",
    )?;
    Ok(Some(value))
}
fn evidence(dir: &Path) -> Result<String, String> {
    // A failed pre-launch attempt may have no runner directory yet.
    if dir.exists() {
        root_path(dir.to_str().ok_or("invalid evidence directory")?)?;
    }
    let mut hashes = vec![];
    for (name, limit) in [
        ("intent.json", 1024 * 1024),
        ("result.json", 1024 * 1024),
        ("snapshot.json", 8 * 1024 * 1024),
    ] {
        hashes.push((
            name,
            bytes(&dir.join(name), limit)?.map(|b| format!("{:x}", Sha256::digest(b))),
        ));
    }
    digest(&hashes)
}
pub(crate) fn review_digest(
    root: &Path,
    job: &VerificationJob,
    store: &DurableWorkflow,
) -> Result<String, String> {
    digest(&(
        &job.project_id,
        &job.operation_id,
        &job.task_id,
        &job.attempt_id,
        &job.started_at,
        store.history().last().map(|e| &e.hash),
        evidence(&directory(root, job))?,
    ))
}
/// None means no decision; a malformed or stale decision is an error, never a skip.
fn read(dir: &Path) -> Result<Option<Abandonment>, String> {
    let Some(bytes) = bytes(&dir.join("abandonment.json"), 64 * 1024)? else {
        return Ok(None);
    };
    let record: Abandonment = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    require(
        record.schema_version == 1 && record.user_id == "user",
        "invalid abandonment authority",
    )?;
    for id in [
        &record.project_id,
        &record.operation_id,
        &record.task_id,
        &record.attempt_id,
    ] {
        identifier(id)?;
    }
    timestamp(&record.started_at)?;
    timestamp(&record.at)?;
    sha256(&record.review_digest)?;
    require(
        !record.reason.trim().is_empty() && record.reason.len() <= 2000,
        "invalid abandonment reason",
    )?;
    require(
        dir.file_name()
            .is_some_and(|s| s == record.operation_id.as_str()),
        "abandonment operation mismatch",
    )?;
    require(
        dir.parent()
            .and_then(Path::parent)
            .and_then(Path::file_name)
            .is_some_and(|s| s == record.project_id.as_str()),
        "abandonment project directory mismatch",
    )?;
    require(
        record.evidence_digest == evidence(dir)?,
        "abandonment evidence changed; reconciliation required",
    )?;
    Ok(Some(record))
}
pub(crate) fn retired(dir: &Path) -> Result<bool, String> {
    Ok(read(dir)?.is_some())
}
pub(crate) fn applies(root: &Path, job: &VerificationJob) -> Result<bool, String> {
    let Some(record) = read(&directory(root, job))? else {
        return Ok(false);
    };
    require(
        record.project_id == job.project_id
            && record.task_id == job.task_id
            && record.attempt_id == job.attempt_id
            && record.started_at == job.started_at,
        "abandonment job mismatch",
    )?;
    Ok(true)
}
pub(crate) fn abandon(
    root: &Path,
    job: &VerificationJob,
    store: &DurableWorkflow,
    wanted: &str,
    reason: &str,
) -> Result<(), String> {
    require(
        !reason.trim().is_empty() && reason.len() <= 2000,
        "请填写不超过 2000 字节的放弃原因",
    )?;
    require(
        review_digest(root, job, store)? == wanted,
        "验收证据或任务状态已变化，请刷新后重新审阅",
    )?;
    let dir = directory(root, job);
    let parent = dir.parent().ok_or("invalid verification path")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    root_path(parent.to_str().ok_or("invalid verification parent")?)?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    root_path(dir.to_str().ok_or("invalid verification directory")?)?;
    if applies(root, job)? {
        return Ok(());
    }
    let record = Abandonment {
        schema_version: 1,
        project_id: job.project_id.clone(),
        operation_id: job.operation_id.clone(),
        task_id: job.task_id.clone(),
        attempt_id: job.attempt_id.clone(),
        started_at: job.started_at.clone(),
        user_id: "user".into(),
        at: chrono::Utc::now().to_rfc3339(),
        reason: reason.trim().into(),
        review_digest: wanted.into(),
        evidence_digest: evidence(&dir)?,
    };
    let dest = dir.join("abandonment.json");
    let temp = dir.join(format!(
        "abandonment-{}.tmp",
        chrono::Utc::now()
            .timestamp_nanos_opt()
            .ok_or("clock out of range")?
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec(&record).map_err(|e| e.to_string())?)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    drop(file);
    require(!dest.exists(), "abandonment already exists")?;
    fs::rename(temp, dest).map_err(|e| e.to_string())
}
