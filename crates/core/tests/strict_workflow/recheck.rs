//! Real ledger and Windows snapshot boundaries with synthetic result evidence.
//! This does not exercise the pending corrected-check runner or any provider.
use super::*;
use cerulean_domain::verification_recheck::*;
#[path = "recheck_disposition.rs"]
mod disposition;
#[path = "recheck_host.rs"]
mod host;
#[path = "recheck_interface.rs"]
mod interface;
#[path = "recheck_runner.rs"]
mod runner;
#[path = "recheck_workspace.rs"]
mod workspace;

const APPROVAL_SOURCE: &str = "authenticated local-user verification recheck approval";

fn fixture(protected: bool) -> (tempfile::TempDir, DurableWorkflow, RecheckApproval) {
    let (root, mut host, p, pins) = async_fixture("detail-first-fail");
    host.start_verification("project", "stage-check", "task", "attempt", pins)
        .unwrap();
    complete_detail_verification(&mut host);
    let original_job = host.snapshot().verification_jobs[0].clone();
    assert_eq!(original_job.state, VerificationState::Failed);
    let mut checks = p.execution[0].checks.clone();
    checks[0].args.push("--test-threads=1".into());
    let review = host
        .review_verification_recheck(&RecheckRequest {
            project_id: "project".into(),
            original_operation_id: "stage-check".into(),
            replacement_checks: checks,
            reason: "Synthetic corrected checker ledger test".into(),
        })
        .unwrap();
    drop(host);
    let data = root.path().join("data");
    if protected {
        let archive = root.path().join("before-reader18");
        let digest = cerulean_core::backup::create(&data, &archive)
            .unwrap()
            .manifest_digest;
        let review =
            cerulean_core::verification_recheck_guard::review(&data, &archive, &digest).unwrap();
        cerulean_core::verification_recheck_guard::promote(
            &data,
            &archive,
            &digest,
            &review.digest,
        )
        .unwrap();
    }
    let ledger = data.join("workflows/project");
    let config = serde_json::from_slice(&fs::read(ledger.join("bindings.json")).unwrap()).unwrap();
    (
        root,
        DurableWorkflow::open(&ledger, config).unwrap(),
        RecheckApproval {
            schema_version: 1,
            operation_id: "corrected-check".into(),
            review,
            original_job,
        },
    )
}

fn approval_command(a: &RecheckApproval) -> WorkflowCommand {
    WorkflowCommand::ApproveVerificationRecheck {
        task: a.review.task_id.clone(),
        attempt: a.review.attempt_id.clone(),
        operation_id: a.operation_id.clone(),
        review_digest: digest(&a.review).unwrap(),
    }
}
fn approval_context(a: &RecheckApproval) -> CommandContext {
    CommandContext {
        verification_recheck: Some(Box::new(a.clone())),
        approval_source: Some(APPROVAL_SOURCE.into()),
        ..Default::default()
    }
}
fn approve_recheck(s: &mut DurableWorkflow, a: &RecheckApproval) -> Entry {
    s.command(
        "approve-recheck",
        "user",
        approval_command(a),
        approval_context(a),
        false,
    )
    .unwrap()
}
fn result_fixture(
    root: &Path,
    s: &DurableWorkflow,
    a: &RecheckApproval,
    approved: &Entry,
    passed: bool,
) -> (WorkflowCommand, CommandContext) {
    let (_, manifest) = s.completed_artifacts("task", "attempt").unwrap();
    let dir = root
        .join("data/workflows/project/verification")
        .join(&a.operation_id);
    fs::create_dir(&dir).unwrap();
    let dir = fs::canonicalize(dir).unwrap();
    let snapshot = cerulean_core::source_snapshot::SnapshotLease::create(
        s.revision().unwrap(),
        manifest,
        &dir,
        &a.operation_id,
    )
    .unwrap();
    assert_eq!(
        snapshot.seal.manifest_sha256,
        a.review.original_source_snapshot.manifest_sha256
    );
    let result = RecheckResult {
        schema_version: 1,
        operation_id: a.operation_id.clone(),
        review_digest: digest(&a.review).unwrap(),
        report: VerificationReport {
            schema_version: 1,
            verifier_id: "core".into(),
            observed_at: chrono::Utc::now().to_rfc3339(),
            manifest_digest: digest(manifest).unwrap(),
            source_snapshot: Some(snapshot.seal.clone()),
            checks: a
                .review
                .replacement_checks
                .iter()
                .map(|check| CheckObservation {
                    check_id: check.id.clone(),
                    command_digest: digest(check).unwrap(),
                    exit_code: Some(if passed { 0 } else { 1 }),
                    timed_out: false,
                    stdout_sha256: "a".repeat(64),
                    stderr_sha256: "b".repeat(64),
                })
                .collect(),
        },
    };
    assert!(result
        .report
        .require_pass(s.revision().unwrap(), manifest, "core")
        .is_err());
    let command = WorkflowCommand::Verify {
        task: "task".into(),
        attempt: "attempt".into(),
        passed,
        evidence: vec![digest(&result).unwrap()],
    };
    let context = CommandContext {
        execution: Some(ExecutionEvidence::VerificationRecheck {
            approval: EventAnchor {
                sequence: approved.body.event.sequence,
                hash: approved.hash.clone(),
            },
            result: Box::new(result),
        }),
        ..Default::default()
    };
    (command, context)
}

