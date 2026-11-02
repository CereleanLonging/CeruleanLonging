//! Real management journal, background writer and Node checks; synthetic provider/UI transports.
use super::*;
#[path = "desktop_observation_context_tests.rs"]
mod observation_context;
#[path = "workbuddy_product_live.rs"]
mod product_live;
use cerulean_adapters::openai::DecisionObservation;
use cerulean_domain::{
    management::{DecisionEnvelope, ManagementDecision, ProposedTask, Purpose},
    project_setup::RunState,
};

fn manager(i: crate::management_execution::DecisionIntent) -> Result<DecisionObservation, String> {
    let tasks = &i.context["workflow"]["state"]["tasks"];
    let requirement = i.context["requirement"].as_str().unwrap_or_default();
    let decision = match i.purpose {
        Purpose::Plan => {
            let mut tasks = vec![ProposedTask {
                profile_id: None,
                id: "task".into(),
                goal: format!("Implement sum. {requirement}"),
                dependencies: vec![],
                worker_id: "worker-qoder".into(),
                validator_id: "node-sum".into(),
            }];
            if requirement.contains("TWO_TASKS") || requirement.contains("THREE_TASKS") {
                tasks.push(ProposedTask {
                    profile_id: None,
                    id: "second".into(),
                    goal: "Second task after acceptance".into(),
                    dependencies: vec!["task".into()],
                    worker_id: "worker-qoder".into(),
                    validator_id: "node-sum".into(),
                });
            }
            if requirement.contains("THREE_TASKS") {
                tasks.push(ProposedTask {
                    profile_id: None,
                    id: "third".into(),
                    goal: "Third task after acceptance".into(),
                    dependencies: vec!["second".into()],
                    worker_id: "worker-qoder".into(),
                    validator_id: "node-sum".into(),
                });
            }
            ManagementDecision::Plan {
                tasks,
                rationale: "Approved files and pinned independent Node validator".into(),
            }
        }
        Purpose::Assign => {
            let task = if tasks["second"]["state"] == "accepted" {
                "third"
            } else if tasks["task"]["state"] == "accepted" {
                "second"
            } else {
                "task"
            };
            assert_eq!(tasks[task]["state"], "pending");
            ManagementDecision::Assign {
                task_id: task.into(),
                worker_id: "worker-qoder".into(),
                rationale: "Approved desktop identity".into(),
            }
        }
        Purpose::Accept => {
            let task = ["task", "second", "third"]
                .into_iter()
                .find(|t| tasks[*t]["state"] == "submitted")
                .unwrap();
            assert_eq!(tasks[task]["verified"], true);
            ManagementDecision::Accept {
                task_id: task.into(),
                attempt_id: tasks[task]["attempt"].as_str().unwrap().into(),
                rationale: "Independent Node checks passed".into(),
            }
        }
        Purpose::Advise => return Err("Unexpected supervision".into()),
    };
    Ok(DecisionObservation {
        response_id: format!("resp_{}", i.operation_id.replace('-', "_")),
        requested_model: i.model.clone(),
        actual_model: i.model,
        input_tokens: 1,
        output_tokens: 1,
        envelope: DecisionEnvelope {
            state_digest: i.state_digest,
            decision,
        },
    })
}
fn worker(request: &ReadRequest) -> Result<ReadReply, String> {
    match request {
        ReadRequest::TaskPrepare {
            previous: Some(previous),
            ..
        } => {
            let mut observed = previous.observation.clone();
            observed.captured_at = chrono::Utc::now().to_rfc3339();
            observed.content_digest = digest(&previous.prompt).unwrap();
            observed.layout_digest = digest(&previous.id).unwrap();
            let mut baseline = previous.baseline.clone();
            baseline.push(task_proof(previous).round_sha256.unwrap());
            Ok(ReadReply {
                observation: Some(observed),
                baseline,
                ..Default::default()
            })
        }
        ReadRequest::TaskSend { review, .. } => {
            let project = Path::new(&review.attempt.binding.workspace);
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(project.join(format!("sent-once-{}", review.attempt.task_id)))
                .map_err(|e| e.to_string())?;
            fs::write(
                project.join("sum.mjs"),
                if review.prompt.contains("FAIL_OUTPUT") {
                    "export function sum(){return 0;}\n"
                } else {
                    "export function sum(a,b){return a+b;}\n"
                },
            )
            .unwrap();
            if review.prompt.contains("LOST_REPLY") {
                return Err("Injected lost reply".into());
            }
            Ok(ReadReply {
                task_proof: Some({
                    let mut p = task_proof(review);
                    if review.prompt.contains("LEGACY_PROOF") {
                        p.round_sha256 = None;
                    }
                    if review.prompt.contains("MISSING_LOCAL_PROOF") {
                        p.local_binding = None;
                    }
                    if review.prompt.contains("FOREIGN_LOCAL_REVIEW") {
                        p.local_binding.as_mut().unwrap().task_review_digest = "f".repeat(64);
                    }
                    if review.prompt.contains("INVENTED_PROVIDER_ID") {
                        p.session_id = Some("invented-provider-id".into());
                    }
                    p
                }),
                ..Default::default()
            })
        }
        _ => background_reply(request),
    }
}
fn configured(root: &Path, mode: &str) -> RuntimeHost {
    configured_for(root, mode, DesktopApp::Qoder)
}
fn configured_for(root: &Path, mode: &str, app: DesktopApp) -> RuntimeHost {
    configured_for_version(root, mode, app, None)
}
fn configured_for_version(
    root: &Path,
    mode: &str,
    app: DesktopApp,
    version: Option<&str>,
) -> RuntimeHost {
    let (mut host, mut draft) = desktop_plan_fixture(root, app.clone());
    let b = &mut draft.desktop_worker.as_mut().unwrap().binding;
    b.observation.window.version = version
        .unwrap_or(match app {
            DesktopApp::Qoder => "0.4.3.0",
            DesktopApp::Workbuddy => "5.7.7.0",
        })
        .into();
    b.id = binding_digest(b).unwrap();
    persist(&host.root, b).unwrap();
    host.desktop_access.bindings.push(b.clone());
    configured_draft(host, draft, mode)
}
fn configured_draft(
    mut host: RuntimeHost,
    mut draft: cerulean_domain::project_setup::ProjectSetupDraft,
    mode: &str,
) -> RuntimeHost {
    let node = std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|p| p.join("node.exe"))
        .find(|p| p.is_file())
        .expect("Node required");
    host.register_validator(
        "node-sum".into(),
        "Independent sum behavior".into(),
        node.to_string_lossy().into(),
        vec![],
        Default::default(),
    )
    .unwrap();
    draft.requirement = format!("Implement sum in sum.mjs. {mode}");
    draft.validator_id = "node-sum".into();
    draft.management_max_requests = 10;
    draft.worker_max_requests = 3;
    draft.check_args=vec!["--input-type=module".into(),"--eval".into(),"import assert from 'node:assert/strict'; import {sum} from './sum.mjs'; assert.equal(sum(2,3),5); assert.equal(sum(-7,4),-3); assert.equal(sum(0,0),0); console.log('desktop managed independent checks passed');".into()];
    host.configure_project(draft).unwrap();
    host.set_openai_key(Some(
        secret::SecretInput::new("sk-synthetic-desktop-management-only".into()).unwrap(),
    ))
    .unwrap();
    attach(&mut host);
    host
}
fn attach(host: &mut RuntimeHost) {
    host.management_fixture = Some(
        if host.project_setups["project"].setup.worker_adapter
            == cerulean_domain::project_setup::ProjectWorkerAdapter::Workbuddy
        {
            workbuddy_manager
        } else {
            manager
        },
    );
    host.desktop_fixture = Some(worker);
}
fn workbuddy_manager(
    i: crate::management_execution::DecisionIntent,
) -> Result<DecisionObservation, String> {
    let mut response = manager(i)?;
    match &mut response.envelope.decision {
        ManagementDecision::Plan { tasks, .. } => {
            for task in tasks {
                task.worker_id = "worker-workbuddy".into();
            }
        }
        ManagementDecision::Assign { worker_id, .. } => *worker_id = "worker-workbuddy".into(),
        _ => {}
    }
    Ok(response)
}
fn until(host: &mut RuntimeHost, done: impl Fn(&RuntimeHost) -> bool) {
    let start = Instant::now();
    loop {
        host.poll_verifications();
        if done(host) {
            return;
        }
        assert!(
            start.elapsed() < Duration::from_secs(40),
            "{:?} {:?}",
            host.runs.all(),
            host.desktop_access.error
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn plan_and_approve(host: &mut RuntimeHost) {
    let setup = host.project_setups["project"].digest.clone();
    host.request_chief_plan("project", &setup).unwrap();
    until(host, |h| h.projects["project"].revision().is_some());
    assert!(host.desktop_access.tasks.is_empty());
    let revision = host.projects["project"]
        .revision()
        .unwrap()
        .approval_digest()
        .unwrap();
    host.approve("project", &revision).unwrap();
}
fn start(host: &mut RuntimeHost) {
    let revision = host.projects["project"]
        .revision()
        .unwrap()
        .approval_digest()
        .unwrap();
    let setup = host.project_setups["project"].digest.clone();
    host.start_project_run("project", &revision, &setup)
        .unwrap();
}
fn assigned(host: &mut RuntimeHost) -> String {
    until(host, |h| {
        h.projects
            .get("project")
            .is_some_and(|s| s.state().tasks["task"].state == TaskState::Assigned)
    });
    host.projects["project"].state().tasks["task"]
        .attempt
        .clone()
        .unwrap()
}
fn prepared(host: &mut RuntimeHost, attempt: &str) -> String {
    host.prepare_desktop_task("project", "task", attempt)
        .unwrap();
    spin(host);
    host.desktop_access.task_review.as_ref().unwrap().id.clone()
}
#[test]
fn desktop_managed_run_waits_for_user_and_requires_real_verification_before_chief_accepts() {
    managed_run_contract(DesktopApp::Qoder);
}
fn managed_run_contract(app: DesktopApp) {
    managed_run_version_contract(app, None);
}
fn managed_run_version_contract(app: DesktopApp, version: Option<&str>) {
    for mode in ["correct", "FAIL_OUTPUT", "TWO_TASKS", "THREE_TASKS"] {
        let root = tempfile::tempdir().unwrap();
        let mut host = configured_for_version(root.path(), mode, app.clone(), version);
        plan_and_approve(&mut host);
        start(&mut host);
        let attempt = assigned(&mut host);
        for _ in 0..12 {
            host.poll_verifications();
        }
        assert_eq!(host.management.statuses().len(), 2);
        assert!(!root.path().join("project/sent-once-task").exists());
        let review = prepared(&mut host, &attempt);
        host.send_desktop_task(&review).unwrap();
        let count = if mode == "THREE_TASKS" {
            3
        } else if mode == "TWO_TASKS" {
            2
        } else {
            1
        };
        let mut prior_id = review;
        for task in ["second", "third"].into_iter().take(count - 1) {
            until(&mut host, |h| {
                h.projects
                    .get("project")
                    .is_some_and(|s| s.state().tasks[task].state == TaskState::Assigned)
            });
            let attempt = host.projects["project"].state().tasks[task]
                .attempt
                .clone()
                .unwrap();
            host.prepare_desktop_task("project", task, &attempt)
                .unwrap();
            spin(&mut host);
            let next = host
                .desktop_access
                .task_review
                .clone()
                .expect("continuation review");
            assert_eq!(
                next.continuation.as_ref().unwrap().previous_review_digest,
                prior_id
            );
            prior_id = next.id.clone();
            host.send_desktop_task(&next.id).unwrap();
        }
        until(&mut host, |h| {
            h.runs
                .all()
                .iter()
                .any(|r| matches!(r.state, RunState::Completed | RunState::Attention))
        });
        if mode == "FAIL_OUTPUT" {
            let state = &host.projects["project"].state().tasks["task"];
            assert_eq!(state.state, TaskState::Submitted);
            assert!(!state.verified);
            assert_eq!(host.management.statuses().len(), 2);
            let prior = &host.desktop_access.tasks[0].review;
            assert!(crate::desktop_execution::predecessor(
                &host.root,
                &host.projects["project"],
                &prior.binding_id
            )
            .unwrap_err()
            .contains("尚未独立验收"));
        } else {
            assert!(host.projects["project"]
                .state()
                .tasks
                .values()
                .all(|t| t.state == TaskState::Accepted && t.verified));
            assert_eq!(host.management.statuses().len(), 1 + 2 * count);
        }
        assert_eq!(host.jobs.len(), count);
        assert!(host.desktop_access.tasks[0].submitted);
        if app == DesktopApp::Workbuddy {
            for task in &host.desktop_access.tasks {
                let proof = task.proof.as_ref().unwrap();
                assert!(proof.session_id.is_none());
                assert_eq!(
                    proof.local_binding.as_ref().unwrap().binding_id,
                    task.review.binding_id
                );
                assert_eq!(
                    task.review.attempt.binding.session_id,
                    format!("ui-binding-{}", task.review.binding_id)
                );
            }
        }
        assert!(host.api_jobs.is_empty() && host.codex_jobs.is_empty() && host.omp_jobs.is_empty());
        let history = host.projects["project"].history().to_vec();
        let decisions = host.management.statuses().len();
        drop(host);
        let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
        attach(&mut host);
        host.poll_verifications();
        assert_eq!(host.projects["project"].history(), history);
        assert_eq!(host.management.statuses().len(), decisions);
    }
}
#[test]
fn desktop_managed_pause_and_assigned_restart_never_send_until_user_confirms() {
    pause_restart_contract(DesktopApp::Qoder);
}
fn pause_restart_contract(app: DesktopApp) {
    pause_restart_version_contract(app, None);
}
fn pause_restart_version_contract(app: DesktopApp, version: Option<&str>) {
    let root = tempfile::tempdir().unwrap();
    let mut host = configured_for_version(root.path(), "correct", app, version);
    plan_and_approve(&mut host);
    start(&mut host);
    let attempt = assigned(&mut host);
    let review = prepared(&mut host, &attempt);
    let run = host.runs.all()[0].id.clone();
    host.pause_project_run(&run).unwrap();
    assert!(host
        .send_desktop_task(&review)
        .unwrap_err()
        .contains("暂停"));
    assert!(host.desktop_access.tasks.is_empty());
    drop(host);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    attach(&mut host);
    host.poll_verifications();
    assert_eq!(host.runs.all()[0].state, RunState::Paused);
    assert!(host.desktop_access.task_review.is_none());
    assert!(!root.path().join("project/sent-once-task").exists());
    start(&mut host);
    for _ in 0..12 {
        host.poll_verifications();
    }
    assert_eq!(host.management.statuses().len(), 2);
    let review = prepared(&mut host, &attempt);
    host.send_desktop_task(&review).unwrap();
    until(&mut host, |h| h.runs.all()[0].state == RunState::Completed);
}
#[test]
fn desktop_managed_unknown_requires_original_readback_and_explicit_run_resume() {
    unknown_readback_contract(DesktopApp::Qoder);
}
fn unknown_readback_contract(app: DesktopApp) {
    unknown_readback_version_contract(app, None);
}
fn unknown_readback_version_contract(app: DesktopApp, version: Option<&str>) {
    let root = tempfile::tempdir().unwrap();
    let mut host = configured_for_version(root.path(), "LOST_REPLY", app, version);
    plan_and_approve(&mut host);
    start(&mut host);
    let attempt = assigned(&mut host);
    let review = prepared(&mut host, &attempt);
    host.send_desktop_task(&review).unwrap();
    until(&mut host, |h| h.runs.all()[0].state == RunState::Attention);
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Unknown
    );
    assert_eq!(host.management.statuses().len(), 2);
    assert!(host.jobs.is_empty());
    let revision = host.projects["project"]
        .revision()
        .unwrap()
        .approval_digest()
        .unwrap();
    let setup = host.project_setups["project"].digest.clone();
    assert!(host
        .start_project_run("project", &revision, &setup)
        .is_err());
    drop(host);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    attach(&mut host);
    host.read_desktop_task("project", &attempt).unwrap();
    background_spin(&mut host);
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Submitted
    );
    assert!(host.jobs.is_empty());
    assert_eq!(host.runs.all()[0].state, RunState::Attention);
    start(&mut host);
    until(&mut host, |h| h.runs.all()[0].state == RunState::Completed);
    assert_eq!(host.management.statuses().len(), 3);
}

#[test]
fn desktop_managed_missing_chief_credentials_cannot_record_run_or_bill_assignment() {
    let root = tempfile::tempdir().unwrap();
    let mut host = configured(root.path(), "correct");
    plan_and_approve(&mut host);
    host.set_openai_key(None).unwrap();
    let revision = host.projects["project"]
        .revision()
        .unwrap()
        .approval_digest()
        .unwrap();
    let setup = host.project_setups["project"].digest.clone();
    assert!(host
        .start_project_run("project", &revision, &setup)
        .is_err());
    assert!(host.runs.all().is_empty());
    assert_eq!(host.management.statuses().len(), 1);
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Pending
    );
    assert!(host.desktop_access.tasks.is_empty());
}
#[test]
fn desktop_continuation_rejects_changed_history_identity_and_legacy_proofs() {
    continuation_contract(DesktopApp::Qoder, &["TWO_TASKS", "TWO_TASKS LEGACY_PROOF"]);
}
fn continuation_contract(app: DesktopApp, modes: &[&str]) {
    use crate::desktop_execution as execution;
    for mode in modes {
        let root = tempfile::tempdir().unwrap();
        let mut host = configured_for(root.path(), mode, app.clone());
        plan_and_approve(&mut host);
        start(&mut host);
        let attempt = assigned(&mut host);
        let id = prepared(&mut host, &attempt);
        host.send_desktop_task(&id).unwrap();
        until(&mut host, |h| {
            h.projects
                .get("project")
                .is_some_and(|s| s.state().tasks["task"].state == TaskState::Accepted)
        });
        if mode.contains("LEGACY") {
            until(&mut host, |h| h.runs.all()[0].state == RunState::Attention);
            assert_eq!(host.management.statuses().len(), 3);
            assert_eq!(
                host.projects["project"].state().tasks["second"].state,
                TaskState::Pending
            );
            assert!(host.runs.all()[0].message.contains("整轮消息摘要"));
            let old = &host.desktop_access.tasks[0];
            let json = serde_json::to_value(old.proof.as_ref().unwrap()).unwrap();
            assert!(json.get("roundSha256").is_none());
            let review_json = serde_json::to_value(&old.review).unwrap();
            assert!(review_json.get("continuation").is_none());
            let bytes = serde_json::to_vec(&old.review).unwrap();
            assert_eq!(
                serde_json::to_vec(
                    &serde_json::from_slice::<execution::TaskReview>(&bytes).unwrap()
                )
                .unwrap(),
                bytes
            );
            drop(host);
            RuntimeHost::open(&root.path().join("data")).unwrap();
            continue;
        }
        until(&mut host, |h| {
            h.projects["project"].state().tasks["second"].state == TaskState::Assigned
        });
        let prior = host.desktop_access.tasks[0].review.clone();
        let mut reply = worker(&ReadRequest::TaskPrepare {
            version: 1,
            window: prior.observation.window.clone(),
            previous: Some(prior.clone()),
        })
        .unwrap();
        let good = reply.observation.take().unwrap();
        let next = host.projects["project"].state().tasks["second"]
            .attempt
            .clone()
            .unwrap();
        let store = &host.projects["project"];
        for fault in [
            "reply", "history", "short", "model", "session", "document", "window", "time", "draft",
            "busy",
        ] {
            let mut observation = good.clone();
            let mut baseline = reply.baseline.clone();
            match fault {
                "reply" => *baseline.last_mut().unwrap() = "f".repeat(64),
                "history" => baseline[1] = "f".repeat(64),
                "short" => baseline = baseline[baseline.len() - 2..].to_vec(),
                "model" => observation.model_label = Some("different-model".into()),
                "session" => observation.session_id = Some("different-chat".into()),
                "document" => observation.document_runtime_id.push_str(".9"),
                "window" => observation.window.pid += 1,
                "time" => observation.captured_at = prior.observation.captured_at.clone(),
                "draft" => observation.draft_empty = false,
                "busy" => observation.busy = true,
                _ => unreachable!(),
            }
            assert!(
                execution::prepare_continuing(
                    &host.root,
                    store,
                    "second",
                    &next,
                    observation,
                    baseline
                )
                .is_err(),
                "{fault}"
            );
        }
        let valid = execution::prepare_continuing(
            &host.root,
            store,
            "second",
            &next,
            good.clone(),
            reply.baseline[1..].to_vec(),
        )
        .unwrap();
        assert_eq!(
            valid.continuation.as_ref().unwrap().previous_review_digest,
            prior.id
        );
        for fault in ["attempt", "digest", "missing", "baseline"] {
            let mut forged = valid.clone();
            match fault {
                "attempt" => {
                    forged.continuation.as_mut().unwrap().previous_attempt_id =
                        "foreign-attempt".into()
                }
                "digest" => {
                    forged.continuation.as_mut().unwrap().previous_review_digest = "f".repeat(64)
                }
                "baseline" => forged.baseline[0] = "f".repeat(64),
                "missing" => {
                    forged.continuation = None;
                    let at = forged.observation.captured_at.clone();
                    forged.observation = prior.observation.clone();
                    forged.observation.captured_at = at;
                }
                _ => unreachable!(),
            }
            forged.id.clear();
            forged.id = digest(&forged).unwrap();
            assert!(
                execution::reserve(
                    &host.root,
                    host.projects.get_mut("project").unwrap(),
                    &forged
                )
                .is_err(),
                "{fault}"
            );
        }
        let store = &host.projects["project"];
        assert!(execution::prepare(store, "second", &next, good, reply.baseline).is_err());
        assert_eq!(execution::recorded_attempts(&host.root).unwrap().len(), 1);
        let dir = host
            .root
            .join("desktop-task-dispatch-v1")
            .join(digest(&("project", &prior.attempt.id)).unwrap());
        let mut proof: serde_json::Value =
            serde_json::from_slice(&fs::read(dir.join("proof.json")).unwrap()).unwrap();
        proof["roundSha256"] = "f".repeat(64).into();
        fs::write(dir.join("proof.json"), serde_json::to_vec(&proof).unwrap()).unwrap();
        assert!(execution::predecessor(&host.root, store, &prior.binding_id).is_err());
        drop(host);
        assert!(RuntimeHost::open(&root.path().join("data")).is_err());
    }
}

