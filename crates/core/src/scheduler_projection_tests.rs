use super::*;
use std::time::Duration;

pub(super) fn config() -> SchedulerConfig {
    SchedulerConfig {
        schema_version: 1,
        core_id: "core".into(),
        execution_scope: None,
        principals: [
            ("user", Role::User),
            ("chief", Role::Chief),
            ("deputy", Role::Deputy),
            ("core", Role::Core),
            ("worker", Role::Worker),
        ]
        .into_iter()
        .map(|(id, role)| Principal {
            id: id.into(),
            role,
            provider: matches!(role, Role::Chief | Role::Deputy).then(|| "openai".into()),
        })
        .collect(),
    }
}
pub(super) fn workflow(root: &Path) -> DurableWorkflow {
    let mut s = DurableWorkflow::open(root, config()).unwrap();
    s.command(
        "plan",
        "chief",
        WorkflowCommand::Propose {
            plan: cerulean_domain::workflow::WorkflowPlan {
                id: "plan".into(),
                version: 1,
                tasks: vec![cerulean_domain::workflow::WorkflowTask {
                    id: "a".into(),
                    dependencies: vec![],
                    workspace_key: "space".into(),
                    goal: "projection fault fixture".into(),
                    resource_keys: None,
                }],
            },
        },
        CommandContext::default(),
        false,
    )
    .unwrap();
    s.command(
        "approve",
        "user",
        WorkflowCommand::Approve {
            digest: s.state().digest.clone().unwrap(),
        },
        CommandContext {
            approval_source: Some("local test".into()),
            ..Default::default()
        },
        false,
    )
    .unwrap();
    s.command(
        "assign",
        "deputy",
        WorkflowCommand::Assign {
            task: "a".into(),
            worker: "worker".into(),
            attempt: "try-a".into(),
        },
        CommandContext::default(),
        false,
    )
    .unwrap();
    s.wait_for_projections(Duration::from_secs(20)).unwrap();
    s.wait_for_index(Duration::from_secs(20)).unwrap();
    s
}
fn intent() -> WorkflowCommand {
    WorkflowCommand::DispatchIntent {
        task: "a".into(),
        attempt: "try-a".into(),
    }
}
fn binding() -> CommandContext {
    CommandContext {
        thread_id: Some("session".into()),
        cwd: Some("isolated".into()),
        ..Default::default()
    }
}
fn state(root: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(root.join("state.json")).unwrap()).unwrap()
}

#[test]
fn projection_queue_does_not_block_commit_and_coalesces_to_final_head() {
    let root = tempfile::tempdir().unwrap();
    let mut s = workflow(root.path());
    let before = s.index.test_stats().0;
    let gate = s.index.test_gate();
    let mut boundaries = vec![];
    s.command_observed("dispatch", "core", intent(), binding(), true, |p| {
        boundaries.push(p)
    })
    .unwrap();
    gate.entered();
    assert_eq!(boundaries.last(), Some(&CommitPoint::ProjectionQueued));
    assert_eq!(state(root.path())["sequence"], 3);
    assert_eq!(fs::read_dir(root.path().join("events")).unwrap().count(), 4);
    assert!(s.wait_for_projections(Duration::ZERO).is_err());
    assert!(s.projection_error().is_none());
    assert!(!s.requires_reopen());
    assert!(s.handoffs().unwrap()[0]
        .event_ids
        .contains(&"event-4".into()));
    for n in 0..20 {
        s.command(
            &format!("advice-{n}"),
            "deputy",
            WorkflowCommand::Advise {
                task: "a".into(),
                suggestion: format!("test advice {n}"),
            },
            CommandContext::default(),
            false,
        )
        .unwrap();
    }
    assert_eq!(s.index.test_stats(), (before + 1, Some(24), true));
    gate.release();
    s.wait_for_projections(Duration::from_secs(20)).unwrap();
    s.wait_for_index(Duration::from_secs(20)).unwrap();
    assert_eq!(s.index.test_stats().0, before + 2);
    assert_eq!(state(root.path())["sequence"], 24);
    assert_eq!(
        state(root.path())["lastHash"],
        s.history().last().unwrap().hash
    );
    for v in s.handoffs().unwrap() {
        assert_eq!(
            fs::read_to_string(root.path().join(v.path)).unwrap(),
            v.markdown
        );
    }
    assert!(s
        .command("duplicate", "core", intent(), binding(), true)
        .is_err());
}

