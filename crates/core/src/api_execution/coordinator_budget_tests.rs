use super::*;
use crate::workflow_coordinator::{Scope, WorkflowClient, WorkflowCoordinator};
use std::sync::{Arc, Barrier};

// Actual approved Git worktrees and journal publication; no model or file tools.
fn temporary() -> tempfile::TempDir {
    let root =
        fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports")).unwrap();
    tempfile::Builder::new()
        .prefix("dispatch-budget-")
        .tempdir_in(root)
        .unwrap()
}
fn isolate(root: &Path, plan: &mut PlanRevision) {
    let executable = std::env::var("CERULEAN_TEST_GIT").expect("explicit Git fixture required");
    let pin = PinnedCheck::describe(Path::new(&executable), &[], Default::default())
        .unwrap()
        .executable;
    plan.project.root = fs::canonicalize(&plan.project.root)
        .unwrap()
        .to_string_lossy()
        .into();
    let git = |args: &[&str]| {
        let mut command = std::process::Command::new(&pin.path);
        command
            .env_clear()
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "NUL")
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(std::process::Stdio::null());
        for key in ["SystemRoot", "WINDIR", "TEMP", "TMP"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        let output = command
            .current_dir(&plan.project.root)
            .args(["-c", "core.hooksPath=NUL", "-c", "core.fsmonitor=false"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    git(&["init", "--initial-branch=main"]);
    fs::write(root.join("project/base.txt"), "approved base").unwrap();
    git(&["add", "--all"]);
    git(&[
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@localhost",
        "commit",
        "--no-gpg-sign",
        "-m",
        "base",
    ]);
    let base_commit = git(&["rev-parse", "HEAD"]);
    fs::create_dir(root.join("pool")).unwrap();
    plan.worktree = Some(GitWorktreePolicy {
        integration: None,
        schema_version: 1,
        base_commit,
        pool_root: fs::canonicalize(root.join("pool"))
            .unwrap()
            .to_string_lossy()
            .into(),
        git: pin,
        input: GitWorktreeInput::CommittedRawBlobs,
    });
    plan.snapshot_policy.as_mut().unwrap().exclude = vec![".git".into()];
}
fn fixture(root: &Path, budget: u32, rounds: u32) -> (WorkflowCoordinator, Vec<Scope>) {
    fixture_with_omp(root, budget, rounds, false)
}
fn fixture_with_omp(
    root: &Path,
    budget: u32,
    rounds: u32,
    omp: bool,
) -> (WorkflowCoordinator, Vec<Scope>) {
    fixture_with_cli(root, budget, rounds, omp.then_some("omp"))
}
fn fixture_with_cli(
    root: &Path,
    budget: u32,
    rounds: u32,
    cli: Option<&str>,
) -> (WorkflowCoordinator, Vec<Scope>) {
    let mut worker = selection(targets().remove(2));
    worker.max_rounds = rounds;
    let mut plan = plan(root, worker);
    plan.policy.max_requests = budget;
    isolate(root, &mut plan);
    plan.workflow.tasks[0].workspace_key = isolated_task_key("project", "task").unwrap();
    for index in 2..=3 {
        let mut task = plan.workflow.tasks[0].clone();
        task.id = format!("task-{index}");
        task.workspace_key = isolated_task_key("project", &task.id).unwrap();
        let mut execution = plan.execution[0].clone();
        execution.task_id = task.id.clone();
        if let Some(adapter) = cli.filter(|_| index == 3) {
            execution.adapter_id = adapter.into();
            execution.api_worker = None;
            execution.worker = Some(WorkerSelection {
                reasoning_effort: None,
                omp_runtime_parent: None,
                provider: if adapter == "codex" {
                    "openai"
                } else {
                    "deepseek"
                }
                .into(),
                model: "fixture".into(),
                executable: execution.checks[0]
                    .tool
                    .as_ref()
                    .unwrap()
                    .executable
                    .clone(),
            });
        }
        plan.workflow.tasks.push(task);
        plan.execution.push(execution);
    }
    let mut store = assigned(root, plan);
    let original = store.attempt_record("task", "attempt").unwrap().clone();
    for index in 2..=3 {
        let mut attempt = original.clone();
        attempt.task_id = format!("task-{index}");
        attempt.id = format!("attempt-{index}");
        let (workspace, proof) = store
            .prepare_task_workspace(&attempt.task_id, &attempt.id)
            .unwrap();
        attempt.binding.workspace = workspace;
        attempt.workspace_proof = Some(proof);
        attempt.binding.session_id = format!("session-{index}");
        attempt.binding.adapter_id = store
            .revision()
            .unwrap()
            .task(&attempt.task_id)
            .unwrap()
            .adapter_id
            .clone();
        attempt.binding.identity_strength =
            IdentityStrength::ControllerRequest(ControllerRequestIdentity {
                connection_id: format!("connection-{index}"),
                request_id: format!("request-{index}"),
            });
        if cli == Some("codex") && index == 3 {
            attempt.binding.identity_strength = IdentityStrength::ProviderId;
        }
        record(
            &mut store,
            "deputy",
            WorkflowCommand::Assign {
                task: attempt.task_id.clone(),
                worker: attempt.worker_id.clone(),
                attempt: attempt.id.clone(),
            },
            CommandContext {
                execution: Some(ExecutionEvidence::Attempt { attempt }),
                ..Default::default()
            },
        )
        .unwrap();
    }
    let mut scopes = Vec::new();
    for (task, attempt) in [
        ("task", "attempt"),
        ("task-2", "attempt-2"),
        ("task-3", "attempt-3"),
    ] {
        if cli.is_none() || task != "task-3" {
            let (_, _, identity, _) = scope(&store, task, attempt).unwrap();
            let path = store.api_attempt_path(attempt).unwrap();
            fs::create_dir(&path).unwrap();
            publish(&path, "identity.json", &identity).unwrap();
        }
        scopes.push(Scope {
            project: "project".into(),
            task: task.into(),
            attempt: attempt.into(),
            revision: original.revision_digest.clone(),
        });
    }
    (
        WorkflowCoordinator::start(store, root.join("ledger")).unwrap(),
        scopes,
    )
}

fn begin(client: &mut WorkflowClient, scope: &Scope) -> Result<u32, String> {
    client.begin_api_dispatch(&scope.task, &scope.attempt)
}

fn begin_omp(client: &mut WorkflowClient, scope: &Scope) -> Result<(), String> {
    use crate::omp_workflow_access::OmpWorkflowAccess;
    let attempt = client.attempt_record(&scope.task, &scope.attempt).unwrap();
    let context = CommandContext {
        thread_id: Some(attempt.binding.session_id.clone()),
        cwd: Some(attempt.binding.workspace.clone()),
        ..Default::default()
    };
    client.record_omp(
        "core",
        WorkflowCommand::DispatchIntent {
            task: scope.task.clone(),
            attempt: scope.attempt.clone(),
        },
        context,
    )
}
fn begin_codex(client: &mut WorkflowClient, scope: &Scope) -> Result<(), String> {
    let attempt = client.attempt_record(&scope.task, &scope.attempt).unwrap();
    let context = CommandContext {
        thread_id: Some(attempt.binding.session_id.clone()),
        cwd: Some(attempt.binding.workspace.clone()),
        ..Default::default()
    };
    client
        .command(
            &format!("codex-{}-intent", scope.attempt),
            "core",
            WorkflowCommand::DispatchIntent {
                task: scope.task.clone(),
                attempt: scope.attempt.clone(),
            },
            context,
            false,
        )
        .map(|_| ())
}
#[test]
#[ignore = "real Git worktrees; concurrent API/Codex budget publication; no model calls"]
fn api_and_codex_compete_atomically_and_unknown_keeps_both_reservations() {
    for race in [true, false] {
        let budgets: &[u32] = if race { &[1] } else { &[3, 4] };
        for &budget in budgets {
            let root = temporary();
            let (mut owner, scopes) = fixture_with_cli(root.path(), budget, 3, Some("codex"));
            let mut api = owner.client(scopes[0].clone()).unwrap();
            let mut codex = owner.codex_client(scopes[2].clone()).unwrap();
            assert_eq!(prior_requests(&codex).unwrap(), 0);
            if race {
                let barrier = Arc::new(Barrier::new(3));
                let a = barrier.clone();
                let scope = scopes[0].clone();
                let api = std::thread::spawn(move || {
                    a.wait();
                    begin(&mut api, &scope).map(|_| ())
                });
                let b = barrier.clone();
                let scope = scopes[2].clone();
                let codex = std::thread::spawn(move || {
                    b.wait();
                    begin_codex(&mut codex, &scope)
                });
                barrier.wait();
                let results = [api.join().unwrap(), codex.join().unwrap()];
                assert_eq!(
                    results.iter().filter(|r| r.is_ok()).count(),
                    1,
                    "{results:?}"
                );
                assert!(results
                    .iter()
                    .find_map(|r| r.as_ref().err())
                    .unwrap()
                    .contains("budget exhausted"));
            } else {
                assert_eq!(begin(&mut api, &scopes[0]).unwrap(), 3);
                // Codex still holds its pre-API snapshot. The writer must recheck.
                let result = begin_codex(&mut codex, &scopes[2]);
                assert_eq!(result.is_ok(), budget == 4, "{result:?}");
                drop(api);
                drop(codex);
            }
            let store = owner.finish(true).unwrap();
            let count = store
                .history()
                .iter()
                .filter(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. }))
                .count();
            assert_eq!(count, if budget == 4 { 2 } else { 1 });
            assert!(prior_requests(&store).unwrap() >= budget);
            let history = store.history().to_vec();
            let config = store.configuration().clone();
            drop(store);
            let store = DurableWorkflow::open(&root.path().join("ledger"), config).unwrap();
            assert_eq!(store.history(), history);
            let mut owner = WorkflowCoordinator::start(store, root.path().join("ledger")).unwrap();
            let mut pending = owner.client(scopes[1].clone()).unwrap();
            assert!(begin(&mut pending, &scopes[1])
                .unwrap_err()
                .contains("budget exhausted"));
            drop(pending);
            let _ = owner.finish(false).unwrap();
            println!("Codex/API budget race={race} budget={budget}: original request reservations retained; no resend");
        }
    }
}
#[test]
#[ignore = "real approved Git worktrees; API reservation and OMP compete under one writer, no model calls"]
fn api_and_omp_clients_compete_atomically_for_last_request() {
    let root = temporary();
    let (mut owner, scopes) = fixture_with_omp(root.path(), 1, 4, true);
    let mut api = owner.client(scopes[0].clone()).unwrap();
    let mut omp = owner.omp_client(scopes[2].clone()).unwrap();
    assert_eq!(prior_requests(&api).unwrap(), 0);
    assert_eq!(prior_requests(&omp).unwrap(), 0);
    let barrier = Arc::new(Barrier::new(3));
    let a = barrier.clone();
    let scope = scopes[0].clone();
    let api = std::thread::spawn(move || {
        a.wait();
        begin(&mut api, &scope).map(|_| ())
    });
    let b = barrier.clone();
    let scope = scopes[2].clone();
    let omp = std::thread::spawn(move || {
        b.wait();
        begin_omp(&mut omp, &scope)
    });
    barrier.wait();
    let outcomes = [api.join().unwrap(), omp.join().unwrap()];
    assert_eq!(
        outcomes.iter().filter(|r| r.is_ok()).count(),
        1,
        "{outcomes:?}"
    );
    assert!(outcomes
        .iter()
        .find_map(|r| r.as_ref().err())
        .unwrap()
        .contains("budget exhausted"));
    let store = owner.finish(true).unwrap();
    assert_eq!(
        store
            .history()
            .iter()
            .filter(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. }))
            .count(),
        1
    );
    assert!(prior_requests(&store).unwrap() >= 1);
    let history = store.history().to_vec();
    let config = store.configuration().clone();
    drop(store);
    let store = DurableWorkflow::open(&root.path().join("ledger"), config).unwrap();
    assert_eq!(store.history(), history);
    println!("OMP/API budget race: one remaining request, one dispatch, unresolved capacity retained, reopen events 0");
}
#[test]
#[ignore = "real approved Git worktrees; unsealed API rounds reserve capacity before OMP, no model calls"]
fn omp_budget_counts_api_reservations_and_retains_unknown_cli_dispatch() {
    for budget in [3, 4] {
        let root = temporary();
        let (mut owner, scopes) = fixture_with_omp(root.path(), budget, 3, true);
        let mut api = owner.client(scopes[0].clone()).unwrap();
        let mut omp = owner.omp_client(scopes[2].clone()).unwrap();
        assert_eq!(begin(&mut api, &scopes[0]).unwrap(), 3);
        let result = begin_omp(&mut omp, &scopes[2]);
        assert_eq!(result.is_ok(), budget == 4, "{result:?}");
        drop(api);
        drop(omp);
        let store = owner.finish(true).unwrap();
        assert_eq!(prior_requests(&store).unwrap(), budget);
        assert_eq!(
            store
                .history()
                .iter()
                .filter(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. }))
                .count(),
            if budget == 4 { 2 } else { 1 }
        );
        let mut owner = WorkflowCoordinator::start(store, root.path().join("ledger")).unwrap();
        let mut next = owner.client(scopes[1].clone()).unwrap();
        assert!(begin(&mut next, &scopes[1])
            .unwrap_err()
            .contains("budget exhausted"));
        drop(next);
        let _ = owner.finish(false).unwrap();
    }
    println!("OMP budget: unsealed API reserves 3, CLI needs another 1, Unknown does not refund either reservation");
}

