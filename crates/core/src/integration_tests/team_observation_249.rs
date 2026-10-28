use super::*;
use crate::host::team_observation;
fn outer() -> PathBuf {
    PathBuf::from(std::env::var_os("CERULEAN_TEAM_249_DIR").unwrap())
}
fn unique_worker(request: &PreparedRequest, round: u32) -> Result<ApiReply, ApiFailure> {
    let ApiMessage::User(prompt) = &fixture_messages(request)[0] else {
        panic!("missing prompt")
    };
    worker_named(
        request,
        round,
        &format!("team249-{}-{round}", &digest(prompt).unwrap()[..20]),
    )
}
fn setup(name: &str) -> (PathBuf, RuntimeHost, PlanRevision, String) {
    let root = outer().join(name);
    fs::create_dir(&root).unwrap();
    let mut host = configured(&root, "success");
    host.api_fixture = Some(unique_worker);
    let original_setup = host.project_setups["project"].digest.clone();
    let strict_setup = host
        .prepare_team_experiment_inputs("project", &original_setup)
        .unwrap();
    assert_ne!(strict_setup, original_setup);
    assert!(host
        .prepare_team_experiment_inputs("project", &original_setup)
        .is_err());
    assert_eq!(
        host.prepare_team_experiment_inputs("project", &strict_setup)
            .unwrap(),
        strict_setup
    );
    // This observation requires the working seed and committed raw input to agree.
    fs::write(root.join("project/base.txt"), "committed dependency base\n").unwrap();
    let plan = planned(&mut host);
    assert!(host
        .prepare_team_experiment_inputs("project", &strict_setup)
        .is_err());
    let policy = plan.worktree.as_ref().unwrap();
    drop(
        crate::git_workspaces::GitWorkspaces::open(Path::new(&policy.pool_root), &policy.git)
            .unwrap(),
    );
    let saved = host.project_setups["project"].digest.clone();
    (root, host, plan, saved)
}
fn clear(host: &mut RuntimeHost) {
    host.set_openai_key(None).unwrap();
    host.set_provider_key(CredentialProvider::DeepSeek, None)
        .unwrap();
}
fn save(root: &Path, name: &str, value: &impl serde::Serialize) {
    crate::api_execution::publish(root, name, value).unwrap();
}

