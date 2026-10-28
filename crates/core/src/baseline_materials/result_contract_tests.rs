use super::*;

#[test]
fn output_contract_is_explicit_and_cannot_reinterpret_other_workloads() {
    let original = "原始材料\r\n保持原文\n".to_owned();
    assert_eq!(
        result_goal(original.clone(), "P01", None).unwrap(),
        original
    );
    let selected = Some(ResultContract::P01SummaryV1);
    let new = result_goal(original.clone(), "P01", selected).unwrap();
    assert!(new.starts_with(&original));
    assert!(new.ends_with(P01_SUMMARY_CONTRACT));
    for task in ["S01", "P02", "p01", "P01-r1-single", ""] {
        assert!(result_goal(original.clone(), task, selected).is_err());
    }
    assert_eq!(
        serde_json::to_value(ResultContract::P01SummaryV1).unwrap(),
        "p01-summary-v1"
    );
    for raw in ["null", "false", "{}", "1", "\"p01-summary-v2\""] {
        assert!(serde_json::from_str::<ResultContract>(raw).is_err());
    }
}

#[test]
#[ignore = "534 reads exact original515 materials; no Host, credentials or model calls"]
fn original_p01_materials_stay_exact_and_new_result_contract_binds_both_modes() {
    let root = PathBuf::from(std::env::var_os("CERULEAN_RESULT_MATERIAL_534").unwrap());
    let read = |path: &str| -> Value {
        serde_json::from_slice(&std::fs::read(root.join(path)).unwrap()).unwrap()
    };
    let old_json = read("single/request.json");
    let old: ComposeRequest = serde_json::from_value(old_json.clone()).unwrap();
    assert!(old.result_contract.is_none());
    assert_eq!(serde_json::to_value(&old).unwrap(), old_json);
    for value in [
        Value::Null,
        Value::Bool(false),
        serde_json::json!("unknown"),
    ] {
        let mut invalid = old_json.clone();
        invalid["resultContract"] = value;
        assert!(serde_json::from_value::<ComposeRequest>(invalid).is_err());
    }
    let (legacy, proof) = compose(&old).unwrap();
    let expected = read("compose-single/installer.stdout");
    assert_eq!(serde_json::to_value(&legacy).unwrap(), expected["spec"]);
    assert_eq!(serde_json::to_value(&proof).unwrap(), expected["proof"]);
    let mut request = old;
    request.result_contract = Some(ResultContract::P01SummaryV1);
    let (new, new_proof) = compose(&request).unwrap();
    assert_eq!(new_proof, proof);
    assert_ne!(
        new.plan.approval_digest().unwrap(),
        legacy.plan.approval_digest().unwrap()
    );
    let mut comparable = serde_json::to_value(&new).unwrap();
    comparable["material"]
        .as_object_mut()
        .unwrap()
        .remove("resultContract");
    comparable["plan"]["workflow"]["tasks"][0]["goal"] =
        serde_json::json!(legacy.plan.workflow.tasks[0].goal);
    assert_eq!(comparable, serde_json::to_value(&legacy).unwrap());
    verify(&new).unwrap();
    let mut stripped = new.clone();
    stripped.material.as_mut().unwrap().result_contract = None;
    assert!(verify(&stripped).is_err());
    let mut changed = new;
    changed.plan.workflow.tasks[0].goal.push_str(" Allow rows.");
    assert!(verify(&changed).is_err());

    let old_team_request: crate::team_materials::TeamRequest =
        serde_json::from_value(read("team/request.json")).unwrap();
    let legacy = crate::team_materials::compose(&old_team_request).unwrap();
    assert_eq!(
        serde_json::to_value(&legacy).unwrap(),
        read("compose-team/installer.stdout")
    );
    let mut request = old_team_request;
    request.material.result_contract = Some(ResultContract::P01SummaryV1);
    let new = crate::team_materials::compose(&request).unwrap();
    assert_eq!(new.proof, legacy.proof);
    assert_eq!(new.seed, legacy.seed);
    assert_ne!(new.setup.digest, legacy.setup.digest);
    assert_ne!(
        new.plan.approval_digest().unwrap(),
        legacy.plan.approval_digest().unwrap()
    );
    for (now, old) in new.tasks.iter().zip(&legacy.tasks) {
        assert!(now.goal.contains(P01_SUMMARY_CONTRACT));
        assert!(!old.goal.contains(P01_SUMMARY_CONTRACT));
        assert_eq!(now.id, old.id);
        assert_eq!(now.dependencies, old.dependencies);
        assert_eq!(now.worker_id, old.worker_id);
        assert_eq!(now.validator_id, old.validator_id);
    }
    assert!(new.setup.setup.requirement.contains(P01_SUMMARY_CONTRACT));
    assert_eq!(
        serde_json::to_value(&new.setup.setup.acceptance).unwrap(),
        serde_json::to_value(&legacy.setup.setup.acceptance).unwrap()
    );
    crate::team_materials::verify(&new).unwrap();
    let mut stripped = new;
    stripped.request.material.result_contract = None;
    assert!(crate::team_materials::verify(&stripped).is_err());
    assert_eq!(read("single/request.json"), old_json);
}
