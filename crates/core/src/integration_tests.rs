//! Real Git, resident scheduler, file tools and independent project checks.
//! Only manager and provider responses are synthetic; no network or desktop.
use super::*;
use cerulean_adapters::{
    api_generation::{
        fixture_messages, fixture_reply, ApiFailure, ApiMessage, ApiReply, PreparedRequest,
    },
    openai::DecisionObservation,
};
use cerulean_domain::{
    api_worker::ApiTarget,
    credentials::{CredentialProvider, ProviderEndpoint},
    management::{DecisionEnvelope, ManagementDecision, ProposedTask, Purpose},
    project_setup::{IntegrationDraft, ProjectWorkerAdapter, RunState},
};
use serde_json::json;
use std::time::{Duration, Instant};
#[path = "integration_tests/api_write_scope_251.rs"]
mod api_write_scope_251;
#[path = "integration_tests/baseline_product_256.rs"]
mod baseline_product_256;
#[path = "integration_tests/codex_parallel.rs"]
mod codex_parallel;
#[path = "integration_tests/coverage_252.rs"]
mod coverage_252;
#[path = "integration_tests/deepseek_routing.rs"]
pub(in crate::host) mod deepseek_routing;
#[path = "integration_tests/efficiency_240.rs"]
mod efficiency_240;
#[path = "integration_tests/external_accept.rs"]
mod external_accept;
#[path = "integration_tests/mixed.rs"]
mod mixed;
#[path = "integration_tests/native_lifecycle_260.rs"]
mod native_lifecycle_260;
#[path = "integration_tests/native_team_lifecycle_261.rs"]
mod native_team_lifecycle_261;
#[path = "integration_tests/omp_parallel.rs"]
mod omp_parallel;
#[path = "integration_tests/parallel.rs"]
mod parallel;
#[path = "integration_tests/profiles.rs"]
mod profiles;
#[path = "integration_tests/scoped_currency.rs"]
mod scoped_currency;
#[path = "integration_tests/shared_acceptance.rs"]
mod shared_acceptance;
#[path = "integration_tests/shared_crash.rs"]
mod shared_crash;
#[path = "integration_tests/shared_supervision.rs"]
mod shared_supervision;
#[path = "integration_tests/shared_verification.rs"]
mod shared_verification;
#[path = "integration_tests/team_materials_250.rs"]
mod team_materials_250;
#[path = "integration_tests/team_observation_249.rs"]
mod team_observation_249;
#[path = "integration_tests/team_timing_253.rs"]
mod team_timing_253;
#[path = "integration_tests/team_timing_product_255.rs"]
mod team_timing_product_255;

