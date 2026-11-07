#![cfg(windows)]
use cerulean_core::{
    host::RuntimeHost,
    ipc::{Client, ServiceReply, UserOperation},
    scheduler::*,
};
use cerulean_domain::{
    execution::*, workflow::*, workspace::*, Command, Request, PROTOCOL_VERSION,
};
use std::{fs, path::Path};
fn plan(root: &Path) -> PlanRevision {
    PlanRevision {
        management_setup_digest: None,
        worktree: None,
        snapshot_policy: None,
        schema_version: 1,
        project: Project {
            id: "project".into(),
            root: root.to_string_lossy().into(),
            timezone: "Asia/Shanghai".into(),
        },
        workflow: WorkflowPlan {
            id: "plan".into(),
            version: 1,
            tasks: vec![WorkflowTask {
                id: "task".into(),
                workspace_key: "project".into(),
                goal: "Fixture result".into(),
                dependencies: vec![],
                resource_keys: None,
            }],
        },
        policy: ExecutionPolicy {
            supervision: None,
            version: 1,
            tools: vec![
                ToolPermission::ReadFiles,
                ToolPermission::WriteFiles,
                ToolPermission::RunCommands,
            ],
            max_requests: 3,
            max_seconds: 60,
            currency_budget: None,
            budget_usd_cents: Some(25),
        },
        execution: vec![TaskExecution {
            worker_directory: None,
            supervision_override: None,
            profile_id: None,
            desktop_worker: None,
            api_worker: None,
            worker: None,
            task_id: "task".into(),
            adapter_id: "codex".into(),
            artifact_paths: vec!["result.txt".into()],
            checks: vec![AcceptanceCheck {
                expected_assertions: vec![],
                name: None,
                tool: Some(
                    cerulean_core::verifier::PinnedCheck::describe(
                        &std::env::current_exe().unwrap(),
                        &[],
                        Default::default(),
                    )
                    .unwrap(),
                ),
                id: "check".into(),
                program: "fixture-validator".into(),
                args: vec![],
                timeout_seconds: 10,
            }],
        }],
    }
}

