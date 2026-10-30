//! One approved project across three real host routes; only model peers are synthetic.
use super::*;
use cerulean_domain::project_setup::{AcceptanceDraft, AcceptanceStageDraft, WorkerProfileDraft};

pub(super) fn mixed_manager(
    i: crate::management_execution::DecisionIntent,
) -> Result<DecisionObservation, String> {
    let context = i.context.clone();
    let mut result = manager(i)?;
    match &mut result.envelope.decision {
        ManagementDecision::Plan { tasks, .. } => {
            for task in tasks {
                let profile = context["executionSetup"]["setup"]["profiles"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|p| p["id"] == format!("profile-{}", task.id))
                    .unwrap();
                task.profile_id = Some(profile["id"].as_str().unwrap().into());
                task.worker_id = format!("worker-{}", profile["workerAdapter"].as_str().unwrap());
            }
        }
        ManagementDecision::Assign {
            task_id, worker_id, ..
        } => {
            let task = context["workflow"]["revision"]["execution"]
                .as_array()
                .unwrap()
                .iter()
                .find(|e| e["taskId"] == *task_id)
                .unwrap();
            *worker_id = format!("worker-{}", task["adapterId"].as_str().unwrap());
        }
        _ => {}
    }
    Ok(result)
}

fn mixed_host(root: &Path, runtime: &Path, order: [ProjectWorkerAdapter; 3]) -> RuntimeHost {
    mixed_host_mode(root, runtime, order, false)
}
pub(super) fn mixed_host_mode(
    root: &Path,
    runtime: &Path,
    order: [ProjectWorkerAdapter; 3],
    parallel: bool,
) -> RuntimeHost {
    mixed_host_options(root, runtime, order, parallel, false)
}
pub(super) fn mixed_host_options(
    root: &Path,
    runtime: &Path,
    order: [ProjectWorkerAdapter; 3],
    parallel: bool,
    supervision: bool,
) -> RuntimeHost {
    mixed_host_options_scoped(root, runtime, order, parallel, supervision, false)
}
pub(super) fn mixed_host_scoped(
    root: &Path,
    runtime: &Path,
    order: [ProjectWorkerAdapter; 3],
) -> RuntimeHost {
    mixed_host_options_scoped(root, runtime, order, true, false, true)
}
fn mixed_host_options_scoped(
    root: &Path,
    runtime: &Path,
    order: [ProjectWorkerAdapter; 3],
    parallel: bool,
    supervision: bool,
    scoped: bool,
) -> RuntimeHost {
    mixed_host_complete(root, runtime, order, parallel, supervision, scoped, false)
}
pub(super) fn mixed_host_external(
    root: &Path,
    runtime: &Path,
    order: [ProjectWorkerAdapter; 3],
) -> RuntimeHost {
    mixed_host_complete(root, runtime, order, true, false, false, true)
}
fn mixed_host_complete(
    root: &Path,
    runtime: &Path,
    order: [ProjectWorkerAdapter; 3],
    parallel: bool,
    supervision: bool,
    scoped: bool,
    external: bool,
) -> RuntimeHost {
    if scoped {
        crate::reader_generation::tests::promote_fixture(&root.join("data"));
    }
    let bin = root.join("bin");
    fs::create_dir(&bin).unwrap();
    let codex = bin.join("codex.exe");
    let source = PathBuf::from(
        std::env::var_os("CERULEAN_DAG_EXE").expect("explicit offline Codex fixture"),
    );
    assert_eq!(source.file_name().unwrap(), "codex_dag_fixture.exe");
    fs::copy(source, &codex).unwrap();
    fs::write(
        bin.join("workspace.json"),
        serde_json::to_vec(&root.join("project")).unwrap(),
    )
    .unwrap();
    fs::write(
        bin.join("worktree-pool.json"),
        serde_json::to_vec(&root.join("pool")).unwrap(),
    )
    .unwrap();
    let omp = PathBuf::from(
        std::env::var_os("CERULEAN_OMP_FIXTURE_EXE").expect("explicit offline OMP fixture"),
    );
    assert_eq!(omp.file_name().unwrap(), "omp_offline_worker_fixture.exe");
    let mut selected = None;
    let mut host = configured_with(root, "mixed-cli-api", |draft| {
        if supervision {
            assert!(order.iter().all(|a| *a == ProjectWorkerAdapter::Codex));
            draft.worker_adapter = ProjectWorkerAdapter::Codex;
            draft.api_worker = None;
            draft.codex_executable = codex.to_string_lossy().into_owned();
            draft.worker_model = "fixture-worker".into();
            draft.supervision = Some(cerulean_domain::supervision::SupervisionPolicy {
                schema_version: 1,
                repeated_failure_threshold: 2,
                max_advice_per_attempt: 2,
                delivery_enabled: true,
            });
        }
        for (id, adapter) in ["a", "b", "integration"].into_iter().zip(order) {
            let api = adapter.is_api().then(|| draft.api_worker.clone().unwrap());
            let (model, exe) = match adapter {
                ProjectWorkerAdapter::Omp => (
                    if parallel {
                        "fixture-file-integration-gated"
                    } else {
                        "fixture-file-integration"
                    }
                    .into(),
                    omp.to_string_lossy().into_owned(),
                ),
                ProjectWorkerAdapter::Codex => (
                    "fixture-worker".into(),
                    codex.to_string_lossy().into_owned(),
                ),
                _ => (api.as_ref().unwrap().model.clone(), String::new()),
            };
            draft.profiles.push(WorkerProfileDraft {
                supervision_override: scoped.then(|| {
                    use cerulean_domain::supervision::{SupervisionOverride, SupervisionPolicy};
                    if adapter == ProjectWorkerAdapter::Codex {
                        SupervisionOverride::Enabled {
                            policy: SupervisionPolicy {
                                schema_version: 1,
                                repeated_failure_threshold: 2,
                                max_advice_per_attempt: 1,
                                delivery_enabled: true,
                            },
                        }
                    } else {
                        SupervisionOverride::Disabled {}
                    }
                }),
                // The original serial/restart comparator now declares its mutual exclusion explicitly.
                resource_keys: if parallel {
                    vec![]
                } else {
                    vec!["serial-mixed-comparator".into()]
                },
                id: format!("profile-{id}"),
                description: format!("Implement {id} using {}", adapter.id()),
                worker_adapter: adapter,
                codex_executable: exe,
                worker_model: model,
                omp_runtime_parent: (adapter == ProjectWorkerAdapter::Omp)
                    .then(|| runtime.to_string_lossy().into_owned()),
                api_worker: api,
                desktop_worker: None,
                artifact_paths: vec![format!("{id}.txt")],
                acceptance: AcceptanceDraft {
                    schema_version: 1,
                    stages: vec![AcceptanceStageDraft {
                        id: format!("check-{id}"),
                        name: format!("Check {id}"),
                        validator_id: "tests".into(),
                        args: draft.check_args.clone(),
                        timeout_seconds: 30,
                    }],
                },
            });
        }
        if external {
            selected = Some(draft.clone());
        }
    });
    if external {
        use cerulean_domain::project_setup::{ChiefSource, DeputySource, ManagementSources};
        drop(host);
        crate::management_source_guard::tests::promote_fixture(&root.join("data"));
        // Current API dispatch requires measured/rejected-usage protection.
        // Opt this NEW fixture in through the ordinary cold-backup promotion;
        // do not weaken the production guard or change an existing run.
        crate::rejected_usage_guard::tests::promote_fixture(
            &root.join("data"),
            &root.join("reader17-before-dispatch-backup"),
        );
        host = RuntimeHost::open(&root.join("data")).unwrap();
        let mut draft = selected.unwrap();
        draft.management_sources = Some(ManagementSources {
            chief: ChiefSource::CodexSession {
                session_id: "current-session".into(),
            },
            deputy: DeputySource::OpenAi {},
        });
        draft.chief_model = "external-codex-session".into();
        host.configure_project(draft).unwrap();
    }
    host.set_provider_key(
        CredentialProvider::DeepSeek,
        Some(
            secret::SecretInput::new("cerulean-offline-placeholder-not-a-real-key".into()).unwrap(),
        ),
    )
    .unwrap();
    host.management_fixture = Some(mixed_manager);
    host
}

