//! The bounded desktop read protocol over a real authenticated Windows pipe.
use super::*;
use cerulean_core::ipc::{Client, ServiceReply, UserOperation};
use cerulean_domain::workspace::{WorkspaceQuery, WorkspaceResult};

async fn read(data: &Path, query: WorkspaceQuery) -> ServiceReply {
    Client::connect(data)
        .await
        .unwrap()
        .call(UserOperation::Workspace {
            id: "bounded-recheck".into(),
            protocol_version: PROTOCOL_VERSION,
            query,
        })
        .await
        .unwrap()
}
fn list(offset: u32, revision: Option<String>) -> WorkspaceQuery {
    WorkspaceQuery::VerificationRechecks {
        project_id: "project".into(),
        offset,
        revision,
    }
}
fn lookup(operation: &str) -> WorkspaceQuery {
    WorkspaceQuery::VerificationRecheck {
        project_id: "project".into(),
        operation_id: operation.into(),
    }
}
fn page(reply: ServiceReply) -> cerulean_domain::workspace::WorkspaceRecheckPage {
    assert!(reply.response.error.is_none(), "{:?}", reply.response.error);
    assert!(
        reply.response.result.is_none(),
        "bounded reads cannot return a full snapshot"
    );
    let Some(WorkspaceResult::VerificationRechecks(p)) = reply.workspace else {
        panic!("page missing")
    };
    p
}
fn status(reply: ServiceReply) -> RecheckStatus {
    assert!(reply.response.error.is_none(), "{:?}", reply.response.error);
    assert!(reply.response.result.is_none());
    let Some(WorkspaceResult::VerificationRecheck(s)) = reply.workspace else {
        panic!("status missing")
    };
    *s
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "explicit ordinary Windows accounts; run the native recheck suite"]
async fn bounded_reads_discover_unstarted_approval_and_reopen_without_execution() {
    let (root, store, a) = runner::corrected("corrected_checker");
    drop(store);
    let data = root.path().join("data");
    let mut host = RuntimeHost::open(&data).unwrap();
    let tool = a.review.replacement_checks[0].tool.as_ref().unwrap();
    let registration = host.handle(Request {
        id: "register-recheck-fixture".into(),
        protocol_version: PROTOCOL_VERSION,
        command: Command::RegisterValidator {
            id: "corrected-tool".into(),
            name: "Corrected fixture tool".into(),
            executable: tool.executable.path.clone(),
            inputs: tool.inputs.iter().map(|f| f.path.clone()).collect(),
            environment: tool.environment.clone(),
        },
    });
    assert!(
        registration.error.is_some(),
        "additional checker registration must remain disabled"
    );
    let owned = data.clone();
    let service = tokio::spawn(async move { cerulean_core::ipc::serve_host(&owned, host).await });
    interface::connect(&data).await;
    let empty = page(read(&data, list(0, None)).await);
    assert_eq!(empty.total, 0);
    let inputs_query = || WorkspaceQuery::VerificationRecheckInputs {
        project_id: "project".into(),
        original_operation_id: a.review.original_operation_id.clone(),
    };
    let inputs = read(&data, inputs_query()).await;
    assert!(inputs.response.result.is_none());
    assert!(
        inputs.response.error.is_none(),
        "{:?}",
        inputs.response.error
    );
    let Some(WorkspaceResult::VerificationRecheckInputs(inputs)) = inputs.workspace else {
        panic!("inputs missing")
    };
    assert_eq!(inputs.original_job, a.original_job);
    assert_eq!(
        inputs.original_checks_digest,
        a.review.original_checks_digest
    );
    assert_eq!(
        digest(&inputs.checks).unwrap(),
        a.review.original_checks_digest
    );
    assert_eq!(inputs.revision_digest, a.review.revision_digest);
    assert_eq!(
        inputs.checks.iter().map(|c| &c.id).collect::<Vec<_>>(),
        a.review
            .replacement_checks
            .iter()
            .map(|c| &c.id)
            .collect::<Vec<_>>()
    );
    let v = inputs
        .validators
        .iter()
        .find(|v| v.id == cerulean_domain::objective_checker::ID)
        .unwrap();
    let registered = read(
        &data,
        WorkspaceQuery::RegisteredValidator {
            id: v.id.clone(),
            digest: v.digest.clone(),
        },
    )
    .await;
    let Some(WorkspaceResult::RegisteredValidator(registered)) = registered.workspace else {
        panic!("validator missing")
    };
    assert_eq!(registered.tool.executable, tool.executable);
    assert!(registered.tool.inputs.is_empty());
    assert!(registered.tool.environment.is_empty());
    assert_eq!(digest(registered.as_ref()).unwrap(), v.digest);
    for q in [
        WorkspaceQuery::RegisteredValidator {
            id: v.id.clone(),
            digest: "0".repeat(64),
        },
        WorkspaceQuery::RegisteredValidator {
            id: "missing".into(),
            digest: v.digest.clone(),
        },
        WorkspaceQuery::VerificationRecheckInputs {
            project_id: "project".into(),
            original_operation_id: "missing".into(),
        },
        WorkspaceQuery::CurrentVerificationRecheck {
            project_id: "missing".into(),
        },
    ] {
        assert!(read(&data, q).await.response.error.is_some());
    }
    assert!(matches!(
        read(
            &data,
            WorkspaceQuery::CurrentVerificationRecheck {
                project_id: "project".into()
            }
        )
        .await
        .workspace,
        Some(WorkspaceResult::CurrentVerificationRecheck(None))
    ));
    let reviewed = read(
        &data,
        WorkspaceQuery::VerificationRecheckReview {
            input: Box::new(RecheckRequest {
                project_id: "project".into(),
                original_operation_id: a.review.original_operation_id.clone(),
                replacement_checks: a.review.replacement_checks.clone(),
                reason: a.review.reason.clone(),
            }),
        },
    )
    .await;
    assert!(
        reviewed.response.error.is_none(),
        "{:?}",
        reviewed.response.error
    );
    let Some(WorkspaceResult::VerificationRecheckReview(review)) = reviewed.workspace else {
        panic!("review missing")
    };
    assert_eq!(*review, a.review);
    assert!(reviewed.response.result.is_none());
    let approved = interface::call(
        &data,
        Command::ApproveVerificationRecheck {
            review,
            approval_operation_id: "workspace-approve".into(),
            operation_id: a.operation_id.clone(),
        },
    )
    .await;
    assert!(
        approved.response.error.is_none(),
        "{:?}",
        approved.response.error
    );
    assert!(read(&data, list(0, Some(empty.revision)))
        .await
        .response
        .error
        .is_some());
    let first = page(read(&data, list(0, None)).await);
    assert_eq!(first.total, 1);
    assert_eq!(first.entries.len(), 1);
    assert_eq!(first.next_offset, None);
    let item = &first.entries[0];
    assert_eq!(item.operation_id, a.operation_id);
    assert_eq!(item.original_operation_id, a.review.original_operation_id);
    assert_eq!(item.review_digest, digest(&a.review).unwrap());
    let original = status(read(&data, lookup(&a.operation_id)).await);
    assert!(original.job.is_none());
    assert_eq!(original.approval_event, item.approval_event);
    assert_eq!(original.approval, a);
    assert!(
        read(&data, inputs_query()).await.response.error.is_some(),
        "pending approval cannot prepare another correction"
    );
    let current = read(
        &data,
        WorkspaceQuery::CurrentVerificationRecheck {
            project_id: "project".into(),
        },
    )
    .await;
    let Some(WorkspaceResult::CurrentVerificationRecheck(Some(current))) = current.workspace else {
        panic!("current missing")
    };
    assert_eq!(*current, original);
    for q in [
        list(1, None),
        list(2, Some(first.revision.clone())),
        lookup("missing"),
        WorkspaceQuery::VerificationRechecks {
            project_id: "missing".into(),
            offset: 0,
            revision: None,
        },
        WorkspaceQuery::VerificationRecheck {
            project_id: "missing".into(),
            operation_id: a.operation_id.clone(),
        },
    ] {
        assert!(read(&data, q).await.response.error.is_some());
    }
    let end = page(read(&data, list(1, Some(first.revision.clone()))).await);
    assert!(end.entries.is_empty() && end.next_offset.is_none());
    let claim = data
        .join("verification-recheck-jobs/project")
        .join(&a.operation_id);
    assert!(
        !claim.exists(),
        "all reads and approval must leave the checker unstarted"
    );
    interface::stop(&data, true).await;
    service.await.unwrap().unwrap();
    let host = RuntimeHost::open(&data).unwrap();
    let owned = data.clone();
    let service = tokio::spawn(async move { cerulean_core::ipc::serve_host(&owned, host).await });
    interface::connect(&data).await;
    let reopened = page(read(&data, list(0, None)).await);
    assert_eq!(
        serde_json::to_value(&first).unwrap(),
        serde_json::to_value(reopened).unwrap()
    );
    assert_eq!(status(read(&data, lookup(&a.operation_id)).await), original);
    assert!(!claim.exists());
    interface::stop(&data, true).await;
    service.await.unwrap().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "explicit ordinary Windows accounts; run the native recheck suite"]
