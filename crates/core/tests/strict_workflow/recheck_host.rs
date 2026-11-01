use super::*;
use std::time::{Duration, Instant};

fn setup(checker: &str) -> (tempfile::TempDir, RuntimeHost, RecheckApproval) {
    let (root, store, a) = runner::corrected(checker);
    drop(store);
    let host = RuntimeHost::open(&root.path().join("data")).unwrap();
    (root, host, a)
}
fn approve(host: &mut RuntimeHost, a: &RecheckApproval) {
    host.approve_verification_recheck(&a.review, "approve-recheck", &a.operation_id)
        .unwrap();
}
fn state(host: &RuntimeHost, a: &RecheckApproval) -> VerificationState {
    host.snapshot()
        .verification_jobs
        .into_iter()
        .find(|j| j.operation_id == a.operation_id)
        .unwrap()
        .state
}
fn finish(host: &mut RuntimeHost, a: &RecheckApproval) {
    let deadline = Instant::now() + Duration::from_secs(45);
    while matches!(
        state(host, a),
        VerificationState::Running | VerificationState::Cancelling
    ) {
        assert!(Instant::now() < deadline, "original recheck did not finish");
        host.poll_verifications();
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn evidence(root: &Path, a: &RecheckApproval) -> std::path::PathBuf {
    root.join("data/workflows/project/verification")
        .join(&a.operation_id)
}
fn claim(root: &Path, a: &RecheckApproval) -> std::path::PathBuf {
    root.join("data/verification-recheck-jobs/project")
        .join(&a.operation_id)
}
fn request(command: Command) -> Request {
    Request {
        id: "recheck-host-observation".into(),
        protocol_version: PROTOCOL_VERSION,
        command,
    }
}

#[test]
fn host_recheck_persists_separate_job_and_keeps_original_failure_after_reopen() {
    let (root, mut host, a) = setup("corrected_checker");
    assert!(host
        .start_verification_recheck("project", &a.operation_id)
        .is_err());
    approve(&mut host, &a);
    assert!(host
        .start_verification_recheck("project", "different-operation")
        .is_err());
    host.start_verification_recheck("project", &a.operation_id)
        .unwrap();
    assert!(claim(root.path(), &a).join("intent.json").exists());
    let launch: serde_json::Value =
        serde_json::from_slice(&fs::read(claim(root.path(), &a).join("intent.json")).unwrap())
            .unwrap();
    assert_eq!(launch["schemaVersion"], 2);
    assert_eq!(launch["processScope"].as_str().unwrap().len(), 64);
    assert!(!root
        .path()
        .join("data/verification-jobs/project")
        .join(format!("{}.json", a.operation_id))
        .exists());
    assert!(host
        .start_verification_recheck("project", &a.operation_id)
        .is_err());
    let started = Instant::now();
    for _ in 0..10 {
        let _ = host.snapshot();
    }
    assert!(started.elapsed() < Duration::from_secs(2));
    finish(&mut host, &a);
    assert_eq!(state(&host, &a), VerificationState::Succeeded);
    let old = host
        .snapshot()
        .verification_jobs
        .into_iter()
        .find(|j| j.operation_id == a.review.original_operation_id)
        .unwrap();
    assert_eq!(old.state, VerificationState::Failed);
    let details = host.handle(request(Command::ReadVerificationDetail {
        project_id: "project".into(),
        operation_id: a.operation_id.clone(),
    }));
    assert!(details.error.is_none(), "{:?}", details.error);
    let detail = details.result.unwrap().verification_detail.unwrap();
    assert!(detail.ledger_recorded);
    assert_eq!(detail.stages.len(), 2);
    let count = fs::read(evidence(root.path(), &a).join("check-temp/validator-count")).unwrap();
    assert_eq!(count, b"one");
    drop(host);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(state(&host, &a), VerificationState::Succeeded);
    host.recover_verification_recheck("project", &a.operation_id)
        .unwrap();
    assert!(host
        .start_verification_recheck("project", &a.operation_id)
        .is_err());
    assert_eq!(
        fs::read(evidence(root.path(), &a).join("check-temp/validator-count")).unwrap(),
        count
    );
}

#[test]
fn host_recheck_cancel_is_unknown_and_blocks_old_recovery_or_rerun_after_reopen() {
    let (root, mut host, a) = setup("slow_checker");
    approve(&mut host, &a);
    host.start_verification_recheck("project", &a.operation_id)
        .unwrap();
    let marker = evidence(root.path(), &a).join("check-temp/validator-started");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !marker.exists() {
        assert!(Instant::now() < deadline);
        host.poll_verifications();
        assert_eq!(state(&host, &a), VerificationState::Running);
        std::thread::sleep(Duration::from_millis(20));
    }
    let cancelled = host.handle(request(Command::CancelVerification {
        project_id: "project".into(),
        operation_id: a.operation_id.clone(),
    }));
    assert!(cancelled.error.is_none());
    finish(&mut host, &a);
    assert_eq!(state(&host, &a), VerificationState::Unknown);
    assert!(evidence(root.path(), &a)
        .join("check.process-exit.json")
        .exists());
    assert!(host
        .recover_verification_recheck("project", &a.operation_id)
        .is_err());
    assert!(!evidence(root.path(), &a)
        .join("check-temp/validator-count")
        .exists());
    drop(host);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(state(&host, &a), VerificationState::Unknown);
    assert!(host
        .start_verification_recheck("project", &a.operation_id)
        .is_err());
    assert!(host
        .start_verification("project", "bypass", "task", "attempt", vec![])
        .is_err());
    let job = host
        .snapshot()
        .verification_jobs
        .into_iter()
        .find(|j| j.operation_id == a.operation_id)
        .unwrap();
    assert!(
        job.reconciliation_digest.is_none(),
        "old abandonment must not be offered"
    );
    assert!(!host.snapshot().workflows[0].state.tasks["task"].verified);
}

#[test]
fn host_recheck_publication_gap_recovers_original_result_without_checker_execution() {
    let (root, mut host, a) = setup("corrected_checker");
    approve(&mut host, &a);
    host.start_verification_recheck("project", &a.operation_id)
        .unwrap();
    finish(&mut host, &a);
    assert_eq!(state(&host, &a), VerificationState::Succeeded);
    drop(host);
    let events = root.path().join("data/workflows/project/events");
    let matched: Vec<_> = fs::read_dir(&events)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with(&format!("_{}.json", a.operation_id))
        })
        .collect();
    assert_eq!(matched.len(), 1);
    // Disposable fixture injection: retain all actual process files and the stale success badge.
    fs::remove_file(&matched[0]).unwrap();
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(state(&host, &a), VerificationState::Unknown);
    assert!(host
        .start_verification_recheck("project", &a.operation_id)
        .is_err());
    host.recover_verification_recheck("project", &a.operation_id)
        .unwrap();
    assert_eq!(state(&host, &a), VerificationState::Succeeded);
    assert_eq!(
        fs::read(evidence(root.path(), &a).join("check-temp/validator-count")).unwrap(),
        b"one"
    );
    host.recover_verification_recheck("project", &a.operation_id)
        .unwrap();
    drop(host);
    assert_eq!(
        state(&RuntimeHost::open(&root.path().join("data")).unwrap(), &a),
        VerificationState::Succeeded
    );
}

