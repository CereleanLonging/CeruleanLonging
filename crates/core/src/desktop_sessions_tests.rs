use super::*;
use std::time::Instant;

#[path = "desktop_run_tests.rs"]
mod run_tests;

fn window(app: DesktopApp) -> DesktopWindow {
    DesktopWindow {
        exe: format!(
            "D:\\apps\\{}.exe",
            if app == DesktopApp::Qoder {
                "Qoder"
            } else {
                "WorkBuddy"
            }
        ),
        app,
        hwnd: "123456".into(),
        pid: 123,
        start_ticks: "123456789".into(),
        title: "工作台接入验证".into(),
        version: "fixture-1".into(),
    }
}
fn observation(w: DesktopWindow) -> DesktopObservation {
    DesktopObservation {
        model_label: Some("fixture-model".into()),
        session_id: (w.app == DesktopApp::Qoder)
            .then(|| "12345678-1234-1234-1234-123456789abc".into()),
        window: w,
        captured_at: chrono::Utc::now().to_rfc3339(),
        document_runtime_id: "42.1.2".into(),
        content_digest: "a".repeat(64),
        layout_digest: "b".repeat(64),
        node_count: 10,
        draft_empty: true,
        busy: false,
    }
}
fn reply(request: &ReadRequest) -> Result<ReadReply, String> {
    Ok(match request {
        ReadRequest::Discover { .. } => ReadReply {
            windows: vec![window(DesktopApp::Qoder), window(DesktopApp::Workbuddy)],
            observation: None,
            ..Default::default()
        },
        ReadRequest::Inspect { window, .. } => ReadReply {
            windows: vec![],
            observation: Some(observation(window.clone())),
            ..Default::default()
        },
        _ => return Err("unexpected mutable desktop request".into()),
    })
}
fn changed(request: &ReadRequest) -> Result<ReadReply, String> {
    let mut r = reply(request)?;
    r.observation.as_mut().unwrap().content_digest = "c".repeat(64);
    Ok(r)
}
fn spin(host: &mut RuntimeHost) {
    let started = Instant::now();
    while host.desktop_probe.is_some() {
        host.poll_desktop();
        assert!(started.elapsed() < Duration::from_secs(30));
        std::thread::sleep(Duration::from_millis(5));
    }
}
fn fixture(root: &Path) -> RuntimeHost {
    let (mut host, _) = project_planning::tests::fixture(root);
    host.desktop_fixture = Some(reply);
    host
}
fn inspect(host: &mut RuntimeHost, app: DesktopApp) {
    host.discover_desktop().unwrap();
    spin(host);
    host.inspect_desktop(window(app)).unwrap();
    spin(host);
    assert!(host.desktop_access.error.is_none());
}
fn historical_workbuddy_binding(host: &mut RuntimeHost) -> DesktopBinding {
    let mut binding = DesktopBinding {
        id: String::new(),
        project_id: "project".into(),
        label: "Historical WorkBuddy".into(),
        observation: observation(window(DesktopApp::Workbuddy)),
        confirmed_at: chrono::Utc::now().to_rfc3339(),
        bridge_sha256: fingerprint(),
    };
    binding.id = binding_digest(&binding).unwrap();
    persist(&host.root, &binding).unwrap();
    host.desktop_access.bindings.push(binding.clone());
    binding
}
#[test]
fn retired_workbuddy_is_hidden_and_rejected_before_observation_or_publication() {
    let root = tempfile::tempdir().unwrap();
    let mut host = fixture(root.path());
    host.discover_desktop().unwrap();
    spin(&mut host);
    assert_eq!(host.desktop_access.windows, vec![window(DesktopApp::Qoder)]);
    assert!(host
        .inspect_desktop(window(DesktopApp::Workbuddy))
        .unwrap_err()
        .contains("支持已移除"));
    let old = observation(window(DesktopApp::Workbuddy));
    let old_digest = digest(&old).unwrap();
    host.desktop_access.observation = Some(old.clone());
    host.desktop_access.observation_digest = Some(old_digest.clone());
    assert!(host
        .bind_desktop("project", &old_digest, "stale")
        .unwrap_err()
        .contains("支持已移除"));
    assert!(host.desktop_probe.is_none() && load(&host.root).unwrap().is_empty());
    assert!(observe(
        &host.root,
        &ReadRequest::Inspect {
            version: 1,
            window: old.window
        }
    )
    .unwrap_err()
    .contains("支持已移除"));
    assert!(!host.root.join("desktop-observer-v1").exists());
}
#[test]
fn retired_workbuddy_configurations_remain_readable_but_cannot_start_new_work() {
    let root = tempfile::tempdir().unwrap();
    let (mut host, draft) = desktop_plan_fixture(root.path(), DesktopApp::Workbuddy);
    draft.validate().unwrap(); // Historical format remains valid.
    assert!(host
        .configure_project(draft.clone())
        .unwrap_err()
        .contains("支持已移除"));
    assert!(host.project_setups.is_empty());
    // Additional profiles cannot bypass the default-executor restriction.
    let mut extra = draft.clone();
    extra.worker_adapter = cerulean_domain::project_setup::ProjectWorkerAdapter::Codex;
    extra
        .profiles
        .push(cerulean_domain::project_setup::WorkerProfileDraft {
            supervision_override: None,
            resource_keys: vec![],
            id: "retired".into(),
            description: "old".into(),
            worker_adapter: draft.worker_adapter,
            codex_executable: String::new(),
            worker_model: draft.worker_model.clone(),
            omp_runtime_parent: None,
            api_worker: None,
            desktop_worker: draft.desktop_worker.clone(),
            artifact_paths: vec!["sum.mjs".into()],
            acceptance: cerulean_domain::project_setup::AcceptanceDraft {
                schema_version: 1,
                stages: vec![],
            },
        });
    assert!(host
        .configure_project(extra)
        .unwrap_err()
        .contains("支持已移除"));
    project_planning::tests::save_historical_workbuddy_setup(&mut host, draft);
    let saved = host.project_setups["project"].clone();
    drop(host);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(host.project_setups["project"].digest, saved.digest);
    assert!(host
        .request_chief_plan("project", &saved.digest)
        .unwrap_err()
        .contains("支持已移除"));
    assert!(host
        .start_project_run("project", &"a".repeat(64), &saved.digest)
        .unwrap_err()
        .contains("支持已移除"));
    assert!(host.management.statuses().is_empty() && host.runs.all().is_empty());
    assert_eq!(host.desktop_access.bindings.len(), 1);
}
#[test]
fn user_binding_rechecks_qoder_and_retains_historical_workbuddy_on_reopen() {
    let root = tempfile::tempdir().unwrap();
    let mut host = fixture(root.path());
    {
        let app = DesktopApp::Qoder;
        inspect(&mut host, app);
        let digest = host.desktop_access.observation_digest.clone().unwrap();
        assert!(host.bind_desktop("missing", &digest, "chat").is_err());
        assert!(host
            .bind_desktop("project", &"0".repeat(64), "chat")
            .is_err());
        host.bind_desktop("project", &digest, "已确认聊天").unwrap();
        assert!(host.desktop_access.checking);
        assert!(host.bind_desktop("project", &digest, "duplicate").is_err());
        spin(&mut host);
        assert!(host.desktop_access.error.is_none());
        assert!(host.bind_desktop("project", &digest, "replay").is_err());
    }
    historical_workbuddy_binding(&mut host);
    let history = host.projects["project"].history().to_vec();
    let bindings = host.desktop_access.bindings.clone();
    assert_eq!(bindings.len(), 2);
    assert!(host.management.statuses().is_empty());
    assert!(host.api_jobs.is_empty());
    assert!(host.runs.all().is_empty());
    drop(host);
    let host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(host.desktop_access.bindings, bindings);
    assert!(host.desktop_access.observation.is_none());
    assert!(host.desktop_access.windows.is_empty());
    assert!(!host.desktop_access.checking);
    assert_eq!(host.projects["project"].history(), history);
}
#[test]
fn changed_session_or_expired_observation_never_persists_binding() {
    let root = tempfile::tempdir().unwrap();
    let mut host = fixture(root.path());
    inspect(&mut host, DesktopApp::Qoder);
    let digest = host.desktop_access.observation_digest.clone().unwrap();
    host.desktop_fixture = Some(changed);
    host.bind_desktop("project", &digest, "chat").unwrap();
    spin(&mut host);
    assert!(host
        .desktop_access
        .error
        .as_ref()
        .unwrap()
        .contains("已变化"));
    assert!(load(&host.root).unwrap().is_empty());
    host.desktop_fixture = Some(reply);
    inspect(&mut host, DesktopApp::Qoder);
    host.desktop_access
        .observation
        .as_mut()
        .unwrap()
        .captured_at = (chrono::Utc::now() - chrono::Duration::minutes(3)).to_rfc3339();
    let digest =
        cerulean_domain::execution::digest(&host.desktop_access.observation.as_ref().unwrap())
            .unwrap();
    host.desktop_access.observation_digest = Some(digest.clone());
    assert!(host
        .bind_desktop("project", &digest, "old")
        .unwrap_err()
        .contains("过期"));
    let original = observation(window(DesktopApp::Qoder));
    for field in 0..8 {
        let mut different = original.clone();
        match field {
            0 => different.window.start_ticks = "99".into(),
            1 => different.window.pid += 1,
            2 => different.window.version = "new".into(),
            3 => different.window.title = "other".into(),
            4 => different.session_id = Some("22222222-1234-1234-1234-123456789abc".into()),
            5 => different.content_digest = "c".repeat(64),
            6 => different.layout_digest = "d".repeat(64),
            _ => different.document_runtime_id = "other".into(),
        };
        assert!(!same_view(&original, &different));
    }
    let mut bad = original.clone();
    bad.busy = true;
    assert!(validate_observation(&bad).is_err());
    bad = original;
    bad.draft_empty = false;
    assert!(validate_observation(&bad).is_err());
    let mut bad = observation(window(DesktopApp::Workbuddy));
    bad.session_id = Some("made-up".into());
    assert!(validate_observation(&bad).is_err());
}
#[test]
fn binding_publication_keeps_complete_pending_and_rejects_conflicts_or_tamper() {
    let root = tempfile::tempdir().unwrap();
    let mut binding = DesktopBinding {
        id: String::new(),
        project_id: "project".into(),
        label: "chat".into(),
        observation: observation(window(DesktopApp::Workbuddy)),
        confirmed_at: chrono::Utc::now().to_rfc3339(),
        bridge_sha256: fingerprint(),
    };
    binding.id = binding_digest(&binding).unwrap();
    persist(root.path(), &binding).unwrap();
    let folder = root.path().join("desktop-bindings-v1");
    let formal = folder.join(format!("{}.json", binding.id));
    let pending = folder.join(format!("{}.pending", binding.id));
    let bytes = fs::read(&formal).unwrap();
    fs::write(&pending, &bytes).unwrap();
    assert_eq!(load(root.path()).unwrap(), vec![binding.clone()]);
    fs::remove_file(&formal).unwrap();
    assert_eq!(load(root.path()).unwrap(), vec![binding.clone()]);
    fs::write(&formal, &bytes).unwrap();
    fs::write(&pending, b"{").unwrap();
    assert!(load(root.path()).is_err());
    assert_eq!(fs::read(&formal).unwrap(), bytes);
    fs::remove_file(pending).unwrap();
    binding.label = "changed".into();
    fs::write(&formal, serde_json::to_vec(&binding).unwrap()).unwrap();
    assert!(load(root.path()).is_err());
}
#[test]
fn embedded_observer_runs_owned_and_rejects_unrelated_window_without_input() {
    let root = tempfile::tempdir().unwrap();
    let mut unrelated = window(DesktopApp::Qoder);
    unrelated.hwnd = "1".into();
    for path in [
        root.path().to_path_buf(),
        fs::canonicalize(root.path()).unwrap(),
    ] {
        let result = observe(
            &path,
            &ReadRequest::Inspect {
                version: 1,
                window: unrelated.clone(),
            },
        );
        assert_eq!(
            result.unwrap_err(),
            "桌面观察暂不可用，请核对应用窗口后重试",
            "observer must run and return its structured rejection for {path:?}"
        );
    }
    let dir = root.path().join("desktop-observer-v1");
    // Also exercise the suspended, foreground-delegating owned process path.
    // An unrelated HWND is rejected by the embedded bridge before any edit.
    let mut target = unrelated.clone();
    target.version = "0.4.3.0".into();
    let marker = format!("CL_DESKTOP_{}", "1".repeat(32));
    let request = ReadRequest::TrialSend {
        version: 1,
        review: DesktopTrialReview {
            id: "1".repeat(64),
            binding_id: "2".repeat(64),
            observation: observation(target),
            marker: marker.clone(),
            prompt: desktop_trials::prompt(&marker),
            baseline: vec!["3".repeat(64), "4".repeat(64)],
            bridge_sha256: fingerprint(),
        },
    };
    assert_eq!(
        observe(root.path(), &request).unwrap_err(),
        "桌面观察暂不可用，请核对应用窗口后重试"
    );
    assert!(dir.join(format!("{}.exe", fingerprint())).is_file());
    // Verify byte pinning in an independent cache. Overwriting a recently
    // executed image can hit a transient Windows sharing lock even after the
    // owned Job has drained; production never overwrites a cached image.
    let tampered = tempfile::tempdir().unwrap();
    let tampered_dir = tampered.path().join("desktop-observer-v1");
    fs::create_dir(&tampered_dir).unwrap();
    fs::write(
        tampered_dir.join(format!("{}.exe", fingerprint())),
        b"changed",
    )
    .unwrap();
    assert!(
        observe(tampered.path(), &ReadRequest::Discover { version: 1 })
            .unwrap_err()
            .contains("内容已变化")
    );
}
#[test]
#[ignore = "explicit live read-only Qoder/WorkBuddy inspection; no input or model calls"]
fn live_desktop_observation() {
    let root = PathBuf::from(std::env::var_os("CERULEAN_DESKTOP_OBSERVE_DIR").unwrap());
    assert!(root.is_absolute() && root.is_dir());
    let list = observe(&root, &ReadRequest::Discover { version: 1 }).unwrap();
    let mut results = vec![];
    for w in list.windows {
        let result = observe(
            &root,
            &ReadRequest::Inspect {
                version: 1,
                window: w.clone(),
            },
        );
        results.push(serde_json::json!({"window":w,"observation":result.as_ref().ok().and_then(|r|r.observation.as_ref()),"error":result.err()}));
    }
    fs::write(
        root.join("result.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"readOnly":true,"externalMessages":0,"windows":results}),
        )
        .unwrap(),
    )
    .unwrap();
}
#[test]
#[ignore = "native UI service with synthetic desktop observations, no external application mutation"]
fn native_desktop_binding_server() {
    let root = PathBuf::from(std::env::var_os("CERULEAN_DESKTOP_NATIVE_DIR").unwrap());
    assert!(root.is_absolute() && root.is_dir());
    assert!(!root.join("data").exists());
    let host = fixture(&root);
    let data = root.join("data");
    fs::write(
        root.join("fixture.json"),
        serde_json::to_vec(&serde_json::json!({"data":data})).unwrap(),
    )
    .unwrap();
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(crate::ipc::serve_host(&data, host))
        .unwrap();
}

