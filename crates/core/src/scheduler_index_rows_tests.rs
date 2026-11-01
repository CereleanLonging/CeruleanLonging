use super::*;
use crate::derived_index::{Data, DataRef, Head, IndexedEvent};
use cerulean_domain::workflow::{ReconcileOutcome, WorkflowPlan, WorkflowTask};
use std::{
    collections::BTreeSet,
    time::{Duration, Instant},
};

fn plan(version: u32) -> WorkflowPlan {
    WorkflowPlan {
        id: "rows-plan".into(),
        version,
        tasks: (0..32)
            .map(|n| WorkflowTask {
                id: format!("task-{n}"),
                dependencies: vec![],
                workspace_key: "space".into(),
                goal: "中文、escaped \"quote\" \\ path\n".repeat(16),
                resource_keys: None,
            })
            .collect(),
    }
}

fn commands() -> Vec<WorkflowCommand> {
    use WorkflowCommand::*;
    let proof = || "a".repeat(64);
    let task = || "task-0".to_owned();
    let attempt = || "attempt-1".to_owned();
    let text = || "中文、引号\"、反斜杠\\、换行\n与\t控制字符".to_owned();
    vec![
        RetireVerificationRecheck {
            task: task(),
            attempt: attempt(),
            operation_id: "retired-check".into(),
            review_digest: proof(),
        },
        ApproveVerificationRecheck {
            task: task(),
            attempt: attempt(),
            operation_id: "corrected-check".into(),
            review_digest: proof(),
        },
        ConfirmApiGenerationResolution {
            review_digest: proof(),
        },
        ConfirmApiCountResolution {
            review_digest: proof(),
        },
        ConfirmManagementCountResolution {
            review_digest: proof(),
        },
        ConfirmHistoricalCharge {
            review_digest: proof(),
        },
        AuthorizePlanningBudget {
            setup_digest: proof(),
        },
        RetireSubmittedPlan {
            digest: proof(),
            reason: text(),
        },
        RequestRevision {
            digest: proof(),
            reason: text(),
        },
        Propose { plan: plan(1) },
        Approve { digest: proof() },
        Assign {
            task: task(),
            worker: "worker".into(),
            attempt: attempt(),
        },
        AbandonUnsent {
            task: task(),
            attempt: attempt(),
            reason: text(),
        },
        RetireUnknownDesktop {
            task: task(),
            attempt: attempt(),
            evidence_digest: proof(),
            reason: text(),
        },
        RetireUnknownApi {
            task: task(),
            attempt: attempt(),
            reason: text(),
        },
        DispatchIntent {
            task: task(),
            attempt: attempt(),
        },
        Started {
            task: task(),
            attempt: attempt(),
        },
        CancelIntent {
            task: task(),
            attempt: attempt(),
            reason: text(),
        },
        RecoveryUnknown {
            task: task(),
            attempt: attempt(),
        },
        Reconcile {
            task: task(),
            attempt: attempt(),
            outcome: ReconcileOutcome::Running,
            evidence: vec![text()],
        },
        Submit {
            task: task(),
            attempt: attempt(),
            summary: text(),
            evidence: None,
        },
        Submit {
            task: task(),
            attempt: attempt(),
            summary: text(),
            evidence: Some(vec![text()]),
        },
        Verify {
            task: task(),
            attempt: attempt(),
            passed: true,
            evidence: vec![text()],
        },
        Accept {
            task: task(),
            attempt: attempt(),
        },
        Advise {
            task: task(),
            suggestion: text(),
        },
    ]
}

fn entry(command: WorkflowCommand, sequence: usize) -> Entry {
    Entry {
        body: EntryBody {
            schema_version: 1,
            operation_id: format!("op-{sequence}"),
            received_at: "2026-10-24T03:00:00+08:00".into(),
            bindings_digest: "a".repeat(64),
            event: WorkflowEvent {
                sequence: sequence.try_into().unwrap(),
                principal: "core".into(),
                command,
            },
            context: CommandContext::default(),
            previous_hash: "b".repeat(64),
        },
        hash: "c".repeat(64),
    }
}

// Independent legacy algorithm. Do not call the new kind/task/attempt helpers here.
fn legacy_events(entries: &[Entry]) -> Vec<IndexedEvent> {
    entries
        .iter()
        .map(|e| {
            let command = serde_json::to_value(&e.body.event.command).unwrap();
            IndexedEvent {
                sequence: e.body.event.sequence.into(),
                operation_id: e.body.operation_id.clone(),
                kind: command["kind"].as_str().unwrap().into(),
                received_at: e.body.received_at.clone(),
                hash: e.hash.clone(),
                task: command["task"].as_str().map(str::to_owned),
                attempt: command["attempt"].as_str().map(str::to_owned),
            }
        })
        .collect()
}

