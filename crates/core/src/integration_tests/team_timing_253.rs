use super::team_materials_250 as material;
use super::*;
use crate::host::team_observation::timing;
fn outer() -> PathBuf {
    PathBuf::from(std::env::var_os("CERULEAN_TEAM_250_DIR").unwrap())
}
fn save(name: &str, v: &impl serde::Serialize) {
    crate::api_execution::publish(&outer(), name, v).unwrap();
}
#[test]
#[ignore = "253 fresh fixed material Host; synthetic management/worker and real original timeline sources"]
fn full_timeline_measures_planning_review_and_original_accept() {
    let (b, mut h) = material::configured("P01");
    let id = &b.plan.project.id;
    h.bind_team_experiment_materials(&b.request).unwrap();
    material::keys(&mut h, true);
    let mut timeline = h
        .begin_timed_team_planning(id, &b.setup.digest, true)
        .unwrap();
    let selected = timeline.start_digest().to_owned();
    assert!(h
        .begin_timed_team_planning(id, &b.setup.digest, true)
        .is_err());
    timeline.set_human_active(&h, true).unwrap();
    std::thread::sleep(Duration::from_millis(8));
    timeline.set_human_active(&h, false).unwrap();
    spin(&mut h, |h| {
        h.projects[id].revision().is_some() && h.management_work_count() == 0
    });
    timeline.proposal_ready(&h).unwrap();
    assert!(timeline.proposal_ready(&h).is_err());
    assert!(timeline.start_approved_run(&mut h).is_err());
    timeline.set_human_active(&h, true).unwrap();
    std::thread::sleep(Duration::from_millis(8));
    timeline.set_human_active(&h, false).unwrap();
    let revision = b.plan.approval_digest().unwrap();
    timeline.approve_plan(&mut h, &revision).unwrap();
    timeline.start_approved_run(&mut h).unwrap();
    assert!(timeline.start_approved_run(&mut h).is_err());
    assert!(timeline.finish(&h).is_err());
    spin(&mut h, |h| {
        h.runs.all()[0].state != RunState::Active
            && h.api_active.is_empty()
            && h.active.is_empty()
            && h.management_work_count() == 0
    });
    let end = timeline.finish(&h).unwrap();
    assert_eq!(end["accepted"], true);
    assert!(end["elapsedMs"].as_u64().unwrap() > end["executionAndClosureMs"].as_u64().unwrap());
    assert!(end["humanActiveMs"].as_u64().unwrap() >= 16);
    assert_eq!(
        end["planningMs"].as_u64().unwrap()
            + end["approvalWaitMs"].as_u64().unwrap()
            + end["executionAndClosureMs"].as_u64().unwrap(),
        end["elapsedMs"]
    );
    assert!(timeline.finish(&h).is_err());
    material::keys(&mut h, false);
    save("accepted-terminal.json", &end);
    let history = h.projects[id].history().to_vec();
    drop(timeline);
    drop(h);
    let report = timing::inspect(Path::new(&b.request.material.data_root), id, &selected).unwrap();
    assert_eq!(report["execution"]["coreExport"]["apiReplies"], 8);
    save("accepted-inspection.json", &report);
    save(
        "accepted-selection.json",
        &json!({"dataRoot":b.request.material.data_root,"project":id,"startDigest":selected}),
    );
    for _ in 0..2 {
        let mut h = RuntimeHost::open(Path::new(&b.request.material.data_root)).unwrap();
        h.poll_verifications();
        assert_eq!(h.projects[id].history(), history);
        assert!(h
            .begin_timed_team_planning(id, &b.setup.digest, true)
            .is_err());
        assert!(h.close_interrupted_team_timeline(id, &selected).is_err());
    }
    // Only a new timing record is perturbed; all original execution evidence stays intact.
    let path = Path::new(&b.request.material.data_root)
        .join("team-timelines")
        .join(id)
        .join("terminal.json");
    let bytes = fs::read(&path).unwrap();
    let mut v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    v["elapsedMs"] = json!(1);
    fs::write(&path, serde_json::to_vec(&v).unwrap()).unwrap();
    let rejected =
        timing::inspect(Path::new(&b.request.material.data_root), id, &selected).is_err();
    fs::write(path, bytes).unwrap();
    assert!(rejected);
    save(
        "accepted-result.json",
        &json!({"accepted":true,"reopensWithoutRedispatch":2,"forgedTerminalRejected":true,"realModelCalls":0}),
    );
}
#[test]
#[ignore = "253 fresh planning-only Host; lost clock never synthesizes elapsed or retries work"]
fn lost_clock_during_review_preserves_unknown_time_and_original_proposal() {
    let (b, mut h) = material::configured("S01");
    let id = &b.plan.project.id;
    h.bind_team_experiment_materials(&b.request).unwrap();
    material::keys(&mut h, true);
    let mut timeline = h
        .begin_timed_team_planning(id, &b.setup.digest, true)
        .unwrap();
    let selected = timeline.start_digest().to_owned();
    spin(&mut h, |h| {
        h.projects[id].revision().is_some() && h.management_work_count() == 0
    });
    timeline.proposal_ready(&h).unwrap();
    timeline.set_human_active(&h, true).unwrap();
    drop(timeline);
    let original = h.root.join("team-observations");
    fs::create_dir(&original).unwrap();
    fs::create_dir(original.join(id)).unwrap();
    assert!(h.close_interrupted_team_timeline(id, &selected).is_err());
    assert!(!h
        .root
        .join("team-timelines")
        .join(id)
        .join("terminal.json")
        .exists());
    fs::remove_dir(original.join(id)).unwrap();
    fs::remove_dir(original).unwrap();
    let end = h.close_interrupted_team_timeline(id, &selected).unwrap();
    assert!(end["elapsedMs"].is_null() && end["humanActiveMs"].is_null());
    assert_eq!(end["accepted"], false);
    assert!(h.runs.all().is_empty());
    material::keys(&mut h, false);
    drop(h);
    let report = timing::inspect(Path::new(&b.request.material.data_root), id, &selected).unwrap();
    assert!(report["execution"].is_null());
    assert_eq!(report["terminal"], end);
    save("interrupted-inspection.json", &report);
    save(
        "interrupted-selection.json",
        &json!({"dataRoot":b.request.material.data_root,"project":id,"startDigest":selected}),
    );
}