fn trial_reply(request: &ReadRequest) -> Result<ReadReply, String> {
    match request {
        ReadRequest::TrialPrepare { window, .. } => Ok(ReadReply {
            observation: Some(observation(window.clone())),
            baseline: vec!["1".repeat(64), "2".repeat(64)],
            ..Default::default()
        }),
        ReadRequest::TrialSend { review, .. } | ReadRequest::TrialRead { review, .. } => {
            Ok(ReadReply {
                trial_proof: Some(DesktopTrialProof {
                    observed_baseline: None,
                    reader_sha256: None,
                    marker: review.marker.clone(),
                    session_id: review.observation.session_id.clone().unwrap(),
                    baseline: review.baseline.clone(),
                    prompt_seen: true,
                    completed: true,
                    reply_matches: true,
                    observed_at: chrono::Utc::now().to_rfc3339(),
                }),
                ..Default::default()
            })
        }
        _ => reply(request),
    }
}
fn uncertain_trial(request: &ReadRequest) -> Result<ReadReply, String> {
    if matches!(request, ReadRequest::TrialSend { .. }) {
        Err("fixture interrupted after possible send".into())
    } else {
        trial_reply(request)
    }
}
fn changed_trial(request: &ReadRequest) -> Result<ReadReply, String> {
    let mut r = trial_reply(request)?;
    if let Some(o) = r.observation.as_mut() {
        o.content_digest = "c".repeat(64);
    }
    Ok(r)
}
fn bad_trial(request: &ReadRequest) -> Result<ReadReply, String> {
    let mut r = trial_reply(request)?;
    if let Some(p) = r.trial_proof.as_mut() {
        p.marker = "old-marker".into();
    }
    Ok(r)
}
fn trial_binding(host: &mut RuntimeHost) -> String {
    let mut w = window(DesktopApp::Qoder);
    w.version = "0.4.3.0".into();
    host.desktop_access.windows.push(w.clone());
    host.inspect_desktop(w).unwrap();
    spin(host);
    let digest = host.desktop_access.observation_digest.clone().unwrap();
    host.bind_desktop("project", &digest, "Qoder验证").unwrap();
    spin(host);
    host.desktop_access.bindings.last().unwrap().id.clone()
}
#[test]
fn desktop_trial_confirmation_is_exact_and_intent_survives_uncertain_send_without_replay() {
    let root = tempfile::tempdir().unwrap();
    let mut host = fixture(root.path());
    host.desktop_fixture = Some(uncertain_trial);
    let binding = trial_binding(&mut host);
    let history = host.projects["project"].history().to_vec();
    host.prepare_desktop_trial(&binding).unwrap();
    spin(&mut host);
    let review = host.desktop_access.trial_review.clone().unwrap();
    assert!(host.send_desktop_trial(&"a".repeat(64)).is_err());
    assert!(load_trials(&host.root).unwrap().is_empty());
    host.send_desktop_trial(&review.id).unwrap();
    // Publication precedes both receipt polling and worker completion.
    assert_eq!(load_trials(&host.root).unwrap().len(), 1);
    assert!(host.send_desktop_trial(&review.id).is_err());
    assert!(!host.can_shutdown());
    spin(&mut host);
    assert!(host.desktop_access.error.is_some());
    assert!(host.desktop_access.trials[0].proof.is_none());
    assert!(host.prepare_desktop_trial(&binding).is_err());
    drop(host);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert!(host.desktop_access.trial_review.is_none());
    assert!(host.desktop_probe.is_none());
    assert_eq!(host.desktop_access.trials.len(), 1);
    assert!(host.desktop_access.trials[0].proof.is_none());
    assert_eq!(host.projects["project"].history(), history);
    assert!(host.runs.all().is_empty());
    assert!(host.management.statuses().is_empty());
    assert!(host.prepare_desktop_trial(&binding).is_err());
    assert!(host.send_desktop_trial(&review.id).is_err());
    host.desktop_fixture = Some(bad_trial);
    host.read_desktop_trial(&review.id, &fingerprint()).unwrap();
    spin(&mut host);
    assert!(host.desktop_access.trials[0].proof.is_none());
    host.desktop_fixture = Some(trial_reply);
    host.read_desktop_trial(&review.id, &fingerprint()).unwrap();
    spin(&mut host);
    assert!(host.desktop_access.error.is_none());
    assert!(host.desktop_access.trials[0].proof.is_some());
    assert!(host.read_desktop_trial(&review.id, &fingerprint()).is_err());
    assert!(host.send_desktop_trial(&review.id).is_err());
}
#[test]
fn desktop_trial_pending_publication_is_read_logically_and_conflicts_block_new_sends() {
    let root = tempfile::tempdir().unwrap();
    let mut host = fixture(root.path());
    host.desktop_fixture = Some(trial_reply);
    let binding = trial_binding(&mut host);
    host.prepare_desktop_trial(&binding).unwrap();
    spin(&mut host);
    let review = host.desktop_access.trial_review.clone().unwrap();
    host.send_desktop_trial(&review.id).unwrap();
    spin(&mut host);
    assert!(host.desktop_access.trials[0].proof.is_some());
    let dir = host.root.join("desktop-trials-v1");
    let intent = dir.join(format!("{}.intent.json", review.id));
    let pending = dir.join(format!("{}.intent.pending", review.id));
    let original = fs::read(&intent).unwrap();
    fs::write(&pending, &original).unwrap();
    assert!(load_trials(&host.root).unwrap()[0].proof.is_some());
    fs::remove_file(&intent).unwrap();
    assert!(load_trials(&host.root).unwrap()[0].proof.is_some());
    fs::write(&intent, &original).unwrap();
    fs::write(&pending, b"{").unwrap();
    assert!(host.prepare_desktop_trial(&binding).is_err());
    assert!(load_trials(&host.root).is_err());
    assert_eq!(fs::read(&intent).unwrap(), original);
    fs::remove_file(&pending).unwrap();
    fs::remove_file(&intent).unwrap();
    assert!(load_trials(&host.root).unwrap_err().contains("缺少原始"));
}
#[test]
fn desktop_trial_rejects_wrong_app_version_stale_binding_and_expired_confirmation() {
    let root = tempfile::tempdir().unwrap();
    let mut host = fixture(root.path());
    host.desktop_fixture = Some(trial_reply);
    let wb = historical_workbuddy_binding(&mut host).id;
    assert!(host
        .prepare_desktop_trial(&wb)
        .unwrap_err()
        .contains("尚未开放"));
    let binding = trial_binding(&mut host);
    host.desktop_fixture = Some(changed_trial);
    host.prepare_desktop_trial(&binding).unwrap();
    spin(&mut host);
    assert!(host.desktop_access.trial_review.is_none());
    assert!(host
        .desktop_access
        .error
        .as_ref()
        .unwrap()
        .contains("历史关联已变化"));
    host.desktop_fixture = Some(trial_reply);
    host.prepare_desktop_trial(&binding).unwrap();
    spin(&mut host);
    let mut review = host.desktop_access.trial_review.clone().unwrap();
    review.observation.captured_at =
        (chrono::Utc::now() - chrono::Duration::minutes(3)).to_rfc3339();
    let mut hashable = review.clone();
    hashable.id.clear();
    review.id = cerulean_domain::execution::digest(&hashable).unwrap();
    host.desktop_access.trial_review = Some(review.clone());
    assert!(host
        .send_desktop_trial(&review.id)
        .unwrap_err()
        .contains("过期"));
    assert!(load_trials(&host.root).unwrap().is_empty());
}
#[test]
#[ignore = "native UI trial service with synthetic observations and responses; no real desktop input"]
fn native_desktop_trial_server() {
    let root = PathBuf::from(std::env::var_os("CERULEAN_DESKTOP_NATIVE_DIR").unwrap());
    assert!(root.is_absolute() && root.is_dir());
    assert!(!root.join("data").exists());
    let mut host = fixture(&root);
    host.desktop_fixture = Some(trial_reply);
    let binding = trial_binding(&mut host);
    let data = root.join("data");
    fs::write(
        root.join("fixture.json"),
        serde_json::to_vec(&serde_json::json!({"data":data,"binding":binding})).unwrap(),
    )
    .unwrap();
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(crate::ipc::serve_host(&data, host))
        .unwrap();
}

