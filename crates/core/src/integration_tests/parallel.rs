//! User-run authorization -> automatic parallel dispatch -> original verification
//! and chief acceptance. Real Git/files/processes; only model replies controlled.
use super::*;
use cerulean_domain::project_setup::{AcceptanceDraft, AcceptanceStageDraft, WorkerProfileDraft};
use std::io::{BufRead, Read, Write};
use std::net::TcpListener;
use std::sync::{mpsc, Mutex};

type Gate = (mpsc::Sender<()>, mpsc::Receiver<()>);
static GATES: Mutex<BTreeMap<String, Gate>> = Mutex::new(BTreeMap::new());
static LOCAL_TCP_PORT: Mutex<Option<u16>> = Mutex::new(None);
static LOCAL_SQLITE_DB: Mutex<Option<PathBuf>> = Mutex::new(None);
pub(super) fn gated(request: &PreparedRequest, round: u32) -> Result<ApiReply, ApiFailure> {
    wait_gate(request)?;
    worker(request, round)
}
pub(super) fn wait_gate(request: &PreparedRequest) -> Result<(), ApiFailure> {
    let ApiMessage::User(prompt) = &fixture_messages(request)[0] else {
        panic!("prompt");
    };
    let task = if prompt.contains("INTEGRATION_a") {
        "a"
    } else if prompt.contains("INTEGRATION_b") {
        "b"
    } else if prompt.contains("INTEGRATION_c") {
        "c"
    } else {
        "integration"
    };
    let gate = GATES.lock().unwrap().remove(task);
    if let Some((entered, resume)) = gate {
        entered.send(()).unwrap();
        resume.recv_timeout(Duration::from_secs(60)).unwrap();
    }
    if task == "a" && prompt.contains("fail-parallel") {
        return Err(ApiFailure::Http { status: 503 });
    }
    Ok(())
}
pub(super) fn gate(task: &str) -> (mpsc::Receiver<()>, mpsc::Sender<()>) {
    let (entered, wait) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    assert!(GATES
        .lock()
        .unwrap()
        .insert(task.into(), (entered, resume))
        .is_none());
    (wait, release)
}

fn local_tcp_gated(request: &PreparedRequest, round: u32) -> Result<ApiReply, ApiFailure> {
    if round == 1 {
        let ApiMessage::User(prompt) = &fixture_messages(request)[0] else {
            panic!("approved prompt missing");
        };
        let task = if prompt.contains("INTEGRATION_a") {
            "a"
        } else if prompt.contains("INTEGRATION_b") {
            "b"
        } else {
            "integration"
        };
        if matches!(task, "a" | "b") {
            let port = *LOCAL_TCP_PORT.lock().unwrap();
            let listener = TcpListener::bind(("127.0.0.1", port.unwrap_or(0)))
                .expect("approved task must acquire its actual local TCP port");
            if task == "a" {
                assert!(port.is_none(), "fresh local TCP test port required");
                *LOCAL_TCP_PORT.lock().unwrap() = Some(listener.local_addr().unwrap().port());
            } else {
                assert_eq!(Some(listener.local_addr().unwrap().port()), port);
            }
            let (entered, resume) = GATES.lock().unwrap().remove(task).expect("task gate");
            entered.send(()).unwrap();
            resume.recv_timeout(Duration::from_secs(60)).unwrap();
            drop(listener);
        }
    }
    worker(request, round)
}

fn sqlite_helper_script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/resource-sqlite-lock.mjs")
}

