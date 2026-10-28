//! Original local management journals through the bounded read path; no provider calls.
use super::*;
use crate::management_execution::{DecisionIntent, UserResolution};
use cerulean_adapters::openai::DecisionObservation;
use cerulean_domain::management::{DecisionEnvelope, ManagementDecision, Purpose};
use cerulean_domain::workspace::*;
use sha2::{Digest, Sha256};

fn fixture(root: &Path, count: usize) -> RuntimeHost {
    let (mut host, draft) = project_planning::tests::fixture(root);
    host.configure_project(draft.clone()).unwrap();
    fs::create_dir(root.join("project-two")).unwrap();
    host.register(Project {
        id: "project-two".into(),
        root: root.join("project-two").to_string_lossy().into(),
        timezone: "Asia/Shanghai".into(),
    })
    .unwrap();
    let mut second_draft = draft;
    second_draft.project_id = "project-two".into();
    host.configure_project(second_draft).unwrap();
    for n in 0..count {
        let id = format!("bounded-management-{n:04}");
        let context = serde_json::json!({"requirement":"original local read fixture", "workflow":{"project":"project"}});
        let state_digest = digest(&context).unwrap();
        host.management
            .begin(DecisionIntent {
                operation_id: id.clone(),
                project_id: "project".into(),
                purpose: Purpose::Plan,
                model: "fixture-history".into(),
                state_digest: state_digest.clone(),
                context,
                created_at: "2026-01-01T00:00:00Z".into(),
            })
            .unwrap();
        if n + 1 == count {
            continue;
        }
        host.management
            .finish(
                &id,
                Ok(DecisionObservation {
                    response_id: format!("resp_bounded_management_{n}"),
                    requested_model: "fixture-history".into(),
                    actual_model: "fixture-history".into(),
                    input_tokens: 0,
                    output_tokens: 0,
                    envelope: DecisionEnvelope {
                        state_digest,
                        decision: ManagementDecision::Wait {
                            reason: "original question ✓ ".repeat(120),
                        },
                    },
                }),
            )
            .unwrap();
        let reviewed = host.management.evidence_digest(&id).unwrap().to_owned();
        host.management
            .resolve(
                &id,
                &reviewed,
                UserResolution::Discard {
                    reason: "original local disposition ✓ ".repeat(120),
                },
            )
            .unwrap();
    }
    host
}

fn list(offset: u32, revision: Option<String>) -> WorkspaceQuery {
    WorkspaceQuery::ManagementDecisions {
        project_id: "project".into(),
        offset,
        revision,
    }
}
fn detail(operation_id: &str, revision: &str) -> WorkspaceQuery {
    WorkspaceQuery::ManagementDecision {
        project_id: "project".into(),
        operation_id: operation_id.into(),
        revision: revision.into(),
    }
}
fn first(host: &RuntimeHost) -> WorkspaceManagementDecisionPage {
    let WorkspaceResult::ManagementDecisions(page) = host.workspace_query(&list(0, None)).unwrap()
    else {
        panic!("original management page")
    };
    page
}
fn original_hashes(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    crate::reader_generation::tests::snapshot(root)
        .into_iter()
        .map(|(path, bytes)| (path, Sha256::digest(bytes).to_vec()))
        .collect()
}

