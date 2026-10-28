//! Full approved API pipeline, real file tools and independent validator
//! processes. Only the management/provider transports are synthetic.
use super::*;

#[path = "api_supervision_crash_tests.rs"]
mod crash;

const ADVICE: &str =
    "Create result.txt for the approved DAG_A task; stop repeating missing-file reads.";

fn supervised_manager(
    i: crate::management_execution::DecisionIntent,
) -> Result<DecisionObservation, String> {
    if i.purpose != Purpose::Advise {
        return manager(i);
    }
    // Exercise ordinary polling while the original management request is
    // still in flight, before a durable response can be observed.
    std::thread::sleep(Duration::from_millis(25));
    assert_eq!(i.context["supervision"]["taskId"], "a");
    assert_eq!(
        i.context["supervisionFailureContext"]["failures"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let decision = if i.context["requirement"]
        .as_str()
        .unwrap()
        .contains("SUPERVISION_WAIT")
    {
        ManagementDecision::Wait {
            reason: "Review the original missing files before continuing.".into(),
        }
    } else {
        ManagementDecision::Advise {
            task_id: "a".into(),
            suggestion: ADVICE.into(),
            rationale: "Two matching original file failures.".into(),
        }
    };
    Ok(DecisionObservation {
        response_id: format!("resp_{}", i.operation_id.replace('-', "_")),
        requested_model: i.model.clone(),
        actual_model: i.model,
        input_tokens: 1,
        output_tokens: 1,
        envelope: DecisionEnvelope {
            state_digest: i.state_digest,
            decision,
        },
    })
}

fn supervised_worker(prepared: &PreparedRequest, round: u32) -> Result<ApiReply, ApiFailure> {
    let messages = fixture_messages(prepared);
    let ApiMessage::User(prompt) = &messages[0] else {
        panic!("approved task prompt missing")
    };
    let advice_count = messages
        .iter()
        .filter(|m| matches!(m, ApiMessage::User(s) if s.contains(ADVICE)))
        .count();
    if prompt.contains("DAG_B") {
        assert_eq!(
            advice_count, 0,
            "advice must not leak into dependent task B"
        );
        return worker(prepared, round);
    }
    if round > 1 {
        assert_eq!(
            advice_count, 1,
            "original advice must be present exactly once"
        );
        return worker(prepared, round - 1);
    }
    assert_eq!(advice_count, 0);
    let args = json!({"path":"missing.txt","offset":0});
    let value = match &prepared.fingerprint().target {
        ApiTarget::OpenAi {} => json!({"id":"resp_failures","model":"fixture","status":"completed",
            "output":(["failure-1","failure-2"].map(|id| json!({"type":"function_call","call_id":id,"name":"cerulean_read_file","arguments":args.to_string()}))),
            "usage":{"input_tokens":10,"output_tokens":10}}),
        target if target.provider() == CredentialProvider::Anthropic => {
            json!({"id":"msg_failures","type":"message","role":"assistant","model":"fixture","stop_reason":"tool_use",
            "content":(["failure-1","failure-2"].map(|id| json!({"type":"tool_use","id":id,"name":"cerulean_read_file","input":args}))),
            "usage":{"input_tokens":10,"output_tokens":10}})
        }
        _ => {
            json!({"id":"chat_failures","model":"fixture","choices":[{"index":0,"finish_reason":"tool_calls","message":{"role":"assistant","content":null,
            "tool_calls":(["failure-1","failure-2"].map(|id| json!({"id":id,"type":"function","function":{"name":"cerulean_read_file","arguments":args.to_string()}})))}}],
            "usage":{"prompt_tokens":10,"completion_tokens":10}})
        }
    };
    fixture_reply(prepared, value).map_err(|_| ApiFailure::InvalidResponse {})
}

fn attach_supervised(host: &mut RuntimeHost) {
    host.management_fixture = Some(supervised_manager);
    host.api_fixture = Some(supervised_worker);
}

fn exercise(
    root: &Path,
    adapter: ProjectWorkerAdapter,
    target: ApiTarget,
    mode: &str,
) -> serde_json::Value {
    let provider = target.provider();
    let mut host = configured(root, adapter, target.clone(), mode, 8);
    attach_supervised(&mut host);
    approved(&mut host);
    start(&mut host);
    spin(&mut host, |h| {
        h.runs.all()[0].state != RunState::Active && h.api_active.is_empty()
    });
    let success = mode == "SUPERVISION_SUCCESS";
    assert_eq!(
        host.runs.all()[0].state,
        if success {
            RunState::Completed
        } else {
            RunState::Attention
        },
        "{:?}",
        host.runs.all()
    );
    assert!(host.management_active.is_empty());
    let statuses = host.management.statuses();
    let advice = statuses
        .iter()
        .filter(|s| s.purpose == Purpose::Advise)
        .collect::<Vec<_>>();
    assert_eq!(advice.len(), 1);
    let operation = advice[0].operation_id.clone();
    let intent = host.management.intent(&operation).unwrap().clone();
    let store = &host.projects["project"];
    let history = store.history().to_vec();
    let advise_events = history
        .iter()
        .filter(|e| matches!(e.body.event.command, WorkflowCommand::Advise { .. }))
        .count();
    assert_eq!(advise_events, usize::from(mode != "SUPERVISION_WAIT"));
    let state = host.api_advice_delivery_state(&intent).unwrap();
    assert_eq!(
        state,
        if mode == "SUPERVISION_WAIT" {
            "awaitingApplication"
        } else {
            "responseObserved"
        }
    );
    if success {
        assert_eq!(
            fs::read_to_string(root.join("project/result.txt")).unwrap(),
            "alpha beta"
        );
        let sequence = |task: &str, kind: &str| {
            history
                .iter()
                .find(|e| {
                    e.body.event.command.task() == Some(task)
                        && serde_json::to_value(&e.body.event.command).unwrap()["kind"] == kind
                })
                .unwrap()
                .body
                .event
                .sequence
        };
        for task in ["a", "b"] {
            assert_eq!(store.state().tasks[task].state, TaskState::Accepted);
            assert!(sequence(task, "submit") < sequence(task, "verify"));
            assert!(sequence(task, "verify") < sequence(task, "accept"));
        }
        assert!(sequence("a", "advise") < sequence("a", "submit"));
        assert!(sequence("a", "accept") < sequence("b", "assign"));
        assert_eq!(host.jobs.len(), 2);
        assert!(host
            .jobs
            .values()
            .all(|j| j.state == VerificationState::Succeeded));
        assert_eq!(statuses.len(), 6);
        assert_eq!(crate::api_execution::prior_requests(store).unwrap(), 6);
    } else {
        assert_eq!(store.state().tasks["b"].state, TaskState::Pending);
        assert!(!history
            .iter()
            .any(|e| matches!(e.body.event.command, WorkflowCommand::Accept { .. })));
        assert!(host.api_startups.values().all(|s| s.attempt.task_id == "a"));
        if mode == "SUPERVISION_BUDGET" {
            assert_eq!(host.jobs.len(), 1);
            assert!(host
                .jobs
                .values()
                .all(|j| j.state == VerificationState::Succeeded));
            assert_eq!(store.state().tasks["a"].state, TaskState::Submitted);
        } else if mode == "SUPERVISION_FAIL_VALIDATOR" {
            assert_eq!(host.jobs.len(), 1);
            assert!(host
                .jobs
                .values()
                .all(|j| j.state == VerificationState::Failed));
        } else {
            assert!(host.jobs.is_empty());
        }
        assert_eq!(statuses.len(), 3);
    }
    let jobs = host.api_jobs.len();
    let operations = host.management_operation_ids("project");
    host.validate_api_supervision_requests().unwrap();
    let workflow_digest = digest(&history).unwrap();
    let result = json!({"mode":mode,"target":target,"state":host.runs.all()[0].state,"managementRequests":statuses.len(),
        "apiAttempts":jobs,"independentChecks":host.jobs.len(),"adviceEvents":advise_events,"deliveryState":state,
        "workflowDigest":workflow_digest,"adviceOperation":operation,"realProviderCalls":0});
    // This is isolated synthetic test authentication, not a user credential.
    host.set_openai_key(None).unwrap();
    if provider != CredentialProvider::OpenAi {
        host.set_provider_key(provider, None).unwrap();
    }
    drop(host);
    let mut reopened = RuntimeHost::open(&root.join("data")).unwrap();
    attach_supervised(&mut reopened);
    for _ in 0..5 {
        reopened.poll_verifications();
    }
    assert_eq!(reopened.projects["project"].history(), history.as_slice());
    assert_eq!(reopened.api_jobs.len(), jobs);
    assert_eq!(reopened.management_operation_ids("project"), operations);
    assert_eq!(reopened.api_advice_delivery_state(&intent).unwrap(), state);
    assert!(reopened.api_active.is_empty() && reopened.management_active.is_empty());
    assert!(!reopened.openai_connection.key_configured);
    let evidence = root.join("pipeline-result.json");
    crate::api_execution::publish(root, "pipeline-result.json", &result).unwrap();
    assert!(evidence.exists());
    result
}

#[test]
#[ignore = "explicit isolated retained report path; eight API routes and independent validator processes"]
fn api_supervision_programming_pipeline_verifies_accepts_and_reopens_without_replay() {
    let root = PathBuf::from(
        std::env::var_os("API_SUPERVISION_DAG_ROOT").expect("explicit new fixture root"),
    );
    assert!(root.is_absolute());
    fs::create_dir(&root).unwrap();
    let mut results = Vec::new();
    for (n, (adapter, target)) in api_project_tests::routes().into_iter().enumerate() {
        let case = root.join(format!("success-{n}"));
        fs::create_dir(&case).unwrap();
        results.push(exercise(&case, adapter, target, "SUPERVISION_SUCCESS"));
    }
    for mode in [
        "SUPERVISION_FAIL_VALIDATOR",
        "SUPERVISION_BUDGET",
        "SUPERVISION_WAIT",
    ] {
        let case = root.join(mode);
        fs::create_dir(&case).unwrap();
        results.push(exercise(
            &case,
            ProjectWorkerAdapter::Openai,
            ApiTarget::OpenAi {},
            mode,
        ));
    }
    crate::api_execution::publish(&root, "result.json", &json!({"cases":results,"realProviderCalls":0,
        "scope":"original Host, approved API workers, real file tools and independent processes; synthetic model transports"})).unwrap();
}
