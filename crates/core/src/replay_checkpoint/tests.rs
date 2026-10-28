use super::*;
use crate::{
    scheduler::{CommandContext, DurableWorkflow, SchedulerConfig},
    Store,
};
use cerulean_domain::{workflow::*, Command};
use std::time::Duration;

fn append(s: &mut Store, n: usize) {
    s.execute_command(Command::CreatePlan {
        title: format!("plan-{n}"),
        tasks: vec!["isolated".into()],
    })
    .unwrap();
}
fn seeded(root: &Path) {
    let mut s = Store::open(root).unwrap();
    for n in 0..PERIOD {
        append(&mut s, n);
    }
    drop(s);
    assert!(root.join(FILE).is_file());
}
fn paths(root: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(root.join("events"))
        .unwrap()
        .map(|p| p.unwrap().path())
        .collect();
    paths.sort();
    paths
}

#[test]
fn authenticated_prefix_restores_tail_and_idempotent_approval_without_new_event() {
    let root = tempfile::tempdir().unwrap();
    let mut s = Store::open(root.path()).unwrap();
    for n in 0..PERIOD {
        append(&mut s, n);
    }
    let plan = s.snapshot().plans[0].clone();
    s.execute_command(Command::ApprovePlan {
        id: plan.id.clone(),
        digest: plan.digest.clone(),
    })
    .unwrap();
    let expected = s.snapshot();
    drop(s);
    let mut s = Store::open(root.path()).unwrap();
    assert_eq!(s.checkpoint_events(), PERIOD);
    assert_eq!(s.snapshot().plans, expected.plans);
    assert_eq!(s.snapshot().events, expected.events);
    s.execute_command(Command::ApprovePlan {
        id: plan.id.clone(),
        digest: plan.digest,
    })
    .unwrap();
    assert_eq!(s.snapshot().events, expected.events);
    assert!(s
        .execute_command(Command::ApprovePlan {
            id: plan.id,
            digest: "stale".into()
        })
        .is_err());
}

#[test]
fn corrupt_cache_falls_back_but_changed_or_missing_events_still_fail() {
    let root = tempfile::tempdir().unwrap();
    seeded(root.path());
    let saved = fs::read(root.path().join(FILE)).unwrap();
    fs::write(root.path().join(FILE), b"untrusted snapshot").unwrap();
    let s = Store::open(root.path()).unwrap();
    assert_eq!(s.checkpoint_events(), 0);
    assert_eq!(s.snapshot().plans.len(), PERIOD);
    drop(s);
    fs::write(root.path().join(FILE), &saved).unwrap();
    let first = paths(root.path())[0].clone();
    let event = fs::read(&first).unwrap();
    fs::write(&first, b"corrupt original event").unwrap();
    assert!(Store::open(root.path()).is_err());
    fs::write(&first, &event).unwrap();
    fs::remove_file(paths(root.path())[17].clone()).unwrap();
    assert!(Store::open(root.path()).is_err());
}

#[test]
fn changed_executable_binding_and_moved_root_force_full_replay() {
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a");
    let b = root.path().join("b");
    seeded(&a);
    let p = load(&a, "store", "", &paths(&a)).unwrap();
    let other = format!("{}/other-executable", identity(&a).unwrap());
    let plain = serde_json::to_vec(&Envelope {
        schema: 1,
        identity: other.clone(),
        publication: p,
    })
    .unwrap();
    fs::write(
        a.join(FILE),
        crypt(&plain, other.as_bytes(), false).unwrap(),
    )
    .unwrap();
    let s = Store::open(&a).unwrap();
    assert_eq!(s.checkpoint_events(), 0);
    drop(s);
    // Rename only this test-owned directory. No production data is moved.
    fs::rename(&a, &b).unwrap();
    let s = Store::open(&b).unwrap();
    assert_eq!(s.checkpoint_events(), 0);
    assert_eq!(s.snapshot().plans.len(), PERIOD);
}

#[test]
fn captured_bytes_are_not_rehashed_from_mutated_disk_and_pending_checkpoint_survives_coalescing() {
    let root = tempfile::tempdir().unwrap();
    let mut s = Store::open(root.path()).unwrap();
    s.wait_for_index(Duration::from_secs(20)).unwrap();
    let gate = s.index.test_gate();
    append(&mut s, 0);
    gate.entered();
    for n in 1..=PERIOD {
        append(&mut s, n);
    }
    let event = paths(root.path())[0].clone();
    let mut bytes = fs::read(&event).unwrap();
    bytes.push(b'\n');
    fs::write(&event, bytes).unwrap();
    gate.release();
    drop(s);
    assert!(root.path().join(FILE).is_file());
    let s = Store::open(root.path()).unwrap();
    assert_eq!(
        s.checkpoint_events(),
        0,
        "writer must seal captured bytes, not rescan mutated files"
    );
    assert_eq!(s.snapshot().plans.len(), PERIOD + 1);
}

