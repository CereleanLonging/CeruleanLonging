use super::*;
use cerulean_adapters::{
    api_generation::{fixture_reply, ApiFailure, ApiReply, PreparedRequest},
    openai::DecisionObservation,
};
use cerulean_domain::{
    api_jobs::ApiJobState,
    api_worker::ApiTarget,
    credentials::CredentialProvider,
    management::{DecisionEnvelope, ManagementDecision, Purpose},
};
use serde_json::json;
use std::time::{Duration, Instant};

pub(super) fn protect_new_api_fixture(root: &Path) {
    let data = root.join("data");
    if !data.exists() {
        // New synthetic API fixtures opt into measured replies before any
        // project or dispatch exists. Existing roots are never auto-promoted.
        drop(Store::open(&data).unwrap());
        crate::rejected_usage_guard::tests::promote_fixture(
            &data,
            &root.join("reader17-initial-backup"),
        );
    }
    crate::rejected_usage_guard::require_protected(&data, true).unwrap();
}

pub(super) fn fixture(
    root: &Path,
    adapter: project_setup::ProjectWorkerAdapter,
    target: ApiTarget,
    key: bool,
) -> RuntimeHost {
    fixture_with_revision(root, adapter, target, key, |_, _| {})
}
pub(super) fn fixture_with_revision(
    root: &Path,
    adapter: project_setup::ProjectWorkerAdapter,
    target: ApiTarget,
    key: bool,
    revise: impl FnOnce(&Path, &mut PlanRevision),
) -> RuntimeHost {
    fixture_with_configuration(root, adapter, target, key, |_| {}, revise)
}
pub(super) fn fixture_with_configuration(
    root: &Path,
    adapter: project_setup::ProjectWorkerAdapter,
    target: ApiTarget,
    key: bool,
    configure: impl FnOnce(&mut project_setup::ProjectSetupDraft),
    revise: impl FnOnce(&Path, &mut PlanRevision),
) -> RuntimeHost {
    protect_new_api_fixture(root);
    let (mut host, draft) = project_planning::tests::fixture(root);
    let provider = target.provider();
    let mut draft = api_project_tests::draft_for(draft, adapter, target);
    draft.artifact_paths = vec!["result.txt".into()];
    configure(&mut draft);
    host.configure_project(draft).unwrap();
    let mut proposal = project_planning::tests::proposal("tests");
    if let ManagementDecision::Plan { tasks, .. } = &mut proposal {
        tasks[0].worker_id = adapter.worker_id();
    }
    project_planning::tests::complete(&mut host, proposal);
    let original = host.projects["project"].revision().unwrap().clone();
    let mut revision = original.clone();
    revise(root, &mut revision);
    if revision != original {
        revision.workflow.version += 1;
        host.propose("project", "fixture-worktree-plan", revision)
            .unwrap();
    }
    let wanted = host.projects["project"]
        .revision()
        .unwrap()
        .approval_digest()
        .unwrap();
    host.approve("project", &wanted).unwrap();
    let context = host
        .management_context("project", "Execute the approved task")
        .unwrap();
    let state_digest = digest(&context).unwrap();
    host.management
        .begin(crate::management_execution::DecisionIntent {
            operation_id: "deputy-api".into(),
            project_id: "project".into(),
            purpose: Purpose::Assign,
            model: "fixture".into(),
            state_digest: state_digest.clone(),
            context,
            created_at: chrono::Utc::now().to_rfc3339(),
        })
        .unwrap();
    host.management
        .finish(
            "deputy-api",
            Ok(DecisionObservation {
                response_id: "resp_assignment".into(),
                requested_model: "fixture".into(),
                actual_model: "fixture".into(),
                input_tokens: 1,
                output_tokens: 1,
                envelope: DecisionEnvelope {
                    state_digest,
                    decision: ManagementDecision::Assign {
                        task_id: "task".into(),
                        worker_id: adapter.worker_id(),
                        rationale: "approved".into(),
                    },
                },
            }),
        )
        .unwrap();
    if key {
        crate::credential_vault::save_for(
            &host.root,
            provider,
            &secret::SecretInput::new("sk-synthetic-api-background-test".into()).unwrap(),
        )
        .unwrap();
    }
    host.api_fixture = Some(reply);
    host
}

