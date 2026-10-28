//! Existing OMP fixture and user command; no provider, desktop, or subprocess.
use super::*;
use cerulean_adapters::omp_transport::worker::advice::AdviceEvent;

/// Retain original synthetic records for an actual frozen-runtime reader.
/// This never starts an OMP child, registers a real key, or sends a model call.
#[test]
#[ignore = "explicit new repository report directory for frozen-reader inspection"]
fn export_omp_advice_for_frozen_reader() {
    let root = PathBuf::from(std::env::var_os("OMP_COMPAT_FIXTURE_ROOT").unwrap());
    let reports = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../reports")
        .canonicalize()
        .unwrap();
    assert!(root
        .parent()
        .unwrap()
        .canonicalize()
        .unwrap()
        .starts_with(reports));
    fs::create_dir(&root).unwrap();
    for mode in ["record-only", "confirmed", "lost-message", "closed-unknown"] {
        let case = root.join(mode);
        fs::create_dir(&case).unwrap();
        let (mut host, operation) = if mode == "record-only" {
            let mut f = fixture(&case, 4);
            f.failure("a");
            f.failure("b");
            f.host.poll_project_runs();
            let operation = f.wait();
            let run = f.host.runs.all()[0].id.clone();
            f.host.pause_project_run(&run).unwrap();
            drop(f);
            (RuntimeHost::open(&case.join("data")).unwrap(), operation)
        } else {
            let (host, operation, _) = stopped_advice(
                &case,
                if mode == "closed-unknown" {
                    "lost-message"
                } else {
                    mode
                },
            );
            (host, operation)
        };
        if mode == "closed-unknown" {
            let review = host.advice_tracking_review(&operation).unwrap();
            host.close_advice_tracking(
                &operation,
                &review.evidence_digest,
                "Synthetic frozen-reader case: preserve unknown outcome without replay",
            )
            .unwrap();
        }
        assert!(!host.openai_connection.key_configured);
        assert!(host.omp_active.is_empty() && host.management_active.is_empty());
        assert_eq!(host.runs.all()[0].state, RunState::Paused);
        let view = host.supervision_advice_view(&operation).unwrap().unwrap();
        let count = host.management.statuses().len();
        drop(host);
        for _ in 0..2 {
            let mut reopened = RuntimeHost::open(&case.join("data")).unwrap();
            reopened.poll_project_runs();
            let current = reopened
                .supervision_advice_view(&operation)
                .unwrap()
                .unwrap();
            assert_eq!(current.delivery_state, view.delivery_state);
            assert_eq!(current.delivered, view.delivered);
            assert_eq!(reopened.management.statuses().len(), count);
            assert!(!reopened.openai_connection.key_configured);
            assert!(reopened.omp_active.is_empty() && reopened.management_active.is_empty());
        }
        let bytes = serde_json::to_vec_pretty(&json!({
            "schema":1,"mode":mode,"operation":operation,"deliveryState":view.delivery_state,
            "delivered":view.delivered,"managementRequests":count,"runState":"paused",
            "currentHostReopens":2,"syntheticPeerEvidence":true,"realModelCalls":0,
            "keyConfigured":false,"closure":mode=="closed-unknown"
        }))
        .unwrap();
        fs::write(case.join("case-result.json"), bytes).unwrap();
    }
}

