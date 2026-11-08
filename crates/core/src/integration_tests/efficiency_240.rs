//! Actual Host/Git/file tools/checks; explicit synthetic model/approval fixtures.
use super::*;
use cerulean_domain::project_setup::{AcceptanceDraft, AcceptanceStageDraft, WorkerProfileDraft};
use sha2::{Digest, Sha256};
use std::process::{Command as ProcessCommand, Stdio};

fn selected() -> (String, serde_json::Value, serde_json::Value) {
    let root = PathBuf::from(std::env::var_os("CERULEAN_EFFICIENCY_240_DIR").unwrap());
    let all: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("prepared.json")).unwrap()).unwrap();
    assert_eq!(all["modelTransport"], "synthetic-only");
    let id = std::env::var("CERULEAN_EFFICIENCY_240_CASE").unwrap();
    let case = all["cases"][&id].clone();
    assert!(case.is_object(), "unknown fixture case");
    let task = all["workloads"][case["workload"].as_str().unwrap()].clone();
    (id, case, task)
}
fn fixture_manager(
    intent: crate::management_execution::DecisionIntent,
) -> Result<DecisionObservation, String> {
    let (id, case, workload) = selected();
    assert_eq!(
        intent.context["executionSetup"]["setup"]["requirement"],
        format!("EFF240_{id}\n{}", workload["requirement"].as_str().unwrap())
    );
    let mut result = manager(intent)?;
    if let ManagementDecision::Plan { tasks, .. } = &mut result.envelope.decision {
        if !case["team"].as_bool().unwrap() {
            tasks.retain(|t| t.id == "a");
        }
        for task in tasks {
            let scope = if case["team"] != true {
                "Complete all declared artifact files."
            } else {
                match task.id.as_str() {
                    "a" => "Only modify left.mjs; preserve the other modules.",
                    "b" => "Only modify right.mjs; preserve the other modules.",
                    _ => "Only modify main.mjs; use accepted left/right modules.",
                }
            };
            task.goal = format!(
                "INTEGRATION_{} EFF240_{id}. {}\nAssignment scope: {scope}",
                task.id,
                workload["requirement"].as_str().unwrap()
            );
            task.validator_id = if case["team"] == true && task.id != "integration" {
                task.profile_id = Some(format!("profile-{}", task.id));
                format!("eff-{}", task.id)
            } else {
                "eff-full".into()
            };
        }
    }
    Ok(result)
}
fn fixture_worker(request: &PreparedRequest, round: u32) -> Result<ApiReply, ApiFailure> {
    let (id, case, task) = selected();
    let messages = fixture_messages(request);
    let ApiMessage::User(prompt) = &messages[0] else {
        panic!("approved prompt missing")
    };
    assert!(prompt.contains(&format!("EFF240_{id}")));
    if case["team"] == true {
        parallel::wait_gate(request)?;
    }
    let integration = prompt.contains("INTEGRATION_integration");
    let files: Vec<&str> = if case["team"] == true {
        vec![if integration {
            "main.mjs"
        } else if prompt.contains("INTEGRATION_a") {
            "left.mjs"
        } else {
            assert!(prompt.contains("INTEGRATION_b"));
            "right.mjs"
        }]
    } else {
        task["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect()
    };
    if round > 1 {
        let ApiMessage::Tools(results) = messages.last().unwrap() else {
            panic!("actual file tool result missing")
        };
        assert_eq!(results.len(), 1);
        assert!(!results[0].is_error, "actual write failed");
    }
    let tool = files.get((round - 1) as usize).map(|name| {
        let source = if integration && case["badIntegration"] == true {
            "export function run() { return null; }\n"
        } else {
            task["reference"][*name].as_str().unwrap()
        };
        let before = task["seed"][*name].as_str().unwrap();
        json!({"id":format!("eff-write-{round}"),"type":"function","function":{
            "name":"cerulean_write_file","arguments":json!({"path":name,"content":source,
            "expectedSha256":format!("{:x}",Sha256::digest(before.as_bytes()))}).to_string()}})
    });
    let message = if let Some(call) = &tool {
        json!({"role":"assistant","content":null,"tool_calls":[call]})
    } else {
        json!({"role":"assistant","content":"Controlled fixture completed; independent check required."})
    };
    fixture_reply(request, json!({"id":format!("eff-{round}"),"model":"fixture",
        "choices":[{"index":0,"finish_reason":if tool.is_some(){"tool_calls"}else{"stop"},"message":message}],
        "usage":{"prompt_tokens":10,"completion_tokens":10}}))
        .map_err(|_| ApiFailure::InvalidResponse {})
}
fn git(project: &Path, args: &[&str]) -> String {
    let mut command = ProcessCommand::new(std::env::var("CERULEAN_TEST_GIT").unwrap());
    command
        .env_clear()
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "NUL")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .current_dir(project)
        .args(["-c", "core.hooksPath=NUL", "-c", "core.fsmonitor=false"])
        .args(args);
    for key in ["SystemRoot", "WINDIR", "TEMP", "TMP"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x08000000);
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn configured_efficiency(root: &Path) -> RuntimeHost {
    let (id, case, task) = selected();
    let (mut host, draft) = project_planning::tests::fixture(root);
    let project = root.join("project");
    for (name, text) in task["seed"].as_object().unwrap() {
        fs::write(project.join(name), text.as_str().unwrap()).unwrap();
    }
    git(&project, &["init", "--initial-branch=main"]);
    git(&project, &["add", "--all"]);
    git(
        &project,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@localhost",
            "commit",
            "--no-gpg-sign",
            "-m",
            "Frozen 239 seed",
        ],
    );
    let base = git(&project, &["rev-parse", "HEAD"]);
    fs::create_dir(root.join("pool")).unwrap();
    let suite = PathBuf::from(std::env::var_os("CERULEAN_EFFICIENCY_240_DIR").unwrap());
    let launcher = suite.join("product-verifier.mjs");
    let checker = suite
        .parent()
        .unwrap()
        .join("efficiency-preparation-239/verify.mjs");
    let node = std::env::var("CERULEAN_TEST_NODE").unwrap();
    let mut draft = api_project_tests::draft_for(
        draft,
        ProjectWorkerAdapter::Deepseek,
        ApiTarget::Provider {
            endpoint: ProviderEndpoint::DeepSeek {},
        },
    );
    let artifacts: Vec<String> = task["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().into())
        .collect();
    for (name, key) in [
        ("eff-full", "spec"),
        ("eff-a", "leftSpec"),
        ("eff-b", "rightSpec"),
    ] {
        let Some(spec) = task[key].as_str() else {
            continue;
        };
        host.register_validator(
            name.into(),
            format!("239 fixed check {name}"),
            node.clone(),
            vec![
                launcher.to_string_lossy().into(),
                checker.to_string_lossy().into(),
                spec.into(),
            ],
            Default::default(),
        )
        .unwrap();
    }
    draft.requirement = format!("EFF240_{id}\n{}", task["requirement"].as_str().unwrap());
    draft.validator_id = "eff-full".into();
    draft.check_args = vec![
        launcher.to_string_lossy().into(),
        task["spec"].as_str().unwrap().into(),
    ];
    draft.artifact_paths = artifacts.clone();
    draft.worker_max_requests = 20;
    draft.management_max_requests = 20;
    draft.api_worker.as_mut().unwrap().max_rounds = 6;
    draft.worktree = Some(cerulean_domain::project_setup::WorktreeSetupDraft {
        inherit_dependencies: true,
        base_commit: base,
        pool_root: fs::canonicalize(root.join("pool"))
            .unwrap()
            .to_string_lossy()
            .into(),
        git_executable: std::env::var("CERULEAN_TEST_GIT").unwrap(),
        integration: (case["team"] == true).then(|| IntegrationDraft {
            task_id: "integration".into(),
            artifact_paths: artifacts,
            validator_id: "eff-full".into(),
            args: draft.check_args.clone(),
            timeout_seconds: 30,
        }),
    });
    if case["team"] == true {
        for (id, file, key) in [
            ("a", "left.mjs", "leftSpec"),
            ("b", "right.mjs", "rightSpec"),
        ] {
            draft.profiles.push(WorkerProfileDraft {
                worker_reasoning_effort: None,
                supervision_override: None,
                resource_keys: vec![],
                id: format!("profile-{id}"),
                description: format!("Independent {file} module"),
                worker_adapter: ProjectWorkerAdapter::Deepseek,
                codex_executable: String::new(),
                worker_model: draft.worker_model.clone(),
                omp_runtime_parent: None,
                api_worker: draft.api_worker.clone(),
                desktop_worker: None,
                artifact_paths: vec![file.into()],
                acceptance: AcceptanceDraft {
                    schema_version: 1,
                    stages: vec![AcceptanceStageDraft {
                        expected_assertions: vec![],
                        id: format!("check-{id}"),
                        name: format!("Check {file} interface"),
                        validator_id: format!("eff-{id}"),
                        args: vec![
                            launcher.to_string_lossy().into(),
                            task[key].as_str().unwrap().into(),
                        ],
                        timeout_seconds: 30,
                    }],
                },
            });
        }
    }
    host.configure_project(draft).unwrap();
    host.set_openai_key(Some(
        secret::SecretInput::new("synthetic-efficiency-manager-invalid".into()).unwrap(),
    ))
    .unwrap();
    host.set_provider_key(
        CredentialProvider::DeepSeek,
        Some(secret::SecretInput::new("synthetic-efficiency-worker-invalid".into()).unwrap()),
    )
    .unwrap();
    host.management_fixture = Some(fixture_manager);
    host.api_fixture = Some(fixture_worker);
    host
}
#[test]
#[ignore = "explicit 240 isolated directory; actual Host/Git/tools/verifier, synthetic models only"]
fn workload_binding_acceptance_and_reopen() {
    let (id, case, task) = selected();
    let outer = fs::canonicalize(std::env::var_os("CERULEAN_EFFICIENCY_240_DIR").unwrap()).unwrap();
    let reports =
        fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports")).unwrap();
    assert!(outer.starts_with(&reports));
    let root = outer.join(format!("host-{id}"));
    assert!(!root.exists(), "preserve original attempt");
    fs::create_dir(&root).unwrap();
    let mut host = configured_efficiency(&root);
    let plan = planned(&mut host);
    assert!(host.projects["project"].approval().is_none());
    assert!(host.api_startups.is_empty());
    host.approve("project", &plan.approval_digest().unwrap())
        .unwrap();
    let setup = host.project_setups["project"].digest.clone();
    let gates = (case["team"] == true).then(|| (parallel::gate("a"), parallel::gate("b")));
    host.start_project_run("project", &plan.approval_digest().unwrap(), &setup)
        .unwrap();
    let mut maximum_active = 1;
    if let Some(((a_wait, a_release), (b_wait, b_release))) = gates {
        spin(&mut host, |_| a_wait.try_recv().is_ok());
        spin(&mut host, |_| b_wait.try_recv().is_ok());
        assert_eq!(host.api_active.len(), 2);
        assert_eq!(host.api_writers.len(), 1);
        maximum_active = 2;
        a_release.send(()).unwrap();
        b_release.send(()).unwrap();
    }
    spin(&mut host, |h| {
        h.runs.all()[0].state != RunState::Active && h.api_active.is_empty() && h.active.is_empty()
    });
    let bad = case["badIntegration"] == true;
    assert_eq!(
        host.runs.all()[0].state,
        if bad {
            RunState::Attention
        } else {
            RunState::Completed
        },
        "{:?}",
        host.runs.all()
    );
    let store = &host.projects["project"];
    let history = store.history().to_vec();
    for (task_id, state) in &store.state().tasks {
        let expected_bad = bad && task_id == "integration";
        assert_eq!(
            state.state,
            if expected_bad {
                TaskState::Submitted
            } else {
                TaskState::Accepted
            }
        );
        assert_eq!(state.verified, !expected_bad);
        assert_eq!(history.iter().filter(|e| matches!(&e.body.event.command,WorkflowCommand::DispatchIntent{task,..} if task==task_id)).count(),1);
    }
    for (file, seed) in task["seed"].as_object().unwrap() {
        assert_eq!(
            fs::read_to_string(root.join("project").join(file)).unwrap(),
            seed.as_str().unwrap()
        );
    }
    let dispatches = history
        .iter()
        .filter(|e| {
            matches!(
                &e.body.event.command,
                WorkflowCommand::DispatchIntent { .. }
            )
        })
        .count();
    fs::write(
        root.join("snapshot.json"),
        serde_json::to_vec_pretty(&host.snapshot()).unwrap(),
    )
    .unwrap();
    fs::write(
        root.join("history.json"),
        serde_json::to_vec_pretty(&history).unwrap(),
    )
    .unwrap();
    assert_eq!(host.can_shutdown(), !bad);
    host.set_openai_key(None).unwrap();
    host.set_provider_key(CredentialProvider::DeepSeek, None)
        .unwrap();
    drop(host);
    for _ in 0..2 {
        let mut reopened = RuntimeHost::open(&root.join("data")).unwrap();
        reopened.poll_verifications();
        assert_eq!(reopened.projects["project"].history(), history);
        assert_eq!(reopened.can_shutdown(), !bad);
    }
    fs::write(root.join("result.json"),serde_json::to_vec_pretty(&json!({
        "status":"passed","case":id,"scope":"controlled model/approval, real Host/Git/file tools/independent verifier",
        "maxConcurrentWorkers":maximum_active,"dispatches":dispatches,"reopensWithoutRedispatch":2,
        "badIntegrationRejected":bad,"sourceProjectUnchanged":true,"syntheticCredentialsRemoved":true,
        "realModelCalls":0,"realEfficiencyRuns":0,"releaseAccepted":false
    })).unwrap()).unwrap();
}

#[test]
#[ignore = "read original failed 240 fixture only; no dispatch or model fixture installed"]
fn recover_original_bad_integration_without_dispatch() {
    let (_, case, task) = selected();
    assert_eq!(case["badIntegration"], true);
    let outer = fs::canonicalize(std::env::var_os("CERULEAN_EFFICIENCY_240_DIR").unwrap()).unwrap();
    let root = outer.join("host-bad-integration");
    assert!(root.join("snapshot.json").exists());
    assert!(!root.join("recovery-result.json").exists());
    let original: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("history.json")).unwrap()).unwrap();
    for index in 0..2 {
        let mut host = RuntimeHost::open(&root.join("data")).unwrap();
        assert!(host.management_fixture.is_none() && host.api_fixture.is_none());
        host.poll_verifications();
        assert_eq!(host.runs.all()[0].state, RunState::Attention);
        assert!(host.api_active.is_empty() && host.active.is_empty());
        assert_eq!(host.management_work_count(), 0);
        let store = &host.projects["project"];
        assert_eq!(serde_json::to_value(store.history()).unwrap(), original);
        for task in ["a", "b"] {
            assert_eq!(store.state().tasks[task].state, TaskState::Accepted);
        }
        assert_eq!(
            store.state().tasks["integration"].state,
            TaskState::Submitted
        );
        assert!(!store.state().tasks["integration"].verified);
        assert!(
            !host.can_shutdown(),
            "Submitted still requires explicit resolution"
        );
        if index == 0 {
            host.set_openai_key(None).unwrap();
            host.set_provider_key(CredentialProvider::DeepSeek, None)
                .unwrap();
        }
        assert!(!host.openai_connection.key_configured && !host.deepseek_key_configured);
    }
    for (file, seed) in task["seed"].as_object().unwrap() {
        assert_eq!(
            fs::read_to_string(root.join("project").join(file)).unwrap(),
            seed.as_str().unwrap()
        );
    }
    fs::write(root.join("recovery-result.json"),serde_json::to_vec_pretty(&json!({
        "status":"passed-original-negative-task-recovery","originalCaseFailure":"fixture asserted can_shutdown for unresolved Submitted task",
        "productGuardPreserved":true,"canShutdown":false,"activeOwnedWork":0,
        "newDispatches":0,"newModelCalls":0,"reopensWithoutRedispatch":2,
        "badIntegrationRejected":true,"syntheticCredentialsRemoved":true,
        "sourceProjectUnchanged":true,"releaseAccepted":false
    })).unwrap()).unwrap();
}
