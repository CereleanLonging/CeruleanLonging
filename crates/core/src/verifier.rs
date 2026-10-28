//! Trusted host-only independent checks; no arbitrary command IPC endpoint.
pub(crate) mod process;
use crate::artifacts::{root_path, ArtifactObserver};
use cerulean_domain::execution::*;
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
    identifier(operation_id)?;
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
    require(
        !crate::artifacts::contains_path(&root, &evidence),
        "evidence must be outside worker project",
    )?;
    let specs = &input.plan.task(&input.attempt.task_id)?.checks;
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
        for (spec, pin) in specs.iter().zip(pins) {
            observer.require_unchanged(input.manifest)?;
            snapshot.require_unchanged()?;
            let temp = dir.join(format!("{}-temp", spec.id));
            fs::create_dir(&temp).map_err(|e| e.to_string())?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            require(!remaining.is_zero(), "verification overall time limit")?;
            let outcome = process::run(
                &pin.executable,
                &spec.args,
                &snapshot.work,
                &temp,
                remaining.min(Duration::from_secs(u64::from(spec.timeout_seconds))),
                cancel,
                &pin.environment,
            )?;
            save(&dir.join(format!("{}.stdout", spec.id)), &outcome.stdout)?;
            save(&dir.join(format!("{}.stderr", spec.id)), &outcome.stderr)?;
            let observation = CheckObservation {
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