#[test]
#[ignore = "explicit real Git; concurrent budget publication with approved isolated directories"]
fn stale_clients_compete_for_one_budget_and_only_one_dispatch_is_published() {
    let root = temporary();
    let (mut owner, scopes) = fixture(root.path(), 3, 4);
    let before = owner.snapshot().unwrap().sequence;
    let barrier = Arc::new(Barrier::new(3));
    let mut handles = Vec::new();
    for scope in &scopes[..2] {
        let mut client = owner.client(scope.clone()).unwrap();
        assert_eq!(prior_requests(&client).unwrap(), 0);
        let scope = scope.clone();
        let barrier = barrier.clone();
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            begin(&mut client, &scope)
        }));
    }
    barrier.wait();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| **r == Ok(3)).count(), 1);
    assert!(results
        .iter()
        .find_map(|r| r.as_ref().err())
        .unwrap()
        .contains("budget exhausted"));
    assert_eq!(owner.snapshot().unwrap().sequence, before + 1);
    let store = owner.finish(true).unwrap();
    assert_eq!(prior_requests(&store).unwrap(), 4);
    // Original reopen policy publishes RecoveryUnknown once for an unfinished dispatch.
    assert_eq!(store.history().len(), before as usize + 2);
    assert_eq!(
        store
            .history()
            .iter()
            .filter(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. }))
            .count(),
        1
    );
    assert_eq!(
        store
            .history()
            .iter()
            .filter(|e| matches!(
                e.body.event.command,
                WorkflowCommand::RecoveryUnknown { .. }
            ))
            .count(),
        1
    );
    assert_eq!(
        store
            .state()
            .tasks
            .values()
            .filter(|t| t.state == TaskState::Unknown)
            .count(),
        1
    );
    assert!(!root.path().join("project/result.txt").exists());
}

