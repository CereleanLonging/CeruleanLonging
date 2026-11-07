use super::*;
use std::time::{Duration, Instant};
fn setup(checker: &str) -> (FixtureRoot, RuntimeHost, RecheckApproval) {
    let (root, store, a) = runner::corrected(checker);
    drop(store);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    host.approve_verification_recheck(&a.review, "approve-recheck", &a.operation_id)
        .unwrap();
    (root, host, a)
}
fn state(host: &RuntimeHost, op: &str) -> VerificationState {
    host.snapshot()
        .verification_jobs
        .iter()
        .find(|j| j.operation_id == op)
        .unwrap()
        .state
        .clone()
}
fn finish(host: &mut RuntimeHost, op: &str) {
    let end = Instant::now() + Duration::from_secs(45);
    while matches!(
        state(host, op),
        VerificationState::Running | VerificationState::Cancelling
    ) {
        assert!(Instant::now() < end);
        host.poll_verifications();
        std::thread::sleep(Duration::from_millis(20));
    }
}
fn evidence(root: &Path, op: &str) -> std::path::PathBuf {
    root.join("data/workflows/project/verification").join(op)
}
fn acl_has_sid(path: &Path, sid: &str) -> bool {
    use std::os::windows::process::CommandExt;
    let output = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            concat!(
                "$ErrorActionPreference='Stop'; ",
                "Import-Module (Join-Path $PSHOME 'Modules/Microsoft.PowerShell.Security/Microsoft.PowerShell.Security.psd1') -ErrorAction Stop; ",
                "(Get-Acl -LiteralPath $env:CERULEAN_ACL_TEST_PATH).GetSecurityDescriptorSddlForm([Security.AccessControl.AccessControlSections]::Access)",
            ),
        ])
        .env("CERULEAN_ACL_TEST_PATH", path)
        .creation_flags(0x08000000)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().contains(sid)
}
fn next(host: &mut RuntimeHost, a: &RecheckApproval) -> String {
    let mut checks = a.review.replacement_checks.clone();
    checks[0] = fixed_check(&checks[0], "corrected_checker");
    let r = host
        .review_verification_recheck(&RecheckRequest {
            project_id: "project".into(),
            original_operation_id: a.review.original_operation_id.clone(),
            replacement_checks: checks,
            reason: "Explicit separate corrected verification".into(),
        })
        .unwrap();
    assert!(host
        .approve_verification_recheck(&r, "another-approval", &a.operation_id)
        .is_err());
    let op = "new-corrected-operation".to_owned();
    host.approve_verification_recheck(&r, "new-approval", &op)
        .unwrap();
    host.start_verification_recheck("project", &op).unwrap();
    finish(host, &op);
    assert_eq!(state(host, &op), VerificationState::Succeeded);
    op
}
#[test]
#[ignore = "explicit ordinary Windows accounts; run the native recheck suite"]
fn unstarted_disposition_is_user_event_preserves_submission_and_allows_only_new_approval() {
    let (root, mut host, a) = setup("corrected_checker");
    let before = host.snapshot().workflows[0].state.clone();
    assert!(host
        .review_verification_recheck_disposition("project", &a.operation_id, "")
        .is_err());
    let r = host
        .review_verification_recheck_disposition(
            "project",
            &a.operation_id,
            "Withdraw this unstarted check",
        )
        .unwrap();
    assert!(r.launch.is_none());
    let e = host
        .retire_verification_recheck(&r, "dispose-recheck")
        .unwrap();
    assert_eq!(e.body.event.principal, "user");
    assert_eq!(host.snapshot().workflows[0].state, before);
    assert!(!evidence(root.path(), &a.operation_id).exists());
    assert_eq!(
        host.retire_verification_recheck(&r, "dispose-recheck")
            .unwrap(),
        e
    );
    assert!(host
        .retire_verification_recheck(&r, "different-disposition")
        .is_err());
    assert!(host
        .start_verification_recheck("project", &a.operation_id)
        .is_err());
    drop(host);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(host.snapshot().workflows[0].state, before);
    next(&mut host, &a);
    assert!(!evidence(root.path(), &a.operation_id).exists());
    assert_eq!(
        state(&host, &a.review.original_operation_id),
        VerificationState::Failed
    );
}
#[test]
#[ignore = "explicit ordinary Windows accounts; run the native recheck suite"]
fn cancelled_disposition_rejects_active_stale_and_wrong_authority_then_replays() {
    let (root, mut host, a) = setup("slow_checker");
    let unstarted = host
        .review_verification_recheck_disposition("project", &a.operation_id, "Cancel old review")
        .unwrap();
    host.start_verification_recheck("project", &a.operation_id)
        .unwrap();
    let dir = evidence(root.path(), &a.operation_id);
    let end = Instant::now() + Duration::from_secs(30);
    while !dir.join("check-temp/validator-started").exists() {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(host
        .review_verification_recheck_disposition("project", &a.operation_id, "Still active")
        .is_err());
    let reply = host.handle(Request {
        id: "cancel".into(),
        protocol_version: PROTOCOL_VERSION,
        command: Command::CancelVerificationRecheck {
            project_id: "project".into(),
            operation_id: a.operation_id.clone(),
        },
    });
    assert!(reply.error.is_none());
    finish(&mut host, &a.operation_id);
    assert_eq!(state(&host, &a.operation_id), VerificationState::Unknown);
    assert!(host
        .retire_verification_recheck(&unstarted, "stale-dispose")
        .is_err());
    let r = host
        .review_verification_recheck_disposition(
            "project",
            &a.operation_id,
            "Retain cancelled evidence",
        )
        .unwrap();
    let result = fs::read(dir.join("result.json")).unwrap();
    fs::write(dir.join("result.json"), b"{interrupted-result").unwrap();
    assert!(host
        .retire_verification_recheck(&r, "changed-evidence")
        .is_err());
    fs::write(dir.join("result.json"), &result).unwrap();
    drop(host);
    let ledger = root.path().join("data/workflows/project");
    let config = serde_json::from_slice(&fs::read(ledger.join("bindings.json")).unwrap()).unwrap();
    let mut store = DurableWorkflow::open(&ledger, config).unwrap();
    let command = WorkflowCommand::RetireVerificationRecheck {
        task: r.task_id.clone(),
        attempt: r.attempt_id.clone(),
        operation_id: r.operation_id.clone(),
        review_digest: digest(&r).unwrap(),
    };
    let context = CommandContext {
        recheck_disposition: Some(Box::new(r.clone())),
        approval_source: Some("authenticated local-user verification recheck disposition".into()),
        ..Default::default()
    };
    assert!(store
        .command("wrong-role", "core", command.clone(), context.clone(), true)
        .is_err());
    assert!(store
        .command(
            "missing-proof",
            "user",
            command.clone(),
            Default::default(),
            true
        )
        .is_err());
    let mut fake = context.clone();
    fake.recheck_disposition
        .as_mut()
        .unwrap()
        .process_evidence_digest = "b".repeat(64);
    let mut fake_command = command;
    if let WorkflowCommand::RetireVerificationRecheck { review_digest, .. } = &mut fake_command {
        *review_digest = digest(fake.recheck_disposition.as_ref().unwrap()).unwrap();
    }
    assert!(store
        .command("forged-process-proof", "user", fake_command, fake, true)
        .is_err());
    drop(store);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    host.retire_verification_recheck(&r, "dispose-cancelled")
        .unwrap();
    assert_eq!(state(&host, &a.operation_id), VerificationState::Abandoned);
    assert!(!host.snapshot().workflows[0].state.tasks["task"].verified);
    assert_eq!(
        host.snapshot().workflows[0].state.tasks["task"].state,
        TaskState::Submitted
    );
    assert!(host
        .recover_verification_recheck("project", &a.operation_id)
        .is_err());
    drop(host);
    let host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(state(&host, &a.operation_id), VerificationState::Abandoned);
    // Reconstruct from immutable original identity and ledger without repairing
    // missing or malformed derived job projection on a read/open.
    drop(host);
    let projection = root
        .path()
        .join("data/verification-recheck-jobs/project")
        .join(&a.operation_id)
        .join("job.json");
    let retained = fs::read(&projection).unwrap();
    fs::remove_file(&projection).unwrap();
    let host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(state(&host, &a.operation_id), VerificationState::Abandoned);
    assert!(!projection.exists());
    drop(host);
    fs::write(&projection, b"{partial-projection").unwrap();
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(state(&host, &a.operation_id), VerificationState::Abandoned);
    assert_eq!(fs::read(&projection).unwrap(), b"{partial-projection");
    let status = host.handle(Request {
        id: "disposed-status".into(),
        protocol_version: PROTOCOL_VERSION,
        command: Command::ReadVerificationRecheck {
            project_id: "project".into(),
            operation_id: a.operation_id.clone(),
        },
    });
    let d = status
        .result
        .unwrap()
        .verification_recheck_status
        .unwrap()
        .disposition
        .unwrap();
    assert!(d.evidence_unchanged);
    assert_eq!(d.review, r);
    assert_eq!(d.event_operation_id, "dispose-cancelled");
    fs::write(&projection, retained).unwrap();
    next(&mut host, &a);
    assert_eq!(fs::read(dir.join("result.json")).unwrap(), result);
    assert!(!dir.join("check-temp/validator-count").exists());
}
#[test]
#[ignore = "explicit ordinary Windows accounts; run the native recheck suite"]
fn crashed_owner_without_receipt_can_be_disposed_without_restarting_original_checker() {
    let (root, host, a) = setup("slow_checker");
    drop(host);
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "recheck_ledger::disposition::crash_host",
            "--test-threads=1",
        ])
        .env("CERULEAN_RECHECK_CRASH_DATA", root.path().join("data"))
        .env("CERULEAN_RECHECK_CRASH_OPERATION", &a.operation_id)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(45);
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let _ = child.wait();
            panic!("owned crash helper timed out")
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(status.success());
    let dir = evidence(root.path(), &a.operation_id);
    assert!(dir.join("check-temp/validator-started").exists());
    assert!(!dir.join("check.process-exit.json").exists());
    assert!(!dir.join("result.json").exists());
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(state(&host, &a.operation_id), VerificationState::Unknown);
    assert!(host
        .recover_verification_recheck("project", &a.operation_id)
        .is_err());
    let r = host
        .review_verification_recheck_disposition(
            "project",
            &a.operation_id,
            "Retain interrupted owner evidence",
        )
        .unwrap();
    assert!(r.launch.is_some());
    let intent = fs::read(dir.join("intent.json")).unwrap();
    let isolation_path = dir.join("check-isolation.json");
    let isolation = fs::read(&isolation_path).unwrap();
    let proof: serde_json::Value = serde_json::from_slice(&isolation).unwrap();
    let account = root
        .path()
        .join("data/worker-accounts/project")
        .join(proof["identity"].as_str().unwrap());
    let lease_path = account.join("check-active-lease.json");
    let lease = fs::read(&lease_path).unwrap();
    let original: serde_json::Value = serde_json::from_slice(&lease).unwrap();
    let sid = proof["sid"].as_str().unwrap();
    assert!(acl_has_sid(&dir.join("work"), sid));
    assert!(acl_has_sid(&dir.join("check-temp/validator-started"), sid));
    // Read-only opening and reviewing retained the original lease and ACLs.
    // An authenticated retirement must still reject unrelated/tampered leases.
    for (field, value) in [
        ("sid", serde_json::json!("S-1-5-21-1-2-3-9999")),
        (
            "paths",
            serde_json::json!([dir.join("work"), root.path().join("chief")]),
        ),
    ] {
        let mut changed = original.clone();
        changed[field] = value;
        fs::write(&lease_path, serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(host
            .retire_verification_recheck(&r, "dispose-crashed")
            .is_err());
        assert!(!dir.join("check-access-retirement.json").exists());
        assert!(acl_has_sid(&dir.join("work"), sid));
    }
    fs::write(&lease_path, &lease).unwrap();
    let mut changed = proof.clone();
    changed["operationId"] = serde_json::json!("another-operation");
    fs::write(&isolation_path, serde_json::to_vec(&changed).unwrap()).unwrap();
    assert!(host
        .retire_verification_recheck(&r, "dispose-crashed")
        .is_err());
    fs::write(&isolation_path, &isolation).unwrap();
    let quarantine = account.join("check-quarantine.json");
    fs::write(
        &quarantine,
        br#"{"reason":"check-workspace-permission-cleanup-failed"}"#,
    )
    .unwrap();
    let retired = host
        .retire_verification_recheck(&r, "dispose-crashed")
        .unwrap();
    assert!(!lease_path.exists());
    assert!(!quarantine.exists());
    for path in [
        "work",
        "work/result.txt",
        "check-temp",
        "check-temp/validator-started",
    ] {
        assert!(!acl_has_sid(&dir.join(path), sid), "{path}");
    }
    let recovery = fs::read(dir.join("check-access-retirement.json")).unwrap();
    assert!(dir.join("check-access-retired.json").exists());
    // Resume a crash after lease deletion but before the final cleanup receipt.
    fs::remove_file(dir.join("check-access-retired.json")).unwrap();
    assert_eq!(
        host.retire_verification_recheck(&r, "dispose-crashed")
            .unwrap(),
        retired
    );
    // Replaying an old completed retirement cannot touch a later lease.
    fs::write(&lease_path, br#"{"newOperation":true}"#).unwrap();
    assert_eq!(
        host.retire_verification_recheck(&r, "dispose-crashed")
            .unwrap(),
        retired
    );
    assert_eq!(fs::read(&lease_path).unwrap(), br#"{"newOperation":true}"#);
    fs::remove_file(&lease_path).unwrap();
    assert_eq!(
        fs::read(dir.join("check-access-retirement.json")).unwrap(),
        recovery
    );
    assert_eq!(fs::read(&isolation_path).unwrap(), isolation);
    assert_eq!(state(&host, &a.operation_id), VerificationState::Abandoned);
    next(&mut host, &a);
    assert_eq!(fs::read(dir.join("intent.json")).unwrap(), intent);
    assert!(!dir.join("result.json").exists());
    assert!(!dir.join("check-temp/validator-count").exists());
    drop(host);
    let host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(state(&host, &a.operation_id), VerificationState::Abandoned);
}
#[test]
#[ignore = "bounded parent-owned native crash fixture"]
fn crash_host() {
    let data = std::path::PathBuf::from(std::env::var_os("CERULEAN_RECHECK_CRASH_DATA").unwrap());
    let op = std::env::var("CERULEAN_RECHECK_CRASH_OPERATION").unwrap();
    let mut host = RuntimeHost::open(&data).unwrap();
    host.start_verification_recheck("project", &op).unwrap();
    let marker = data
        .join("workflows/project/verification")
        .join(&op)
        .join("check-temp/validator-started");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !marker.exists() {
        if Instant::now() >= deadline {
            std::process::exit(2)
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    // Exit without running Rust destructors; Windows closes the Job handle.
    std::process::exit(0);
}