async fn active_recheck_history_is_unavailable_but_exact_original_status_is_readable() {
    let (root, store, a) = runner::corrected("slow_checker");
    drop(store);
    let data = root.path().join("data");
    let mut host = RuntimeHost::open(&data).unwrap();
    host.approve_verification_recheck(&a.review, "workspace-approve", &a.operation_id)
        .unwrap();
    host.start_verification_recheck("project", &a.operation_id)
        .unwrap();
    let owned = data.clone();
    let service = tokio::spawn(async move { cerulean_core::ipc::serve_host(&owned, host).await });
    interface::connect(&data).await;
    let list_reply = read(&data, list(0, None)).await;
    assert!(list_reply.response.error.is_some());
    assert!(
        list_reply.workspace.is_none(),
        "active history cannot look like an empty page"
    );
    let running = status(read(&data, lookup(&a.operation_id)).await);
    let current = read(
        &data,
        WorkspaceQuery::CurrentVerificationRecheck {
            project_id: "project".into(),
        },
    )
    .await;
    assert!(current.response.result.is_none());
    let Some(WorkspaceResult::CurrentVerificationRecheck(Some(current))) = current.workspace else {
        panic!("live current missing")
    };
    assert_eq!(*current, running);
    assert_eq!(running.approval, a);
    assert_eq!(
        running.job.as_ref().unwrap().state,
        VerificationState::Running
    );
    // Observe an actual checker before cancellation; preparation-only
    // cancellation deliberately has no process exit evidence to preserve.
    let marker = data
        .join("workflows/project/verification")
        .join(&a.operation_id)
        .join("check-temp/validator-started");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(45);
    while !marker.exists() {
        assert!(std::time::Instant::now() < deadline);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let cancel = interface::call(
        &data,
        Command::CancelVerificationRecheck {
            project_id: "project".into(),
            operation_id: a.operation_id.clone(),
        },
    )
    .await;
    assert!(
        cancel.response.error.is_none(),
        "{:?}",
        cancel.response.error
    );
    let terminal = interface::terminal(&data, &a.operation_id).await;
    assert_eq!(terminal.job.unwrap().state, VerificationState::Unknown);
    assert_eq!(page(read(&data, list(0, None)).await).total, 1);
    interface::stop(&data, true).await;
    service.await.unwrap().unwrap();
}
