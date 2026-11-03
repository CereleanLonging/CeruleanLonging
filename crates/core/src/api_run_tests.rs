//! Actual scheduler, protected file tools and independent processes; only model transports are synthetic.
use super::*;
use cerulean_adapters::{
    api_generation::{
        fixture_messages, fixture_reply, ApiFailure, ApiMessage, ApiReply, PreparedRequest,
    },
    openai::DecisionObservation,
};
use cerulean_domain::{
    api_worker::ApiTarget,
    credentials::CredentialProvider,
    management::{DecisionEnvelope, ManagementDecision, ProposedTask, Purpose},
    project_setup::{ProjectWorkerAdapter, RunState},
};
use serde_json::json;
use std::time::{Duration, Instant};

#[path = "flexible_management_tests.rs"]
mod flexible_management;
#[path = "api_run_native_continuation_tests.rs"]
mod native_continuation;
#[path = "api_supervision_dag_tests.rs"]
mod supervision_pipeline;
#[path = "api_run_wait_commit_tests.rs"]
mod wait_commit_interleaving;

// PowerShell's provider location can fall back to C:\ under private ancestor
// ACLs even though the native process cwd is correct. Use the launch contract,
// and stop on read errors instead of returning a null assertion detail.
const ROLE_ASSERTION_SCRIPT: &str = r#"$ErrorActionPreference='Stop';$s=[IO.File]::ReadAllText([IO.Path]::Combine([Environment]::CurrentDirectory,'result.txt'));[Console]::Write((@{schemaVersion=1;assertions=@(@{id='artifact-content';passed=($s -eq 'alpha' -or $s -eq 'alpha beta');detail=$s})}|ConvertTo-Json -Compress -Depth 4));exit 0"#;

