//! Actual worker, workflow owner and file tools; only provider transport is synthetic.
use super::*;
use crate::{api_tool_checkpoint::Request, async_api::Update};
use cerulean_adapters::api_generation::{fixture_reply, ApiFailure, ApiReply, PreparedRequest};
use cerulean_domain::{
    api_jobs::ApiJobState, api_worker::ApiTarget, supervision::SupervisionPolicy,
};
use serde_json::json;
use sha2::Digest;
use std::time::Duration;

fn policy() -> SupervisionPolicy {
    SupervisionPolicy {
        schema_version: 1,
        repeated_failure_threshold: 2,
        max_advice_per_attempt: 2,
        delivery_enabled: false,
    }
}
pub(in crate::host) fn response(
    prepared: &PreparedRequest,
    round: u32,
) -> Result<ApiReply, ApiFailure> {
    let output = match round {
        1 => json!(["10", "2"].map(|id| json!({"type":"function_call","call_id":id,
            "name":"cerulean_read_file","arguments":json!({"path":"missing.txt","offset":0}).to_string()}))),
        2 => json!([{"type":"function_call","call_id":"write-result","name":"cerulean_write_file",
            "arguments":json!({"path":"result.txt","content":"checkpoint fixture","expectedSha256":"absent"}).to_string()}]),
        _ => json!([{"type":"message","id":"msg_done","role":"assistant","status":"completed",
            "content":[{"type":"output_text","text":"Ready for independent verification."}]}]),
    };
    fixture_reply(
        prepared,
        json!({"id":format!("resp-{round}"),"model":"fixture","status":"completed",
        "output":output,"usage":{"input_tokens":10,"output_tokens":10}}),
    )
    .map_err(|_| ApiFailure::InvalidResponse {})
}
fn start(root: &Path) -> RuntimeHost {
    let mut host = api_host_tests::fixture_with_revision(
        root,
        project_setup::ProjectWorkerAdapter::Openai,
        ApiTarget::OpenAi {},
        true,
        |_, plan| plan.policy.max_requests = 3,
    );
    host.api_fixture = Some(response);
    host.start_managed_api("deputy-api", "attempt").unwrap();
    host
}
fn key() -> (String, String) {
    ("project".into(), "attempt".into())
}
fn attempt_root(host: &RuntimeHost) -> PathBuf {
    host.root
        .join("workflows/project")
        .join(format!("api-attempt-{}", digest(&"attempt").unwrap()))
}
// Consume the original owner channel, holding just the tool ACK. Assignment and
// progress use the same handlers/state transitions as poll_api.
pub(in crate::host) fn next_checkpoint(host: &mut RuntimeHost) -> Request {
    loop {
        match host.api_active[&key()]
            .receiver
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
        {
            Update::Assignment(crate::managed_assignment::Update::Prepare {
                operation,
                attempt,
                snapshot,
                reply,
            }) => {
                reply
                    .send(host.prepare_managed_assignment(
                        "project", &operation, *attempt, *snapshot, "openai",
                    ))
                    .unwrap();
            }
            Update::Assignment(crate::managed_assignment::Update::Commit {
                operation,
                entry,
                snapshot,
                reply,
            }) => {
                reply
                    .send(host.commit_managed_assignment(
                        "project", &operation, *entry, *snapshot, "openai",
                    ))
                    .unwrap();
            }
            Update::Progress(snapshot) => {
                let active = host.api_active.get_mut(&key()).unwrap();
                active.snapshot = *snapshot;
                active.job.state = ApiJobState::Executing;
                host.api_jobs.insert(key(), active.job.clone());
            }
            Update::Currency(request) => host.handle_api_currency("project", "attempt", *request),
            Update::ToolCheckpoint(request) => return *request,
            Update::Done { outcome, .. } => panic!("worker ended before checkpoint: {outcome:?}"),
            Update::WorkspaceReady { .. } => panic!("unexpected worktree fixture"),
        }
    }
}
fn assert_waiting(host: &RuntimeHost, round: u32) {
    assert!(matches!(
        host.api_active[&key()]
            .receiver
            .recv_timeout(Duration::from_millis(80)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout)
    ));
    assert!(!attempt_root(host)
        .join("rounds")
        .join(format!("{:03}", round + 1))
        .exists());
    assert!(!attempt_root(host).join("completion.json").exists());
}

