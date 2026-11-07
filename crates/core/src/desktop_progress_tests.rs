use super::*;
use std::sync::atomic::AtomicU32;

fn pair(root: &Path) -> (RuntimeHost, DesktopTaskStatus) {
    let (host, review) =
        crate::host::desktop_sessions::tests::assigned_desktop_task_with_validator(root, false);
    let status = DesktopTaskStatus {
        progress: None,
        reviewed_stop_record: None,
        cancellation: None,
        stopping: false,
        retired: false,
        retirement_digest: None,
        review,
        proof: None,
        submitted: false,
        active: true,
        reading: false,
        error: None,
    };
    (host, status)
}
fn sample(review: &DesktopTaskReview) -> DesktopTaskProgress {
    let mut observation = review.observation.clone();
    observation.captured_at = chrono::Utc::now().to_rfc3339();
    observation.busy = true;
    DesktopTaskProgress {
        schema_version: 1,
        task_review_digest: review.id.clone(),
        observation,
        baseline: review.baseline[1..].to_vec(),
        turn_runtime_id: "42.1.10".into(),
        prompt_runtime_id: "42.1.9".into(),
        visible_digest: "d".repeat(64),
        status_text: Some("生成中".into()),
    }
}
fn reply(request: &ReadRequest) -> Result<ReadReply, String> {
    match request {
        ReadRequest::TaskProgress { review, .. } => Ok(ReadReply {
            task_progress: Some(sample(review)),
            ..Default::default()
        }),
        _ => panic!("progress must not send, focus, stop or inspect unrelated windows"),
    }
}
fn collect(tracker: &mut Tracker, root: &Path, status: &mut DesktopTaskStatus, f: Fixture) {
    tracker.next = Instant::now();
    tracker.poll(root, status, false, Some(f));
    let until = Instant::now() + Duration::from_secs(5);
    while !tracker.idle() {
        assert!(Instant::now() < until);
        tracker.poll(root, status, false, Some(f));
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn desktop_progress_rejects_foreign_stale_and_unbound_samples_for_both_apps() {
    let root = tempfile::tempdir().unwrap();
    let (_host, original) = pair(root.path());
    for (app, version) in [
        (DesktopApp::Qoder, "0.4.3.0"),
        (DesktopApp::Qoder, "0.4.8.0"),
        (DesktopApp::Workbuddy, "5.7.7.0"),
        (DesktopApp::Workbuddy, "5.8.3.0"),
    ] {
        let mut review = original.review.clone();
        review.observation.window.version = version.into();
        if app == DesktopApp::Workbuddy {
            review.observation.window.app = app;
            review.observation.window.exe = "D:\\apps\\WorkBuddy.exe".into();
            review.observation.window.version = version.into();
            review.observation.session_id = None;
        }
        review.id.clear();
        review.id = digest(&review).unwrap();
        let good = sample(&review);
        validate(&review, &good).unwrap();
        crate::desktop_execution::validate_task_window(&review.observation.window).unwrap();
        for mutation in 0..12 {
            let mut bad = good.clone();
            match mutation {
                0 => bad.task_review_digest = "f".repeat(64),
                1 => bad.observation.window.pid += 1,
                2 => bad.observation.document_runtime_id.push('9'),
                3 => bad.observation.session_id = Some("foreign".into()),
                4 => bad.observation.model_label = Some("foreign".into()),
                5 => bad.baseline[0] = "f".repeat(64),
                6 => bad.baseline.truncate(1),
                7 => bad.observation.draft_empty = false,
                8 => {
                    bad.observation.captured_at =
                        (chrono::Utc::now() - chrono::Duration::seconds(31)).to_rfc3339()
                }
                9 => {
                    bad.observation.captured_at =
                        (chrono::Utc::now() + chrono::Duration::seconds(5)).to_rfc3339()
                }
                10 => bad.turn_runtime_id = bad.prompt_runtime_id.clone(),
                _ => bad.visible_digest = "invalid".into(),
            }
            assert!(validate(&review, &bad).is_err(), "mutation {mutation}");
        }
    }
}

#[test]
fn desktop_progress_keeps_errors_separate_and_never_turns_labels_into_completion() {
    let root = tempfile::tempdir().unwrap();
    let (host, mut status) = pair(root.path());
    let original_state = digest(&host.projects["project"].state()).unwrap();
    let mut tracker = Tracker::default();
    collect(&mut tracker, root.path(), &mut status, reply);
    assert_eq!(status.progress.as_ref().unwrap().samples, 1);
    fn completed(r: &ReadRequest) -> Result<ReadReply, String> {
        let mut reply = reply(r)?;
        let sample = reply.task_progress.as_mut().unwrap();
        sample.status_text = Some("已完成 1s".into());
        sample.observation.busy = false;
        Ok(reply)
    }
    collect(&mut tracker, root.path(), &mut status, completed);
    assert!(status.proof.is_none() && !status.submitted && status.error.is_none());
    fn foreign_turn(r: &ReadRequest) -> Result<ReadReply, String> {
        let mut reply = reply(r)?;
        reply.task_progress.as_mut().unwrap().turn_runtime_id = "42.1.99".into();
        Ok(reply)
    }
    collect(&mut tracker, root.path(), &mut status, foreign_turn);
    let view = status.progress.as_ref().unwrap();
    assert_eq!(view.samples, 2);
    assert!(view.error.is_some());
    assert_eq!(view.latest.as_ref().unwrap().turn_runtime_id, "42.1.10");
    assert_eq!(
        digest(&host.projects["project"].state()).unwrap(),
        original_state
    );
}

#[test]
fn desktop_progress_bounds_polling_and_retires_inflight_observation_on_stop() {
    let root = tempfile::tempdir().unwrap();
    let (_host, mut status) = pair(root.path());
    let mut tracker = Tracker::default();
    tracker.poll(root.path(), &mut status, false, Some(reply));
    assert!(tracker.idle() && status.progress.is_none());
    tracker.attempts = MAX_SAMPLES;
    tracker.next = Instant::now();
    tracker.poll(root.path(), &mut status, false, Some(reply));
    assert!(tracker.idle());
    tracker.attempts = 0;
    fn slow(request: &ReadRequest) -> Result<ReadReply, String> {
        std::thread::sleep(Duration::from_millis(35));
        reply(request)
    }
    tracker.poll(root.path(), &mut status, false, Some(slow));
    assert!(!tracker.idle());
    let until = Instant::now() + Duration::from_secs(5);
    while !tracker.idle() {
        assert!(Instant::now() < until);
        tracker.poll(root.path(), &mut status, true, Some(slow));
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(status.progress.as_ref().unwrap().latest.is_none());
    assert!(!status.progress.as_ref().unwrap().checking);
}

#[test]
fn desktop_progress_exhaustion_marks_previous_sample_without_restarting_or_failing_task() {
    fn unavailable(_: &ReadRequest) -> Result<ReadReply, String> {
        Err("原窗口已变化".into())
    }
    fn must_not_probe(_: &ReadRequest) -> Result<ReadReply, String> {
        panic!("exhausted observation must not restart")
    }
    for final_failure in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (host, mut status) = pair(root.path());
        let original_state = digest(&host.projects["project"].state()).unwrap();
        let mut tracker = Tracker::default();
        collect(&mut tracker, root.path(), &mut status, reply);
        tracker.attempts = MAX_SAMPLES - 1;
        collect(
            &mut tracker,
            root.path(),
            &mut status,
            if final_failure { unavailable } else { reply },
        );
        let view = status.progress.as_ref().unwrap();
        assert!(!view.checking);
        assert_eq!(view.samples, if final_failure { 1 } else { 2 });
        assert!(view.error.as_ref().unwrap().contains(if final_failure {
            "原窗口已变化"
        } else {
            "观察已达到上限"
        }));
        let last = view.latest.clone();
        for _ in 0..3 {
            tracker.next = Instant::now();
            tracker.poll(root.path(), &mut status, false, Some(must_not_probe));
            assert!(tracker.idle());
        }
        assert_eq!(status.progress.as_ref().unwrap().latest, last);
        assert!(status.active && !status.submitted && status.proof.is_none());
        assert!(status.error.is_none());
        assert_eq!(
            digest(&host.projects["project"].state()).unwrap(),
            original_state
        );
    }
}

static SENDS: AtomicU32 = AtomicU32::new(0);
static PROBES: AtomicU32 = AtomicU32::new(0);
static OBSERVED: AtomicBool = AtomicBool::new(false);
fn live_owner_fixture(r: &ReadRequest) -> Result<ReadReply, String> {
    match r {
        ReadRequest::TaskSend { review, .. } => {
            SENDS.fetch_add(1, Ordering::SeqCst);
            let until = Instant::now() + Duration::from_secs(15);
            while !OBSERVED.load(Ordering::SeqCst) {
                if Instant::now() >= until {
                    return Err("fixture timed out".into());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            crate::host::desktop_sessions::tests::write_sum(
                Path::new(&review.attempt.binding.workspace)
                    .parent()
                    .unwrap(),
                true,
            );
            Ok(ReadReply {
                task_proof: Some(crate::host::desktop_sessions::tests::task_proof(review)),
                ..Default::default()
            })
        }
        ReadRequest::TaskProgress { .. } => {
            PROBES.fetch_add(1, Ordering::SeqCst);
            reply(r)
        }
        _ => panic!("unexpected desktop side effect"),
    }
}
#[test]
fn desktop_progress_original_background_owner_observes_without_resending_and_reopen_is_quiet() {
    SENDS.store(0, Ordering::SeqCst);
    PROBES.store(0, Ordering::SeqCst);
    OBSERVED.store(false, Ordering::SeqCst);
    let root = tempfile::tempdir().unwrap();
    let (mut host, status) = pair(root.path());
    host.desktop_fixture = Some(live_owner_fixture);
    host.desktop_access.task_review = Some(status.review.clone());
    host.retain_desktop_fixture_owner(status.review.clone());
    let until = Instant::now() + Duration::from_secs(20);
    while host.desktop_active.is_some() {
        assert!(Instant::now() < until);
        host.poll_desktop_tasks();
        if let Some(active) = &host.desktop_active {
            if active
                .status
                .progress
                .as_ref()
                .is_some_and(|p| p.samples == 1)
            {
                assert!(active.status.active && !active.status.submitted);
                OBSERVED.store(true, Ordering::SeqCst);
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(SENDS.load(Ordering::SeqCst), 1);
    assert_eq!(PROBES.load(Ordering::SeqCst), 1);
    assert!(OBSERVED.load(Ordering::SeqCst));
    assert_eq!(
        host.desktop_access.tasks[0]
            .progress
            .as_ref()
            .unwrap()
            .samples,
        1
    );
    assert!(host.desktop_access.tasks[0].submitted);
    let data = host.root.clone();
    drop(host);
    for _ in 0..2 {
        let mut reopened = RuntimeHost::open(&data).unwrap();
        reopened.desktop_fixture = Some(live_owner_fixture);
        reopened.poll_desktop_tasks();
        assert!(reopened.desktop_active.is_none());
        assert!(reopened.desktop_access.tasks[0].progress.is_none());
        assert_eq!(SENDS.load(Ordering::SeqCst), 1);
        assert_eq!(PROBES.load(Ordering::SeqCst), 1);
    }
}
