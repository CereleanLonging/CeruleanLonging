//! Trusted host-only independent checks; no arbitrary command IPC endpoint.
pub(crate) mod process;
use crate::artifacts::{root_path, ArtifactObserver};
use crate::verification_recheck_evidence::RecheckBinding;
use cerulean_domain::execution::*;
use cerulean_domain::verification_recheck::{RecheckApproval, RecheckBasis};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

/// Chosen by a trusted host tool registry, never from a worker receipt.
/// Pins include the executable and any trusted validation scripts/configuration.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PinnedCheck {
    check_digest: String,
    executable: PathBuf,
    files: Vec<(PathBuf, String)>,
    environment: std::collections::BTreeMap<String, String>,
}
fn file_hash(path: &Path) -> Result<String, String> {
    require(path.is_absolute(), "validator path must be absolute")?;
    root_path(
        path.parent()
            .ok_or("validator parent missing")?
            .to_str()
            .ok_or("non-Unicode validator path")?,
    )?;
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
    require(
        meta.is_file() && meta.file_attributes() & 0x400 == 0,
        "validator must be an ordinary file",
    )?;
    require(meta.len() <= 256 * 1024 * 1024, "validator size limit")?;
    // Read-only sharing disallows a concurrent writer/delete while hashing.
    let mut f = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(path)
        .map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buf = [0; 65536];
    let mut size = 0;
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        size += n as u64;
        require(size <= 256 * 1024 * 1024, "validator size limit")?;
        hash.update(&buf[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
impl PinnedCheck {
    /// Read-only tool inspection. The resulting complete metadata must be placed
    /// in a proposal before user approval, never synthesized after approval.
    pub fn describe(
        executable: &Path,
        inputs: &[PathBuf],
        environment: std::collections::BTreeMap<String, String>,
    ) -> Result<ValidationTool, String> {
        fn inspect(path: &Path) -> Result<ValidationFile, String> {
            let hash = file_hash(path)?;
            let path = fs::canonicalize(path).map_err(|e| e.to_string())?;
            Ok(ValidationFile {
                path: path.to_str().ok_or("non-Unicode validator path")?.into(),
                sha256: hash,
            })
        }
        let tool = ValidationTool {
            executable: inspect(executable)?,
            inputs: inputs
                .iter()
                .map(|p| inspect(p))
                .collect::<Result<_, _>>()?,
            environment,
        };
        tool.validate()?;
        Ok(tool)
    }
    pub fn from_approved(check: &AcceptanceCheck) -> Result<Self, String> {
        let tool = check
            .tool
            .as_ref()
            .ok_or("approved validator binding required")?;
        tool.validate()?;
        Self::new(
            check,
            Path::new(&tool.executable.path),
            &tool
                .inputs
                .iter()
                .map(|p| PathBuf::from(&p.path))
                .collect::<Vec<_>>(),
        )
    }
    pub fn new(
        check: &AcceptanceCheck,
        executable: &Path,
        inputs: &[PathBuf],
    ) -> Result<Self, String> {
        let approved = check
            .tool
            .as_ref()
            .ok_or("approved validator binding required")?;
        approved.validate()?;
        let observed = Self::describe(executable, inputs, approved.environment.clone())?;
        require(
            observed == *approved,
            "validator differs from approved executable or inputs",
        )?;
        require(
            executable
                .extension()
                .is_some_and(|s| s.eq_ignore_ascii_case("exe")),
            "native exe required; no implicit shell or batch",
        )?;
        // Preserve the approved hashes. Rehashing here as the new baseline
        // could accept a replacement between inspection and construction.
        let executable = PathBuf::from(&approved.executable.path);
        let files = std::iter::once(&approved.executable)
            .chain(&approved.inputs)
            .map(|file| (PathBuf::from(&file.path), file.sha256.clone()))
            .collect();
        Ok(Self {
            check_digest: digest(check)?,
            executable,
            files,
            environment: approved.environment.clone(),
        })
    }
    pub(crate) fn lock(&self, check: &AcceptanceCheck) -> Result<Vec<File>, String> {
        use std::os::windows::fs::OpenOptionsExt;
        require(
            self.check_digest == digest(check)?,
            "pinned command changed",
        )?;
        let mut locked = vec![];
        for (path, expected) in &self.files {
            let file = OpenOptions::new()
                .read(true)
                .share_mode(1)
                .open(path)
                .map_err(|e| e.to_string())?;
            require(file_hash(path)? == *expected, "validator source changed")?;
            locked.push(file);
        }
        Ok(locked)
    }
}
pub struct VerificationInput<'a> {
    pub plan: &'a PlanRevision,
    pub approval: &'a Approval,
    pub authenticated_user: &'a str,
    pub policy_version: u32,
    pub attempt: &'a Attempt,
    pub receipt: &'a ExecutionReceipt,
    pub manifest: &'a ArtifactManifest,
    pub verifier_id: &'a str,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Record<'a> {
    schema_version: u32,
    operation_id: &'a str,
    manifest_digest: String,
    pins: &'a [PinnedCheck],
    report: Option<&'a VerificationReport>,
    error: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    recheck: Option<&'a RecheckBinding>,
}

pub(crate) struct RecheckAuthority<'a> {
    pub approval: &'a RecheckApproval,
    pub event: &'a EventAnchor,
    pub original_report: &'a VerificationReport,
    pub retired_operations: &'a [String],
}
fn save(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())
}
/// Evidence root must be host-owned, outside the project. A new operation
/// directory is claimed before execution; reopening the same ID never reruns it.
pub fn verify(
    input: VerificationInput<'_>,
    pins: &[PinnedCheck],
    evidence_root: &Path,
    operation_id: &str,
    cancel: &AtomicBool,
) -> Result<VerificationReport, String> {
    execute(input, pins, evidence_root, operation_id, cancel, None, &[])
}

/// Trusted scheduler only: every retired ID was revalidated against its user
/// ledger event and unchanged original evidence while preparing this run.
pub(crate) fn verify_with_retirements(
    input: VerificationInput<'_>,
    pins: &[PinnedCheck],
    evidence_root: &Path,
    operation_id: &str,
    cancel: &AtomicBool,
    retired: &[String],
) -> Result<VerificationReport, String> {
    execute(
        input,
        pins,
        evidence_root,
        operation_id,
        cancel,
        None,
        retired,
    )
}

/// A separate trusted entry: the original plan approval is still verified as-is.
/// Only the new user event authorizes replacement checks on its original source.
pub(crate) fn verify_recheck(
    input: VerificationInput<'_>,
    pins: &[PinnedCheck],
    evidence_root: &Path,
    authority: RecheckAuthority<'_>,
    cancel: &AtomicBool,
) -> Result<VerificationReport, String> {
    let operation = authority.approval.operation_id.as_str();
    execute(
        input,
        pins,
        evidence_root,
        operation,
        cancel,
        Some(&authority),
        authority.retired_operations,
    )
}

fn execute(
    input: VerificationInput<'_>,
    pins: &[PinnedCheck],
    evidence_root: &Path,
    operation_id: &str,
    cancel: &AtomicBool,
    recheck: Option<&RecheckAuthority<'_>>,
    retired: &[String],
) -> Result<VerificationReport, String> {
    identifier(operation_id)?;
    require(
        !retired.iter().any(|id| id == operation_id),
        "retired verification operation cannot be reused",
    )?;
    identifier(input.verifier_id)?;
    require(
        input.plan.snapshot_policy.is_some(),
        "approved snapshot policy required",
    )?;
    let observer = ArtifactObserver::new(
        input.plan,
        input.approval,
        input.authenticated_user,
        input.policy_version,
        input.attempt,
        input.receipt,
        input.receipt.turn_id.as_deref(),
    )?;
    observer.require_unchanged(input.manifest)?;
    let root = root_path(
        &input
            .plan
            .execution_root(&input.attempt.task_id, &input.attempt.id)?,
    )?;
    let evidence = root_path(evidence_root.to_str().ok_or("non-Unicode evidence root")?)?;
    if let Some(data) = evidence
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
    {
        crate::blind_review_guard::require_isolated_execution(
            data,
            input
                .plan
                .task(&input.attempt.task_id)?
                .worker_directory
                .is_some(),
        )?;
    }
    let isolated_account = if input
        .plan
        .task(&input.attempt.task_id)?
        .worker_directory
        .is_some()
    {
        let data = evidence
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .ok_or("检查数据根目录缺失")?;
        Some(crate::windows_worker_account::load(
            data,
            input.plan,
            &input.attempt.task_id,
        )?)
    } else {
        None
    };
    require(
        !crate::artifacts::contains_path(&root, &evidence),
        "evidence must be outside worker project",
    )?;
    let binding = if let Some(recheck) = recheck {
        let approval = recheck.approval;
        approval.validate(&RecheckBasis {
            plan: input.plan,
            manifest: input.manifest,
            original_job: &approval.original_job,
            original_report: recheck.original_report,
            ledger_head: &approval.review.ledger_head,
            trusted_verifier: input.verifier_id,
        })?;
        require(
            approval.operation_id == operation_id,
            "recheck operation mismatch",
        )?;
        crate::source_snapshot::require_sealed(
            input.plan,
            input.manifest,
            &approval.review.original_source_snapshot,
            &evidence,
        )?;
        Some(RecheckBinding::new(approval, recheck.event)?)
    } else {
        None
    };
    let specs = if let Some(recheck) = recheck {
        &recheck.approval.review.replacement_checks
    } else {
        &input.plan.task(&input.attempt.task_id)?.checks
    };
    require(pins.len() == specs.len(), "validator bindings missing")?;
    for pin in pins {
        for (path, _) in &pin.files {
            let canonical = fs::canonicalize(path).map_err(|e| e.to_string())?;
            require(
                !crate::artifacts::contains_path(&root, &canonical),
                "trusted validator source must be outside worker project",
            )?;
        }
    }
    // Hold all pinned validation sources read-only through execution/observation.
    let _locked: Vec<_> = pins
        .iter()
        .zip(specs)
        .map(|(p, c)| p.lock(c))
        .collect::<Result<_, _>>()?;
    if let Some(account) = &isolated_account {
        crate::windows_worker_account::with_identity(account, || {
            for pin in pins {
                for (path, _) in &pin.files {
                    File::open(path).map_err(|_|"隔离检查账户无法读取已批准的检查工具；请先准备可读取的固定工具，未降回宿主权限".to_string())?;
                }
            }
            Ok(())
        })?;
    }
    let dir = evidence.join(operation_id);
    // A crash may leave an intent without a terminal result. A new ID must not
    // bypass that uncertainty and repeat potentially effectful checks.
    for entry in fs::read_dir(&evidence).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let metadata = entry.file_type().map_err(|e| e.to_string())?;
        require(
            metadata.is_dir() && !metadata.is_symlink(),
            "unexpected verifier storage entry",
        )?;
        root_path(
            entry
                .path()
                .to_str()
                .ok_or("non-Unicode verifier storage")?,
        )?;
        if retired.iter().any(|id| entry.file_name() == id.as_str()) {
            continue;
        }
        if crate::verification_reconciliation::retired(&entry.path())? {
            continue;
        }
        let terminal = entry.path().join("result.json");
        let mut bytes = Vec::new();
        let file = File::open(&terminal)
            .map_err(|_| "unfinished verification requires reconciliation before another run")?;
        file.take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        require(bytes.len() <= 1024 * 1024, "verification result too large")?;
        let result: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|_| "incomplete verification result requires reconciliation")?;
        require(
            result["schemaVersion"] == 1
                && (result["report"].is_object() || result["error"].is_string()),
            "invalid verification result",
        )?;
    }
    fs::create_dir(&dir)
        .map_err(|e| format!("verification operation already exists or unavailable: {e}"))?;
    let record = |report, error| Record {
        schema_version: 1,
        operation_id,
        manifest_digest: digest(input.manifest).unwrap_or_default(),
        pins,
        report,
        error,
        recheck: binding.as_ref(),
    };
    save(
        &dir.join("intent.json"),
        &serde_json::to_vec_pretty(&record(None, None)).map_err(|e| e.to_string())?,
    )?;
    let result = (|| {
        let snapshot = crate::source_snapshot::SnapshotLease::create(
            input.plan,
            input.manifest,
            &dir,
            operation_id,
        )?;
        if let Some(recheck) = recheck {
            require(
                snapshot.seal.manifest_sha256
                    == recheck
                        .approval
                        .review
                        .original_source_snapshot
                        .manifest_sha256,
                "recheck input snapshot differs from original failed check",
            )?;
        }
        let mut report = VerificationReport {
            schema_version: 1,
            verifier_id: input.verifier_id.into(),
            observed_at: chrono::Utc::now().to_rfc3339(),
            manifest_digest: digest(input.manifest)?,
            checks: vec![],
            source_snapshot: Some(snapshot.seal.clone()),
        };
        let deadline =
            Instant::now() + Duration::from_secs(u64::from(input.plan.policy.max_seconds));
        if let Some(account) = &isolated_account {
            save(&dir.join("check-isolation.json"), &serde_json::to_vec(&serde_json::json!({
                "schemaVersion":1,"operationId":operation_id,"revisionDigest":input.plan.approval_digest()?,
                "attemptId":input.attempt.id,"sid":account.record.sid,"ownerSid":account.record.owner_sid,
                "identity":account.record.identity,"mode":"windows-account-job-v1"
            })).map_err(|e|e.to_string())?)?;
        }
        for (spec, pin) in specs.iter().zip(pins) {
            observer.require_unchanged(input.manifest)?;
            snapshot.require_unchanged()?;
            let temp = dir.join(format!("{}-temp", spec.id));
            fs::create_dir(&temp).map_err(|e| e.to_string())?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            require(!remaining.is_zero(), "verification overall time limit")?;
            let job_name = recheck
                .map(|authority| {
                    let scope =
                        process::named_job::scope(evidence_root, authority.event, operation_id)?;
                    process::named_job::name(&scope, &spec.id)
                })
                .transpose()?;
            let outcome = if let Some(account) = &isolated_account {
                crate::windows_worker_account::with_check_workspaces(
                    &[snapshot.work.as_path(), temp.as_path()],
                    account,
                    || {
                        process::run_isolated(
                            &pin.executable,
                            &spec.args,
                            &snapshot.work,
                            &temp,
                            remaining.min(Duration::from_secs(u64::from(spec.timeout_seconds))),
                            cancel,
                            &pin.environment,
                            job_name.as_deref(),
                            account,
                        )
                    },
                )?
            } else {
                process::run_scoped(
                    &pin.executable,
                    &spec.args,
                    &snapshot.work,
                    &temp,
                    remaining.min(Duration::from_secs(u64::from(spec.timeout_seconds))),
                    cancel,
                    &pin.environment,
                    job_name.as_deref(),
                )?
            };
            if let Some(name) = &job_name {
                let receipt = process::named_job::ExitReceipt::new(name, &outcome)?;
                save(
                    &dir.join(format!("{}.process-exit.json", spec.id)),
                    &serde_json::to_vec(&receipt).map_err(|e| e.to_string())?,
                )?;
            }
            save(&dir.join(format!("{}.stdout", spec.id)), &outcome.stdout)?;
            save(&dir.join(format!("{}.stderr", spec.id)), &outcome.stderr)?;
            let (assertions, assertion_error) =
                match cerulean_domain::execution::parse_assertions(spec, &outcome.stdout) {
                    Ok(results) => (results, None),
                    Err(error) => (vec![], Some(error)),
                };
            let observation = CheckObservation {
                assertions,
                assertion_error,
                check_id: spec.id.clone(),
                command_digest: digest(spec)?,
                exit_code: Some(outcome.exit_code),
                timed_out: outcome.timed_out,
                stdout_sha256: format!("{:x}", Sha256::digest(&outcome.stdout)),
                stderr_sha256: format!("{:x}", Sha256::digest(&outcome.stderr)),
            };
            save(
                &dir.join(format!("{}.json", spec.id)),
                &serde_json::to_vec_pretty(&observation).map_err(|e| e.to_string())?,
            )?;
            if let Some(failure) = outcome.failure {
                return Err(format!("check {}: {failure}", spec.id));
            }
            report.checks.push(observation);
            observer.require_unchanged(input.manifest)?;
            snapshot.require_unchanged()?;
            if outcome.timed_out {
                break;
            }
        }
        observer.require_unchanged(input.manifest)?;
        report.observed_at = chrono::Utc::now().to_rfc3339();
        Ok(report)
    })();
    let value = match &result {
        Ok(report) => record(Some(report), None),
        Err(error) => record(None, Some(error.as_str())),
    };
    save(
        &dir.join("result.json"),
        &serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?,
    )?;
    result
}