#[test]
#[ignore = "seed an isolated product project for an explicitly authorized live desktop trial; no model transport fixture"]
fn live_desktop_trial_seed() {
    let root = PathBuf::from(std::env::var_os("CERULEAN_DESKTOP_NATIVE_DIR").unwrap());
    assert!(root.is_absolute() && root.is_dir());
    assert!(!root.join("data").exists());
    let (host, _) = project_planning::tests::fixture(&root);
    drop(host);
    fs::write(
        root.join("fixture.json"),
        serde_json::to_vec(&serde_json::json!({"data":root.join("data")})).unwrap(),
    )
    .unwrap();
}

fn suffix_trial(request: &ReadRequest) -> Result<ReadReply, String> {
    if matches!(request, ReadRequest::TrialSend { .. }) {
        return Err("fixture interrupted after send".into());
    }
    let mut reply = trial_reply(request)?;
    if matches!(request, ReadRequest::TrialPrepare { .. }) {
        reply.baseline = vec![
            "1".repeat(64),
            "2".repeat(64),
            "3".repeat(64),
            "4".repeat(64),
        ];
    }
    if let Some(p) = reply.trial_proof.as_mut() {
        p.observed_baseline = Some(p.baseline[1..].to_vec());
    }
    Ok(reply)
}
fn insufficient_history(request: &ReadRequest) -> Result<ReadReply, String> {
    let mut reply = suffix_trial(request)?;
    if let Some(p) = reply.trial_proof.as_mut() {
        p.observed_baseline = Some(vec![p.baseline.last().unwrap().clone()]);
    }
    Ok(reply)
}
fn foreign_history(request: &ReadRequest) -> Result<ReadReply, String> {
    let mut reply = suffix_trial(request)?;
    if let Some(p) = reply.trial_proof.as_mut() {
        p.observed_baseline.as_mut().unwrap()[0] = "f".repeat(64);
    }
    Ok(reply)
}
#[test]
fn desktop_trial_clipped_history_requires_two_exact_suffix_rounds_and_current_reader_authorization()
{
    let root = tempfile::tempdir().unwrap();
    let mut host = fixture(root.path());
    host.desktop_fixture = Some(suffix_trial);
    let binding = trial_binding(&mut host);
    host.prepare_desktop_trial(&binding).unwrap();
    spin(&mut host);
    let review = host.desktop_access.trial_review.clone().unwrap();
    host.send_desktop_trial(&review.id).unwrap();
    spin(&mut host);
    let intent = host
        .root
        .join("desktop-trials-v1")
        .join(format!("{}.intent.json", review.id));
    let bytes = fs::read(&intent).unwrap();
    assert!(host
        .read_desktop_trial(&review.id, &"0".repeat(64))
        .unwrap_err()
        .contains("已更新"));
    assert!(host.desktop_probe.is_none());
    for f in [insufficient_history as Fixture, foreign_history] {
        host.desktop_fixture = Some(f);
        host.read_desktop_trial(&review.id, &fingerprint()).unwrap();
        spin(&mut host);
        assert!(host.desktop_access.trials[0].proof.is_none());
        assert!(host
            .desktop_access
            .error
            .as_ref()
            .unwrap()
            .contains("原历史后缀"));
    }
    host.desktop_fixture = Some(suffix_trial);
    host.read_desktop_trial(&review.id, &fingerprint()).unwrap();
    spin(&mut host);
    let proof = host.desktop_access.trials[0].proof.as_ref().unwrap();
    assert_eq!(proof.observed_baseline.as_ref().unwrap().len(), 3);
    assert_eq!(proof.baseline.len(), 4);
    assert_eq!(proof.reader_sha256.as_deref(), Some(fingerprint().as_str()));
    assert_eq!(fs::read(&intent).unwrap(), bytes);
    let expected = host.desktop_access.trials.clone();
    drop(host);
    let host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(host.desktop_access.trials, expected);
    assert!(host.desktop_probe.is_none());
}