#[test]
#[ignore = "249 isolated Host/Git/tools/checks; synthetic management and API only"]
fn successful_team_observation_preserves_sources_and_original_execution() {
    let (root, mut host, plan, saved) = setup("accepted");
    let revision = plan.approval_digest().unwrap();
    host.approve("project", &revision).unwrap();
    let observation = host
        .start_observed_team_run("project", &revision, &saved)
        .unwrap();
    assert!(observation.launch_error().is_none());
    let selected = observation.start_digest().to_owned();
    save(
        &root,
        "selection.json",
        &json!({"dataRoot":host.root,"project":"project","startDigest":selected}),
    );
    assert!(observation.finish(&host).is_err());
    assert!(host
        .start_observed_team_run("project", &revision, &saved)
        .is_err());
    assert!(team_observation::inspect(&host.root, "project", &selected).is_err());
    spin(&mut host, |h| {
        h.runs.all()[0].state != RunState::Active
            && h.api_active.is_empty()
            && h.active.is_empty()
            && h.management_work_count() == 0
    });
    let terminal = observation.finish(&host).unwrap();
    save(&root, "terminal-result.json", &terminal);
    assert_eq!(terminal["accepted"], true);
    assert!(terminal["elapsedMs"].as_u64().unwrap() > 0);
    assert!(observation.finish(&host).is_err());
    let history = host.projects["project"].history().to_vec();
    clear(&mut host);
    drop(host);
    let report = team_observation::inspect(&root.join("data"), "project", &selected).unwrap();
    assert_eq!(
        report["started"]["management"]["operations"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        report["coreExport"]["management"]["operations"]
            .as_array()
            .unwrap()
            .len(),
        7
    );
    assert_eq!(report["coreExport"]["apiReplies"], 10);
    assert_eq!(report["experimentRecordReady"], false);
    save(&root, "inspection.json", &report);
    for _ in 0..2 {
        let mut host = RuntimeHost::open(&root.join("data")).unwrap();
        host.poll_verifications();
        assert_eq!(host.projects["project"].history(), history);
        assert!(host.api_fixture.is_none() && host.management_fixture.is_none());
        assert!(host
            .start_observed_team_run("project", &revision, &saved)
            .is_err());
    }
    save(
        &root,
        "result.json",
        &json!({"accepted":true,"originalWorkerReplies":10,"originalManagerOperations":7,
        "preRunManagerOperations":1,"reopensWithoutRedispatch":2,"realModelCalls":0}),
    );
}

#[test]
#[ignore = "249 isolated Host; synthetic failure and intentionally absent credentials"]
fn start_failure_and_manager_failure_keep_observations_without_retry() {
    for name in ["missing-credentials", "manager-failure"] {
        let (root, mut host, plan, saved) = setup(name);
        let revision = plan.approval_digest().unwrap();
        host.approve("project", &revision).unwrap();
        if name == "missing-credentials" {
            clear(&mut host);
        } else {
            host.management_fixture = Some(|_| Err("synthetic manager transport failure".into()));
        }
        let observation = host
            .start_observed_team_run("project", &revision, &saved)
            .unwrap();
        if name == "manager-failure" {
            spin(&mut host, |h| {
                h.runs.all()[0].state != RunState::Active && h.management_work_count() == 0
            });
        } else {
            assert!(observation.launch_error().is_some());
        }
        let end = observation.finish(&host).unwrap();
        assert_eq!(end["accepted"], false);
        assert!(host
            .start_observed_team_run("project", &revision, &saved)
            .is_err());
        let selected = observation.start_digest().to_owned();
        clear(&mut host);
        drop(host);
        let report = team_observation::inspect(&root.join("data"), "project", &selected).unwrap();
        assert_eq!(report["coreExport"]["apiIntentCount"], 0);
        save(
            &root,
            "selection.json",
            &json!({"dataRoot":root.join("data"),"project":"project","startDigest":selected}),
        );
        save(&root, "inspection.json", &report);
    }
}

#[test]
#[ignore = "249 lost observation token; paused before dispatch, no model calls after planning"]
fn lost_clock_stays_unknown_and_never_resumes_original_run() {
    let (root, mut host, plan, saved) = setup("lost-clock");
    let revision = plan.approval_digest().unwrap();
    host.approve("project", &revision).unwrap();
    let observation = host
        .start_observed_team_run("project", &revision, &saved)
        .unwrap();
    let selected = observation.start_digest().to_owned();
    let run = host.runs.all()[0].id.clone();
    host.pause_project_run(&run).unwrap();
    clear(&mut host);
    drop(observation);
    drop(host);
    let unfinished = team_observation::inspect(&root.join("data"), "project", &selected).unwrap();
    assert_eq!(unfinished["status"], "started-without-terminal");
    assert!(unfinished["elapsedMs"].is_null());
    let host = RuntimeHost::open(&root.join("data")).unwrap();
    let end = host
        .close_interrupted_team_observation("project", &selected)
        .unwrap();
    assert!(end["elapsedMs"].is_null());
    assert_eq!(end["clockContinuity"], false);
    assert_eq!(end["runState"], "paused");
    drop(host);
    let report = team_observation::inspect(&root.join("data"), "project", &selected).unwrap();
    assert_eq!(report["coreExport"]["apiIntentCount"], 0);
    save(
        &root,
        "selection.json",
        &json!({"dataRoot":root.join("data"),"project":"project","startDigest":selected}),
    );
    save(&root, "inspection.json", &report);
}

#[test]
#[ignore = "249 pre-start scope, seed, approval and partial directory guards"]
fn prestart_guards_do_not_grant_runs() {
    let (root, mut host, plan, saved) = setup("guards");
    let revision = plan.approval_digest().unwrap();
    assert!(host
        .start_observed_team_run("project", &revision, &saved)
        .is_err());
    host.approve("project", &revision).unwrap();
    assert!(host
        .start_observed_team_run("project", &"0".repeat(64), &saved)
        .is_err());
    assert!(host
        .start_observed_team_run("project", &revision, &"0".repeat(64))
        .is_err());
    fs::write(root.join("project/base.txt"), "different working input").unwrap();
    assert!(host
        .start_observed_team_run("project", &revision, &saved)
        .is_err());
    fs::write(root.join("project/base.txt"), "committed dependency base\n").unwrap();
    assert!(!host.root.join("team-observations").exists());
    fs::create_dir_all(host.root.join("team-observations/project")).unwrap();
    fs::write(
        host.root
            .join("team-observations/project/started.json.pending"),
        "interrupted",
    )
    .unwrap();
    assert!(host
        .start_observed_team_run("project", &revision, &saved)
        .is_err());
    assert!(host.runs.all().is_empty());
    assert!(host.api_startups.is_empty());
    clear(&mut host);
}
