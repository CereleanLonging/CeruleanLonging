//! Explicit one-message native product probe; synthetic management, real desktop.
//! Preparation binds the already verified chat without approval or sending.
use super::*;
use serde_json::{json, Value};
use std::io::{Read, Write};

#[path = "qoder_product_probe.rs"]
mod qoder_probe;

#[derive(Clone, Copy)]
enum ProductProfile {
    Workbuddy,
    QoderQwen,
}
impl ProductProfile {
    fn model(self) -> &'static str {
        match self {
            Self::Workbuddy => "Hy3",
            Self::QoderQwen => "Qwen-3.8-Flash, 中",
        }
    }
    fn worker(self) -> &'static str {
        match self {
            Self::Workbuddy => "worker-workbuddy",
            Self::QoderQwen => "worker-qoder",
        }
    }
    fn app(self) -> DesktopApp {
        match self {
            Self::Workbuddy => DesktopApp::Workbuddy,
            Self::QoderQwen => DesktopApp::Qoder,
        }
    }
}

const GOAL: &str = "仅在任务指定的隔离 projectDirectory 内创建 sum.mjs，导出 function sum(a,b)。两个参数均为有限 number 时返回 a+b，否则抛出 TypeError。只写这一份文件，不修改其他文件，不运行命令、不联网、不读凭证；需要工作目录或工具授权时请停止并说明。完成后用简短中文说明并按消息要求输出关联标记。";
const REFERENCE: &str = "reports/workbuddy-task-read-200/prepare-live-read-owned/stdout.json";

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn hash(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    let mut file = fs::File::open(path).unwrap();
    let mut h = Sha256::new();
    let mut bytes = [0; 65536];
    loop {
        let n = file.read(&mut bytes).unwrap();
        if n == 0 {
            break;
        }
        h.update(&bytes[..n]);
    }
    format!("{:x}", h.finalize())
}
fn save(path: &Path, value: &impl serde::Serialize) {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    file.write_all(&serde_json::to_vec_pretty(value).unwrap())
        .unwrap();
    file.sync_all().unwrap();
}
fn root() -> PathBuf {
    owned_root("CERULEAN_WORKBUDDY_PRODUCT")
}
fn owned_root(prefix: &str) -> PathBuf {
    assert_eq!(std::env::var(format!("{prefix}_OWNED")).as_deref(), Ok("1"));
    let mut in_job = 0;
    assert_ne!(
        unsafe {
            windows_sys::Win32::System::JobObjects::IsProcessInJob(
                windows_sys::Win32::System::Threading::GetCurrentProcess(),
                std::ptr::null_mut(),
                &mut in_job,
            )
        },
        0
    );
    assert_ne!(in_job, 0);
    let root = PathBuf::from(std::env::var_os(format!("{prefix}_DIR")).unwrap())
        .canonicalize()
        .unwrap();
    let reports = repo().join("reports").canonicalize().unwrap();
    assert!(root.starts_with(&reports) && root != reports);
    for parent in root.ancestors() {
        ordinary(parent, true).unwrap();
    }
    root
}
fn original() -> (DesktopObservation, Vec<String>) {
    let v: Value = serde_json::from_slice(&fs::read(repo().join(REFERENCE)).unwrap()).unwrap();
    (
        serde_json::from_value(v["native"]["observation"].clone()).unwrap(),
        serde_json::from_value(v["native"]["baseline"].clone()).unwrap(),
    )
}
fn original_view(o: &DesktopObservation, baseline: &[String]) -> Result<(), String> {
    let (expected, history) = original();
    check_view(&expected, &history, o, baseline)
}
fn check_view(
    expected: &DesktopObservation,
    history: &[String],
    o: &DesktopObservation,
    baseline: &[String],
) -> Result<(), String> {
    check_profile_view(ProductProfile::Workbuddy, expected, history, o, baseline)
}
fn check_profile_view(
    profile: ProductProfile,
    expected: &DesktopObservation,
    history: &[String],
    o: &DesktopObservation,
    baseline: &[String],
) -> Result<(), String> {
    require(
        (match profile {
            ProductProfile::QoderQwen => {
                crate::desktop_execution::same_approved_qoder_context(expected, o)
            }
            ProductProfile::Workbuddy => same_view(expected, o),
        }) && history == baseline
            && o.draft_empty
            && !o.busy
            && o.window.app == profile.app()
            && o.model_label.as_deref() == Some(profile.model())
            && match profile {
                ProductProfile::Workbuddy => o.session_id.is_none(),
                ProductProfile::QoderQwen => {
                    o.session_id.as_deref().is_some_and(|id| {
                        id.len() == 36
                            && id.bytes().enumerate().all(|(i, c)| {
                                if [8, 13, 18, 23].contains(&i) {
                                    c == b'-'
                                } else {
                                    c.is_ascii_hexdigit()
                                }
                            })
                    }) && history.len() >= 2
                }
            },
        "Original verified chat, process, model, empty draft or history changed",
    )
}
fn installed() -> Value {
    let v: Value = serde_json::from_slice(
        &fs::read(repo().join("reports/workbuddy-layout-198/installed-after-start.json")).unwrap(),
    )
    .unwrap();
    for row in v.as_array().unwrap() {
        assert_eq!(
            hash(Path::new(row["path"].as_str().unwrap())),
            row["sha256"]
        );
    }
    v
}
fn preflight(host: &RuntimeHost) -> ReadReply {
    assert!(host.desktop_fixture.is_none());
    let (expected, _) = original();
    let read = observe(
        &host.root,
        &ReadRequest::TaskPrepare {
            version: 1,
            window: expected.window,
            previous: None,
        },
    )
    .unwrap();
    original_view(read.observation.as_ref().unwrap(), &read.baseline).unwrap();
    read
}

