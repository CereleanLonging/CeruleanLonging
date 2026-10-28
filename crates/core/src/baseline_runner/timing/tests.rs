use super::*;
use cerulean_domain::secret::SecretInput;
use std::time::Duration;

fn outer() -> PathBuf {
    PathBuf::from(std::env::var_os("CERULEAN_BASELINE_254_DIR").unwrap())
}
fn area(task: &str) -> PathBuf {
    outer().join(format!("fixtures/{task}-r1-single"))
}
fn begin(task: &str) -> TimedBaseline {
    let root = area(task);
    let req: ComposeRequest =
        serde_json::from_slice(&fs::read(root.join("request.json")).unwrap()).unwrap();
    TimedBaseline::begin(&root.join("timeline"), req, true).unwrap()
}
fn authorize(t: &mut TimedBaseline) -> RunSpec {
    let data = guard(&t.inner).unwrap().start.data_root.clone();
    let spec: RunSpec = load(Path::new(&data), "run-spec.json").unwrap();
    t.approve(approval(&spec)).unwrap();
    spec
}
fn approval(spec: &RunSpec) -> RunApproval {
    RunApproval {
        spec_digest: digest(spec).unwrap(),
        approval: Approval {
            plan_id: spec.plan.workflow.id.clone(),
            revision: spec.plan.workflow.version,
            policy_version: spec.plan.policy.version,
            revision_digest: spec.plan.approval_digest().unwrap(),
            user_id: "user".into(),
            approved_at: chrono::Utc::now().to_rfc3339(),
        },
    }
}
fn save(name: &str, v: &impl Serialize) {
    publish(&outer(), name, v).unwrap();
}
fn client(_: &RunSpec) -> Result<ApiClient, String> {
    ApiClient::new(SecretInput::new("synthetic-254-not-a-key".into()).unwrap())
}
fn record(kind: Kind, offset: u64) -> Mark {
    Mark {
        sequence: 1,
        previous: "unused".into(),
        kind,
        offset_ms: Some(offset),
        observed_at: "2026-01-01T00:00:00Z".into(),
        history_length: 0,
        ledger_head: None,
    }
}

#[test]
fn known_stop_preserves_clock_but_cannot_abort_started_execution() {
    let r = summary_for(true, "test", &[record(Kind::Aborted, 100)], false, "test").unwrap();
    assert_eq!(r["elapsedMs"], 100);
    assert_eq!(r["planningMs"], 100);
    let r = summary_for(
        true,
        "test",
        &[record(Kind::Proposal, 100), record(Kind::Aborted, 150)],
        false,
        "test",
    )
    .unwrap();
    assert_eq!(r["approvalWaitMs"], 50);
    assert!(replay_for(
        true,
        &[record(Kind::HumanStarted, 1), record(Kind::Aborted, 2)]
    )
    .is_err());
    assert!(replay_for(
        true,
        &[
            record(Kind::Proposal, 1),
            record(Kind::Approved, 2),
            record(
                Kind::Execution {
                    start_digest: "a".repeat(64)
                },
                3
            ),
            record(Kind::Aborted, 4)
        ]
    )
    .is_err());
}