#[test]
#[ignore = "253 read original251 evidence only; no Host, replay or dispatch"]
fn original_execution_observation_still_reads_identically() {
    let prior = outer().parent().unwrap().join("api-write-scope-251");
    let selected: serde_json::Value =
        serde_json::from_slice(&fs::read(prior.join("selection.json")).unwrap()).unwrap();
    let actual = crate::host::team_observation::inspect(
        Path::new(selected["dataRoot"].as_str().unwrap()),
        selected["project"].as_str().unwrap(),
        selected["startDigest"].as_str().unwrap(),
    )
    .unwrap();
    let original: serde_json::Value =
        serde_json::from_slice(&fs::read(prior.join("inspection.json")).unwrap()).unwrap();
    // As documented in 247, observedAt is the current read time, not execution time.
    let mut stable_actual = actual.clone();
    let mut stable_original = original;
    for report in [&mut stable_actual, &mut stable_original] {
        for verification in report["coreExport"]["verifications"]
            .as_array_mut()
            .unwrap()
        {
            let label = verification["detail"]
                .as_object_mut()
                .unwrap()
                .remove("observedAt")
                .unwrap();
            chrono::DateTime::parse_from_rfc3339(label.as_str().unwrap()).unwrap();
        }
    }
    assert_eq!(stable_actual, stable_original);
    save("original251-inspection.json", &actual);
}

#[test]
#[ignore = "253 original timeline reread only after activity extension; no Host or dispatch"]
fn original_timelines_remain_readable_after_execution_activity_extension() {
    let mut reports = vec![];
    for name in ["accepted", "interrupted"] {
        let selection: serde_json::Value = serde_json::from_slice(
            &fs::read(outer().join(format!("{name}-selection.json"))).unwrap(),
        )
        .unwrap();
        let report = timing::inspect(
            Path::new(selection["dataRoot"].as_str().unwrap()),
            selection["project"].as_str().unwrap(),
            selection["startDigest"].as_str().unwrap(),
        )
        .unwrap();
        let old: serde_json::Value = serde_json::from_slice(
            &fs::read(outer().join(format!("{name}-inspection.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(report["start"], old["start"]);
        assert_eq!(report["marks"], old["marks"]);
        assert_eq!(report["terminal"], old["terminal"]);
        reports.push(report);
    }
    save("activity-extension-inspections.json", &reports);
}
