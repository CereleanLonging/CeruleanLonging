use super::*;
use crate::{scheduler::SchedulerConfig, verifier::PinnedCheck};
use cerulean_adapters::api_generation::{fixture_reply, ApiTarget};
use cerulean_domain::{
    credentials::ProviderEndpoint,
    secret::SecretInput,
    workflow::{Principal, WorkflowPlan, WorkflowTask},
};
use serde_json::{json, Value};
#[path = "coordinator_budget_tests.rs"]
mod coordinator_budget;
#[path = "single_executor_tests.rs"]
mod single_executor;
#[path = "write_scope_tests.rs"]
mod write_scope;
pub(crate) fn baseline_plan_fixture(root: &Path) -> PlanRevision {
    plan(root, selection(targets().remove(2)))
}
pub(crate) fn baseline_reply_fixture(
    prepared: &PreparedRequest,
    round: usize,
) -> Result<ApiReply, ApiFailure> {
    fixture_reply(prepared, body(&targets().remove(2), round, "ok"))
        .map_err(|_| ApiFailure::InvalidResponse {})
}
fn targets() -> Vec<ApiTarget> {
    vec![
        ApiTarget::OpenAi {},
        ApiTarget::Provider {
            endpoint: ProviderEndpoint::Anthropic {},
        },
        ApiTarget::Provider {
            endpoint: ProviderEndpoint::DeepSeek {},
        },
        ApiTarget::Provider {
            endpoint: ProviderEndpoint::QwenSingapore {},
        },
        ApiTarget::Provider {
            endpoint: ProviderEndpoint::KimiChina {},
        },
        ApiTarget::Provider {
            endpoint: ProviderEndpoint::Grok {},
        },
    ]
}
fn selection(target: ApiTarget) -> ApiWorkerSelection {
    ApiWorkerSelection {
        write_scope: None,
        target,
        model: "fixture".into(),
        file_tools: vec![ToolPermission::ReadFiles, ToolPermission::WriteFiles],
        max_rounds: 4,
        max_tool_calls: 4,
        max_output_tokens: 1000,
        max_total_tokens: 10000,
        request_timeout_seconds: 2,
    }
}
fn plan(root: &Path, worker: ApiWorkerSelection) -> PlanRevision {
    fs::create_dir(root.join("project")).unwrap();
    let validator =
        PinnedCheck::describe(&std::env::current_exe().unwrap(), &[], Default::default()).unwrap();
    PlanRevision {
        management_setup_digest: None,
        worktree: None,
        schema_version: 1,
        project: Project {
            id: "project".into(),
            root: root.join("project").to_string_lossy().into(),
            timezone: "Asia/Shanghai".into(),
        },
        workflow: WorkflowPlan {
            id: "plan".into(),
            version: 1,
            tasks: vec![WorkflowTask {
                id: "task".into(),
                workspace_key: "project".into(),
                goal: "Create result.txt containing fixture output".into(),
                dependencies: vec![],
                resource_keys: None,
            }],
        },
        policy: ExecutionPolicy {
            version: 1,
            tools: vec![
                ToolPermission::ReadFiles,
                ToolPermission::WriteFiles,
                ToolPermission::RunCommands,
            ],
            max_requests: 3,
            max_seconds: 30,
            currency_budget: None,
            budget_usd_cents: None,
            supervision: None,
        },
        snapshot_policy: Some(SnapshotPolicy {
            exclude: vec![],
            max_files: 100,
            max_bytes: 1024 * 1024,
        }),
        execution: vec![TaskExecution {
            worker_directory: None,
            supervision_override: None,
            profile_id: None,
            task_id: "task".into(),
            adapter_id: worker.target.provider().id().into(),
            worker: None,
            desktop_worker: None,
            api_worker: Some(worker),
            artifact_paths: vec!["result.txt".into()],
            checks: vec![AcceptanceCheck {
                expected_assertions: vec![],
                id: "check".into(),
                name: None,
                program: validator.executable.path.clone(),
                args: vec![
                    "--ignored".into(),
                    "--exact".into(),
                    "api_execution::tests::artifact_validator".into(),
                ],
                timeout_seconds: 5,
                tool: Some(validator),
            }],
        }],
    }
}
fn assigned(root: &Path, plan: PlanRevision) -> DurableWorkflow {
    let principals = [
        ("user", Role::User, None),
        ("chief", Role::Chief, Some("openai")),
        ("deputy", Role::Deputy, Some("openai")),
        ("core", Role::Core, None),
        ("worker", Role::Worker, None),
    ]
    .into_iter()
    .map(|(id, role, provider)| Principal {
        id: id.into(),
        role,
        provider: provider.map(str::to_owned),
    })
    .collect();
    let config = SchedulerConfig {
        schema_version: 1,
        core_id: "core".into(),
        principals,
        execution_scope: Some(ExecutionScope {
            project: plan.project.clone(),
            policy_version: 1,
        }),
    };
    let mut store = DurableWorkflow::open(&root.join("ledger"), config).unwrap();
    record(
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
    let approved = Approval {
        plan_id: "plan".into(),
        revision: 1,
        policy_version: 1,
        revision_digest: plan.approval_digest().unwrap(),
        user_id: "user".into(),
        approved_at: chrono::Utc::now().to_rfc3339(),
    };
    let workflow_digest = store.state().digest.clone().unwrap();
    record(
        &mut store,
        "user",
        WorkflowCommand::Approve {
            digest: workflow_digest,
        },
        CommandContext {
            approval_source: Some("offline-api-fixture".into()),
            execution: Some(ExecutionEvidence::Approval { approval: approved }),
            ..Default::default()
        },
    )
    .unwrap();
    let (workspace, workspace_proof) = if plan.worktree.is_some() {
        let (workspace, proof) = store.prepare_task_workspace("task", "attempt").unwrap();
        (workspace, Some(proof))
    } else {
        (plan.execution_root("task", "attempt").unwrap(), None)
    };
    let attempt = Attempt {
        workspace_proof,
        id: "attempt".into(),
        task_id: "task".into(),
        worker_id: "worker".into(),
        revision_digest: plan.approval_digest().unwrap(),
        fence: 1,
        binding: SessionBinding {
            adapter_id: plan.execution[0].adapter_id.clone(),
            adapter_version: "api-worker-v1".into(),
            session_id: "api-session".into(),
            workspace,
            identity_strength: IdentityStrength::ControllerRequest(ControllerRequestIdentity {
                connection_id: "api-connection".into(),
                request_id: "api-attempt".into(),
            }),
        },
    };
    record(
        &mut store,
        "deputy",
        WorkflowCommand::Assign {
            task: "task".into(),
            worker: "worker".into(),
            attempt: "attempt".into(),
        },
        CommandContext {
            execution: Some(ExecutionEvidence::Attempt { attempt }),
            ..Default::default()
        },
    )
    .unwrap();
    store
}
fn body(target: &ApiTarget, round: usize, mode: &str) -> Value {
    let args = json!({"path":if mode=="escape"{"../outside.txt"}else{"result.txt"},"content":"fixture output","expectedSha256":"absent"});
    let tool = round == 1;
    if matches!(target, ApiTarget::OpenAi {}) {
        json!({"id":format!("resp-{round}"),"model":"fixture","status":"completed","output":if tool{json!([{"type":"function_call","call_id":"call-1","name":"cerulean_write_file","arguments":args.to_string()}])}else{json!([{"type":"message","id":"msg-2","role":"assistant","status":"completed","content":[{"type":"output_text","text":"Done."}]}])},"usage":{"input_tokens":10,"output_tokens":10}})
    } else if target.provider() == cerulean_domain::credentials::CredentialProvider::Anthropic {
        json!({"id":format!("msg-{round}"),"type":"message","role":"assistant","model":"fixture","stop_reason":if tool{"tool_use"}else{"end_turn"},"content":if tool{json!([{"type":"tool_use","id":"call-1","name":"cerulean_write_file","input":args}])}else{json!([{"type":"text","text":"Done."}])},"usage":{"input_tokens":10,"output_tokens":10}})
    } else {
        let mut message = if tool {
            json!({"role":"assistant","content":null,"reasoning_content":"fixture-private-continuation","tool_calls":[{"id":"call-1","type":"function","function":{"name":"cerulean_write_file","arguments":args.to_string()}}]})
        } else {
            json!({"role":"assistant","content":"Done."})
        };
        if mode == "read-failures" && tool {
            let write = message["tool_calls"][0].clone();
            let read = |id: &str| {
                json!({"id":id,"type":"function","function":{
                    "name":"cerulean_read_file","arguments":json!({"path":"missing.txt","offset":0}).to_string()
                }})
            };
            message["tool_calls"] = json!([read("read-10"), read("read-2"), write]);
        }
        if mode == "tool-limit" && tool {
            let mut other = message["tool_calls"][0].clone();
            other["id"] = json!("call-2");
            message["tool_calls"].as_array_mut().unwrap().push(other);
        }
        json!({"id":format!("chat-{round}"),"model":"fixture","choices":[{"index":0,"finish_reason":if tool{"tool_calls"}else{"stop"},"message":message}],"usage":{"prompt_tokens":10,"completion_tokens":10}})
    }
}
async fn run(
    store: &mut DurableWorkflow,
    mode: &str,
    rx: &mut oneshot::Receiver<()>,
    observe: &mut impl FnMut(&str) -> Result<(), String>,
) -> (Result<(), String>, usize) {
    let target = store.revision().unwrap().execution[0]
        .api_worker
        .as_ref()
        .unwrap()
        .target
        .clone();
    let root = store.api_attempt_path("attempt").unwrap();
    let mut sent = 0usize;
    let client =
        ApiClient::new(SecretInput::new("sk-synthetic-api-loop-only".into()).unwrap()).unwrap();
    let result = dispatch_inner(
        store,
        &client,
        "task",
        "attempt",
        rx,
        &mut async |prepared: PreparedRequest, _cancel: &mut oneshot::Receiver<()>, gateway: &mut cerulean_adapters::api_generation::GatewayObserver<'_>| {
            sent += 1;
            assert!(root.join(format!("rounds/{sent:03}/00.json")).is_file());
            if mode == "gateway" {
                use cerulean_adapters::api_generation::gateway::{GatewayHeaders, GatewayRequestId};
                gateway(&GatewayHeaders { status: 200, request_id: GatewayRequestId::Accepted {
                    value: format!("gateway-driver-{sent}"),
                }});
            }
            if mode == "provider-error" {
                return Err(ApiFailure::Http { status: 401 });
            }
            fixture_reply(&prepared, body(&target, sent, mode))
                .map_err(|_| ApiFailure::InvalidResponse {})
        },
        observe,
        &mut |_| {},
    )
    .await;
    (result, sent)
}
fn reopen(store: DurableWorkflow, root: &Path) -> DurableWorkflow {
    let config = store.configuration().clone();
    drop(store);
    DurableWorkflow::open(&root.join("ledger"), config).unwrap()
}

