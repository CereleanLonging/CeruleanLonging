use super::*;

fn retry_request(evidence: &str, reason: &str) -> Request {
    Request {
        id: "explicit-preflight-retry".into(),
        protocol_version: PROTOCOL_VERSION,
        command: Command::RetryVerificationStart {
            project_id: "project".into(),
            original_operation_id: "stage-check".into(),
            evidence_digest: evidence.into(),
            reason: reason.into(),
        },
    }
}

fn preflight_failure() -> (tempfile::TempDir, RuntimeHost) {
    let (root, mut host, _, pins) = async_fixture("success");
    fs::write(root.path().join("project/result.txt"), "changed").unwrap();
    host.start_verification("project", "stage-check", "task", "attempt", pins)
        .unwrap();
    complete_detail_verification(&mut host);
    assert_eq!(
        host.snapshot().verification_jobs[0].state,
        VerificationState::Failed
    );
    assert!(!root
        .path()
        .join("data/workflows/project/verification/stage-check")
        .exists());
    fs::write(root.path().join("project/result.txt"), "abc").unwrap();
    (root, host)
}

#[test]
fn explicit_start_retry_runs_original_submission_once_and_preserves_failure() {
    let (root, mut host) = preflight_failure();
    let detail = host
        .handle(detail_request())
        .result
        .unwrap()
        .verification_detail
        .unwrap();
    assert!(detail.can_retry_start);
    assert!(host
        .handle(retry_request(&"0".repeat(64), "repair"))
        .error
        .is_some());
    assert!(host
        .handle(retry_request(&detail.evidence_digest, " "))
        .error
        .is_some());
    assert!(!root.path().join("data/verification-start-retries").exists());
    let started = host.handle(retry_request(
        &detail.evidence_digest,
        "Restored the approved artifact before starting the original checks",
    ));
    assert!(started.error.is_none(), "{:?}", started.error);
    assert!(host
        .handle(retry_request(&detail.evidence_digest, "repair"))
        .error
        .is_some());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while host
        .snapshot()
        .verification_jobs
        .iter()
        .any(|job| job.state == VerificationState::Running)
    {
        assert!(std::time::Instant::now() < deadline);
        host.poll_verifications();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let snapshot = host.snapshot();
    assert_eq!(snapshot.verification_jobs.len(), 2);
    assert_eq!(
        snapshot
            .verification_jobs
            .iter()
            .filter(|j| j.state == VerificationState::Succeeded)
            .count(),
        1
    );
    assert!(snapshot.workflows[0].state.tasks["task"].verified);
    assert_eq!(
        snapshot.workflows[0].state.tasks["task"].state,
        TaskState::Submitted
    );
    drop(host);
    let mut reopened = RuntimeHost::open(&root.path().join("data")).unwrap();
    let original = reopened
        .handle(detail_request())
        .result
        .unwrap()
        .verification_detail
        .unwrap();
    assert_eq!(original.state, VerificationState::Failed);
    assert!(!original.can_retry_start);
    assert!(reopened
        .handle(retry_request(&detail.evidence_digest, "repair"))
        .error
        .is_some());
    assert_eq!(reopened.snapshot().verification_jobs.len(), 2);
}

#[test]
fn start_retry_rejects_runner_intent_and_incomplete_authorization() {
    for stored in ["runner", "authority"] {
        let (root, mut host) = preflight_failure();
        let evidence = host
            .handle(detail_request())
            .result
            .unwrap()
            .verification_detail
            .unwrap()
            .evidence_digest;
        if stored == "runner" {
            fs::create_dir_all(
                root.path()
                    .join("data/workflows/project/verification/stage-check"),
            )
            .unwrap();
        } else {
            let directory = root.path().join("data/verification-start-retries/project");
            fs::create_dir_all(&directory).unwrap();
            fs::write(directory.join("stage-check.json"), b"partial-authority").unwrap();
        }
        let detail = host
            .handle(detail_request())
            .result
            .unwrap()
            .verification_detail
            .unwrap();
        assert!(!detail.can_retry_start, "{stored}");
        assert!(host
            .handle(retry_request(&evidence, "repair"))
            .error
            .is_some());
        assert_eq!(host.snapshot().verification_jobs.len(), 1);
    }
}

#[test]
fn start_retry_rejects_completed_real_checker_failure() {
    let (_root, mut host, _, pins) = async_fixture("detail-first-fail");
    host.start_verification("project", "stage-check", "task", "attempt", pins)
        .unwrap();
    complete_detail_verification(&mut host);
    let detail = host
        .handle(detail_request())
        .result
        .unwrap()
        .verification_detail
        .unwrap();
    assert_eq!(detail.state, VerificationState::Failed);
    assert!(detail.ledger_recorded);
    assert!(!detail.can_retry_start);
    assert!(host
        .handle(retry_request(&detail.evidence_digest, "repair"))
        .error
        .is_some());
}
