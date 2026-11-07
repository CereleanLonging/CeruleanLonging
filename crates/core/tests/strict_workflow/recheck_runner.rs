use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

pub(super) fn corrected(checker: &str) -> (FixtureRoot, DurableWorkflow, RecheckApproval) {
    let (root, store, mut a) = fixture_mode(true, true);
    a.review.replacement_checks[0] = fixed_check(&a.review.replacement_checks[0], checker);
    (root, store, a)
}
fn directory(root: &Path, a: &RecheckApproval) -> std::path::PathBuf {
    root.join("data/workflows/project/verification")
        .join(&a.operation_id)
}
fn reopen(root: &Path) -> DurableWorkflow {
    let ledger = root.join("data/workflows/project");
    let config = serde_json::from_slice(&fs::read(ledger.join("bindings.json")).unwrap()).unwrap();
    DurableWorkflow::open(&ledger, config).unwrap()
}
fn verified(entry: &Entry) -> &RecheckResult {
    let Some(ExecutionEvidence::VerificationRecheck { result, .. }) = &entry.body.context.execution
    else {
        panic!("expected recheck proof")
    };
    result
}
fn count(root: &Path, a: &RecheckApproval) -> Vec<u8> {
    fs::read(directory(root, a).join("check-temp/validator-count")).unwrap()
}

