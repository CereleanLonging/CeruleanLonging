//! Fixed team materials mapped to the existing Host configuration and proposal.
//! Composition is read-only and does not approve or dispatch work.
use crate::{
    baseline_materials::{self, ComposeRequest, Manager, MaterialProof},
    source_snapshot::Tree,
    verifier::PinnedCheck,
};
use cerulean_domain::{
    credentials::ProviderEndpoint,
    execution::*,
    management::ProposedTask,
    project_setup::*,
    workflow::{WorkflowPlan, WorkflowTask},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};

const WORKLOADS: &str = "6a35106045fc2affb9810a605fe22b82fb3a90b5ad30415e1df53fa85d9e13c2";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TeamRequest {
    pub material: ComposeRequest,
    pub workloads: String,
    pub branch_specs: BTreeMap<String, ValidationFile>,
    pub chief_session_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TeamBlueprint {
    pub schema: u32,
    pub request: TeamRequest,
    pub proof: MaterialProof,
    pub workloads_sha256: String,
    pub setup: SavedProjectSetup,
    pub plan: PlanRevision,
    pub tasks: Vec<ProposedTask>,
    pub seed: Tree,
}

impl TeamBlueprint {
    /// Register the pinned validators, configure this draft using the existing
    /// Host entry, then explicitly select the strict experiment snapshot (249).
    pub fn configuration_draft(&self) -> ProjectSetupDraft {
        let s = &self.setup.setup;
        ProjectSetupDraft {
            chief_directory: s.chief_directory.clone(),
            management_sources: s.management_sources.clone(),
            currency_budget: None,
            resource_keys: vec![],
            profiles: s.profiles.iter().map(WorkerProfile::draft).collect(),
            worktree: s.worktree.as_ref().map(|w| WorktreeSetupDraft {
                integration: w.integration.as_deref().map(ProjectIntegration::draft),
                inherit_dependencies: true,
                base_commit: w.base_commit.clone(),
                pool_root: w.pool_root.clone(),
                git_executable: w.git.path.clone(),
            }),
            desktop_worker: None,
            api_worker: s.api_worker.clone(),
            omp_runtime_parent: None,
            worker_adapter: s.worker_adapter,
            supervision: None,
            project_id: s.project_id.clone(),
            requirement: s.requirement.clone(),
            codex_executable: String::new(),
            worker_model: s.api_worker.as_ref().unwrap().model.clone(),
            chief_model: s.chief_model.clone(),
            deputy_model: s.deputy_model.clone(),
            validator_id: s.validator_id.clone(),
            check_args: s.check.args.clone(),
            acceptance: s.acceptance.as_ref().map(|a| AcceptanceDraft {
                schema_version: a.schema_version,
                stages: a
                    .stages
                    .iter()
                    .map(|s| AcceptanceStageDraft {
                        expected_assertions: s.check.expected_assertions.clone(),
                        id: s.check.id.clone(),
                        name: s.check.name.clone().unwrap(),
                        validator_id: s.validator_id.clone(),
                        args: s.check.args.clone(),
                        timeout_seconds: s.check.timeout_seconds,
                    })
                    .collect(),
            }),
            artifact_paths: s.artifact_paths.clone(),
            worker_max_requests: s.policy.max_requests,
            management_max_requests: s.management_max_requests,
            max_seconds: s.policy.max_seconds,
        }
    }
}

fn check(tool: &ValidationTool, id: &str, name: &str) -> AcceptanceCheck {
    AcceptanceCheck {
        expected_assertions: vec![],
        id: id.into(),
        name: Some(name.into()),
        program: tool.executable.path.clone(),
        args: vec![tool.inputs[0].path.clone(), tool.inputs[2].path.clone()],
        timeout_seconds: 30,
        tool: Some(tool.clone()),
    }
}
fn acceptance(check: AcceptanceCheck, validator: &str) -> AcceptanceSetup {
    AcceptanceSetup {
        schema_version: 1,
        stages: vec![AcceptanceStage {
            validator_id: validator.into(),
            check,
        }],
    }
}
fn separate(root: &Path, file: &Path) -> Result<(), String> {
    require(
        !crate::artifacts::contains_path(root, file)
            && !crate::artifacts::contains_path(file, root),
        "team control file overlaps mutable or bundled data",
    )
}

fn management_sources(
    chief: &Manager,
    deputy: &Manager,
    chief_session_id: Option<&str>,
) -> Result<ManagementSources, String> {
    let chief = match chief.source.as_str() {
        "codex-session" => ChiefSource::CodexSession {
            session_id: chief_session_id
                .ok_or("Codex session identity required")?
                .into(),
        },
        "openai-api" if chief_session_id.is_none() => ChiefSource::OpenAi {},
        "deepseek-api" if chief_session_id.is_none() => ChiefSource::Provider {
            endpoint: ProviderEndpoint::DeepSeek {},
        },
        "a6api-api" if chief_session_id.is_none() => ChiefSource::Provider {
            endpoint: ProviderEndpoint::A6Api {},
        },
        _ => return Err("unsupported chief source or Codex session".into()),
    };
    let deputy = match deputy.source.as_str() {
        "deepseek-api" => DeputySource::DeepSeek {},
        "openai-api" => DeputySource::OpenAi {},
        "a6api-api" => DeputySource::Provider {
            endpoint: ProviderEndpoint::A6Api {},
        },
        _ => return Err("unsupported deputy source".into()),
    };
    Ok(ManagementSources { chief, deputy })
}

/// The fixed decomposition is an experimental protocol, not a model-generated
/// plan. The real chief must propose it through the normal source journal, then
/// the user must approve it; no provenance is manufactured here.
pub fn compose(request: &TeamRequest) -> Result<TeamBlueprint, String> {
    let (template, proof, config) = baseline_materials::compose_mode(&request.material, "team")?;
    let metadata: Value = serde_json::from_slice(&baseline_materials::bytes(
        Path::new(&request.workloads),
        WORKLOADS,
    )?)
    .map_err(|e| e.to_string())?;
    let selected = metadata["workloads"]
        .as_array()
        .ok_or("workloads missing")?
        .iter()
        .filter(|w| {
            w["taskSha256"] == proof.task_sha256 && w["checkerSha256"] == proof.check_spec_sha256
        })
        .collect::<Vec<_>>();
    require(
        selected.len() == 1,
        "fixed team workload missing or ambiguous",
    )?;
    let parts = selected[0]["decomposition"]
        .as_array()
        .ok_or("fixed decomposition missing")?;
    let parallel = parts.len() == 3;
    require(
        parts.len() == 1 || parallel,
        "unsupported fixed decomposition",
    )?;
    let expected_branches: Vec<String> = if parallel {
        vec!["left".into(), "right".into()]
    } else {
        vec![]
    };
    require(
        request.branch_specs.keys().cloned().collect::<Vec<_>>() == expected_branches,
        "branch specification inventory differs",
    )?;
    let full_tool = template.plan.execution[0].checks[0].tool.as_ref().unwrap();
    let full: Value = serde_json::from_slice(&baseline_materials::bytes(
        Path::new(&full_tool.inputs[2].path),
        &proof.check_spec_sha256,
    )?)
    .map_err(|e| e.to_string())?;
    let mut branch_tools = BTreeMap::new();
    let worktree = template.plan.worktree.as_ref().unwrap();
    let mut controls = vec![std::fs::canonicalize(&request.workloads).map_err(|e| e.to_string())?];
    for (branch, file) in &request.branch_specs {
        let actual: Value = serde_json::from_slice(&baseline_materials::bytes(
            Path::new(&file.path),
            &file.sha256,
        )?)
        .map_err(|e| e.to_string())?;
        let mut expected = full.clone();
        let cases = full["cases"]
            .as_array()
            .ok_or("full check cases missing")?
            .iter()
            .filter(|c| {
                c["expression"]
                    .as_str()
                    .is_some_and(|s| s.starts_with(&format!("{branch}.")))
            })
            .cloned()
            .collect::<Vec<_>>();
        require(!cases.is_empty(), "branch has no independent assertions")?;
        expected["cases"] = Value::Array(cases);
        require(
            actual == expected,
            "branch assertions differ from exact full-check projection",
        )?;
        let tool = PinnedCheck::describe(
            Path::new(&request.material.node.path),
            &[
                full_tool.inputs[0].path.clone().into(),
                full_tool.inputs[1].path.clone().into(),
                file.path.clone().into(),
            ],
            Default::default(),
        )?;
        require(
            tool.executable == request.material.node && tool.inputs[2] == *file,
            "branch tool pin differs",
        )?;
        controls.push(std::fs::canonicalize(&file.path).map_err(|e| e.to_string())?);
        branch_tools.insert(branch.clone(), tool);
    }
    for control in controls {
        for root in [
            &template.data_root,
            &template.plan.project.root,
            &worktree.pool_root,
            &request.material.bundle,
        ] {
            let canonical = crate::baseline_runner::prospective(Path::new(root))?;
            separate(&canonical, &control)?;
        }
    }
    let chief = config
        .management
        .iter()
        .find(|m| m.role == "chief")
        .unwrap();
    let deputy = config
        .management
        .iter()
        .find(|m| m.role == "deputy")
        .unwrap();
    let sources = management_sources(chief, deputy, request.chief_session_id.as_deref())?;
    sources.validate(&chief.model, false)?;
    let adapter: ProjectWorkerAdapter =
        serde_json::from_value(Value::String(config.worker.target.provider().id().into()))
            .map_err(|e| e.to_string())?;
    let mut tasks = Vec::new();
    let mut profiles = Vec::new();
    for part in parts {
        let id = part["id"].as_str().ok_or("task identity missing")?;
        identifier(id)?;
        let files: Vec<String> =
            serde_json::from_value(part["files"].clone()).map_err(|e| e.to_string())?;
        let dependencies: Vec<String> =
            serde_json::from_value(part["dependsOn"].clone()).map_err(|e| e.to_string())?;
        let branch = branch_tools.get(id);
        let validator = if branch.is_some() {
            format!("eff-{id}")
        } else {
            "eff-full".into()
        };
        let profile_id = branch.map(|_| format!("profile-{id}"));
        if let Some(tool) = branch {
            profiles.push(WorkerProfile {
                id: profile_id.clone().unwrap(),
                description: format!("Fixed {id} branch"),
                resource_keys: vec![],
                worker_adapter: adapter,
                worker: None,
                api_worker: Some(config.worker.clone()),
                desktop_worker: None,
                supervision_override: None,
                artifact_paths: files.clone(),
                acceptance: acceptance(
                    check(tool, &format!("check-{id}"), &format!("Fixed {id} branch")),
                    &validator,
                ),
            });
        }
        tasks.push(ProposedTask { id: id.into(), dependencies, profile_id, worker_id: adapter.worker_id(), validator_id: validator,
            goal: format!("{}\nFixed task: {id}. Modify only: {}. Preserve other files; use accepted dependency inputs.", template.plan.workflow.tasks[0].goal, files.join(", ")) });
    }
    let mut policy = worktree.clone();
    policy.input = GitWorktreeInput::CommittedRawBlobsWithAcceptedDependencies;
    if parallel {
        let final_task = parts.last().unwrap();
        require(final_task["id"] == "integrate", "fixed final task mismatch")?;
        policy.integration = Some(Box::new(ProjectIntegration {
            task_id: "integrate".into(),
            artifact_paths: vec!["main.mjs".into()],
            validator_id: "eff-full".into(),
            check: check(full_tool, "project-integration-check", "项目整体检查"),
        }));
    }
    let full_check = check(full_tool, "fixed-workload-check", "Fixed workload");
    let setup = ProjectSetup {
        worker_directories: Default::default(),
        chief_directory: None,
        management_sources: sources.uses_external_provider().then_some(sources),
        resource_keys: vec![],
        profiles,
        worktree: Some(policy),
        desktop_worker: None,
        api_worker: Some(config.worker),
        worker_adapter: adapter,
        project_id: template.plan.project.id.clone(),
        version: 2,
        requirement: format!(
            "{}\nExperimental decomposition (propose these exact tasks, without executing):\n{}",
            template.plan.workflow.tasks[0].goal,
            serde_json::to_string(&tasks).map_err(|e| e.to_string())?
        ),
        worker: None,
        chief_model: chief.model.clone(),
        deputy_model: deputy.model.clone(),
        validator_id: "eff-full".into(),
        check: full_check.clone(),
        acceptance: Some(acceptance(full_check, "eff-full")),
        artifact_paths: template.plan.execution[0].artifact_paths.clone(),
        policy: template.plan.policy.clone(),
        snapshot_policy: template.plan.snapshot_policy.clone().unwrap(),
        management_max_requests: config.limits.management_requests,
    };
    let saved = SavedProjectSetup {
        digest: digest(&setup)?,
        setup,
    };
    crate::project_setup::validate(&saved)?;
    let plan = PlanRevision {
        schema_version: 1,
        management_setup_digest: saved
            .setup
            .management_sources
            .as_ref()
            .map(|_| saved.digest.clone()),
        project: template.plan.project.clone(),
        workflow: WorkflowPlan {
            id: format!("plan-{}", &digest(&template.plan.project.id)?[..32]),
            version: 1,
            tasks: tasks
                .iter()
                .map(|t| {
                    Ok(WorkflowTask {
                        id: t.id.clone(),
                        goal: t.goal.clone(),
                        dependencies: t.dependencies.clone(),
                        workspace_key: isolated_task_key(&template.plan.project.id, &t.id)?,
                        resource_keys: None,
                    })
                })
                .collect::<Result<_, String>>()?,
        },
        execution: tasks
            .iter()
            .map(|t| saved.setup.execution_for(&t.id, t.profile_id.as_deref()))
            .collect::<Result<_, _>>()?,
        policy: saved.setup.policy.clone(),
        snapshot_policy: Some(saved.setup.snapshot_policy.clone()),
        worktree: saved.setup.worktree.clone(),
    };
    plan.validate()?;
    Ok(TeamBlueprint {
        schema: 1,
        request: request.clone(),
        proof,
        workloads_sha256: WORKLOADS.into(),
        setup: saved,
        plan,
        tasks,
        seed: template.seed,
    })
}

/// Re-read pinned materials and actual committed seed. Never accepts a caller's
/// self-reported hashes as a substitute for deriving the full expected content.
pub fn verify(blueprint: &TeamBlueprint) -> Result<(), String> {
    require(
        digest(&compose(&blueprint.request)?)? == digest(blueprint)?,
        "team blueprint differs from fixed materials",
    )
}

#[cfg(test)]
mod source_tests_289 {
    use super::*;

    fn manager(role: &str, source: &str) -> Manager {
        Manager {
            role: role.into(),
            source: source.into(),
            model: "fixed-model".into(),
        }
    }

    #[test]
    fn a6api_source_is_preserved_for_each_management_role() {
        let a6_chief = manager("chief", "a6api-api");
        let a6_deputy = manager("deputy", "a6api-api");
        let sources = management_sources(&a6_chief, &a6_deputy, None).unwrap();
        assert_eq!(
            sources,
            ManagementSources {
                chief: ChiefSource::Provider {
                    endpoint: ProviderEndpoint::A6Api {},
                },
                deputy: DeputySource::Provider {
                    endpoint: ProviderEndpoint::A6Api {},
                },
            }
        );
        assert!(sources.uses_provider_selection());
        assert!(sources.uses_external_provider());
        sources.validate("fixed-model", false).unwrap();

        let mixed = management_sources(
            &manager("chief", "codex-session"),
            &a6_deputy,
            Some("session-289"),
        )
        .unwrap();
        assert!(matches!(mixed.chief, ChiefSource::CodexSession { .. }));
        assert!(matches!(
            mixed.deputy,
            DeputySource::Provider {
                endpoint: ProviderEndpoint::A6Api {}
            }
        ));
        assert!(management_sources(&a6_chief, &a6_deputy, Some("session-289")).is_err());
        assert!(management_sources(&manager("chief", "invalid-api"), &a6_deputy, None).is_err());
        assert!(management_sources(&a6_chief, &manager("deputy", "codex-session"), None).is_err());
    }

    #[test]
    fn legacy_sources_retain_their_original_identity() {
        let sources = management_sources(
            &manager("chief", "openai-api"),
            &manager("deputy", "deepseek-api"),
            None,
        )
        .unwrap();
        assert_eq!(sources.chief, ChiefSource::OpenAi {});
        assert_eq!(sources.deputy, DeputySource::DeepSeek {});
    }

    #[test]
    fn deepseek_chief_preserves_provider_and_legacy_deputy_identity() {
        let chief = manager("chief", "deepseek-api");
        let deputy = manager("deputy", "deepseek-api");
        let sources = management_sources(&chief, &deputy, None).unwrap();
        assert_eq!(
            sources.chief,
            ChiefSource::Provider {
                endpoint: ProviderEndpoint::DeepSeek {},
            }
        );
        assert_eq!(sources.deputy, DeputySource::DeepSeek {});
        sources.validate("fixed-model", false).unwrap();
        assert!(management_sources(&chief, &deputy, Some("session-509")).is_err());
        assert!(management_sources(&chief, &manager("deputy", "codex-session"), None).is_err());
    }

    #[test]
    #[ignore = "509 fixed synthetic material copy, pinned Git/Node; no dispatch"]
    fn deepseek_chief_composes_original_materials_and_rejects_changed_binding() {
        let path = std::env::var_os("CERULEAN_DEEPSEEK_MATERIAL_509").unwrap();
        let request: TeamRequest = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let blueprint = compose(&request).unwrap();
        let sources = blueprint.setup.setup.management_sources.as_ref().unwrap();
        assert_eq!(
            sources.chief,
            ChiefSource::Provider {
                endpoint: ProviderEndpoint::DeepSeek {},
            }
        );
        assert_eq!(sources.deputy, DeputySource::DeepSeek {});
        assert_eq!(blueprint.tasks.len(), 1);
        assert_eq!(blueprint.plan.execution[0].adapter_id, "deepseek");
        verify(&blueprint).unwrap();
        let mut changed = blueprint.clone();
        changed
            .setup
            .setup
            .management_sources
            .as_mut()
            .unwrap()
            .chief = ChiefSource::OpenAi {};
        changed.setup.digest = digest(&changed.setup.setup).unwrap();
        assert!(verify(&changed).is_err());
        let mut mismatched_session = request.clone();
        mismatched_session.chief_session_id = Some("session-509".into());
        assert!(compose(&mismatched_session).is_err());
        assert!(!Path::new(&request.material.data_root).exists());
    }
}
