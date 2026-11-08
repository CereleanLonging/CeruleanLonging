//! Real host/file tools/verifier, synthetic provider responses; never network.
use super::*;
use cerulean_adapters::deputy_management::{self, DeputyResult, PreparedDeputy};
use cerulean_domain::{
    credentials::ProviderEndpoint as E,
    project_setup::{ChiefSource, DeputySource, ManagementSources},
};

fn endpoints() -> Vec<E> {
    vec![
        E::DeepSeek {},
        E::Anthropic {},
        E::KimiChina {},
        E::Grok {},
        E::A6Api {},
        E::QwenBeijing {
            workspace_id: "test-workspace".into(),
        },
        E::QwenSingapore {},
        E::QwenHongKong {},
    ]
}
fn reply(p: &PreparedDeputy) -> DeputyResult {
    let request = p.request();
    request.validate().unwrap();
    let h = &request.handoff;
    let observation = manager(crate::management_execution::DecisionIntent {
        operation_id: h.operation_id.clone(),
        project_id: h.project_id.clone(),
        purpose: h.purpose,
        model: request.wire.requested_model.clone(),
        state_digest: h.state_digest.clone(),
        context: h.context.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
    })
    .unwrap();
    let content = serde_json::to_string(&observation.envelope).unwrap();
    let value = if request.wire.target.provider() == CredentialProvider::Anthropic {
        json!({"id":observation.response_id,"model":observation.actual_model,"type":"message","role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":content}],"usage":{"input_tokens":12,"output_tokens":8,"cache_read_input_tokens":2,"cache_creation_input_tokens":1}})
    } else {
        json!({"id":observation.response_id,"model":observation.actual_model,"usage":{"prompt_tokens":12,"completion_tokens":8},"choices":[{"index":0,"finish_reason":"stop","message":{"role":"assistant","content":content}}]})
    };
    deputy_management::fixture_result(p, value)
}
fn configured(temp: &Path, chief: E, deputy: E) -> RuntimeHost {
    let (host, draft) = project_planning::tests::fixture(temp);
    drop(host);
    let data = temp.join("data");
    crate::relay_guard::tests::promote_fixture(&data, &temp.join("before16"));
    crate::rejected_usage_guard::tests::promote_fixture(&data, &temp.join("before17"));
    let mut host = RuntimeHost::open(&data).unwrap();
    let mut draft = api_project_tests::draft_for(
        draft,
        ProjectWorkerAdapter::A6api,
        ApiTarget::Provider {
            endpoint: E::A6Api {},
        },
    );
    draft.requirement = "Two task API DAG success".into();
    draft.artifact_paths = vec!["result.txt".into()];
    draft.worker_max_requests = 8;
    draft.management_max_requests = 10;
    draft.worker_model = "gpt-6.1-sol".into();
    draft.api_worker.as_mut().unwrap().model = draft.worker_model.clone();
    draft.chief_model = "fixture-planner".into();
    draft.deputy_model = "fixture-coordinator".into();
    let providers: std::collections::BTreeSet<_> = [
        chief.provider(),
        deputy.provider(),
        CredentialProvider::A6Api,
    ]
    .into_iter()
    .collect();
    draft.management_sources = Some(ManagementSources {
        chief_reasoning_effort: None,
        deputy_reasoning_effort: None,
        chief: ChiefSource::Provider { endpoint: chief },
        deputy: DeputySource::Provider { endpoint: deputy },
    });
    draft.check_args = vec![
        "--ignored".into(),
        "--exact".into(),
        "host::api_run_tests::api_dag_validator".into(),
    ];
    project_planning::tests::isolate_draft(temp, &mut draft);
    host.configure_project(draft).unwrap();
    for provider in providers {
        host.set_provider_key(
            provider,
            Some(secret::SecretInput::new("invalid-local-fixture-only".into()).unwrap()),
        )
        .unwrap();
    }
    host.deputy_management_fixture = Some(reply);
    host.api_fixture = Some(worker);
    host
}

