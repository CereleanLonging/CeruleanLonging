use super::*;
use crate::desktop_execution as execution;

pub(super) fn unknown(root: &Path) -> (RuntimeHost, String) {
    let mut host = configured(root, "LOST_REPLY");
    plan_and_approve(&mut host);
    let attempt = assigned(&mut host);
    assert!(host.desktop_retirement_digest("project", &attempt).is_err());
    let review = prepared(&mut host, &attempt);
    project_planning::tests::retain_legacy_run(&mut host);
    retain_send(&mut host, &review);
    assert!(host.desktop_retirement_digest("project", &attempt).is_err());
    until(&mut host, |h| {
        h.runs.all()[0].state == RunState::Attention && h.desktop_active.is_none()
    });
    (host, attempt)
}
fn retire(host: &mut RuntimeHost, attempt: &str, hash: &str, reason: &str) -> Response {
    host.handle(Request {
        id: "desktop-disposition".into(),
        protocol_version: PROTOCOL_VERSION,
        command: Command::RetireUnknownDesktop {
            project_id: "project".into(),
            attempt_id: attempt.into(),
            evidence_digest: hash.into(),
            reason: reason.into(),
        },
    })
}
#[test]
fn desktop_retirement_is_user_only_preserves_unknown_outcome_and_allows_explicit_revision() {
    let root = tempfile::tempdir().unwrap();
    let (mut host, attempt) = unknown(root.path());
    let old = host.snapshot().desktop_access.unwrap().tasks[0].clone();
    let hash = old.retirement_digest.unwrap();
    let bytes = fs::read(root.path().join("project/sum.mjs")).unwrap();
    let history = host.projects["project"].history().to_vec();
    assert!(retire(&mut host, &attempt, &"0".repeat(64), "reason")
        .error
        .is_some());
    assert!(retire(&mut host, &attempt, &hash, " ").error.is_some());
    for principal in ["core", "chief", "deputy", "worker-qoder"] {
        assert!(host
            .projects
            .get_mut("project")
            .unwrap()
            .command(
                "forbidden-retirement",
                principal,
                WorkflowCommand::RetireUnknownDesktop {
                    task: "task".into(),
                    attempt: attempt.clone(),
                    evidence_digest: hash.clone(),
                    reason: "reason".into()
                },
                CommandContext::default(),
                false
            )
            .is_err());
    }
    assert!(host
        .projects
        .get_mut("project")
        .unwrap()
        .command(
            "wrong-adapter",
            "user",
            WorkflowCommand::RetireUnknownApi {
                task: "task".into(),
                attempt: attempt.clone(),
                reason: "reason".into()
            },
            CommandContext::default(),
            false
        )
        .is_err());
    assert_eq!(host.projects["project"].history(), history);
    let run_id = host.runs.all()[0].id.clone();
    host.pause_project_run(&run_id).unwrap();
    assert!(retire(&mut host, &attempt, &hash, "reason").error.is_some());
    let hash = host.desktop_retirement_digest("project", &attempt).unwrap();
    host.desktop_fixture = Some(|_| panic!("retirement must not touch the external app"));
    let response = retire(
        &mut host,
        &attempt,
        &hash,
        "自行处理外部应用后结束本地跟踪，保留未知结果和修改",
    );
    assert!(response.error.is_none(), "{:?}", response.error);
    assert!(retire(&mut host, &attempt, &hash, "duplicate")
        .error
        .is_some());
    let store = &host.projects["project"];
    assert_eq!(&store.history()[..history.len()], history);
    assert_eq!(store.state().tasks["task"].state, TaskState::Retired);
    assert!(!store.state().tasks["task"].verified);
    assert!(store.state().tasks["task"].cancellation_requested != Some(true));
    assert_eq!(host.management.statuses().len(), 2);
    assert!(host.jobs.is_empty());
    assert_eq!(
        fs::read(root.path().join("project/sum.mjs")).unwrap(),
        bytes
    );
    assert!(host.read_desktop_task("project", &attempt).is_err());
    assert!(host
        .read_desktop_task_with_reader("project", &attempt, Some(&fingerprint()))
        .is_err());
    assert!(execution::recover_and_submit(
        &host.root,
        host.projects.get_mut("project").unwrap(),
        "project",
        &attempt,
        |_| panic!("no late callback")
    )
    .is_err());
    assert!(host.require_desktop_tasks_resolved().is_ok());
    drop(host);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    attach(&mut host);
    let task = &host.snapshot().desktop_access.unwrap().tasks[0];
    assert!(
        task.retired && !task.submitted && task.proof.is_none() && task.retirement_digest.is_none()
    );
    let revision = host
        .snapshot()
        .project_revisions
        .into_iter()
        .find(|r| r.project_id == "project")
        .unwrap();
    assert_eq!(revision.retired_tasks, 1);
    assert_eq!(revision.cancelled_tasks, 0);
    host.request_project_revision(
        "project",
        &revision.evidence_digest,
        "重新观察聊天并规划剩余工作",
    )
    .unwrap();
    assert!(host.projects["project"]
        .state()
        .revision_requested
        .is_some());
    assert!(host.read_desktop_task("project", &attempt).is_err());
    assert!(host
        .read_desktop_task_with_reader("project", &attempt, Some(&fingerprint()))
        .is_err());
    drop(host);
    let host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert!(host.snapshot().desktop_access.unwrap().tasks[0].retired);
    assert_eq!(
        fs::read(root.path().join("project/sum.mjs")).unwrap(),
        bytes
    );
}
#[test]
fn desktop_retirement_review_expires_when_readback_produces_a_real_submission() {
    let root = tempfile::tempdir().unwrap();
    let (mut host, attempt) = unknown(root.path());
    let hash = host.desktop_retirement_digest("project", &attempt).unwrap();
    host.read_desktop_task("project", &attempt).unwrap();
    assert!(retire(&mut host, &attempt, &hash, "racing readback")
        .error
        .is_some());
    background_spin(&mut host);
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Submitted
    );
    assert!(retire(&mut host, &attempt, &hash, "stale proof")
        .error
        .is_some());
    assert!(host.snapshot().desktop_access.unwrap().tasks[0]
        .retirement_digest
        .is_none());
    assert!(host.jobs.is_empty());
    assert_eq!(host.runs.all()[0].state, RunState::Attention);
    assert_new_run_blocked(&mut host);
    assert_eq!(host.runs.all()[0].state, RunState::Attention);
    assert_eq!(host.management.statuses().len(), 2);
    assert_eq!(
        fs::read_dir(root.path().join("project"))
            .unwrap()
            .filter_map(Result::ok)
            .filter(|p| p.file_name().to_string_lossy().starts_with("sent-once-"))
            .count(),
        1
    );
}
#[test]
fn desktop_retirement_rejects_changed_or_partial_dispatch_evidence() {
    let root = tempfile::tempdir().unwrap();
    let (mut host, attempt) = unknown(root.path());
    let hash = host.desktop_retirement_digest("project", &attempt).unwrap();
    let dir = host
        .root
        .join("desktop-task-dispatch-v1")
        .join(digest(&("project", &attempt)).unwrap());
    fs::write(dir.join("proof.pending"), b"{").unwrap();
    assert!(retire(&mut host, &attempt, &hash, "damaged evidence")
        .error
        .is_some());
    assert!(host.snapshot().desktop_access.unwrap().tasks[0]
        .retirement_digest
        .is_none());
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Unknown
    );
    assert_eq!(host.management.statuses().len(), 2);
    drop(host);
    assert!(RuntimeHost::open(&root.path().join("data")).is_err());
}

