//! An explicit revised synthetic plan using an already provisioned identity.
//! The failed attempt and its checker are immutable; no UAC or provider calls.
use super::*;
use cerulean_domain::project_setup::{AcceptanceDraft, AcceptanceStageDraft, ProjectSetupDraft};
use std::io::Write;

fn revised_manager(
    i: crate::management_execution::DecisionIntent,
) -> Result<DecisionObservation, String> {
    let mut observation = manager_reviewing_materials(i)?;
    if let ManagementDecision::Plan { tasks, .. } = &mut observation.envelope.decision {
        tasks[0].goal = "DAG_A: read and validate the existing alpha file. ROLE_ISOLATION; do not write or delete it.".into();
    }
    Ok(observation)
}

#[test]
fn historical_failed_verification_does_not_stop_new_api_attempt() {
    let root = tempfile::tempdir().unwrap();
    let mut host = configured(
        root.path(),
        ProjectWorkerAdapter::Openai,
        ApiTarget::OpenAi {},
        "success",
        8,
    );
    approved(&mut host);
    // Model the retained job projection from an earlier approved version. The
    // same task ID is intentionally reused; the current attempt will differ.
    let old = VerificationJob {
        project_id: "project".into(),
        operation_id: "historical-failed-check".into(),
        task_id: "a".into(),
        attempt_id: "historical-attempt".into(),
        state: VerificationState::Failed,
        started_at: "2026-11-03T00:00:00Z".into(),
        updated_at: "2026-11-03T00:00:00Z".into(),
        message: None,
        reconciliation_digest: None,
    };
    host.jobs.insert(
        (old.project_id.clone(), old.operation_id.clone()),
        old.clone(),
    );
    start(&mut host);
    spin(&mut host, |h| {
        h.runs.all()[0].state != RunState::Active && h.api_active.is_empty()
    });
    assert_eq!(host.runs.all()[0].state, RunState::Attention);
    assert_eq!(
        host.jobs[&(old.project_id.clone(), old.operation_id.clone())],
        old
    );
    assert_eq!(
        host.projects["project"].state().tasks["a"].state,
        TaskState::Submitted
    );
    assert_eq!(
        host.projects["project"].state().tasks["b"].state,
        TaskState::Pending
    );
    assert_eq!(host.api_jobs.len(), 1);
    assert_eq!(
        fs::read(project_planning::tests::worker_root(root.path()).join("result.txt")).unwrap(),
        b"alpha"
    );
}

fn revised_worker(prepared: &PreparedRequest, round: u32) -> Result<ApiReply, ApiFailure> {
    let messages = fixture_messages(prepared);
    let ApiMessage::User(prompt) = &messages[0] else {
        panic!("missing prompt")
    };
    if prompt.contains("DAG_B") {
        return worker(prepared, round);
    }
    // This revision consumes the preserved alpha file. It does not repeat the
    // failed plan's original write or delete its output to make a fresh test pass.
    assert!(prompt.contains("DAG_A"));
    let output = if round == 1 {
        json!([{"type":"function_call","call_id":"read-existing-alpha","name":"cerulean_read_file","arguments":json!({"path":"result.txt","offset":0}).to_string()}])
    } else {
        assert_eq!(round, 2);
        let ApiMessage::Tools(results) = messages.last().unwrap() else {
            panic!("read missing")
        };
        assert!(!results[0].is_error);
        let read: serde_json::Value = serde_json::from_str(&results[0].text).unwrap();
        assert_eq!(read["content"], "alpha");
        json!([{"type":"message","id":"read-done","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Existing alpha independently read."}]}])
    };
    fixture_reply(prepared, json!({"id":format!("revised-{round}"),"model":"fixture","status":"completed","output":output,"usage":{"input_tokens":10,"output_tokens":10}}))
        .map_err(|_| ApiFailure::InvalidResponse {})
}

