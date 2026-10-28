//! Bind the fixed experiment suite to a complete proposal. No approval or API calls.
use crate::{
    artifacts::{contains_path, root_path},
    baseline_runner::RunSpec,
    source_snapshot::{observe, Tree},
    verifier::PinnedCheck,
};
use cerulean_domain::{
    api_worker::ApiWorkerSelection,
    execution::*,
    workflow::{WorkflowPlan, WorkflowTask},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    io::Read,
    os::windows::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

const SCHEDULE: &str = "3e5cd950221fcb3dbf0d67d4719e7c18ead737e76547e4dbf6162cdd16612391";
const LAUNCHER: &str = "a818acfbe45f361890872bf82f190ee957afc7ee09abe879f76fb9f46ed11f06";
const CHECKER: &str = "5056b67d9ba932c784c9625a1fd457ae3cb83e2aaf70dfd9300171302103e536";

/// An explicit new proposal-text contract; original material bytes stay pinned.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskTextFormat {
    LfV1,
}

fn explicit_task_text_format<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<TaskTextFormat>, D::Error> {
    TaskTextFormat::deserialize(deserializer).map(Some)
}

fn task_goal(text: String, format: Option<TaskTextFormat>) -> String {
    match format {
        None => text,
        Some(TaskTextFormat::LfV1) => text.replace("\r\n", "\n").replace('\r', "\n"),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ComposeRequest {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "explicit_task_text_format"
    )]
    pub task_text_format: Option<TaskTextFormat>,
    pub bundle: String,
    pub manifest_sha256: String,
    pub schedule: String,
    pub launcher: String,
    pub checker: String,
    pub node: ValidationFile,
    pub project_root: String,
    pub data_root: String,
    pub worktree: GitWorktreePolicy,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MaterialProof {
    pub manifest_sha256: String,
    pub configuration_sha256: String,
    pub schedule_sha256: String,
    pub worker_sha256: String,
    pub task_sha256: String,
    pub check_spec_sha256: String,
    pub seed_digest: String,
    pub validator_digest: String,
    pub declared_execution_kind: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    schema: u32,
    kind: String,
    slot: Value,
    prepared_at: String,
    original_workspace: String,
    proposed_project_id: String,
    prepared_ready_sha256: String,
    configuration_sha256: String,
    worker_configuration_sha256: String,
    input_files: BTreeMap<String, String>,
    declared_execution_kind: String,
    actual_execution_kind: Option<String>,
    authorized: bool,
    experiment_record_ready: bool,
    release_accepted: bool,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Configuration {
    schema: u32,
    execution_kind: String,
    mode: String,
    pub(crate) worker: ApiWorkerSelection,
    pub(crate) management: Vec<Manager>,
    pub(crate) limits: Limits,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Manager {
    pub role: String,
    pub source: String,
    pub model: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Limits {
    task_seconds: u32,
    worker_requests: u32,
    pub(crate) management_requests: u32,
    failure_policy: String,
}
pub(crate) fn bytes(path: &Path, expected: &str) -> Result<Vec<u8>, String> {
    sha256(expected)?;
    crate::host::ordinary(path, false)?;
    root_path(
        path.parent()
            .and_then(Path::to_str)
            .ok_or("invalid material parent")?,
    )?;
    let mut value = vec![];
    OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(path)
        .map_err(|e| e.to_string())?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut value)
        .map_err(|e| e.to_string())?;
    require(
        value.len() <= 1024 * 1024 && format!("{:x}", Sha256::digest(&value)) == expected,
        "material digest changed or exceeds limit",
    )?;
    Ok(value)
}
fn decode<T: serde::de::DeserializeOwned>(value: &[u8]) -> Result<T, String> {
    serde_json::from_slice(value).map_err(|_| "invalid material JSON".into())
}
fn string(value: &Value, name: &str) -> Result<String, String> {
    value[name]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("missing material {name}"))
}
fn path(value: &str) -> Result<PathBuf, String> {
    require(
        Path::new(value).is_absolute()
            && !Path::new(value)
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir)),
        "absolute material path without traversal required",
    )?;
    root_path(value)
}
fn separate(a: &Path, b: &Path) -> Result<(), String> {
    require(
        !contains_path(a, b) && !contains_path(b, a),
        "material, data, project, pool and tools overlap",
    )
}
fn policy(exclude: Vec<String>) -> SnapshotPolicy {
    SnapshotPolicy {
        exclude,
        max_files: 100,
        max_bytes: 1024 * 1024,
    }
}

