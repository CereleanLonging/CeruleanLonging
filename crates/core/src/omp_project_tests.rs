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
                    profile_id: None,
                    id: "a".into(),
                    goal: "OMP_DAG_FIRST: create result.txt".into(),
                    dependencies: vec![],
                    worker_id: "worker-omp".into(),
                    validator_id: "tests".into(),
                },
                ProposedTask {
                    profile_id: None,
                    id: "b".into(),
                    goal: "OMP_DAG_SECOND: update result.txt after A acceptance".into(),
                    dependencies: vec!["a".into()],
                    worker_id: "worker-omp".into(),
                    validator_id: "tests".into(),
                },
            ],
            rationale: "independent acceptance before dependency dispatch".into(),
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
                worker_id: "worker-omp".into(),
                rationale: "eligible task".into(),
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
                rationale: "independent check passed".into(),
            }
        }
        Purpose::Advise => panic!("OMP supervision is not enabled"),
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
pub(super) fn enable(host: &mut RuntimeHost) {
    host.openai_connection.key_configured = true;
    host.management_fixture = Some(manager);
}
fn collect(host: &mut RuntimeHost) {
    host.orchestrating = true;
    host.poll_verifications();
    host.orchestrating = false;
}
#[test]
fn executor_selection_preserves_legacy_digest_and_blocks_unbound_dispatch() {
    for adapter in [ProjectWorkerAdapter::Codex, ProjectWorkerAdapter::Omp] {
        let root = tempfile::tempdir().unwrap();
        let (mut host, mut draft) = super::project_planning::tests::fixture(root.path());
        draft.worker_adapter = adapter;
        host.configure_project(draft).unwrap();
        let saved = host.project_setups["project"].clone();
        let value = serde_json::to_value(&saved.setup).unwrap();
        if adapter == ProjectWorkerAdapter::Codex {
            assert!(value.get("workerAdapter").is_none());
            assert!(value["worker"].get("ompRuntimeParent").is_none());
            let old: ProjectSetup = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(digest(&old).unwrap(), saved.digest);
        } else {
            assert_eq!(value["workerAdapter"], "omp");
            assert_eq!(saved.setup.worker.as_ref().unwrap().provider, "deepseek");
        }
        let mut proposal = super::project_planning::tests::proposal("tests");
        if let ManagementDecision::Plan { tasks, .. } = &mut proposal {
            tasks[0].worker_id = adapter.worker_id();
        }
        super::project_planning::tests::complete(&mut host, proposal);
        let revision = host.projects["project"].revision().unwrap();
        assert_eq!(revision.execution[0].adapter_id, adapter.id());
        assert_eq!(
            revision.execution[0].worker.as_ref(),
            saved.setup.worker.as_ref()
        );
        assert!(host.projects["project"].approval().is_none());
        let wanted = revision.approval_digest().unwrap();
        assert!(host
            .start_project_run("project", &wanted, &saved.digest)
            .is_err());
        host.approve("project", &wanted).unwrap();
        assert!(host
            .start_project_run("project", &wanted, &saved.digest)
            .unwrap_err()
            .contains("正常执行必须"));
        assert!(host.runs.all().is_empty());
        drop(host);
        let host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(host.project_setups["project"].digest, saved.digest);
        assert_eq!(host.project_setups["project"].setup.worker_adapter, adapter);
    }
}
#[test]
fn omp_configuration_rejects_unsupported_supervision_and_wrong_worker_plan() {
    let root = tempfile::tempdir().unwrap();
    let (mut host, mut draft) = super::project_planning::tests::fixture(root.path());
    draft.worker_adapter = ProjectWorkerAdapter::Omp;
    draft.supervision = Some(cerulean_domain::supervision::SupervisionPolicy {
        schema_version: 2,
        repeated_failure_threshold: 2,
        max_advice_per_attempt: 1,
        delivery_enabled: true,
    });
    assert!(host.configure_project(draft.clone()).is_err());
    assert!(host.project_setups.is_empty());
    draft.supervision.as_mut().unwrap().schema_version = 1;
    draft.supervision.as_mut().unwrap().delivery_enabled = false;
    host.configure_project(draft.clone()).unwrap();
    assert!(
        !host.project_setups["project"]
            .setup
            .policy
            .supervision
            .as_ref()
            .unwrap()
            .delivery_enabled
    );
    draft.supervision = None;
    let mut bad = draft.clone();
    bad.codex_executable = root
        .path()
        .join("project/worker.exe")
        .to_string_lossy()
        .into();
    fs::write(&bad.codex_executable, b"fixture").unwrap();
    assert!(host.configure_project(bad).is_err());
    host.configure_project(draft).unwrap();
    super::project_planning::tests::complete(
        &mut host,
        super::project_planning::tests::proposal("tests"),
    );
    assert!(host.projects["project"].revision().is_none());
    assert!(!host.planning_errors.is_empty());
}
#[test]
#[ignore = "isolated independent OMP DAG validator"]
fn omp_dag_validator() {
    if Path::new("base.txt").exists() {
        assert_eq!(
            fs::read_to_string("base.txt").unwrap(),
            "committed dependency base\n"
        );
        assert!(!Path::new(".git").exists());
    }
    let text = fs::read_to_string("result.txt").unwrap();
    assert!(matches!(
        text.as_str(),
        "fixture output" | "fixture output second"
    ));
}
#[test]
#[ignore = "explicit offline worker and isolated parent; actual OMP DAG and cancellation, zero provider calls"]
fn actual_omp_project_dag_and_cancellation() {
    run_project_dag(false)
}
#[test]
#[ignore = "real Git; resident OMP dependency worktrees with restart and cancellation, offline peer only"]
fn dependency_worktree_omp_dag_restart_and_cancel() {
    run_project_dag(true)
}
fn run_project_dag(isolated: bool) {
    let parent = PathBuf::from(std::env::var_os("CERULEAN_OMP_TEST_PARENT").unwrap());
    let exe = PathBuf::from(std::env::var_os("CERULEAN_OMP_FIXTURE_EXE").unwrap());
    assert_eq!(exe.file_name().unwrap(), "omp_offline_worker_fixture.exe");
    let shared_runtime = tempfile::tempdir_in(&parent).unwrap();
    for (external, cancel) in [(false, false), (false, true), (true, false), (true, true)] {
        let test = tempfile::tempdir_in(&parent).unwrap();
        let root = tempfile::tempdir_in(test.path()).unwrap();
        let (mut host, mut draft) = super::project_planning::tests::fixture(root.path());
        draft.worker_adapter = ProjectWorkerAdapter::Omp;
        if external {
            fs::create_dir(root.path().join(".codex")).unwrap();
            fs::write(root.path().join("AGENTS.md"), "unapproved ancestor context").unwrap();
            fs::write(root.path().join("CLAUDE.md"), "unapproved ancestor context").unwrap();
            draft.omp_runtime_parent = Some(shared_runtime.path().to_string_lossy().into());
        }
        draft.codex_executable = exe.to_string_lossy().into();
        draft.worker_model = if cancel {
            "fixture-file-wait"
        } else {
            "fixture-file-dag"
        }
        .into();
        draft.artifact_paths = vec!["result.txt".into()];
        draft.check_args = vec![
            "--ignored".into(),
            "--exact".into(),
            "host::omp_project_tests::omp_dag_validator".into(),
        ];
        draft.management_max_requests = 5;
        draft.worker_max_requests = 2;
        if isolated {
            draft.worktree = Some(dependency_host_fixture::configure(root.path()));
        }
        host.configure_project(draft).unwrap();
        crate::credential_vault::save_for(
            &host.root,
            credentials::CredentialProvider::DeepSeek,
            &secret::SecretInput::new("cerulean-offline-placeholder-not-a-real-key".into())
                .unwrap(),
        )
        .unwrap();
        host.deepseek_key_configured = true;
        enable(&mut host);
        let setup = host.project_setups["project"].digest.clone();
        host.request_chief_plan("project", &setup).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while host.projects["project"].revision().is_none() && Instant::now() < deadline {
            collect(&mut host);
            std::thread::sleep(Duration::from_millis(5));
        }
        let revision = host.projects["project"]
            .revision()
            .unwrap_or_else(|| panic!("planning failed: {:?}", host.snapshot()))
            .approval_digest()
            .unwrap();
        assert!(host.projects["project"].approval().is_none());
        assert!(host.omp_jobs.is_empty());
        host.approve("project", &revision).unwrap();
        assert!(host
            .start_omp("project", "b", "premature-b", &revision)
            .is_err());
        assert!(host.omp_startups.all().is_empty());
        let execution_plan = host.projects["project"].revision().unwrap().clone();
        host.start_project_run("project", &revision, &setup)
            .unwrap();
        let mut restarted = false;
        let mut cancelled = false;
        let deadline = Instant::now() + Duration::from_secs(if isolated { 90 } else { 45 });
        loop {
            assert!(
                Instant::now() < deadline,
                "DAG timeout: {:?}",
                host.snapshot()
            );
            collect(&mut host);
            let cancel_output = if isolated {
                host.snapshot().omp_jobs.first().map(|job| {
                    PathBuf::from(execution_plan.execution_root("a", &job.attempt_id).unwrap())
                        .join("result.txt")
                })
            } else {
                Some(root.path().join("project/result.txt"))
            };
            if cancel && !cancelled && cancel_output.is_some_and(|p| p.exists()) {
                let id = host.snapshot().omp_jobs[0].attempt_id.clone();
                host.cancel_omp("project", &id, "stop current OMP and future work")
                    .unwrap();
                cancelled = true;
                assert_eq!(host.runs.all()[0].state, RunState::Paused);
                assert!(host
                    .start_project_run("project", &revision, &setup)
                    .is_err());
            }
            if !cancel
                && !restarted
                && host
                    .projects
                    .get("project")
                    .is_some_and(|s| s.state().tasks["a"].verified)
                && host.active.is_empty()
                && host.omp_active.is_empty()
            {
                let run = host.runs.all()[0].clone();
                host.pause_project_run(&run.id).unwrap();
                let requests = host.management.statuses().len();
                let events = host.projects["project"].history().len();
                drop(host);
                host = RuntimeHost::open(&root.path().join("data")).unwrap();
                enable(&mut host);
                host.poll_verifications();
                assert_eq!(host.management.statuses().len(), requests);
                assert_eq!(host.projects["project"].history().len(), events);
                assert_eq!(host.runs.all()[0].state, RunState::Paused);
                host.start_project_run("project", &revision, &setup)
                    .unwrap();
                restarted = true;
            }
            host.poll_project_runs();
            let run = host.runs.all()[0].clone();
            if host.omp_active.is_empty()
                && host.active.is_empty()
                && host.management_active.is_empty()
                && run.state != RunState::Active
            {
                assert_eq!(
                    run.state,
                    if cancel {
                        RunState::Paused
                    } else {
                        RunState::Completed
                    },
                    "{:?}",
                    host.snapshot()
                );
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let history = host.projects["project"].history();
        assert_eq!(
            history
                .iter()
                .filter(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. }))
                .count(),
            if cancel { 1 } else { 2 }
        );
        if cancel {
            assert!(cancelled);
            assert_eq!(
                host.projects["project"].state().tasks["b"].state,
                TaskState::Pending
            );
        } else {
            assert!(restarted);
            assert_eq!(host.jobs.len(), 2);
            assert!(host.projects["project"]
                .state()
                .tasks
                .values()
                .all(|t| t.state == TaskState::Accepted));
            assert_eq!(
                fs::read_to_string(
                    dependency_host_fixture::task_path(&host, "b").join("result.txt")
                )
                .unwrap(),
                "fixture output second"
            );
            let accepted=history.iter().find(|e|matches!(&e.body.event.command,WorkflowCommand::Accept{task,..} if task=="a")).unwrap().body.event.sequence;
            let assigned=history.iter().find(|e|matches!(&e.body.event.command,WorkflowCommand::Assign{task,..} if task=="b")).unwrap().body.event.sequence;
            assert!(accepted < assigned);
            if isolated {
                dependency_host_fixture::assert_delivery(
                    &host,
                    "fixture output",
                    "fixture output second",
                );
            }
        }
        let events = history.len();
        let requests = host.management.statuses().len();
        drop(host);
        let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
        enable(&mut host);
        host.poll_verifications();
        assert_eq!(host.projects["project"].history().len(), events);
        assert_eq!(host.management.statuses().len(), requests);
        if external {
            assert!(!host.root.join("omp-profiles").exists());
            let parent = host.project_setups["project"]
                .setup
                .worker
                .as_ref()
                .unwrap()
                .omp_runtime_parent
                .as_ref()
                .unwrap();
            let profiles = Path::new(parent).join("cerulean-omp-profiles");
            assert_eq!(
                fs::read_dir(&profiles).unwrap().count(),
                if cancel { 3 } else { 2 }
            );
            for job in host.omp_jobs.values() {
                assert_eq!(job.worker.omp_runtime_parent.as_ref(), Some(parent));
                let id = digest(&(
                    host.root.to_str().unwrap(),
                    job.project_id.as_str(),
                    job.attempt_id.as_str(),
                ))
                .unwrap();
                assert!(profiles
                    .join(id)
                    .join("workspace/fixture-ready.json")
                    .exists());
                assert_eq!(
                    host.omp_startups
                        .get(&job.project_id, &job.attempt_id)
                        .unwrap()
                        .intent
                        .worker,
                    job.worker
                );
            }
        }
    }
}

