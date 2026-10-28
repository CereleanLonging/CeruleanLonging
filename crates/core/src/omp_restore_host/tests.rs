use super::*;
use crate::omp_file_tools::tests::{dispatch_event, request};
use cerulean_adapters::omp_transport::worker::{host_calls::HostService, WorkerEvent};
async fn seed(root: &Path) -> RuntimeHost {
    seed_fixture(root, false).await
}
async fn seed_fixture(root: &Path, settled_write_setup: bool) -> RuntimeHost {
    fs::create_dir(root.join("seed")).unwrap();
    let (original, mut plan) = crate::omp_execution::tests::assigned(&root.join("seed"), false);
    drop(original);
    let mut host = RuntimeHost::open(&root.join("data")).unwrap();
    host.register(plan.project.clone()).unwrap();
    plan.project = host.projects["project"]
        .configuration()
        .execution_scope
        .as_ref()
        .unwrap()
        .project
        .clone();
    host.propose("project", "fixture-plan", plan.clone())
        .unwrap();
    host.approve("project", &plan.approval_digest().unwrap())
        .unwrap();
    let store = host.projects.get_mut("project").unwrap();
    let attempt = Attempt {
        workspace_proof: None,
        id: "attempt".into(),
        task_id: "task".into(),
        worker_id: "worker-omp".into(),
        revision_digest: plan.approval_digest().unwrap(),
        fence: 1,
        binding: SessionBinding {
            adapter_id: "omp".into(),
            adapter_version: "18.4.4".into(),
            session_id: "session".into(),
            identity_strength: IdentityStrength::ControllerRequest(ControllerRequestIdentity {
                connection_id: "connection".into(),
                request_id: "prompt-request".into(),
            }),
            workspace: plan.project.root.clone(),
        },
    };
    store
        .command(
            "assign",
            "deputy",
            WorkflowCommand::Assign {
                task: "task".into(),
                worker: "worker-omp".into(),
                attempt: "attempt".into(),
            },
            CommandContext {
                execution: Some(ExecutionEvidence::Attempt { attempt }),
                ..Default::default()
            },
            false,
        )
        .unwrap();
    let (_, _, intent, _) = crate::omp_execution::scope(store, "task", "attempt").unwrap();
    store
        .command(
            "dispatch",
            "core",
            WorkflowCommand::DispatchIntent {
                task: "task".into(),
                attempt: "attempt".into(),
            },
            CommandContext {
                thread_id: Some("session".into()),
                cwd: Some(plan.project.root.clone()),
                ..Default::default()
            },
            false,
        )
        .unwrap();
    let mut prompt =
        crate::omp_journal::OmpJournal::open(&store.omp_attempt_path("attempt").unwrap(), &intent)
            .unwrap();
    prompt
        .record(&WorkerEvent::DispatchIntent {
            intent: intent.clone(),
        })
        .unwrap();
    drop(prompt);
    if settled_write_setup {
        // This controlled fault fixture starts only after setup publications
        // finish. The original concurrent fixture and production stay unchanged.
        store
            .wait_for_projections(std::time::Duration::from_secs(30))
            .unwrap();
    }
    let mut files = crate::omp_file_tools::OmpFileTools::create(
        &plan,
        store.approval().unwrap(),
        "user",
        1,
        &intent,
        &store.omp_file_service_path("attempt").unwrap(),
    )
    .unwrap();
    fs::write(
        Path::new(&plan.project.root).join("result.txt"),
        "Original reviewed source\n",
    )
    .unwrap();
    let q = request(
        "write-1",
        "write_file",
        serde_json::json!({"path":"result.txt","content":"Interrupted task version\n","expectedSha256":format!("{:x}",sha2::Sha256::digest(b"Original reviewed source\n"))}),
    );
    files.record(&intent, &dispatch_event(&q)).unwrap();
    let (_tx, rx) = tokio::sync::watch::channel(false);
    assert!(!files.start(q, rx).await.unwrap().is_error);
    drop(files);
    store
        .command(
            "unknown",
            "core",
            WorkflowCommand::RecoveryUnknown {
                task: "task".into(),
                attempt: "attempt".into(),
            },
            CommandContext::default(),
            false,
        )
        .unwrap();
    host
}
fn read_command() -> Command {
    Command::ReadOmpFiles {
        project_id: "project".into(),
        task_id: "task".into(),
        attempt_id: "attempt".into(),
        offset: 0,
        call_id: Some("write-1".into()),
    }
}
fn send(host: &mut RuntimeHost, command: Command) -> Response {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let kind = match &command {
        Command::ReadOmpFiles { .. } => "ReadOmpFiles",
        Command::RestoreOmpFile { .. } => "RestoreOmpFile",
        _ => "Other",
    };
    eprintln!("omp pin 328 command entry: sequence={sequence} kind={kind}");
    let response = host.handle(Request {
        id: "test".into(),
        protocol_version: PROTOCOL_VERSION,
        command,
    });
    eprintln!(
        "omp pin 328 command exit: sequence={sequence} kind={kind} error={:?} resultPresent={}",
        response.error,
        response.result.is_some()
    );
    response
}
fn restore_command(digest: &str) -> Command {
    Command::RestoreOmpFile {
        project_id: "project".into(),
        task_id: "task".into(),
        attempt_id: "attempt".into(),
        call_id: "write-1".into(),
        evidence_digest: digest.into(),
    }
}
#[tokio::test]
async fn user_commands_bind_preview_reject_stale_digest_restore_and_reopen_without_replay() {
    // Same success cleanup as before; retain this exact fixture on panic.
    let retained = OmpRestore328RetainOnFailure(Some(tempfile::tempdir().unwrap()));
    let root = retained.0.as_ref().unwrap();
    let mut host = seed(root.path()).await;
    let history = host.projects["project"].history().len();
    let response = send(&mut host, read_command());
    assert!(
        response.error.is_none(),
        "initial read: {:?}",
        response.error
    );
    let view = response.result.unwrap().omp_files.unwrap();
    let detail = view.detail.unwrap();
    assert_eq!(
        detail.original.text.as_deref(),
        Some("Original reviewed source\n")
    );
    assert_eq!(
        detail.current.text.as_deref(),
        Some("Interrupted task version\n")
    );
    assert_eq!(host.projects["project"].history().len(), history);
    assert!(host.snapshot().omp_files.is_none());
    let path =
        Path::new(&host.projects["project"].revision().unwrap().project.root).join("result.txt");
    fs::write(&path, "user changed it").unwrap();
    let stale = send(
        &mut host,
        restore_command(detail.evidence_digest.as_ref().unwrap()),
    );
    assert_eq!(
        stale
            .error
            .as_ref()
            .map(|e| (e.code.as_str(), e.message.as_str())),
        Some(("queryRejected", "文件或复核证据已变化，请重新读取并确认")),
        "stale review must reject for the expected reason: {:?}",
        stale.error
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "user changed it");
    let response = send(&mut host, read_command());
    assert!(
        response.error.is_none(),
        "second read: {:?}",
        response.error
    );
    let detail = response.result.unwrap().omp_files.unwrap().detail.unwrap();
    let response = send(
        &mut host,
        restore_command(detail.evidence_digest.as_ref().unwrap()),
    );
    assert!(response.error.is_none(), "{:?}", response.error);
    let snapshot = response.result.unwrap();
    let outcome = snapshot.omp_restore.unwrap();
    assert_eq!(
        outcome.review_digest,
        *detail.evidence_digest.as_ref().unwrap()
    );
    assert_eq!(outcome.restored_sha256, detail.original_sha256);
    if let Some(view) = snapshot.omp_files {
        assert!(outcome.readback_error.is_none());
        assert_eq!(
            view.detail.unwrap().restore_status.as_deref(),
            Some("restored")
        );
    } else {
        assert!(outcome.readback_error.is_some());
    }
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "Original reviewed source\n"
    );
    assert_eq!(host.projects["project"].history().len(), history);
    drop(host);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    let response = send(&mut host, read_command());
    assert!(
        response.error.is_none(),
        "reopened read: {:?}",
        response.error
    );
    let detail = response.result.unwrap().omp_files.unwrap().detail.unwrap();
    assert!(detail.evidence_digest.is_none());
    assert_eq!(detail.restore_status.as_deref(), Some("restored"));
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Unknown
    );
}
#[tokio::test]
async fn completed_restore_survives_failed_readback_without_replay() {
    let retained = OmpRestore328RetainOnFailure(Some(tempfile::tempdir().unwrap()));
    let root = retained.0.as_ref().unwrap();
    let host = seed_fixture(root.path(), true).await;
    // Isolate the injected result-handling fault from unrelated asynchronous
    // fixture setup. Production restore does not gain a projection barrier.
    host.projects["project"]
        .wait_for_projections(std::time::Duration::from_secs(30))
        .unwrap();
    let read = host.omp_restore_request(&read_command()).unwrap().unwrap();
    let detail = read.omp_files.unwrap().detail.unwrap();
    let digest = detail.evidence_digest.as_ref().unwrap();
    let command = restore_command(digest);
    let history = host.projects["project"].history().len();
    let path =
        Path::new(&host.projects["project"].revision().unwrap().project.root).join("result.txt");
    let calls = std::cell::Cell::new(0);
    let snapshot = host
        .omp_restore_request_with_readback(&command, |_, _, _, _, _| {
            calls.set(calls.get() + 1);
            assert_eq!(
                fs::read_to_string(&path).unwrap(),
                "Original reviewed source\n"
            );
            Err("injected post-restore preview failure".into())
        })
        .unwrap()
        .unwrap();
    assert_eq!(calls.get(), 1);
    assert!(snapshot.omp_files.is_none());
    let outcome = snapshot.omp_restore.unwrap();
    assert_eq!(outcome.project_id, "project");
    assert_eq!(outcome.task_id, "task");
    assert_eq!(outcome.attempt_id, "attempt");
    assert_eq!(outcome.call_id, "write-1");
    assert_eq!(outcome.review_digest, *digest);
    assert_eq!(outcome.restored_sha256, detail.original_sha256);
    assert_eq!(
        outcome.readback_error.as_deref(),
        Some("injected post-restore preview failure")
    );
    assert!(!outcome.at.is_empty());
    assert_eq!(host.projects["project"].history().len(), history);
    assert!(host.snapshot().omp_restore.is_none());
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Unknown
    );
    assert!(host
        .omp_restore_request_with_readback(&command, |_, _, _, _, _| {
            panic!("rejected restore must not reach post-restore readback")
        })
        .unwrap()
        .is_err());
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "Original reviewed source\n"
    );
    drop(host);
    let reopened = RuntimeHost::open(&root.path().join("data")).unwrap();
    let view = reopened
        .omp_restore_request(&read_command())
        .unwrap()
        .unwrap();
    let detail = view.omp_files.unwrap().detail.unwrap();
    assert_eq!(detail.restore_status.as_deref(), Some("restored"));
    assert!(detail.evidence_digest.is_none());
    assert!(view.omp_restore.is_none());
    assert_eq!(reopened.projects["project"].history().len(), history);
}