/// Reconstructs every plan field. Reading materials never authorizes a run.
/// The Git pool must already be initialized; commit reads never create a worktree.
pub fn compose(request: &ComposeRequest) -> Result<(RunSpec, MaterialProof), String> {
    let (spec, proof, _) = compose_mode(request, "single")?;
    Ok((spec, proof))
}

pub(crate) fn compose_mode(
    request: &ComposeRequest,
    mode: &str,
) -> Result<(RunSpec, MaterialProof, Configuration), String> {
    require(
        matches!(mode, "single" | "team"),
        "unsupported material mode",
    )?;
    let bundle = path(&request.bundle)?;
    let manifest_bytes = bytes(&bundle.join("manifest.json"), &request.manifest_sha256)?;
    let manifest: Manifest = decode(&manifest_bytes)?;
    require(
        manifest.schema == 1
            && manifest.kind == "experiment-materials-not-an-approved-run"
            && !manifest.authorized
            && !manifest.experiment_record_ready
            && !manifest.release_accepted
            && manifest.actual_execution_kind.is_none(),
        "not an unapproved material bundle",
    )?;
    timestamp(&manifest.prepared_at)?;
    sha256(&manifest.prepared_ready_sha256)?;
    require(
        Path::new(&manifest.original_workspace).is_absolute(),
        "invalid original workspace label",
    )?;
    let schedule_path = Path::new(&request.schedule);
    let schedule: Value = decode(&bytes(schedule_path, SCHEDULE)?)?;
    let slot_id = string(&manifest.slot, "id")?;
    identifier(&slot_id)?;
    let slots = schedule["slots"]
        .as_array()
        .ok_or("schedule slots missing")?;
    let found = slots
        .iter()
        .filter(|s| s["id"] == slot_id)
        .collect::<Vec<_>>();
    require(
        found.len() == 1
            && *found[0] == manifest.slot
            && manifest.slot["mode"] == mode
            && manifest.proposed_project_id == format!("eff-{slot_id}"),
        "slot differs from fixed requested schedule mode",
    )?;
    let seed_hashes: BTreeMap<String, String> =
        serde_json::from_value(manifest.slot["seed"].clone())
            .map_err(|_| "invalid schedule seed")?;
    let task_sha = string(&manifest.slot, "taskSha256")?;
    let check_sha = string(&manifest.slot, "checkerSha256")?;
    let mut inputs: BTreeMap<String, String> = seed_hashes
        .iter()
        .map(|(n, h)| (format!("inputs/project/{n}"), h.clone()))
        .collect();
    inputs.insert("inputs/control/task.txt".into(), task_sha.clone());
    inputs.insert("inputs/control/verify.json".into(), check_sha.clone());
    require(
        inputs == manifest.input_files,
        "bundle input inventory differs from fixed workload",
    )?;
    let configuration_bytes = bytes(
        &bundle.join("configuration.json"),
        &manifest.configuration_sha256,
    )?;
    let config: Configuration = decode(&configuration_bytes)?;
    config.worker.validate()?;
    // Python's bundle writer uses ASCII canonical JSON. These identifiers are
    // intentionally ASCII so both writers have identical escaping semantics.
    let worker_value = serde_json::to_value(&config.worker).map_err(|e| e.to_string())?;
    let worker_json = serde_json::to_vec(&worker_value).map_err(|e| e.to_string())?;
    require(
        worker_json.is_ascii() && digest(&worker_value)? == manifest.worker_configuration_sha256,
        "worker configuration differs from bundle",
    )?;
    require(
        config.schema == 1
            && config.mode == mode
            && matches!(config.execution_kind.as_str(), "synthetic" | "real-attempt")
            && config.execution_kind == manifest.declared_execution_kind
            && config.worker.file_tools
                == vec![ToolPermission::ReadFiles, ToolPermission::WriteFiles]
            && ((mode == "single"
                && config.management.is_empty()
                && config.limits.management_requests == 0)
                || (mode == "team"
                    && config.management.len() == 2
                    && (1..=1000).contains(&config.limits.management_requests)))
            && config.limits.failure_policy == "stop-no-retry"
            && config.limits.worker_requests > 0
            && config.limits.worker_requests <= 10000
            && config.limits.task_seconds > 0
            && config.limits.task_seconds <= 10000,
        "invalid experiment configuration or limits",
    )?;
    if mode == "team" {
        for role in ["chief", "deputy"] {
            let selected = config
                .management
                .iter()
                .filter(|m| m.role == role)
                .collect::<Vec<_>>();
            require(
                selected.len() == 1,
                "team must declare one chief and one deputy",
            )?;
            let m = selected[0];
            require(
                !m.model.is_empty()
                    && m.model.len() <= 200
                    && m.model
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b))
                    && match role {
                        "chief" => matches!(
                            m.source.as_str(),
                            "openai-api" | "codex-session" | "deepseek-api" | "a6api-api"
                        ),
                        _ => matches!(
                            m.source.as_str(),
                            "openai-api" | "deepseek-api" | "a6api-api"
                        ),
                    },
                "unsupported team manager",
            )?;
        }
    }
    let mut content = BTreeMap::new();
    for (name, hash) in &inputs {
        artifact_path(name)?;
        content.insert(name.clone(), bytes(&bundle.join(name), hash)?);
    }
    // Only frozen material files and the initially empty attachment directory.
    // Attaching later evidence is supported by historical inspect, not by rerun.
    let inventory = observe(&bundle, &policy(vec![]))?;
    let mut expected = inputs.clone();
    expected.insert("manifest.json".into(), request.manifest_sha256.clone());
    expected.insert(
        "configuration.json".into(),
        manifest.configuration_sha256.clone(),
    );
    let observed: BTreeMap<_, _> = inventory
        .files
        .iter()
        .map(|f| (f.path.clone(), f.sha256.clone()))
        .collect();
    require(
        observed == expected
            && inventory.directories
                == vec!["attachments", "inputs", "inputs/control", "inputs/project"],
        "unexpected bundle entry or attachment; use a fresh material bundle",
    )?;
    let seed = Tree {
        directories: vec![],
        files: seed_hashes
            .iter()
            .map(|(name, hash)| {
                Ok(Artifact {
                    path: name.clone(),
                    sha256: hash.clone(),
                    bytes: u32::try_from(content[&format!("inputs/project/{name}")].len())
                        .map_err(|_| "seed size overflow")?,
                })
            })
            .collect::<Result<_, String>>()?,
    };
    let goal = String::from_utf8(content["inputs/control/task.txt"].clone())
        .map_err(|_| "task is not UTF-8")?;
    let goal = task_goal(goal, request.task_text_format);
    bytes(Path::new(&request.launcher), LAUNCHER)?;
    bytes(Path::new(&request.checker), CHECKER)?;
    let launcher = std::fs::canonicalize(&request.launcher).map_err(|e| e.to_string())?;
    let checker = std::fs::canonicalize(&request.checker).map_err(|e| e.to_string())?;
    let relative_checker = launcher
        .parent()
        .and_then(Path::parent)
        .ok_or("invalid launcher location")?
        .join("efficiency-preparation-239/verify.mjs");
    require(
        std::fs::canonicalize(relative_checker).map_err(|e| e.to_string())? == checker,
        "launcher resolves a different checker",
    )?;
    let tool = PinnedCheck::describe(
        Path::new(&request.node.path),
        &[launcher, checker, bundle.join("inputs/control/verify.json")],
        Default::default(),
    )?;
    require(
        tool.executable == request.node,
        "Node executable differs from selected pin",
    )?;
    let git = PinnedCheck::describe(
        Path::new(&request.worktree.git.path),
        &[],
        Default::default(),
    )?;
    require(
        git.executable == request.worktree.git
            && request.worktree.integration.is_none()
            && request.worktree.input == GitWorktreeInput::CommittedRawBlobs,
        "Git policy differs from baseline",
    )?;
    let project = path(&request.project_root)?;
    let pool = path(&request.worktree.pool_root)?;
    let data = crate::baseline_runner::prospective(Path::new(&request.data_root))?;
    for (i, a) in [&project, &pool, &data, &bundle].into_iter().enumerate() {
        for b in [&project, &pool, &data, &bundle].into_iter().skip(i + 1) {
            separate(a, b)?;
        }
    }
    for file in std::iter::once(&tool.executable)
        .chain(tool.inputs.iter())
        .chain(std::iter::once(&git.executable))
    {
        for root in [&project, &pool, &data] {
            separate(root, Path::new(&file.path))?;
        }
    }
    let canonical_schedule = std::fs::canonicalize(schedule_path).map_err(|e| e.to_string())?;
    for root in [&project, &pool, &data] {
        separate(root, &canonical_schedule)?;
    }
    let mut worktree = request.worktree.clone();
    worktree.pool_root = pool.to_string_lossy().into();
    let plan = PlanRevision {
        schema_version: 1,
        management_setup_digest: None,
        project: Project {
            id: manifest.proposed_project_id.clone(),
            root: project.to_string_lossy().into(),
            timezone: "Asia/Shanghai".into(),
        },
        workflow: WorkflowPlan {
            id: format!("plan-{slot_id}"),
            version: 1,
            tasks: vec![WorkflowTask {
                id: "complete".into(),
                dependencies: vec![],
                resource_keys: None,
                workspace_key: isolated_task_key(&manifest.proposed_project_id, "complete")?,
                goal,
            }],
        },
        policy: ExecutionPolicy {
            version: 1,
            tools: vec![
                ToolPermission::ReadFiles,
                ToolPermission::WriteFiles,
                ToolPermission::RunCommands,
            ],
            max_requests: config.limits.worker_requests,
            max_seconds: config.limits.task_seconds,
            currency_budget: None,
            budget_usd_cents: None,
            supervision: None,
        },
        execution: vec![TaskExecution {
            task_id: "complete".into(),
            adapter_id: config.worker.target.provider().id().into(),
            profile_id: None,
            worker: None,
            desktop_worker: None,
            api_worker: Some(config.worker.clone()),
            supervision_override: None,
            artifact_paths: seed_hashes.keys().cloned().collect(),
            checks: vec![AcceptanceCheck {
                id: "fixed-workload-check".into(),
                name: None,
                program: tool.executable.path.clone(),
                args: vec![tool.inputs[0].path.clone(), tool.inputs[2].path.clone()],
                timeout_seconds: 30,
                tool: Some(tool.clone()),
            }],
        }],
        snapshot_policy: Some(policy(vec![".git".into()])),
        worktree: Some(worktree),
    };
    let spec = RunSpec {
        schema: 2,
        slot_id,
        data_root: data.to_string_lossy().into(),
        material_manifest_sha256: request.manifest_sha256.clone(),
        configuration_sha256: manifest.configuration_sha256.clone(),
        plan,
        seed,
        material: Some(request.clone()),
    };
    spec.validate()?;
    require(
        observe(&project, spec.plan.snapshot_policy.as_ref().unwrap())? == spec.seed,
        "project differs from fixed seed",
    )?;
    crate::git_workspaces::GitWorkspaces::inspect_committed_seed(
        &pool,
        &request.worktree.git,
        &project,
        &request.worktree.base_commit,
        &spec.seed,
    )?;
    let proof = MaterialProof {
        manifest_sha256: request.manifest_sha256.clone(),
        configuration_sha256: manifest.configuration_sha256,
        schedule_sha256: SCHEDULE.into(),
        worker_sha256: manifest.worker_configuration_sha256,
        task_sha256: task_sha,
        check_spec_sha256: check_sha,
        seed_digest: digest(&spec.seed)?,
        validator_digest: digest(&tool)?,
        declared_execution_kind: config.execution_kind.clone(),
    };
    Ok((spec, proof, config))
}

pub fn verify(spec: &RunSpec) -> Result<MaterialProof, String> {
    let (expected, proof) = compose(
        spec.material
            .as_ref()
            .ok_or("bound material specification required")?,
    )?;
    require(
        digest(&expected)? == digest(spec)?,
        "run specification differs from material-derived proposal",
    )?;
    Ok(proof)
}

#[cfg(test)]
#[path = "baseline_materials/text_tests.rs"]
mod text_tests;
