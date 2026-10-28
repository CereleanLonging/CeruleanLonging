//! Actual Windows authenticated named pipes with controlled original journals.
use super::*;
use crate::ipc::{self, Client, Greeting, ServiceReply, UserOperation};
use std::time::Duration;

struct Service(tokio::task::JoinHandle<Result<(), String>>);
impl Drop for Service {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn connect(data: &Path) -> Client {
    for _ in 0..100 {
        if let Ok(client) = Client::connect(data).await {
            return client;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("isolated historical service not ready");
}
async fn call(data: &Path, id: &str, command: Command) -> ServiceReply {
    connect(data)
        .await
        .call(UserOperation::Request {
            request: Request {
                id: id.into(),
                protocol_version: PROTOCOL_VERSION,
                command,
            },
        })
        .await
        .unwrap()
}
fn lookup(r: &HistoricalReview) -> Command {
    Command::ReadHistoricalConfirmation {
        project_id: r.project_id.clone(),
        scope: r.source.scope.clone(),
        review_digest: r.digest().unwrap(),
    }
}
async fn historical(data: &Path, id: &str, command: Command) -> ServiceReply {
    connect(data)
        .await
        .call(UserOperation::Historical {
            request: Request {
                id: id.into(),
                protocol_version: PROTOCOL_VERSION,
                command,
            },
        })
        .await
        .unwrap()
}
async fn stop(data: &Path, mut service: Service) {
    let stopped = connect(data)
        .await
        .call(UserOperation::Shutdown {
            id: "native-stop".into(),
            protocol_version: PROTOCOL_VERSION,
        })
        .await
        .unwrap();
    assert!(stopped.stopping && stopped.response.error.is_none());
    tokio::time::timeout(Duration::from_secs(20), &mut service.0)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires normal Windows host named-pipe permissions; run explicitly"]
async fn workspace_history_native_pages_original_detail_and_reopen() {
    use cerulean_domain::workspace::*;
    async fn read(data: &Path, query: WorkspaceQuery) -> WorkspaceResult {
        let reply = connect(data)
            .await
            .call(UserOperation::Workspace {
                id: "history-native".into(),
                protocol_version: PROTOCOL_VERSION,
                query,
            })
            .await
            .unwrap();
        assert!(reply.response.error.is_none(), "{:?}", reply.response.error);
        assert!(reply.response.result.is_none());
        assert!(serde_json::to_vec(&reply).unwrap().len() < 128 * 1024);
        reply.workspace.unwrap()
    }
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = RuntimeHost::open(&data).unwrap();
    for i in 0..45 {
        host.base
            .execute(Command::CreatePlan {
                title: format!("history {i}"),
                tasks: vec!["original task".into()],
            })
            .unwrap();
    }
    let original = serde_json::to_string(&host.base.events[21]).unwrap();
    let before = snapshot(&data);
    let directory = data.clone();
    let service = Service(tokio::spawn(async move {
        ipc::serve_host(&directory, host).await
    }));
    let WorkspaceResult::History(first) = read(
        &data,
        WorkspaceQuery::History {
            project_id: None,
            offset: 0,
            revision: None,
        },
    )
    .await
    else {
        panic!("page")
    };
    assert_eq!(first.total, 45);
    let mut count = first.entries.len();
    let mut next = first.next_offset;
    while let Some(offset) = next {
        let WorkspaceResult::History(page) = read(
            &data,
            WorkspaceQuery::History {
                project_id: None,
                offset,
                revision: Some(first.revision.clone()),
            },
        )
        .await
        else {
            panic!("page")
        };
        count += page.entries.len();
        next = page.next_offset;
    }
    assert_eq!(count, 45);
    let query = WorkspaceQuery::HistoryEntry {
        project_id: None,
        sequence: 22,
        revision: first.revision,
    };
    let WorkspaceResult::HistoryEntry(detail) = read(&data, query.clone()).await else {
        panic!("detail")
    };
    assert_eq!(detail.original_json, original);
    stop(&data, service).await;
    let host = RuntimeHost::open(&data).unwrap();
    assert_eq!(
        serde_json::to_value(host.workspace_query(&query).unwrap()).unwrap(),
        serde_json::to_value(WorkspaceResult::HistoryEntry(detail)).unwrap()
    );
    assert_eq!(snapshot(&data), before);
    println!("native original history pages=45; original detail and restart revision preserved; model_calls=0");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires normal Windows host named-pipe permissions; run explicitly"]
async fn workspace_recovery_native_original_detail_and_reopen() {
    use cerulean_domain::workspace::*;
    async fn query(data: &Path, query: WorkspaceQuery) -> WorkspaceResult {
        let client = connect(data).await;
        println!(
            "recovery named-pipe authenticated server_pid={}",
            client.server_pid
        );
        let reply = client
            .call(UserOperation::Workspace {
                id: "recovery-native".into(),
                protocol_version: PROTOCOL_VERSION,
                query,
            })
            .await
            .unwrap();
        assert!(reply.response.error.is_none(), "{:?}", reply.response.error);
        assert!(reply.response.result.is_none());
        reply.workspace.unwrap()
    }
    let temp = tempfile::tempdir().unwrap();
    let (data, relative) = startup_closure_tests::fixture(
        temp.path(),
        false,
        crate::startup_journal::PublishPoint::PartialWritten,
    );
    startup_closure_tests::acknowledge(&data, &relative);
    let mut host = RuntimeHost::open(&data).unwrap();
    startup_closure_tests::close(&mut host, false);
    let original = host.startups.closure_reviews[0].clone();
    let before = snapshot(&data);
    let directory = data.clone();
    let service = Service(tokio::spawn(async move {
        ipc::serve_host(&directory, host).await
    }));
    let page_query = || WorkspaceQuery::Recovery {
        project_id: "project".into(),
        offset: 0,
        revision: None,
    };
    let WorkspaceResult::Recovery(page) = query(&data, page_query()).await else {
        panic!("page")
    };
    assert_eq!(page.total, 1);
    assert_eq!(page.entries[0].id, relative);
    let detail_query = WorkspaceQuery::RecoveryDetail {
        project_id: "project".into(),
        kind: WorkspaceRecoveryKind::StartupClosure,
        id: relative,
        revision: page.revision.clone(),
    };
    let WorkspaceResult::RecoveryDetail(detail) = query(&data, detail_query.clone()).await else {
        panic!("detail")
    };
    assert_eq!(
        serde_json::to_value(&detail.record).unwrap()["value"],
        serde_json::to_value(original).unwrap()
    );
    assert_eq!(before, snapshot(&data));
    stop(&data, service).await;
    println!("recovery native first service exited cleanly and writer released");
    let reopened = RuntimeHost::open(&data).unwrap();
    let directory = data.clone();
    let service = Service(tokio::spawn(async move {
        ipc::serve_host(&directory, reopened).await
    }));
    let WorkspaceResult::Recovery(again) = query(&data, page_query()).await else {
        panic!("reopened page")
    };
    assert_eq!(again.revision, page.revision);
    let found = query(&data, detail_query).await;
    assert_eq!(
        serde_json::to_value(found).unwrap(),
        serde_json::to_value(WorkspaceResult::RecoveryDetail(detail)).unwrap()
    );
    stop(&data, service).await;
    assert_eq!(before, snapshot(&data));
    println!("recovery native second service exited cleanly; original evidence bytes unchanged across reopen");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires normal Windows host named-pipe permissions; run explicitly"]
async fn workspace_jobs_native_pages_original_detail_and_restart() {
    use cerulean_domain::{
        execution::{VerificationJob, VerificationState},
        workspace::*,
    };
    async fn query(data: &Path, q: WorkspaceQuery) -> WorkspaceResult {
        let reply = connect(data)
            .await
            .call(UserOperation::Workspace {
                id: "jobs-native".into(),
                protocol_version: PROTOCOL_VERSION,
                query: q,
            })
            .await
            .unwrap();
        assert!(reply.response.error.is_none(), "{:?}", reply.response.error);
        assert!(reply.response.result.is_none());
        assert!(serde_json::to_vec(&reply).unwrap().len() < 128 * 1024);
        reply.workspace.unwrap()
    }
    async fn stop_large(data: &Path, mut service: Service) {
        let stopped = connect(data)
            .await
            .call(UserOperation::Shutdown {
                id: "jobs-stop".into(),
                protocol_version: PROTOCOL_VERSION,
            })
            .await
            .unwrap();
        assert!(stopped.stopping);
        assert_eq!(stopped.response.error.unwrap().code, "responseTooLarge");
        tokio::time::timeout(Duration::from_secs(20), &mut service.0)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let (mut host, draft) = project_planning::tests::fixture(temp.path());
    host.configure_project(draft).unwrap();
    // Controlled legal persisted status records. No model or validator is run;
    // these failed records are not claims of successful independent checks.
    for n in 0..90 {
        let job = VerificationJob {
            project_id: "project".into(),
            operation_id: format!("check-{n:03}"),
            task_id: "task".into(),
            attempt_id: "attempt".into(),
            state: VerificationState::Failed,
            started_at: "2026-10-14T00:00:00Z".into(),
            updated_at: "2026-10-14T00:00:00Z".into(),
            message: Some(format!("original failed check {n}: {}", "x".repeat(50000))),
            reconciliation_digest: None,
        };
        crate::async_verification::save(&data, &job).unwrap();
    }
    drop(host);
    let host = RuntimeHost::open(&data).unwrap();
    let full_bytes = serde_json::to_vec(&host.snapshot()).unwrap().len();
    assert!(full_bytes > 4 * 1024 * 1024);
    let before = snapshot(&data);
    let directory = data.clone();
    let service = Service(tokio::spawn(async move {
        ipc::serve_host(&directory, host).await
    }));
    assert_eq!(
        call(&data, "jobs-full", Command::Snapshot)
            .await
            .response
            .error
            .unwrap()
            .code,
        "responseTooLarge"
    );
    let WorkspaceResult::Jobs(mut p) = query(
        &data,
        WorkspaceQuery::Jobs {
            project_id: "project".into(),
            offset: 0,
            revision: None,
        },
    )
    .await
    else {
        panic!("page")
    };
    let revision = p.revision.clone();
    let mut ids = vec![];
    loop {
        ids.extend(p.entries.iter().map(|j| j.id.clone()));
        let Some(offset) = p.next_offset else { break };
        let WorkspaceResult::Jobs(next) = query(
            &data,
            WorkspaceQuery::Jobs {
                project_id: "project".into(),
                offset,
                revision: Some(revision.clone()),
            },
        )
        .await
        else {
            panic!("page")
        };
        p = next;
    }
    assert_eq!(
        ids,
        (0..90).map(|n| format!("check-{n:03}")).collect::<Vec<_>>()
    );
    let q = WorkspaceQuery::Job {
        project_id: "project".into(),
        kind: WorkspaceJobKind::Verification,
        id: "check-089".into(),
        revision,
    };
    let original = query(&data, q.clone()).await;
    let WorkspaceResult::Job(detail) = &original else {
        panic!("job")
    };
    let WorkspaceJobRecord::Verification(job) = &detail.record else {
        panic!("verification")
    };
    let persisted: VerificationJob = serde_json::from_slice(
        &std::fs::read(data.join("verification-jobs/project/check-089.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(job).unwrap(),
        serde_json::to_value(persisted).unwrap()
    );
    stop_large(&data, service).await;
    assert_eq!(snapshot(&data), before);
    let reopened = RuntimeHost::open(&data).unwrap();
    assert_eq!(
        serde_json::to_value(reopened.workspace_query(&q).unwrap()).unwrap(),
        serde_json::to_value(original).unwrap()
    );
    drop(reopened);
    assert_eq!(snapshot(&data), before);
    println!("native original job pages=90; full_snapshot_bytes={full_bytes}; model_calls=0; files_preserved=true");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires normal Windows host named-pipe permissions; run explicitly"]
async fn workspace_execution_native_complete_review_and_lost_approval_ack_readback() {
    use cerulean_domain::workspace::*;
    async fn query(data: &Path, digest: &str) -> WorkspaceExecutionReview {
        let reply = connect(data)
            .await
            .call(UserOperation::Workspace {
                id: "execution-review".into(),
                protocol_version: PROTOCOL_VERSION,
                query: WorkspaceQuery::ExecutionReview {
                    project_id: "project".into(),
                    digest: Some(digest.into()),
                },
            })
            .await
            .unwrap();
        assert!(reply.response.error.is_none() && reply.response.result.is_none());
        assert!(serde_json::to_vec(&reply).unwrap().len() < 128 * 1024);
        let WorkspaceResult::ExecutionReview(review) = reply.workspace.unwrap() else {
            panic!("review")
        };
        *review
    }
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let (mut host, mut draft) = project_planning::tests::fixture(temp.path());
    draft.worker_max_requests = 30;
    host.configure_project(draft).unwrap();
    let mut proposal = project_planning::tests::proposal("tests");
    if let ManagementDecision::Plan { tasks, .. } = &mut proposal {
        let first = tasks[0].clone();
        *tasks = (0..25)
            .map(|i| {
                let mut task = first.clone();
                task.id = format!("task-{i:02}");
                task.goal = format!("Original authorization {i}");
                task
            })
            .collect();
    }
    project_planning::tests::complete(&mut host, proposal);
    let original = host.projects["project"].revision().unwrap().clone();
    let digest = original.approval_digest().unwrap();
    let sequence = host.projects["project"].history().len();
    for i in 0..75 {
        host.base
            .execute(Command::CreatePlan {
                title: format!("unrelated {i}"),
                tasks: vec!["x".repeat(2000); 30],
            })
            .unwrap();
    }
    let full_size = serde_json::to_vec(&host.snapshot()).unwrap().len();
    assert!(full_size > 4 * 1024 * 1024);
    let before = snapshot(&data);
    let directory = data.clone();
    let mut service = Service(tokio::spawn(async move {
        ipc::serve_host(&directory, host).await
    }));
    let review = query(&data, &digest).await;
    assert_eq!(review.revision.as_ref(), Some(&original));
    assert!(review.approval.is_none());
    assert!(snapshot(&data) == before);
    let rejected = connect(&data)
        .await
        .call(UserOperation::Command {
            request: Request {
                id: "stale-approval".into(),
                protocol_version: PROTOCOL_VERSION,
                command: Command::ApproveRevision {
                    project_id: "project".into(),
                    digest: "0".repeat(64),
                },
            },
        })
        .await
        .unwrap();
    assert!(rejected.response.error.is_some() && rejected.command_receipt.is_none());
    assert!(snapshot(&data) == before);
    let mut pipe = tokio::net::windows::named_pipe::ClientOptions::new()
        .open(ipc::endpoint(&data).unwrap())
        .unwrap();
    let greeting: Greeting = ipc::read_frame(&mut pipe, 64 * 1024).await.unwrap();
    assert_eq!(greeting.owner_sid, ipc::current_sid().unwrap());
    assert_eq!(greeting.server_pid, std::process::id());
    assert_eq!(greeting.protocol_version, PROTOCOL_VERSION);
    ipc::write_frame(
        &mut pipe,
        &UserOperation::Command {
            request: Request {
                id: "approve-once-drop-ack".into(),
                protocol_version: PROTOCOL_VERSION,
                command: Command::ApproveRevision {
                    project_id: "project".into(),
                    digest: digest.clone(),
                },
            },
        },
        64 * 1024,
    )
    .await
    .unwrap();
    drop(pipe);
    let mut approval = None;
    for _ in 0..100 {
        let readback = query(&data, &digest).await;
        assert_eq!(readback.revision.as_ref(), Some(&original));
        if readback.approval.is_some() {
            approval = readback.approval;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let approval = approval.expect("original approval recovered without resend");
    assert_eq!(approval.revision_digest, digest);
    let after = snapshot(&data);
    let readback = query(&data, &digest).await;
    assert_eq!(readback.approval.as_ref(), Some(&approval));
    assert!(snapshot(&data) == after);
    let stopped = connect(&data)
        .await
        .call(UserOperation::Shutdown {
            id: "execution-review-stop".into(),
            protocol_version: PROTOCOL_VERSION,
        })
        .await
        .unwrap();
    assert!(stopped.stopping);
    assert_eq!(stopped.response.error.unwrap().code, "responseTooLarge");
    tokio::time::timeout(Duration::from_secs(20), &mut service.0)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let host = RuntimeHost::open(&data).unwrap();
    assert_eq!(host.projects["project"].history().len(), sequence + 1);
    assert_eq!(host.projects["project"].approval(), Some(&approval));
    assert_eq!(host.projects["project"].revision(), Some(&original));
    assert!(snapshot(&data) == after);
    println!("native execution review: complete tasks=25, all permissions/workers/checks preserved; old snapshot bytes={full_size}; stale approval rejected; valid approval sends=1; dropped ACK=1; resends=0; original approval read after restart; model calls=0");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires normal Windows host named-pipe permissions; run explicitly"]
async fn workspace_tasks_native_original_project_pages_details_and_restart_without_snapshot() {
    use cerulean_domain::workspace::*;
    async fn query(data: &Path, query: WorkspaceQuery) -> WorkspaceResult {
        let reply = connect(data)
            .await
            .call(UserOperation::Workspace {
                id: "tasks-native".into(),
                protocol_version: PROTOCOL_VERSION,
                query,
            })
            .await
            .unwrap();
        assert!(reply.response.error.is_none(), "{:?}", reply.response.error);
        assert!(reply.response.result.is_none());
        assert!(serde_json::to_vec(&reply).unwrap().len() < 128 * 1024);
        reply.workspace.unwrap()
    }
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let (mut host, mut draft) = project_planning::tests::fixture(temp.path());
    draft.worker_max_requests = 30;
    host.configure_project(draft).unwrap();
    let mut proposal = project_planning::tests::proposal("tests");
    if let ManagementDecision::Plan { tasks, .. } = &mut proposal {
        let first = tasks[0].clone();
        *tasks = (0..25)
            .map(|i| {
                let mut task = first.clone();
                task.id = format!("task-{i:02}");
                task.goal = format!("Original task {i}");
                task
            })
            .collect();
    }
    project_planning::tests::complete(&mut host, proposal);
    let original_plan = host.projects["project"].state().plan.clone().unwrap();
    for i in 0..75 {
        host.base
            .execute(Command::CreatePlan {
                title: format!("unrelated {i}"),
                tasks: vec!["x".repeat(2000); 30],
            })
            .unwrap();
    }
    let original_size = serde_json::to_vec(&host.snapshot()).unwrap().len();
    assert!(original_size > 4 * 1024 * 1024);
    let before = snapshot(&data);
    let directory = data.clone();
    let mut service = Service(tokio::spawn(async move {
        ipc::serve_host(&directory, host).await
    }));
    assert_eq!(
        call(&data, "original-full-query", Command::Snapshot)
            .await
            .response
            .error
            .unwrap()
            .code,
        "responseTooLarge"
    );
    let WorkspaceResult::Tasks(first) = query(
        &data,
        WorkspaceQuery::Tasks {
            project_id: "project".into(),
            offset: 0,
            revision: None,
        },
    )
    .await
    else {
        panic!("tasks")
    };
    assert_eq!(first.total, 25);
    assert_eq!(first.entries.len(), 20);
    assert!(!first.approved);
    let WorkspaceResult::Tasks(second) = query(
        &data,
        WorkspaceQuery::Tasks {
            project_id: "project".into(),
            offset: first.next_offset.unwrap(),
            revision: Some(first.revision.clone()),
        },
    )
    .await
    else {
        panic!("tasks")
    };
    assert_eq!(second.entries.len(), 5);
    assert_eq!(second.next_offset, None);
    let ids = first
        .entries
        .iter()
        .chain(&second.entries)
        .map(|t| t.id.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        original_plan
            .tasks
            .iter()
            .map(|t| t.id.clone())
            .collect::<Vec<_>>()
    );
    let detail_query = WorkspaceQuery::Task {
        project_id: "project".into(),
        task_id: "task-24".into(),
        revision: first.revision.clone(),
    };
    let WorkspaceResult::Task(detail) = query(&data, detail_query.clone()).await else {
        panic!("task")
    };
    assert_eq!(detail.task, original_plan.tasks[24]);
    assert_eq!(detail.run.evidence_count, 0);
    let WorkspaceResult::TaskEvidence(evidence) = query(
        &data,
        WorkspaceQuery::TaskEvidence {
            project_id: "project".into(),
            task_id: "task-24".into(),
            offset: 0,
            revision: first.revision.clone(),
        },
    )
    .await
    else {
        panic!("evidence")
    };
    assert_eq!(evidence.total, 0);
    assert!(evidence.entries.is_empty());
    assert!(snapshot(&data) == before);
    let stopped = connect(&data)
        .await
        .call(UserOperation::Shutdown {
            id: "tasks-native-stop".into(),
            protocol_version: PROTOCOL_VERSION,
        })
        .await
        .unwrap();
    assert!(stopped.stopping);
    assert_eq!(stopped.response.error.unwrap().code, "responseTooLarge");
    tokio::time::timeout(Duration::from_secs(20), &mut service.0)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let host = RuntimeHost::open(&data).unwrap();
    let reopened = host.workspace_query(&detail_query).unwrap();
    assert_eq!(
        serde_json::to_value(reopened).unwrap(),
        serde_json::to_value(WorkspaceResult::Task(detail)).unwrap()
    );
    assert!(snapshot(&data) == before);
    println!("native project tasks: original legal proposal tasks=25; pages=20+5; full Snapshot bytes={original_size}; original selected detail and empty evidence complete; whole replies below 128KiB; restart cursor unchanged; writes during readback=0; model calls=0");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires normal Windows host named-pipe permissions; run explicitly"]
async fn command_receipt_native_large_snapshot_and_lost_ack_read_original_once() {
    use cerulean_domain::workspace::*;
    async fn query(data: &Path, query: WorkspaceQuery) -> WorkspaceResult {
        let reply = connect(data)
            .await
            .call(UserOperation::Workspace {
                id: "receipt-readback".into(),
                protocol_version: PROTOCOL_VERSION,
                query,
            })
            .await
            .unwrap();
        assert!(reply.response.error.is_none() && reply.response.result.is_none());
        reply.workspace.unwrap()
    }
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = fixture(temp.path(), false);
    for i in 0..75 {
        host.base
            .execute(Command::CreatePlan {
                title: format!("receipt seed {i}"),
                tasks: vec!["x".repeat(2000); 30],
            })
            .unwrap();
    }
    let size = serde_json::to_vec(&host.snapshot()).unwrap().len();
    assert!(size > 4 * 1024 * 1024);
    let directory = data.clone();
    let mut service = Service(tokio::spawn(async move {
        ipc::serve_host(&directory, host).await
    }));
    let reply = connect(&data)
        .await
        .call(UserOperation::Command {
            request: Request {
                id: "single-create".into(),
                protocol_version: PROTOCOL_VERSION,
                command: Command::CreatePlan {
                    title: "original receipt plan".into(),
                    tasks: vec!["no execution".into()],
                },
            },
        })
        .await
        .unwrap();
    let bytes = serde_json::to_vec(&reply).unwrap().len();
    assert!(bytes < 1024);
    assert!(reply.response.error.is_none() && reply.response.result.is_none());
    assert_eq!(reply.command_receipt.unwrap().request_id, "single-create");
    assert_eq!(
        call(&data, "separate-refresh", Command::Snapshot)
            .await
            .response
            .error
            .unwrap()
            .code,
        "responseTooLarge"
    );
    let mut offset = 0;
    let mut revision = None;
    let mut matches = vec![];
    loop {
        let WorkspaceResult::Plans(page) =
            query(&data, WorkspaceQuery::Plans { offset, revision }).await
        else {
            panic!("plans")
        };
        assert_eq!(page.total, 76);
        matches.extend(
            page.entries
                .into_iter()
                .filter(|p| p.title == "original receipt plan"),
        );
        revision = Some(page.revision);
        if let Some(next) = page.next_offset {
            offset = next;
        } else {
            break;
        }
    }
    assert_eq!(matches.len(), 1);
    let plan = matches.pop().unwrap();
    let mut pipe = tokio::net::windows::named_pipe::ClientOptions::new()
        .open(ipc::endpoint(&data).unwrap())
        .unwrap();
    let greeting: Greeting = ipc::read_frame(&mut pipe, 64 * 1024).await.unwrap();
    assert_eq!(greeting.owner_sid, ipc::current_sid().unwrap());
    assert_eq!(greeting.server_pid, std::process::id());
    assert_eq!(greeting.protocol_version, PROTOCOL_VERSION);
    ipc::write_frame(
        &mut pipe,
        &UserOperation::Command {
            request: Request {
                id: "single-approve-drop-ack".into(),
                protocol_version: PROTOCOL_VERSION,
                command: Command::ApprovePlan {
                    id: plan.id.clone(),
                    digest: plan.digest.clone(),
                },
            },
        },
        64 * 1024,
    )
    .await
    .unwrap();
    drop(pipe);
    // Reconnect and only read the exact original identity/content. No resend.
    let mut found = None;
    for _ in 0..100 {
        let WorkspaceResult::Plans(page) = query(
            &data,
            WorkspaceQuery::Plans {
                offset: 0,
                revision: None,
            },
        )
        .await
        else {
            panic!("plans")
        };
        let WorkspaceResult::Plan(original) = query(
            &data,
            WorkspaceQuery::Plan {
                id: plan.id.clone(),
                revision: page.revision,
            },
        )
        .await
        else {
            panic!("plan")
        };
        assert_eq!(original.digest, plan.digest);
        if original.approved {
            found = Some(original);
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let original = found.expect("original approval observed without resending");
    let before = snapshot(&data);
    let stopped = connect(&data)
        .await
        .call(UserOperation::Shutdown {
            id: "receipt-stop".into(),
            protocol_version: PROTOCOL_VERSION,
        })
        .await
        .unwrap();
    assert!(stopped.stopping);
    assert_eq!(stopped.response.error.unwrap().code, "responseTooLarge");
    tokio::time::timeout(Duration::from_secs(20), &mut service.0)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(Client::connect(&data).await.is_err());
    let host = RuntimeHost::open(&data).unwrap();
    let reopened = host.base.snapshot();
    assert_eq!(reopened.plans.len(), 76);
    let stored = reopened.plans.iter().find(|p| p.id == original.id).unwrap();
    assert!(stored.approved);
    assert_eq!(stored.digest, original.digest);
    assert_eq!(reopened.plans.iter().filter(|p| p.approved).count(), 1);
    assert!(before == snapshot(&data));
    println!("command receipt bytes={bytes}; legacy snapshot bytes={size}; creates=1; approvals=1; dropped approval ACK=1; resends=0; original plans=76; persisted approval read after restart; model calls=0");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires normal Windows host named-pipe permissions; run explicitly"]
async fn workspace_native_ipc_large_plans_and_projects_are_complete_bounded_and_readonly() {
    use cerulean_domain::workspace::*;
    async fn query(data: &Path, query: WorkspaceQuery) -> WorkspaceResult {
        let reply = connect(data)
            .await
            .call(UserOperation::Workspace {
                id: "workspace-native".into(),
                protocol_version: PROTOCOL_VERSION,
                query,
            })
            .await
            .unwrap();
        assert!(reply.response.error.is_none(), "{:?}", reply.response.error);
        assert!(reply.response.result.is_none() && reply.historical.is_none());
        assert!(serde_json::to_vec(&reply).unwrap().len() < 128 * 1024);
        reply.workspace.unwrap()
    }
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = fixture(temp.path(), false);
    for i in 0..75 {
        let command = Command::CreatePlan {
            title: format!("workspace plan {i}"),
            tasks: vec!["x".repeat(2000); 30],
        };
        assert!(
            serde_json::to_vec(&Request {
                id: format!("seed-{i}"),
                protocol_version: PROTOCOL_VERSION,
                command: command.clone()
            })
            .unwrap()
            .len()
                < 64 * 1024
        );
        host.base.execute(command).unwrap();
    }
    for i in 0..20 {
        let root = temp.path().join(format!("registered-{i}"));
        fs::create_dir(&root).unwrap();
        host.register(cerulean_domain::execution::Project {
            id: format!("registered-{i:02}"),
            root: root.display().to_string(),
            timezone: "Asia/Shanghai".into(),
        })
        .unwrap();
    }
    let legacy_bytes = serde_json::to_vec(&host.snapshot()).unwrap().len();
    assert!(legacy_bytes > 4 * 1024 * 1024);
    let before = snapshot(&data);
    let directory = data.clone();
    let mut service = Service(tokio::spawn(async move {
        ipc::serve_host(&directory, host).await
    }));
    let legacy = call(&data, "workspace-legacy", Command::Snapshot).await;
    assert_eq!(legacy.response.error.unwrap().code, "responseTooLarge");
    let WorkspaceResult::Overview(overview) = query(&data, WorkspaceQuery::Overview).await else {
        panic!("overview")
    };
    assert_eq!(overview.legacy_plan_count, Some(75));
    assert_eq!(overview.pending_legacy_plan_count, Some(75));
    assert_eq!(overview.project_count, Some(21));
    let mut offset = 0;
    let mut revision = None;
    let mut ids = std::collections::BTreeSet::new();
    let mut detail_query = None;
    loop {
        let WorkspaceResult::Plans(page) = query(
            &data,
            WorkspaceQuery::Plans {
                offset,
                revision: revision.clone(),
            },
        )
        .await
        else {
            panic!("plans")
        };
        assert_eq!(page.total, 75);
        assert!(page.entries.len() <= 20);
        revision = Some(page.revision.clone());
        for entry in &page.entries {
            assert!(ids.insert(entry.id.clone()));
            assert_eq!(entry.task_count, 30);
        }
        if detail_query.is_none() {
            detail_query = Some(WorkspaceQuery::Plan {
                id: page.entries[0].id.clone(),
                revision: page.revision,
            });
        }
        if let Some(next) = page.next_offset {
            offset = next;
        } else {
            break;
        }
    }
    assert_eq!(ids.len(), 75);
    let WorkspaceResult::Plan(detail) = query(&data, detail_query.clone().unwrap()).await else {
        panic!("detail")
    };
    assert_eq!(detail.tasks, vec!["x".repeat(2000); 30]);
    let WorkspaceResult::Projects(first) = query(
        &data,
        WorkspaceQuery::Projects {
            offset: 0,
            revision: None,
        },
    )
    .await
    else {
        panic!("projects")
    };
    assert_eq!(
        (first.total, first.entries.len(), first.next_offset),
        (21, 20, Some(20))
    );
    let second_query = WorkspaceQuery::Projects {
        offset: 20,
        revision: Some(first.revision.clone()),
    };
    let WorkspaceResult::Projects(second) = query(&data, second_query.clone()).await else {
        panic!("projects")
    };
    assert_eq!(second.entries.len(), 1);
    assert_eq!(
        first
            .entries
            .iter()
            .chain(second.entries.iter())
            .map(|e| &e.project.id)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        21
    );
    assert!(snapshot(&data) == before);
    let stopped = connect(&data)
        .await
        .call(UserOperation::Shutdown {
            id: "workspace-stop".into(),
            protocol_version: PROTOCOL_VERSION,
        })
        .await
        .unwrap();
    assert!(stopped.stopping);
    assert_eq!(stopped.response.error.unwrap().code, "responseTooLarge");
    tokio::time::timeout(Duration::from_secs(20), &mut service.0)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let host = RuntimeHost::open(&data).unwrap();
    assert!(matches!(
        host.workspace_query(&detail_query.unwrap()).unwrap(),
        WorkspaceResult::Plan(_)
    ));
    assert!(
        matches!(host.workspace_query(&second_query).unwrap(),WorkspaceResult::Projects(p) if p.entries==second.entries)
    );
    assert!(snapshot(&data) == before);
    println!("native workspace: legacy_bytes={legacy_bytes}; all 75 plans, 21 projects and exact original 30-task detail; each whole reply below 128 KiB; no Snapshot result; unchanged records/restart cursors; model calls=0");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires normal Windows host named-pipe permissions; run explicitly"]
async fn historical_native_ipc_paging_dropped_confirmation_readback_and_restart() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = fixture(temp.path(), false);
    let scope = closed(&mut host, "native-original");
    for i in 0..21 {
        closed(&mut host, &format!("native-unaccounted-{i:02}"));
    }
    let original = host
        .review_historical_charge("project", &scope, 17, &"b".repeat(8000), &"e".repeat(8000))
        .unwrap();
    let before = snapshot(&data);
    let directory = data.clone();
    let service = Service(tokio::spawn(async move {
        ipc::serve_host(&directory, host).await
    }));
    let first = call(
        &data,
        "native-first",
        Command::ReadHistoricalSources {
            offset: 0,
            revision: None,
        },
    )
    .await;
    assert_eq!(first.response.id, "native-first");
    assert!(first.response.error.is_none());
    let bytes = serde_json::to_vec(&first).unwrap().len();
    assert!(bytes < 4 * 1024 * 1024);
    let page = first.response.result.unwrap().historical_sources.unwrap();
    assert_eq!(page.total, 22);
    assert_eq!(page.next_offset, Some(20));
    let next = call(
        &data,
        "native-next",
        Command::ReadHistoricalSources {
            offset: 20,
            revision: Some(page.revision.clone()),
        },
    )
    .await
    .response
    .result
    .unwrap()
    .historical_sources
    .unwrap();
    assert_eq!(next.entries.len(), 2);
    assert_eq!(next.next_offset, None);
    let seen = page
        .entries
        .iter()
        .chain(&next.entries)
        .map(|e| &e.gap.source_id)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(seen.len(), 22);
    let stale = call(
        &data,
        "native-stale",
        Command::ReadHistoricalSources {
            offset: 20,
            revision: Some("a".repeat(64)),
        },
    )
    .await;
    assert!(stale.response.error.is_some());
    let prepared = historical(
        &data,
        "native-review",
        Command::ReviewHistoricalCharge {
            input: original.input(),
        },
    )
    .await
    .historical
    .unwrap()
    .historical_charge_review
    .unwrap();
    assert_eq!(prepared, original.view().unwrap());
    assert!(snapshot(&data) == before);

    // Send exactly one confirmation, intentionally close without reading its
    // reply, then use only the original readback command on new connections.
    let endpoint = ipc::endpoint(&data).unwrap();
    let mut pipe = tokio::net::windows::named_pipe::ClientOptions::new()
        .open(endpoint)
        .unwrap();
    let greeting: Greeting = ipc::read_frame(&mut pipe, 64 * 1024).await.unwrap();
    assert_eq!(greeting.server_pid, std::process::id());
    assert_eq!(greeting.owner_sid, ipc::current_sid().unwrap());
    assert_eq!(greeting.protocol_version, PROTOCOL_VERSION);
    ipc::write_frame(
        &mut pipe,
        &UserOperation::Historical {
            request: Request {
                id: "native-one-confirmation".into(),
                protocol_version: PROTOCOL_VERSION,
                command: Command::ConfirmHistoricalCharge {
                    input: original.input(),
                    review_digest: original.digest().unwrap(),
                },
            },
        },
        64 * 1024,
    )
    .await
    .unwrap();
    drop(pipe);
    let mut recovered = None;
    for _ in 0..50 {
        let reply = call(&data, "native-readback", lookup(&original)).await;
        assert!(reply.response.error.is_none(), "{:?}", reply.response.error);
        recovered = reply
            .response
            .result
            .unwrap()
            .historical_confirmation
            .unwrap()
            .record;
        if recovered.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let recovered = recovered.expect("original confirmation not found; no resend attempted");
    assert_eq!(recovered.review, prepared);
    assert_eq!(
        recovered.state,
        cerulean_domain::historical_money::HistoricalAccountingState::Accounted
    );
    let after = snapshot(&data);
    let accounted = call(
        &data,
        "native-amounts",
        Command::ReadHistoricalAccounting {
            offset: 0,
            revision: None,
        },
    )
    .await
    .response
    .result
    .unwrap()
    .historical_accounting
    .unwrap();
    assert_eq!(accounted.total, 1);
    assert_eq!(
        accounted.entries[0].user_event_digest,
        recovered.user_event_digest
    );
    assert!(call(
        &data,
        "native-old-page",
        Command::ReadHistoricalSources {
            offset: 20,
            revision: Some(page.revision)
        }
    )
    .await
    .response
    .error
    .is_some());
    let snapshot_view = call(&data, "native-account", Command::Snapshot)
        .await
        .response
        .result
        .unwrap();
    assert_eq!(snapshot_view.currency_budgets[0].charged_usd_nanos, 17);
    assert!(snapshot_view.currency_budgets[0]
        .available_usd_nanos
        .is_none());
    assert!(snapshot(&data) == after);
    stop(&data, service).await;
    assert!(Client::connect(&data).await.is_err());
    let host = RuntimeHost::open(&data).unwrap();
    assert_eq!(
        host.projects["project"]
            .history()
            .iter()
            .filter(|e| e.body.context.historical_charge.is_some())
            .count(),
        1
    );
    assert!(snapshot(&data) == after);
    let directory = data.clone();
    let service = Service(tokio::spawn(async move {
        ipc::serve_host(&directory, host).await
    }));
    let reopened = call(&data, "native-reopened", lookup(&original))
        .await
        .response
        .result
        .unwrap()
        .historical_confirmation
        .unwrap()
        .record
        .unwrap();
    assert_eq!(reopened, recovered);
    assert!(snapshot(&data) == after);
    stop(&data, service).await;
    let reopened = RuntimeHost::open(&data).unwrap();
    assert_eq!(
        reopened.currency_accounts["project"]
            .state()
            .charged_usd_nanos,
        17
    );
    println!("native historical IPC: source scopes=22; confirmation sends=1; accounted records=1; charge=17 nanos; snapshot reply bytes={bytes}; original records preserved; restart readback matched; model calls=0");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires normal Windows host named-pipe permissions; run explicitly"]
async fn historical_native_ipc_large_unrelated_snapshot_reports_committed_outcome_without_resend() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let mut host = fixture(temp.path(), false);
    let scope = closed(&mut host, "native-large-original");
    let original = review(&host, &scope);
    // Real Store records, each within its existing plan/input limits. None is
    // dispatched; these exercise unrelated snapshot growth, not model work.
    for i in 0..75 {
        let command = Command::CreatePlan {
            title: format!("Local capacity fixture {i}"),
            tasks: vec!["x".repeat(2000); 30],
        };
        let input = Request {
            id: format!("seed-{i}"),
            protocol_version: PROTOCOL_VERSION,
            command: command.clone(),
        };
        assert!(
            serde_json::to_vec(&UserOperation::Request { request: input })
                .unwrap()
                .len()
                < 64 * 1024
        );
        host.base.execute(command).unwrap();
    }
    let size = serde_json::to_vec(&host.snapshot()).unwrap().len();
    assert!(size > 4 * 1024 * 1024);
    let directory = data.clone();
    let mut service = Service(tokio::spawn(async move {
        ipc::serve_host(&directory, host).await
    }));
    let reply = call(
        &data,
        "native-large-confirm",
        Command::ConfirmHistoricalCharge {
            input: original.input(),
            review_digest: original.digest().unwrap(),
        },
    )
    .await;
    assert_eq!(reply.response.id, "native-large-confirm");
    assert_eq!(reply.response.error.unwrap().code, "responseTooLarge");
    assert!(!reply.stopping && reply.response.result.is_none());
    let before = snapshot(&data);
    let readback = call(&data, "native-large-read-only", lookup(&original)).await;
    assert_eq!(readback.response.error.unwrap().code, "responseTooLarge"); // legacy full-snapshot route
    let bounded = historical(&data, "native-large-independent", lookup(&original)).await;
    assert!(bounded.response.error.is_none() && bounded.response.result.is_none());
    let bounded_bytes = serde_json::to_vec(&bounded).unwrap().len();
    assert!(bounded_bytes < 128 * 1024);
    let result = bounded.historical.unwrap();
    let record = result.historical_confirmation.unwrap().record.unwrap();
    assert_eq!(record.review, original.view().unwrap());
    assert_eq!(
        record.state,
        cerulean_domain::historical_money::HistoricalAccountingState::Accounted
    );
    assert_eq!(result.currency_budgets.len(), 1);
    assert_eq!(result.currency_budgets[0].charged_usd_nanos, 17);
    let sources = historical(
        &data,
        "native-large-sources",
        Command::ReadHistoricalSources {
            offset: 0,
            revision: None,
        },
    )
    .await
    .historical
    .unwrap()
    .historical_sources
    .unwrap();
    assert_eq!(sources.total, 1);
    assert_eq!(sources.pending_total, Some(0));
    assert!(snapshot(&data) == before);
    let stopped = connect(&data)
        .await
        .call(UserOperation::Shutdown {
            id: "native-large-stop".into(),
            protocol_version: PROTOCOL_VERSION,
        })
        .await
        .unwrap();
    assert!(stopped.stopping);
    assert_eq!(stopped.response.error.unwrap().code, "responseTooLarge");
    tokio::time::timeout(Duration::from_secs(20), &mut service.0)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(Client::connect(&data).await.is_err());
    let host = RuntimeHost::open(&data).unwrap();
    assert_eq!(host.base.snapshot().plans.len(), 75);
    assert_eq!(
        host.currency_accounts["project"].state().charged_usd_nanos,
        17
    );
    assert_eq!(
        host.projects["project"]
            .history()
            .iter()
            .filter(|e| e.body.context.historical_charge.is_some())
            .count(),
        1
    );
    assert!(snapshot(&data) == before);
    println!("native large snapshot: original legal plans=75; snapshot bytes={size}; confirmation sends=1; charge=17 nanos; legacy overflow retained; independent readback bytes={bounded_bytes}; original confirmation recovered; graceful stop and original records preserved; model calls=0");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires normal Windows host named-pipe permissions; run explicitly"]
async fn workspace_configurations_native_complete_saved_setups_and_reopen() {
    use cerulean_domain::workspace::*;
    async fn reply(data: &Path, query: WorkspaceQuery) -> ServiceReply {
        let client = connect(data).await;
        assert_eq!(client.server_pid, std::process::id());
        let reply = client
            .call(UserOperation::Workspace {
                id: "configurations-native".into(),
                protocol_version: PROTOCOL_VERSION,
                query,
            })
            .await
            .unwrap();
        assert_eq!(reply.response.id, "configurations-native");
        assert!(reply.response.result.is_none() && reply.historical.is_none());
        assert!(reply.command_receipt.is_none() && !reply.stopping);
        assert!(serde_json::to_vec(&reply).unwrap().len() < 128 * 1024);
        reply
    }
    async fn query(data: &Path, q: WorkspaceQuery) -> WorkspaceResult {
        let r = reply(data, q).await;
        assert!(r.response.error.is_none(), "{:?}", r.response.error);
        r.workspace.unwrap()
    }
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let (mut host, draft) = project_planning::tests::fixture(temp.path());
    host.configure_project(draft.clone()).unwrap();
    let WorkspaceResult::Configurations(old) = host
        .workspace_query(&WorkspaceQuery::Configurations {
            offset: 0,
            revision: None,
        })
        .unwrap()
    else {
        panic!("original page")
    };
    for n in 0..25 {
        let project_id = format!("configured-{n:02}");
        let root = temp.path().join(&project_id);
        fs::create_dir(&root).unwrap();
        host.register(Project {
            id: project_id.clone(),
            root: root.to_string_lossy().into(),
            timezone: "Asia/Shanghai".into(),
        })
        .unwrap();
        let mut setup = draft.clone();
        setup.project_id = project_id;
        setup.requirement = format!(
            "Original requirement {n}: 保留原始字节\n{}",
            "x".repeat(2000)
        );
        host.configure_project(setup).unwrap();
    }
    let expected = crate::project_setup::load(&data).unwrap();
    assert_eq!(expected.len(), 26);
    assert_eq!(
        serde_json::to_value(&expected).unwrap(),
        serde_json::to_value(&host.project_setups).unwrap()
    );
    drop(host);
    let before = snapshot(&data);
    let mut host = Some(RuntimeHost::open(&data).unwrap());
    let mut revision = None;
    let mut original_detail = None;
    for reopen in 0..2 {
        let current = host.take().unwrap();
        let directory = data.clone();
        let service = Service(tokio::spawn(async move {
            ipc::serve_host(&directory, current).await
        }));
        let mut offset = 0;
        let mut found = std::collections::BTreeMap::new();
        let mut page_lengths = vec![];
        loop {
            let WorkspaceResult::Configurations(page) = query(
                &data,
                WorkspaceQuery::Configurations {
                    offset,
                    revision: revision.clone(),
                },
            )
            .await
            else {
                panic!("configuration page")
            };
            assert_eq!((page.offset, page.total), (offset, 26));
            assert_ne!(page.revision, old.revision);
            if let Some(version) = &revision {
                assert_eq!(&page.revision, version);
            }
            revision = Some(page.revision);
            page_lengths.push(page.entries.len());
            for entry in &page.entries {
                assert_eq!(entry.digest, expected[&entry.project_id].digest);
                assert!(found
                    .insert(entry.project_id.clone(), entry.digest.clone())
                    .is_none());
            }
            if let Some(next) = page.next_offset {
                assert_eq!(next, offset + page.entries.len() as u32);
                assert!(next > offset);
                offset = next;
            } else {
                assert_eq!(offset + page.entries.len() as u32, page.total);
                break;
            }
        }
        assert_eq!(page_lengths, vec![20, 6]);
        assert_eq!(
            found,
            expected
                .iter()
                .map(|(id, saved)| (id.clone(), saved.digest.clone()))
                .collect()
        );
        let version = revision.clone().unwrap();
        let detail_query = WorkspaceQuery::Configuration {
            project_id: "configured-24".into(),
            revision: version.clone(),
        };
        let WorkspaceResult::Configuration(detail) = query(&data, detail_query.clone()).await
        else {
            panic!("configuration detail")
        };
        assert_eq!(detail.project_id, "configured-24");
        assert_eq!(detail.revision, version);
        assert_eq!(
            serde_json::to_value(&detail.saved).unwrap(),
            serde_json::to_value(&expected["configured-24"]).unwrap()
        );
        let detail_value = serde_json::to_value(WorkspaceResult::Configuration(detail)).unwrap();
        if let Some(original) = &original_detail {
            assert_eq!(&detail_value, original);
        }
        original_detail = Some(detail_value.clone());
        for invalid in [
            WorkspaceQuery::Configurations {
                offset: 20,
                revision: None,
            },
            WorkspaceQuery::Configurations {
                offset: 27,
                revision: Some(version.clone()),
            },
            WorkspaceQuery::Configurations {
                offset: u32::MAX,
                revision: Some(version.clone()),
            },
            WorkspaceQuery::Configurations {
                offset: 0,
                revision: Some("not-a-digest".into()),
            },
            WorkspaceQuery::Configurations {
                offset: 0,
                revision: Some(old.revision.clone()),
            },
            WorkspaceQuery::Configuration {
                project_id: "configured-24".into(),
                revision: old.revision.clone(),
            },
            WorkspaceQuery::Configuration {
                project_id: "missing".into(),
                revision: version.clone(),
            },
            WorkspaceQuery::Configuration {
                project_id: "../configured-24".into(),
                revision: version.clone(),
            },
            WorkspaceQuery::Configuration {
                project_id: "configured-24".into(),
                revision: "not-a-digest".into(),
            },
        ] {
            let rejected = reply(&data, invalid).await;
            assert!(rejected.workspace.is_none());
            assert_eq!(
                rejected.response.error.unwrap().code,
                "workspaceReadRejected"
            );
        }
        // Transport interruption only: no claim of a production crash or write.
        let mut pipe = tokio::net::windows::named_pipe::ClientOptions::new()
            .open(ipc::endpoint(&data).unwrap())
            .unwrap();
        let greeting: Greeting = ipc::read_frame(&mut pipe, 64 * 1024).await.unwrap();
        assert_eq!(greeting.owner_sid, ipc::current_sid().unwrap());
        assert_eq!(greeting.server_pid, std::process::id());
        assert_eq!(greeting.protocol_version, PROTOCOL_VERSION);
        ipc::write_frame(
            &mut pipe,
            &UserOperation::Workspace {
                id: "configurations-drop-read-reply".into(),
                protocol_version: PROTOCOL_VERSION,
                query: detail_query.clone(),
            },
            64 * 1024,
        )
        .await
        .unwrap();
        drop(pipe);
        assert_eq!(
            serde_json::to_value(query(&data, detail_query).await).unwrap(),
            detail_value
        );
        stop(&data, service).await;
        assert!(Client::connect(&data).await.is_err());
        assert_eq!(before, snapshot(&data));
        if reopen == 0 {
            host = Some(RuntimeHost::open(&data).unwrap());
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires normal Windows host named-pipe permissions; run explicitly"]
async fn workspace_configurations_native_synthetic_incomplete_publication_diagnostic_rejection() {
    use cerulean_domain::workspace::*;
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let (mut host, draft) = project_planning::tests::fixture(temp.path());
    host.configure_project(draft).unwrap();
    let WorkspaceResult::Configurations(page) = host
        .workspace_query(&WorkspaceQuery::Configurations {
            offset: 0,
            revision: None,
        })
        .unwrap()
    else {
        panic!("original page")
    };
    drop(host);
    // Synthetic incomplete publication; not a production crash reproduction.
    fs::write(
        data.join("project-setups/project.00000002.pending"),
        b"synthetic incomplete publication",
    )
    .unwrap();
    let before = snapshot(&data);
    assert!(RuntimeHost::open(&data).is_err());
    let directory = data.clone();
    let service = Service(tokio::spawn(async move { ipc::serve(&directory).await }));
    for query in [
        WorkspaceQuery::Configurations {
            offset: 0,
            revision: None,
        },
        WorkspaceQuery::Configuration {
            project_id: "project".into(),
            revision: page.revision,
        },
    ] {
        let reply = connect(&data)
            .await
            .call(UserOperation::Workspace {
                id: "configurations-diagnostic".into(),
                protocol_version: PROTOCOL_VERSION,
                query,
            })
            .await
            .unwrap();
        assert!(serde_json::to_vec(&reply).unwrap().len() < 128 * 1024);
        assert!(reply.workspace.is_none() && reply.response.result.is_none() && !reply.stopping);
        assert_eq!(reply.response.error.unwrap().code, "workspaceReadRejected");
    }
    stop(&data, service).await;
    assert!(Client::connect(&data).await.is_err());
    assert_eq!(before, snapshot(&data));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "normal Windows named-pipe permissions and CERULEAN_DAG_EXE offline fixture required"]
async fn workspace_observation_native_production_pages_original_and_refusals() {
    use cerulean_domain::{supervision::SupervisionPolicy, workspace::*};
    async fn reply(data: &Path, query: WorkspaceQuery) -> ServiceReply {
        let r = connect(data)
            .await
            .call(UserOperation::Workspace {
                id: "observation-native".into(),
                protocol_version: PROTOCOL_VERSION,
                query,
            })
            .await
            .unwrap();
        assert!(serde_json::to_vec(&r).unwrap().len() < 4 * 1024 * 1024);
        assert!(r.response.result.is_none());
        r
    }
    async fn read(data: &Path, query: WorkspaceQuery) -> WorkspaceResult {
        let r = reply(data, query).await;
        assert!(r.response.error.is_none(), "{:?}", r.response.error);
        let result = r.workspace.unwrap();
        assert!(serde_json::to_vec(&result).unwrap().len() <= 512 * 1024);
        result
    }
    async fn rejected(data: &Path, query: WorkspaceQuery) {
        let r = reply(data, query).await;
        assert!(r.workspace.is_none() && r.response.error.is_some());
    }
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let data = root.join("data");
    let (mut host, mut draft) = project_planning::tests::fixture(root);
    let bin = root.join("bin");
    fs::create_dir(&bin).unwrap();
    let peer = bin.join("codex.exe");
    let source = PathBuf::from(
        std::env::var_os("CERULEAN_DAG_EXE").expect("built offline codex_dag_fixture executable"),
    );
    assert_eq!(source.file_name().unwrap(), "codex_dag_fixture.exe");
    fs::copy(source, &peer).unwrap();
    fs::write(
        bin.join("workspace.json"),
        serde_json::to_vec(&root.join("project")).unwrap(),
    )
    .unwrap();
    for marker in [
        "emit-observations",
        "observations-clean",
        "workspace-observation-pages",
    ] {
        fs::write(
            bin.join(marker),
            b"controlled adapter metadata; no provider",
        )
        .unwrap();
    }
    fs::write(root.join("project/fixture-mode"), "success").unwrap();
    host.register_validator(
        "dag-check".into(),
        "Local fixture check".into(),
        peer.to_string_lossy().into(),
        vec![],
        Default::default(),
    )
    .unwrap();
    draft.codex_executable = peer.to_string_lossy().into();
    draft.requirement = "DAG_STEP_A".into();
    draft.validator_id = "dag-check".into();
    draft.check_args = vec!["validate".into(), "syntax".into()];
    draft.artifact_paths = vec!["result.txt".into()];
    draft.supervision = Some(SupervisionPolicy {
        schema_version: 1,
        repeated_failure_threshold: 2,
        max_advice_per_attempt: 0,
        delivery_enabled: false,
    });
    host.configure_project(draft).unwrap();
    let mut proposal = project_planning::tests::proposal("dag-check");
    if let ManagementDecision::Plan { tasks, .. } = &mut proposal {
        tasks[0].goal = "DAG_STEP_A".into();
    }
    project_planning::tests::complete(&mut host, proposal);
    let digest = host.projects["project"]
        .revision()
        .unwrap()
        .approval_digest()
        .unwrap();
    host.approve("project", &digest).unwrap();
    let setup = host.project_setups["project"].digest.clone();
    host.start_project_run("project", &digest, &setup).unwrap();
    host.orchestrating = true;
    let started = host.start_codex("project", "task", "observation-native", &digest);
    host.orchestrating = false;
    started.unwrap();
    host.pause_project_run(&host.runs.all()[0].id.clone())
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        host.orchestrating = true;
        host.poll_verifications();
        host.orchestrating = false;
        if host.codex_active.is_empty() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "local child did not finish"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // Shutdown correctly refuses a merely Submitted task. Finish this owned
    // fixture via its original approved deterministic local check and explicit
    // chief acceptance, before freezing the read-only observation baseline.
    let store = host.projects.get_mut("project").unwrap();
    let pins = store.revision().unwrap().execution[0]
        .checks
        .iter()
        .map(crate::verifier::PinnedCheck::from_approved)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    store
        .run_verification(
            "observation-local-check",
            "task",
            "observation-native",
            &pins,
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap();
    assert!(store.state().tasks["task"].verified);
    store
        .command(
            "observation-local-accept",
            "chief",
            WorkflowCommand::Accept {
                task: "task".into(),
                attempt: "observation-native".into(),
            },
            CommandContext::default(),
            true,
        )
        .unwrap();
    assert_eq!(store.state().tasks["task"].state, TaskState::Accepted);
    // Production job/capture/persistence, controlled synthetic peer metadata.
    // No injected job or real model output; acceptance is local fixture-only.
    let job = host
        .codex_jobs
        .values()
        .find(|j| j.attempt_id == "observation-native")
        .unwrap()
        .clone();
    let original = crate::worker_observation::read(&data, &job).unwrap();
    assert_eq!(original.trace.events.len(), 45);
    assert!(original.trace.finished);
    assert_eq!(original.trace.rejected, 0);
    assert_eq!(original.trace.duplicates, 1);
    assert!(!original.incidents.is_empty());
    drop(host);
    let before = snapshot(&data);
    let host = RuntimeHost::open(&data).unwrap();
    let original_host = serde_json::to_vec(&host.snapshot()).unwrap();
    let directory = data.clone();
    let service = Service(tokio::spawn(async move {
        ipc::serve_host(&directory, host).await
    }));
    let detail_query = |revision| WorkspaceQuery::WorkerObservation {
        project_id: "project".into(),
        attempt_id: job.attempt_id.clone(),
        revision,
    };
    let WorkspaceResult::WorkerObservation(detail) = read(&data, detail_query(None)).await else {
        panic!("detail")
    };
    assert_eq!(detail.event_count, 45);
    assert_eq!(detail.incident_count as usize, original.incidents.len());
    assert_eq!(detail.evidence_digest, original.evidence_digest);
    assert_eq!(detail.revision_digest, original.revision_digest);
    assert_eq!(detail.task_id, original.task_id);
    assert_eq!(detail.thread_id, original.trace.thread_id);
    assert_eq!(detail.turn_id, original.trace.turn_id);
    assert_eq!(detail.supervision, original.supervision);
    assert!(detail.finished && detail.storage_error.is_none());
    let mut events = vec![];
    let mut offset = 0;
    let mut pages = 0;
    loop {
        let WorkspaceResult::WorkerObservationEvents(page) = read(
            &data,
            WorkspaceQuery::WorkerObservationEvents {
                project_id: "project".into(),
                attempt_id: job.attempt_id.clone(),
                offset,
                revision: detail.revision.clone(),
            },
        )
        .await
        else {
            panic!("events")
        };
        assert_eq!(page.offset, offset);
        assert_eq!(page.total, 45);
        assert_eq!(page.revision, detail.revision);
        assert!(page.entries.len() <= 20 && serde_json::to_vec(&page).unwrap().len() <= 64 * 1024);
        events.extend(page.entries);
        pages += 1;
        match page.next_offset {
            Some(next) => {
                assert!(next > offset);
                offset = next;
            }
            None => break,
        }
    }
    assert!(pages >= 3);
    assert_eq!(events, original.trace.events);
    let mut incidents = vec![];
    offset = 0;
    loop {
        let WorkspaceResult::WorkerObservationIncidents(page) = read(
            &data,
            WorkspaceQuery::WorkerObservationIncidents {
                project_id: "project".into(),
                attempt_id: job.attempt_id.clone(),
                offset,
                revision: detail.revision.clone(),
            },
        )
        .await
        else {
            panic!("incidents")
        };
        assert_eq!(page.offset, offset);
        assert_eq!(page.total as usize, original.incidents.len());
        assert!(page.entries.len() <= 20 && serde_json::to_vec(&page).unwrap().len() <= 64 * 1024);
        incidents.extend(page.entries);
        match page.next_offset {
            Some(next) => {
                assert!(next > offset);
                offset = next;
            }
            None => break,
        }
    }
    assert_eq!(incidents, original.incidents);
    rejected(&data, detail_query(Some("stale".into()))).await;
    rejected(
        &data,
        WorkspaceQuery::WorkerObservationEvents {
            project_id: "project".into(),
            attempt_id: job.attempt_id.clone(),
            offset: 0,
            revision: "stale".into(),
        },
    )
    .await;
    rejected(
        &data,
        WorkspaceQuery::WorkerObservationIncidents {
            project_id: "project".into(),
            attempt_id: job.attempt_id.clone(),
            offset: u32::MAX,
            revision: detail.revision.clone(),
        },
    )
    .await;
    rejected(
        &data,
        WorkspaceQuery::WorkerObservation {
            project_id: "project".into(),
            attempt_id: "unknown-attempt".into(),
            revision: None,
        },
    )
    .await;
    stop(&data, service).await;
    assert_eq!(snapshot(&data), before);
    let reopened = RuntimeHost::open(&data).unwrap();
    assert_eq!(
        serde_json::to_vec(&reopened.snapshot()).unwrap(),
        original_host
    );
    let directory = data.clone();
    let service = Service(tokio::spawn(async move {
        ipc::serve_host(&directory, reopened).await
    }));
    assert_eq!(
        serde_json::to_value(read(&data, detail_query(Some(detail.revision.clone()))).await)
            .unwrap(),
        serde_json::to_value(WorkspaceResult::WorkerObservation(detail.clone())).unwrap()
    );
    stop(&data, service).await;
    assert_eq!(snapshot(&data), before);
    // Synthetic replacement/boundary tests, explicitly separate from lifecycle.
    let file = data
        .join("worker-observations/project")
        .join(format!("{}.json", job.attempt_id));
    let bytes = fs::read(&file).unwrap();
    let mut trace = original.trace.clone();
    trace.duplicates += 1;
    let changed =
        crate::worker_observation::view(&job, trace, None, original.supervision.clone()).unwrap();
    let mut record: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    record["view"] = serde_json::to_value(changed).unwrap();
    fs::write(&file, serde_json::to_vec(&record).unwrap()).unwrap();
    let host = RuntimeHost::open(&data).unwrap();
    let directory = data.clone();
    let service = Service(tokio::spawn(async move {
        ipc::serve_host(&directory, host).await
    }));
    rejected(&data, detail_query(Some(detail.revision.clone()))).await;
    rejected(
        &data,
        WorkspaceQuery::WorkerObservationEvents {
            project_id: "project".into(),
            attempt_id: job.attempt_id.clone(),
            offset: 20,
            revision: detail.revision.clone(),
        },
    )
    .await;
    stop(&data, service).await;
    fs::write(&file, &bytes).unwrap();
    for oversized in [false, true] {
        if oversized {
            fs::write(&file, vec![b' '; 256 * 1024 + 1]).unwrap();
        } else {
            fs::remove_file(&file).unwrap();
        }
        let boundary_before = snapshot(&data);
        let host = RuntimeHost::open(&data).unwrap();
        let directory = data.clone();
        let service = Service(tokio::spawn(async move {
            ipc::serve_host(&directory, host).await
        }));
        rejected(&data, detail_query(None)).await;
        stop(&data, service).await;
        assert_eq!(snapshot(&data), boundary_before);
        fs::write(&file, &bytes).unwrap();
    }
    assert_eq!(snapshot(&data), before);
    for interrupted_owner in [false, true] {
        let mut host = RuntimeHost::open(&data).unwrap();
        if interrupted_owner {
            let store = host.projects.remove("project").unwrap();
            let mut writer =
                crate::async_api::ApiWorkflow::start(store, data.join("workflows/project"))
                    .unwrap();
            writer.interrupted = true;
            host.api_writers.insert("project".into(), writer);
        } else {
            let workflow_digest = host.projects["project"].state().digest.clone().unwrap();
            let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                host.projects.get_mut("project").unwrap().command_observed(
                    "observation-poisoned-writer",
                    "user",
                    WorkflowCommand::RequestRevision {
                        digest: workflow_digest,
                        reason: "controlled observation read-boundary interruption".into(),
                    },
                    CommandContext {
                        approval_source: Some("authenticated local-user revision request".into()),
                        ..Default::default()
                    },
                    true,
                    |point| {
                        if point == crate::scheduler::CommitPoint::BeforeEventWrite {
                            panic!("controlled observation interruption before write");
                        }
                    },
                )
            }));
            assert!(interrupted.is_err());
            assert!(host.projects["project"].requires_reopen());
        }
        let directory = data.clone();
        let service = Service(tokio::spawn(async move {
            ipc::serve_host(&directory, host).await
        }));
        rejected(&data, detail_query(None)).await;
        rejected(
            &data,
            WorkspaceQuery::WorkerObservationEvents {
                project_id: "project".into(),
                attempt_id: job.attempt_id.clone(),
                offset: 0,
                revision: detail.revision.clone(),
            },
        )
        .await;
        rejected(
            &data,
            WorkspaceQuery::WorkerObservationIncidents {
                project_id: "project".into(),
                attempt_id: job.attempt_id.clone(),
                offset: 0,
                revision: detail.revision.clone(),
            },
        )
        .await;
        stop(&data, service).await;
        assert_eq!(snapshot(&data), before);
    }
    fs::write(
        data.join("project-setups/project.00000002.pending"),
        b"synthetic incomplete publication",
    )
    .unwrap();
    let diagnostic_before = snapshot(&data);
    assert!(RuntimeHost::open(&data).is_err());
    let directory = data.clone();
    let service = Service(tokio::spawn(async move { ipc::serve(&directory).await }));
    rejected(&data, detail_query(None)).await;
    rejected(
        &data,
        WorkspaceQuery::WorkerObservationEvents {
            project_id: "project".into(),
            attempt_id: job.attempt_id.clone(),
            offset: 0,
            revision: detail.revision.clone(),
        },
    )
    .await;
    rejected(
        &data,
        WorkspaceQuery::WorkerObservationIncidents {
            project_id: "project".into(),
            attempt_id: job.attempt_id.clone(),
            offset: 0,
            revision: detail.revision.clone(),
        },
    )
    .await;
    stop(&data, service).await;
    assert_eq!(snapshot(&data), diagnostic_before);
    println!("production lifecycle with synthetic adapter metadata:45 original events, complete incidents, bounded native replies, reopen unchanged; model_calls=0; controlled poisoned/interrupted original owner refusals covered, not real crash reproduction");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "normal Windows authenticated named-pipe permissions and CERULEAN_DAG_EXE offline peer required"]
async fn workspace_verification_result_native_production_pages_original_and_refusals() {
    use cerulean_domain::{
        project_setup::{AcceptanceDraft, AcceptanceStageDraft},
        workspace::*,
    };
    async fn reply(data: &Path, query: WorkspaceQuery) -> ServiceReply {
        let r = connect(data)
            .await
            .call(UserOperation::Workspace {
                id: "verification-result-native".into(),
                protocol_version: PROTOCOL_VERSION,
                query,
            })
            .await
            .unwrap();
        assert!(serde_json::to_vec(&r).unwrap().len() < 4 * 1024 * 1024);
        assert!(r.response.result.is_none());
        r
    }
    async fn read(data: &Path, query: WorkspaceQuery) -> WorkspaceResult {
        let r = reply(data, query).await;
        assert!(r.response.error.is_none(), "{:?}", r.response.error);
        let result = r.workspace.unwrap();
        assert!(serde_json::to_vec(&result).unwrap().len() <= 512 * 1024);
        result
    }
    async fn rejected(data: &Path, query: WorkspaceQuery) {
        let r = reply(data, query).await;
        assert!(r.workspace.is_none() && r.response.error.is_some());
        assert!(!r.response.error.unwrap().message.is_empty());
    }
    fn stable(mut value: serde_json::Value) -> serde_json::Value {
        value.as_object_mut().unwrap().remove("observedAt");
        value
    }
    fn all_queries(operation: &str, revision: &str) -> [WorkspaceQuery; 3] {
        [
            WorkspaceQuery::VerificationResult {
                project_id: "project".into(),
                operation_id: operation.into(),
                revision: Some(revision.into()),
            },
            WorkspaceQuery::VerificationResultArtifacts {
                project_id: "project".into(),
                operation_id: operation.into(),
                offset: 0,
                revision: revision.into(),
            },
            WorkspaceQuery::VerificationResultStages {
                project_id: "project".into(),
                operation_id: operation.into(),
                offset: 0,
                revision: revision.into(),
            },
        ]
    }
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let data = root.join("data");
    let (mut host, mut draft) = project_planning::tests::fixture(root);
    let bin = root.join("bin");
    fs::create_dir(&bin).unwrap();
    let source = PathBuf::from(
        std::env::var_os("CERULEAN_DAG_EXE").expect("built offline codex_dag_fixture executable"),
    );
    assert_eq!(source.file_name().unwrap(), "codex_dag_fixture.exe");
    let peer = bin.join("codex.exe");
    fs::copy(source, &peer).unwrap();
    fs::write(
        bin.join("workspace.json"),
        serde_json::to_vec(&root.join("project")).unwrap(),
    )
    .unwrap();
    fs::write(root.join("project/fixture-mode"), "success").unwrap();
    host.register_validator(
        "dag-check".into(),
        "Local offline check".into(),
        peer.to_string_lossy().into(),
        vec![],
        Default::default(),
    )
    .unwrap();
    draft.codex_executable = peer.to_string_lossy().into();
    draft.requirement =
        "DAG_STEP_A: preserve approved auxiliary artifacts and write result.txt".into();
    draft.validator_id = "dag-check".into();
    draft.check_args = vec!["validate".into(), "syntax".into()];
    draft.artifact_paths = vec!["result.txt".into()];
    for i in 0..44 {
        let path = format!("aux-{i:02}.txt");
        fs::write(
            root.join("project").join(&path),
            format!("approved existing auxiliary artifact {i}\n"),
        )
        .unwrap();
        draft.artifact_paths.push(path);
    }
    // Production acceptance configuration allows at most eight stages; do not
    // forge an illegal >20-stage plan to manufacture native pagination.
    draft.acceptance = Some(AcceptanceDraft {
        schema_version: 1,
        stages: (0..8)
            .map(|i| AcceptanceStageDraft {
                id: format!("check-{i}"),
                name: format!("Approved local check {i}"),
                validator_id: "dag-check".into(),
                args: vec![
                    "validate".into(),
                    if i % 2 == 0 {
                        "syntax".into()
                    } else {
                        "unit".into()
                    },
                ],
                timeout_seconds: 10,
            })
            .collect(),
    });
    host.configure_project(draft).unwrap();
    let mut proposal = project_planning::tests::proposal("dag-check");
    if let ManagementDecision::Plan { tasks, .. } = &mut proposal {
        tasks[0].goal = "DAG_STEP_A".into();
    }
    project_planning::tests::complete(&mut host, proposal);
    let digest = host.projects["project"]
        .revision()
        .unwrap()
        .approval_digest()
        .unwrap();
    host.approve("project", &digest).unwrap();
    let setup = host.project_setups["project"].digest.clone();
    host.start_project_run("project", &digest, &setup).unwrap();
    host.orchestrating = true;
    host.start_codex("project", "task", "verification-native", &digest)
        .unwrap();
    host.orchestrating = false;
    host.pause_project_run(&host.runs.all()[0].id.clone())
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    loop {
        host.orchestrating = true;
        host.poll_verifications();
        host.orchestrating = false;
        if host.codex_active.is_empty() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "offline worker did not finish"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Submitted
    );
    let pins = host.projects["project"].revision().unwrap().execution[0]
        .checks
        .iter()
        .map(crate::verifier::PinnedCheck::from_approved)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    host.orchestrating = true;
    host.start_verification(
        "project",
        "verification-local-check",
        "task",
        "verification-native",
        pins,
    )
    .unwrap();
    host.orchestrating = false;
    loop {
        host.orchestrating = true;
        host.poll_verifications();
        host.orchestrating = false;
        if host.active.is_empty() {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "independent checks did not finish"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let job = host.jobs[&("project".into(), "verification-local-check".into())].clone();
    assert_eq!(job.state, VerificationState::Succeeded);
    host.projects
        .get_mut("project")
        .unwrap()
        .command(
            "verification-local-accept",
            "chief",
            WorkflowCommand::Accept {
                task: "task".into(),
                attempt: "verification-native".into(),
            },
            CommandContext::default(),
            true,
        )
        .unwrap();
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Accepted
    );
    drop(host);
    let data = fs::canonicalize(&data).unwrap();
    let mut host = RuntimeHost::open(&data).unwrap();
    let basis = crate::verification_detail::basis(&host.projects["project"], &job).unwrap();
    let original = crate::verification_detail::read(&data, &job, &basis).unwrap();
    assert_eq!(original.artifacts.len(), 45);
    assert_eq!(original.stages.len(), 8);
    assert!(original.ledger_recorded);
    crate::host::workspace_verification::production_shared_owner_transfer(
        &mut host,
        &job.operation_id,
    );
    drop(host);
    let before = snapshot(&data);
    let project_before = snapshot(&root.join("project"));
    let host = RuntimeHost::open(&data).unwrap();
    let directory = data.clone();
    let service = Service(tokio::spawn(async move {
        ipc::serve_host(&directory, host).await
    }));
    let detail_query = |revision| WorkspaceQuery::VerificationResult {
        project_id: "project".into(),
        operation_id: job.operation_id.clone(),
        revision,
    };
    let WorkspaceResult::VerificationResult(detail) = read(&data, detail_query(None)).await else {
        panic!("metadata")
    };
    assert_eq!(detail.artifact_count, 45);
    assert_eq!(detail.stage_count, 8);
    let mut metadata = serde_json::to_value(&detail).unwrap();
    for key in ["revision", "artifactCount", "stageCount"] {
        metadata.as_object_mut().unwrap().remove(key);
    }
    let mut expected = serde_json::to_value(&original).unwrap();
    for key in ["revision", "artifacts", "stages"] {
        expected.as_object_mut().unwrap().remove(key);
    }
    assert_eq!(stable(metadata), stable(expected));
    let mut artifacts = vec![];
    let mut offset = 0;
    let mut pages = 0;
    loop {
        let WorkspaceResult::VerificationResultArtifacts(page) = read(
            &data,
            WorkspaceQuery::VerificationResultArtifacts {
                project_id: "project".into(),
                operation_id: job.operation_id.clone(),
                offset,
                revision: detail.revision.clone(),
            },
        )
        .await
        else {
            panic!("artifacts")
        };
        assert_eq!(page.project_id, "project");
        assert_eq!(page.operation_id, job.operation_id);
        assert_eq!(page.revision, detail.revision);
        assert_eq!(page.offset, offset);
        assert_eq!(page.total, 45);
        assert!(!page.entries.is_empty() && page.entries.len() <= 20);
        assert!(serde_json::to_vec(&page).unwrap().len() <= 64 * 1024);
        artifacts.extend(page.entries);
        pages += 1;
        match page.next_offset {
            Some(next) => {
                assert_eq!(next as usize, artifacts.len());
                assert!(next > offset);
                offset = next;
            }
            None => break,
        }
    }
    assert!(pages >= 3);
    assert_eq!(artifacts, original.artifacts);
    let WorkspaceResult::VerificationResultStages(page) = read(
        &data,
        WorkspaceQuery::VerificationResultStages {
            project_id: "project".into(),
            operation_id: job.operation_id.clone(),
            offset: 0,
            revision: detail.revision.clone(),
        },
    )
    .await
    else {
        panic!("stages")
    };
    assert_eq!(page.project_id, "project");
    assert_eq!(page.operation_id, job.operation_id);
    assert_eq!(page.revision, detail.revision);
    assert_eq!(page.offset, 0);
    assert_eq!(page.total, 8);
    assert!(page.next_offset.is_none() && page.entries.len() <= 20);
    assert!(serde_json::to_vec(&page).unwrap().len() <= 64 * 1024);
    assert_eq!(
        serde_json::to_value(&page.entries).unwrap(),
        serde_json::to_value(&original.stages).unwrap()
    );
    for query in all_queries(&job.operation_id, &"0".repeat(64)) {
        rejected(&data, query).await;
    }
    for query in [
        WorkspaceQuery::VerificationResultArtifacts {
            project_id: "project".into(),
            operation_id: job.operation_id.clone(),
            offset: u32::MAX,
            revision: detail.revision.clone(),
        },
        WorkspaceQuery::VerificationResultStages {
            project_id: "project".into(),
            operation_id: job.operation_id.clone(),
            offset: u32::MAX,
            revision: detail.revision.clone(),
        },
        WorkspaceQuery::VerificationResult {
            project_id: "unknown-project".into(),
            operation_id: job.operation_id.clone(),
            revision: None,
        },
        WorkspaceQuery::VerificationResult {
            project_id: "project".into(),
            operation_id: "unknown-operation".into(),
            revision: None,
        },
    ] {
        rejected(&data, query).await;
    }
    stop(&data, service).await;
    assert!(Client::connect(&data).await.is_err());
    assert_eq!(snapshot(&data), before);
    assert_eq!(snapshot(&root.join("project")), project_before);
    let host = RuntimeHost::open(&data).unwrap();
    let directory = data.clone();
    let service = Service(tokio::spawn(async move {
        ipc::serve_host(&directory, host).await
    }));
    let WorkspaceResult::VerificationResult(reopened) =
        read(&data, detail_query(Some(detail.revision.clone()))).await
    else {
        panic!("reopened metadata")
    };
    assert_eq!(
        stable(serde_json::to_value(&reopened).unwrap()),
        stable(serde_json::to_value(&detail).unwrap())
    );
    for query in all_queries(&job.operation_id, &detail.revision) {
        read(&data, query).await;
    }
    stop(&data, service).await;
    assert_eq!(snapshot(&data), before);
    // Controlled file-boundary injections on this isolated owned fixture only.
    // They prove refusal, not a naturally reproduced process crash.
    let file = data
        .join("workflows/project/verification")
        .join(&job.operation_id)
        .join("intent.json");
    let bytes = fs::read(&file).unwrap();
    for oversized in [false, true] {
        if oversized {
            fs::write(&file, vec![b' '; 1024 * 1024 + 1]).unwrap();
        } else {
            fs::remove_file(&file).unwrap();
        }
        let boundary_before = snapshot(&data);
        let host = RuntimeHost::open(&data).unwrap();
        let directory = data.clone();
        let service = Service(tokio::spawn(async move {
            ipc::serve_host(&directory, host).await
        }));
        for query in all_queries(&job.operation_id, &detail.revision) {
            rejected(&data, query).await;
        }
        stop(&data, service).await;
        assert_eq!(snapshot(&data), boundary_before);
        fs::write(&file, &bytes).unwrap();
    }
    for interrupted_owner in [false, true] {
        let mut host = RuntimeHost::open(&data).unwrap();
        if interrupted_owner {
            let store = host.projects.remove("project").unwrap();
            let mut writer =
                crate::async_api::ApiWorkflow::start(store, data.join("workflows/project"))
                    .unwrap();
            writer.interrupted = true;
            host.api_writers.insert("project".into(), writer);
        } else {
            let workflow_digest = host.projects["project"].state().digest.clone().unwrap();
            let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = host.projects.get_mut("project").unwrap().command_observed(
                    "verification-poisoned-writer",
                    "user",
                    WorkflowCommand::RequestRevision {
                        digest: workflow_digest,
                        reason: "controlled read-boundary interruption".into(),
                    },
                    CommandContext {
                        approval_source: Some("authenticated local-user revision request".into()),
                        ..Default::default()
                    },
                    true,
                    |point| {
                        if point == crate::scheduler::CommitPoint::BeforeEventWrite {
                            panic!("controlled verification interruption before write");
                        }
                    },
                );
            }));
            assert!(interrupted.is_err());
            assert!(host.projects["project"].requires_reopen());
        }
        let directory = data.clone();
        let service = Service(tokio::spawn(async move {
            ipc::serve_host(&directory, host).await
        }));
        for query in all_queries(&job.operation_id, &detail.revision) {
            rejected(&data, query).await;
        }
        stop(&data, service).await;
        assert_eq!(snapshot(&data), before);
    }
    fs::write(
        data.join("project-setups/project.00000002.pending"),
        b"controlled incomplete publication",
    )
    .unwrap();
    let diagnostic_before = snapshot(&data);
    assert!(RuntimeHost::open(&data).is_err());
    let directory = data.clone();
    let service = Service(tokio::spawn(async move { ipc::serve(&directory).await }));
    for query in all_queries(&job.operation_id, &detail.revision) {
        rejected(&data, query).await;
    }
    stop(&data, service).await;
    assert_eq!(snapshot(&data), diagnostic_before);
    assert_eq!(snapshot(&root.join("project")), project_before);
    println!("158 native: original approved production worker/verification/chief acceptance; 45 artifacts across {pages} pages; eight legal stages; all three queries bounded; stale/cursor/identity/missing/oversize/poisoned/interrupted/diagnostic rejected; original records preserved; shutdown/reopen cursor stable; model_calls=0; native stage cross-page and real crash/lost ACK are not claimed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "normal Windows authenticated named-pipe permissions and CERULEAN_DAG_EXE offline peer required"]
async fn workspace_codex_startups_native_production_pages_original_and_refusals() {
    use cerulean_domain::workspace::*;
    async fn reply(data: &Path, query: WorkspaceQuery) -> ServiceReply {
        let client = connect(data).await;
        assert_eq!(client.server_pid, std::process::id());
        drop(client);
        let endpoint = ipc::endpoint(data).unwrap();
        let mut pipe = tokio::net::windows::named_pipe::ClientOptions::new()
            .open(endpoint)
            .unwrap();
        let greeting: Greeting = ipc::read_frame(&mut pipe, 64 * 1024).await.unwrap();
        assert_eq!(greeting.product, cerulean_domain::PRODUCT_NAME);
        assert_eq!(greeting.owner_sid, ipc::current_sid().unwrap());
        assert_eq!(greeting.server_pid, std::process::id());
        assert_eq!(greeting.protocol_version, PROTOCOL_VERSION);
        let r: ServiceReply = {
            ipc::write_frame(
                &mut pipe,
                &UserOperation::Workspace {
                    id: "codex-startups-native".into(),
                    protocol_version: PROTOCOL_VERSION,
                    query,
                },
                64 * 1024,
            )
            .await
            .unwrap();
            ipc::read_frame(&mut pipe, 4 * 1024 * 1024).await.unwrap()
        };
        assert_eq!(r.response.id, "codex-startups-native");
        assert!(serde_json::to_vec(&r).unwrap().len() < 4 * 1024 * 1024);
        assert!(r.response.result.is_none() && r.historical.is_none());
        assert!(r.command_receipt.is_none() && !r.stopping);
        r
    }
    async fn read(data: &Path, query: WorkspaceQuery) -> WorkspaceResult {
        let r = reply(data, query).await;
        assert!(r.response.error.is_none(), "{:?}", r.response.error);
        let result = r.workspace.unwrap();
        assert!(serde_json::to_vec(&result).unwrap().len() <= 512 * 1024);
        result
    }
    async fn rejected(data: &Path, query: WorkspaceQuery) {
        let r = reply(data, query).await;
        assert!(r.workspace.is_none());
        let error = r.response.error.expect("explicit startup refusal");
        assert_eq!(error.code, "workspaceReadRejected");
        assert!(!error.message.is_empty());
    }
    fn list(offset: u32, revision: Option<String>) -> WorkspaceQuery {
        WorkspaceQuery::CodexStartups {
            project_id: "project".into(),
            offset,
            revision,
        }
    }
    fn detail(attempt: &str, revision: &str) -> WorkspaceQuery {
        WorkspaceQuery::CodexStartup {
            project_id: "project".into(),
            attempt_id: attempt.into(),
            revision: revision.into(),
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let data = root.join("data");
    let (mut host, mut draft) = project_planning::tests::fixture(root);
    let bin = root.join("bin");
    fs::create_dir(&bin).unwrap();
    let source = PathBuf::from(
        std::env::var_os("CERULEAN_DAG_EXE").expect("built offline codex_dag_fixture executable"),
    );
    assert_eq!(source.file_name().unwrap(), "codex_dag_fixture.exe");
    let peer = bin.join("codex.exe");
    fs::copy(source, &peer).unwrap();
    fs::write(
        bin.join("workspace.json"),
        serde_json::to_vec(&root.join("project")).unwrap(),
    )
    .unwrap();
    fs::write(root.join("project/fixture-mode"), "success").unwrap();
    fs::write(
        bin.join("allow-explicit-rework"),
        b"approved repeated startup fixture",
    )
    .unwrap();
    host.register_validator(
        "dag-check".into(),
        "Offline approved check".into(),
        peer.to_string_lossy().into(),
        vec![],
        Default::default(),
    )
    .unwrap();
    draft.codex_executable = peer.to_string_lossy().into();
    draft.requirement = "DAG_STEP_A: write result.txt for approved startup records".into();
    draft.validator_id = "dag-check".into();
    draft.check_args = vec!["validate".into(), "syntax".into()];
    draft.artifact_paths = vec!["result.txt".into()];
    draft.worker_max_requests = 21;
    host.configure_project(draft).unwrap();
    let mut proposal = project_planning::tests::proposal("dag-check");
    if let ManagementDecision::Plan { tasks, .. } = &mut proposal {
        let original = tasks[0].clone();
        *tasks = (0..21)
            .map(|n| {
                let mut task = original.clone();
                task.id = format!("task-{n:02}");
                task.goal = "DAG_STEP_A".into();
                task
            })
            .collect();
    }
    project_planning::tests::complete(&mut host, proposal);
    let revision = host.projects["project"]
        .revision()
        .unwrap()
        .approval_digest()
        .unwrap();
    host.approve("project", &revision).unwrap();
    let setup = host.project_setups["project"].digest.clone();
    host.start_project_run("project", &revision, &setup)
        .unwrap();
    let run = host.runs.all()[0].id.clone();
    for n in 0..21 {
        let task = format!("task-{n:02}");
        let attempt = format!("startup-{n:02}");
        host.orchestrating = true;
        host.start_codex("project", &task, &attempt, &revision)
            .unwrap();
        host.orchestrating = false;
        if n == 0 {
            host.pause_project_run(&run).unwrap();
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(60);
        loop {
            host.orchestrating = true;
            host.poll_verifications();
            host.orchestrating = false;
            if host.codex_active.is_empty() {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "offline worker did not finish {attempt}"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let store = host.projects.get_mut("project").unwrap();
        assert_eq!(store.state().tasks[&task].state, TaskState::Submitted);
        let pins = store
            .revision()
            .unwrap()
            .execution
            .iter()
            .find(|e| e.task_id == task)
            .unwrap()
            .checks
            .iter()
            .map(crate::verifier::PinnedCheck::from_approved)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        store
            .run_verification(
                &format!("startup-check-{n:02}"),
                &task,
                &attempt,
                &pins,
                &std::sync::atomic::AtomicBool::new(false),
            )
            .unwrap();
        assert!(store.state().tasks[&task].verified);
        store
            .command(
                &format!("startup-accept-{n:02}"),
                "chief",
                WorkflowCommand::Accept {
                    task: task.clone(),
                    attempt,
                },
                CommandContext::default(),
                true,
            )
            .unwrap();
        assert_eq!(store.state().tasks[&task].state, TaskState::Accepted);
    }
    let original = host.startup_detail("project", "startup-20").unwrap();
    let summaries = host.startup_summaries();
    assert_eq!(summaries.len(), 21);
    drop(host);
    let before = snapshot(&data);
    let project_before = snapshot(&root.join("project"));
    let mut page_revision = None;
    let mut original_detail = None;
    let mut pages = 0;
    for reopen in 0..2 {
        let host = RuntimeHost::open(&data).unwrap();
        let directory = data.clone();
        let service = Service(tokio::spawn(async move {
            ipc::serve_host(&directory, host).await
        }));
        let mut offset = 0;
        let mut entries = vec![];
        loop {
            let WorkspaceResult::CodexStartups(page) =
                read(&data, list(offset, page_revision.clone())).await
            else {
                panic!("startup page")
            };
            assert_eq!(page.project_id, "project");
            assert_eq!(page.total, 21);
            assert_eq!(page.offset, offset);
            assert!(!page.entries.is_empty() && page.entries.len() <= 20);
            assert!(serde_json::to_vec(&page).unwrap().len() <= 64 * 1024);
            if let Some(expected) = &page_revision {
                assert_eq!(&page.revision, expected);
            }
            page_revision = Some(page.revision.clone());
            entries.extend(page.entries);
            if reopen == 0 {
                pages += 1;
            }
            match page.next_offset {
                Some(next) => {
                    assert_eq!(next as usize, entries.len());
                    assert!(next > offset);
                    offset = next;
                }
                None => break,
            }
        }
        assert_eq!(
            serde_json::to_value(entries).unwrap(),
            serde_json::to_value(&summaries).unwrap()
        );
        let version = page_revision.as_deref().unwrap();
        let found = read(&data, detail("startup-20", version)).await;
        let WorkspaceResult::CodexStartup(value) = &found else {
            panic!("startup detail")
        };
        assert_eq!(value.project_id, "project");
        assert_eq!(value.revision, version);
        assert_eq!(
            serde_json::to_value(&value.detail).unwrap(),
            serde_json::to_value(&original).unwrap()
        );
        let found = serde_json::to_value(found).unwrap();
        if let Some(expected) = &original_detail {
            assert_eq!(&found, expected);
        }
        original_detail = Some(found);
        for q in [
            list(20, None),
            list(u32::MAX, Some(version.into())),
            list(0, Some("0".repeat(64))),
            detail("startup-20", &"0".repeat(64)),
            detail("missing-attempt", version),
            detail("startup-20", ""),
            WorkspaceQuery::CodexStartups {
                project_id: "missing-project".into(),
                offset: 0,
                revision: None,
            },
            WorkspaceQuery::CodexStartup {
                project_id: "missing-project".into(),
                attempt_id: "startup-20".into(),
                revision: version.into(),
            },
        ] {
            rejected(&data, q).await;
        }
        stop(&data, service).await;
        assert!(Client::connect(&data).await.is_err());
        assert_eq!(snapshot(&data), before);
        assert_eq!(snapshot(&root.join("project")), project_before);
    }
    assert!(pages >= 2);
    let version = page_revision.unwrap();
    for mode in ["interrupted", "poisoned", "inconsistent"] {
        let mut host = RuntimeHost::open(&data).unwrap();
        match mode {
            "interrupted" => {
                let store = host.projects.remove("project").unwrap();
                let mut writer =
                    crate::async_api::ApiWorkflow::start(store, data.join("workflows/project"))
                        .unwrap();
                writer.interrupted = true;
                host.api_writers.insert("project".into(), writer);
            }
            "poisoned" => host.base.poisoned = true,
            _ => {
                host.codex_jobs
                    .get_mut(&("project".into(), "startup-20".into()))
                    .unwrap()
                    .task_id = "wrong-task".into();
            }
        }
        let directory = data.clone();
        let service = Service(tokio::spawn(async move {
            ipc::serve_host(&directory, host).await
        }));
        rejected(&data, list(0, None)).await;
        rejected(&data, detail("startup-20", &version)).await;
        stop(&data, service).await;
        assert_eq!(snapshot(&data), before);
    }
    let file = data.join("codex-startups/project/startup-20.0.json");
    let bytes = fs::read(&file).unwrap();
    for oversized in [false, true] {
        if oversized {
            fs::write(&file, vec![b' '; 64 * 1024 + 1]).unwrap();
        } else {
            fs::remove_file(&file).unwrap();
        }
        let boundary_before = snapshot(&data);
        let service = if oversized {
            assert!(RuntimeHost::open(&data).is_err());
            let directory = data.clone();
            Service(tokio::spawn(async move { ipc::serve(&directory).await }))
        } else {
            let host = RuntimeHost::open(&data).unwrap();
            let directory = data.clone();
            Service(tokio::spawn(async move {
                ipc::serve_host(&directory, host).await
            }))
        };
        rejected(&data, list(0, None)).await;
        rejected(&data, detail("startup-20", &version)).await;
        stop(&data, service).await;
        assert_eq!(snapshot(&data), boundary_before);
        fs::write(&file, &bytes).unwrap();
    }
    fs::write(
        data.join("project-setups/project.00000002.pending"),
        b"controlled incomplete publication",
    )
    .unwrap();
    let diagnostic_before = snapshot(&data);
    assert!(RuntimeHost::open(&data).is_err());
    let directory = data.clone();
    let service = Service(tokio::spawn(async move { ipc::serve(&directory).await }));
    rejected(&data, list(0, None)).await;
    rejected(&data, detail("startup-20", &version)).await;
    stop(&data, service).await;
    assert_eq!(snapshot(&data), diagnostic_before);
    assert_eq!(snapshot(&root.join("project")), project_before);
    println!("159 native: original approved production starts=21; pages={pages}; full replies<4MiB; result<=512KiB; page<=64KiB and <=20; exact original detail and files unchanged across Shutdown/reopen; stale/missing/cursor/unknown identity/interrupted/poisoned/inconsistent/diagnostic/persisted oversize refused; model_calls=0; synthetic chief and local protocol peer; not real model/crash/lost ACK or legally publishable oversized detail");
}