#[test]
#[ignore = "read-only original WorkBuddy chat; prepare isolated product review, zero messages"]
fn prepare_workbuddy_product() {
    let root = root();
    assert!(fs::read_dir(&root).unwrap().next().is_none());
    let installed = installed();
    let (mut host, draft) = project_planning::tests::fixture(&root);
    assert!(host.desktop_fixture.is_none() && host.management_fixture.is_none());
    let read = preflight(&host);
    save(
        &root.join("preflight.json"),
        &json!({"observation":read.observation,"baseline":read.baseline,"messages":0}),
    );
    host.discover_desktop().unwrap();
    spin(&mut host);
    let (original, _) = original();
    assert!(host.desktop_access.windows.contains(&original.window));
    host.inspect_desktop(original.window).unwrap();
    spin(&mut host);
    let observed = host.desktop_access.observation.as_ref().unwrap();
    assert!(same_view(read.observation.as_ref().unwrap(), observed));
    let digest = host.desktop_access.observation_digest.clone().unwrap();
    host.bind_desktop("project", &digest, "工作台接入验证 / Hy3 / 202")
        .unwrap();
    spin(&mut host);
    assert!(host.desktop_access.error.is_none());
    assert_eq!(host.desktop_access.bindings.len(), 1);
    let (node, verifier) = configure_product(&mut host, draft);
    let revision = host.projects["project"].revision().unwrap();
    let review = json!({"schemaVersion":1,"scope":"real WorkBuddy desktop and RuntimeHost; synthetic chief/deputy; not full three-level acceptance",
        "testExecutableSha256":hash(&std::env::current_exe().unwrap()),"bridgeSha256":fingerprint(),"installed":installed,
        "referenceSha256":hash(&repo().join(REFERENCE)),"nodeSha256":hash(&node),"verifierSha256":hash(&verifier),"codingVerifierSha256":hash(&repo().join("tests/fixtures/workbuddy-coding-verifier.mjs")),
        "revisionDigest":revision.approval_digest().unwrap(),"setupDigest":host.project_setups["project"].digest,"plan":revision,
        "chat":"工作台接入验证","model":"Hy3","maxMessages":1,"maxStopActions":0,"maxAdviceMessages":0,
        "taskMaxSeconds":90,"nativeReplyWaitSeconds":50,"outerTimeoutSeconds":210,"realManagementCalls":0,
        "goal":GOAL,"automaticRetry":false,"workspaceScope":"Requested isolated directory; WorkBuddy tools remain under the external application's permissions, not a per-file ACL.",
        "prompt":"Production TaskReview prompt derived from this exact plan. Attempt ID and CL_TASK marker are generated only after approval; full message saved before the one send.",
        "preparedWithoutApproval":true,"releaseAccepted":false});
    assert!(
        host.projects["project"].approval().is_none()
            && host.runs.all().is_empty()
            && host.desktop_access.tasks.is_empty()
    );
    save(&root.join("review.json"), &review);
    save(
        &root.join("review-digest.json"),
        &json!({"digest":cerulean_domain::execution::digest(&review).unwrap()}),
    );
}

