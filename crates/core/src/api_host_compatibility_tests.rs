//! Full Host compatibility fixture; every response is synthetic and no HTTP is sent.
use super::*;
use cerulean_adapters::api_generation::{
    gateway::{GatewayHeaders, GatewayRequestId},
    ApiFailure, ApiReply, GatewayObserver, PreparedRequest,
};
use cerulean_domain::{
    api_jobs::ApiJobState,
    api_worker::ApiTarget,
    credentials::{CredentialProvider, ProviderEndpoint},
    project_setup::ProjectWorkerAdapter,
    workspace::{WorkspaceJobKind, WorkspaceQuery},
};
use serde_json::{json, Value};
use std::time::Duration;

fn gateway_reply(
    prepared: &PreparedRequest,
    round: u32,
    gateway: &mut GatewayObserver<'_>,
) -> Result<ApiReply, ApiFailure> {
    assert_eq!(
        prepared.fingerprint().target.provider(),
        CredentialProvider::A6Api
    );
    assert!((1..=2).contains(&round));
    gateway(&GatewayHeaders {
        status: 200,
        request_id: GatewayRequestId::Accepted {
            value: format!("gateway-host-327-{round}"),
        },
    });
    api_host_tests::reply(prepared, round)
}

fn read_views(host: &RuntimeHost) -> Value {
    let query = |query| {
        serde_json::to_value(host.workspace_query(&query).unwrap()).unwrap()["value"].clone()
    };
    let jobs = query(WorkspaceQuery::Jobs {
        project_id: "project".into(),
        offset: 0,
        revision: None,
    });
    assert_eq!(jobs["total"], 1);
    let job = query(WorkspaceQuery::Job {
        project_id: "project".into(),
        kind: WorkspaceJobKind::Api,
        id: "attempt".into(),
        revision: jobs["revision"].as_str().unwrap().into(),
    });
    let tasks = query(WorkspaceQuery::Tasks {
        project_id: "project".into(),
        offset: 0,
        revision: None,
    });
    let task = query(WorkspaceQuery::Task {
        project_id: "project".into(),
        task_id: "task".into(),
        revision: tasks["revision"].as_str().unwrap().into(),
    });
    let review = query(WorkspaceQuery::ExecutionReview {
        project_id: "project".into(),
        digest: None,
    });
    json!({"expectedJobsPage":jobs,"expectedJobDetail":job,
        "expectedApiJob":job["record"]["value"],"expectedTasksPage":tasks,
        "expectedTaskDetail":task,"expectedExecutionReview":review})
}

fn check_links(host: &RuntimeHost) {
    let store = &host.projects["project"];
    crate::api_execution::CallInventory::read(store)
        .unwrap()
        .check(None)
        .unwrap();
    let attempt = store.api_attempt_path("attempt").unwrap();
    for round in 1..=2 {
        let journal = crate::api_call_journal::ApiCallJournal::read_existing(
            &attempt.join(format!("rounds/{round:03}")),
        )
        .unwrap();
        let review = journal.gateway_evidence(&attempt).unwrap().unwrap();
        assert_eq!(review.status, "linked");
        assert_eq!(
            review.gateway_request_id,
            Some(format!("gateway-host-327-{round}"))
        );
        assert_eq!(review.local_id, journal.identity().request_id);
        assert_eq!(review.body_response_id, Some(format!("chat-{round}")));
        assert_ne!(review.gateway_request_id, review.body_response_id);
    }
}