fn stopped_advice(root: &Path, mode: &str) -> (RuntimeHost, String, PathBuf) {
    let mut fixture = fixture_delivery(root, 4, true);
    fixture.failure("a");
    fixture.failure("b");
    let (reply, mut received) = tokio::sync::oneshot::channel();
    fixture
        .host
        .handle_omp_advice_boundary("project", "attempt", Some(reply))
        .unwrap();
    fixture.host.poll_project_runs();
    let operation = fixture.wait();
    fixture.host.poll_project_runs();
    let mut advice = received.try_recv().unwrap().unwrap().unwrap();
    let intent = fixture.host.management.intent(&operation).unwrap().clone();
    let original = evidence(&intent, &binding(&intent).unwrap().unwrap()).unwrap();
    if mode != "no-send" && mode != "not-sent" {
        (advice.observe)(&AdviceEvent::SendIntent {
            command_id: "cerulean-omp-steer-1".into(),
            message_sha256: crate::omp_advice_journal::text_hash(&advice.message),
        })
        .unwrap();
        (advice.observe)(&AdviceEvent::Accepted {
            command_id: "cerulean-omp-steer-1".into(),
        })
        .unwrap();
        (advice.observe)(&AdviceEvent::Queued {
            state_id: "cerulean-omp-state-1".into(),
        })
        .unwrap();
        (advice.observe)(&AdviceEvent::ToolResultReleased {
            result_sha256: original.held_result_sha256().into(),
        })
        .unwrap();
        if mode == "confirmed" {
            (advice.observe)(&AdviceEvent::MessageObserved {
                message_id: "original-advice-message".into(),
            })
            .unwrap();
        }
    }
    let folder = fixture
        .host
        .omp_read("project", "task", "attempt")
        .unwrap()
        .omp_attempt_path("attempt")
        .unwrap()
        .join(format!("advice-{operation}"));
    if mode == "pending" {
        fs::write(folder.join("05.json.pending"), b"partial").unwrap();
    } else if mode == "same-name-pending" {
        fs::copy(folder.join("04.json"), folder.join("04.json.pending")).unwrap();
    }
    // Even a valid original proof cannot be closed while its worker owns it.
    assert!(fixture.host.advice_tracking_review(&operation).is_err());
    let run = fixture.host.runs.all()[0].id.clone();
    fixture.host.pause_project_run(&run).unwrap();
    if mode == "not-sent" {
        (advice.observe)(&AdviceEvent::RevokedBeforeSend).unwrap();
        (advice.observe)(&AdviceEvent::ToolResultReleased {
            result_sha256: original.held_result_sha256().into(),
        })
        .unwrap();
    }
    assert!(fixture.host.advice_tracking_review(&operation).is_err());
    drop(advice);
    drop(fixture);
    let host = RuntimeHost::open(&root.join("data")).unwrap();
    (host, operation, folder)
}

#[test]
fn omp_user_disposition_preserves_unknown_and_never_resumes_or_replays() {
    for mode in ["no-send", "lost-message", "pending", "same-name-pending"] {
        let temp = tempfile::tempdir().unwrap();
        let (mut host, operation, _) = stopped_advice(temp.path(), mode);
        assert!(
            host.require_resolved_omp_advice("project").is_err(),
            "{mode}"
        );
        let workflow = digest(&host.projects["project"].history()).unwrap();
        let jobs = digest(&host.omp_jobs.values().collect::<Vec<_>>()).unwrap();
        let management = host
            .management
            .evidence_digest(&operation)
            .unwrap()
            .to_owned();
        let count = host.management.statuses().len();
        let delivery = host
            .supervision_advice_view(&operation)
            .unwrap()
            .unwrap()
            .delivery_state;
        let review = host.advice_tracking_review(&operation).unwrap();
        assert!(host
            .close_advice_tracking(&operation, &"0".repeat(64), "stale")
            .is_err());
        assert!(host
            .close_advice_tracking(&operation, &review.evidence_digest, " ")
            .is_err());
        let response = host.handle(Request {
            id: "omp-close-advice".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::CloseAdviceTracking {
                operation_id: operation.clone(),
                evidence_digest: review.evidence_digest,
                reason: "保留未确认的原建议，结束本地跟踪；任务结果独立核对".into(),
            },
        });
        assert!(response.error.is_none(), "{mode}: {:?}", response.error);
        assert!(host.validate_omp_advice_closure(&operation).unwrap());
        assert!(host.require_resolved_omp_advice("project").is_ok());
        assert!(host.advice_tracking_review(&operation).is_err());
        assert_eq!(
            digest(&host.projects["project"].history()).unwrap(),
            workflow
        );
        assert_eq!(
            digest(&host.omp_jobs.values().collect::<Vec<_>>()).unwrap(),
            jobs
        );
        assert_eq!(
            host.management.evidence_digest(&operation).unwrap(),
            management
        );
        assert_eq!(host.runs.all()[0].state, RunState::Paused);
        drop(host);
        for _ in 0..2 {
            host = RuntimeHost::open(&temp.path().join("data")).unwrap();
            for _ in 0..3 {
                host.poll_project_runs();
            }
            assert!(host.validate_omp_advice_closure(&operation).unwrap());
            assert!(host
                .management_detail(&operation)
                .unwrap()
                .advice_tracking_closure
                .is_some());
            let view = host.supervision_advice_view(&operation).unwrap().unwrap();
            assert!(!view.delivered);
            assert_eq!(view.delivery_state, delivery);
            assert_eq!(host.management.statuses().len(), count);
            assert_eq!(
                digest(&host.projects["project"].history()).unwrap(),
                workflow
            );
            assert!(host.omp_active.is_empty() && host.management_active.is_empty());
            assert_eq!(host.runs.all()[0].state, RunState::Paused);
            drop(host);
        }
    }
}