#[test]
fn borrowed_index_rows_match_original_for_every_command_and_snapshot() {
    let entries = commands()
        .into_iter()
        .enumerate()
        .map(|(n, c)| entry(c, n + 1))
        .collect::<Vec<_>>();
    let old = legacy_events(&entries);
    assert_eq!(
        old.iter()
            .map(|e| e.kind.as_str())
            .collect::<BTreeSet<_>>()
            .len(),
        24
    );
    for n in 0..entries.len() {
        assert_eq!(
            serde_json::to_vec(&index_rows::Events(&entries[n..n + 1])).unwrap(),
            serde_json::to_vec(&old[n..n + 1]).unwrap()
        );
    }
    for slice in [&entries[..], &entries[..0]] {
        let data = Data {
            head: Head {
                ledger: "workflow".into(),
                count: slice.len(),
                hash: "c".repeat(64),
                bindings: "a".repeat(64),
            },
            events: legacy_events(slice),
            plans: vec![],
            summaries: vec![],
            tasks: vec![],
            daily: vec![DailyVolume {
                path: "daily/2026-10-24/001.md".into(),
                day: "2026-10-24".into(),
                number: 1,
                sealed: false,
                event_ids: vec!["op-1".into()],
                markdown: "中文\n\"引号\"\\路径".into(),
            }],
        };
        let events = index_rows::Events(slice);
        let borrowed = DataRef {
            head: &data.head,
            events: &events,
            plans: &data.plans,
            summaries: &data.summaries,
            tasks: &data.tasks,
            daily: &data.daily,
        };
        let encoded = serde_json::to_vec(&borrowed).unwrap();
        assert_eq!(encoded, serde_json::to_vec(&data).unwrap());
        let decoded: Data = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded.events, data.events);
        assert_eq!(decoded.daily, data.daily);
    }
}

#[test]
fn reused_task_id_matches_legacy_creation_and_update() {
    let root = tempfile::tempdir().unwrap();
    let mut s = DurableWorkflow::open(root.path(), projection_tests::config()).unwrap();
    let mut initial = plan(1);
    initial.tasks.truncate(1);
    s.command(
        "plan-1",
        "chief",
        WorkflowCommand::Propose {
            plan: initial.clone(),
        },
        CommandContext::default(),
        false,
    )
    .unwrap();
    s.command(
        "approve-1",
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
        "advice-1",
        "deputy",
        WorkflowCommand::Advise {
            task: "task-0".into(),
            suggestion: "first revision update".into(),
        },
        CommandContext::default(),
        false,
    )
    .unwrap();
    s.command(
        "revise",
        "user",
        WorkflowCommand::RequestRevision {
            digest: s.state().digest.clone().unwrap(),
            reason: "reuse task ID".into(),
        },
        CommandContext {
            approval_source: Some("authenticated local-user revision request".into()),
            ..Default::default()
        },
        false,
    )
    .unwrap();
    initial.version = 2;
    let proposal = s
        .command(
            "plan-2",
            "chief",
            WorkflowCommand::Propose { plan: initial },
            CommandContext::default(),
            false,
        )
        .unwrap();
    let old = legacy_index_data(&s, vec![]).unwrap();
    let tasks = s.index_tasks().unwrap();
    assert_eq!(tasks, old.tasks);
    assert_eq!(tasks[0].updated_at, proposal.body.received_at);
    let daily = s.handoffs().unwrap();
    let events = index_rows::Events(&s.entries);
    let head = s.index_head();
    let borrowed = DataRef {
        head: &head,
        events: &events,
        plans: &[],
        summaries: &[],
        tasks: &tasks,
        daily: &daily,
    };
    assert_eq!(
        serde_json::to_vec(&borrowed).unwrap(),
        serde_json::to_vec(&legacy_index_data(&s, daily.clone()).unwrap()).unwrap()
    );
}

#[test]
fn borrowed_index_budget_failure_preserves_committed_event_and_fallback() {
    let root = tempfile::tempdir().unwrap();
    let mut s = projection_tests::workflow(root.path());
    s.handoff_cache = None;
    s.index = crate::derived_index::Cache::test_with_capacity(4096);
    let old_count = s.entries.len();
    s.command(
        "budget",
        "deputy",
        WorkflowCommand::Advise {
            task: "a".into(),
            suggestion: "bounded index preparation".into(),
        },
        CommandContext::default(),
        false,
    )
    .unwrap();
    assert_eq!(s.entries.len(), old_count + 1);
    assert!(!s.requires_reopen());
    assert!(s.wait_for_index(Duration::from_secs(20)).is_err());
    assert_eq!(
        s.unfinished_before("9999-12-31").unwrap(),
        s.index_tasks().unwrap()
    );
    let entries = s.entries.clone();
    s.index = crate::derived_index::Cache::default();
    s.refresh_index(s.handoffs().unwrap());
    s.wait_for_projections(Duration::from_secs(20)).unwrap();
    s.wait_for_index(Duration::from_secs(20)).unwrap();
    assert_eq!(s.entries, entries);
    drop(s);
    let s = DurableWorkflow::open(root.path(), projection_tests::config()).unwrap();
    assert_eq!(s.entries, entries);
    assert_eq!(
        s.index_tasks().unwrap(),
        legacy_index_data(&s, vec![]).unwrap().tasks
    );
}

