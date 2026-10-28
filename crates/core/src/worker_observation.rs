//! Bounded projections owned by the host, separate from the execution ledger.
use crate::{async_codex::ActiveCodex, host::ordinary};
use cerulean_domain::supervision::{repeated_failures, SupervisionPolicy};
use cerulean_domain::{execution::*, worker_observation::*, CodexJob};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    schema_version: u32,
    view: WorkerObservationView,
}
fn fingerprint(
    job: &CodexJob,
    trace: &WorkerObservationTrace,
    policy: Option<&SupervisionPolicy>,
) -> Result<String, String> {
    let legacy = digest(&(
        "worker-observation-v1",
        &job.project_id,
        &job.task_id,
        &job.attempt_id,
        &job.revision_digest,
        trace,
    ))?;
    match policy {
        Some(policy) => digest(&("worker-supervision-v1", legacy, policy)),
        None => Ok(legacy),
    }
}
pub(crate) fn view(
    job: &CodexJob,
    trace: WorkerObservationTrace,
    error: Option<String>,
    policy: Option<SupervisionPolicy>,
) -> Result<WorkerObservationView, String> {
    let scope = digest(&(
        &job.project_id,
        &job.task_id,
        &job.attempt_id,
        &job.revision_digest,
    ))?;
    let incidents = policy
        .as_ref()
        .map(|p| repeated_failures(p, &trace, &scope))
        .transpose()?
        .unwrap_or_default();
    Ok(WorkerObservationView {
        project_id: job.project_id.clone(),
        task_id: job.task_id.clone(),
        attempt_id: job.attempt_id.clone(),
        revision_digest: job.revision_digest.clone(),
        evidence_digest: fingerprint(job, &trace, policy.as_ref())?,
        supervision: policy,
        incidents,
        trace,
        storage_error: error,
    })
}
fn folder(root: &Path, job: &CodexJob, create: bool) -> Result<PathBuf, String> {
    identifier(&job.project_id)?;
    identifier(&job.attempt_id)?;
    let base = root.join("worker-observations");
    if create {
        fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    }
    ordinary(&base, true)?;
    let dir = base.join(&job.project_id);
    if create {
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    }
    ordinary(&dir, true)?;
    Ok(dir)
}
pub(crate) fn persist(root: &Path, active: &mut ActiveCodex) {
    let Some(capture) = &active.observation else {
        return;
    };
    let trace = capture.snapshot();
    let result = (|| {
        let policy = active
            .snapshot
            .revision
            .as_ref()
            .ok_or("监督原方案缺失")?
            .supervision_for(&active.job.task_id)?
            .cloned();
        let view = view(&active.job, trace, None, policy)?;
        if active.observation_saved.as_ref() == Some(&view.evidence_digest) {
            return Ok(());
        }
        let dir = folder(root, &active.job, true)?;
        let target = dir.join(format!("{}.json", active.job.attempt_id));
        if target.exists() {
            ordinary(&target, false)?;
        }
        let tmp = dir.join(format!(
            "{}.{}.tmp",
            active.job.attempt_id,
            chrono::Utc::now()
                .timestamp_nanos_opt()
                .ok_or("clock range")?
        ));
        let digest = view.evidence_digest.clone();
        let bytes = serde_json::to_vec(&Record {
            schema_version: 1,
            view,
        })
        .map_err(|e| e.to_string())?;
        require(
            bytes.len() <= 256 * 1024,
            "observation projection too large",
        )?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|e| e.to_string())?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);
        fs::rename(tmp, target).map_err(|e| e.to_string())?;
        active.observation_saved = Some(digest);
        Ok(())
    })();
    // Storage trouble does not rewrite execution evidence or trigger a model call.
    active.observation_error = result
        .err()
        .map(|_: String| "执行观察尚未可靠保存；记录可能不完整".into());
}
pub(crate) fn read(root: &Path, job: &CodexJob) -> Result<WorkerObservationView, String> {
    let target = folder(root, job, false)?.join(format!("{}.json", job.attempt_id));
    ordinary(&target, false)?;
    let mut bytes = vec![];
    fs::File::open(target)
        .map_err(|e| e.to_string())?
        .take(256 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    require(bytes.len() <= 256 * 1024, "观察记录超过大小限制")?;
    let record: Record = serde_json::from_slice(&bytes).map_err(|_| "观察记录不可解析")?;
    let v = record.view;
    require(
        record.schema_version == 1
            && v.project_id == job.project_id
            && v.task_id == job.task_id
            && v.attempt_id == job.attempt_id
            && v.revision_digest == job.revision_digest
            && v.storage_error.is_none()
            && v.evidence_digest == fingerprint(job, &v.trace, v.supervision.as_ref())?,
        "观察记录关联或摘要不符",
    )?;
    require(
        v.trace.events.len() <= 128 && v.trace.thread_id.is_some() == v.trace.turn_id.is_some(),
        "观察范围无效",
    )?;
    require(
        v.trace.events.is_empty() || v.trace.turn_id.is_some(),
        "观察缺少轮次关联",
    )?;
    let mut items = std::collections::BTreeSet::new();
    for (i, event) in v.trace.events.iter().enumerate() {
        require(
            event.sequence == i as u32 + 1 && items.insert(event.item_digest.clone()),
            "观察序号或去重记录无效",
        )?;
        sha256(&event.item_digest)?;
        if let Some(a) = &event.action_digest {
            sha256(a)?;
        }
        require(
            match event.kind {
                ObservedToolKind::Command => event.action_digest.is_some(),
                ObservedToolKind::FileChange => {
                    event.action_digest.is_none() && event.exit_code.is_none()
                }
            },
            "观察元数据无效",
        )?;
    }
    let expected = view(job, v.trace.clone(), None, v.supervision.clone())?;
    require(
        v.incidents == expected.incidents,
        "异常记录与原观察证据不符",
    )?;
    Ok(v)
}