fn desktop_plan_fixture(
    root: &Path,
    app: DesktopApp,
) -> (
    RuntimeHost,
    cerulean_domain::project_setup::ProjectSetupDraft,
) {
    use cerulean_domain::project_setup::ProjectWorkerAdapter;
    let (mut host, mut draft) = project_planning::tests::fixture(root);
    host.desktop_fixture = Some(reply);
    let binding = if app == DesktopApp::Workbuddy {
        historical_workbuddy_binding(&mut host)
    } else {
        inspect(&mut host, app.clone());
        let observed = host.desktop_access.observation_digest.clone().unwrap();
        host.bind_desktop("project", &observed, "编程验证聊天")
            .unwrap();
        spin(&mut host);
        host.desktop_access.bindings[0].clone()
    };
    draft.worker_adapter = match app {
        DesktopApp::Qoder => ProjectWorkerAdapter::Qoder,
        DesktopApp::Workbuddy => ProjectWorkerAdapter::Workbuddy,
    };
    draft.codex_executable.clear();
    draft.worker_model = binding.observation.model_label.clone().unwrap();
    draft.desktop_worker = Some(DesktopWorkerSelection {
        binding,
        project_root: host.snapshot().workflows[0].project.root.clone(),
        model_label: draft.worker_model.clone(),
        workspace_confirmed: true,
        external_tools_acknowledged: true,
    });
    (host, draft)
}
fn desktop_proposal(adapter: &str) -> cerulean_domain::management::ManagementDecision {
    let mut proposal = project_planning::tests::proposal("tests");
    if let cerulean_domain::management::ManagementDecision::Plan { tasks, .. } = &mut proposal {
        tasks[0].worker_id = format!("worker-{adapter}");
    }
    proposal
}
#[test]
fn desktop_plan_preserves_approval_and_reopens_without_dispatch_or_model_calls() {
    {
        let app = DesktopApp::Qoder;
        let root = tempfile::tempdir().unwrap();
        let (mut host, draft) = desktop_plan_fixture(root.path(), app);
        let selection = draft.desktop_worker.clone().unwrap();
        host.configure_project(draft).unwrap();
        project_planning::tests::complete(&mut host, desktop_proposal(selection.adapter_id()));
        let revision = host.projects["project"].revision().unwrap().clone();
        revision.validate().unwrap();
        assert_eq!(
            revision.execution[0].desktop_worker.as_ref(),
            Some(&selection)
        );
        assert!(revision.policy.tools.contains(&ToolPermission::DesktopSend));
        let digest = revision.approval_digest().unwrap();
        let mut changed = revision.clone();
        changed.execution[0]
            .desktop_worker
            .as_mut()
            .unwrap()
            .workspace_confirmed = false;
        assert!(changed.approval_digest().is_err());
        assert!(changed.validate().is_err());
        changed = revision.clone();
        let selected = changed.execution[0].desktop_worker.as_mut().unwrap();
        selected.model_label = "another-model".into();
        selected.binding.observation.model_label = Some(selected.model_label.clone());
        selected.binding.id = binding_digest(&selected.binding).unwrap();
        assert_ne!(changed.approval_digest().unwrap(), digest);
        assert!(host
            .handle(Request {
                id: "approve".into(),
                protocol_version: PROTOCOL_VERSION,
                command: Command::ApproveRevision {
                    project_id: "project".into(),
                    digest: digest.clone()
                }
            })
            .error
            .is_none());
        let setup_digest = host.project_setups["project"].digest.clone();
        let decisions = host.management.statuses().len();
        assert!(host
            .start_project_run("project", &digest, &setup_digest)
            .unwrap_err()
            .contains("尚未接入"));
        assert_eq!(host.management.statuses().len(), decisions);
        assert!(host.runs.all().is_empty());
        assert!(host.api_jobs.is_empty() && host.codex_jobs.is_empty() && host.jobs.is_empty());
        drop(host);
        let host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(host.projects["project"].revision().unwrap(), &revision);
        assert_eq!(
            host.project_setups["project"].setup.desktop_worker.as_ref(),
            Some(&selection)
        );
        assert_eq!(
            host.projects["project"].approval().unwrap().revision_digest,
            digest
        );
    }
}
#[test]
fn desktop_plan_rejects_forged_selection_missing_ack_and_wrong_scope_before_publish() {
    let root = tempfile::tempdir().unwrap();
    let (mut host, draft) = desktop_plan_fixture(root.path(), DesktopApp::Qoder);
    for case in 0..11 {
        let mut bad = draft.clone();
        let d = bad.desktop_worker.as_mut().unwrap();
        match case {
            0 => d.workspace_confirmed = false,
            1 => d.external_tools_acknowledged = false,
            2 => d.project_root = root.path().to_string_lossy().into(),
            3 => d.model_label = "different-model".into(),
            4 => {
                d.binding.label = "forged".into();
                d.binding.id = binding_digest(&d.binding).unwrap();
            }
            5 => {
                d.binding.project_id = "other-project".into();
                d.binding.id = binding_digest(&d.binding).unwrap();
            }
            6 => bad.worker_model = "different-model".into(),
            7 => {
                bad.worker_adapter = cerulean_domain::project_setup::ProjectWorkerAdapter::Workbuddy
            }
            8 => bad.codex_executable = root.path().join("codex.exe").to_string_lossy().into(),
            9 => {
                d.binding.observation.model_label = None;
                d.binding.id = binding_digest(&d.binding).unwrap();
            }
            _ => bad.desktop_worker = None,
        }
        assert!(host.configure_project(bad).is_err(), "case {case}");
        assert!(host.project_setups.is_empty());
        assert!(host.management.statuses().is_empty());
    }
    assert_eq!(
        fs::read_dir(root.path().join("data/project-setups"))
            .unwrap()
            .count(),
        0
    );
}
#[test]
fn desktop_plan_rejects_wrong_identity_receipts_and_unapproved_tools() {
    use cerulean_domain::execution::*;
    let root = tempfile::tempdir().unwrap();
    let (mut host, draft) = desktop_plan_fixture(root.path(), DesktopApp::Qoder);
    let selection = draft.desktop_worker.clone().unwrap();
    host.configure_project(draft).unwrap();
    project_planning::tests::complete(&mut host, desktop_proposal("qoder"));
    let revision = host.projects["project"].revision().unwrap().clone();
    let binding = SessionBinding {
        adapter_id: "qoder".into(),
        adapter_version: format!("desktop-ui-v1:{}", selection.binding.bridge_sha256),
        session_id: selection.session_id(),
        identity_strength: IdentityStrength::UiObservation,
        workspace: selection.project_root.clone(),
    };
    let attempt = Attempt {
        workspace_proof: None,
        id: "attempt-1".into(),
        task_id: "task".into(),
        worker_id: "worker-qoder".into(),
        revision_digest: revision.approval_digest().unwrap(),
        fence: 1,
        binding: binding.clone(),
    };
    let receipt = ExecutionReceipt {
        schema_version: 1,
        task_id: "task".into(),
        attempt_id: attempt.id.clone(),
        fence: 1,
        binding,
        turn_id: None,
        observed_at: chrono::Utc::now().to_rfc3339(),
        status: ReceiptStatus::Completed,
        evidence_sha256: "a".repeat(64),
    };
    receipt.validate(&revision, &attempt, None).unwrap();
    for case in 0..6 {
        let mut bad = receipt.clone();
        match case {
            0 => bad.binding.identity_strength = IdentityStrength::ProviderId,
            1 => bad.binding.session_id = "other-chat".into(),
            2 => bad.binding.workspace = root.path().to_string_lossy().into(),
            3 => bad.binding.adapter_version = "other-bridge".into(),
            4 => bad.binding.adapter_id = "workbuddy".into(),
            _ => bad.turn_id = Some("fake-provider-turn".into()),
        }
        let mut forged_attempt = attempt.clone();
        forged_attempt.binding = bad.binding.clone();
        assert!(
            bad.validate(&revision, &forged_attempt, bad.turn_id.as_deref())
                .is_err(),
            "case {case}"
        );
    }
    for case in 0..3 {
        let mut bad = revision.clone();
        match case {
            0 => bad
                .policy
                .tools
                .retain(|t| t != &ToolPermission::DesktopSend),
            1 => bad.policy.budget_usd_cents = Some(100),
            _ => bad.execution[0].desktop_worker = None,
        }
        assert!(bad.validate().is_err(), "policy case {case}");
    }
}
#[test]
fn desktop_plan_cannot_reopen_after_binding_removed_and_legacy_observation_is_stable() {
    let root = tempfile::tempdir().unwrap();
    let (mut host, draft) = desktop_plan_fixture(root.path(), DesktopApp::Workbuddy);
    let binding = draft.desktop_worker.as_ref().unwrap().binding.clone();
    project_planning::tests::save_historical_workbuddy_setup(&mut host, draft);
    drop(host);
    let path = root
        .path()
        .join("data/desktop-bindings-v1")
        .join(format!("{}.json", binding.id));
    let bytes = fs::read(&path).unwrap();
    fs::remove_file(&path).unwrap();
    assert!(RuntimeHost::open(&root.path().join("data")).is_err());
    fs::write(&path, bytes).unwrap();
    RuntimeHost::open(&root.path().join("data")).unwrap();
    let mut legacy = binding;
    legacy.observation.model_label = None;
    legacy.id = binding_digest(&legacy).unwrap();
    let json = serde_json::to_value(&legacy).unwrap();
    assert!(json["observation"].get("modelLabel").is_none());
    let restored: DesktopBinding = serde_json::from_value(json).unwrap();
    assert_eq!(binding_digest(&restored).unwrap(), legacy.id);
}
#[test]
#[ignore = "isolated desktop configuration fixture; synthetic observations, no external input"]
fn native_desktop_plan_configuration_fixture() {
    let root =
        fs::canonicalize(std::env::var_os("CERULEAN_DESKTOP_PLAN_NATIVE_DIR").unwrap()).unwrap();
    let reports =
        fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports")).unwrap();
    assert!(root.starts_with(reports) && !root.join("data").exists());
    let (mut host, mut draft) = desktop_plan_fixture(&root, DesktopApp::Qoder);
    inspect(&mut host, DesktopApp::Workbuddy);
    let observed = host.desktop_access.observation_digest.clone().unwrap();
    host.bind_desktop("project", &observed, "WorkBuddy编程验证聊天")
        .unwrap();
    spin(&mut host);
    // Start with an ordinary local configuration; UI must choose and acknowledge desktop explicitly.
    draft.worker_adapter = Default::default();
    draft.desktop_worker = None;
    draft.codex_executable = root.join("codex.exe").to_string_lossy().into();
    host.configure_project(draft).unwrap();
}
#[test]
#[ignore = "synthetic chief proposal after native UI saved the desktop configuration"]
fn native_desktop_plan_proposal_fixture() {
    let root =
        fs::canonicalize(std::env::var_os("CERULEAN_DESKTOP_PLAN_NATIVE_DIR").unwrap()).unwrap();
    let reports =
        fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports")).unwrap();
    assert!(root.starts_with(reports));
    let mut host = RuntimeHost::open(&root.join("data")).unwrap();
    assert!(host.project_setups["project"]
        .setup
        .desktop_worker
        .is_some());
    assert!(host.management.statuses().is_empty());
    project_planning::tests::complete(&mut host, desktop_proposal("qoder"));
    assert!(host.projects["project"].revision().is_some());
}

