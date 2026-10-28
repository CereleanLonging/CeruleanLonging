//! Real journal/API/Git/verifier lifecycle. Chief replies remain controlled.
use super::*;
use crate::{
    management_execution::{ApplicationTarget, DecisionIntent, DecisionState},
    workflow_access::WorkflowAccess,
};

fn target(attempt: &str) -> ApplicationTarget {
    ApplicationTarget::Workflow {
        principal: "chief".into(),
        command: WorkflowCommand::Accept {
            task: "a".into(),
            attempt: attempt.into(),
        },
        context: Box::default(),
    }
}
#[test]
#[ignore = "real Git and verifier; saved chief response, interleaved events, lost ACK, stale response and worker authority"]
fn shared_chief_application_reconciles_original_event_and_rejects_stale_authority() {
    for mode in [
        "interleaved",
        "lost-ack",
        "lost-ack-reopen",
        "missing-event",
    ] {
        let root = temporary();
        let mut host = configured(root.path(), mode);
        host.api_fixture = Some(parallel::gated);
        let plan = planned(&mut host);
        host.approve("project", &plan.approval_digest().unwrap())
            .unwrap();
        let setup = host.project_setups["project"].digest.clone();
        let (a_wait, a_release) = parallel::gate("a");
        let (b_wait, b_release) = parallel::gate("b");
        host.start_project_run("project", &plan.approval_digest().unwrap(), &setup)
            .unwrap();
        spin(&mut host, |_| a_wait.try_recv().is_ok());
        spin(&mut host, |_| b_wait.try_recv().is_ok());
        // Retain the actual Active run authority while this fixture manually
        // controls chief acceptance. User pause now revokes API tool-boundary
        // permission; it cannot be used merely to suppress the scheduler.
        // Reuse the existing shared-supervision fixture's scheduling guard.
        host.orchestrating = true;
        assert_eq!(host.runs.all()[0].state, RunState::Active);
        a_release.send(()).unwrap();
        spin(&mut host, |h| h.api_active.len() == 1);
        let snapshot = host.api_writers["project"].owner.snapshot().unwrap();
        let attempt = snapshot.state.tasks["a"].attempt.clone().unwrap();
        assert_eq!(snapshot.state.tasks["a"].state, TaskState::Submitted);
        let pins = plan
            .task("a")
            .unwrap()
            .checks
            .iter()
            .map(crate::verifier::PinnedCheck::from_approved)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        host.start_verification("project", "shared-chief-check", "a", &attempt, pins)
            .unwrap();
        spin(&mut host, |h| h.active.is_empty());
        assert_eq!(
            host.jobs[&("project".into(), "shared-chief-check".into())].state,
            VerificationState::Succeeded
        );
        let context = host
            .management_context("project", "Accept the verified original A")
            .unwrap();
        let original: ExecutionSnapshot =
            serde_json::from_value(context["workflow"].clone()).unwrap();
        {
            let operation = "shared-chief";
            let intent = DecisionIntent {
                operation_id: operation.into(),
                project_id: "project".into(),
                purpose: Purpose::Accept,
                model: "gpt-4.1".into(),
                state_digest: digest(&context).unwrap(),
                context: context.clone(),
                created_at: chrono::Utc::now().to_rfc3339(),
            };
            host.management.begin(intent.clone()).unwrap();
            host.management.finish(operation, manager(intent)).unwrap();
        }
        assert!(host.management_decision("shared-chief").is_ok());
        let mut forged = original.clone();
        forged.state.tasks.get_mut("a").unwrap().verified = false;
        let reader = host.api_read("project", "a", &attempt).unwrap();
        assert!(crate::acceptance_scope::validate(&*reader, &forged, "a", &attempt).is_err());
        drop(reader);
        let mut worker = host.api_writers["project"]
            .owner
            .client(crate::workflow_coordinator::Scope {
                project: "project".into(),
                task: "a".into(),
                attempt: attempt.clone(),
                revision: plan.approval_digest().unwrap(),
            })
            .unwrap();
        assert!(worker
            .command(
                "worker-forged-accept",
                "chief",
                WorkflowCommand::Accept {
                    task: "a".into(),
                    attempt: attempt.clone()
                },
                CommandContext::default(),
                true
            )
            .is_err());
        drop(worker);
        if mode == "missing-event" {
            let digest = host
                .management
                .intent("shared-chief")
                .unwrap()
                .state_digest
                .clone();
            host.management
                .prepare_application("shared-chief", target(&attempt), &digest)
                .unwrap();
            let before = host.api_writers["project"]
                .owner
                .snapshot()
                .unwrap()
                .sequence;
            for _ in 0..2 {
                assert!(host
                    .apply_management_decision_inner("shared-chief", target(&attempt))
                    .is_err());
            }
            assert_eq!(
                host.api_writers["project"]
                    .owner
                    .snapshot()
                    .unwrap()
                    .sequence,
                before
            );
            b_release.send(()).unwrap();
            spin(&mut host, |h| h.api_active.is_empty());
            let history = host.projects["project"].history().to_vec();
            assert!(
                host.apply_management_decision_inner("shared-chief", target(&attempt))
                    .is_err(),
                "returning the shared writer must not enable a second Accept send"
            );
            assert_eq!(host.projects["project"].history(), history);
            assert!(!history
                .iter()
                .any(|e| matches!(e.body.event.command, WorkflowCommand::Accept { .. })));
            assert_eq!(
                history
                    .iter()
                    .filter(|e| matches!(
                        e.body.event.command,
                        WorkflowCommand::DispatchIntent { .. }
                    ))
                    .count(),
                2
            );
            drop(host);
            let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
            assert!(
                host.apply_management_decision_inner("shared-chief", target(&attempt))
                    .is_err(),
                "reopen must not enable a second Accept send"
            );
            assert_eq!(host.projects["project"].history(), history);
            assert_eq!(
                host.management
                    .statuses()
                    .iter()
                    .find(|s| s.operation_id == "shared-chief")
                    .unwrap()
                    .state,
                DecisionState::Applying
            );
            println!("shared chief missing-event: zero Accept, original application remains unresolved, zero resend on retry/reopen");
            continue;
        }
        if mode == "interleaved" {
            b_release.send(()).unwrap();
            spin(&mut host, |h| h.api_active.is_empty());
            assert!(host.projects["project"].history().len() > original.sequence as usize);
            assert!(
                host.management_decision("shared-chief").is_ok(),
                "B's submit does not stale A's authority"
            );
            host.apply_management_decision_inner("shared-chief", target(&attempt))
                .unwrap();
        } else {
            host.api_writers["project"]
                .owner
                .lose_next_acknowledgement();
            assert!(host
                .apply_management_decision_inner("shared-chief", target(&attempt))
                .is_err());
            assert_eq!(
                host.management
                    .statuses()
                    .iter()
                    .find(|s| s.operation_id == "shared-chief")
                    .unwrap()
                    .state,
                DecisionState::Applying
            );
            let snapshot = host.api_writers["project"].owner.snapshot().unwrap();
            assert_eq!(snapshot.state.tasks["a"].state, TaskState::Accepted);
            assert_eq!(
                host.snapshot().workflows[0].state.tasks["a"].state,
                TaskState::Accepted,
                "lost acknowledgement must not leave a stale workflow projection"
            );
            assert_eq!(host.api_active.len(), 1);
            if mode == "lost-ack" {
                host.apply_management_decision_inner("shared-chief", target(&attempt))
                    .unwrap();
                host.apply_management_decision_inner("shared-chief", target(&attempt))
                    .unwrap();
                assert_eq!(
                    host.api_writers["project"]
                        .owner
                        .snapshot()
                        .unwrap()
                        .sequence,
                    snapshot.sequence,
                    "reconcile and idempotent read must not resubmit Accept"
                );
            }
            assert!(host.api_writers["project"]
                .owner
                .accept(
                    original.clone(),
                    "stale-writer-command".into(),
                    "chief".into(),
                    "a".into(),
                    attempt.clone(),
                    CommandContext::default()
                )
                .is_err());
            b_release.send(()).unwrap();
            spin(&mut host, |h| h.api_active.is_empty());
        }
        let history = host.projects["project"].history().to_vec();
        assert_eq!(
            history
                .iter()
                .filter(|e| matches!(e.body.event.command, WorkflowCommand::Accept { .. }))
                .count(),
            1
        );
        assert_eq!(
            history
                .iter()
                .filter(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. }))
                .count(),
            2
        );
        assert_eq!(host.management.statuses().len(), 4);
        drop(host);
        let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(host.projects["project"].history(), history);
        assert_eq!(
            host.management
                .statuses()
                .iter()
                .find(|s| s.operation_id == "shared-chief")
                .unwrap()
                .state,
            DecisionState::Applied
        );
        let stale = DecisionIntent {
            operation_id: "stale-chief".into(),
            project_id: "project".into(),
            purpose: Purpose::Accept,
            model: "gpt-4.1".into(),
            state_digest: digest(&context).unwrap(),
            context: context.clone(),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        host.management.begin(stale.clone()).unwrap();
        host.management
            .finish("stale-chief", manager(stale))
            .unwrap();
        assert!(host.management_decision("stale-chief").is_err());
        assert!(host
            .apply_management_decision_inner("stale-chief", target(&attempt))
            .is_err());
        assert!(host.management.application("stale-chief").is_none());
        assert_eq!(host.projects["project"].history(), history);
        assert_eq!(host.management.statuses().len(), 5);
        println!("shared chief {mode}: one Accept, two original dispatches, immutable authority, zero reexecution on reconcile/reopen");
    }
}
