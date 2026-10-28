#![cfg(windows)]
use cerulean_core::{host::RuntimeHost, verifier::PinnedCheck};
use cerulean_domain::{execution::*, *};
use std::{collections::BTreeMap, fs};

fn request(command: Command) -> Request {
    Request {
        id: "catalog-request".into(),
        protocol_version: PROTOCOL_VERSION,
        command,
    }
}
fn registration(id: &str, script: &std::path::Path) -> Command {
    Command::RegisterValidator {
        id: id.into(),
        name: "validation v1".into(),
        executable: std::env::current_exe().unwrap().to_string_lossy().into(),
        inputs: vec![script.to_string_lossy().into()],
        environment: [("CI".into(), "true".into())].into(),
    }
}
#[test]
fn registration_is_immutable_restartable_and_drives_approved_binding() {
    let data = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    let script = external.path().join("check.txt");
    fs::write(&script, "version-one").unwrap();
    let mut host = RuntimeHost::open(data.path()).unwrap();
    let snapshot = host
        .handle(request(registration("tool-1", &script)))
        .result
        .unwrap();
    assert_eq!(snapshot.validators.len(), 1);
    assert!(
        snapshot.plans.is_empty()
            && snapshot.workflows.is_empty()
            && snapshot.verification_jobs.is_empty()
    );
    let saved = fs::read(data.path().join("validators/tool-1.json")).unwrap();
    host.handle(request(registration("tool-1", &script)))
        .result
        .unwrap();
    assert_eq!(
        saved,
        fs::read(data.path().join("validators/tool-1.json")).unwrap()
    );
    let check = AcceptanceCheck {
        name: None,
        id: "check".into(),
        program: "test".into(),
        args: vec![],
        timeout_seconds: 1,
        tool: Some(host.registered_validator("tool-1").unwrap().tool.clone()),
    };
    PinnedCheck::from_approved(&check).unwrap();
    fs::write(&script, "version-two").unwrap();
    assert!(host
        .handle(request(registration("tool-1", &script)))
        .error
        .is_some());
    assert!(PinnedCheck::from_approved(&check).is_err());
    host.handle(request(registration("tool-2", &script)))
        .result
        .unwrap();
    assert_ne!(
        host.registered_validator("tool-1").unwrap().tool,
        host.registered_validator("tool-2").unwrap().tool
    );
    drop(host);
    let host = RuntimeHost::open(data.path()).unwrap();
    assert_eq!(host.snapshot().validators.len(), 2);
    assert_eq!(
        saved,
        fs::read(data.path().join("validators/tool-1.json")).unwrap()
    );
    assert!(host.registered_validator("missing").is_err());
}
#[test]
fn unsafe_registration_and_damaged_catalog_are_rejected() {
    let data = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    let script = project.path().join("check.txt");
    fs::write(&script, "not executable").unwrap();
    let mut host = RuntimeHost::open(data.path()).unwrap();
    host.handle(request(Command::RegisterProject {
        project: Project {
            id: "project".into(),
            root: project.path().to_string_lossy().into(),
            timezone: "Asia/Shanghai".into(),
        },
    }))
    .result
    .unwrap();
    assert!(host
        .handle(request(registration("tool-1", &script)))
        .error
        .is_some());
    for (exe, env) in [
        (script.to_string_lossy().into_owned(), BTreeMap::new()),
        (
            std::env::current_exe()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
            [("PATH".into(), "x".into())].into(),
        ),
    ] {
        assert!(host
            .handle(request(Command::RegisterValidator {
                id: "bad".into(),
                name: "bad".into(),
                executable: exe,
                inputs: vec![],
                environment: env
            }))
            .error
            .is_some());
    }
    assert!(host.snapshot().validators.is_empty());
    let external = tempfile::tempdir().unwrap();
    let script = external.path().join("trusted.txt");
    fs::write(&script, "ok").unwrap();
    host.handle(request(registration("good", &script)))
        .result
        .unwrap();
    drop(host);
    let path = data.path().join("validators/good.json");
    let mut record: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    record["registration"]["name"] = "tampered".into();
    fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
    assert!(RuntimeHost::open(data.path()).is_err());
}
