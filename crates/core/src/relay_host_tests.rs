use super::*;
use cerulean_domain::{
    api_worker::ApiTarget, credentials::ProviderEndpoint, management::ManagementDecision,
    project_setup::ProjectWorkerAdapter,
};

#[test]
fn relay_project_approval_and_reopen_keep_endpoint_model_and_provider_bound() {
    relay_approval(ProviderEndpoint::A6Api {});
}

#[test]
fn relay_vip_project_approval_and_reopen_keep_distinct_origin() {
    relay_approval(ProviderEndpoint::A6ApiVip {});
}

fn relay_approval(endpoint: ProviderEndpoint) {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let (mut host, draft) = project_planning::tests::fixture(temp.path());
    let mut draft = api_project_tests::draft_for(
        draft,
        ProjectWorkerAdapter::A6api,
        ApiTarget::Provider {
            endpoint: endpoint.clone(),
        },
    );
    draft.api_worker.as_mut().unwrap().model = "gpt-6.1-sol".into();
    draft.worker_model = "gpt-6.1-sol".into();
    let before = crate::reader_generation::tests::snapshot(&data);
    assert!(host
        .configure_project(draft.clone())
        .unwrap_err()
        .contains("reader16"));
    assert_eq!(crate::reader_generation::tests::snapshot(&data), before);
    drop(host);
    crate::relay_guard::tests::promote_fixture(&data, &temp.path().join("cold"));
    let mut host = RuntimeHost::open(&data).unwrap();
    host.configure_project(draft.clone()).unwrap();
    let mut proposal = project_planning::tests::proposal("tests");
    if let ManagementDecision::Plan { tasks, .. } = &mut proposal {
        tasks[0].worker_id = "worker-a6api".into();
    }
    project_planning::tests::complete(&mut host, proposal);
    let revision = host.projects["project"].revision().unwrap().clone();
    assert_eq!(revision.execution[0].adapter_id, "a6api");
    assert_eq!(revision.execution[0].api_worker, draft.api_worker);
    let digest = revision.approval_digest().unwrap();
    host.approve("project", &digest).unwrap();
    let mut altered = revision.clone();
    altered.execution[0].api_worker.as_mut().unwrap().target = ApiTarget::Provider {
        endpoint: if matches!(endpoint, ProviderEndpoint::A6Api {}) {
            ProviderEndpoint::A6ApiVip {}
        } else {
            ProviderEndpoint::A6Api {}
        },
    };
    assert_ne!(altered.approval_digest().unwrap(), digest);
    let mut altered = revision.clone();
    altered.execution[0].api_worker.as_mut().unwrap().model = "another-model".into();
    assert_ne!(altered.approval_digest().unwrap(), digest);
    altered.execution[0].api_worker.as_mut().unwrap().target = ApiTarget::OpenAi {};
    assert!(altered.validate().is_err());
    assert!(host.api_startups.is_empty());
    let history = host.projects["project"].history().to_vec();
    drop(host);
    let host = RuntimeHost::open(&data).unwrap();
    assert_eq!(host.projects["project"].history(), history);
    assert_eq!(host.projects["project"].revision(), Some(&revision));
    assert_eq!(
        host.projects["project"].approval().unwrap().revision_digest,
        digest
    );
    assert!(!host.openai_connection.key_configured);
    assert!(host.api_startups.is_empty());
}
