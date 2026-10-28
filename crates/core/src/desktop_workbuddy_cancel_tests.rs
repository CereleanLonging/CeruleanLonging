use super::*;
use cerulean_domain::desktop_sessions::{DesktopApp, DesktopWindow};

fn fixture() -> (TaskReview, StopRequest, StopIntent, StopProof) {
    let view = DesktopObservation {
        window: DesktopWindow {
            app: DesktopApp::Workbuddy,
            hwnd: "123".into(),
            pid: 42,
            start_ticks: "123456".into(),
            exe: "D:\\apps\\WorkBuddy.exe".into(),
            title: "WorkBuddy".into(),
            version: "5.7.7.0".into(),
        },
        model_label: Some("Hy3".into()),
        captured_at: "2026-10-10T12:00:00Z".into(),
        document_runtime_id: "42.1".into(),
        session_id: None,
        content_digest: "a".repeat(64),
        layout_digest: "b".repeat(64),
        node_count: 100,
        draft_empty: true,
        busy: false,
    };
    let mut r = TaskReview {
        id: String::new(),
        binding_id: "2".repeat(64),
        marker: format!("CL_TASK_{}", "3".repeat(32)),
        prompt: "synthetic approved prompt".into(),
        prompt_sha256: text_hash("synthetic approved prompt"),
        bridge_sha256: "5".repeat(64),
        observation: view.clone(),
        baseline: vec!["6".repeat(64), "7".repeat(64), "8".repeat(64)],
        attempt: Attempt {
            workspace_proof: None,
            id: "attempt".into(),
            task_id: "task".into(),
            worker_id: "worker".into(),
            revision_digest: "9".repeat(64),
            fence: 1,
            binding: SessionBinding {
                adapter_id: "workbuddy".into(),
                adapter_version: "fixture".into(),
                session_id: format!("ui-binding-{}", "2".repeat(64)),
                identity_strength: IdentityStrength::UiObservation,
                workspace: "D:\\test".into(),
            },
        },
        project_id: "project".into(),
        workflow_sequence: 1,
        workflow_hash: "a".repeat(64),
        max_wait_seconds: 10,
        continuation: None,
    };
    r.id = review_hash(&r).unwrap();
    let q = StopRequest {
        review_digest: r.id.clone(),
        actor: "user".into(),
        reason: "停止原任务".into(),
        requested_at: "2026-10-10T12:00:01Z".into(),
    };
    let mut running = view.clone();
    running.busy = true;
    running.captured_at = "2026-10-10T12:00:02Z".into();
    let i = StopIntent {
        request_digest: digest(&q).unwrap(),
        observation: StopObservation {
            task_review_digest: r.id.clone(),
            observation: running,
            baseline: r.baseline.clone(),
            turn_runtime_id: "42.1.10".into(),
            prompt_runtime_id: "42.1.9".into(),
            stop_runtime_id: "42.1.11".into(),
        },
        reserved_at: "2026-10-10T12:00:03Z".into(),
    };
    let mut final_view = view;
    final_view.captured_at = "2026-10-10T12:00:04.300Z".into();
    let p = StopProof {
        task_review_digest: r.id.clone(),
        observation: final_view,
        first_observed_at: "2026-10-10T12:00:04Z".into(),
        turn_runtime_id: i.observation.turn_runtime_id.clone(),
        prompt_runtime_id: i.observation.prompt_runtime_id.clone(),
        observed_baseline: r.baseline[1..].to_vec(),
        status_text: "已取消".into(),
        cancelled: true,
    };
    (r, q, i, p)
}

// Production records() validates the observation before validating its proof.
fn validate_chain(
    r: &TaskReview,
    q: &StopRequest,
    i: &StopIntent,
    p: &StopProof,
) -> Result<(), String> {
    validate_request(r, q)?;
    validate_observation(r, q, &i.observation)?;
    validate_proof(r, i, p)
}