#[test]
#[ignore = "explicit existing failed native fixture; new reviewed plan, no UAC or network; runs once"]
fn revised_native_plan_preserves_failure_and_completes_blind_review() {
    let root = PathBuf::from(std::env::var_os("BLIND_REVIEW_HOST_NATIVE_ROOT").unwrap())
        .canonicalize()
        .unwrap();
    assert!(root.starts_with(Path::new(r"D:\test").canonicalize().unwrap()));
    let marker = root.join("native-revision-771.json");
    let mut marker = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(marker)
        .unwrap();
    marker.write_all(br#"{"purpose":"correct native cwd contract; preserve failed plan","liveProviderRequests":0,"uac":false}"#).unwrap();
    marker.sync_all().unwrap();
    let mut host = RuntimeHost::open(&root.join("data")).unwrap();
    assert_eq!(
        host.projects["project"]
            .revision()
            .unwrap()
            .workflow
            .version,
        1
    );
    assert_eq!(host.jobs.len(), 1);
    let failed = host.jobs.values().next().unwrap().clone();
    assert_eq!(failed.state, VerificationState::Failed);
    let failure_path = root
        .join("data/workflows/project/verification")
        .join(&failed.operation_id)
        .join("result.json");
    let failure_bytes = fs::read(&failure_path).unwrap();
    let script_bytes = fs::read(root.join("assertions.ps1")).unwrap();
    let history = host.projects["project"].history().to_vec();
    let old = host.project_setups["project"].setup.clone();
    assert!(old.profiles.is_empty() && old.worktree.is_none());
    let worker_model = old.api_worker.as_ref().unwrap().model.clone();
    let project = host.projects["project"].revision().unwrap().project.clone();
    let binding = old.worker_directories.values().next().unwrap();
    let account =
        crate::windows_worker_account::load_binding(&host.root, &project, binding).unwrap();
    assert_eq!(
        fs::read(Path::new(&account.record.workspace).join("result.txt")).unwrap(),
        b"alpha"
    );
    drop(account);
    let script = root.join("assertions-native-cwd-771.ps1");
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&script)
        .unwrap()
        .write_all(ROLE_ASSERTION_SCRIPT.as_bytes())
        .unwrap();
    let shell = PathBuf::from(std::env::var_os("SystemRoot").unwrap())
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let validator_id = "assertion-native-cwd";
    host.register_validator(
        validator_id.into(),
        "Native cwd assertion report".into(),
        shell.to_string_lossy().into(),
        vec![script.to_string_lossy().into()],
        Default::default(),
    )
    .unwrap();
    let args: Vec<String> = vec![
        "-NoProfile".into(),
        "-NonInteractive".into(),
        "-ExecutionPolicy".into(),
        "Bypass".into(),
        "-File".into(),
        crate::windows_worker_account::dos_path(&script)
            .unwrap()
            .to_string_lossy()
            .into(),
    ];
    let review = host
        .project_revision_candidates()
        .into_iter()
        .find(|r| r.project_id == "project")
        .unwrap();
    assert_eq!(review.submitted_tasks, 1);
    host.retire_submitted_plan("project", &review.evidence_digest, "Synthetic fixture correction: preserve original failure; use native cwd under private ACLs and consume the existing alpha without repeating its write").unwrap();
    host.configure_project(ProjectSetupDraft {
        worker_reasoning_effort: None,
        chief_directory: old.chief_directory,
        management_sources: old.management_sources,
        currency_budget: None,
        resource_keys: old.resource_keys,
        profiles: vec![], worktree: None, desktop_worker: None,
        api_worker: old.api_worker,
        omp_runtime_parent: None,
        worker_adapter: old.worker_adapter,
        supervision: old.policy.supervision,
        project_id: "project".into(),
        requirement: "ROLE_ISOLATION revised native cwd check. DAG_A reads the existing alpha; DAG_B extends to alpha beta. Preserve the failed original plan.".into(),
        codex_executable: String::new(), worker_model,
        chief_model: old.chief_model, deputy_model: old.deputy_model,
        validator_id: validator_id.into(), check_args: args.clone(),
        acceptance: Some(AcceptanceDraft { schema_version:1, stages:vec![AcceptanceStageDraft { id:"assertions".into(),name:"Artifact content".into(),validator_id:validator_id.into(),args,timeout_seconds:10,expected_assertions:vec!["artifact-content".into()] }] }),
        artifact_paths: old.artifact_paths,
        worker_max_requests: old.policy.max_requests,
        management_max_requests: old.management_max_requests,
        max_seconds: old.policy.max_seconds,
    }).unwrap();
    host.management_fixture = Some(revised_manager);
    host.api_fixture = Some(revised_worker);
    let setup = host.project_setups["project"].digest.clone();
    host.request_chief_plan("project", &setup).unwrap();
    spin(&mut host, |h| {
        h.projects["project"].revision().unwrap().workflow.version == 2
    });
    let digest = host.projects["project"]
        .revision()
        .unwrap()
        .approval_digest()
        .unwrap();
    host.approve("project", &digest).unwrap();
    start(&mut host);
    spin(&mut host, |h| {
        h.runs
            .all()
            .iter()
            .find(|r| r.revision_digest == digest)
            .is_some_and(|r| r.state != RunState::Active)
            && h.api_active.is_empty()
    });
    let run = host
        .runs
        .all()
        .into_iter()
        .find(|r| r.revision_digest == digest)
        .unwrap();
    assert_eq!(run.state, RunState::Completed, "{:?}", host.jobs);
    assert_eq!(
        host.jobs[&(failed.project_id.clone(), failed.operation_id.clone())],
        failed
    );
    assert_eq!(
        &host.projects["project"].history()[..history.len()],
        history.as_slice()
    );
    assert_eq!(fs::read(&failure_path).unwrap(), failure_bytes);
    assert_eq!(fs::read(root.join("assertions.ps1")).unwrap(), script_bytes);
    complete_native_blind_review_and_boundaries(&root, host);
    let receipt = json!({"passed":true,"originalFailureUnchanged":true,"oldCheckerUnchanged":true,"newPlanVersion":2,"liveProviderRequests":0,"uac":false});
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(root.join("native-revision-771-result.json"))
        .unwrap()
        .write_all(serde_json::to_string_pretty(&receipt).unwrap().as_bytes())
        .unwrap();
}

