//! Real Git + production resident dispatch/file tools; synthetic API replies only.
use super::*;
use crate::verifier::PinnedCheck;
use std::process::{Command, Stdio};
#[path = "api_workspace_async_tests.rs"]
mod background;

pub(super) fn revise(root: &Path, plan: &mut PlanRevision) {
    let executable = std::env::var("CERULEAN_TEST_GIT").expect("explicit Git fixture required");
    let pin = PinnedCheck::describe(Path::new(&executable), &[], Default::default())
        .unwrap()
        .executable;
    let git = |args: &[&str]| {
        let mut command = Command::new(&pin.path);
        command
            .env_clear()
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "NUL")
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null());
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
            "Git fixture: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    git(&["init", "--initial-branch=main"]);
    fs::write(root.join("project/base.txt"), "committed base\n").unwrap();
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
        "worktree_validation_fixture::api_worktree_inputs".into(),
        "--ignored".into(),
        "--nocapture".into(),
    ];
    // Deliberately differ from the committed input: dispatch must not read this.
    fs::write(root.join("project/base.txt"), "uncommitted user edit\n").unwrap();
}
fn temporary() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("api-worktree-")
        .tempdir_in(
            fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports")).unwrap(),
        )
        .unwrap()
}
fn host(root: &Path, key: bool) -> RuntimeHost {
    fixture_with_revision(
        root,
        project_setup::ProjectWorkerAdapter::Deepseek,
        ApiTarget::Provider {
            endpoint: credentials::ProviderEndpoint::DeepSeek {},
        },
        key,
        revise,
    )
}
fn acknowledge_workspace(host: &mut RuntimeHost) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        host.poll_api();
        if host.api_startups[&("project".into(), "attempt".into())]
            .attempt
            .workspace_proof
            .is_some()
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "workspace acknowledgement timeout: {:?}",
            host.snapshot().api_jobs
        );
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
#[ignore = "explicit real Git; eight production API host routes with synthetic replies"]
fn eight_routes_dispatch_to_worktree_verify_and_reopen_without_resend() {
    for (adapter, target) in api_project_tests::routes() {
        let temp = temporary();
        let mut host = fixture_with_revision(temp.path(), adapter, target, true, revise);
        host.start_managed_api("deputy-api", "attempt").unwrap();
        acknowledge_workspace(&mut host);
        let startup = host.api_startups[&("project".into(), "attempt".into())].clone();
        assert!(startup.attempt.workspace_proof.is_some());
        let workspace = Path::new(&startup.attempt.binding.workspace);
        assert_eq!(
            fs::read_to_string(workspace.join("base.txt")).unwrap(),
            "committed base\n"
        );
        drain(&mut host);
        assert_eq!(
            host.snapshot().api_jobs[0].state,
            ApiJobState::Submitted,
            "{:?}",
            host.snapshot().api_jobs[0]
        );
        assert_eq!(
            fs::read_to_string(workspace.join("result.txt")).unwrap(),
            "background fixture result"
        );
        assert!(!temp.path().join("project/result.txt").exists());
        assert_eq!(
            fs::read_to_string(temp.path().join("project/base.txt")).unwrap(),
            "uncommitted user edit\n"
        );
        let store = host.projects.get_mut("project").unwrap();
        let pins = store.revision().unwrap().execution[0]
            .checks
            .iter()
            .map(PinnedCheck::from_approved)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        store
            .run_verification(
                "isolated-api-check",
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
        let history = store.history().len();
        drop(host);
        let mut reopened = RuntimeHost::open(&temp.path().join("data")).unwrap();
        assert_eq!(
            reopened.projects["project"].state().tasks["task"].state,
            TaskState::Accepted
        );
        reopened.poll_verifications();
        assert!(reopened.api_active.is_empty());
        assert_eq!(reopened.projects["project"].history().len(), history);
        assert!(reopened
            .start_managed_api("deputy-api", "new-attempt")
            .is_err());
    }
}

#[test]
#[ignore = "explicit real Git; missing credential and readiness gates"]
fn missing_key_or_ready_never_starts_model_or_repairs_directory() {
    let temp = temporary();
    let mut host = host(temp.path(), false);
    assert!(host.start_managed_api("deputy-api", "attempt").is_err());
    assert!(host.api_startups.is_empty());
    assert!(!temp.path().join("pool/records").exists());
    crate::credential_vault::save_for(
        &host.root,
        CredentialProvider::DeepSeek,
        &secret::SecretInput::new("sk-synthetic-worktree-test".into()).unwrap(),
    )
    .unwrap();
    host.start_managed_api("deputy-api", "attempt").unwrap();
    acknowledge_workspace(&mut host);
    let update = host.api_active[&("project".into(), "attempt".into())]
        .receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    let crate::async_api::Update::Assignment(crate::managed_assignment::Update::Prepare {
        operation,
        attempt,
        snapshot,
        reply,
    }) = update
    else {
        panic!("expected prepare")
    };
    let key = workspace_key("project", "task", "attempt").unwrap();
    let ready = temp
        .path()
        .join("pool/records")
        .join(format!("{key}.ready.json"));
    fs::rename(&ready, temp.path().join("saved-ready.json")).unwrap();
    let result =
        host.prepare_managed_assignment("project", &operation, *attempt, *snapshot, "deepseek");
    assert!(result.is_err());
    reply.send(result).unwrap();
    drain(&mut host);
    assert!(!ready.exists());
    assert!(host.management.application("deputy-api").is_none());
    assert!(host.projects["project"]
        .attempt_record("task", "attempt")
        .is_none());
    assert!(!host.projects["project"]
        .api_attempt_path("attempt")
        .unwrap()
        .exists());
    drop(host);
    let mut reopened = RuntimeHost::open(&temp.path().join("data")).unwrap();
    assert_eq!(reopened.snapshot().api_jobs[0].state, ApiJobState::Unknown);
    reopened.poll_verifications();
    assert!(reopened.api_active.is_empty());
    assert!(!ready.exists());
}

#[test]
#[ignore = "explicit real Git; lost management ACK recovery retains original directory"]
fn lost_prepare_ack_reopens_application_for_review_without_resend() {
    let temp = temporary();
    let mut host = host(temp.path(), true);
    host.start_managed_api("deputy-api", "attempt").unwrap();
    acknowledge_workspace(&mut host);
    let update = host.api_active[&("project".into(), "attempt".into())]
        .receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    let crate::async_api::Update::Assignment(crate::managed_assignment::Update::Prepare {
        operation,
        attempt,
        snapshot,
        reply,
    }) = update
    else {
        panic!("expected prepare")
    };
    let workspace = attempt.binding.workspace.clone();
    host.prepare_managed_assignment("project", &operation, *attempt, *snapshot, "deepseek")
        .unwrap();
    reply
        .send(Err("simulated lost preparation ACK".into()))
        .unwrap();
    drain(&mut host);
    assert!(host.management.application("deputy-api").is_some());
    drop(host);
    let mut reopened = RuntimeHost::open(&temp.path().join("data")).unwrap();
    reopened.poll_verifications();
    assert!(reopened.api_active.is_empty());
    reopened.application_recovery_review("deputy-api").unwrap();
    assert!(!Path::new(&workspace).join("result.txt").exists());
    assert!(Path::new(&workspace).join("base.txt").exists());
    assert!(reopened
        .start_managed_api("deputy-api", "new-attempt")
        .is_err());
}

fn read_then_write(prepared: &PreparedRequest, round: u32) -> Result<ApiReply, ApiFailure> {
    use cerulean_adapters::api_generation::{fixture_messages, ApiMessage};
    if round == 1 {
        let args = json!({"path":"base.txt","offset":0});
        return fixture_reply(prepared, json!({"id":"read-base","model":"fixture","choices":[{"index":0,"finish_reason":"tool_calls","message":{"role":"assistant","content":null,"tool_calls":[{"id":"call-read","type":"function","function":{"name":"cerulean_read_file","arguments":args.to_string()}}]}}],"usage":{"prompt_tokens":10,"completion_tokens":10}})).map_err(|_| ApiFailure::InvalidResponse {});
    }
    if round == 2 {
        let messages = fixture_messages(prepared);
        let ApiMessage::Tools(results) = messages.last().unwrap() else {
            panic!("expected read evidence")
        };
        assert!(!results[0].is_error);
        let result: serde_json::Value = serde_json::from_str(&results[0].text).unwrap();
        assert_eq!(result["content"], "committed base\n");
    }
    super::reply(prepared, round - 1)
}

#[test]
#[ignore = "explicit real Git; actual API tool read must use committed worktree input"]
fn reads_committed_files_instead_of_uncommitted_source() {
    let temp = temporary();
    let mut host = fixture_with_revision(
        temp.path(),
        project_setup::ProjectWorkerAdapter::Deepseek,
        ApiTarget::Provider {
            endpoint: credentials::ProviderEndpoint::DeepSeek {},
        },
        true,
        |root, plan| {
            revise(root, plan);
            plan.policy.max_requests = 3;
        },
    );
    host.api_fixture = Some(read_then_write);
    host.start_managed_api("deputy-api", "attempt").unwrap();
    drain(&mut host);
    assert_eq!(
        host.snapshot().api_jobs[0].state,
        ApiJobState::Submitted,
        "{:?}",
        host.snapshot().api_jobs[0]
    );
    let startup = &host.api_startups[&("project".into(), "attempt".into())];
    assert_eq!(
        fs::read_to_string(Path::new(&startup.attempt.binding.workspace).join("result.txt"))
            .unwrap(),
        "background fixture result"
    );
    assert_eq!(
        fs::read_to_string(temp.path().join("project/base.txt")).unwrap(),
        "uncommitted user edit\n"
    );
}

fn write_then_fail(prepared: &PreparedRequest, round: u32) -> Result<ApiReply, ApiFailure> {
    if round == 1 {
        super::reply(prepared, round)
    } else {
        Err(ApiFailure::Http { status: 503 })
    }
}

#[test]
#[ignore = "explicit real Git; uncertain API result preserves isolated edits and never restarts"]
fn provider_failure_after_write_retains_unknown_worktree_on_reopen() {
    let temp = temporary();
    let mut host = host(temp.path(), true);
    host.api_fixture = Some(write_then_fail);
    host.start_managed_api("deputy-api", "attempt").unwrap();
    drain(&mut host);
    assert_eq!(host.snapshot().api_jobs[0].state, ApiJobState::Unknown);
    let workspace = host.api_startups[&("project".into(), "attempt".into())]
        .attempt
        .binding
        .workspace
        .clone();
    assert_eq!(
        fs::read_to_string(Path::new(&workspace).join("result.txt")).unwrap(),
        "background fixture result"
    );
    assert!(!temp.path().join("project/result.txt").exists());
    let history = host.projects["project"].history().len();
    drop(host);
    let mut reopened = RuntimeHost::open(&temp.path().join("data")).unwrap();
    reopened.poll_verifications();
    assert!(reopened.api_active.is_empty());
    assert!(reopened.recover_api("project", "attempt").is_err());
    assert!(reopened.start_managed_api("deputy-api", "another").is_err());
    assert_eq!(reopened.projects["project"].history().len(), history);
    assert_eq!(
        fs::read_to_string(Path::new(&workspace).join("result.txt")).unwrap(),
        "background fixture result"
    );
    assert_eq!(reopened.snapshot().api_jobs[0].state, ApiJobState::Unknown);
}