#[cfg(windows)]
#[tokio::test]
async fn gateway_324_legacy_driver_files_inventory_and_reopen_keep_linked_headers() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    drop(crate::Store::open(&data).unwrap());
    crate::relay_guard::tests::promote_fixture(&data, &temp.path().join("before16"));
    let workflows = data.join("workflows");
    fs::create_dir_all(&workflows).unwrap();
    let p = plan(
        temp.path(),
        selection(ApiTarget::Provider {
            endpoint: ProviderEndpoint::A6Api {},
        }),
    );
    let mut store = assigned(&workflows, p);
    let (_keep, mut cancel) = oneshot::channel();
    let (result, sends) = run(&mut store, "gateway", &mut cancel, &mut |_| Ok(())).await;
    result.unwrap();
    assert_eq!(sends, 2);
    assert_eq!(store.state().tasks["task"].state, TaskState::Submitted);
    assert_eq!(
        fs::read_to_string(temp.path().join("project/result.txt")).unwrap(),
        "fixture output"
    );
    CallInventory::read(&store).unwrap().check(None).unwrap();
    let root = store.api_attempt_path("attempt").unwrap();
    let mut original = Vec::new();
    for round in 1..=2 {
        let path = root.join(format!("rounds/{round:03}"));
        let journal = ApiCallJournal::read_existing(&path).unwrap();
        let review = journal.gateway_evidence(&root).unwrap().unwrap();
        assert_eq!(review.status, "linked");
        assert_eq!(
            review.gateway_request_id,
            Some(format!("gateway-driver-{round}"))
        );
        assert_eq!(review.local_id, journal.identity().request_id);
        assert_ne!(review.gateway_request_id, review.body_response_id);
        original.push(fs::read(path.join("01.json")).unwrap());
    }
    let store = reopen(store, &workflows);
    CallInventory::read(&store).unwrap().check(None).unwrap();
    for round in 1..=2 {
        assert_eq!(
            fs::read(root.join(format!("rounds/{round:03}/01.json"))).unwrap(),
            original[round - 1]
        );
    }
    assert_eq!(
        crate::data_format::status(&data)
            .unwrap()
            .minimum_reader_generation,
        16
    );
}
#[test]
fn api_selection_binds_target_limits_and_permissions_without_changing_old_json() {
    let dir = tempfile::tempdir().unwrap();
    let original = plan(dir.path(), selection(targets().remove(2)));
    let approval = Approval {
        plan_id: "plan".into(),
        revision: 1,
        policy_version: 1,
        revision_digest: original.approval_digest().unwrap(),
        user_id: "user".into(),
        approved_at: chrono::Utc::now().to_rfc3339(),
    };
    for change in [
        "model", "target", "rounds", "tokens", "output", "tools", "timeout",
    ] {
        let mut altered = original.clone();
        let worker = altered.execution[0].api_worker.as_mut().unwrap();
        match change {
            "model" => worker.model = "other".into(),
            "target" => worker.target = targets().remove(0),
            "rounds" => worker.max_rounds = 3,
            "tokens" => worker.max_total_tokens = 100,
            "output" => worker.max_output_tokens = 393216,
            "tools" => altered.policy.tools = vec![ToolPermission::ReadFiles],
            "timeout" => worker.request_timeout_seconds = 1,
            _ => unreachable!(),
        };
        assert!(approval.validate(&altered, "user", 1).is_err());
    }
    let mut invalid = original.clone();
    invalid.execution[0]
        .api_worker
        .as_mut()
        .unwrap()
        .file_tools
        .push(ToolPermission::RunCommands);
    assert!(invalid.validate().is_err());
    invalid = original.clone();
    invalid.policy.budget_usd_cents = Some(1);
    assert!(invalid.validate().is_err());
    let mut legacy = serde_json::to_value(&original.execution[0]).unwrap();
    legacy.as_object_mut().unwrap().remove("apiWorker");
    legacy["adapterId"] = json!("codex");
    let parsed: TaskExecution = serde_json::from_value(legacy.clone()).unwrap();
    assert!(parsed.api_worker.is_none());
    assert_eq!(serde_json::to_value(parsed).unwrap(), legacy);
}
#[tokio::test]
async fn provider_output_limits_keep_budget_journal_and_recovery_bound_to_approval() {
    // Real workflow/journal and file tool writes with synthetic provider replies.
    // This verifies local protocol support, not remote model availability.
    for (output, total) in [
        (393216, 10_000_000),
        (393216, 30000),
        (u32::MAX, 10_000_000),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let mut worker = selection(targets().remove(2));
        worker.max_output_tokens = output;
        worker.max_total_tokens = total;
        let mut store = assigned(dir.path(), plan(dir.path(), worker));
        let (_tx, mut rx) = oneshot::channel();
        let (result, sends) = run(&mut store, "ok", &mut rx, &mut |_| Ok(())).await;
        result.unwrap();
        assert_eq!(sends, 2);
        let root = store.api_attempt_path("attempt").unwrap();
        let mut original = Vec::new();
        for round in 1..=2 {
            let path = root.join(format!("rounds/{round:03}"));
            let journal = ApiCallJournal::read_existing(&path).unwrap();
            assert_eq!(
                journal.identity().fingerprint.max_output_tokens,
                output.min((total - 20 * (round - 1)) as u32)
            );
            for record in ["00.json", "01.json"] {
                original.push((path.join(record), fs::read(path.join(record)).unwrap()));
            }
        }
        CallInventory::read(&store).unwrap().check(None).unwrap();
        let mut store = reopen(store, dir.path());
        CallInventory::read(&store).unwrap().check(None).unwrap();
        assert_eq!(store.state().tasks["task"].state, TaskState::Submitted);
        let (result, sends) = run(&mut store, "ok", &mut rx, &mut |_| Ok(())).await;
        assert!(result.is_err());
        assert_eq!(sends, 0);
        for (path, bytes) in original {
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
    }
}
#[tokio::test]
async fn six_provider_loops_write_real_files_and_require_independent_acceptance() {
    for target in targets() {
        let dir = tempfile::tempdir().unwrap();
        let p = plan(dir.path(), selection(target));
        let mut store = assigned(dir.path(), p.clone());
        let (_tx, mut rx) = oneshot::channel();
        let (result, sends) = run(&mut store, "ok", &mut rx, &mut |_| Ok(())).await;
        result.unwrap();
        assert_eq!(sends, 2);
        assert_eq!(prior_requests(&store).unwrap(), 2);
        CallInventory::read(&store).unwrap().check(None).unwrap();
        let source = CallInventory::read(&store)
            .unwrap()
            .historical_source(&store, "attempt")
            .unwrap();
        assert_eq!(source.references.len(), 5);
        assert_eq!(
            CallInventory::read(&store)
                .unwrap()
                .historical_source(&store, "attempt")
                .unwrap(),
            source
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("project/result.txt")).unwrap(),
            "fixture output"
        );
        assert_eq!(store.state().tasks["task"].state, TaskState::Submitted);
        assert!(!store.state().tasks["task"].verified);
        let accept = WorkflowCommand::Accept {
            task: "task".into(),
            attempt: "attempt".into(),
        };
        assert!(record(
            &mut store,
            "chief",
            accept.clone(),
            CommandContext::default()
        )
        .is_err());
        let mut store = reopen(store, dir.path());
        let (result, sends) = run(&mut store, "ok", &mut rx, &mut |_| Ok(())).await;
        assert!(result.is_err());
        assert_eq!(sends, 0);
        if p.execution[0].adapter_id == "deepseek" {
            let pin = PinnedCheck::from_approved(&p.execution[0].checks[0]).unwrap();
            store
                .run_verification(
                    "api-independent-check",
                    "task",
                    "attempt",
                    &[pin],
                    &std::sync::atomic::AtomicBool::new(false),
                )
                .unwrap();
            assert!(store.state().tasks["task"].verified);
            record(&mut store, "chief", accept, CommandContext::default()).unwrap();
            assert_eq!(
                reopen(store, dir.path()).state().tasks["task"].state,
                TaskState::Accepted
            );
        }
    }
}
#[tokio::test]
async fn limits_errors_and_escape_stop_without_unapproved_writes() {
    for mode in [
        "tokens",
        "exact-tokens",
        "rounds",
        "project-requests",
        "read-only",
        "tool-limit",
        "provider-error",
        "escape",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let mut worker = selection(targets().remove(2));
        match mode {
            "tokens" => worker.max_total_tokens = 15,
            "exact-tokens" => worker.max_total_tokens = 20,
            "rounds" => worker.max_rounds = 1,
            "tool-limit" => worker.max_tool_calls = 1,
            "read-only" => worker.file_tools = vec![ToolPermission::ReadFiles],
            _ => {}
        }
        let mut p = plan(dir.path(), worker);
        if mode == "project-requests" {
            p.policy.max_requests = 1;
        }
        let mut store = assigned(dir.path(), p);
        let (_tx, mut rx) = oneshot::channel();
        let (result, sends) = run(&mut store, mode, &mut rx, &mut |_| Ok(())).await;
        assert!(result.is_err(), "{mode}");
        assert_eq!(sends, if mode == "escape" { 2 } else { 1 });
        assert_eq!(store.state().tasks["task"].state, TaskState::Unknown);
        CallInventory::read(&store).unwrap().check(None).unwrap();
        assert!(!dir.path().join("project/result.txt").exists());
        assert!(!dir.path().join("outside.txt").exists());
        let mut store = reopen(store, dir.path());
        assert!(recover_terminal(&mut store, "task", "attempt").is_err());
        let (result, sends) = run(&mut store, "ok", &mut rx, &mut |_| Ok(())).await;
        assert!(result.is_err());
        assert_eq!(sends, 0);
    }
}
#[tokio::test]
async fn cancel_after_file_result_keeps_change_and_does_not_start_next_round() {
    let dir = tempfile::tempdir().unwrap();
    let p = plan(dir.path(), selection(targets().remove(2)));
    let mut store = assigned(dir.path(), p);
    let (tx, mut rx) = oneshot::channel();
    let mut tx = Some(tx);
    let (result, sends) = run(&mut store, "ok", &mut rx, &mut |at| {
        if at == "tool-recorded" {
            tx.take().unwrap().send(()).unwrap();
        }
        Ok(())
    })
    .await;
    assert!(result.is_err());
    assert_eq!(sends, 1);
    assert_eq!(prior_requests(&store).unwrap(), 4);
    CallInventory::read(&store).unwrap().check(None).unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("project/result.txt")).unwrap(),
        "fixture output"
    );
    assert_eq!(
        reopen(store, dir.path()).state().tasks["task"].state,
        TaskState::Unknown
    );
}
#[tokio::test]
async fn overall_deadline_drops_inflight_request_and_retains_unknown_intent() {
    let dir = tempfile::tempdir().unwrap();
    let mut p = plan(dir.path(), selection(targets().remove(2)));
    p.policy.max_seconds = 1;
    p.execution[0].checks[0].timeout_seconds = 1;
    let mut store = assigned(dir.path(), p);
    let client =
        ApiClient::new(SecretInput::new("sk-synthetic-api-deadline".into()).unwrap()).unwrap();
    let (_tx, mut rx) = oneshot::channel();
    let mut sent = 0;
    let result = dispatch_inner(
        &mut store,
        &client,
        "task",
        "attempt",
        &mut rx,
        &mut async |_prepared: PreparedRequest, _cancel: &mut oneshot::Receiver<()>, _gateway: &mut cerulean_adapters::api_generation::GatewayObserver<'_>| {
            sent += 1;
            std::future::pending::<Result<ApiReply, ApiFailure>>().await
        },
        &mut |_| Ok(()),
        &mut |_| {},
    )
    .await;
    assert!(result.unwrap_err().contains("timed out"));
    assert_eq!(sent, 1);
    let root = store.api_attempt_path("attempt").unwrap();
    assert!(root.join("rounds/001/00.json").is_file());
    assert!(!root.join("rounds/001/01.json").exists());
    CallInventory::read(&store).unwrap().check(None).unwrap();
    assert!(!dir.path().join("project/result.txt").exists());
    assert_eq!(
        reopen(store, dir.path()).state().tasks["task"].state,
        TaskState::Unknown
    );
}
#[tokio::test]
async fn original_seal_recovers_without_sending_and_changed_evidence_rejects() {
    for mode in [
        "intact",
        "artifact",
        "round",
        "tool",
        "pending",
        "missing-round",
        "missing-lock",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let p = plan(dir.path(), selection(targets().remove(2)));
        let mut store = assigned(dir.path(), p);
        let (_tx, mut rx) = oneshot::channel();
        let (result, sends) = run(&mut store, "ok", &mut rx, &mut |at| {
            if at == "artifacts-sealed" {
                Err("injected stop".into())
            } else {
                Ok(())
            }
        })
        .await;
        assert!(result.is_err());
        assert_eq!(sends, 2);
        assert_eq!(store.state().tasks["task"].state, TaskState::Unknown);
        let root = store.api_attempt_path("attempt").unwrap();
        let config = store.configuration().clone();
        drop(store);
        match mode {
            "artifact" => fs::write(dir.path().join("project/result.txt"), "changed").unwrap(),
            "round" => fs::write(root.join("rounds/001/01.json"), b"{}").unwrap(),
            "tool" => fs::remove_file(root.join("files/events/001.json")).unwrap(),
            "pending" => fs::write(root.join("completion.json.pending"), b"partial").unwrap(),
            "missing-round" => {
                fs::rename(root.join("rounds/001"), dir.path().join("retained-round")).unwrap()
            }
            "missing-lock" => fs::rename(
                root.join("rounds/001/writer.lock"),
                dir.path().join("retained-lock"),
            )
            .unwrap(),
            _ => {}
        }
        // Reload the same recorded configuration, not a freshly approved plan.
        let mut store = DurableWorkflow::open(&dir.path().join("ledger"), config).unwrap();
        let result = recover_terminal(&mut store, "task", "attempt");
        if mode == "missing-round" {
            assert!(!root.join("rounds/001").exists());
        }
        if mode == "missing-lock" {
            assert!(!root.join("rounds/001/writer.lock").exists());
        }
        assert_eq!(result.is_ok(), mode == "intact", "{mode}: {result:?}");
        assert_eq!(
            store.state().tasks["task"].state,
            if mode == "intact" {
                TaskState::Submitted
            } else {
                TaskState::Unknown
            }
        );
    }
}
#[test]
#[ignore = "invoked as an isolated approved independent check"]
fn artifact_validator() {
    assert_eq!(fs::read_to_string("result.txt").unwrap(), "fixture output");
}

