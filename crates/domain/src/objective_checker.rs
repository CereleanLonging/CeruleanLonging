//! The single workbench checker executes reviewed test source. Model suggestions
//! cannot select a program: the host binds an observed runtime before approval.
use crate::execution::{require, validate_assertion_ids, AcceptanceCheck, ValidationFile};
use serde::{Deserialize, Serialize};

pub const ID: &str = "cerulean-objective-checker";
pub const PROTOCOL: &str = "cerulean-objective-checker-v1";
pub const ARGUMENT: &str = "--approved-script-v1";

pub fn validate_artifacts(paths: &[String]) -> Result<(), String> {
    require(
        !paths.is_empty() && paths.len() <= 64,
        "每项任务须在方案中列明 1–64 个交付文件",
    )?;
    let mut unique = std::collections::BTreeSet::new();
    for path in paths {
        crate::execution::artifact_path(path)?;
        require(unique.insert(path.to_uppercase()), "任务交付文件重复")?;
    }
    Ok(())
}

/// Report inspection is a separate protocol. A checker cannot prepare review
/// materials, inspect ballots or count votes through this endpoint.
pub fn handle(bytes: &[u8]) -> Result<Vec<u8>, String> {
    use crate::independent_verifier::{Operation, Request, Response, MAX_MESSAGE_BYTES};
    require(bytes.len() <= MAX_MESSAGE_BYTES, "独立检查请求超限")?;
    let request: Request = serde_json::from_slice(bytes).map_err(|_| "独立检查请求格式无效")?;
    require(request.protocol == PROTOCOL, "独立检查协议不符")?;
    require(
        matches!(&request.operation, Operation::InspectChecks { .. }),
        "独立检查工具不接受盲评或计票操作",
    )?;
    let response = Response {
        protocol: PROTOCOL.into(),
        version: env!("CARGO_PKG_VERSION").into(),
        request_digest: crate::execution::digest(&request)?,
        process_id: std::process::id(),
        result: crate::independent_verifier::evaluate(&request.operation),
    };
    serde_json::to_vec(&response).map_err(|e| e.to_string())
}

