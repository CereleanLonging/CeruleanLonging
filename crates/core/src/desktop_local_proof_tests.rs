use super::*;
use cerulean_domain::desktop_sessions::*;

fn pair() -> (TaskReview, TaskProof) {
    let o = DesktopObservation {
        model_label: Some("Hy3".into()),
        window: DesktopWindow {
            app: DesktopApp::Workbuddy,
            hwnd: "123".into(),
            pid: 42,
            start_ticks: "123456".into(),
            exe: "D:\\apps\\WorkBuddy.exe".into(),
            title: "WorkBuddy".into(),
            version: "5.7.7.0".into(),
        },
        captured_at: "2026-10-10T12:00:00Z".into(),
        document_runtime_id: "42.1".into(),
        session_id: None,
        content_digest: "a".repeat(64),
        layout_digest: "b".repeat(64),
        node_count: 100,
        draft_empty: true,
        busy: false,
    };
    let r = TaskReview {
        id: "1".repeat(64),
        binding_id: "2".repeat(64),
        marker: format!("CL_TASK_{}", "3".repeat(32)),
        prompt: "synthetic approved prompt".into(),
        prompt_sha256: "4".repeat(64),
        bridge_sha256: "5".repeat(64),
        observation: o.clone(),
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
    let p = TaskProof {
        reader_sha256: None,
        marker: r.marker.clone(),
        session_id: None,
        local_binding: Some(DesktopTaskLocalBindingProof {
            binding_id: r.binding_id.clone(),
            task_review_digest: r.id.clone(),
            observation: o,
        }),
        prompt_sha256: r.prompt_sha256.clone(),
        observed_baseline: r.baseline[1..].to_vec(),
        reply_sha256: "b".repeat(64),
        observed_at: r.observation.captured_at.clone(),
        completed: true,
        round_sha256: Some("c".repeat(64)),
    };
    (r, p)
}

#[test]
fn workbuddy_proof_binds_local_review_and_rejects_identity_or_history_changes() {
    let (r, p) = pair();
    validate_proof(&r, &p).unwrap();
    for field in 0..22 {
        let mut bad = p.clone();
        match field {
            0 => bad.local_binding = None,
            1 => bad.session_id = Some("invented-provider-id".into()),
            2 => bad.local_binding.as_mut().unwrap().binding_id = "f".repeat(64),
            3 => bad.local_binding.as_mut().unwrap().task_review_digest = "f".repeat(64),
            4 => bad.local_binding.as_mut().unwrap().observation.window.pid += 1,
            5 => {
                bad.local_binding
                    .as_mut()
                    .unwrap()
                    .observation
                    .window
                    .start_ticks = "999".into()
            }
            6 => {
                bad.local_binding
                    .as_mut()
                    .unwrap()
                    .observation
                    .window
                    .version = "5.7.8.0".into()
            }
            7 => {
                bad.local_binding
                    .as_mut()
                    .unwrap()
                    .observation
                    .document_runtime_id = "other".into()
            }
            8 => bad.local_binding.as_mut().unwrap().observation.model_label = Some("other".into()),
            9 => bad.local_binding.as_mut().unwrap().observation.session_id = Some("fake".into()),
            10 => {
                bad.local_binding.as_mut().unwrap().observation.captured_at =
                    "2026-10-10T12:00:01Z".into()
            }
            11 => bad.local_binding.as_mut().unwrap().observation.draft_empty = false,
            12 => bad.local_binding.as_mut().unwrap().observation.busy = true,
            13 => bad.local_binding.as_mut().unwrap().observation.node_count = 0,
            14 => {
                bad.local_binding
                    .as_mut()
                    .unwrap()
                    .observation
                    .content_digest = "bad".into()
            }
            15 => bad.round_sha256 = None,
            16 => bad.observed_baseline.reverse(),
            17 => bad.observed_baseline.truncate(1),
            18 => bad.completed = false,
            19 => bad.marker.push('x'),
            20 => bad.prompt_sha256 = "e".repeat(64),
            _ => {
                bad.observed_at = "2026-10-09T12:00:00Z".into();
                bad.local_binding.as_mut().unwrap().observation.captured_at =
                    bad.observed_at.clone();
            }
        }
        assert!(validate_proof(&r, &bad).is_err(), "mutation {field}");
    }
}

#[test]
fn workbuddy_583_proof_and_dispatch_require_supported_original_version() {
    let (mut review, mut proof) = pair();
    review.observation.window.version = "5.8.3.0".into();
    proof
        .local_binding
        .as_mut()
        .unwrap()
        .observation
        .window
        .version = "5.8.3.0".into();
    validate_proof(&review, &proof).unwrap();
    validate_task_window(&review.observation.window).unwrap();
    proof
        .local_binding
        .as_mut()
        .unwrap()
        .observation
        .window
        .version = "5.7.7.0".into();
    assert!(validate_proof(&review, &proof).is_err());
    review.observation.window.version = "5.8.4.0".into();
    proof
        .local_binding
        .as_mut()
        .unwrap()
        .observation
        .window
        .version = "5.8.4.0".into();
    assert!(validate_proof(&review, &proof).is_err());
    assert!(validate_task_window(&review.observation.window).is_err());
}

#[test]
fn legacy_qoder_proof_keeps_serialized_shape_and_requires_provider_session() {
    let (mut r, mut p) = pair();
    r.observation.window.app = DesktopApp::Qoder;
    r.observation.window.version = "0.4.3.0".into();
    r.observation.session_id = Some("12345678-1234-1234-1234-123456789abc".into());
    p.session_id = r.observation.session_id.clone();
    p.local_binding = None;
    let bytes = serde_json::to_vec(&p).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(json["sessionId"].is_string());
    assert!(json.get("localBinding").is_none());
    assert_eq!(
        serde_json::to_vec(&serde_json::from_slice::<TaskProof>(&bytes).unwrap()).unwrap(),
        bytes
    );
    validate_proof(&r, &p).unwrap();
    p.session_id = None;
    assert!(validate_proof(&r, &p).is_err());
    r.observation.session_id = None;
    assert!(validate_proof(&r, &p).is_err());
}

#[test]
#[ignore = "requires owned native C# synthetic reply report"]
fn native_workbuddy_reply_contract() {
    assert_eq!(
        std::env::var("CERULEAN_NATIVE_OWNED_JOB").as_deref(),
        Ok("1")
    );
    let path = std::path::PathBuf::from(std::env::var("CERULEAN_WORKBUDDY_REPLY_REPORT").unwrap());
    let r: TaskReview =
        serde_json::from_slice(&std::fs::read(path.join("review.json")).unwrap()).unwrap();
    let value: serde_json::Value =
        serde_json::from_slice(&std::fs::read(path.join("success.json")).unwrap()).unwrap();
    let p: TaskProof = serde_json::from_value(value["taskProof"].clone()).unwrap();
    validate_proof(&r, &p).unwrap();
    let bytes = serde_json::to_vec(&p).unwrap();
    validate_proof(&r, &serde_json::from_slice(&bytes).unwrap()).unwrap();
}

#[test]
fn task_reader_provenance_accepts_historical_hash_but_rejects_malformed_identity() {
    let (review, mut proof) = pair();
    proof.reader_sha256 = Some("e".repeat(64));
    validate_proof(&review, &proof).unwrap();
    proof.reader_sha256 = Some("invalid-reader".into());
    assert!(validate_proof(&review, &proof).is_err());
}