#[test]
fn workspace_management_decisions_pages_original_45_and_full_detail_without_writes() {
    let temp = tempfile::tempdir().unwrap();
    let host = fixture(temp.path(), 45);
    let before = original_hashes(&host.root);
    let page = first(&host);
    assert_eq!(page.total, 45);
    assert_eq!(page.entries.len(), 20);
    let mut entries = page.entries.clone();
    let mut next = page.next_offset;
    while let Some(offset) = next {
        let result = host
            .workspace_query(&list(offset, Some(page.revision.clone())))
            .unwrap();
        assert!(serde_json::to_vec(&result).unwrap().len() <= crate::workspace_read::RESULT_BYTES);
        let WorkspaceResult::ManagementDecisions(part) = result else {
            panic!("page")
        };
        assert_eq!(part.revision, page.revision);
        assert_eq!(part.total, 45);
        assert_eq!(part.offset, offset);
        assert!(!part.entries.is_empty() && part.entries.len() <= 20);
        next = part.next_offset;
        entries.extend(part.entries);
    }
    assert_eq!(entries.len(), 45);
    let (original_entries, original_history) =
        host.management_page(Some("project"), 0, None).unwrap();
    assert_eq!(original_history.total, 45);
    assert_eq!(
        entries
            .iter()
            .map(|entry| &entry.operation_id)
            .collect::<Vec<_>>(),
        original_entries
            .iter()
            .map(|entry| &entry.operation_id)
            .collect::<Vec<_>>()
    );
    for entry in entries {
        assert_eq!(entry.project_id, "project");
        assert!(entry
            .message
            .as_ref()
            .is_none_or(|m| m.chars().count() <= 160));
        let result = host
            .workspace_query(&detail(&entry.operation_id, &page.revision))
            .unwrap();
        assert!(serde_json::to_vec(&result).unwrap().len() <= crate::workspace_read::RESULT_BYTES);
        let WorkspaceResult::ManagementDecision(found) = result else {
            panic!("detail")
        };
        assert_eq!(found.project_id, "project");
        assert_eq!(found.revision, page.revision);
        assert_eq!(
            serde_json::to_value(&found.record).unwrap(),
            serde_json::to_value(host.management_detail(&entry.operation_id).unwrap()).unwrap()
        );
        if entry.operation_id == "bounded-management-0044" {
            assert_eq!(entry.state, "unknown");
            assert_eq!(found.record.state, "unknown");
            assert!(found.record.wait_reason.is_none());
        } else {
            assert_eq!(
                found.record.wait_reason.as_deref(),
                Some("original question ✓ ".repeat(120).as_str())
            );
            assert_eq!(
                found.record.discard_reason.as_deref(),
                Some("original local disposition ✓ ".repeat(120).as_str())
            );
        }
    }
    assert_eq!(before, original_hashes(&host.root));
}

#[test]
fn workspace_management_decisions_rejects_identity_cursor_version_and_poison() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), 25);
    let page = first(&host);
    let before = original_hashes(&host.root);
    for query in [
        WorkspaceQuery::ManagementDecisions {
            project_id: "missing-project".into(),
            offset: 0,
            revision: None,
        },
        WorkspaceQuery::ManagementDecision {
            project_id: "project-two".into(),
            operation_id: "bounded-management-0000".into(),
            revision: page.revision.clone(),
        },
        detail("missing-operation", &page.revision),
        detail("../bounded-management-0000", &page.revision),
        detail("bounded-management-0000", ""),
        detail("bounded-management-0000", &"0".repeat(64)),
        list(20, None),
        list(26, Some(page.revision.clone())),
        list(20, Some("0".repeat(64))),
        list(0, Some("invalid-version".into())),
    ] {
        assert!(
            host.workspace_query(&query).is_err(),
            "unexpected success: {query:?}"
        );
    }
    host.base.poisoned = true;
    assert!(host.workspace_query(&list(0, None)).is_err());
    assert!(host
        .workspace_query(&detail("bounded-management-0000", &page.revision))
        .is_err());
    host.base.poisoned = false;
    let store = host.projects.remove("project").unwrap();
    let wrong = host.projects.remove("project-two").unwrap();
    host.projects.insert("project".into(), wrong);
    assert!(host.workspace_query(&list(0, None)).is_err());
    assert!(host
        .workspace_query(&detail("bounded-management-0000", &page.revision))
        .is_err());
    host.projects.insert("project".into(), store);
    assert_eq!(before, original_hashes(&host.root));
}

#[test]
fn workspace_management_decisions_same_sequence_full_detail_changes_invalidate_revision() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), 2);
    let id = "bounded-management-0000";
    let sequence = host.projects["project"].history().len();
    host.planning_errors
        .insert(id.into(), format!("{}first", "x".repeat(160)));
    let page = first(&host);
    let before = original_hashes(&host.root);
    host.planning_errors
        .insert(id.into(), format!("{}second", "x".repeat(160)));
    let changed = first(&host);
    assert_eq!(sequence, host.projects["project"].history().len());
    let original_summary = page
        .entries
        .iter()
        .find(|entry| entry.operation_id == id)
        .unwrap();
    let changed_summary = changed
        .entries
        .iter()
        .find(|entry| entry.operation_id == id)
        .unwrap();
    assert_eq!(original_summary.message, changed_summary.message);
    assert_eq!(
        original_summary.message.as_ref().unwrap().chars().count(),
        160
    );
    assert_ne!(page.revision, changed.revision);
    assert!(host
        .workspace_query(&list(0, Some(page.revision.clone())))
        .is_err());
    assert!(host.workspace_query(&detail(id, &page.revision)).is_err());
    let WorkspaceResult::ManagementDecision(found) = host
        .workspace_query(&detail(id, &changed.revision))
        .unwrap()
    else {
        panic!("detail")
    };
    assert_eq!(found.record.message, host.planning_errors.get(id).cloned());
    assert_eq!(before, original_hashes(&host.root));
}

