use super::*;
use cerulean_domain::{
    api_worker::{ApiTarget, ApiWorkerSelection},
    credentials::ProviderEndpoint,
    management::ManagementDecision,
    project_setup::*,
};

pub(super) fn routes() -> Vec<(ProjectWorkerAdapter, ApiTarget)> {
    use ProjectWorkerAdapter::*;
    let provider = |endpoint| ApiTarget::Provider { endpoint };
    vec![
        (Openai, ApiTarget::OpenAi {}),
        (Anthropic, provider(ProviderEndpoint::Anthropic {})),
        (Deepseek, provider(ProviderEndpoint::DeepSeek {})),
        (
            Qwen,
            provider(ProviderEndpoint::QwenBeijing {
                workspace_id: "approved-space".into(),
            }),
        ),
        (Qwen, provider(ProviderEndpoint::QwenSingapore {})),
        (Qwen, provider(ProviderEndpoint::QwenHongKong {})),
        (Kimi, provider(ProviderEndpoint::KimiChina {})),
        (Grok, provider(ProviderEndpoint::Grok {})),
    ]
}
fn selection(target: ApiTarget) -> ApiWorkerSelection {
    ApiWorkerSelection {
        write_scope: None,
        target,
        model: "vendor/fixture-model".into(),
        file_tools: vec![ToolPermission::ReadFiles, ToolPermission::WriteFiles],
        max_rounds: 3,
        max_tool_calls: 8,
        max_output_tokens: 512,
        max_total_tokens: 4096,
        request_timeout_seconds: 30,
    }
}
pub(super) fn draft_for(
    mut draft: ProjectSetupDraft,
    adapter: ProjectWorkerAdapter,
    target: ApiTarget,
) -> ProjectSetupDraft {
    let api = selection(target);
    draft.worker_adapter = adapter;
    draft.worker_model = api.model.clone();
    draft.codex_executable.clear();
    draft.api_worker = Some(api);
    draft
}
#[test]
fn api_project_routes_save_plan_approve_reopen_without_executable_or_key() {
    for (adapter, target) in routes() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, draft) = project_planning::tests::fixture(root.path());
        let draft = draft_for(draft, adapter, target);
        host.configure_project(draft.clone()).unwrap();
        let saved = host.project_setups["project"].clone();
        assert!(saved.setup.worker.is_none());
        assert_eq!(saved.setup.api_worker, draft.api_worker);
        let serialized = serde_json::to_value(&saved.setup).unwrap();
        assert!(serialized.get("worker").is_none());
        assert!(!host.openai_connection.key_configured);
        let mut proposal = project_planning::tests::proposal("tests");
        if let ManagementDecision::Plan { tasks, .. } = &mut proposal {
            tasks[0].worker_id = adapter.worker_id();
        }
        project_planning::tests::complete(&mut host, proposal);
        let revision = host.projects["project"].revision().unwrap().clone();
        assert_eq!(revision.execution[0].api_worker, draft.api_worker);
        assert!(revision.execution[0].worker.is_none());
        assert_eq!(revision.execution[0].adapter_id, adapter.id());
        let approved = revision.approval_digest().unwrap();
        host.approve("project", &approved).unwrap();
        // Missing local management credentials must not create run authorization or billed work.
        let events = host.projects["project"].history().len();
        let requests = host.management.statuses().len();
        let error = host
            .start_project_run("project", &approved, &saved.digest)
            .unwrap_err();
        assert!(error.contains("正常执行必须"));
        assert!(host.runs.all().is_empty());
        assert_eq!(host.projects["project"].history().len(), events);
        assert_eq!(host.management.statuses().len(), requests);
        drop(host);
        let host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(host.project_setups["project"].digest, saved.digest);
        assert_eq!(
            host.projects["project"]
                .revision()
                .unwrap()
                .approval_digest()
                .unwrap(),
            approved
        );
        assert_eq!(
            host.projects["project"].approval().unwrap().revision_digest,
            approved
        );
    }
}
#[test]
fn api_project_rejects_mixed_or_unsupported_configuration_before_publication() {
    let root = tempfile::tempdir().unwrap();
    let (mut host, original) = project_planning::tests::fixture(root.path());
    let base = draft_for(
        original.clone(),
        ProjectWorkerAdapter::Deepseek,
        ApiTarget::Provider {
            endpoint: ProviderEndpoint::DeepSeek {},
        },
    );
    for mode in 0..10 {
        let mut bad = base.clone();
        match mode {
            0 => bad.api_worker = None,
            1 => bad.worker_adapter = ProjectWorkerAdapter::Omp,
            2 => bad.worker_adapter = ProjectWorkerAdapter::Grok,
            3 => bad.codex_executable = original.codex_executable.clone(),
            4 => bad.worker_model = "different".into(),
            5 => bad.omp_runtime_parent = Some("D:\\unused".into()),
            6 => {
                // API delivery is now supported, but cannot be enabled with
                // zero approved advice requests.
                bad.supervision = Some(cerulean_domain::supervision::SupervisionPolicy {
                    schema_version: 1,
                    repeated_failure_threshold: 2,
                    max_advice_per_attempt: 0,
                    delivery_enabled: true,
                })
            }
            7 => bad
                .api_worker
                .as_mut()
                .unwrap()
                .file_tools
                .push(ToolPermission::RunCommands),
            8 => bad.api_worker.as_mut().unwrap().max_total_tokens = 0,
            9 => bad.chief_model = "unapproved/model-format".into(),
            _ => unreachable!(),
        }
        assert!(host.configure_project(bad).is_err(), "case {mode}");
        assert!(host.project_setups.is_empty());
        assert_eq!(
            fs::read_dir(host.root.join("project-setups"))
                .unwrap()
                .count(),
            0
        );
    }
    host.configure_project(base).unwrap();
    project_planning::tests::complete(&mut host, project_planning::tests::proposal("tests"));
    assert!(host.projects["project"].revision().is_none());
}
#[test]
fn api_approval_binds_every_configured_api_field() {
    let root = tempfile::tempdir().unwrap();
    let (mut host, draft) = project_planning::tests::fixture(root.path());
    host.configure_project(draft_for(
        draft,
        ProjectWorkerAdapter::Qwen,
        ApiTarget::Provider {
            endpoint: ProviderEndpoint::QwenBeijing {
                workspace_id: "first".into(),
            },
        },
    ))
    .unwrap();
    let mut proposal = project_planning::tests::proposal("tests");
    if let ManagementDecision::Plan { tasks, .. } = &mut proposal {
        tasks[0].worker_id = "worker-qwen".into();
    }
    project_planning::tests::complete(&mut host, proposal);
    let original = host.projects["project"].revision().unwrap().clone();
    let approved = original.approval_digest().unwrap();
    for mode in 0..9 {
        let mut changed = original.clone();
        let api = changed.execution[0].api_worker.as_mut().unwrap();
        match mode {
            0 => {
                api.target = ApiTarget::Provider {
                    endpoint: ProviderEndpoint::QwenSingapore {},
                }
            }
            1 => {
                api.target = ApiTarget::Provider {
                    endpoint: ProviderEndpoint::QwenBeijing {
                        workspace_id: "second".into(),
                    },
                }
            }
            2 => api.model = "changed".into(),
            3 => api.file_tools = vec![ToolPermission::ReadFiles],
            4 => api.max_rounds += 1,
            5 => api.max_tool_calls += 1,
            6 => api.max_output_tokens += 1,
            7 => api.max_total_tokens += 1,
            8 => api.request_timeout_seconds += 1,
            _ => unreachable!(),
        }
        changed.validate().unwrap();
        assert_ne!(changed.approval_digest().unwrap(), approved, "case {mode}");
    }
}
#[test]
fn api_project_reopen_rejects_rehashed_mixed_worker_record() {
    let root = tempfile::tempdir().unwrap();
    let (mut host, draft) = project_planning::tests::fixture(root.path());
    host.configure_project(draft.clone()).unwrap();
    let local = host.project_setups["project"].setup.worker.clone();
    host.configure_project(draft_for(
        draft,
        ProjectWorkerAdapter::Openai,
        ApiTarget::OpenAi {},
    ))
    .unwrap();
    let mut saved = host.project_setups["project"].clone();
    saved.setup.worker = local;
    saved.digest = digest(&saved.setup).unwrap();
    let path = host.root.join("project-setups/project.00000002.json");
    drop(host);
    fs::write(path, serde_json::to_vec(&saved).unwrap()).unwrap();
    assert!(RuntimeHost::open(&root.path().join("data")).is_err());
}