#[test]
fn checkpoint_publication_failure_does_not_fail_events_or_other_projections() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(FILE)).unwrap();
    let mut s = Store::open(root.path()).unwrap();
    for n in 0..PERIOD {
        append(&mut s, n);
    }
    s.wait_for_index(Duration::from_secs(20)).unwrap();
    assert_eq!(s.snapshot().plans.len(), PERIOD);
    assert!(s.index.diagnostics().error().is_some());
    assert!(s.index_error().is_none());
    drop(s);
    assert_eq!(
        Store::open(root.path()).unwrap().snapshot().plans.len(),
        PERIOD
    );
}

#[test]
fn byte_limits_and_linked_cache_never_authorize_a_snapshot() {
    let root = tempfile::tempdir().unwrap();
    seeded(root.path());
    let cache = root.path().join(FILE);
    let other = root.path().join("outside");
    fs::rename(&cache, &other).unwrap();
    fs::hard_link(&other, &cache).unwrap();
    assert!(load(root.path(), "store", "", &paths(root.path())).is_none());
    fs::remove_file(&cache).unwrap();
    let file = fs::File::create(&cache).unwrap();
    file.set_len(MAX_SEALED as u64 + 1).unwrap();
    drop(file);
    assert!(load(root.path(), "store", "", &paths(root.path())).is_none());
    let mut tracker = Tracker::default();
    tracker.record("large.json".into(), &vec![0; MAX_PREFIX + 1]);
    assert!(tracker.proofs.is_none());
    assert!(tracker.due().is_none());
    let mut writer = Bounded(vec![0; MAX_PLAIN]);
    assert!(writer.write_all(b"x").is_err());
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
fn command(s: &mut DurableWorkflow, who: &str, command: WorkflowCommand, context: CommandContext) {
    s.command(
        &format!("op-{}", s.history().len() + 1),
        who,
        command,
        context,
        true,
    )
    .unwrap();
}
fn workflow(root: &Path) -> DurableWorkflow {
    let mut s = DurableWorkflow::open(root, config()).unwrap();
    command(
        &mut s,
        "chief",
        WorkflowCommand::Propose {
            plan: WorkflowPlan {
                id: "plan".into(),
                version: 1,
                tasks: vec![WorkflowTask {
                    id: "a".into(),
                    workspace_key: "space".into(),
                    goal: "test".into(),
                    dependencies: vec![],
                    resource_keys: Some(vec!["port-main".into()]),
                }],
            },
        },
        CommandContext::default(),
    );
    let digest = s.state().digest.clone().unwrap();
    command(
        &mut s,
        "user",
        WorkflowCommand::Approve { digest },
        CommandContext {
            approval_source: Some("local user".into()),
            ..Default::default()
        },
    );
    command(
        &mut s,
        "deputy",
        WorkflowCommand::Assign {
            task: "a".into(),
            worker: "worker".into(),
            attempt: "try-a".into(),
        },
        CommandContext::default(),
    );
    command(
        &mut s,
        "core",
        WorkflowCommand::DispatchIntent {
            task: "a".into(),
            attempt: "try-a".into(),
        },
        CommandContext {
            thread_id: Some("session".into()),
            cwd: Some("space".into()),
            ..Default::default()
        },
    );
    while s.history().len() < PERIOD {
        command(
            &mut s,
            "deputy",
            WorkflowCommand::Advise {
                task: "a".into(),
                suggestion: "observe original task only".into(),
            },
            CommandContext::default(),
        );
    }
    s
}

#[test]
fn workflow_checkpoint_preserves_unknown_recovery_no_resend_and_full_replay_equivalence() {
    let root = tempfile::tempdir().unwrap();
    drop(workflow(root.path()));
    let mut s = DurableWorkflow::open(root.path(), config()).unwrap();
    assert_eq!(s.checkpoint_events(), PERIOD);
    assert_eq!(s.state().tasks["a"].state, TaskState::Unknown);
    assert_eq!(s.history().len(), PERIOD + 1);
    assert!(s
        .command(
            "new-intent",
            "core",
            WorkflowCommand::DispatchIntent {
                task: "a".into(),
                attempt: "try-a".into()
            },
            CommandContext {
                thread_id: Some("session".into()),
                cwd: Some("space".into()),
                ..Default::default()
            },
            true
        )
        .is_err());
    let state = s.state().clone();
    let history = s.history().to_vec();
    drop(s);
    let before = fs::read(root.path().join(FILE)).unwrap();
    DurableWorkflow::inspect_existing(root.path(), |s| {
        assert_eq!(s.checkpoint_events(), 0);
        assert_eq!(s.state(), &state);
        Ok(())
    })
    .unwrap();
    assert_eq!(before, fs::read(root.path().join(FILE)).unwrap());
    fs::remove_file(root.path().join(FILE)).unwrap();
    let s = DurableWorkflow::open(root.path(), config()).unwrap();
    assert_eq!(s.checkpoint_events(), 0);
    assert_eq!(s.state(), &state);
    assert_eq!(s.history(), history);
}

#[test]
fn authenticated_cache_still_checks_current_fee_guard() {
    let root = tempfile::tempdir().unwrap();
    drop(workflow(root.path()));
    let path_list = paths(root.path());
    let original = fs::read(root.path().join("bindings.json")).unwrap();
    let config: SchedulerConfig = serde_json::from_slice(&original).unwrap();
    let bindings = format!("{:x}", Sha256::digest(serde_json::to_vec(&config).unwrap()));
    let mut p = load(root.path(), "workflow", &bindings, &path_list).unwrap();
    // White-box authenticated fixture: the cached prefix carries an API fee
    // confirmation but the current directory has no reader12 protection.
    // This is deliberately not a production save/authorization path.
    if let Payload::Workflow { entries, .. } = &mut p.payload {
        entries[0].body.event.command = WorkflowCommand::ConfirmApiCountResolution {
            review_digest: "0".repeat(64),
        };
    } else {
        panic!("workflow fixture");
    }
    p.publish(root.path()).unwrap();
    let error = DurableWorkflow::open(root.path(), config)
        .err()
        .expect("missing current guard must fail");
    assert!(
        error.contains("reader") || error.contains("guard") || error.contains("protected"),
        "{error}"
    );
}

#[test]
fn authenticated_cache_still_requires_original_host_currency_context() {
    let root = tempfile::tempdir().unwrap();
    drop(workflow(root.path()));
    let configuration = config();
    let bindings = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&configuration).unwrap())
    );
    let mut p = load(root.path(), "workflow", &bindings, &paths(root.path())).unwrap();
    if let Payload::Workflow { entries, .. } = &mut p.payload {
        entries[1].body.context.currency_approval =
            Some(crate::currency_approval::ApprovalAnchor {
                schema: 1,
                policy_digest: "a".repeat(64),
                previous_policy_digest: "b".repeat(64),
                previous_ledger_head: "c".repeat(64),
            });
    } else {
        panic!("workflow fixture");
    }
    p.publish(root.path()).unwrap();
    let error = DurableWorkflow::open(root.path(), configuration)
        .err()
        .unwrap();
    assert!(error.contains("guarded host directory"), "{error}");
}

