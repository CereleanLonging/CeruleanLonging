#![cfg(windows)]

use cerulean_core::scheduler::{CommandContext, CommitPoint, DurableWorkflow, SchedulerConfig};
use cerulean_domain::workflow::{
    Principal, Role, TaskState, WorkflowCommand, WorkflowPlan, WorkflowTask,
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::Write,
    os::windows::fs::OpenOptionsExt,
    path::Path,
};

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
            provider: matches!(&role, Role::Chief | Role::Deputy).then(|| "openai".into()),
            role,
        })
        .collect(),
    }
}

fn prepare(root: &Path) -> DurableWorkflow {
    let mut store = DurableWorkflow::open(root, config()).unwrap();
    store
        .command(
            "plan",
            "chief",
            WorkflowCommand::Propose {
                plan: WorkflowPlan {
                    id: "plan".into(),
                    version: 1,
                    tasks: vec![WorkflowTask {
                        id: "task".into(),
                        dependencies: vec![],
                        workspace_key: "isolated".into(),
                        goal: "test event publication failure".into(),
                        resource_keys: None,
                    }],
                },
            },
            CommandContext::default(),
            false,
        )
        .unwrap();
    store
        .command(
            "approve",
            "user",
            WorkflowCommand::Approve {
                digest: store.state().digest.clone().unwrap(),
            },
            CommandContext {
                approval_source: Some("isolated stage 298 test".into()),
                ..Default::default()
            },
            false,
        )
        .unwrap();
    store
        .command(
            "assign",
            "deputy",
            WorkflowCommand::Assign {
                task: "task".into(),
                worker: "worker".into(),
                attempt: "attempt".into(),
            },
            CommandContext::default(),
            false,
        )
        .unwrap();
    store
}

fn intent() -> WorkflowCommand {
    WorkflowCommand::DispatchIntent {
        task: "task".into(),
        attempt: "attempt".into(),
    }
}

fn binding() -> CommandContext {
    CommandContext {
        thread_id: Some("session".into()),
        cwd: Some("isolated".into()),
        ..Default::default()
    }
}

fn committed_events(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(root.join("events"))
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            let name = entry.file_name().into_string().unwrap();
            (name, fs::read(entry.path()).unwrap())
        })
        .filter(|(name, _)| name.ends_with(".json"))
        .collect()
}

#[test]
fn event_publish_failure_poison_and_corrupt_startup_requires_explicit_recovery() {
    let reports = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports/p1-ledger-publish-298");
    fs::create_dir_all(&reports).unwrap();
    let case = tempfile::Builder::new()
        .prefix("held-destination-")
        .tempdir_in(&reports)
        .unwrap()
        .keep();
    let root = case.join("ledger");
    let mut store = prepare(&root);
    store
        .wait_for_projections(std::time::Duration::from_secs(20))
        .unwrap();
    let before = committed_events(&root);
    let state_before = fs::read(root.join("state.json")).unwrap();
    assert_eq!(before.len(), 3);
    assert_eq!(store.state().tasks["task"].state, TaskState::Assigned);

    let destination = root.join("events/000000000004_dispatch.json");
    let mut held: Option<File> = None;
    let mut temp_synced = false;
    let error = store
        .command_observed("dispatch", "core", intent(), binding(), true, |point| {
            if point == CommitPoint::TempSynced {
                assert!(!temp_synced);
                temp_synced = true;
                // Real Windows file sharing blocks the subsequent rename. The
                // file also models a corrupt committed-looking journal entry.
                let mut file = OpenOptions::new()
                    .create_new(true)
                    .read(true)
                    .write(true)
                    .share_mode(3) // read/write shared; DELETE deliberately not shared
                    .open(&destination)
                    .unwrap();
                file.write_all(b"{corrupt event").unwrap();
                file.sync_all().unwrap();
                held = Some(file);
            }
        })
        .unwrap_err();
    assert!(temp_synced && held.is_some(), "fault reached real rename");
    assert!(error.contains("os error"), "{error}");
    assert_eq!(store.history().len(), 3);
    assert_eq!(fs::read(&destination).unwrap(), b"{corrupt event");
    assert_eq!(fs::read(root.join("state.json")).unwrap(), state_before);
    assert!(store
        .command("retry-before-reopen", "core", intent(), binding(), true)
        .unwrap_err()
        .contains("workflow requires reopen"));
    drop(held.take());
    drop(store);

    let corrupt_open_error = DurableWorkflow::open(&root, config())
        .err()
        .expect("corrupt published-looking event must block startup");
    assert!(!corrupt_open_error.is_empty());
    assert_eq!(fs::read(&destination).unwrap(), b"{corrupt event");
    for (name, bytes) in &before {
        assert_eq!(fs::read(root.join("events").join(name)).unwrap(), *bytes);
    }
    assert_eq!(fs::read(root.join("state.json")).unwrap(), state_before);

    // Only the file created by this test is removed. The synced orphan temp
    // remains, so reopening also checks that it is never mistaken for an event.
    fs::remove_file(&destination).unwrap();
    let temps: Vec<_> = fs::read_dir(root.join("events"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "tmp"))
        .collect();
    assert_eq!(temps.len(), 1);
    assert!(!fs::read(&temps[0]).unwrap().is_empty());
    let mut reopened = DurableWorkflow::open(&root, config()).unwrap();
    assert_eq!(reopened.history().len(), 3);
    assert_eq!(reopened.state().tasks["task"].state, TaskState::Assigned);
    assert_eq!(committed_events(&root), before);
    assert_eq!(fs::read(root.join("state.json")).unwrap(), state_before);

    let published = reopened
        .command("dispatch", "core", intent(), binding(), true)
        .unwrap();
    assert_eq!(published.body.event.sequence, 4);
    assert!(reopened
        .command("automatic-duplicate", "core", intent(), binding(), true)
        .is_err());
    drop(reopened);
    let reopened = DurableWorkflow::open(&root, config()).unwrap();
    assert_eq!(reopened.state().tasks["task"].state, TaskState::Unknown);
    assert_eq!(
        reopened
            .history()
            .iter()
            .filter(|entry| matches!(
                entry.body.event.command,
                WorkflowCommand::DispatchIntent { .. }
            ))
            .count(),
        1
    );
    for (name, bytes) in &before {
        assert_eq!(fs::read(root.join("events").join(name)).unwrap(), *bytes);
    }

    let result = json!({
        "stage": 298,
        "boundary": "Windows held destination prevents event rename after temp sync",
        "renameError": error,
        "corruptOpenError": corrupt_open_error,
        "originalCommittedEventsUnchanged": true,
        "failedIntentNotCommitted": true,
        "writerPoisonedUntilReopen": true,
        "corruptEventRejected": true,
        "orphanTempIgnored": true,
        "explicitRetryDispatchIntents": 1,
        "reopenedTaskState": "unknown",
        "modelCalls": 0,
        "networkCalls": 0,
        "physicalDiskFullOrPowerLossVerified": false
    });
    fs::write(
        case.join("result.json"),
        serde_json::to_vec_pretty(&result).unwrap(),
    )
    .unwrap();
    println!("STAGE_298_CASE={}", case.display());
}
