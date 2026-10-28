use super::*;
use std::time::{Duration, Instant};

fn advice(n: usize) -> WorkflowCommand {
    WorkflowCommand::Advise {
        task: "a".into(),
        suggestion: format!("bounded synthetic advice {n}"),
    }
}
fn assert_projection(s: &DurableWorkflow) {
    let reference = s.handoffs_for(s.entries.iter()).unwrap();
    assert_eq!(s.handoffs().unwrap(), reference);
    if let Some(cached) = &s.handoff_cache {
        assert_eq!(cached.head, s.index_head());
        assert_eq!(cached.events, s.handoff_events(s.entries.iter()).unwrap());
        assert_eq!(s.render_handoffs(cached.events.iter()).unwrap(), reference);
    }
}
fn append(
    s: &mut DurableWorkflow,
    id: &str,
    actor: &str,
    command: WorkflowCommand,
    context: CommandContext,
) {
    s.command(id, actor, command, context, false).unwrap();
    assert_projection(s);
}
fn binding() -> CommandContext {
    CommandContext {
        thread_id: Some("session".into()),
        cwd: Some("isolated".into()),
        ..Default::default()
    }
}
fn running(s: &mut DurableWorkflow) {
    append(
        s,
        "dispatch",
        "core",
        WorkflowCommand::DispatchIntent {
            task: "a".into(),
            attempt: "try-a".into(),
        },
        binding(),
    );
    append(
        s,
        "started",
        "core",
        WorkflowCommand::Started {
            task: "a".into(),
            attempt: "try-a".into(),
        },
        CommandContext {
            turn_id: Some("turn".into()),
            ..binding()
        },
    );
}

#[test]
fn cached_prefix_matches_full_replay_through_submit_verify_accept_and_reopen() {
    let root = tempfile::tempdir().unwrap();
    let mut s = projection_tests::workflow(root.path());
    assert!(s.handoff_cache.is_some());
    running(&mut s);
    append(
        &mut s,
        "advice",
        "deputy",
        advice(1),
        CommandContext::default(),
    );
    append(
        &mut s,
        "submit",
        "worker",
        WorkflowCommand::Submit {
            task: "a".into(),
            attempt: "try-a".into(),
            summary: "local synthetic result".into(),
            evidence: Some(vec!["artifact.txt".into()]),
        },
        CommandContext {
            turn_id: Some("turn".into()),
            terminal_status: Some("completed".into()),
            ..binding()
        },
    );
    append(
        &mut s,
        "verify",
        "core",
        WorkflowCommand::Verify {
            task: "a".into(),
            attempt: "try-a".into(),
            passed: true,
            evidence: vec!["controlled-check".into()],
        },
        CommandContext::default(),
    );
    append(
        &mut s,
        "accept",
        "chief",
        WorkflowCommand::Accept {
            task: "a".into(),
            attempt: "try-a".into(),
        },
        CommandContext::default(),
    );
    let events = s.entries.clone();
    let daily = s.handoffs().unwrap();
    drop(s);
    let s = DurableWorkflow::open(root.path(), projection_tests::config()).unwrap();
    assert_eq!(s.entries, events);
    assert_eq!(s.handoffs().unwrap(), daily);
    assert_projection(&s);
}

#[test]
fn rendering_rejection_preserves_prefix_and_allows_a_valid_next_command() {
    let root = tempfile::tempdir().unwrap();
    let mut s = projection_tests::workflow(root.path());
    running(&mut s);
    let before = s.handoff_cache.as_ref().unwrap().events.clone();
    let head = s.index_head();
    let error = s
        .command(
            "oversized",
            "worker",
            WorkflowCommand::Submit {
                task: "a".into(),
                attempt: "try-a".into(),
                summary: "done".into(),
                evidence: Some(vec!["&".repeat(8192); 7]),
            },
            CommandContext {
                terminal_status: Some("completed".into()),
                turn_id: Some("turn".into()),
                ..binding()
            },
            false,
        )
        .unwrap_err();
    assert!(error.contains("volume budget"));
    assert_eq!(s.index_head(), head);
    assert_eq!(s.handoff_cache.as_ref().unwrap().events, before);
    assert!(!s.requires_reopen());
    append(
        &mut s,
        "next",
        "deputy",
        advice(2),
        CommandContext::default(),
    );
}

