//! Public native Codex mixed execution with original files, IPC, and independent checks.
use super::*;
use std::sync::mpsc;

fn running(host: &mut RuntimeHost, done: impl Fn(&RuntimeHost) -> bool) {
    spin(host, |h| {
        assert!(
            matches!(
                h.runs.all()[0].state,
                RunState::Active | RunState::Completed
            ),
            "runs={:?} codex={:?} fault={:?}",
            h.runs.all(),
            h.snapshot().codex_jobs,
            h.api_fault
        );
        done(h)
    });
}
fn release(
    host: &RuntimeHost,
    root: &Path,
    task: &str,
    adapter: ProjectWorkerAdapter,
    api: &mut Option<mpsc::Sender<()>>,
) {
    match adapter {
        ProjectWorkerAdapter::Codex => fs::write(
            root.join("bin").join(format!("release-{task}")),
            "original terminal",
        )
        .unwrap(),
        ProjectWorkerAdapter::Omp => fs::write(
            omp_parallel::profile(host).unwrap().join("fixture-release"),
            "original terminal",
        )
        .unwrap(),
        _ => api.take().unwrap().send(()).unwrap(),
    }
}
pub(super) fn all_running(
    host: &RuntimeHost,
    root: &Path,
    order: &[ProjectWorkerAdapter; 3],
) -> bool {
    for (task, adapter) in ["a", "b"].into_iter().zip(order) {
        let expected = if adapter.is_api() {
            TaskState::Dispatching
        } else {
            TaskState::Running
        };
        if host.snapshot().workflows[0].state.tasks[task].state != expected {
            return false;
        }
        if *adapter == ProjectWorkerAdapter::Codex
            && !root.join("bin").join(format!("gated-{task}")).exists()
        {
            return false;
        }
        if *adapter == ProjectWorkerAdapter::Omp
            && !omp_parallel::profile(host).is_some_and(|p| p.join("fixture-gated").exists())
        {
            return false;
        }
    }
    true
}
#[test]
#[ignore = "real Git, native Codex/OMP children and API threads; public automatic parallel host, controlled replies"]
fn automatic_codex_mixed_parallel_completion_and_single_attempt_cancel() {
    use ProjectWorkerAdapter::{Codex, Deepseek, Omp};
    for order in [
        [Codex, Deepseek, Deepseek],
        [Deepseek, Codex, Deepseek],
        [Codex, Omp, Deepseek],
        [Codex, Codex, Deepseek],
    ] {
        for cancel in [false, true] {
            let root = temporary();
            let runtime = omp_parallel::runtime();
            let mut host = mixed::mixed_host_mode(root.path(), runtime.path(), order, true);
            fs::write(root.path().join("bin/gate-codex"), "parallel fixture only").unwrap();
            host.api_fixture = Some(parallel::gated);
            let api_task =
                order[..2]
                    .iter()
                    .position(|a| a.is_api())
                    .map(|n| if n == 0 { "a" } else { "b" });
            let gate = api_task.map(parallel::gate);
            let (api_wait, mut api_release) = match gate {
                Some((w, r)) => (Some(w), Some(r)),
                None => (None, None),
            };
            let plan = planned(&mut host);
            let revision = plan.approval_digest().unwrap();
            host.approve("project", &revision).unwrap();
            let setup = host.project_setups["project"].digest.clone();
            host.start_project_run("project", &revision, &setup)
                .unwrap();
            if let Some(wait) = api_wait {
                running(&mut host, |_| wait.try_recv().is_ok());
            }
            running(&mut host, |h| all_running(h, root.path(), &order));
            assert_eq!(
                host.codex_active.len() + host.omp_active.len() + host.api_active.len(),
                2
            );
            assert_eq!(host.api_writers.len(), 1);
            assert!(!host.projects.contains_key("project"));
            assert_eq!(host.snapshot().workflows.len(), 1);
            assert_eq!(host.management.statuses().len(), 3);
            let codex_task = if order[0] == Codex { "a" } else { "b" };
            let key = host
                .codex_active
                .iter()
                .find(|(_, a)| a.job.task_id == codex_task)
                .unwrap()
                .0
                .clone();
            assert_eq!(
                host.startups
                    .get("project", &key.1)
                    .unwrap()
                    .intent
                    .schema_version,
                2
            );
            assert!(host
                .cancel_codex("project", "foreign", &"a".repeat(64), "wrong attempt")
                .is_err());
            // Delay a stale worker message. Neither the project nor cancellation source may move backward.
            let before = host.api_writers["project"]
                .owner
                .snapshot()
                .unwrap()
                .sequence;
            let (send, recv) = mpsc::channel();
            let original =
                std::mem::replace(&mut host.codex_active.get_mut(&key).unwrap().receiver, recv);
            let mut stale = host.codex_active[&key].snapshot.clone();
            stale.sequence = 0;
            stale.state.tasks.clear();
            send.send(crate::async_codex::Update::Progress(Box::new(stale)))
                .unwrap();
            host.poll_codex_workers();
            host.codex_active.get_mut(&key).unwrap().receiver = original;
            assert_eq!(host.snapshot().workflows.len(), 1);
            assert!(host.snapshot().workflows[0].sequence >= before);
            assert!(host
                .codex_cancellations()
                .iter()
                .any(|r| r.attempt_id == key.1));
            if cancel {
                let review = host
                    .codex_cancellations()
                    .into_iter()
                    .find(|r| r.attempt_id == key.1)
                    .unwrap();
                host.cancel_codex(
                    "project",
                    &key.1,
                    &review.evidence_digest,
                    "cancel only this original native attempt",
                )
                .unwrap();
                spin(&mut host, |h| !h.codex_active.contains_key(&key));
                assert_eq!(host.codex_jobs[&key].state, CodexJobState::Cancelled);
                assert_eq!(host.api_writers.len(), 1);
                let other = if codex_task == "a" { "b" } else { "a" };
                release(
                    &host,
                    root.path(),
                    other,
                    order[usize::from(other == "b")],
                    &mut api_release,
                );
                spin(&mut host, |h| {
                    h.codex_active.is_empty() && h.omp_active.is_empty() && h.api_active.is_empty()
                });
                assert_eq!(host.runs.all()[0].state, RunState::Paused);
                assert_eq!(
                    host.projects["project"].state().tasks[other].state,
                    TaskState::Submitted
                );
                assert_eq!(
                    host.projects["project"].state().tasks["integration"].state,
                    TaskState::Pending
                );
                assert!(host.jobs.is_empty());
            } else {
                release(&host, root.path(), "a", order[0], &mut api_release);
                running(&mut host, |h| {
                    h.snapshot().workflows[0].state.tasks["a"].state == TaskState::Accepted
                });
                assert_eq!(host.api_writers.len(), 1);
                assert!(!host.projects.contains_key("project"));
                release(&host, root.path(), "b", order[1], &mut api_release);
                running(&mut host, |h| h.runs.all()[0].state == RunState::Completed);
                assert!(host.projects["project"]
                    .state()
                    .tasks
                    .values()
                    .all(|s| s.state == TaskState::Accepted && s.verified));
                assert_eq!(host.management.statuses().len(), 7);
                assert_eq!(
                    fs::read_to_string(
                        dependency_host_fixture::task_path(&host, "integration").join("bundle.txt")
                    )
                    .unwrap(),
                    "AB"
                );
            }
            assert!(
                host.api_writers.is_empty() && host.api_fault.is_none(),
                "{:?}",
                host.api_fault
            );
            let history = host.projects["project"].history().to_vec();
            for task in ["a", "b", "integration"] {
                assert_eq!(history.iter().filter(|e|matches!(&e.body.event.command,WorkflowCommand::DispatchIntent{task:t,..} if t==task)).count(),usize::from(task!="integration"||!cancel));
            }
            let management = host.management.statuses().len();
            drop(host);
            let mut reopened = RuntimeHost::open(&root.path().join("data")).unwrap();
            reopened.poll_verifications();
            assert_eq!(reopened.projects["project"].history(), history);
            assert_eq!(reopened.management.statuses().len(), management);
            assert!(
                reopened.codex_active.is_empty()
                    && reopened.omp_active.is_empty()
                    && reopened.api_active.is_empty()
            );
            println!("Codex parallel {:?}/{:?} cancel={cancel}: one writer, original identities, one dispatch each, reopen events 0",order[0],order[1]);
        }
    }
}