#[tokio::test]
async fn api_tool_supervision_replays_real_file_failures_without_new_dispatch_or_acceptance() {
    use sha2::Digest;
    let dir = tempfile::tempdir().unwrap();
    let target = ApiTarget::Provider {
        endpoint: ProviderEndpoint::DeepSeek {},
    };
    let p = plan(dir.path(), selection(target));
    let mut store = assigned(dir.path(), p.clone());
    let (_tx, mut rx) = oneshot::channel();
    let (result, sends) = run(&mut store, "read-failures", &mut rx, &mut |_| Ok(())).await;
    result.unwrap();
    assert_eq!(sends, 2);
    assert_eq!(
        fs::read_to_string(Path::new(&p.project.root).join("result.txt")).unwrap(),
        "fixture output"
    );
    assert_eq!(store.state().tasks["task"].state, TaskState::Submitted);
    assert!(!store.state().tasks["task"].verified);
    let sequence = store.history().len();
    let root = store.api_attempt_path("attempt").unwrap();
    let identity: ApiRunIdentity = load(&root, "identity.json").unwrap();
    identity.validate_plan(&p).unwrap();
    let path = root.join("files/events");
    let before = fs::read_dir(&path)
        .unwrap()
        .filter(|entry| entry.as_ref().unwrap().file_name() != "writer.lock")
        .map(|e| {
            let file = e.unwrap().path();
            let bytes = fs::read(&file).unwrap();
            (file, bytes)
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let journal = OmpToolJournal::open(&path, &identity).unwrap();
    let policy = cerulean_domain::supervision::SupervisionPolicy {
        schema_version: 1,
        repeated_failure_threshold: 2,
        max_advice_per_attempt: 2,
        delivery_enabled: false,
    };
    let incidents = journal.supervision_incidents(&policy).unwrap();
    assert_eq!(incidents.len(), 1);
    assert_eq!(
        (incidents[0].first_sequence, incidents[0].trigger_sequence),
        (2, 4)
    );
    for (id, error) in [
        ("api-tool-1", true),
        ("api-tool-2", true),
        ("api-tool-3", false),
    ] {
        assert_eq!(journal.observation().calls[id].result_is_error, Some(error));
        if error {
            assert_eq!(
                journal.observation().calls[id].result_sha256,
                Some(format!(
                    "{:x}",
                    sha2::Sha256::digest(b"File unavailable or concurrently modified")
                ))
            );
        }
    }
    drop(journal);
    let reopened = OmpToolJournal::open(&path, &identity).unwrap();
    assert_eq!(reopened.supervision_incidents(&policy).unwrap(), incidents);
    for (path, bytes) in before {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
    assert_eq!(store.history().len(), sequence);
    assert_eq!(store.state().tasks["task"].state, TaskState::Submitted);
    assert!(p.supervision_for("task").unwrap().is_none());
}
