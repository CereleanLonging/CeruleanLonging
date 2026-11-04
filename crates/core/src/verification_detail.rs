//! Bounded, read-only views of one original verification. Never drives polling.
use crate::{host::ordinary, scheduler::DurableWorkflow};
use cerulean_domain::{execution::*, verification_detail::*, workflow::WorkflowCommand};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path};
#[path = "verification_recheck_detail.rs"]
mod recheck;

#[derive(Clone)]
pub(crate) struct Basis {
    pub(crate) plan: PlanRevision,
    pub(crate) manifest: ArtifactManifest,
    recorded: Option<(VerificationReport, bool)>,
    recheck: Option<recheck::RecheckDetail>,
}
impl Basis {
    pub(crate) fn approved_checks(&self, task: &str) -> Result<&[AcceptanceCheck], String> {
        Ok(match &self.recheck {
            Some(recheck) => &recheck.approval.review.replacement_checks,
            None => &self.plan.task(task)?.checks,
        })
    }
}
#[cfg(windows)]
pub(crate) fn recheck_authority(
    basis: &Basis,
) -> Option<(
    cerulean_domain::verification_recheck::RecheckApproval,
    EventAnchor,
)> {
    basis
        .recheck
        .as_ref()
        .map(|r| (r.approval.clone(), r.event.clone()))
}
pub(crate) fn basis(store: &DurableWorkflow, job: &VerificationJob) -> Result<Basis, String> {
    basis_from_history(store.history(), job)
}
pub(crate) fn basis_from_history(
    history: &[crate::scheduler::Entry],
    job: &VerificationJob,
) -> Result<Basis, String> {
    let manifest = history
        .iter()
        .rev()
        .find_map(|entry| match &entry.body.context.execution {
            Some(ExecutionEvidence::Receipt {
                receipt,
                manifest: Some(manifest),
            }) if receipt.task_id == job.task_id
                && receipt.attempt_id == job.attempt_id
                && receipt.status == ReceiptStatus::Completed =>
            {
                Some(manifest)
            }
            _ => None,
        })
        .ok_or("原验收尝试缺少提交产物证据")?;
    let plan = history
        .iter()
        .find_map(|entry| match &entry.body.context.execution {
            Some(ExecutionEvidence::Revision { revision })
                if revision.approval_digest().ok().as_ref() == Some(&manifest.revision_digest) =>
            {
                Some(revision.clone())
            }
            _ => None,
        })
        .ok_or("原验收方案证据不存在")?;
    require(
        plan.project.id == job.project_id
            && manifest.task_id == job.task_id
            && manifest.attempt_id == job.attempt_id,
        "验收详情关联不匹配",
    )?;
    let recheck = recheck::RecheckDetail::from_history(history, job, &plan, manifest)?;
    let recorded = if let Some(recheck) = &recheck {
        recheck.recorded.clone()
    } else {
        history
            .iter()
            .find(|entry| entry.body.operation_id == job.operation_id)
            .map(
                |entry| match (&entry.body.event.command, &entry.body.context.execution) {
                    (
                        WorkflowCommand::Verify {
                            task,
                            attempt,
                            passed,
                            ..
                        },
                        Some(ExecutionEvidence::Verification { report }),
                    ) if task == &job.task_id && attempt == &job.attempt_id => {
                        Ok((report.clone(), *passed))
                    }
                    _ => Err("验收操作与账本冲突".to_string()),
                },
            )
            .transpose()?
    };
    Ok(Basis {
        plan,
        manifest: manifest.clone(),
        recorded,
        recheck,
    })
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Record {
    schema_version: u32,
    operation_id: String,
    manifest_digest: String,
    pins: Vec<serde_json::Value>,
    report: Option<VerificationReport>,
    error: Option<String>,
    #[serde(default)]
    recheck: Option<crate::verification_recheck_evidence::RecheckBinding>,
}
fn bytes(path: &Path, limit: u64) -> Result<Option<Vec<u8>>, String> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
        Ok(_) => (),
    }
    ordinary(path, false)?;
    let mut bytes = vec![];
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    require(bytes.len() as u64 <= limit, "验收详情文件超过读取上限")?;
    Ok(Some(bytes))
}
fn parse<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    serde_json::from_slice(bytes).map_err(|_| "验收证据不完整或格式不符，请稍后重新读取".into())
}
fn file_digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn log_ref(dir: &Path, id: &str, ext: &str, expected: &str) -> Result<VerificationLogRef, String> {
    sha256(expected)?;
    let path = dir.join(format!("{id}.{ext}"));
    let content = bytes(&path, 1024 * 1024)?.ok_or("验收日志缺失")?;
    require(file_digest(&content) == expected, "验收日志摘要不匹配")?;
    Ok(VerificationLogRef {
        path: path.to_string_lossy().into(),
        bytes: content.len() as u32,
        sha256: expected.into(),
    })
}
pub(crate) fn read(
    root: &Path,
    job: &VerificationJob,
    basis: &Basis,
) -> Result<VerificationDetail, String> {
    Ok(inspect(root, job, basis)?.0)
}

