use super::*;
use crate::{
    scheduler::{CommandContext, DurableWorkflow, SchedulerConfig},
    Store,
};
use cerulean_domain::{workflow::*, Command};
use std::collections::BTreeMap;

fn base_open(root: &Path) -> Result<Store, String> {
    let store = Store::open(root)?;
    let _ = store.wait_for_index(std::time::Duration::from_secs(20));
    Ok(store)
}
fn workflow_open(root: &Path, configuration: SchedulerConfig) -> Result<DurableWorkflow, String> {
    let store = DurableWorkflow::open(root, configuration)?;
    let _ = store.wait_for_index(std::time::Duration::from_secs(20));
    Ok(store)
}
fn config() -> SchedulerConfig {
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
fn workflow(root: &Path) -> DurableWorkflow {
    let mut s = workflow_open(root, config()).unwrap();
    s.command(
        "plan",
        "chief",
        WorkflowCommand::Propose {
            plan: WorkflowPlan {
                id: "plan".into(),
                version: 1,
                tasks: vec![WorkflowTask {
                    id: "a".into(),
                    dependencies: vec![],
                    workspace_key: "isolated".into(),
                    goal: "test".into(),
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
            approval_source: Some("local index test".into()),
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
    s.wait_for_index(std::time::Duration::from_secs(20))
        .unwrap();
    s
}
fn intent() -> WorkflowCommand {
    WorkflowCommand::DispatchIntent {
        task: "a".into(),
        attempt: "try-a".into(),
    }
}
fn context() -> CommandContext {
    CommandContext {
        thread_id: Some("session".into()),
        cwd: Some("isolated".into()),
        ..Default::default()
    }
}
fn event_bytes(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(root.join("events"))
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            (
                e.file_name().into_string().unwrap(),
                fs::read(e.path()).unwrap(),
            )
        })
        .collect()
}
fn create_plan(s: &mut Store) {
    s.execute(Command::CreatePlan {
        title: "index test".into(),
        tasks: vec!["write code".into()],
    })
    .unwrap();
    let _ = s.wait_for_index(std::time::Duration::from_secs(20));
}

#[test]
fn both_ledgers_rebuild_missing_corrupt_stale_ahead_schema_and_modified_cache() {
    let parent = tempfile::tempdir().unwrap();
    let sr = parent.path().join("store");
    let wr = parent.path().join("workflow");
    let mut s = base_open(&sr).unwrap();
    let empty_s = fs::read(sr.join("index.sqlite")).unwrap();
    create_plan(&mut s);
    let plan = s.snapshot().plans[0].clone();
    s.execute(Command::ApprovePlan {
        id: plan.id,
        digest: plan.digest,
    })
    .unwrap();
    s.wait_for_index(std::time::Duration::from_secs(20))
        .unwrap();
    assert!(s.snapshot().plans[0].approved);
    assert!(s.index_error().is_none(), "{:?}", s.index_error());
    let initial = workflow_open(&wr, config()).unwrap();
    let empty_w = fs::read(wr.join("index.sqlite")).unwrap();
    drop(initial);
    let w = workflow(&wr);
    assert!(w.index_error().is_none(), "{:?}", w.index_error());
    let original_daily = w.handoffs().unwrap();
    let original_s = event_bytes(&sr);
    let original_w = event_bytes(&wr);
    drop((s, w));
    for fault in [
        "missing", "corrupt", "stale", "ahead", "schema", "row", "column",
    ] {
        for (root, empty) in [(&sr, &empty_s), (&wr, &empty_w)] {
            let path = root.join("index.sqlite");
            match fault {
                "missing" => fs::remove_file(path).unwrap(),
                "corrupt" => fs::write(path, b"broken sqlite").unwrap(),
                "stale" => fs::write(path, empty).unwrap(),
                _ => {
                    let db = Connection::open(path).unwrap();
                    db.execute_batch(match fault {
                        "ahead" => "UPDATE meta SET head=json_set(head,'$.count',999999)",
                        "schema" => "UPDATE meta SET schema_version=999",
                        "row" => "UPDATE events SET payload=json_set(payload,'$.kind','forged') WHERE sequence=1",
                        _ => "UPDATE events SET kind='forged' WHERE sequence=1",
                    }).unwrap();
                }
            }
        }
        let s = base_open(&sr).unwrap();
        let w = workflow_open(&wr, config()).unwrap();
        assert!(s.index_error().is_none(), "{fault}: {:?}", s.index_error());
        assert!(w.index_error().is_none(), "{fault}: {:?}", w.index_error());
        assert!(s.snapshot().plans[0].approved);
        assert_eq!(w.handoffs().unwrap(), original_daily);
        assert_eq!(event_bytes(&sr), original_s);
        assert_eq!(event_bytes(&wr), original_w);
    }
}

#[test]
fn live_corruption_falls_back_without_returning_forged_state_or_daily() {
    let root = tempfile::tempdir().unwrap();
    let mut s = base_open(root.path()).unwrap();
    create_plan(&mut s);
    let connection = Connection::open(root.path().join("index.sqlite")).unwrap();
    connection
        .execute_batch("UPDATE plans SET payload=json_set(payload,'$.approved',1),approved=1")
        .unwrap();
    drop(connection);
    assert!(s.index_error().is_some());
    assert!(!s.snapshot().plans[0].approved);
    let wr = tempfile::tempdir().unwrap();
    let w = workflow(wr.path());
    let expected = w.handoffs().unwrap();
    let connection = Connection::open(wr.path().join("index.sqlite")).unwrap();
    connection
        .execute_batch("UPDATE daily_volumes SET payload=json_set(payload,'$.markdown','forged')")
        .unwrap();
    drop(connection);
    assert!(w.index_error().is_some());
    assert_eq!(w.handoffs().unwrap(), expected);
}

#[cfg(windows)]
#[test]
fn held_cache_does_not_undo_event_or_allow_duplicate_dispatch() {
    use std::os::windows::fs::OpenOptionsExt;
    let base = tempfile::tempdir().unwrap();
    let mut s = base_open(base.path()).unwrap();
    let held_base = fs::OpenOptions::new()
        .read(true)
        .share_mode(3)
        .open(base.path().join("index.sqlite"))
        .unwrap();
    create_plan(&mut s);
    assert!(s.index_error().is_some());
    assert_eq!(s.snapshot().plans.len(), 1);
    let overview = s.workspace_overview(
        cerulean_domain::workspace::WorkspaceMode::Ready,
        None,
        Some(0),
    );
    assert!(overview
        .message
        .as_ref()
        .is_some_and(|message| message.contains("索引")));
    drop((held_base, s));
    let s = base_open(base.path()).unwrap();
    assert!(s.index_error().is_none());
    assert_eq!(s.snapshot().events.len(), 1);
    let root = tempfile::tempdir().unwrap();
    let mut w = workflow(root.path());
    let held = fs::OpenOptions::new()
        .read(true)
        .share_mode(3)
        .open(root.path().join("index.sqlite"))
        .unwrap();
    let before = fs::read(root.path().join("index.sqlite")).unwrap();
    w.command("dispatch", "core", intent(), context(), true)
        .unwrap();
    assert!(w
        .wait_for_index(std::time::Duration::from_secs(20))
        .is_err());
    assert!(w.index_error().is_some());
    assert_eq!(w.state().tasks["a"].state, TaskState::Dispatching);
    assert_eq!(fs::read(root.path().join("index.sqlite")).unwrap(), before);
    assert!(w
        .command("dispatch-again", "core", intent(), context(), true)
        .is_err());
    assert!(!w.handoffs().unwrap().is_empty());
    drop((held, w));
    let w = workflow_open(root.path(), config()).unwrap();
    assert!(w.index_error().is_none(), "{:?}", w.index_error());
    assert_eq!(w.state().tasks["a"].state, TaskState::Unknown);
    assert_eq!(
        w.history()
            .iter()
            .filter(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. }))
            .count(),
        1
    );
    assert!(!fs::read_dir(root.path()).unwrap().any(|e| e
        .unwrap()
        .file_name()
        .to_string_lossy()
        .ends_with(".sqlite.tmp")));
}

#[cfg(windows)]
#[test]
fn readonly_inspection_does_not_create_or_repair_cache() {
    let root = tempfile::tempdir().unwrap();
    drop(workflow(root.path()));
    fs::remove_file(root.path().join("index.sqlite")).unwrap();
    let before = event_bytes(root.path());
    DurableWorkflow::inspect_existing(root.path(), |w| {
        assert!(!w.handoffs()?.is_empty());
        assert!(w.index_error().is_some());
        Ok(())
    })
    .unwrap();
    assert!(!root.path().join("index.sqlite").exists());
    assert_eq!(event_bytes(root.path()), before);
}

#[test]
fn unsafe_paths_and_unknown_sidecars_are_preserved_and_never_opened() {
    for name in [
        "index.sqlite",
        "index.sqlite-wal",
        "index.sqlite-shm",
        "index.sqlite-journal",
    ] {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join(name)).unwrap();
        fs::write(root.path().join(name).join("keep"), b"keep").unwrap();
        let mut s = base_open(root.path()).unwrap();
        create_plan(&mut s);
        assert!(s.index_error().is_some());
        assert_eq!(s.snapshot().plans.len(), 1);
        assert_eq!(
            fs::read(root.path().join(name).join("keep")).unwrap(),
            b"keep"
        );
    }
    let root = tempfile::tempdir().unwrap();
    let sidecar = root.path().join("index.sqlite-journal");
    fs::write(&sidecar, b"unknown recovery state").unwrap();
    let s = base_open(root.path()).unwrap();
    assert!(s.index_error().is_some());
    assert_eq!(fs::read(sidecar).unwrap(), b"unknown recovery state");
}

#[cfg(windows)]
#[test]
fn hardlink_cache_is_rejected_without_changing_other_file() {
    let root = tempfile::tempdir().unwrap();
    let outside = root.path().join("outside.sqlite");
    let data = root.path().join("data");
    fs::create_dir(&data).unwrap();
    fs::write(&outside, b"do not follow").unwrap();
    fs::hard_link(&outside, data.join("index.sqlite")).unwrap();
    let mut s = base_open(&data).unwrap();
    create_plan(&mut s);
    assert!(s.index_error().is_some());
    assert_eq!(fs::read(&outside).unwrap(), b"do not follow");
}

#[cfg(windows)]
#[test]
fn cold_backup_restore_retains_events_and_reopens_both_indexes() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let mut s = base_open(&source).unwrap();
    create_plan(&mut s);
    drop(s);
    let wr = source.join("workflows/example");
    drop(workflow(&wr));
    let original = event_bytes(&source);
    let original_w = event_bytes(&wr);
    let archive = root.path().join("archive");
    let review = crate::backup::create(&source, &archive).unwrap();
    // Product restores are intentionally bound to the original canonical root.
    // Preserve this test's source instead of weakening that protection.
    let preserved = root.path().join("source-preserved");
    fs::rename(&source, &preserved).unwrap();
    let restored = source;
    crate::backup::restore(&archive, &restored, &review.manifest_digest).unwrap();
    assert_eq!(
        fs::read(preserved.join("index.sqlite")).unwrap(),
        fs::read(restored.join("index.sqlite")).unwrap()
    );
    let s = base_open(&restored).unwrap();
    let w = workflow_open(&restored.join("workflows/example"), config()).unwrap();
    assert!(s.index_error().is_none());
    assert!(w.index_error().is_none());
    assert_eq!(event_bytes(&restored), original);
    assert_eq!(event_bytes(&restored.join("workflows/example")), original_w);
}