#[test]
#[ignore = "existing native revision stopped before dispatch; reconcile and close unsent attempt once, no UAC/network"]
fn reconciled_unsent_native_plan_completes_blind_review() {
    let root = PathBuf::from(std::env::var_os("BLIND_REVIEW_HOST_NATIVE_ROOT").unwrap())
        .canonicalize()
        .unwrap();
    assert!(root.starts_with(Path::new(r"D:\test").canonicalize().unwrap()));
    let mut marker = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(root.join("native-reconciled-771.json"))
        .unwrap();
    marker.write_all(br#"{"purpose":"reconcile unsent startup after historical verification guard repair","liveProviderRequests":0,"uac":false}"#).unwrap();
    marker.sync_all().unwrap();
    let mut host = RuntimeHost::open(&root.join("data")).unwrap();
    assert_eq!(
        host.projects["project"]
            .revision()
            .unwrap()
            .workflow
            .version,
        2
    );
    let history = host.projects["project"].history().to_vec();
    let failed = host
        .jobs
        .values()
        .find(|j| j.state == VerificationState::Failed)
        .unwrap()
        .clone();
    let failure_path = root
        .join("data/workflows/project/verification")
        .join(&failed.operation_id)
        .join("result.json");
    let failure = fs::read(&failure_path).unwrap();
    let original_checker = fs::read(root.join("assertions.ps1")).unwrap();
    let review = host
        .snapshot()
        .unsent_assignments
        .into_iter()
        .find(|r| r.project_id == "project" && r.task_id == "a")
        .expect("must prove assigned but never dispatched before closing");
    let abandoned = review.attempt_id.clone();
    assert!(!history.iter().any(|e| matches!(&e.body.event.command, WorkflowCommand::DispatchIntent { attempt, .. } if attempt == &abandoned)));
    host.abandon_unsent_attempt(&review.project_id,&review.task_id,&review.attempt_id,&review.evidence_digest,"Synthetic validation: close the proven-unsent startup; preserve its assignment and authorize a distinct attempt only after scheduler repair").unwrap();
    host.management_fixture = Some(revised_manager);
    host.api_fixture = Some(revised_worker);
    let digest = host.projects["project"]
        .revision()
        .unwrap()
        .approval_digest()
        .unwrap();
    start(&mut host);
    spin(&mut host, |h| {
        h.runs
            .all()
            .iter()
            .find(|r| r.revision_digest == digest)
            .is_some_and(|r| r.state != RunState::Active)
            && h.api_active.is_empty()
    });
    let run = host
        .runs
        .all()
        .into_iter()
        .find(|r| r.revision_digest == digest)
        .unwrap();
    assert_eq!(
        run.state,
        RunState::Completed,
        "{}; jobs={:?}",
        run.message,
        host.jobs
    );
    assert_eq!(
        host.jobs[&(failed.project_id.clone(), failed.operation_id.clone())],
        failed
    );
    assert_eq!(
        &host.projects["project"].history()[..history.len()],
        history.as_slice()
    );
    assert!(!host.projects["project"].history().iter().any(|e|matches!(&e.body.event.command,WorkflowCommand::DispatchIntent {attempt,..} if attempt==&abandoned)));
    assert_eq!(fs::read(&failure_path).unwrap(), failure);
    assert_eq!(
        fs::read(root.join("assertions.ps1")).unwrap(),
        original_checker
    );
    complete_native_blind_review_and_boundaries(&root, host);
    let result = json!({"passed":true,"originalFailureUnchanged":true,"oldCheckerUnchanged":true,"closedAttemptNeverDispatched":abandoned,"newPlanVersion":2,"liveProviderRequests":0,"uac":false});
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(root.join("native-reconciled-771-result.json"))
        .unwrap()
        .write_all(serde_json::to_string_pretty(&result).unwrap().as_bytes())
        .unwrap();
}