pub fn bind_execution(
    setup: &crate::project_setup::ProjectSetup,
    proposed: &crate::management::ProposedTask,
    runtimes: &std::collections::BTreeMap<Language, ValidationFile>,
) -> Result<crate::execution::TaskExecution, String> {
    let mut execution = setup.execution_for(&proposed.id, proposed.profile_id.as_deref())?;
    if execution.artifact_paths.is_empty() && proposed.validator_id == ID {
        execution.artifact_paths = proposed
            .artifact_paths
            .clone()
            .ok_or("主指挥尚未列明任务交付文件，请修订方案")?;
        validate_artifacts(&execution.artifact_paths)?;
    } else if let Some(paths) = &proposed.artifact_paths {
        require(
            paths == &execution.artifact_paths,
            "任务交付文件与原固定配置不符",
        )?;
    }
    if proposed.validator_id != ID {
        return Ok(execution);
    } // historical replay only
    let test = proposed
        .objective_test
        .as_ref()
        .ok_or("主指挥尚未提供客观测试源码和断言，请修订方案")?;
    let runtime = runtimes
        .get(&test.language)
        .ok_or("所需客观测试运行时未安装或未由核心绑定")?;
    require(
        execution.checks.len() == 1,
        "固定独立检查入口只能绑定一个批准测试方案",
    )?;
    execution.checks[0] = bind(&execution.checks[0], test, runtime)?;
    Ok(execution)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Language {
    Python,
    JavaScript,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScriptTest {
    pub language: Language,
    pub source: String,
    pub assertions: Vec<String>,
}
impl ScriptTest {
    pub fn validate(&self) -> Result<(), String> {
        require(
            !self.source.trim().is_empty()
                && self.source.len() <= 8192
                && !self.source.contains('\0'),
            "客观测试源码须为 1–8192 字节",
        )?;
        require(
            !self.assertions.is_empty(),
            "主指挥必须提供明确的客观测试断言",
        )?;
        validate_assertion_ids(&self.assertions)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BoundScript {
    pub schema_version: u32,
    pub test: ScriptTest,
    pub runtime: ValidationFile,
    pub timeout_seconds: u32,
}
impl BoundScript {
    pub fn validate(&self) -> Result<(), String> {
        require(
            self.schema_version == 1 && self.timeout_seconds > 0 && self.timeout_seconds <= 3600,
            "独立检查方案版本或时限无效",
        )?;
        self.test.validate()?;
        let name = self
            .runtime
            .path
            .replace('\\', "/")
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        require(
            name == match self.test.language {
                Language::Python => "python.exe",
                Language::JavaScript => "node.exe",
            },
            "客观测试语言与批准运行时不符",
        )?;
        crate::execution::ValidationTool {
            executable: self.runtime.clone(),
            inputs: vec![],
            environment: Default::default(),
        }
        .validate()
    }
    pub fn arguments(&self) -> Result<Vec<String>, String> {
        self.validate()?;
        let text = serde_json::to_string(self).map_err(|e| e.to_string())?;
        require(text.len() <= 16000, "客观测试方案超出批准参数容量")?;
        let mut args = vec![ARGUMENT.into()];
        let mut part = String::new();
        for c in text.chars() {
            if part.len() + c.len_utf8() > 3000 {
                args.push(std::mem::take(&mut part));
            }
            part.push(c);
        }
        if !part.is_empty() {
            args.push(part);
        }
        Ok(args)
    }
    pub fn parse(args: &[String]) -> Result<Self, String> {
        require(
            args.first().map(String::as_str) == Some(ARGUMENT) && args.len() <= 8 && args.len() > 1,
            "缺少已批准的客观测试方案，不能判为通过",
        )?;
        require(
            args.iter().skip(1).all(|s| s.len() <= 3000)
                && args.iter().map(String::len).sum::<usize>() <= 16384,
            "客观测试参数超限",
        )?;
        let value: Self =
            serde_json::from_str(&args[1..].concat()).map_err(|_| "客观测试方案格式无效")?;
        value.validate()?;
        Ok(value)
    }
}

/// Compare the immutable role setup to a concrete approved task without losing
/// any worker, directory, permission, artifact or checker-template field.
pub fn matches_setup(
    setup: &crate::project_setup::ProjectSetup,
    actual: &crate::execution::TaskExecution,
) -> Result<bool, String> {
    let mut expected = setup.execution_for(&actual.task_id, actual.profile_id.as_deref())?;
    if setup.validator_for(&actual.task_id, actual.profile_id.as_deref())? == ID {
        if expected.artifact_paths.is_empty() {
            validate_artifacts(&actual.artifact_paths)?;
            expected.artifact_paths = actual.artifact_paths.clone();
        }
        require(
            actual.checks.len() == 1 && expected.checks.len() == 1,
            "独立检查绑定数量不符",
        )?;
        let recipe = BoundScript::parse(&actual.checks[0].args)?;
        expected.checks[0] = bind(&expected.checks[0], &recipe.test, &recipe.runtime)?;
    }
    Ok(expected == *actual)
}

pub fn bind(
    base: &AcceptanceCheck,
    test: &ScriptTest,
    runtime: &ValidationFile,
) -> Result<AcceptanceCheck, String> {
    test.validate()?;
    let mut check = base.clone();
    let tool = check.tool.as_mut().ok_or("固定检查工具绑定缺失")?;
    require(
        tool.inputs.is_empty() && tool.environment.is_empty() && base.args.is_empty(),
        "固定检查模板被修改",
    )?;
    tool.inputs.push(runtime.clone());
    check.expected_assertions = test.assertions.clone();
    check.args = BoundScript {
        schema_version: 1,
        test: test.clone(),
        runtime: runtime.clone(),
        timeout_seconds: base.timeout_seconds,
    }
    .arguments()?;
    Ok(check)
}

/// Included in every chief transport; missing runtime or coverage asks the user.
pub const PLANNING_INSTRUCTIONS: &str = r#"When objectiveRuntimes is present, the only validator is cerulean-objective-checker. Every proposed task MUST include artifactPaths:['relative/source-or-deliverable.ext'] listing 1-64 actual deliverables owned by that task, in addition to objectiveTest:{language:'python'|'javaScript',source:'reviewable test code',assertions:['stable-id']}. Choose an available objectiveRuntimes language; never supply an executable path. Source <=8192 UTF-8 bytes executes in the sealed task working directory under an isolated account, with no inherited credentials or PATH. Use standard libraries and local project imports only; do not install dependencies, use the network, read other directories, or modify project sources. Run actual behavioral checks against requirements, including negative cases, not just file existence, syntax or self-reports. Print exactly one JSON object {schemaVersion:1,assertions:[{id:'stable-id',passed:true|false,detail:'observed result'}]} with each declared assertion exactly once. Failures must be reported honestly, with a nonzero exit permitted. These tests are proposals and execute only after user review/approval. The final task must test the complete deliverable and cover all requirements; preserve explicit integration constraints. If no runtime, insufficient context, missing dependencies or no meaningful objective test is possible, return Wait with the needed prerequisite instead of inventing a pass."#;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn empty_or_unapproved_test_is_not_a_pass() {
        assert!(BoundScript::parse(&[]).is_err());
        assert!(ScriptTest {
            language: Language::Python,
            source: "print('ok')".into(),
            assertions: vec![]
        }
        .validate()
        .is_err());
    }
    #[test]
    fn unicode_source_roundtrips_and_every_field_is_bound() {
        let value = BoundScript {
            schema_version: 1,
            test: ScriptTest {
                language: Language::JavaScript,
                source: "//中文\n".repeat(700),
                assertions: vec!["actual-behavior".into()],
            },
            runtime: ValidationFile {
                path: "D:\\runtime\\node.exe".into(),
                sha256: "a".repeat(64),
            },
            timeout_seconds: 30,
        };
        let args = value.arguments().unwrap();
        assert_eq!(BoundScript::parse(&args).unwrap(), value);
        assert!(BoundScript::parse(&[ARGUMENT.into(), "{}".into()]).is_err());
    }
    #[test]
    fn checker_protocol_cannot_count_votes_or_accept_verifier_protocol() {
        let operation = crate::independent_verifier::Operation::Tally {
            material: crate::blind_review::ReviewMaterial {
                independent_check_evidence: None,
                protocol: crate::blind_review::PROTOCOL.into(),
                requirement: "r".into(),
                criteria: vec![],
                fragments: vec![],
                objective_checks: 0,
            },
            seats: vec![],
            ballots: vec![],
        };
        let request = crate::independent_verifier::Request {
            protocol: PROTOCOL.into(),
            operation,
        };
        assert!(handle(&serde_json::to_vec(&request).unwrap())
            .unwrap_err()
            .contains("不接受"));
        let mut wrong = request;
        wrong.protocol = crate::independent_verifier::TOOL_PROTOCOL.into();
        assert!(handle(&serde_json::to_vec(&wrong).unwrap())
            .unwrap_err()
            .contains("协议"));
    }
    #[test]
    fn approved_recipe_changes_digest_and_requires_exact_assertions() {
        use crate::execution::*;
        let base = AcceptanceCheck {
            id: "tests".into(),
            name: None,
            program: "D:\\tools\\cerulean-checker.exe".into(),
            args: vec![],
            timeout_seconds: 30,
            expected_assertions: vec![],
            tool: Some(ValidationTool {
                executable: ValidationFile {
                    path: "D:\\tools\\cerulean-checker.exe".into(),
                    sha256: "b".repeat(64),
                },
                inputs: vec![],
                environment: Default::default(),
            }),
        };
        let runtime = ValidationFile {
            path: "D:\\tools\\node.exe".into(),
            sha256: "a".repeat(64),
        };
        let test = ScriptTest {
            language: Language::JavaScript,
            source: "behavioral test".into(),
            assertions: vec!["sum".into()],
        };
        let bound = bind(&base, &test, &runtime).unwrap();
        let mut changed = test.clone();
        changed.source.push_str(" changed");
        assert_ne!(
            digest(&bound).unwrap(),
            digest(&bind(&base, &changed, &runtime).unwrap()).unwrap()
        );
        assert!(bind(&bound, &test, &runtime).is_err());
        assert!(parse_assertions(&bound, b"ok").is_err());
        assert!(parse_assertions(&bound, br#"{"schemaVersion":1,"assertions":[]}"#).is_err());
        let failures=parse_assertions(&bound,br#"{"schemaVersion":1,"assertions":[{"id":"sum","passed":false,"detail":"wrong result"}]}"#).unwrap();
        assert!(require_assertions(&bound, &failures, true).is_err());
    }
}
