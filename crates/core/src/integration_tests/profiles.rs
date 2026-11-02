use super::*;
use cerulean_domain::project_setup::{AcceptanceDraft, AcceptanceStageDraft, WorkerProfileDraft};

fn profile_manager(
    i: crate::management_execution::DecisionIntent,
) -> Result<DecisionObservation, String> {
    let context = i.context.clone();
    let mut result = manager(i)?;
    match &mut result.envelope.decision {
        ManagementDecision::Plan { tasks, .. } => {
            for (task, profile) in tasks.iter_mut().take(2).zip(
                context["executionSetup"]["setup"]["profiles"]
                    .as_array()
                    .unwrap(),
            ) {
                task.profile_id = Some(profile["id"].as_str().unwrap().into());
                task.worker_id = format!("worker-{}", profile["workerAdapter"].as_str().unwrap());
            }
            if context["executionSetup"]["setup"]["requirement"] == "bad-profile" {
                tasks[0].profile_id = Some("unknown".into());
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
fn profile_worker(request: &PreparedRequest, round: u32) -> Result<ApiReply, ApiFailure> {
    let ApiMessage::User(prompt) = &fixture_messages(request)[0] else {
        panic!("prompt missing")
    };
    let (model, provider) = if prompt.contains("INTEGRATION_a") {
        ("fixture-a", CredentialProvider::DeepSeek)
    } else if prompt.contains("INTEGRATION_b") {
        (
            "fixture-b",
            if prompt.contains("mixed-adapter") {
                CredentialProvider::Grok
            } else {
                CredentialProvider::DeepSeek
            },
        )
    } else {
        ("vendor/fixture-model", CredentialProvider::DeepSeek)
    };
    assert_eq!(request.fingerprint().requested_model, model);
    assert_eq!(request.fingerprint().target.provider(), provider);
    worker(request, round)
}
fn host(root: &Path, mode: &str) -> RuntimeHost {
    let mut host = configured_with(root, mode, |draft| {
        for id in ["a", "b"] {
            let mut api = draft.api_worker.clone().unwrap();
            api.model = format!("fixture-{id}");
            let adapter = if id == "b" && mode.contains("mixed-adapter") {
                api.target = ApiTarget::Provider {
                    endpoint: ProviderEndpoint::Grok {},
                };
                ProjectWorkerAdapter::Grok
            } else {
                ProjectWorkerAdapter::Deepseek
            };
            draft.profiles.push(WorkerProfileDraft {
                supervision_override: None,
                resource_keys: vec![],
                id: format!("profile-{id}"),
                description: format!("Implement branch {id}"),
                worker_adapter: adapter,
                codex_executable: String::new(),
                worker_model: api.model.clone(),
                omp_runtime_parent: None,
                api_worker: Some(api),
                desktop_worker: None,
                artifact_paths: vec![format!("{id}.txt")],
                acceptance: AcceptanceDraft {
                    schema_version: 1,
                    stages: vec![AcceptanceStageDraft {
                        expected_assertions: vec![],
                        id: format!("check-{id}"),
                        name: format!("Check branch {id}"),
                        validator_id: "tests".into(),
                        args: draft.check_args.clone(),
                        timeout_seconds: 30,
                    }],
                },
            });
        }
    });
    host.management_fixture = Some(profile_manager);
    host.api_fixture = Some(profile_worker);
    if mode.contains("mixed-adapter") {
        host.set_provider_key(
            CredentialProvider::Grok,
            Some(secret::SecretInput::new("sk-synthetic-profile-grok".into()).unwrap()),
        )
        .unwrap();
    }
    host
}
#[test]
#[ignore = "real Git; named profiles, distinct API models/providers, independent final integration and restart; synthetic responses"]
fn named_models_and_mixed_providers_complete_one_project() {
    for mode in ["same-adapter", "mixed-adapter"] {
        let root = temporary();
        let mut host = host(root.path(), mode);
        let plan = planned(&mut host);
        assert_eq!(
            plan.task("a").unwrap().profile_id.as_deref(),
            Some("profile-a")
        );
        assert_eq!(
            plan.task("b").unwrap().profile_id.as_deref(),
            Some("profile-b")
        );
        assert!(plan.task("integration").unwrap().profile_id.is_none());
        assert_eq!(plan.task("a").unwrap().artifact_paths, vec!["a.txt"]);
        assert_eq!(plan.task("b").unwrap().artifact_paths, vec!["b.txt"]);
        assert_ne!(
            plan.task("a").unwrap().api_worker,
            plan.task("b").unwrap().api_worker
        );
        assert_eq!(
            plan.task("b").unwrap().adapter_id,
            if mode == "mixed-adapter" {
                "grok"
            } else {
                "deepseek"
            }
        );
        host.approve("project", &plan.approval_digest().unwrap())
            .unwrap();
        let saved = host.project_setups["project"].clone();
        // Even same authenticated adapter cannot substitute another named model after approval.
        let mut changed = saved.clone();
        changed.setup.profiles[0].api_worker.as_mut().unwrap().model = "replacement".into();
        changed.digest = digest(&changed.setup).unwrap();
        host.project_setups
            .insert("project".into(), changed.clone());
        assert!(host
            .start_project_run("project", &plan.approval_digest().unwrap(), &changed.digest)
            .is_err());
        assert!(host.runs.all().is_empty() && host.api_startups.is_empty());
        host.project_setups.insert("project".into(), saved.clone());
        host.start_project_run("project", &plan.approval_digest().unwrap(), &saved.digest)
            .unwrap();
        // Attention stops new scheduling immediately; collect the owned worker
        // before inspecting the returned idle ledger or reopening it.
        spin(&mut host, |h| {
            h.runs.all()[0].state != RunState::Active && h.api_active.is_empty()
        });
        assert_eq!(
            host.runs.all()[0].state,
            RunState::Completed,
            "{:?}",
            host.runs.all()
        );
        let store = &host.projects["project"];
        assert!(store
            .state()
            .tasks
            .values()
            .all(|t| t.state == TaskState::Accepted && t.verified));
        assert_eq!(crate::api_execution::prior_requests(store).unwrap(), 10);
        assert_eq!(host.api_jobs.len(), 3);
        let path = dependency_host_fixture::task_path(&host, "integration");
        assert_eq!(fs::read_to_string(path.join("bundle.txt")).unwrap(), "AB");
        assert!(!root.path().join("project/bundle.txt").exists());
        let history = store.history().to_vec();
        let setup = host.project_setups["project"].digest.clone();
        drop(host);
        let mut reopened = RuntimeHost::open(&root.path().join("data")).unwrap();
        reopened.poll_verifications();
        assert_eq!(reopened.project_setups["project"].digest, setup);
        assert_eq!(reopened.projects["project"].history(), history);
        assert_eq!(reopened.runs.all()[0].state, RunState::Completed);
        println!("named profiles {mode}: 3 accepted tasks, 10 synthetic API rounds, final AB, reopen events 0");
    }
}
#[test]
#[ignore = "real Git config; named selection, credentials and persisted profile corruption gates"]
fn profile_selection_and_missing_credentials_block_before_dispatch() {
    let root = temporary();
    let mut host = host(root.path(), "mixed-adapter");
    let plan = planned(&mut host);
    host.approve("project", &plan.approval_digest().unwrap())
        .unwrap();
    host.set_provider_key(CredentialProvider::Grok, None)
        .unwrap();
    let saved = host.project_setups["project"].clone();
    assert!(host
        .start_project_run("project", &plan.approval_digest().unwrap(), &saved.digest)
        .unwrap_err()
        .contains("grok"));
    assert!(host.runs.all().is_empty() && host.api_jobs.is_empty());
    assert!(!root.path().join("pool/records").exists());
    drop(host);
    let path = root
        .path()
        .join("data/project-setups/project.00000001.json");
    let mut corrupt = saved;
    corrupt.setup.profiles[0].worker_adapter = ProjectWorkerAdapter::Grok;
    corrupt.digest = digest(&corrupt.setup).unwrap();
    fs::write(path, serde_json::to_vec(&corrupt).unwrap()).unwrap();
    assert!(RuntimeHost::open(&root.path().join("data")).is_err());
    let root2 = temporary();
    let mut invalid = host_for_invalid(root2.path());
    let setup = invalid.project_setups["project"].digest.clone();
    invalid.request_chief_plan("project", &setup).unwrap();
    spin(&mut invalid, |h| !h.planning_errors.is_empty());
    assert!(invalid.projects["project"].revision().is_none());
    assert!(invalid.api_startups.is_empty());
}
fn host_for_invalid(root: &Path) -> RuntimeHost {
    host(root, "bad-profile")
}
