use super::*;
use crate::management_execution::{ApplicationTarget, DecisionState};
use cerulean_domain::secret::SecretInput;
use serde_json::json;
fn setup() -> (tempfile::TempDir, RuntimeHost) {
    let temp = tempfile::tempdir().unwrap();
    let (host, _) = super::super::project_planning::tests::fixture(temp.path());
    drop(host);
    crate::management_source_guard::tests::promote_fixture(&temp.path().join("data"));
    let host = RuntimeHost::open(&temp.path().join("data")).unwrap();
    (temp, host)
}
fn prepared(host: &RuntimeHost, id: &str) -> (DecisionIntent, PreparedDeputy) {
    let context = host
        .management_context("project", "Implement a sum function")
        .unwrap();
    let intent = DecisionIntent {
        operation_id: id.into(),
        project_id: "project".into(),
        purpose: Purpose::Advise,
        model: "deepseek-flash".into(),
        state_digest: digest(&context).unwrap(),
        context: context.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    let client = ApiClient::new(SecretInput::new("invalid-fixture-value".into()).unwrap()).unwrap();
    let p = deputy_management::prepare(
        &client,
        ManagementHandoff {
            operation_id: id.into(),
            project_id: "project".into(),
            purpose: intent.purpose,
            source: ManagementSource::DeepSeek {
                model: intent.model.clone(),
            },
            state_digest: intent.state_digest.clone(),
            context,
        },
    )
    .unwrap();
    (intent, p)
}
fn poll(host: &mut RuntimeHost) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !host.management_active.is_empty() {
        host.poll_verifications();
        assert!(
            std::time::Instant::now() < deadline,
            "owned deputy thread did not finish"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
#[test]
fn deputy_host_records_before_thread_and_preserves_provider_usage_after_apply_and_reopen() {
    let (temp, mut host) = setup();
    let (intent, p) = prepared(&host, "host-deputy");
    let path = temp
        .path()
        .join("data/management-decisions/host-deputy.0.json");
    host.launch_deepseek_management(intent,p,move|p, _| {
        assert!(path.is_file(),"send must follow durable intent");
        deputy_management::fixture_result(&p,json!({"id":"ds-host-reply","model":"deepseek-flash","usage":{"prompt_tokens":12,"completion_tokens":5},"choices":[{"index":0,"finish_reason":"stop","message":{"role":"assistant","content":json!({"stateDigest":p.request().handoff.state_digest,"decision":{"action":"wait","reason":"require independent approval"}}).to_string()}}]}))
    }).unwrap();
    poll(&mut host);
    assert_eq!(host.management_decisions()[0].state, DecisionState::Ready);
    assert!(host.management_count_errors.is_empty());
    host.apply_management_decision_inner("host-deputy", ApplicationTarget::Wait)
        .unwrap();
    assert!(host.projects["project"].approval().is_none());
    assert!(host.runs.all().is_empty());
    let original = host.read_deepseek_management("host-deputy").unwrap();
    assert_eq!(
        original
            .1
            .as_ref()
            .unwrap()
            .billing()
            .unwrap()
            .usage
            .input_tokens,
        12
    );
    assert!(host.management.billing_evidence("host-deputy").is_err());
    drop(host);
    let host = RuntimeHost::open(&temp.path().join("data")).unwrap();
    assert_eq!(host.management_decisions()[0].state, DecisionState::Applied);
    assert_eq!(
        digest(&host.read_deepseek_management("host-deputy").unwrap()).unwrap(),
        digest(&original).unwrap()
    );
    assert!(host.management_active.is_empty());
}
#[test]
fn deputy_host_disconnected_worker_keeps_unknown_and_preflight_never_uses_openai_fallback() {
    let (temp, mut host) = setup();
    for purpose in [
        Purpose::Plan,
        Purpose::Accept,
        Purpose::Assign,
        Purpose::Advise,
    ] {
        assert!(host
            .start_deepseek_management(
                "project",
                "denied",
                purpose,
                "deepseek-flash",
                "Implement a sum function"
            )
            .is_err());
    }
    assert!(host.management_decisions().is_empty());
    let (intent, p) = prepared(&host, "disconnected");
    host.launch_deepseek_management(intent, p, |_, _| {
        panic!("simulated lost worker before returning evidence")
    })
    .unwrap();
    poll(&mut host);
    assert_eq!(host.management_decisions()[0].state, DecisionState::Unknown);
    let outcome = host
        .read_deepseek_management("disconnected")
        .unwrap()
        .1
        .unwrap();
    assert!(matches!(
        outcome.outcome,
        DeputyOutcome::Failed {
            failure: ApiFailure::Transport {}
        }
    ));
    assert!(outcome.billing().is_none());
    drop(host);
    let mut host = RuntimeHost::open(&temp.path().join("data")).unwrap();
    assert!(host.management_active.is_empty());
    let (i, p) = prepared(&host, "disconnected");
    assert!(host
        .launch_deepseek_management(i, p, |_, _| panic!("must not retry"))
        .is_err());
}

fn blocked_deputy(host: &mut RuntimeHost, id: &str) -> std::sync::mpsc::Sender<()> {
    let (intent, prepared) = prepared(host, id);
    let (release, wait) = std::sync::mpsc::channel();
    host.launch_deepseek_management(intent, prepared, move |prepared, _| {
        wait.recv_timeout(std::time::Duration::from_secs(10))
            .expect("test owner must release deputy without network");
        DeputyResult {
            request: prepared.request(),
            decode_stage: None,
            outcome: DeputyOutcome::Failed {
                failure: ApiFailure::NotSent {},
            },
        }
    })
    .unwrap();
    release
}

#[test]
fn deputy_in_flight_blocks_key_removal_and_rotation_without_changing_vault_or_status() {
    let (temp, mut host) = setup();
    let provider = CredentialProvider::DeepSeek;
    host.set_provider_key(
        provider,
        Some(SecretInput::new("invalid-local-deputy-fixture".into()).unwrap()),
    )
    .unwrap();
    let key_path = temp.path().join("data/credentials/deepseek.dpapi");
    let original = fs::read(&key_path).unwrap();
    let connection = serde_json::to_value(&host.provider_connections[&provider]).unwrap();
    let release = blocked_deputy(&mut host, "key-guard");
    let removal = host.set_provider_key(provider, None);
    let rotation = host.set_provider_key(
        provider,
        Some(SecretInput::new("invalid-local-replacement-fixture".into()).unwrap()),
    );
    let unchanged = fs::read(&key_path).ok().as_deref() == Some(original.as_slice());
    let status = serde_json::to_value(&host.provider_connections[&provider]).unwrap();
    let other_provider = host.set_provider_key(CredentialProvider::Kimi, None);
    release.send(()).unwrap();
    poll(&mut host);
    assert!(
        removal.is_err(),
        "in-flight deputy allowed credential deletion"
    );
    assert!(
        rotation.is_err(),
        "in-flight deputy allowed credential replacement"
    );
    assert!(
        unchanged,
        "rejected changes must preserve encrypted vault bytes"
    );
    assert_eq!(status, connection);
    assert!(host.deepseek_key_configured);
    assert!(
        other_provider.is_ok(),
        "unrelated provider should remain editable"
    );
    host.set_provider_key(provider, None).unwrap();
    assert!(!key_path.exists());
    assert!(!host.deepseek_key_configured);
}

#[test]
fn deputy_in_flight_blocks_model_probe_without_starting_thread() {
    let (temp, mut host) = setup();
    let provider = CredentialProvider::DeepSeek;
    host.set_provider_key(
        provider,
        Some(SecretInput::new("invalid-local-deputy-fixture".into()).unwrap()),
    )
    .unwrap();
    // Even on the buggy implementation, a wrongly started probe cannot reach
    // the network: loading this deliberately invalid local envelope must fail.
    let key_path = temp.path().join("data/credentials/deepseek.dpapi");
    fs::write(&key_path, b"invalid encrypted local fixture").unwrap();
    let connection = serde_json::to_value(&host.provider_connections[&provider]).unwrap();
    let release = blocked_deputy(&mut host, "probe-guard");
    let probe = host.check_provider_models(credentials::ProviderEndpoint::DeepSeek {});
    let started = host.provider_probes.contains_key(&provider);
    let status = serde_json::to_value(&host.provider_connections[&provider]).unwrap();
    release.send(()).unwrap();
    poll(&mut host);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !host.provider_probes.is_empty() {
        host.poll_verifications();
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(
        probe.is_err(),
        "in-flight deputy allowed another credential probe"
    );
    assert!(!started);
    assert_eq!(status, connection);
    assert_eq!(
        fs::read(&key_path).unwrap(),
        b"invalid encrypted local fixture"
    );
    host.set_provider_key(provider, None).unwrap();
}

#[test]
fn unrelated_openai_management_does_not_block_deepseek_key_changes() {
    let (_temp, mut host) = setup();
    let (intent, _) = prepared(&host, "original-openai");
    // Original OpenAI record with a coincident model name. Ownership must come
    // from the stored provider source, not from the model name or current setup.
    host.management.begin(intent).unwrap();
    let (_sender, receiver) = std::sync::mpsc::channel();
    host.management_active
        .insert("original-openai".into(), receiver);
    host.set_provider_key(
        CredentialProvider::DeepSeek,
        Some(SecretInput::new("invalid-local-other-provider".into()).unwrap()),
    )
    .unwrap();
    host.set_provider_key(CredentialProvider::DeepSeek, None)
        .unwrap();
}

#[test]
fn gateway_324_management_thread_completion_and_reopen_keep_original_evidence() {
    let temp = tempfile::tempdir().unwrap();
    gateway_324_management_fixture(temp.path());
}

fn gateway_324_management_fixture(root: &Path) -> serde_json::Value {
    use cerulean_adapters::api_generation::gateway::{GatewayHeaders, GatewayRequestId};
    use cerulean_domain::{
        credentials::ProviderEndpoint,
        management_ingress::ManagementRole,
        project_setup::{ChiefSource, DeputySource, ManagementSources},
    };
    let data = root.join("data");
    let (host, mut draft) = super::super::project_planning::tests::fixture(root);
    drop(host);
    crate::management_source_guard::tests::promote_fixture(&data);
    crate::relay_guard::tests::promote_fixture(&data, &root.join("before16"));
    let mut host = RuntimeHost::open(&data).unwrap();
    let endpoint = ProviderEndpoint::A6Api {};
    draft.chief_model = "gpt-6.1-sol".into();
    draft.deputy_model = "gpt-6.1-sol".into();
    draft.management_sources = Some(ManagementSources {
        chief: ChiefSource::Provider {
            endpoint: endpoint.clone(),
        },
        deputy: DeputySource::Provider {
            endpoint: endpoint.clone(),
        },
    });
    host.configure_project(draft).unwrap();
    let context = host
        .management_context("project", "Implement a sum function")
        .unwrap();
    let id = "management-thread-324";
    let intent = DecisionIntent {
        operation_id: id.into(),
        project_id: "project".into(),
        purpose: Purpose::Advise,
        model: "gpt-6.1-sol".into(),
        state_digest: digest(&context).unwrap(),
        context: context.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    let client = ApiClient::new(SecretInput::new("synthetic-thread-324".into()).unwrap()).unwrap();
    let prepared = deputy_management::prepare(
        &client,
        ManagementHandoff {
            operation_id: id.into(),
            project_id: "project".into(),
            purpose: intent.purpose,
            source: ManagementSource::Provider {
                endpoint,
                model: intent.model.clone(),
                role: ManagementRole::Deputy,
            },
            state_digest: intent.state_digest.clone(),
            context,
        },
    )
    .unwrap();
    let intent_path = data.join(format!("management-decisions/{id}.0.json"));
    host.launch_deepseek_management(intent, prepared, move |prepared, gateway| {
        assert!(intent_path.is_file(), "header callback must follow original Intent");
        gateway(&GatewayHeaders { status: 200, request_id: GatewayRequestId::Accepted {
            value: "management-thread-gateway-324".into(),
        }});
        deputy_management::fixture_result(&prepared, json!({
            "id":"management-thread-body-324", "model":"gpt-6.1-sol",
            "usage":{"prompt_tokens":3,"completion_tokens":2},
            "choices":[{"index":0,"finish_reason":"stop","message":{"role":"assistant","content":
                json!({"stateDigest":prepared.request().handoff.state_digest,
                    "decision":{"action":"wait","reason":"require independent approval"}}).to_string()}}]
        }))
    }).unwrap();
    poll(&mut host);
    assert_eq!(host.management_decisions()[0].state, DecisionState::Ready);
    assert!(host.management_count_errors.is_empty());
    let review = host.management.gateway_evidence(id).unwrap().unwrap();
    assert_eq!(review.status, "linked");
    assert_eq!(review.local_id, id);
    assert_eq!(
        review.gateway_request_id.as_deref(),
        Some("management-thread-gateway-324")
    );
    assert_eq!(
        review.body_response_id.as_deref(),
        Some("management-thread-body-324")
    );
    let response = data.join(format!("management-decisions/{id}.1.json"));
    let bytes = fs::read(&response).unwrap();
    let original = digest(&host.read_deepseek_management(id).unwrap()).unwrap();
    drop(host);
    let host = RuntimeHost::open(&data).unwrap();
    assert!(host.management_active.is_empty());
    assert_eq!(host.management_decisions()[0].state, DecisionState::Ready);
    assert_eq!(
        host.management.gateway_evidence(id).unwrap().unwrap(),
        review
    );
    assert_eq!(
        digest(&host.read_deepseek_management(id).unwrap()).unwrap(),
        original
    );
    assert_eq!(fs::read(response).unwrap(), bytes);
    assert_eq!(
        crate::data_format::status(&data)
            .unwrap()
            .minimum_reader_generation,
        16
    );
    let expected = serde_json::to_value(host.management_detail(id).unwrap()).unwrap();
    drop(host);
    serde_json::json!({
        "schema":1,"lane":"management","synthetic":true,"realModelCalls":0,
        "dataRoot":fs::canonicalize(&data).unwrap(),"ownerSid":crate::ipc::current_sid().unwrap(),
        "projectId":"project","operationId":id,"readerGeneration":16,
        "expectedOriginalDetail":expected,"expectedGateway":review,
        "originalDeputyDigest":original,"currentTestExecutable":std::env::current_exe().unwrap()
    })
}

#[cfg(windows)]
#[test]
#[ignore = "explicit fixed-path compatibility export; never automatic"]
fn gateway_324_export_management_compatibility_fixture() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let parent = repository.join("reports/gateway-id-324/compatibility");
    for ancestor in parent.ancestors() {
        crate::host::ordinary(ancestor, true).unwrap();
    }
    let case = fs::canonicalize(parent).unwrap().join("case-management");
    fs::create_dir(&case).unwrap();
    let mut export = gateway_324_management_fixture(&case);
    let archive = crate::backup::create(&case.join("data"), &case.join("current-backup")).unwrap();
    export["backupDigest"] = archive.manifest_digest.into();
    crate::api_execution::publish(&case, "export.json", &export).unwrap();
}

#[cfg(windows)]
#[test]
#[ignore = "explicit current-reader check after fixed-path old-reader cold restore"]
fn gateway_324_check_management_compatibility_restore() {
    let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let case = repository.join("reports/gateway-id-324/compatibility/case-management");
    for ancestor in case.ancestors() {
        crate::host::ordinary(ancestor, true).unwrap();
    }
    let case = fs::canonicalize(case).unwrap();
    let export: serde_json::Value = crate::api_execution::load(&case, "export.json").unwrap();
    assert_eq!(export["synthetic"], true);
    let data = case.join("data");
    assert_eq!(
        export["dataRoot"],
        serde_json::to_value(fs::canonicalize(&data).unwrap()).unwrap()
    );
    assert_eq!(export["ownerSid"], crate::ipc::current_sid().unwrap());
    let old: serde_json::Value = crate::api_execution::load(&case, "old-core-result.json").unwrap();
    assert_eq!(old["coldRestoreByteIdentical"], true);
    assert_eq!(old["oldManagementSemanticReadProven"], true);
    let host = RuntimeHost::open(&data).unwrap();
    let id = export["operationId"].as_str().unwrap();
    assert_eq!(
        serde_json::to_value(host.management_detail(id).unwrap()).unwrap(),
        export["expectedOriginalDetail"]
    );
    assert_eq!(
        serde_json::to_value(host.management.gateway_evidence(id).unwrap().unwrap()).unwrap(),
        export["expectedGateway"]
    );
    assert_eq!(
        digest(&host.read_deepseek_management(id).unwrap()).unwrap(),
        export["originalDeputyDigest"]
    );
    assert!(host.management_active.is_empty());
    drop(host);
    assert_eq!(
        crate::data_format::status(&data)
            .unwrap()
            .minimum_reader_generation,
        16
    );
    crate::api_execution::publish(
        &case,
        "current-restored-check.json",
        &serde_json::json!({
            "status":"passed","lane":"management","synthetic":true,"realModelCalls":0,
            "originalDetailEqual":true,"gatewayBindingEqual":true,"readerGeneration":16
        }),
    )
    .unwrap();
}