#[test]
fn workspace_management_decisions_oversize_original_detail_fails_without_truncation_or_writes() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), 2);
    host.planning_errors.insert(
        "bounded-management-0000".into(),
        "x".repeat(crate::workspace_read::RESULT_BYTES + 1),
    );
    let before = original_hashes(&host.root);
    let page = first(&host);
    let oversized_summary = page
        .entries
        .iter()
        .find(|entry| entry.operation_id == "bounded-management-0000")
        .unwrap();
    assert!(oversized_summary.message.as_ref().unwrap().chars().count() <= 160);
    assert!(host
        .workspace_query(&detail("bounded-management-0000", &page.revision))
        .is_err());
    assert_eq!(before, original_hashes(&host.root));
}

#[test]
fn workspace_management_decisions_shared_durable_owner_before_during_after_and_interruption() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), 2);
    let page = first(&host);
    let query = detail("bounded-management-0001", &page.revision);
    let original = serde_json::to_value(host.workspace_query(&query).unwrap()).unwrap();
    let before = original_hashes(&host.root);
    let basis =
        workspace_host::management_decisions::original_basis(&host.projects["project"], "project")
            .unwrap()
            .version;
    assert!(
        host.management_detail("bounded-management-0001")
            .unwrap()
            .can_discard
    );
    let store = host.projects.remove("project").unwrap();
    let writer =
        crate::async_api::ApiWorkflow::start(store, host.root.join("workflows/project")).unwrap();
    host.api_writers.insert("project".into(), writer);
    assert_eq!(
        host.api_writers["project"]
            .owner
            .workspace_management_basis("project")
            .unwrap()
            .version,
        basis
    );
    let owned = first(&host);
    assert_ne!(owned.revision, page.revision);
    assert!(host.workspace_query(&query).is_err());
    let owned_query = detail("bounded-management-0001", &owned.revision);
    let owned_result = host.workspace_query(&owned_query).unwrap();
    let WorkspaceResult::ManagementDecision(found) = &owned_result else {
        panic!("detail")
    };
    let actual = host.management_detail("bounded-management-0001").unwrap();
    assert!(!actual.can_discard);
    assert!(!actual.can_reply);
    assert_eq!(
        serde_json::to_value(&found.record).unwrap(),
        serde_json::to_value(actual).unwrap()
    );
    host.api_writers
        .get_mut("project")
        .unwrap()
        .snapshot
        .sequence += 999;
    assert_eq!(first(&host).revision, owned.revision);
    assert_eq!(
        serde_json::to_value(host.workspace_query(&owned_query).unwrap()).unwrap(),
        serde_json::to_value(owned_result).unwrap()
    );
    assert_eq!(
        host.api_writers["project"]
            .owner
            .workspace_management_basis("project")
            .unwrap()
            .version,
        basis
    );
    host.api_writers.get_mut("project").unwrap().interrupted = true;
    assert!(host.workspace_query(&list(0, None)).is_err());
    assert!(host.workspace_query(&owned_query).is_err());
    host.api_writers.get_mut("project").unwrap().interrupted = false;
    let mut writer = host.api_writers.remove("project").unwrap();
    host.projects
        .insert("project".into(), writer.owner.finish(false).unwrap());
    assert_eq!(
        workspace_host::management_decisions::original_basis(&host.projects["project"], "project")
            .unwrap()
            .version,
        basis
    );
    assert_eq!(first(&host).revision, page.revision);
    assert_eq!(
        serde_json::to_value(host.workspace_query(&query).unwrap()).unwrap(),
        original
    );
    assert_eq!(before, original_hashes(&host.root));
}