#[test]
#[ignore = "explicit bounded startup measurement; no model calls or GUI work"]
fn replay_checkpoint_measure_full_and_cached_workflow_startup() {
    use std::time::Instant;
    let root = tempfile::tempdir().unwrap();
    let mut s = workflow(root.path());
    while s.history().len() < 1024 {
        command(
            &mut s,
            "deputy",
            WorkflowCommand::Advise {
                task: "a".into(),
                suggestion: "synthetic retained-history measurement".into(),
            },
            CommandContext::default(),
        );
    }
    drop(s);
    // Commit the normal Unknown recovery once before comparing equal histories.
    drop(DurableWorkflow::open(root.path(), config()).unwrap());
    let mut samples = Vec::new();
    for pass in 0..3 {
        fs::remove_file(root.path().join(FILE)).unwrap();
        let start = Instant::now();
        let cold = DurableWorkflow::open(root.path(), config()).unwrap();
        let full_us = start.elapsed().as_micros();
        assert_eq!(cold.checkpoint_events(), 0);
        let expected = cold.state().clone();
        let history = cold.history().to_vec();
        drop(cold);
        let start = Instant::now();
        let warm = DurableWorkflow::open(root.path(), config()).unwrap();
        let checkpoint_us = start.elapsed().as_micros();
        assert_eq!(warm.checkpoint_events(), 1025);
        assert_eq!(warm.state(), &expected);
        assert_eq!(warm.history(), history);
        samples.push(serde_json::json!({"pass":pass,"events":history.len(),"fullOpenUs":full_us,"checkpointOpenUs":checkpoint_us,"checkpointBytes":fs::metadata(root.path().join(FILE)).unwrap().len()}));
        drop(warm);
    }
    let report = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports/replay-checkpoint-306");
    fs::create_dir_all(&report).unwrap();
    let output = report.join(format!(
        "startup-{}-{}.json",
        std::process::id(),
        chrono::Utc::now().timestamp_millis()
    ));
    fs::write(output, serde_json::to_vec_pretty(&serde_json::json!({"synthetic":true,"profile":"debug","sameProcess":true,"executableHashAlreadyCached":true,"includesEventReadAndHash":true,"includesProjectionSnapshotConstruction":true,"waitsForBackgroundPublication":false,"modelCalls":0,"samples":samples})).unwrap()).unwrap();
}
