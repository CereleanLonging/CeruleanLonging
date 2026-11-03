//! Explicit configuration, real local ledgers; no provider calls or workers.
use super::*;
use cerulean_domain::{
    management::{DecisionEnvelope, Purpose},
    management_ingress::ExternalChiefDecision,
    project_setup::{ChiefSource, DeputySource, ManagementSources, ProjectRun, RunState},
};

fn sources() -> ManagementSources {
    ManagementSources {
        chief: ChiefSource::CodexSession {
            session_id: "current-session".into(),
        },
        deputy: DeputySource::DeepSeek {},
    }
}
fn configured(root: &Path) -> RuntimeHost {
    let (host, mut draft) = super::project_planning::tests::fixture(root);
    drop(host);
    crate::management_source_guard::tests::promote_fixture(&root.join("data"));
    let mut host = RuntimeHost::open(&root.join("data")).unwrap();
    draft.management_sources = Some(sources());
    draft.chief_model = "external-codex-session".into();
    draft.deputy_model = "deepseek-flash".into();
    super::project_planning::tests::isolate_draft(root, &mut draft);
    host.configure_project(draft).unwrap();
    host
}
#[test]
fn management_configuration_plan_wait_reopen_and_approval_binding() {
    let root = tempfile::tempdir().unwrap();
    let mut host = configured(root.path());
    let setup = host.project_setups["project"].clone();
    assert!(!host.openai_connection.key_configured);
    host.request_chief_plan("project", &setup.digest).unwrap();
    let status = host.management_decisions().remove(0);
    assert_eq!(
        status.state,
        crate::management_execution::DecisionState::AwaitingExternal
    );
    let request = host
        .read_external_chief_request(&status.operation_id)
        .unwrap();
    assert!(host.request_chief_plan("project", &setup.digest).is_err());
    assert!(host.management_active.is_empty());
    drop(host);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(host.project_setups["project"].digest, setup.digest);
    host.submit_external_chief_decision(
        &request.operation_id,
        "current-session",
        ExternalChiefDecision {
            request_digest: digest(&request).unwrap(),
            session_id: "current-session".into(),
            envelope: DecisionEnvelope {
                state_digest: request.state_digest.clone(),
                decision: super::project_planning::tests::proposal("tests"),
            },
        },
    )
    .unwrap();
    assert!(host.projects["project"].approval().is_none());
    let revision = host.projects["project"].revision().unwrap().clone();
    assert_eq!(
        revision.management_setup_digest.as_ref(),
        Some(&setup.digest)
    );
    let approved = revision.approval_digest().unwrap();
    let mut other = revision.clone();
    other.management_setup_digest = Some("b".repeat(64));
    assert_ne!(other.approval_digest().unwrap(), approved);
    host.approve("project", &approved).unwrap();
    let mut run = ProjectRun {
        id: "fixture-run".into(),
        project_id: "project".into(),
        revision_digest: approved.clone(),
        setup_digest: setup.digest.clone(),
        state: RunState::Active,
        message: "controlled check".into(),
        acknowledged_decisions: 1,
    };
    host.run_scope(&run).unwrap();
    assert_eq!(
        host.start_project_run("project", &approved, &setup.digest)
            .unwrap_err(),
        "请配置所选指挥供应商的本机凭证"
    );
    assert!(host.runs.all().is_empty());
    assert!(host.management_active.is_empty() && host.codex_active.is_empty());
    let mut changed = setup.clone();
    changed.setup.deputy_model = "changed-model".into();
    changed.digest = digest(&changed.setup).unwrap();
    run.setup_digest = changed.digest.clone();
    host.project_setups.insert("project".into(), changed);
    assert!(host.run_scope(&run).unwrap_err().contains("批准方案绑定"));
    host.project_setups.insert("project".into(), setup);
    drop(host);
    let host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(
        host.projects["project"]
            .revision()
            .unwrap()
            .approval_digest()
            .unwrap(),
        approved
    );
    assert_eq!(host.management_operation_ids("project").len(), 1);
    assert!(host.management_active.is_empty() && host.codex_active.is_empty());
}
#[test]
fn management_configuration_rejects_wrong_session_provider_and_model() {
    let root = tempfile::tempdir().unwrap();
    let mut host = configured(root.path());
    let setup = host.project_setups["project"].digest.clone();
    assert!(host
        .prepare_external_chief_plan("project", &setup, "different-session")
        .is_err());
    assert!(host
        .start_deepseek_management(
            "project",
            "wrong-model",
            Purpose::Assign,
            "other-model",
            "requirement"
        )
        .unwrap_err()
        .contains("来源或模型不符"));
    let context = host.management_context("project", "requirement").unwrap();
    let intent = crate::management_execution::DecisionIntent {
        operation_id: "wrong-provider".into(),
        project_id: "project".into(),
        purpose: Purpose::Assign,
        model: "deepseek-flash".into(),
        state_digest: digest(&context).unwrap(),
        context,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    assert!(host
        .launch_authenticated_management(intent)
        .unwrap_err()
        .contains("不允许回退"));
    assert!(host.management_operation_ids("project").is_empty());
}
#[test]
fn management_configuration_reader_guard_legacy_bytes_and_validation() {
    let root = tempfile::tempdir().unwrap();
    let (mut host, mut draft) = super::project_planning::tests::fixture(root.path());
    let legacy = serde_json::to_value(&draft).unwrap();
    assert!(legacy.get("managementSources").is_none());
    assert_eq!(
        digest(&draft).unwrap(),
        digest(
            &serde_json::from_value::<cerulean_domain::project_setup::ProjectSetupDraft>(
                legacy.clone()
            )
            .unwrap()
        )
        .unwrap()
    );
    draft.management_sources = Some(sources());
    assert!(draft.validate().is_err());
    draft.chief_model = "external-codex-session".into();
    draft.validate().unwrap();
    assert!(host.configure_project(draft.clone()).is_err());
    assert!(host.project_setups.is_empty());
    assert!(sources().validate(&draft.chief_model, true).is_err());
    let mut invalid = serde_json::to_value(&draft).unwrap();
    invalid["managementSources"]["chief"]["kind"] = "deepSeek".into();
    assert!(
        serde_json::from_value::<cerulean_domain::project_setup::ProjectSetupDraft>(invalid)
            .is_err()
    );
    let mut invalid = serde_json::to_value(&draft).unwrap();
    invalid["managementSources"]["apiKey"] = "invalid-fixture-value".into();
    assert!(
        serde_json::from_value::<cerulean_domain::project_setup::ProjectSetupDraft>(invalid)
            .is_err()
    );
    draft.management_sources = None;
    draft.chief_model = "fixture-chief".into();
    assert_eq!(serde_json::to_value(&draft).unwrap(), legacy);
}