#[test]
fn api_failure_context_tracks_cross_round_errors_and_reopens_without_success_contents() {
    fn split_response(prepared: &PreparedRequest, round: u32) -> Result<ApiReply, ApiFailure> {
        if round > 2 {
            return response(prepared, round - 1);
        }
        let id = if round == 1 { "10" } else { "2" };
        fixture_reply(
            prepared,
            json!({"id":format!("resp_split_{round}"),"model":"fixture","status":"completed",
            "output":[{"type":"function_call","call_id":id,"name":"cerulean_read_file",
                "arguments":json!({"path":"missing.txt","offset":0}).to_string()}],
            "usage":{"input_tokens":10,"output_tokens":10}}),
        )
        .map_err(|_| ApiFailure::InvalidResponse {})
    }
    let temp = tempfile::tempdir().unwrap();
    let mut host = api_host_tests::fixture_with_configuration(
        temp.path(),
        project_setup::ProjectWorkerAdapter::Openai,
        ApiTarget::OpenAi {},
        true,
        |draft| {
            draft.worker_max_requests = 4;
            draft.api_worker.as_mut().unwrap().max_rounds = 4;
        },
        |_, _| {},
    );
    host.api_fixture = Some(split_response);
    host.start_managed_api("deputy-api", "attempt").unwrap();
    let first = next_checkpoint(&mut host);
    assert!(first
        .checkpoint
        .tools
        .supervision_incidents(&policy())
        .unwrap()
        .is_empty());
    host.handle_api_tool_checkpoint("project", "attempt", first);
    let second = next_checkpoint(&mut host);
    let evidence = second.checkpoint.evidence();
    let root = attempt_root(&host);
    let incident = second
        .checkpoint
        .tools
        .supervision_incidents(&policy())
        .unwrap()
        .remove(0);
    let context = evidence
        .failure_context(&root, &policy(), &incident.id, &second.checkpoint.errors)
        .unwrap();
    let json = serde_json::to_value(&context).unwrap();
    assert_eq!(json["failures"].as_array().unwrap().len(), 2);
    assert_eq!(json["failures"][0]["callId"], "10");
    assert_eq!(json["failures"][1]["callId"], "2");
    for failure in json["failures"].as_array().unwrap() {
        assert_eq!(failure["path"], "missing.txt");
        assert_eq!(
            failure["error"],
            "File unavailable or concurrently modified"
        );
        assert!(failure.get("content").is_none());
    }
    // Missing/oversized diagnostic text is represented explicitly as unavailable.
    let unavailable = evidence
        .failure_context(&root, &policy(), &incident.id, &Default::default())
        .unwrap();
    assert!(serde_json::to_value(unavailable).unwrap()["failures"][0]["error"].is_null());
    host.handle_api_tool_checkpoint("project", "attempt", second);
    let third = next_checkpoint(&mut host);
    assert_eq!(third.checkpoint.errors.len(), 2);
    assert!(!third.checkpoint.errors.contains_key("write-result"));
    host.handle_api_tool_checkpoint("project", "attempt", third);
    api_host_tests::drain(&mut host);
    assert_eq!(host.api_jobs[&key()].state, ApiJobState::Submitted);
    assert_eq!(
        fs::read_to_string(temp.path().join("project/result.txt")).unwrap(),
        "checkpoint fixture"
    );
    drop(host);
    let reopened = RuntimeHost::open(&temp.path().join("data")).unwrap();
    evidence.verify_original(&root).unwrap();
    context
        .verify(&evidence, &root, &policy(), &incident.id)
        .unwrap();
    assert_eq!(
        reopened.projects["project"].state().tasks["task"].state,
        TaskState::Submitted
    );
}