#[test]
fn workspace_management_decisions_service_dispatch_reopen_preserves_original_sha() {
    let temp = tempfile::tempdir().unwrap();
    let host = fixture(temp.path(), 2);
    let page = first(&host);
    let query = detail("bounded-management-0000", &page.revision);
    let original = serde_json::to_value(host.workspace_query(&query).unwrap()).unwrap();
    let root = host.root.clone();
    let before = original_hashes(&root);
    drop(host);
    let service = crate::diagnostic_service::ServiceHost::open(&root).unwrap();
    assert!(!service.is_diagnostic());
    let WorkspaceResult::ManagementDecisions(reopened) =
        service.workspace_query(&list(0, None)).unwrap()
    else {
        panic!("page")
    };
    assert_eq!(reopened.revision, page.revision);
    assert_eq!(
        serde_json::to_value(service.workspace_query(&query).unwrap()).unwrap(),
        original
    );
    assert_eq!(before, original_hashes(&root));
}

#[test]
fn workspace_management_decisions_poisoned_durable_writer_and_shared_owner_fail_closed() {
    let temp = tempfile::tempdir().unwrap();
    let (mut host, draft) = project_planning::tests::fixture(temp.path());
    host.configure_project(draft).unwrap();
    project_planning::tests::complete(&mut host, project_planning::tests::proposal("tests"));
    let approval_digest = host.projects["project"]
        .revision()
        .unwrap()
        .approval_digest()
        .unwrap();
    host.approve("project", &approval_digest).unwrap();
    let page = first(&host);
    assert_eq!(page.total, 1);
    let query = detail(&page.entries[0].operation_id, &page.revision);
    let history = host.projects["project"].history().to_vec();
    let state = host.projects["project"].state().clone();
    let before = original_hashes(&host.root);
    let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        host.projects.get_mut("project").unwrap().command_observed(
            "management-reader-poisoned-writer",
            "user",
            WorkflowCommand::RequestRevision {
                digest: state.digest.clone().unwrap(),
                reason: "controlled original write interruption".into(),
            },
            CommandContext {
                approval_source: Some("authenticated local-user revision request".into()),
                ..Default::default()
            },
            true,
            |point| {
                if point == crate::scheduler::CommitPoint::BeforeEventWrite {
                    panic!("controlled interruption before original event write");
                }
            },
        )
    }));
    assert!(interrupted.is_err());
    assert!(host.projects["project"].requires_reopen());
    assert_eq!(host.projects["project"].history(), history);
    assert_eq!(host.projects["project"].state(), &state);
    assert!(host.workspace_query(&list(0, None)).is_err());
    assert!(host.workspace_query(&query).is_err());
    let store = host.projects.remove("project").unwrap();
    let writer =
        crate::async_api::ApiWorkflow::start(store, host.root.join("workflows/project")).unwrap();
    host.api_writers.insert("project".into(), writer);
    assert!(host.workspace_query(&list(0, None)).is_err());
    assert!(host.workspace_query(&query).is_err());
    assert_eq!(before, original_hashes(&host.root));
}

#[test]
fn workspace_management_decisions_same_sequence_original_resolution_stales_page_and_detail() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), 2);
    let id = "bounded-management-0001";
    let page = first(&host);
    let sequence = host.projects["project"].history().len();
    let reviewed = host.management.evidence_digest(id).unwrap().to_owned();
    host.management
        .resolve(
            id,
            &reviewed,
            UserResolution::Discard {
                reason: "explicit local disposition of unresolved original call".into(),
            },
        )
        .unwrap();
    assert_eq!(sequence, host.projects["project"].history().len());
    let before = original_hashes(&host.root);
    let changed = first(&host);
    assert_ne!(page.revision, changed.revision);
    assert!(host
        .workspace_query(&list(0, Some(page.revision.clone())))
        .is_err());
    assert!(host.workspace_query(&detail(id, &page.revision)).is_err());
    let WorkspaceResult::ManagementDecision(found) = host
        .workspace_query(&detail(id, &changed.revision))
        .unwrap()
    else {
        panic!("detail")
    };
    assert_eq!(found.record.state, "discarded");
    assert_eq!(
        found.record.discard_reason.as_deref(),
        Some("explicit local disposition of unresolved original call")
    );
    assert_ne!(found.record.evidence_digest, reviewed);
    assert_eq!(before, original_hashes(&host.root));
}