#[test]
#[ignore = "explicit real Git; concurrent budget publication with approved isolated directories"]
fn writer_returns_only_remaining_allowance_and_unknown_keeps_reservation() {
    let root = temporary();
    let (mut owner, scopes) = fixture(root.path(), 4, 3);
    let mut first = owner.client(scopes[0].clone()).unwrap();
    let mut second = owner.client(scopes[1].clone()).unwrap();
    let mut third = owner.client(scopes[2].clone()).unwrap();
    assert_eq!(begin(&mut first, &scopes[0]).unwrap(), 3);
    first
        .record_api(
            "core",
            WorkflowCommand::RecoveryUnknown {
                task: scopes[0].task.clone(),
                attempt: scopes[0].attempt.clone(),
            },
            CommandContext::default(),
        )
        .unwrap();
    // Both later clients still hold the pre-dispatch snapshot.
    assert_eq!(prior_requests(&second).unwrap(), 0);
    assert_eq!(begin(&mut second, &scopes[1]).unwrap(), 1);
    assert!(begin(&mut third, &scopes[2])
        .unwrap_err()
        .contains("budget exhausted"));
    drop((first, second, third));
    let store = owner.finish(true).unwrap();
    // Conservative original rule: until completion, charge full approved rounds.
    assert_eq!(prior_requests(&store).unwrap(), 6);
    assert_eq!(store.state().tasks["task"].state, TaskState::Unknown);
    assert_eq!(store.state().tasks["task-2"].state, TaskState::Unknown);
    assert_eq!(store.state().tasks["task-3"].state, TaskState::Assigned);
}