pub(super) fn assigned_desktop_task(
    root: &Path,
) -> (RuntimeHost, crate::desktop_execution::TaskReview) {
    assigned_desktop_task_with_validator(root, false)
}
pub(crate) fn assigned_desktop_task_with_validator(
    root: &Path,
    real_node: bool,
) -> (RuntimeHost, crate::desktop_execution::TaskReview) {
    use crate::management_execution::{ApplicationTarget, DecisionIntent};
    use cerulean_adapters::openai::DecisionObservation;
    use cerulean_domain::management::{DecisionEnvelope, ManagementDecision, Purpose};
    let (mut host, mut draft) = desktop_plan_fixture(root, DesktopApp::Qoder);
    let b = &mut draft.desktop_worker.as_mut().unwrap().binding;
    b.observation.window.version = "0.4.3.0".into();
    b.id = binding_digest(b).unwrap();
    persist(&host.root, b).unwrap();
    host.desktop_access.bindings.push(b.clone());
    if real_node {
        let node = std::env::split_paths(&std::env::var_os("PATH").unwrap())
            .map(|p| p.join("node.exe"))
            .find(|p| p.is_file())
            .expect("Node is required for actual desktop completion acceptance");
        host.register_validator(
            "node-sum".into(),
            "JavaScript behavior check".into(),
            node.to_string_lossy().into(),
            vec![],
            Default::default(),
        )
        .unwrap();
        draft.validator_id = "node-sum".into();
        draft.check_args=vec!["--input-type=module".into(),"--eval".into(),"import assert from 'node:assert/strict'; import {sum} from './sum.mjs'; assert.equal(sum(2,3),5); assert.equal(sum(-7,4),-3); assert.equal(sum(0,0),0); console.log('independent sum checks passed');".into()];
    }
    draft.management_max_requests = 10;
    host.configure_project(draft).unwrap();
    let mut proposed = desktop_proposal("qoder");
    if real_node {
        if let ManagementDecision::Plan { tasks, .. } = &mut proposed {
            tasks[0].validator_id = "node-sum".into();
        }
    }
    project_planning::tests::complete(&mut host, proposed);
    let revision = host.projects["project"].revision().unwrap().clone();
    assert!(host
        .handle(Request {
            id: "approve".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::ApproveRevision {
                project_id: "project".into(),
                digest: revision.approval_digest().unwrap()
            }
        })
        .error
        .is_none());
    let context = host
        .management_context("project", "Assign approved task")
        .unwrap();
    let state_digest = digest(&context).unwrap();
    host.management
        .begin(DecisionIntent {
            operation_id: "desktop-assign".into(),
            project_id: "project".into(),
            purpose: Purpose::Assign,
            model: "fixture-deputy".into(),
            state_digest: state_digest.clone(),
            context,
            created_at: chrono::Utc::now().to_rfc3339(),
        })
        .unwrap();
    host.management
        .finish(
            "desktop-assign",
            Ok(DecisionObservation {
                response_id: "resp_desktop_assign".into(),
                requested_model: "fixture-deputy".into(),
                actual_model: "fixture-deputy".into(),
                input_tokens: 1,
                output_tokens: 1,
                envelope: DecisionEnvelope {
                    state_digest,
                    decision: ManagementDecision::Assign {
                        task_id: "task".into(),
                        worker_id: "worker-qoder".into(),
                        rationale: "Approved desktop task".into(),
                    },
                },
            }),
        )
        .unwrap();
    let d = revision.execution[0].desktop_worker.as_ref().unwrap();
    let attempt = Attempt {
        workspace_proof: None,
        id: "desktop-attempt".into(),
        task_id: "task".into(),
        worker_id: "worker-qoder".into(),
        revision_digest: revision.approval_digest().unwrap(),
        fence: 1,
        binding: SessionBinding {
            adapter_id: "qoder".into(),
            adapter_version: format!("desktop-ui-v1:{}", d.binding.bridge_sha256),
            session_id: d.session_id(),
            identity_strength: IdentityStrength::UiObservation,
            workspace: revision.project.root.clone(),
        },
    };
    host.apply_management_decision(
        "desktop-assign",
        ApplicationTarget::Workflow {
            principal: "deputy".into(),
            command: WorkflowCommand::Assign {
                task: "task".into(),
                worker: attempt.worker_id.clone(),
                attempt: attempt.id.clone(),
            },
            context: Box::new(CommandContext {
                execution: Some(ExecutionEvidence::Attempt { attempt }),
                ..Default::default()
            }),
        },
    )
    .unwrap();
    let review = crate::desktop_execution::prepare(
        &host.projects["project"],
        "task",
        "desktop-attempt",
        d.binding.observation.clone(),
        vec!["a".repeat(64), "b".repeat(64), "c".repeat(64)],
    )
    .unwrap();
    (host, review)
}
pub(crate) fn task_proof(
    r: &crate::desktop_execution::TaskReview,
) -> crate::desktop_execution::TaskProof {
    let observed_at = chrono::Utc::now().to_rfc3339();
    crate::desktop_execution::TaskProof {
        reader_sha256: None,
        marker: r.marker.clone(),
        session_id: r.observation.session_id.clone(),
        local_binding: (r.observation.window.app == DesktopApp::Workbuddy).then(|| {
            let mut observation = r.observation.clone();
            observation.captured_at = observed_at.clone();
            cerulean_domain::desktop_sessions::DesktopTaskLocalBindingProof {
                binding_id: r.binding_id.clone(),
                task_review_digest: r.id.clone(),
                observation,
            }
        }),
        prompt_sha256: r.prompt_sha256.clone(),
        observed_baseline: r.baseline[1..].to_vec(),
        reply_sha256: "d".repeat(64),
        observed_at,
        completed: true,
        round_sha256: Some(digest(&r.prompt).unwrap()),
    }
}