#[test]
fn workbuddy_583_stop_read_chain_stays_bound_to_original_version() {
    let (mut r, mut q, mut i, mut p) = fixture();
    r.observation.window.version = "5.8.3.0".into();
    r.id = review_hash(&r).unwrap();
    q.review_digest = r.id.clone();
    i.request_digest = digest(&q).unwrap();
    i.observation.task_review_digest = r.id.clone();
    i.observation.observation.window.version = "5.8.3.0".into();
    p.task_review_digest = r.id.clone();
    p.observation.window.version = "5.8.3.0".into();
    validate_chain(&r, &q, &i, &p).unwrap();
    validate_task_window(&r.observation.window).unwrap();
    p.observation.window.version = "5.7.7.0".into();
    assert!(validate_chain(&r, &q, &i, &p).is_err());
}

#[test]
fn workbuddy_cancellation_accepts_only_exact_terminal_status_labels() {
    let (r, q, i, mut p) = fixture();
    for status in ["已取消", "已停止", "已中断", "任务已停止", "任务已取消"] {
        p.status_text = status.into();
        validate_chain(&r, &q, &i, &p).unwrap();
    }
    for status in [
        "",
        "取消",
        "停止",
        "中断",
        "已完成 1s",
        "生成中",
        "已取消 1s",
        "已取消，耗时：1s",
        "已停止任务",
        "任务已取消完成",
        "状态：已取消",
        "\"已取消\"",
        "引用：已取消",
        "> 已取消",
        " 已取消",
        "已取消 ",
        "已取消\n",
        "已取消\0",
        "已取消\u{feff}",
    ] {
        p.status_text = status.into();
        assert!(validate_chain(&r, &q, &i, &p).is_err(), "{status:?}");
    }
}

#[test]
fn workbuddy_cancellation_rejects_changed_final_identity_turn_history_or_time() {
    let (r, q, i, p) = fixture();
    for field in 0..26 {
        let mut bad = p.clone();
        match field {
            0 => bad.task_review_digest = "f".repeat(64),
            1 => bad.observation.window.hwnd.push('1'),
            2 => bad.observation.window.pid += 1,
            3 => bad.observation.window.start_ticks.push('1'),
            4 => bad.observation.window.exe.push('x'),
            5 => bad.observation.window.title.push('x'),
            6 => bad.observation.window.version = "5.7.8.0".into(),
            7 => bad.observation.window.app = DesktopApp::Qoder,
            8 => bad.observation.session_id = Some("invented-provider-id".into()),
            9 => bad.observation.document_runtime_id.push('1'),
            10 => bad.observation.model_label = Some("other".into()),
            11 => bad.observation.draft_empty = false,
            12 => bad.observation.busy = true,
            13 => bad.observation.content_digest = "invalid".into(),
            14 => bad.observation.layout_digest = "invalid".into(),
            15 => bad.observation.node_count = 0,
            16 => bad.observation.node_count = 1001,
            17 => bad.turn_runtime_id.push('1'),
            18 => bad.prompt_runtime_id.push('1'),
            19 => bad.observed_baseline.reverse(),
            20 => bad.observed_baseline.truncate(1),
            21 => bad.observed_baseline[0] = "f".repeat(64),
            22 => bad.cancelled = false,
            23 => bad.first_observed_at = "2026-10-10T12:00:02Z".into(),
            24 => bad.observation.captured_at = "2026-10-10T12:00:04.249Z".into(),
            _ => bad.observation.captured_at = "invalid".into(),
        }
        assert!(validate_chain(&r, &q, &i, &bad).is_err(), "field {field}");
    }
}

#[test]
fn workbuddy_cancellation_keeps_stop_intent_bound_to_original_observation() {
    let (r, q, i, p) = fixture();
    for field in 0..8 {
        let mut intent = i.clone();
        let mut proof = p.clone();
        let running = &mut intent.observation;
        match field {
            0 => running.observation.window.pid += 1,
            1 => running.observation.document_runtime_id.push('1'),
            2 => running.observation.model_label = Some("other".into()),
            3 => running.observation.session_id = Some("invented-provider-id".into()),
            4 => running.baseline.reverse(),
            5 => running.task_review_digest = "f".repeat(64),
            6 => running.stop_runtime_id.clear(),
            _ => running.observation.captured_at = "2026-10-10T12:00:00Z".into(),
        }
        // Changing both later views cannot replace the original binding/view.
        proof.observation.window = running.observation.window.clone();
        proof.observation.document_runtime_id = running.observation.document_runtime_id.clone();
        proof.observation.model_label = running.observation.model_label.clone();
        proof.observation.session_id = running.observation.session_id.clone();
        proof.observed_baseline = running.baseline.clone();
        assert!(
            validate_chain(&r, &q, &intent, &proof).is_err(),
            "field {field}"
        );
    }
}

