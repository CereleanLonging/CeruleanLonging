//! Public automatic OMP/API execution under one approved plan and writer.
use super::*;
use cerulean_domain::omp_jobs::OmpJobState;

pub(super) fn runtime() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("parallel-omp-")
        .tempdir_in(
            std::env::var_os("CERULEAN_OMP_TEST_PARENT").expect("explicit clean runtime parent"),
        )
        .unwrap()
}
pub(super) fn profile(host: &RuntimeHost) -> Option<PathBuf> {
    host.omp_jobs.values().next().map(|job| {
        let id = digest(&(
            host.root.to_str().unwrap(),
            job.project_id.as_str(),
            job.attempt_id.as_str(),
        ))
        .unwrap();
        Path::new(job.worker.omp_runtime_parent.as_ref().unwrap())
            .join("cerulean-omp-profiles")
            .join(id)
            .join("workspace")
    })
}
fn running(host: &mut RuntimeHost, done: impl Fn(&RuntimeHost) -> bool) {
    spin(host, |h| {
        assert!(
            matches!(
                h.runs.all()[0].state,
                RunState::Active | RunState::Completed
            ),
            "{:?}; {:?}; {:?}",
            h.runs.all(),
            h.snapshot().omp_jobs,
            h.api_fault
        );
        done(h)
    });
}

