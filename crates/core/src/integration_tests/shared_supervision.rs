//! Simultaneous independent native incidents must never cross attempt/turn scopes.
use super::*;

#[test]
#[ignore = "actual Codex/OMP children, API thread, Git and validators under mixed per-profile supervision; controlled replies only"]
fn mixed_scoped_supervision_delivers_only_to_original_codex_and_reopens() {
    use ProjectWorkerAdapter::{Codex, Deepseek, Omp};
    let outer = PathBuf::from(
        std::env::var_os("CERULEAN_SCOPED_SUPERVISION_DIR").expect("fresh evidence directory"),
    )
    .canonicalize()
    .unwrap();
    let reports = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../reports")
        .canonicalize()
        .unwrap();
    assert!(outer.starts_with(&reports) && outer != reports);
    for other in [Deepseek, Omp] {
        let root = outer.join(other.id());
        fs::create_dir(&root).unwrap();
        let runtime = omp_parallel::runtime();
        let order = [other, Codex, Deepseek];
        let mut host = mixed::mixed_host_scoped(&root, runtime.path(), order);
        for marker in ["gate-codex", "delay-observations", "await-steer"] {
            fs::write(
                root.join("bin").join(marker),
                "scoped supervised fixture only",
            )
            .unwrap();
        }
        host.management_fixture = Some(codex_parallel::advice_manager);
        host.api_fixture = Some(parallel::gated);
        let gate = (other == Deepseek).then(|| parallel::gate("a"));
        let plan = planned(&mut host);
        assert!(plan.policy.supervision.is_none());
        assert!(plan.supervision_for("a").unwrap().is_none());
        assert!(plan.supervision_for("integration").unwrap().is_none());
        assert!(plan.supervision_for("b").unwrap().unwrap().delivery_enabled);
        let revision = plan.approval_digest().unwrap();
        host.approve("project", &revision).unwrap();
        let setup = host.project_setups["project"].digest.clone();
        host.start_project_run("project", &revision, &setup)
            .unwrap();
        if let Some((wait, _)) = &gate {
            spin(&mut host, |_| wait.try_recv().is_ok());
        }
        spin(&mut host, |h| codex_parallel::all_running(h, &root, &order));
        assert_eq!(host.api_writers.len(), 1);
        fs::write(
            root.join("bin/observe-both"),
            "two observed failures for original Codex only",
        )
        .unwrap();
        spin(&mut host, |h| {
            assert_eq!(
                h.runs.all()[0].state,
                RunState::Active,
                "{:?}",
                h.runs.all()
            );
            h.management
                .statuses()
                .iter()
                .any(|s| s.purpose == Purpose::Advise && h.advice_delivered(&s.operation_id))
        });
        let advice: Vec<_> = host
            .management
            .statuses()
            .into_iter()
            .filter(|s| s.purpose == Purpose::Advise)
            .collect();
        assert_eq!(advice.len(), 1);
        let intent = host.management.intent(&advice[0].operation_id).unwrap();
        let bound = supervision_requests::binding(intent).unwrap().unwrap();
        assert_eq!(bound.task_id, "b");
        assert!(
            supervision_requests::request_policy(intent)
                .unwrap()
                .delivery_enabled
        );
        let mut forged = intent.clone();
        forged.context["supervision"]["taskId"] = json!("a");
        assert!(supervision_requests::request_policy(&forged).is_err());
        let mut forged = intent.clone();
        forged.context["supervision"]["taskId"] = json!("missing");
        assert!(supervision_requests::request_policy(&forged).is_err());
        let delivered = host.delivery_for(&advice[0].operation_id).unwrap().unwrap();
        host.validate_delivery_record(&delivered).unwrap();
        assert_eq!(delivered.intent.thread_id, bound.thread_id);
        assert_eq!(delivered.intent.turn_id, bound.turn_id);
        // The other executor remains live and owns its original lease while the
        // Codex advice is delivered; it receives no extra attempt or advice.
        assert_eq!(
            host.snapshot().workflows[0].state.tasks["a"].state,
            if other.is_api() {
                TaskState::Dispatching
            } else {
                TaskState::Running
            }
        );
        match gate {
            Some((_, release)) => release.send(()).unwrap(),
            None => fs::write(
                omp_parallel::profile(&host)
                    .unwrap()
                    .join("fixture-release"),
                "original other completion",
            )
            .unwrap(),
        }
        spin(&mut host, |h| {
            h.snapshot().workflows[0].state.tasks["a"].state == TaskState::Accepted
        });
        assert_eq!(
            host.snapshot().workflows[0].state.tasks["b"].state,
            TaskState::Running
        );
        fs::write(root.join("bin/release-b"), "original Codex completion").unwrap();
        spin(&mut host, |h| h.runs.all()[0].state == RunState::Completed);
        assert!(host.projects["project"]
            .state()
            .tasks
            .values()
            .all(|t| t.state == TaskState::Accepted));
        let history = host.projects["project"].history().to_vec();
        assert_eq!(
            history
                .iter()
                .filter(|e| matches!(
                    &e.body.event.command,
                    WorkflowCommand::DispatchIntent { .. }
                ))
                .count(),
            3
        );
        assert_eq!(history.iter().filter(|e| matches!(&e.body.event.command, WorkflowCommand::Advise {task,..} if task == "b")).count(), 1);
        assert_eq!(host.management.statuses().len(), 8);
        assert_eq!(
            fs::read_to_string(
                dependency_host_fixture::task_path(&host, "integration").join("bundle.txt")
            )
            .unwrap(),
            "AB"
        );
        let wire = fs::read_to_string(root.join("bin/protocol.jsonl")).unwrap();
        let calls: Vec<serde_json::Value> = wire
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        assert_eq!(
            calls.iter().filter(|v| v["method"] == "turn/start").count(),
            1
        );
        assert_eq!(
            calls.iter().filter(|v| v["method"] == "turn/steer").count(),
            1
        );
        assert!(
            host.api_writers.is_empty()
                && host.codex_active.is_empty()
                && host.omp_active.is_empty()
                && host.api_active.is_empty()
        );
        drop(host);
        let mut reopened = RuntimeHost::open(&root.join("data")).unwrap();
        reopened.poll_verifications();
        assert_eq!(reopened.projects["project"].history(), history);
        assert_eq!(reopened.management.statuses().len(), 8);
        assert!(reopened.advice_delivered(&advice[0].operation_id));
        assert!(reopened
            .handle(Request {
                id: "scoped-history".into(),
                protocol_version: PROTOCOL_VERSION,
                command: Command::ReadWorkerObservation {
                    project_id: "project".into(),
                    attempt_id: bound.attempt_id.clone()
                }
            })
            .error
            .is_none());
        assert_eq!(
            fs::read_to_string(root.join("bin/protocol.jsonl")).unwrap(),
            wire
        );
        let evidence = json!({"status":"passed","other":other,"acceptedTasks":3,"managementRequests":8,
            "steerCalls":1,"nativeCodexTurns":1,"reopenNewEvents":0,"promptResends":0,"realModelCalls":0,
            "userDesktopActions":0,"originalTask":bound.task_id,"originalAttempt":bound.attempt_id,
            "snapshot":reopened.snapshot()});
        let mut out = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(root.join("result.json"))
            .unwrap();
        serde_json::to_writer_pretty(&mut out, &evidence).unwrap();
        out.sync_all().unwrap();
        println!("scoped Codex + {}: original turn advice once, other uninterrupted, 3 accepted, reopen unchanged", other.id());
    }
}