#[test]
fn host_recheck_partial_launch_claim_is_preserved_and_never_restarted() {
    let (root, mut host, a) = setup("corrected_checker");
    approve(&mut host, &a);
    fs::create_dir_all(claim(root.path(), &a)).unwrap();
    assert!(host
        .start_verification_recheck("project", &a.operation_id)
        .is_err());
    assert_eq!(state(&host, &a), VerificationState::Unknown);
    assert!(!claim(root.path(), &a).join("intent.json").exists());
    assert!(!evidence(root.path(), &a).exists());
    drop(host);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(state(&host, &a), VerificationState::Unknown);
    assert!(host
        .recover_verification_recheck("project", &a.operation_id)
        .is_err());
    assert!(host
        .start_verification_recheck("project", &a.operation_id)
        .is_err());
    assert!(!claim(root.path(), &a).join("intent.json").exists());
}

#[test]
fn host_recheck_damaged_projection_recovers_with_original_launch_identity() {
    let (root, mut host, a) = setup("corrected_checker");
    approve(&mut host, &a);
    host.start_verification_recheck("project", &a.operation_id)
        .unwrap();
    finish(&mut host, &a);
    assert_eq!(state(&host, &a), VerificationState::Succeeded);
    let dir = claim(root.path(), &a);
    let intent = fs::read(dir.join("intent.json")).unwrap();
    let initial: serde_json::Value = serde_json::from_slice(&intent).unwrap();
    let started = initial["initial"]["startedAt"].as_str().unwrap();
    drop(host);
    for corrupt in [false, true] {
        if corrupt {
            fs::write(dir.join("job.json"), b"{broken projection").unwrap();
        } else {
            fs::remove_file(dir.join("job.json")).unwrap();
        }
        let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(state(&host, &a), VerificationState::Unknown);
        let job = host
            .snapshot()
            .verification_jobs
            .into_iter()
            .find(|j| j.operation_id == a.operation_id)
            .unwrap();
        assert_eq!(job.started_at, started);
        if corrupt {
            assert_eq!(
                fs::read(dir.join("job.json")).unwrap(),
                b"{broken projection"
            );
        } else {
            assert!(!dir.join("job.json").exists());
        }
        assert!(host
            .start_verification_recheck("project", &a.operation_id)
            .is_err());
        host.recover_verification_recheck("project", &a.operation_id)
            .unwrap();
        assert_eq!(state(&host, &a), VerificationState::Succeeded);
        assert_eq!(fs::read(dir.join("intent.json")).unwrap(), intent);
        assert_eq!(
            fs::read(evidence(root.path(), &a).join("check-temp/validator-count")).unwrap(),
            b"one"
        );
    }
    assert_eq!(
        state(&RuntimeHost::open(&root.path().join("data")).unwrap(), &a),
        VerificationState::Succeeded
    );
}