#[test]
#[ignore = "real native child and API thread; original assignment ACKs interleaved with other task completion"]
fn codex_shared_assignment_ack_windows_preserve_original_authority_and_no_resend() {
    use crate::async_codex::Update;
    use ProjectWorkerAdapter::{Codex, Deepseek};
    for window in ["prepare", "commit", "lost-commit"] {
        let root = temporary();
        let runtime = omp_parallel::runtime();
        let mut host = mixed::mixed_host_mode(
            root.path(),
            runtime.path(),
            [Deepseek, Codex, Deepseek],
            true,
        );
        fs::write(root.path().join("bin/gate-codex"), "parallel fixture only").unwrap();
        host.api_fixture = Some(parallel::gated);
        let plan = planned(&mut host);
        host.approve("project", &plan.approval_digest().unwrap())
            .unwrap();
        let (waiting, release) = parallel::gate("a");
        omp_parallel::decision(&mut host, "parallel-api", "a", "worker-deepseek");
        host.start_managed_api("parallel-api", "api-attempt")
            .unwrap();
        spin(&mut host, |_| waiting.try_recv().is_ok());
        omp_parallel::decision(&mut host, "parallel-codex", "b", "worker-codex");
        host.start_managed_codex("parallel-codex", "codex-attempt")
            .unwrap();
        let key = ("project".into(), "codex-attempt".into());
        let Update::PrepareAssignment {
            operation,
            attempt,
            snapshot,
            reply,
        } = host.codex_active[&key]
            .receiver
            .recv_timeout(Duration::from_secs(30))
            .unwrap()
        else {
            panic!("original prepare required")
        };
        assert_eq!(
            host.api_writers["project"]
                .owner
                .snapshot()
                .unwrap()
                .state
                .tasks["b"]
                .state,
            TaskState::Pending
        );
        assert!(host
            .api_writers
            .get_mut("project")
            .unwrap()
            .owner
            .finish(false)
            .is_err());
        if window == "prepare" {
            omp_parallel::finish_api_only(&mut host, &release);
        }
        let mut forged = snapshot.clone();
        forged.sequence = 1;
        assert!(host
            .prepare_managed_codex_assignment("project", &operation, *attempt.clone(), *forged)
            .is_err());
        assert!(host.management.application(&operation).is_none());
        host.prepare_managed_codex_assignment("project", &operation, *attempt, *snapshot)
            .unwrap();
        reply.send(Ok(())).unwrap();
        let Update::CommitAssignment {
            operation,
            entry,
            snapshot,
            reply,
        } = host.codex_active[&key]
            .receiver
            .recv_timeout(Duration::from_secs(30))
            .unwrap()
        else {
            panic!("original commit required")
        };
        assert_eq!(
            host.api_writers["project"]
                .owner
                .snapshot()
                .unwrap()
                .state
                .tasks["b"]
                .state,
            TaskState::Assigned
        );
        assert!(!root.path().join("bin/gated-b").exists());
        if window != "prepare" {
            omp_parallel::finish_api_only(&mut host, &release);
        }
        host.commit_managed_codex_assignment("project", &operation, *entry, *snapshot)
            .unwrap();
        if window == "lost-commit" {
            drop(reply);
        } else {
            reply.send(Ok(())).unwrap();
            spin(&mut host, |_| root.path().join("bin/gated-b").exists());
            fs::write(root.path().join("bin/release-b"), "original terminal").unwrap();
        }
        spin(&mut host, |h| h.codex_active.is_empty());
        assert!(
            host.api_writers.is_empty() && host.api_fault.is_none(),
            "{:?}",
            host.api_fault
        );
        assert!(host.management.application("parallel-codex").unwrap().1);
        let history = host.projects["project"].history().to_vec();
        assert_eq!(history.iter().filter(|e|matches!(&e.body.event.command,WorkflowCommand::DispatchIntent{task,..}if task=="b")).count(),usize::from(window!="lost-commit"));
        assert_eq!(
            host.projects["project"].state().tasks["b"].state,
            if window == "lost-commit" {
                TaskState::Assigned
            } else {
                TaskState::Submitted
            }
        );
        drop(host);
        let mut reopened = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(reopened.projects["project"].history(), history);
        assert!(reopened
            .start_managed_codex("parallel-codex", "another")
            .is_err());
        assert_eq!(reopened.projects["project"].history(), history);
        println!("Codex shared ACK {window}: original prefix retained across other completion, no resend, reopen events 0");
    }
}