#[test]
fn prepublication_failure_does_not_advance_cache_and_reopen_has_no_duplicate() {
    let root = tempfile::tempdir().unwrap();
    let mut s = projection_tests::workflow(root.path());
    let before = s.handoff_cache.as_ref().unwrap().events.clone();
    let count = s.entries.len();
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        s.command_observed(
            "unwritten",
            "deputy",
            advice(3),
            CommandContext::default(),
            false,
            |point| {
                if point == CommitPoint::BeforeEventWrite {
                    panic!("controlled prepublication interruption");
                }
            },
        )
    }));
    assert!(failure.is_err());
    assert_eq!(s.entries.len(), count);
    assert_eq!(s.handoff_cache.as_ref().unwrap().events, before);
    assert!(s.requires_reopen());
    drop(s);
    let s = DurableWorkflow::open(root.path(), projection_tests::config()).unwrap();
    assert_eq!(s.entries.len(), count);
    assert_projection(&s);
}

#[test]
fn mismatched_prefix_is_discarded_and_full_history_is_used() {
    let root = tempfile::tempdir().unwrap();
    let mut s = projection_tests::workflow(root.path());
    s.handoff_cache.as_mut().unwrap().head.hash = "stale".into();
    append(
        &mut s,
        "fallback",
        "deputy",
        advice(4),
        CommandContext::default(),
    );
    assert!(s.handoff_cache.is_none());
    s.project().unwrap();
    assert!(s.handoff_cache.is_some());
    assert_projection(&s);
}

#[test]
fn shared_budget_exhaustion_drops_optional_cache_without_failing_commit() {
    let root = tempfile::tempdir().unwrap();
    let mut s = projection_tests::workflow(root.path());
    s.handoff_cache = None;
    s.index = crate::derived_index::Cache::test_with_capacity(4096);
    s.project().unwrap();
    assert!(s.handoff_cache.is_some());
    let count = s.entries.len();
    append(
        &mut s,
        "capacity",
        "deputy",
        WorkflowCommand::Advise {
            task: "a".into(),
            suggestion: "x".repeat(4000),
        },
        CommandContext::default(),
    );
    assert_eq!(s.entries.len(), count + 1);
    assert!(s.handoff_cache.is_none());
    assert!(!s.requires_reopen());
    assert!(s.wait_for_index(Duration::from_secs(20)).is_err());
    drop(s);
    let s = DurableWorkflow::open(root.path(), projection_tests::config()).unwrap();
    assert_eq!(s.entries.len(), count + 1);
    assert_projection(&s);
}

#[test]
fn read_only_inspection_uses_verified_ledger_without_creating_a_memo() {
    let root = tempfile::tempdir().unwrap();
    let s = projection_tests::workflow(root.path());
    let expected = s.handoffs().unwrap();
    drop(s);
    #[cfg(windows)]
    DurableWorkflow::inspect_existing(root.path(), |s| {
        assert!(s.handoff_cache.is_none());
        assert_eq!(s.handoffs()?, expected);
        Ok(())
    })
    .unwrap();
}

#[test]
#[ignore = "explicit bounded same-process synthetic handoff-preparation measurement"]
fn measure_handoff_prefix_against_full_replay() {
    let output =
        std::env::var_os("CERULEAN_HANDOFF_MEASURE_317").expect("dedicated report path required");
    let root = tempfile::tempdir().unwrap();
    let mut s = projection_tests::workflow(root.path());
    let mut samples = Vec::new();
    for count in [128, 512, 1024] {
        while s.entries.len() < count {
            let n = s.entries.len();
            s.command(
                &format!("advice-{n}"),
                "deputy",
                advice(n),
                CommandContext::default(),
                false,
            )
            .unwrap();
        }
        s.wait_for_projections(Duration::from_secs(30)).unwrap();
        s.wait_for_index(Duration::from_secs(30)).unwrap();
        let mut next = s.core.clone();
        let event = next.apply("deputy", advice(count)).unwrap();
        let body = EntryBody {
            schema_version: 1,
            operation_id: "measurement-only-not-committed".into(),
            received_at: s.entries.last().unwrap().body.received_at.clone(),
            bindings_digest: s.bindings_digest.clone(),
            event,
            context: CommandContext::default(),
            previous_hash: s.entries.last().unwrap().hash.clone(),
        };
        let entry = Entry {
            hash: digest(&body).unwrap(),
            body,
        };
        for pass in 0..3 {
            let mut full = None;
            let mut cached = None;
            let mut full_us = 0;
            let mut cached_us = 0;
            for baseline in if pass % 2 == 0 {
                [true, false]
            } else {
                [false, true]
            } {
                let start = Instant::now();
                if baseline {
                    full = Some(
                        s.handoffs_for(s.entries.iter().chain(std::iter::once(&entry)))
                            .unwrap(),
                    );
                    full_us = start.elapsed().as_micros();
                } else {
                    cached = Some(s.prepare_handoffs(&entry, next.state()).unwrap().0);
                    cached_us = start.elapsed().as_micros();
                }
            }
            assert_eq!(full, cached);
            samples.push(serde_json::json!({"events":count,"pass":pass,"fullReplayAndRenderUs":full_us,"cachedPrefixAndRenderUs":cached_us,"outputsByteEqual":true}));
        }
    }
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output)
        .unwrap();
    serde_json::to_writer_pretty(&mut file, &serde_json::json!({"profile":"debug","synthetic":true,"sameProcess":true,"measuredStage":"precommit handoff preparation only; excludes core clone, index encoding and fsync","modelCalls":0,"samples":samples})).unwrap();
    file.sync_all().unwrap();
}