#[test]
fn host_recheck_approval_pauses_persisted_run_and_recovery_never_resumes_it() {
    use cerulean_domain::project_setup::{ProjectRun, RunState};
    // Seed only a disposable fixture's initial run authorization. The actual
    // pause, recheck and recovery below all use the production Host paths.
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Body {
        sequence: u32,
        previous: String,
        at: String,
        actor: String,
        run: ProjectRun,
    }
    let (root, store, a) = runner::corrected("corrected_checker");
    let body = Body {
        sequence: 1,
        previous: "genesis".into(),
        at: chrono::Utc::now().to_rfc3339(),
        actor: "user".into(),
        run: ProjectRun {
            id: "original-project-run".into(),
            project_id: "project".into(),
            revision_digest: store.revision().unwrap().approval_digest().unwrap(),
            setup_digest: "a".repeat(64),
            state: RunState::Active,
            message: "Synthetic initial authorization for pause coverage".into(),
            acknowledged_decisions: 0,
        },
    };
    drop(store);
    let runs = root.path().join("data/run-control");
    assert_eq!(fs::read_dir(&runs).unwrap().count(), 0);
    let bytes =
        serde_json::to_vec(&serde_json::json!({"hash": digest(&body).unwrap(), "body": body}))
            .unwrap();
    fs::write(runs.join("00000001.json"), &bytes).unwrap();
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(host.snapshot().project_runs[0].state, RunState::Active);
    assert!(host
        .start_verification_recheck("project", &a.operation_id)
        .is_err());
    assert!(host
        .recover_verification_recheck("project", &a.operation_id)
        .is_err());
    assert_eq!(host.snapshot().project_runs[0].state, RunState::Active);
    assert_eq!(fs::read_dir(&runs).unwrap().count(), 1);
    approve(&mut host, &a);
    assert_eq!(host.snapshot().project_runs[0].state, RunState::Paused);
    assert!(!evidence(root.path(), &a).exists());
    let pause: serde_json::Value =
        serde_json::from_slice(&fs::read(runs.join("00000002.json")).unwrap()).unwrap();
    assert_eq!(pause["body"]["actor"], "user");
    assert_eq!(pause["body"]["run"]["state"], "paused");
    drop(host);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(host.snapshot().project_runs[0].state, RunState::Paused);
    host.start_verification_recheck("project", &a.operation_id)
        .unwrap();
    finish(&mut host, &a);
    assert_eq!(state(&host, &a), VerificationState::Succeeded);
    host.recover_verification_recheck("project", &a.operation_id)
        .unwrap();
    assert_eq!(host.snapshot().project_runs[0].state, RunState::Paused);
    assert_eq!(fs::read(runs.join("00000001.json")).unwrap(), bytes);
    assert_eq!(fs::read_dir(&runs).unwrap().count(), 2);
    drop(host);
    assert_eq!(
        RuntimeHost::open(&root.path().join("data"))
            .unwrap()
            .snapshot()
            .project_runs[0]
            .state,
        RunState::Paused
    );
}

