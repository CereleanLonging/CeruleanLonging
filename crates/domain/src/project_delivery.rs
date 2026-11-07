//! Host-assembled delivery authority. Executors keep their original directories.
use crate::execution::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use ts_rs::TS;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeliveryInput {
    pub identity: String,
    pub acceptance: EventAnchor,
    pub manifest: ArtifactManifest,
    pub verification_operation: String,
    pub verification_report_digest: String,
    pub source: SourceSnapshot,
    pub source_files: Vec<Artifact>,
    pub source_directories: Vec<String>,
    pub checks: Vec<AcceptanceCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeliveryManifest {
    pub schema_version: u32,
    pub project_id: String,
    pub revision: String,
    pub setup: String,
    pub inputs: Vec<DeliveryInput>,
    pub files: Vec<DeliveryFile>,
    pub directories: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeliveryFile {
    pub source_task: String,
    pub artifact: Artifact,
}

impl DeliveryManifest {
    pub fn assemble(
        project_id: String,
        revision: String,
        setup: String,
        inputs: Vec<DeliveryInput>,
    ) -> Result<Self, String> {
        let mut result = Self {
            schema_version: 1,
            project_id,
            revision,
            setup,
            inputs,
            files: vec![],
            directories: vec![],
        };
        (result.files, result.directories) = result.compose()?;
        result.validate()?;
        Ok(result)
    }

    pub fn validate(&self) -> Result<(), String> {
        require(self.schema_version == 1, "整体交付清单版本无效")?;
        identifier(&self.project_id)?;
        sha256(&self.revision)?;
        sha256(&self.setup)?;
        let (files, directories) = self.compose()?;
        require(
            files == self.files && directories == self.directories,
            "整体交付文件与已接受来源不符",
        )?;
        require(
            serde_json::to_vec(self).map_err(|e| e.to_string())?.len() <= 512 * 1024,
            "整体交付权威记录超限",
        )
    }

    fn compose(&self) -> Result<(Vec<DeliveryFile>, Vec<String>), String> {
        require(
            !self.inputs.is_empty() && self.inputs.len() <= 100,
            "整体交付需要已接受的任务来源",
        )?;
        let mut tasks = BTreeSet::new();
        let mut sequences = BTreeSet::new();
        let mut owners: BTreeMap<String, &str> = BTreeMap::new();
        let mut latest: BTreeMap<&str, &DeliveryInput> = BTreeMap::new();
        for input in &self.inputs {
            let manifest = &input.manifest;
            identifier(&manifest.task_id)?;
            identifier(&manifest.attempt_id)?;
            identifier(&input.verification_operation)?;
            for hash in [
                &input.identity,
                &input.acceptance.hash,
                &manifest.receipt_digest,
                &input.verification_report_digest,
                &input.source.manifest_sha256,
            ] {
                sha256(hash)?;
            }
            require(
                manifest.schema_version == 1
                    && input.acceptance.sequence > 0
                    && manifest.revision_digest == self.revision
                    && input.source.operation_id == input.verification_operation
                    && tasks.insert(&manifest.task_id)
                    && sequences.insert(input.acceptance.sequence)
                    && !manifest.artifacts.is_empty()
                    && !input.checks.is_empty()
                    && input.checks.len() <= 64,
                "整体交付来源重复、过期或缺少批准检查",
            )?;
            input.validate_source()?;
            let mut own = BTreeSet::new();
            for artifact in &manifest.artifacts {
                artifact_path(&artifact.path)?;
                sha256(&artifact.sha256)?;
                let key = artifact.path.replace('\\', "/").to_uppercase();
                require(own.insert(key.clone()), "任务交付路径重复")?;
                for (path, owner) in &owners {
                    let overlap = path == &key
                        || path.starts_with(&format!("{key}/"))
                        || key.starts_with(&format!("{path}/"));
                    require(
                        !overlap || *owner == input.identity,
                        "整体交付存在跨执行模型的源码归属冲突",
                    )?;
                }
                owners.insert(key, &input.identity);
            }
            let previous = latest.entry(&input.identity).or_insert(input);
            if input.acceptance.sequence > previous.acceptance.sequence {
                *previous = input;
            }
        }
        require(latest.len() <= 5, "整体交付来源超过五个执行模型")?;
        let mut files = BTreeMap::new();
        let mut directories = BTreeMap::new();
        for input in latest.values() {
            for path in &input.source_directories {
                artifact_path(path)?;
                let key = path.to_uppercase();
                if let Some(previous) = directories.insert(key, path.clone()) {
                    require(previous == *path, "整体交付目录存在大小写别名")?;
                }
            }
            for artifact in &input.source_files {
                artifact_path(&artifact.path)?;
                sha256(&artifact.sha256)?;
                let key = artifact.path.replace('\\', "/").to_uppercase();
                require(
                    owners.get(&key) == Some(&input.identity.as_str()),
                    "封存源码含未声明归属的文件，请修订方案后重新检查",
                )?;
                require(
                    files
                        .insert(
                            key,
                            DeliveryFile {
                                source_task: input.manifest.task_id.clone(),
                                artifact: artifact.clone(),
                            },
                        )
                        .is_none(),
                    "整体交付文件重复",
                )?;
            }
        }
        require(files.keys().eq(owners.keys()), "最新封存缺少已批准交付文件")?;
        for path in files.keys() {
            require(!directories.contains_key(path), "整体交付文件与目录冲突")?;
            for other in files.keys() {
                require(
                    !other.starts_with(&format!("{path}/")),
                    "整体交付文件路径嵌套冲突",
                )?;
            }
            require(
                !directories
                    .keys()
                    .any(|other| other.starts_with(&format!("{path}/"))),
                "整体交付文件遮挡目录",
            )?;
        }
        for input in &self.inputs {
            for artifact in &input.manifest.artifacts {
                require(
                    files
                        .get(&artifact.path.to_uppercase())
                        .is_some_and(|f| f.artifact == *artifact),
                    "后续任务改变了已检查成果，请重新安排覆盖这些修改的检查",
                )?;
            }
        }
        require(
            files.len() <= 128
                && directories.len() <= 512
                && files
                    .values()
                    .map(|f| u64::from(f.artifact.bytes))
                    .sum::<u64>()
                    <= crate::blind_review::MAX_MATERIAL_BYTES as u64,
            "完整交付超过当前盲评容量，请拆分项目范围",
        )?;
        let mut files: Vec<_> = files.into_values().collect();
        files.sort_by(|a, b| a.artifact.path.cmp(&b.artifact.path));
        let mut directories: Vec<_> = directories.into_values().collect();
        directories.sort();
        Ok((files, directories))
    }
}

impl DeliveryInput {
    fn source_manifest_digest(&self) -> Result<String, String> {
        // Match the original snapshot serialization, including field order.
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Tree<'a> {
            directories: &'a [String],
            files: &'a [Artifact],
        }
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Snapshot<'a> {
            schema_version: u32,
            revision_digest: &'a str,
            task_id: &'a str,
            attempt_id: &'a str,
            artifact_manifest_digest: String,
            tree: Tree<'a>,
        }
        let source = Snapshot {
            schema_version: 1,
            revision_digest: &self.manifest.revision_digest,
            task_id: &self.manifest.task_id,
            attempt_id: &self.manifest.attempt_id,
            artifact_manifest_digest: digest(&self.manifest)?,
            tree: Tree {
                directories: &self.source_directories,
                files: &self.source_files,
            },
        };
        digest(&source)
    }
    fn validate_source(&self) -> Result<(), String> {
        require(
            self.source_manifest_digest()? == self.source.manifest_sha256,
            "汇总源码与原独立检查封存摘要不符",
        )?;
        require(
            self.source_files.len() <= 512 && self.source_directories.len() <= 512,
            "单个交付封存过大",
        )?;
        let mut paths = BTreeSet::new();
        for file in &self.source_files {
            artifact_path(&file.path)?;
            sha256(&file.sha256)?;
            require(
                paths.insert(file.path.to_uppercase()),
                "源码封存存在重复路径",
            )?;
        }
        for artifact in &self.manifest.artifacts {
            require(
                self.source_files.contains(artifact),
                "已检查成果缺少匹配封存",
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn input(task: &str, identity: char, path: &str, sequence: u32) -> DeliveryInput {
        let artifact = Artifact {
            path: path.into(),
            bytes: 3,
            sha256: "a".repeat(64),
        };
        let mut value = DeliveryInput {
            identity: identity.to_string().repeat(64),
            acceptance: EventAnchor {
                sequence,
                hash: "b".repeat(64),
            },
            manifest: ArtifactManifest {
                schema_version: 1,
                revision_digest: "c".repeat(64),
                task_id: task.into(),
                attempt_id: format!("attempt-{task}"),
                receipt_digest: "d".repeat(64),
                artifacts: vec![artifact.clone()],
            },
            verification_operation: format!("verify-{task}"),
            verification_report_digest: "e".repeat(64),
            source: SourceSnapshot {
                operation_id: format!("verify-{task}"),
                manifest_sha256: "0".repeat(64),
            },
            source_files: vec![artifact],
            source_directories: vec![],
            checks: vec![AcceptanceCheck {
                id: "check".into(),
                name: None,
                program: "checker.exe".into(),
                args: vec![],
                timeout_seconds: 5,
                expected_assertions: vec![],
                tool: None,
            }],
        };
        value.source.manifest_sha256 = value.source_manifest_digest().unwrap();
        value
    }
    fn assemble(inputs: Vec<DeliveryInput>) -> Result<DeliveryManifest, String> {
        DeliveryManifest::assemble("project".into(), "c".repeat(64), "f".repeat(64), inputs)
    }
    #[test]
    fn delivery_assembly_preserves_each_checked_source_and_rejects_conflicts() {
        let a = input("a", '1', "a.mjs", 1);
        let b = input("b", '2', "b.mjs", 2);
        let joined = assemble(vec![a.clone(), b.clone()]).unwrap();
        assert_eq!(joined.files.len(), 2);
        let mut tampered = b.clone();
        tampered.source_files[0].sha256 = "0".repeat(64);
        assert!(assemble(vec![a.clone(), tampered]).is_err());
        let alias = input("b", '2', "A.mjs", 2);
        assert!(assemble(vec![a.clone(), alias]).is_err());
        let mut unowned = b.clone();
        unowned.source_files.push(Artifact {
            path: "hidden.mjs".into(),
            bytes: 1,
            sha256: "0".repeat(64),
        });
        unowned.source.manifest_sha256 = unowned.source_manifest_digest().unwrap();
        assert!(assemble(vec![a.clone(), unowned]).is_err());
        let mut changed = input("next", '1', "b.mjs", 3);
        let mut old = a.source_files[0].clone();
        old.sha256 = "0".repeat(64);
        changed.source_files.push(old);
        changed.source.manifest_sha256 = changed.source_manifest_digest().unwrap();
        assert!(assemble(vec![a.clone(), changed])
            .unwrap_err()
            .contains("改变了已检查成果"));
        let mut next = input("next", '1', "b.mjs", 3);
        next.source_files.push(a.source_files[0].clone());
        next.source.manifest_sha256 = next.source_manifest_digest().unwrap();
        assert_eq!(assemble(vec![a, next]).unwrap().files.len(), 2);
    }
    #[test]
    fn delivery_assembly_requires_all_reports_before_blind_review() {
        let mut source = input("a", '1', "a.mjs", 1);
        let tool = ValidationTool {
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
        };
        source.checks[0].tool = Some(tool.clone());
        let manifest = assemble(vec![source.clone()]).unwrap();
        let record = crate::independent_checks::CheckRecord {
            schema_version: 1,
            project_id: "project".into(),
            revision: "c".repeat(64),
            task_id: "a".into(),
            attempt_id: source.manifest.attempt_id.clone(),
            operation_id: source.verification_operation.clone(),
            source_digest: source.source.manifest_sha256.clone(),
            manifest_digest: digest(&source.manifest).unwrap(),
            report_digest: source.verification_report_digest.clone(),
            observed_at: "2026-11-07T02:00:00Z".into(),
            checks: vec![crate::independent_checks::CheckFact {
                id: "check".into(),
                command_digest: digest(&source.checks[0]).unwrap(),
                tool_digest: digest(&tool).unwrap(),
                passed: true,
                reason: "检查通过".into(),
            }],
        };
        let mut archive = crate::independent_checks::CheckArchive {
            schema_version: 2,
            project_id: "project".into(),
            revision: "c".repeat(64),
            final_task: "core-assembled-delivery".into(),
            final_source: digest(&manifest).unwrap(),
            records: vec![record],
            assembled: Some(manifest),
        };
        archive.validate().unwrap();
        archive.records[0].checks[0].passed = false;
        assert!(archive.validate().is_err());
        archive.records.clear();
        assert!(archive.validate().is_err());
    }
}