pub(super) fn advice_manager(
    i: crate::management_execution::DecisionIntent,
) -> Result<DecisionObservation, String> {
    if i.purpose != Purpose::Advise {
        return mixed::mixed_manager(i);
    }
    Ok(DecisionObservation {
        response_id: format!("resp_{}", i.operation_id.replace('-', "_")),
        requested_model: i.model.clone(),
        actual_model: i.model,
        input_tokens: 1,
        output_tokens: 1,
        envelope: DecisionEnvelope {
            state_digest: i.state_digest,
            decision: ManagementDecision::Advise {
                task_id: i.context["supervision"]["taskId"].as_str().unwrap().into(),
                suggestion: "Inspect the failed check within the original approved scope.".into(),
                rationale: "Two observed failures, without inferring a loop.".into(),
            },
        },
    })
}
fn next_advice(
    host: &RuntimeHost,
    key: &(String, String),
) -> crate::supervised_worker::AdviceUpdate {
    loop {
        match host.codex_active[key]
            .receiver
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
        {
            crate::async_codex::Update::Advice(a) => return a,
            crate::async_codex::Update::Progress(_) => {}
            _ => panic!("original advice transaction required"),
        }
    }
}

fn finish_other_codex(host: &mut RuntimeHost, root: &Path, key: &(String, String)) {
    let (hold, receiver) = mpsc::channel();
    let original = std::mem::replace(
        &mut host.codex_active.get_mut(key).unwrap().receiver,
        receiver,
    );
    fs::write(root.join("bin/release-a"), "original other terminal").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while host.codex_active.values().any(|a| a.job.task_id == "a") {
        host.poll_codex_workers();
        assert!(
            Instant::now() < deadline,
            "other Codex did not finish during original ACK"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    host.codex_active.get_mut(key).unwrap().receiver = original;
    drop(hold);
    assert_eq!(host.api_writers.len(), 1);
    assert_eq!(
        host.snapshot().workflows[0].state.tasks["a"].state,
        TaskState::Submitted
    );
}

#[test]
#[ignore = "two real native children; scoped supervision across other-task completion, with original ACK-loss recovery"]
fn codex_shared_supervision_interleaving_preserves_turn_and_never_resends() {
    use crate::supervised_worker::AdviceUpdate;
    use ProjectWorkerAdapter::Codex;
    for window in ["prepare", "commit", "lost-commit"] {
        let root = temporary();
        let runtime = omp_parallel::runtime();
        let mut host = mixed::mixed_host_options(
            root.path(),
            runtime.path(),
            [Codex, Codex, Codex],
            true,
            true,
        );
        for marker in [
            "gate-codex",
            "emit-observations",
            "observations-clean",
            "observations-only-b",
            "await-steer",
        ] {
            fs::write(root.path().join("bin").join(marker), "controlled peer only").unwrap();
        }
        host.management_fixture = Some(advice_manager);
        let plan = planned(&mut host);
        let revision = plan.approval_digest().unwrap();
        host.approve("project", &revision).unwrap();
        let setup = host.project_setups["project"].digest.clone();
        // Hold only the test driver's scheduling loop while original protocol ACKs are inspected.
        host.orchestrating = true;
        host.start_project_run("project", &revision, &setup)
            .unwrap();
        omp_parallel::decision(&mut host, "advice-other", "a", "worker-codex");
        host.start_managed_codex("advice-other", "other-attempt")
            .unwrap();
        spin(&mut host, |_| root.path().join("bin/gated-a").exists());
        omp_parallel::decision(&mut host, "advice-codex", "b", "worker-codex");
        host.start_managed_codex("advice-codex", "codex-attempt")
            .unwrap();
        let key = ("project".into(), "codex-attempt".into());
        spin(&mut host, |h| {
            all_running(h, root.path(), &[Codex, Codex, Codex])
        });
        let run = host.runs.all()[0].clone();
        let deadline = Instant::now() + Duration::from_secs(15);
        let operation = loop {
            host.poll_codex_workers();
            host.poll_supervision_request(&run).unwrap();
            host.poll_management();
            if let Some(s) = host.management.statuses().iter().find(|s| {
                s.purpose == Purpose::Advise
                    && s.state == crate::management_execution::DecisionState::Ready
            }) {
                break s.operation_id.clone();
            }
            assert!(
                Instant::now() < deadline,
                "original observation/management response missing"
            );
            std::thread::sleep(Duration::from_millis(5));
        };
        let bound = supervision_requests::binding(host.management.intent(&operation).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(bound.version, 2);
        assert_eq!(bound.attempt_id, key.1);
        host.wake_ready_advice(&run).unwrap();
        let prepare = next_advice(&host, &key);
        let AdviceUpdate::Prepare { snapshot, .. } = &prepare else {
            panic!("prepare expected")
        };
        let mut forged = snapshot.clone();
        forged.sequence = 1;
        assert!(host
            .validate_shared_advice(&operation, &bound, &forged, None, true)
            .is_err());
        if window == "prepare" {
            finish_other_codex(&mut host, root.path(), &key);
        }
        host.handle_advice_update("project", &key.1, prepare);
        let commit = next_advice(&host, &key);
        assert!(matches!(&commit, AdviceUpdate::Commit { .. }));
        if window != "prepare" {
            finish_other_codex(&mut host, root.path(), &key);
        }
        if window == "lost-commit" {
            let AdviceUpdate::Commit {
                operation,
                entry,
                snapshot,
                reply,
            } = commit
            else {
                unreachable!()
            };
            let (ack, observed) = tokio::sync::oneshot::channel();
            host.handle_advice_update(
                "project",
                &key.1,
                AdviceUpdate::Commit {
                    operation,
                    entry,
                    snapshot,
                    reply: ack,
                },
            );
            assert!(observed.blocking_recv().unwrap().unwrap().is_some());
            drop(reply);
            spin(&mut host, |h| h.codex_active.is_empty());
            assert!(!root.path().join("bin/steer-request.json").exists());
            assert_eq!(
                host.projects["project"].state().tasks["b"].state,
                TaskState::Unknown
            );
        } else {
            host.handle_advice_update("project", &key.1, commit);
            let finish = next_advice(&host, &key);
            assert!(matches!(&finish, AdviceUpdate::Finish { .. }));
            host.handle_advice_update("project", &key.1, finish);
            let delivered = host.delivery_for(&operation).unwrap().unwrap();
            assert!(delivered.acknowledged());
            assert_eq!(delivered.intent.thread_id, bound.thread_id);
            assert_eq!(delivered.intent.turn_id, bound.turn_id);
            fs::remove_file(root.path().join("bin/await-steer")).unwrap();
            fs::remove_file(root.path().join("bin/emit-observations")).unwrap();
            fs::write(root.path().join("bin/release-b"), "original terminal").unwrap();
            host.orchestrating = false;
            running(&mut host, |h| h.runs.all()[0].state == RunState::Completed);
            assert_eq!(host.management.statuses().len(), 8);
            let protocol = fs::read_to_string(root.path().join("bin/protocol.jsonl")).unwrap();
            assert_eq!(
                protocol
                    .lines()
                    .filter(|l| l.contains("turn/steer"))
                    .count(),
                1
            );
        }
        assert!(
            host.api_writers.is_empty() && host.api_fault.is_none(),
            "{:?}",
            host.api_fault
        );
        let history = host.projects["project"].history().to_vec();
        assert_eq!(
            history
                .iter()
                .filter(
                    |e| matches!(&e.body.event.command,WorkflowCommand::Advise{task,..}if task=="b")
                )
                .count(),
            1
        );
        let count = host.management.statuses().len();
        drop(host);
        let mut reopened = RuntimeHost::open(&root.path().join("data")).unwrap();
        reopened.poll_verifications();
        assert_eq!(reopened.projects["project"].history(), history);
        assert_eq!(reopened.management.statuses().len(), count);
        assert!(reopened.codex_active.is_empty());
        println!("Codex shared advice {window}: original turn, other completion retained, reopen events 0, no resend");
    }
}