fn configure_product(
    host: &mut RuntimeHost,
    draft: cerulean_domain::project_setup::ProjectSetupDraft,
) -> (PathBuf, PathBuf) {
    configure_profile(ProductProfile::Workbuddy, host, draft)
}
fn configure_profile(
    profile: ProductProfile,
    host: &mut RuntimeHost,
    mut draft: cerulean_domain::project_setup::ProjectSetupDraft,
) -> (PathBuf, PathBuf) {
    let node = PathBuf::from("D:/qqagent/node/node.exe");
    let verifier =
        normal_path(&repo().join("tests/fixtures/workbuddy-product-verifier.mjs")).unwrap();
    let binding = host.desktop_access.bindings[0].clone();
    let project = host.snapshot().workflows[0].project.root.clone();
    draft.worker_adapter = match profile {
        ProductProfile::Workbuddy => {
            cerulean_domain::project_setup::ProjectWorkerAdapter::Workbuddy
        }
        ProductProfile::QoderQwen => cerulean_domain::project_setup::ProjectWorkerAdapter::Qoder,
    };
    draft.codex_executable.clear();
    draft.worker_model = profile.model().into();
    draft.desktop_worker = Some(DesktopWorkerSelection {
        binding,
        project_root: project.clone(),
        model_label: profile.model().into(),
        workspace_confirmed: true,
        external_tools_acknowledged: true,
    });
    host.register_validator(
        "numeric-sum".into(),
        "Independent isolated numeric sum verifier".into(),
        node.to_string_lossy().into(),
        vec![
            verifier.to_string_lossy().into(),
            normal_path(&repo().join("tests/fixtures/workbuddy-coding-verifier.mjs"))
                .unwrap()
                .to_string_lossy()
                .into(),
        ],
        Default::default(),
    )
    .unwrap();
    draft.requirement = GOAL.into();
    draft.validator_id = "numeric-sum".into();
    // Pinned launcher normalizes the inherited sealed working directory before
    // spawning Node with an exact artifact/script read allowlist and a VM.
    draft.check_args = vec![verifier.to_string_lossy().into()];
    draft.worker_max_requests = 1;
    draft.management_max_requests = 3;
    draft.max_seconds = 90;
    host.configure_project(draft).unwrap();
    let mut plan = project_planning::tests::proposal("numeric-sum");
    if let ManagementDecision::Plan { tasks, .. } = &mut plan {
        tasks[0].worker_id = profile.worker().into();
        tasks[0].goal = GOAL.into();
    }
    project_planning::tests::complete(host, plan);
    (node, verifier)
}