#[test]
fn host_recheck_damaged_launch_authority_cannot_publish_existing_result() {
    let (root, mut host, a) = setup("corrected_checker");
    approve(&mut host, &a);
    host.start_verification_recheck("project", &a.operation_id)
        .unwrap();
    finish(&mut host, &a);
    assert_eq!(state(&host, &a), VerificationState::Succeeded);
    drop(host);
    let events = root.path().join("data/workflows/project/events");
    let matched: Vec<_> = fs::read_dir(&events)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .ends_with(&format!("_{}.json", a.operation_id))
        })
        .collect();
    assert_eq!(matched.len(), 1);
    fs::remove_file(&matched[0]).unwrap();
    // Preserve the actual completed checker result, but damage only the
    // disposable fixture's immutable launch intent before read-only recovery.
    let intent = claim(root.path(), &a).join("intent.json");
    fs::write(&intent, b"{broken launch authority").unwrap();
    let count = fs::read_dir(&events).unwrap().count();
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(state(&host, &a), VerificationState::Unknown);
    assert!(host
        .recover_verification_recheck("project", &a.operation_id)
        .is_err());
    assert!(host
        .start_verification_recheck("project", &a.operation_id)
        .is_err());
    assert_eq!(fs::read_dir(&events).unwrap().count(), count);
    assert!(!matched[0].exists());
    assert_eq!(fs::read(&intent).unwrap(), b"{broken launch authority");
    assert_eq!(
        fs::read(evidence(root.path(), &a).join("check-temp/validator-count")).unwrap(),
        b"one"
    );
}