#[test]
#[ignore = "explicit real Git; concurrent budget publication with approved isolated directories"]
fn lost_budget_ack_keeps_dispatch_and_does_not_restore_capacity() {
    let root = temporary();
    let (mut owner, scopes) = fixture(root.path(), 3, 3);
    let mut first = owner.client(scopes[0].clone()).unwrap();
    let mut second = owner.client(scopes[1].clone()).unwrap();
    let before = owner.snapshot().unwrap().sequence;
    owner.lose_next_acknowledgement();
    assert!(begin(&mut first, &scopes[0])
        .unwrap_err()
        .contains("回执丢失"));
    assert!(begin(&mut first, &scopes[0])
        .unwrap_err()
        .contains("失去确认"));
    assert!(begin(&mut second, &scopes[1])
        .unwrap_err()
        .contains("budget exhausted"));
    drop((first, second));
    let store = owner.finish(true).unwrap();
    // Original reopen policy publishes RecoveryUnknown once for an unfinished dispatch.
    assert_eq!(store.history().len(), before as usize + 2);
    assert_eq!(
        store
            .history()
            .iter()
            .filter(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. }))
            .count(),
        1
    );
    assert_eq!(
        store
            .history()
            .iter()
            .filter(|e| matches!(
                e.body.event.command,
                WorkflowCommand::RecoveryUnknown { .. }
            ))
            .count(),
        1
    );
    assert_eq!(prior_requests(&store).unwrap(), 3);
}