fn manager(i: crate::management_execution::DecisionIntent) -> Result<DecisionObservation, String> {
    let tasks = &i.context["workflow"]["state"]["tasks"];
    let mode = i.context["executionSetup"]["setup"]["requirement"]
        .as_str()
        .unwrap();
    let decision = if i.purpose == Purpose::Plan {
        ManagementDecision::Plan {
            tasks: ["a", "b", "integration"]
                .iter()
                .map(|id| ProposedTask {
                    profile_id: None,
                    id: (*id).into(),
                    goal: format!("INTEGRATION_{id} {mode}"),
                    dependencies: if *id == "integration" {
                        vec!["a".into(), "b".into()]
                    } else {
                        vec![]
                    },
                    worker_id: "worker-deepseek".into(),
                    validator_id: if *id == "integration" {
                        "whole"
                    } else {
                        "tests"
                    }
                    .into(),
                })
                .collect(),
            rationale: "Two independent branches and an explicit final integration".into(),
        }
    } else {
        let allowed =
            serde_json::from_str::<serde_json::Value>(i.context["requirement"].as_str().unwrap())
                .ok();
        let id = tasks
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .find(|id| match i.purpose {
                Purpose::Assign => {
                    tasks[*id]["state"] == "pending"
                        && i.context["workflow"]["revision"]["workflow"]["tasks"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|t| t["id"] == *id)
                            .unwrap()["dependencies"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .all(|d| tasks[d.as_str().unwrap()]["state"] == "accepted")
                        && allowed
                            .as_ref()
                            .and_then(|v| v["taskCandidates"].as_array())
                            .is_none_or(|ids| ids.iter().any(|v| v == *id))
                }
                Purpose::Accept => {
                    tasks[*id]["state"] == "submitted" && tasks[*id]["verified"] == true
                }
                _ => false,
            })
            .unwrap();
        match i.purpose {
            Purpose::Assign => ManagementDecision::Assign {
                task_id: id.into(),
                worker_id: "worker-deepseek".into(),
                rationale: "Approved ready task".into(),
            },
            Purpose::Accept => {
                assert_eq!(tasks[id]["verified"], true);
                ManagementDecision::Accept {
                    task_id: id.into(),
                    attempt_id: tasks[id]["attempt"].as_str().unwrap().into(),
                    rationale: "Independent checks passed".into(),
                }
            }
            _ => panic!("unexpected purpose"),
        }
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
fn worker(request: &PreparedRequest, round: u32) -> Result<ApiReply, ApiFailure> {
    worker_named(request, round, &format!("chat-{round}"))
}
fn worker_named(
    request: &PreparedRequest,
    round: u32,
    response_id: &str,
) -> Result<ApiReply, ApiFailure> {
    let messages = fixture_messages(request);
    let ApiMessage::User(prompt) = &messages[0] else {
        panic!("approved prompt missing")
    };
    let final_task = prompt.contains("INTEGRATION_integration");
    let b = prompt.contains("INTEGRATION_b");
    let call = if final_task {
        if matches!(round, 2 | 3) {
            let ApiMessage::Tools(results) = messages.last().unwrap() else {
                panic!("actual read missing")
            };
            assert!(!results[0].is_error);
            let content: serde_json::Value = serde_json::from_str(&results[0].text).unwrap();
            assert_eq!(content["content"], if round == 2 { "A" } else { "B" });
        }
        match round {
            1 => Some(("cerulean_read_file", json!({"path":"a.txt","offset":0}))),
            2 => Some(("cerulean_read_file", json!({"path":"b.txt","offset":0}))),
            3 => Some((
                "cerulean_write_file",
                json!({"path":"bundle.txt","content":if prompt.contains("bad-whole"){"wrong"}else{"AB"},"expectedSha256":"absent"}),
            )),
            _ => None,
        }
    } else {
        match round {
            1 => Some((
                "cerulean_write_file",
                json!({"path":"result.txt","content":if b && prompt.contains("conflict"){"beta"}else{"alpha"},"expectedSha256":"absent"}),
            )),
            2 => Some((
                "cerulean_write_file",
                json!({"path":if b{"b.txt"}else{"a.txt"},"content":if b{"B"}else{"A"},"expectedSha256":"absent"}),
            )),
            _ => None,
        }
    };
    let message = match &call {
        Some((name, args)) => {
            json!({"role":"assistant","content":null,"tool_calls":[{"id":format!("call-{round}"),"type":"function","function":{"name":name,"arguments":args.to_string()}}]})
        }
        None => json!({"role":"assistant","content":"Integration fixture completed"}),
    };
    fixture_reply(request,json!({"id":response_id,"model":"fixture","choices":[{"index":0,"finish_reason":if call.is_some(){"tool_calls"}else{"stop"},"message":message}],"usage":{"prompt_tokens":10,"completion_tokens":10}})).map_err(|_| ApiFailure::InvalidResponse {})
}
fn configured(root: &Path, mode: &str) -> RuntimeHost {
    configured_with(root, mode, |_| {})
}
fn configured_with(
    root: &Path,
    mode: &str,
    configure: impl FnOnce(&mut cerulean_domain::project_setup::ProjectSetupDraft),
) -> RuntimeHost {
    configured_using(root, mode, |_, draft| configure(draft))
}
fn configured_using(
    root: &Path,
    mode: &str,
    configure: impl FnOnce(&mut RuntimeHost, &mut cerulean_domain::project_setup::ProjectSetupDraft),
) -> RuntimeHost {
    let (mut host, draft) = project_planning::tests::fixture(root);
    host.register_validator(
        "whole".into(),
        "Entire project".into(),
        std::env::current_exe().unwrap().to_string_lossy().into(),
        vec![],
        Default::default(),
    )
    .unwrap();
    let mut draft = api_project_tests::draft_for(
        draft,
        ProjectWorkerAdapter::Deepseek,
        ApiTarget::Provider {
            endpoint: ProviderEndpoint::DeepSeek {},
        },
    );
    draft.requirement = mode.into();
    draft.artifact_paths = vec!["result.txt".into()];
    draft.worker_max_requests = 20;
    draft.management_max_requests = 20;
    draft.api_worker.as_mut().unwrap().max_rounds = 4;
    draft.check_args = vec![
        "--ignored".into(),
        "--exact".into(),
        "host::integration_tests::partial_validator".into(),
    ];
    let mut worktree = dependency_host_fixture::configure(root);
    worktree.integration = Some(IntegrationDraft {
        task_id: "integration".into(),
        artifact_paths: vec!["bundle.txt".into(), "a.txt".into(), "b.txt".into()],
        validator_id: "whole".into(),
        args: vec![
            "--ignored".into(),
            "--exact".into(),
            "host::integration_tests::whole_validator".into(),
        ],
        timeout_seconds: 30,
    });
    draft.worktree = Some(worktree);
    configure(&mut host, &mut draft);
    host.configure_project(draft).unwrap();
    host.set_openai_key(Some(
        secret::SecretInput::new("sk-synthetic-integration-manager".into()).unwrap(),
    ))
    .unwrap();
    host.set_provider_key(
        CredentialProvider::DeepSeek,
        Some(secret::SecretInput::new("sk-synthetic-integration-worker".into()).unwrap()),
    )
    .unwrap();
    host.management_fixture = Some(manager);
    host.api_fixture = Some(worker);
    host
}
fn spin(host: &mut RuntimeHost, done: impl Fn(&RuntimeHost) -> bool) {
    let limit = Instant::now() + Duration::from_secs(120);
    loop {
        host.poll_verifications();
        if done(host) {
            return;
        }
        assert!(
            Instant::now() < limit,
            "timeout: {:?}; {:?}",
            host.runs.all(),
            host.planning_errors
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn planned(host: &mut RuntimeHost) -> PlanRevision {
    let digest = host.project_setups["project"].digest.clone();
    host.request_chief_plan("project", &digest).unwrap();
    spin(host, |h| h.projects["project"].revision().is_some());
    host.projects["project"].revision().unwrap().clone()
}
fn temporary() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("project-integration-")
        .tempdir_in(
            fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports")).unwrap(),
        )
        .unwrap()
}
#[test]
#[ignore = "independent partial validator subprocess"]
fn partial_validator() {
    assert!(matches!(
        fs::read_to_string("result.txt").unwrap().as_str(),
        "alpha" | "beta"
    ));
    assert!(!Path::new("bundle.txt").exists());
}
#[test]
#[ignore = "independent whole-project validator subprocess"]
fn whole_validator() {
    assert_eq!(
        fs::read_to_string("base.txt").unwrap(),
        "committed dependency base\n"
    );
    assert_eq!(fs::read_to_string("result.txt").unwrap(), "alpha");
    assert_eq!(fs::read_to_string("a.txt").unwrap(), "A");
    assert_eq!(fs::read_to_string("b.txt").unwrap(), "B");
    assert_eq!(fs::read_to_string("bundle.txt").unwrap(), "AB");
    assert!(!Path::new(".git").exists());
}
#[test]
#[ignore = "real Git, resident API integration, independent project checks; synthetic models"]
fn final_integration_success_failure_conflict_and_reopen() {
    for mode in ["success", "bad-whole", "conflict"] {
        let root = temporary();
        let mut host = configured(root.path(), mode);
        let plan = planned(&mut host);
        assert!(host.projects["project"].approval().is_none());
        assert!(!root.path().join("pool/records").exists());
        host.approve("project", &plan.approval_digest().unwrap())
            .unwrap();
        let setup = host.project_setups["project"].digest.clone();
        host.start_project_run("project", &plan.approval_digest().unwrap(), &setup)
            .unwrap();
        // Attention stops new scheduling immediately; collect the owned worker
        // before inspecting the returned idle ledger or reopening it.
        spin(&mut host, |h| {
            h.runs.all()[0].state != RunState::Active && h.api_active.is_empty()
        });
        let store = &host.projects["project"];
        assert_eq!(store.state().tasks["a"].state, TaskState::Accepted);
        assert_eq!(store.state().tasks["b"].state, TaskState::Accepted);
        let final_state = &store.state().tasks["integration"];
        match mode {
            "success" => {
                assert_eq!(host.runs.all()[0].state, RunState::Completed);
                assert_eq!(final_state.state, TaskState::Accepted);
                assert!(final_state.verified);
                let path = dependency_host_fixture::task_path(&host, "integration");
                assert_eq!(fs::read_to_string(path.join("bundle.txt")).unwrap(), "AB");
                assert!(host.runs.all()[0]
                    .message
                    .contains(&path.to_string_lossy().to_string()));
                let a = dependency_host_fixture::task_path(&host, "a");
                let b = dependency_host_fixture::task_path(&host, "b");
                assert!(path != a && path != b && a != b);
                assert!(!a.join("b.txt").exists() && !b.join("a.txt").exists());
            }
            "bad-whole" => {
                assert_eq!(host.runs.all()[0].state, RunState::Attention);
                assert_eq!(final_state.state, TaskState::Submitted);
                assert!(!final_state.verified);
            }
            _ => {
                assert_eq!(host.runs.all()[0].state, RunState::Attention);
                assert_eq!(final_state.state, TaskState::Pending);
                // Preparation reserves a durable startup before merging inputs.
                // A merge conflict retains that record for review but dispatches no model round.
                assert_eq!(crate::api_execution::prior_requests(store).unwrap(), 6);
                let job = host
                    .api_jobs
                    .values()
                    .find(|j| j.task_id == "integration")
                    .unwrap();
                assert_eq!(job.state, cerulean_domain::api_jobs::ApiJobState::Unknown);
                let expected = plan.execution_root("integration", &job.attempt_id).unwrap();
                assert!(!Path::new(&expected).exists());
            }
        }
        assert!(!root.path().join("project/bundle.txt").exists());
        assert_eq!(
            fs::read_to_string(root.path().join("project/base.txt")).unwrap(),
            "uncommitted source change\n"
        );
        let history = store.history().to_vec();
        let state = host.runs.all()[0].state.clone();
        let final_events = history.iter().filter(|e| matches!(&e.body.event.command,WorkflowCommand::DispatchIntent {task,..} if task=="integration")).count();
        assert_eq!(final_events, usize::from(mode != "conflict"));
        drop(host);
        let mut reopened = RuntimeHost::open(&root.path().join("data")).unwrap();
        reopened.poll_verifications();
        assert_eq!(reopened.runs.all()[0].state, state);
        assert_eq!(reopened.projects["project"].history(), history);
        println!(
            "integration scenario {mode}: passed, final dispatches {final_events}, reopen events 0"
        );
    }
}
#[test]
#[ignore = "real pinned Git configuration; plan scope and approval mutation checks"]
fn integration_scope_binding_rejects_missing_dependencies_or_checks() {
    let root = temporary();
    let mut host = configured(root.path(), "configuration");
    let plan = planned(&mut host);
    let digest = plan.approval_digest().unwrap();
    let mut missing = plan.clone();
    missing
        .worktree
        .as_mut()
        .unwrap()
        .integration
        .as_mut()
        .unwrap()
        .task_id = "missing".into();
    assert!(missing.validate().is_err());
    let mut disconnected = plan.clone();
    disconnected.workflow.tasks[2].dependencies = vec!["a".into()];
    assert!(disconnected.validate().is_err());
    let mut check = plan.clone();
    check.execution[2].checks = check.execution[0].checks.clone();
    assert!(check.validate().is_err());
    let mut artifacts = plan.clone();
    artifacts.execution[2].artifact_paths = vec!["result.txt".into()];
    assert!(artifacts.validate().is_err());
    let mut mode = plan.clone();
    mode.worktree.as_mut().unwrap().input = GitWorktreeInput::CommittedRawBlobs;
    assert!(mode.validate().is_err());
    let mut changed = plan.clone();
    changed
        .worktree
        .as_mut()
        .unwrap()
        .integration
        .as_mut()
        .unwrap()
        .check
        .args
        .push("--nocapture".into());
    changed.execution[2].checks = vec![changed.integration().unwrap().check.clone()];
    assert_ne!(changed.approval_digest().unwrap(), digest);
    let mut legacy = plan.clone();
    legacy.worktree.as_mut().unwrap().integration = None;
    assert!(!serde_json::to_value(&legacy.worktree)
        .unwrap()
        .as_object()
        .unwrap()
        .contains_key("integration"));
    assert!(!root.path().join("pool/records").exists());
    drop(host);
    let reopened = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(
        reopened.projects["project"]
            .revision()
            .unwrap()
            .approval_digest()
            .unwrap(),
        digest
    );
}