#[test]
fn host_recheck_exit_receipts_are_required_and_bound_without_reexecuting_checker() {
    let (root, mut host, a) = setup("corrected_checker");
    approve(&mut host, &a);
    host.start_verification_recheck("project", &a.operation_id)
        .unwrap();
    finish(&mut host, &a);
    assert_eq!(state(&host, &a), VerificationState::Succeeded);
    drop(host);
    let dir = evidence(root.path(), &a);
    let receipt = dir.join("check.process-exit.json");
    let bytes = fs::read(&receipt).unwrap();
    let intent = claim(root.path(), &a).join("intent.json");
    let original = fs::read(&intent).unwrap();
    let claim_value: serde_json::Value = serde_json::from_slice(&original).unwrap();
    // Reject absent, partial, wrong-job, nonzero-process and impossible-time
    // receipts even though normal logs and the successful ledger still exist.
    for variant in 0..5 {
        let mut value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        match variant {
            0 => fs::remove_file(&receipt).unwrap(),
            1 => fs::write(&receipt, b"{incomplete").unwrap(),
            2 => {
                value["jobName"] = serde_json::json!("Global\\CeruleanLonging-recheck-invalid");
                fs::write(&receipt, serde_json::to_vec(&value).unwrap()).unwrap();
            }
            3 => {
                value["activeProcesses"] = serde_json::json!(1);
                fs::write(&receipt, serde_json::to_vec(&value).unwrap()).unwrap();
            }
            _ => {
                value["observedAt"] = serde_json::json!("2000-01-01T00:00:00Z");
                fs::write(&receipt, serde_json::to_vec(&value).unwrap()).unwrap();
            }
        }
        let mut reopened = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert!(reopened
            .recover_verification_recheck("project", &a.operation_id)
            .is_err());
        assert!(reopened
            .start_verification_recheck("project", &a.operation_id)
            .is_err());
        drop(reopened);
        fs::write(&receipt, &bytes).unwrap();
    }
    for variant in 0..3 {
        let mut value = claim_value.clone();
        match variant {
            0 => {
                value["processScope"] = serde_json::json!("b".repeat(64));
            }
            1 => {
                value.as_object_mut().unwrap().remove("processScope");
            }
            _ => {
                value["schemaVersion"] = serde_json::json!(1);
            }
        }
        fs::write(&intent, serde_json::to_vec(&value).unwrap()).unwrap();
        let mut reopened = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(state(&reopened, &a), VerificationState::Unknown);
        assert!(reopened
            .recover_verification_recheck("project", &a.operation_id)
            .is_err());
        drop(reopened);
    }
    fs::write(&intent, &original).unwrap();
    let mut reopened = RuntimeHost::open(&root.path().join("data")).unwrap();
    reopened
        .recover_verification_recheck("project", &a.operation_id)
        .unwrap();
    drop(reopened);
    // Compatibility fixture: schema1 has no process proof. It can only use the
    // old complete-result recovery, never acquire invented v2 exit evidence.
    let mut legacy = claim_value;
    legacy["schemaVersion"] = serde_json::json!(1);
    legacy.as_object_mut().unwrap().remove("processScope");
    fs::write(&intent, serde_json::to_vec(&legacy).unwrap()).unwrap();
    fs::remove_file(&receipt).unwrap();
    let mut reopened = RuntimeHost::open(&root.path().join("data")).unwrap();
    reopened
        .recover_verification_recheck("project", &a.operation_id)
        .unwrap();
    assert_eq!(state(&reopened, &a), VerificationState::Succeeded);
    assert_eq!(
        fs::read(dir.join("check-temp/validator-count")).unwrap(),
        b"one"
    );
}

#[test]
fn host_recheck_exit_receipt_survives_later_log_publication_failure() {
    let (root, mut host, a) = setup("slow_checker");
    approve(&mut host, &a);
    host.start_verification_recheck("project", &a.operation_id)
        .unwrap();
    let dir = evidence(root.path(), &a);
    let deadline = Instant::now() + Duration::from_secs(30);
    while !dir.join("check-temp/validator-started").exists() {
        assert!(Instant::now() < deadline);
        host.poll_verifications();
        std::thread::sleep(Duration::from_millis(20));
    }
    // Only the disposable synthetic fixture: cause publication to fail AFTER
    // the real process tree has exited, without fabricating a terminal result.
    fs::create_dir(dir.join("check.stdout")).unwrap();
    let cancelled = host.handle(request(Command::CancelVerification {
        project_id: "project".into(),
        operation_id: a.operation_id.clone(),
    }));
    assert!(cancelled.error.is_none());
    finish(&mut host, &a);
    assert_eq!(state(&host, &a), VerificationState::Unknown);
    let receipt = fs::read(dir.join("check.process-exit.json")).unwrap();
    let exit: serde_json::Value = serde_json::from_slice(&receipt).unwrap();
    assert_eq!(exit["activeProcesses"], 0);
    assert!(!dir.join("check.json").exists());
    assert!(!dir.join("check-temp/validator-count").exists());
    drop(host);
    let mut reopened = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(state(&reopened, &a), VerificationState::Unknown);
    assert!(reopened
        .recover_verification_recheck("project", &a.operation_id)
        .is_err());
    assert!(reopened
        .start_verification_recheck("project", &a.operation_id)
        .is_err());
    assert_eq!(
        fs::read(dir.join("check.process-exit.json")).unwrap(),
        receipt
    );
    assert!(!reopened.snapshot().workflows[0].state.tasks["task"].verified);
}