async fn connect(data: &Path) -> Client {
    for _ in 0..150 {
        if let Ok(c) = Client::connect(data).await {
            return c;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("fixture service did not start");
}
async fn read(
    data: &Path,
    project: &str,
    after: Option<EventAnchor>,
    before: Option<EventAnchor>,
) -> ServiceReply {
    connect(data)
        .await
        .call(UserOperation::Workspace {
            id: "activity-test".into(),
            protocol_version: PROTOCOL_VERSION,
            query: WorkspaceQuery::ProjectActivity {
                project_id: project.into(),
                after,
                before,
            },
        })
        .await
        .unwrap()
}
fn page(reply: ServiceReply) -> WorkspaceActivityPage {
    assert!(reply.response.error.is_none(), "{:?}", reply.response.error);
    assert!(reply.response.result.is_none(), "no full snapshot");
    match reply.workspace.unwrap() {
        WorkspaceResult::ProjectActivity(p) => p,
        _ => panic!("wrong response"),
    }
}
async fn stop(data: &Path) {
    let r = connect(data)
        .await
        .call(UserOperation::Shutdown {
            id: "fixture-stop".into(),
            protocol_version: PROTOCOL_VERSION,
        })
        .await
        .unwrap();
    assert!(r.stopping, "{:?}", r.response.error);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn committed_activity_paginates_authenticates_cursors_and_survives_reopen_without_writes() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let project = temp.path().join("project");
    fs::create_dir(&project).unwrap();
    let mut p = plan(&project);
    let mut host = RuntimeHost::open(&data).unwrap();
    assert!(host
        .handle(Request {
            id: "register".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::RegisterProject {
                project: p.project.clone()
            }
        })
        .error
        .is_none());
    p.project = host.snapshot().workflows[0].project.clone();
    drop(host);
    let ledger = data.join("workflows/project");
    let cfg: SchedulerConfig =
        serde_json::from_slice(&fs::read(ledger.join("bindings.json")).unwrap()).unwrap();
    let mut store = DurableWorkflow::open(&ledger, cfg.clone()).unwrap();
    for n in 1..=90 {
        p.workflow.version = n;
        store
            .command(
                &format!("proposal-{n}"),
                "chief",
                WorkflowCommand::Propose {
                    plan: p.workflow.clone(),
                },
                CommandContext {
                    execution: Some(ExecutionEvidence::Revision {
                        revision: p.clone(),
                    }),
                    ..Default::default()
                },
                true,
            )
            .unwrap();
    }
    let original = serde_json::to_vec(store.history()).unwrap();
    drop(store);
    let host = RuntimeHost::open(&data).unwrap();
    let owned = data.clone();
    let service = tokio::spawn(async move { cerulean_core::ipc::serve_host(&owned, host).await });
    let tail = page(read(&data, "project", None, None).await);
    assert_eq!(tail.total, 90);
    assert_eq!(tail.entries.len(), 40);
    assert_eq!(tail.entries[0].anchor.sequence, 51);
    assert!(tail.has_older);
    assert!(!tail.has_newer);
    assert!(tail.entries.iter().all(|e| e.model.is_none()));
    let older = page(read(&data, "project", None, Some(tail.entries[0].anchor.clone())).await);
    assert_eq!(older.entries[0].anchor.sequence, 11);
    assert_eq!(older.entries.last().unwrap().anchor.sequence, 50);
    assert!(older.has_newer);
    let first = page(
        read(
            &data,
            "project",
            None,
            Some(older.entries[0].anchor.clone()),
        )
        .await,
    );
    assert_eq!(first.entries.len(), 10);
    assert!(!first.has_older);
    let delta = page(
        read(
            &data,
            "project",
            Some(older.entries.last().unwrap().anchor.clone()),
            None,
        )
        .await,
    );
    assert_eq!(
        serde_json::to_value(&delta.entries).unwrap(),
        serde_json::to_value(&tail.entries).unwrap()
    );
    assert!(page(read(&data, "project", tail.head.clone(), None).await)
        .entries
        .is_empty());
    let mut bad = tail.head.clone().unwrap();
    bad.hash = "0".repeat(64);
    assert!(read(&data, "project", Some(bad), None)
        .await
        .response
        .error
        .is_some());
    assert!(read(&data, "missing", tail.head.clone(), None)
        .await
        .response
        .error
        .is_some());
    assert!(read(&data, "project", tail.head.clone(), tail.head.clone())
        .await
        .response
        .error
        .is_some());
    stop(&data).await;
    service.await.unwrap().unwrap();
    let mut store = DurableWorkflow::open(&ledger, cfg.clone()).unwrap();
    assert_eq!(original, serde_json::to_vec(store.history()).unwrap());
    p.workflow.version = 91;
    store
        .command(
            "proposal-91",
            "chief",
            WorkflowCommand::Propose {
                plan: p.workflow.clone(),
            },
            CommandContext {
                execution: Some(ExecutionEvidence::Revision {
                    revision: p.clone(),
                }),
                ..Default::default()
            },
            true,
        )
        .unwrap();
    store
        .command(
            "approval-92",
            "user",
            WorkflowCommand::Approve {
                digest: store.state().digest.clone().unwrap(),
            },
            CommandContext {
                approval_source: Some("fixture approval".into()),
                execution: Some(ExecutionEvidence::Approval {
                    approval: Approval {
                        worker_account_setup: None,
                        plan_id: p.workflow.id.clone(),
                        revision: p.workflow.version,
                        policy_version: p.policy.version,
                        revision_digest: p.approval_digest().unwrap(),
                        user_id: "user".into(),
                        approved_at: "2026-11-06T00:00:00Z".into(),
                    },
                }),
                ..Default::default()
            },
            true,
        )
        .unwrap();
    drop(store);
    let host = RuntimeHost::open(&data).unwrap();
    let owned = data.clone();
    let service = tokio::spawn(async move { cerulean_core::ipc::serve_host(&owned, host).await });
    let delta = page(read(&data, "project", tail.head, None).await);
    assert_eq!(delta.entries.len(), 2);
    assert_eq!(delta.entries[0].anchor.sequence, 91);
    assert_eq!(delta.entries[0].files, vec!["result.txt"]);
    assert!(
        delta.entries[1].files.is_empty(),
        "Intermediate events must not repeat deliverables"
    );
    stop(&data).await;
    service.await.unwrap().unwrap();
}