#[test]
fn workspace_management_decisions_diagnostic_service_refuses_list_and_detail_without_writes() {
    let temp = tempfile::tempdir().unwrap();
    let host = fixture(temp.path(), 2);
    let page = first(&host);
    let root = host.root.clone();
    drop(host);
    let damaged = root.join("desktop-task-dispatch-v1").join("a".repeat(64));
    fs::create_dir_all(&damaged).unwrap();
    fs::write(
        damaged.join("intent.pending"),
        br#"{"private-desktop-marker":"never expose","unfinished":"#,
    )
    .unwrap();
    let before = original_hashes(&root);
    let service = crate::diagnostic_service::ServiceHost::open(&root).unwrap();
    assert!(service.is_diagnostic());
    for query in [
        list(0, None),
        detail("bounded-management-0000", &page.revision),
        gateway_329_query("bounded-management-0000", &page.revision),
    ] {
        assert_eq!(
            service.workspace_query(&query).unwrap_err(),
            "项目记录处于诊断状态，清单暂不可用；不代表没有项目"
        );
    }
    assert_eq!(before, original_hashes(&root));
}

fn gateway_329_query(operation_id: &str, revision: &str) -> WorkspaceQuery {
    WorkspaceQuery::ManagementGatewayEvidence {
        project_id: "project".into(),
        operation_id: operation_id.into(),
        revision: revision.into(),
    }
}

fn gateway_329_read(
    host: &RuntimeHost,
    operation_id: &str,
    revision: &str,
) -> WorkspaceManagementGatewayEvidenceDetail {
    let result = host
        .workspace_query(&gateway_329_query(operation_id, revision))
        .unwrap();
    assert!(serde_json::to_vec(&result).unwrap().len() <= crate::workspace_read::RESULT_BYTES);
    let WorkspaceResult::ManagementGatewayEvidence(detail) = result else {
        panic!("independent gateway evidence result");
    };
    assert_eq!(detail.project_id, "project");
    assert_eq!(detail.operation_id, operation_id);
    assert_eq!(detail.revision, revision);
    assert_eq!(
        detail.evidence_digest,
        host.management.evidence_digest(operation_id).unwrap()
    );
    *detail
}

fn gateway_329_directories(root: &Path) -> std::collections::BTreeSet<PathBuf> {
    let mut directories = std::collections::BTreeSet::new();
    for entry in fs::read_dir(root).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            directories.insert(entry.path());
            directories.extend(gateway_329_directories(&entry.path()));
        }
    }
    directories
}

fn gateway_329_fixture(
    root: &Path,
) -> (
    RuntimeHost,
    cerulean_adapters::deputy_management::PreparedDeputy,
) {
    use cerulean_adapters::{api_generation::ApiClient, deputy_management};
    use cerulean_domain::{
        credentials::ProviderEndpoint,
        management_ingress::{ManagementHandoff, ManagementRole, ManagementSource},
        secret::SecretInput,
    };
    let host = fixture(root, 0);
    let data = host.root.clone();
    drop(host);
    if crate::data_format::status(&data)
        .unwrap()
        .minimum_reader_generation
        < 13
    {
        crate::management_source_guard::tests::promote_fixture(&data);
    }
    if crate::data_format::status(&data)
        .unwrap()
        .minimum_reader_generation
        < 16
    {
        crate::relay_guard::tests::promote_fixture(&data, &root.join("gateway-before16-329"));
    }
    let mut host = RuntimeHost::open(&data).unwrap();
    let endpoint = ProviderEndpoint::A6Api {};
    let context = serde_json::json!({"requirement":"synthetic original management query", "executionSetup":{"setup":{
        "chiefModel":"gpt-6.1-sol", "managementSources":{"chief":{"kind":"provider","endpoint":endpoint}}
    }}});
    let intent = DecisionIntent {
        operation_id: "gateway-local-329".into(),
        project_id: "project".into(),
        purpose: Purpose::Plan,
        model: "gpt-6.1-sol".into(),
        state_digest: digest(&context).unwrap(),
        context: context.clone(),
        created_at: "2026-10-24T00:00:00Z".into(),
    };
    let client =
        ApiClient::new(SecretInput::new("invalid-synthetic-gateway-329".into()).unwrap()).unwrap();
    let prepared = deputy_management::prepare(
        &client,
        ManagementHandoff {
            operation_id: intent.operation_id.clone(),
            project_id: intent.project_id.clone(),
            purpose: intent.purpose,
            source: ManagementSource::Provider {
                endpoint,
                model: intent.model.clone(),
                role: ManagementRole::Chief,
            },
            state_digest: intent.state_digest.clone(),
            context,
        },
    )
    .unwrap();
    host.management.begin_deputy(intent, &prepared).unwrap();
    (host, prepared)
}

