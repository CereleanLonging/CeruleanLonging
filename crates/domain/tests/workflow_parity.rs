use cerulean_domain::workflow::*;
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct Fixture {
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    name: String,
    principals: Vec<Principal>,
    steps: Vec<Step>,
}
#[derive(Deserialize)]
struct Step {
    operation: Value,
    accepted: bool,
    state: Value,
    journal: Value,
}

#[test]
fn prototype_cases_match_every_transition_and_rejection() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../tests/fixtures/workflow-parity-v1.json"
    ))
    .unwrap();
    assert!(fixture.cases.len() >= 12);
    for case in fixture.cases {
        let mut core = Workflow::new(case.principals.clone()).unwrap();
        for (i, step) in case.steps.into_iter().enumerate() {
            let before = core.state().clone();
            let journal = core.journal().to_vec();
            let result = if step.operation.get("restore") == Some(&Value::Bool(true)) {
                Workflow::restore(case.principals.clone(), core.journal(), "core").map(|restored| {
                    core = restored;
                })
            } else {
                let principal = step.operation["principal"].as_str().unwrap();
                let command: WorkflowCommand =
                    serde_json::from_value(step.operation["command"].clone()).unwrap();
                core.apply(principal, command).map(|_| ())
            };
            assert_eq!(
                result.is_ok(),
                step.accepted,
                "{} step {i}: {result:?}",
                case.name
            );
            if result.is_err() {
                assert_eq!(core.state(), &before);
                assert_eq!(core.journal(), journal);
            }
            assert_eq!(
                serde_json::to_value(core.state()).unwrap(),
                step.state,
                "{} step {i} state",
                case.name
            );
            assert_eq!(
                serde_json::to_value(core.journal()).unwrap(),
                step.journal,
                "{} step {i} journal",
                case.name
            );
        }
    }
}

#[test]
fn rejects_invalid_bindings_and_sequence_gaps() {
    assert!(Workflow::new(vec![Principal {
        id: "chief".into(),
        role: Role::Chief,
        provider: Some("other".into())
    }])
    .is_err());
    let p = Principal {
        id: "core".into(),
        role: Role::Core,
        provider: None,
    };
    assert!(Workflow::new(vec![p.clone(), p.clone()]).is_err());
    let e = WorkflowEvent {
        sequence: 2,
        principal: "core".into(),
        command: WorkflowCommand::Approve {
            digest: "fake".into(),
        },
    };
    assert!(Workflow::replay(vec![p], &[e]).is_err());
}
