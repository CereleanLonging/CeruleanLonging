use cerulean_core::handoff::*;
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    name: String,
    events: Vec<HandoffEvent>,
    options: HandoffOptions,
    expected: Option<Vec<DailyVolume>>,
    error: Option<String>,
}
#[test]
fn daily_volumes_match_prototype_bytes_and_rejections() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../tests/fixtures/handoff-parity-v1.json"
    ))
    .unwrap();
    assert_eq!(fixture.cases.len(), 18);
    for case in fixture.cases {
        let result = project_handoffs(&case.events, &case.options);
        if case.error.is_some() {
            assert!(result.is_err(), "{} should reject", case.name);
        } else {
            assert_eq!(result.unwrap(), case.expected.unwrap(), "{}", case.name);
        }
    }
}
#[test]
fn invalid_calendar_and_non_ascii_timestamp_are_rejected() {
    let mut event = HandoffEvent {
        sequence: 1,
        message_id: "m".into(),
        task_id: "t".into(),
        attempt_id: "a".into(),
        actor: "w".into(),
        received_at: "2026-02-30T00:00:00Z".into(),
        occurred_at: "2026-10-07T00:00:00Z".into(),
        status: HandoffStatus::Running,
        summary: "".into(),
        next_action: "".into(),
        evidence: vec![],
    };
    // Deliberate tightening: JavaScript Date.parse can normalize impossible dates.
    for time in [
        "2026-02-30T00:00:00Z",
        "2026-10-07T00:00:60Z",
        "2026-10-07T00:00:00.1234Z",
        "年月日日日日日日日日日日日日日日日日日日日日日",
    ] {
        event.received_at = time.into();
        assert!(project_handoffs(&[event.clone()], &HandoffOptions::default()).is_err());
    }
}