fn gateway_329_reply(
    prepared: &cerulean_adapters::deputy_management::PreparedDeputy,
) -> cerulean_adapters::deputy_management::DeputyResult {
    cerulean_adapters::deputy_management::fixture_result(
        prepared,
        serde_json::json!({
            "id":"gateway-body-329", "model":"gpt-6.1-sol", "usage":{"prompt_tokens":3,"completion_tokens":2},
            "choices":[{"index":0,"finish_reason":"stop","message":{"role":"assistant","content":
                serde_json::json!({"stateDigest":prepared.request().handoff.state_digest,"decision":{"action":"wait","reason":"synthetic review pending"}}).to_string()
            }}]
        }),
    )
}

fn gateway_329_headers() -> cerulean_adapters::api_generation::gateway::GatewayHeaders {
    use cerulean_adapters::api_generation::gateway::{GatewayHeaders, GatewayRequestId};
    GatewayHeaders {
        status: 200,
        request_id: GatewayRequestId::Accepted {
            value: "gateway-remote-329".into(),
        },
    }
}

#[test]
fn workspace_management_gateway_329_non_a6_keeps_legacy_shape_and_original_guards() {
    let temp = tempfile::tempdir().unwrap();
    let mut host = fixture(temp.path(), 2);
    let id = "bounded-management-0001";
    let page = first(&host);
    let before = original_hashes(&host.root);
    let directories = gateway_329_directories(&host.root);
    let original = host.workspace_query(&detail(id, &page.revision)).unwrap();
    let original_json = serde_json::to_value(&original).unwrap();
    let keys = original_json["value"].as_object().unwrap();
    assert_eq!(keys.len(), 3);
    for name in ["projectId", "revision", "record"] {
        assert!(keys.contains_key(name));
    }
    assert!(gateway_329_read(&host, id, &page.revision)
        .observation
        .is_none());
    let WorkspaceResult::ManagementDecisions(other) = host
        .workspace_query(&WorkspaceQuery::ManagementDecisions {
            project_id: "project-two".into(),
            offset: 0,
            revision: None,
        })
        .unwrap()
    else {
        panic!("other original project");
    };
    for query in [
        WorkspaceQuery::ManagementGatewayEvidence {
            project_id: "project-two".into(),
            operation_id: id.into(),
            revision: other.revision,
        },
        gateway_329_query("missing-original", &page.revision),
        gateway_329_query("../outside", &page.revision),
        gateway_329_query(id, &"0".repeat(64)),
    ] {
        assert!(host.workspace_query(&query).is_err(), "{query:?}");
    }
    host.base.poisoned = true;
    assert!(host
        .workspace_query(&gateway_329_query(id, &page.revision))
        .is_err());
    host.base.poisoned = false;
    assert_eq!(before, original_hashes(&host.root));
    assert_eq!(directories, gateway_329_directories(&host.root));
    assert_eq!(
        serde_json::to_value(host.workspace_query(&detail(id, &page.revision)).unwrap()).unwrap(),
        original_json
    );
    let evidence = host.management.evidence_digest(id).unwrap().to_owned();
    host.management
        .resolve(
            id,
            &evidence,
            UserResolution::Discard {
                reason: "synthetic explicit disposition changes original revision".into(),
            },
        )
        .unwrap();
    let before = original_hashes(&host.root);
    assert!(host
        .workspace_query(&gateway_329_query(id, &page.revision))
        .is_err());
    let current = first(&host);
    assert_ne!(current.revision, page.revision);
    assert!(gateway_329_read(&host, id, &current.revision)
        .observation
        .is_none());
    assert_eq!(before, original_hashes(&host.root));
}