#[test]
fn workbuddy_cancellation_rejects_changed_original_local_review() {
    let (r, q, i, p) = fixture();
    for field in 0..12 {
        let mut original = r.clone();
        match field {
            0 => original.binding_id = "f".repeat(64),
            1 => original.attempt.binding.session_id.push('x'),
            2 => original.attempt.binding.workspace.push('x'),
            3 => original.attempt.id.push('x'),
            4 => original.attempt.fence += 1,
            5 => original.bridge_sha256 = "f".repeat(64),
            6 => original.prompt.push('x'),
            7 => original.baseline[0] = "f".repeat(64),
            8 => original.observation.window.pid += 1,
            9 => original.observation.model_label = Some("other".into()),
            10 => original.workflow_hash = "f".repeat(64),
            _ => original.project_id.push('x'),
        }
        assert!(validate_observation(&original, &q, &i.observation).is_err());
        assert!(validate_proof(&original, &i, &p).is_err(), "field {field}");
        original.id = review_hash(&original).unwrap();
        assert!(validate_chain(&original, &q, &i, &p).is_err());
    }
}

#[test]
fn workbuddy_cancellation_rejects_unsupported_or_provider_session_review_even_if_rehashed() {
    for field in 0..7 {
        let (mut r, mut q, mut i, mut p) = fixture();
        match field {
            0 => r.observation.window.version = "5.7.6.0".into(),
            1 => r.observation.window.version = "5.7.8.0".into(),
            2 => r.observation.session_id = Some("invented-provider-id".into()),
            3 => r.observation.document_runtime_id.clear(),
            4 => r.observation.model_label = None,
            5 => r.observation.model_label = Some(" ".into()),
            _ => r.binding_id = "invalid".into(),
        }
        r.id = review_hash(&r).unwrap();
        q.review_digest = r.id.clone();
        i.request_digest = digest(&q).unwrap();
        i.observation.task_review_digest = r.id.clone();
        p.task_review_digest = r.id.clone();
        for o in [&mut i.observation.observation, &mut p.observation] {
            o.window = r.observation.window.clone();
            o.document_runtime_id = r.observation.document_runtime_id.clone();
            o.model_label = r.observation.model_label.clone();
            o.session_id = r.observation.session_id.clone();
        }
        assert!(validate_observation(&r, &q, &i.observation).is_err());
        assert!(validate_proof(&r, &i, &p).is_err(), "field {field}");
    }
}

#[test]
fn qoder_cancellation_retains_legacy_status_and_provider_session_semantics() {
    let (mut r, mut q, mut i, mut p) = fixture();
    r.observation.window.app = DesktopApp::Qoder;
    r.observation.window.version = "0.4.3.0".into();
    r.observation.session_id = Some("12345678-1234-1234-1234-123456789abc".into());
    r.id = review_hash(&r).unwrap();
    q.review_digest = r.id.clone();
    i.request_digest = digest(&q).unwrap();
    i.observation.task_review_digest = r.id.clone();
    p.task_review_digest = r.id.clone();
    for o in [&mut i.observation.observation, &mut p.observation] {
        o.window = r.observation.window.clone();
        o.session_id = r.observation.session_id.clone();
    }
    for status in ["已取消", "取消", "已停止", "停止", "已中断", "中断"] {
        for suffix in ["", "，耗时：1s"] {
            p.status_text = format!("状态：{status}{suffix}");
            validate_chain(&r, &q, &i, &p).unwrap();
        }
    }
    for status in [
        "已取消",
        "任务已停止",
        "状态：已取消任务",
        "引用：状态：已取消",
    ] {
        p.status_text = status.into();
        assert!(validate_chain(&r, &q, &i, &p).is_err());
    }
    p.status_text = "状态：已中断，耗时：1s".into();
    p.observation.session_id = None;
    assert!(validate_chain(&r, &q, &i, &p).is_err());
}

