//! IPC host attached only to a caller-owned WPF window; desktop transport is real.
use super::*;

#[test]
#[ignore = "Tauri/IPC plus owned WPF send/stop; synthetic management only; fresh owned report required"]
fn native_desktop_joint_ui_server() {
    assert_eq!(
        std::env::var("CERULEAN_NATIVE_OWNED_JOB").as_deref(),
        Ok("1")
    );
    let root =
        fs::canonicalize(std::env::var_os("CERULEAN_DESKTOP_JOINT_UI_DIR").unwrap()).unwrap();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert!(root.starts_with(fs::canonicalize(repo.join("reports")).unwrap()));
    assert!(!root.join("data").exists());
    let root = normal_path(&root).unwrap();
    let mut value: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("window.json")).unwrap()).unwrap();
    value.as_object_mut().unwrap().remove("foregroundGranted");
    let identity: DesktopWindow = serde_json::from_value(value).unwrap();
    assert_eq!(
        fs::canonicalize(&identity.exe).unwrap(),
        fs::canonicalize(root.join("Qoder.exe")).unwrap()
    );
    assert_eq!(
        identity.pid.to_string(),
        std::env::var("CERULEAN_OWNED_WINDOW_PID").unwrap()
    );
    let (mut host, mut draft) = project_planning::tests::fixture(&root);
    assert!(host.desktop_fixture.is_none());
    host.discover_desktop().unwrap();
    spin(&mut host);
    assert!(host.desktop_access.windows.contains(&identity));
    host.inspect_desktop(identity.clone()).unwrap();
    spin(&mut host);
    let observed = host
        .desktop_access
        .observation_digest
        .clone()
        .expect("real UIA observation");
    host.bind_desktop("project", &observed, "owned native UIA chat")
        .unwrap();
    spin(&mut host);
    let binding = host.desktop_access.bindings[0].clone();
    draft.worker_adapter = cerulean_domain::project_setup::ProjectWorkerAdapter::Qoder;
    draft.codex_executable.clear();
    draft.worker_model = binding.observation.model_label.clone().unwrap();
    draft.desktop_worker = Some(DesktopWorkerSelection {
        binding,
        project_root: host.snapshot().workflows[0].project.root.clone(),
        model_label: draft.worker_model.clone(),
        workspace_confirmed: true,
        external_tools_acknowledged: true,
    });
    draft.max_seconds = 90;
    let mut host = configured_draft(host, draft, "JOINT_UIA_IPC");
    host.desktop_fixture = None;
    let data = root.join("data");
    save(
        &root.join("fixture.json"),
        &serde_json::json!({"data":data,"pid":std::process::id(),"window":identity,"desktopFixtureUsed":false,"managementFixtureUsed":true}),
    );
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(crate::ipc::serve_host(&data, host))
        .unwrap();
}