#[test]
fn workspace_management_gateway_329_projects_safe_states_and_distinct_ids_without_writes() {
    use cerulean_adapters::api_generation::{
        gateway::{GatewayRequestId, GatewayRequestIdRejection},
        ApiFailure,
    };
    use cerulean_adapters::deputy_management::{DeputyOutcome, DeputyResult};
    for mode in [
        "linked",
        "headerOnly",
        "missing",
        "noHeadersObserved",
        "unavailable",
        "http",
        "rejectedId",
        "missingId",
    ] {
        let temp = tempfile::tempdir().unwrap();
        let (mut host, prepared) = gateway_329_fixture(temp.path());
        let id = "gateway-local-329";
        let mut capture = host.management.gateway_capture(id).unwrap();
        let mut headers = gateway_329_headers();
        match mode {
            "http" => headers.status = 429,
            "rejectedId" => {
                headers.request_id = GatewayRequestId::Rejected {
                    reason: GatewayRequestIdRejection::CredentialReflection,
                }
            }
            "missingId" => headers.request_id = GatewayRequestId::Missing {},
            _ => (),
        }
        if !matches!(mode, "missing" | "noHeadersObserved") {
            capture.headers(&headers);
        }
        if mode == "unavailable" {
            capture.headers(&headers);
        }
        if mode != "headerOnly" {
            let response = if mode == "http" {
                DeputyResult {
                    request: prepared.request(),
                    decode_stage: None,
                    outcome: DeputyOutcome::Failed {
                        failure: ApiFailure::Http { status: 429 },
                    },
                }
            } else {
                gateway_329_reply(&prepared)
            };
            host.management.finish_deputy(id, response).unwrap();
            if mode != "missing" {
                host.management.finish_gateway(id, capture);
            }
        }
        let page = first(&host);
        let original =
            serde_json::to_value(host.workspace_query(&detail(id, &page.revision)).unwrap())
                .unwrap();
        let before = original_hashes(&host.root);
        let directories = gateway_329_directories(&host.root);
        let observation = gateway_329_read(&host, id, &page.revision)
            .observation
            .unwrap();
        assert_eq!(observation.provider, WorkspaceGatewayProvider::A6api);
        assert_eq!(observation.local_id, id);
        assert_eq!(
            observation.body_response_id.as_deref(),
            if matches!(mode, "headerOnly" | "http") {
                None
            } else {
                Some("gateway-body-329")
            }
        );
        let state = match mode {
            "headerOnly" => WorkspaceGatewayEvidenceStatus::HeaderOnly,
            "missing" => WorkspaceGatewayEvidenceStatus::Missing,
            "noHeadersObserved" => WorkspaceGatewayEvidenceStatus::NoHeadersObserved,
            "unavailable" => WorkspaceGatewayEvidenceStatus::Unavailable,
            _ => WorkspaceGatewayEvidenceStatus::Linked,
        };
        assert_eq!(observation.status, state);
        match mode {
            "missing" | "noHeadersObserved" | "unavailable" => {
                assert!(observation.headers.is_none());
            }
            "rejectedId" => assert!(matches!(
                observation.headers.unwrap().request_id,
                WorkspaceGatewayRequestId::Rejected {
                    reason: WorkspaceGatewayRequestIdRejection::CredentialReflection
                }
            )),
            "missingId" => assert!(matches!(
                observation.headers.unwrap().request_id,
                WorkspaceGatewayRequestId::Missing {}
            )),
            _ => {
                let headers = observation.headers.unwrap();
                assert_eq!(headers.status, if mode == "http" { 429 } else { 200 });
                assert_eq!(
                    headers.request_id,
                    WorkspaceGatewayRequestId::Accepted {
                        value: "gateway-remote-329".into()
                    }
                );
            }
        }
        assert_eq!(before, original_hashes(&host.root));
        assert_eq!(directories, gateway_329_directories(&host.root));
        assert_eq!(
            serde_json::to_value(host.workspace_query(&detail(id, &page.revision)).unwrap())
                .unwrap(),
            original
        );
    }
}

