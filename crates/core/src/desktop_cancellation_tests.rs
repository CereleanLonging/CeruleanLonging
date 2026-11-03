use super::*;
use crate::desktop_execution::{self as execution, cancellation};

#[test]
fn upgrade_desktop_unknown_blocks_but_confirmed_cancellation_can_prepare() {
    let root = tempfile::tempdir().unwrap();
    let (host, task) = unknown(root.path());
    let mut service = crate::diagnostic_service::ServiceHost::Ready(Box::new(host));
    let (review, _) = crate::upgrade::review(&mut service).unwrap();
    assert!(!review.ready);
    assert!(review
        .blockers
        .iter()
        .any(|b| b == "unresolved-desktop-history"));
    let crate::diagnostic_service::ServiceHost::Ready(host) = &mut service else {
        panic!()
    };
    assert!(cancel(host, &task).error.is_none());
    background_spin(host);
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Cancelled
    );
    let (review, _) = crate::upgrade::review(&mut service).unwrap();
    assert!(review.ready, "{:?}", review.blockers);
}

pub(super) fn running_observation(r: &execution::TaskReview) -> serde_json::Value {
    let mut o = r.observation.clone();
    o.busy = true;
    o.captured_at = chrono::Utc::now().to_rfc3339();
    serde_json::json!({"taskReviewDigest":r.id,"observation":o,"baseline":r.baseline,"turnRuntimeId":"fixture-turn","promptRuntimeId":"fixture-prompt","stopRuntimeId":"fixture-stop"})
}
pub(super) fn cancelled_proof(o: &serde_json::Value) -> serde_json::Value {
    let first = chrono::Utc::now().to_rfc3339();
    std::thread::sleep(Duration::from_millis(260));
    let mut view = o["observation"].clone();
    view["busy"] = false.into();
    view["capturedAt"] = chrono::Utc::now().to_rfc3339().into();
    serde_json::json!({"taskReviewDigest":o["taskReviewDigest"],"observation":view,"firstObservedAt":first,"turnRuntimeId":o["turnRuntimeId"],"promptRuntimeId":o["promptRuntimeId"],"observedBaseline":o["baseline"],"statusText":"状态：已中断，耗时：1s","cancelled":true})
}
pub(super) fn stop_fixture(request: &ReadRequest) -> Result<ReadReply, String> {
    match request {
        ReadRequest::TaskStopPrepare { review, .. } => {
            if Path::new(&review.attempt.binding.workspace)
                .join("complete-before-stop")
                .exists()
            {
                return Err("task already completed".into());
            }
            Ok(ReadReply {
                stop_observation: Some(running_observation(review)),
                ..Default::default()
            })
        }
        ReadRequest::TaskStop {
            review,
            observation,
            ..
        } => {
            let project = Path::new(&review.attempt.binding.workspace);
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(project.join("stop-once"))
                .map_err(|e| e.to_string())?;
            if project.join("unknown-stop").exists() || project.join("complete-on-stop").exists() {
                return Err("stop reply lost or completion race".into());
            }
            let mut p = cancelled_proof(observation);
            if project.join("bad-stop").exists() {
                p["turnRuntimeId"] = "foreign-turn".into();
            }
            Ok(ReadReply {
                stop_proof: Some(p),
                ..Default::default()
            })
        }
        ReadRequest::TaskStopRead {
            review,
            observation,
            ..
        } => {
            let project = Path::new(&review.attempt.binding.workspace);
            require(
                project.join("read-confirm").exists(),
                "fixture cancellation remains unknown",
            )?;
            Ok(ReadReply {
                stop_proof: Some(cancelled_proof(observation)),
                ..Default::default()
            })
        }
        ReadRequest::TaskRead { review, .. } => {
            let project = Path::new(&review.attempt.binding.workspace);
            require(
                ["complete-before-stop", "complete-on-stop"]
                    .iter()
                    .any(|p| project.join(p).exists()),
                "fixture completion unavailable",
            )?;
            fs::write(
                project.join("sum.mjs"),
                "export function sum(a,b){return a+b;}\n",
            )
            .map_err(|e| e.to_string())?;
            Ok(ReadReply {
                task_proof: Some(task_proof(review)),
                ..Default::default()
            })
        }
        _ => background_reply(request),
    }
}
pub(super) fn unknown(root: &Path) -> (RuntimeHost, execution::TaskReview) {
    let (mut host, r) = assigned_desktop_task(root);
    let data = host.root.clone();
    assert!(
        execution::dispatch(&data, host.projects.get_mut("project").unwrap(), &r, |_| {
            Err("lost original reply".into())
        })
        .is_err()
    );
    host.refresh_desktop_tasks().unwrap();
    host.desktop_fixture = Some(stop_fixture);
    (host, r)
}
fn cancel(host: &mut RuntimeHost, r: &execution::TaskReview) -> Response {
    host.handle(Request {
        id: "desktop-stop-user".into(),
        protocol_version: PROTOCOL_VERSION,
        command: Command::CancelDesktopTask {
            project_id: r.project_id.clone(),
            attempt_id: r.attempt.id.clone(),
            review_digest: r.id.clone(),
            reason: "用户要求停止当前原任务".into(),
        },
    })
}
#[test]
fn desktop_cancellation_confirms_once_and_reopens_without_resend_or_completion() {
    let root = tempfile::tempdir().unwrap();
    let (mut host, r) = unknown(root.path());
    assert!(host
        .cancel_desktop_task("project", "foreign-attempt", &r.id, "stop")
        .is_err());
    assert!(host
        .cancel_desktop_task("project", &r.attempt.id, &"f".repeat(64), "stop")
        .is_err());
    let response = cancel(&mut host, &r);
    assert!(response.error.is_none(), "{:?}", response.error);
    background_spin(&mut host);
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Cancelled
    );
    assert!(!host.projects["project"].state().tasks["task"].verified);
    assert!(
        host.desktop_access.tasks[0]
            .cancellation
            .as_ref()
            .unwrap()
            .confirmed
    );
    assert!(host.require_desktop_tasks_resolved().is_ok());
    assert!(cancel(&mut host, &r).error.is_some());
    assert!(host.read_desktop_task("project", &r.attempt.id).is_err());
    assert!(host
        .read_desktop_task_with_reader("project", &r.attempt.id, Some(&fingerprint()))
        .is_err());
    let before = fs::read(sealed_directory(root.path()).join("stop-proof.json")).unwrap();
    let data = host.root.clone();
    drop(host);
    let mut host = RuntimeHost::open(&data).unwrap();
    assert!(
        host.desktop_access.tasks[0]
            .cancellation
            .as_ref()
            .unwrap()
            .confirmed
    );
    assert!(execution::recover(
        &data,
        &host.projects["project"],
        "project",
        &r.attempt.id,
        |_| panic!("cancelled task must not read completion")
    )
    .is_err());
    assert!(host
        .cancel_desktop_task("project", &r.attempt.id, &r.id, "again")
        .is_err());
    assert_eq!(
        fs::read(sealed_directory(root.path()).join("stop-proof.json")).unwrap(),
        before
    );
    assert!(!host.projects["project"].history().iter().any(|e| matches!(
        e.body.event.command,
        WorkflowCommand::Submit { .. }
            | WorkflowCommand::Verify { .. }
            | WorkflowCommand::Accept { .. }
    )));
}
#[test]
fn desktop_cancellation_background_request_precedes_observer_release_and_preserves_completed_race()
{
    for completes in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (mut host, r) = assigned_desktop_task(root.path());
        host.desktop_fixture = Some(stop_fixture);
        host.desktop_access.task_review = Some(r.clone());
        if !completes {
            fs::write(root.path().join("project/lost-reply"), "lost").unwrap();
        }
        host.retain_desktop_fixture_owner(r.clone());
        let response = cancel(&mut host, &r);
        assert!(response.error.is_none(), "{:?}", response.error);
        assert!(sealed_directory(root.path())
            .join("stop-request.json")
            .exists());
        assert!(!sealed_directory(root.path())
            .join("stop-intent.json")
            .exists());
        assert!(!host.projects.contains_key("project"));
        assert!(host.desktop_active.as_ref().unwrap().status.stopping);
        assert!(cancel(&mut host, &r).error.is_some());
        fs::write(root.path().join("project/release-send"), "release").unwrap();
        background_spin(&mut host);
        assert_eq!(
            host.projects["project"].state().tasks["task"].state,
            if completes {
                TaskState::Submitted
            } else {
                TaskState::Cancelled
            }
        );
        assert_eq!(root.path().join("project/stop-once").exists(), !completes);
        let cancels = host.projects["project"]
            .history()
            .iter()
            .filter(|e| matches!(e.body.event.command, WorkflowCommand::CancelIntent { .. }))
            .count();
        assert_eq!(cancels, usize::from(!completes));
        assert!(!host.projects["project"].state().tasks["task"].verified);
    }
}
#[test]
fn desktop_cancellation_completion_races_seal_original_result_without_cancelled_claim() {
    for mode in ["complete-before-stop", "complete-on-stop"] {
        let root = tempfile::tempdir().unwrap();
        let (mut host, r) = unknown(root.path());
        fs::write(root.path().join("project").join(mode), "mode").unwrap();
        assert!(cancel(&mut host, &r).error.is_none());
        background_spin(&mut host);
        assert_eq!(
            host.projects["project"].state().tasks["task"].state,
            TaskState::Submitted,
            "{:?}",
            host.desktop_access.error
        );
        assert_eq!(
            root.path().join("project/stop-once").exists(),
            mode == "complete-on-stop"
        );
        assert!(
            !host.desktop_access.tasks[0]
                .cancellation
                .as_ref()
                .unwrap()
                .confirmed
        );
        assert!(!host.projects["project"].state().tasks["task"].verified);
        let data = host.root.clone();
        drop(host);
        let host = RuntimeHost::open(&data).unwrap();
        assert!(host.desktop_access.tasks[0].submitted);
        assert!(!sealed_directory(root.path())
            .join("stop-proof.json")
            .exists());
    }
}
#[test]
fn desktop_cancellation_unknown_and_invalid_proof_recover_readonly_after_reopen() {
    for mode in ["unknown-stop", "bad-stop"] {
        let root = tempfile::tempdir().unwrap();
        let (mut host, r) = unknown(root.path());
        fs::write(root.path().join("project").join(mode), "mode").unwrap();
        assert!(cancel(&mut host, &r).error.is_none());
        background_spin(&mut host);
        assert_eq!(
            host.projects["project"].state().tasks["task"].state,
            TaskState::Unknown
        );
        assert!(host.projects["project"].state().tasks["task"]
            .cancellation_requested
            .unwrap());
        assert!(!sealed_directory(root.path())
            .join("stop-proof.json")
            .exists());
        let intent = fs::read(sealed_directory(root.path()).join("stop-intent.json")).unwrap();
        let data = host.root.clone();
        drop(host);
        let mut host = RuntimeHost::open(&data).unwrap();
        host.desktop_fixture = Some(stop_fixture);
        assert!(cancel(&mut host, &r).error.is_some());
        fs::write(root.path().join("project/read-confirm"), "ready").unwrap();
        host.read_desktop_task("project", &r.attempt.id).unwrap();
        background_spin(&mut host);
        assert_eq!(
            host.projects["project"].state().tasks["task"].state,
            TaskState::Cancelled,
            "{:?}",
            host.desktop_access.error
        );
        assert_eq!(
            fs::read(sealed_directory(root.path()).join("stop-intent.json")).unwrap(),
            intent
        );
    }
}
#[test]
fn desktop_cancellation_request_only_reopen_never_reconstructs_stop_permission() {
    let root = tempfile::tempdir().unwrap();
    let (host, r) = unknown(root.path());
    drop(cancellation::request(&host.root, &r, "interrupted before signal").unwrap());
    let data = host.root.clone();
    drop(host);
    let mut host = RuntimeHost::open(&data).unwrap();
    host.desktop_fixture = Some(stop_fixture);
    assert!(cancel(&mut host, &r).error.is_some());
    host.read_desktop_task("project", &r.attempt.id).unwrap();
    background_spin(&mut host);
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Unknown
    );
    assert!(!root.path().join("project/stop-once").exists());
    assert!(!sealed_directory(root.path())
        .join("stop-intent.json")
        .exists());
}
#[test]
fn desktop_cancellation_rejects_tampered_conflicting_and_partial_evidence() {
    for mode in [
        "partial-request",
        "conflicting-request",
        "missing-request",
        "missing-intent",
        "missing-proof",
        "forged-terminal",
        "wrong-turn",
        "too-fast",
    ] {
        let root = tempfile::tempdir().unwrap();
        let (mut host, r) = unknown(root.path());
        assert!(cancel(&mut host, &r).error.is_none());
        background_spin(&mut host);
        let dir = sealed_directory(root.path());
        match mode {
            "partial-request" => fs::write(dir.join("stop-request.pending"), "{").unwrap(),
            "conflicting-request" => fs::write(dir.join("stop-request.pending"), "{}").unwrap(),
            "missing-request" => fs::remove_file(dir.join("stop-request.json")).unwrap(),
            "missing-intent" => fs::remove_file(dir.join("stop-intent.json")).unwrap(),
            "missing-proof" => fs::remove_file(dir.join("stop-proof.json")).unwrap(),
            _ => {
                let path = dir.join("stop-proof.json");
                let mut v: serde_json::Value =
                    serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                if mode == "forged-terminal" {
                    v["body"]["statusText"] = "状态：成功".into();
                }
                if mode == "wrong-turn" {
                    v["body"]["promptRuntimeId"] = "different".into();
                }
                if mode == "too-fast" {
                    v["body"]["firstObservedAt"] = v["body"]["observation"]["capturedAt"].clone();
                }
                v["hash"] = digest(
                    &serde_json::from_value::<cancellation::StopProof>(v["body"].clone()).unwrap(),
                )
                .unwrap()
                .into();
                fs::write(path, serde_json::to_vec(&v).unwrap()).unwrap();
            }
        }
        assert!(
            execution::inspect(
                &host.root,
                &host.projects["project"],
                "project",
                &r.attempt.id
            )
            .is_err(),
            "{mode}"
        );
    }
}