fn validate_review(
    root: &Path,
    host: &RuntimeHost,
    review: &Value,
    approved: &str,
) -> Result<(), String> {
    require(
        digest(review)? == approved,
        "Approved scope digest mismatch",
    )?;
    require(
        review["testExecutableSha256"] == hash(&std::env::current_exe().unwrap())
            && review["bridgeSha256"] == fingerprint()
            && review["installed"] == installed()
            && review["referenceSha256"] == hash(&repo().join(REFERENCE))
            && review["nodeSha256"] == hash(Path::new("D:/qqagent/node/node.exe"))
            && review["verifierSha256"]
                == hash(&repo().join("tests/fixtures/workbuddy-product-verifier.mjs"))
            && review["codingVerifierSha256"]
                == hash(&repo().join("tests/fixtures/workbuddy-coding-verifier.mjs")),
        "Frozen executable or evidence changed",
    )?;
    let revision = host.projects["project"].revision().ok_or("Missing plan")?;
    require(
        review["revisionDigest"] == revision.approval_digest()?
            && review["setupDigest"] == host.project_setups["project"].digest,
        "Prepared plan changed",
    )?;
    require(
        host.projects["project"].approval().is_none()
            && host.runs.all().is_empty()
            && host.desktop_access.tasks.is_empty()
            && !root.join("live-intent.json").exists()
            && fs::read_dir(root.join("project"))
                .map_err(|e| e.to_string())?
                .next()
                .is_none(),
        "Already started or project is no longer empty",
    )
}
fn wait_for(
    host: &mut RuntimeHost,
    seconds: u64,
    done: impl Fn(&RuntimeHost) -> bool,
) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(seconds);
    loop {
        host.poll_verifications();
        host.poll_project_runs();
        if done(host) {
            return Ok(());
        }
        require(
            Instant::now() < deadline,
            "Original operation timed out; no retry",
        )?;
        std::thread::sleep(Duration::from_millis(20));
    }
}
struct RemoveSyntheticCredential(PathBuf);
impl Drop for RemoveSyntheticCredential {
    fn drop(&mut self) {
        let _ = crate::credential_vault::remove(&self.0);
    }
}
fn execute(
    host: &mut RuntimeHost,
    root: &Path,
    review: &Value,
    expected: &DesktopObservation,
    baseline: &[String],
) -> Result<(), String> {
    execute_profile(
        ProductProfile::Workbuddy,
        host,
        root,
        review,
        expected,
        baseline,
    )
}
fn execute_profile(
    profile: ProductProfile,
    host: &mut RuntimeHost,
    root: &Path,
    review: &Value,
    expected: &DesktopObservation,
    baseline: &[String],
) -> Result<(), String> {
    host.management_fixture = Some(match profile {
        ProductProfile::Workbuddy => workbuddy_manager,
        ProductProfile::QoderQwen => manager,
    });
    require(
        !crate::credential_vault::exists_for(
            &host.root,
            cerulean_domain::credentials::CredentialProvider::OpenAi,
        )?,
        "Isolated probe must not contain a real credential",
    )?;
    // Existing product startup checks decryptability even for fixture managers.
    // A fixed nonfunctional test string satisfies that local invariant. Neither
    // manager uses a network transport, and no user/provider key is accessed.
    let _cleanup = RemoveSyntheticCredential(host.root.clone());
    host.set_openai_key(Some(secret::SecretInput::new(
        "sk-synthetic-workbuddy-product-not-a-provider-key".into(),
    )?))?;
    let revision = review["revisionDigest"].as_str().unwrap();
    host.approve("project", revision)?;
    host.start_project_run("project", revision, review["setupDigest"].as_str().unwrap())?;
    wait_for(host, 15, |h| {
        h.projects
            .get("project")
            .is_some_and(|p| p.state().tasks["task"].state == TaskState::Assigned)
    })?;
    let attempt = host.projects["project"].state().tasks["task"]
        .attempt
        .clone()
        .unwrap();
    host.prepare_desktop_task("project", "task", &attempt)?;
    wait_for(host, 20, |h| h.desktop_probe.is_none())?;
    let task = host
        .desktop_access
        .task_review
        .clone()
        .ok_or("Task preparation failed; no send")?;
    check_profile_view(
        profile,
        expected,
        baseline,
        &task.observation,
        &task.baseline,
    )?;
    save(&root.join("task-review.json"), &task);
    host.send_desktop_task(&task.id)?; // Sole send call. No recovery branch may call this again.
    wait_for(host, 130, |h| {
        h.desktop_active.is_none()
            && h.active.is_empty()
            && h.management_active.is_empty()
            && h.runs.all().iter().any(|r| r.state != RunState::Active)
    })?;
    let state = &host.projects["project"].state().tasks["task"];
    require(state.state == TaskState::Accepted && state.verified && host.runs.all()[0].state == RunState::Completed,
        "Original task did not reach independently verified Accept; preserve for read-only reconciliation")?;
    require(
        host.desktop_access.tasks.len() == 1
            && host.desktop_access.tasks[0].submitted
            && host.jobs.len() == 1,
        "Expected one original desktop task",
    )?;
    let project = &host.projects["project"].revision().unwrap().project.root;
    let entries: Vec<_> = fs::read_dir(project)
        .map_err(|e| e.to_string())?
        .map(|x| x.unwrap().path())
        .collect();
    require(
        entries.len() == 1 && entries[0].file_name().is_some_and(|x| x == "sum.mjs"),
        "Unexpected project output",
    )?;
    ordinary(&entries[0], false)?;
    require(
        fs::metadata(&entries[0]).map_err(|e| e.to_string())?.len() <= 65536,
        "Artifact too large",
    )?;
    save(
        &root.join("artifact.json"),
        &json!({"path":entries[0],"sha256":hash(&entries[0]),"checks":15,"verified":true}),
    );
    Ok(())
}
#[test]
#[ignore = "REAL WorkBuddy message: explicit frozen digest and owned runner required; no retry"]
fn workbuddy_product_once() {
    let root = root();
    let review: Value =
        serde_json::from_slice(&fs::read(root.join("review.json")).unwrap()).unwrap();
    let mut host = RuntimeHost::open(&root.join("data")).unwrap();
    validate_review(
        &root,
        &host,
        &review,
        &std::env::var("CERULEAN_WORKBUDDY_PRODUCT_APPROVED_DIGEST").unwrap(),
    )
    .unwrap();
    preflight(&host); // Read-only, before consuming the single-use action claim.
    save(
        &root.join("live-intent.json"),
        &json!({"reviewDigest":digest(&review).unwrap(),"at":chrono::Utc::now().to_rfc3339(),"maxMessages":1,"automaticRetry":false}),
    );
    assert!(host.desktop_fixture.is_none());
    let (expected, history) = original();
    let outcome = execute(&mut host, &root, &review, &expected, &history);
    host.set_openai_key(None).unwrap();
    save(&root.join("after-live.json"), &host.snapshot());
    save(
        &root.join("execution-result.json"),
        &json!({"success":outcome.is_ok(),"error":outcome.as_ref().err(),"realManagementCalls":0,"releaseAccepted":false}),
    );
    outcome.unwrap();
    let history = host.projects["project"].history().to_vec();
    let decisions = host.management.statuses().len();
    drop(host);
    for i in 0..2 {
        let mut reopened = RuntimeHost::open(&root.join("data")).unwrap();
        reopened.management_fixture = Some(workbuddy_manager);
        reopened.openai_connection.key_configured = true;
        for _ in 0..5 {
            reopened.poll_verifications();
        }
        assert_eq!(reopened.projects["project"].history(), history);
        assert_eq!(reopened.management.statuses().len(), decisions);
        assert_eq!(reopened.desktop_access.tasks.len(), 1);
        assert!(reopened.desktop_active.is_none() && reopened.desktop_probe.is_none());
        save(&root.join(format!("reopen-{i}.json")), &reopened.snapshot());
    }
    save(
        &root.join("result.json"),
        &json!({"status":"passed","realDesktopMessages":1,"realManagementCalls":0,"reopensWithoutResend":2,"realThreeLevelAcceptance":false,"realCancellationAccepted":false,"releaseAccepted":false}),
    );
}