#[test]
fn document_refinement_uses_configured_provider_without_workflow_mutation() {
    use cerulean_domain::management::DocumentRefinementRequest;
    for chief in [E::DeepSeek {}, E::A6Api {}] {
        let temp = tempfile::tempdir().unwrap();
        let mut host = configured(temp.path(), chief.clone(), E::DeepSeek {});
        host.deputy_management_fixture = Some(|p| {
            let r = p.request();
            assert_eq!(r.handoff.purpose, Purpose::Refine);
            assert!(r.handoff.context.get("workflow").is_none());
            let text = r.handoff.context["document"]["text"].as_str().unwrap();
            deputy_management::fixture_result(
                p,
                json!({
                    "id":"refinement-fixture", "model":r.wire.requested_model,
                    "usage":{"prompt_tokens":12,"completion_tokens":8},
                    "choices":[{"index":0,"finish_reason":"stop","message":{"role":"assistant","content":
                        json!({"stateDigest":r.handoff.state_digest,"decision":{"action":"refine","document":{
                            "title":"需求提炼","points":[{"kind":"acceptance","text":"核对 CSV 全部行", "quote":text}]
                        }}}).to_string()}}]
                }),
            )
        });
        let before = serde_json::to_value(host.projects["project"].history()).unwrap();
        host.request_document_refinement(DocumentRefinementRequest {
            project_id: "project".into(),
            setup_digest: host.project_setups["project"].digest.clone(),
            operation_id: "md-provider-0".into(),
            document_digest: "a".repeat(64),
            part: 0,
            total: 1,
            text: "CSV 不得遗漏任何行".into(),
        })
        .unwrap();
        spin(&mut host, |h| {
            h.management
                .application("md-provider-0")
                .is_some_and(|(_, applied, _)| applied)
        });
        let (request, result, _) = host.read_deepseek_management("md-provider-0").unwrap();
        assert_eq!(request.handoff.source.api_selection().unwrap().0, chief);
        assert!(result.unwrap().billing().is_some());
        assert_eq!(
            serde_json::to_value(host.projects["project"].history()).unwrap(),
            before
        );
        assert!(host.projects["project"].approval().is_none());
        assert!(host.runs.all().is_empty());
        drop(host);
        let host = RuntimeHost::open(&temp.path().join("data")).unwrap();
        assert!(host
            .management_detail("md-provider-0")
            .unwrap()
            .refinement
            .is_some());
        assert!(host.management_active.is_empty());
    }
}

#[test]
fn flexible_managers_plan_assign_and_gate_a6api_worker_without_official_key_and_reopen() {
    let all = endpoints();
    for (index, chief) in all.iter().enumerate() {
        let temp = tempfile::tempdir().unwrap();
        let deputy = all[(index + 1) % all.len()].clone();
        let mut host = configured(temp.path(), chief.clone(), deputy.clone());
        assert!(!host.openai_connection.key_configured);
        let setup = host.project_setups["project"].digest.clone();
        host.request_chief_plan("project", &setup).unwrap();
        spin(&mut host, |h| h.projects["project"].revision().is_some());
        assert!(host.projects["project"].approval().is_none());
        assert!(host.api_jobs.is_empty());
        let revision = host.projects["project"]
            .revision()
            .unwrap()
            .approval_digest()
            .unwrap();
        assert!(host
            .start_project_run("project", &revision, &setup)
            .is_err());
        host.approve("project", &revision).unwrap();
        start(&mut host);
        spin(&mut host, |h| {
            h.runs.all()[0].state != RunState::Active
                && h.api_active.is_empty()
                && h.management_active.is_empty()
        });
        assert_eq!(
            host.runs.all()[0].state,
            RunState::Attention,
            "{:?}",
            host.runs.all()
        );
        assert_eq!(
            fs::read_to_string(
                project_planning::tests::worker_root(temp.path()).join("result.txt")
            )
            .unwrap(),
            "alpha"
        );
        assert_eq!(
            host.projects["project"].state().tasks["a"].state,
            TaskState::Submitted
        );
        assert_eq!(
            host.projects["project"].state().tasks["b"].state,
            TaskState::Pending
        );
        assert!(!host.projects["project"].state().tasks["a"].verified);
        assert_eq!(host.management.statuses().len(), 2);
        let ids = host.management_operation_ids("project");
        for id in &ids {
            let (request, result, _) = host.read_deepseek_management(id).unwrap();
            let expected = match request.handoff.purpose {
                Purpose::Plan | Purpose::Accept => chief,
                _ => &deputy,
            };
            assert_eq!(
                request.wire.target,
                ApiTarget::Provider {
                    endpoint: expected.clone()
                }
            );
            assert!(result.unwrap().envelope().is_some());
            let source =
                serde_json::to_value(host.management.stored_decision(id).unwrap()).unwrap();
            assert_eq!(source["source"], "provider");
        }
        let history = host.projects["project"].history().to_vec();
        let records = crate::reader_generation::tests::snapshot(
            &temp.path().join("data/management-decisions"),
        );
        drop(host);
        for _ in 0..2 {
            let mut host = RuntimeHost::open(&temp.path().join("data")).unwrap();
            host.poll_verifications();
            assert_eq!(host.projects["project"].history(), history);
            assert_eq!(host.runs.all()[0].state, RunState::Attention);
            assert!(host.management_active.is_empty() && host.api_active.is_empty());
            assert_eq!(
                crate::reader_generation::tests::snapshot(
                    &temp.path().join("data/management-decisions")
                ),
                records
            );
        }
    }
}