#[test]
fn desktop_retirement_preserves_interrupted_seals_without_recapturing_or_late_submission() {
    for stop in [
        "proof-published",
        "seal-reserved",
        "artifacts-captured",
        "completion-published",
    ] {
        let root = tempfile::tempdir().unwrap();
        let (mut host, review) = assigned_desktop_task(root.path());
        let data = host.root.clone();
        assert!(execution::dispatch_and_seal_observed(
            &data,
            host.projects.get_mut("project").unwrap(),
            &review,
            |r| {
                write_sum(root.path(), true);
                Ok(task_proof(r))
            },
            |at| if at == stop {
                Err("injected interruption".into())
            } else {
                Ok(())
            }
        )
        .is_err());
        drop(host);
        let mut host = RuntimeHost::open(&data).unwrap();
        let dir = sealed_directory(root.path());
        let proof = fs::read(dir.join("proof.json")).unwrap();
        let stale = host
            .desktop_retirement_digest("project", "desktop-attempt")
            .unwrap();
        // A complete pending publication is readable, but changes review evidence.
        fs::write(dir.join("proof.pending"), &proof).unwrap();
        assert!(
            retire(&mut host, "desktop-attempt", &stale, "old file inventory")
                .error
                .is_some()
        );
        let hash = host
            .desktop_retirement_digest("project", "desktop-attempt")
            .unwrap();
        assert_ne!(hash, stale);
        let original: std::collections::BTreeMap<_, _> = fs::read_dir(&dir)
            .unwrap()
            .map(|p| {
                let p = p.unwrap().path();
                (p.file_name().unwrap().to_owned(), fs::read(p).unwrap())
            })
            .collect();
        write_sum(root.path(), false);
        let bytes = fs::read(root.path().join("project/sum.mjs")).unwrap();
        assert!(retire(
            &mut host,
            "desktop-attempt",
            &hash,
            "保留中断证据与后续修改，结束本地跟踪"
        )
        .error
        .is_none());
        for (name, content) in &original {
            assert_eq!(&fs::read(dir.join(name)).unwrap(), content);
        }
        assert_eq!(
            fs::read(root.path().join("project/sum.mjs")).unwrap(),
            bytes
        );
        assert!(execution::submit_sealed(
            &data,
            host.projects.get_mut("project").unwrap(),
            "project",
            "desktop-attempt"
        )
        .is_err());
        assert!(execution::recover_and_submit(
            &data,
            host.projects.get_mut("project").unwrap(),
            "project",
            "desktop-attempt",
            |_| panic!("must not read or recapture")
        )
        .is_err());
        drop(host);
        let host = RuntimeHost::open(&data).unwrap();
        let task = &host.snapshot().desktop_access.unwrap().tasks[0];
        assert!(task.retired && !task.submitted && task.proof.is_some());
        assert_eq!(
            fs::read(root.path().join("project/sum.mjs")).unwrap(),
            bytes
        );
    }
}
