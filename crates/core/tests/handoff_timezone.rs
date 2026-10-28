use cerulean_core::handoff::*;

fn event(n: u64, at: &str) -> HandoffEvent {
    HandoffEvent {
        sequence: n,
        message_id: format!("m{n}"),
        task_id: format!("t{n}"),
        attempt_id: format!("a{n}"),
        actor: "worker".into(),
        received_at: at.into(),
        occurred_at: at.into(),
        status: HandoffStatus::Running,
        summary: "work".into(),
        next_action: "continue".into(),
        evidence: vec![],
    }
}
fn render(events: &[HandoffEvent], zone: &str) -> Vec<DailyVolume> {
    project_handoffs_in_timezone(events, &HandoffOptions::default(), zone).unwrap()
}

#[test]
fn timezone_midnight_late_arrival_and_unfinished_index_use_receipt_day() {
    let first = event(1, "2026-01-02T04:59:59Z");
    let mut second = event(2, "2026-01-02T05:00:00Z");
    second.occurred_at = first.received_at.clone();
    let volumes = render(&[first, second], "America/New_York");
    assert_eq!(volumes.len(), 2);
    assert_eq!(volumes[0].day, "2026-01-01");
    assert!(volumes[0].sealed);
    assert_eq!(volumes[1].day, "2026-01-02");
    assert!(volumes[1].path.ends_with("_001_000000-0500.md"));
    assert!(volumes[1]
        .markdown
        .contains("跨日未完成任务：\n\n- t1：running"));
    assert!(volumes[1].markdown.contains("跨日到达，按接收日归档。"));
}

#[test]
fn timezone_dst_spring_skips_hour_and_fall_does_not_reverse_receipts() {
    let spring = render(
        &[
            event(1, "2026-03-08T06:59:59Z"),
            event(2, "2026-03-08T07:00:00Z"),
        ],
        "America/New_York",
    );
    assert_eq!(spring.len(), 1);
    assert!(spring[0].markdown.contains("01:59:59.000-05:00"));
    assert!(spring[0].markdown.contains("03:00:00.000-04:00"));
    let fall = project_handoffs_in_timezone(
        &[
            event(1, "2026-11-01T05:30:00Z"),
            event(2, "2026-11-01T06:30:00Z"),
        ],
        &HandoffOptions {
            max_new_tasks: 1,
            ..Default::default()
        },
        "America/New_York",
    )
    .unwrap();
    assert_eq!(fall.len(), 2);
    assert!(fall[0].path.ends_with("_001_013000-0400.md"));
    assert!(fall[1].path.ends_with("_002_013000-0500.md"));
}

#[test]
fn timezone_fractional_offsets_and_utc_are_encoded_in_filename() {
    for (zone, suffix) in [
        ("Asia/Kathmandu", "_054500+0545.md"),
        ("America/St_Johns", "_203000-0330.md"),
        ("UTC", "_000000+0000.md"),
    ] {
        assert!(render(&[event(1, "2026-01-02T00:00:00Z")], zone)[0]
            .path
            .ends_with(suffix));
    }
}

#[test]
fn timezone_invalid_names_rejected_even_for_empty_ledger() {
    for zone in ["", "local", "../UTC", "America/Nowhere", " UTC ", "+08:00"] {
        assert!(project_handoffs_in_timezone(&[], &HandoffOptions::default(), zone).is_err());
    }
}

#[test]
fn timezone_task_and_byte_thresholds_preserve_same_task_semantics() {
    let first = event(1, "2026-01-01T10:00:00Z");
    let mut update = event(2, "2026-01-01T10:00:01Z");
    update.task_id = first.task_id.clone();
    let third = event(3, "2026-01-01T10:00:02Z");
    let inputs = [first, update, third];
    let volumes = project_handoffs_in_timezone(
        &inputs,
        &HandoffOptions {
            max_new_tasks: 1,
            ..Default::default()
        },
        "Europe/London",
    )
    .unwrap();
    assert_eq!(volumes[0].event_ids, ["m1", "m2"]);
    assert_eq!(volumes[1].event_ids, ["m3"]);
    let mut large = event(1, "2026-01-01T10:00:00Z");
    large.summary = "x".repeat(220);
    let mut next = large.clone();
    next.sequence = 2;
    next.message_id = "m2".into();
    next.task_id = "t2".into();
    let limited = HandoffOptions {
        max_bytes: 650,
        ..Default::default()
    };
    let volumes = project_handoffs_in_timezone(&[large.clone(), next], &limited, "UTC").unwrap();
    assert_eq!(volumes.len(), 2);
    assert!(volumes.iter().all(|v| v.markdown.len() <= 650));
    large.summary = "x".repeat(700);
    assert!(project_handoffs_in_timezone(&[large], &limited, "UTC").is_err());
}

#[test]
fn timezone_legacy_shanghai_retains_fixed_offset_even_for_historical_dst() {
    let input = [event(1, "1988-07-01T00:00:00Z")];
    let old = project_handoffs(&input, &HandoffOptions::default()).unwrap();
    assert!(old[0].path.ends_with("_080000+0800.md"));
    let iana = render(&input, "Asia/Shanghai");
    assert!(iana[0].path.ends_with("_090000+0900.md"));
}
