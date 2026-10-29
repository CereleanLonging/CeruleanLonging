//! Explicit local experiment runner, reusing the original durable workflow,
//! API/file loop and verifier. Never resumes an intent or fabricates managers.
use crate::{
    api_execution::{load, publish},
    artifacts::{contains_path, root_path},
    scheduler::{CommandContext, DurableWorkflow, SchedulerConfig},
    source_snapshot::{observe, Tree},
    verifier::PinnedCheck,
};
use cerulean_adapters::api_generation::{ApiClient, ApiFailure, ApiReply, PreparedRequest};
use cerulean_domain::{
    execution::*,
    workflow::{TaskState, WorkflowCommand},
};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::Instant,
};
use tokio::sync::oneshot;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub material: Option<crate::baseline_materials::ComposeRequest>,
    pub schema: u32,
    pub slot_id: String,
    pub data_root: String,
    pub material_manifest_sha256: String,
    pub configuration_sha256: String,
    pub plan: PlanRevision,
    pub seed: Tree,
}
impl RunSpec {
    pub fn validate(&self) -> Result<(), String> {
        require(
            (self.schema == 1 && self.material.is_none())
                || (self.schema == 2 && self.material.is_some()),
            "unsupported baseline spec or missing material binding",
        )?;
        identifier(&self.slot_id)?;
        require(
            Path::new(&self.data_root).is_absolute(),
            "baseline data root must be absolute",
        )?;
        sha256(&self.material_manifest_sha256)?;
        sha256(&self.configuration_sha256)?;
        self.plan.validate()?;
        require(
            self.plan.project.id == format!("eff-{}", self.slot_id)
                && self.plan.workflow.version == 1
                && self.plan.workflow.tasks.len() == 1
                && self.plan.workflow.tasks[0].dependencies.is_empty()
                && self.plan.execution.len() == 1
                && self.plan.execution[0].api_worker.is_some()
                && self.plan.management_setup_digest.is_none()
                && self.plan.policy.supervision.is_none()
                && self.plan.execution[0].supervision_override.is_none()
                && self.plan.policy.currency_budget.is_none(),
            "not a single-executor experiment",
        )?;
        let policy = self
            .plan
            .snapshot_policy
            .as_ref()
            .ok_or("baseline requires full input snapshot")?;
        require(
            policy.exclude.iter().all(|p| p == ".git"),
            "baseline input exclusions may only omit Git metadata",
        )?;
        for file in &self.seed.files {
            artifact_path(&file.path)?;
            sha256(&file.sha256)?;
        }
        Ok(())
    }
    fn config(&self) -> SchedulerConfig {
        SchedulerConfig::single_executor(ExecutionScope {
            project: self.plan.project.clone(),
            policy_version: self.plan.policy.version,
        })
    }
    fn check_seed(&self, root: &Path) -> Result<(), String> {
        require(
            observe(
                root,
                self.plan
                    .snapshot_policy
                    .as_ref()
                    .ok_or("snapshot policy missing")?,
            )? == self.seed,
            "initial file tree differs from frozen baseline seed",
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunApproval {
    pub spec_digest: String,
    pub approval: Approval,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Prepared {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    material_proof: Option<crate::baseline_materials::MaterialProof>,
    spec_digest: String,
    data_root: String,
    ledger_head: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Started {
    spec_digest: String,
    prepared_head: String,
    approval: Approval,
    started_at: String,
    transport: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Terminal {
    pub spec_digest: String,
    pub start_digest: String,
    pub ledger_head: String,
    pub revision_digest: String,
    pub task_state: TaskState,
    pub elapsed_ms: u64,
    pub finished_at: String,
    pub accepted: bool,
    pub error: Option<String>,
}
struct Session {
    root: PathBuf,
    _lock: File,
    spec: RunSpec,
    prepared: Prepared,
}
pub(crate) fn prospective(path: &Path) -> Result<PathBuf, String> {
    require(
        path.is_absolute()
            && !path
                .components()
                .any(|p| matches!(p, std::path::Component::ParentDir)),
        "baseline root must be absolute without traversal",
    )?;
    let parent = root_path(
        path.parent()
            .and_then(Path::to_str)
            .ok_or("invalid baseline parent")?,
    )?;
    Ok(parent.join(path.file_name().ok_or("baseline root name missing")?))
}
fn empty_management(root: &Path) -> Result<(), String> {
    crate::host::ordinary(&root.join("management-decisions"), true)?;
    require(
        fs::read_dir(root.join("management-decisions"))
            .map_err(|e| e.to_string())?
            .next()
            .transpose()
            .map_err(|e| e.to_string())?
            .is_none(),
        "baseline management inventory must remain empty",
    )
}
fn lock(root: &Path, create: bool) -> Result<File, String> {
    if !create {
        crate::host::ordinary(&root.join("writer.lock"), false)?;
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(create)
        .open(root.join("writer.lock"))
        .map_err(|e| e.to_string())?;
    file.try_lock_exclusive()
        .map_err(|_| "baseline root writer is active")?;
    Ok(file)
}
impl Session {
    fn open(root: &Path, expected: &str) -> Result<Self, String> {
        sha256(expected)?;
        let root = root_path(root.to_str().ok_or("non-Unicode baseline root")?)?;
        let lock = lock(&root, false)?;
        let spec: RunSpec = load(&root, "run-spec.json")?;
        spec.validate()?;
        let prepared: Prepared = load(&root, "prepared.json")?;
        require(
            (spec.schema == 1 && prepared.material_proof.is_none())
                || (spec.schema == 2 && prepared.material_proof.is_some()),
            "prepared material proof missing or unexpected",
        )?;
        require(
            digest(&spec)? == expected
                && prepared.spec_digest == expected
                && prepared.data_root == root.to_string_lossy()
                && prospective(Path::new(&spec.data_root))? == root,
            "baseline selection changed",
        )?;
        empty_management(&root)?;
        Ok(Self {
            root,
            _lock: lock,
            spec,
            prepared,
        })
    }
    fn workflow(&self) -> PathBuf {
        self.root.join("workflows").join(&self.spec.plan.project.id)
    }
    fn fresh(&self) -> Result<(), String> {
        for name in [
            "started.json",
            "started.json.pending",
            "terminal.json",
            "terminal.json.pending",
        ] {
            require(
                !self
                    .root
                    .join(name)
                    .try_exists()
                    .map_err(|e| e.to_string())?,
                "baseline intent already exists; inspect original run, never retry",
            )?;
        }
        if self.spec.schema == 2 {
            require(
                self.prepared.material_proof.as_ref()
                    == Some(&crate::baseline_materials::verify(&self.spec)?),
                "prepared material proof changed",
            )?;
        }
        DurableWorkflow::inspect_existing(&self.workflow(), |store| {
            require(
                store.configuration() == &self.spec.config()
                    && store.history().len() == 1
                    && store.history()[0].hash == self.prepared.ledger_head
                    && store.revision() == Some(&self.spec.plan)
                    && !store.state().approved,
                "baseline is no longer an unapproved preparation",
            )
        })?;
        self.spec
            .check_seed(Path::new(&self.spec.plan.project.root))
    }
}

fn absent(path: &Path, message: &str) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
        Ok(_) => Err(message.into()),
    }
}

// Only the new root selected by this prepare operation is eligible. Existing
// roots, including interrupted preparations, never enter the promotion path.
fn relay_archive(root: &Path, relay: bool) -> Result<Option<PathBuf>, String> {
    if !relay {
        return Ok(None);
    }
    require(
        cfg!(windows),
        "relay baseline preparation requires Windows cold-backup protection",
    )?;
    let name = root
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or("invalid baseline root name")?;
    let archive = prospective(&root.with_file_name(format!("{name}.reader16-cold-backup")))?;
    absent(
        &archive,
        "baseline relay cold-backup destination already exists; preserve original materials",
    )?;
    Ok(Some(archive))
}

#[cfg(windows)]
fn require_empty_relay_backup(archive: &Path, expected: &str) -> Result<(), String> {
    use crate::backup::Entry;
    use sha2::Digest;
    let (manifest, reviewed, _tree, _archive_pin, _manifest_pin) =
        crate::backup::verified(archive).map_err(|e| e.to_string())?;
    require(
        reviewed.manifest_digest == expected
            && manifest.entries
                == [
                    Entry {
                        path: "events".into(),
                        directory: true,
                        bytes: 0,
                        sha256: String::new(),
                    },
                    Entry {
                        path: "writer.lock".into(),
                        directory: false,
                        bytes: 0,
                        sha256: format!("{:x}", sha2::Sha256::digest([])),
                    },
                ],
        "baseline relay cold backup is no longer the empty new root; preserve original materials",
    )
}

#[cfg(windows)]
fn lock_promoted_relay_root(root: &Path, expected_guard: &str) -> Result<File, String> {
    use std::collections::BTreeSet;
    sha256(expected_guard)?;
    // The generic promotion APIs own their locks. Reacquire a pinned, ordinary,
    // zero-length lock, then reject any writer's additions in the intervening gap.
    let writer =
        crate::backup::open(&root.join("writer.lock"), false, true).map_err(|e| e.to_string())?;
    let _root_pin = crate::backup::open(root, true, false).map_err(|e| e.to_string())?;
    let names = fs::read_dir(root)
        .map_err(|e| e.to_string())?
        .take(3)
        .map(|entry| {
            entry
                .map(|entry| entry.file_name())
                .map_err(|e| e.to_string())
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    require(
        names
            == ["events".into(), "writer.lock".into()]
                .into_iter()
                .collect(),
        "baseline relay new root changed during protection; preserve original materials",
    )?;
    let events =
        crate::backup::inventory(&root.join("events"), false).map_err(|e| e.to_string())?;
    require(
        events.nodes.len() == 1
            && events.nodes[0].entry.path == crate::relay_guard::FILE_NAME
            && !events.nodes[0].entry.directory
            && events.nodes[0].entry.sha256 == expected_guard,
        "baseline relay new-root guard or event inventory changed; preserve original materials",
    )?;
    crate::relay_guard::require_protected(root, true)?;
    Ok(writer)
}

#[cfg(windows)]
fn protect_new_relay_root(root: &Path, archive: &Path) -> Result<File, String> {
    // There is no new-root guard format. Reuse the actual cold-backup review and
    // promotion path, before writing any relay-bearing specification or event.
    // Failure deliberately retains this new root and every backup/pending file.
    fs::create_dir(root.join("events")).map_err(|e| e.to_string())?;
    drop(lock(root, true)?);
    #[cfg(test)]
    relay_prepare_tests::checkpoint(root, "before-backup")?;
    let backup = crate::backup::create(root, archive)?;
    require_empty_relay_backup(archive, &backup.manifest_digest)?;
    let review = crate::relay_guard::review(root, archive, &backup.manifest_digest)?;
    let promoted =
        crate::relay_guard::promote(root, archive, &backup.manifest_digest, &review.digest)?;
    #[cfg(test)]
    relay_prepare_tests::checkpoint(root, "after-promotion")?;
    lock_promoted_relay_root(
        root,
        promoted
            .reader_guard_digest
            .as_deref()
            .ok_or("new relay guard digest missing")?,
    )
}

#[cfg(not(windows))]
fn protect_new_relay_root(_: &Path, _: &Path) -> Result<File, String> {
    Err("relay baseline preparation requires Windows cold-backup protection".into())
}

/// Creates a new isolated data root. The project already exists and must match
/// the full frozen input tree. This does not approve or dispatch anything.
pub fn prepare(root: &Path, spec: RunSpec) -> Result<Value, String> {
    spec.validate()?;
    let material_proof = if spec.schema == 2 {
        Some(crate::baseline_materials::verify(&spec)?)
    } else {
        None
    };
    let project = root_path(&spec.plan.project.root)?;
    spec.check_seed(&project)?;
    require(
        root.is_absolute()
            && !root.exists()
            && !root
                .components()
                .any(|p| matches!(p, std::path::Component::ParentDir)),
        "baseline data root must be new, absolute and normalized",
    )?;
    let root = prospective(root)?;
    require(
        root == prospective(Path::new(&spec.data_root))?,
        "baseline spec selects another data root",
    )?;
    require(
        !contains_path(&root, &project) && !contains_path(&project, &root),
        "baseline data and project roots must not overlap",
    )?;
    absent(
        &root,
        "baseline data root must be new; preserve original preparation",
    )?;
    let archive = relay_archive(&root, crate::relay_guard::revision_uses_relay(&spec.plan))?;
    if let Some(archive) = &archive {
        require(
            !contains_path(archive, &project) && !contains_path(&project, archive),
            "baseline relay backup and project roots must not overlap",
        )?;
    }
    #[cfg(windows)]
    let _parent_pin =
        crate::backup::open(root.parent().ok_or("baseline parent missing")?, true, false)
            .map_err(|e| e.to_string())?;
    fs::create_dir(&root).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    let _root_pin = crate::backup::open(&root, true, false).map_err(|e| e.to_string())?;
    let _lock = match archive.as_deref() {
        Some(archive) => protect_new_relay_root(&root, archive)?,
        None => lock(&root, true)?,
    };
    publish(&root, "run-spec.json", &spec)?;
    fs::create_dir(root.join("workflows")).map_err(|e| e.to_string())?;
    // An actual empty original inventory allows the existing management reader
    // to distinguish zero records from a missing directory.
    crate::management_execution::DecisionJournal::open(&root)?;
    let mut store = DurableWorkflow::open(
        &root.join("workflows").join(&spec.plan.project.id),
        spec.config(),
    )?;
    let entry = store.command(
        "baseline-propose",
        "user",
        WorkflowCommand::Propose {
            plan: spec.plan.workflow.clone(),
        },
        CommandContext {
            execution: Some(ExecutionEvidence::Revision {
                revision: spec.plan.clone(),
            }),
            ..Default::default()
        },
        true,
    )?;
    let ready = Prepared {
        material_proof,
        spec_digest: digest(&spec)?,
        data_root: root.to_string_lossy().into(),
        ledger_head: entry.hash,
    };
    publish(&root, "prepared.json", &ready)?;
    Ok(
        json!({"specDigest":ready.spec_digest,"revisionDigest":spec.plan.approval_digest()?,
        "prepared":true,"authorized":false,"dispatched":false}),
    )
}

/// Explicit trusted local-user invocation only. The credential directory is
/// selected by the caller; keys are never copied to the experiment root.
pub async fn run_from_vault(
    root: &Path,
    expected: &str,
    approval: RunApproval,
    credential_root: &Path,
    cancel: &mut oneshot::Receiver<()>,
    verification_cancel: &AtomicBool,
) -> Result<Terminal, String> {
    execute(
        root,
        expected,
        approval,
        "production-api",
        |spec| {
            ApiClient::new(crate::credential_vault::load_for(
                credential_root,
                spec.plan.execution[0]
                    .api_worker
                    .as_ref()
                    .unwrap()
                    .target
                    .provider(),
            )?)
        },
        &mut async |client: &ApiClient, prepared, cancel: &mut oneshot::Receiver<()>, gateway: &mut cerulean_adapters::api_generation::GatewayObserver<'_>| {
            client.send_with_gateway(prepared, cancel, gateway).await
        },
        cancel,
        verification_cancel,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn execute(
    root: &Path,
    expected: &str,
    approval: RunApproval,
    transport: &str,
    make_client: impl FnOnce(&RunSpec) -> Result<ApiClient, String>,
    send: &mut impl AsyncFnMut(
        &ApiClient,
        PreparedRequest,
        &mut oneshot::Receiver<()>,
        &mut cerulean_adapters::api_generation::GatewayObserver<'_>,
    ) -> Result<ApiReply, ApiFailure>,
    cancel: &mut oneshot::Receiver<()>,
    verification_cancel: &AtomicBool,
) -> Result<Terminal, String> {
    execute_observed(
        root,
        expected,
        approval,
        transport,
        make_client,
        send,
        cancel,
        verification_cancel,
        |_| Ok(()),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn execute_observed(
    root: &Path,
    expected: &str,
    approval: RunApproval,
    transport: &str,
    make_client: impl FnOnce(&RunSpec) -> Result<ApiClient, String>,
    send: &mut impl AsyncFnMut(
        &ApiClient,
        PreparedRequest,
        &mut oneshot::Receiver<()>,
        &mut cerulean_adapters::api_generation::GatewayObserver<'_>,
    ) -> Result<ApiReply, ApiFailure>,
    cancel: &mut oneshot::Receiver<()>,
    verification_cancel: &AtomicBool,
    on_started: impl FnOnce(&str) -> Result<(), String>,
) -> Result<Terminal, String> {
    let session = Session::open(root, expected)?;
    session.fresh()?;
    let spec = &session.spec;
    if transport == "production-api" {
        require(
            spec.schema == 2
                && session
                    .prepared
                    .material_proof
                    .as_ref()
                    .is_some_and(|p| p.declared_execution_kind == "real-attempt"),
            "production execution requires bound real-attempt materials",
        )?;
    }
    require(
        approval.spec_digest == expected,
        "approval selects another baseline spec",
    )?;
    approval
        .approval
        .validate(&spec.plan, "user", spec.plan.policy.version)?;
    crate::api_execution::check_cancel(cancel)?;
    let clock = Instant::now();
    let started = Started {
        spec_digest: expected.into(),
        prepared_head: session.prepared.ledger_head.clone(),
        approval: approval.approval.clone(),
        started_at: chrono::Utc::now().to_rfc3339(),
        transport: transport.into(),
    };
    // Publication precedes credentials, workspace preparation and all requests.
    // Even a truncated intent blocks another execution.
    publish(&session.root, "started.json", &started)?;
    let mut store = DurableWorkflow::open(&session.workflow(), spec.config())?;
    let task = &spec.plan.workflow.tasks[0].id;
    let attempt_id = "baseline-attempt";
    let outcome: Result<(), String> = async {
        // The original start is durable; telemetry failure stops before credentials or requests.
        on_started(&digest(&started)?)?;
        let client = make_client(spec)?;
        let approval_digest = store
            .state()
            .digest
            .clone()
            .ok_or("workflow digest missing")?;
        store.command(
            "baseline-approve",
            "user",
            WorkflowCommand::Approve {
                digest: approval_digest,
            },
            CommandContext {
                approval_source: Some("explicit local-user baseline invocation".into()),
                execution: Some(ExecutionEvidence::Approval {
                    approval: approval.approval,
                }),
                ..Default::default()
            },
            true,
        )?;
        let (workspace, proof) = if spec.plan.worktree.is_some() {
            let (workspace, proof) = store.prepare_task_workspace(task, attempt_id)?;
            (workspace, Some(proof))
        } else {
            (spec.plan.project.root.clone(), None)
        };
        spec.check_seed(Path::new(&workspace))?;
        let identity = format!("baseline-{}", &expected[..24]);
        let attempt = Attempt {
            id: attempt_id.into(),
            task_id: task.clone(),
            worker_id: "worker".into(),
            revision_digest: spec.plan.approval_digest()?,
            fence: 1,
            workspace_proof: proof,
            binding: SessionBinding {
                adapter_id: spec.plan.execution[0].adapter_id.clone(),
                adapter_version: "api-worker-v1".into(),
                session_id: identity.clone(),
                workspace,
                identity_strength: IdentityStrength::ControllerRequest(ControllerRequestIdentity {
                    connection_id: identity.clone(),
                    request_id: format!("request-{identity}"),
                }),
            },
        };
        store.command(
            "baseline-assign",
            "core",
            WorkflowCommand::Assign {
                task: task.clone(),
                worker: "worker".into(),
                attempt: attempt_id.into(),
            },
            CommandContext {
                execution: Some(ExecutionEvidence::Attempt { attempt }),
                ..Default::default()
            },
            true,
        )?;
        crate::api_execution::dispatch_inner(
                &mut store,
                &client,
                task,
                attempt_id,
                cancel,
                &mut async |prepared,
                            cancellation: &mut oneshot::Receiver<()>,
                            gateway: &mut cerulean_adapters::api_generation::GatewayObserver<
                    '_,
                >| { send(&client, prepared, cancellation, gateway).await },
                &mut |_| Ok(()),
                &mut |_| {},
            )
            .await?;
        crate::api_execution::check_cancel(cancel)?;
        let pins = spec.plan.execution[0]
            .checks
            .iter()
            .map(PinnedCheck::from_approved)
            .collect::<Result<Vec<_>, _>>()?;
        store.run_verification(
            "baseline-verification",
            task,
            attempt_id,
            &pins,
            verification_cancel,
        )?;
        store.command(
            "baseline-accept",
            "core",
            WorkflowCommand::Accept {
                task: task.clone(),
                attempt: attempt_id.into(),
            },
            CommandContext::default(),
            true,
        )?;
        Ok(())
    }
    .await;
    require(
        !store.requires_reopen(),
        "baseline publication uncertain; retain original intent without terminal",
    )?;
    let terminal = Terminal {
        spec_digest: expected.into(),
        start_digest: digest(&started)?,
        ledger_head: store
            .history()
            .last()
            .ok_or("baseline ledger missing")?
            .hash
            .clone(),
        revision_digest: spec.plan.approval_digest()?,
        task_state: store.state().tasks[task].state,
        elapsed_ms: u64::try_from(clock.elapsed().as_millis())
            .map_err(|_| "baseline timing overflow")?,
        finished_at: chrono::Utc::now().to_rfc3339(),
        accepted: outcome.is_ok() && store.state().tasks[task].state == TaskState::Accepted,
        error: outcome.err(),
    };
    publish(&session.root, "terminal.json", &terminal)?;
    Ok(terminal)
}

/// Read-only original evidence. It never opens a mutable workflow, restores,
/// retries, launches verifiers, or reads credentials.
pub fn inspect(root: &Path, expected: &str) -> Result<Value, String> {
    let session = Session::open(root, expected)?;
    let started = if session.root.join("started.json").exists()
        || session.root.join("started.json.pending").exists()
    {
        Some(load::<Started>(&session.root, "started.json")?)
    } else {
        None
    };
    let terminal = if session.root.join("terminal.json").exists()
        || session.root.join("terminal.json.pending").exists()
    {
        Some(load::<Terminal>(&session.root, "terminal.json")?)
    } else {
        None
    };
    let (head, revision, state) =
        DurableWorkflow::inspect_existing(&session.workflow(), |store| {
            require(
                store.configuration() == &session.spec.config()
                    && store.revision() == Some(&session.spec.plan),
                "baseline workflow differs from frozen specification",
            )?;
            if let Some(start) = &started {
                require(
                    start.spec_digest == expected
                        && start.prepared_head == session.prepared.ledger_head,
                    "baseline start scope mismatch",
                )?;
                start.approval.validate(
                    &session.spec.plan,
                    "user",
                    session.spec.plan.policy.version,
                )?;
                if let Some(approval) = store.approval() {
                    require(
                        approval == &start.approval,
                        "baseline original approval differs",
                    )?;
                }
            } else {
                require(
                    store.history().len() == 1
                        && store.history()[0].hash == session.prepared.ledger_head,
                    "baseline activity has no original start",
                )?;
            }
            let head = store
                .history()
                .last()
                .ok_or("baseline history missing")?
                .hash
                .clone();
            let state = store.state().tasks[&session.spec.plan.workflow.tasks[0].id].state;
            let revision = session.spec.plan.approval_digest()?;
            if let Some(end) = &terminal {
                let start = started.as_ref().ok_or("terminal has no original start")?;
                require(
                    end.spec_digest == expected
                        && end.start_digest == digest(start)?
                        && end.ledger_head == head
                        && end.revision_digest == revision
                        && end.task_state == state
                        && end.accepted == (state == TaskState::Accepted && end.error.is_none()),
                    "baseline terminal differs from original ledger",
                )?;
            }
            Ok((head, revision, state))
        })?;
    let evidence = crate::efficiency_evidence::read_inner(
        &session.root,
        &session.spec.plan.project.id,
        &revision,
        &head,
        true,
    )?;
    require(
        evidence["management"]["operations"]
            .as_array()
            .is_some_and(Vec::is_empty)
            && evidence["management"]["reviewedFragments"]
                .as_array()
                .is_some_and(Vec::is_empty),
        "baseline contains unexpected management evidence",
    )?;
    let mut result = json!({"schema":1,"kind":"baseline-original-run","specDigest":expected,
        "slotId":session.spec.slot_id,"materialManifestSha256":session.spec.material_manifest_sha256,
        "configurationSha256":session.spec.configuration_sha256,
        "status":if terminal.is_some() {"terminal"} else if started.is_some() {"started-without-terminal"} else {"prepared"},
        "taskState":state,"started":started,"terminal":terminal,"coreExport":evidence,
        "managementModelRequests":0,"realityAttested":false,"experimentRecordReady":false,"releaseAccepted":false});
    if let Some(proof) = session.prepared.material_proof {
        result["materialProofAtPreparation"] =
            serde_json::to_value(proof).map_err(|e| e.to_string())?;
    }
    Ok(result)
}

#[cfg(test)]
mod coverage_252;
#[cfg(test)]
mod material_tests;
#[cfg(all(test, windows))]
mod relay_prepare_tests;
#[cfg(test)]
mod tests;
pub mod timing;