/// Only the specific cancellation observation produced after process::run has
/// confirmed an empty Windows Job and joined both output readers can permit
/// preserving an unresolved recheck. Arbitrary errors or absent results cannot.
#[cfg(windows)]
pub(crate) fn preserved_recheck_cancellation(
    root: &Path,
    job: &VerificationJob,
    basis: &Basis,
) -> Result<String, String> {
    require(
        basis.recheck.is_some() && job.state == VerificationState::Unknown,
        "仅原复验取消可核对保存关闭",
    )?;
    let inspection = inspect_evidence(root, job, basis)?;
    let detail = inspection.detail;
    require(
        inspection.report.is_none() && !detail.ledger_recorded && detail.snapshot_digest.is_some(),
        "复验取消证据与原结果不符",
    )?;
    let last = detail
        .stages
        .iter()
        .rposition(|s| s.exit_code.is_some())
        .ok_or("复验取消缺少进程结束观察")?;
    require(
        inspection.terminal_error.as_deref()
            == Some(format!("check {}: cancelled", detail.stages[last].id).as_str())
            && !detail.stages[last].timed_out
            && detail.stages[..=last]
                .iter()
                .all(|s| s.exit_code.is_some() && s.stdout.is_some() && s.stderr.is_some())
            && detail.stages[last + 1..]
                .iter()
                .all(|s| s.exit_code.is_none() && s.stdout.is_none() && s.stderr.is_none()),
        "仅确认进程树已结束的原取消可保存关闭；其他失败须保留核对",
    )?;
    Ok(detail.evidence_digest)
}
pub(crate) fn inspect(
    root: &Path,
    job: &VerificationJob,
    basis: &Basis,
) -> Result<(VerificationDetail, Option<VerificationReport>), String> {
    let inspection = inspect_evidence(root, job, basis)?;
    Ok((inspection.detail, inspection.report))
}
struct Inspection {
    detail: VerificationDetail,
    report: Option<VerificationReport>,
    #[cfg(windows)]
    terminal_error: Option<String>,
}
fn inspect_evidence(
    root: &Path,
    job: &VerificationJob,
    basis: &Basis,
) -> Result<Inspection, String> {
    for id in [
        &job.project_id,
        &job.operation_id,
        &job.task_id,
        &job.attempt_id,
    ] {
        identifier(id)?;
    }
    let specs = if let Some(recheck) = &basis.recheck {
        crate::verification_recheck_guard::require_protected(root, true)?;
        &recheck.approval.review.replacement_checks
    } else {
        &basis.plan.task(&job.task_id)?.checks
    };
    let expected_recheck = basis.recheck.as_ref().map(|r| r.binding()).transpose()?;
    require(specs.len() <= 64, "验收详情超过64项，请使用后续分页功能")?;
    let manifest_digest = digest(&basis.manifest)?;
    let mut dir = root.to_path_buf();
    ordinary(&dir, true)?;
    let mut exists = true;
    for part in [
        "workflows",
        &job.project_id,
        "verification",
        &job.operation_id,
    ] {
        dir.push(part);
        if exists {
            match fs::symlink_metadata(&dir) {
                Ok(_) => ordinary(&dir, true)?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => exists = false,
                Err(e) => return Err(e.to_string()),
            }
        }
    }
    let intent_bytes = if exists {
        bytes(&dir.join("intent.json"), 1024 * 1024)?
    } else {
        None
    };
    let terminal_bytes = if exists {
        bytes(&dir.join("result.json"), 1024 * 1024)?
    } else {
        None
    };
    let intent = intent_bytes.as_deref().map(parse::<Record>).transpose()?;
    let terminal = terminal_bytes.as_deref().map(parse::<Record>).transpose()?;
    let expected_pins = specs.iter().map(|check| {
        let tool = check.tool.as_ref().ok_or("原检查缺少工具绑定")?;
        let files: Vec<_> = std::iter::once(&tool.executable).chain(&tool.inputs)
            .map(|f| (&f.path, &f.sha256)).collect();
        Ok(serde_json::json!({"checkDigest":digest(check)?,"executable":tool.executable.path,"files":files,"environment":tool.environment}))
    }).collect::<Result<Vec<_>, String>>()?;
    for record in [&intent, &terminal].into_iter().flatten() {
        require(
            record.schema_version == 1
                && record.operation_id == job.operation_id
                && record.manifest_digest == manifest_digest
                && record.pins == expected_pins
                && record.recheck == expected_recheck,
            "验收记录与原批准检查不匹配",
        )?;
    }
    if let Some(intent) = &intent {
        require(
            intent.report.is_none() && intent.error.is_none(),
            "验收意图包含终态",
        )?;
    }
    if let Some(terminal) = &terminal {
        require(
            intent.is_some() && terminal.report.is_some() != terminal.error.is_some(),
            "验收终态缺少对应意图或结果",
        )?;
    }
    let report = terminal.as_ref().and_then(|r| r.report.as_ref());
    if let Some((recorded, passed)) = &basis.recorded {
        require(report == Some(recorded), "结果文件与验收账本不同")?;
        require(
            // An active/lost-ACK status can lag the authoritative Verify. Keep
            // that status visible; reading evidence must not reconcile it.
            matches!(
                job.state,
                VerificationState::Running
                    | VerificationState::Cancelling
                    | VerificationState::Unknown
            ) || *passed == (job.state == VerificationState::Succeeded),
            "验收状态与账本不同",
        )?;
    } else {
        require(
            job.state != VerificationState::Succeeded,
            "验收通过状态缺少账本证据",
        )?;
    }
    if let Some(report) = report {
        if let Some(recheck) = &basis.recheck {
            recheck.validate_report(report, &basis.plan, &basis.manifest)?;
        }
        require(
            report.schema_version == 1
                && report.manifest_digest == manifest_digest
                && report.verifier_id == "core"
                && report.checks.len() <= specs.len(),
            "验收报告关联不匹配",
        )?;
        timestamp(&report.observed_at)?;
        for (observation, spec) in report.checks.iter().zip(specs) {
            require(
                observation.check_id == spec.id && observation.command_digest == digest(spec)?,
                "验收报告检查顺序或命令不符",
            )?;
        }
    }
    let snapshot_bytes = if exists {
        bytes(&dir.join("snapshot.json"), 8 * 1024 * 1024)?
    } else {
        None
    };
    let snapshot_digest = if let Some(content) = &snapshot_bytes {
        let snapshot: serde_json::Value = parse(content)?;
        require(
            snapshot["schemaVersion"] == 1
                && snapshot["revisionDigest"] == basis.manifest.revision_digest
                && snapshot["taskId"] == job.task_id
                && snapshot["attemptId"] == job.attempt_id
                && snapshot["artifactManifestDigest"] == manifest_digest,
            "受测副本关联不匹配",
        )?;
        // The original snapshot is stored as compact serialized bytes.
        Some(file_digest(content))
    } else {
        None
    };
    if let Some(report) = report {
        let seal = report
            .source_snapshot
            .as_ref()
            .ok_or("验收报告缺少受测副本摘要")?;
        require(
            seal.operation_id == job.operation_id
                && snapshot_digest.as_ref() == Some(&seal.manifest_sha256),
            "受测副本摘要不匹配",
        )?;
    }
    let mut stages = Vec::new();
    let mut hashes = vec![];
    let mut gap = false;
    let timeout_prefix = report
        .filter(|r| r.checks.last().is_some_and(|c| c.timed_out))
        .map(|r| r.checks.len());
    for (index, spec) in specs.iter().enumerate() {
        let observation_bytes = if exists {
            bytes(&dir.join(format!("{}.json", spec.id)), 16384)?
        } else {
            None
        };
        let observation: Option<CheckObservation> =
            observation_bytes.as_deref().map(parse).transpose()?;
        if let Some(observation) = &observation {
            require(
                intent.is_some()
                    && !gap
                    && observation.check_id == spec.id
                    && observation.command_digest == digest(spec)?,
                "阶段观察缺少意图或顺序/命令不符",
            )?;
            require(
                timeout_prefix.is_none_or(|n| index < n),
                "超时后存在意外阶段结果",
            )?;
            if let Some(report) = report {
                require(
                    report.checks.get(index) == Some(observation),
                    "阶段文件与完整报告不匹配",
                )?;
            }
        } else {
            gap = true;
            require(
                report.is_none_or(|r| index >= r.checks.len()),
                "完整报告的阶段证据缺失",
            )?;
        }
        let stdout = observation
            .as_ref()
            .map(|o| log_ref(&dir, &spec.id, "stdout", &o.stdout_sha256))
            .transpose()?;
        let stderr = observation
            .as_ref()
            .map(|o| log_ref(&dir, &spec.id, "stderr", &o.stderr_sha256))
            .transpose()?;
        let state = match &observation {
            Some(o) if o.timed_out => VerificationStageState::TimedOut,
            Some(o) if o.assertion_error.is_some() || o.assertions.iter().any(|a| !a.passed) => {
                VerificationStageState::Failed
            }
            Some(o) if o.exit_code.is_some_and(|c| c != 0) => VerificationStageState::Failed,
            Some(o) if o.exit_code == Some(0) && basis.recorded.is_some() => {
                VerificationStageState::Passed
            }
            Some(_) => VerificationStageState::Observed,
            None if timeout_prefix.is_some_and(|n| index >= n) => VerificationStageState::NotRun,
            None if matches!(
                job.state,
                VerificationState::Running | VerificationState::Cancelling
            ) =>
            {
                VerificationStageState::Pending
            }
            None => VerificationStageState::Unknown,
        };
        hashes.push(observation_bytes.as_deref().map(file_digest));
        stages.push(VerificationStageView {
            id: spec.id.clone(),
            name: spec.name.clone().unwrap_or_else(|| spec.id.clone()),
            command_digest: digest(spec)?,
            timeout_seconds: spec.timeout_seconds,
            state,
            exit_code: observation.as_ref().and_then(|o| o.exit_code),
            timed_out: observation.as_ref().is_some_and(|o| o.timed_out),
            stdout,
            stderr,
        });
    }
    let evidence_digest = digest(&(
        job,
        &basis.manifest.revision_digest,
        &manifest_digest,
        intent_bytes.as_deref().map(file_digest),
        terminal_bytes.as_deref().map(file_digest),
        &snapshot_digest,
        &hashes,
    ))?;
    let validated_report = report.cloned();
    let terminal_error = terminal.and_then(|r| r.error);
    Ok(Inspection {
        detail: VerificationDetail {
            project_id: job.project_id.clone(),
            operation_id: job.operation_id.clone(),
            task_id: job.task_id.clone(),
            attempt_id: job.attempt_id.clone(),
            state: job.state.clone(),
            revision_digest: basis.manifest.revision_digest.clone(),
            manifest_digest,
            snapshot_digest,
            ledger_recorded: basis.recorded.is_some(),
            evidence_digest,
            observed_at: chrono::Utc::now().to_rfc3339(),
            message: terminal_error
                .as_ref()
                .map(|e| e.chars().take(2000).collect())
                .or_else(|| job.message.clone()),
            artifacts: basis.manifest.artifacts.clone(),
            stages,
            recovery: None,
            recovery_error: None,
        },
        report: validated_report,
        #[cfg(windows)]
        terminal_error,
    })
}