#[test]
fn product_original_chat_guard_rejects_changed_history_model_and_process() {
    let mut o = observation(window(DesktopApp::Workbuddy));
    o.model_label = Some("Hy3".into());
    let history = vec!["a".repeat(64), "b".repeat(64)];
    let check = |current: &DesktopObservation, baseline: &[String]| {
        check_view(&o, &history, current, baseline)
    };
    check(&o, &history).unwrap();
    let mut altered = o.clone();
    altered.window.pid += 1;
    assert!(check(&altered, &history).is_err());
    altered = o.clone();
    altered.model_label = Some("other-model".into());
    assert!(check(&altered, &history).is_err());
    altered = o.clone();
    altered.draft_empty = false;
    assert!(check(&altered, &history).is_err());
    let mut changed = history.clone();
    changed[0] = "0".repeat(64);
    assert!(check(&o, &changed).is_err());
}

// Exercises the exact new configuration, startup, dispatch and verifier flow
// without any real desktop or provider transport. Covers the missed vault gate.
#[test]
#[ignore = "controlled native product executor, requires local Node; no live desktop or provider"]
fn product_executor_rehearsal_uses_real_host_and_cleans_synthetic_credential() {
    let root = tempfile::Builder::new()
        .prefix("workbuddy-product-rehearsal-")
        .tempdir_in(repo().join("reports"))
        .unwrap()
        .keep()
        .canonicalize()
        .unwrap();
    println!("Controlled rehearsal evidence: {}", root.display());
    let (mut host, draft) = project_planning::tests::fixture(&root);
    host.desktop_fixture = Some(rehearsal_transport);
    host.discover_desktop().unwrap();
    spin(&mut host);
    let w = rehearsal_window();
    host.inspect_desktop(w.clone()).unwrap();
    spin(&mut host);
    let expected = host.desktop_access.observation.clone().unwrap();
    let observed = host.desktop_access.observation_digest.clone().unwrap();
    host.bind_desktop("project", &observed, "Controlled original chat")
        .unwrap();
    spin(&mut host);
    configure_product(&mut host, draft);
    let review = json!({"revisionDigest":host.projects["project"].revision().unwrap().approval_digest().unwrap(),"setupDigest":host.project_setups["project"].digest});
    assert!(!crate::credential_vault::exists_for(
        &host.root,
        cerulean_domain::credentials::CredentialProvider::OpenAi
    )
    .unwrap());
    let outcome = execute(
        &mut host,
        &root,
        &review,
        &expected,
        &["a".repeat(64), "b".repeat(64)],
    );
    save(&root.join("after-rehearsal.json"), &host.snapshot());
    save(
        &root.join("rehearsal-result.json"),
        &json!({"success":outcome.is_ok(),"error":outcome.as_ref().err(),"realMessages":0,"realProviderCalls":0}),
    );
    outcome.unwrap();
    assert!(!crate::credential_vault::exists_for(
        &host.root,
        cerulean_domain::credentials::CredentialProvider::OpenAi
    )
    .unwrap());
    assert!(root.join("controlled-send-once.json").is_file());
    let history = host.projects["project"].history().to_vec();
    let decisions = host.management.statuses().len();
    drop(host);
    for _ in 0..2 {
        let mut host = RuntimeHost::open(&root.join("data")).unwrap();
        host.desktop_fixture = Some(rehearsal_transport);
        host.management_fixture = Some(workbuddy_manager);
        for _ in 0..5 {
            host.poll_verifications();
        }
        assert_eq!(host.projects["project"].history(), history);
        assert_eq!(host.management.statuses().len(), decisions);
        assert!(host.desktop_active.is_none());
    }
}
fn rehearsal_window() -> DesktopWindow {
    let mut w = window(DesktopApp::Workbuddy);
    w.version = "5.8.3.0".into();
    w
}
fn rehearsal_transport(request: &ReadRequest) -> Result<ReadReply, String> {
    match request {
        ReadRequest::Discover { .. } => Ok(ReadReply {
            windows: vec![rehearsal_window()],
            ..Default::default()
        }),
        ReadRequest::Inspect { window, .. }
        | ReadRequest::TaskPrepare {
            window,
            previous: None,
            ..
        } => {
            let mut o = observation(window.clone());
            o.model_label = Some("Hy3".into());
            Ok(ReadReply {
                observation: Some(o),
                baseline: vec!["a".repeat(64), "b".repeat(64)],
                ..Default::default()
            })
        }
        ReadRequest::TaskSend { review, .. } => {
            let project = Path::new(&review.attempt.binding.workspace);
            let mut claim = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(project.parent().unwrap().join("controlled-send-once.json"))
                .map_err(|e| e.to_string())?;
            claim
                .write_all(review.id.as_bytes())
                .map_err(|e| e.to_string())?;
            claim.sync_all().map_err(|e| e.to_string())?;
            fs::write(project.join("sum.mjs"),"export function sum(a,b){if(typeof a!=='number'||typeof b!=='number'||!Number.isFinite(a)||!Number.isFinite(b))throw new TypeError('finite numbers required');return a+b;}\n").map_err(|e|e.to_string())?;
            let mut proof = task_proof(review);
            // The shared fixture drops one history row; this fixture starts
            // with exactly two, which must both be retained by the proof.
            proof.observed_baseline = review.baseline.clone();
            Ok(ReadReply {
                task_proof: Some(proof),
                ..Default::default()
            })
        }
        _ => Err("Controlled rehearsal forbids other desktop operations".into()),
    }
}