#[test]
#[ignore = "native desktop managed run fixture; synthetic OpenAI/UI responses and actual Node validation"]
fn native_desktop_managed_server() {
    serve_native("TWO_TASKS");
}
#[test]
#[ignore = "native unknown desktop recovery/disposition; synthetic transports"]
fn native_desktop_recovery_server() {
    serve_native("LOST_REPLY");
}
#[test]
#[ignore = "native product cancellation UI/IPC with synthetic desktop and management transports"]
fn native_desktop_cancellation_server() {
    serve_native("CANCEL_TASK");
}
fn cancellation_native_worker(request: &ReadRequest) -> Result<ReadReply, String> {
    if let ReadRequest::TaskSend { review, .. } = request {
        let project = Path::new(&review.attempt.binding.workspace);
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(project.join(format!("sent-once-{}", review.attempt.id)))
            .map_err(|e| e.to_string())?;
        fs::write(
            project.join("sum.mjs"),
            "export function sum(a,b){return a+b;}\n",
        )
        .map_err(|e| e.to_string())?;
        let root = project.parent().ok_or("fixture root missing")?;
        let request_path = root
            .join("data/desktop-task-dispatch-v1")
            .join(digest(&(&review.project_id, &review.attempt.id))?)
            .join("stop-request.json");
        let start = Instant::now();
        while !request_path.exists() {
            require(
                start.elapsed() < Duration::from_secs(60),
                "fixture stop request timed out",
            )?;
            std::thread::sleep(Duration::from_millis(10));
        }
        return Err("synthetic observer released after durable user request".into());
    }
    super::cancellation_tests::stop_fixture(request)
}
fn serve_native(mode: &str) {
    let root = fs::canonicalize(std::env::var_os("CERULEAN_DESKTOP_NATIVE_DIR").unwrap()).unwrap();
    let reports =
        fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports")).unwrap();
    assert!(root.starts_with(reports));
    let mut host = if std::env::var("CERULEAN_DESKTOP_NATIVE_REOPEN").as_deref() == Ok("1") {
        assert!(["LOST_REPLY", "CANCEL_TASK"].contains(&mode));
        let mut host = RuntimeHost::open(&root.join("data")).unwrap();
        attach(&mut host);
        host
    } else {
        assert!(!root.join("data").exists());
        configured(&root, mode)
    };
    if mode == "CANCEL_TASK" {
        host.desktop_fixture = Some(cancellation_native_worker);
    }
    let data = root.join("data");
    fs::write(
        root.join("fixture.json"),
        serde_json::to_vec(&serde_json::json!({"data":data,"pid":std::process::id()})).unwrap(),
    )
    .unwrap();
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(crate::ipc::serve_host(&data, host))
        .unwrap();
}

#[path = "desktop_disposition_tests.rs"]
mod disposition_tests;

#[path = "desktop_joint_native_tests.rs"]
mod joint_native;