#[test]
#[ignore = "explicit real Git; concurrent budget publication with approved isolated directories"]
fn generic_dispatch_command_cannot_bypass_budget_or_identity_validation() {
    for corrupt in [false, true] {
        let root = temporary();
        let (mut owner, scopes) = fixture(root.path(), 3, 3);
        let mut first = owner.client(scopes[0].clone()).unwrap();
        let mut second = owner.client(scopes[1].clone()).unwrap();
        let expected = if corrupt {
            let path = second
                .api_attempt_path(&scopes[1].attempt)
                .unwrap()
                .join("identity.json");
            let mut value: serde_json::Value =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            value["worker"]["maxRounds"] = json!(1);
            fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
            "identity changed"
        } else {
            assert_eq!(begin(&mut first, &scopes[0]).unwrap(), 3);
            "budget exhausted"
        };
        let before = owner.snapshot().unwrap().sequence;
        let (principal, command, context) =
            dispatch_command(&second, &scopes[1].task, &scopes[1].attempt).unwrap();
        assert!(second
            .command("raw-dispatch", &principal, command, context, true)
            .unwrap_err()
            .contains(expected));
        assert_eq!(owner.snapshot().unwrap().sequence, before);
        drop((first, second));
        let store = owner.finish(false).unwrap();
        assert_eq!(store.state().tasks["task-2"].state, TaskState::Assigned);
    }
}

