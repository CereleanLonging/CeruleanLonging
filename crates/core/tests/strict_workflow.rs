use cerulean_core::{artifacts::ArtifactObserver, host::RuntimeHost, scheduler::*};
use cerulean_domain::{execution::*, workflow::*, Command, Request, PROTOCOL_VERSION};
use std::{fs, path::Path};

fn plan(root: &Path) -> PlanRevision {
    PlanRevision {
        management_setup_digest: None,
        worktree: None,
        snapshot_policy: None,
        schema_version: 1,
        project: Project {
            id: "project".into(),
            root: root.to_string_lossy().into(),
            timezone: "Asia/Shanghai".into(),
        },
        workflow: WorkflowPlan {
            id: "plan".into(),
            version: 1,
            tasks: vec![WorkflowTask {
                id: "task".into(),
                workspace_key: "project".into(),
                goal: "Fixture result".into(),
                dependencies: vec![],
                resource_keys: None,
            }],
        },
        policy: ExecutionPolicy {
            supervision: None,
            version: 1,
            tools: vec![
                ToolPermission::ReadFiles,
                ToolPermission::WriteFiles,
                ToolPermission::RunCommands,
            ],
            max_requests: 3,
            max_seconds: 60,
            currency_budget: None,
            budget_usd_cents: Some(25),
        },
        execution: vec![TaskExecution {
            supervision_override: None,
            profile_id: None,
            desktop_worker: None,
            api_worker: None,
            worker: None,
            task_id: "task".into(),
            adapter_id: "codex".into(),
            artifact_paths: vec!["result.txt".into()],
            checks: vec![AcceptanceCheck {
                name: None,
                tool: Some(
                    cerulean_core::verifier::PinnedCheck::describe(
                        &std::env::current_exe().unwrap(),
                        &[],
                        Default::default(),
                    )
                    .unwrap(),
                ),
                id: "check".into(),
                program: "fixture-validator".into(),
                args: vec![],
                timeout_seconds: 10,
            }],
        }],
    }
}

#[test]
#[ignore = "exports a fresh owned fixture for offline binary compatibility, no provider calls"]
fn timezone_export_236_for_original_binary_and_backup_checks() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let root = repository.join("reports/timezone-compat-236/live");
    assert!(
        !root.exists(),
        "preserve the original fixture; never replay"
    );
    fs::create_dir(&root).unwrap();
    let project = root.join("project");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("result.txt"), b"local fixture only").unwrap();
    let data = root.join("data");
    let mut host = RuntimeHost::open(&data).unwrap();
    let mut p = plan(&project);
    p.project.timezone = "America/New_York".into();
    let response = host.handle(Request {
        id: "timezone-fixture-register".into(),
        protocol_version: PROTOCOL_VERSION,
        command: Command::RegisterProject {
            project: p.project.clone(),
        },
    });
    assert!(response.error.is_none(), "{:?}", response.error);
    p.project = host.snapshot().workflows[0].project.clone();
    drop(host);
    let ledger = data.join("workflows/project");
    let cfg: SchedulerConfig =
        serde_json::from_slice(&fs::read(ledger.join("bindings.json")).unwrap()).unwrap();
    assert_eq!(cfg.schema_version, 2);
    let mut store = DurableWorkflow::open(&ledger, cfg).unwrap();
    apply(
        &mut store,
        "chief",
        WorkflowCommand::Propose {
            plan: p.workflow.clone(),
        },
        CommandContext {
            execution: Some(ExecutionEvidence::Revision {
                revision: p.clone(),
            }),
            ..Default::default()
        },
    )
    .unwrap();
    approve(&mut store, &p);
    assign(&mut store, &p);
    apply(
        &mut store,
        "user",
        abandon_unsent_command(),
        CommandContext::default(),
    )
    .unwrap();
    assert_eq!(store.state().tasks["task"].state, TaskState::Pending);
    assert_eq!(
        store
            .handoffs()
            .unwrap()
            .iter()
            .map(|v| v.event_ids.len())
            .sum::<usize>(),
        2
    );
    assert!(!store
        .history()
        .iter()
        .any(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. })));
    let evidence = serde_json::json!({
        "scope": "synthetic management, no dispatch, original binary/data maintenance fixture",
        "project": p.project, "approval": store.approval(), "history": store.history(),
        "handoffs": store.handoffs().unwrap(), "providerCalls": 0
    });
    fs::write(
        root.join("fixture.json"),
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
    drop(store);
    let mut reopened = RuntimeHost::open(&data).unwrap();
    assert!(reopened.can_shutdown());
    assert_eq!(
        reopened.snapshot().workflows[0].project.timezone,
        "America/New_York"
    );
}

#[test]
fn timezone_scoped_ledger_reopens_with_identical_approval_events_and_daily_bytes() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    fs::create_dir(&project).unwrap();
    let mut p = plan(&project);
    p.project.timezone = "America/New_York".into();
    p.validate().unwrap();
    let mut cfg = config(&p);
    cfg.schema_version = 2;
    let ledger = root.path().join("ledger");
    let mut store = DurableWorkflow::open(&ledger, cfg.clone()).unwrap();
    apply(
        &mut store,
        "chief",
        WorkflowCommand::Propose {
            plan: p.workflow.clone(),
        },
        CommandContext {
            execution: Some(ExecutionEvidence::Revision {
                revision: p.clone(),
            }),
            ..Default::default()
        },
    )
    .unwrap();
    approve(&mut store, &p);
    assign(&mut store, &p);
    let approval = store.approval().unwrap().clone();
    let events = serde_json::to_vec(store.history()).unwrap();
    let daily = store.handoffs().unwrap();
    assert!(!daily.is_empty());
    assert!(daily
        .iter()
        .all(|v| v.path.ends_with("-0400.md") || v.path.ends_with("-0500.md")));
    // Daily files are asynchronous projections; compare bytes after the owned
    // publication has drained, while keeping the full original equality check.
    store
        .wait_for_projections(std::time::Duration::from_secs(20))
        .unwrap();
    for v in &daily {
        assert_eq!(
            fs::read(ledger.join(&v.path)).unwrap(),
            v.markdown.as_bytes()
        );
    }
    drop(store);
    for _ in 0..2 {
        let reopened = DurableWorkflow::open(&ledger, cfg.clone()).unwrap();
        assert_eq!(reopened.approval().unwrap(), &approval);
        assert_eq!(serde_json::to_vec(reopened.history()).unwrap(), events);
        assert_eq!(reopened.handoffs().unwrap(), daily);
    }
    let bindings = fs::read(ledger.join("bindings.json")).unwrap();
    let mut changed = cfg.clone();
    changed.execution_scope.as_mut().unwrap().project.timezone = "Europe/London".into();
    assert!(DurableWorkflow::open(&ledger, changed).is_err());
    assert_eq!(fs::read(ledger.join("bindings.json")).unwrap(), bindings);
    let mut invalid = cfg.clone();
    invalid.execution_scope.as_mut().unwrap().project.timezone = "invalid-zone".into();
    let fresh = root.path().join("invalid");
    assert!(DurableWorkflow::open(&fresh, invalid).is_err());
    assert!(
        !fresh.exists(),
        "invalid timezone must fail before directory or event writes"
    );
    cfg.schema_version = 1;
    assert!(DurableWorkflow::open(&fresh, cfg).is_err());
    assert!(!fresh.exists(), "new timezone must not use old schema");
}

#[test]
fn timezone_host_register_persist_reopen_and_invalid_or_mutated_scope_refusal() {
    let root = tempfile::tempdir().unwrap();
    let data = root.path().join("data");
    let project = root.path().join("project");
    fs::create_dir(&project).unwrap();
    let mut host = RuntimeHost::open(&data).unwrap();
    let mut p = Project {
        id: "timezone-project".into(),
        root: project.to_string_lossy().into(),
        timezone: "Bad/Zone".into(),
    };
    let request = |project| Request {
        id: "register".into(),
        protocol_version: PROTOCOL_VERSION,
        command: Command::RegisterProject { project },
    };
    assert!(host.handle(request(p.clone())).error.is_some());
    assert_eq!(fs::read_dir(data.join("workflows")).unwrap().count(), 0);
    p.timezone = "Asia/Kathmandu".into();
    let response = host.handle(request(p.clone()));
    assert!(response.error.is_none(), "{:?}", response.error);
    let saved = host.snapshot().workflows[0].project.clone();
    let bindings_path = data.join("workflows/timezone-project/bindings.json");
    let bindings = fs::read(&bindings_path).unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bindings).unwrap()["schemaVersion"],
        2
    );
    p.timezone = "UTC".into();
    assert!(host.handle(request(p)).error.is_some());
    assert_eq!(fs::read(&bindings_path).unwrap(), bindings);
    drop(host);
    for _ in 0..2 {
        let reopened = RuntimeHost::open(&data).unwrap();
        assert_eq!(reopened.snapshot().workflows[0].project, saved);
        assert_eq!(fs::read(&bindings_path).unwrap(), bindings);
    }
}