#[test]
#[ignore = "real Git, resident Codex/OMP/API in one project; explicit offline peers and new reports directory"]
fn three_routes_share_one_plan_with_each_final_integrator_and_restart() {
    use ProjectWorkerAdapter::{Codex, Deepseek, Omp};
    let outer =
        fs::canonicalize(std::env::var_os("CERULEAN_MIXED_DIR").expect("new reports directory"))
            .unwrap();
    let reports =
        fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports")).unwrap();
    assert!(outer.starts_with(&reports) && outer != reports);
    let parent = fs::canonicalize(
        std::env::var_os("CERULEAN_OMP_TEST_PARENT").expect("clean runtime parent"),
    )
    .unwrap();
    for (name, order) in [
        ("final-api", [Omp, Codex, Deepseek]),
        ("final-codex", [Deepseek, Omp, Codex]),
        ("final-omp", [Codex, Deepseek, Omp]),
    ] {
        let root = outer.join(name);
        fs::create_dir(&root).unwrap();
        let runtime = tempfile::Builder::new()
            .prefix("mixed-omp-")
            .tempdir_in(&parent)
            .unwrap();
        let mut host = mixed_host(&root, runtime.path(), order);
        let plan = planned(&mut host);
        for (id, adapter) in ["a", "b", "integration"].into_iter().zip(order) {
            assert_eq!(plan.task(id).unwrap().adapter_id, adapter.id());
            assert_eq!(
                plan.task(id).unwrap().profile_id.as_deref(),
                Some(format!("profile-{id}").as_str())
            );
        }
        host.approve("project", &plan.approval_digest().unwrap())
            .unwrap();
        let setup = host.project_setups["project"].digest.clone();
        host.start_project_run("project", &plan.approval_digest().unwrap(), &setup)
            .unwrap();
        let mut restarted = false;
        let limit = Instant::now() + Duration::from_secs(180);
        loop {
            host.poll_verifications();
            if !restarted
                && host
                    .projects
                    .get("project")
                    .is_some_and(|s| s.state().tasks["a"].state == TaskState::Accepted)
            {
                assert_eq!(
                    host.projects["project"].state().tasks["b"].state,
                    TaskState::Pending
                );
                let history = host.projects["project"].history().to_vec();
                let run = host.runs.all()[0].clone();
                host.pause_project_run(&run.id).unwrap();
                assert!(host.can_shutdown());
                drop(host);
                host = RuntimeHost::open(&root.join("data")).unwrap();
                assert_eq!(host.projects["project"].history(), history);
                assert_eq!(host.runs.all()[0].state, RunState::Paused);
                host.management_fixture = Some(mixed_manager);
                host.api_fixture = Some(worker);
                host.start_project_run("project", &run.revision_digest, &run.setup_digest)
                    .unwrap();
                restarted = true;
            }
            if host.runs.all()[0].state != RunState::Active {
                break;
            }
            assert!(
                Instant::now() < limit,
                "mixed timeout {:?}",
                host.runs.all()
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        fs::write(
            root.join("snapshot.json"),
            serde_json::to_vec_pretty(&host.snapshot()).unwrap(),
        )
        .unwrap();
        assert_eq!(
            host.runs.all()[0].state,
            RunState::Completed,
            "{:?}",
            host.runs.all()
        );
        assert!(restarted);
        let store = &host.projects["project"];
        assert!(store
            .state()
            .tasks
            .values()
            .all(|t| t.state == TaskState::Accepted && t.verified));
        assert_eq!(host.api_jobs.len(), 1);
        assert_eq!(host.codex_jobs.len(), 1);
        assert_eq!(host.omp_jobs.len(), 1);
        assert_eq!(host.management.statuses().len(), 7);
        for id in ["a", "b", "integration"] {
            assert_eq!(store.history().iter().filter(|e| matches!(&e.body.event.command, WorkflowCommand::DispatchIntent {task, ..} if task==id)).count(), 1);
            assert_eq!(store.history().iter().filter(|e| matches!(&e.body.event.command, WorkflowCommand::Accept {task, ..} if task==id)).count(), 1);
        }
        let path = dependency_host_fixture::task_path(&host, "integration");
        assert_eq!(fs::read_to_string(path.join("bundle.txt")).unwrap(), "AB");
        assert_eq!(
            fs::read_to_string(root.join("project/base.txt")).unwrap(),
            "uncommitted source change\n"
        );
        assert!(!root.join("project/bundle.txt").exists());
        let protocol: Vec<serde_json::Value> = fs::read_to_string(root.join("bin/protocol.jsonl"))
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(
            protocol
                .iter()
                .filter(|e| e["method"] == "turn/start")
                .count(),
            1
        );
        let history = store.history().to_vec();
        assert!(host.can_shutdown());
        drop(host);
        let mut reopened = RuntimeHost::open(&root.join("data")).unwrap();
        reopened.poll_verifications();
        assert_eq!(reopened.projects["project"].history(), history);
        assert_eq!(reopened.runs.all()[0].state, RunState::Completed);
        assert_eq!(reopened.project_setups["project"].digest, setup);
        fs::write(root.join("result.json"), serde_json::to_vec_pretty(&json!({"case":name,"status":"passed","accepted":3,"dispatchesPerTask":1,"managementRequests":7,"intermediateRestart":true,"finalReopenNewEvents":0,"realModelCalls":0,"finalDirectory":path,"scope":"real resident hosts, Git and verifiers; synthetic model peers"})).unwrap()).unwrap();
        println!("mixed {name}: 3 routes, 3 accepted, one dispatch each, intermediate restart, final AB, reopen events 0");
    }
}

fn write_then_fail(request: &PreparedRequest, round: u32) -> Result<ApiReply, ApiFailure> {
    if round == 1 {
        worker(request, round)
    } else {
        Err(ApiFailure::Http { status: 503 })
    }
}

#[test]
#[ignore = "real Git and Codex/API; provider failure after actual write must not dispatch downstream OMP"]
fn api_failure_after_cli_acceptance_retains_unknown_and_blocks_final_omp() {
    use ProjectWorkerAdapter::{Codex, Deepseek, Omp};
    let outer =
        fs::canonicalize(std::env::var_os("CERULEAN_MIXED_DIR").expect("reports directory"))
            .unwrap();
    let reports =
        fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports")).unwrap();
    assert!(outer.starts_with(&reports) && outer != reports);
    let root = outer.join("failure-after-cli");
    fs::create_dir(&root).unwrap();
    let parent = fs::canonicalize(
        std::env::var_os("CERULEAN_OMP_TEST_PARENT").expect("clean runtime parent"),
    )
    .unwrap();
    let runtime = tempfile::Builder::new()
        .prefix("mixed-omp-")
        .tempdir_in(parent)
        .unwrap();
    let mut host = mixed_host(&root, runtime.path(), [Codex, Deepseek, Omp]);
    host.api_fixture = Some(write_then_fail);
    let plan = planned(&mut host);
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
    fs::write(
        root.join("snapshot.json"),
        serde_json::to_vec_pretty(&host.snapshot()).unwrap(),
    )
    .unwrap();
    assert_eq!(host.runs.all()[0].state, RunState::Attention);
    let store = &host.projects["project"];
    assert_eq!(store.state().tasks["a"].state, TaskState::Accepted);
    assert!(!store.state().tasks["b"].verified);
    assert_eq!(store.state().tasks["integration"].state, TaskState::Pending);
    assert!(host.omp_jobs.is_empty() && host.omp_startups.all().is_empty());
    assert_eq!(host.codex_jobs.len(), 1);
    assert_eq!(host.api_jobs.len(), 1);
    assert_eq!(
        host.snapshot().api_jobs[0].state,
        cerulean_domain::api_jobs::ApiJobState::Unknown
    );
    assert_eq!(host.management.statuses().len(), 4);
    let written = dependency_host_fixture::task_path(&host, "b");
    assert_eq!(
        fs::read_to_string(written.join("result.txt")).unwrap(),
        "alpha"
    );
    assert!(!written.join("b.txt").exists());
    let history = store.history().to_vec();
    let run = host.runs.all()[0].clone();
    drop(host);
    let mut reopened = RuntimeHost::open(&root.join("data")).unwrap();
    for _ in 0..3 {
        reopened.poll_verifications();
    }
    assert_eq!(reopened.projects["project"].history(), history);
    assert!(reopened
        .start_project_run("project", &run.revision_digest, &run.setup_digest)
        .is_err());
    assert_eq!(
        reopened.snapshot().api_jobs[0].state,
        cerulean_domain::api_jobs::ApiJobState::Unknown
    );
    assert!(reopened.omp_jobs.is_empty() && reopened.omp_startups.all().is_empty());
    assert_eq!(reopened.management.statuses().len(), 4);
    assert_eq!(
        fs::read_to_string(written.join("result.txt")).unwrap(),
        "alpha"
    );
    fs::write(root.join("result.json"), serde_json::to_vec_pretty(&json!({"status":"passed","case":"failure-after-cli","accepted":1,"managementRequests":4,"apiState":"unknown","finalOmpDispatches":0,"partialWritePreserved":true,"resumeBlocked":true,"finalReopenNewEvents":0,"realModelCalls":0})).unwrap()).unwrap();
    println!("mixed failure-after-cli: Codex accepted, API partial write retained unknown, final OMP dispatches 0, resume blocked, reopen events 0");
}