#[test]
#[ignore = "Read-only recovery of consumed 203 task; no desktop sends, synthetic management only"]
fn recover_workbuddy_203_original() {
    let evidence = root();
    assert!(fs::read_dir(&evidence).unwrap().next().is_none());
    let original = repo().join("reports/workbuddy-product-203/prepared-visible");
    let review: crate::desktop_execution::TaskReview =
        serde_json::from_slice(&fs::read(original.join("task-review.json")).unwrap()).unwrap();
    assert_eq!(
        review.id,
        "5cad97cbb170f773b7df1b5e92e79d051a17a4deb9b0bab4571f202f7ab89f3b"
    );
    assert_eq!(
        hash(&original.join("project/sum.mjs")),
        "dea927e9c9671aaf69e280bc5b22dc8a263ec06ec36930c2d71b1737090a3df7"
    );
    installed();
    let data = original.join("data");
    let mut host = RuntimeHost::open(&data).unwrap();
    assert!(host.desktop_fixture.is_none());
    host.management_fixture = Some(workbuddy_manager);
    assert_eq!(host.desktop_access.tasks.len(), 1);
    assert_eq!(host.desktop_access.tasks[0].review, review);
    assert!(
        !host.desktop_access.tasks[0].submitted && host.desktop_access.tasks[0].proof.is_none()
    );
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Unknown
    );
    assert_eq!(host.runs.all()[0].state, RunState::Attention);
    let intent_path = data
        .join("desktop-task-dispatch-v1")
        .join(digest(&(&review.project_id, &review.attempt.id)).unwrap())
        .join("intent.json");
    let intent = fs::read(&intent_path).unwrap();
    let decisions_before = host.management.statuses().len();
    save(&evidence.join("before.json"), &host.snapshot());
    save(
        &evidence.join("recovery-intent.json"),
        &json!({"reviewDigest":review.id,"readerSha256":fingerprint(),"messages":0,"automaticRetry":false,"at":chrono::Utc::now().to_rfc3339()}),
    );
    let outcome = (|| -> Result<(), String> {
        host.read_desktop_task_with_reader("project", &review.attempt.id, Some(&fingerprint()))?;
        wait_for(&mut host, 65, |h| h.desktop_active.is_none())?;
        save(&evidence.join("after-read.json"), &host.snapshot());
        require(
            host.desktop_access.tasks[0].submitted,
            "Original read failed; preserve evidence and do not resend",
        )?;
        require(
            host.desktop_access.tasks[0]
                .proof
                .as_ref()
                .is_some_and(|p| p.reader_sha256.as_deref() == Some(fingerprint().as_str())),
            "Current host reader was not recorded",
        )?;
        require(
            fs::read(&intent_path).map_err(|e| e.to_string())? == intent,
            "Original intent changed",
        )?;
        require(
            host.management.statuses().len() == decisions_before,
            "Read-only recovery unexpectedly invoked management",
        )?;
        require(
            !crate::credential_vault::exists_for(
                &data,
                cerulean_domain::credentials::CredentialProvider::OpenAi,
            )?,
            "Isolated data must contain no provider credential",
        )?;
        let _cleanup = RemoveSyntheticCredential(data.clone());
        host.set_openai_key(Some(secret::SecretInput::new(
            "sk-synthetic-workbuddy-product-not-a-provider-key".into(),
        )?))?;
        let revision = host.projects["project"]
            .revision()
            .unwrap()
            .approval_digest()?;
        let setup = host.project_setups["project"].digest.clone();
        host.start_project_run("project", &revision, &setup)?;
        wait_for(&mut host, 100, |h| {
            h.desktop_active.is_none()
                && h.active.is_empty()
                && h.management_active.is_empty()
                && h.runs.all()[0].state != RunState::Active
        })?;
        let task = &host.projects["project"].state().tasks["task"];
        require(
            task.state == TaskState::Accepted
                && task.verified
                && host.runs.all()[0].state == RunState::Completed,
            "Original recovered task did not reach independent Accept",
        )?;
        require(
            host.desktop_access.tasks.len() == 1
                && host.jobs.len() == 1
                && host.management.statuses().len() == decisions_before + 1,
            "Unexpected task or management count",
        )?;
        host.set_openai_key(None)?;
        Ok(())
    })();
    save(&evidence.join("after-recovery.json"), &host.snapshot());
    save(
        &evidence.join("recovery-outcome.json"),
        &json!({"success":outcome.is_ok(),"error":outcome.as_ref().err(),"realDesktopMessages":0,"realManagementCalls":0,"releaseAccepted":false}),
    );
    outcome.unwrap();
    assert!(!crate::credential_vault::exists_for(
        &data,
        cerulean_domain::credentials::CredentialProvider::OpenAi
    )
    .unwrap());
    let history = host.projects["project"].history().to_vec();
    let decisions = host.management.statuses().len();
    drop(host);
    for i in 0..2 {
        let mut reopened = RuntimeHost::open(&data).unwrap();
        reopened.management_fixture = Some(workbuddy_manager);
        for _ in 0..5 {
            reopened.poll_verifications();
            reopened.poll_project_runs();
        }
        assert_eq!(reopened.projects["project"].history(), history);
        assert_eq!(reopened.management.statuses().len(), decisions);
        assert_eq!(reopened.desktop_access.tasks.len(), 1);
        assert!(reopened.desktop_active.is_none() && reopened.desktop_probe.is_none());
        assert_eq!(fs::read(&intent_path).unwrap(), intent);
        save(
            &evidence.join(format!("reopen-{i}.json")),
            &reopened.snapshot(),
        );
    }
    save(
        &evidence.join("result.json"),
        &json!({"status":"passed","originalTaskRecovered":true,"additionalDesktopMessages":0,"realManagementCalls":0,"reopensWithoutResend":2,"realThreeLevelAcceptance":false,"releaseAccepted":false}),
    );
}