#[test]
#[ignore = "explicit configuration-free runtime parent required; no model calls"]
fn isolated_runtime_configuration_scope_and_approval() {
    let parent = PathBuf::from(std::env::var_os("CERULEAN_OMP_TEST_PARENT").unwrap());
    let test = tempfile::tempdir_in(parent).unwrap();
    let user = test.path().join("user");
    fs::create_dir(&user).unwrap();
    let (mut host, mut draft) = super::project_planning::tests::fixture(&user);
    draft.worker_adapter = ProjectWorkerAdapter::Omp;
    for name in ["runtime-a", "runtime-b"] {
        fs::create_dir(test.path().join(name)).unwrap();
    }
    draft.omp_runtime_parent = Some(test.path().join("runtime-a").to_string_lossy().into());
    host.configure_project(draft.clone()).unwrap();
    let first = host.project_setups["project"].clone();
    draft.omp_runtime_parent = Some(test.path().join("runtime-b").to_string_lossy().into());
    host.configure_project(draft.clone()).unwrap();
    let second = host.project_setups["project"].clone();
    assert_ne!(first.digest, second.digest);
    let mut proposal = super::project_planning::tests::proposal("tests");
    if let ManagementDecision::Plan { tasks, .. } = &mut proposal {
        tasks[0].worker_id = "worker-omp".into();
    }
    super::project_planning::tests::complete(&mut host, proposal);
    let plan = host.projects["project"].revision().unwrap().clone();
    let mut other = plan.clone();
    other.execution[0]
        .worker
        .as_mut()
        .unwrap()
        .omp_runtime_parent = first.setup.worker.unwrap().omp_runtime_parent;
    assert_ne!(
        plan.approval_digest().unwrap(),
        other.approval_digest().unwrap()
    );
    for path in [
        user.join("project"),
        user.join("data"),
        test.path().to_owned(),
    ] {
        assert!(host
            .validate_omp_runtime_parent(path.to_str().unwrap())
            .is_err());
    }
    let runtime = test.path().join("runtime-b");
    assert!(host
        .register(Project {
            id: "overlap".into(),
            root: runtime.to_string_lossy().into(),
            timezone: "Asia/Shanghai".into()
        })
        .is_err());
    fs::write(runtime.join("AGENTS.md"), "later context").unwrap();
    assert!(host
        .validate_omp_runtime_parent(runtime.to_str().unwrap())
        .is_err());
    fs::remove_file(runtime.join("AGENTS.md")).unwrap();
    drop(host);
    let mut host = RuntimeHost::open(&user.join("data")).unwrap();
    assert_eq!(host.project_setups["project"].digest, second.digest);
    assert!(host.omp_startups.all().is_empty());
    assert!(!runtime.join("cerulean-omp-profiles").exists());
    // A filesystem failure before spawn must release the resident project;
    // it must not leave a fictitious running/unknown worker occupying it.
    fs::write(runtime.join("cerulean-omp-profiles"), "not a directory").unwrap();
    crate::credential_vault::save_for(
        &host.root,
        credentials::CredentialProvider::DeepSeek,
        &secret::SecretInput::new("cerulean-offline-placeholder-not-a-real-key".into()).unwrap(),
    )
    .unwrap();
    let wanted = plan.approval_digest().unwrap();
    host.approve("project", &wanted).unwrap();
    let task = &plan.workflow.tasks[0].id;
    host.start_omp("project", task, "bad-runtime", &wanted)
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !host.omp_active.is_empty() && Instant::now() < deadline {
        collect(&mut host);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(host.omp_active.is_empty());
    assert_eq!(
        host.omp_jobs.values().next().unwrap().state,
        omp_jobs::OmpJobState::Failed
    );
    assert_eq!(
        host.projects["project"].state().tasks[task].state,
        TaskState::Pending
    );
    assert!(!host.projects["project"]
        .history()
        .iter()
        .any(|e| e.body.event.command.attempt() == Some("bad-runtime")));
    let evidence = host.snapshot().omp_jobs[0].closure_digest.clone().unwrap();
    host.abandon_omp_startup(
        "project",
        "bad-runtime",
        &evidence,
        "directory failure before launch",
    )
    .unwrap();
    drop(host);
    let host = RuntimeHost::open(&user.join("data")).unwrap();
    assert_eq!(
        host.omp_jobs.values().next().unwrap().state,
        omp_jobs::OmpJobState::Abandoned
    );
}