#[cfg(windows)]
fn codex_host_fixture(root: &Path, executable: &Path, model: &str) -> (RuntimeHost, PlanRevision) {
    fs::create_dir(root.join("project")).unwrap();
    fs::write(root.join("project/result.txt"), "abc").unwrap();
    let mut host = RuntimeHost::open(&root.join("data")).unwrap();
    let mut p = plan(&root.join("project"));
    p.project = host
        .handle(Request {
            id: "register".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::RegisterProject {
                project: p.project.clone(),
            },
        })
        .result
        .unwrap()
        .workflows[0]
        .project
        .clone();
    p.policy.budget_usd_cents = None;
    p.snapshot_policy = Some(SnapshotPolicy {
        exclude: vec![],
        max_files: 100,
        max_bytes: 1024 * 1024,
    });
    p.execution[0].worker = Some(RuntimeHost::describe_codex_worker(executable, model).unwrap());
    (host, p)
}
#[cfg(windows)]
fn host_approve(host: &mut RuntimeHost, p: &PlanRevision) {
    host.handle(Request {
        id: "approve".into(),
        protocol_version: PROTOCOL_VERSION,
        command: Command::ApproveRevision {
            project_id: p.project.id.clone(),
            digest: p.approval_digest().unwrap(),
        },
    })
    .result
    .unwrap();
}
#[cfg(windows)]
#[test]
fn codex_host_requires_full_worker_approval_and_rejects_replacement_before_process() {
    use cerulean_domain::CodexJobState;
    use std::time::{Duration, Instant};
    let root = tempfile::tempdir().unwrap();
    let executable = root.path().join("codex.exe");
    fs::write(&executable, b"not executable").unwrap();
    let (mut host, mut p) = codex_host_fixture(root.path(), &executable, "fixture-model");
    host.propose("project", "chief-v1", p.clone()).unwrap();
    assert!(host
        .start_codex(
            "project",
            "task",
            "host-attempt",
            &p.approval_digest().unwrap()
        )
        .is_err());
    assert!(host.snapshot().codex_jobs.is_empty());
    host_approve(&mut host, &p);
    let old = p.approval_digest().unwrap();
    p.workflow.version = 2;
    p.execution[0].worker.as_mut().unwrap().model = "other-model".into();
    assert_ne!(old, p.approval_digest().unwrap());
    host.propose("project", "chief-v2", p.clone()).unwrap();
    assert!(host
        .start_codex("project", "task", "host-attempt", &old)
        .is_err());
    host_approve(&mut host, &p);
    fs::write(&executable, b"changed executable").unwrap();
    host.start_codex(
        "project",
        "task",
        "host-attempt",
        &p.approval_digest().unwrap(),
    )
    .unwrap();
    assert_eq!(host.snapshot().codex_jobs[0].state, CodexJobState::Starting);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        host.poll_verifications();
        if host.snapshot().codex_jobs[0].state == CodexJobState::Failed {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    let snap = host.snapshot();
    assert!(snap.codex_jobs[0]
        .message
        .as_ref()
        .unwrap()
        .contains("changed since approval"));
    assert_eq!(snap.workflows[0].sequence, 4);
    assert_eq!(
        snap.workflows[0].state.tasks["task"].state,
        TaskState::Pending
    );
    assert!(host
        .start_codex(
            "project",
            "task",
            "host-attempt",
            &p.approval_digest().unwrap()
        )
        .is_err());
    drop(host);
    assert_eq!(
        RuntimeHost::open(&root.path().join("data"))
            .unwrap()
            .snapshot()
            .codex_jobs[0]
            .state,
        CodexJobState::Failed
    );
}
#[cfg(windows)]
#[test]
fn codex_host_never_trusts_success_badge_without_ledger_or_changed_worker() {
    use cerulean_domain::{CodexJob, CodexJobState};
    let root = tempfile::tempdir().unwrap();
    let executable = root.path().join("codex.exe");
    fs::write(&executable, b"not executable").unwrap();
    let (mut host, p) = codex_host_fixture(root.path(), &executable, "fixture-model");
    host.propose("project", "chief", p.clone()).unwrap();
    host_approve(&mut host, &p);
    drop(host);
    let folder = root.path().join("data/codex-jobs/project");
    fs::create_dir_all(&folder).unwrap();
    let now = chrono::Utc::now().to_rfc3339();
    let mut job = CodexJob {
        management_operation: None,
        project_id: "project".into(),
        task_id: "task".into(),
        attempt_id: "host-attempt".into(),
        revision_digest: p.approval_digest().unwrap(),
        worker: p.execution[0].worker.clone().unwrap(),
        state: CodexJobState::Submitted,
        started_at: now.clone(),
        updated_at: now,
        message: None,
    };
    fs::write(
        folder.join("host-attempt.json"),
        serde_json::to_vec(&job).unwrap(),
    )
    .unwrap();
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(host.snapshot().codex_jobs[0].state, CodexJobState::Unknown);
    assert!(!host.can_shutdown());
    assert!(host
        .start_codex(
            "project",
            "task",
            "different-attempt",
            &p.approval_digest().unwrap()
        )
        .is_err());
    assert!(host
        .handle(Request {
            id: "reconcile".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::ReconcileCodex {
                project_id: "project".into(),
                attempt_id: "host-attempt".into()
            }
        })
        .error
        .is_some());
    drop(host);
    job.worker.model = "unapproved".into();
    fs::write(
        folder.join("host-attempt.json"),
        serde_json::to_vec(&job).unwrap(),
    )
    .unwrap();
    assert!(RuntimeHost::open(&root.path().join("data")).is_err());
}
#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "explicit one-turn resident Codex integration; isolated fresh report directory required"]
async fn live_codex_resident_worker_and_recovery() {
    use cerulean_core::{
        ipc::{self, Client},
        verifier::PinnedCheck,
    };
    use cerulean_domain::{CodexJob, CodexJobState};
    use std::{
        path::PathBuf,
        time::{Duration, Instant},
    };
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let root = PathBuf::from(
        std::env::var_os("CERULEAN_LIVE_CODEX_DIR").expect("fresh isolated directory"),
    )
    .canonicalize()
    .unwrap();
    assert!(root.starts_with(repository.join("reports").canonicalize().unwrap()));
    assert!(
        !root.join("data").exists() && !root.join("project").exists(),
        "never resend a claimed case"
    );
    let executable =
        PathBuf::from(std::env::var_os("CERULEAN_LIVE_CODEX_EXE").expect("Codex executable"));
    let node = PathBuf::from(std::env::var_os("CERULEAN_LIVE_NODE_EXE").expect("Node executable"));
    let model = std::env::var("CERULEAN_LIVE_CODEX_MODEL").expect("explicit model selection");
    let (mut host, mut p) = codex_host_fixture(&root, &executable, &model);
    p.policy.max_requests = 1;
    p.policy.max_seconds = 180;
    p.workflow.tasks[0].goal="Create only sum.mjs. Export function sum(values). Require an array of finite numbers; throw TypeError for non-arrays, non-number elements, NaN or Infinity. Return the arithmetic sum, return 0 for an empty array, preserve the original array. Use native JavaScript ES modules without dependencies. Do not modify result.txt, create tests, install packages or use network.".into();
    let validator = repository.join("tests/fixtures/sum-validator.mjs");
    p.execution[0].artifact_paths = vec!["sum.mjs".into()];
    p.execution[0].checks = vec![AcceptanceCheck {
        name: None,
        id: "sum-behavior".into(),
        program: "independent-sum-validator".into(),
        args: vec![validator.to_string_lossy().into()],
        timeout_seconds: 15,
        tool: Some(
            PinnedCheck::describe(&node, std::slice::from_ref(&validator), Default::default())
                .unwrap(),
        ),
    }];
    host.propose("project", "fixture-chief", p.clone()).unwrap();
    host_approve(&mut host, &p);
    fs::write(
        root.join("approved-fixture-plan.json"),
        serde_json::to_vec_pretty(&p).unwrap(),
    )
    .unwrap();
    fs::write(root.join("result.json"),serde_json::to_vec_pretty(&serde_json::json!({"status":"starting","at":chrono::Utc::now().to_rfc3339(),"scope":"preapproved fixture; actual resident worker and IPC, not real chief/deputy acceptance","maxModelTurns":1})).unwrap()).unwrap();
    let began = Instant::now();
    host.start_codex(
        "project",
        "task",
        "resident-attempt",
        &p.approval_digest().unwrap(),
    )
    .unwrap();
    assert!(began.elapsed() < Duration::from_secs(1));
    assert!(!host.can_shutdown());
    assert!(host
        .start_codex(
            "project",
            "task",
            "duplicate-attempt",
            &p.approval_digest().unwrap()
        )
        .is_err());
    let mut duplicate = p.project.clone();
    duplicate.id = "other-project".into();
    assert!(host
        .handle(Request {
            id: "overlap".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::RegisterProject { project: duplicate }
        })
        .error
        .is_some());
    let data = root.join("data");
    let service_data = data.clone();
    let service = tokio::spawn(async move { ipc::serve_host(&service_data, host).await });
    let deadline = Instant::now() + Duration::from_secs(210);
    let first = loop {
        if let Ok(s) = Client::snapshot(&data).await {
            break s;
        }
        assert!(!service.is_finished());
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert!(matches!(
        first.codex_jobs[0].state,
        CodexJobState::Starting | CodexJobState::Executing
    ));
    // Every snapshot uses a fresh client and drops it; the service still owns the worker.
    let mut observations = 0;
    let mut max_query_ms = 0;
    let completed = loop {
        let query = Instant::now();
        let snapshot = Client::snapshot(&data).await.unwrap();
        max_query_ms = max_query_ms.max(query.elapsed().as_millis());
        observations += 1;
        if matches!(
            snapshot.codex_jobs[0].state,
            CodexJobState::Submitted | CodexJobState::Failed | CodexJobState::Unknown
        ) {
            break snapshot;
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    fs::write(
        root.join("worker-snapshot.json"),
        serde_json::to_vec_pretty(&completed).unwrap(),
    )
    .unwrap();
    service.abort();
    let _ = service.await;
    assert_eq!(
        completed.codex_jobs[0].state,
        CodexJobState::Submitted,
        "{:?}",
        completed.codex_jobs[0].message
    );
    assert!(observations > 1 && max_query_ms < 1000);
    let mut host = RuntimeHost::open(&data).unwrap();
    host.start_verification(
        "project",
        "resident-independent-check",
        "task",
        "resident-attempt",
        vec![PinnedCheck::from_approved(&p.execution[0].checks[0]).unwrap()],
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        host.poll_verifications();
        if host.snapshot().verification_jobs[0].state != VerificationState::Running {
            break;
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        host.snapshot().verification_jobs[0].state,
        VerificationState::Succeeded
    );
    let verified = host.snapshot();
    drop(host);
    // Read-only recovery from the actual host journal prefix. No second turn/start.
    let source = data.join("workflows/project");
    let recovery = root.join("recovery-data");
    let restored = recovery.join("workflows/project");
    fs::create_dir_all(restored.join("events")).unwrap();
    fs::copy(source.join("bindings.json"), restored.join("bindings.json")).unwrap();
    let mut entries = fs::read_dir(source.join("events"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect::<Vec<_>>();
    entries.sort();
    for entry in entries {
        let record: Entry = serde_json::from_slice(&fs::read(&entry).unwrap()).unwrap();
        fs::copy(
            &entry,
            restored.join("events").join(entry.file_name().unwrap()),
        )
        .unwrap();
        if matches!(
            record.body.event.command,
            WorkflowCommand::DispatchIntent { .. }
        ) {
            break;
        }
    }
    let mut job: CodexJob = verified.codex_jobs[0].clone();
    job.state = CodexJobState::Executing;
    fs::create_dir_all(recovery.join("codex-jobs/project")).unwrap();
    fs::write(
        recovery.join("codex-jobs/project/resident-attempt.json"),
        serde_json::to_vec(&job).unwrap(),
    )
    .unwrap();
    let mut recovered = RuntimeHost::open(&recovery).unwrap();
    assert_eq!(
        recovered.snapshot().codex_jobs[0].state,
        CodexJobState::Unknown
    );
    recovered
        .handle(Request {
            id: "reconcile".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::ReconcileCodex {
                project_id: "project".into(),
                attempt_id: "resident-attempt".into(),
            },
        })
        .result
        .unwrap();
    assert_eq!(
        recovered.snapshot().codex_jobs[0].state,
        CodexJobState::Reconciling
    );
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        recovered.poll_verifications();
        if recovered.snapshot().codex_jobs[0].state != CodexJobState::Reconciling {
            break;
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        recovered.snapshot().codex_jobs[0].state,
        CodexJobState::Submitted,
        "{:?}",
        recovered.snapshot().codex_jobs[0].message
    );
    let final_recovery = recovered.snapshot();
    drop(recovered);
    assert_eq!(
        RuntimeHost::open(&recovery).unwrap().snapshot().codex_jobs[0].state,
        CodexJobState::Submitted
    );
    assert_eq!(
        fs::read_to_string(root.join("project/result.txt")).unwrap(),
        "abc"
    );
    fs::write(root.join("result.json"),serde_json::to_vec_pretty(&serde_json::json!({"status":"passed","at":chrono::Utc::now().to_rfc3339(),"scope":"preapproved fixture, real service/worker/verifier/recovery; chief/deputy not accepted","modelTurnsRequested":1,"recoveryNewModelTurns":0,"clientReconnectObservations":observations,"maxQueryMilliseconds":max_query_ms,"verified":verified,"recovered":final_recovery})).unwrap()).unwrap();
}
#[cfg(windows)]
fn detail_request() -> Request {
    Request {
        id: "readonly-stage-detail".into(),
        protocol_version: PROTOCOL_VERSION,
        command: Command::ReadVerificationDetail {
            project_id: "project".into(),
            operation_id: "stage-check".into(),
        },
    }
}
#[cfg(windows)]
#[test]
#[ignore = "native verification details fixture; real validators, synthetic worker receipt"]
fn seed_native_verification_details() {
    let outer = fs::canonicalize(std::env::var("CERULEAN_DETAIL_NATIVE_DIR").unwrap()).unwrap();
    let reports = fs::canonicalize(std::env::current_dir().unwrap().join("../../reports")).unwrap();
    assert!(outer.starts_with(reports));
    let mode = std::env::var("CERULEAN_DETAIL_NATIVE_MODE").unwrap();
    assert!(["success", "second-fail", "timeout", "unknown"].contains(&mode.as_str()));
    let runner_mode = if mode == "unknown" {
        "detail-success".into()
    } else {
        format!("detail-{mode}")
    };
    let (root, mut host, _, pins) =
        async_fixture_in(tempfile::tempdir_in(&outer).unwrap(), &runner_mode);
    host.start_verification("project", "stage-check", "task", "attempt", pins)
        .unwrap();
    complete_detail_verification(&mut host);
    let data = root.path().join("data");
    drop(host);
    if mode == "unknown" {
        let events = data.join("workflows/project/events");
        let mut removed = false;
        for entry in fs::read_dir(events).unwrap() {
            let path = entry.unwrap().path();
            let event: serde_json::Value =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            if event["body"]["operationId"] == "stage-check" {
                fs::remove_file(path).unwrap();
                removed = true;
            }
        }
        assert!(removed);
    }
    let mut host = RuntimeHost::open(&data).unwrap();
    let detail = host
        .handle(detail_request())
        .result
        .unwrap()
        .verification_detail
        .unwrap();
    fs::write(
        outer.join("fixture.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"data":data,"mode":mode,"detail":detail,"realModelCalls":0}),
        )
        .unwrap(),
    )
    .unwrap();
    drop(host);
    let _ = root.keep();
}
#[cfg(windows)]
fn complete_detail_verification(host: &mut RuntimeHost) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(12);
    loop {
        host.poll_verifications();
        if !matches!(
            host.snapshot().verification_jobs[0].state,
            VerificationState::Running | VerificationState::Cancelling
        ) {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}
#[cfg(windows)]
fn recovery_fixture(mode: &str) -> (tempfile::TempDir, RuntimeHost) {
    let (root, mut host, _, pins) = async_fixture(mode);
    host.start_verification("project", "stage-check", "task", "attempt", pins)
        .unwrap();
    complete_detail_verification(&mut host);
    drop(host);
    // Model the precise publication gap: complete on-disk report, no Verify yet,
    // and the pre-completion job projection. No real process crash is claimed.
    let data = root.path().join("data");
    let events = data.join("workflows/project/events");
    let mut removed = 0;
    for entry in fs::read_dir(events).unwrap() {
        let path = entry.unwrap().path();
        let event: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        if event["body"]["operationId"] == "stage-check" {
            fs::remove_file(path).unwrap();
            removed += 1;
        }
    }
    assert_eq!(removed, 1);
    let projection = data.join("verification-jobs/project/stage-check.json");
    let mut job: VerificationJob = serde_json::from_slice(&fs::read(&projection).unwrap()).unwrap();
    job.state = VerificationState::Running;
    fs::write(projection, serde_json::to_vec(&job).unwrap()).unwrap();
    let host = RuntimeHost::open(&data).unwrap();
    assert_eq!(
        host.snapshot().verification_jobs[0].state,
        VerificationState::Unknown
    );
    (root, host)
}
#[cfg(windows)]
fn recovery_request(evidence: &str) -> Request {
    Request {
        id: "explicit-recovery".into(),
        protocol_version: PROTOCOL_VERSION,
        command: Command::RecoverVerification {
            project_id: "project".into(),
            operation_id: "stage-check".into(),
            evidence_digest: evidence.into(),
        },
    }
}
#[cfg(windows)]
fn retirement_request(evidence: &str) -> Request {
    Request {
        id: "explicit-submitted-retirement".into(),
        protocol_version: PROTOCOL_VERSION,
        command: Command::RetireSubmittedPlan {
            project_id: "project".into(),
            evidence_digest: evidence.into(),
            reason: "End this submitted attempt and revise the approved work".into(),
        },
    }
}
#[cfg(windows)]
#[test]
fn submitted_retirement_preserves_evidence_and_requires_new_approval_and_attempt() {
    for mode in ["detail-success", "detail-second-fail", "detail-timeout"] {
        let (root, mut host, mut plan, pins) = async_fixture(mode);
        host.start_verification("project", "stage-check", "task", "attempt", pins)
            .unwrap();
        complete_detail_verification(&mut host);
        let before = host.snapshot();
        let candidate = before.project_revisions[0].clone();
        assert_eq!(candidate.submitted_tasks, 1);
        assert_eq!(
            candidate.verified_submissions,
            u32::from(mode == "detail-success")
        );
        let original = host
            .handle(detail_request())
            .result
            .unwrap()
            .verification_detail
            .unwrap();
        let response = host.handle(Request {
            id: "ordinary-revision".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::RequestProjectRevision {
                project_id: "project".into(),
                evidence_digest: candidate.evidence_digest.clone(),
                reason: "ordinary revision is not explicit retirement".into(),
            },
        });
        assert!(response.error.is_some());
        assert!(host
            .handle(retirement_request(&"0".repeat(64)))
            .error
            .is_some());
        assert_eq!(
            host.snapshot().workflows[0].sequence,
            before.workflows[0].sequence
        );
        let response = host.handle(retirement_request(&candidate.evidence_digest));
        assert!(response.error.is_none(), "{:?}", response.error);
        let retired = host.snapshot();
        assert_eq!(
            retired.workflows[0].sequence,
            before.workflows[0].sequence + 1
        );
        assert_eq!(
            retired.workflows[0].state.tasks["task"],
            before.workflows[0].state.tasks["task"]
        );
        assert!(retired.workflows[0].state.revision_requested.is_some());
        assert!(host
            .handle(retirement_request(&candidate.evidence_digest))
            .error
            .is_some());
        drop(host);
        let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
        let old_digest = plan.approval_digest().unwrap();
        assert!(host
            .start_verification("project", "old-recheck", "task", "attempt", vec![])
            .is_err());
        plan.workflow.version += 1;
        host.propose("project", "chief-revised-submission", plan.clone())
            .unwrap();
        assert!(host.snapshot().workflows[0].approval.is_none());
        assert_eq!(
            host.snapshot().workflows[0].state.tasks["task"].state,
            TaskState::Pending
        );
        assert_eq!(host.snapshot().workflows[0].state.tasks["task"].fence, 1);
        assert!(host
            .handle(Request {
                id: "stale-approval".into(),
                protocol_version: PROTOCOL_VERSION,
                command: Command::ApproveRevision {
                    project_id: "project".into(),
                    digest: old_digest
                }
            })
            .error
            .is_some());
        host_approve(&mut host, &plan);
        let historical = host
            .handle(detail_request())
            .result
            .unwrap()
            .verification_detail
            .unwrap();
        assert_eq!(historical.evidence_digest, original.evidence_digest);
        drop(host);
        let ledger = root.path().join("data/workflows/project");
        let config =
            serde_json::from_slice(&fs::read(ledger.join("bindings.json")).unwrap()).unwrap();
        let mut store = DurableWorkflow::open(&ledger, config).unwrap();
        let mut attempt = store.attempt_record("task", "attempt").unwrap().clone();
        attempt.fence = 2;
        attempt.revision_digest = plan.approval_digest().unwrap();
        let command = WorkflowCommand::Assign {
            task: "task".into(),
            worker: attempt.worker_id.clone(),
            attempt: "attempt".into(),
        };
        assert!(apply(
            &mut store,
            "deputy",
            command,
            CommandContext {
                execution: Some(ExecutionEvidence::Attempt {
                    attempt: attempt.clone()
                }),
                ..Default::default()
            }
        )
        .is_err());
        attempt.id = "attempt-revised".into();
        let command = WorkflowCommand::Assign {
            task: "task".into(),
            worker: attempt.worker_id.clone(),
            attempt: attempt.id.clone(),
        };
        apply(
            &mut store,
            "deputy",
            command,
            CommandContext {
                execution: Some(ExecutionEvidence::Attempt { attempt }),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(store.state().tasks["task"].fence, 2);
        assert_eq!(
            store
                .history()
                .iter()
                .filter(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. }))
                .count(),
            1
        );
    }
}
#[cfg(windows)]
#[test]
fn submitted_retirement_rejects_unresolved_verification_and_non_user_authority() {
    let (root, mut host) = recovery_fixture("detail-success");
    assert!(host.snapshot().project_revisions.is_empty());
    assert!(host
        .handle(retirement_request(&"0".repeat(64)))
        .error
        .is_some());
    let recovery = recovery_digest(&mut host);
    assert!(host.handle(recovery_request(&recovery)).error.is_none());
    assert_eq!(host.snapshot().project_revisions[0].submitted_tasks, 1);
    drop(host);
    let ledger = root.path().join("data/workflows/project");
    let config = serde_json::from_slice(&fs::read(ledger.join("bindings.json")).unwrap()).unwrap();
    let mut store = DurableWorkflow::open(&ledger, config).unwrap();
    let command = WorkflowCommand::RetireSubmittedPlan {
        digest: store.state().digest.clone().unwrap(),
        reason: "retire".into(),
    };
    for principal in ["chief", "deputy", "core", "worker"] {
        assert!(apply(
            &mut store,
            principal,
            command.clone(),
            CommandContext {
                approval_source: Some("authenticated local-user submitted-plan retirement".into()),
                ..Default::default()
            }
        )
        .is_err());
    }
    assert!(apply(&mut store, "user", command, CommandContext::default()).is_err());
    assert!(store.state().revision_requested.is_none());
}
#[cfg(windows)]
fn recovery_digest(host: &mut RuntimeHost) -> String {
    let response = host.handle(detail_request());
    assert!(response.error.is_none(), "{:?}", response.error);
    let detail = response.result.unwrap().verification_detail.unwrap();
    assert!(detail.recovery.is_some(), "{:?}", detail.recovery_error);
    detail.recovery.unwrap().evidence_digest
}
#[cfg(windows)]
#[test]
fn verification_recovery_records_original_outcome_once_without_accept_or_rerun() {
    for (mode, passed) in [
        ("detail-success", true),
        ("detail-second-fail", false),
        ("detail-timeout", false),
    ] {
        let (root, mut host) = recovery_fixture(mode);
        let evidence = root
            .path()
            .join("data/workflows/project/verification/stage-check");
        let original = fs::read(evidence.join("result.json")).unwrap();
        let before = host.snapshot().workflows[0].sequence;
        let digest = recovery_digest(&mut host);
        assert_eq!(host.snapshot().workflows[0].sequence, before);
        assert!(host
            .handle(recovery_request(&"0".repeat(64)))
            .error
            .is_some());
        let response = host.handle(recovery_request(&digest));
        assert!(response.error.is_none(), "{mode}: {:?}", response.error);
        let snap = host.snapshot();
        assert_eq!(snap.workflows[0].sequence, before + 1);
        assert_eq!(
            snap.workflows[0].state.tasks["task"].state,
            TaskState::Submitted
        );
        assert_eq!(snap.workflows[0].state.tasks["task"].verified, passed);
        assert!(snap.codex_jobs.is_empty() && snap.management_decisions.is_empty());
        assert!(host.handle(recovery_request(&digest)).error.is_none());
        assert_eq!(host.snapshot().workflows[0].sequence, before + 1);
        assert_eq!(fs::read(evidence.join("result.json")).unwrap(), original);
        drop(host);
        let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(
            host.snapshot().verification_jobs[0].state,
            if passed {
                VerificationState::Succeeded
            } else {
                VerificationState::Failed
            }
        );
        assert!(host.handle(recovery_request(&digest)).error.is_none());
        assert_eq!(host.snapshot().workflows[0].sequence, before + 1);
        assert_eq!(
            host.snapshot().workflows[0].state.tasks["task"].state,
            TaskState::Submitted
        );
        assert_eq!(fs::read(evidence.join("result.json")).unwrap(), original);
    }
}
#[cfg(windows)]
#[test]
fn verification_recovery_rejects_changed_or_partial_evidence() {
    let (root, mut host) = recovery_fixture("detail-success");
    let digest = recovery_digest(&mut host);
    let sequence = host.snapshot().workflows[0].sequence;
    for relative in [
        "data/workflows/project/verification/stage-check/check.stdout",
        "data/workflows/project/verification/stage-check/result.json",
        "data/workflows/project/verification/stage-check/snapshot.json",
        "project/result.txt",
    ] {
        let path = root.path().join(relative);
        let original = fs::read(&path).unwrap();
        fs::write(&path, b"changed").unwrap();
        assert!(
            host.handle(recovery_request(&digest)).error.is_some(),
            "{relative}"
        );
        assert_eq!(host.snapshot().workflows[0].sequence, sequence);
        fs::write(path, original).unwrap();
    }
    let result = root
        .path()
        .join("data/workflows/project/verification/stage-check/result.json");
    let held = result.with_extension("held");
    fs::rename(&result, &held).unwrap();
    let detail = host
        .handle(detail_request())
        .result
        .unwrap()
        .verification_detail
        .unwrap();
    assert!(detail.recovery.is_none() && detail.recovery_error.is_some());
    assert!(host.handle(recovery_request(&digest)).error.is_some());
    assert_eq!(host.snapshot().workflows[0].sequence, sequence);
    fs::rename(held, result).unwrap();
    let fresh = recovery_digest(&mut host);
    assert!(host.handle(recovery_request(&fresh)).error.is_none());
}
#[cfg(windows)]
#[test]
fn verification_recovery_survives_projection_publication_failure() {
    let (root, mut host) = recovery_fixture("detail-success");
    let digest = recovery_digest(&mut host);
    let sequence = host.snapshot().workflows[0].sequence;
    let projection = root.path().join("data/verification-jobs/project");
    let held = root.path().join("data/verification-jobs/project-held");
    fs::rename(&projection, &held).unwrap();
    fs::write(&projection, b"injected projection write failure").unwrap();
    assert!(host.handle(recovery_request(&digest)).error.is_some());
    assert_eq!(host.snapshot().workflows[0].sequence, sequence + 1);
    fs::remove_file(&projection).unwrap();
    fs::rename(held, projection).unwrap();
    drop(host);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(
        host.snapshot().verification_jobs[0].state,
        VerificationState::Succeeded
    );
    assert!(host.handle(recovery_request(&digest)).error.is_none());
    assert_eq!(host.snapshot().workflows[0].sequence, sequence + 1);
    assert_eq!(
        host.snapshot().workflows[0].state.tasks["task"].state,
        TaskState::Submitted
    );
}
#[cfg(windows)]
#[test]
fn verification_recovery_rejects_stale_workflow_and_recovers_failed_journal_write() {
    let (root, mut host) = recovery_fixture("detail-success");
    let old_digest = recovery_digest(&mut host);
    drop(host);
    let ledger = root.path().join("data/workflows/project");
    let config = serde_json::from_slice(&fs::read(ledger.join("bindings.json")).unwrap()).unwrap();
    let mut store = DurableWorkflow::open(&ledger, config).unwrap();
    apply(
        &mut store,
        "deputy",
        WorkflowCommand::Advise {
            task: "task".into(),
            suggestion: "Review the original report before proceeding".into(),
        },
        CommandContext::default(),
    )
    .unwrap();
    drop(store);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    let sequence = host.snapshot().workflows[0].sequence;
    assert!(host.handle(recovery_request(&old_digest)).error.is_some());
    assert_eq!(host.snapshot().workflows[0].sequence, sequence);
    let fresh = recovery_digest(&mut host);
    assert_ne!(fresh, old_digest);
    let events = ledger.join("events");
    let held = ledger.join("events-held");
    fs::rename(&events, &held).unwrap();
    fs::write(&events, b"injected event publication failure").unwrap();
    assert!(host.handle(recovery_request(&fresh)).error.is_some());
    fs::remove_file(&events).unwrap();
    fs::rename(held, events).unwrap();
    drop(host);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(host.snapshot().workflows[0].sequence, sequence);
    assert_eq!(
        host.snapshot().verification_jobs[0].state,
        VerificationState::Unknown
    );
    let fresh = recovery_digest(&mut host);
    assert!(host.handle(recovery_request(&fresh)).error.is_none());
    assert_eq!(host.snapshot().workflows[0].sequence, sequence + 1);
}
#[cfg(windows)]
#[test]
fn verification_details_bind_real_stages_and_reject_corrupt_evidence() {
    use cerulean_domain::verification_detail::VerificationStageState as State;
    for mode in [
        "detail-success",
        "detail-first-fail",
        "detail-second-fail",
        "detail-timeout",
    ] {
        let (root, mut host, plan, pins) = async_fixture(mode);
        host.start_verification("project", "stage-check", "task", "attempt", pins)
            .unwrap();
        complete_detail_verification(&mut host);
        let before = host.snapshot();
        let response = host.handle(detail_request());
        assert!(response.error.is_none(), "{:?}", response.error);
        let detail = response.result.unwrap().verification_detail.unwrap();
        if !detail.ledger_recorded {
            // Keep the real process evidence when this invariant fails under load.
            // Do not turn an execution error into a timeout retry or a passing test.
            let evidence = root.keep();
            panic!(
                "{mode}: verification not journaled; evidence={}; detail={detail:?}",
                evidence.display()
            );
        }
        assert_eq!(detail.revision_digest, plan.approval_digest().unwrap());
        assert_eq!(detail.stages.len(), 2);
        assert!(detail.snapshot_digest.is_some());
        assert_eq!(detail.artifacts[0].path, "result.txt");
        match mode {
            "detail-success" => assert!(detail
                .stages
                .iter()
                .all(|s| matches!(s.state, State::Passed))),
            "detail-first-fail" => {
                assert!(matches!(detail.stages[0].state, State::Failed));
                assert_eq!(detail.stages[0].exit_code, Some(7));
            }
            "detail-second-fail" => assert!(matches!(detail.stages[1].state, State::Failed)),
            _ => {
                assert!(matches!(detail.stages[0].state, State::TimedOut));
                assert!(matches!(detail.stages[1].state, State::NotRun));
            }
        }
        assert_eq!(
            host.snapshot().workflows[0].sequence,
            before.workflows[0].sequence
        );
        assert!(host.snapshot().verification_detail.is_none());
        let evidence = root
            .path()
            .join("data/workflows/project/verification/stage-check");
        for (name, replacement) in [
            ("check.stdout", b"tampered".as_slice()),
            ("intent.json", b"{}"),
            ("result.json", b"{partial"),
            ("snapshot.json", b"{}"),
            ("check.json", b"{}"),
        ] {
            let path = evidence.join(name);
            let original = fs::read(&path).unwrap();
            fs::write(&path, replacement).unwrap();
            assert!(
                host.handle(detail_request()).error.is_some(),
                "{mode} {name}"
            );
            assert_eq!(
                host.snapshot().workflows[0].sequence,
                before.workflows[0].sequence
            );
            fs::write(&path, original).unwrap();
        }
        drop(host);
        let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
        let reopened = host
            .handle(detail_request())
            .result
            .unwrap()
            .verification_detail
            .unwrap();
        assert_eq!(reopened.evidence_digest, detail.evidence_digest);
        assert_eq!(
            host.snapshot().workflows[0].sequence,
            before.workflows[0].sequence
        );
    }
}
#[cfg(windows)]
#[test]
fn active_stage_detail_does_not_poll_collect_or_rerun() {
    use cerulean_domain::verification_detail::VerificationStageState as State;
    let (root, mut host, _, pins) = async_fixture("detail-slow-second");
    host.start_verification("project", "stage-check", "task", "attempt", pins)
        .unwrap();
    let marker = root
        .path()
        .join("data/workflows/project/verification/stage-check/second-temp/validator-started");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
    while !marker.exists() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let sequence = host.snapshot().workflows[0].sequence;
    for _ in 0..3 {
        let response = host.handle(detail_request());
        assert!(response.error.is_none(), "{:?}", response.error);
        let detail = response.result.unwrap().verification_detail.unwrap();
        assert!(!detail.ledger_recorded);
        assert!(matches!(detail.stages[0].state, State::Observed));
        assert!(matches!(detail.stages[1].state, State::Pending));
        assert_eq!(detail.state, VerificationState::Running);
        assert_eq!(host.snapshot().workflows[0].sequence, sequence);
    }
    host.handle(Request {
        id: "cancel".into(),
        protocol_version: PROTOCOL_VERSION,
        command: Command::CancelVerification {
            project_id: "project".into(),
            operation_id: "stage-check".into(),
        },
    })
    .result
    .unwrap();
    complete_detail_verification(&mut host);
    assert_ne!(
        host.snapshot().verification_jobs[0].state,
        VerificationState::Succeeded
    );
    let detail = host
        .handle(detail_request())
        .result
        .unwrap()
        .verification_detail
        .unwrap();
    assert!(!detail.ledger_recorded);
    assert!(matches!(detail.stages[0].state, State::Observed));
    assert!(!host.snapshot().workflows[0].state.tasks["task"].verified);
}
#[cfg(windows)]
#[test]
fn terminal_revision_requires_user_and_preserves_old_detail_and_attempts() {
    use cerulean_domain::verification_detail::VerificationStageState as State;
    let (root, mut host, mut plan, pins) = async_fixture("detail-success");
    host.start_verification("project", "stage-check", "task", "attempt", pins)
        .unwrap();
    complete_detail_verification(&mut host);
    let original = host
        .handle(detail_request())
        .result
        .unwrap()
        .verification_detail
        .unwrap();
    drop(host);
    let ledger = root.path().join("data/workflows/project");
    let config = serde_json::from_slice(&fs::read(ledger.join("bindings.json")).unwrap()).unwrap();
    let mut store = DurableWorkflow::open(&ledger, config).unwrap();
    apply(
        &mut store,
        "chief",
        WorkflowCommand::Accept {
            task: "task".into(),
            attempt: "attempt".into(),
        },
        CommandContext::default(),
    )
    .unwrap();
    drop(store);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    plan.workflow.version += 1;
    plan.execution[0].checks[0].name = Some("新的检查名称".into());
    assert!(host
        .propose("project", "chief-next-revision", plan.clone())
        .unwrap_err()
        .contains("cannot replace plan after assignment"));
    let historical = host
        .handle(detail_request())
        .result
        .unwrap()
        .verification_detail
        .unwrap();
    assert_eq!(historical.revision_digest, original.revision_digest);
    assert_eq!(historical.stages[0].name, "语法检查");
    let candidate = host.snapshot().project_revisions[0].clone();
    let before_sequence = host.snapshot().workflows[0].sequence;
    for (hash, reason) in [
        ("0".repeat(64), "valid"),
        (candidate.evidence_digest.clone(), " "),
        (candidate.evidence_digest.clone(), "bad\0reason"),
    ] {
        assert!(host
            .handle(Request {
                id: "reject-revision".into(),
                protocol_version: PROTOCOL_VERSION,
                command: Command::RequestProjectRevision {
                    project_id: "project".into(),
                    evidence_digest: hash,
                    reason: reason.into()
                }
            })
            .error
            .is_some());
        assert_eq!(host.snapshot().workflows[0].sequence, before_sequence);
    }
    let request = || Request {
        id: "open-revision".into(),
        protocol_version: PROTOCOL_VERSION,
        command: Command::RequestProjectRevision {
            project_id: "project".into(),
            evidence_digest: candidate.evidence_digest.clone(),
            reason: "下一版继续完善功能".into(),
        },
    };
    let requested = host.handle(request()).result.unwrap();
    assert!(requested.workflows[0].state.revision_requested.is_some());
    assert_eq!(requested.workflows[0].sequence, before_sequence + 1);
    assert!(host.handle(request()).error.is_some());
    drop(host);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert!(host.snapshot().workflows[0]
        .state
        .revision_requested
        .is_some());
    host.propose("project", "chief-next-revision", plan.clone())
        .unwrap();
    let fresh = host.snapshot();
    assert!(fresh.workflows[0].approval.is_none());
    assert!(!fresh.workflows[0].state.approved);
    assert_eq!(fresh.workflows[0].state.tasks["task"].fence, 1);
    assert_eq!(
        fresh.workflows[0].state.tasks["task"].state,
        TaskState::Pending
    );
    assert!(fresh.workflows[0].state.revision_requested.is_none());
    assert!(host
        .handle(Request {
            id: "old-approval".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::ApproveRevision {
                project_id: "project".into(),
                digest: original.revision_digest.clone()
            }
        })
        .error
        .is_some());
    assert_eq!(
        host.handle(detail_request())
            .result
            .unwrap()
            .verification_detail
            .unwrap()
            .revision_digest,
        original.revision_digest
    );
    host_approve(&mut host, &plan);
    assert!(host.snapshot().codex_jobs.is_empty());
    drop(host);
    let config = serde_json::from_slice(&fs::read(ledger.join("bindings.json")).unwrap()).unwrap();
    let mut store = DurableWorkflow::open(&ledger, config).unwrap();
    let mut attempt = store.attempt_record("task", "attempt").unwrap().clone();
    attempt.fence = 2;
    attempt.revision_digest = plan.approval_digest().unwrap();
    let assign_command = |id: &str| WorkflowCommand::Assign {
        task: "task".into(),
        worker: attempt.worker_id.clone(),
        attempt: id.into(),
    };
    assert!(apply(
        &mut store,
        "deputy",
        assign_command("attempt"),
        CommandContext {
            execution: Some(ExecutionEvidence::Attempt {
                attempt: attempt.clone()
            }),
            ..Default::default()
        }
    )
    .is_err());
    let command = assign_command("attempt-next");
    attempt.id = "attempt-next".into();
    apply(
        &mut store,
        "deputy",
        command,
        CommandContext {
            execution: Some(ExecutionEvidence::Attempt { attempt }),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(store.state().tasks["task"].fence, 2);
    assert_eq!(
        store
            .history()
            .iter()
            .filter(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. }))
            .count(),
        1
    );
    drop(store);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(
        host.handle(detail_request())
            .result
            .unwrap()
            .verification_detail
            .unwrap()
            .revision_digest,
        original.revision_digest
    );
    drop(host);
    // Simulate the pre-Verify crash boundary, retaining the real result files.
    let mut events = fs::read_dir(ledger.join("events"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect::<Vec<_>>();
    events.sort();
    let mut found = false;
    for path in events {
        let entry: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        if entry["body"]["operationId"] == "stage-check" {
            found = true;
        }
        if found {
            fs::remove_file(path).unwrap();
        }
    }
    assert!(found);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    let unknown = host
        .handle(detail_request())
        .result
        .unwrap()
        .verification_detail
        .unwrap();
    assert_eq!(unknown.state, VerificationState::Unknown);
    assert!(!unknown.ledger_recorded);
    assert!(unknown
        .stages
        .iter()
        .all(|s| matches!(s.state, State::Observed)));
    assert!(!host.snapshot().workflows[0].state.tasks["task"].verified);
}
#[cfg(windows)]
#[tokio::test]
#[ignore = "explicit one-turn live Codex integration; isolated reports directory and existing login required"]
async fn live_codex_dispatch_and_verify() {
    use cerulean_adapters::codex::CodexClient;
    use cerulean_core::{codex_execution, verifier::PinnedCheck};
    use std::{path::PathBuf, sync::atomic::AtomicBool};
    let root = PathBuf::from(
        std::env::var_os("CERULEAN_LIVE_CODEX_DIR").expect("isolated report directory required"),
    )
    .canonicalize()
    .unwrap();
    let repository = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    assert!(root.starts_with(repository.join("reports").canonicalize().unwrap()));
    assert!(
        !root.join("ledger").exists() && !root.join("project").exists(),
        "never rerun a claimed live case"
    );
    let executable = PathBuf::from(
        std::env::var_os("CERULEAN_LIVE_CODEX_EXE").expect("explicit Codex executable required"),
    );
    let node = PathBuf::from(
        std::env::var_os("CERULEAN_LIVE_NODE_EXE").expect("explicit Node executable required"),
    );
    let (mut store, mut p) = proposed(&root);
    p.workflow.version = 2;
    p.policy.budget_usd_cents = None;
    p.policy.max_requests = 1;
    p.policy.max_seconds = 180;
    p.workflow.tasks[0].goal="Create only sum.mjs. Export function sum(values). Require an array of finite numbers; throw TypeError for non-arrays, non-number elements, NaN or Infinity. Return the arithmetic sum, return 0 for an empty array, preserve the original array. Use native JavaScript ES modules without dependencies. Do not modify result.txt, create tests, install packages or use network.".into();
    let validator = repository.join("tests/fixtures/sum-validator.mjs");
    p.execution[0].artifact_paths = vec!["sum.mjs".into()];
    p.execution[0].checks = vec![AcceptanceCheck {
        name: None,
        id: "sum-behavior".into(),
        program: "independent-sum-validator".into(),
        args: vec![validator.to_string_lossy().into()],
        timeout_seconds: 15,
        tool: Some(
            PinnedCheck::describe(&node, std::slice::from_ref(&validator), Default::default())
                .unwrap(),
        ),
    }];
    p.snapshot_policy = Some(SnapshotPolicy {
        exclude: vec![],
        max_files: 100,
        max_bytes: 1024 * 1024,
    });
    apply(
        &mut store,
        "chief",
        WorkflowCommand::Propose {
            plan: p.workflow.clone(),
        },
        CommandContext {
            execution: Some(ExecutionEvidence::Revision {
                revision: p.clone(),
            }),
            ..Default::default()
        },
    )
    .unwrap();
    // Deliberately labeled fixture approval: this test is not product chief/user acceptance.
    approve(&mut store, &p);
    fs::write(
        root.join("approved-fixture-plan.json"),
        serde_json::to_vec_pretty(&p).unwrap(),
    )
    .unwrap();
    let mut report = serde_json::json!({"startedAt":chrono::Utc::now().to_rfc3339(),"status":"starting","scope":"preapproved integration fixture; real worker and independent checks, not real chief/deputy/user-approval acceptance","modelTurnsRequested":0});
    fs::write(
        root.join("result.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    let mut client = CodexClient::launch(&executable, Path::new(&p.project.root)).unwrap();
    let outcome: Result<(), String> = async {
        let connected = client.connect().await?;
        if connected.account_type.as_deref() != Some("chatgpt") {
            return Err("live test requires existing ChatGPT login".into());
        }
        let model = &connected
            .models
            .iter()
            .find(|m| m.is_default)
            .ok_or("default model missing")?
            .model;
        let binding = client.create_worker(model).await?;
        report["binding"] = serde_json::to_value(&binding).unwrap();
        let a = Attempt {
            workspace_proof: None,
            id: "live-attempt".into(),
            task_id: "task".into(),
            worker_id: "worker-codex".into(),
            revision_digest: p.approval_digest()?,
            fence: 1,
            binding: SessionBinding {
                adapter_id: "codex".into(),
                adapter_version: connected.server_version,
                session_id: binding.thread_id.clone(),
                identity_strength: IdentityStrength::ProviderId,
                workspace: p.project.root.clone(),
            },
        };
        apply(
            &mut store,
            "deputy",
            WorkflowCommand::Assign {
                task: "task".into(),
                worker: a.worker_id.clone(),
                attempt: a.id.clone(),
            },
            CommandContext {
                execution: Some(ExecutionEvidence::Attempt { attempt: a }),
                ..Default::default()
            },
        )?;
        report["modelTurnsRequested"] = 1.into();
        report["status"] = "dispatching".into();
        fs::write(
            root.join("result.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .map_err(|e| e.to_string())?;
        let observation =
            codex_execution::dispatch(&mut store, &mut client, &binding, "task", "live-attempt")
                .await?;
        report["observation"] = serde_json::to_value(&observation).unwrap();
        let pin = PinnedCheck::from_approved(&p.execution[0].checks[0])?;
        store.run_verification(
            "live-independent-check",
            "task",
            "live-attempt",
            &[pin],
            &AtomicBool::new(false),
        )?;
        if !store.state().tasks["task"].verified {
            return Err("independent validator failed".into());
        }
        report["status"] = "worker-and-verifier-passed".into();
        Ok(())
    }
    .await;
    let cleanup = client.close().await;
    if let Err(error) = &outcome {
        report["status"] = "failed".into();
        report["error"] = error.clone().into();
    }
    report["cleanupPassed"] = cleanup.is_ok().into();
    report["finishedAt"] = chrono::Utc::now().to_rfc3339().into();
    report["state"] = serde_json::to_value(store.state()).unwrap();
    fs::write(
        root.join("result.json"),
        serde_json::to_vec_pretty(&report).unwrap(),
    )
    .unwrap();
    drop(store);
    outcome.unwrap();
    cleanup.unwrap();
    let reopened = DurableWorkflow::open(&root.join("ledger"), config(&p)).unwrap();
    assert!(reopened.state().tasks["task"].verified);
    assert_ne!(reopened.state().tasks["task"].state, TaskState::Accepted);
}

#[cfg(windows)]
#[tokio::test]
#[ignore = "explicit read-only Codex reconciliation against a previously completed live fixture"]
async fn live_codex_reconcile_lost_ack_without_resend() {
    use cerulean_adapters::codex::CodexClient;
    use cerulean_core::{codex_execution, verifier::PinnedCheck};
    use std::{path::PathBuf, sync::atomic::AtomicBool};
    let root = PathBuf::from(
        std::env::var_os("CERULEAN_LIVE_CODEX_DIR").expect("existing live report required"),
    )
    .canonicalize()
    .unwrap();
    let reports = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../reports")
        .canonicalize()
        .unwrap();
    assert!(root.starts_with(reports));
    let previous: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("result.json")).unwrap()).unwrap();
    assert_eq!(previous["status"], "worker-and-verifier-passed");
    let plan: PlanRevision =
        serde_json::from_slice(&fs::read(root.join("approved-fixture-plan.json")).unwrap())
            .unwrap();
    let recovery = root.join("recovery-ledger-with-bindings");
    assert!(
        !recovery.exists(),
        "recovery fixture cannot be silently restarted"
    );
    fs::create_dir(&recovery).unwrap();
    fs::create_dir(recovery.join("events")).unwrap();
    fs::copy(
        root.join("ledger/bindings.json"),
        recovery.join("bindings.json"),
    )
    .unwrap();
    let mut events = fs::read_dir(root.join("ledger/events"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect::<Vec<_>>();
    events.sort();
    let mut copied = 0;
    for event in events {
        let bytes = fs::read(&event).unwrap();
        let record: Entry = serde_json::from_slice(&bytes).unwrap();
        fs::write(
            recovery.join("events").join(event.file_name().unwrap()),
            bytes,
        )
        .unwrap();
        copied += 1;
        if matches!(
            record.body.event.command,
            WorkflowCommand::DispatchIntent { .. }
        ) {
            break;
        }
    }
    assert_eq!(copied, 5);
    let mut store = DurableWorkflow::open(&recovery, config(&plan)).unwrap();
    assert_eq!(store.state().tasks["task"].state, TaskState::Unknown);
    let executable = PathBuf::from(std::env::var_os("CERULEAN_LIVE_CODEX_EXE").unwrap());
    let mut client = CodexClient::launch(&executable, Path::new(&plan.project.root)).unwrap();
    let result: Result<_, String> = async {
        client.connect().await?;
        let proof =
            codex_execution::reconcile(&mut store, &mut client, "task", "live-attempt").await?;
        if proof.turn_id()
            != previous["observation"]["turnId"]
                .as_str()
                .ok_or("original turn missing")?
        {
            return Err("reconciliation changed turn".into());
        }
        store.run_verification(
            "recovered-independent-check",
            "task",
            "live-attempt",
            &[PinnedCheck::from_approved(&plan.execution[0].checks[0])?],
            &AtomicBool::new(false),
        )?;
        if !store.state().tasks["task"].verified {
            return Err("recovered verification failed".into());
        }
        Ok(proof)
    }
    .await;
    let cleanup = client.close().await;
    fs::write(root.join("recovery-result.json"),serde_json::to_vec_pretty(&serde_json::json!({
        "at":chrono::Utc::now().to_rfc3339(),"status":if result.is_ok()&&cleanup.is_ok(){"passed"}else{"failed"},
        "fault":"copy of actual ledger truncated after dispatch intent; original ledger preserved",
        "newModelTurns":0,"sameOriginalTurn":result.as_ref().map(|proof|proof.turn_id().to_owned()).ok(),"error":result.as_ref().err(),"cleanupPassed":cleanup.is_ok(),"state":store.state()
    })).unwrap()).unwrap();
    result.unwrap();
    cleanup.unwrap();
    assert!(!store
        .history()
        .iter()
        .any(|entry| matches!(entry.body.event.command, WorkflowCommand::Started { .. })));
    assert_eq!(
        store
            .history()
            .iter()
            .filter(|entry| matches!(
                entry.body.event.command,
                WorkflowCommand::DispatchIntent { .. }
            ))
            .count(),
        1
    );
}
fn config(p: &PlanRevision) -> SchedulerConfig {
    SchedulerConfig {
        schema_version: 1,
        core_id: "core".into(),
        execution_scope: Some(ExecutionScope {
            project: p.project.clone(),
            policy_version: 1,
        }),
        principals: vec![
            Principal {
                id: "user".into(),
                role: Role::User,
                provider: None,
            },
            Principal {
                id: "chief".into(),
                role: Role::Chief,
                provider: Some("openai".into()),
            },
            Principal {
                id: "deputy".into(),
                role: Role::Deputy,
                provider: Some("openai".into()),
            },
            Principal {
                id: "core".into(),
                role: Role::Core,
                provider: None,
            },
            Principal {
                id: "worker-codex".into(),
                role: Role::Worker,
                provider: None,
            },
        ],
    }
}

fn abandon_unsent_command() -> WorkflowCommand {
    WorkflowCommand::AbandonUnsent {
        task: "task".into(),
        attempt: "attempt".into(),
        reason: "用户确认只放弃本地尚未发送的尝试".into(),
    }
}

#[test]
fn abandon_unsent_strict_context_replay_and_fence_preserve_original_binding() {
    let root = tempfile::tempdir().unwrap();
    let (mut store, plan) = proposed(root.path());
    approve(&mut store, &plan);
    let original = assign(&mut store, &plan);
    let before = store.history().to_vec();
    for principal in ["chief", "deputy", "core", "worker-codex"] {
        assert!(apply(
            &mut store,
            principal,
            abandon_unsent_command(),
            CommandContext::default()
        )
        .is_err());
    }
    for context in [
        CommandContext {
            execution: Some(ExecutionEvidence::Attempt {
                attempt: original.clone(),
            }),
            ..Default::default()
        },
        CommandContext {
            thread_id: Some("session".into()),
            ..Default::default()
        },
        CommandContext {
            cancellation_source: Some("pretend remote cancellation".into()),
            ..Default::default()
        },
    ] {
        assert!(apply(&mut store, "user", abandon_unsent_command(), context).is_err());
    }
    assert_eq!(store.history(), before);
    let abandoned = apply(
        &mut store,
        "user",
        abandon_unsent_command(),
        CommandContext::default(),
    )
    .unwrap();
    assert_eq!(abandoned.body.event.principal, "user");
    assert_eq!(store.state().tasks["task"].state, TaskState::Pending);
    assert_eq!(store.state().tasks["task"].fence, 1);
    assert_eq!(store.attempt_record("task", "attempt"), Some(&original));
    assert_eq!(&store.history()[..before.len()], before);
    assert!(store
        .handoffs()
        .unwrap()
        .iter()
        .any(|volume| volume.markdown.contains("尚未发送的本地尝试")
            && volume.markdown.contains("attempt")));
    drop(store);
    let mut reopened = DurableWorkflow::open(&root.path().join("ledger"), config(&plan)).unwrap();
    assert_eq!(reopened.state().tasks["task"].state, TaskState::Pending);
    assert_eq!(reopened.state().tasks["task"].fence, 1);
    let mut next = original.clone();
    next.fence = 2;
    assert!(apply(
        &mut reopened,
        "deputy",
        WorkflowCommand::Assign {
            task: "task".into(),
            worker: "worker-codex".into(),
            attempt: "attempt".into()
        },
        CommandContext {
            execution: Some(ExecutionEvidence::Attempt {
                attempt: next.clone()
            }),
            ..Default::default()
        }
    )
    .unwrap_err()
    .contains("attempt already used"));
    next.id = "fresh-attempt".into();
    apply(
        &mut reopened,
        "deputy",
        WorkflowCommand::Assign {
            task: "task".into(),
            worker: "worker-codex".into(),
            attempt: next.id.clone(),
        },
        CommandContext {
            execution: Some(ExecutionEvidence::Attempt {
                attempt: next.clone(),
            }),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(reopened.state().tasks["task"].fence, 2);
    assert_eq!(reopened.attempt_record("task", "attempt"), Some(&original));
    assert!(apply(
        &mut reopened,
        "user",
        abandon_unsent_command(),
        CommandContext::default()
    )
    .is_err());
}

#[test]
fn abandon_unsent_strict_rejects_not_started_after_a_durable_dispatch_intent() {
    let root = tempfile::tempdir().unwrap();
    let (mut store, plan) = proposed(root.path());
    approve(&mut store, &plan);
    let attempt = assign(&mut store, &plan);
    dispatch(&mut store, &plan);
    assert!(apply(
        &mut store,
        "user",
        abandon_unsent_command(),
        CommandContext::default()
    )
    .is_err());
    drop(store);
    let mut reopened = DurableWorkflow::open(&root.path().join("ledger"), config(&plan)).unwrap();
    assert_eq!(reopened.state().tasks["task"].state, TaskState::Unknown);
    let mut observation = receipt(&attempt, ReceiptStatus::NotStarted);
    observation.turn_id = None;
    let evidence = digest(&observation).unwrap();
    apply(
        &mut reopened,
        "core",
        WorkflowCommand::Reconcile {
            task: "task".into(),
            attempt: "attempt".into(),
            outcome: ReconcileOutcome::NotStarted,
            evidence: vec![evidence],
        },
        ctx(observation, None),
    )
    .unwrap();
    assert_eq!(reopened.state().tasks["task"].state, TaskState::Assigned);
    assert!(apply(
        &mut reopened,
        "user",
        abandon_unsent_command(),
        CommandContext::default()
    )
    .unwrap_err()
    .contains("dispatch history"));
}

#[test]
fn abandon_unsent_published_event_recovers_after_projection_interruption() {
    let root = tempfile::tempdir().unwrap();
    let (mut store, plan) = proposed(root.path());
    approve(&mut store, &plan);
    assign(&mut store, &plan);
    let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = store.command_observed(
            "abandon-crash",
            "user",
            abandon_unsent_command(),
            CommandContext::default(),
            true,
            |point| {
                if point == CommitPoint::EventPublished {
                    panic!("synthetic crash after event publication");
                }
            },
        );
    }));
    assert!(interrupted.is_err());
    assert!(store
        .command(
            "abandon-crash",
            "user",
            abandon_unsent_command(),
            CommandContext::default(),
            true,
        )
        .unwrap_err()
        .contains("workflow requires reopen"));
    drop(store);
    let mut reopened = DurableWorkflow::open(&root.path().join("ledger"), config(&plan)).unwrap();
    assert_eq!(reopened.state().tasks["task"].state, TaskState::Pending);
    assert_eq!(reopened.state().tasks["task"].fence, 1);
    assert_eq!(
        reopened
            .history()
            .iter()
            .filter(|entry| matches!(
                entry.body.event.command,
                WorkflowCommand::AbandonUnsent { .. }
            ))
            .count(),
        1
    );
    assert!(reopened
        .command(
            "abandon-crash",
            "user",
            abandon_unsent_command(),
            CommandContext::default(),
            true
        )
        .is_err());
    assert!(reopened.history().iter().all(|entry| !matches!(
        entry.body.event.command,
        WorkflowCommand::DispatchIntent { .. }
    )));
}
fn apply(
    s: &mut DurableWorkflow,
    who: &str,
    command: WorkflowCommand,
    context: CommandContext,
) -> Result<Entry, String> {
    s.command(
        &format!("op-{}", s.history().len() + 1),
        who,
        command,
        context,
        false,
    )
}
fn proposed(root: &Path) -> (DurableWorkflow, PlanRevision) {
    let project = root.join("project");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("result.txt"), "abc").unwrap();
    let p = plan(&project);
    let mut s = DurableWorkflow::open(&root.join("ledger"), config(&p)).unwrap();
    apply(
        &mut s,
        "chief",
        WorkflowCommand::Propose {
            plan: p.workflow.clone(),
        },
        CommandContext {
            execution: Some(ExecutionEvidence::Revision {
                revision: p.clone(),
            }),
            ..Default::default()
        },
    )
    .unwrap();
    (s, p)
}

#[test]
fn omp_controller_request_replays_submission_and_cannot_regress_started_to_unsent() {
    for finish in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        fs::create_dir(&project).unwrap();
        fs::write(project.join("result.txt"), "fixture-output").unwrap();
        let mut p = plan(&project);
        p.execution[0].adapter_id = "omp".into();
        let ledger = root.path().join("ledger");
        let cfg = config(&p);
        let mut s = DurableWorkflow::open(&ledger, cfg.clone()).unwrap();
        apply(
            &mut s,
            "chief",
            WorkflowCommand::Propose {
                plan: p.workflow.clone(),
            },
            CommandContext {
                execution: Some(ExecutionEvidence::Revision {
                    revision: p.clone(),
                }),
                ..Default::default()
            },
        )
        .unwrap();
        approve(&mut s, &p);
        let a = Attempt {
            workspace_proof: None,
            id: "attempt".into(),
            task_id: "task".into(),
            worker_id: "worker-codex".into(),
            revision_digest: p.approval_digest().unwrap(),
            fence: 1,
            binding: SessionBinding {
                adapter_id: "omp".into(),
                adapter_version: "18.4.4".into(),
                session_id: "session".into(),
                workspace: p.project.root.clone(),
                identity_strength: IdentityStrength::ControllerRequest(ControllerRequestIdentity {
                    connection_id: "owned-connection".into(),
                    request_id: "request".into(),
                }),
            },
        };
        apply(
            &mut s,
            "deputy",
            WorkflowCommand::Assign {
                task: "task".into(),
                worker: a.worker_id.clone(),
                attempt: a.id.clone(),
            },
            CommandContext {
                execution: Some(ExecutionEvidence::Attempt { attempt: a.clone() }),
                ..Default::default()
            },
        )
        .unwrap();
        dispatch(&mut s, &p);
        let mut running = receipt(&a, ReceiptStatus::Running);
        running.turn_id = None;
        let mut fake = running.clone();
        fake.turn_id = Some("request".into());
        assert!(apply(
            &mut s,
            "core",
            WorkflowCommand::Started {
                task: "task".into(),
                attempt: a.id.clone()
            },
            ctx(fake, None)
        )
        .is_err());
        apply(
            &mut s,
            "core",
            WorkflowCommand::Started {
                task: "task".into(),
                attempt: a.id.clone(),
            },
            ctx(running, None),
        )
        .unwrap();
        if finish {
            let mut completed = receipt(&a, ReceiptStatus::Completed);
            completed.turn_id = None;
            let manifest =
                ArtifactObserver::new(&p, s.approval().unwrap(), "user", 1, &a, &completed, None)
                    .unwrap()
                    .capture()
                    .unwrap();
            let mut foreign = completed.clone();
            foreign.binding.identity_strength =
                IdentityStrength::ControllerRequest(ControllerRequestIdentity {
                    connection_id: "foreign".into(),
                    request_id: "request".into(),
                });
            let submit = WorkflowCommand::Submit {
                task: "task".into(),
                attempt: a.id.clone(),
                summary: "fixture only".into(),
                evidence: Some(vec![digest(&manifest).unwrap()]),
            };
            assert!(apply(
                &mut s,
                &a.worker_id,
                submit.clone(),
                ctx(foreign, Some(manifest.clone()))
            )
            .is_err());
            apply(&mut s, &a.worker_id, submit, ctx(completed, Some(manifest))).unwrap();
            assert_eq!(s.state().tasks["task"].state, TaskState::Submitted);
            // A typed completion still cannot replace independent verification.
            assert!(apply(
                &mut s,
                "chief",
                WorkflowCommand::Accept {
                    task: "task".into(),
                    attempt: a.id.clone()
                },
                CommandContext::default()
            )
            .is_err());
        } else {
            apply(
                &mut s,
                "core",
                WorkflowCommand::RecoveryUnknown {
                    task: "task".into(),
                    attempt: a.id.clone(),
                },
                CommandContext::default(),
            )
            .unwrap();
            let mut unsent = receipt(&a, ReceiptStatus::NotStarted);
            unsent.turn_id = None;
            assert!(apply(
                &mut s,
                "core",
                WorkflowCommand::Reconcile {
                    task: "task".into(),
                    attempt: a.id.clone(),
                    outcome: ReconcileOutcome::NotStarted,
                    evidence: vec!["a".repeat(64)]
                },
                ctx(unsent, None)
            )
            .is_err());
            assert_eq!(s.state().tasks["task"].state, TaskState::Unknown);
        }
        let count = s.history().len();
        drop(s);
        let s = DurableWorkflow::open(&ledger, cfg).unwrap();
        assert_eq!(s.history().len(), count);
        assert_eq!(
            s.state().tasks["task"].state,
            if finish {
                TaskState::Submitted
            } else {
                TaskState::Unknown
            }
        );
        assert!(s.history().iter().all(|e| e.body.context.turn_id.is_none()));
    }
}
fn approve(s: &mut DurableWorkflow, p: &PlanRevision) {
    let approval = Approval {
        plan_id: p.workflow.id.clone(),
        revision: p.workflow.version,
        policy_version: 1,
        revision_digest: p.approval_digest().unwrap(),
        user_id: "user".into(),
        approved_at: "2026-10-07T15:00:00Z".into(),
    };
    apply(
        s,
        "user",
        WorkflowCommand::Approve {
            digest: s.state().digest.clone().unwrap(),
        },
        CommandContext {
            approval_source: Some("fixture".into()),
            execution: Some(ExecutionEvidence::Approval { approval }),
            ..Default::default()
        },
    )
    .unwrap();
}
fn assign(s: &mut DurableWorkflow, p: &PlanRevision) -> Attempt {
    let attempt = Attempt {
        workspace_proof: None,
        id: "attempt".into(),
        task_id: "task".into(),
        worker_id: "worker-codex".into(),
        revision_digest: p.approval_digest().unwrap(),
        fence: 1,
        binding: SessionBinding {
            adapter_id: "codex".into(),
            adapter_version: "fixture-1".into(),
            session_id: "session".into(),
            identity_strength: IdentityStrength::ProviderId,
            workspace: p.project.root.clone(),
        },
    };
    apply(
        s,
        "deputy",
        WorkflowCommand::Assign {
            task: "task".into(),
            worker: "worker-codex".into(),
            attempt: "attempt".into(),
        },
        CommandContext {
            execution: Some(ExecutionEvidence::Attempt {
                attempt: attempt.clone(),
            }),
            ..Default::default()
        },
    )
    .unwrap();
    attempt
}
fn dispatch(s: &mut DurableWorkflow, p: &PlanRevision) {
    apply(
        s,
        "core",
        WorkflowCommand::DispatchIntent {
            task: "task".into(),
            attempt: "attempt".into(),
        },
        CommandContext {
            thread_id: Some("session".into()),
            cwd: Some(p.project.root.clone()),
            ..Default::default()
        },
    )
    .unwrap();
}
fn receipt(attempt: &Attempt, status: ReceiptStatus) -> ExecutionReceipt {
    ExecutionReceipt {
        schema_version: 1,
        task_id: "task".into(),
        attempt_id: attempt.id.clone(),
        fence: attempt.fence,
        binding: attempt.binding.clone(),
        turn_id: Some("turn".into()),
        observed_at: "2026-10-07T15:01:00Z".into(),
        status,
        evidence_sha256: "a".repeat(64),
    }
}
fn ctx(receipt: ExecutionReceipt, manifest: Option<ArtifactManifest>) -> CommandContext {
    CommandContext {
        thread_id: Some(receipt.binding.session_id.clone()),
        turn_id: receipt.turn_id.clone(),
        terminal_status: if receipt.status == ReceiptStatus::Completed {
            Some("completed".into())
        } else {
            None
        },
        execution: Some(ExecutionEvidence::Receipt { receipt, manifest }),
        ..Default::default()
    }
}
fn submit(s: &mut DurableWorkflow, p: &PlanRevision, a: &Attempt) -> ArtifactManifest {
    let r = receipt(a, ReceiptStatus::Completed);
    let manifest = ArtifactObserver::new(p, s.approval().unwrap(), "user", 1, a, &r, Some("turn"))
        .unwrap()
        .capture()
        .unwrap();
    apply(
        s,
        "worker-codex",
        WorkflowCommand::Submit {
            task: "task".into(),
            attempt: a.id.clone(),
            summary: "Fixture produced".into(),
            evidence: Some(vec![digest(&manifest).unwrap()]),
        },
        ctx(r, Some(manifest.clone())),
    )
    .unwrap();
    manifest
}
fn report(p: &PlanRevision, m: &ArtifactManifest) -> VerificationReport {
    VerificationReport {
        source_snapshot: None,
        schema_version: 1,
        verifier_id: "core".into(),
        observed_at: "2026-10-07T15:02:00Z".into(),
        manifest_digest: digest(m).unwrap(),
        checks: vec![CheckObservation {
            check_id: "check".into(),
            command_digest: digest(&p.execution[0].checks[0]).unwrap(),
            exit_code: Some(0),
            timed_out: false,
            stdout_sha256: "a".repeat(64),
            stderr_sha256: "b".repeat(64),
        }],
    }
}

#[cfg(windows)]
mod codex_dispatch_tests {
    use super::*;
    use cerulean_adapters::codex::{Terminal, TurnObservation, WorkerBinding};
    use cerulean_core::codex_execution::{self, WorkerChannel};
    use sha2::{Digest, Sha256};
    use std::time::Duration;
    struct Channel {
        ledger: std::path::PathBuf,
        sends: u32,
        lose_ack: bool,
        wrong_turn: bool,
        prompt: String,
        invalidated: bool,
    }
    impl WorkerChannel for Channel {
        fn invalidate(&mut self) {
            self.invalidated = true;
        }
        async fn start(
            &mut self,
            _: &WorkerBinding,
            _: &str,
            prompt: &str,
        ) -> Result<String, String> {
            let files = fs::read_dir(self.ledger.join("events")).unwrap();
            assert!(
                files
                    .filter_map(Result::ok)
                    .any(|e| fs::read_to_string(e.path())
                        .unwrap()
                        .contains("dispatch-intent")),
                "intent must precede send"
            );
            self.sends += 1;
            self.prompt = prompt.into();
            if self.lose_ack {
                Err("ack lost".into())
            } else {
                Ok("turn".into())
            }
        }
        async fn wait(
            &mut self,
            thread: &str,
            turn: &str,
            _: Duration,
        ) -> Result<Terminal, String> {
            Ok(Terminal {
                thread_id: thread.into(),
                turn_id: turn.into(),
                status: "completed".into(),
            })
        }
        async fn read(
            &mut self,
            thread: &str,
            attempt: &str,
            prompt: &str,
            _: Option<&str>,
        ) -> Result<TurnObservation, String> {
            assert!(!self.invalidated, "must reconcile with a fresh channel");
            Ok(TurnObservation {
                thread_id: thread.into(),
                turn_id: if self.wrong_turn { "wrong" } else { "turn" }.into(),
                client_message_id: attempt.into(),
                status: "completed".into(),
                prompt_sha256: format!("{:x}", Sha256::digest(prompt.as_bytes())),
                item_types: vec!["userMessage".into(), "agentMessage".into()],
            })
        }
    }
    fn ready(
        root: &Path,
        budget: Option<u32>,
    ) -> (DurableWorkflow, PlanRevision, WorkerBinding, Channel) {
        let (mut store, mut plan) = proposed(root);
        plan.workflow.version = 2;
        plan.policy.budget_usd_cents = budget;
        apply(
            &mut store,
            "chief",
            WorkflowCommand::Propose {
                plan: plan.workflow.clone(),
            },
            CommandContext {
                execution: Some(ExecutionEvidence::Revision {
                    revision: plan.clone(),
                }),
                ..Default::default()
            },
        )
        .unwrap();
        approve(&mut store, &plan);
        let attempt = assign(&mut store, &plan);
        let binding = WorkerBinding {
            thread_id: attempt.binding.session_id,
            workspace: plan.project.root.clone(),
            model: "fixture".into(),
            provider: "openai".into(),
        };
        (
            store,
            plan,
            binding,
            Channel {
                ledger: root.join("ledger"),
                sends: 0,
                lose_ack: false,
                wrong_turn: false,
                prompt: String::new(),
                invalidated: false,
            },
        )
    }
    #[tokio::test]
    async fn dispatch_persists_intent_then_bound_receipts_and_never_resends() {
        let root = tempfile::tempdir().unwrap();
        let (mut store, _, binding, mut channel) = ready(root.path(), None);
        codex_execution::dispatch(&mut store, &mut channel, &binding, "task", "attempt")
            .await
            .unwrap();
        assert_eq!(store.state().tasks["task"].state, TaskState::Submitted);
        assert!(!store.state().tasks["task"].verified);
        assert!(store.completed_artifacts("task", "attempt").is_some());
        assert!(
            codex_execution::dispatch(&mut store, &mut channel, &binding, "task", "attempt")
                .await
                .is_err()
        );
        assert_eq!(channel.sends, 1);
    }
    #[tokio::test]
    async fn lost_ack_reopens_unknown_and_reads_original_attempt_without_sending() {
        let root = tempfile::tempdir().unwrap();
        let (mut store, plan, binding, mut channel) = ready(root.path(), None);
        channel.lose_ack = true;
        assert!(
            codex_execution::dispatch(&mut store, &mut channel, &binding, "task", "attempt")
                .await
                .is_err()
        );
        assert_eq!(store.state().tasks["task"].state, TaskState::Unknown);
        assert!(channel.invalidated);
        assert!(
            codex_execution::dispatch(&mut store, &mut channel, &binding, "task", "attempt")
                .await
                .is_err()
        );
        assert_eq!(channel.sends, 1);
        drop(store);
        let mut store = DurableWorkflow::open(&root.path().join("ledger"), config(&plan)).unwrap();
        channel.invalidated = false;
        codex_execution::reconcile(&mut store, &mut channel, "task", "attempt")
            .await
            .unwrap();
        assert_eq!(store.state().tasks["task"].state, TaskState::Submitted);
        assert_eq!(channel.sends, 1);
        assert!(!store.state().tasks["task"].verified);
    }
    #[tokio::test]
    async fn mismatched_readback_stays_unknown_and_unmeasured_money_budget_blocks_send() {
        let root = tempfile::tempdir().unwrap();
        let (mut store, _, binding, mut channel) = ready(root.path(), None);
        channel.wrong_turn = true;
        assert!(
            codex_execution::dispatch(&mut store, &mut channel, &binding, "task", "attempt")
                .await
                .is_err()
        );
        assert_eq!(store.state().tasks["task"].state, TaskState::Unknown);
        channel.invalidated = false;
        assert!(
            codex_execution::reconcile(&mut store, &mut channel, "task", "attempt")
                .await
                .is_err()
        );
        assert_eq!(channel.sends, 1);
        let other = tempfile::tempdir().unwrap();
        let (mut store, _, binding, mut channel) = ready(other.path(), Some(25));
        assert!(
            codex_execution::dispatch(&mut store, &mut channel, &binding, "task", "attempt")
                .await
                .is_err()
        );
        assert_eq!(channel.sends, 0);
        assert_eq!(store.state().tasks["task"].state, TaskState::Assigned);
    }
}
#[test]
fn strict_workflow_refuses_legacy_approval_missing_attempt_and_unproven_verification() {
    let root = tempfile::tempdir().unwrap();
    let (mut s, p) = proposed(root.path());
    let old = WorkflowCommand::Approve {
        digest: s.state().digest.clone().unwrap(),
    };
    assert!(apply(
        &mut s,
        "user",
        old,
        CommandContext {
            approval_source: Some("old plan".into()),
            ..Default::default()
        }
    )
    .unwrap_err()
    .contains("full revision"));
    approve(&mut s, &p);
    assert!(apply(
        &mut s,
        "deputy",
        WorkflowCommand::Assign {
            task: "task".into(),
            worker: "worker-codex".into(),
            attempt: "attempt".into()
        },
        CommandContext::default()
    )
    .is_err());
    let a = assign(&mut s, &p);
    dispatch(&mut s, &p);
    apply(
        &mut s,
        "core",
        WorkflowCommand::Started {
            task: "task".into(),
            attempt: a.id.clone(),
        },
        ctx(receipt(&a, ReceiptStatus::Running), None),
    )
    .unwrap();
    submit(&mut s, &p, &a);
    assert!(apply(
        &mut s,
        "core",
        WorkflowCommand::Verify {
            task: "task".into(),
            attempt: a.id.clone(),
            passed: true,
            evidence: vec!["self declared".into()]
        },
        CommandContext::default()
    )
    .is_err());
    assert!(!s.state().tasks["task"].verified);
}
#[test]
fn acceptance_rehashes_files_and_replay_preserves_historical_evidence_without_recertifying() {
    let root = tempfile::tempdir().unwrap();
    let (mut s, p) = proposed(root.path());
    approve(&mut s, &p);
    let a = assign(&mut s, &p);
    dispatch(&mut s, &p);
    apply(
        &mut s,
        "core",
        WorkflowCommand::Started {
            task: "task".into(),
            attempt: a.id.clone(),
        },
        ctx(receipt(&a, ReceiptStatus::Running), None),
    )
    .unwrap();
    let m = submit(&mut s, &p, &a);
    let proof = report(&p, &m);
    apply(
        &mut s,
        "core",
        WorkflowCommand::Verify {
            task: "task".into(),
            attempt: a.id.clone(),
            passed: true,
            evidence: vec![digest(&proof).unwrap()],
        },
        CommandContext {
            execution: Some(ExecutionEvidence::Verification { report: proof }),
            ..Default::default()
        },
    )
    .unwrap();
    fs::write(Path::new(&p.project.root).join("result.txt"), "xyz").unwrap();
    let accept = WorkflowCommand::Accept {
        task: "task".into(),
        attempt: a.id.clone(),
    };
    assert!(apply(&mut s, "chief", accept.clone(), CommandContext::default()).is_err());
    assert_eq!(s.history().len(), 7);
    drop(s);
    let mut s = DurableWorkflow::open(&root.path().join("ledger"), config(&p)).unwrap();
    assert_eq!(s.history().len(), 7);
    assert!(apply(&mut s, "chief", accept.clone(), CommandContext::default()).is_err());
    fs::write(Path::new(&p.project.root).join("result.txt"), "abc").unwrap();
    apply(&mut s, "chief", accept, CommandContext::default()).unwrap();
    drop(s);
    fs::remove_file(Path::new(&p.project.root).join("result.txt")).unwrap();
    let s = DurableWorkflow::open(&root.path().join("ledger"), config(&p)).unwrap();
    assert_eq!(s.state().tasks["task"].state, TaskState::Accepted);
    assert_eq!(s.completed_artifacts("task", "attempt").unwrap().1, &m);
}
#[test]
fn lost_start_ack_reconciles_bound_turn_then_allows_submission_without_resend() {
    let root = tempfile::tempdir().unwrap();
    let (mut s, p) = proposed(root.path());
    approve(&mut s, &p);
    let a = assign(&mut s, &p);
    dispatch(&mut s, &p);
    drop(s);
    let mut s = DurableWorkflow::open(&root.path().join("ledger"), config(&p)).unwrap();
    assert_eq!(s.state().tasks["task"].state, TaskState::Unknown);
    let command = WorkflowCommand::Reconcile {
        task: "task".into(),
        attempt: a.id.clone(),
        outcome: ReconcileOutcome::Running,
        evidence: vec!["trusted adapter observation fixture".into()],
    };
    assert!(apply(&mut s, "core", command.clone(), CommandContext::default()).is_err());
    let mut wrong = receipt(&a, ReceiptStatus::Running);
    wrong.binding.session_id = "other".into();
    assert!(apply(&mut s, "core", command.clone(), ctx(wrong, None)).is_err());
    apply(
        &mut s,
        "core",
        command,
        ctx(receipt(&a, ReceiptStatus::Running), None),
    )
    .unwrap();
    submit(&mut s, &p, &a);
    assert_eq!(s.state().tasks["task"].state, TaskState::Submitted);
    assert_eq!(
        s.history()
            .iter()
            .filter(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. }))
            .count(),
        1
    );
}

#[cfg(windows)]
#[test]
fn replay_checkpoint_strict_acceptance_keeps_evidence_and_live_file_checks() {
    let root = tempfile::tempdir().unwrap();
    let (mut s, p) = proposed(root.path());
    approve(&mut s, &p);
    let a = assign(&mut s, &p);
    dispatch(&mut s, &p);
    apply(
        &mut s,
        "core",
        WorkflowCommand::Started {
            task: "task".into(),
            attempt: a.id.clone(),
        },
        ctx(receipt(&a, ReceiptStatus::Running), None),
    )
    .unwrap();
    let manifest = submit(&mut s, &p, &a);
    let proof = report(&p, &manifest);
    apply(
        &mut s,
        "core",
        WorkflowCommand::Verify {
            task: "task".into(),
            attempt: a.id.clone(),
            passed: true,
            evidence: vec![digest(&proof).unwrap()],
        },
        CommandContext {
            execution: Some(ExecutionEvidence::Verification { report: proof }),
            ..Default::default()
        },
    )
    .unwrap();
    while s.history().len() < 256 {
        apply(
            &mut s,
            "deputy",
            WorkflowCommand::Advise {
                task: "task".into(),
                suggestion: "preserve original verification".into(),
            },
            CommandContext::default(),
        )
        .unwrap();
    }
    let expected = s.state().clone();
    let history = s.history().to_vec();
    drop(s);
    let ledger = root.path().join("ledger");
    let mut s = DurableWorkflow::open(&ledger, config(&p)).unwrap();
    assert_eq!(s.checkpoint_events(), 256);
    assert_eq!(s.state(), &expected);
    assert_eq!(s.history(), history);
    assert_eq!(s.completed_artifacts("task", &a.id).unwrap().1, &manifest);
    fs::write(
        Path::new(&p.project.root).join("result.txt"),
        "changed after verification",
    )
    .unwrap();
    let accept = WorkflowCommand::Accept {
        task: "task".into(),
        attempt: a.id.clone(),
    };
    assert!(apply(&mut s, "chief", accept.clone(), CommandContext::default()).is_err());
    fs::write(Path::new(&p.project.root).join("result.txt"), "abc").unwrap();
    apply(&mut s, "chief", accept, CommandContext::default()).unwrap();
    let accepted = s.state().clone();
    drop(s);
    let s = DurableWorkflow::open(&ledger, config(&p)).unwrap();
    assert_eq!(s.checkpoint_events(), 256);
    assert_eq!(s.state().tasks["task"].state, TaskState::Accepted);
    assert_eq!(s.state(), &accepted);
    drop(s);
    fs::remove_file(ledger.join("replay-checkpoint.dpapi")).unwrap();
    let s = DurableWorkflow::open(&ledger, config(&p)).unwrap();
    assert_eq!(s.checkpoint_events(), 0);
    assert_eq!(s.state(), &accepted);
}

#[cfg(windows)]
#[test]
fn actual_runner_report_is_journaled_and_allows_acceptance_without_worker_self_report() {
    use cerulean_core::verifier::PinnedCheck;
    use std::sync::atomic::AtomicBool;
    let root = tempfile::tempdir().unwrap();
    let (mut s, mut p) = proposed(root.path());
    p.workflow.version = 2;
    p.execution[0].checks[0].program = "trusted-validator-fixture".into();
    p.snapshot_policy = Some(SnapshotPolicy {
        exclude: vec![],
        max_files: 100,
        max_bytes: 1024 * 1024,
    });
    p.execution[0].checks[0].args = vec![
        "--ignored".into(),
        "--exact".into(),
        "real_validator_fixture".into(),
        "--nocapture".into(),
    ];
    apply(
        &mut s,
        "chief",
        WorkflowCommand::Propose {
            plan: p.workflow.clone(),
        },
        CommandContext {
            execution: Some(ExecutionEvidence::Revision {
                revision: p.clone(),
            }),
            ..Default::default()
        },
    )
    .unwrap();
    approve(&mut s, &p);
    let a = assign(&mut s, &p);
    dispatch(&mut s, &p);
    apply(
        &mut s,
        "core",
        WorkflowCommand::Started {
            task: "task".into(),
            attempt: a.id.clone(),
        },
        ctx(receipt(&a, ReceiptStatus::Running), None),
    )
    .unwrap();
    let pins = [PinnedCheck::new(
        &p.execution[0].checks[0],
        &std::env::current_exe().unwrap(),
        &[],
    )
    .unwrap()];
    assert!(s
        .run_verification(
            "verify-before-submit",
            "task",
            "attempt",
            &pins,
            &AtomicBool::new(false)
        )
        .is_err());
    assert!(!root.path().join("project/validator-count").exists());
    fs::write(root.path().join("project/dependency.txt"), "old dependency").unwrap();
    submit(&mut s, &p, &a);
    let event = s
        .run_verification(
            "verify-actual",
            "task",
            "attempt",
            &pins,
            &AtomicBool::new(false),
        )
        .unwrap();
    assert!(s.state().tasks["task"].verified);
    assert!(matches!(
        event.body.context.execution,
        Some(ExecutionEvidence::Verification { .. })
    ));
    assert!(root
        .path()
        .join("ledger/verification/verify-actual/result.json")
        .exists());
    drop(s);
    let mut s = DurableWorkflow::open(&root.path().join("ledger"), config(&p)).unwrap();
    assert!(s
        .run_verification(
            "verify-actual",
            "task",
            "attempt",
            &pins,
            &AtomicBool::new(false)
        )
        .is_err());
    assert_eq!(
        fs::read_to_string(
            root.path()
                .join("ledger/verification/verify-actual/check-temp/validator-count")
        )
        .unwrap(),
        "one"
    );
    let accept = WorkflowCommand::Accept {
        task: "task".into(),
        attempt: a.id,
    };
    fs::write(root.path().join("project/dependency.txt"), "new dependency").unwrap();
    assert!(apply(&mut s, "chief", accept.clone(), CommandContext::default()).is_err());
    fs::write(root.path().join("project/dependency.txt"), "old dependency").unwrap();
    apply(&mut s, "chief", accept, CommandContext::default()).unwrap();
    assert_eq!(s.state().tasks["task"].state, TaskState::Accepted);
}

#[test]
#[ignore = "explicit real validator process fixture"]
fn real_validator_fixture() {
    assert_eq!(fs::read_to_string("result.txt").unwrap(), "abc");
    let temp = std::path::PathBuf::from(std::env::var_os("TEMP").unwrap());
    fs::write(temp.join("validator-started"), "ready").unwrap();
    let mode = fs::read_to_string("validator-mode").unwrap_or_default();
    let first = temp.file_name().unwrap() == "check-temp";
    if mode == "slow"
        || (mode == "detail-timeout" && first)
        || (mode == "detail-slow-second" && !first)
    {
        std::thread::sleep(std::time::Duration::from_secs(30));
    }
    fs::write(temp.join("validator-count"), "one").unwrap();
    println!("independent fixture validation passed");
    if (mode == "detail-first-fail" && first) || (mode == "detail-second-fail" && !first) {
        std::process::exit(7);
    }
}

#[cfg(windows)]
fn async_fixture(
    mode: &str,
) -> (
    tempfile::TempDir,
    RuntimeHost,
    PlanRevision,
    Vec<cerulean_core::verifier::PinnedCheck>,
) {
    async_fixture_in(tempfile::tempdir().unwrap(), mode)
}
#[cfg(windows)]
fn async_fixture_in(
    root: tempfile::TempDir,
    mode: &str,
) -> (
    tempfile::TempDir,
    RuntimeHost,
    PlanRevision,
    Vec<cerulean_core::verifier::PinnedCheck>,
) {
    let project = root.path().join("project");
    fs::create_dir(&project).unwrap();
    fs::write(project.join("result.txt"), "abc").unwrap();
    fs::write(project.join("validator-mode"), mode).unwrap();
    let data = root.path().join("data");
    let mut host = RuntimeHost::open(&data).unwrap();
    let mut p = plan(&project);
    let snap = host
        .handle(Request {
            id: "register".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::RegisterProject {
                project: p.project.clone(),
            },
        })
        .result
        .unwrap();
    p.project = snap.workflows[0].project.clone();
    p.snapshot_policy = Some(SnapshotPolicy {
        exclude: vec![],
        max_files: 100,
        max_bytes: 1024 * 1024,
    });
    p.execution[0].checks[0].args = vec![
        "--ignored".into(),
        "--exact".into(),
        "real_validator_fixture".into(),
        "--nocapture".into(),
    ];
    p.execution[0].checks[0].timeout_seconds = 30;
    if mode.starts_with("detail-") {
        p.execution[0].checks[0].name = Some("语法检查".into());
        p.execution[0].checks[0].timeout_seconds = if mode == "detail-timeout" { 1 } else { 5 };
        let mut second = p.execution[0].checks[0].clone();
        second.id = "second".into();
        second.name = Some("单元测试".into());
        second.timeout_seconds = 30;
        p.execution[0].checks.push(second);
    }
    host.propose("project", "chief", p.clone()).unwrap();
    host.handle(Request {
        id: "approve".into(),
        protocol_version: PROTOCOL_VERSION,
        command: Command::ApproveRevision {
            project_id: "project".into(),
            digest: p.approval_digest().unwrap(),
        },
    })
    .result
    .unwrap();
    drop(host);
    let ledger = data.join("workflows/project");
    let cfg = serde_json::from_slice(&fs::read(ledger.join("bindings.json")).unwrap()).unwrap();
    let mut store = DurableWorkflow::open(&ledger, cfg).unwrap();
    let attempt = assign(&mut store, &p);
    dispatch(&mut store, &p);
    apply(
        &mut store,
        "core",
        WorkflowCommand::Started {
            task: "task".into(),
            attempt: attempt.id.clone(),
        },
        ctx(receipt(&attempt, ReceiptStatus::Running), None),
    )
    .unwrap();
    submit(&mut store, &p, &attempt);
    drop(store);
    let pins = p.execution[0]
        .checks
        .iter()
        .map(|check| {
            cerulean_core::verifier::PinnedCheck::new(check, &std::env::current_exe().unwrap(), &[])
                .unwrap()
        })
        .collect();
    (root, RuntimeHost::open(&data).unwrap(), p, pins)
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "explicit native UI service fixture; isolated reports directory only"]
async fn native_async_service_fixture() {
    let outer = std::path::PathBuf::from(
        std::env::var_os("CERULEAN_NATIVE_ASYNC_DIR").expect("fixture directory"),
    )
    .canonicalize()
    .unwrap();
    let reports = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../reports")
        .canonicalize()
        .unwrap();
    assert!(outer.starts_with(reports));
    let (root, mut host, _, pins) = async_fixture_in(tempfile::tempdir_in(&outer).unwrap(), "slow");
    let data = root.path().join("data");
    if std::env::var_os("CERULEAN_NATIVE_RECONCILE").is_some() {
        drop(host);
        seed_unknown(root.path());
        host = RuntimeHost::open(&data).unwrap();
    } else {
        host.start_verification("project", "native-check", "task", "attempt", pins)
            .unwrap();
    }
    let dir = data.clone();
    let service = tokio::spawn(async move { cerulean_core::ipc::serve_host(&dir, host).await });
    let mut ready = false;
    for _ in 0..100 {
        if cerulean_core::ipc::Client::connect(&data).await.is_ok() {
            ready = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(ready);
    fs::write(
        outer.join("fixture-service.json"),
        serde_json::to_vec(
            &serde_json::json!({"dataDirectory":data,"ownerPid":std::process::id(),"modelCalls":0}),
        )
        .unwrap(),
    )
    .unwrap();
    service.await.unwrap().unwrap();
}
#[cfg(windows)]
#[test]
fn asynchronous_host_remains_responsive_and_cancellation_persists() {
    use std::time::{Duration, Instant};
    let (root, mut host, p, pins) = async_fixture("slow");
    host.start_verification("project", "async-check", "task", "attempt", pins.clone())
        .unwrap();
    let started = root
        .path()
        .join("data/workflows/project/verification/async-check/check-temp/validator-started");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !started.exists() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!host.can_shutdown());
    let before = Instant::now();
    let response = host.handle(Request {
        id: "abandon-live".into(),
        protocol_version: PROTOCOL_VERSION,
        command: Command::AbandonVerification {
            project_id: "project".into(),
            operation_id: "async-check".into(),
            digest: "0".repeat(64),
            reason: "must refuse running process".into(),
        },
    });
    assert!(response.error.is_some());
    for n in 0..10 {
        let response = host.handle(Request {
            id: format!("snapshot-{n}"),
            protocol_version: PROTOCOL_VERSION,
            command: Command::Snapshot,
        });
        assert_eq!(
            response.result.unwrap().verification_jobs[0].state,
            VerificationState::Running
        );
    }
    assert!(before.elapsed() < Duration::from_secs(1));
    assert!(host
        .start_verification("project", "other-id", "task", "attempt", pins)
        .is_err());
    let mut duplicate = p.project.clone();
    duplicate.id = "other-project".into();
    assert!(host
        .handle(Request {
            id: "overlap".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::RegisterProject { project: duplicate }
        })
        .error
        .is_some());
    assert!(host
        .handle(Request {
            id: "wrong-cancel".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::CancelVerification {
                project_id: "project".into(),
                operation_id: "wrong".into()
            }
        })
        .error
        .is_some());
    let cancelled = host
        .handle(Request {
            id: "cancel".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::CancelVerification {
                project_id: "project".into(),
                operation_id: "async-check".into(),
            },
        })
        .result
        .unwrap();
    assert_eq!(
        cancelled.verification_jobs[0].state,
        VerificationState::Cancelling
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        host.poll_verifications();
        if host.snapshot().verification_jobs[0].state == VerificationState::Failed {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(!host.snapshot().workflows[0].state.tasks["task"].verified);
    drop(host);
    let host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(
        host.snapshot().verification_jobs[0].state,
        VerificationState::Failed
    );
}
#[cfg(windows)]
#[test]
fn asynchronous_completion_recovers_from_journal_and_unknown_never_restarts() {
    use std::time::{Duration, Instant};
    let (root, mut host, _, pins) = async_fixture("success");
    host.start_verification("project", "async-check", "task", "attempt", pins)
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        host.poll_verifications();
        if host.snapshot().verification_jobs[0].state == VerificationState::Succeeded {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut job = host.snapshot().verification_jobs[0].clone();
    drop(host);
    job.state = VerificationState::Running;
    fs::write(
        root.path()
            .join("data/verification-jobs/project/async-check.json"),
        serde_json::to_vec(&job).unwrap(),
    )
    .unwrap();
    let host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(
        host.snapshot().verification_jobs[0].state,
        VerificationState::Succeeded
    );
    drop(host);
    let (unknown, host, _, pins) = async_fixture("success");
    drop(host);
    let dir = unknown.path().join("data/verification-jobs/project");
    fs::create_dir_all(&dir).unwrap();
    job.state = VerificationState::Running;
    fs::write(
        dir.join("async-check.json"),
        serde_json::to_vec(&job).unwrap(),
    )
    .unwrap();
    let mut host = RuntimeHost::open(&unknown.path().join("data")).unwrap();
    assert_eq!(
        host.snapshot().verification_jobs[0].state,
        VerificationState::Unknown
    );
    let error = host
        .start_verification("project", "different-id", "task", "attempt", pins)
        .unwrap_err();
    assert!(error.contains("对账"));
    assert!(!host.can_shutdown());
    assert!(!unknown
        .path()
        .join("data/workflows/project/verification")
        .exists());
    drop(host);
    job.state = VerificationState::Failed;
    fs::write(
        dir.join("async-check.json"),
        serde_json::to_vec(&job).unwrap(),
    )
    .unwrap();
    let unfinished = unknown
        .path()
        .join("data/workflows/project/verification/async-check");
    fs::create_dir_all(&unfinished).unwrap();
    fs::write(unfinished.join("result.json"), "{partial").unwrap();
    let host = RuntimeHost::open(&unknown.path().join("data")).unwrap();
    assert_eq!(
        host.snapshot().verification_jobs[0].state,
        VerificationState::Unknown
    );
}
#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn named_pipe_serves_queries_and_cancel_while_verifier_is_running() {
    use cerulean_core::ipc::{self, Client, UserOperation};
    use std::time::Duration;
    let (root, mut host, _, pins) = async_fixture("slow");
    let data = root.path().join("data");
    host.start_verification("project", "async-check", "task", "attempt", pins)
        .unwrap();
    let directory = data.clone();
    let server = tokio::spawn(async move { ipc::serve_host(&directory, host).await });
    let mut connected = false;
    for _ in 0..100 {
        if Client::connect(&data).await.is_ok() {
            connected = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(connected);
    let snapshot = tokio::time::timeout(Duration::from_secs(1), Client::snapshot(&data))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        snapshot.verification_jobs[0].state,
        VerificationState::Running
    );
    let response = Client::connect(&data)
        .await
        .unwrap()
        .call(UserOperation::Request {
            request: Request {
                id: "cancel".into(),
                protocol_version: PROTOCOL_VERSION,
                command: Command::CancelVerification {
                    project_id: "project".into(),
                    operation_id: "async-check".into(),
                },
            },
        })
        .await
        .unwrap();
    assert!(response.response.error.is_none());
    let mut finished = false;
    for _ in 0..150 {
        if Client::snapshot(&data).await.unwrap().verification_jobs[0].state
            == VerificationState::Failed
        {
            finished = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(finished);
    server.abort();
    let _ = server.await;
}

#[cfg(windows)]
fn seed_unknown(root: &Path) -> VerificationJob {
    let now = chrono::Utc::now().to_rfc3339();
    let job = VerificationJob {
        project_id: "project".into(),
        operation_id: "interrupted-check".into(),
        task_id: "task".into(),
        attempt_id: "attempt".into(),
        state: VerificationState::Running,
        started_at: now.clone(),
        updated_at: now,
        message: None,
        reconciliation_digest: None,
    };
    let folder = root.join("data/verification-jobs/project");
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("interrupted-check.json"),
        serde_json::to_vec(&job).unwrap(),
    )
    .unwrap();
    let dir = root.join("data/workflows/project/verification/interrupted-check");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("intent.json"),
        "{fixture interrupted before result}",
    )
    .unwrap();
    job
}
#[cfg(windows)]
fn abandon_request(hash: &str, reason: &str) -> Request {
    Request {
        id: "user-abandon".into(),
        protocol_version: PROTOCOL_VERSION,
        command: Command::AbandonVerification {
            project_id: "project".into(),
            operation_id: "interrupted-check".into(),
            digest: hash.into(),
            reason: reason.into(),
        },
    }
}
#[cfg(windows)]
#[test]
fn explicit_abandonment_preserves_evidence_rejects_stale_review_and_never_passes_task() {
    use std::time::{Duration, Instant};
    let (root, host, _, pins) = async_fixture("success");
    drop(host);
    seed_unknown(root.path());
    let data = root.path().join("data");
    let dir = data.join("workflows/project/verification/interrupted-check");
    let original = fs::read(dir.join("intent.json")).unwrap();
    let mut host = RuntimeHost::open(&data).unwrap();
    let before = host.snapshot();
    let hash = before.verification_jobs[0]
        .reconciliation_digest
        .clone()
        .unwrap();
    assert!(host.handle(abandon_request(&hash, "")).error.is_some());
    assert!(host
        .handle(abandon_request(&"0".repeat(64), "no result"))
        .error
        .is_some());
    fs::write(dir.join("result.json"), "{incomplete").unwrap();
    assert!(host
        .handle(abandon_request(&hash, "no result"))
        .error
        .is_some());
    assert!(!dir.join("abandonment.json").exists());
    let hash = host.snapshot().verification_jobs[0]
        .reconciliation_digest
        .clone()
        .unwrap();
    let response = host.handle(abandon_request(&hash, "检查被中断，保留证据并放弃本次结果"));
    assert!(response.error.is_none());
    let snapshot = response.result.unwrap();
    assert_eq!(
        snapshot.verification_jobs[0].state,
        VerificationState::Abandoned
    );
    assert_eq!(snapshot.workflows[0].sequence, before.workflows[0].sequence);
    assert!(!snapshot.workflows[0].state.tasks["task"].verified);
    assert_eq!(fs::read(dir.join("intent.json")).unwrap(), original);
    assert_eq!(fs::read(dir.join("result.json")).unwrap(), b"{incomplete");
    let audit = fs::read(dir.join("abandonment.json")).unwrap();
    assert!(host
        .handle(abandon_request(&hash, "repeat"))
        .error
        .is_none());
    assert_eq!(fs::read(dir.join("abandonment.json")).unwrap(), audit);
    assert_eq!(
        fs::read_dir(data.join("workflows/project/verification"))
            .unwrap()
            .count(),
        1
    );
    drop(host);
    let mut host = RuntimeHost::open(&data).unwrap();
    assert_eq!(
        host.snapshot().verification_jobs[0].state,
        VerificationState::Abandoned
    );
    assert!(host
        .start_verification(
            "project",
            "interrupted-check",
            "task",
            "attempt",
            pins.clone()
        )
        .is_err());
    host.start_verification("project", "explicit-new-check", "task", "attempt", pins)
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        host.poll_verifications();
        if host.snapshot().verification_jobs.iter().any(|j| {
            j.operation_id == "explicit-new-check" && j.state == VerificationState::Succeeded
        }) {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(fs::read(dir.join("intent.json")).unwrap(), original);
}
#[cfg(windows)]
#[test]
fn abandoned_evidence_tampering_returns_to_unknown_and_blocks_new_attempt() {
    let (root, host, _, pins) = async_fixture("success");
    drop(host);
    seed_unknown(root.path());
    let data = root.path().join("data");
    let mut host = RuntimeHost::open(&data).unwrap();
    let hash = host.snapshot().verification_jobs[0]
        .reconciliation_digest
        .clone()
        .unwrap();
    host.handle(abandon_request(&hash, "retire incomplete run"))
        .result
        .unwrap();
    drop(host);
    fs::write(
        data.join("workflows/project/verification/interrupted-check/intent.json"),
        "changed after retirement",
    )
    .unwrap();
    let mut host = RuntimeHost::open(&data).unwrap();
    assert_eq!(
        host.snapshot().verification_jobs[0].state,
        VerificationState::Unknown
    );
    assert!(host
        .start_verification("project", "new-id", "task", "attempt", pins)
        .is_err());
    assert!(!host.can_shutdown());
}
#[test]
fn host_keeps_foundation_approval_separate_and_rejects_old_revision_after_scope_change() {
    let data = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let mut host = RuntimeHost::open(data.path()).unwrap();
    let request = |command| Request {
        id: "user-request".into(),
        protocol_version: PROTOCOL_VERSION,
        command,
    };
    let snapshot = host
        .handle(request(Command::RegisterProject {
            project: plan(project.path()).project,
        }))
        .result
        .unwrap();
    let mut p = plan(project.path());
    p.project = snapshot.workflows[0].project.clone();
    p.snapshot_policy = Some(SnapshotPolicy {
        exclude: vec![".git".into(), "target".into()],
        max_files: 10000,
        max_bytes: 512 * 1024 * 1024,
    });
    assert!(host
        .propose("other-project", "chief-wrong", p.clone())
        .is_err());
    assert!(host.snapshot().workflows[0].revision.is_none());
    let mut unbound = p.clone();
    unbound.execution[0].checks[0].tool = None;
    assert!(host.propose("project", "chief-unbound", unbound).is_err());
    assert!(host.snapshot().workflows[0].revision.is_none());
    host.propose("project", "chief-1", p.clone()).unwrap();
    let hash = p.approval_digest().unwrap();
    assert!(host
        .handle(request(Command::ApproveRevision {
            project_id: "project".into(),
            digest: workflow_digest(&p.workflow).unwrap()
        }))
        .error
        .is_some());
    host.handle(request(Command::ApproveRevision {
        project_id: "project".into(),
        digest: hash.clone(),
    }))
    .result
    .unwrap();
    assert!(host.snapshot().workflows[0].state.approved);
    p.workflow.version = 2;
    p.policy.max_requests = 4;
    host.propose("project", "chief-2", p.clone()).unwrap();
    assert!(host.snapshot().workflows[0].approval.is_none());
    assert!(!host.snapshot().workflows[0].state.approved);
    assert!(host
        .handle(request(Command::ApproveRevision {
            project_id: "project".into(),
            digest: hash
        }))
        .error
        .is_some());
    host.handle(request(Command::ApproveRevision {
        project_id: "project".into(),
        digest: p.approval_digest().unwrap(),
    }))
    .result
    .unwrap();
    let expected = host.snapshot().workflows[0].sequence;
    drop(host);
    let host = RuntimeHost::open(data.path()).unwrap();
    assert!(host.snapshot().workflows[0].state.approved);
    assert_eq!(host.snapshot().workflows[0].sequence, expected);
}

#[test]
#[ignore = "explicit native UI fixture builder, not part of the product binary"]
fn seed_native_review_fixture() {
    let root = std::path::PathBuf::from(
        std::env::var_os("CERULEAN_NATIVE_FIXTURE_DIR").expect("fixture directory required"),
    );
    let reports = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../reports")
        .canonicalize()
        .unwrap();
    let root = root.canonicalize().unwrap();
    assert!(root.starts_with(reports));
    assert!(
        !root.join("data").exists(),
        "fixture must be a fresh directory"
    );
    fs::create_dir(root.join("project")).unwrap();
    fs::write(root.join("project/result.txt"), "abc").unwrap();
    let mut host = RuntimeHost::open(&root.join("data")).unwrap();
    let mut p = plan(&root.join("project"));
    let snapshot = host
        .handle(Request {
            id: "fixture-register".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::RegisterProject {
                project: p.project.clone(),
            },
        })
        .result
        .unwrap();
    p.project = snapshot.workflows[0].project.clone();
    p.snapshot_policy = Some(SnapshotPolicy {
        exclude: vec![".git".into(), "target".into()],
        max_files: 10000,
        max_bytes: 512 * 1024 * 1024,
    });
    let fixture_executable = root.join("codex.exe");
    fs::write(&fixture_executable, b"review fixture only; never execute").unwrap();
    p.execution[0].worker = Some(
        RuntimeHost::describe_codex_worker(&fixture_executable, "fixture-review-model").unwrap(),
    );
    host.propose("project", "fixture-chief", p.clone()).unwrap();
    fs::write(root.join("workflow-fixture.json"),serde_json::to_vec_pretty(&serde_json::json!({"projectId":p.project.id,"revisionDigest":p.approval_digest().unwrap(),"modelCalls":0,"scope":"synthetic chief proposal; real host ledger"})).unwrap()).unwrap();
}

#[cfg(windows)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn service_approves_full_revision_and_refuses_shutdown_with_assigned_task() {
    use cerulean_core::ipc::{Client, UserOperation};
    use std::{
        os::windows::process::CommandExt,
        process::{Child, Command as ProcessCommand, Stdio},
        time::Duration,
    };
    struct Server(Child);
    impl Drop for Server {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    fn start(path: &Path) -> Server {
        Server(
            ProcessCommand::new(env!("CARGO_BIN_EXE_cerulean-core"))
                .args(["--serve", "--data-dir"])
                .arg(path)
                .creation_flags(0x08000000)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        )
    }
    async fn connect(server: &mut Server, path: &Path) -> Client {
        for _ in 0..100 {
            assert!(server.0.try_wait().unwrap().is_none());
            if let Ok(c) = Client::connect(path).await {
                return c;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
        panic!("service unavailable");
    }
    let data = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let mut host = RuntimeHost::open(data.path()).unwrap();
    let snapshot = host
        .handle(Request {
            id: "register".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::RegisterProject {
                project: plan(project.path()).project,
            },
        })
        .result
        .unwrap();
    let mut p = plan(project.path());
    p.project = snapshot.workflows[0].project.clone();
    p.snapshot_policy = Some(SnapshotPolicy {
        exclude: vec![".git".into(), "target".into()],
        max_files: 10000,
        max_bytes: 512 * 1024 * 1024,
    });
    host.propose("project", "chief", p.clone()).unwrap();
    drop(host);
    let mut server = start(data.path());
    let client = connect(&mut server, data.path()).await;
    let response = client
        .call(UserOperation::Request {
            request: Request {
                id: "approve".into(),
                protocol_version: PROTOCOL_VERSION,
                command: Command::ApproveRevision {
                    project_id: "project".into(),
                    digest: p.approval_digest().unwrap(),
                },
            },
        })
        .await
        .unwrap()
        .response;
    let snapshot = response.result.unwrap();
    assert!(snapshot.workflows[0].state.approved);
    assert_eq!(
        snapshot.workflows[0].approval.as_ref().unwrap().user_id,
        "user"
    );
    let stop = Client::connect(data.path())
        .await
        .unwrap()
        .call(UserOperation::Shutdown {
            id: "stop".into(),
            protocol_version: PROTOCOL_VERSION,
        })
        .await
        .unwrap();
    assert!(stop.stopping);
    assert!(server.0.wait().unwrap().success());
    drop(server);
    let path = data.path().join("workflows/project");
    let saved: SchedulerConfig =
        serde_json::from_slice(&fs::read(path.join("bindings.json")).unwrap()).unwrap();
    let mut workflow = DurableWorkflow::open(&path, saved).unwrap();
    assign(&mut workflow, &p);
    drop(workflow);
    let mut server = start(data.path());
    let client = connect(&mut server, data.path()).await;
    let refused = client
        .call(UserOperation::Shutdown {
            id: "unsafe-stop".into(),
            protocol_version: PROTOCOL_VERSION,
        })
        .await
        .unwrap();
    assert!(!refused.stopping);
    assert_eq!(refused.response.error.unwrap().code, "unresolvedTasks");
    assert!(server.0.try_wait().unwrap().is_none());
    let snapshot = Client::snapshot(data.path()).await.unwrap();
    assert_eq!(
        snapshot.workflows[0].state.tasks["task"].state,
        TaskState::Assigned
    );
    // Drop terminates only this test's server; there is no external worker/effect.
}
