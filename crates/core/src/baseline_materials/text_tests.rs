use super::*;

#[test]
fn text_format_is_explicit_and_only_changes_line_endings() {
    let text = "原文\r\nsecond\rthird\nlast\u{2028}line\t".to_owned();
    assert_eq!(task_goal(text.clone(), None), text);
    let expected = "原文\nsecond\nthird\nlast\u{2028}line\t";
    assert_eq!(task_goal(text, Some(TaskTextFormat::LfV1)), expected);
    assert_eq!(
        task_goal(expected.into(), Some(TaskTextFormat::LfV1)),
        expected
    );
    assert_eq!(serde_json::to_value(TaskTextFormat::LfV1).unwrap(), "lf-v1");
    for bad in ["null", "false", "1", "{}", "\"lf-v2\"", "\"LF-V1\""] {
        assert!(serde_json::from_str::<TaskTextFormat>(bad).is_err());
    }
}

#[test]
#[ignore = "explicit original513 material paths; read-only composition, no Host or model"]
fn original_materials_remain_exact_and_new_text_format_is_bound() {
    let root = PathBuf::from(std::env::var_os("CERULEAN_TEXT_MATERIAL_514").unwrap());
    let raw = std::fs::read(root.join("single/request.json")).unwrap();
    let old: ComposeRequest = serde_json::from_slice(&raw).unwrap();
    assert!(old.task_text_format.is_none());
    let original: Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(serde_json::to_value(&old).unwrap(), original);
    for bad in [Value::Null, Value::Bool(false), serde_json::json!("lf-v2")] {
        let mut invalid = original.clone();
        invalid["taskTextFormat"] = bad;
        assert!(serde_json::from_value::<ComposeRequest>(invalid).is_err());
    }
    let baseline: Value =
        serde_json::from_slice(&std::fs::read(root.join("compose-single.stdout")).unwrap())
            .unwrap();
    let (legacy, proof) = compose(&old).unwrap();
    assert_eq!(serde_json::to_value(&legacy).unwrap(), baseline["spec"]);
    assert_eq!(serde_json::to_value(&proof).unwrap(), baseline["proof"]);
    let mut request = old.clone();
    request.task_text_format = Some(TaskTextFormat::LfV1);
    let (new, new_proof) = compose(&request).unwrap();
    assert_eq!(new_proof, proof);
    assert_eq!(new.seed, legacy.seed);
    assert_eq!(
        new.plan.workflow.tasks[0].goal,
        legacy.plan.workflow.tasks[0].goal.replace("\r\n", "\n")
    );
    assert!(!new.plan.workflow.tasks[0].goal.contains('\r'));
    verify(&new).unwrap();
    let mut stripped = new.clone();
    stripped.material.as_mut().unwrap().task_text_format = None;
    assert!(verify(&stripped).is_err());

    let old_request: crate::team_materials::TeamRequest =
        serde_json::from_slice(&std::fs::read(root.join("team/request-canonical.json")).unwrap())
            .unwrap();
    let expected: Value =
        serde_json::from_slice(&std::fs::read(root.join("compose-team-canonical.stdout")).unwrap())
            .unwrap();
    let old_team = crate::team_materials::compose(&old_request).unwrap();
    assert_eq!(serde_json::to_value(&old_team).unwrap(), expected);
    let mut request = old_request;
    request.material.task_text_format = Some(TaskTextFormat::LfV1);
    let team = crate::team_materials::compose(&request).unwrap();
    assert_eq!(team.proof, old_team.proof);
    assert_eq!(team.seed, old_team.seed);
    assert!(!team.setup.setup.requirement.contains('\r'));
    assert_ne!(team.setup.digest, old_team.setup.digest);
    assert_ne!(
        team.plan.approval_digest().unwrap(),
        old_team.plan.approval_digest().unwrap()
    );
    for (now, old) in team.tasks.iter().zip(&old_team.tasks) {
        assert_eq!(now.goal, old.goal.replace("\r\n", "\n"));
        assert!(!now.goal.contains('\r'));
        assert_eq!(now.worker_id, old.worker_id);
        assert_eq!(now.validator_id, old.validator_id);
        assert_eq!(now.dependencies, old.dependencies);
    }
    crate::team_materials::verify(&team).unwrap();
    let mut stripped = team.clone();
    stripped.request.material.task_text_format = None;
    assert!(crate::team_materials::verify(&stripped).is_err());
    let mut changed = team;
    changed.plan.workflow.tasks[0].goal.push('\r');
    assert!(crate::team_materials::verify(&changed).is_err());
    assert_eq!(
        std::fs::read(root.join("single/request.json")).unwrap(),
        raw
    );
}
