//! Explicit retirement reclaims only the original check's temporary ACLs.
//! Original evidence and failed results remain immutable. Opening a Host never
//! calls this path; a repeated authenticated retirement resumes its cleanup.
use super::*;
use crate::scheduler::DurableWorkflow;
use cerulean_domain::verification_recheck::RecheckDispositionReview;

#[derive(Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Lease {
    schema: u32,
    paths: Vec<PathBuf>,
    sid: String,
}

#[derive(Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Retirement {
    schema_version: u32,
    review_digest: String,
    lease: Lease,
    quarantine: Option<serde_json::Value>,
}

fn optional<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
        Ok(_) => serde_json::from_slice(&bounded(path, 16384)?)
            .map(Some)
            .map_err(|_| "原检查权限回收记录无效".into()),
    }
}

pub(crate) fn retire_check_access(
    data: &Path,
    store: &DurableWorkflow,
    review: &RecheckDispositionReview,
) -> Result<(), String> {
    require(
        store.history().iter().any(|entry| {
            entry.body.event.principal == "user"
                && entry.body.context.recheck_disposition.as_deref() == Some(review)
        }) && store
            .retired_recheck_operations()?
            .contains(&review.operation_id),
        "检查权限回收需要原用户处置记录",
    )?;
    let (launch, _) = crate::async_verification_recheck::disposition_process_basis(
        data,
        store,
        &review.operation_id,
    )?;
    require(launch == review.launch, "原检查启动身份已变化")?;
    let Some(job) = launch else { return Ok(()) };
    let evidence = data
        .join("workflows")
        .join(&job.project_id)
        .join("verification")
        .join(&job.operation_id);
    // A launch can fail before the verifier creates its evidence directory.
    if !evidence.exists() {
        return Ok(());
    }
    root_path(evidence.to_str().ok_or("原检查路径无效")?)?;
    let review_digest = digest(review)?;
    if let Some(done) = optional::<String>(&evidence.join("check-access-retired.json"))? {
        return require(done == review_digest, "原检查权限回收完成记录冲突");
    }
    let basis = crate::verification_detail::basis(store, &job)?;
    let Some(binding) = &basis.plan.task(&job.task_id)?.worker_directory else {
        return Ok(());
    };
    let folder = directory(data, &job.project_id, &binding.identity)?;
    let lease_path = folder.join("check-active-lease.json");
    let quarantine_path = folder.join("check-quarantine.json");
    let current_lease = optional::<Lease>(&lease_path)?;
    let quarantine = optional::<serde_json::Value>(&quarantine_path)?;
    let pending = optional::<Retirement>(&evidence.join("check-access-retirement.json"))?;
    if current_lease.is_none() && pending.is_none() {
        return require(quarantine.is_none(), "原检查权限隔离标记缺少租约证据");
    }
    let account = validated_binding(data, &basis.plan.project, binding)?;
    let record = match pending {
        Some(record) => {
            require(
                record.schema_version == 1 && record.review_digest == review_digest,
                "原检查权限回收意图不匹配",
            )?;
            require(
                current_lease
                    .as_ref()
                    .is_none_or(|lease| *lease == record.lease)
                    && quarantine
                        .as_ref()
                        .is_none_or(|value| Some(value) == record.quarantine.as_ref()),
                "检查权限租约已变化，保留现场核对",
            )?;
            record
        }
        None => Retirement {
            schema_version: 1,
            review_digest: review_digest.clone(),
            lease: current_lease.ok_or("原检查权限租约缺失")?,
            quarantine,
        },
    };
    require(
        record.lease.schema == 1
            && record.lease.sid == account.record.sid
            && record.lease.paths.len() == 2,
        "原检查权限租约身份不符",
    )?;
    require(
        record.quarantine.as_ref().is_none_or(|value| {
            *value
                == serde_json::json!({
                    "reason":"check-workspace-permission-cleanup-failed"
                })
        }),
        "原检查权限隔离原因不符",
    )?;
    let expected_work = root_path(evidence.join("work").to_str().ok_or("检查工作目录无效")?)?;
    let paths = record
        .lease
        .paths
        .iter()
        .map(|path| root_path(path.to_str().ok_or("检查目录无效")?))
        .collect::<Result<Vec<_>, String>>()?;
    require(paths[0] == expected_work, "原检查工作目录与租约不符")?;
    let expected_stage = basis
        .approved_checks(&job.task_id)?
        .iter()
        .map(|check| evidence.join(format!("{}-temp", check.id)))
        .find(|path| *path == record.lease.paths[1])
        .ok_or("检查权限租约不属于原批准阶段")?;
    require(
        paths[1] == root_path(expected_stage.to_str().ok_or("检查阶段路径无效")?)?,
        "原检查阶段目录与租约不符",
    )?;
    let isolation: serde_json::Value =
        optional(&evidence.join("check-isolation.json"))?.ok_or("原检查隔离证据缺失")?;
    require(
        isolation
            == serde_json::json!({
                "schemaVersion":1,"operationId":job.operation_id,
                "revisionDigest":basis.plan.approval_digest()?,"attemptId":job.attempt_id,
                "sid":account.record.sid,"ownerSid":account.record.owner_sid,
                "identity":account.record.identity,"mode":"windows-account-job-v1"
            }),
        "原检查隔离证据与权限租约不符",
    )?;
    // Commit recovery evidence before changing ACLs or deleting any marker.
    crate::independent_checks::persist(&evidence, "check-access-retirement", &record)?;
    for path in &paths {
        revoke_check_workspace(path, &account)?;
    }
    // The lease remains until both revocations and quarantine cleanup succeed.
    // Any interruption can resume using the immutable recovery record.
    if quarantine_path.exists() {
        fs::remove_file(&quarantine_path).map_err(|e| e.to_string())?;
    }
    if lease_path.exists() {
        fs::remove_file(&lease_path).map_err(|e| e.to_string())?;
    }
    crate::independent_checks::persist(&evidence, "check-access-retired", &review_digest)
}