#[test]
fn two_real_tool_batches_wait_for_owner_ack_and_replay_the_same_incident() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = start(temp.path());
    let mut first = next_checkpoint(&mut host);
    assert_eq!(first.checkpoint.call.round, 1);
    assert_waiting(&host, 1);
    let incidents = first
        .checkpoint
        .tools
        .supervision_incidents(&policy())
        .unwrap();
    assert_eq!(incidents.len(), 1);
    for call in first.checkpoint.tools.observation().calls.values() {
        assert_eq!(call.result_is_error, Some(true));
        assert_eq!(
            call.result_sha256.as_ref().unwrap(),
            &format!(
                "{:x}",
                sha2::Sha256::digest(b"File unavailable or concurrently modified")
            )
        );
    }
    let journal = ApiCallJournal::read_existing(&attempt_root(&host).join("rounds/001")).unwrap();
    assert_eq!(first.checkpoint.errors.len(), 2);
    let original_error = first.checkpoint.errors["10"].clone();
    for text in [
        "Invented failure".into(),
        "x".repeat(crate::api_tool_checkpoint::MAX_ERROR_BYTES + 1),
    ] {
        first.checkpoint.errors.insert("10".into(), text);
        assert!(first.checkpoint.validate(None, &journal).is_err());
    }
    first.checkpoint.errors.insert("10".into(), original_error);
    first.checkpoint.validate(None, &journal).unwrap();
    assert!(first
        .checkpoint
        .validate(Some(&first.checkpoint), &journal)
        .is_err());
    drop(journal);
    host.handle_api_tool_checkpoint("project", "attempt", first);
    let mut second = next_checkpoint(&mut host);
    assert_eq!(second.checkpoint.call.round, 2);
    assert_waiting(&host, 2);
    assert_eq!(
        fs::read_to_string(temp.path().join("project/result.txt")).unwrap(),
        "checkpoint fixture"
    );
    let journal = ApiCallJournal::read_existing(&attempt_root(&host).join("rounds/002")).unwrap();
    let previous = host.api_active[&key()].tool_checkpoint.as_ref();
    second.checkpoint.validate(previous, &journal).unwrap();
    assert_eq!(second.checkpoint.errors.len(), 2);
    assert!(!second.checkpoint.errors.contains_key("write-result"));
    second
        .checkpoint
        .errors
        .insert("write-result".into(), "checkpoint fixture".into());
    assert!(
        second.checkpoint.validate(previous, &journal).is_err(),
        "success content is not failure evidence"
    );
    second.checkpoint.errors.remove("write-result");
    assert!(second.checkpoint.validate(None, &journal).is_err());
    let head = second.checkpoint.call_head.clone();
    second.checkpoint.call_head = "0".repeat(64);
    assert!(second.checkpoint.validate(previous, &journal).is_err());
    second.checkpoint.call_head = head;
    second.checkpoint.run.fence += 1;
    assert!(second.checkpoint.validate(previous, &journal).is_err());
    second.checkpoint.run.fence -= 1;
    assert_eq!(
        second
            .checkpoint
            .tools
            .supervision_incidents(&policy())
            .unwrap(),
        incidents
    );
    let run = second.checkpoint.run.clone();
    let tool_head = second.checkpoint.tools.head().to_owned();
    drop(journal);
    host.handle_api_tool_checkpoint("project", "attempt", second);
    api_host_tests::drain(&mut host);
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Submitted
    );
    let root = attempt_root(&host);
    let tools =
        crate::omp_tool_journal::OmpToolJournal::open(&root.join("files/events"), &run).unwrap();
    assert_eq!(tools.head(), tool_head);
    assert_eq!(tools.supervision_incidents(&policy()).unwrap(), incidents);
    assert!(
        tools.checkpoint().is_err(),
        "reopened reader cannot impersonate original live owner"
    );
    drop(tools);
    drop(host);
    let mut reopened = RuntimeHost::open(&temp.path().join("data")).unwrap();
    reopened.poll_verifications();
    assert!(reopened.api_active.is_empty());
    assert_eq!(
        reopened.snapshot().api_jobs[0].state,
        ApiJobState::Submitted
    );
}