#[test]
#[ignore = "bounded same-process synthetic metadata preparation measurement"]
fn measure_borrowed_index_metadata_against_original() {
    let mut measurements = Vec::new();
    for count in [128, 512, 1024] {
        let samples = commands();
        let entries = (0..count)
            .map(|n| entry(samples[n % samples.len()].clone(), n + 1))
            .collect::<Vec<_>>();
        for repeat in 0..3 {
            let baseline = || {
                let now = Instant::now();
                let rows = legacy_events(std::hint::black_box(&entries));
                let bytes = serde_json::to_vec(&rows).unwrap();
                (bytes, now.elapsed().as_nanos())
            };
            let borrowed = || {
                let now = Instant::now();
                let bytes = serde_json::to_vec(&index_rows::Events(std::hint::black_box(&entries)))
                    .unwrap();
                (bytes, now.elapsed().as_nanos())
            };
            let (old, new) = if repeat % 2 == 0 {
                (baseline(), borrowed())
            } else {
                let new = borrowed();
                (baseline(), new)
            };
            assert_eq!(old.0, new.0);
            measurements.push(serde_json::json!({"events":count,"repeat":repeat,"legacyNs":old.1,"borrowedNs":new.1,"bytes":new.0.len()}));
        }
    }
    println!(
        "metadata-measurement={}",
        serde_json::to_string(&measurements).unwrap()
    );
}

fn legacy_index_data(
    s: &DurableWorkflow,
    daily: Vec<DailyVolume>,
) -> Result<crate::derived_index::Data, String> {
    use crate::derived_index::{Data, IndexedEvent, IndexedTask};
    let mut events = Vec::with_capacity(s.entries.len());
    for e in &s.entries {
        let command = serde_json::to_value(&e.body.event.command).map_err(|e| e.to_string())?;
        events.push(IndexedEvent {
            sequence: e.body.event.sequence.into(),
            operation_id: e.body.operation_id.clone(),
            kind: command["kind"]
                .as_str()
                .ok_or("missing command kind")?
                .into(),
            received_at: e.body.received_at.clone(),
            hash: e.hash.clone(),
            task: command["task"].as_str().map(str::to_owned),
            attempt: command["attempt"].as_str().map(str::to_owned),
        });
    }
    let zone = cerulean_domain::timezone::parse(
        s.config
            .execution_scope
            .as_ref()
            .map(|s| s.project.timezone.as_str())
            .unwrap_or(cerulean_domain::timezone::DEFAULT),
    )?;
    let mut tasks = Vec::new();
    for (id, run) in &s.state().tasks {
        // A revised plan may reuse task IDs: derive creation from the current proposal.
        let proposal = s.entries.iter().rev().find(|e| matches!(
                &e.body.event.command, WorkflowCommand::Propose { plan } if plan.tasks.iter().any(|t| &t.id == id)
            )).ok_or("task proposal missing")?;
        let first = chrono::DateTime::parse_from_rfc3339(&proposal.body.received_at)
            .map_err(|_| "invalid task creation time")?;
        let updated_at = events
            .iter()
            .rev()
            .find(|e| {
                e.task.as_ref() == Some(id) && e.sequence >= u64::from(proposal.body.event.sequence)
            })
            .map(|e| e.received_at.clone())
            .unwrap_or_else(|| proposal.body.received_at.clone());
        tasks.push(IndexedTask {
            id: id.clone(),
            state: serde_json::to_value(run.state)
                .map_err(|e| e.to_string())?
                .as_str()
                .ok_or("missing task state")?
                .into(),
            attempt: run.attempt.clone(),
            first_day: if s.config.schema_version == 1 {
                first
                    .with_timezone(&chrono::FixedOffset::east_opt(8 * 3600).unwrap())
                    .format("%Y-%m-%d")
                    .to_string()
            } else {
                first.with_timezone(&zone).format("%Y-%m-%d").to_string()
            },
            updated_at,
            unfinished: !matches!(run.state, TaskState::Accepted | TaskState::Cancelled),
        });
    }
    Ok(Data {
        head: s.index_head(),
        events,
        plans: vec![],
        summaries: vec![],
        tasks,
        daily,
    })
}
