//! Production host/worker/verifier orchestration with an explicitly offline protocol peer.
use super::*;
use cerulean_adapters::openai::DecisionObservation;
use cerulean_domain::{
    management::{DecisionEnvelope, ManagementDecision, ProposedTask, Purpose},
    project_setup::*,
};
use std::time::{Duration, Instant};

fn manager(i: crate::management_execution::DecisionIntent) -> Result<DecisionObservation, String> {
    let tasks = &i.context["workflow"]["state"]["tasks"];
    let decision = match i.purpose {
        Purpose::Refine => return Err("Document refinement uses its dedicated fixture".into()),
        Purpose::Plan => ManagementDecision::Plan {
            tasks: vec![
                ProposedTask {
                    artifact_paths: None,
                    objective_test: None,
                    profile_id: None,
                    id: "a".into(),
                    goal: "DAG_STEP_A: write alpha to result.txt".into(),
                    dependencies: vec![],
                    worker_id: "worker-codex".into(),
                    validator_id: "dag-validator".into(),
                },
                ProposedTask {
                    artifact_paths: None,
                    objective_test: None,
                    profile_id: None,
                    id: "b".into(),
                    goal: "DAG_STEP_B: extend alpha to alpha beta in result.txt".into(),
                    dependencies: vec!["a".into()],
                    worker_id: "worker-codex".into(),
                    validator_id: "dag-validator".into(),
                },
            ],
            rationale: "Two dependent changes, independently verified and accepted".into(),
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
                worker_id: "worker-codex".into(),
                rationale: "Only eligible task".into(),
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
                rationale: "Both independent checks passed".into(),
            }
        }
        Purpose::Advise => {
            assert!(i.context.get("supervision").is_some());
            if i.context["requirement"]
                .as_str()
                .unwrap()
                .contains("ADVICE_UNKNOWN")
            {
                return Err("synthetic lost response".into());
            }
            let task_id = if i.context["requirement"]
                .as_str()
                .unwrap()
                .contains("ADVICE_WRONG_TASK")
            {
                "b".into()
            } else {
                i.context["supervision"]["taskId"].as_str().unwrap().into()
            };
            ManagementDecision::Advise {task_id,suggestion:"Inspect the approved task and the failing check before continuing within the original scope.".into(),rationale:"Two observed command failures; this does not prove a loop.".into()}
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
pub(super) fn enable_offline_management(host: &mut RuntimeHost) {
    host.openai_connection.key_configured = true;
    host.management_fixture = Some(manager);
}
fn collect_only(host: &mut RuntimeHost) {
    host.orchestrating = true;
    host.poll_verifications();
    host.orchestrating = false;
}
fn command_count(host: &RuntimeHost, task: &str, dispatch: bool) -> usize {
    host.projects["project"].history().iter().filter(|e| if dispatch {
        matches!(&e.body.event.command, WorkflowCommand::DispatchIntent { task: t, .. } if t == task)
    } else {
        matches!(&e.body.event.command, WorkflowCommand::Accept { task: t, .. } if t == task)
    }).count()
}
fn resume(host: &mut RuntimeHost) {
    let run = host.runs.all()[0].clone();
    host.start_project_run("project", &run.revision_digest, &run.setup_digest)
        .unwrap();
}
#[test]
#[ignore = "offline two-task production pipeline; explicit fresh reports directory and built fixture executable"]
fn two_task_production_pipeline_with_multichecks_and_restarts() {
    run_production_pipeline(false)
}
#[test]
#[ignore = "real Git; resident Codex dependent worktrees with multi-check verification and host restart, offline peer"]
fn dependency_worktree_codex_dag_with_restarts() {
    run_production_pipeline(true)
}
fn run_production_pipeline(isolated: bool) {
    let outer = PathBuf::from(std::env::var_os("CERULEAN_DAG_DIR").unwrap())
        .canonicalize()
        .unwrap();
    let reports = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../reports")
        .canonicalize()
        .unwrap();
    assert!(outer.starts_with(&reports) && outer != reports && !outer.join("data").exists());
    let mode = std::env::var("CERULEAN_DAG_MODE").unwrap();
    if isolated {
        assert!(matches!(mode.as_str(), "success" | "recover-a"));
    }
    let successful = matches!(mode.as_str(), "success" | "recover-a");
    assert!(matches!(
        mode.as_str(),
        "success" | "recover-a" | "fail-a-syntax" | "fail-a-unit" | "fail-b-unit"
    ));
    let (mut host, mut draft) = super::project_planning::tests::fixture(&outer);
    let bin = outer.join("bin");
    fs::create_dir(&bin).unwrap();
    let observations = std::env::var_os("CERULEAN_DAG_OBSERVATIONS").is_some();
    let advice = std::env::var("CERULEAN_DAG_ADVICE").ok();
    let delivery = std::env::var("CERULEAN_DAG_DELIVERY").ok();
    if let Some(mode) = &delivery {
        assert!(matches!(
            mode.as_str(),
            "ACK" | "WRONG_ACK" | "LOST_ACK" | "PAUSE" | "PAUSE_COMMIT" | "CANCEL"
        ));
        if !mode.starts_with("PAUSE") {
            fs::write(bin.join("await-steer"), "synthetic delivery fixture").unwrap();
        }
        if mode == "LOST_ACK" {
            fs::write(bin.join("lost-steer-ack"), "").unwrap();
        }
        if mode == "WRONG_ACK" {
            fs::write(bin.join("wrong-steer-ack"), "").unwrap();
        }
    }
    if observations {
        fs::write(bin.join("emit-observations"), "synthetic metadata fixture").unwrap();
        if std::env::var_os("CERULEAN_DAG_OBSERVATIONS_CLEAN").is_some() {
            fs::write(bin.join("observations-clean"), "synthetic clean metadata").unwrap();
        }
    }
    let peer = bin.join("codex.exe");
    fs::copy(std::env::var_os("CERULEAN_DAG_EXE").unwrap(), &peer).unwrap();
    fs::write(
        bin.join("workspace.json"),
        serde_json::to_vec(&outer.join("project")).unwrap(),
    )
    .unwrap();
    fs::write(outer.join("project/fixture-mode"), &mode).unwrap();
    host.register_validator(
        "dag-validator".into(),
        "Offline independent DAG checks".into(),
        peer.to_string_lossy().into(),
        vec![],
        Default::default(),
    )
    .unwrap();
    draft.requirement = "Apply DAG_STEP_A then DAG_STEP_B only after independent acceptance".into();
    if let Some(mode) = &advice {
        draft.requirement.push_str(&format!(" ADVICE_{mode}"));
    }
    draft.codex_executable = peer.to_string_lossy().into();
    draft.worker_max_requests = 2;
    if observations && std::env::var_os("CERULEAN_DAG_SUPERVISION").is_some() {
        draft.supervision = Some(cerulean_domain::supervision::SupervisionPolicy {
            schema_version: 1,
            repeated_failure_threshold: 2,
            max_advice_per_attempt: if advice.is_some() || delivery.is_some() {
                2
            } else {
                0
            },
            delivery_enabled: delivery.is_some(),
        });
    }
    draft.management_max_requests = if delivery.is_some() { 7 } else { 5 };
    draft.validator_id = "dag-validator".into();
    draft.check_args = vec!["validate".into(), "syntax".into()];
    draft.artifact_paths = vec!["result.txt".into()];
    draft.acceptance = Some(AcceptanceDraft {
        schema_version: 1,
        stages: [("syntax", "语法检查"), ("unit", "单元测试")]
            .into_iter()
            .map(|(id, name)| AcceptanceStageDraft {
                expected_assertions: vec![],
                id: id.into(),
                name: name.into(),
                validator_id: "dag-validator".into(),
                args: vec!["validate".into(), id.into()],
                timeout_seconds: 10,
            })
            .collect(),
    });
    if isolated {
        let worktree = dependency_host_fixture::configure(&outer);
        fs::write(
            bin.join("worktree-pool.json"),
            serde_json::to_vec(&worktree.pool_root).unwrap(),
        )
        .unwrap();
        draft.worktree = Some(worktree);
    }
    host.configure_project(draft.clone()).unwrap();
    enable_offline_management(&mut host);
    let setup = host.project_setups["project"].digest.clone();
    host.request_chief_plan("project", &setup).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while host.projects["project"].revision().is_none() && Instant::now() < deadline {
        collect_only(&mut host);

        std::thread::sleep(Duration::from_millis(5));
    }
    let revision = host.projects["project"]
        .revision()
        .unwrap_or_else(|| panic!("planning did not finish: {:?}", host.snapshot()))
        .approval_digest()
        .unwrap();
    assert!(host.projects["project"].approval().is_none());
    assert!(host.codex_jobs.is_empty());
    host.approve("project", &revision).unwrap();
    // Even a trusted caller cannot assign/dispatch B before its dependency.
    assert!(host
        .start_codex("project", "b", "premature-b", &revision)
        .is_err());
    assert!(host.codex_jobs.is_empty());
    assert!(!bin.join("protocol.jsonl").exists());
    host.start_project_run("project", &revision, &setup)
        .unwrap();
    let mut restart_before_accept = false;
    let mut restart_after_accept = false;
    let mut recovered_a = false;
    let mut live_observation_read = false;
    let mut paused_delivery = false;
    let deadline = Instant::now() + Duration::from_secs(if isolated { 120 } else { 75 });
    loop {
        assert!(
            Instant::now() < deadline,
            "DAG timeout: {:?}",
            host.snapshot()
        );
        collect_only(&mut host);
        if let Some(mode) = &delivery {
            if matches!(mode.as_str(), "PAUSE" | "CANCEL")
                && !paused_delivery
                && host.management.statuses().iter().any(|s| {
                    s.purpose == Purpose::Advise
                        && s.state == crate::management_execution::DecisionState::Ready
                })
            {
                // Wake then pause before the host can acknowledge the worker's Prepare.
                host.poll_project_runs();
                if mode == "CANCEL" {
                    let review = host.codex_cancellations()[0].clone();
                    host.cancel_codex(
                        "project",
                        &review.attempt_id,
                        &review.evidence_digest,
                        "Cancel queued advice fixture",
                    )
                    .unwrap();
                } else {
                    host.pause_project_run(&host.runs.all()[0].id.clone())
                        .unwrap();
                }
                paused_delivery = true;
                // An earlier response must not regain delivery authorization on resume.
                assert!(host
                    .start_project_run("project", &revision, &setup)
                    .is_err());
            }
            host.poll_project_runs();
            let terminal = host.codex_active.is_empty()
                && host.active.is_empty()
                && host.management_active.is_empty()
                && host.runs.all()[0].state != RunState::Active;
            if terminal {
                let accepted = mode == "ACK";
                assert_eq!(
                    host.runs.all()[0].state,
                    if accepted {
                        RunState::Completed
                    } else if mode.starts_with("PAUSE") || mode == "CANCEL" {
                        RunState::Paused
                    } else {
                        RunState::Attention
                    }
                );
                let protocol: Vec<serde_json::Value> =
                    fs::read_to_string(bin.join("protocol.jsonl"))
                        .unwrap()
                        .lines()
                        .map(|s| serde_json::from_str(s).unwrap())
                        .collect();
                let starts = protocol
                    .iter()
                    .filter(|v| v["method"] == "turn/start")
                    .count();
                let steers = protocol
                    .iter()
                    .filter(|v| v["method"] == "turn/steer")
                    .count();
                assert_eq!(starts, if accepted { 2 } else { 1 });
                assert_eq!(
                    steers,
                    if accepted {
                        2
                    } else if mode.starts_with("PAUSE") || mode == "CANCEL" {
                        0
                    } else {
                        1
                    }
                );
                assert_eq!(command_count(&host, "b", true), usize::from(accepted));
                assert_eq!(command_count(&host, "a", false), usize::from(accepted));
                assert_eq!(command_count(&host, "b", false), usize::from(accepted));
                assert_eq!(host.jobs.len(), if accepted { 2 } else { 0 });
                assert_eq!(
                    host.management.statuses().len(),
                    if accepted { 7 } else { 3 }
                );
                let advised = host.projects["project"]
                    .history()
                    .iter()
                    .filter(|e| matches!(e.body.event.command, WorkflowCommand::Advise { .. }))
                    .count();
                assert_eq!(advised, if mode == "PAUSE_COMMIT" { 1 } else { steers });
                if mode == "CANCEL" {
                    assert_eq!(
                        host.projects["project"].state().tasks["a"].state,
                        TaskState::Cancelled
                    );
                    assert_eq!(
                        protocol
                            .iter()
                            .filter(|v| v["method"] == "turn/interrupt")
                            .count(),
                        1
                    );
                }
                let operations: Vec<_> = host
                    .management
                    .statuses()
                    .into_iter()
                    .filter(|s| s.purpose == Purpose::Advise)
                    .map(|s| s.operation_id)
                    .collect();
                for operation in &operations {
                    assert_eq!(
                        host.management_detail(operation)
                            .unwrap()
                            .supervision_advice
                            .unwrap()
                            .delivered,
                        accepted
                    );
                }
                let sequence = host.projects["project"].history().len();
                let data = outer.join("data");
                drop(host);
                host = RuntimeHost::open(&data).unwrap();
                enable_offline_management(&mut host);
                for _ in 0..5 {
                    host.poll_verifications();
                }
                assert_eq!(host.projects["project"].history().len(), sequence);
                assert_eq!(
                    host.management.statuses().len(),
                    if accepted { 7 } else { 3 }
                );
                let mut details = vec![];
                for operation in &operations {
                    let detail = host.management_detail(operation).unwrap();
                    assert_eq!(
                        detail.supervision_advice.as_ref().unwrap().delivered,
                        accepted
                    );
                    details.push(detail);
                }
                fs::write(outer.join("delivery-result.json"),serde_json::to_vec_pretty(&serde_json::json!({"at":chrono::Utc::now().to_rfc3339(),"status":"passed","mode":mode,"realModelCalls":0,"turnStarts":starts,"steers":steers,"localAdviceEvents":advised,"managementRequests":host.management.statuses().len(),"verificationJobs":host.jobs.len(),"newEventsAfterRestart":0,"details":details,"snapshot":host.snapshot()})).unwrap()).unwrap();
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }
        if advice.is_some() {
            host.poll_project_runs();
            let result = host
                .management
                .statuses()
                .into_iter()
                .find(|s| s.purpose == Purpose::Advise);
            if let Some(status) =
                result.filter(|s| !host.management_active.contains_key(&s.operation_id))
            {
                use crate::management_execution::DecisionState;
                let wanted = if advice.as_deref() == Some("SAVED") {
                    DecisionState::Ready
                } else {
                    DecisionState::Unknown
                };
                assert_eq!(status.state, wanted);
                let detail = host.management_detail(&status.operation_id).unwrap();
                assert!(!detail.supervision_advice.as_ref().unwrap().delivered);
                assert_eq!(
                    detail
                        .supervision_advice
                        .as_ref()
                        .unwrap()
                        .suggestion
                        .is_some(),
                    wanted == DecisionState::Ready
                );
                assert_eq!(host.management.statuses().len(), 3);
                for _ in 0..5 {
                    host.poll_project_runs();
                }
                assert_eq!(host.management.statuses().len(), 3);
                host.pause_project_run(&host.runs.all()[0].id.clone())
                    .unwrap();
                let deadline = Instant::now() + Duration::from_secs(10);
                while !host.codex_active.is_empty() && Instant::now() < deadline {
                    collect_only(&mut host);
                    std::thread::sleep(Duration::from_millis(10));
                }
                assert!(host.codex_active.is_empty());
                assert_eq!(command_count(&host, "a", true), 1);
                assert_eq!(command_count(&host, "b", true), 0);
                assert!(host.projects["project"]
                    .history()
                    .iter()
                    .all(|e| !matches!(e.body.event.command, WorkflowCommand::Advise { .. })));
                assert!(host.jobs.is_empty());
                let sequence = host.projects["project"].history().len();
                let data = outer.join("data");
                drop(host);
                host = RuntimeHost::open(&data).unwrap();
                enable_offline_management(&mut host);
                for _ in 0..5 {
                    host.poll_verifications();
                }
                let reopened = host.management_detail(&status.operation_id).unwrap();
                assert_eq!(
                    reopened.supervision_advice.as_ref().unwrap().suggestion,
                    detail.supervision_advice.as_ref().unwrap().suggestion
                );
                assert!(!reopened.supervision_advice.as_ref().unwrap().current);
                assert_eq!(host.projects["project"].history().len(), sequence);
                assert_eq!(host.management.statuses().len(), 3);
                fs::write(outer.join("advice-result.json"),serde_json::to_vec_pretty(&serde_json::json!({"status":"passed","mode":advice,"realModelCalls":0,"managementRequests":3,"adviceRequests":1,"workerStarts":1,"newEventsAfterRestart":0,"delivered":false,"detail":reopened,"snapshot":host.snapshot()})).unwrap()).unwrap();
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }
        if observations && !live_observation_read {
            let candidate = host
                .codex_active
                .values()
                .find(|a| {
                    a.observation
                        .as_ref()
                        .is_some_and(|c| !c.snapshot().events.is_empty())
                })
                .map(|a| (a.job.clone(), a.snapshot.sequence));
            if let Some((job, sequence)) = candidate {
                let count = host.management.statuses().len();
                let response = host.handle(Request {
                    id: "read-live-observation".into(),
                    protocol_version: PROTOCOL_VERSION,
                    command: Command::ReadWorkerObservation {
                        project_id: job.project_id.clone(),
                        attempt_id: job.attempt_id.clone(),
                    },
                });
                assert!(response.error.is_none(), "{:?}", response.error);
                let view = response.result.unwrap().worker_observation.unwrap();
                assert!(!view.trace.events.is_empty());
                assert_eq!(
                    host.codex_active[&(job.project_id.clone(), job.attempt_id.clone())]
                        .snapshot
                        .sequence,
                    sequence
                );
                assert_eq!(host.management.statuses().len(), count);
                assert!(host.snapshot().worker_observation.is_none());
                live_observation_read = true;
            }
        }
        if let Some(store) = host.projects.get("project") {
            let a = store.state().tasks["a"].clone();
            if a.state != TaskState::Accepted {
                assert_eq!(command_count(&host, "b", true), 0);
                assert_eq!(store.state().tasks["b"].state, TaskState::Pending);
            }
            if !mode.starts_with("fail-a")
                && !restart_before_accept
                && a.verified
                && a.state == TaskState::Submitted
            {
                assert_eq!(command_count(&host, "a", false), 0);
                assert_eq!(host.management.statuses().len(), 2);
                if mode == "recover-a" && !recovered_a {
                    let job = host
                        .jobs
                        .values()
                        .find(|j| j.task_id == "a")
                        .unwrap()
                        .clone();
                    let sequence = host.projects["project"].history().len();
                    let data = outer.join("data");
                    drop(host);
                    // Inject only the report-to-Verify publication gap, keeping the
                    // original active user run and all real validator evidence.
                    let mut removed = 0;
                    for entry in fs::read_dir(data.join("workflows/project/events")).unwrap() {
                        let path = entry.unwrap().path();
                        let event: serde_json::Value =
                            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                        if event["body"]["operationId"] == job.operation_id {
                            fs::remove_file(path).unwrap();
                            removed += 1;
                        }
                    }
                    assert_eq!(removed, 1);
                    host = RuntimeHost::open(&data).unwrap();
                    enable_offline_management(&mut host);
                    assert_eq!(host.runs.all()[0].state, RunState::Active);
                    let job = host
                        .jobs
                        .values()
                        .find(|j| j.task_id == "a")
                        .unwrap()
                        .clone();
                    assert_eq!(job.state, VerificationState::Unknown);
                    let review = host.verification_recovery_review(&job).unwrap();
                    let response = host.handle(Request {
                        id: "explicit-dag-recovery".into(),
                        protocol_version: PROTOCOL_VERSION,
                        command: Command::RecoverVerification {
                            project_id: "project".into(),
                            operation_id: job.operation_id.clone(),
                            evidence_digest: review.evidence_digest,
                        },
                    });
                    assert!(response.error.is_none(), "{:?}", response.error);
                    assert_eq!(host.runs.all()[0].state, RunState::Paused);
                    for _ in 0..3 {
                        host.poll_verifications();
                    }
                    assert_eq!(host.projects["project"].history().len(), sequence);
                    assert_eq!(host.management.statuses().len(), 2);
                    assert_eq!(command_count(&host, "a", false), 0);
                    assert_eq!(command_count(&host, "b", true), 0);
                    recovered_a = true;
                }
                host.pause_project_run(&host.runs.all()[0].id.clone())
                    .unwrap();
                let sequence = host.projects["project"].history().len();
                drop(host);
                host = RuntimeHost::open(&outer.join("data")).unwrap();
                enable_offline_management(&mut host);
                collect_only(&mut host);
                assert_eq!(host.projects["project"].history().len(), sequence);
                assert_eq!(command_count(&host, "b", true), 0);
                assert_eq!(host.management.statuses().len(), 2);
                resume(&mut host);
                restart_before_accept = true;
            } else if !restart_after_accept && a.state == TaskState::Accepted {
                assert_eq!(command_count(&host, "b", true), 0);
                host.pause_project_run(&host.runs.all()[0].id.clone())
                    .unwrap();
                drop(host);
                host = RuntimeHost::open(&outer.join("data")).unwrap();
                enable_offline_management(&mut host);
                collect_only(&mut host);
                assert_eq!(command_count(&host, "a", true), 1);
                assert_eq!(command_count(&host, "b", true), 0);
                resume(&mut host);
                restart_after_accept = true;
            }
        }
        host.poll_project_runs();
        if matches!(
            host.runs.all()[0].state,
            RunState::Attention | RunState::Completed
        ) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        host.codex_active.is_empty() && host.active.is_empty() && host.management_active.is_empty()
    );
    let store = &host.projects["project"];
    let failed_a = mode.starts_with("fail-a");
    assert_eq!(command_count(&host, "a", true), 1);
    assert_eq!(command_count(&host, "b", true), usize::from(!failed_a));
    assert_eq!(command_count(&host, "a", false), usize::from(!failed_a));
    assert_eq!(command_count(&host, "b", false), usize::from(successful));
    assert_eq!(host.jobs.len(), if failed_a { 1 } else { 2 });
    assert_eq!(
        host.management.statuses().len(),
        if failed_a {
            2
        } else if successful {
            5
        } else {
            4
        }
    );
    assert_eq!(
        host.runs.all()[0].state,
        if successful {
            RunState::Completed
        } else {
            RunState::Attention
        }
    );
    let sequence = store.history().len();
    let management_count = host.management.statuses().len();
    let mut details = vec![];
    for job in host.jobs.values().cloned().collect::<Vec<_>>() {
        let response = host
            .handle(Request {
                id: format!("detail-{}", job.task_id),
                protocol_version: PROTOCOL_VERSION,
                command: Command::ReadVerificationDetail {
                    project_id: "project".into(),
                    operation_id: job.operation_id,
                },
            })
            .result
            .unwrap();
        let detail = response.verification_detail.unwrap();
        assert_eq!(detail.stages.len(), 2);
        for stage in &detail.stages {
            assert_eq!(
                stage.exit_code,
                Some(if mode == format!("fail-{}-{}", job.task_id, stage.id) {
                    7
                } else {
                    0
                })
            );
        }
        details.push(detail);
    }
    assert_eq!(host.projects["project"].history().len(), sequence);
    fs::write(
        outer.join("details.json"),
        serde_json::to_vec_pretty(&details).unwrap(),
    )
    .unwrap();
    fs::write(
        outer.join("snapshot.json"),
        serde_json::to_vec_pretty(&host.snapshot()).unwrap(),
    )
    .unwrap();
    drop(host);
    host = RuntimeHost::open(&outer.join("data")).unwrap();
    enable_offline_management(&mut host);
    for _ in 0..5 {
        host.poll_verifications();
    }
    assert_eq!(host.projects["project"].history().len(), sequence);
    assert_eq!(host.management.statuses().len(), management_count);
    if !successful {
        resume(&mut host);
        host.poll_verifications();
        assert_eq!(host.runs.all()[0].state, RunState::Attention);
        assert_eq!(host.projects["project"].history().len(), sequence);
        assert_eq!(host.management.statuses().len(), management_count);
    }
    let protocol = fs::read_to_string(bin.join("protocol.jsonl")).unwrap();
    let turns = protocol
        .lines()
        .filter(|line| {
            serde_json::from_str::<serde_json::Value>(line).unwrap()["method"] == "turn/start"
        })
        .count();
    assert_eq!(turns, if failed_a { 1 } else { 2 });
    if isolated {
        dependency_host_fixture::assert_delivery(&host, "alpha", "alpha beta");
    }
    if observations {
        assert!(live_observation_read);
        let jobs = host.codex_jobs.values().cloned().collect::<Vec<_>>();
        let mut views = vec![];
        for job in jobs {
            let request = || Request {
                id: "read-observation".into(),
                protocol_version: PROTOCOL_VERSION,
                command: Command::ReadWorkerObservation {
                    project_id: job.project_id.clone(),
                    attempt_id: job.attempt_id.clone(),
                },
            };
            let view = host
                .handle(request())
                .result
                .unwrap()
                .worker_observation
                .unwrap();
            assert_eq!(view.trace.events.len(), 3);
            assert!(view.trace.finished);
            assert_eq!(view.trace.duplicates, 1);
            assert_eq!(
                view.trace.rejected,
                u32::from(std::env::var_os("CERULEAN_DAG_OBSERVATIONS_CLEAN").is_none())
            );
            assert_eq!(view.trace.dropped, 0);
            assert!(!serde_json::to_string(&view)
                .unwrap()
                .contains("private-observation-sentinel"));
            if std::env::var_os("CERULEAN_DAG_SUPERVISION").is_some() {
                assert_eq!(view.incidents.len(), 1);
                assert_eq!(view.incidents[0].first_sequence, 1);
                assert_eq!(view.incidents[0].trigger_sequence, 2);
                assert_eq!(view.supervision.as_ref().unwrap().max_advice_per_attempt, 0);
                let again = host
                    .handle(request())
                    .result
                    .unwrap()
                    .worker_observation
                    .unwrap();
                assert_eq!(again.incidents, view.incidents);
            } else {
                assert!(view.incidents.is_empty());
            }
            let file = outer
                .join("data/worker-observations/project")
                .join(format!("{}.json", job.attempt_id));
            let original = fs::read(&file).unwrap();
            if std::env::var_os("CERULEAN_DAG_SUPERVISION").is_some() {
                let mut tampered: serde_json::Value = serde_json::from_slice(&original).unwrap();
                tampered["view"]["incidents"][0]["id"] = serde_json::json!("fabricated-incident");
                fs::write(&file, serde_json::to_vec(&tampered).unwrap()).unwrap();
                assert!(host.handle(request()).error.is_some());
                let mut wrong_policy = view.supervision.clone().unwrap();
                wrong_policy.max_advice_per_attempt = 1;
                let forged = crate::worker_observation::view(
                    &job,
                    view.trace.clone(),
                    None,
                    Some(wrong_policy),
                )
                .unwrap();
                tampered["view"] = serde_json::to_value(forged).unwrap();
                fs::write(&file, serde_json::to_vec(&tampered).unwrap()).unwrap();
                assert!(
                    host.handle(request()).error.is_some(),
                    "a recomputed record cannot change the original approved policy"
                );
            }
            fs::write(&file, b"{damaged").unwrap();
            assert!(host.handle(request()).error.is_some());
            fs::write(&file, &original).unwrap();
            assert_eq!(
                host.handle(request())
                    .result
                    .unwrap()
                    .worker_observation
                    .unwrap()
                    .evidence_digest,
                view.evidence_digest
            );
            views.push(view);
        }
        assert_eq!(host.projects["project"].history().len(), sequence);
        fs::write(
            outer.join("observations.json"),
            serde_json::to_vec_pretty(&views).unwrap(),
        )
        .unwrap();
    }
    assert_eq!(recovered_a, mode == "recover-a");
    fs::write(outer.join("result.json"),serde_json::to_vec_pretty(&serde_json::json!({"at":chrono::Utc::now().to_rfc3339(),"status":"passed","mode":mode,"realModelCalls":0,"scope":"production host/managed-worker/validator; synthetic chief and Codex protocol peer","turnStarts":turns,"managementDecisions":management_count,"restartBeforeAAccept":restart_before_accept,"restartAfterAAccept":restart_after_accept,"bDispatchBeforeAAccept":0,"checksPerTask":2,"finalRestartNewEvents":0,"failedVerificationResumeDoesNotRerun":!successful,"explicitRecoveryPausedActiveRun":recovered_a})).unwrap()).unwrap();
    if std::env::var_os("CERULEAN_DAG_REVISE_FAILED").is_some() {
        assert_eq!(mode, "fail-a-unit");
        let old_attempt = host.projects["project"].state().tasks["a"]
            .attempt
            .clone()
            .unwrap();
        let original_history = host.projects["project"].history().to_vec();
        let review = host.project_revision_candidates()[0].clone();
        assert_eq!(review.submitted_tasks, 1);
        assert_eq!(review.pending_tasks, 1);
        assert_eq!(review.verified_submissions, 0);
        assert!(host
            .request_project_revision("project", &review.evidence_digest, "ordinary revision")
            .is_err());
        host.retire_submitted_plan(
            "project",
            &review.evidence_digest,
            "Keep the failed result; revise and explicitly authorize a new attempt",
        )
        .unwrap();
        assert_eq!(host.runs.all()[0].state, RunState::Superseded);
        for _ in 0..3 {
            host.poll_verifications();
        }
        assert_eq!(host.codex_jobs.len(), 1);
        assert_eq!(host.jobs.len(), 1);
        assert_eq!(host.management.statuses().len(), 2);
        // User raises the cumulative budgets explicitly; revision never resets usage.
        draft.worker_max_requests = 3;
        draft.management_max_requests = 7;
        draft.requirement = "Rework failed A, then perform dependent B".into();
        fs::write(outer.join("project/fixture-mode"), "success").unwrap();
        fs::write(
            bin.join("allow-explicit-rework"),
            "user-authorized new revision fixture",
        )
        .unwrap();
        drop(host);
        host = RuntimeHost::open(&outer.join("data")).unwrap();
        enable_offline_management(&mut host);
        host.configure_project(draft).unwrap();
        let setup = host.project_setups["project"].digest.clone();
        host.request_chief_plan("project", &setup).unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while host.projects["project"]
            .revision()
            .unwrap()
            .workflow
            .version
            == 1
        {
            assert!(Instant::now() < deadline, "{:?}", host.snapshot());
            collect_only(&mut host);
            std::thread::sleep(Duration::from_millis(10));
        }
        let revised = host.projects["project"]
            .revision()
            .unwrap()
            .approval_digest()
            .unwrap();
        assert!(host.projects["project"].approval().is_none());
        assert!(host.start_project_run("project", &revised, &setup).is_err());
        assert_eq!(host.codex_jobs.len(), 1);
        host.approve("project", &revised).unwrap();
        for _ in 0..3 {
            host.poll_verifications();
        }
        assert_eq!(
            host.codex_jobs.len(),
            1,
            "approval is not a new run authorization"
        );
        host.start_project_run("project", &revised, &setup).unwrap();
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            host.poll_verifications();
            if host
                .runs
                .all()
                .iter()
                .any(|r| r.revision_digest == revised && r.state == RunState::Completed)
            {
                break;
            }
            assert!(
                !host
                    .runs
                    .all()
                    .iter()
                    .any(|r| r.revision_digest == revised && r.state == RunState::Attention),
                "{:?}",
                host.snapshot()
            );
            assert!(Instant::now() < deadline, "{:?}", host.snapshot());
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_ne!(
            host.projects["project"].state().tasks["a"]
                .attempt
                .as_deref(),
            Some(old_attempt.as_str())
        );
        assert_eq!(host.projects["project"].state().tasks["a"].fence, 2);
        assert_eq!(command_count(&host, "a", true), 2);
        assert_eq!(command_count(&host, "b", true), 1);
        assert_eq!(host.management.statuses().len(), 7);
        assert_eq!(host.jobs.len(), 3);
        assert!(host
            .jobs
            .values()
            .any(|j| j.attempt_id == old_attempt && j.state == VerificationState::Failed));
        assert_eq!(
            &host.projects["project"].history()[..original_history.len()],
            original_history.as_slice()
        );
        let final_sequence = host.projects["project"].history().len();
        drop(host);
        host = RuntimeHost::open(&outer.join("data")).unwrap();
        enable_offline_management(&mut host);
        for _ in 0..3 {
            host.poll_verifications();
        }
        assert_eq!(host.projects["project"].history().len(), final_sequence);
        assert_eq!(host.codex_jobs.len(), 3);
        fs::write(outer.join("revision-result.json"),serde_json::to_vec_pretty(&serde_json::json!({
            "status":"passed","realModelCalls":0,"oldFailedEvidencePreserved":true,"ordinaryRevisionRejected":true,
            "oldRunSuperseded":true,"newApprovalAndStartRequired":true,"aFence":2,"workerDispatches":3,
            "managementDecisions":7,"oldFailureCount":1,"oldJournalPrefixUnchanged":true,"restartNewEvents":0,
            "snapshot":host.snapshot()
        })).unwrap()).unwrap();
    }
}