#[test]
#[ignore = "explicit real Git; concurrent budget publication with approved isolated directories"]
fn interrupted_dispatch_publication_recovers_once_without_redispatch() {
    let root = temporary();
    let (mut owner, scopes) = fixture(root.path(), 3, 3);
    let mut first = owner.client(scopes[0].clone()).unwrap();
    let before = owner.snapshot().unwrap().sequence;
    owner.interrupt_next_publication();
    assert!(begin(&mut first, &scopes[0])
        .unwrap_err()
        .contains("写入中断"));
    assert!(owner.snapshot().is_err());
    drop(first);
    let store = owner.finish(false).unwrap();
    // Original reopen policy publishes RecoveryUnknown once for an unfinished dispatch.
    assert_eq!(store.history().len(), before as usize + 2);
    assert_eq!(
        store
            .history()
            .iter()
            .filter(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. }))
            .count(),
        1
    );
    assert_eq!(
        store
            .history()
            .iter()
            .filter(|e| matches!(
                e.body.event.command,
                WorkflowCommand::RecoveryUnknown { .. }
            ))
            .count(),
        1
    );
    assert_eq!(prior_requests(&store).unwrap(), 3);
    let config = store.configuration().clone();
    drop(store);
    let store = DurableWorkflow::open(&root.path().join("ledger"), config).unwrap();
    // Original reopen policy publishes RecoveryUnknown once for an unfinished dispatch.
    assert_eq!(store.history().len(), before as usize + 2);
    assert_eq!(
        store
            .history()
            .iter()
            .filter(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. }))
            .count(),
        1
    );
    assert_eq!(
        store
            .history()
            .iter()
            .filter(|e| matches!(
                e.body.event.command,
                WorkflowCommand::RecoveryUnknown { .. }
            ))
            .count(),
        1
    );
    assert_eq!(prior_requests(&store).unwrap(), 3);
}

#[test]
#[ignore = "explicit real Git; actual completion publication paused after hard link"]
fn completion_publication_retains_full_budget_until_pending_is_removed() {
    let root = temporary();
    let mut plan = plan(root.path(), selection(targets().remove(2)));
    plan.policy.max_requests = 8;
    isolate(root.path(), &mut plan);
    let store = assigned(root.path(), plan);
    let scope = Scope {
        project: "project".into(),
        task: "task".into(),
        attempt: "attempt".into(),
        revision: store.revision().unwrap().approval_digest().unwrap(),
    };
    let mut owner = WorkflowCoordinator::start(store, root.path().join("ledger")).unwrap();
    let mut worker = owner.client(scope.clone()).unwrap();
    let (linked, waiting) = std::sync::mpsc::channel();
    let (release, resume) = std::sync::mpsc::channel();
    let handle = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let client =
            ApiClient::new(SecretInput::new("sk-controlled-no-network".into()).unwrap()).unwrap();
        let (_keep, mut cancel) = oneshot::channel();
        let mut round = 0;
        runtime
            .block_on(dispatch_inner(
                &mut worker,
                &client,
                "task",
                "attempt",
                &mut cancel,
                &mut async |prepared,
                            _,
                            _gateway: &mut cerulean_adapters::api_generation::GatewayObserver<
                    '_,
                >| {
                    round += 1;
                    fixture_reply(&prepared, body(&prepared.fingerprint().target, round, "ok"))
                        .map_err(|_| ApiFailure::InvalidResponse {})
                },
                &mut |stage| {
                    if stage == "completion-linked" {
                        linked.send(()).unwrap();
                        resume.recv_timeout(Duration::from_secs(30)).unwrap();
                    }
                    Ok(())
                },
                &mut |_| {},
            ))
            .unwrap();
    });
    waiting.recv_timeout(Duration::from_secs(30)).unwrap();
    let mut reader = owner.client(scope).unwrap();
    let evidence = reader.api_attempt_path("attempt").unwrap();
    assert!(evidence.join("completion.json").is_file());
    assert!(evidence.join("completion.json.pending").is_file());
    assert_eq!(prior_requests(&reader).unwrap(), 4);
    release.send(()).unwrap();
    handle.join().unwrap();
    reader.refresh().unwrap();
    assert!(!evidence.join("completion.json.pending").exists());
    assert_eq!(prior_requests(&reader).unwrap(), 2);
    assert_eq!(reader.state().tasks["task"].state, TaskState::Submitted);
    drop(reader);
    let store = owner.finish(false).unwrap();
    assert_eq!(prior_requests(&store).unwrap(), 2);
    println!("completion publication: actual hard-link window retains 4 approved requests; after seal counts 2 actual requests; no network");
}