fn fixture(root: &Path) -> Value {
    let data = root.join("data");
    // Create/promote a real empty root before the existing full Host helper.
    // No baseline binding or authoritative journal is fabricated or rewritten.
    drop(Store::open(&data).unwrap());
    crate::relay_guard::tests::promote_fixture(&data, &root.join("reader16-cold"));
    crate::rejected_usage_guard::tests::promote_fixture(&data, &root.join("reader17-cold"));
    let mut host = api_host_tests::fixture(
        root,
        ProjectWorkerAdapter::A6api,
        ApiTarget::Provider {
            endpoint: ProviderEndpoint::A6Api {},
        },
        true,
    );
    host.api_fixture = None;
    host.api_gateway_fixture = Some(gateway_reply);
    host.start_managed_api("deputy-api", "attempt").unwrap();
    let attempt_path = data
        .join("workflows/project")
        .join(format!("api-attempt-{}", digest(&"attempt").unwrap()));
    let update = host.api_active[&("project".into(), "attempt".into())]
        .receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    let crate::async_api::Update::Assignment(crate::managed_assignment::Update::Prepare {
        operation,
        attempt,
        snapshot,
        reply,
    }) = update
    else {
        panic!("expected durable Prepare ACK");
    };
    assert!(!attempt_path.exists());
    assert!(host.management.application("deputy-api").is_none());
    assert_eq!(
        host.api_writers["project"]
            .owner
            .snapshot()
            .unwrap()
            .state
            .tasks["task"]
            .state,
        TaskState::Pending
    );
    let result =
        host.prepare_managed_assignment("project", &operation, *attempt, *snapshot, "a6api");
    assert!(result.is_ok(), "{result:?}");
    reply.send(result).unwrap();
    let update = host.api_active[&("project".into(), "attempt".into())]
        .receiver
        .recv_timeout(Duration::from_secs(5))
        .unwrap();
    let crate::async_api::Update::Assignment(crate::managed_assignment::Update::Commit {
        operation,
        entry,
        snapshot,
        reply,
    }) = update
    else {
        panic!("expected durable Commit ACK");
    };
    assert!(!attempt_path.exists());
    assert!(!host.management.application("deputy-api").unwrap().1);
    assert_eq!(
        host.api_writers["project"]
            .owner
            .snapshot()
            .unwrap()
            .state
            .tasks["task"]
            .state,
        TaskState::Assigned
    );
    let result = host.commit_managed_assignment("project", &operation, *entry, *snapshot, "a6api");
    assert!(result.is_ok(), "{result:?}");
    reply.send(result).unwrap();
    api_host_tests::drain(&mut host);
    assert_eq!(
        host.snapshot().api_jobs[0].state,
        ApiJobState::Submitted,
        "{:?}",
        host.snapshot().api_jobs[0]
    );
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Submitted
    );
    assert_eq!(
        fs::read_to_string(project_planning::tests::worker_root(root).join("result.txt")).unwrap(),
        "background fixture result"
    );
    assert!(host.management.application("deputy-api").unwrap().1);
    assert!(host.start_managed_api("deputy-api", "another").is_err());
    check_links(&host);
    // Reuse the existing 280/efficiency fixture disposition. Submitted is not
    // Accepted and intentionally blocks normal service exit until explicitly
    // retired by the local user; never bypass that production protection.
    assert!(!host.can_shutdown());
    let review = host
        .project_revision_candidates()
        .into_iter()
        .find(|r| r.project_id == "project")
        .unwrap();
    assert_eq!(review.submitted_tasks, 1);
    assert_eq!(review.verified_submissions, 0);
    let original_history = host.projects["project"].history().to_vec();
    host.retire_submitted_plan(
        "project",
        &review.evidence_digest,
        "327 synthetic fixture: preserve unaccepted submission and end this plan",
    )
    .unwrap();
    assert_eq!(
        &host.projects["project"].history()[..original_history.len()],
        original_history.as_slice()
    );
    assert_eq!(
        host.projects["project"].history().len(),
        original_history.len() + 1
    );
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Submitted
    );
    assert!(!host.projects["project"].state().tasks["task"].verified);
    let expected = read_views(&host);
    // Remove only this fixture's known synthetic secret before any cold export.
    crate::credential_vault::remove_for(&data, CredentialProvider::A6Api).unwrap();
    assert!(!crate::credential_vault::exists_for(&data, CredentialProvider::A6Api).unwrap());
    assert!(host.can_shutdown());
    drop(host);
    for _ in 0..2 {
        let mut host = RuntimeHost::open(&data).unwrap();
        assert_eq!(read_views(&host), expected);
        check_links(&host);
        assert!(host.api_active.is_empty());
        assert!(host.start_managed_api("deputy-api", "another").is_err());
        assert_eq!(read_views(&host), expected);
        assert!(host.can_shutdown());
    }
    crate::derived_index::wait_for_test_projections(&data);
    expected
}

struct RetainOnFailure(Option<tempfile::TempDir>);
impl Drop for RetainOnFailure {
    fn drop(&mut self) {
        if std::thread::panicking() {
            if let Some(temp) = self.0.take() {
                eprintln!("327 synthetic Host failure retained at {:?}", temp.keep());
            }
        }
    }
}

#[test]
fn gateway_327_full_host_acks_links_and_reopen_without_resend() {
    let root = RetainOnFailure(Some(tempfile::tempdir().unwrap()));
    fixture(root.0.as_ref().unwrap().path());
}