#[test]
fn cross_day_unfinished_includes_unknown_and_excludes_future_tasks() {
    let root = tempfile::tempdir().unwrap();
    let mut w = workflow(root.path());
    w.command("dispatch", "core", intent(), context(), true)
        .unwrap();
    drop(w);
    let w = workflow_open(root.path(), config()).unwrap();
    let tomorrow = (chrono::Utc::now() + chrono::Duration::days(2))
        .format("%Y-%m-%d")
        .to_string();
    let tasks = w.unfinished_before(&tomorrow).unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].state, "unknown");
    assert!(w.unfinished_before("2000-01-01").unwrap().is_empty());
    assert!(w.unfinished_before("2026-1-1").is_err());
    fs::remove_file(root.path().join("index.sqlite")).unwrap();
    assert_eq!(w.unfinished_before(&tomorrow).unwrap(), tasks);
}

#[test]
fn legacy_task_day_keeps_fixed_shanghai_offset_during_historical_dst() {
    let root = tempfile::tempdir().unwrap();
    drop(workflow(root.path()));
    let mut files: Vec<_> = fs::read_dir(root.path().join("events"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    let times = [
        "1988-07-01T15:30:00Z",
        "1988-07-01T16:10:00Z",
        "1988-07-01T16:20:00Z",
    ];
    let mut previous = "genesis".to_string();
    for (path, at) in files.iter().zip(times) {
        let mut event: crate::scheduler::Entry =
            serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        event.body.received_at = at.into();
        event.body.previous_hash = previous;
        event.hash = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&event.body).unwrap())
        );
        previous = event.hash.clone();
        fs::write(path, serde_json::to_vec(&event).unwrap()).unwrap();
    }
    let w = workflow_open(root.path(), config()).unwrap();
    assert!(w.index_error().is_none());
    let tasks = w.unfinished_before("1988-07-02").unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].first_day, "1988-07-01");
    assert_eq!(w.handoffs().unwrap()[0].day, "1988-07-02");
}