#[test]
fn cached_prefix_unknown_reconciliation_and_followup_preserve_original_attempt() {
    let root = tempfile::tempdir().unwrap();
    let mut s = projection_tests::workflow(root.path());
    let assert_cached = |s: &DurableWorkflow| {
        let cached = s.handoff_cache.as_ref().expect("expected cached prefix");
        assert_eq!(cached.head, s.index_head());
        assert_projection(s);
    };
    assert_cached(&s);
    running(&mut s);
    assert_cached(&s);
    append(
        &mut s,
        "unknown",
        "core",
        WorkflowCommand::RecoveryUnknown {
            task: "a".into(),
            attempt: "try-a".into(),
        },
        CommandContext::default(),
    );
    assert_cached(&s);
    assert_eq!(s.state().tasks["a"].state, TaskState::Unknown);
    assert_eq!(
        s.handoff_cache
            .as_ref()
            .unwrap()
            .events
            .last()
            .unwrap()
            .status,
        HandoffStatus::Blocked
    );
    let head = s.index_head();
    let prefix = s.handoff_cache.as_ref().unwrap().events.clone();
    assert!(s
        .command(
            "forbidden-retry",
            "core",
            WorkflowCommand::DispatchIntent {
                task: "a".into(),
                attempt: "try-a".into(),
            },
            binding(),
            true,
        )
        .is_err());
    let reconcile = WorkflowCommand::Reconcile {
        task: "a".into(),
        attempt: "try-a".into(),
        outcome: cerulean_domain::workflow::ReconcileOutcome::Running,
        evidence: vec!["original-turn-readback".into()],
    };
    let turn = CommandContext {
        turn_id: Some("turn".into()),
        ..binding()
    };
    assert!(s
        .command(
            "wrong-reconcile",
            "worker",
            reconcile.clone(),
            turn.clone(),
            false,
        )
        .is_err());
    assert_eq!(s.index_head(), head);
    assert_eq!(s.handoff_cache.as_ref().unwrap().events, prefix);
    assert_cached(&s);
    append(&mut s, "reconcile", "core", reconcile, turn.clone());
    assert_cached(&s);
    assert_eq!(s.state().tasks["a"].state, TaskState::Running);
    let last = s.handoff_cache.as_ref().unwrap().events.last().unwrap();
    assert_eq!(last.status, HandoffStatus::Running);
    assert_eq!(last.attempt_id, "try-a");
    assert_eq!(last.evidence, vec!["original-turn-readback".to_string()]);
    append(
        &mut s,
        "followup",
        "deputy",
        advice(5),
        CommandContext::default(),
    );
    assert_cached(&s);
    append(
        &mut s,
        "submitted-after-readback",
        "worker",
        WorkflowCommand::Submit {
            task: "a".into(),
            attempt: "try-a".into(),
            summary: "original task completed after local reconciliation".into(),
            evidence: Some(vec!["original-artifact".into()]),
        },
        CommandContext {
            terminal_status: Some("completed".into()),
            ..turn
        },
    );
    assert_cached(&s);
    assert_eq!(s.state().tasks["a"].state, TaskState::Submitted);
    assert_eq!(
        s.entries
            .iter()
            .filter(|e| matches!(e.body.event.command, WorkflowCommand::DispatchIntent { .. }))
            .count(),
        1
    );
    let entries = s.entries.clone();
    let daily = s.handoffs().unwrap();
    drop(s);
    let s = DurableWorkflow::open(root.path(), projection_tests::config()).unwrap();
    assert_eq!(s.entries, entries);
    assert_eq!(s.handoffs().unwrap(), daily);
    assert_eq!(s.state().tasks["a"].state, TaskState::Submitted);
    assert_cached(&s);
}
