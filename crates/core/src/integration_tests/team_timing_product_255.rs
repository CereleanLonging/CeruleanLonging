use super::*;
use crate::ipc::{Client, UserOperation};
use cerulean_domain::team_timing::TeamTimingAction as Action;
use sha2::{Digest, Sha256};
use std::sync::OnceLock;
static BLUEPRINT: OnceLock<crate::team_materials::TeamBlueprint> = OnceLock::new();

#[tokio::test]
#[ignore = "516 fresh synthetic failing worker; real IPC/verifier/shutdown; no models"]
async fn closed_failed_team_shutdown_preserves_evidence_and_refuses_changed_closure() {
    let (h, path, sha) = host("S04");
    let root = h.root.clone();
    let owned = root.clone();
    let service = tokio::spawn(async move { crate::ipc::serve_host(&owned, h).await });
    let started = Instant::now();
    while Client::connect(&root).await.is_err() {
        assert!(started.elapsed() < Duration::from_secs(10));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    ok(
        &root,
        command(Action::Import {
            path: path.to_string_lossy().into(),
            sha256: sha,
        }),
    )
    .await;
    let imported = wait(&root, |s| {
        s.team_timelines
            .first()
            .is_some_and(|v| v.phase == "prepared")
    })
    .await;
    let project = imported.workflows[0].project.id.clone();
    let setup = imported.project_setups[0].digest.clone();
    ok(
        &root,
        command(Action::Begin {
            project_id: project.clone(),
            setup_digest: setup.clone(),
        }),
    )
    .await;
    let review = wait(&root, |s| {
        s.team_timelines
            .first()
            .is_some_and(|v| v.phase == "review")
    })
    .await;
    let selected = review.team_timelines[0].start_digest.clone().unwrap();
    let revision = review.workflows[0]
        .revision
        .as_ref()
        .unwrap()
        .approval_digest()
        .unwrap();
    ok(
        &root,
        Command::ApproveRevision {
            project_id: project.clone(),
            digest: revision.clone(),
        },
    )
    .await;
    ok(
        &root,
        Command::StartProjectRun {
            project_id: project.clone(),
            revision_digest: revision,
            setup_digest: setup,
        },
    )
    .await;
    let failed = wait(&root, |s| {
        !s.project_runs.is_empty() && s.project_runs.iter().all(|r| r.state != RunState::Active)
    })
    .await;
    assert_eq!(failed.project_runs[0].state, RunState::Attention);
    assert!(failed
        .verification_jobs
        .iter()
        .any(|j| j.state == VerificationState::Failed));
    async fn stop(root: &Path, id: &str) -> crate::ipc::ServiceReply {
        Client::connect(root)
            .await
            .unwrap()
            .call(UserOperation::Shutdown {
                id: id.into(),
                protocol_version: PROTOCOL_VERSION,
            })
            .await
            .unwrap()
    }
    assert!(!stop(&root, "reject-unclosed").await.stopping);
    ok(
        &root,
        command(Action::Finish {
            project_id: project.clone(),
            start_digest: selected.clone(),
        }),
    )
    .await;
    let closed = Client::snapshot(&root).await.unwrap();
    assert_eq!(closed.team_timelines[0].accepted, Some(false));
    let timeline = root
        .join("team-timelines")
        .join(&project)
        .join("terminal.json");
    let execution = root
        .join("team-observations")
        .join(&project)
        .join("terminal.json");
    // Only synthetic fixtures are mutated here; each original is restored.
    let bytes = fs::read(&timeline).unwrap();
    fs::rename(&timeline, timeline.with_extension("held")).unwrap();
    assert!(!stop(&root, "reject-missing-terminal").await.stopping);
    fs::rename(timeline.with_extension("held"), &timeline).unwrap();
    let mut altered: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    altered["accepted"] = json!(true);
    fs::write(&timeline, serde_json::to_vec(&altered).unwrap()).unwrap();
    assert!(!stop(&root, "reject-changed-timing").await.stopping);
    fs::write(&timeline, &bytes).unwrap();
    let original = fs::read(&execution).unwrap();
    let mut altered: serde_json::Value = serde_json::from_slice(&original).unwrap();
    altered["ledgerHead"] = json!("0".repeat(64));
    fs::write(&execution, serde_json::to_vec(&altered).unwrap()).unwrap();
    assert!(!stop(&root, "reject-changed-execution").await.stopping);
    fs::write(&execution, &original).unwrap();
    ok(&root, Command::RemoveOpenAiKey).await;
    ok(
        &root,
        Command::RemoveProviderKey {
            provider: CredentialProvider::DeepSeek,
        },
    )
    .await;
    assert!(stop(&root, "stop-preserved-failure").await.stopping);
    tokio::time::timeout(Duration::from_secs(20), service)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let report =
        crate::host::team_observation::timing::inspect(&root, &project, &selected).unwrap();
    assert_eq!(report["sourceValidated"], true);
    assert_eq!(report["terminal"]["accepted"], false);
    assert_eq!(fs::read(&timeline).unwrap(), bytes);
    assert_eq!(fs::read(&execution).unwrap(), original);
    let mut reopened = RuntimeHost::open(&root).unwrap();
    assert!(reopened.can_shutdown());
    assert_eq!(
        reopened.snapshot().project_runs[0].state,
        RunState::Attention
    );
    assert_eq!(
        reopened.projects[&project].state().tasks,
        closed.workflows[0].state.tasks
    );
    drop(reopened);
    let recovered =
        crate::host::team_observation::timing::inspect(&root, &project, &selected).unwrap();
    save("failed-shutdown-before-reopen.json", &report);
    save("failed-shutdown-after-reopen.json", &recovered);
    // VerificationDetail.observed_at describes this read, not the original
    // verification. Keep both read times; compare every original evidence field.
    fn stable_evidence(mut value: serde_json::Value) -> serde_json::Value {
        let checks = value["execution"]["coreExport"]["verifications"]
            .as_array_mut()
            .unwrap();
        assert!(!checks.is_empty());
        for check in checks {
            let observed = check["detail"]
                .as_object_mut()
                .unwrap()
                .remove("observedAt")
                .unwrap();
            chrono::DateTime::parse_from_rfc3339(observed.as_str().unwrap()).unwrap();
        }
        value
    }
    assert_eq!(stable_evidence(recovered), stable_evidence(report));
    save(
        "failed-shutdown-result.json",
        &json!({"passed":true,"refusalCases":4,
        "realModelCalls":0,"nativeColdSourceValidated":true,"reopenWithoutRedispatch":true,
        "failedSubmissionPreserved":true,"retirementUsed":false}),
    );
}

fn next_id() -> String {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    format!(
        "timing255-{}",
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}
fn outer() -> PathBuf {
    PathBuf::from(std::env::var_os("CERULEAN_TEAM_255_DIR").unwrap())
}
fn save(name: &str, value: &impl serde::Serialize) {
    crate::api_execution::publish(&outer(), name, value).unwrap();
}
fn manager_fixture(
    intent: crate::management_execution::DecisionIntent,
) -> Result<DecisionObservation, String> {
    let mut result = manager(intent)?;
    if let ManagementDecision::Plan { tasks, .. } = &mut result.envelope.decision {
        *tasks = BLUEPRINT.get().unwrap().tasks.clone();
    }
    Ok(result)
}
fn host(task: &str) -> (RuntimeHost, PathBuf, String) {
    let path = outer().join(format!("fixtures/{task}-r1-team/request.json"));
    let bytes = fs::read(&path).unwrap();
    let request = serde_json::from_slice(&bytes).unwrap();
    let b = crate::team_materials::compose(&request).unwrap();
    let mut h = RuntimeHost::open(Path::new(&b.request.material.data_root)).unwrap();
    assert!(BLUEPRINT.set(b).is_ok());
    h.management_fixture = Some(manager_fixture);
    h.api_fixture = Some(crate::workload_fixture_252::worker);
    team_materials_250::keys(&mut h, true);
    (h, path, format!("{:x}", Sha256::digest(bytes)))
}
fn command(action: Action) -> Command {
    Command::TeamTiming { action }
}
async fn call(root: &Path, c: Command) -> crate::ipc::ServiceReply {
    Client::connect(root)
        .await
        .unwrap()
        .call(UserOperation::Command {
            request: Request {
                id: next_id(),
                protocol_version: PROTOCOL_VERSION,
                command: c,
            },
        })
        .await
        .unwrap()
}
async fn ok(root: &Path, c: Command) {
    let r = call(root, c).await;
    assert!(r.response.error.is_none(), "{:?}", r.response.error);
    assert!(r.command_receipt.is_some());
}
async fn wait(root: &Path, p: impl Fn(&Snapshot) -> bool) -> Snapshot {
    let started = Instant::now();
    loop {
        let s = Client::snapshot(root).await.unwrap();
        if p(&s) {
            return s;
        }
        assert!(
            started.elapsed() < Duration::from_secs(120),
            "wait deadline"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
#[tokio::test]
#[ignore = "255 fresh synthetic fixture, real authenticated IPC/Host/files/independent checks; no network models"]
async fn team_timing_ipc_preserves_original_approval_execution_and_reopen() {
    let (h, path, sha) = host("S04");
    let root = h.root.clone();
    let owned = root.clone();
    let service = tokio::spawn(async move { crate::ipc::serve_host(&owned, h).await });
    let start = Instant::now();
    while Client::connect(&root).await.is_err() {
        assert!(start.elapsed() < Duration::from_secs(10));
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(call(
        &root,
        command(Action::Import {
            path: path.to_string_lossy().into(),
            sha256: "0".repeat(64)
        })
    )
    .await
    .response
    .error
    .is_some());
    assert!(!root.join("team-material-import.json").exists());
    ok(
        &root,
        command(Action::Import {
            path: path.to_string_lossy().into(),
            sha256: sha.clone(),
        }),
    )
    .await;
    let prepared = Client::snapshot(&root).await.unwrap();
    assert_eq!(prepared.team_timelines[0].phase, "prepared");
    let b = BLUEPRINT.get().unwrap();
    let project = b.plan.project.id.clone();
    let setup = b.setup.digest.clone();
    let revision = b.plan.approval_digest().unwrap();
    assert!(call(
        &root,
        Command::RequestChiefPlan {
            project_id: project.clone(),
            setup_digest: setup.clone()
        }
    )
    .await
    .response
    .error
    .is_some());
    assert!(call(
        &root,
        Command::ConfigureProject {
            draft: Box::new(b.configuration_draft())
        }
    )
    .await
    .response
    .error
    .is_some());
    assert!(call(
        &root,
        command(Action::Import {
            path: path.to_string_lossy().into(),
            sha256: sha
        })
    )
    .await
    .response
    .error
    .is_some());
    ok(
        &root,
        command(Action::Begin {
            project_id: project.clone(),
            setup_digest: setup.clone(),
        }),
    )
    .await;
    let review = wait(&root, |s| {
        s.team_timelines
            .first()
            .is_some_and(|v| v.phase == "review")
    })
    .await;
    let selected = review.team_timelines[0].start_digest.clone().unwrap();
    let human = |active| {
        command(Action::Human {
            project_id: project.clone(),
            start_digest: selected.clone(),
            active,
        })
    };
    assert!(call(
        &root,
        command(Action::Begin {
            project_id: project.clone(),
            setup_digest: setup.clone()
        })
    )
    .await
    .response
    .error
    .is_some());
    assert!(call(
        &root,
        command(Action::Human {
            project_id: project.clone(),
            start_digest: "0".repeat(64),
            active: true
        })
    )
    .await
    .response
    .error
    .is_some());
    ok(&root, human(true)).await;
    assert!(call(
        &root,
        Command::ApproveRevision {
            project_id: project.clone(),
            digest: revision.clone()
        }
    )
    .await
    .response
    .error
    .is_some());
    ok(&root, human(false)).await;
    ok(
        &root,
        Command::ApproveRevision {
            project_id: project.clone(),
            digest: revision.clone(),
        },
    )
    .await;
    ok(
        &root,
        Command::StartProjectRun {
            project_id: project.clone(),
            revision_digest: revision.clone(),
            setup_digest: setup.clone(),
        },
    )
    .await;
    // Human toggles use the actual original execution journal while workers run.
    let live = Client::snapshot(&root).await.unwrap();
    assert_eq!(live.team_timelines.len(), 1);
    assert_eq!(live.team_timelines[0].phase, "executing");
    ok(&root, human(true)).await;
    ok(&root, human(false)).await;
    let done = wait(&root, |s| {
        !s.project_runs.is_empty() && s.project_runs.iter().all(|r| r.state != RunState::Active)
    })
    .await;
    save("completed-before-timing-close.json", &done);
    ok(
        &root,
        command(Action::Finish {
            project_id: project.clone(),
            start_digest: selected.clone(),
        }),
    )
    .await;
    let closed = Client::snapshot(&root).await.unwrap();
    assert_eq!(closed.team_timelines[0].accepted, Some(true));
    assert_eq!(closed.team_timelines[0].phase, "finished");
    assert!(call(
        &root,
        Command::StartProjectRun {
            project_id: project.clone(),
            revision_digest: revision,
            setup_digest: setup
        }
    )
    .await
    .response
    .error
    .is_some());
    ok(&root, Command::RemoveOpenAiKey).await;
    ok(
        &root,
        Command::RemoveProviderKey {
            provider: CredentialProvider::DeepSeek,
        },
    )
    .await;
    let stop = Client::connect(&root)
        .await
        .unwrap()
        .call(UserOperation::Shutdown {
            id: "stop".into(),
            protocol_version: PROTOCOL_VERSION,
        })
        .await
        .unwrap();
    assert!(stop.stopping);
    tokio::time::timeout(Duration::from_secs(20), service)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let original =
        crate::host::team_observation::timing::inspect(&root, &project, &selected).unwrap();
    assert_eq!(original["execution"]["coreExport"]["apiReplies"], 2);
    for _ in 0..2 {
        let h = RuntimeHost::open(&root).unwrap();
        let view = h.snapshot().team_timelines;
        assert_eq!(
            serde_json::to_value(view).unwrap(),
            serde_json::to_value(&closed.team_timelines).unwrap()
        );
    }
    save("ipc-original-inspection.json", &original);
    save("ipc-final-snapshot.json", &closed);
    save(
        "ipc-result.json",
        &json!({"syntheticWorkerReplies":2,"reopensWithoutRedispatch":2,"authenticatedIpc":true,"realModelCalls":0,"releaseAccepted":false}),
    );
}

#[test]
#[ignore = "255 read failed S06 intent only; never open Host or resume original task"]
fn failed_original_intent_is_preserved_without_reexecution() {
    let root = outer().join("fixtures/S06-r1-team/data");
    let project = "eff-S06-r1-team";
    let path = root.join("team-timelines").join(project);
    let first: serde_json::Value = crate::api_execution::load(&path, "mark-0001.json").unwrap();
    let selected = first["previous"].as_str().unwrap();
    assert!(!path.join("terminal.json").exists());
    let result = crate::host::team_observation::timing::inspect(&root, project, selected);
    save(
        "failed-original-inspection.json",
        &json!({"inspection":result,"newRequests":0,"originalRetried":false,"releaseAccepted":false}),
    );
}

#[test]
fn wire_action_rejects_unknown_fields_and_hides_role_callbacks() {
    assert!(serde_json::from_value::<Action>(json!({"kind":"human","projectId":"p","startDigest":"a".repeat(64),"active":true,"principal":"core"})).is_err());
    assert!(serde_json::from_value::<Action>(json!({"kind":"approve","projectId":"p"})).is_err());
    assert!(command(Action::Finish {
        project_id: "p".into(),
        start_digest: "a".repeat(64)
    })
    .supports_receipt());
}

#[test]
#[ignore = "255 fresh planning-only fixture; reopen loses live clock and refuses original execution"]
fn reopened_timed_review_is_unknown_and_does_not_resume() {
    let (mut h, path, sha) = host("S05");
    let handle = |h: &mut RuntimeHost, c| {
        h.handle(Request {
            id: next_id(),
            protocol_version: PROTOCOL_VERSION,
            command: c,
        })
    };
    assert!(handle(
        &mut h,
        command(Action::Import {
            path: path.to_string_lossy().into(),
            sha256: sha
        })
    )
    .error
    .is_none());
    let b = BLUEPRINT.get().unwrap();
    let project = b.plan.project.id.clone();
    let root = h.root.clone();
    assert!(handle(
        &mut h,
        command(Action::Begin {
            project_id: project.clone(),
            setup_digest: b.setup.digest.clone()
        })
    )
    .error
    .is_none());
    spin(&mut h, |h| h.team_timing_views()[0].phase == "review");
    let selected = h.team_timing_views()[0].start_digest.clone().unwrap();
    assert!(handle(
        &mut h,
        command(Action::Human {
            project_id: project.clone(),
            start_digest: selected.clone(),
            active: true
        })
    )
    .error
    .is_none());
    team_materials_250::keys(&mut h, false);
    let count = h.management.statuses().len();
    drop(h);
    let mut h = RuntimeHost::open(&root).unwrap();
    let view = h.snapshot().team_timelines.remove(0);
    assert_eq!(view.phase, "clockLost");
    assert!(view.elapsed_ms.is_none());
    assert!(view.human_active_ms.is_none());
    assert!(handle(
        &mut h,
        Command::ApproveRevision {
            project_id: project.clone(),
            digest: b.plan.approval_digest().unwrap()
        }
    )
    .error
    .is_some());
    assert!(handle(
        &mut h,
        Command::StartProjectRun {
            project_id: project.clone(),
            revision_digest: b.plan.approval_digest().unwrap(),
            setup_digest: b.setup.digest.clone()
        }
    )
    .error
    .is_some());
    assert!(handle(
        &mut h,
        command(Action::CloseInterrupted {
            project_id: project.clone(),
            start_digest: selected.clone()
        })
    )
    .error
    .is_none());
    let closed = h.snapshot().team_timelines.remove(0);
    assert_eq!(closed.phase, "interrupted");
    assert!(closed.elapsed_ms.is_none());
    assert_eq!(h.management.statuses().len(), count);
    assert!(h.runs.all().is_empty());
    drop(h);
    save(
        "recovery-inspection.json",
        &crate::host::team_observation::timing::inspect(&root, &project, &selected).unwrap(),
    );
    save(
        "recovery-result.json",
        &json!({"lostClockUnknown":true,"newRequestsOnReopen":0,"workerRequests":0,"originalPlanningOperations":count,"realModelCalls":0}),
    );
}