#[test]
fn lost_ack_cancel_or_changed_authority_preserves_file_and_stops_without_resend() {
    for mode in ["lost", "cancel", "authority", "wrong-head", "wrong-owner"] {
        let temp = tempfile::tempdir().unwrap();
        let mut host = start(temp.path());
        let first = next_checkpoint(&mut host);
        host.handle_api_tool_checkpoint("project", "attempt", first);
        let mut second = next_checkpoint(&mut host);
        let root = attempt_root(&host);
        let run = second.checkpoint.run.clone();
        let head = second.checkpoint.tools.head().to_owned();
        assert_waiting(&host, 2);
        match mode {
            "lost" => drop(second.reply),
            "cancel" => {
                host.cancel_api("project", "attempt", "stop at original tool checkpoint")
                    .unwrap();
                host.handle_api_tool_checkpoint("project", "attempt", second);
            }
            "authority" => {
                host.runner_error = Some("fixture project control needs recovery".into());
                host.handle_api_tool_checkpoint("project", "attempt", second);
            }
            "wrong-head" => {
                second.checkpoint.call_head = "0".repeat(64);
                host.handle_api_tool_checkpoint("project", "attempt", second);
            }
            _ => host.handle_api_tool_checkpoint("other", "attempt", second),
        }
        api_host_tests::drain(&mut host);
        assert_eq!(
            host.snapshot().api_jobs[0].state,
            ApiJobState::Unknown,
            "{mode}"
        );
        assert!(!root.join("rounds/003").exists(), "{mode}");
        assert!(!root.join("completion.json").exists(), "{mode}");
        assert_eq!(
            fs::read_to_string(temp.path().join("project/result.txt")).unwrap(),
            "checkpoint fixture"
        );
        let tools = crate::omp_tool_journal::OmpToolJournal::open(&root.join("files/events"), &run)
            .unwrap();
        assert_eq!(tools.head(), head);
        assert_eq!(tools.observation().calls.len(), 3);
        drop(tools);
        drop(host);
        let mut reopened = RuntimeHost::open(&temp.path().join("data")).unwrap();
        reopened.poll_verifications();
        assert!(reopened.api_active.is_empty());
        assert_eq!(reopened.snapshot().api_jobs[0].state, ApiJobState::Unknown);
        assert!(reopened
            .start_managed_api("deputy-api", "replacement")
            .is_err());
        assert!(!root.join("rounds/003").exists());
    }
}

#[test]
fn held_ack_times_out_with_original_tool_effects_and_no_followup_request() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = start(temp.path());
    let first = next_checkpoint(&mut host);
    host.handle_api_tool_checkpoint("project", "attempt", first);
    let held = next_checkpoint(&mut host);
    let root = attempt_root(&host);
    let limit = std::time::Instant::now() + Duration::from_secs(40);
    // Keep the ACK sender alive so this specifically exercises its timeout,
    // not channel loss. poll_api has no copy of the intercepted request.
    while !host.api_active.is_empty() && std::time::Instant::now() < limit {
        host.poll_verifications();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(host.api_active.is_empty());
    assert!(held.reply.is_closed());
    let job = &host.api_jobs[&key()];
    assert_eq!(job.state, ApiJobState::Unknown);
    assert!(job
        .message
        .as_ref()
        .unwrap()
        .contains("acknowledgement timed out"));
    assert!(!root.join("rounds/003").exists());
    assert!(!root.join("completion.json").exists());
    assert_eq!(
        fs::read_to_string(temp.path().join("project/result.txt")).unwrap(),
        "checkpoint fixture"
    );
    let journal = crate::omp_tool_journal::OmpToolJournal::open(
        &root.join("files/events"),
        &held.checkpoint.run,
    )
    .unwrap();
    assert_eq!(journal.head(), held.checkpoint.tools.head());
    assert!(journal.checkpoint().is_err());
}

