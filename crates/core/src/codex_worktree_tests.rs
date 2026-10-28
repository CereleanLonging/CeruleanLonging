//! Real Git and resident CodexClient with an explicitly offline protocol peer.
use super::*;
use crate::{async_codex::Update, verifier::PinnedCheck};
use std::time::{Duration, Instant};

fn temporary() -> tempfile::TempDir {
    let parent =
        fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports")).unwrap();
    tempfile::Builder::new()
        .prefix("codex-worktree-")
        .tempdir_in(parent)
        .unwrap()
}
fn revise(root: &Path, plan: &mut PlanRevision) {
    let pin = PinnedCheck::describe(
        Path::new(&std::env::var_os("CERULEAN_TEST_GIT").expect("explicit Git")),
        &[],
        Default::default(),
    )
    .unwrap()
    .executable;
    let repo = PathBuf::from(&plan.project.root);
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
            .current_dir(&repo)
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
    fs::write(repo.join("base.txt"), "committed base\n").unwrap();
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
    fs::write(repo.join("base.txt"), "uncommitted user edit\n").unwrap();
}
fn decision(host: &mut RuntimeHost) {
    use cerulean_domain::management::{DecisionEnvelope, ManagementDecision, Purpose};
    let context = host
        .management_context("project", "Execute approved programming task")
        .unwrap();
    let state_digest = digest(&context).unwrap();
    host.management
        .begin(crate::management_execution::DecisionIntent {
            operation_id: "deputy-worktree".into(),
            project_id: "project".into(),
            purpose: Purpose::Assign,
            model: "fixture-deputy".into(),
            state_digest: state_digest.clone(),
            context,
            created_at: chrono::Utc::now().to_rfc3339(),
        })
        .unwrap();
    host.management
        .finish(
            "deputy-worktree",
            Ok(cerulean_adapters::openai::DecisionObservation {
                response_id: "resp_fixture".into(),
                requested_model: "fixture-deputy".into(),
                actual_model: "fixture-deputy".into(),
                input_tokens: 1,
                output_tokens: 1,
                envelope: DecisionEnvelope {
                    state_digest,
                    decision: ManagementDecision::Assign {
                        task_id: "task".into(),
                        worker_id: "worker-codex".into(),
                        rationale: "Approved isolated task".into(),
                    },
                },
            }),
        )
        .unwrap();
}
fn fixture(root: &Path, mode: &str) -> (RuntimeHost, PathBuf) {
    let exe = PathBuf::from(
        std::env::var_os("CERULEAN_CODEX_FIXTURE_EXE").expect("offline peer executable"),
    );
    assert_eq!(exe.file_name().unwrap(), "codex_protocol_fixture.exe");
    let (mut host, mut draft) = project_planning::tests::fixture(root);
    fs::copy(exe, root.join("codex.exe")).unwrap();
    draft.check_args = vec![
        "--exact".into(),
        "worktree_validation_fixture::codex_worktree_inputs".into(),
        "--ignored".into(),
    ];
    host.configure_project(draft).unwrap();
    project_planning::tests::complete(&mut host, project_planning::tests::proposal("tests"));
    let mut plan = host.projects["project"].revision().unwrap().clone();
    revise(root, &mut plan);
    plan.workflow.version += 1;
    host.propose("project", "isolate-workspace", plan.clone())
        .unwrap();
    host.approve("project", &plan.approval_digest().unwrap())
        .unwrap();
    let workspace = PathBuf::from(plan.execution_root("task", "attempt").unwrap());
    fs::write(
        root.join("fixture-config.json"),
        serde_json::to_vec(&serde_json::json!({"version":1,"mode":mode,"workspace":workspace}))
            .unwrap(),
    )
    .unwrap();
    decision(&mut host);
    (host, workspace)
}
fn drain(host: &mut RuntimeHost) {
    let deadline = Instant::now() + Duration::from_secs(50);
    while !host.codex_active.is_empty() && Instant::now() < deadline {
        host.poll_verifications();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        host.codex_active.is_empty(),
        "{:?}",
        host.snapshot().codex_jobs
    );
}
fn count(root: &Path, method: &str) -> usize {
    let content = fs::read_to_string(root.join("protocol-log.jsonl")).unwrap_or_default();
    content
        .lines()
        .filter(|line| serde_json::from_str::<serde_json::Value>(line).unwrap()["method"] == method)
        .count()
}
fn cancel(host: &mut RuntimeHost) {
    let review = host.codex_cancellations()[0].clone();
    host.cancel_codex(
        "project",
        "attempt",
        &review.evidence_digest,
        "stop isolated task",
    )
    .unwrap();
}

