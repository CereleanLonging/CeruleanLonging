//! Deterministic inspection of approved check evidence. No filesystem or model
//! access. Provenance and current source identity are established by the host.
use crate::execution::{
    digest, identifier, require, sha256, timestamp, AcceptanceCheck, ArtifactManifest,
    VerificationReport,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckFact {
    pub id: String,
    pub command_digest: String,
    pub tool_digest: String,
    pub passed: bool,
    pub reason: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckRecord {
    pub schema_version: u32,
    pub project_id: String,
    pub revision: String,
    pub task_id: String,
    pub attempt_id: String,
    pub operation_id: String,
    pub source_digest: String,
    pub manifest_digest: String,
    pub report_digest: String,
    pub observed_at: String,
    pub checks: Vec<CheckFact>,
}
impl CheckRecord {
    pub fn validate(&self) -> Result<(), String> {
        require(self.schema_version == 1, "独立检查报告版本无效")?;
        for id in [
            &self.project_id,
            &self.task_id,
            &self.attempt_id,
            &self.operation_id,
        ] {
            identifier(id)?;
        }
        for hash in [
            &self.revision,
            &self.source_digest,
            &self.manifest_digest,
            &self.report_digest,
        ] {
            sha256(hash)?;
        }
        timestamp(&self.observed_at)?;
        require(
            !self.checks.is_empty() && self.checks.len() <= 100,
            "独立检查项缺失或超限",
        )?;
        let mut seen = BTreeSet::new();
        for fact in &self.checks {
            identifier(&fact.id)?;
            sha256(&fact.command_digest)?;
            sha256(&fact.tool_digest)?;
            require(
                seen.insert(&fact.id) && !fact.reason.is_empty() && fact.reason.len() <= 2048,
                "检查项重复或原因无效",
            )?;
        }
        Ok(())
    }
}

pub fn inspect(
    project: &str,
    operation: &str,
    manifest: &ArtifactManifest,
    approved: &[AcceptanceCheck],
    report: &VerificationReport,
) -> Result<CheckRecord, String> {
    let source = report
        .source_snapshot
        .as_ref()
        .ok_or("独立检查缺少源码封存")?;
    require(
        manifest.schema_version == 1
            && report.schema_version == 1
            && report.manifest_digest == digest(manifest)?
            && source.operation_id == operation,
        "独立检查报告与尝试或源码封存不符",
    )?;
    require(
        !approved.is_empty() && approved.len() <= 100 && report.checks.len() <= approved.len(),
        "独立检查配置或结果数量无效",
    )?;
    let mut seen = BTreeSet::new();
    for observed in &report.checks {
        require(
            seen.insert(&observed.check_id) && approved.iter().any(|s| s.id == observed.check_id),
            "检查报告含重复或未批准检查",
        )?;
        sha256(&observed.stdout_sha256)?;
        sha256(&observed.stderr_sha256)?;
    }
    let mut checks = vec![];
    for spec in approved {
        let tool = spec.tool.as_ref().ok_or("独立检查未绑定批准的工具摘要")?;
        tool.validate()?;
        let expected = digest(spec)?;
        let observed = report.checks.iter().find(|c| c.check_id == spec.id);
        let (passed, reason) = match observed {
            None => (false, "缺少检查结果"),
            Some(c) if c.command_digest != expected => return Err("检查命令与批准摘要不符".into()),
            Some(c) if c.timed_out => (false, "检查超时"),
            Some(c) if c.exit_code != Some(0) => (false, "检查未正常通过"),
            Some(c)
                if c.assertion_error.is_some()
                    || crate::execution::require_assertions(spec, &c.assertions, true).is_err() =>
            {
                (false, "断言失败或核验依据不足")
            }
            Some(_) => (true, "批准的检查及断言通过"),
        };
        checks.push(CheckFact {
            id: spec.id.clone(),
            command_digest: expected,
            tool_digest: digest(tool)?,
            passed,
            reason: reason.into(),
        });
    }
    let record = CheckRecord {
        schema_version: 1,
        project_id: project.into(),
        revision: manifest.revision_digest.clone(),
        task_id: manifest.task_id.clone(),
        attempt_id: manifest.attempt_id.clone(),
        operation_id: operation.into(),
        source_digest: source.manifest_sha256.clone(),
        manifest_digest: digest(manifest)?,
        report_digest: digest(report)?,
        observed_at: report.observed_at.clone(),
        checks,
    };
    record.validate()?;
    Ok(record)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CheckArchive {
    pub schema_version: u32,
    pub project_id: String,
    pub revision: String,
    pub final_task: String,
    pub final_source: String,
    pub records: Vec<CheckRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assembled: Option<crate::project_delivery::DeliveryManifest>,
}
impl CheckArchive {
    pub fn validate(&self) -> Result<(), String> {
        require(
            matches!(self.schema_version, 1 | 2)
                && !self.records.is_empty()
                && self.records.len() <= 100,
            "整体验收检查集缺失或超限",
        )?;
        identifier(&self.project_id)?;
        identifier(&self.final_task)?;
        sha256(&self.revision)?;
        sha256(&self.final_source)?;
        let mut seen = BTreeSet::new();
        for report in &self.records {
            report.validate()?;
            require(
                report.project_id == self.project_id
                    && report.revision == self.revision
                    && seen.insert(&report.task_id),
                "整体验收检查报告过期、跨项目或任务重复",
            )?;
        }
        if let Some(delivery) = &self.assembled {
            delivery.validate()?;
            require(
                self.schema_version == 2
                    && self.final_task == "core-assembled-delivery"
                    && self.project_id == delivery.project_id
                    && self.revision == delivery.revision
                    && self.final_source == digest(delivery)?
                    && self.records.len() == delivery.inputs.len(),
                "汇总交付与独立检查归档不符",
            )?;
            for input in &delivery.inputs {
                require(
                    self.records.iter().any(|record| {
                        record.task_id == input.manifest.task_id
                            && record.attempt_id == input.manifest.attempt_id
                            && record.operation_id == input.verification_operation
                            && record.source_digest == input.source.manifest_sha256
                            && record.manifest_digest == digest(&input.manifest).unwrap_or_default()
                            && record.report_digest == input.verification_report_digest
                            && record.checks.len() == input.checks.len()
                            && record
                                .checks
                                .iter()
                                .zip(&input.checks)
                                .all(|(fact, check)| {
                                    fact.passed
                                        && fact.id == check.id
                                        && digest(check)
                                            .is_ok_and(|hash| hash == fact.command_digest)
                                        && check.tool.as_ref().is_some_and(|tool| {
                                            digest(tool).is_ok_and(|hash| hash == fact.tool_digest)
                                        })
                                })
                    }),
                    "汇总交付含未合格、缺失或不匹配的执行端检查",
                )?;
            }
            return Ok(());
        }
        require(self.schema_version == 1, "汇总交付缺少封存清单")?;
        require(
            self.records
                .iter()
                .any(|r| r.task_id == self.final_task && r.source_digest == self.final_source),
            "最终集成源码没有匹配的完整检查报告",
        )
    }
    pub fn basis_points(&self) -> Result<u32, String> {
        self.validate()?;
        let total: usize = self.records.iter().map(|r| r.checks.len()).sum();
        let passed = self
            .records
            .iter()
            .flat_map(|r| &r.checks)
            .filter(|c| c.passed)
            .count();
        Ok((passed * 10000 / total) as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution::*;
    #[test]
    fn independent_checks_bind_tools_attempts_and_missing_results() {
        let spec = AcceptanceCheck {
            id: "test".into(),
            name: None,
            program: "checker.exe".into(),
            args: vec![],
            timeout_seconds: 10,
            expected_assertions: vec!["required".into()],
            tool: Some(ValidationTool {
                executable: ValidationFile {
                    path: std::env::current_dir()
                        .unwrap()
                        .join("checker.exe")
                        .to_string_lossy()
                        .into(),
                    sha256: "a".repeat(64),
                },
                inputs: vec![],
                environment: Default::default(),
            }),
        };
        let manifest = ArtifactManifest {
            schema_version: 1,
            revision_digest: "b".repeat(64),
            task_id: "task".into(),
            attempt_id: "attempt".into(),
            receipt_digest: "c".repeat(64),
            artifacts: vec![],
        };
        let mut report = VerificationReport {
            schema_version: 1,
            verifier_id: "core".into(),
            observed_at: "2026-11-05T01:00:00Z".into(),
            manifest_digest: digest(&manifest).unwrap(),
            checks: vec![],
            source_snapshot: Some(SourceSnapshot {
                operation_id: "check-1".into(),
                manifest_sha256: "d".repeat(64),
            }),
        };
        let check = |report: &VerificationReport| {
            inspect(
                "project",
                "check-1",
                &manifest,
                std::slice::from_ref(&spec),
                report,
            )
        };
        assert!(!check(&report).unwrap().checks[0].passed);
        report.checks.push(CheckObservation {
            assertion_error: None,
            assertions: vec![],
            check_id: "test".into(),
            command_digest: digest(&spec).unwrap(),
            exit_code: Some(0),
            timed_out: false,
            stdout_sha256: "e".repeat(64),
            stderr_sha256: "f".repeat(64),
        });
        assert!(!check(&report).unwrap().checks[0].passed);
        report.checks[0].assertions.push(AssertionResult {
            id: "required".into(),
            passed: true,
            detail: "fixed synthetic assertion observed".into(),
        });
        assert!(check(&report).unwrap().checks[0].passed);
        report.checks[0].timed_out = true;
        assert!(!check(&report).unwrap().checks[0].passed);
        report.checks[0].command_digest = "f".repeat(64);
        assert!(check(&report).is_err());
        report.manifest_digest = "a".repeat(64);
        assert!(check(&report).is_err());
    }
}