#[test]
fn index_failure_does_not_suppress_documents_and_next_snapshot_repairs_both() {
    let root = tempfile::tempdir().unwrap();
    let mut s = workflow(root.path());
    fs::remove_file(root.path().join("index.sqlite")).unwrap();
    fs::create_dir(root.path().join("index.sqlite")).unwrap();
    s.command("dispatch", "core", intent(), binding(), true)
        .unwrap();
    s.wait_for_projections(Duration::from_secs(20)).unwrap();
    assert!(s.wait_for_index(Duration::from_secs(20)).is_err());
    assert!(s.derived_diagnostics().error().is_some());
    assert!(s.projection_error().is_none());
    assert_eq!(state(root.path())["sequence"], 4);
    let original = fs::read(root.path().join("events/000000000004_dispatch.json")).unwrap();
    fs::remove_dir(root.path().join("index.sqlite")).unwrap();
    s.command(
        "ack",
        "core",
        WorkflowCommand::Started {
            task: "a".into(),
            attempt: "try-a".into(),
        },
        CommandContext {
            turn_id: Some("turn".into()),
            ..binding()
        },
        false,
    )
    .unwrap();
    s.wait_for_projections(Duration::from_secs(20)).unwrap();
    s.wait_for_index(Duration::from_secs(20)).unwrap();
    assert!(s.derived_diagnostics().error().is_none());
    assert_eq!(state(root.path())["sequence"], 5);
    assert_eq!(
        fs::read(root.path().join("events/000000000004_dispatch.json")).unwrap(),
        original
    );
}

#[test]
fn daily_failure_keeps_previous_state_marker_and_reopen_rebuilds_without_resend() {
    let root = tempfile::tempdir().unwrap();
    let mut s = workflow(root.path());
    let volume = root.path().join(&s.handoffs().unwrap()[0].path);
    fs::remove_file(&volume).unwrap();
    fs::create_dir(&volume).unwrap();
    s.command("dispatch", "core", intent(), binding(), true)
        .unwrap();
    assert!(s.wait_for_projections(Duration::from_secs(20)).is_err());
    s.wait_for_index(Duration::from_secs(20)).unwrap();
    assert_eq!(state(root.path())["sequence"], 3);
    assert_eq!(s.history().len(), 4);
    assert!(s.derived_diagnostics().error().is_some());
    let events = s.history().to_vec();
    drop(s);
    // A read-only inspection does not rebuild or execute anything.
    #[cfg(windows)]
    {
        let inspected =
            DurableWorkflow::inspect_existing(root.path(), |s| Ok(s.history().to_vec())).unwrap();
        assert_eq!(inspected, events);
        assert!(volume.is_dir());
    }
    fs::remove_dir(&volume).unwrap();
    let mut s = DurableWorkflow::open(root.path(), config()).unwrap();
    s.wait_for_projections(Duration::from_secs(20)).unwrap();
    assert_eq!(&s.history()[..4], events);
    assert_eq!(s.state().tasks["a"].state, TaskState::Unknown);
    assert!(s
        .command("retry", "core", intent(), binding(), true)
        .is_err());
    assert_eq!(state(root.path())["sequence"], 5);
}

#[test]
fn workflow_writer_lock_is_held_until_document_worker_drains() {
    let root = tempfile::tempdir().unwrap();
    let mut s = workflow(root.path());
    let gate = s.index.test_gate();
    s.command("dispatch", "core", intent(), binding(), true)
        .unwrap();
    gate.entered();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let owner = std::thread::spawn(move || {
        started_tx.send(()).unwrap();
        drop(s);
        done_tx.send(()).unwrap();
    });
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(DurableWorkflow::open(root.path(), config()).is_err());
    assert!(done_rx.try_recv().is_err());
    gate.release();
    done_rx.recv_timeout(Duration::from_secs(20)).unwrap();
    owner.join().unwrap();
    assert_eq!(state(root.path())["sequence"], 4);
    let s = DurableWorkflow::open(root.path(), config()).unwrap();
    assert_eq!(s.state().tasks["a"].state, TaskState::Unknown);
}
