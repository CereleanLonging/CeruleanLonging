//! Portable planning input. Parsing or reviewing a template grants no authority.
use crate::execution::{digest, require};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use ts_rs::TS;

pub const MAX_TEMPLATE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowTemplate {
    pub schema_version: u32,
    pub workflow_type: String,
    pub name: String,
    pub objective: String,
    pub steps: Vec<WorkflowTemplateStep>,
    pub deputy_guidance: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supervision: Option<WorkflowTemplateSupervision>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowTemplateSupervision {
    pub repeated_failure_threshold: u32,
    pub max_advice_per_attempt: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowTemplateStep {
    pub id: String,
    pub depends_on: Vec<String>,
    pub goal: String,
    pub completion_criteria: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkflowTemplateReview {
    pub template: WorkflowTemplate,
    pub digest: String,
    pub requirement: String,
}

fn text(value: &str, max: usize) -> Result<(), String> {
    require(
        !value.trim().is_empty()
            && value.len() <= max
            && !value
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t'),
        "模板文字为空、过长或包含不支持的控制字符",
    )
}
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Same parser for desktop and future local integrations; no filesystem/network IO.
pub fn review(input: &str) -> Result<WorkflowTemplateReview, String> {
    require(
        input.len() <= MAX_TEMPLATE_BYTES,
        "工作流模板不得超过64 KiB",
    )?;
    let template: WorkflowTemplate = serde_json::from_str(input.trim_start_matches('\u{feff}'))
        .map_err(|_| "工作流模板JSON无效、字段重复或含不支持的字段".to_owned())?;
    require(template.schema_version == 1, "不支持此工作流模板版本")?;
    require(
        identifier(&template.workflow_type),
        "工作流类型须为1–64位字母、数字、下划线或连字符",
    )?;
    text(&template.name, 160)?;
    text(&template.objective, 2400)?;
    require(
        (1..=32).contains(&template.steps.len()),
        "工作流须有1–32个步骤",
    )?;
    require(
        template.deputy_guidance.len() <= 12,
        "副指挥监督要点最多12条",
    )?;
    if let Some(policy) = &template.supervision {
        crate::supervision::SupervisionPolicy {
            schema_version: 1,
            repeated_failure_threshold: policy.repeated_failure_threshold,
            max_advice_per_attempt: policy.max_advice_per_attempt,
            delivery_enabled: false,
        }
        .validate()?;
    }
    let mut ids = BTreeSet::new();
    for step in &template.steps {
        require(
            identifier(&step.id) && ids.insert(step.id.as_str()),
            "步骤编号无效或重复",
        )?;
        text(&step.goal, 1600)?;
        require(
            (1..=12).contains(&step.completion_criteria.len()),
            "每步须有1–12条完成标准",
        )?;
        for criterion in &step.completion_criteria {
            text(criterion, 800)?;
        }
    }
    for step in &template.steps {
        let dependencies: BTreeSet<_> = step.depends_on.iter().collect();
        require(
            dependencies.len() == step.depends_on.len()
                && step
                    .depends_on
                    .iter()
                    .all(|id| id != &step.id && ids.contains(id.as_str())),
            "步骤依赖重复、自引用或引用了不存在的步骤",
        )?;
    }
    let mut completed = BTreeSet::new();
    loop {
        let previous = completed.len();
        for step in &template.steps {
            if step
                .depends_on
                .iter()
                .all(|id| completed.contains(id.as_str()))
            {
                completed.insert(step.id.as_str());
            }
        }
        if completed.len() == template.steps.len() {
            break;
        }
        require(completed.len() > previous, "工作流步骤存在循环依赖")?;
    }
    for guidance in &template.deputy_guidance {
        text(guidance, 800)?;
    }
    let mut requirement = format!(
        "工作流：{}（{}，模板版本1）\n目标：{}\n\n步骤与完成标准：\n",
        template.name, template.workflow_type, template.objective
    );
    for step in &template.steps {
        requirement.push_str(&format!(
            "\n[{}] {}\n依赖：{}\n",
            step.id,
            step.goal,
            if step.depends_on.is_empty() {
                "无".to_owned()
            } else {
                step.depends_on.join(", ")
            }
        ));
        for criterion in &step.completion_criteria {
            requirement.push_str(&format!("- {criterion}\n"));
        }
    }
    requirement.push_str("\n副指挥监督要点：\n");
    for guidance in &template.deputy_guidance {
        requirement.push_str(&format!("- {guidance}\n"));
    }
    requirement.push_str("\n规划时请将步骤依赖、完成标准和相关监督要点落实到任务目标。模型、工具、产物及检查器以本机配置和最终批准方案为准。此模板仅为规划输入，不授予执行、额外调用或验收权限。\n");
    require(
        requirement.len() <= 8000,
        "模板转换后的需求超过8000字节，请精简内容",
    )?;
    Ok(WorkflowTemplateReview {
        digest: digest(&template)?,
        template,
        requirement,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    fn sample() -> Value {
        json!({"schemaVersion":1,"workflowType":"coding","name":"Feature","objective":"Deliver a checked change","steps":[{"id":"check","dependsOn":["build"],"goal":"Review output","completionCriteria":["Independent checks pass"]},{"id":"build","dependsOn":[],"goal":"Implement","completionCriteria":["Declared files only"]}],"deputyGuidance":["Preserve failure evidence and pause for missing inputs"]})
    }
    #[test]
    fn unordered_dag_preserves_criteria_and_guidance() {
        let v = sample();
        let r = review(&v.to_string()).unwrap();
        assert!(r.requirement.contains("Independent checks pass"));
        assert!(r.requirement.contains("Preserve failure evidence"));
        assert_eq!(
            r.digest,
            review(&serde_json::to_string_pretty(&v).unwrap())
                .unwrap()
                .digest
        );
    }
    #[test]
    fn rejects_unsupported_authority_and_duplicate_fields() {
        let mut v = sample();
        v["execute"] = json!(true);
        assert!(review(&v.to_string()).is_err());
        let mut v = sample();
        v["steps"][0]["command"] = json!("run");
        assert!(review(&v.to_string()).is_err());
        assert!(review(
            &sample()
                .to_string()
                .replacen("{", "{\"schemaVersion\":1,", 1)
        )
        .is_err());
        let mut v = sample();
        v["schemaVersion"] = json!(2);
        assert!(review(&v.to_string()).is_err());
        let mut v = sample();
        v["supervision"] =
            json!({"repeatedFailureThreshold":2,"maxAdvicePerAttempt":1,"deliveryEnabled":true});
        assert!(review(&v.to_string()).is_err());
    }
    #[test]
    fn supervision_reuses_existing_bounds_without_delivery_authority() {
        let mut v = sample();
        v["supervision"] = json!({"repeatedFailureThreshold":2,"maxAdvicePerAttempt":1});
        assert!(review(&v.to_string())
            .unwrap()
            .template
            .supervision
            .is_some());
        for value in [
            json!({"repeatedFailureThreshold":1,"maxAdvicePerAttempt":1}),
            json!({"repeatedFailureThreshold":2,"maxAdvicePerAttempt":3}),
        ] {
            v["supervision"] = value;
            assert!(review(&v.to_string()).is_err());
        }
    }
    #[test]
    fn rejects_invalid_graphs() {
        for deps in [
            json!(["missing"]),
            json!(["check"]),
            json!(["build", "build"]),
        ] {
            let mut v = sample();
            v["steps"][0]["dependsOn"] = deps;
            assert!(review(&v.to_string()).is_err());
        }
        let mut v = sample();
        v["steps"][1]["dependsOn"] = json!(["check"]);
        assert!(review(&v.to_string()).is_err());
        let mut v = sample();
        v["steps"][1]["id"] = json!("check");
        assert!(review(&v.to_string()).is_err());
    }
    #[test]
    fn bounds_bytes_and_required_completion_evidence() {
        assert!(review(&" ".repeat(MAX_TEMPLATE_BYTES + 1)).is_err());
        let mut v = sample();
        v["steps"][0]["completionCriteria"] = json!([]);
        assert!(review(&v.to_string()).is_err());
        let mut v = sample();
        v["deputyGuidance"] = json!(vec!["x".repeat(800); 12]);
        assert!(review(&v.to_string()).is_err());
        let mut v = sample();
        v["objective"] = json!("bad\u{0000}");
        assert!(review(&v.to_string()).is_err());
    }
}
