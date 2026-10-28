use super::*;
use crate::{
    team_materials::{self, TeamBlueprint},
    workload_fixture_252 as fixture,
};
use std::sync::OnceLock;
static BLUEPRINT: OnceLock<TeamBlueprint> = OnceLock::new();

fn fixture_manager(
    intent: crate::management_execution::DecisionIntent,
) -> Result<DecisionObservation, String> {
    let mut result = manager(intent)?;
    if let ManagementDecision::Plan { tasks, .. } = &mut result.envelope.decision {
        *tasks = BLUEPRINT.get().unwrap().tasks.clone();
    }
    Ok(result)
}
#[test]
#[ignore = "252 explicit isolated seed-only slot; synthetic managers/worker and real Host/files/checker"]
fn missing_team_workload_accepts_and_reopens() {
    assert!(fixture::slot().ends_with("-team"));
    let request =
        serde_json::from_slice(&fs::read(fixture::area().join("request.json")).unwrap()).unwrap();
    let b = team_materials::compose(&request).unwrap();
    assert!(BLUEPRINT.set(b.clone()).is_ok());
    let id = &b.plan.project.id;
    let mut host = RuntimeHost::open(Path::new(&b.request.material.data_root)).unwrap();
    host.register(b.plan.project.clone()).unwrap();
    let s = &b.setup.setup;
    let mut validators = vec![(s.validator_id.clone(), s.check.tool.clone().unwrap())];
    for p in &s.profiles {
        let stage = &p.acceptance.stages[0];
        validators.push((
            stage.validator_id.clone(),
            stage.check.tool.clone().unwrap(),
        ));
    }
    for (id, tool) in validators {
        host.register_validator(
            id.clone(),
            id,
            tool.executable.path,
            tool.inputs.into_iter().map(|f| f.path).collect(),
            tool.environment,
        )
        .unwrap();
    }
    host.configure_project(b.configuration_draft()).unwrap();
    let before = host.project_setups[id].digest.clone();
    host.prepare_team_experiment_inputs(id, &before).unwrap();
    assert_eq!(host.project_setups[id].digest, b.setup.digest);
    assert!(b
        .plan
        .execution
        .iter()
        .all(|t| t.api_worker.as_ref().unwrap().write_scope.is_some()));
    host.management_fixture = Some(fixture_manager);
    host.api_fixture = Some(fixture::worker);
    host.bind_team_experiment_materials(&b.request).unwrap();
    super::team_materials_250::keys(&mut host, true);
    super::team_materials_250::plan(&mut host, &b);
    let revision = b.plan.approval_digest().unwrap();
    host.approve(id, &revision).unwrap();
    let observation = host
        .start_observed_team_run(id, &revision, &b.setup.digest)
        .unwrap();
    assert!(observation.launch_error().is_none());
    let selected = observation.start_digest().to_owned();
    fixture::save(
        "selection.json",
        &json!({"dataRoot":host.root,"project":id,"startDigest":selected}),
    );
    spin(&mut host, |h| {
        h.runs.all()[0].state != RunState::Active
            && h.api_active.is_empty()
            && h.active.is_empty()
            && h.management_work_count() == 0
    });
    let end = observation.finish(&host).unwrap();
    super::team_materials_250::keys(&mut host, false);
    fixture::save("terminal.json", &end);
    assert_eq!(end["accepted"], true);
    let history = host.projects[id].history().to_vec();
    drop(host);
    let report = crate::host::team_observation::inspect(
        Path::new(&b.request.material.data_root),
        id,
        &selected,
    )
    .unwrap();
    let parallel = fixture::slot().starts_with('P');
    let expected_replies = if parallel { 8 } else { 2 };
    assert_eq!(report["materialContentBound"], true);
    assert_eq!(report["coreExport"]["apiReplies"], expected_replies);
    assert_eq!(
        report["coreExport"]["verifications"]
            .as_array()
            .unwrap()
            .len(),
        if parallel { 3 } else { 1 }
    );
    fixture::save("inspection.json", &report);
    for _ in 0..2 {
        let mut h = RuntimeHost::open(Path::new(&b.request.material.data_root)).unwrap();
        h.poll_verifications();
        assert_eq!(h.projects[id].history(), history);
        assert!(h
            .start_observed_team_run(id, &revision, &b.setup.digest)
            .is_err());
    }
    fixture::save(
        "accepted-result.json",
        &json!({"accepted":true,"slot":fixture::slot(),
        "workerReplies":expected_replies,"managementOperations":if parallel {7} else {3},
        "independentChecks":if parallel {3} else {1},"reopensWithoutRedispatch":2,
        "realModelCalls":0,"writeScope":"declared-artifacts"}),
    );
}