#[test]
fn omp_disposition_binds_partial_bytes_absence_and_rejects_stale_reviews() {
    let temp = tempfile::tempdir().unwrap();
    let (mut host, operation, folder) = stopped_advice(temp.path(), "lost-message");
    let before = host.advice_tracking_review(&operation).unwrap();
    let pending = folder.join("05.json.pending");
    fs::write(&pending, b"partial-one").unwrap();
    let current = host.advice_tracking_review(&operation).unwrap();
    assert_ne!(before.evidence_digest, current.evidence_digest);
    assert!(host
        .close_advice_tracking(&operation, &before.evidence_digest, "old files")
        .is_err());
    let mut run = host.runs.all()[0].clone();
    run.state = RunState::Active;
    host.runs.save(run.clone(), "user").unwrap();
    assert!(host.advice_tracking_review(&operation).is_err());
    host.pause_project_run(&run.id).unwrap();
    assert!(host
        .close_advice_tracking(&operation, &current.evidence_digest, "old run")
        .is_err());
    let reviewed = host.advice_tracking_review(&operation).unwrap();
    host.close_advice_tracking(
        &operation,
        &reviewed.evidence_digest,
        "reviewed uncertainty",
    )
    .unwrap();
    drop(host);
    fs::write(&pending, b"partial-two").unwrap();
    assert!(RuntimeHost::open(&temp.path().join("data")).is_err());
    fs::write(&pending, b"partial-one").unwrap();
    let host = RuntimeHost::open(&temp.path().join("data")).unwrap();
    assert!(host.validate_omp_advice_closure(&operation).unwrap());
}

#[test]
fn omp_confirmed_and_not_sent_advice_do_not_offer_unknown_disposition() {
    for mode in ["confirmed", "not-sent"] {
        let temp = tempfile::tempdir().unwrap();
        let (mut host, operation, _) = stopped_advice(temp.path(), mode);
        assert!(host.require_resolved_omp_advice("project").is_ok());
        assert!(host.advice_tracking_review(&operation).is_err());
        assert!(host
            .close_advice_tracking(&operation, &"0".repeat(64), "unneeded")
            .is_err());
        assert!(host.advice_closures.get(&operation).is_none());
    }
}

#[test]
fn omp_disposition_rejects_rehashed_cross_ledger_proof() {
    let temp = tempfile::tempdir().unwrap();
    let (mut host, operation, _) = stopped_advice(temp.path(), "lost-message");
    let review = host.advice_tracking_review(&operation).unwrap();
    host.close_advice_tracking(&operation, &review.evidence_digest, "original proof")
        .unwrap();
    drop(host);
    let path = fs::read_dir(temp.path().join("data/advice-closures"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let bytes = fs::read(&path).unwrap();
    for field in [
        "deliveryDigest",
        "managementDigest",
        "adviceEventHash",
        "attempt",
    ] {
        let mut saved: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        saved["body"]["proof"][field] = json!(if field == "attempt" {
            "foreign-attempt".to_owned()
        } else {
            "0".repeat(64)
        });
        let closure: crate::advice_closure::Closure =
            serde_json::from_value(saved["body"].clone()).unwrap();
        saved["hash"] = json!(digest(&closure).unwrap());
        fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();
        assert!(
            RuntimeHost::open(&temp.path().join("data")).is_err(),
            "{field}"
        );
    }
    fs::write(path, bytes).unwrap();
    assert!(RuntimeHost::open(&temp.path().join("data")).is_ok());
}
