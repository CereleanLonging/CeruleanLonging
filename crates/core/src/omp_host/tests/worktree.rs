//! Actual Git, owned offline RPC peer and host file tools. No provider calls.
use super::*;
use crate::{managed_assignment::Update as Assignment, verifier::PinnedCheck};

fn temporary() -> tempfile::TempDir {
    let parent =
        fs::canonicalize(std::env::var_os("CERULEAN_OMP_TEST_PARENT").expect("clean test parent"))
            .unwrap();
    tempfile::Builder::new()
        .prefix("omp-worktree-")
        .tempdir_in(parent)
        .unwrap()
}
fn revise(root: &Path, plan: &mut PlanRevision) {
    let executable = std::env::var_os("CERULEAN_TEST_GIT").expect("explicit Git");
    let pin = PinnedCheck::describe(Path::new(&executable), &[], Default::default())
        .unwrap()
        .executable;
    let repository = PathBuf::from(&plan.project.root);
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
            .current_dir(&repository)
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
    fs::write(repository.join("base.txt"), "committed base\n").unwrap();
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
    plan.execution[0].checks[0].args = vec![
        "--exact".into(),
        "worktree_validation_fixture::omp_worktree_inputs".into(),
        "--ignored".into(),
    ];
    fs::write(repository.join("base.txt"), "uncommitted user edit\n").unwrap();
}
fn host(root: &Path, model: &str, key: bool) -> RuntimeHost {
    let exe = PathBuf::from(std::env::var_os("CERULEAN_OMP_FIXTURE_EXE").expect("offline peer"));
    assert_eq!(exe.file_name().unwrap(), "omp_offline_worker_fixture.exe");
    let mut host = fixture_with_revision(root, Some((&exe, model)), revise);
    if key {
        crate::credential_vault::save_for(
            &host.root,
            credentials::CredentialProvider::DeepSeek,
            &secret::SecretInput::new("cerulean-offline-placeholder-not-a-real-key".into())
                .unwrap(),
        )
        .unwrap();
        host.deepseek_key_configured = true;
    }
    super::super::super::omp_management_bridge::tests::decision(&mut host);
    host
}
fn workspace(host: &RuntimeHost) -> PathBuf {
    PathBuf::from(
        host.projects["project"]
            .revision()
            .unwrap()
            .execution_root("task", "attempt")
            .unwrap(),
    )
}
fn drain(host: &mut RuntimeHost) {
    let deadline = Instant::now() + Duration::from_secs(40);
    while !host.omp_active.is_empty() && Instant::now() < deadline {
        host.poll_verifications();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(host.omp_active.is_empty(), "{:?}", host.snapshot().omp_jobs);
}
fn prepare(host: &RuntimeHost) -> Assignment {
    match host.omp_active[&("project".into(), "attempt".into())]
        .receiver
        .recv_timeout(Duration::from_secs(40))
        .unwrap()
    {
        crate::async_omp::Update::Assignment(update) => update,
        _ => panic!("expected management prepare"),
    }
}

#[test]
#[ignore = "explicit Git and configuration-free parent; actual owned offline RPC peer"]
fn managed_worktree_read_write_verify_cancel_unknown_and_reopen() {
    for (model, expected) in [
        ("fixture-file-worktree", OmpJobState::Submitted),
        ("fixture-file-wait", OmpJobState::Cancelled),
        ("fixture-file-lost-terminal", OmpJobState::Unknown),
        ("fixture-wrong-model", OmpJobState::Failed),
    ] {
        let temp = temporary();
        let mut host = host(temp.path(), model, true);
        let workspace = workspace(&host);
        host.start_managed_omp("deputy-omp", "attempt").unwrap();
        assert!(!host.can_shutdown());
        assert!(host.require_available("project").is_err());
        let deadline = Instant::now() + Duration::from_secs(40);
        let mut cancelled = false;
        while !host.omp_active.is_empty() && Instant::now() < deadline {
            host.poll_verifications();
            if model == "fixture-file-wait" && !cancelled && workspace.join("result.txt").exists() {
                let response = control(
                    &mut host,
                    Command::CancelOmp {
                        project_id: "project".into(),
                        attempt_id: "attempt".into(),
                        reason: "cancel isolated fixture".into(),
                    },
                );
                assert!(response.error.is_none(), "{:?}", response.error);
                cancelled = true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(host.omp_active.is_empty());
        assert_eq!(
            host.snapshot().omp_jobs[0].state,
            expected,
            "{model}: {:?}",
            host.snapshot().omp_jobs
        );
        assert!(!temp.path().join("seed/project/result.txt").exists());
        assert_eq!(
            fs::read_to_string(temp.path().join("seed/project/base.txt")).unwrap(),
            "uncommitted user edit\n"
        );
        assert_eq!(
            fs::read_to_string(workspace.join("base.txt")).unwrap(),
            "committed base\n"
        );
        let store = host.projects.get_mut("project").unwrap();
        if model == "fixture-wrong-model" {
            assert!(store.attempt_record("task", "attempt").is_none());
            assert!(!workspace.join("result.txt").exists());
        } else {
            assert_eq!(
                fs::read_to_string(workspace.join("result.txt")).unwrap(),
                "fixture output"
            );
            let attempt = store.attempt_record("task", "attempt").unwrap();
            assert!(attempt.workspace_proof.is_some());
            assert_eq!(Path::new(&attempt.binding.workspace), workspace);
        }
        if model == "fixture-file-worktree" {
            let pins = store.revision().unwrap().execution[0]
                .checks
                .iter()
                .map(PinnedCheck::from_approved)
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            store
                .run_verification(
                    "isolated-omp-check",
                    "task",
                    "attempt",
                    &pins,
                    &std::sync::atomic::AtomicBool::new(false),
                )
                .unwrap();
            assert!(store.state().tasks["task"].verified);
            store
                .command(
                    "fixture-chief-accept",
                    "chief",
                    WorkflowCommand::Accept {
                        task: "task".into(),
                        attempt: "attempt".into(),
                    },
                    CommandContext::default(),
                    true,
                )
                .unwrap();
        }
        let count = store.history().len();
        drop(host);
        let mut reopened = RuntimeHost::open(&temp.path().join("data")).unwrap();
        reopened.poll_verifications();
        assert!(reopened.omp_active.is_empty());
        assert_eq!(reopened.projects["project"].history().len(), count);
        assert!(reopened.start_managed_omp("deputy-omp", "second").is_err());
        if model == "fixture-file-worktree" {
            assert_eq!(
                reopened.projects["project"].state().tasks["task"].state,
                TaskState::Accepted
            );
        }
    }
}

#[test]
#[ignore = "explicit real Git and offline RPC peer; missing key/ready must not dispatch"]
fn missing_credential_and_ready_preserve_unsent_attempt() {
    let temp = temporary();
    let mut host = host(temp.path(), "fixture-file-worktree", false);
    assert!(host.start_managed_omp("deputy-omp", "attempt").is_err());
    assert!(host.omp_startups.all().is_empty());
    assert!(!temp.path().join("pool/records").exists());
    crate::credential_vault::save_for(
        &host.root,
        credentials::CredentialProvider::DeepSeek,
        &secret::SecretInput::new("cerulean-offline-placeholder-not-a-real-key".into()).unwrap(),
    )
    .unwrap();
    let workspace = workspace(&host);
    host.start_managed_omp("deputy-omp", "attempt").unwrap();
    let Assignment::Prepare {
        operation,
        attempt,
        snapshot,
        reply,
    } = prepare(&host)
    else {
        panic!("expected prepare")
    };
    let ready = temp.path().join("pool/records").join(format!(
        "{}.ready.json",
        workspace_key("project", "task", "attempt").unwrap()
    ));
    fs::rename(&ready, temp.path().join("saved-ready.json")).unwrap();
    let result = host.prepare_managed_assignment("project", &operation, *attempt, *snapshot, "omp");
    assert!(result.is_err());
    reply.send(result).unwrap();
    drain(&mut host);
    assert!(!ready.exists());
    assert!(!workspace.join("result.txt").exists());
    assert!(host.management.application("deputy-omp").is_none());
    assert!(host.projects["project"]
        .attempt_record("task", "attempt")
        .is_none());
    drop(host);
    let mut reopened = RuntimeHost::open(&temp.path().join("data")).unwrap();
    reopened.poll_verifications();
    assert!(reopened.omp_active.is_empty());
    assert!(!ready.exists());
}

#[test]
#[ignore = "explicit real Git and offline RPC peer; lost management ACK, no resend"]
fn prepared_assignment_recovery_keeps_worktree_without_resend() {
    let temp = temporary();
    let mut host = host(temp.path(), "fixture-file-worktree", true);
    let workspace = workspace(&host);
    host.start_managed_omp("deputy-omp", "attempt").unwrap();
    let Assignment::Prepare {
        operation,
        attempt,
        snapshot,
        reply,
    } = prepare(&host)
    else {
        panic!("expected prepare")
    };
    host.prepare_managed_assignment("project", &operation, *attempt, *snapshot, "omp")
        .unwrap();
    reply.send(Err("simulated lost ACK".into())).unwrap();
    drain(&mut host);
    drop(host);
    let mut reopened = RuntimeHost::open(&temp.path().join("data")).unwrap();
    reopened.application_recovery_review("deputy-omp").unwrap();
    assert!(reopened.omp_active.is_empty());
    assert!(!workspace.join("result.txt").exists());
    assert_eq!(
        fs::read_to_string(workspace.join("base.txt")).unwrap(),
        "committed base\n"
    );
    assert!(reopened
        .start_managed_omp("deputy-omp", "new-attempt")
        .is_err());
}

#[test]
#[ignore = "explicit Git and clean parent; deterministic background preparation pause"]
fn preparation_cancel_keeps_reservation_and_never_launches_peer() {
    let temp = temporary();
    let mut host = host(temp.path(), "fixture-file-worktree", true);
    let (entered, started) = std::sync::mpsc::channel();
    let (_release, wait) = std::sync::mpsc::channel();
    host.omp_prepare_gate = Some(crate::async_omp::PreparationGate {
        entered,
        release: wait,
    });
    let began = Instant::now();
    host.start_managed_omp("deputy-omp", "attempt").unwrap();
    assert!(began.elapsed() < Duration::from_secs(5));
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(host.snapshot().omp_jobs[0].state, OmpJobState::Starting);
    assert!(host.omp_startups.get("project", "attempt").is_some());
    assert!(host.require_available("project").is_err());
    let response = control(
        &mut host,
        Command::CancelOmp {
            project_id: "project".into(),
            attempt_id: "attempt".into(),
            reason: "stop preparation".into(),
        },
    );
    assert!(response.error.is_none(), "{:?}", response.error);
    drain(&mut host);
    assert!(!temp.path().join("pool/records").exists());
    assert!(!host.root.join("omp-profiles").exists());
    assert!(host.projects["project"]
        .attempt_record("task", "attempt")
        .is_none());
    drop(host);
    let mut reopened = RuntimeHost::open(&temp.path().join("data")).unwrap();
    assert!(reopened.start_managed_omp("deputy-omp", "attempt").is_err());
    let review = reopened.snapshot().omp_jobs[0]
        .closure_digest
        .clone()
        .unwrap();
    let response = control(
        &mut reopened,
        Command::AbandonOmpStartup {
            project_id: "project".into(),
            attempt_id: "attempt".into(),
            evidence_digest: review,
            reason: "close unsent preparation".into(),
        },
    );
    assert!(response.error.is_none(), "{:?}", response.error);
    drop(reopened);
    let reopened = RuntimeHost::open(&temp.path().join("data")).unwrap();
    assert_eq!(
        reopened.snapshot().omp_jobs[0].state,
        OmpJobState::Abandoned
    );
    assert!(!temp.path().join("pool/records").exists());
}