fn background_reply(request: &ReadRequest) -> Result<ReadReply, String> {
    match request {
        ReadRequest::TrialPrepare { window, .. }
        | ReadRequest::TaskPrepare {
            window,
            previous: None,
            ..
        } => Ok(ReadReply {
            observation: Some(observation(window.clone())),
            baseline: vec!["a".repeat(64), "b".repeat(64), "c".repeat(64)],
            ..Default::default()
        }),
        ReadRequest::TaskSend { review, .. } => {
            let project = Path::new(&review.attempt.binding.workspace);
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(project.join("send-once"))
                .map_err(|e| e.to_string())?;
            let start = Instant::now();
            while !project.join("release-send").exists() {
                if start.elapsed() > Duration::from_secs(60) {
                    return Err("test gate timed out".into());
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            if project.join("panic-send").exists() {
                panic!("injected desktop thread panic");
            }
            if project.join("lost-reply").exists() {
                return Err("injected lost UI reply".into());
            }
            fs::write(
                project.join("sum.mjs"),
                "export function sum(a,b){return a+b;}\n",
            )
            .unwrap();
            Ok(ReadReply {
                task_proof: Some(task_proof(review)),
                ..Default::default()
            })
        }
        ReadRequest::TaskRead { review, .. } => {
            let project = Path::new(&review.attempt.binding.workspace);
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(project.join("read-once"))
                .map_err(|e| e.to_string())?;
            Ok(ReadReply {
                task_proof: Some(task_proof(review)),
                ..Default::default()
            })
        }
        _ => Err("unexpected desktop operation".into()),
    }
}
fn background_spin(host: &mut RuntimeHost) {
    let start = Instant::now();
    while host.desktop_active.is_some() {
        host.poll_verifications();
        assert!(
            start.elapsed() < Duration::from_secs(15),
            "{:?}",
            host.desktop_access.error
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}
#[test]
fn desktop_background_ipc_keeps_snapshot_responsive_and_exclusive_until_submission() {
    let root = tempfile::tempdir().unwrap();
    let (mut host, _) = assigned_desktop_task(root.path());
    host.desktop_fixture = Some(background_reply);
    let request = |command| Request {
        id: "desktop-background".into(),
        protocol_version: PROTOCOL_VERSION,
        command,
    };
    assert!(host
        .handle(request(Command::PrepareDesktopTask {
            project_id: "project".into(),
            task_id: "task".into(),
            attempt_id: "desktop-attempt".into()
        }))
        .error
        .is_none());
    spin(&mut host);
    let review = host.desktop_access.task_review.clone().unwrap();
    assert!(!root.path().join("project/send-once").exists());
    assert!(host.send_desktop_task(&"f".repeat(64)).is_err());
    assert!(host
        .handle(request(Command::SendDesktopTask {
            review_digest: review.id.clone()
        }))
        .error
        .is_none());
    assert!(!host.projects.contains_key("project"));
    let snapshot = host.handle(request(Command::Snapshot)).result.unwrap();
    assert_eq!(snapshot.workflows.len(), 1);
    assert_eq!(
        snapshot.workflows[0].state.tasks["task"].state,
        TaskState::Dispatching
    );
    assert!(snapshot.desktop_access.unwrap().tasks[0].active);
    assert!(!host.can_shutdown());
    assert!(host.require_available("project").is_err());
    let mut overlapping = snapshot.workflows[0].project.clone();
    overlapping.id = "overlap".into();
    assert!(host.register(overlapping).is_err());
    assert!(host.send_desktop_task(&review.id).is_err());
    assert!(host
        .read_desktop_task("project", "desktop-attempt")
        .is_err());
    assert!(host.discover_desktop().is_err());
    assert!(host.prepare_desktop_trial(&review.binding_id).is_err());
    fs::write(root.path().join("project/release-send"), "release").unwrap();
    background_spin(&mut host);
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Submitted
    );
    assert!(!host.projects["project"].state().tasks["task"].verified);
    assert!(host.desktop_access.tasks[0].submitted);
    assert!(host.desktop_access.tasks[0].proof.is_some());
    assert!(!host.desktop_access.tasks[0].active);
    assert!(host
        .read_desktop_task("project", "desktop-attempt")
        .is_err());
    let original = fs::read(sealed_directory(root.path()).join("intent.json")).unwrap();
    drop(host);
    let host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert!(host.desktop_access.tasks[0].submitted);
    assert!(host.desktop_access.task_review.is_none());
    assert_eq!(
        fs::read(sealed_directory(root.path()).join("intent.json")).unwrap(),
        original
    );
}
#[test]
fn desktop_background_loss_and_panic_recover_by_read_only_without_second_send() {
    for fault in ["lost-reply", "panic-send"] {
        let root = tempfile::tempdir().unwrap();
        let (mut host, review) = assigned_desktop_task(root.path());
        host.desktop_fixture = Some(background_reply);
        host.desktop_access.task_review = Some(review.clone());
        write_sum(root.path(), true);
        fs::write(root.path().join("project").join(fault), "fault").unwrap();
        fs::write(root.path().join("project/release-send"), "release").unwrap();
        host.send_desktop_task(&review.id).unwrap();
        background_spin(&mut host);
        assert_eq!(
            host.projects["project"].state().tasks["task"].state,
            TaskState::Unknown
        );
        assert!(host.desktop_access.tasks[0].error.is_some());
        let original = fs::read(sealed_directory(root.path()).join("intent.json")).unwrap();
        drop(host);
        let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
        host.desktop_fixture = Some(background_reply);
        host.desktop_access.task_review = Some(review.clone());
        assert!(host.send_desktop_task(&review.id).is_err());
        host.read_desktop_task("project", "desktop-attempt")
            .unwrap();
        background_spin(&mut host);
        assert_eq!(
            host.projects["project"].state().tasks["task"].state,
            TaskState::Submitted
        );
        assert!(host.desktop_access.tasks[0].error.is_none());
        assert_eq!(
            fs::read(sealed_directory(root.path()).join("intent.json")).unwrap(),
            original
        );
        assert!(root.path().join("project/read-once").exists());
    }
}
#[test]
fn desktop_background_reserved_before_launch_never_reconstructs_send_permission() {
    let root = tempfile::tempdir().unwrap();
    let (mut host, review) = assigned_desktop_task(root.path());
    let _permit = crate::desktop_execution::reserve(
        &host.root.clone(),
        host.projects.get_mut("project").unwrap(),
        &review,
    )
    .unwrap();
    drop(host);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Unknown
    );
    assert_eq!(host.desktop_access.tasks.len(), 1);
    host.desktop_access.task_review = Some(review.clone());
    assert!(host.send_desktop_task(&review.id).is_err());
    assert!(!root.path().join("project/send-once").exists());
}

#[test]
#[ignore = "native task UI/IPC with synthetic management and desktop replies, no external messages"]
fn native_desktop_task_server() {
    let root = fs::canonicalize(std::env::var_os("CERULEAN_DESKTOP_NATIVE_DIR").unwrap()).unwrap();
    let reports =
        fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports")).unwrap();
    assert!(root.starts_with(reports) && !root.join("data").exists());
    let (mut host, _) = assigned_desktop_task(&root);
    host.desktop_fixture = Some(background_reply);
    let data = root.join("data");
    fs::write(
        root.join("fixture.json"),
        serde_json::to_vec(&serde_json::json!({"data":data})).unwrap(),
    )
    .unwrap();
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(crate::ipc::serve_host(&data, host))
        .unwrap();
}
#[test]
fn desktop_dispatch_real_workflow_anchors_one_send_and_preserves_deputy_application() {
    use crate::desktop_execution as execution;
    let root = tempfile::tempdir().unwrap();
    let (mut host, review) = assigned_desktop_task(root.path());
    let mut calls = 0;
    let data = host.root.clone();
    let proof = execution::dispatch_observed(
        &data,
        host.projects.get_mut("project").unwrap(),
        &review,
        |r| {
            calls += 1;
            Ok(task_proof(r))
        },
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Dispatching
    );
    let mut replayed = false;
    assert!(execution::dispatch(
        &data,
        host.projects.get_mut("project").unwrap(),
        &review,
        |r| {
            replayed = true;
            Ok(task_proof(r))
        }
    )
    .is_err());
    assert!(!replayed);
    assert!(host.management.application("desktop-assign").unwrap().1);
    assert_eq!(
        execution::inspect(
            &data,
            &host.projects["project"],
            "project",
            "desktop-attempt"
        )
        .unwrap(),
        (review.clone(), Some(proof.clone()))
    );
    let store = host.projects.get_mut("project").unwrap();
    let receipt = ExecutionReceipt {
        schema_version: 1,
        task_id: "task".into(),
        attempt_id: review.attempt.id.clone(),
        fence: 1,
        binding: review.attempt.binding.clone(),
        turn_id: None,
        observed_at: proof.observed_at.clone(),
        status: ReceiptStatus::Running,
        evidence_sha256: digest(&proof).unwrap(),
    };
    let context = CommandContext {
        thread_id: Some(review.attempt.binding.session_id.clone()),
        execution: Some(ExecutionEvidence::Receipt {
            receipt: receipt.clone(),
            manifest: None,
        }),
        ..Default::default()
    };
    let mut invalid = context.clone();
    invalid.turn_id = Some("invented-turn".into());
    assert!(store
        .command(
            "bad-start",
            "core",
            WorkflowCommand::Started {
                task: "task".into(),
                attempt: review.attempt.id.clone()
            },
            invalid,
            false
        )
        .is_err());
    store
        .command(
            "desktop-start",
            "core",
            WorkflowCommand::Started {
                task: "task".into(),
                attempt: review.attempt.id.clone(),
            },
            context,
            false,
        )
        .unwrap();
    assert_eq!(store.state().tasks["task"].state, TaskState::Running);
    assert!(!store.state().tasks["task"].verified);
    drop(host);
    let mut host = RuntimeHost::open(&data).unwrap();
    let store = host.projects.get_mut("project").unwrap();
    assert_eq!(store.state().tasks["task"].state, TaskState::Unknown);
    store
        .command(
            "desktop-reconcile",
            "core",
            WorkflowCommand::Reconcile {
                task: "task".into(),
                attempt: review.attempt.id.clone(),
                outcome: ReconcileOutcome::Running,
                evidence: vec![digest(&proof).unwrap()],
            },
            CommandContext {
                thread_id: Some(review.attempt.binding.session_id.clone()),
                execution: Some(ExecutionEvidence::Receipt {
                    receipt,
                    manifest: None,
                }),
                ..Default::default()
            },
            false,
        )
        .unwrap();
    assert_eq!(store.state().tasks["task"].state, TaskState::Running);
    assert!(!store.state().tasks["task"].verified);
    assert!(!store.history().iter().any(|e| matches!(
        e.body.event.command,
        WorkflowCommand::Submit { .. }
            | WorkflowCommand::Verify { .. }
            | WorkflowCommand::Accept { .. }
    )));
}
#[test]
fn desktop_dispatch_interruption_boundaries_never_retry_and_read_recovery_keeps_intent() {
    use crate::desktop_execution as execution;
    for stop in [
        "intent-published",
        "workflow-dispatched",
        "reply-observed",
        "proof-published",
    ] {
        let root = tempfile::tempdir().unwrap();
        let (mut host, review) = assigned_desktop_task(root.path());
        let data = host.root.clone();
        let mut sends = 0;
        assert!(execution::dispatch_observed(
            &data,
            host.projects.get_mut("project").unwrap(),
            &review,
            |r| {
                sends += 1;
                Ok(task_proof(r))
            },
            |stage| if stage == stop {
                Err("simulated interruption".into())
            } else {
                Ok(())
            }
        )
        .is_err());
        assert_eq!(
            sends,
            if stop == "reply-observed" || stop == "proof-published" {
                1
            } else {
                0
            }
        );
        drop(host);
        let mut host = RuntimeHost::open(&data).unwrap();
        let dir = data
            .join("desktop-task-dispatch-v1")
            .join(digest(&("project", "desktop-attempt")).unwrap());
        let original = fs::read(dir.join("intent.json")).unwrap();
        let mut replayed = false;
        assert!(execution::dispatch(
            &data,
            host.projects.get_mut("project").unwrap(),
            &review,
            |r| {
                replayed = true;
                Ok(task_proof(r))
            }
        )
        .is_err());
        assert!(!replayed);
        if stop != "proof-published" {
            assert!(host
                .prepare_desktop_trial(&review.binding_id)
                .unwrap_err()
                .contains("存在未核实桌面任务发送"));
        }
        let mut reads = 0;
        let recovered = execution::recover(
            &data,
            &host.projects["project"],
            "project",
            "desktop-attempt",
            |r| {
                reads += 1;
                Ok(task_proof(r))
            },
        );
        if stop == "intent-published" {
            assert!(recovered.is_err());
            assert_eq!(reads, 0);
        } else {
            assert!(recovered.is_ok());
            assert_eq!(reads, if stop == "proof-published" { 0 } else { 1 });
        }
        assert_eq!(fs::read(dir.join("intent.json")).unwrap(), original);
    }
}
#[test]
fn desktop_dispatch_rejects_changed_review_bad_reply_and_partial_publications() {
    use crate::desktop_execution as execution;
    let root = tempfile::tempdir().unwrap();
    let (mut host, review) = assigned_desktop_task(root.path());
    let data = host.root.clone();
    for case in 0..5 {
        let mut bad = review.clone();
        match case {
            0 => bad.prompt.push_str("extra"),
            1 => bad.attempt.fence += 1,
            2 => bad.observation.model_label = Some("other".into()),
            3 => bad.baseline.clear(),
            _ => bad.workflow_hash = "0".repeat(64),
        }
        assert!(execution::dispatch(
            &data,
            host.projects.get_mut("project").unwrap(),
            &bad,
            |_| panic!("invalid review reached transport")
        )
        .is_err());
    }
    assert!(!data.join("desktop-task-dispatch-v1").exists());
    assert!(execution::dispatch(
        &data,
        host.projects.get_mut("project").unwrap(),
        &review,
        |r| {
            let mut p = task_proof(r);
            p.session_id = Some("other-session".into());
            Ok(p)
        }
    )
    .is_err());
    for case in 0..5 {
        assert!(execution::recover(
            &data,
            &host.projects["project"],
            "project",
            "desktop-attempt",
            |r| {
                let mut p = task_proof(r);
                match case {
                    0 => p.marker = "old-reply".into(),
                    1 => p.prompt_sha256 = "0".repeat(64),
                    2 => p.observed_baseline = vec!["c".repeat(64)],
                    3 => p.completed = false,
                    _ => p.observed_baseline = vec!["a".repeat(64), "c".repeat(64)],
                }
                Ok(p)
            }
        )
        .is_err());
    }
    let dir = data
        .join("desktop-task-dispatch-v1")
        .join(digest(&("project", "desktop-attempt")).unwrap());
    let original = fs::read(dir.join("intent.json")).unwrap();
    fs::write(dir.join("intent.pending"), &original).unwrap();
    assert!(execution::inspect(
        &data,
        &host.projects["project"],
        "project",
        "desktop-attempt"
    )
    .is_ok());
    fs::write(dir.join("intent.pending"), b"{").unwrap();
    assert!(execution::inspect(
        &data,
        &host.projects["project"],
        "project",
        "desktop-attempt"
    )
    .is_err());
    assert_eq!(fs::read(dir.join("intent.json")).unwrap(), original);
    drop(host);
    assert!(RuntimeHost::open(&data).is_err());
}

#[test]
fn desktop_dispatch_embedded_task_bridge_rejects_unapproved_text_before_window_access() {
    let root = tempfile::tempdir().unwrap();
    let (_host, mut review) = assigned_desktop_task(root.path());
    review.prompt.push_str(" changed after review");
    for request in [
        ReadRequest::TaskPrepare {
            version: 1,
            window: review.observation.window.clone(),
            previous: Some(review.clone()),
        },
        ReadRequest::TaskSend {
            version: 1,
            review: review.clone(),
        },
        ReadRequest::TaskRead {
            version: 1,
            review: review.clone(),
        },
    ] {
        assert_eq!(
            observe(root.path(), &request).unwrap_err(),
            "发送审阅内容无效，未执行输入"
        );
    }
}

fn sealed_directory(root: &Path) -> PathBuf {
    root.join("data/desktop-task-dispatch-v1")
        .join(digest(&("project", "desktop-attempt")).unwrap())
}
pub(crate) fn write_sum(root: &Path, correct: bool) {
    fs::write(
        root.join("project/sum.mjs"),
        if correct {
            "export function sum(a,b){return a+b;}\n"
        } else {
            "export function sum(a,b){return 0;}\n"
        },
    )
    .unwrap();
}
fn chief_accept_desktop(host: &mut RuntimeHost) -> Result<(), String> {
    use crate::management_execution::{ApplicationTarget, DecisionIntent};
    use cerulean_adapters::openai::DecisionObservation;
    use cerulean_domain::management::{DecisionEnvelope, ManagementDecision, Purpose};
    let context =
        host.management_context("project", "Accept only after independent verification")?;
    let state_digest = digest(&context)?;
    let operation = format!("chief-desktop-{}", host.management.statuses().len());
    host.management.begin(DecisionIntent {
        operation_id: operation.clone(),
        project_id: "project".into(),
        purpose: Purpose::Accept,
        model: "fixture-chief".into(),
        state_digest: state_digest.clone(),
        context,
        created_at: chrono::Utc::now().to_rfc3339(),
    })?;
    host.management.finish(
        &operation,
        Ok(DecisionObservation {
            response_id: "resp_desktop_accept".into(),
            requested_model: "fixture-chief".into(),
            actual_model: "fixture-chief".into(),
            input_tokens: 1,
            output_tokens: 1,
            envelope: DecisionEnvelope {
                state_digest,
                decision: ManagementDecision::Accept {
                    task_id: "task".into(),
                    attempt_id: "desktop-attempt".into(),
                    rationale: "Review the independent verification evidence".into(),
                },
            },
        }),
    )?;
    host.apply_management_decision(
        &operation,
        ApplicationTarget::Workflow {
            principal: "chief".into(),
            command: WorkflowCommand::Accept {
                task: "task".into(),
                attempt: "desktop-attempt".into(),
            },
            context: Box::default(),
        },
    )
}
#[test]
fn desktop_completion_actual_node_verification_and_chief_acceptance() {
    use crate::desktop_execution as execution;
    let evidence = std::env::var_os("CERULEAN_DESKTOP_COMPLETION_EVIDENCE_DIR").map(|p| {
        let path = fs::canonicalize(p).unwrap();
        let reports =
            fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../reports")).unwrap();
        assert!(path.starts_with(reports));
        assert_eq!(
            fs::read_dir(&path).unwrap().count(),
            0,
            "evidence destination must be fresh"
        );
        path
    });
    for correct in [true, false] {
        let root = if let Some(parent) = &evidence {
            tempfile::Builder::new()
                .prefix(if correct { "passed-" } else { "failed-" })
                .tempdir_in(parent)
                .unwrap()
        } else {
            tempfile::tempdir().unwrap()
        };
        let (mut host, review) = assigned_desktop_task_with_validator(root.path(), true);
        let data = host.root.clone();
        execution::dispatch(
            &data,
            host.projects.get_mut("project").unwrap(),
            &review,
            |r| {
                write_sum(root.path(), correct);
                Ok(task_proof(r))
            },
        )
        .unwrap();
        assert_eq!(
            host.projects["project"].state().tasks["task"].state,
            TaskState::Submitted
        );
        assert!(!host.projects["project"].state().tasks["task"].verified);
        assert!(host
            .projects
            .get_mut("project")
            .unwrap()
            .command(
                "reject-early-accept",
                "chief",
                WorkflowCommand::Accept {
                    task: "task".into(),
                    attempt: "desktop-attempt".into()
                },
                CommandContext::default(),
                false
            )
            .is_err());
        let pins = host.projects["project"].revision().unwrap().execution[0]
            .checks
            .iter()
            .map(crate::verifier::PinnedCheck::from_approved)
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        host.start_verification(
            "project",
            "desktop-independent-check",
            "task",
            "desktop-attempt",
            pins,
        )
        .unwrap();
        let start = Instant::now();
        while !host.active.is_empty() {
            host.poll_verifications();
            assert!(start.elapsed() < Duration::from_secs(30));
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            host.projects["project"].state().tasks["task"].verified,
            correct
        );
        if correct {
            chief_accept_desktop(&mut host).unwrap();
            assert_eq!(
                host.projects["project"].state().tasks["task"].state,
                TaskState::Accepted
            );
        } else {
            assert!(chief_accept_desktop(&mut host).is_err());
        }
        let history = host.projects["project"].history().to_vec();
        drop(host);
        let mut host = RuntimeHost::open(&data).unwrap();
        assert_eq!(host.projects["project"].history(), history);
        let before = host.projects["project"].history().len();
        execution::submit_sealed(
            &data,
            host.projects.get_mut("project").unwrap(),
            "project",
            "desktop-attempt",
        )
        .unwrap();
        assert_eq!(host.projects["project"].history().len(), before);
        if evidence.is_some() {
            let snapshot = host.snapshot();
            fs::write(root.path().join("case-result.json"),serde_json::to_vec_pretty(&serde_json::json!({"status":"passed","correctArtifact":correct,"scope":"synthetic desktop and management replies; actual Node behavior verification, persistence and chief application","realModelCalls":0,"externalMessages":0,"snapshot":snapshot})).unwrap()).unwrap();
            drop(host);
            let kept = root.keep();
            println!("desktop-completion-evidence={}", kept.display());
        }
    }
}
#[test]
fn desktop_completion_interruption_never_recaptures_and_replays_only_original_seal() {
    use crate::desktop_execution as execution;
    for stop in [
        "proof-published",
        "seal-reserved",
        "artifacts-captured",
        "completion-published",
        "workflow-submitted",
    ] {
        let root = tempfile::tempdir().unwrap();
        let (mut host, review) = assigned_desktop_task(root.path());
        let data = host.root.clone();
        let mut sends = 0;
        assert!(execution::dispatch_and_seal_observed(
            &data,
            host.projects.get_mut("project").unwrap(),
            &review,
            |r| {
                sends += 1;
                write_sum(root.path(), true);
                Ok(task_proof(r))
            },
            |at| if at == stop {
                Err("simulated seal interruption".into())
            } else {
                Ok(())
            }
        )
        .is_err());
        assert_eq!(sends, 1);
        let dir = sealed_directory(root.path());
        let intent = fs::read(dir.join("intent.json")).unwrap();
        let proof = fs::read(dir.join("proof.json")).unwrap();
        drop(host);
        let mut host = RuntimeHost::open(&data).unwrap();
        let result = execution::recover_and_submit(
            &data,
            host.projects.get_mut("project").unwrap(),
            "project",
            "desktop-attempt",
            |_| panic!("stored proof must not reread UI"),
        );
        let sealed = stop == "completion-published" || stop == "workflow-submitted";
        assert_eq!(result.is_ok(), sealed, "stage {stop}: {result:?}");
        if sealed {
            assert_eq!(
                host.projects["project"].state().tasks["task"].state,
                TaskState::Submitted
            );
        } else {
            write_sum(root.path(), false);
            assert!(execution::recover_and_submit(
                &data,
                host.projects.get_mut("project").unwrap(),
                "project",
                "desktop-attempt",
                |_| panic!("no input or reread")
            )
            .is_err());
            assert!(!dir.join("completion.json").exists());
            assert!(host.prepare_desktop_trial(&review.binding_id).is_err());
        }
        assert_eq!(fs::read(dir.join("intent.json")).unwrap(), intent);
        assert_eq!(fs::read(dir.join("proof.json")).unwrap(), proof);
    }
}
#[test]
fn desktop_completion_recovery_requires_unchanged_files_and_intact_seal() {
    use crate::desktop_execution as execution;
    let root = tempfile::tempdir().unwrap();
    let (mut host, review) = assigned_desktop_task(root.path());
    let data = host.root.clone();
    assert!(execution::dispatch_and_seal_observed(
        &data,
        host.projects.get_mut("project").unwrap(),
        &review,
        |r| {
            write_sum(root.path(), true);
            Ok(task_proof(r))
        },
        |at| if at == "completion-published" {
            Err("stop before submit".into())
        } else {
            Ok(())
        }
    )
    .is_err());
    let dir = sealed_directory(root.path());
    let sealed = fs::read(dir.join("completion.json")).unwrap();
    write_sum(root.path(), false);
    assert!(execution::submit_sealed(
        &data,
        host.projects.get_mut("project").unwrap(),
        "project",
        "desktop-attempt"
    )
    .is_err());
    assert_eq!(fs::read(dir.join("completion.json")).unwrap(), sealed);
    drop(host);
    let mut host = RuntimeHost::open(&data).unwrap();
    write_sum(root.path(), true);
    execution::submit_sealed(
        &data,
        host.projects.get_mut("project").unwrap(),
        "project",
        "desktop-attempt",
    )
    .unwrap();
    fs::write(dir.join("completion.pending"), &sealed).unwrap();
    drop(host);
    let host = RuntimeHost::open(&data).unwrap();
    drop(host);
    fs::write(dir.join("completion.pending"), b"{").unwrap();
    assert!(RuntimeHost::open(&data).is_err());
    fs::remove_file(dir.join("completion.pending")).unwrap();
    fs::remove_file(dir.join("completion.json")).unwrap();
    assert!(RuntimeHost::open(&data).is_err());
}
#[test]
fn desktop_completion_missing_delivery_and_cancelled_attempt_cannot_submit() {
    use crate::desktop_execution as execution;
    let root = tempfile::tempdir().unwrap();
    let (mut host, review) = assigned_desktop_task(root.path());
    let data = host.root.clone();
    assert!(execution::dispatch(
        &data,
        host.projects.get_mut("project").unwrap(),
        &review,
        |r| Ok(task_proof(r))
    )
    .is_err());
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Unknown
    );
    write_sum(root.path(), true);
    assert!(execution::recover_and_submit(
        &data,
        host.projects.get_mut("project").unwrap(),
        "project",
        "desktop-attempt",
        |_| panic!("never resend")
    )
    .is_err());
    assert!(!sealed_directory(root.path())
        .join("completion.json")
        .exists());
    let root = tempfile::tempdir().unwrap();
    let (mut host, review) = assigned_desktop_task(root.path());
    let data = host.root.clone();
    assert!(execution::dispatch_and_seal_observed(
        &data,
        host.projects.get_mut("project").unwrap(),
        &review,
        |r| {
            write_sum(root.path(), true);
            Ok(task_proof(r))
        },
        |at| if at == "completion-published" {
            Err("before submit".into())
        } else {
            Ok(())
        }
    )
    .is_err());
    host.projects
        .get_mut("project")
        .unwrap()
        .command(
            "desktop-user-cancel",
            "core",
            WorkflowCommand::CancelIntent {
                task: "task".into(),
                attempt: "desktop-attempt".into(),
                reason: "user cancelled".into(),
            },
            CommandContext {
                thread_id: Some(review.attempt.binding.session_id.clone()),
                cancellation_source: Some("user".into()),
                ..Default::default()
            },
            false,
        )
        .unwrap();
    assert!(execution::submit_sealed(
        &data,
        host.projects.get_mut("project").unwrap(),
        "project",
        "desktop-attempt"
    )
    .is_err());
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Cancelling
    );
}