#[test]
fn workspace_management_gateway_329_late_completion_keeps_original_revision_and_reopens() {
    let temp = tempfile::tempdir().unwrap();
    let (mut host, prepared) = gateway_329_fixture(temp.path());
    let id = "gateway-local-329";
    let mut capture = host.management.gateway_capture(id).unwrap();
    capture.headers(&gateway_329_headers());
    host.management
        .finish_deputy(id, gateway_329_reply(&prepared))
        .unwrap();
    let page = first(&host);
    let original =
        serde_json::to_value(host.workspace_query(&detail(id, &page.revision)).unwrap()).unwrap();
    let before = original_hashes(&host.root);
    let first_read = gateway_329_read(&host, id, &page.revision);
    assert_eq!(
        first_read.observation.unwrap().status,
        WorkspaceGatewayEvidenceStatus::HeaderOnly
    );
    assert_eq!(before, original_hashes(&host.root));
    host.management.finish_gateway(id, capture);
    let before = original_hashes(&host.root);
    let final_read = gateway_329_read(&host, id, &page.revision);
    assert_eq!(
        final_read.observation.as_ref().unwrap().status,
        WorkspaceGatewayEvidenceStatus::Linked
    );
    assert_eq!(first(&host).revision, page.revision);
    assert_eq!(
        serde_json::to_value(host.workspace_query(&detail(id, &page.revision)).unwrap()).unwrap(),
        original
    );
    assert_eq!(before, original_hashes(&host.root));
    let data = host.root.clone();
    drop(host);
    let host = RuntimeHost::open(&data).unwrap();
    assert_eq!(
        serde_json::to_value(gateway_329_read(&host, id, &page.revision)).unwrap(),
        serde_json::to_value(final_read).unwrap()
    );
    assert_eq!(before, original_hashes(&data));
}

#[test]
fn workspace_management_gateway_329_bad_sidecar_errors_do_not_hide_original_detail_or_repair() {
    for mode in ["digest", "binding", "pending", "missingHeader", "oversized"] {
        let temp = tempfile::tempdir().unwrap();
        let (mut host, prepared) = gateway_329_fixture(temp.path());
        let id = "gateway-local-329";
        let mut capture = host.management.gateway_capture(id).unwrap();
        capture.headers(&gateway_329_headers());
        host.management
            .finish_deputy(id, gateway_329_reply(&prepared))
            .unwrap();
        host.management.finish_gateway(id, capture);
        let page = first(&host);
        let original =
            serde_json::to_value(host.workspace_query(&detail(id, &page.revision)).unwrap())
                .unwrap();
        let directory = host.root.join("gateway-response-evidence");
        let header = fs::read_dir(&directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.to_string_lossy().ends_with(".headers.json"))
            .unwrap();
        match mode {
            "pending" => fs::write(header.with_extension("pending"), b"partial").unwrap(),
            "missingHeader" => fs::remove_file(&header).unwrap(),
            "oversized" => fs::write(&header, vec![b'x'; 4097]).unwrap(),
            "binding" => {
                // Preserve exact typed field order while changing the binding
                // and recomputing its own digest. The original intent still wins.
                let raw = fs::read_to_string(&header).unwrap();
                let end = raw.rfind(",\"hash\":").unwrap();
                let body = raw["{\"body\":".len()..end]
                    .replace("\"projectId\":\"project\"", "\"projectId\":\"project-two\"");
                assert_ne!(body, raw["{\"body\":".len()..end]);
                let hash = format!("{:x}", Sha256::digest(body.as_bytes()));
                fs::write(&header, format!("{{\"body\":{body},\"hash\":\"{hash}\"}}")).unwrap();
            }
            _ => {
                let raw = fs::read_to_string(&header).unwrap();
                fs::write(
                    &header,
                    raw.replace("gateway-remote-329", "gateway-tamper-329"),
                )
                .unwrap();
            }
        }
        let before = original_hashes(&host.root);
        let directories = gateway_329_directories(&host.root);
        assert!(
            host.workspace_query(&gateway_329_query(id, &page.revision))
                .is_err(),
            "{mode}"
        );
        assert_eq!(
            serde_json::to_value(host.workspace_query(&detail(id, &page.revision)).unwrap())
                .unwrap(),
            original,
            "gateway diagnostics cannot replace original management evidence"
        );
        assert_eq!(before, original_hashes(&host.root));
        assert_eq!(directories, gateway_329_directories(&host.root));
    }
}
