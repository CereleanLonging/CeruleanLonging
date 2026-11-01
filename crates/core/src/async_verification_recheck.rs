//! Durable, single-use Host launch claims for corrected checks.
//! Opening a claim only reconstructs status; it never starts a checker.
use crate::{
    async_verification::{snapshot, ActiveVerification, Completion},
    host::ordinary,
    scheduler::{DurableWorkflow, Entry},
};
use cerulean_domain::{execution::*, workflow::WorkflowCommand};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{atomic::AtomicBool, mpsc, Arc},
};

const DIRECTORY: &str = "verification-recheck-jobs";
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Claim {
    schema_version: u32,
    approval: EventAnchor,
    initial: VerificationJob,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    process_scope: Option<String>,
}
fn validate_scope(root: &Path, claim: &Claim) -> Result<(), String> {
    match (claim.schema_version, claim.process_scope.as_deref()) {
        (1, None) => Ok(()), // Legacy launches never acquire invented OS proof.
        (2, Some(scope)) => require(
            scope
                == crate::verifier::process::named_job::scope(
                    &root
                        .join("workflows")
                        .join(&claim.initial.project_id)
                        .join("verification"),
                    &claim.approval,
                    &claim.initial.operation_id,
                )?,
            "复验进程身份与原批准、账户或数据位置不匹配",
        ),
        _ => Err("复验启动记录缺少对应版本的进程身份".into()),
    }
}
pub(crate) fn directory(root: &Path, project: &str, operation: &str) -> PathBuf {
    root.join(DIRECTORY).join(project).join(operation)
}
fn approval<'a>(store: &'a DurableWorkflow, operation: &str) -> Result<&'a Entry, String> {
    let mut found = store.history().iter().filter(|entry| {
        entry
            .body
            .context
            .verification_recheck
            .as_ref()
            .is_some_and(|a| a.operation_id == operation)
    });
    let entry = found.next().ok_or("复验启动记录缺少原用户批准")?;
    require(found.next().is_none(), "复验启动对应多个批准")?;
    Ok(entry)
}
fn initial(entry: &Entry) -> Result<VerificationJob, String> {
    let a = entry
        .body
        .context
        .verification_recheck
        .as_ref()
        .ok_or("复验批准缺失")?;
    Ok(VerificationJob {
        project_id: a.review.project_id.clone(),
        operation_id: a.operation_id.clone(),
        task_id: a.review.task_id.clone(),
        attempt_id: a.review.attempt_id.clone(),
        state: VerificationState::Running,
        started_at: entry.body.received_at.clone(),
        updated_at: entry.body.received_at.clone(),
        message: None,
        reconciliation_digest: None,
    })
}
fn same_identity(a: &VerificationJob, b: &VerificationJob) -> bool {
    a.project_id == b.project_id
        && a.operation_id == b.operation_id
        && a.task_id == b.task_id
        && a.attempt_id == b.attempt_id
        && a.started_at == b.started_at
}
fn read<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    ordinary(path, false)?;
    let mut bytes = vec![];
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(65537)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    require(bytes.len() <= 65536, "复验启动记录过大")?;
    serde_json::from_slice(&bytes).map_err(|_| "复验启动记录不完整，请保留原文件".into())
}
fn write_new<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    require(bytes.len() <= 65536, "复验启动记录过大")?;
    let mut f = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    f.write_all(&bytes)
        .and_then(|_| f.sync_all())
        .map_err(|e| e.to_string())
}
fn validate_claim(
    root: &Path,
    claim: &Claim,
    store: &DurableWorkflow,
    operation: &str,
) -> Result<(), String> {
    let entry = approval(store, operation)?;
    let expected = initial(entry)?;
    require(
        matches!(claim.schema_version, 1 | 2)
            && claim.approval
                == EventAnchor {
                    sequence: entry.body.event.sequence,
                    hash: entry.hash.clone(),
                }
            && claim.initial.project_id == expected.project_id
            && claim.initial.operation_id == expected.operation_id
            && claim.initial.task_id == expected.task_id
            && claim.initial.attempt_id == expected.attempt_id
            && claim.initial.state == VerificationState::Running
            && claim.initial.reconciliation_digest.is_none()
            && claim.initial.message.is_none(),
        "复验启动记录与原批准不匹配",
    )?;
    validate_scope(root, claim)?;
    timestamp(&claim.initial.started_at)?;
    require(
        claim.initial.updated_at == claim.initial.started_at
            && chrono::DateTime::parse_from_rfc3339(&claim.initial.started_at)
                .map_err(|e| e.to_string())?
                >= chrono::DateTime::parse_from_rfc3339(&expected.started_at)
                    .map_err(|e| e.to_string())?,
        "复验启动时间与原批准不匹配",
    )
}
pub(crate) fn claim(
    root: &Path,
    store: &DurableWorkflow,
    job: &VerificationJob,
) -> Result<(), String> {
    crate::verification_recheck_guard::require_protected(root, true)?;
    for id in [
        &job.project_id,
        &job.operation_id,
        &job.task_id,
        &job.attempt_id,
    ] {
        identifier(id)?;
    }
    let entry = approval(store, &job.operation_id)?;
    let claim = Claim {
        schema_version: 2,
        approval: EventAnchor {
            sequence: entry.body.event.sequence,
            hash: entry.hash.clone(),
        },
        initial: job.clone(),
        process_scope: Some(crate::verifier::process::named_job::scope(
            &root
                .join("workflows")
                .join(&job.project_id)
                .join("verification"),
            &EventAnchor {
                sequence: entry.body.event.sequence,
                hash: entry.hash.clone(),
            },
            &job.operation_id,
        )?),
    };
    validate_claim(root, &claim, store, &job.operation_id)?;
    for folder in [
        root.join(DIRECTORY),
        root.join(DIRECTORY).join(&job.project_id),
    ] {
        match fs::create_dir(&folder) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(e) => return Err(e.to_string()),
        }
        ordinary(&folder, true)?;
    }
    // The directory itself reserves this operation even if either write fails.
    let dir = directory(root, &job.project_id, &job.operation_id);
    fs::create_dir(&dir).map_err(|_| "复验启动编号已占用或目录不可用；不得重跑".to_string())?;
    ordinary(&dir, true)?;
    write_new(&dir.join("intent.json"), &claim)?;
    write_new(&dir.join("job.json"), job)
}
pub(crate) fn require_claim(
    root: &Path,
    store: &DurableWorkflow,
    job: &VerificationJob,
) -> Result<(), String> {
    crate::verification_recheck_guard::require_protected(root, true)?;
    ordinary(&root.join(DIRECTORY), true)?;
    ordinary(&root.join(DIRECTORY).join(&job.project_id), true)?;
    let dir = directory(root, &job.project_id, &job.operation_id);
    ordinary(&dir, true)?;
    let claim: Claim = read(&dir.join("intent.json"))?;
    validate_claim(root, &claim, store, &job.operation_id)?;
    require(
        same_identity(&claim.initial, job),
        "复验状态不得替换原启动身份",
    )
}
/// Caller must exclude every active owner before consulting this evidence.
/// The original DurableWorkflow remains exclusively locked during the query.
/// None is legacy/unsupported, not proof of termination or permission to retire.
pub(crate) fn process_exit_proof(
    root: &Path,
    store: &DurableWorkflow,
    job: &VerificationJob,
) -> Result<Option<String>, String> {
    require(!store.requires_reopen(), "原复验账本需要重开核对")?;
    require_claim(root, store, job)?;
    let claim: Claim =
        read(&directory(root, &job.project_id, &job.operation_id).join("intent.json"))?;
    validate_claim(root, &claim, store, &job.operation_id)?;
    let Some(scope) = &claim.process_scope else {
        return Ok(None);
    };
    let entry = approval(store, &job.operation_id)?;
    let approved = entry
        .body
        .context
        .verification_recheck
        .as_ref()
        .ok_or("原复验批准缺失")?;
    let mut observations = Vec::new();
    let evidence = root
        .join("workflows")
        .join(&job.project_id)
        .join("verification")
        .join(&job.operation_id);
    let exists = match fs::symlink_metadata(&evidence) {
        Ok(_) => {
            ordinary(&evidence, true)?;
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(e.to_string()),
    };
    for stage in &approved.review.replacement_checks {
        let name = crate::verifier::process::named_job::name(scope, &stage.id)?;
        let count = crate::verifier::process::named_job::observe(&name)?;
        require(
            count.is_none_or(|count| count == 0),
            "原复验检查器进程仍未退出",
        )?;
        let mut receipt_digest = None;
        if exists {
            let stage_dir = evidence.join(format!("{}-temp", stage.id));
            let receipt_path = evidence.join(format!("{}.process-exit.json", stage.id));
            match fs::symlink_metadata(&stage_dir) {
                Ok(_) => {
                    ordinary(&stage_dir, true)?;
                    // A disappeared named object is NOT a durable process exit
                    // receipt. Missing/partial completion stays unresolved.
                    let receipt: crate::verifier::process::named_job::ExitReceipt =
                        read(&receipt_path)?;
                    receipt.validate(&name, &claim.initial.started_at)?;
                    receipt_digest = Some(digest(&receipt)?);
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    require(
                        matches!(fs::symlink_metadata(&receipt_path), Err(e) if e.kind() == std::io::ErrorKind::NotFound),
                        "未启动阶段出现孤立进程回执",
                    )?;
                }
                Err(e) => return Err(e.to_string()),
            }
        }
        observations.push((name, count, receipt_digest));
    }
    Ok(Some(digest(&(
        "recheck-process-exit-v1",
        &claim,
        observations,
    ))?))
}

/// A separate disposition observation, not a replacement for result recovery.
/// Under the exclusive original ledger lock, v2 fixes every Global Job name
/// before any checker runs. Windows destroys a Job only after its processes
/// exit; exact absence/zero count can therefore prove no owned tree remains.
/// References: Microsoft Learn Job Objects / Managing Job Objects. Access or
/// namespace errors never become absence; old anonymous launches need old proof.
pub(crate) fn disposition_process_basis(
    root: &Path,
    store: &DurableWorkflow,
    operation: &str,
) -> Result<(Option<VerificationJob>, String), String> {
    require(!store.requires_reopen(), "原账本需要重开核对")?;
    let event = approval(store, operation)?;
    let expected = initial(event)?;
    let dir = directory(root, &expected.project_id, operation);
    // Inspect parents too: a missing leaf behind a reparse is not unstarted.
    for parent in [
        root.join(DIRECTORY),
        root.join(DIRECTORY).join(&expected.project_id),
    ] {
        match fs::symlink_metadata(&parent) {
            Ok(_) => ordinary(&parent, true)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.to_string()),
        }
    }
    match fs::symlink_metadata(&dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let evidence = root
                .join("workflows")
                .join(&expected.project_id)
                .join("verification");
            crate::artifacts::root_path(evidence.to_str().ok_or("invalid evidence path")?)?;
            require(
                matches!(fs::symlink_metadata(evidence.join(operation)), Err(e) if e.kind() == std::io::ErrorKind::NotFound),
                "无Host启动身份但存在检查器证据，请保留并核对",
            )?;
            return Ok((
                None,
                digest(&("recheck-not-launched-v1", &event.hash, operation))?,
            ));
        }
        Err(e) => return Err(e.to_string()),
        Ok(_) => ordinary(&dir, true)?,
    }
    let claim: Claim = read(&dir.join("intent.json"))?;
    validate_claim(root, &claim, store, operation)?;
    let Some(scope) = &claim.process_scope else {
        let basis = crate::verification_detail::basis(store, &claim.initial)?;
        let mut job = claim.initial.clone();
        job.state = VerificationState::Unknown;
        let proof = crate::verification_detail::preserved_recheck_cancellation(root, &job, &basis)?;
        return Ok((
            Some(claim.initial),
            digest(&("legacy-recheck-cancel-exit-v1", proof))?,
        ));
    };
    let a = event
        .body
        .context
        .verification_recheck
        .as_ref()
        .ok_or("原复验批准缺失")?;
    let mut observations = Vec::new();
    for stage in &a.review.replacement_checks {
        let name = crate::verifier::process::named_job::name(scope, &stage.id)?;
        let active = crate::verifier::process::named_job::observe(&name)?;
        require(active.is_none_or(|n| n == 0), "原复验检查器进程仍未退出")?;
        observations.push((name, active));
    }
    let proof = digest(&(
        "scoped-recheck-disposition-processes-v1",
        &claim,
        observations,
    ))?;
    Ok((Some(claim.initial), proof))
}
pub(crate) fn save(root: &Path, job: &VerificationJob) -> Result<(), String> {
    crate::verification_recheck_guard::require_protected(root, true)?;
    let dir = directory(root, &job.project_id, &job.operation_id);
    ordinary(&root.join(DIRECTORY), true)?;
    ordinary(&root.join(DIRECTORY).join(&job.project_id), true)?;
    ordinary(&dir, true)?;
    let claim: Claim = read(&dir.join("intent.json"))?;
    require(
        matches!(claim.schema_version, 1 | 2) && same_identity(&claim.initial, job),
        "复验状态不得替换原启动身份",
    )?;
    validate_scope(root, &claim)?;
    let dest = dir.join("job.json");
    if dest.exists() {
        ordinary(&dest, false)?;
    }
    let temp = dir.join(format!(
        "job-{}.tmp",
        chrono::Utc::now()
            .timestamp_nanos_opt()
            .ok_or("invalid clock")?
    ));
    write_new(&temp, job)?;
    fs::rename(temp, dest).map_err(|e| e.to_string())
}
pub(crate) fn load(
    root: &Path,
    project: &str,
    store: &DurableWorkflow,
) -> Result<Vec<VerificationJob>, String> {
    let base = root.join(DIRECTORY).join(project);
    if !base.exists() {
        return Ok(vec![]);
    }
    crate::verification_recheck_guard::require_protected(root, true)?;
    ordinary(&root.join(DIRECTORY), true)?;
    ordinary(&base, true)?;
    let mut jobs = vec![];
    for entry in fs::read_dir(&base).map_err(|e| e.to_string())? {
        require(jobs.len() < 1000, "复验记录达到上限")?;
        let entry = entry.map_err(|e| e.to_string())?;
        let dir = entry.path();
        ordinary(&dir, true)?;
        let operation = entry
            .file_name()
            .into_string()
            .map_err(|_| "invalid recheck operation")?;
        identifier(&operation)?;
        let event = approval(store, &operation)?;
        let fallback = initial(event)?;
        require(fallback.project_id == project, "复验启动项目不匹配")?;
        let parsed_claim = (|| {
            let claim: Claim = read(&dir.join("intent.json"))?;
            validate_claim(root, &claim, store, &operation)?;
            Ok::<_, String>(claim)
        })();
        let valid_claim = parsed_claim.is_ok();
        let (mut job, damage) = match parsed_claim {
            Ok(claim) => {
                let parsed_job = (|| {
                    let job: VerificationJob = read(&dir.join("job.json"))?;
                    require(same_identity(&claim.initial, &job), "复验状态身份已变化")?;
                    timestamp(&job.updated_at)?;
                    Ok::<_, String>(job)
                })();
                match parsed_job {
                    Ok(job) => (job, None),
                    // Retain the actual launch identity when only its mutable
                    // projection is damaged. Opening never repairs either file.
                    Err(e) => (claim.initial, Some(e)),
                }
            }
            Err(e) => (fallback, Some(e)),
        };
        job.reconciliation_digest = None;
        if valid_claim {
            if let Some(disposition) = store.recheck_disposition(event) {
                let review = disposition
                    .body
                    .context
                    .recheck_disposition
                    .as_ref()
                    .ok_or("复验处置审阅缺失")?;
                let a = event
                    .body
                    .context
                    .verification_recheck
                    .as_ref()
                    .ok_or("原批准缺失")?;
                if crate::recheck_disposition_evidence::fingerprint(root, a)?
                    == review.evidence_digest
                {
                    job.state = VerificationState::Abandoned;
                    job.updated_at = disposition.body.received_at.clone();
                    job.message = Some(review.reason.clone());
                    jobs.push(job);
                    continue;
                }
                job.state = VerificationState::Unknown;
                job.message = Some("复验处置后的原证据已变化，请保留并核对".into());
                jobs.push(job);
                continue;
            }
        }
        if damage.is_none() {
            if let Some(proof) = store
                .history()
                .iter()
                .find(|e| e.body.operation_id == operation)
            {
                let Some(ExecutionEvidence::VerificationRecheck { approval, .. }) =
                    &proof.body.context.execution
                else {
                    return Err("复验操作与原账本冲突".into());
                };
                require(
                    approval.sequence == event.body.event.sequence && approval.hash == event.hash,
                    "复验结果批准不匹配",
                )?;
                match &proof.body.event.command {
                    WorkflowCommand::Verify {
                        task,
                        attempt,
                        passed,
                        ..
                    } if task == &job.task_id && attempt == &job.attempt_id => {
                        job.state = if *passed {
                            VerificationState::Succeeded
                        } else {
                            VerificationState::Failed
                        };
                        job.message = None;
                    }
                    _ => return Err("复验结果身份不匹配".into()),
                }
                jobs.push(job);
                continue;
            }
        }
        job.state = VerificationState::Unknown;
        job.message =
            Some(damage.unwrap_or_else(|| {
                "复验启动已保留，尚无账本结果；仅核对原记录，不自动重跑".into()
            }));
        jobs.push(job);
        require(jobs.len() <= 1000, "复验记录达到上限")?;
    }
    Ok(jobs)
}
pub(crate) fn launch(
    root: &Path,
    mut store: DurableWorkflow,
    job: VerificationJob,
) -> Result<ActiveVerification, String> {
    let snap = snapshot(&store);
    let derived_diagnostics = Some(store.derived_diagnostics());
    let detail_basis = crate::verification_detail::basis(&store, &job)?;
    let config = store.configuration().clone();
    let path = root.join("workflows").join(&job.project_id);
    let cancel = Arc::new(AtomicBool::new(false));
    let token = cancel.clone();
    let operation = job.operation_id.clone();
    let (sender, receiver) = mpsc::channel();
    std::thread::Builder::new()
        .name(format!("recheck-{operation}"))
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                store.run_verification_recheck(&operation, &token)
            }));
            let completion = match result {
                Ok(outcome) => {
                    let uncertain = store.requires_reopen() || outcome.is_err();
                    let store = if store.requires_reopen() {
                        drop(store);
                        DurableWorkflow::open(&path, config)
                    } else {
                        Ok(store)
                    };
                    Completion {
                        store: Some(store),
                        uncertain,
                        outcome: outcome.map(|e| {
                            matches!(
                                e.body.event.command,
                                WorkflowCommand::Verify { passed: true, .. }
                            )
                        }),
                    }
                }
                Err(_) => {
                    drop(store);
                    Completion {
                        store: Some(DurableWorkflow::open(&path, config)),
                        uncertain: true,
                        outcome: Err("recheck worker panicked; inspect original evidence".into()),
                    }
                }
            };
            let _ = sender.send(completion);
        })
        .map_err(|e| format!("recheck thread launch failed: {e}"))?;
    Ok(ActiveVerification {
        derived_diagnostics,
        shared: false,
        snapshot: snap,
        detail_basis,
        job,
        cancel,
        receiver,
    })
}
