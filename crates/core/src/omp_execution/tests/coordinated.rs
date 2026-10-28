use super::*;
use crate::{
    workflow_access::WorkflowAccess,
    workflow_coordinator::{Scope, WorkflowCoordinator},
};

fn client_scope(plan: &PlanRevision) -> Scope {
    Scope {
        project: "project".into(),
        task: "task".into(),
        attempt: "attempt".into(),
        revision: plan.approval_digest().unwrap(),
    }
}
#[tokio::test]
async fn coordinated_wire_preserves_completion_cancel_unknown_and_responsive_owner() {
    for mode in [
        "completed",
        "early-cancel",
        "lost-ack",
        "provider-error",
        "missing-artifact",
    ] {
        let root = tempfile::tempdir().unwrap();
        let (store, plan) = assigned(root.path(), false);
        let mut owner = WorkflowCoordinator::start(store, root.path().join("ledger")).unwrap();
        let mut store = owner.omp_client(client_scope(&plan)).unwrap();
        let (client, server) = tokio::io::duplex(8192);
        let (r, w) = tokio::io::split(client);
        let mut channel = WorkerTransport::new(BufReader::new(r), w);
        let (cancel, mut rx) = oneshot::channel();
        let (entered, wait) = oneshot::channel();
        let (release, resume) = oneshot::channel();
        let peer = tokio::spawn(peer_gated(
            server,
            root.path().into(),
            mode,
            cancel,
            Some((entered, resume)),
        ));
        channel.connect(Duration::from_secs(3)).await.unwrap();
        let mut observe = |_: &crate::workflow_coordinator::WorkflowClient, _: &str| Ok(());
        let execute = dispatch_inner(
            &mut store,
            &mut channel,
            "task",
            "attempt",
            &mut rx,
            false,
            &mut observe,
        );
        let (result, ()) = tokio::join!(execute, async {
            wait.await.unwrap();
            assert_eq!(
                owner.snapshot().unwrap().state.tasks["task"].state,
                TaskState::Dispatching
            );
            assert!(
                owner.finish(false).is_err(),
                "live client prevents writer transfer"
            );
            release.send(()).unwrap();
        });
        let expected = match mode {
            "completed" => TaskState::Submitted,
            "early-cancel" => TaskState::Cancelled,
            _ => TaskState::Unknown,
        };
        assert_eq!(
            result.is_ok(),
            matches!(mode, "completed" | "early-cancel"),
            "{mode}: {result:?}"
        );
        assert_eq!(store.state().tasks["task"].state, expected);
        assert_eq!(channel.command_attempts().get("prompt"), Some(&1));
        if mode == "early-cancel" {
            assert_eq!(channel.command_attempts().get("abort"), Some(&1));
        }
        channel.close_input().await.unwrap();
        peer.await.unwrap();
        let history = store.history().to_vec();
        drop(store);
        let store = owner.finish(false).unwrap();
        assert_eq!(store.history(), history);
        let store = reopen(store, root.path());
        assert_eq!(store.history(), history);
        assert_eq!(crate::api_execution::prior_requests(&store).unwrap(), 1);
        println!("OMP coordinated {mode}: one prompt, owner responsive during remote wait, original journal and recovery, zero resend");
    }
}
#[test]
fn coordinated_omp_rejects_api_authority_foreign_paths_and_privileged_commands() {
    let root = tempfile::tempdir().unwrap();
    let (store, plan) = assigned(root.path(), false);
    let mut owner = WorkflowCoordinator::start(store, root.path().join("ledger")).unwrap();
    let scope = client_scope(&plan);
    let mut api = owner.client(scope.clone()).unwrap();
    assert!(api.omp_attempt_path("attempt").is_err());
    assert!(api
        .record_omp(
            "core",
            WorkflowCommand::RecoveryUnknown {
                task: "task".into(),
                attempt: "attempt".into()
            },
            CommandContext::default()
        )
        .is_err());
    drop(api);
    let before = owner.snapshot().unwrap().sequence;
    for command in [
        WorkflowCommand::Accept {
            task: "task".into(),
            attempt: "attempt".into(),
        },
        WorkflowCommand::Verify {
            task: "task".into(),
            attempt: "attempt".into(),
            passed: true,
            evidence: vec!["forged".into()],
        },
    ] {
        let mut client = owner.omp_client(scope.clone()).unwrap();
        assert!(client.omp_attempt_path("another-attempt").is_err());
        assert!(client.begin_api_dispatch("task", "attempt").is_err());
        assert!(client
            .command(
                "forged-privilege",
                "chief",
                command,
                CommandContext::default(),
                true
            )
            .unwrap_err()
            .contains("OMP执行线程不能批准、验收"));
    }
    assert_eq!(owner.snapshot().unwrap().sequence, before);
    assert!(owner.finish(false).is_ok());
}
#[test]
fn coordinated_omp_lost_ack_and_interrupted_publication_preserve_one_dispatch() {
    for interrupt in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (store, plan) = assigned(root.path(), false);
        let mut owner = WorkflowCoordinator::start(store, root.path().join("ledger")).unwrap();
        let mut store = owner.omp_client(client_scope(&plan)).unwrap();
        if interrupt {
            owner.interrupt_next_publication();
        } else {
            owner.lose_next_acknowledgement();
        }
        let command = WorkflowCommand::DispatchIntent {
            task: "task".into(),
            attempt: "attempt".into(),
        };
        let context = CommandContext {
            thread_id: Some("session".into()),
            cwd: Some(plan.project.root.clone()),
            ..Default::default()
        };
        assert!(store
            .record_omp("core", command.clone(), context.clone())
            .is_err());
        assert!(store.requires_reopen());
        assert!(store.record_omp("core", command, context).is_err());
        drop(store);
        let store = owner.finish(true).unwrap();
        assert_eq!(store.state().tasks["task"].state, TaskState::Unknown);
        assert_eq!(
            store
                .history()
                .iter()
                .filter(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. }))
                .count(),
            1
        );
        assert_eq!(crate::api_execution::prior_requests(&store).unwrap(), 1);
        let history = store.history().to_vec();
        let store = reopen(store, root.path());
        assert_eq!(store.history(), history);
    }
}
