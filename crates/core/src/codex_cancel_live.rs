//! Opt-in real worker cancellation. Management and approval are explicit fixtures.
use super::*;
use cerulean_domain::{management::ManagementDecision, project_setup::RunState};
use serde_json::{json, Value};
use std::{
    io::Write,
    time::{Duration, Instant},
};

fn save(path: &Path, value: &impl serde::Serialize) {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    file.write_all(&serde_json::to_vec_pretty(value).unwrap())
        .unwrap();
    file.sync_all().unwrap();
}

fn history(data: &Path) -> Vec<Value> {
    let mut paths: Vec<_> = fs::read_dir(data.join("workflows/project/events"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| serde_json::from_slice(&fs::read(path).unwrap()).unwrap())
        .collect()
}

fn count(events: &[Value], kind: &str) -> usize {
    events
        .iter()
        .filter(|entry| entry["body"]["event"]["command"]["kind"] == kind)
        .count()
}

#[test]
#[ignore = "explicit real Codex cancellation; one worker turn, synthetic management and approval; fresh reports directory required"]
fn live_codex_cancel_once_and_reopen() {
    assert_eq!(
        std::env::var("CERULEAN_CANCEL_LIVE_OWNED_JOB").as_deref(),
        Ok("1"),
        "use scripts/test-codex-cancel-live.ps1 so every worker has bounded process-tree cleanup"
    );
    let mut in_job = 0;
    assert_ne!(
        unsafe {
            windows_sys::Win32::System::JobObjects::IsProcessInJob(
                windows_sys::Win32::System::Threading::GetCurrentProcess(),
                std::ptr::null_mut(),
                &mut in_job,
            )
        },
        0
    );
    assert_ne!(
        in_job, 0,
        "live cancellation test requires a Windows Job Object"
    );
    let began = Instant::now();
    let deadline = began + Duration::from_secs(180);
    let root = PathBuf::from(
        std::env::var_os("CERULEAN_CANCEL_LIVE_DIR")
            .expect("CERULEAN_CANCEL_LIVE_DIR must name a fresh reports directory"),
    )
    .canonicalize()
    .unwrap();
    let reports = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../reports")
        .canonicalize()
        .unwrap();
    assert!(root.starts_with(&reports) && root != reports);
    ordinary(&root, true).unwrap();
    assert!(
        fs::read_dir(&root).unwrap().next().is_none(),
        "report directory must be empty"
    );
    let executable =
        std::env::var("CERULEAN_CODEX_EXE").expect("explicit Codex executable required");
    let model = std::env::var("CERULEAN_CODEX_MODEL").expect("explicit Codex model required");
    let (mut host, mut draft) = super::project_planning::tests::fixture(&root);
    draft.codex_executable = executable;
    draft.worker_model = model;
    draft.worker_max_requests = 1;
    draft.management_max_requests = 1;
    draft.max_seconds = 60;
    let goal = "Create sum.mjs as an ES module exporting sum(a,b) that returns a+b for numeric inputs. Keep it minimal.";
    draft.requirement = goal.into();
    host.configure_project(draft).unwrap();
    let mut proposal = super::project_planning::tests::proposal("tests");
    if let ManagementDecision::Plan { tasks, .. } = &mut proposal {
        tasks[0].goal = goal.into();
    }
    super::project_planning::tests::complete(&mut host, proposal);
    let revision = host.projects["project"]
        .revision()
        .unwrap()
        .approval_digest()
        .unwrap();
    host.handle(Request {
        id: "synthetic-user-approval-for-live-cancel".into(),
        protocol_version: PROTOCOL_VERSION,
        command: Command::ApproveRevision {
            project_id: "project".into(),
            digest: revision.clone(),
        },
    })
    .result
    .unwrap();
    // Keep the real run Active to exercise the cancellation pause, while preventing
    // all automatic management requests. No credential is read or fabricated.
    host.orchestrating = true;
    let setup = host.project_setups["project"].digest.clone();
    host.start_project_run("project", &revision, &setup)
        .unwrap();
    assert_eq!(host.runs.all()[0].state, RunState::Active);
    assert_eq!(host.management.statuses().len(), 1);
    assert!(host.management_active.is_empty());
    let data = root.join("data");
    save(&root.join("before-start.json"), &host.snapshot());
    save(
        &root.join("scope.json"),
        &json!({
            "scope": "real logged-in Codex worker; synthetic chief response and simulated user approval",
            "realManagementCalls": 0, "syntheticManagementDecisions": 1,
            "realThreeLevelClosure": false, "maxHostStartCalls": 1,
            "maxCancelCalls": 1, "maxReconcileCalls": 1, "maxSeconds": 180,
            "rawProviderProofPersisted": false,
            "proofEvidence": "original workflow receipts retain provider proof SHA256; raw remote proof is not persisted by production execution"
        }),
    );
    let start_error = host
        .start_codex("project", "task", "cancel-live-once", &revision)
        .err();
    let mut cancel_attempted = false;
    let mut cancel_accepted = false;
    let mut cancel_error = None;
    let mut reconcile_attempted = false;
    let mut reconcile_error = None;
    let mut polls = 0;
    while start_error.is_none() && Instant::now() < deadline {
        host.poll_verifications();
        polls += 1;
        if !cancel_attempted {
            if let Some(candidate) = host.codex_cancellations().into_iter().next() {
                save(&root.join("cancel-candidate.json"), &candidate);
                save(&root.join("before-cancel.json"), &host.snapshot());
                cancel_attempted = true;
                let response = host.handle(Request {
                    id: "user-live-cancel-once".into(), protocol_version: PROTOCOL_VERSION,
                    command: Command::CancelCodex {
                        project_id: candidate.project_id, attempt_id: candidate.attempt_id,
                        evidence_digest: candidate.evidence_digest,
                        reason: "Explicit one-shot live cancellation verification; preserve any existing edits and stop subsequent scheduling".into(),
                    },
                });
                cancel_error = response.error.map(|error| error.message);
                cancel_accepted = cancel_error.is_none();
                save(&root.join("after-cancel-request.json"), &host.snapshot());
            }
        }
        if !host.has_codex_activity("project") {
            let state = host
                .codex_jobs
                .get(&("project".into(), "cancel-live-once".into()))
                .map(|job| job.state.clone());
            if state == Some(CodexJobState::Unknown)
                && !reconcile_attempted
                && deadline.saturating_duration_since(Instant::now()) >= Duration::from_secs(60)
            {
                // The only extra provider operation is readback of the original
                // bound attempt; it cannot start another turn or interrupt again.
                save(&root.join("before-reconcile.json"), &host.snapshot());
                save(&root.join("history-before-reconcile.json"), &history(&data));
                reconcile_attempted = true;
                reconcile_error = host.reconcile_codex("project", "cancel-live-once").err();
                if reconcile_error.is_none() {
                    continue;
                }
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let active_at_deadline = host.has_codex_activity("project");
    let final_snapshot = host.snapshot();
    let job_state = final_snapshot
        .codex_jobs
        .first()
        .map(|job| job.state.clone());
    let task_state = final_snapshot.workflows[0].state.tasks["task"].state;
    let run_paused = host.runs.all()[0].state == RunState::Paused;
    let events = history(&data);
    let dispatches = count(&events, "dispatch-intent");
    let started_receipts = count(&events, "started");
    let cancel_intents = count(&events, "cancel-intent");
    let original_binding = events.iter().find_map(|entry| {
        let evidence = &entry["body"]["context"]["execution"];
        (evidence["kind"] == "attempt").then(|| evidence["attempt"].clone())
    });
    let receipts: Vec<_> = events
        .iter()
        .filter(|entry| entry["body"]["context"]["execution"]["kind"] == "receipt")
        .map(|entry| entry["body"]["context"]["execution"].clone())
        .collect();
    let cancelled_receipts = receipts
        .iter()
        .filter(|evidence| evidence["receipt"]["status"] == "cancelled")
        .count();
    let management_requests = host.management.statuses().len();
    save(&root.join("terminal-snapshot.json"), &final_snapshot);
    save(&root.join("original-workflow-history.json"), &events);
    save(
        &root.join("original-attempt-and-receipts.json"),
        &json!({
            "attempt": original_binding, "receipts": receipts,
            "cancelIntents": events.iter().filter(|entry| entry["body"]["event"]["command"]["kind"] == "cancel-intent").collect::<Vec<_>>()
        }),
    );
    let mut reopened_state = None;
    let mut restart_new_dispatches = None;
    let mut restart_new_management_requests = None;
    let mut reopen_error = None;
    let mut reopen_history_unchanged = false;
    let mut reopened_run_paused = None;
    if !active_at_deadline {
        drop(host);
        match RuntimeHost::open(&data) {
            Ok(reopened) => {
                // Deliberately no polling: reopening and reading alone may not
                // advance the run, resend a turn, or repeat cancellation.
                let snapshot = reopened.snapshot();
                reopened_state = snapshot.codex_jobs.first().map(|job| job.state.clone());
                reopened_run_paused = Some(reopened.runs.all()[0].state == RunState::Paused);
                let after = history(&data);
                restart_new_dispatches =
                    Some(count(&after, "dispatch-intent").saturating_sub(dispatches));
                restart_new_management_requests = Some(
                    reopened
                        .management
                        .statuses()
                        .len()
                        .saturating_sub(management_requests),
                );
                reopen_history_unchanged = after == events;
                save(&root.join("reopened-snapshot.json"), &snapshot);
                save(&root.join("reopened-workflow-history.json"), &after);
            }
            Err(error) => reopen_error = Some(error),
        }
    }
    let passed = cancel_accepted
        && cancel_intents == 1
        && dispatches == 1
        && started_receipts == 1
        && cancelled_receipts == 1
        && job_state == Some(CodexJobState::Cancelled)
        && task_state == TaskState::Cancelled
        && run_paused
        && reopened_state == Some(CodexJobState::Cancelled)
        && reopened_run_paused == Some(true)
        && reopen_history_unchanged
        && restart_new_dispatches == Some(0)
        && restart_new_management_requests == Some(0)
        && management_requests == 1;
    let status = if passed {
        "passed"
    } else if start_error.is_some()
        || job_state == Some(CodexJobState::Failed)
        || reopen_error.is_some()
        || (!cancel_attempted && started_receipts == 0)
    {
        "blocked"
    } else {
        "inconclusive"
    };
    let result = json!({
        "at": chrono::Utc::now().to_rfc3339(), "status": status,
        "scope": "one real Codex worker cancellation, synthetic management and user approval",
        "realThreeLevelClosure": false, "realManagementCalls": 0, "independentVerifierCalls": 0,
        "syntheticManagementDecisions": 1, "managementJournalRequests": management_requests,
        "hostStartCalls": 1, "workerDispatchIntents": dispatches,
        "workerStartedReceipts": started_receipts, "cancelCalls": usize::from(cancel_attempted),
        "cancelAccepted": cancel_accepted, "cancelIntents": cancel_intents,
        "cancelledReceipts": cancelled_receipts, "reconcileCalls": usize::from(reconcile_attempted),
        "jobState": job_state, "taskState": task_state, "runPaused": run_paused,
        "reopenedState": reopened_state, "reopenedRunPaused": reopened_run_paused,
        "restartNewDispatches": restart_new_dispatches,
        "restartNewManagementRequests": restart_new_management_requests,
        "reopenHistoryUnchanged": reopen_history_unchanged,
        "naturalCompletionRace": job_state == Some(CodexJobState::Submitted),
        "activeAtDeadline": active_at_deadline, "elapsedSeconds": began.elapsed().as_secs_f64(),
        "polls": polls, "startError": start_error, "cancelError": cancel_error,
        "reconcileError": reconcile_error, "reopenError": reopen_error,
        "rawProviderProofPersisted": false,
        "evidence": ["original-workflow-history.json", "original-attempt-and-receipts.json", "terminal-snapshot.json"]
    });
    save(&root.join("result.json"), &result);
    println!("{}", serde_json::to_string(&result).unwrap());
    // These are safety invariants, not a claim that cancellation succeeded.
    // A completion race, missing terminal, or timeout remains explicitly unproven.
    assert!(dispatches <= 1 && started_receipts <= 1 && cancel_intents <= 1);
    assert_eq!(management_requests, 1);
    assert!(restart_new_dispatches.is_none_or(|n| n == 0));
    assert!(restart_new_management_requests.is_none_or(|n| n == 0));
}