fn manager(i: crate::management_execution::DecisionIntent) -> Result<DecisionObservation, String> {
    let tasks = &i.context["workflow"]["state"]["tasks"];
    let worker = format!(
        "worker-{}",
        i.context["executionSetup"]["setup"]["workerAdapter"]
            .as_str()
            .ok_or("worker selection missing")?
    );
    let validator = i.context["executionSetup"]["setup"]["validatorId"]
        .as_str()
        .unwrap_or("tests");
    let decision = match i.purpose {
        Purpose::Refine => return Err("Document refinement uses its dedicated fixture".into()),
        Purpose::Plan => ManagementDecision::Plan {
            tasks: vec![
                ProposedTask {
                    profile_id: None,
                    id: "a".into(),
                    goal: format!(
                        "DAG_A: create alpha. {}",
                        i.context["requirement"].as_str().unwrap()
                    ),
                    dependencies: vec![],
                    worker_id: worker.clone(),
                    validator_id: validator.into(),
                },
                ProposedTask {
                    profile_id: None,
                    id: "b".into(),
                    goal: "DAG_B: read alpha then extend to alpha beta".into(),
                    dependencies: vec!["a".into()],
                    worker_id: worker.clone(),
                    validator_id: validator.into(),
                },
            ],
            rationale: "Two dependent file changes with independent validation".into(),
        },
        Purpose::Assign => {
            let task = if tasks["a"]["state"] == "accepted" {
                "b"
            } else {
                "a"
            };
            assert_eq!(tasks[task]["state"], "pending");
            ManagementDecision::Assign {
                task_id: task.into(),
                worker_id: worker,
                rationale: "Ready dependency".into(),
            }
        }
        Purpose::Accept => {
            let task = if tasks["a"]["state"] == "accepted" {
                "b"
            } else {
                "a"
            };
            assert_eq!(tasks[task]["state"], "submitted");
            assert_eq!(tasks[task]["verified"], true);
            ManagementDecision::Accept {
                task_id: task.into(),
                attempt_id: tasks[task]["attempt"].as_str().unwrap().into(),
                rationale: "Independent check passed".into(),
            }
        }
        Purpose::Advise => return Err("API supervision is not enabled".into()),
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
fn worker(prepared: &PreparedRequest, round: u32) -> Result<ApiReply, ApiFailure> {
    let messages = fixture_messages(prepared);
    let ApiMessage::User(prompt) = &messages[0] else {
        panic!("expected approved prompt")
    };
    if prompt.contains("FAIL_PROVIDER") {
        return Err(ApiFailure::Http { status: 503 });
    }
    let b = prompt.contains("DAG_B");
    let call = match (b, round) {
        (false, 1) => Some((
            "cerulean_write_file",
            json!({"path":"result.txt","content":if prompt.contains("FAIL_VALIDATOR"){"invalid"}else{"alpha"},"expectedSha256":"absent"}),
        )),
        (true, 1) => Some((
            "cerulean_read_file",
            json!({"path":"result.txt","offset":0}),
        )),
        (true, 2) => {
            let ApiMessage::Tools(results) = messages.last().unwrap() else {
                panic!("expected actual read result")
            };
            assert!(!results[0].is_error);
            let read: serde_json::Value = serde_json::from_str(&results[0].text).unwrap();
            assert_eq!(read["content"], "alpha");
            Some((
                "cerulean_write_file",
                json!({"path":"result.txt","content":"alpha beta","expectedSha256":read["sha256"]}),
            ))
        }
        _ => None,
    };
    let tool = call.is_some();
    let (name, args) = call.unwrap_or(("", json!({})));
    let target = &prepared.fingerprint().target;
    let value = if matches!(target, ApiTarget::OpenAi {}) {
        json!({"id":format!("resp-{round}"),"model":"fixture","status":"completed","output":if tool{json!([{"type":"function_call","call_id":format!("call-{round}"),"name":name,"arguments":args.to_string()}])}else{json!([{"type":"message","id":"msg-done","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Done."}]}])},"usage":{"input_tokens":10,"output_tokens":10}})
    } else if target.provider() == CredentialProvider::Anthropic {
        json!({"id":format!("msg-{round}"),"type":"message","role":"assistant","model":"fixture","stop_reason":if tool{"tool_use"}else{"end_turn"},"content":if tool{json!([{"type":"tool_use","id":format!("call-{round}"),"name":name,"input":args}])}else{json!([{"type":"text","text":"Done."}])},"usage":{"input_tokens":10,"output_tokens":10}})
    } else {
        json!({"id":format!("chat-{round}"),"model":"fixture","choices":[{"index":0,"finish_reason":if tool{"tool_calls"}else{"stop"},"message":if tool{json!({"role":"assistant","content":null,"tool_calls":[{"id":format!("call-{round}"),"type":"function","function":{"name":name,"arguments":args.to_string()}}]})}else{json!({"role":"assistant","content":"Done."})}}],"usage":{"prompt_tokens":10,"completion_tokens":10}})
    };
    fixture_reply(prepared, value).map_err(|_| ApiFailure::InvalidResponse {})
}
fn attach(host: &mut RuntimeHost) {
    host.management_fixture = Some(manager);
    host.api_fixture = Some(worker);
}
fn manager_reviewing_materials(
    i: crate::management_execution::DecisionIntent,
) -> Result<DecisionObservation, String> {
    assert!(i.context["roleWorkspace"]["material"]["requirement"]
        .as_str()
        .unwrap()
        .contains("ROLE_ISOLATION"));
    assert_eq!(i.context["roleWorkspace"]["deputyChiefAccess"], "read-only");
    if i.purpose == Purpose::Accept {
        let submissions = i.context["chiefReview"]["packet"]["submissions"]
            .as_array()
            .unwrap();
        assert!(!submissions.is_empty());
        for s in submissions {
            let expected = if s["taskId"] == "a" {
                "alpha"
            } else {
                "alpha beta"
            };
            assert_eq!(s["artifacts"][0]["content"], expected);
            assert_eq!(
                s["verification"]["checks"][0]["assertions"][0]["id"],
                "artifact-content"
            );
            assert_eq!(
                s["verification"]["checks"][0]["assertions"][0]["passed"],
                true
            );
        }
    }
    manager(i)
}
#[test]
fn role_isolated_api_dag_missing_checker_account_stops_without_acceptance() {
    let root = tempfile::tempdir().unwrap();
    let mut host = configured(
        root.path(),
        ProjectWorkerAdapter::Openai,
        ApiTarget::OpenAi {},
        "ROLE_ISOLATION",
        8,
    );
    host.management_fixture = Some(manager_reviewing_materials);
    approved(&mut host);
    start(&mut host);
    spin(&mut host, |h| {
        h.runs.all()[0].state != RunState::Active && h.api_active.is_empty()
    });
    assert_eq!(host.runs.all()[0].state, RunState::Attention);
    let state = host.projects["project"].state();
    assert_eq!(state.tasks["a"].state, TaskState::Submitted);
    assert!(!state.tasks["a"].verified);
    assert_eq!(state.tasks["b"].state, TaskState::Pending);
    assert!(host
        .jobs
        .values()
        .all(|j| j.state != VerificationState::Succeeded));
    assert!(host
        .blind_review_action(cerulean_domain::blind_review::BlindReviewAction::Prepare {
            project_id: "project".into(),
            redactions: vec![],
            model_aliases: BTreeMap::new()
        })
        .is_err());
    let history = host.projects["project"].history().to_vec();
    let requests = host.management.statuses().len();
    assert_eq!(requests, 2); // Plan and assignment only; no chief acceptance.
    drop(host);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    host.poll_verifications();
    assert_eq!(host.projects["project"].history(), history.as_slice());
    assert_eq!(host.management.statuses().len(), requests);
    assert!(host.snapshot().blind_reviews.is_empty());
}
#[test]
#[ignore = "requires explicit fresh disposable root and one UAC for checker account; synthetic model transports only"]
fn role_isolated_api_dag_supplies_actual_sealed_contents_to_chief_and_reopens() {
    let root = PathBuf::from(
        std::env::var_os("BLIND_REVIEW_HOST_NATIVE_ROOT")
            .expect("explicit disposable root required"),
    )
    .canonicalize()
    .unwrap();
    assert!(
        root.starts_with(Path::new(r"D:\test").canonicalize().unwrap())
            && !root.join("data").exists()
    );
    let mut host = configured(
        &root,
        ProjectWorkerAdapter::Openai,
        ApiTarget::OpenAi {},
        "ROLE_ISOLATION",
        8,
    );
    host.management_fixture = Some(manager_reviewing_materials);
    approved(&mut host);
    let setup = &host.project_setups["project"].setup;
    let project = host.projects["project"].revision().unwrap().project.clone();
    let prepared = crate::windows_worker_account::prepare(
        &host.root,
        &project,
        setup.chief_directory.as_deref().unwrap(),
        &setup.worker_directories,
    )
    .unwrap();
    // This test is ignored by default and may only be explicitly invoked after
    // the operator authorizes this single account-setup attempt. Never retries.
    crate::windows_worker_account::elevate(prepared.plan_path_for_test()).unwrap();
    drop(prepared);
    start(&mut host);
    spin(&mut host, |h| {
        h.runs.all()[0].state != RunState::Active && h.api_active.is_empty()
    });
    if host.runs.all()[0].state != RunState::Completed {
        panic!(
            "role DAG failed; evidence retained at {}: runs={:?}, jobs={:?}",
            root.display(),
            host.runs.all(),
            host.jobs
        );
    }
    complete_native_blind_review_and_boundaries(&root, host);
}
fn complete_native_blind_review_and_boundaries(root: &Path, mut host: RuntimeHost) {
    let setup = &host.project_setups["project"].setup;
    let project = host.projects["project"].revision().unwrap().project.clone();
    let binding = setup.worker_directories.values().next().unwrap();
    let boundary_plan = root.join("boundary-plan.json");
    assert!(!boundary_plan.exists());
    fs::write(&boundary_plan,serde_json::to_vec_pretty(&serde_json::json!({"projectId":"project","dataRoot":host.root,"sharedRoot":project.root,"slots":[{"identity":binding.identity}]})).unwrap()).unwrap();
    let workspace = binding
        .root(
            &host.projects["project"]
                .configuration()
                .execution_scope
                .as_ref()
                .unwrap()
                .project
                .root,
        )
        .unwrap();
    assert_eq!(
        fs::read_to_string(Path::new(&workspace).join("result.txt")).unwrap(),
        "alpha beta"
    );
    assert!(!root.join("project/result.txt").exists());
    let packets =
        fs::read_dir(Path::new(setup.chief_directory.as_deref().unwrap()).join("reviews"))
            .unwrap()
            .map(|e| fs::read_to_string(e.unwrap().path()).unwrap())
            .collect::<Vec<_>>();
    assert_eq!(packets.len(), 2);
    assert!(packets.iter().any(|p| p.contains("\"content\":\"alpha\"")));
    host.blind_fixture = Some(|seat, _, _, material, analysis| {
        assert!(analysis.is_none());
        Ok(blind_review_host::ReviewResponse::Vote(
            cerulean_adapters::blind_review::Observation {
                model: seat.model,
                ballot: cerulean_domain::blind_review::Ballot {
                    vote: cerulean_domain::blind_review::Vote::Pass,
                    citations: vec![material.fragments[0].id.clone()],
                    reason: "Synthetic transport confirms the fixed alpha beta fixture".into(),
                },
            },
        ))
    });
    use cerulean_domain::blind_review::BlindReviewAction;
    host.blind_review_action(BlindReviewAction::Prepare {
        project_id: "project".into(),
        redactions: vec![],
        model_aliases: BTreeMap::new(),
    })
    .unwrap();
    let round = host.snapshot().blind_reviews.into_iter().next().unwrap();
    host.blind_review_action(BlindReviewAction::Start {
        project_id: "project".into(),
        round_id: round.round_id.clone(),
        digest: round.digest.clone(),
    })
    .unwrap();
    spin(&mut host, |h| h.blind_active.is_none());
    host.blind_review_action(BlindReviewAction::Confirm {
        project_id: "project".into(),
        round_id: round.round_id,
        digest: round.digest,
    })
    .unwrap();
    assert!(host.snapshot().blind_reviews[0].final_accepted);
    let history = host.projects["project"].history().to_vec();
    drop(host);
    let host = RuntimeHost::open(&root.join("data")).unwrap();
    assert_eq!(host.projects["project"].history(), history.as_slice());
    assert!(host.snapshot().blind_reviews[0].final_accepted);
    assert!(host.blind_active.is_none());
    drop(host);
    crate::blind_review::tests::native_checker_boundary_for(
        &boundary_plan,
        &root.join("boundary-result.json"),
    );
}
fn configured(
    root: &Path,
    adapter: ProjectWorkerAdapter,
    target: ApiTarget,
    mode: &str,
    budget: u32,
) -> RuntimeHost {
    configured_inputs(root, adapter, target, mode, budget, false)
}
fn configured_inputs(
    root: &Path,
    adapter: ProjectWorkerAdapter,
    target: ApiTarget,
    mode: &str,
    budget: u32,
    isolated: bool,
) -> RuntimeHost {
    api_host_tests::protect_new_api_fixture(root);
    let (mut host, draft) = project_planning::tests::fixture(root);
    let provider = target.provider();
    let mut draft = api_project_tests::draft_for(draft, adapter, target);
    draft.requirement = format!("Two task API DAG {mode}");
    draft.artifact_paths = vec!["result.txt".into()];
    draft.worker_max_requests = budget;
    draft.management_max_requests = 10;
    if mode.starts_with("SUPERVISION") {
        draft.supervision = Some(cerulean_domain::supervision::SupervisionPolicy {
            schema_version: 1,
            repeated_failure_threshold: 2,
            max_advice_per_attempt: 1,
            delivery_enabled: true,
        });
        if mode == "SUPERVISION_BUDGET" {
            draft.management_max_requests = 3;
        }
    }
    draft.check_args = vec![
        "--ignored".into(),
        "--exact".into(),
        "host::api_run_tests::api_dag_validator".into(),
    ];
    if isolated {
        draft.worktree = Some(dependency_host_fixture::configure(root));
    }
    if mode == "ROLE_ISOLATION" {
        let script = root.join("assertions.ps1");
        fs::write(&script, ROLE_ASSERTION_SCRIPT).unwrap();
        let shell = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
            .join("System32/WindowsPowerShell/v1.0/powershell.exe");
        host.register_validator(
            "assertion-check".into(),
            "Bound assertion report".into(),
            shell.to_string_lossy().into(),
            vec![script.to_string_lossy().into()],
            Default::default(),
        )
        .unwrap();
        draft.chief_directory = Some(root.join("chief").to_string_lossy().into());
        draft.validator_id = "assertion-check".into();
        draft.acceptance = Some(cerulean_domain::project_setup::AcceptanceDraft {
            schema_version: 1,
            stages: vec![cerulean_domain::project_setup::AcceptanceStageDraft {
                id: "assertions".into(),
                name: "Artifact content".into(),
                validator_id: "assertion-check".into(),
                args: vec![
                    "-NoProfile".into(),
                    "-NonInteractive".into(),
                    "-ExecutionPolicy".into(),
                    "Bypass".into(),
                    "-File".into(),
                    script.to_string_lossy().into(),
                ],
                timeout_seconds: 10,
                expected_assertions: vec!["artifact-content".into()],
            }],
        });
        draft.check_args = draft.acceptance.as_ref().unwrap().stages[0].args.clone();
    }
    host.configure_project(draft).unwrap();
    host.set_openai_key(Some(
        secret::SecretInput::new("sk-synthetic-api-dag".into()).unwrap(),
    ))
    .unwrap();
    if provider != CredentialProvider::OpenAi {
        host.set_provider_key(
            provider,
            Some(secret::SecretInput::new("sk-synthetic-api-worker".into()).unwrap()),
        )
        .unwrap();
    }
    attach(&mut host);
    host
}
fn spin(host: &mut RuntimeHost, done: impl Fn(&RuntimeHost) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(45);
    loop {
        host.poll_verifications();
        if done(host) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "DAG timeout: {:?}",
            host.runs.all()
        );
        std::thread::sleep(Duration::from_millis(4));
    }
}
fn approved(host: &mut RuntimeHost) {
    let setup = host.project_setups["project"].digest.clone();
    host.request_chief_plan("project", &setup).unwrap();
    spin(host, |h| h.projects["project"].revision().is_some());
    assert_eq!(host.management.statuses().len(), 1);
    assert!(host.api_jobs.is_empty());
    let digest = host.projects["project"]
        .revision()
        .unwrap()
        .approval_digest()
        .unwrap();
    host.approve("project", &digest).unwrap();
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
#[test]
#[ignore = "actual independent validator subprocess for the API DAG test"]
fn api_dag_validator() {
    if Path::new("base.txt").exists() {
        assert_eq!(
            fs::read_to_string("base.txt").unwrap(),
            "committed dependency base\n"
        );
        assert!(!Path::new(".git").exists());
    }
    let text = fs::read_to_string("result.txt").unwrap();
    assert!(
        matches!(text.as_str(), "alpha" | "alpha beta"),
        "artifact format invalid"
    );
}
#[test]
#[ignore = "real Git; eight resident API dependency worktree DAGs, synthetic transports and real verifier"]
fn dependency_worktree_eight_api_routes_complete_and_reopen() {
    for (adapter, target) in api_project_tests::routes() {
        let root = tempfile::Builder::new()
            .prefix("api-dependency-")
            .tempdir_in(
                fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports"))
                    .unwrap(),
            )
            .unwrap();
        let mut host = configured_inputs(root.path(), adapter, target, "success", 8, true);
        approved(&mut host);
        start(&mut host);
        // Attention stops new scheduling immediately; collect the owned worker
        // before inspecting the returned idle ledger or reopening it.
        spin(&mut host, |h| {
            h.runs.all()[0].state != RunState::Active && h.api_active.is_empty()
        });
        assert_eq!(
            host.runs.all()[0].state,
            RunState::Completed,
            "{:?}",
            host.snapshot()
        );
        dependency_host_fixture::assert_delivery(&host, "alpha", "alpha beta");
        let store = &host.projects["project"];
        let accepted = store
            .history()
            .iter()
            .find(|e| matches!(&e.body.event.command,WorkflowCommand::Accept{task,..} if task=="a"))
            .unwrap()
            .body
            .event
            .sequence;
        let assigned = store
            .history()
            .iter()
            .find(|e| matches!(&e.body.event.command,WorkflowCommand::Assign{task,..} if task=="b"))
            .unwrap()
            .body
            .event
            .sequence;
        assert!(accepted < assigned);
        assert_eq!(crate::api_execution::prior_requests(store).unwrap(), 5);
        assert_eq!(host.management.statuses().len(), 5);
        assert_eq!(host.jobs.len(), 2);
        let history = store.history().to_vec();
        drop(host);
        let mut reopened = RuntimeHost::open(&root.path().join("data")).unwrap();
        attach(&mut reopened);
        reopened.poll_verifications();
        assert_eq!(reopened.projects["project"].history(), history.as_slice());
        assert_eq!(reopened.runs.all()[0].state, RunState::Completed);
        dependency_host_fixture::assert_delivery(&reopened, "alpha", "alpha beta");
    }
}
#[test]
fn eight_api_routes_complete_dependent_tasks_only_after_independent_verification_and_chief_acceptance(
) {
    for (adapter, target) in api_project_tests::routes() {
        let root = tempfile::tempdir().unwrap();
        let mut host = configured(root.path(), adapter, target, "success", 8);
        approved(&mut host);
        start(&mut host);
        // Attention stops new scheduling immediately; collect the owned worker
        // before inspecting the returned idle ledger or reopening it.
        spin(&mut host, |h| {
            h.runs.all()[0].state != RunState::Active && h.api_active.is_empty()
        });
        assert_eq!(
            host.runs.all()[0].state,
            RunState::Completed,
            "{:?}",
            host.runs.all()
        );
        assert_eq!(
            fs::read_to_string(root.path().join("project/result.txt")).unwrap(),
            "alpha beta"
        );
        let store = &host.projects["project"];
        assert!(store
            .state()
            .tasks
            .values()
            .all(|t| t.state == TaskState::Accepted));
        assert_eq!(crate::api_execution::prior_requests(store).unwrap(), 5);
        let sequence = |task: &str, kind: &str| {
            store
                .history()
                .iter()
                .find(|e| {
                    e.body.event.command.task() == Some(task)
                        && serde_json::to_value(&e.body.event.command).unwrap()["kind"] == kind
                })
                .unwrap()
                .body
                .event
                .sequence
        };
        for task in ["a", "b"] {
            assert!(sequence(task, "submit") < sequence(task, "verify"));
            assert!(sequence(task, "verify") < sequence(task, "accept"));
        }
        assert!(sequence("a", "accept") < sequence("b", "assign"));
        assert_eq!(host.jobs.len(), 2);
        assert!(host
            .jobs
            .values()
            .all(|j| j.state == VerificationState::Succeeded));
        assert_eq!(host.management.statuses().len(), 5);
        let history = store.history().to_vec();
        drop(host);
        let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
        attach(&mut host);
        host.poll_verifications();
        assert_eq!(host.projects["project"].history(), history.as_slice());
        assert_eq!(host.runs.all()[0].state, RunState::Completed);
        assert!(host.api_active.is_empty());
    }
}
#[test]
fn api_dag_pause_resume_and_reopen_after_first_acceptance_do_not_repeat_original_attempts() {
    let root = tempfile::tempdir().unwrap();
    let (adapter, target) = api_project_tests::routes().remove(2);
    let mut host = configured(root.path(), adapter, target, "pause", 8);
    approved(&mut host);
    start(&mut host);
    spin(&mut host, |h| {
        h.projects
            .get("project")
            .is_some_and(|s| s.state().tasks["a"].state == TaskState::Accepted)
    });
    let run = host.runs.all()[0].clone();
    host.pause_project_run(&run.id).unwrap();
    let before = host.projects["project"].history().to_vec();
    for _ in 0..5 {
        host.poll_verifications();
    }
    assert_eq!(host.projects["project"].history(), before.as_slice());
    assert_eq!(
        host.projects["project"].state().tasks["b"].state,
        TaskState::Pending
    );
    drop(host);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    attach(&mut host);
    assert_eq!(host.runs.all()[0].state, RunState::Paused);
    start(&mut host);
    // Attention stops new scheduling immediately; collect the owned worker
    // before inspecting the returned idle ledger or reopening it.
    spin(&mut host, |h| {
        h.runs.all()[0].state != RunState::Active && h.api_active.is_empty()
    });
    assert_eq!(host.runs.all()[0].state, RunState::Completed);
    assert_eq!(
        &host.projects["project"].history()[..before.len()],
        before.as_slice()
    );
    assert_eq!(host.api_jobs.len(), 2);
}
#[test]
fn api_dag_failure_budget_and_missing_credentials_stop_before_downstream_billing() {
    for (mode, budget, expected_requests) in [
        ("FAIL_PROVIDER", 8, 2),
        ("FAIL_VALIDATOR", 8, 2),
        ("budget", 2, 3),
    ] {
        let root = tempfile::tempdir().unwrap();
        let (adapter, target) = api_project_tests::routes().remove(2);
        let mut host = configured(root.path(), adapter, target, mode, budget);
        approved(&mut host);
        start(&mut host);
        // Attention stops new scheduling immediately; collect the owned worker
        // before inspecting the returned idle ledger or reopening it.
        spin(&mut host, |h| {
            h.runs.all()[0].state != RunState::Active && h.api_active.is_empty()
        });
        assert_eq!(host.runs.all()[0].state, RunState::Attention);
        assert_eq!(host.management.statuses().len(), expected_requests);
        assert_eq!(
            host.projects["project"].state().tasks["b"].state,
            TaskState::Pending
        );
        let count = host.projects["project"].history().len();
        for _ in 0..5 {
            host.poll_verifications();
        }
        assert_eq!(host.projects["project"].history().len(), count);
    }
    let root = tempfile::tempdir().unwrap();
    let (adapter, target) = api_project_tests::routes().remove(2);
    let mut host = configured(root.path(), adapter, target, "missing", 8);
    approved(&mut host);
    host.set_provider_key(CredentialProvider::DeepSeek, None)
        .unwrap();
    let setup = host.project_setups["project"].digest.clone();
    let revision = host.projects["project"]
        .revision()
        .unwrap()
        .approval_digest()
        .unwrap();
    assert!(host
        .start_project_run("project", &revision, &setup)
        .unwrap_err()
        .contains("deepseek"));
    assert!(host.runs.all().is_empty());
    assert_eq!(host.management.statuses().len(), 1);
}
#[test]
#[ignore = "real Git; dependent API run failures/budget/credentials cannot prepare or bill downstream"]
fn dependency_worktree_api_failure_gates_preserve_pending_downstream() {
    for (mode, budget, requests) in [
        ("FAIL_PROVIDER", 8, 2),
        ("FAIL_VALIDATOR", 8, 2),
        ("budget", 2, 3),
        ("missing", 8, 1),
    ] {
        let root = tempfile::Builder::new()
            .prefix("api-dependency-gate-")
            .tempdir_in(
                fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports"))
                    .unwrap(),
            )
            .unwrap();
        let (adapter, target) = api_project_tests::routes().remove(2);
        let mut host = configured_inputs(root.path(), adapter, target, mode, budget, true);
        approved(&mut host);
        if mode == "missing" {
            host.set_provider_key(CredentialProvider::DeepSeek, None)
                .unwrap();
            let setup = host.project_setups["project"].digest.clone();
            let revision = host.projects["project"]
                .revision()
                .unwrap()
                .approval_digest()
                .unwrap();
            assert!(host
                .start_project_run("project", &revision, &setup)
                .is_err());
            assert!(host.runs.all().is_empty());
            assert!(!root.path().join("pool/records").exists());
        } else {
            start(&mut host);
            // Attention stops new scheduling immediately; collect the owned worker
            // before inspecting the returned idle ledger or reopening it.
            spin(&mut host, |h| {
                h.runs.all()[0].state != RunState::Active && h.api_active.is_empty()
            });
            assert_eq!(host.runs.all()[0].state, RunState::Attention);
        }
        assert_eq!(host.management.statuses().len(), requests);
        assert_eq!(
            host.projects["project"].state().tasks["b"].state,
            TaskState::Pending
        );
        assert!(host.api_startups.values().all(|s| s.attempt.task_id != "b"));
        let history = host.projects["project"].history().to_vec();
        drop(host);
        let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
        attach(&mut host);
        host.poll_verifications();
        assert_eq!(host.projects["project"].history(), history.as_slice());
        assert!(host.api_startups.values().all(|s| s.attempt.task_id != "b"));
    }
}
#[test]
fn api_io_fault_restart_requires_no_live_owner_or_active_run() {
    let root = tempfile::tempdir().unwrap();
    let (adapter, target) = api_project_tests::routes().remove(2);
    let mut host = configured(root.path(), adapter, target, "recovery", 8);
    approved(&mut host);
    start(&mut host);
    host.api_fault = Some("synthetic disk publication failure".into());
    assert!(!host.can_restart_api_fault());
    let run = host.runs.all()[0].id.clone();
    host.pause_project_run(&run).unwrap();
    assert!(host.can_restart_api_fault());
    let recovery = host.snapshot().api_recovery.unwrap();
    assert!(recovery.can_restart);
    assert!(host.api_active.is_empty());
    let (tx, rx) = std::sync::mpsc::channel();
    host.management_active.insert("pending".into(), rx);
    assert!(!host.can_restart_api_fault());
    drop(tx);
    host.management_active.clear();
    assert!(host.can_restart_api_fault());
    host.api_fault = None;
    assert!(!host.can_restart_api_fault());
}

#[test]
#[ignore = "isolated native UI test server with synthetic model transports"]
fn native_api_dag_server() {
    let root = PathBuf::from(std::env::var_os("CERULEAN_API_DAG_NATIVE_DIR").unwrap());
    assert!(root.is_absolute() && root.is_dir());
    assert!(!root.join("data").exists());
    let (adapter, target) = api_project_tests::routes().remove(2);
    let host = configured(&root, adapter, target, "native", 8);
    let data = root.join("data");
    fs::write(
        root.join("fixture.json"),
        serde_json::to_vec(&json!({"data":data})).unwrap(),
    )
    .unwrap();
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(crate::ipc::serve_host(&data, host))
        .unwrap();
}