fn sqlite_helper(mode: &str, db: &Path) -> String {
    let node =
        std::env::var_os("CERULEAN_TEST_NODE").expect("explicit Node with node:sqlite required");
    let output = std::process::Command::new(node)
        .arg(sqlite_helper_script())
        .arg(mode)
        .arg(db)
        .output()
        .expect("start isolated SQLite helper");
    assert!(
        output.status.success(),
        "SQLite {mode} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}

fn sqlite_unknown_gated(request: &PreparedRequest, round: u32) -> Result<ApiReply, ApiFailure> {
    if round == 1 {
        let ApiMessage::User(prompt) = &fixture_messages(request)[0] else {
            panic!("approved prompt missing");
        };
        if prompt.contains("INTEGRATION_b") {
            panic!("B must not reach its worker while A is Unknown");
        }
        if prompt.contains("INTEGRATION_c") {
            wait_gate(request)?;
        }
        if prompt.contains("INTEGRATION_a") {
            let db = LOCAL_SQLITE_DB
                .lock()
                .unwrap()
                .clone()
                .expect("test SQLite path");
            let node = std::env::var_os("CERULEAN_TEST_NODE")
                .expect("explicit Node with node:sqlite required");
            let mut child = std::process::Command::new(node)
                .arg(sqlite_helper_script())
                .arg("hold")
                .arg(&db)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .expect("start isolated SQLite holder");
            let mut stdout = std::io::BufReader::new(child.stdout.take().unwrap());
            let mut line = String::new();
            stdout.read_line(&mut line).unwrap();
            assert_eq!(line.trim(), "READY", "SQLite holder must own the file lock");
            let (entered, resume) = GATES.lock().unwrap().remove("a").expect("A gate");
            entered.send(()).unwrap();
            resume.recv_timeout(Duration::from_secs(60)).unwrap();
            let mut stdin = child.stdin.take().unwrap();
            stdin.write_all(b"release\n").unwrap();
            drop(stdin);
            line.clear();
            stdout.read_line(&mut line).unwrap();
            assert_eq!(line.trim(), "RELEASED", "SQLite file lock must be closed");
            let status = child.wait().unwrap();
            let mut stderr = String::new();
            child
                .stderr
                .take()
                .unwrap()
                .read_to_string(&mut stderr)
                .unwrap();
            assert!(status.success(), "SQLite holder failed: {stderr}");
            return Err(ApiFailure::Http { status: 503 });
        }
    }
    worker(request, round)
}
fn running(host: &mut RuntimeHost, done: impl Fn(&RuntimeHost) -> bool) {
    spin(host, |h| {
        assert_eq!(
            h.runs.all()[0].state,
            RunState::Active,
            "{:?}; jobs={:?}; fault={:?}",
            h.runs.all(),
            h.snapshot().api_jobs,
            h.api_fault
        );
        done(h)
    });
}

#[test]
#[ignore = "real Git and independent verifier; automatic parallel API run with controlled gates"]
fn automatic_parallel_run_completes_or_preserves_failure_without_downstream_dispatch() {
    for mode in ["parallel-success", "fail-parallel"] {
        let root = temporary();
        let mut host = configured(root.path(), mode);
        host.api_fixture = Some(gated);
        let plan = planned(&mut host);
        host.approve("project", &plan.approval_digest().unwrap())
            .unwrap();
        let setup = host.project_setups["project"].digest.clone();
        let (a_wait, a_release) = gate("a");
        let (b_wait, b_release) = gate("b");
        host.start_project_run("project", &plan.approval_digest().unwrap(), &setup)
            .unwrap();
        running(&mut host, |_| a_wait.try_recv().is_ok());
        running(&mut host, |_| b_wait.try_recv().is_ok());
        assert_eq!(host.api_active.len(), 2);
        assert_eq!(host.api_writers.len(), 1);
        assert_eq!(host.management.statuses().len(), 3);
        for _ in 0..20 {
            host.poll_verifications();
        }
        assert_eq!(
            host.management.statuses().len(),
            3,
            "no management billing while slots/dependencies blocked"
        );
        assert!(host.jobs.is_empty());
        a_release.send(()).unwrap();
        if mode == "fail-parallel" {
            spin(&mut host, |h| h.runs.all()[0].state == RunState::Attention);
            assert_eq!(host.management.statuses().len(), 3);
            assert!(!host
                .api_startups
                .values()
                .any(|s| s.attempt.task_id == "integration"));
            b_release.send(()).unwrap();
            spin(&mut host, |h| h.api_active.is_empty());
            assert_eq!(
                host.projects["project"].state().tasks["a"].state,
                TaskState::Unknown
            );
            assert_eq!(
                host.projects["project"].state().tasks["b"].state,
                TaskState::Submitted
            );
            assert!(host.jobs.is_empty());
        } else {
            running(&mut host, |h| {
                h.jobs
                    .values()
                    .any(|j| j.task_id == "a" && j.state == VerificationState::Succeeded)
                    && h.active.is_empty()
                    && h.snapshot().workflows[0].state.tasks["a"].state == TaskState::Accepted
            });
            assert_eq!(
                host.api_active.len(),
                1,
                "B remains gated during A verification"
            );
            assert_eq!(host.jobs.len(), 1);
            let view = host.snapshot();
            assert_eq!(
                view.workflows.len(),
                1,
                "one authoritative project projection"
            );
            assert!(view.workflows[0].state.tasks["a"].verified);
            let operation = host
                .jobs
                .values()
                .find(|j| j.task_id == "a")
                .unwrap()
                .operation_id
                .clone();
            let detail = host
                .management_read(&Command::ReadVerificationDetail {
                    project_id: "project".into(),
                    operation_id: operation,
                })
                .unwrap()
                .unwrap()
                .verification_detail
                .unwrap();
            assert!(detail.ledger_recorded);
            assert_eq!(detail.state, VerificationState::Succeeded);
            assert_eq!(
                host.management.statuses().len(),
                4,
                "chief accepts A while B still owns the writer"
            );
            let run = host.runs.all()[0].clone();
            host.pause_project_run(&run.id).unwrap();
            b_release.send(()).unwrap();
            spin(&mut host, |h| h.api_active.is_empty());
            assert_eq!(host.runs.all()[0].state, RunState::Paused);
            assert_eq!(host.management.statuses().len(), 4);
            assert_eq!(host.jobs.len(), 1);
            drop(host);
            host = RuntimeHost::open(&root.path().join("data")).unwrap();
            host.management_fixture = Some(manager);
            host.api_fixture = Some(worker);
            host.start_project_run("project", &plan.approval_digest().unwrap(), &setup)
                .unwrap();
            spin(&mut host, |h| h.runs.all()[0].state != RunState::Active);
            assert_eq!(
                host.runs.all()[0].state,
                RunState::Completed,
                "{:?}",
                host.runs.all()
            );
            assert_eq!(host.management.statuses().len(), 7);
            assert_eq!(host.jobs.len(), 3);
            assert!(host.projects["project"]
                .state()
                .tasks
                .values()
                .all(|t| t.state == TaskState::Accepted && t.verified));
            assert_eq!(
                fs::read_to_string(
                    dependency_host_fixture::task_path(&host, "integration").join("bundle.txt")
                )
                .unwrap(),
                "AB"
            );
        }
        let history = host.projects["project"].history().to_vec();
        let dispatches: Vec<_> = history
            .iter()
            .filter_map(|e| match &e.body.event.command {
                WorkflowCommand::DispatchIntent { task, .. } => Some(task.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(dispatches.iter().filter(|t| *t == "a").count(), 1);
        assert_eq!(dispatches.iter().filter(|t| *t == "b").count(), 1);
        assert_eq!(
            dispatches.iter().filter(|t| *t == "integration").count(),
            usize::from(mode == "parallel-success")
        );
        drop(host);
        let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
        host.poll_verifications();
        assert_eq!(host.projects["project"].history(), history);
        println!("automatic {mode}: two API workers from user run, one writer, no duplicate dispatch, dependency/slot wait without billing, reopen events 0");
    }
}

#[test]
#[ignore = "real Git and independent verifier; shared resource serializes automatic API tasks"]
fn shared_resource_waits_without_new_management_request_then_completes() {
    let root = temporary();
    let mut host = configured_with(root.path(), "parallel-resource-wait", |draft| {
        draft.resource_keys = vec!["shared-database".into()];
    });
    host.api_fixture = Some(gated);
    let plan = planned(&mut host);
    host.approve("project", &plan.approval_digest().unwrap())
        .unwrap();
    let setup = host.project_setups["project"].digest.clone();
    let (wait, release) = gate("a");
    host.start_project_run("project", &plan.approval_digest().unwrap(), &setup)
        .unwrap();
    running(&mut host, |_| wait.try_recv().is_ok());
    for _ in 0..20 {
        host.poll_verifications();
    }
    assert_eq!(host.api_active.len(), 1);
    assert_eq!(host.management.statuses().len(), 2);
    assert_eq!(host.runs.all()[0].state, RunState::Active);
    release.send(()).unwrap();
    spin(&mut host, |h| h.runs.all()[0].state != RunState::Active);
    assert_eq!(
        host.runs.all()[0].state,
        RunState::Completed,
        "{:?}; jobs={:?}; fault={:?}",
        host.runs.all(),
        host.snapshot().api_jobs,
        host.api_fault
    );
    assert_eq!(host.management.statuses().len(), 7);
    println!("resource wait: one worker and two management records while occupied, then three accepted tasks and seven management records total");
}

#[test]
#[ignore = "real Git, localhost TCP socket and independent verifier; explicit fresh reports directory"]
fn approved_named_resource_prevents_real_local_tcp_overlap() {
    let root = PathBuf::from(
        std::env::var_os("CERULEAN_RESOURCE_TCP_294_DIR").expect("fresh evidence directory"),
    );
    let reports =
        fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports")).unwrap();
    assert_eq!(
        root.parent().unwrap().canonicalize().unwrap().parent(),
        Some(reports.as_path()),
        "evidence case must be under a dedicated reports directory"
    );
    assert!(!root.exists(), "preserve any prior local TCP case");
    fs::create_dir(&root).unwrap();
    assert!(LOCAL_TCP_PORT.lock().unwrap().is_none());

    let mut host = configured_with(&root, "real-local-tcp-resource", |draft| {
        draft.resource_keys = vec!["tcp-local-294".into()];
    });
    host.api_fixture = Some(local_tcp_gated);
    let plan = planned(&mut host);
    host.approve("project", &plan.approval_digest().unwrap())
        .unwrap();
    let setup = host.project_setups["project"].digest.clone();
    let (a_wait, a_release) = gate("a");
    let (b_wait, b_release) = gate("b");
    host.start_project_run("project", &plan.approval_digest().unwrap(), &setup)
        .unwrap();
    running(&mut host, |_| a_wait.try_recv().is_ok());
    let port = LOCAL_TCP_PORT
        .lock()
        .unwrap()
        .expect("A must own a real port");
    assert!(TcpListener::bind(("127.0.0.1", port)).is_err());
    assert_eq!(dispatch_count(&host, "a"), 1);
    assert_eq!(dispatch_count(&host, "b"), 0);
    assert!(!host.api_startups.values().any(|s| s.attempt.task_id == "b"));
    assert!(b_wait.try_recv().is_err());
    for _ in 0..20 {
        host.poll_verifications();
    }
    assert_eq!(host.management.statuses().len(), 2);

    a_release.send(()).unwrap();
    running(&mut host, |_| b_wait.try_recv().is_ok());
    assert_eq!(
        host.snapshot().workflows[0].state.tasks["a"].state,
        TaskState::Accepted
    );
    assert_eq!(dispatch_count(&host, "b"), 1);
    assert!(TcpListener::bind(("127.0.0.1", port)).is_err());
    b_release.send(()).unwrap();
    spin(&mut host, |h| {
        h.runs.all()[0].state == RunState::Completed && h.projects.contains_key("project")
    });
    assert_eq!(dispatch_count(&host, "integration"), 1);
    assert!(host.projects["project"]
        .state()
        .tasks
        .values()
        .all(|t| t.state == TaskState::Accepted && t.verified));
    assert_eq!(
        fs::read_to_string(
            dependency_host_fixture::task_path(&host, "integration").join("bundle.txt")
        )
        .unwrap(),
        "AB"
    );
    let _released_port = TcpListener::bind(("127.0.0.1", port)).unwrap();
    let history = host.projects["project"].history().to_vec();
    drop(host);
    let reopened = RuntimeHost::open(&root.join("data")).unwrap();
    assert_eq!(reopened.projects["project"].history(), history);
    drop(reopened);
    fs::write(
        root.join("result.json"),
        serde_json::to_vec_pretty(&json!({
            "status":"passed",
            "actualTcpPort":port,
            "actualTcpBindRejectedWhileAAndBActive":true,
            "bDispatchesWhileAHeldPort":0,
            "dispatches":{"a":1,"b":1,"integration":1},
            "realGit":true,"independentVerifier":true,
            "realModelCalls":0,"realDesktopActions":0,
            "resourceSemantics":"same workbench core only; no cross-process or database lock"
        }))
        .unwrap(),
    )
    .unwrap();
    *LOCAL_TCP_PORT.lock().unwrap() = None;
}

#[test]
#[ignore = "real Git, isolated SQLite file lock and independent verifier; Unknown retains the named lease after the OS lock closes"]
fn approved_named_resource_preserves_sqlite_unknown_after_file_lock_release() {
    let root = PathBuf::from(
        std::env::var_os("CERULEAN_RESOURCE_SQLITE_DIR").expect("fresh evidence directory"),
    );
    let reports =
        fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports")).unwrap();
    assert_eq!(
        root.parent().unwrap().canonicalize().unwrap().parent(),
        Some(reports.as_path()),
        "SQLite case must be under a dedicated reports directory"
    );
    assert!(
        !root.exists(),
        "preserve each original SQLite resource case"
    );
    assert!(sqlite_helper_script().is_file());
    fs::create_dir(&root).unwrap();
    let db = root.join("shared-resource.sqlite");
    assert_eq!(sqlite_helper("init", &db), "INITIALIZED");
    assert_eq!(sqlite_helper("probe", &db), "FREE");
    {
        let mut slot = LOCAL_SQLITE_DB.lock().unwrap();
        assert!(slot.is_none(), "a previous SQLite case is still active");
        *slot = Some(db.clone());
    }

    let mut draft = None;
    let mut host = resource_named_host(
        &root,
        &mut draft,
        "real-local-sqlite-unknown",
        &["db-local-sqlite-file"],
    );
    host.management_fixture = Some(resource_matrix_manager);
    host.api_fixture = Some(sqlite_unknown_gated);
    let plan = planned(&mut host);
    for task in ["a", "b"] {
        assert_eq!(
            plan.workflow
                .tasks
                .iter()
                .find(|entry| entry.id == task)
                .unwrap()
                .resource_keys,
            Some(vec!["db-local-sqlite-file".into()])
        );
    }
    let approval = plan.approval_digest().unwrap();
    host.approve("project", &approval).unwrap();
    let setup = host.project_setups["project"].digest.clone();
    let (a_wait, a_release) = gate("a");
    let (c_wait, c_release) = gate("c");
    host.start_project_run("project", &approval, &setup)
        .unwrap();
    running(&mut host, |_| a_wait.try_recv().is_ok());
    running(&mut host, |_| c_wait.try_recv().is_ok());
    assert_eq!(
        sqlite_helper("probe", &db),
        "BUSY",
        "A owns a real SQLite write lock"
    );
    assert_eq!(dispatch_count(&host, "a"), 1);
    assert_eq!(dispatch_count(&host, "b"), 0);
    assert!(!host.api_startups.values().any(|s| s.attempt.task_id == "b"));
    assert!(host
        .task_resource_conflict("project", "b", "candidate-b")
        .unwrap()
        .is_some());
    c_release.send(()).unwrap();
    running(&mut host, |h| {
        h.snapshot().workflows[0].state.tasks["c"].state == TaskState::Accepted
    });
    assert_eq!(dispatch_count(&host, "b"), 0);

    a_release.send(()).unwrap();
    spin(&mut host, |h| {
        h.runs.all()[0].state == RunState::Attention
            && h.api_active.is_empty()
            && h.projects.contains_key("project")
    });
    assert_eq!(
        sqlite_helper("probe", &db),
        "FREE",
        "A closed its OS transaction"
    );
    assert_eq!(
        host.projects["project"].state().tasks["a"].state,
        TaskState::Unknown
    );
    assert_eq!(
        host.projects["project"].state().tasks["b"].state,
        TaskState::Pending
    );
    assert!(
        host.task_resource_conflict("project", "b", "candidate-b")
            .unwrap()
            .is_some(),
        "Unknown keeps the durable logical lease after the file unlocks"
    );
    assert_eq!(dispatch_count(&host, "b"), 0);
    let original_history = host.projects["project"].history().to_vec();
    let old_attempt = host
        .api_startups
        .values()
        .find(|startup| startup.attempt.task_id == "a")
        .unwrap()
        .attempt
        .id
        .clone();
    drop(host);

    let data = root.join("data");
    let mut host = RuntimeHost::open(&data).unwrap();
    host.poll_verifications();
    assert_eq!(host.projects["project"].history(), original_history);
    assert_eq!(
        host.projects["project"].state().tasks["a"].state,
        TaskState::Unknown
    );
    assert_eq!(sqlite_helper("probe", &db), "FREE");
    assert!(host
        .task_resource_conflict("project", "b", "candidate-b")
        .unwrap()
        .is_some());
    assert!(host.recover_api("project", &old_attempt).is_err());
    assert!(host
        .start_project_run("project", &approval, &setup)
        .is_err());
    assert_eq!(dispatch_count(&host, "b"), 0);
    assert_eq!(host.projects["project"].history(), original_history);

    let retirement = host
        .snapshot()
        .api_jobs
        .into_iter()
        .find(|job| job.attempt_id == old_attempt)
        .unwrap()
        .retirement_digest
        .expect("original Unknown review");
    host.retire_unknown_api(
        "project",
        &old_attempt,
        &retirement,
        "SQLite OS lock released, but original API outcome is unknown; keep evidence and require a new approved revision",
    )
    .unwrap();
    assert_eq!(
        host.projects["project"].state().tasks["a"].state,
        TaskState::Retired
    );
    assert!(host
        .task_resource_conflict("project", "b", "candidate-b")
        .unwrap()
        .is_none());
    assert_eq!(dispatch_count(&host, "b"), 0);
    assert_eq!(sqlite_helper("probe", &db), "FREE");
    let retired_history = host.projects["project"].history().to_vec();
    drop(host);
    let reopened = RuntimeHost::open(&data).unwrap();
    assert_eq!(reopened.projects["project"].history(), retired_history);
    assert_eq!(dispatch_count(&reopened, "b"), 0);
    assert!(reopened
        .task_resource_conflict("project", "b", "candidate-b")
        .unwrap()
        .is_none());
    drop(reopened);
    fs::write(
        root.join("result.json"),
        serde_json::to_vec_pretty(&json!({
            "status":"passed",
            "physicalSQLiteBusyWhileAHeldTransaction":true,
            "physicalSQLiteFreeAfterAUnknown":true,
            "logicalLeaseHeldDuringUnknownAndColdReopen":true,
            "logicalLeaseReleasedOnlyAfterExplicitRetirement":true,
            "bDispatches":0,
            "originalAttemptResends":0,
            "realModelCalls":0,
            "realDesktopActions":0,
            "scope":"single workbench core named lease plus isolated SQLite file lock; no product index.sqlite"
        }))
        .unwrap(),
    )
    .unwrap();
    *LOCAL_SQLITE_DB.lock().unwrap() = None;
}

fn resource_matrix_manager(
    intent: crate::management_execution::DecisionIntent,
) -> Result<DecisionObservation, String> {
    let mut observation = three_branch_manager(intent)?;
    if let ManagementDecision::Plan { tasks, .. } = &mut observation.envelope.decision {
        for task in tasks
            .iter_mut()
            .filter(|task| matches!(task.id.as_str(), "a" | "b"))
        {
            task.profile_id = Some("shared-resource".into());
        }
    }
    Ok(observation)
}

fn resource_matrix_host(
    root: &Path,
    draft: &mut Option<cerulean_domain::project_setup::ProjectSetupDraft>,
) -> RuntimeHost {
    resource_named_host(
        root,
        draft,
        "resource-matrix-fail-parallel",
        &["tcp-resource-matrix-290", "db-resource-matrix-290"],
    )
}

fn resource_named_host(
    root: &Path,
    draft: &mut Option<cerulean_domain::project_setup::ProjectSetupDraft>,
    requirement: &str,
    names: &[&str],
) -> RuntimeHost {
    let resource_keys = names.iter().map(|name| (*name).into()).collect();
    configured_with(root, requirement, |setup| {
        setup.profiles.push(WorkerProfileDraft {
            worker_reasoning_effort: None,
            supervision_override: None,
            resource_keys,
            id: "shared-resource".into(),
            description: "Two branches sharing approved local resource names".into(),
            worker_adapter: ProjectWorkerAdapter::Deepseek,
            codex_executable: String::new(),
            worker_model: setup.worker_model.clone(),
            omp_runtime_parent: None,
            api_worker: setup.api_worker.clone(),
            desktop_worker: None,
            artifact_paths: setup.artifact_paths.clone(),
            acceptance: AcceptanceDraft {
                schema_version: 1,
                stages: vec![AcceptanceStageDraft {
                    expected_assertions: vec![],
                    id: "check".into(),
                    name: "Check isolated branch".into(),
                    validator_id: "tests".into(),
                    args: setup.check_args.clone(),
                    timeout_seconds: 30,
                }],
            },
        });
        *draft = Some(setup.clone());
    })
}

fn dispatch_count(host: &RuntimeHost, task: &str) -> usize {
    // The API coordinator owns the project store while any branch is active.
    // Its journal entries are atomically published, so inspect the original
    // persisted events instead of indexing the temporarily vacant host map.
    fs::read_dir(host.root.join("workflows/project/events"))
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            if path.extension().and_then(std::ffi::OsStr::to_str) != Some("json") {
                return 0;
            }
            let event: crate::scheduler::Entry =
                serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
            usize::from(matches!(&event.body.event.command, WorkflowCommand::DispatchIntent { task: id, .. } if id == task))
        })
        .sum()
}

#[test]
#[ignore = "real Git and original API runner; declared resource contention, unknown and explicit revised-plan continuation"]
fn declared_resource_matrix_survives_unknown_then_requires_new_approval() {
    let root = PathBuf::from(
        std::env::var_os("CERULEAN_RESOURCE_MATRIX_290_DIR").expect("fresh evidence directory"),
    );
    let reports =
        fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports")).unwrap();
    assert_eq!(
        root.parent().unwrap().canonicalize().unwrap().parent(),
        Some(reports.as_path())
    );
    assert!(
        !root.exists(),
        "preserve each original resource matrix case"
    );
    fs::create_dir(&root).unwrap();
    let mut saved_draft = None;
    let mut host = resource_matrix_host(&root, &mut saved_draft);
    host.management_fixture = Some(resource_matrix_manager);
    host.api_fixture = Some(gated);
    let original = planned(&mut host);
    for task in ["a", "b"] {
        assert_eq!(
            original
                .workflow
                .tasks
                .iter()
                .find(|t| t.id == task)
                .unwrap()
                .resource_keys,
            Some(vec![
                "tcp-resource-matrix-290".into(),
                "db-resource-matrix-290".into()
            ])
        );
    }
    assert!(original
        .workflow
        .tasks
        .iter()
        .find(|t| t.id == "c")
        .unwrap()
        .resource_keys
        .is_none());
    let old_approval = original.approval_digest().unwrap();
    host.approve("project", &old_approval).unwrap();
    let old_setup = host.project_setups["project"].digest.clone();
    let (a_wait, a_release) = gate("a");
    let (b_wait, b_release) = gate("b");
    let (c_wait, c_release) = gate("c");
    host.start_project_run("project", &old_approval, &old_setup)
        .unwrap();
    running(&mut host, |_| a_wait.try_recv().is_ok());
    running(&mut host, |_| c_wait.try_recv().is_ok());
    assert_eq!(host.api_active.len(), 2);
    assert_eq!(host.api_writers.len(), 1);
    assert!(host
        .task_resource_conflict("project", "b", "candidate-b")
        .unwrap()
        .is_some());
    assert!(!host.api_startups.values().any(|s| s.attempt.task_id == "b"));
    assert!(b_wait.try_recv().is_err());
    c_release.send(()).unwrap();
    running(&mut host, |h| {
        h.snapshot().workflows[0].state.tasks["c"].state == TaskState::Accepted
    });
    assert_eq!(host.api_active.len(), 1);
    assert_eq!(dispatch_count(&host, "a"), 1);
    assert_eq!(dispatch_count(&host, "c"), 1);
    assert_eq!(dispatch_count(&host, "b"), 0);
    assert_eq!(dispatch_count(&host, "integration"), 0);
    a_release.send(()).unwrap();
    spin(&mut host, |h| {
        h.runs.all()[0].state == RunState::Attention
            && h.api_active.is_empty()
            && h.projects.contains_key("project")
    });
    let original_history = host.projects["project"].history().to_vec();
    let old_attempt = host
        .api_startups
        .values()
        .find(|s| s.attempt.task_id == "a")
        .unwrap()
        .attempt
        .id
        .clone();
    assert_eq!(
        host.projects["project"].state().tasks["a"].state,
        TaskState::Unknown
    );
    assert_eq!(
        host.projects["project"].state().tasks["b"].state,
        TaskState::Pending
    );
    assert_eq!(
        host.projects["project"].state().tasks["c"].state,
        TaskState::Accepted
    );
    assert!(host
        .task_resource_conflict("project", "b", "candidate-b")
        .unwrap()
        .is_some());
    assert!(!host.api_startups.values().any(|s| s.attempt.task_id == "b"));
    fs::write(
        root.join("unknown-checkpoint.json"),
        serde_json::to_vec_pretty(&json!({
            "originalApproval":old_approval,"originalSetup":old_setup,"unknownAttempt":old_attempt,
            "aDispatches":1,"bDispatches":0,"cDispatches":1,"integrationDispatches":0,
            "realModelCalls":0,"namedResourceScope":"one workbench core"
        }))
        .unwrap(),
    )
    .unwrap();
    drop(host);

    let data = root.join("data");
    let mut host = RuntimeHost::open(&data).unwrap();
    host.poll_verifications();
    assert_eq!(host.projects["project"].history(), original_history);
    assert_eq!(
        host.projects["project"].state().tasks["a"].state,
        TaskState::Unknown
    );
    assert!(host
        .task_resource_conflict("project", "b", "candidate-b")
        .unwrap()
        .is_some());
    assert!(
        host.recover_api("project", &old_attempt).is_err(),
        "503 left no sealed completion"
    );
    assert!(host
        .start_project_run("project", &old_approval, &old_setup)
        .is_err());
    assert_eq!(dispatch_count(&host, "b"), 0);
    assert_eq!(host.projects["project"].history(), original_history);
    drop(host);
    let mut host = RuntimeHost::open(&data).unwrap();
    assert_eq!(host.projects["project"].history(), original_history);
    assert!(host
        .task_resource_conflict("project", "b", "candidate-b")
        .unwrap()
        .is_some());
    let retirement = host
        .snapshot()
        .api_jobs
        .into_iter()
        .find(|j| j.attempt_id == old_attempt)
        .unwrap()
        .retirement_digest
        .expect("original unknown review");
    host.retire_unknown_api("project", &old_attempt, &retirement,
        "controlled original API request unknown; retain evidence and require a newly approved revision")
        .unwrap();
    assert_eq!(
        host.projects["project"].state().tasks["a"].state,
        TaskState::Retired
    );
    assert!(host
        .task_resource_conflict("project", "b", "candidate-b")
        .unwrap()
        .is_none());
    assert_eq!(dispatch_count(&host, "b"), 0);
    let review = host
        .project_revision_candidates()
        .into_iter()
        .find(|r| r.project_id == "project")
        .expect("retired attempt can be reviewed for a new plan");
    host.request_project_revision(
        "project",
        &review.evidence_digest,
        "explicitly supersede the failed plan; no automatic resend of the original attempt",
    )
    .unwrap();
    let mut draft = saved_draft.unwrap();
    draft.requirement = "resource-matrix-reapproved".into();
    host.configure_project(draft).unwrap();
    host.management_fixture = Some(resource_matrix_manager);
    host.api_fixture = Some(gated);
    let revised_setup = host.project_setups["project"].digest.clone();
    host.request_chief_plan("project", &revised_setup).unwrap();
    spin(&mut host, |h| {
        h.projects
            .get("project")
            .and_then(|store| store.revision())
            .is_some_and(|revision| revision.workflow.version == original.workflow.version + 1)
    });
    let revised = host.projects["project"].revision().unwrap().clone();
    assert_eq!(revised.workflow.version, 2);
    let new_approval = revised.approval_digest().unwrap();
    assert_ne!(new_approval, old_approval);
    host.approve("project", &new_approval).unwrap();
    let new_setup = host.project_setups["project"].digest.clone();
    let (new_a_wait, new_a_release) = gate("a");
    let (new_c_wait, new_c_release) = gate("c");
    host.start_project_run("project", &new_approval, &new_setup)
        .unwrap();
    spin(&mut host, |_| new_a_wait.try_recv().is_ok());
    spin(&mut host, |_| new_c_wait.try_recv().is_ok());
    assert_eq!(host.api_active.len(), 2);
    assert_eq!(dispatch_count(&host, "b"), 0);
    assert!(host
        .task_resource_conflict("project", "b", "candidate-b")
        .unwrap()
        .is_some());
    new_c_release.send(()).unwrap();
    spin(&mut host, |h| {
        h.snapshot().workflows[0].state.tasks["c"].state == TaskState::Accepted
    });
    assert_eq!(dispatch_count(&host, "b"), 0);
    new_a_release.send(()).unwrap();
    spin(&mut host, |_| b_wait.try_recv().is_ok());
    assert_eq!(
        host.snapshot().workflows[0].state.tasks["a"].state,
        TaskState::Accepted
    );
    assert_eq!(dispatch_count(&host, "b"), 1);
    assert_eq!(dispatch_count(&host, "integration"), 0);
    b_release.send(()).unwrap();
    spin(&mut host, |h| {
        h.runs
            .all()
            .iter()
            .any(|r| r.revision_digest == new_approval && r.state == RunState::Completed)
            && h.projects.contains_key("project")
    });
    assert!(host.projects["project"]
        .state()
        .tasks
        .values()
        .all(|t| t.state == TaskState::Accepted && t.verified));
    assert_eq!(dispatch_count(&host, "a"), 2);
    assert_eq!(dispatch_count(&host, "b"), 1);
    assert_eq!(dispatch_count(&host, "c"), 2);
    assert_eq!(dispatch_count(&host, "integration"), 1);
    assert_eq!(
        fs::read_to_string(
            dependency_host_fixture::task_path(&host, "integration").join("bundle.txt")
        )
        .unwrap(),
        "AB"
    );
    let final_history = host.projects["project"].history().to_vec();
    fs::write(root.join("result.json"), serde_json::to_vec_pretty(&json!({
        "oldApproval":old_approval,"newApproval":new_approval,"oldAttempt":old_attempt,
        "oldRun":"attention then superseded","newRun":"completed",
        "dispatches":{"a":2,"b":1,"c":2,"integration":1},
        "originalAttemptResends":0,"realModelCalls":0,"realDesktopActions":0,
        "resourceSemantics":"explicit named keys within one workbench core; no OS port or DB lock"
    })).unwrap()).unwrap();
    drop(host);
    let host = RuntimeHost::open(&data).unwrap();
    assert_eq!(host.projects["project"].history(), final_history);
    assert_eq!(dispatch_count(&host, "b"), 1);
    assert_eq!(
        host.projects["project"].state().tasks["integration"].state,
        TaskState::Accepted
    );
}

pub(super) fn three_branch_manager(
    i: crate::management_execution::DecisionIntent,
) -> Result<DecisionObservation, String> {
    let mut reply = manager(i)?;
    if let ManagementDecision::Plan { tasks, .. } = &mut reply.envelope.decision {
        let mut third = tasks[0].clone();
        third.id = "c".into();
        third.goal = "INTEGRATION_c third independent task".into();
        tasks
            .iter_mut()
            .find(|t| t.id == "integration")
            .unwrap()
            .dependencies
            .push("c".into());
        tasks.push(third);
    }
    Ok(reply)
}
#[test]
#[ignore = "real Git; three independent tasks prove two model slots and automatic refill"]
fn third_ready_task_waits_without_billing_then_refills_freed_slot() {
    refill(false);
}
#[test]
#[ignore = "real Git; chief acceptance unlocks dependent API while the other branch runs"]
fn chief_acceptance_unlocks_dependency_while_other_branch_runs() {
    refill(true);
}
fn dependent_manager(
    i: crate::management_execution::DecisionIntent,
) -> Result<DecisionObservation, String> {
    let mut reply = three_branch_manager(i)?;
    if let ManagementDecision::Plan { tasks, .. } = &mut reply.envelope.decision {
        tasks.iter_mut().find(|t| t.id == "c").unwrap().dependencies = vec!["a".into()];
    }
    Ok(reply)
}
fn refill(dependent: bool) {
    let root = temporary();
    let mut host = configured(root.path(), "parallel-capacity");
    host.management_fixture = Some(if dependent {
        dependent_manager
    } else {
        three_branch_manager
    });
    host.api_fixture = Some(gated);
    let plan = planned(&mut host);
    host.approve("project", &plan.approval_digest().unwrap())
        .unwrap();
    let setup = host.project_setups["project"].digest.clone();
    let (a_wait, a_release) = gate("a");
    let (b_wait, b_release) = gate("b");
    let (c_wait, c_release) = gate("c");
    host.start_project_run("project", &plan.approval_digest().unwrap(), &setup)
        .unwrap();
    running(&mut host, |_| a_wait.try_recv().is_ok());
    running(&mut host, |_| b_wait.try_recv().is_ok());
    for _ in 0..20 {
        host.poll_verifications();
    }
    assert_eq!(host.api_active.len(), 2);
    assert_eq!(host.management.statuses().len(), 3);
    assert!(host.api_startups.values().all(|s| s.attempt.task_id != "c"));
    a_release.send(()).unwrap();
    running(&mut host, |h| {
        assert!(h.api_active.len() <= 2);
        c_wait.try_recv().is_ok()
    });
    assert_eq!(host.api_active.len(), 2);
    assert!(host.api_active.values().any(|a| a.job.task_id == "b"));
    assert!(host.api_active.values().any(|a| a.job.task_id == "c"));
    assert_eq!(
        host.management
            .statuses()
            .iter()
            .filter(|s| s.purpose == Purpose::Assign)
            .count(),
        3
    );
    assert!(matches!(host.management.statuses().len(), 4 | 5));
    if dependent {
        assert_eq!(
            host.snapshot().workflows[0].state.tasks["a"].state,
            TaskState::Accepted
        );
        assert_eq!(host.management.statuses().len(), 5);
    }
    b_release.send(()).unwrap();
    c_release.send(()).unwrap();
    spin(&mut host, |h| {
        assert!(h.api_active.len() <= 2);
        h.runs.all()[0].state != RunState::Active && h.api_active.is_empty()
    });
    if host.runs.all()[0].state != RunState::Completed {
        fs::write(
            root.path().join("failure-snapshot.json"),
            serde_json::to_vec_pretty(&host.snapshot()).unwrap(),
        )
        .unwrap();
        let saved = root.keep();
        panic!(
            "capacity failed; original fixture preserved at {}: {:?}; {:?}",
            saved.display(),
            host.runs.all(),
            host.snapshot().api_jobs
        );
    }
    assert_eq!(
        host.runs.all()[0].state,
        RunState::Completed,
        "{:?}; jobs={:?}; fault={:?}",
        host.runs.all(),
        host.snapshot().api_jobs,
        host.api_fault
    );
    assert_eq!(host.management.statuses().len(), 9);
    assert_eq!(host.jobs.len(), 4);
    let store = &host.projects["project"];
    assert!(store
        .state()
        .tasks
        .values()
        .all(|t| t.state == TaskState::Accepted && t.verified));
    assert_eq!(
        store
            .history()
            .iter()
            .filter(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. }))
            .count(),
        4
    );
    let history = store.history().to_vec();
    drop(host);
    let host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(host.projects["project"].history(), history);
    println!("capacity refill dependent={dependent}: third ready task waits with zero reservation/billing; freed slot starts it while second still runs; max active 2, four accepted, nine management requests, reopen events 0");
}