#[test]
#[ignore = "explicit ordinary Windows accounts; run the native recheck suite"]
fn runner_executes_corrected_checks_on_original_source_then_accepts_and_reopens() {
    let (root, mut s, a) = corrected("corrected_checker");
    let original = original_files(root.path(), &s);
    let plan = s.revision().unwrap().clone();
    let approved_plan = s.approval().unwrap().clone();
    approve_recheck(&mut s, &a);
    let entry = s
        .run_verification_recheck(&a.operation_id, &AtomicBool::new(false))
        .unwrap();
    let result = verified(&entry);
    assert!(matches!(
        entry.body.event.command,
        WorkflowCommand::Verify { passed: true, .. }
    ));
    assert_eq!(result.report.checks.len(), 2);
    assert_eq!(
        result
            .report
            .source_snapshot
            .as_ref()
            .unwrap()
            .manifest_sha256,
        a.review.original_source_snapshot.manifest_sha256
    );
    let (_, manifest) = s.completed_artifacts("task", "attempt").unwrap();
    assert!(result.report.require_pass(&plan, manifest, "core").is_err());
    assert_eq!(s.revision().unwrap(), &plan);
    assert_eq!(s.approval().unwrap(), &approved_plan);
    assert!(s.state().tasks["task"].verified);
    assert_eq!(s.state().tasks["task"].state, TaskState::Submitted);
    assert_eq!(count(root.path(), &a), b"one");
    assert!(s
        .run_verification_recheck(&a.operation_id, &AtomicBool::new(false))
        .is_err());
    assert_eq!(
        s.recover_verification_recheck(&a.operation_id).unwrap(),
        entry
    );
    apply(
        &mut s,
        "chief",
        WorkflowCommand::Accept {
            task: "task".into(),
            attempt: "attempt".into(),
        },
        Default::default(),
    )
    .unwrap();
    drop(s);
    let mut s = reopen(root.path());
    assert_eq!(s.state().tasks["task"].state, TaskState::Accepted);
    assert_eq!(
        s.recover_verification_recheck(&a.operation_id).unwrap(),
        entry
    );
    for (path, bytes) in original {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}

#[test]
#[ignore = "explicit ordinary Windows accounts; run the native recheck suite"]
fn runner_failed_check_retains_failed_result_and_does_not_accept_or_repeat() {
    let (root, mut s, a) = corrected("failed_checker");
    approve_recheck(&mut s, &a);
    let entry = s
        .run_verification_recheck(&a.operation_id, &AtomicBool::new(false))
        .unwrap();
    assert!(matches!(
        entry.body.event.command,
        WorkflowCommand::Verify { passed: false, .. }
    ));
    // The packaged checker reports a failing runtime as exit code 2.
    assert_eq!(verified(&entry).report.checks[0].exit_code, Some(2));
    assert!(!verified(&entry).report.checks[0].assertions[0].passed);
    assert_eq!(verified(&entry).report.checks.len(), 2);
    assert!(!s.state().tasks["task"].verified);
    assert!(s
        .run_verification_recheck(&a.operation_id, &AtomicBool::new(false))
        .is_err());
    assert!(s
        .run_verification_recheck("alternate-operation", &AtomicBool::new(false))
        .is_err());
    assert_eq!(count(root.path(), &a), b"one");
}

#[test]
#[ignore = "explicit ordinary Windows accounts; run the native recheck suite"]
fn runner_recovery_verifies_original_records_and_logs_without_running_again() {
    let (root, mut s, a) = corrected("corrected_checker");
    let original = original_files(root.path(), &s);
    approve_recheck(&mut s, &a);
    let entry = s
        .run_verification_recheck(&a.operation_id, &AtomicBool::new(false))
        .unwrap();
    let event = root
        .path()
        .join("data/workflows/project/events")
        .join(format!(
            "{:012}_{}.json",
            entry.body.event.sequence, a.operation_id
        ));
    drop(s);
    // Controlled publication-gap injection in this disposable fixture only.
    // All actual process observations and terminal files remain untouched.
    fs::remove_file(event).unwrap();
    let mut s = reopen(root.path());
    assert!(!s.state().tasks["task"].verified);
    let dir = directory(root.path(), &a);
    let terminal = fs::read(dir.join("result.json")).unwrap();
    assert!(s
        .run_verification_recheck(&a.operation_id, &AtomicBool::new(false))
        .is_err());
    let intent_path = dir.join("intent.json");
    let intent = fs::read(&intent_path).unwrap();
    let mut altered: serde_json::Value = serde_json::from_slice(&intent).unwrap();
    altered["recheck"]["approvalDigest"] = serde_json::json!("0".repeat(64));
    fs::write(&intent_path, serde_json::to_vec(&altered).unwrap()).unwrap();
    assert!(s.recover_verification_recheck(&a.operation_id).is_err());
    fs::write(intent_path, intent).unwrap();
    let stdout_path = dir.join("check.stdout");
    let stdout = fs::read(&stdout_path).unwrap();
    fs::write(&stdout_path, b"altered log").unwrap();
    assert!(s.recover_verification_recheck(&a.operation_id).is_err());
    fs::write(stdout_path, stdout).unwrap();
    let stage = dir.join("check.json");
    let bytes = fs::read(&stage).unwrap();
    fs::remove_file(&stage).unwrap();
    assert!(s.recover_verification_recheck(&a.operation_id).is_err());
    fs::write(stage, bytes).unwrap();
    let before = s.history().len();
    let restored = s.recover_verification_recheck(&a.operation_id).unwrap();
    assert_eq!(verified(&restored), verified(&entry));
    assert!(s.state().tasks["task"].verified);
    assert_eq!(s.history().len(), before + 1);
    assert_eq!(
        s.recover_verification_recheck(&a.operation_id).unwrap(),
        restored
    );
    assert_eq!(s.history().len(), before + 1);
    assert_eq!(count(root.path(), &a), b"one");
    assert_eq!(fs::read(dir.join("result.json")).unwrap(), terminal);
    for (path, bytes) in original {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}

#[test]
#[ignore = "explicit ordinary Windows accounts; run the native recheck suite"]
fn runner_timeout_records_only_observed_prefix_and_never_runs_next_stage() {
    let (root, mut s, mut a) = corrected("slow_checker");
    a.review.replacement_checks[0].timeout_seconds = 1;
    a.review.replacement_checks[0] = fixed_check(&a.review.replacement_checks[0], "slow_checker");
    approve_recheck(&mut s, &a);
    let started = Instant::now();
    let entry = s
        .run_verification_recheck(&a.operation_id, &AtomicBool::new(false))
        .unwrap();
    assert!(started.elapsed() < Duration::from_secs(30));
    assert!(matches!(
        entry.body.event.command,
        WorkflowCommand::Verify { passed: false, .. }
    ));
    let report = &verified(&entry).report;
    assert_eq!(report.checks.len(), 1);
    assert!(report.checks[0].timed_out);
    assert!(!directory(root.path(), &a).join("second-temp").exists());
    assert!(!directory(root.path(), &a)
        .join("check-temp/validator-count")
        .exists());
    assert!(!s.state().tasks["task"].verified);
}

#[test]
#[ignore = "explicit ordinary Windows accounts; run the native recheck suite"]
fn runner_cancellation_preserves_unresolved_intent_and_cannot_relaunch() {
    let (root, mut s, a) = corrected("slow_checker");
    approve_recheck(&mut s, &a);
    let operation = a.operation_id.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    let signal = cancel.clone();
    let worker = std::thread::spawn(move || {
        let result = s.run_verification_recheck(&operation, &signal);
        (s, result)
    });
    let marker = directory(root.path(), &a).join("check-temp/validator-started");
    let deadline = Instant::now() + Duration::from_secs(30);
    while !marker.exists() {
        assert!(!worker.is_finished(), "checker did not start");
        assert!(Instant::now() < deadline, "checker launch deadline");
        std::thread::sleep(Duration::from_millis(20));
    }
    cancel.store(true, Ordering::Release);
    let (mut s, result) = worker.join().unwrap();
    assert!(result.unwrap_err().contains("cancelled"));
    assert!(!s.state().tasks["task"].verified);
    assert!(s
        .run_verification_recheck(&a.operation_id, &AtomicBool::new(false))
        .is_err());
    assert!(s.recover_verification_recheck(&a.operation_id).is_err());
    assert!(!directory(root.path(), &a)
        .join("check-temp/validator-count")
        .exists());
    drop(s);
    let mut s = reopen(root.path());
    assert!(s
        .run_verification_recheck(&a.operation_id, &AtomicBool::new(false))
        .is_err());
    assert!(s
        .run_verification("alternate", "task", "attempt", &[], &AtomicBool::new(false))
        .is_err());
}

#[test]
#[ignore = "explicit ordinary Windows accounts; run the native recheck suite"]
fn runner_revalidates_pins_before_claiming_operation_or_launching() {
    let (root, mut s, mut a) = corrected("corrected_checker");
    assert!(s
        .run_verification_recheck(&a.operation_id, &AtomicBool::new(false))
        .is_err());
    let check = &mut a.review.replacement_checks[0];
    let mut recipe = cerulean_domain::objective_checker::BoundScript::parse(&check.args).unwrap();
    let script = root.path().join("node.exe");
    fs::copy(&recipe.runtime.path, &script).unwrap();
    recipe.runtime =
        cerulean_core::verifier::PinnedCheck::describe(&script, &[], Default::default())
            .unwrap()
            .executable;
    check.tool.as_mut().unwrap().inputs = vec![recipe.runtime.clone()];
    check.args = recipe.arguments().unwrap();
    approve_recheck(&mut s, &a);
    let original_log = root
        .path()
        .join("data/workflows/project/verification/stage-check/check.stdout");
    let original_bytes = fs::read(&original_log).unwrap();
    fs::write(&original_log, b"altered after the review was approved").unwrap();
    let rejected = s
        .run_verification_recheck(&a.operation_id, &AtomicBool::new(false))
        .unwrap_err();
    assert!(rejected.contains("日志摘要不匹配"), "{rejected}");
    assert!(!directory(root.path(), &a).exists());
    fs::write(&original_log, original_bytes).unwrap();
    fs::write(script, b"replacement after user approval").unwrap();
    assert!(s
        .run_verification_recheck(&a.operation_id, &AtomicBool::new(false))
        .is_err());
    assert!(!directory(root.path(), &a).exists());
    assert!(!s.state().tasks["task"].verified);
}

#[test]
#[ignore = "explicit owned checker child process only"]
fn corrected_checker() {
    assert_eq!(fs::read_to_string("result.txt").unwrap(), "abc");
    let temp = std::path::PathBuf::from(std::env::var_os("TEMP").unwrap());
    assert!(!temp.join("validator-count").exists());
    fs::write(temp.join("validator-count"), b"one").unwrap();
    println!("corrected checker verified original fixture result");
}
#[test]
#[ignore = "explicit owned failing checker child process only"]
fn failed_checker() {
    corrected_checker();
    std::process::exit(13);
}
#[test]
#[ignore = "explicit owned slow checker child process only"]
fn slow_checker() {
    let temp = std::path::PathBuf::from(std::env::var_os("TEMP").unwrap());
    fs::write(temp.join("validator-started"), b"ready").unwrap();
    std::thread::sleep(Duration::from_secs(30));
    corrected_checker();
}
