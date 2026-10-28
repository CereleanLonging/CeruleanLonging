//! API reservations: recover complete original bytes, review truncated publications, never retry a send.
use crate::host::ordinary;
use cerulean_domain::{api_worker::ApiWorkerSelection, execution::*};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ApiStartup {
    #[serde(default, skip_serializing_if = "is_false")]
    pub prepare_workspace: bool,
    pub version: u32,
    pub project_id: String,
    pub attempt: Attempt,
    pub worker: ApiWorkerSelection,
    pub management_operation: String,
    pub management_hash: String,
    pub workflow_sequence: u32,
    pub workflow_hash: String,
    pub started_at: String,
}
fn is_false(value: &bool) -> bool {
    !*value
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    intent: ApiStartup,
    hash: String,
}
pub(crate) type Startups = BTreeMap<(String, String), ApiStartup>;
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Cancellation {
    startup_hash: String,
    reason: String,
    at: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CancelRecord {
    body: Cancellation,
    hash: String,
}
pub(crate) fn validate_cancellations(root: &Path, startups: &Startups) -> Result<(), String> {
    let reviewed = crate::api_publication::reviews(root)?
        .into_iter()
        .map(|r| r.relative_path)
        .collect();
    for (stem, raw) in crate::api_publication::records(root, "api-cancellations", &reviewed)? {
        let startup = startups
            .values()
            .find(|s| s.filename().is_ok_and(|f| f == format!("{stem}.json")))
            .ok_or("API停止记录缺少原启动")?;
        let sealed: CancelRecord = serde_json::from_slice(&raw).map_err(|_| "API停止记录无效")?;
        require(sealed.hash == digest(&sealed.body)?, "API停止记录摘要不符")?;
        let record = sealed.body;
        require(
            record.startup_hash == startup.reservation_digest()?
                && !record.reason.trim().is_empty()
                && record.reason.len() <= 8000
                && !record.reason.contains('\0'),
            "API停止记录身份或原因不符",
        )?;
        timestamp(&record.at)?;
    }
    Ok(())
}
pub(crate) fn cancel(root: &Path, startup: &ApiStartup, reason: &str) -> Result<(), String> {
    cancel_observed(root, startup, reason, |_| Ok(()))
}
pub(crate) fn cancel_observed(
    root: &Path,
    startup: &ApiStartup,
    reason: &str,
    observe: impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    require(
        !reason.trim().is_empty() && reason.len() <= 8000 && !reason.contains('\0'),
        "请输入有效停止原因",
    )?;
    let dir = root.join("api-cancellations");
    ordinary(&dir, true)?;
    let path = dir.join(startup.filename()?);
    let body = Cancellation {
        startup_hash: startup.reservation_digest()?,
        reason: reason.into(),
        at: chrono::Utc::now().to_rfc3339(),
    };
    let bytes = serde_json::to_vec(&CancelRecord {
        hash: digest(&body)?,
        body,
    })
    .map_err(|_| "API停止编码失败")?;
    write_publication(&path, &bytes, observe)
}
impl ApiStartup {
    pub(crate) fn reservation_digest(&self) -> Result<String, String> {
        let mut original = self.clone();
        if original.prepare_workspace {
            original.attempt.workspace_proof = None;
        }
        digest(&original)
    }
    pub(crate) fn validate_workspace(&self, plan: &PlanRevision) -> Result<(), String> {
        if self.version == 2 {
            require(
                self.prepare_workspace
                    && plan.worktree.is_some()
                    && plan.task(&self.attempt.task_id)?.api_worker.is_some()
                    && plan.workflow.tasks.iter().any(|t| {
                        t.id == self.attempt.task_id
                            && isolated_task_key(&plan.project.id, &t.id)
                                .is_ok_and(|key| key == t.workspace_key)
                    }),
                "scoped API startup requires approved task isolation",
            )?;
        }
        if self.prepare_workspace {
            require(
                plan.worktree.is_some(),
                "API preparation has no approved workspace policy",
            )?;
            if self.attempt.workspace_proof.is_none() {
                return require(
                    self.attempt.binding.workspace
                        == plan.execution_root(&self.attempt.task_id, &self.attempt.id)?,
                    "pending workspace path differs from approval",
                );
            }
        }
        plan.validate_attempt_workspace(&self.attempt)
    }
    fn matches_prepared(&self, prepared: &Self) -> Result<(), String> {
        prepared.validate()?;
        require(
            self.prepare_workspace
                && self.attempt.workspace_proof.is_none()
                && prepared.prepare_workspace
                && prepared.attempt.workspace_proof.is_some()
                && self.reservation_digest()? == prepared.reservation_digest()?,
            "API workspace binding differs from reservation",
        )
    }
    pub fn validate(&self) -> Result<(), String> {
        identifier(&self.project_id)?;
        identifier(&self.management_operation)?;
        identifier(&self.attempt.id)?;
        identifier(&self.attempt.task_id)?;
        self.worker.validate()?;
        timestamp(&self.started_at)?;
        sha256(&self.management_hash)?;
        sha256(&self.workflow_hash)?;
        sha256(&self.attempt.revision_digest)?;
        if let Some(proof) = &self.attempt.workspace_proof {
            sha256(&proof.record_digest)?;
        }
        self.attempt.binding.validate_controller_identity()?;
        require(
            matches!(self.version, 1 | 2)
                && (self.version == 1 || self.prepare_workspace)
                && self.workflow_sequence > 0
                && self.attempt.fence > 0
                && self.attempt.id.len() <= 64
                && self.attempt.worker_id
                    == format!("worker-{}", self.worker.target.provider().id())
                && self.attempt.binding.adapter_id == self.worker.target.provider().id()
                && self.attempt.binding.adapter_version == "api-worker-v1"
                && Path::new(&self.attempt.binding.workspace).is_absolute()
                && matches!(
                    self.attempt.binding.identity_strength,
                    IdentityStrength::ControllerRequest(_)
                ),
            "invalid API startup identity",
        )
    }
    pub fn key(&self) -> (String, String) {
        (self.project_id.clone(), self.attempt.id.clone())
    }
    fn filename(&self) -> Result<String, String> {
        Ok(format!("{}.json", digest(&self.key())?))
    }
}
fn decode(stem: &str, raw: &[u8]) -> Result<ApiStartup, String> {
    let record: Record = serde_json::from_slice(raw).map_err(|_| "API启动记录无效")?;
    record.intent.validate()?;
    require(
        record.hash == digest(&record.intent)?
            && record.intent.filename()? == format!("{stem}.json"),
        "API启动摘要或名称不符",
    )?;
    Ok(record.intent)
}
pub(crate) fn original(root: &Path, stem: &str) -> Result<ApiStartup, String> {
    let pairs = crate::api_publication::pairs(root, "api-startups")?;
    let pair = pairs.get(stem).ok_or("API停止复核缺少原启动")?;
    let raw = crate::api_publication::complete(pair)?.ok_or("API原启动尚未完整")?;
    decode(stem, raw)
}
pub(crate) fn available(root: &Path, project: &str, attempt: &str) -> Result<(), String> {
    let folder = root.join("api-startups");
    ordinary(&folder, true)?;
    let stem = digest(&(project, attempt))?;
    for ext in ["json", "pending"] {
        require(
            crate::startup_review::bytes(&folder.join(format!("{stem}.{ext}")))?.is_none(),
            "API尝试编号已有原记录或半写保留，禁止复用",
        )?;
    }
    Ok(())
}
pub(crate) fn load(root: &Path) -> Result<Startups, String> {
    let reviewed = crate::api_publication::reviews(root)?
        .into_iter()
        .map(|r| r.relative_path)
        .collect();
    let mut result = Startups::new();
    let mut owners = std::collections::BTreeSet::new();
    for (stem, raw) in crate::api_publication::records(root, "api-startups", &reviewed)? {
        let intent = decode(&stem, &raw)?;
        require(
            !intent.prepare_workspace || intent.attempt.workspace_proof.is_none(),
            "original API preparation already contains a workspace proof",
        )?;
        require(
            owners.insert(intent.management_operation.clone())
                && !result.contains_key(&intent.key()),
            "duplicate API startup ownership",
        )?;
        result.insert(intent.key(), intent);
    }
    for (stem, raw) in crate::api_publication::records(root, "api-workspace-bindings", &reviewed)? {
        let prepared = decode(&stem, &raw)?;
        let original = result
            .get(&prepared.key())
            .ok_or("API workspace binding lacks reservation")?;
        original.matches_prepared(&prepared)?;
        result.insert(prepared.key(), prepared);
    }
    Ok(result)
}
pub(crate) fn publish_prepared(root: &Path, prepared: &ApiStartup) -> Result<(), String> {
    let name = prepared.filename()?;
    let stem = name
        .strip_suffix(".json")
        .ok_or("API binding filename invalid")?;
    original(root, stem)?.matches_prepared(prepared)?;
    let dir = root.join("api-workspace-bindings");
    ordinary(&dir, true)?;
    let raw = serde_json::to_vec(&Record {
        intent: prepared.clone(),
        hash: digest(prepared)?,
    })
    .map_err(|e| e.to_string())?;
    require(raw.len() <= 65536, "API workspace binding too large")?;
    write_publication(&dir.join(name), &raw, |_| Ok(()))
}
pub(crate) fn read_prepared(root: &Path, pending: &ApiStartup) -> Result<ApiStartup, String> {
    let name = pending.filename()?;
    let stem = name
        .strip_suffix(".json")
        .ok_or("API binding filename invalid")?;
    let pairs = crate::api_publication::pairs(root, "api-workspace-bindings")?;
    let raw =
        crate::api_publication::complete(pairs.get(stem).ok_or("API workspace binding missing")?)?
            .ok_or("API workspace binding incomplete")?;
    let prepared = decode(stem, raw)?;
    pending.matches_prepared(&prepared)?;
    Ok(prepared)
}
pub(crate) fn publish(root: &Path, intent: &ApiStartup) -> Result<(), String> {
    publish_observed(root, intent, |_| Ok(()))
}
pub(crate) fn publish_observed(
    root: &Path,
    intent: &ApiStartup,
    observe: impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    intent.validate()?;
    require(
        !intent.prepare_workspace || intent.attempt.workspace_proof.is_none(),
        "API reservation must precede workspace binding",
    )?;
    available(root, &intent.project_id, &intent.attempt.id)?;
    let dir = root.join("api-startups");
    ordinary(&dir, true)?;
    require(
        fs::read_dir(&dir)
            .map_err(|_| "API startup unreadable")?
            .count()
            < 1000,
        "API startup history full",
    )?;
    let path = dir.join(intent.filename()?);
    let bytes = serde_json::to_vec(&Record {
        intent: intent.clone(),
        hash: digest(intent)?,
    })
    .map_err(|_| "API startup encoding failed")?;
    require(bytes.len() <= 65536, "API startup too large")?;
    write_publication(&path, &bytes, observe)
}
fn write_publication(
    path: &Path,
    bytes: &[u8],
    mut observe: impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    let pending = path.with_extension("pending");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)
        .map_err(|_| "API startup reservation exists or unavailable")?;
    observe("created")?;
    file.write_all(&bytes[..bytes.len() / 2])
        .map_err(|_| "API publication write failed")?;
    observe("partial")?;
    file.write_all(&bytes[bytes.len() / 2..])
        .and_then(|_| file.sync_all())
        .map_err(|_| "API startup sync failed")?;
    drop(file);
    observe("synced")?;
    fs::hard_link(&pending, path).map_err(|_| "API startup immutable publication failed")?;
    observe("published")?;
    fs::remove_file(pending).map_err(|_| "API startup pending cleanup failed")?;
    observe("finished")
}
