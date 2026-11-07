//! Run the already-approved suites against the core's assembled repository,
//! only after blind review and chief authorization. This uses the existing
//! checker executable and a model-free, read-only Windows identity.
use crate::{
    verifier::{process, PinnedCheck},
    windows_worker_account as accounts,
};
use cerulean_domain::{execution::*, project_delivery::DeliveryManifest};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

pub(crate) struct Prepared {
    root: PathBuf,
    manifest: DeliveryManifest,
    operation: String,
    seconds: u32,
    account: accounts::Account,
    checks: Vec<AcceptanceCheck>,
}
impl Prepared {
    pub(crate) fn new(
        root: &Path,
        manifest: DeliveryManifest,
        operation: &str,
        seconds: u32,
    ) -> Result<Self, String> {
        manifest.validate()?;
        identifier(operation)?;
        crate::project_delivery::require_source(root, &manifest)?;
        let account = crate::delivery_checker_account::load(root, &manifest.project_id)
            .map_err(|_| "请先准备整体功能测试的独立检查账户")?;
        let checks = manifest
            .inputs
            .iter()
            .flat_map(|i| i.checks.clone())
            .collect();
        Ok(Self {
            root: root.into(),
            manifest,
            operation: operation.into(),
            seconds,
            account,
            checks,
        })
    }
    pub(crate) fn checks(&self) -> &[AcceptanceCheck] {
        &self.checks
    }
    pub(crate) fn require_pass(&self, report: &VerificationReport) -> Result<(), String> {
        require(
            report.manifest_digest == digest(&self.manifest)?
                && report.source_snapshot.as_ref().is_some_and(|s| {
                    s.operation_id == self.operation && s.manifest_sha256 == report.manifest_digest
                })
                && report.checks.len() == self.checks.len(),
            "整体功能报告关联或数量不符",
        )?;
        for (index, (observed, check)) in report.checks.iter().zip(&self.checks).enumerate() {
            require(
                observed.check_id == format!("delivery-{}", index + 1)
                    && observed.command_digest == digest(check)?
                    && observed.exit_code == Some(0)
                    && !observed.timed_out
                    && observed.assertion_error.is_none(),
                "整体功能检查未通过",
            )?;
            require_assertions(check, &observed.assertions, true)?;
        }
        Ok(())
    }
    pub(crate) fn run(
        &self,
        pins: &[PinnedCheck],
        cancel: &AtomicBool,
    ) -> Result<VerificationReport, String> {
        use sha2::{Digest, Sha256};
        require(pins.len() == self.checks.len(), "整体功能工具绑定缺失")?;
        let source = crate::project_delivery::require_source(&self.root, &self.manifest)?;
        let locked = pins
            .iter()
            .zip(&self.checks)
            .map(|(p, c)| p.lock(c))
            .collect::<Result<Vec<_>, _>>()?;
        // Keep each source handle read-shared only throughout all suites.
        let source_locks = self
            .manifest
            .files
            .iter()
            .map(|file| {
                crate::backup::open(&source.join(&file.artifact.path), false, false)
                    .map_err(|e| e.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let evidence = self
            .root
            .join("workflows")
            .join(&self.manifest.project_id)
            .join("functional-verification");
        fs::create_dir_all(&evidence).map_err(|e| e.to_string())?;
        crate::host::ordinary(&evidence, true)?;
        let folder = evidence.join(&self.operation);
        fs::create_dir(&folder).map_err(|_| "原整体功能测试已启动或目录不可用，不重复执行")?;
        let save_bytes = |name: &str, bytes: &[u8]| -> Result<(), String> {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(folder.join(name))
                .map_err(|e| e.to_string())?;
            file.write_all(bytes)
                .and_then(|_| file.sync_all())
                .map_err(|e| e.to_string())
        };
        let save = |name: &str, value: serde_json::Value| {
            save_bytes(
                name,
                &serde_json::to_vec(&value).map_err(|e| e.to_string())?,
            )
        };
        save(
            "intent.json",
            serde_json::json!({"schemaVersion":1,"operationId":self.operation,
            "delivery":self.manifest,"checks":self.checks,"pins":pins,"sid":self.account.record.sid}),
        )?;
        let result = (|| {
            let delivery_digest = digest(&self.manifest)?;
            let mut report = VerificationReport {
                schema_version: 1,
                verifier_id: "core".into(),
                observed_at: chrono::Utc::now().to_rfc3339(),
                manifest_digest: delivery_digest.clone(),
                checks: vec![],
                source_snapshot: Some(SourceSnapshot {
                    operation_id: self.operation.clone(),
                    manifest_sha256: delivery_digest,
                }),
            };
            let deadline = Instant::now() + Duration::from_secs(self.seconds.into());
            for (index, check) in self.checks.iter().enumerate() {
                require(!cancel.load(Ordering::Acquire), "整体功能测试已取消")?;
                let remaining = deadline.saturating_duration_since(Instant::now());
                require(!remaining.is_zero(), "整体功能测试期限已到")?;
                let temporary = folder.join(format!("stage-{}", index + 1));
                fs::create_dir(&temporary).map_err(|e| e.to_string())?;
                let tool = check.tool.as_ref().ok_or("批准工具缺失")?;
                let outcome = accounts::with_readonly_check_workspaces(
                    &source,
                    &temporary,
                    &self.account,
                    || {
                        accounts::with_identity(&self.account, || {
                            for file in std::iter::once(&tool.executable).chain(&tool.inputs) {
                                fs::File::open(&file.path)
                                    .map_err(|_| "独立检查账户无法读取批准工具".to_string())?;
                            }
                            Ok(())
                        })?;
                        process::run_isolated(
                            Path::new(&tool.executable.path),
                            &check.args,
                            &source,
                            &temporary,
                            remaining.min(Duration::from_secs(check.timeout_seconds.into())),
                            cancel,
                            &tool.environment,
                            None,
                            &self.account,
                        )
                    },
                )?;
                let stage = format!("delivery-{}", index + 1);
                save_bytes(&format!("{stage}.stdout"), &outcome.stdout)?;
                save_bytes(&format!("{stage}.stderr"), &outcome.stderr)?;
                let (assertions, assertion_error) = match parse_assertions(check, &outcome.stdout) {
                    Ok(results) => (results, None),
                    Err(error) => (vec![], Some(error)),
                };
                let observation = CheckObservation {
                    check_id: stage.clone(),
                    command_digest: digest(check)?,
                    exit_code: Some(outcome.exit_code),
                    timed_out: outcome.timed_out,
                    assertions,
                    assertion_error,
                    stdout_sha256: format!("{:x}", Sha256::digest(&outcome.stdout)),
                    stderr_sha256: format!("{:x}", Sha256::digest(&outcome.stderr)),
                };
                save(
                    &format!("{stage}.json"),
                    serde_json::to_value(&observation).map_err(|e| e.to_string())?,
                )?;
                if let Some(error) = outcome.failure {
                    return Err(error);
                }
                report.checks.push(observation);
                crate::project_delivery::require_source(&self.root, &self.manifest)?;
                if outcome.timed_out {
                    break;
                }
            }
            report.observed_at = chrono::Utc::now().to_rfc3339();
            Ok(report)
        })();
        save(
            "result.json",
            serde_json::json!({"schemaVersion":1,"operationId":self.operation,
            "report":result.as_ref().ok(),"error":result.as_ref().err()}),
        )?;
        drop(source_locks);
        drop(locked);
        result
    }
}