#[test]
fn portable_api_checkpoint_replays_original_rounds_after_owner_and_host_restart() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = start(temp.path());
    assert!(host
        .api_checkpoint_evidence("project", "attempt")
        .unwrap()
        .is_none());
    let first = next_checkpoint(&mut host);
    let Request { checkpoint, reply } = first;
    host.accept_api_tool_checkpoint("project", "attempt", checkpoint)
        .unwrap();
    let first = host
        .api_checkpoint_evidence("project", "attempt")
        .unwrap()
        .unwrap();
    let root = attempt_root(&host);
    assert!(
        first.verify_original(&root).is_err(),
        "original worker still owns tools"
    );
    reply.send(Ok(None)).unwrap();
    let Request { checkpoint, reply } = next_checkpoint(&mut host);
    host.accept_api_tool_checkpoint("project", "attempt", checkpoint)
        .unwrap();
    let second = host
        .api_checkpoint_evidence("project", "attempt")
        .unwrap()
        .unwrap();
    assert_eq!(second.round(), 2);
    assert_eq!(first.run(), second.run());
    assert_ne!(first.tool_evidence().head(), second.tool_evidence().head());
    reply.send(Ok(None)).unwrap();
    api_host_tests::drain(&mut host);
    assert!(host
        .api_checkpoint_evidence("project", "attempt")
        .unwrap()
        .is_none());
    drop(host);
    let mut reopened = RuntimeHost::open(&temp.path().join("data")).unwrap();
    reopened.poll_verifications();
    assert!(reopened.api_active.is_empty());
    let first: crate::ApiToolEvidence =
        serde_json::from_slice(&serde_json::to_vec(&first).unwrap()).unwrap();
    let second: crate::ApiToolEvidence =
        serde_json::from_slice(&serde_json::to_vec(&second).unwrap()).unwrap();
    let incident = first.verify_incidents(&root, &policy()).unwrap();
    assert_eq!(incident.len(), 1);
    assert_eq!(second.verify_incidents(&root, &policy()).unwrap(), incident);
    assert_eq!(
        reopened.snapshot().api_jobs[0].state,
        ApiJobState::Submitted
    );
    assert!(root.join("rounds/003").is_dir());
    assert!(!root.join("rounds/004").exists());
    for mode in [
        "scope",
        "round",
        "head",
        "result",
        "count",
        "foreign-result",
    ] {
        let mut altered = serde_json::to_value(&second).unwrap();
        match mode {
            "scope" => altered["run"]["fence"] = 999.into(),
            "round" => altered["call"]["round"] = 1.into(),
            "head" => altered["callHead"] = "0".repeat(64).into(),
            "result" => {
                altered["tools"]["observation"]["calls"]["api-tool-1"]["result_is_error"] =
                    false.into()
            }
            "count" => altered["tools"]["eventCount"] = 1.into(),
            _ => {
                altered["tools"]["observation"]["calls"]["api-tool-1"]["tool_call_id"] =
                    "foreign-call".into()
            }
        }
        let evidence: crate::ApiToolEvidence = serde_json::from_value(altered).unwrap();
        assert!(evidence.verify_original(&root).is_err(), "{mode}");
    }
    // Replay is evidence-only and cannot turn Submitted into Accepted.
    assert_eq!(
        reopened.projects["project"].state().tasks["task"].state,
        TaskState::Submitted
    );
}
