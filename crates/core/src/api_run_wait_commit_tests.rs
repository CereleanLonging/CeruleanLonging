//! A management Wait and the run outcome are separate durable commits.
use super::*;

fn wait_on_assignment(
    intent: crate::management_execution::DecisionIntent,
) -> Result<DecisionObservation, String> {
    let mut observation = super::manager(intent.clone())?;
    if intent.purpose == Purpose::Assign {
        observation.envelope.decision = ManagementDecision::Wait {
            reason: "Need user input before assigning a worker".into(),
        };
    }
    Ok(observation)
}

#[test]
fn applied_wait_survives_failed_run_outcome_commit_without_new_dispatch() {
    let root = tempfile::tempdir().unwrap();
    let (adapter, target) = api_project_tests::routes().remove(2);
    let mut host = configured(root.path(), adapter, target, "wait-commit-fault", 8);
    approved(&mut host);
    host.management_fixture = Some(wait_on_assignment);
    start(&mut host);
    let run = host.runs.all()[0].clone();
    assert_eq!(host.management.statuses().len(), 1);
    assert!(host.api_jobs.is_empty());

    // The first run authorization is 00000001.json. Obstruct only the next
    // run-control publication, leaving the management journal writable. This
    // reproduces the actual cross-journal commit gap without a model request.
    let obstacle = root.path().join("data/run-control/00000002.pending");
    fs::create_dir(&obstacle).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while host.runner_error.is_none() && Instant::now() < deadline {
        host.poll_verifications();
        std::thread::sleep(Duration::from_millis(4));
    }
    assert!(
        host.runner_error.is_some(),
        "run outcome commit was not reached"
    );
    assert!(
        host.runs.healthy(),
        "pre-publication collision must not corrupt the run ledger"
    );
    assert_eq!(host.runs.all()[0].state, RunState::Active);
    assert!(!obstacle.with_extension("json").exists());
    let statuses = host.management.statuses();
    assert_eq!(statuses.len(), 2);
    let wait = statuses
        .iter()
        .find(|status| status.purpose == Purpose::Assign)
        .unwrap();
    assert!(host.management.unanswered_wait(&wait.operation_id));
    assert!(host.management.application(&wait.operation_id).is_some());
    assert!(host.api_jobs.is_empty());
    assert!(host.api_startups.is_empty());
    let history = host.projects["project"].history().to_vec();
    let management_count = host.management_operation_total();
    let wait_id = wait.operation_id.clone();
    drop(host);

    // Remove only the test-owned obstruction. Neither original journal is
    // edited: the applied Wait and the Active run authorization are replayed.
    fs::remove_dir(&obstacle).unwrap();
    let data = root.path().join("data");
    for reopen in 0..2 {
        let mut recovered = RuntimeHost::open(&data).unwrap();
        recovered.management_fixture = Some(|_| panic!("must not issue a new billed intent"));
        recovered.api_fixture = Some(|_, _| panic!("must not dispatch an API worker"));
        assert_eq!(recovered.runs.all()[0].id, run.id);
        assert_eq!(
            recovered.runs.all()[0].state,
            if reopen == 0 {
                RunState::Active
            } else {
                RunState::Attention
            }
        );
        assert!(recovered.management.unanswered_wait(&wait_id));
        assert_eq!(recovered.management_operation_total(), management_count);
        for _ in 0..5 {
            recovered.poll_verifications();
        }
        assert_eq!(recovered.runs.all()[0].state, RunState::Attention);
        assert_eq!(recovered.management_operation_total(), management_count);
        assert_eq!(recovered.projects["project"].history(), history.as_slice());
        assert!(recovered.api_jobs.is_empty());
        assert!(recovered.api_startups.is_empty());
        assert!(recovered
            .start_project_run("project", &run.revision_digest, &run.setup_digest)
            .is_err());
    }
}
