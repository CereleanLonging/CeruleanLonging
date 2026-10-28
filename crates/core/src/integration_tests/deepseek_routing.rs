//! Full product run entry, real tools/verifiers; both providers remain controlled.
use super::*;
use crate::management_execution::DecisionIntent;
use cerulean_adapters::deputy_management::{self, DeputyResult, PreparedDeputy};

pub(in crate::host) fn deputy(prepared: &PreparedDeputy) -> DeputyResult {
    let request = prepared.request();
    let h = &request.handoff;
    let intent = DecisionIntent {
        operation_id: h.operation_id.clone(),
        project_id: h.project_id.clone(),
        purpose: h.purpose,
        model: request.wire.requested_model.clone(),
        state_digest: h.state_digest.clone(),
        context: h.context.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    let envelope = if h.purpose == Purpose::Advise {
        DecisionEnvelope {
            state_digest: h.state_digest.clone(),
            decision: ManagementDecision::Advise {
                task_id: h.context["supervision"]["taskId"].as_str().unwrap().into(),
                suggestion: "Inspect the approved task instead of repeating the failed operation."
                    .into(),
                rationale: "Original repeated tool failure.".into(),
            },
        }
    } else {
        manager(intent).unwrap().envelope
    };
    if h.purpose == Purpose::Assign && h.context.get("assignmentBindings").is_some() {
        let ManagementDecision::Assign {
            task_id, worker_id, ..
        } = &envelope.decision
        else {
            panic!("assignment")
        };
        let entries = h.context["assignmentBindings"]["tasks"].as_array().unwrap();
        assert!(entries
            .iter()
            .any(|entry| entry["taskId"] == *task_id && entry["workerId"] == *worker_id));
    }
    deputy_management::fixture_result(
        prepared,
        json!({"id":"ds-controlled-route","model":request.wire.requested_model,"usage":{"prompt_tokens":17,"completion_tokens":5},"choices":[{"index":0,"finish_reason":"stop","message":{"role":"assistant","content":serde_json::to_string(&envelope).unwrap()}}]}),
    )
}
fn supervised_worker(request: &PreparedRequest, round: u32) -> Result<ApiReply, ApiFailure> {
    parallel::wait_gate(request)?;
    if round != 1 {
        return worker(request, round - 1);
    }
    fixture_reply(request, json!({"id":"controlled-failures","model":"fixture","usage":{"prompt_tokens":10,"completion_tokens":10},"choices":[{"index":0,"finish_reason":"tool_calls","message":{"role":"assistant","content":null,"tool_calls":[
        {"id":"failure-1","type":"function","function":{"name":"cerulean_read_file","arguments":"{\"path\":\"missing.txt\",\"offset\":0}"}},
        {"id":"failure-2","type":"function","function":{"name":"cerulean_read_file","arguments":"{\"path\":\"missing.txt\",\"offset\":0}"}}
    ]}}]})).map_err(|_| ApiFailure::InvalidResponse {})
}
#[test]
#[ignore = "real Git and independent verifier; full configured Codex/DeepSeek route without OpenAI credentials"]
fn deepseek_routing_complete_serial_parallel_and_api_advice_with_external_accept() {
    for (parallel, advice) in [(false, false), (true, false), (false, true), (true, true)] {
        let root = temporary();
        let mut host = external_accept::configured_source(root.path(), parallel, true, advice);
        if advice {
            host.api_fixture = Some(supervised_worker);
        }
        if parallel {
            let (a_wait, a_release) = parallel::gate("a");
            let (b_wait, b_release) = parallel::gate("b");
            spin(&mut host, |_| a_wait.try_recv().is_ok());
            spin(&mut host, |_| b_wait.try_recv().is_ok());
            assert_eq!(host.api_active.len(), 2);
            a_release.send(()).unwrap();
            b_release.send(()).unwrap();
        }
        assert!(!host.openai_connection.key_configured);
        assert!(crate::credential_vault::load(&host.root).is_err());
        assert!(host.deepseek_key_configured);
        let count = if parallel { 2 } else { 1 };
        for _ in 0..count {
            let request = external_accept::waiting(&mut host);
            let reply = external_accept::response(&host, &request);
            host.submit_external_chief_decision(&request.operation_id, "current-session", reply)
                .unwrap();
        }
        spin(&mut host, |h| h.runs.all()[0].state == RunState::Completed);
        let statuses = host.management_decisions();
        let assigned = statuses
            .iter()
            .filter(|s| s.purpose == Purpose::Assign)
            .count();
        let advised = statuses
            .iter()
            .filter(|s| s.purpose == Purpose::Advise)
            .count();
        assert_eq!(assigned, count);
        assert_eq!(advised, usize::from(advice) * count);
        for status in statuses
            .iter()
            .filter(|s| matches!(s.purpose, Purpose::Assign | Purpose::Advise))
        {
            assert!(host.management.is_deputy(&status.operation_id));
            assert!(host.management.observation(&status.operation_id).is_err());
            let (_, result, _) = host.read_deepseek_management(&status.operation_id).unwrap();
            assert_eq!(result.unwrap().billing().unwrap().usage.input_tokens, 17);
            if status.purpose == Purpose::Advise {
                assert_eq!(
                    host.api_advice_delivery_state(
                        host.management.intent(&status.operation_id).unwrap()
                    )
                    .unwrap(),
                    "responseObserved"
                );
            }
        }
        let history = host.projects["project"].history().to_vec();
        let operations = host.management_operation_total();
        assert!(host.api_active.is_empty() && host.management_active.is_empty());
        drop(host);
        let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
        host.poll_verifications();
        assert_eq!(host.runs.all()[0].state, RunState::Completed);
        assert_eq!(host.management_operation_total(), operations);
        assert_eq!(host.projects["project"].history(), history);
        assert!(!host.openai_connection.key_configured);
        println!("DeepSeek routed parallel={parallel}, advice={advice}: assignments={assigned}, advice={advised}, completed/reopened, no OpenAI credential or model fallback, realProviderCalls=0");
    }
}

#[test]
#[ignore = "real configured product runner; no model request; replays the 216 adapter-ID mistake"]
fn deepseek_assignment_rejects_adapter_id_without_rewriting_or_retry() {
    fn bad(prepared: &PreparedDeputy) -> DeputyResult {
        let h = prepared.request().handoff;
        let binding = &h.context["assignmentBindings"]["tasks"][0];
        assert_eq!(binding["workerId"], "worker-deepseek");
        assert_eq!(binding["adapterId"], "deepseek");
        deputy_management::fixture_result(
            prepared,
            json!({"id":"controlled-216-mistake","model":"deepseek-flash","usage":{"prompt_tokens":5145,"completion_tokens":322},"choices":[{"index":0,"finish_reason":"stop","message":{"role":"assistant","content":json!({"stateDigest":h.state_digest,"decision":{"action":"assign","taskId":binding["taskId"],"workerId":"deepseek","rationale":"Adapter ID incorrectly used as execution identity"}}).to_string()}}]}),
        )
    }
    let root = temporary();
    let mut host = external_accept::configured_source(root.path(), false, true, false);
    host.deputy_management_fixture = Some(bad);
    spin(&mut host, |h| h.runs.all()[0].state == RunState::Attention);
    assert!(host.runs.all()[0].message.contains("未配置的执行器"));
    assert!(host.snapshot().api_jobs.is_empty());
    assert!(host.api_active.is_empty());
    let total = host.management_operation_total();
    for _ in 0..3 {
        host.poll_verifications();
    }
    assert_eq!(host.management_operation_total(), total);
    drop(host);
    let mut reopened = RuntimeHost::open(&root.path().join("data")).unwrap();
    reopened.poll_verifications();
    assert_eq!(reopened.management_operation_total(), total);
    assert_eq!(reopened.runs.all()[0].state, RunState::Attention);
    assert!(reopened.snapshot().api_jobs.is_empty());
}