#[test]
fn flexible_management_missing_key_wrong_model_and_request_limit_stop_before_intent() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = configured(temp.path(), E::DeepSeek {}, E::A6Api {});
    let setup = host.project_setups["project"].digest.clone();
    host.set_provider_key(CredentialProvider::DeepSeek, None)
        .unwrap();
    assert!(host.request_chief_plan("project", &setup).is_err());
    assert!(host.management.statuses().is_empty());
    host.set_provider_key(
        CredentialProvider::DeepSeek,
        Some(secret::SecretInput::new("invalid-local-test-only".into()).unwrap()),
    )
    .unwrap();
    assert!(host
        .start_management_decision(
            "project",
            "wrong-model",
            Purpose::Plan,
            "different-model",
            "Two task API DAG success"
        )
        .is_err());
    assert!(host.management.statuses().is_empty());
    host.project_setups
        .get_mut("project")
        .unwrap()
        .setup
        .management_max_requests = 0;
    assert!(host
        .start_management_decision(
            "project",
            "limit",
            Purpose::Plan,
            "fixture-planner",
            "Two task API DAG success"
        )
        .is_err());
    assert!(host.management.statuses().is_empty());
    assert!(!host.openai_connection.key_configured);
}

#[test]
fn flexible_management_lost_reply_is_unknown_and_never_falls_back_or_resends_on_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = configured(temp.path(), E::A6Api {}, E::DeepSeek {});
    host.deputy_management_fixture = Some(|_| panic!("synthetic lost management reply"));
    let setup = host.project_setups["project"].digest.clone();
    host.request_chief_plan("project", &setup).unwrap();
    spin(&mut host, |h| h.management_active.is_empty());
    assert_eq!(
        host.management.statuses()[0].state,
        crate::management_execution::DecisionState::Unknown
    );
    assert!(host.api_jobs.is_empty());
    assert!(host.projects["project"].revision().is_none());
    assert!(host.request_chief_plan("project", &setup).is_err());
    let records =
        crate::reader_generation::tests::snapshot(&temp.path().join("data/management-decisions"));
    drop(host);
    let mut host = RuntimeHost::open(&temp.path().join("data")).unwrap();
    host.deputy_management_fixture = Some(|_| panic!("must not resend"));
    host.poll_verifications();
    assert_eq!(host.management.statuses().len(), 1);
    assert!(host.request_chief_plan("project", &setup).is_err());
    assert_eq!(
        crate::reader_generation::tests::snapshot(&temp.path().join("data/management-decisions")),
        records
    );
    assert!(host.management_active.is_empty());
}

#[test]
fn submitted_retirement_shutdown_280_preserves_unaccepted_delivery_and_requires_disposition() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = configured(temp.path(), E::A6Api {}, E::DeepSeek {});
    approved(&mut host);
    start(&mut host);
    spin(&mut host, |h| {
        h.runs.all()[0].state == RunState::Attention
            && h.api_active.is_empty()
            && h.management_active.is_empty()
    });
    assert_eq!(
        host.projects["project"].state().tasks["a"].state,
        TaskState::Submitted
    );
    assert!(!host.can_shutdown());
    assert!(host
        .management
        .statuses()
        .iter()
        .all(|s| s.purpose != Purpose::Accept));
    assert!(!host.projects["project"].state().tasks["a"].verified);
    let review = host.project_revision_candidates().pop().unwrap();
    assert!(host
        .request_project_revision(
            "project",
            &review.evidence_digest,
            "ordinary revision insufficient"
        )
        .is_err());
    host.retire_submitted_plan(
        "project",
        &review.evidence_digest,
        "explicitly end unaccepted submission while keeping artifact",
    )
    .unwrap();
    assert!(host.can_shutdown());
    assert_eq!(host.runs.all()[0].state, RunState::Superseded);
    assert_eq!(
        host.projects["project"].state().tasks["a"].state,
        TaskState::Submitted
    );
    assert!(!host.projects["project"].state().tasks["a"].verified);
    let history = host.projects["project"].history().to_vec();
    let artifact =
        fs::read(project_planning::tests::worker_root(temp.path()).join("result.txt")).unwrap();
    drop(host);
    let mut reopened = RuntimeHost::open(&temp.path().join("data")).unwrap();
    assert!(reopened.can_shutdown());
    assert_eq!(reopened.projects["project"].history(), history);
    assert_eq!(
        fs::read(project_planning::tests::worker_root(temp.path()).join("result.txt")).unwrap(),
        artifact
    );
    assert_eq!(
        reopened.projects["project"].state().tasks["a"].state,
        TaskState::Submitted
    );
    assert!(reopened.management_active.is_empty() && reopened.api_active.is_empty());
}