fn parent() -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("reports/api-host-compatibility-327");
    for path in root.ancestors() {
        ordinary(path, true).unwrap();
    }
    fs::canonicalize(root).unwrap()
}

fn inventory(root: &Path) -> (BTreeMap<String, String>, Vec<String>) {
    use sha2::{Digest, Sha256};
    fn walk(
        root: &Path,
        current: &Path,
        files: &mut BTreeMap<String, String>,
        dirs: &mut Vec<String>,
    ) {
        for entry in fs::read_dir(current).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let directory = entry.file_type().unwrap().is_dir();
            ordinary(&path, directory).unwrap();
            let name = path
                .strip_prefix(root)
                .unwrap()
                .to_str()
                .unwrap()
                .replace('\\', "/");
            if directory {
                dirs.push(name);
                walk(root, &path, files, dirs);
            } else {
                files.insert(
                    name,
                    format!("{:x}", Sha256::digest(fs::read(&path).unwrap())),
                );
            }
        }
    }
    let (mut files, mut dirs) = (BTreeMap::new(), Vec::new());
    walk(root, root, &mut files, &mut dirs);
    dirs.sort();
    (files, dirs)
}

#[test]
#[ignore = "explicit fixed-path full Host export; synthetic, no HTTP"]
fn gateway_327_export_full_host_fixture() {
    let case = parent().join("case");
    fs::create_dir(&case).unwrap();
    let expected = fixture(&case);
    let data = case.join("data");
    let project = case.join("project");
    let (files, dirs) = inventory(&data);
    let (project_files, project_dirs) = inventory(&project);
    assert!(!files.keys().any(|p| p.starts_with("credentials/")));
    let sidecars: BTreeMap<_, _> = files
        .iter()
        .filter(|(p, _)| p.split('/').any(|s| s == "gateway-response-evidence"))
        .map(|(p, h)| (p.clone(), h.clone()))
        .collect();
    assert_eq!(sidecars.len(), 4);
    let mut export = expected;
    export.as_object_mut().unwrap().extend(json!({
        "schema":1,"lane":"api-host","synthetic":true,"realModelCalls":0,
        "credentialsRemoved":true,"syntheticResponses":2,"durableAssignmentAcks":2,
        "dataRoot":fs::canonicalize(&data).unwrap(),"projectRoot":fs::canonicalize(&project).unwrap(),
        "ownerSid":crate::ipc::current_sid().unwrap(),"readerGeneration":16,
        "projectId":"project","taskId":"task","attemptId":"attempt",
        "dataFiles":files,"dataDirectories":dirs,"projectFiles":project_files,
        "projectDirectories":project_dirs,"sidecarFiles":sidecars,
        "currentTestExecutable":std::env::current_exe().unwrap(),
        "submittedPlanRetired":true,"independentVerificationPerformed":false,"releaseAccepted":false
    }).as_object().unwrap().clone());
    crate::api_execution::publish(&case, "export.json", &export).unwrap();
}

#[test]
#[ignore = "explicit original full Host read after fixed 315 reader, no execution"]
fn gateway_327_check_original_after_old_host() {
    let case = parent().join("case");
    let export: Value = crate::api_execution::load(&case, "export.json").unwrap();
    assert_eq!(export["synthetic"], true);
    assert_eq!(export["credentialsRemoved"], true);
    assert_eq!(export["ownerSid"], crate::ipc::current_sid().unwrap());
    let data = case.join("data");
    assert_eq!(export["dataRoot"], json!(fs::canonicalize(&data).unwrap()));
    let host = RuntimeHost::open(&data).unwrap();
    let expected = read_views(&host);
    for (name, value) in expected.as_object().unwrap() {
        assert_eq!(&export[name], value);
    }
    check_links(&host);
    assert!(host.api_active.is_empty());
    assert!(!crate::credential_vault::exists_for(&data, CredentialProvider::A6Api).unwrap());
    drop(host);
    crate::derived_index::wait_for_test_projections(&data);
    let (files, _) = inventory(&data);
    for (path, hash) in export["sidecarFiles"].as_object().unwrap() {
        assert_eq!(json!(files[path]), *hash);
    }
    crate::api_execution::publish(
        &case,
        "current-readback.json",
        &json!({
            "status":"passed","originalViewsEqual":true,"linkedGatewayRecords":2,
            "realModelCalls":0,"resends":0,"releaseAccepted":false
        }),
    )
    .unwrap();
}
