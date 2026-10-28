use super::*;
use cerulean_domain::secret::SecretInput;
#[test]
fn independent_slots_restart_and_encrypted_identity_binding() {
    let root = tempfile::tempdir().unwrap();
    drop(RuntimeHost::open(root.path()).unwrap());
    let backup_parent = tempfile::tempdir().unwrap();
    crate::relay_guard::tests::promote_fixture(root.path(), &backup_parent.path().join("cold"));
    let mut host = RuntimeHost::open(root.path()).unwrap();
    let keys: Vec<_> = CredentialProvider::WORKERS
        .into_iter()
        .map(|p| (p, format!("sk-synthetic-{}-only", p.id())))
        .collect();
    for (p, k) in &keys {
        host.set_provider_key(*p, Some(SecretInput::new(k.clone()).unwrap()))
            .unwrap();
    }
    host.set_openai_key(Some(
        SecretInput::new("sk-synthetic-openai-preserve".into()).unwrap(),
    ))
    .unwrap();
    let before = host.snapshot();
    assert_eq!(
        before.provider_credentials.len(),
        CredentialProvider::WORKERS.len() + 1
    );
    assert!(before.provider_credentials.iter().all(|s| s.key_configured));
    assert!(before
        .provider_connections
        .iter()
        .all(|s| s.state == "unchecked" && s.models.is_empty()));
    drop(host);
    let mut host = RuntimeHost::open(root.path()).unwrap();
    for (p, k) in &keys {
        let loaded = crate::credential_vault::load_for(root.path(), *p).unwrap();
        assert_eq!(loaded.expose(), k);
        let raw = fs::read(root.path().join(format!("credentials/{}.dpapi", p.id()))).unwrap();
        assert!(!raw.windows(k.len()).any(|w| w == k.as_bytes()));
        assert!(!serde_json::to_string(&host.snapshot()).unwrap().contains(k));
    }
    let qwen_path = root.path().join("credentials/qwen.dpapi");
    let original = fs::read(&qwen_path).unwrap();
    fs::copy(root.path().join("credentials/kimi.dpapi"), &qwen_path).unwrap();
    assert!(crate::credential_vault::load_for(root.path(), CredentialProvider::Qwen).is_err());
    fs::write(&qwen_path, original).unwrap();
    host.set_provider_key(CredentialProvider::Kimi, None)
        .unwrap();
    assert!(
        !host
            .snapshot()
            .provider_connections
            .iter()
            .find(|s| s.provider == CredentialProvider::Kimi)
            .unwrap()
            .key_configured
    );
    assert!(host
        .snapshot()
        .provider_credentials
        .iter()
        .filter(|s| s.provider != CredentialProvider::Kimi)
        .all(|s| s.key_configured));
    assert!(
        host.snapshot().events.is_empty()
            && host.snapshot().management_decisions.is_empty()
            && host.snapshot().workflows.is_empty()
    );
}
#[test]
fn asynchronous_probe_guards_changes_and_clears_stale_models() {
    let root = tempfile::tempdir().unwrap();
    let mut host = RuntimeHost::open(root.path()).unwrap();
    let p = CredentialProvider::Grok;
    let target = ProviderEndpoint::Grok {};
    assert!(host.check_provider_models(target.clone()).is_err());
    host.set_provider_key(
        p,
        Some(SecretInput::new("sk-synthetic-probe-only".into()).unwrap()),
    )
    .unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    host.provider_probes.insert(p, rx);
    host.provider_connections.get_mut(&p).unwrap().state = "checking".into();
    assert!(host.check_provider_models(target).is_err());
    assert!(host.set_provider_key(p, None).is_err());
    assert!(!host.can_shutdown());
    tx.send(Ok(cerulean_adapters::provider_models::ModelList {
        models: vec!["fixture".into()],
        limited: true,
    }))
    .unwrap();
    host.poll_verifications();
    let status = &host.provider_connections[&p];
    assert_eq!(status.state, "listed");
    assert!(status.list_limited);
    assert_eq!(status.models, ["fixture"]);
    assert!(host.can_shutdown());
    let (tx, rx) = std::sync::mpsc::channel();
    host.provider_probes.insert(p, rx);
    drop(tx);
    host.poll_verifications();
    assert_eq!(host.provider_connections[&p].state, "failed");
    assert!(host.provider_connections[&p].models.is_empty());
    host.set_provider_key(p, None).unwrap();
    assert!(host.provider_connections[&p].checked_at.is_none());
    assert!(host
        .check_provider_models(ProviderEndpoint::QwenBeijing {
            workspace_id: "x.evil".into()
        })
        .is_err());
    assert!(host.provider_probes.is_empty());
}
#[test]
fn actual_probe_thread_rejects_corrupt_credential_without_network() {
    let root = tempfile::tempdir().unwrap();
    let mut host = RuntimeHost::open(root.path()).unwrap();
    for target in [
        ProviderEndpoint::DeepSeek {},
        ProviderEndpoint::Anthropic {},
        ProviderEndpoint::QwenSingapore {},
        ProviderEndpoint::KimiChina {},
        ProviderEndpoint::Grok {},
    ] {
        let p = target.provider();
        host.set_provider_key(
            p,
            Some(SecretInput::new("sk-synthetic-never-send".into()).unwrap()),
        )
        .unwrap();
        fs::write(
            root.path().join(format!("credentials/{}.dpapi", p.id())),
            b"invalid encrypted fixture",
        )
        .unwrap();
        host.check_provider_models(target).unwrap();
        assert_eq!(host.provider_connections[&p].state, "checking");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !host.provider_probes.is_empty() {
            host.poll_verifications();
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(host.provider_connections[&p].state, "failed");
        assert!(host.provider_connections[&p].models.is_empty());
    }
    assert!(host.snapshot().events.is_empty());
}
