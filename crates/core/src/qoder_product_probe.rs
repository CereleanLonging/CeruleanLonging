//! Qoder local rehearsal; native entry points require separate frozen approval.
use super::*;

#[path = "qoder_product_live.rs"]
mod live;

fn qoder_window() -> DesktopWindow {
    let mut w = window(DesktopApp::Qoder);
    w.version = "0.4.3.0".into();
    w
}

fn transport(request: &ReadRequest) -> Result<ReadReply, String> {
    match request {
        ReadRequest::Discover { .. } => Ok(ReadReply {
            windows: vec![qoder_window()],
            ..Default::default()
        }),
        ReadRequest::Inspect { window, .. }
        | ReadRequest::TaskPrepare {
            window,
            previous: None,
            ..
        } => {
            let mut o = observation(window.clone());
            o.model_label = Some(ProductProfile::QoderQwen.model().into());
            Ok(ReadReply {
                observation: Some(o),
                baseline: vec!["a".repeat(64), "b".repeat(64)],
                ..Default::default()
            })
        }
        // This branch only writes the controlled artifact and returns the
        // shared synthetic proof; it cannot call the native desktop bridge.
        ReadRequest::TaskSend { .. } => rehearsal_transport(request),
        _ => Err("Qoder rehearsal forbids other desktop operations".into()),
    }
}

#[test]
fn qoder_profile_rejects_missing_or_changed_session_model_process_and_history() {
    let mut o = observation(window(DesktopApp::Qoder));
    o.model_label = Some(ProductProfile::QoderQwen.model().into());
    let history = vec!["a".repeat(64), "b".repeat(64)];
    let check = |candidate: &DesktopObservation, rows: &[String]| {
        check_profile_view(ProductProfile::QoderQwen, &o, &history, candidate, rows)
    };
    check(&o, &history).unwrap();
    for field in 0..6 {
        let mut other = o.clone();
        match field {
            0 => other.session_id = None,
            1 => other.session_id = Some("87654321-4321-4321-4321-cba987654321".into()),
            2 => other.model_label = Some("GLM-5.3-Flash, 最大".into()),
            3 => other.window.pid += 1,
            4 => other.draft_empty = false,
            _ => other.busy = true,
        }
        assert!(check(&other, &history).is_err());
    }
    assert!(check(&o, &history[..1]).is_err());
    assert!(check_profile_view(ProductProfile::QoderQwen, &o, &[], &o, &[]).is_err());
    let mut malformed = o.clone();
    malformed.session_id = Some("not-a-session".into());
    assert!(check_profile_view(
        ProductProfile::QoderQwen,
        &malformed,
        &history,
        &malformed,
        &history
    )
    .is_err());
    assert!(check_profile_view(ProductProfile::Workbuddy, &o, &history, &o, &history).is_err());
}

#[test]
#[ignore = "controlled Qoder RuntimeHost and local Node; zero live desktop/provider calls"]
fn qoder_product_rehearsal_verifies_accept_and_reopens_without_resend() {
    let root = tempfile::Builder::new()
        .prefix("qoder-product-rehearsal-228-")
        .tempdir_in(repo().join("reports"))
        .unwrap()
        .keep()
        .canonicalize()
        .unwrap();
    println!("Controlled rehearsal evidence: {}", root.display());
    let (mut host, draft) = project_planning::tests::fixture(&root);
    host.desktop_fixture = Some(transport);
    host.discover_desktop().unwrap();
    spin(&mut host);
    host.inspect_desktop(qoder_window()).unwrap();
    spin(&mut host);
    let expected = host.desktop_access.observation.clone().unwrap();
    let observed = host.desktop_access.observation_digest.clone().unwrap();
    host.bind_desktop("project", &observed, "Controlled Qoder Qwen 228")
        .unwrap();
    spin(&mut host);
    configure_profile(ProductProfile::QoderQwen, &mut host, draft);
    let review = json!({
        "revisionDigest":host.projects["project"].revision().unwrap().approval_digest().unwrap(),
        "setupDigest":host.project_setups["project"].digest
    });
    let outcome = execute_profile(
        ProductProfile::QoderQwen,
        &mut host,
        &root,
        &review,
        &expected,
        &["a".repeat(64), "b".repeat(64)],
    );
    save(&root.join("after-rehearsal.json"), &host.snapshot());
    save(
        &root.join("execution-result.json"),
        &json!({"success":outcome.is_ok(),"error":outcome.as_ref().err(),"realMessages":0,"realProviderCalls":0}),
    );
    outcome.unwrap();
    assert!(!crate::credential_vault::exists_for(
        &host.root,
        cerulean_domain::credentials::CredentialProvider::OpenAi
    )
    .unwrap());
    assert!(host.desktop_access.tasks[0]
        .proof
        .as_ref()
        .unwrap()
        .local_binding
        .is_none());
    let history = host.projects["project"].history().to_vec();
    let decisions = host.management.statuses().len();
    let claim = fs::read(root.join("controlled-send-once.json")).unwrap();
    drop(host);
    for i in 0..2 {
        let mut reopened = RuntimeHost::open(&root.join("data")).unwrap();
        reopened.desktop_fixture = Some(transport);
        reopened.management_fixture = Some(manager);
        for _ in 0..5 {
            reopened.poll_verifications();
            reopened.poll_project_runs();
        }
        assert_eq!(reopened.projects["project"].history(), history);
        assert_eq!(reopened.management.statuses().len(), decisions);
        assert_eq!(reopened.desktop_access.tasks.len(), 1);
        assert!(reopened.desktop_active.is_none() && reopened.desktop_probe.is_none());
        assert_eq!(
            fs::read(root.join("controlled-send-once.json")).unwrap(),
            claim
        );
        save(&root.join(format!("reopen-{i}.json")), &reopened.snapshot());
    }
    save(
        &root.join("result.json"),
        &json!({
            "status":"passed","controlled":true,"realMessages":0,"realProviderCalls":0,
            "checks":15,"reopensWithoutResend":2,"releaseAccepted":false
        }),
    );
}