#[test]
#[ignore = "two actual native children with simultaneous incidents, original management/delivery journals and verification; controlled replies"]
fn simultaneous_codex_incidents_deliver_once_to_each_original_turn() {
    use ProjectWorkerAdapter::Codex;
    let outer = PathBuf::from(
        std::env::var_os("CERULEAN_SHARED_SUPERVISION_DIR")
            .expect("fresh supervision evidence directory"),
    )
    .canonicalize()
    .unwrap();
    let reports = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../reports")
        .canonicalize()
        .unwrap();
    assert!(outer.starts_with(&reports) && outer != reports);
    for pause in [false, true] {
        let root = outer.join(if pause { "pause" } else { "continue" });
        fs::create_dir(&root).unwrap();
        let runtime = omp_parallel::runtime();
        let mut host = mixed::mixed_host_options(
            root.as_path(),
            runtime.path(),
            [Codex, Codex, Codex],
            true,
            true,
        );
        for marker in ["gate-codex", "delay-observations", "await-steer"] {
            fs::write(
                root.as_path().join("bin").join(marker),
                "controlled peer only",
            )
            .unwrap();
        }
        host.management_fixture = Some(codex_parallel::advice_manager);
        let plan = planned(&mut host);
        let revision = plan.approval_digest().unwrap();
        host.approve("project", &revision).unwrap();
        let setup = host.project_setups["project"].digest.clone();
        host.start_project_run("project", &revision, &setup)
            .unwrap();
        spin(&mut host, |h| {
            codex_parallel::all_running(h, root.as_path(), &[Codex, Codex, Codex])
        });
        assert!(!host
            .management
            .statuses()
            .iter()
            .any(|s| s.purpose == Purpose::Advise));
        fs::write(
            root.as_path().join("bin/observe-both"),
            "release both original incident streams",
        )
        .unwrap();
        spin(&mut host, |h| {
            if h.runs.all()[0].state != RunState::Active {
                let activities: Vec<_> = h.codex_active.values().map(|a| json!({
                    "job":a.job,"snapshot":a.snapshot,"gate":a.advice_gate.load(std::sync::atomic::Ordering::SeqCst),
                    "saved":a.observation_saved,"error":a.observation_error,"cancel":a.cancel.is_some(),
                    "disconnected":a.disconnected,"trace":a.observation.as_ref().map(|c|c.snapshot())
                })).collect();
                let advice: Vec<_> = h
                    .management
                    .statuses()
                    .iter()
                    .filter(|s| s.purpose == Purpose::Advise)
                    .map(|s| json!({"status":s,"view":h.supervision_advice_view(&s.operation_id)}))
                    .collect();
                fs::write(
                    root.join("failure.json"),
                    serde_json::to_vec_pretty(
                        &json!({"activities":activities,"advice":advice,"snapshot":h.snapshot()}),
                    )
                    .unwrap(),
                )
                .unwrap();
            }
            for active in h.codex_active.values() {
                if let Some(observation) = &active.observation {
                    let trace = observation.snapshot();
                    assert_eq!(
                        trace.rejected, 0,
                        "fixture observation must match the original protocol"
                    );
                    assert_eq!(trace.dropped, 0);
                }
            }
            assert_eq!(
                h.runs.all()[0].state,
                RunState::Active,
                "{:?}",
                h.runs.all()
            );
            let pending = h
                .management
                .statuses()
                .iter()
                .filter(|s| !s.state.resolved())
                .count();
            assert!(
                pending <= 1,
                "management is serialized under the original request bound"
            );
            h.management
                .statuses()
                .iter()
                .filter(|s| s.purpose == Purpose::Advise && h.advice_delivered(&s.operation_id))
                .count()
                == 2
        });
        assert_eq!(host.codex_active.len(), 2);
        assert_eq!(host.api_writers.len(), 1);
        let statuses: Vec<_> = host
            .management
            .statuses()
            .into_iter()
            .filter(|s| s.purpose == Purpose::Advise)
            .collect();
        assert_eq!(statuses.len(), 2);
        let mut tasks = std::collections::BTreeSet::new();
        for status in &statuses {
            let intent = host.management.intent(&status.operation_id).unwrap();
            let bound = supervision_requests::binding(intent).unwrap().unwrap();
            assert_eq!(bound.version, 2);
            assert!(tasks.insert(bound.task_id.clone()));
            let active = &host.codex_active[&("project".into(), bound.attempt_id.clone())];
            assert_eq!(active.job.task_id, bound.task_id);
            let delivery = host.delivery_for(&status.operation_id).unwrap().unwrap();
            assert!(delivery.acknowledged());
            assert_eq!(delivery.intent.thread_id, bound.thread_id);
            assert_eq!(delivery.intent.turn_id, bound.turn_id);
            let request: serde_json::Value = serde_json::from_slice(
                &fs::read(
                    root.as_path()
                        .join("bin")
                        .join(format!("{}-steer-request.json", bound.thread_id)),
                )
                .unwrap(),
            )
            .unwrap();
            assert_eq!(request["threadId"], bound.thread_id);
            assert_eq!(request["expectedTurnId"], bound.turn_id);
        }
        assert_eq!(
            tasks,
            ["a".to_owned(), "b".to_owned()].into_iter().collect()
        );
        if pause {
            let run = host.runs.all()[0].id.clone();
            host.pause_project_run(&run).unwrap();
        }
        fs::remove_file(root.as_path().join("bin/await-steer")).unwrap();
        fs::remove_file(root.as_path().join("bin/delay-observations")).unwrap();
        fs::write(root.as_path().join("bin/release-a"), "original A complete").unwrap();
        if !pause {
            spin(&mut host, |h| {
                h.snapshot().workflows[0].state.tasks["a"].state == TaskState::Accepted
            });
            assert_eq!(
                host.snapshot().workflows[0].state.tasks["b"].state,
                TaskState::Running
            );
        }
        fs::write(root.as_path().join("bin/release-b"), "original B complete").unwrap();
        if pause {
            spin(&mut host, |h| h.codex_active.is_empty());
            assert_eq!(host.runs.all()[0].state, RunState::Paused);
            assert!(host.jobs.is_empty());
            assert_eq!(
                host.projects["project"].state().tasks["a"].state,
                TaskState::Submitted
            );
            assert_eq!(
                host.projects["project"].state().tasks["b"].state,
                TaskState::Submitted
            );
            host.start_project_run("project", &revision, &setup)
                .unwrap();
        }
        spin(&mut host, |h| h.runs.all()[0].state == RunState::Completed);
        assert!(host.projects["project"]
            .state()
            .tasks
            .values()
            .all(|t| t.state == TaskState::Accepted));
        assert_eq!(host.management.statuses().len(), 9);
        assert_eq!(
            fs::read_to_string(
                dependency_host_fixture::task_path(&host, "integration").join("bundle.txt")
            )
            .unwrap(),
            "AB"
        );
        let wire = fs::read_to_string(root.as_path().join("bin/protocol.jsonl")).unwrap();
        let calls: Vec<serde_json::Value> = wire
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(
            calls.iter().filter(|c| c["method"] == "turn/start").count(),
            3
        );
        assert_eq!(
            calls.iter().filter(|c| c["method"] == "turn/steer").count(),
            2
        );
        let history = host.projects["project"].history().to_vec();
        drop(host);
        let mut reopened = RuntimeHost::open(&root.as_path().join("data")).unwrap();
        reopened.poll_verifications();
        assert_eq!(reopened.projects["project"].history(), history);
        assert!(statuses
            .iter()
            .all(|s| reopened.advice_delivered(&s.operation_id)));
        assert_eq!(
            fs::read_to_string(root.as_path().join("bin/protocol.jsonl")).unwrap(),
            wire
        );
        let evidence = json!({"status":"passed","pause":pause,"nativeAttempts":3,"simultaneousIncidentAttempts":2,"steerCalls":2,"acceptedTasks":3,"managementRequests":9,"reopenNewEvents":0,"promptResends":0,"realModelCalls":0,"userDesktopActions":0,"snapshot":reopened.snapshot()});
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(root.join("result.json"))
            .unwrap();
        serde_json::to_writer_pretty(&mut file, &evidence).unwrap();
        file.sync_all().unwrap();
        println!("Simultaneous Codex incidents pause={pause}: two original turns, two steer calls, three accepted tasks, reopen events 0");
    }
}