#[cfg(windows)]
#[test]
fn junction_cache_path_cannot_write_into_another_directory() {
    use std::os::windows::process::CommandExt;
    let root = tempfile::tempdir().unwrap();
    let outside = root.path().join("outside");
    let data = root.path().join("data");
    fs::create_dir(&outside).unwrap();
    fs::create_dir(&data).unwrap();
    fs::write(outside.join("keep"), b"unchanged").unwrap();
    let junction = data.join("index.sqlite");
    let result = std::process::Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(&junction)
        .arg(&outside)
        .creation_flags(0x08000000)
        .output()
        .unwrap();
    assert!(result.status.success(), "junction fixture creation failed");
    let mut s = base_open(&data).unwrap();
    create_plan(&mut s);
    assert!(s.index_error().is_some());
    assert_eq!(fs::read(outside.join("keep")).unwrap(), b"unchanged");
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
    fs::remove_dir(&junction).unwrap();
}

#[cfg(windows)]
#[test]
#[ignore = "explicit local compatibility check against the frozen 297 executable; zero model calls"]
fn frozen_pre_index_reader_appends_and_new_reader_repairs_only_the_cache() {
    use std::{
        io::Write,
        os::windows::process::CommandExt,
        process::{Command as Process, Stdio},
    };
    let binary = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../reports/p6-windows-package-0120-20261023/cerulean-runtime.exe");
    assert_eq!(
        format!("{:x}", Sha256::digest(fs::read(&binary).unwrap())),
        "ce27930d80a2d46d7dab9344048015308ce362f24e15356546881110302b4764"
    );
    let root = tempfile::tempdir().unwrap();
    let mut s = base_open(root.path()).unwrap();
    create_plan(&mut s);
    drop(s);
    let cache_before = fs::read(root.path().join("index.sqlite")).unwrap();
    let events_before = event_bytes(root.path());
    let mut child = Process::new(binary)
        .arg("--stdio")
        .arg("--data-dir")
        .arg(root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(0x08000000)
        .spawn()
        .unwrap();
    let request = cerulean_domain::Request {
        id: "old-reader-write".into(),
        protocol_version: cerulean_domain::PROTOCOL_VERSION,
        command: Command::CreatePlan {
            title: "written by frozen reader".into(),
            tasks: vec!["no model task".into()],
        },
    };
    let mut input = child.stdin.take().unwrap();
    serde_json::to_writer(&mut input, &request).unwrap();
    input.write_all(b"\n").unwrap();
    drop(input);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let response: cerulean_domain::Response = serde_json::from_slice(&output.stdout).unwrap();
    assert!(response.error.is_none());
    assert_eq!(
        fs::read(root.path().join("index.sqlite")).unwrap(),
        cache_before
    );
    let after = event_bytes(root.path());
    assert_eq!(after.len(), events_before.len() + 1);
    for (name, bytes) in events_before {
        assert_eq!(after[&name], bytes);
    }
    let s = base_open(root.path()).unwrap();
    assert!(s.index_error().is_none());
    assert_eq!(s.snapshot().plans.len(), 2);
    assert_eq!(event_bytes(root.path()), after);
    assert_ne!(
        fs::read(root.path().join("index.sqlite")).unwrap(),
        cache_before
    );
}
