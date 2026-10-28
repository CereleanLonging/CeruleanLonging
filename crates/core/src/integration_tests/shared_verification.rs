use super::*;
use crate::workflow_access::WorkflowAccess;

fn gated_manager(
    i: crate::management_execution::DecisionIntent,
) -> Result<DecisionObservation, String> {
    let mut reply = if i.context["executionSetup"]["setup"]["requirement"] == "refill-during-verify"
    {
        parallel::three_branch_manager(i)?
    } else {
        manager(i)?
    };
    if let ManagementDecision::Plan { tasks, .. } = &mut reply.envelope.decision {
        for task in tasks.iter_mut().filter(|t| t.id != "integration") {
            task.validator_id = "gated-tests".into();
        }
    }
    Ok(reply)
}
#[test]
#[ignore = "real Git, API threads and owned verification process; completion, cancel, lost ACK and changed artifact"]
fn shared_verification_preserves_owner_and_never_reexecutes_checks() {
    for mode in [
        "last-api-exits",
        "refill-during-verify",
        "cancel-verifier",
        "lose-verify-ack",
        "changed-artifact",
    ] {
        let root = temporary();
        let control = root.path().join("verify-control");
        fs::create_dir(&control).unwrap();
        let script = fs::canonicalize(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/fixtures/shared-verification-check.cjs"),
        )
        .unwrap();
        let node = std::env::var("CERULEAN_TEST_NODE")
            .expect("explicit local Node verifier path required");
        let mut host = configured_using(root.path(), mode, |h, d| {
            h.register_validator(
                "gated-tests".into(),
                "Owned verification gate".into(),
                node.clone(),
                vec![script.to_string_lossy().into()],
                Default::default(),
            )
            .unwrap();
            d.validator_id = "gated-tests".into();
            d.check_args = vec![
                script.to_string_lossy().into(),
                control.to_string_lossy().into(),
            ];
        });
        host.management_fixture = Some(gated_manager);
        host.api_fixture = Some(parallel::gated);
        let plan = planned(&mut host);
        host.approve("project", &plan.approval_digest().unwrap())
            .unwrap();
        let setup = host.project_setups["project"].digest.clone();
        let (a_wait, a_release) = parallel::gate("a");
        let (b_wait, b_release) = parallel::gate("b");
        let extra = (mode == "refill-during-verify").then(|| parallel::gate("c"));
        host.start_project_run("project", &plan.approval_digest().unwrap(), &setup)
            .unwrap();
        spin(&mut host, |_| a_wait.try_recv().is_ok());
        spin(&mut host, |_| b_wait.try_recv().is_ok());
        a_release.send(()).unwrap();
        spin(&mut host, |_| control.join("entered").exists());
        if let Some((wait, _)) = &extra {
            spin(&mut host, |_| wait.try_recv().is_ok());
        }
        assert!(host.active["project"].shared);
        assert_eq!(host.api_active.len(), if extra.is_some() { 2 } else { 1 });
        assert_eq!(host.api_writers.len(), 1);
        assert!(!host.projects.contains_key("project"));
        let view = host.snapshot();
        assert_eq!(view.workflows.len(), 1);
        assert!(!view.workflows[0].state.tasks["a"].verified);
        let job = host.active["project"].job.clone();
        let read = Command::ReadVerificationDetail {
            project_id: "project".into(),
            operation_id: job.operation_id.clone(),
        };
        let sequence = host.api_writers["project"]
            .owner
            .snapshot()
            .unwrap()
            .sequence;
        for _ in 0..3 {
            let detail = host
                .management_read(&read)
                .unwrap()
                .unwrap()
                .verification_detail
                .unwrap();
            assert!(!detail.ledger_recorded);
            assert_eq!(detail.attempt_id, job.attempt_id);
        }
        assert_eq!(
            host.api_writers["project"]
                .owner
                .snapshot()
                .unwrap()
                .sequence,
            sequence,
            "detail reads never poll or mutate"
        );
        let b_attempt = view.workflows[0].state.tasks["b"].attempt.clone().unwrap();
        let mut worker = host.api_writers["project"]
            .owner
            .client(crate::workflow_coordinator::Scope {
                project: "project".into(),
                task: "b".into(),
                attempt: b_attempt.clone(),
                revision: plan.approval_digest().unwrap(),
            })
            .unwrap();
        let before = worker.history().len();
        assert!(worker
            .command(
                "worker-cannot-verify",
                "core",
                WorkflowCommand::Verify {
                    task: "b".into(),
                    attempt: b_attempt,
                    passed: false,
                    evidence: vec!["forged".into()]
                },
                CommandContext::default(),
                true
            )
            .unwrap_err()
            .contains("不能批准、验收"));
        drop(worker);
        assert_eq!(
            host.api_writers["project"]
                .owner
                .snapshot()
                .unwrap()
                .sequence as usize,
            before
        );
        assert!(host
            .start_verification(
                "project",
                "duplicate-verifier",
                "a",
                &job.attempt_id,
                vec![]
            )
            .is_err());
        assert_eq!(host.jobs.len(), 1);
        match mode {
            "last-api-exits" | "refill-during-verify" => {
                let run = host.runs.all()[0].id.clone();
                host.pause_project_run(&run).unwrap();
                b_release.send(()).unwrap();
                if let Some((_, release)) = extra {
                    release.send(()).unwrap();
                }
                spin(&mut host, |h| h.api_active.is_empty());
                assert!(
                    host.active.contains_key("project") && host.api_writers.contains_key("project")
                );
                assert!(!host.projects.contains_key("project"));
                assert!(host
                    .api_writers
                    .get_mut("project")
                    .unwrap()
                    .owner
                    .finish(false)
                    .is_err());
                fs::write(control.join("release"), b"continue").unwrap();
                spin(&mut host, |h| h.active.is_empty());
                assert!(host.api_writers.is_empty());
                assert_eq!(
                    host.jobs[&("project".into(), job.operation_id.clone())].state,
                    VerificationState::Succeeded
                );
                assert!(host.projects["project"].state().tasks["a"].verified);
            }
            _ => {
                if mode == "cancel-verifier" {
                    host.cancel_verification("project", &job.operation_id)
                        .unwrap();
                } else {
                    if mode == "lose-verify-ack" {
                        host.api_writers["project"]
                            .owner
                            .lose_next_acknowledgement();
                    } else {
                        fs::write(
                            Path::new(
                                &host.api_startups[&("project".into(), job.attempt_id.clone())]
                                    .attempt
                                    .binding
                                    .workspace,
                            )
                            .join("result.txt"),
                            b"changed after snapshot",
                        )
                        .unwrap();
                    }
                    fs::write(control.join("release"), b"continue").unwrap();
                }
                spin(&mut host, |h| h.active.is_empty());
                assert_eq!(
                    host.api_active.len(),
                    1,
                    "stopping verification must not stop B"
                );
                assert_eq!(
                    host.jobs[&("project".into(), job.operation_id.clone())].state,
                    if mode == "lose-verify-ack" {
                        VerificationState::Unknown
                    } else {
                        VerificationState::Failed
                    }
                );
                assert_eq!(host.runs.all()[0].state, RunState::Attention);
                let detail = host
                    .management_read(&read)
                    .unwrap()
                    .unwrap()
                    .verification_detail
                    .unwrap();
                assert_eq!(
                    detail.ledger_recorded,
                    mode == "lose-verify-ack",
                    "completed verification detail remains readable while B owns the writer"
                );
                if mode == "lose-verify-ack" {
                    assert_eq!(detail.state, VerificationState::Unknown);
                    assert_eq!(
                        host.jobs[&("project".into(), job.operation_id.clone())].state,
                        VerificationState::Unknown,
                        "read cannot reconcile the live status"
                    );
                    let reader = host.api_read("project", "a", &job.attempt_id).unwrap();
                    let basis =
                        crate::verification_detail::basis_from_history(reader.history(), &job)
                            .unwrap();
                    let mut forged = job.clone();
                    forged.state = VerificationState::Failed;
                    assert!(
                        crate::verification_detail::read(&host.root, &forged, &basis)
                            .unwrap_err()
                            .contains("验收状态与账本不同"),
                        "contradictory terminal states remain rejected"
                    );
                }
                assert_eq!(host.management.statuses().len(), 3);
                assert!(!host
                    .api_startups
                    .values()
                    .any(|s| s.attempt.task_id == "integration"));
                b_release.send(()).unwrap();
                spin(&mut host, |h| h.api_active.is_empty());
            }
        }
        assert_eq!(
            host.projects["project"].state().tasks["b"].state,
            TaskState::Submitted
        );
        assert_eq!(
            fs::read_dir(root.path().join("data/workflows/project/verification"))
                .unwrap()
                .count(),
            1
        );
        let history = host.projects["project"].history().to_vec();
        assert_eq!(
            history
                .iter()
                .filter(|e| matches!(e.body.event.command, WorkflowCommand::Verify { .. }))
                .count(),
            usize::from(matches!(
                mode,
                "last-api-exits" | "refill-during-verify" | "lose-verify-ack"
            ))
        );
        assert_eq!(
            history
                .iter()
                .filter(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. }))
                .count(),
            if mode == "refill-during-verify" { 3 } else { 2 }
        );
        drop(host);
        let host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(host.projects["project"].history(), history);
        if mode == "lose-verify-ack" {
            assert_eq!(
                host.jobs[&("project".into(), job.operation_id.clone())].state,
                VerificationState::Succeeded
            );
        }
        assert_eq!(
            fs::read_dir(root.path().join("data/workflows/project/verification"))
                .unwrap()
                .count(),
            1
        );
        println!("shared verification {mode}: one writer, one verifier process, API independent, no repeated check or dispatch, reopen events 0");
    }
}