#[tokio::test]
async fn rejected_restore_never_emits_completion_or_reads_preview() {
    let root = tempfile::tempdir().unwrap();
    let host = seed(root.path()).await;
    host.projects["project"]
        .wait_for_projections(std::time::Duration::from_secs(30))
        .unwrap();
    let path =
        Path::new(&host.projects["project"].revision().unwrap().project.root).join("result.txt");
    let result = host
        .omp_restore_request_with_readback(&restore_command(&"0".repeat(64)), |_, _, _, _, _| {
            panic!("stale review must not reach post-restore readback")
        })
        .unwrap();
    assert_eq!(
        result.unwrap_err(),
        "文件或复核证据已变化，请重新读取并确认"
    );
    assert_eq!(
        fs::read_to_string(path).unwrap(),
        "Interrupted task version\n"
    );
    assert!(host.snapshot().omp_restore.is_none());
}

#[tokio::test]
async fn foreign_scope_untrusted_paths_and_unreviewed_mutation_are_rejected() {
    let root = tempfile::tempdir().unwrap();
    let mut host = seed(root.path()).await;
    assert!(send(&mut host, restore_command(&"0".repeat(64)))
        .error
        .is_some());
    for id in ["../project", "missing"] {
        let command = Command::ReadOmpFiles {
            project_id: id.into(),
            task_id: "task".into(),
            attempt_id: "attempt".into(),
            offset: 0,
            call_id: Some("write-1".into()),
        };
        assert!(send(&mut host, command).error.is_some());
    }
    let command = Command::ReadOmpFiles {
        project_id: "project".into(),
        task_id: "task".into(),
        attempt_id: "attempt".into(),
        offset: 0,
        call_id: Some("../../outside".into()),
    };
    assert!(send(&mut host, command).error.is_some());
    let base = Store::open(&root.path().join("base-only")).unwrap();
    drop(base);
    assert!(Store::open(&root.path().join("base-only"))
        .unwrap()
        .execute(read_command())
        .is_err());
}
#[test]
#[ignore = "isolated native OMP file restore fixture; no provider requests"]
fn seed_native() {
    let root =
        PathBuf::from(std::env::var_os("CERULEAN_OMP_NATIVE_ROOT").expect("explicit fixture root"));
    assert!(root.is_dir() && fs::read_dir(&root).unwrap().next().is_none());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    drop(runtime.block_on(seed(&root)));
}

// This follows the existing Gateway324RetainOnFailure guard. It neither retries
// restoration nor changes a test assertion, timeout, scope or Windows share flag.
struct OmpRestore328RetainOnFailure(Option<tempfile::TempDir>);
impl Drop for OmpRestore328RetainOnFailure {
    fn drop(&mut self) {
        if std::thread::panicking() {
            if let Some(temp) = self.0.take() {
                eprintln!(
                    "omp restore 328 failed fixture retained at {:?}",
                    temp.keep()
                );
            }
        }
    }
}