#[test]
fn desktop_completion_first_read_after_uncertain_send_can_seal_once() {
    use crate::desktop_execution as execution;
    let root = tempfile::tempdir().unwrap();
    let (mut host, review) = assigned_desktop_task(root.path());
    let data = host.root.clone();
    assert!(execution::dispatch(
        &data,
        host.projects.get_mut("project").unwrap(),
        &review,
        |_| Err("lost UI reply".into())
    )
    .is_err());
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Unknown
    );
    let mut reads = 0;
    execution::recover_and_submit(
        &data,
        host.projects.get_mut("project").unwrap(),
        "project",
        "desktop-attempt",
        |r| {
            reads += 1;
            write_sum(root.path(), true);
            Ok(task_proof(r))
        },
    )
    .unwrap();
    assert_eq!(reads, 1);
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Submitted
    );
    let seal = fs::read(sealed_directory(root.path()).join("completion.json")).unwrap();
    write_sum(root.path(), false);
    execution::recover_and_submit(
        &data,
        host.projects.get_mut("project").unwrap(),
        "project",
        "desktop-attempt",
        |_| panic!("already submitted; no UI read"),
    )
    .unwrap();
    assert_eq!(
        fs::read(sealed_directory(root.path()).join("completion.json")).unwrap(),
        seal
    );
    assert!(!host.projects["project"].state().tasks["task"].verified);
}