fn original_files(root: &Path, s: &DurableWorkflow) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let events = root.join("data/workflows/project/events");
    let mut paths: Vec<_> = fs::read_dir(events)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    let verify = root.join("data/workflows/project/verification/stage-check");
    for name in [
        "intent.json",
        "result.json",
        "snapshot.json",
        "check.stdout",
        "check.stderr",
        "second.stdout",
        "second.stderr",
    ] {
        paths.push(verify.join(name));
    }
    assert!(!s.state().tasks["task"].verified);
    paths
        .into_iter()
        .map(|p| {
            let bytes = fs::read(&p).unwrap();
            (p, bytes)
        })
        .collect()
}

#[test]
fn recheck_ledger_preserves_failure_and_replays_verified_before_chief_accept() {
    let (root, mut s, a) = fixture(true);
    let original = original_files(root.path(), &s);
    let approved = approve_recheck(&mut s, &a);
    assert_eq!(s.state().tasks["task"].state, TaskState::Submitted);
    assert!(!s.state().tasks["task"].verified);
    let accept = WorkflowCommand::Accept {
        task: "task".into(),
        attempt: "attempt".into(),
    };
    assert!(s
        .command(
            "premature",
            "chief",
            accept.clone(),
            Default::default(),
            false
        )
        .is_err());
    let (command, context) = result_fixture(root.path(), &s, &a, &approved, true);
    s.command(
        &a.operation_id,
        "core",
        command.clone(),
        context.clone(),
        false,
    )
    .unwrap();
    assert_eq!(s.state().tasks["task"].state, TaskState::Submitted);
    assert!(s.state().tasks["task"].verified);
    // Idempotent ledger read is not a second execution permission.
    let count = s.history().len();
    assert_eq!(approve_recheck(&mut s, &a), approved);
    s.command(
        &a.operation_id,
        "core",
        command.clone(),
        context.clone(),
        false,
    )
    .unwrap();
    assert_eq!(s.history().len(), count);
    assert!(s
        .command(&a.operation_id, "core", command, context, true)
        .is_err());
    for (path, bytes) in &original {
        assert_eq!(fs::read(path).unwrap(), *bytes);
    }
    let ledger = root.path().join("data/workflows/project");
    let config: SchedulerConfig =
        serde_json::from_slice(&fs::read(ledger.join("bindings.json")).unwrap()).unwrap();
    drop(s);
    // Ledger replay carries the captured job; it does not depend on a mutable job projection.
    let job = root
        .path()
        .join("data/verification-jobs/project/stage-check.json");
    assert!(job.is_file());
    fs::write(&job, b"not consulted by workflow replay").unwrap();
    let mut s = DurableWorkflow::open(&ledger, config.clone()).unwrap();
    assert!(s.state().tasks["task"].verified);
    for who in ["user", "worker-codex", "core"] {
        assert!(s
            .command(
                "wrong-accept",
                who,
                accept.clone(),
                Default::default(),
                false
            )
            .is_err());
    }
    // A live mutation still prevents accepting a historically valid passing report.
    let source = root.path().join("project/result.txt");
    fs::write(&source, b"changed").unwrap();
    assert!(s
        .command("accept", "chief", accept.clone(), Default::default(), false)
        .is_err());
    fs::write(&source, b"abc").unwrap();
    s.command("accept", "chief", accept, Default::default(), false)
        .unwrap();
    assert_eq!(s.state().tasks["task"].state, TaskState::Accepted);
    drop(s);
    let s = DurableWorkflow::open(&ledger, config).unwrap();
    assert_eq!(s.state().tasks["task"].state, TaskState::Accepted);
    for (path, bytes) in original {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}

#[test]
fn recheck_ledger_rejects_missing_guard_wrong_role_scope_and_changed_source() {
    let (unprotected, mut s, a) = fixture(false);
    assert!(s
        .command(
            "approve",
            "user",
            approval_command(&a),
            approval_context(&a),
            false
        )
        .unwrap_err()
        .contains("reader18"));
    drop(s);
    drop(unprotected);
    let (root, mut s, a) = fixture(true);
    let count = s.history().len();
    for who in ["chief", "worker-codex", "core", "deputy"] {
        assert!(s
            .command(
                "approve",
                who,
                approval_command(&a),
                approval_context(&a),
                false
            )
            .is_err());
    }
    let mutations: &[fn(&mut RecheckApproval)] = &[
        |a| a.operation_id = a.review.original_operation_id.clone(),
        |a| a.review.project_id = "other".into(),
        |a| a.review.task_id = "other".into(),
        |a| a.review.attempt_id = "other".into(),
        |a| a.review.ledger_head = "0".repeat(64),
        |a| a.review.original_report_digest = "0".repeat(64),
        |a| {
            a.review.replacement_checks[0]
                .tool
                .as_mut()
                .unwrap()
                .executable
                .sha256 = "0".repeat(64)
        },
    ];
    for mutate in mutations {
        let mut bad = a.clone();
        mutate(&mut bad);
        assert!(s
            .command(
                "approve",
                "user",
                approval_command(&bad),
                approval_context(&bad),
                false
            )
            .is_err());
    }
    let mut context = approval_context(&a);
    context.approval_source = None;
    assert!(s
        .command("approve", "user", approval_command(&a), context, false)
        .is_err());
    let mut command = approval_command(&a);
    if let WorkflowCommand::ApproveVerificationRecheck { review_digest, .. } = &mut command {
        *review_digest = "0".repeat(64);
    }
    assert!(s
        .command("approve", "user", command, approval_context(&a), false)
        .is_err());
    assert!(s
        .command(
            &a.operation_id,
            "user",
            approval_command(&a),
            approval_context(&a),
            false
        )
        .is_err());
    let source = root.path().join("project/validator-mode");
    fs::write(&source, "changed source outside artifact list").unwrap();
    assert!(s
        .command(
            "approve",
            "user",
            approval_command(&a),
            approval_context(&a),
            false
        )
        .is_err());
    fs::write(source, "detail-first-fail").unwrap();
    assert_eq!(s.history().len(), count);
    approve_recheck(&mut s, &a);
    assert!(!s.state().tasks["task"].verified);
}

#[test]
fn recheck_ledger_rejects_unresolved_approval_and_intervening_event() {
    let (root, mut s, a) = fixture(true);
    let approved = approve_recheck(&mut s, &a);
    let (command, context) = result_fixture(root.path(), &s, &a, &approved, true);
    let mut next = a.clone();
    next.operation_id = "another-check".into();
    next.review.ledger_head = approved.hash;
    assert!(s
        .command(
            "another-approval",
            "user",
            approval_command(&next),
            approval_context(&next),
            false
        )
        .unwrap_err()
        .contains("unresolved"));
    apply(
        &mut s,
        "deputy",
        WorkflowCommand::Advise {
            task: "task".into(),
            suggestion: "intervening event".into(),
        },
        Default::default(),
    )
    .unwrap();
    assert!(s
        .command(&a.operation_id, "core", command, context, false)
        .unwrap_err()
        .contains("stale"));
    assert!(!s.state().tasks["task"].verified);
    let config: SchedulerConfig = serde_json::from_slice(
        &fs::read(root.path().join("data/workflows/project/bindings.json")).unwrap(),
    )
    .unwrap();
    drop(s);
    let s = DurableWorkflow::open(&root.path().join("data/workflows/project"), config).unwrap();
    assert!(!s.state().tasks["task"].verified);
}

#[test]
fn recheck_ledger_result_binds_approval_operation_report_and_observation_time() {
    let (root, mut s, a) = fixture(true);
    let approved = approve_recheck(&mut s, &a);
    let (command, context) = result_fixture(root.path(), &s, &a, &approved, true);
    for who in ["user", "chief", "worker-codex"] {
        assert!(s
            .command(
                &a.operation_id,
                who,
                command.clone(),
                context.clone(),
                false
            )
            .is_err());
    }
    assert!(s
        .command(
            "other-operation",
            "core",
            command.clone(),
            context.clone(),
            false
        )
        .is_err());
    for case in 0..8 {
        let mut bad = context.clone();
        let Some(ExecutionEvidence::VerificationRecheck { approval, result }) = &mut bad.execution
        else {
            unreachable!()
        };
        match case {
            0 => approval.hash = "0".repeat(64),
            1 => approval.sequence -= 1,
            2 => result.operation_id = "other".into(),
            3 => result.review_digest = "0".repeat(64),
            4 => {
                result
                    .report
                    .source_snapshot
                    .as_mut()
                    .unwrap()
                    .manifest_sha256 = "0".repeat(64)
            }
            5 => result.report.checks[0].command_digest = "0".repeat(64),
            6 => result.report.checks[0].exit_code = Some(1),
            7 => result.report.observed_at = a.original_job.started_at.clone(),
            _ => unreachable!(),
        }
        let mut changed = command.clone();
        if let WorkflowCommand::Verify { evidence, .. } = &mut changed {
            *evidence = vec![digest(result).unwrap()];
        }
        assert!(
            s.command(&a.operation_id, "core", changed, bad, false)
                .is_err(),
            "case {case}"
        );
    }
    let mut flag = command.clone();
    if let WorkflowCommand::Verify { passed, .. } = &mut flag {
        *passed = false;
    }
    assert!(s
        .command(&a.operation_id, "core", flag, context.clone(), false)
        .is_err());
    let mut bad = context.clone();
    bad.thread_id = Some("extra-authority".into());
    assert!(s
        .command(&a.operation_id, "core", command.clone(), bad, false)
        .is_err());
    s.command(&a.operation_id, "core", command, context, false)
        .unwrap();
    assert!(s.state().tasks["task"].verified);
}

#[test]
fn recheck_ledger_new_failure_preserves_submission_and_requires_new_approval() {
    let (root, mut s, a) = fixture(true);
    let approved = approve_recheck(&mut s, &a);
    let (command, context) = result_fixture(root.path(), &s, &a, &approved, false);
    let result = s
        .command(&a.operation_id, "core", command, context, false)
        .unwrap();
    assert!(!s.state().tasks["task"].verified);
    assert_eq!(s.state().tasks["task"].state, TaskState::Submitted);
    let mut next = a.clone();
    next.operation_id = "new-check".into();
    next.review.ledger_head = result.hash;
    s.command(
        "new-approval",
        "user",
        approval_command(&next),
        approval_context(&next),
        false,
    )
    .unwrap();
    assert!(!s.state().tasks["task"].verified);
}

#[test]
fn recheck_host_approval_is_durable_idempotent_and_does_not_start_or_redispatch() {
    let (root, s, a) = fixture(true);
    let original = original_files(root.path(), &s);
    let count = s.history().len();
    drop(s);
    let data = root.path().join("data");
    let mut host = RuntimeHost::open(&data).unwrap();
    let orphan = data.join("workflows/project/verification/orphan-operation");
    fs::create_dir(&orphan).unwrap();
    assert!(host
        .approve_verification_recheck(&a.review, "approve-orphan", "orphan-operation")
        .is_err());
    assert_eq!(host.snapshot().workflows[0].sequence as usize, count);
    let approved = host
        .approve_verification_recheck(&a.review, "approve-recheck", &a.operation_id)
        .unwrap();
    assert_eq!(
        approved
            .body
            .context
            .verification_recheck
            .as_ref()
            .unwrap()
            .original_job,
        a.original_job
    );
    assert_eq!(host.snapshot().workflows[0].sequence as usize, count + 1);
    assert_eq!(host.snapshot().verification_jobs.len(), 1);
    assert!(!host.snapshot().workflows[0].state.tasks["task"].verified);
    assert!(!data
        .join("workflows/project/verification")
        .join(&a.operation_id)
        .exists());
    assert_eq!(
        host.approve_verification_recheck(&a.review, "approve-recheck", &a.operation_id)
            .unwrap(),
        approved
    );
    assert!(host
        .start_verification("project", "bypass", "task", "attempt", vec![])
        .is_err());
    assert!(!data.join("workflows/project/verification/bypass").exists());
    let mut changed = a.review.clone();
    changed.reason.push_str(" changed");
    assert!(host
        .approve_verification_recheck(&changed, "approve-recheck", &a.operation_id)
        .is_err());
    drop(host);
    let mut host = RuntimeHost::open(&data).unwrap();
    assert_eq!(
        host.approve_verification_recheck(&a.review, "approve-recheck", &a.operation_id)
            .unwrap(),
        approved
    );
    assert!(host
        .start_verification("project", "bypass-after-reopen", "task", "attempt", vec![])
        .is_err());
    for (path, bytes) in original {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}
