use super::*;
fn start(tracking: bool) -> Start {
    Start {
        schema: 1,
        data_root: "unused".into(),
        project: "test".into(),
        setup_digest: "a".repeat(64),
        binding_digest: "b".repeat(64),
        human_tracking: tracking,
        started_at: "2026-01-01T00:00:00Z".into(),
    }
}
fn mark(kind: Kind, ms: Option<u64>) -> Mark {
    Mark {
        sequence: 1,
        previous: "unused".into(),
        kind,
        offset_ms: ms,
        observed_at: "2000-01-01T00:00:00Z".into(),
        history_length: 0,
        ledger_head: None,
    }
}
fn complete() -> Vec<Mark> {
    vec![
        mark(Kind::HumanStarted, Some(5)),
        mark(Kind::HumanStopped, Some(20)),
        mark(Kind::Proposal, Some(100)),
        mark(Kind::HumanStarted, Some(120)),
        mark(Kind::HumanStopped, Some(140)),
        mark(Kind::Approved, Some(200)),
        mark(
            Kind::Execution {
                start_digest: "a".repeat(64),
            },
            Some(210),
        ),
        mark(Kind::Finished, Some(300)),
    ]
}
#[test]
fn spans_include_review_wait_without_double_counting_human_time() {
    let s = start(true);
    let report = summary(&s, &digest(&s).unwrap(), &complete(), true).unwrap();
    assert_eq!(report["elapsedMs"], 300);
    assert_eq!(report["planningMs"], 100);
    assert_eq!(report["approvalWaitMs"], 100);
    assert_eq!(report["executionAndClosureMs"], 100);
    assert_eq!(report["humanActiveMs"], 35);
    // UTC labels may move backwards; only monotonic offsets define durations.
    let mut changed = complete();
    changed[7].observed_at = "1990-01-01T00:00:00Z".into();
    let changed = summary(&s, &digest(&s).unwrap(), &changed, true).unwrap();
    assert_ne!(changed["chainHead"], report["chainHead"]);
    for key in [
        "elapsedMs",
        "planningMs",
        "approvalWaitMs",
        "executionAndClosureMs",
        "humanActiveMs",
    ] {
        assert_eq!(changed[key], report[key], "{key}");
    }
}
#[test]
fn recovery_and_unenrolled_human_time_remain_unknown() {
    let s = start(true);
    let mut records = complete()[..4].to_vec();
    records.push(mark(Kind::Interrupted, None));
    let v = summary(&s, &digest(&s).unwrap(), &records, false).unwrap();
    for key in [
        "elapsedMs",
        "humanActiveMs",
        "planningMs",
        "approvalWaitMs",
        "executionAndClosureMs",
    ] {
        assert!(v[key].is_null(), "{key}");
    }
    let s = start(false);
    let records = complete()
        .into_iter()
        .filter(|m| !matches!(m.kind, Kind::HumanStarted | Kind::HumanStopped))
        .collect::<Vec<_>>();
    let v = summary(&s, &digest(&s).unwrap(), &records, true).unwrap();
    assert_eq!(v["elapsedMs"], 300);
    assert!(v["humanActiveMs"].is_null());
}
#[test]
fn overlapping_spans_bad_order_and_fabricated_recovery_are_rejected() {
    let s = start(true);
    for records in [
        vec![
            mark(Kind::HumanStarted, Some(1)),
            mark(Kind::HumanStarted, Some(2)),
        ],
        vec![mark(Kind::HumanStopped, Some(1))],
        vec![mark(Kind::Approved, Some(1))],
        vec![mark(Kind::Finished, Some(1))],
        vec![
            mark(Kind::Proposal, Some(10)),
            mark(Kind::Approved, Some(9)),
        ],
        vec![mark(Kind::Interrupted, Some(10))],
        vec![mark(Kind::Proposal, None)],
        vec![
            mark(Kind::Interrupted, None),
            mark(Kind::Proposal, Some(10)),
        ],
    ] {
        assert!(replay(&s, &records).is_err());
    }
    assert!(replay(&start(false), &[mark(Kind::HumanStarted, Some(1))]).is_err());
}
#[test]
fn immutable_journal_rejects_gaps_pending_files_and_chain_tampering() {
    let d = tempfile::tempdir().unwrap();
    let s = start(true);
    publish(d.path(), "start.json", &s).unwrap();
    let mut m = mark(Kind::Proposal, Some(10));
    m.previous = digest(&s).unwrap();
    m.history_length = 1;
    m.ledger_head = Some("c".repeat(64));
    publish(d.path(), "mark-0001.json", &m).unwrap();
    assert_eq!(marks(d.path(), &s).unwrap().len(), 1);
    fs::write(d.path().join("mark-0002.json.pending"), "partial").unwrap();
    assert!(marks(d.path(), &s).is_err());
    fs::remove_file(d.path().join("mark-0002.json.pending")).unwrap();
    fs::rename(
        d.path().join("mark-0001.json"),
        d.path().join("mark-0002.json"),
    )
    .unwrap();
    assert!(marks(d.path(), &s).is_err());
    fs::rename(
        d.path().join("mark-0002.json"),
        d.path().join("mark-0001.json"),
    )
    .unwrap();
    m.previous = "0".repeat(64);
    fs::write(
        d.path().join("mark-0001.json"),
        serde_json::to_vec(&m).unwrap(),
    )
    .unwrap();
    assert!(marks(d.path(), &s).is_err());
}

#[test]
fn human_interventions_across_execution_are_included_and_must_close_before_finish() {
    let s = start(true);
    let mut records = complete();
    records.pop();
    records.push(mark(Kind::HumanStarted, Some(220)));
    let mut unclosed = records.clone();
    unclosed.push(mark(Kind::Finished, Some(300)));
    assert!(replay(&s, &unclosed).is_err());
    records.push(mark(Kind::HumanStopped, Some(250)));
    records.push(mark(Kind::Finished, Some(300)));
    let v = summary(&s, &digest(&s).unwrap(), &records, true).unwrap();
    assert_eq!(v["humanActiveMs"], 65);
    assert_eq!(v["elapsedMs"], 300);
}