#[path = "desktop_cancellation_tests.rs"]
mod cancellation_tests;
#[path = "desktop_stop_native_tests.rs"]
mod stop_native_tests;
#[path = "desktop_stop_publication_tests.rs"]
mod stop_publication_tests;
#[path = "desktop_stop_review_tests.rs"]
mod stop_review_tests;

#[test]
fn desktop_task_current_reader_recovery_preserves_intent_and_host_provenance() {
    let root = tempfile::tempdir().unwrap();
    let (mut host, review) = assigned_desktop_task(root.path());
    write_sum(root.path(), true);
    let _permit = crate::desktop_execution::reserve(
        &host.root.clone(),
        host.projects.get_mut("project").unwrap(),
        &review,
    )
    .unwrap();
    let intent = fs::read(sealed_directory(root.path()).join("intent.json")).unwrap();
    drop(host);
    let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
    host.desktop_fixture = Some(|request| match request {
        ReadRequest::TaskRead { review, .. } => {
            let mut proof = task_proof(review);
            proof.reader_sha256 = Some("adapter-must-not-choose-reader".into());
            Ok(ReadReply {
                task_proof: Some(proof),
                ..Default::default()
            })
        }
        _ => panic!("Recovery must only read the original task"),
    });
    let history = host.projects["project"].history().to_vec();
    assert!(host
        .read_desktop_task_with_reader("project", "desktop-attempt", Some(&"0".repeat(64)))
        .is_err());
    assert_eq!(host.projects["project"].history(), history);
    assert!(host.desktop_active.is_none());
    host.read_desktop_task_with_reader("project", "desktop-attempt", Some(&fingerprint()))
        .unwrap();
    background_spin(&mut host);
    assert!(host.desktop_access.tasks[0].submitted);
    assert_eq!(
        host.desktop_access.tasks[0]
            .proof
            .as_ref()
            .unwrap()
            .reader_sha256
            .as_deref(),
        Some(fingerprint().as_str())
    );
    assert_eq!(
        fs::read(sealed_directory(root.path()).join("intent.json")).unwrap(),
        intent
    );
    assert!(host.send_desktop_task(&review.id).is_err());
    assert!(host
        .read_desktop_task_with_reader("project", "desktop-attempt", Some(&fingerprint()))
        .is_err());
    drop(host);
    let host = RuntimeHost::open(&root.path().join("data")).unwrap();
    assert!(host.desktop_access.tasks[0].submitted);
    assert_eq!(
        host.desktop_access.tasks[0]
            .proof
            .as_ref()
            .unwrap()
            .reader_sha256
            .as_deref(),
        Some(fingerprint().as_str())
    );
    assert!(!root.path().join("project/send-once").exists());
}

#[test]
fn desktop_task_legacy_reader_json_is_byte_compatible() {
    let root = tempfile::tempdir().unwrap();
    let (_, review) = assigned_desktop_task(root.path());
    let proof = task_proof(&review);
    let bytes = serde_json::to_vec(&proof).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("readerSha256"));
    let decoded: crate::desktop_execution::TaskProof = serde_json::from_slice(&bytes).unwrap();
    assert!(decoded.reader_sha256.is_none());
    assert_eq!(serde_json::to_vec(&decoded).unwrap(), bytes);
    let cmd: Command =
        serde_json::from_str(r#"{"type":"readDesktopTask","projectId":"p","attemptId":"a"}"#)
            .unwrap();
    assert!(matches!(
        cmd,
        Command::ReadDesktopTask {
            reader_sha256: None,
            ..
        }
    ));
}