#[test]
#[ignore = "explicit Git and offline executable; actual resident process and independent validator"]
fn committed_input_dispatch_validation_cancel_and_readonly_recovery() {
    for mode in [
        "worktree-completed",
        "worktree-wait",
        "worktree-lost-terminal",
    ] {
        let temp = temporary();
        let (mut host, workspace) = fixture(temp.path(), mode);
        host.start_managed_codex("deputy-worktree", "attempt")
            .unwrap();
        assert_eq!(
            host.startup_detail("project", "attempt").unwrap().workspace,
            workspace.to_string_lossy()
        );
        if mode == "worktree-wait" {
            let deadline = Instant::now() + Duration::from_secs(40);
            loop {
                host.poll_verifications();
                if host.snapshot().workflows[0].state.tasks["task"].state == TaskState::Running {
                    break;
                }
                assert!(Instant::now() < deadline && !host.codex_active.is_empty());
                std::thread::sleep(Duration::from_millis(5));
            }
            // Keep a deliberately late activity projection while Started is
            // already visible from the shared owner. The public cancel review
            // must still bind the actual running attempt, without another tick.
            let key = ("project".into(), "attempt".into());
            let stale = &mut host.codex_active.get_mut(&key).unwrap().snapshot;
            stale.sequence = 2;
            let task = stale.state.tasks.get_mut("task").unwrap();
            task.state = TaskState::Pending;
            task.attempt = None;
            assert_eq!(host.codex_cancellations().len(), 1);
            cancel(&mut host);
        }
        drain(&mut host);
        let expected = match mode {
            "worktree-wait" => CodexJobState::Cancelled,
            "worktree-lost-terminal" => CodexJobState::Unknown,
            _ => CodexJobState::Submitted,
        };
        assert_eq!(
            host.snapshot().codex_jobs[0].state,
            expected,
            "{:?}",
            host.snapshot().codex_jobs
        );
        assert_eq!(count(temp.path(), "turn/start"), 1);
        assert_eq!(count(temp.path(), "thread/start"), 1);
        assert!(workspace.join("sum.mjs").exists());
        assert!(!temp.path().join("project/sum.mjs").exists());
        assert_eq!(
            fs::read_to_string(temp.path().join("project/base.txt")).unwrap(),
            "uncommitted user edit\n"
        );
        assert!(host.projects["project"]
            .attempt_record("task", "attempt")
            .unwrap()
            .workspace_proof
            .is_some());
        drop(host);
        let mut host = RuntimeHost::open(&temp.path().join("data")).unwrap();
        assert_eq!(host.snapshot().codex_jobs[0].state, expected);
        if mode == "worktree-lost-terminal" {
            let ready = temp.path().join("pool/records").join(format!(
                "{}.ready.json",
                workspace_key("project", "task", "attempt").unwrap()
            ));
            fs::rename(&ready, temp.path().join("saved-ready.json")).unwrap();
            host.reconcile_codex("project", "attempt").unwrap();
            drain(&mut host);
            assert_eq!(host.snapshot().codex_jobs[0].state, CodexJobState::Unknown);
            assert!(!ready.exists());
            assert_eq!(count(temp.path(), "turn/start"), 1);
            assert_eq!(count(temp.path(), "thread/start"), 1);
            // Only the test restores its untouched original fixture record. The
            // production readback must not create ready or accept unverified files.
            fs::rename(temp.path().join("saved-ready.json"), &ready).unwrap();
            host.reconcile_codex("project", "attempt").unwrap();
            drain(&mut host);
            assert_eq!(
                host.snapshot().codex_jobs[0].state,
                CodexJobState::Submitted,
                "{:?}",
                host.snapshot().codex_jobs
            );
            assert!(ready.exists());
            assert_eq!(count(temp.path(), "turn/start"), 1);
            assert_eq!(count(temp.path(), "thread/start"), 1);
        }
        if mode == "worktree-completed" {
            let store = host.projects.get_mut("project").unwrap();
            let pins = store.revision().unwrap().execution[0]
                .checks
                .iter()
                .map(PinnedCheck::from_approved)
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            store
                .run_verification(
                    "codex-worktree-check",
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
            drop(host);
            host = RuntimeHost::open(&temp.path().join("data")).unwrap();
            assert_eq!(
                host.projects["project"].state().tasks["task"].state,
                TaskState::Accepted
            );
        }
        assert!(host
            .start_managed_codex("deputy-worktree", "other")
            .is_err());
    }
}

#[test]
#[ignore = "explicit Git and offline peer; ready/management confirmation failures must not send"]
fn missing_ready_and_lost_ack_preserve_original_application_without_resend() {
    for missing in [true, false] {
        let temp = temporary();
        let (mut host, workspace) = fixture(temp.path(), "worktree-completed");
        host.start_managed_codex("deputy-worktree", "attempt")
            .unwrap();
        let Update::PrepareAssignment {
            operation,
            attempt,
            snapshot,
            reply,
        } = host.codex_active[&("project".into(), "attempt".into())]
            .receiver
            .recv_timeout(Duration::from_secs(40))
            .unwrap()
        else {
            panic!("expected prepare")
        };
        let ready = temp.path().join("pool/records").join(format!(
            "{}.ready.json",
            workspace_key("project", "task", "attempt").unwrap()
        ));
        if missing {
            fs::rename(&ready, temp.path().join("saved-ready.json")).unwrap();
        }
        let prepared =
            host.prepare_managed_codex_assignment("project", &operation, *attempt, *snapshot);
        assert_eq!(prepared.is_err(), missing);
        reply
            .send(Err("fixture drops preparation confirmation".into()))
            .unwrap();
        drain(&mut host);
        assert!(!workspace.join("sum.mjs").exists());
        assert_eq!(count(temp.path(), "turn/start"), 0);
        drop(host);
        let mut host = RuntimeHost::open(&temp.path().join("data")).unwrap();
        assert_eq!(
            host.startup_detail("project", "attempt").unwrap().workspace,
            workspace.to_string_lossy()
        );
        if !missing {
            host.application_recovery_review("deputy-worktree").unwrap();
        } else {
            assert!(!ready.exists());
        }
        assert!(host
            .start_managed_codex("deputy-worktree", "second")
            .is_err());
        assert_eq!(count(temp.path(), "turn/start"), 0);
    }
}

#[test]
#[ignore = "explicit Git; paused background preparation and durable startup cancellation"]
fn preparation_stop_is_responsive_and_reopens_without_process_or_rebuild() {
    let temp = temporary();
    let (mut host, workspace) = fixture(temp.path(), "worktree-completed");
    let (entered, started) = std::sync::mpsc::channel();
    let (_release, wait) = std::sync::mpsc::channel();
    host.codex_prepare_gate = Some(crate::async_codex::PreparationGate {
        entered,
        release: wait,
    });
    let began = Instant::now();
    host.start_managed_codex("deputy-worktree", "attempt")
        .unwrap();
    assert!(began.elapsed() < Duration::from_secs(5));
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(host.require_available("project").is_err());
    assert_eq!(
        host.startup_detail("project", "attempt").unwrap().workspace,
        workspace.to_string_lossy()
    );
    cancel(&mut host);
    drain(&mut host);
    assert!(!temp.path().join("pool/records").exists());
    assert_eq!(count(temp.path(), "initialize"), 0);
    drop(host);
    let mut host = RuntimeHost::open(&temp.path().join("data")).unwrap();
    let detail = host.startup_detail("project", "attempt").unwrap();
    host.abandon_codex_startup(
        "project",
        "attempt",
        &detail.evidence_digest.unwrap(),
        "close unsent preparation",
    )
    .unwrap();
    drop(host);
    let mut host = RuntimeHost::open(&temp.path().join("data")).unwrap();
    assert!(host
        .start_managed_codex("deputy-worktree", "attempt")
        .is_err());
    assert!(!workspace.exists());
    assert_eq!(count(temp.path(), "initialize"), 0);
}

#[test]
#[ignore = "explicit Git and offline peer; cancellation after directory readiness before assignment ACK"]
fn late_preparation_cancel_cannot_reach_turn_start() {
    let temp = temporary();
    let (mut host, workspace) = fixture(temp.path(), "worktree-completed");
    host.start_managed_codex("deputy-worktree", "attempt")
        .unwrap();
    let Update::PrepareAssignment {
        operation,
        attempt,
        snapshot,
        reply,
    } = host.codex_active[&("project".into(), "attempt".into())]
        .receiver
        .recv_timeout(Duration::from_secs(40))
        .unwrap()
    else {
        panic!("expected prepare")
    };
    assert!(workspace.join("base.txt").exists());
    cancel(&mut host);
    let result = host.prepare_managed_codex_assignment("project", &operation, *attempt, *snapshot);
    assert!(result.is_ok(), "{result:?}");
    reply.send(result).unwrap();
    drain(&mut host);
    assert_eq!(count(temp.path(), "turn/start"), 0);
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Assigned
    );
    drop(host);
    let mut host = RuntimeHost::open(&temp.path().join("data")).unwrap();
    assert!(!host.unsent_assignments().is_empty());
    assert!(host
        .start_managed_codex("deputy-worktree", "second")
        .is_err());
    assert_eq!(count(temp.path(), "turn/start"), 0);
    assert!(!workspace.join("sum.mjs").exists());
}