#[test]
#[ignore = "requires an owned native C# WorkBuddy stop fixture report and explicit stage"]
fn native_workbuddy_stop_contract() {
    assert_eq!(
        std::env::var("CERULEAN_NATIVE_OWNED_JOB").as_deref(),
        Ok("1")
    );
    let path = std::path::PathBuf::from(
        std::env::var_os("CERULEAN_WORKBUDDY_STOP_REPORT")
            .expect("explicit owned native stop report directory is required"),
    );
    assert!(path.is_absolute() && path.is_dir());
    let stage = std::env::var("CERULEAN_WORKBUDDY_STOP_STAGE")
        .expect("explicit prepare or validate stage is required");
    match stage.as_str() {
        "prepare" => {
            let mut r: TaskReview = serde_json::from_slice(
                &std::fs::read(path.join("task-review-input.json")).unwrap(),
            )
            .unwrap();
            assert_eq!(r.observation.window.app, DesktopApp::Workbuddy);
            assert!(!r.observation.busy);
            r.id = review_hash(&r).unwrap();
            validate_local_review(&r).unwrap();
            let bytes = serde_json::to_vec_pretty(&r).unwrap();
            let restored: TaskReview = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(restored, r);
            assert_eq!(review_hash(&restored).unwrap(), r.id);
            std::fs::write(path.join("task-review.json"), bytes).unwrap();
        }
        "validate" => {
            let r: TaskReview =
                serde_json::from_slice(&std::fs::read(path.join("task-review.json")).unwrap())
                    .unwrap();
            assert_eq!(r.observation.window.app, DesktopApp::Workbuddy);
            assert_eq!(review_hash(&r).unwrap(), r.id);
            let running: serde_json::Value =
                serde_json::from_slice(&std::fs::read(path.join("running-prepare.json")).unwrap())
                    .unwrap();
            let cancelled: serde_json::Value =
                serde_json::from_slice(&std::fs::read(path.join("cancelled-proof.json")).unwrap())
                    .unwrap();
            assert_eq!(running["ok"], true);
            assert_eq!(cancelled["ok"], true);
            let o: StopObservation =
                serde_json::from_value(running["result"]["stopObservation"].clone()).unwrap();
            let p: StopProof =
                serde_json::from_value(cancelled["result"]["stopProof"].clone()).unwrap();

            // These two records are synthetic validator inputs. The observation
            // and proof are unchanged production C# responses from real UIA over
            // an owned fixture; this does not exercise durable user intent.
            let q = StopRequest {
                review_digest: r.id.clone(),
                actor: "user".into(),
                reason: "Owned native stop contract interoperability check".into(),
                requested_at: r.observation.captured_at.clone(),
            };
            let i = StopIntent {
                request_digest: digest(&q).unwrap(),
                reserved_at: o.observation.captured_at.clone(),
                observation: o,
            };
            assert!(at(&q.requested_at).unwrap() <= at(&i.reserved_at).unwrap());
            assert!(at(&i.reserved_at).unwrap() < at(&p.first_observed_at).unwrap());
            validate_request(&r, &q).unwrap();
            validate_observation(&r, &q, &i.observation).unwrap();
            validate_proof(&r, &i, &p).unwrap();

            let bytes = serde_json::to_vec(&(&q, &i, &p)).unwrap();
            let (restored_q, restored_i, restored_p): (StopRequest, StopIntent, StopProof) =
                serde_json::from_slice(&bytes).unwrap();
            assert_eq!((&restored_q, &restored_i, &restored_p), (&q, &i, &p));
            assert_eq!(restored_i.request_digest, digest(&restored_q).unwrap());
            validate_chain(&r, &restored_q, &restored_i, &restored_p).unwrap();
            std::fs::write(
                path.join("stop-contract-validation.json"),
                serde_json::to_vec_pretty(&serde_json::json!({
                    "status": "passed",
                    "taskReviewDigest": r.id,
                    "nativeStopObservationValidated": true,
                    "nativeStopProofValidated": true,
                    "serializationRoundTripValidated": true,
                    "syntheticRequestAndIntent": true,
                    "durableWorkflowTested": false
                }))
                .unwrap(),
            )
            .unwrap();
        }
        _ => panic!("CERULEAN_WORKBUDDY_STOP_STAGE must be prepare or validate"),
    }
}