pub(super) fn decision(host: &mut RuntimeHost, operation: &str, task: &str, worker: &str) {
    let context = host
        .management_context("project", "Execute the approved isolated task")
        .unwrap();
    let state_digest = digest(&context).unwrap();
    host.management
        .begin(crate::management_execution::DecisionIntent {
            operation_id: operation.into(),
            project_id: "project".into(),
            purpose: Purpose::Assign,
            model: "fixture".into(),
            state_digest: state_digest.clone(),
            context,
            created_at: chrono::Utc::now().to_rfc3339(),
        })
        .unwrap();
    host.management
        .finish(
            operation,
            Ok(DecisionObservation {
                response_id: format!("resp_{}", operation.replace('-', "_")),
                requested_model: "fixture".into(),
                actual_model: "fixture".into(),
                input_tokens: 1,
                output_tokens: 1,
                envelope: DecisionEnvelope {
                    state_digest,
                    decision: ManagementDecision::Assign {
                        task_id: task.into(),
                        worker_id: worker.into(),
                        rationale: "approved isolated task".into(),
                    },
                },
            }),
        )
        .unwrap();
}
fn next_assignment(host: &RuntimeHost) -> crate::managed_assignment::Update {
    let update = host.omp_active[&("project".into(), "omp-attempt".into())]
        .receiver
        .recv_timeout(Duration::from_secs(30))
        .unwrap();
    let crate::async_omp::Update::Assignment(update) = update else {
        panic!("original assignment expected")
    };
    update
}
pub(super) fn finish_api_only(host: &mut RuntimeHost, release: &std::sync::mpsc::Sender<()>) {
    release.send(()).unwrap();
    let limit = Instant::now() + Duration::from_secs(30);
    while !host.api_active.is_empty() {
        host.poll_api();
        assert!(
            Instant::now() < limit,
            "API did not finish while OMP waited for management ACK"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(host.api_writers.len(), 1);
    assert!(!host.projects.contains_key("project"));
}

#[test]
#[ignore = "real Git, controlled OMP process and API thread; interleaved management ACK and recovery"]
fn omp_shared_assignment_ack_windows_validate_original_prefix_and_do_not_resend() {
    use crate::managed_assignment::Update::{Commit, Prepare};
    use ProjectWorkerAdapter::{Deepseek, Omp};
    for window in ["prepare", "commit", "lost-commit"] {
        let root = temporary();
        let runtime = runtime();
        let mut host =
            mixed::mixed_host_mode(root.path(), runtime.path(), [Deepseek, Omp, Deepseek], true);
        host.api_fixture = Some(parallel::gated);
        let plan = planned(&mut host);
        host.approve("project", &plan.approval_digest().unwrap())
            .unwrap();
        decision(&mut host, "parallel-api", "a", "worker-deepseek");
        let (waiting, release) = parallel::gate("a");
        host.start_managed_api("parallel-api", "api-attempt")
            .unwrap();
        spin(&mut host, |_| waiting.try_recv().is_ok());
        decision(&mut host, "parallel-omp", "b", "worker-omp");
        host.start_managed_omp("parallel-omp", "omp-attempt")
            .unwrap();
        let Prepare {
            operation,
            attempt,
            snapshot,
            reply,
        } = next_assignment(&host)
        else {
            panic!("prepare expected")
        };
        let before = host.api_writers["project"].owner.snapshot().unwrap();
        assert_eq!(before.state.tasks["b"].state, TaskState::Pending);
        assert!(host
            .api_writers
            .get_mut("project")
            .unwrap()
            .owner
            .finish(false)
            .is_err());
        assert!(!profile(&host).unwrap().join("fixture-gated").exists());
        if window == "prepare" {
            finish_api_only(&mut host, &release);
        }
        let mut forged = snapshot.clone();
        forged.sequence = 1;
        assert!(host
            .prepare_managed_assignment("project", &operation, *attempt.clone(), *forged, "omp")
            .is_err());
        assert!(host.management.application(&operation).is_none());
        host.prepare_managed_assignment("project", &operation, *attempt, *snapshot, "omp")
            .unwrap();
        reply.send(Ok(())).unwrap();
        let Commit {
            operation,
            entry,
            snapshot,
            reply,
        } = next_assignment(&host)
        else {
            panic!("commit expected")
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
        assert!(!profile(&host).unwrap().join("fixture-gated").exists());
        if window != "prepare" {
            finish_api_only(&mut host, &release);
        }
        host.commit_managed_assignment("project", &operation, *entry, *snapshot, "omp")
            .unwrap();
        if window == "lost-commit" {
            drop(reply);
        } else {
            reply.send(Ok(())).unwrap();
            spin(&mut host, |h| {
                profile(h).unwrap().join("fixture-gated").exists()
            });
            fs::write(
                profile(&host).unwrap().join("fixture-release"),
                "release original result",
            )
            .unwrap();
        }
        spin(&mut host, |h| h.omp_active.is_empty());
        assert!(host.api_writers.is_empty());
        assert!(host.api_fault.is_none(), "{:?}", host.api_fault);
        assert!(host.management.application("parallel-omp").unwrap().1);
        let history = host.projects["project"].history().to_vec();
        assert_eq!(history.iter().filter(|e| matches!(&e.body.event.command, WorkflowCommand::DispatchIntent {task,..} if task=="b")).count(), usize::from(window != "lost-commit"));
        assert_eq!(
            host.projects["project"].state().tasks["b"].state,
            if window == "lost-commit" {
                TaskState::Assigned
            } else {
                TaskState::Submitted
            }
        );
        drop(host);
        let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(host.projects["project"].history(), history);
        assert!(host
            .start_managed_omp("parallel-omp", "another-attempt")
            .is_err());
        assert_eq!(host.projects["project"].history(), history);
        println!("OMP shared ACK {window}: interleaved API completion, unchanged original target, forged snapshot rejected, no resend, reopen events 0");
    }
}
#[test]
#[ignore = "real Git, production OMP child and API thread, original verification; controlled model replies"]
fn automatic_omp_api_parallel_completion_and_attempt_local_cancel_preserve_owner() {
    use ProjectWorkerAdapter::{Deepseek, Omp};
    for omp_first in [true, false] {
        for cancel in [false, true] {
            let root = temporary();
            let runtime = runtime();
            let order = if omp_first {
                [Omp, Deepseek, Deepseek]
            } else {
                [Deepseek, Omp, Deepseek]
            };
            let mut host = mixed::mixed_host_mode(root.path(), runtime.path(), order, true);
            host.api_fixture = Some(parallel::gated);
            let plan = planned(&mut host);
            host.approve("project", &plan.approval_digest().unwrap())
                .unwrap();
            let setup = host.project_setups["project"].digest.clone();
            let api_task = if omp_first { "b" } else { "a" };
            let omp_task = if omp_first { "a" } else { "b" };
            let (api_wait, api_release) = parallel::gate(api_task);
            host.start_project_run("project", &plan.approval_digest().unwrap(), &setup)
                .unwrap();
            running(&mut host, |_| api_wait.try_recv().is_ok());
            running(&mut host, |h| {
                profile(h).is_some_and(|p| p.join("fixture-gated").exists())
            });
            assert_eq!(host.api_active.len(), 1);
            assert_eq!(host.omp_active.len(), 1);
            assert_eq!(host.api_writers.len(), 1);
            assert!(!host.projects.contains_key("project"));
            assert_eq!(host.snapshot().workflows.len(), 1);
            assert_eq!(host.management.statuses().len(), 3);
            let omp_job = host.omp_jobs.values().next().unwrap().clone();
            let omp_key = ("project".into(), omp_job.attempt_id.clone());
            let api_key = host.api_active.keys().next().unwrap().clone();
            assert_eq!(
                host.omp_startups
                    .get("project", &omp_job.attempt_id)
                    .unwrap()
                    .intent
                    .schema_version,
                2
            );
            // An old worker message is not an authority to roll back the project.
            let before = host.api_writers["project"]
                .owner
                .snapshot()
                .unwrap()
                .sequence;
            let (sender, receiver) = std::sync::mpsc::channel();
            let old = std::mem::replace(
                &mut host.omp_active.get_mut(&omp_key).unwrap().receiver,
                receiver,
            );
            let mut stale = host.omp_active[&omp_key].snapshot.clone();
            stale.sequence = 1;
            sender
                .send(crate::async_omp::Update::Progress(Box::new(stale)))
                .unwrap();
            host.poll_omp();
            host.omp_active.get_mut(&omp_key).unwrap().receiver = old;
            drop(sender);
            assert_eq!(host.snapshot().workflows[0].sequence, before);
            assert!(host
                .cancel_omp("project", "foreign-attempt", "wrong target")
                .is_err());
            assert!(host.api_active[&api_key].cancel.is_some());
            assert!(!host.worker_slot_available());
            let marker = profile(&host).unwrap();
            if cancel {
                host.cancel_omp(
                    "project",
                    &omp_job.attempt_id,
                    "cancel only the OMP attempt",
                )
                .unwrap();
                spin(&mut host, |h| h.omp_active.is_empty());
                assert_eq!(host.omp_jobs[&omp_key].state, OmpJobState::Cancelled);
                assert_eq!(host.api_active.len(), 1);
                assert_eq!(host.api_writers.len(), 1);
                assert!(host.api_active[&api_key].cancel.is_some());
                api_release.send(()).unwrap();
                spin(&mut host, |h| h.api_active.is_empty());
                assert_eq!(host.runs.all()[0].state, RunState::Paused);
                assert_eq!(
                    host.projects["project"].state().tasks[api_task].state,
                    TaskState::Submitted
                );
                assert_eq!(
                    host.projects["project"].state().tasks[omp_task].state,
                    TaskState::Cancelled
                );
                assert_eq!(
                    host.projects["project"].state().tasks["integration"].state,
                    TaskState::Pending
                );
                assert!(host.jobs.is_empty());
            } else {
                if omp_first {
                    fs::write(marker.join("fixture-release"), "release one terminal").unwrap();
                } else {
                    api_release.send(()).unwrap();
                }
                running(&mut host, |h| {
                    h.snapshot().workflows[0].state.tasks["a"].state == TaskState::Accepted
                });
                assert_eq!(host.api_writers.len(), 1);
                assert!(!host.projects.contains_key("project"));
                assert_eq!(host.api_active.len() + host.omp_active.len(), 1);
                if omp_first {
                    api_release.send(()).unwrap();
                } else {
                    fs::write(marker.join("fixture-release"), "release one terminal").unwrap();
                }
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
            assert!(host.api_writers.is_empty());
            assert!(host.api_fault.is_none(), "{:?}", host.api_fault);
            let history = host.projects["project"].history().to_vec();
            for task in ["a", "b", "integration"] {
                let expected = usize::from(task != "integration" || !cancel);
                assert_eq!(history.iter().filter(|e| matches!(&e.body.event.command, WorkflowCommand::DispatchIntent {task: t,..} if t == task)).count(), expected);
            }
            let management_count = host.management.statuses().len();
            drop(host);
            let mut reopened = RuntimeHost::open(&root.path().join("data")).unwrap();
            reopened.poll_verifications();
            assert_eq!(reopened.projects["project"].history(), history);
            assert_eq!(reopened.management.statuses().len(), management_count);
            assert!(reopened.api_active.is_empty() && reopened.omp_active.is_empty());
            println!("automatic OMP/API omp_first={omp_first} cancel={cancel}: one writer, two actual workers, original ACKs, one dispatch each, reopen events 0");
        }
    }
}