#[path = "api_registry_tests.rs"]
mod registry;
#[path = "api_host_worktree_tests.rs"]
mod worktree;
pub(super) fn reply(prepared: &PreparedRequest, round: u32) -> Result<ApiReply, ApiFailure> {
    let args = json!({"path":"result.txt","content":"background fixture result","expectedSha256":"absent"});
    let tool = round == 1;
    let target = &prepared.fingerprint().target;
    let value = if matches!(target, ApiTarget::OpenAi {}) {
        json!({"id":format!("resp-{round}"),"model":"fixture","status":"completed","output":if tool{json!([{"type":"function_call","call_id":"call-1","name":"cerulean_write_file","arguments":args.to_string()}])}else{json!([{"type":"message","id":"msg-2","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Done."}]}])},"usage":{"input_tokens":10,"output_tokens":10}})
    } else if target.provider() == CredentialProvider::Anthropic {
        json!({"id":format!("msg-{round}"),"type":"message","role":"assistant","model":"fixture","stop_reason":if tool{"tool_use"}else{"end_turn"},"content":if tool{json!([{"type":"tool_use","id":"call-1","name":"cerulean_write_file","input":args}])}else{json!([{"type":"text","text":"Done."}])},"usage":{"input_tokens":10,"output_tokens":10}})
    } else {
        json!({"id":format!("chat-{round}"),"model":"fixture","choices":[{"index":0,"finish_reason":if tool{"tool_calls"}else{"stop"},"message":if tool{json!({"role":"assistant","content":null,"tool_calls":[{"id":"call-1","type":"function","function":{"name":"cerulean_write_file","arguments":args.to_string()}}]})}else{json!({"role":"assistant","content":"Done."})}}],"usage":{"prompt_tokens":10,"completion_tokens":10}})
    };
    fixture_reply(prepared, value).map_err(|_| ApiFailure::InvalidResponse {})
}
pub(super) fn drain(host: &mut RuntimeHost) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !host.api_active.is_empty() && Instant::now() < deadline {
        host.poll_verifications();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        host.api_active.is_empty(),
        "API worker did not return ledger"
    );
}
#[test]
fn api_background_eight_routes_use_two_durable_acks_and_reopen_without_resend() {
    for (adapter, target) in api_project_tests::routes() {
        let root = tempfile::tempdir().unwrap();
        let provider = target.provider();
        let mut host = fixture(root.path(), adapter, target, true);
        host.start_managed_api("deputy-api", "attempt").unwrap();
        assert_eq!(host.api_active.len(), 1);
        assert!(!host.projects.contains_key("project"));
        assert_eq!(host.snapshot().api_jobs[0].state, ApiJobState::Starting);
        let attempt_path = host
            .root
            .join("workflows/project")
            .join(format!("api-attempt-{}", digest(&"attempt").unwrap()));
        // Explicitly receive both handshake messages without polling host. No HTTP may start.
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
        assert!(!attempt_path.exists());
        assert_eq!(
            host.api_writers["project"]
                .owner
                .snapshot()
                .unwrap()
                .state
                .tasks["task"]
                .state,
            TaskState::Pending
        );
        assert!(host.management.application("deputy-api").is_none());
        assert!(host.require_provider_idle(provider).is_err());
        assert!(!host.can_shutdown());
        let adapter_id = adapter.id();
        let result =
            host.prepare_managed_assignment("project", &operation, *attempt, *snapshot, adapter_id);
        assert!(result.is_ok(), "{result:?}");
        reply.send(result).unwrap();
        let update = host.api_active[&("project".into(), "attempt".into())]
            .receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        let crate::async_api::Update::Assignment(crate::managed_assignment::Update::Commit {
            operation,
            entry,
            snapshot,
            reply,
        }) = update
        else {
            panic!("expected commit")
        };
        assert!(!attempt_path.exists());
        assert_eq!(
            host.api_writers["project"]
                .owner
                .snapshot()
                .unwrap()
                .state
                .tasks["task"]
                .state,
            TaskState::Assigned
        );
        assert!(!host.management.application("deputy-api").unwrap().1);
        let result =
            host.commit_managed_assignment("project", &operation, *entry, *snapshot, adapter_id);
        assert!(result.is_ok(), "{result:?}");
        reply.send(result).unwrap();
        drain(&mut host);
        assert_eq!(
            host.snapshot().api_jobs[0].state,
            ApiJobState::Submitted,
            "{:?}",
            host.snapshot().api_jobs[0]
        );
        assert_eq!(
            fs::read_to_string(root.path().join("project/result.txt")).unwrap(),
            "background fixture result"
        );
        assert_eq!(
            host.projects["project"].state().tasks["task"].state,
            TaskState::Submitted
        );
        assert!(host.management.application("deputy-api").unwrap().1);
        assert!(host.require_provider_idle(provider).is_ok());
        assert!(host.start_managed_api("deputy-api", "another").is_err());
        let history = host.projects["project"].history().len();
        drop(host);
        let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(host.snapshot().api_jobs[0].state, ApiJobState::Submitted);
        for _ in 0..3 {
            host.poll_verifications();
        }
        assert!(host.api_active.is_empty());
        assert_eq!(host.projects["project"].history().len(), history);
    }
}
#[test]
fn api_missing_key_never_reserves_startup_and_cancel_before_dispatch_stays_unknown() {
    let root = tempfile::tempdir().unwrap();
    let mut host = fixture(
        root.path(),
        project_setup::ProjectWorkerAdapter::Deepseek,
        ApiTarget::Provider {
            endpoint: credentials::ProviderEndpoint::DeepSeek {},
        },
        false,
    );
    assert!(host.start_managed_api("deputy-api", "attempt").is_err());
    assert!(host.api_startups.is_empty());
    assert!(host.management.application("deputy-api").is_none());
    crate::credential_vault::save_for(
        &host.root,
        CredentialProvider::DeepSeek,
        &secret::SecretInput::new("sk-synthetic-api-cancel-test".into()).unwrap(),
    )
    .unwrap();
    host.start_managed_api("deputy-api", "attempt").unwrap();
    host.cancel_api("project", "attempt", "stop before generation")
        .unwrap();
    drain(&mut host);
    assert_eq!(host.snapshot().api_jobs[0].state, ApiJobState::Unknown);
    assert!(!root.path().join("project/result.txt").exists());
    let store = &host.projects["project"];
    assert!(!store
        .history()
        .iter()
        .any(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. })));
    assert!(host.require_available("project").is_err());
    drop(host);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(host.snapshot().api_jobs[0].state, ApiJobState::Unknown);
    assert!(host.start_managed_api("deputy-api", "new-attempt").is_err());
}
#[test]
fn api_reopen_rejects_changed_startup_and_missing_original_startup() {
    for missing in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut host = fixture(
            root.path(),
            project_setup::ProjectWorkerAdapter::Deepseek,
            ApiTarget::Provider {
                endpoint: credentials::ProviderEndpoint::DeepSeek {},
            },
            true,
        );
        host.start_managed_api("deputy-api", "attempt").unwrap();
        drain(&mut host);
        assert_eq!(host.snapshot().api_jobs[0].state, ApiJobState::Submitted);
        let path = fs::read_dir(host.root.join("api-startups"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        drop(host);
        if missing {
            fs::rename(&path, root.path().join("removed-startup.json")).unwrap();
        } else {
            let mut value: serde_json::Value =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            value["intent"]["worker"]["model"] = json!("changed");
            value["hash"] = json!(digest(
                &serde_json::from_value::<crate::api_startup::ApiStartup>(value["intent"].clone())
                    .unwrap()
            )
            .unwrap());
            fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        }
        assert!(RuntimeHost::open(&root.path().join("data")).is_err());
    }
}

#[test]
fn api_unacknowledged_preparation_cannot_hide_removed_startup_on_reopen() {
    let root = tempfile::tempdir().unwrap();
    let mut host = fixture(
        root.path(),
        project_setup::ProjectWorkerAdapter::Deepseek,
        ApiTarget::Provider {
            endpoint: credentials::ProviderEndpoint::DeepSeek {},
        },
        true,
    );
    host.start_managed_api("deputy-api", "attempt").unwrap();
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
    host.prepare_managed_assignment("project", &operation, *attempt, *snapshot, "deepseek")
        .unwrap();
    reply
        .send(Err("simulated lost acknowledgement".into()))
        .unwrap();
    drain(&mut host);
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Pending
    );
    assert!(host.management.application("deputy-api").is_some());
    let file = fs::read_dir(host.root.join("api-startups"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    drop(host);
    // With the intact startup the preparation reopens, but is never reapplied.
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    host.poll_verifications();
    assert!(host.api_active.is_empty());
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Pending
    );
    drop(host);
    fs::rename(file, root.path().join("removed-startup.json")).unwrap();
    assert!(RuntimeHost::open(&root.path().join("data")).is_err());
}