#[tokio::test]
#[ignore = "254 explicit fresh seed-only fixture; synthetic transport, original checks"]
async fn full_baseline_clock_and_execution_activity_bind_original_accept() {
    let mut t = begin("S02");
    let selected = t.start_digest().unwrap();
    let activity = t.activity();
    activity.set_active(true).unwrap();
    std::thread::sleep(Duration::from_millis(3));
    activity.set_active(false).unwrap();
    t.prepare().unwrap();
    assert!(t.prepare().is_err());
    let spec = authorize(&mut t);
    assert!(t.approve(approval(&spec)).is_err());
    let (_tx, mut rx) = oneshot::channel();
    let mut calls = 0;
    let end =
        t
            .execute::<Result<
                cerulean_adapters::api_generation::ApiReply,
                cerulean_adapters::api_generation::ApiFailure,
            >>(
                "synthetic-fixture",
                client,
                &mut async |_: &ApiClient,
                            request,
                            _: &mut oneshot::Receiver<()>,
                            _gateway: &mut cerulean_adapters::api_generation::GatewayObserver<
                    '_,
                >| {
                    calls += 1;
                    if calls == 1 {
                        activity.set_active(true).unwrap();
                        std::thread::sleep(Duration::from_millis(3));
                        activity.set_active(false).unwrap();
                    }
                    crate::workload_fixture_252::worker(&request, calls)
                },
                &mut rx,
                &AtomicBool::new(false),
            )
            .await
            .unwrap();
    assert!(end.accepted);
    assert_eq!(calls, 2);
    activity.set_active(true).unwrap();
    assert!(t.finish().is_err());
    activity.set_active(false).unwrap();
    let timed = t.finish().unwrap();
    assert_eq!(timed["accepted"], true);
    assert!(timed["humanActiveMs"].as_u64().unwrap() >= 6);
    assert_eq!(
        timed["planningMs"].as_u64().unwrap()
            + timed["approvalWaitMs"].as_u64().unwrap()
            + timed["executionAndClosureMs"].as_u64().unwrap(),
        timed["elapsedMs"]
    );
    assert_eq!(timed["managementModelRequests"], 0);
    assert!(t.finish().is_err());
    assert!(
        t
            .execute::<Result<
                cerulean_adapters::api_generation::ApiReply,
                cerulean_adapters::api_generation::ApiFailure,
            >>(
                "synthetic-fixture",
                |_| panic!("no credential read on repeat"),
                &mut async |_: &ApiClient,
                            _,
                            _: &mut oneshot::Receiver<()>,
                            _gateway: &mut cerulean_adapters::api_generation::GatewayObserver<
                    '_,
                >| { panic!("no repeated send") },
                &mut rx,
                &AtomicBool::new(false)
            )
            .await
            .is_err()
    );
    drop(t);
    assert!(inspect(&area("S02").join("timeline"), &selected).is_err());
    drop(activity);
    let path = area("S02").join("timeline");
    let first = inspect(&path, &selected).unwrap();
    assert_eq!(first["sources"]["baseline"]["coreExport"]["apiReplies"], 2);
    assert_eq!(
        first["sources"]["baseline"]["coreExport"]["verifications"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    for _ in 0..2 {
        let reopened = inspect(&path, &selected).unwrap();
        for key in ["start", "marks", "terminal"] {
            assert_eq!(reopened[key], first[key]);
        }
        assert!(close_interrupted(&path, &selected).is_err());
    }
    // Perturb only this new fixture and restore exact bytes; never edit earlier evidence.
    let mut rejected = vec![];
    for name in [
        "planning-attempt.json",
        "approval.json",
        "run-intent.json",
        "run-return.json",
        "terminal.json",
    ] {
        let p = path.join(name);
        let bytes = fs::read(&p).unwrap();
        fs::write(&p, b"{}").unwrap();
        let result = inspect(&path, &selected);
        fs::write(&p, bytes).unwrap();
        assert!(result.is_err(), "{name}");
        rejected.push(name);
    }
    save("accepted-inspection.json", &first);
    save(
        "accepted-result.json",
        &json!({"accepted":true,"syntheticWorkerReplies":calls,"managementRequests":0,"independentChecks":1,
        "reopensWithoutRedispatch":2,"corruptRecordsRejected":rejected,"realModelCalls":0}),
    );
}

#[tokio::test]
#[ignore = "254 explicit fresh fixture; already-cancelled receiver, no client or sends"]
async fn cancellation_before_native_intent_has_known_stop_and_no_credentials() {
    let mut t = begin("S03");
    let selected = t.start_digest().unwrap();
    t.prepare().unwrap();
    authorize(&mut t);
    let (tx, mut rx) = oneshot::channel();
    tx.send(()).unwrap();
    assert!(
        t
            .execute::<Result<
                cerulean_adapters::api_generation::ApiReply,
                cerulean_adapters::api_generation::ApiFailure,
            >>(
                "synthetic-fixture",
                |_| panic!("cancel must precede credentials"),
                &mut async |_: &ApiClient,
                            _,
                            _: &mut oneshot::Receiver<()>,
                            _gateway: &mut cerulean_adapters::api_generation::GatewayObserver<
                    '_,
                >| { panic!("cancel must precede send") },
                &mut rx,
                &AtomicBool::new(false)
            )
            .await
            .is_err()
    );
    assert!(!area("S03").join("data/started.json").exists());
    let terminal = t.abort_before_execution().unwrap();
    assert!(terminal["elapsedMs"].is_u64());
    assert_eq!(terminal["accepted"], false);
    drop(t);
    save(
        "cancelled-inspection.json",
        &inspect(&area("S03").join("timeline"), &selected).unwrap(),
    );
}

#[test]
#[ignore = "254 explicit fresh fixture; clock lost after original proposal, no execution"]
fn review_clock_loss_does_not_fabricate_elapsed_or_approve() {
    let mut t = begin("S04");
    let selected = t.start_digest().unwrap();
    t.prepare().unwrap();
    let h = t.activity();
    h.set_active(true).unwrap();
    drop(t);
    let path = area("S04").join("timeline");
    assert!(close_interrupted(&path, &selected).is_err());
    drop(h);
    let end = close_interrupted(&path, &selected).unwrap();
    for k in [
        "elapsedMs",
        "planningMs",
        "approvalWaitMs",
        "executionAndClosureMs",
        "humanActiveMs",
    ] {
        assert!(end[k].is_null(), "{k}");
    }
    let report = inspect(&path, &selected).unwrap();
    assert!(report["sources"]["baseline"]["started"].is_null());
    assert_eq!(
        report["sources"]["baseline"]["coreExport"]["apiIntentCount"],
        0
    );
    save("interrupted-inspection.json", &report);
}

#[tokio::test]
#[ignore = "254 explicit fresh fixture; failed observation hook before credentials"]
async fn failed_start_observer_retains_original_intent_and_blocks_credentials() {
    let mut t = begin("S05");
    let selected = t.start_digest().unwrap();
    t.prepare().unwrap();
    let spec = authorize(&mut t);
    let root = area("S05").join("data");
    let (_tx, mut rx) = oneshot::channel();
    let terminal = super::super::execute_observed::<Result<cerulean_adapters::api_generation::ApiReply, cerulean_adapters::api_generation::ApiFailure>>(
        &root,
        &digest(&spec).unwrap(),
        approval(&spec),
        "synthetic-fixture",
        |_| panic!("observer must precede credentials"),
        &mut async |_: &ApiClient, _, _: &mut oneshot::Receiver<()>, _gateway: &mut cerulean_adapters::api_generation::GatewayObserver<'_>| {
            panic!("observer must precede send")
        },
        &mut rx,
        &AtomicBool::new(false),
        |selected| {
            assert_eq!(
                digest(&load::<super::super::Started>(&root, "started.json").unwrap()).unwrap(),
                selected
            );
            Err("synthetic observation failure".into())
        },
    )
    .await
    .unwrap();
    assert!(!terminal.accepted);
    assert_eq!(
        terminal.error.as_deref(),
        Some("synthetic observation failure")
    );
    assert!(t.abort_before_execution().is_err());
    drop(t);
    assert!(inspect(&area("S05").join("timeline"), &selected).is_err());
    assert!(close_interrupted(&area("S05").join("timeline"), &selected).is_err());
    let report = super::super::inspect(&root, &digest(&spec).unwrap()).unwrap();
    assert_eq!(report["coreExport"]["apiIntentCount"], 0);
    save("observer-failed-inspection.json", &report);
}

#[test]
#[ignore = "254 original253 records, read only; no Host or dispatch"]
fn original_team_timelines_preserve_stable_records_after_shared_clock_refactor() {
    let prior = outer().parent().unwrap().join("team-timing-253");
    let mut output = vec![];
    for name in ["accepted", "interrupted"] {
        let s: Value = load(&prior, &format!("{name}-selection.json")).unwrap();
        let now = crate::host::team_observation::timing::inspect(
            Path::new(s["dataRoot"].as_str().unwrap()),
            s["project"].as_str().unwrap(),
            s["startDigest"].as_str().unwrap(),
        )
        .unwrap();
        let old: Value = load(&prior, &format!("{name}-inspection.json")).unwrap();
        for key in ["start", "marks", "terminal"] {
            assert_eq!(now[key], old[key], "{name}/{key}");
        }
        output.push(now);
    }
    save("original253-inspections.json", &output);
}

#[test]
#[ignore = "254 new local preplanning stops; no execution or clients"]
fn preplanning_stops_and_unlinked_proposal_are_checked() {
    let root = outer().join("preplanning-fixed");
    fs::create_dir(&root).unwrap();
    let template: ComposeRequest = load(&area("S03"), "request.json").unwrap();
    let mut outcomes = vec![];
    let previous = outer().join("preplanning/known-stop");
    let start: Start = load(&previous, "start.json").unwrap();
    outcomes.push(inspect(&previous, &digest(&start).unwrap()).unwrap());
    for mode in ["bad-material", "unlinked-proposal"] {
        let mut request = template.clone();
        request.data_root = root.join(format!("{mode}-data")).to_string_lossy().into();
        if mode == "bad-material" {
            request.manifest_sha256 = "0".repeat(64);
        }
        let path = root.join(mode);
        let mut timeline = TimedBaseline::begin(&path, request, true).unwrap();
        let selected = timeline.start_digest().unwrap();
        if mode == "unlinked-proposal" {
            guard(&timeline.inner)
                .unwrap()
                .append(Kind::Proposal)
                .unwrap();
            assert!(timeline.abort_before_execution().is_err());
            drop(timeline);
            assert!(inspect(&path, &selected).is_err());
            assert!(close_interrupted(&path, &selected).is_err());
            outcomes.push(json!({"mode":mode,"unlinkedProposalRejected":true}));
        } else {
            if mode == "bad-material" {
                assert!(timeline.prepare().is_err());
                assert!(timeline.prepare().is_err());
                assert!(!path.join("planning-attempt.json.pending").exists());
            }
            let end = timeline.abort_before_execution().unwrap();
            assert!(end["elapsedMs"].is_u64());
            assert_eq!(end["accepted"], false);
            drop(timeline);
            let report = inspect(&path, &selected).unwrap();
            assert!(report["sources"]["baseline"].is_null());
            outcomes.push(report);
        }
    }
    save("preplanning-result.json", &outcomes);
}

#[test]
#[ignore = "254 read original successful and interrupted cases after reader tightening; no dispatch"]
fn original_baseline_results_read_after_missing_proposal_guard() {
    let mut reports = vec![];
    for (name, task) in [
        ("accepted", "S02"),
        ("cancelled", "S03"),
        ("interrupted", "S04"),
    ] {
        let old: Value = load(&outer(), &format!("{name}-inspection.json")).unwrap();
        let report = inspect(
            &area(task).join("timeline"),
            old["startDigest"].as_str().unwrap(),
        )
        .unwrap();
        for key in ["start", "marks", "terminal"] {
            assert_eq!(report[key], old[key]);
        }
        reports.push(report);
    }
    save("final-original-inspections.json", &reports);
}
