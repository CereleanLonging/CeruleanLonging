//! Upgrade quiescence and a durable, explicitly released startup hold.
use crate::diagnostic_service::ServiceHost;
use cerulean_domain::{execution::digest, Snapshot};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
};
pub(crate) const HOLD: &str = "upgrade-hold.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpgradeReview {
    pub digest: String,
    pub ready: bool,
    pub blockers: Vec<String>,
    pub snapshot_digest: String,
    pub core_version: String,
    pub core_sha256: String,
    pub server_pid: u32,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Hold {
    schema: u32,
    root: String,
    owner_sid: String,
    created_at: String,
    review: UpgradeReview,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HoldStatus {
    pub hold_digest: String,
    pub valid_record: bool,
    pub review: Option<UpgradeReview>,
}
pub(crate) fn review(host: &mut ServiceHost) -> Result<(UpgradeReview, Snapshot), String> {
    let stoppable = host.can_shutdown();
    let snapshot = host.snapshot();
    let mut blockers = Vec::new();
    if snapshot
        .team_import
        .as_ref()
        .is_some_and(|v| v.state != "completed")
    {
        blockers.push("team-import-unresolved".into());
    }
    if !stoppable {
        blockers.push("active-or-unresolved-work".into());
    }
    if snapshot.core_diagnostic.is_some()
        || snapshot.api_recovery.is_some()
        || !snapshot.omp_stop_issues.is_empty()
    {
        blockers.push("diagnostic-or-stop-review".into());
    }
    if !snapshot.management_record_reviews.is_empty()
        || !snapshot.startup_closure_reviews.is_empty()
        || !snapshot.startup_reservations.is_empty()
        || !snapshot.api_publication_reviews.is_empty()
    {
        blockers.push("unresolved-publication-or-startup".into());
    }
    if snapshot
        .management_history
        .projects
        .iter()
        .any(|p| p.pending > 0 || p.running > 0 || p.waiting > 0)
    {
        blockers.push("unresolved-management".into());
    }
    if snapshot.desktop_access.as_ref().is_some_and(|d| {
        d.checking
            || d.trials
                .iter()
                .any(|t| t.proof.as_ref().is_none_or(|p| !p.completed))
            || d.tasks.iter().any(|t| {
                t.active
                    || t.reading
                    || t.stopping
                    || (!t.submitted
                        && !t.retired
                        && !t.cancellation.as_ref().is_some_and(|c| c.confirmed))
            })
    }) {
        blockers.push("unresolved-desktop-history".into());
    }
    let mut result = UpgradeReview {
        digest: String::new(),
        ready: blockers.is_empty(),
        blockers,
        snapshot_digest: digest(&snapshot)?,
        core_version: env!("CARGO_PKG_VERSION").into(),
        core_sha256: runtime_hash()?,
        server_pid: std::process::id(),
    };
    result.digest = digest(&result)?;
    Ok((result, snapshot))
}
pub(crate) fn runtime_hash() -> Result<String, String> {
    static HASH: std::sync::OnceLock<Result<String, String>> = std::sync::OnceLock::new();
    HASH.get_or_init(|| {
        let path = std::env::current_exe().map_err(|e| e.to_string())?;
        let bytes = fs::read(path).map_err(|e| e.to_string())?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    })
    .clone()
}
pub(crate) fn ensure_not_held(root: &Path) -> Result<(), String> {
    match fs::symlink_metadata(root.join(HOLD)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(
            "核心处于升级锁定状态；先用 upgrade status 审阅，再明确 release，禁止自动重启".into(),
        ),
    }
}
pub(crate) fn prepare(root: &Path, review: UpgradeReview) -> Result<HoldStatus, String> {
    let record = Hold {
        schema: 1,
        root: root.to_string_lossy().into_owned(),
        owner_sid: crate::ipc::current_sid().map_err(|e| e.to_string())?,
        created_at: chrono::Utc::now().to_rfc3339(),
        review,
    };
    let bytes = serde_json::to_vec_pretty(&record).map_err(|e| e.to_string())?;
    // A partial record still blocks startup. Never remove it on a write failure.
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(HOLD))
        .map_err(|e| e.to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    drop(file);
    status(root)
}
fn read(root: &Path) -> Result<(Vec<u8>, HoldStatus), String> {
    let canonical = fs::canonicalize(root).map_err(|e| e.to_string())?;
    read_record(&canonical, &canonical.join(HOLD))
}
fn read_record(original_root: &Path, record_path: &Path) -> Result<(Vec<u8>, HoldStatus), String> {
    let mut file = crate::backup::open(record_path, false, false).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 1024 * 1024 {
        return Err("升级记录超过上限".into());
    }
    let record = serde_json::from_slice::<Hold>(&bytes).ok().filter(|r| {
        r.schema == 1
            && Path::new(&r.root) == original_root
            && crate::ipc::current_sid().is_ok_and(|sid| sid == r.owner_sid)
            && r.review.ready
            && r.review.blockers.is_empty()
            && {
                let mut copy = r.review.clone();
                copy.digest.clear();
                digest(&copy).is_ok_and(|d| d == r.review.digest)
            }
    });
    let result = HoldStatus {
        hold_digest: format!("{:x}", Sha256::digest(&bytes)),
        valid_record: record.is_some(),
        review: record.map(|r| r.review),
    };
    Ok((bytes, result))
}
pub fn status(root: &Path) -> Result<HoldStatus, String> {
    read(root).map(|(_, r)| r)
}
/// Inspect preserved hold bytes against the canonical original location, never
/// against the backup payload directory, and never reinstate a released hold.
pub(crate) fn inspect_preserved(
    original_root: &Path,
    record_path: &Path,
) -> Result<HoldStatus, String> {
    read_record(original_root, record_path).map(|(_, r)| r)
}
pub fn release(root: &Path, expected: &str) -> Result<HoldStatus, String> {
    crate::backup_recovery::ensure_not_recovering(root)?;
    let root = fs::canonicalize(root).map_err(|e| e.to_string())?;
    let _directory = crate::backup::open(&root, true, false).map_err(|e| e.to_string())?;
    let _lock =
        crate::backup::open(&root.join("writer.lock"), false, true).map_err(|e| e.to_string())?;
    crate::backup_recovery::ensure_not_recovering(&root)?;
    let (_, result) = read(&root)?;
    if result.hold_digest != expected {
        return Err("升级锁定记录摘要已变化，未解除".into());
    }
    crate::data_format::status(&root)?;
    let history = root.join("upgrade-holds");
    if !history.exists() {
        fs::create_dir(&history).map_err(|e| e.to_string())?;
    }
    let _history = crate::backup::open(&history, true, false).map_err(|e| e.to_string())?;
    let destination = history.join(format!(
        "released-{}-{}.json",
        result.hold_digest,
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    crate::backup::publish(&root.join(HOLD), &destination).map_err(|e| e.to_string())?;
    Ok(result)
}
