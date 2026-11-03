use super::*;
use cerulean_domain::project_setup::{ProjectRun, RunState};

fn fixture(root: &Path) -> (RuntimeHost, String, String) {
    let (mut host, mut draft) = project_planning::tests::fixture(root);
    project_planning::tests::isolate_draft(root, &mut draft);
    host.configure_project(draft).unwrap();
    project_planning::tests::complete(&mut host, project_planning::tests::proposal("tests"));
    let revision = host.projects["project"]
        .revision()
        .unwrap()
        .approval_digest()
        .unwrap();
    let setup = host.project_setups["project"].digest.clone();
    (host, revision, setup)
}

#[test]
fn no_opt_in_preserves_legacy_approval_bytes_and_never_prepares_accounts() {
    let temp = tempfile::tempdir().unwrap();
    let (mut host, revision, setup) = fixture(temp.path());
    host.approve("project", &revision).unwrap();
    let approval = host.projects["project"].approval().unwrap();
    assert!(approval.worker_account_setup.is_none());
    let json = serde_json::to_value(approval).unwrap();
    assert!(json.get("workerAccountSetup").is_none());
    assert!(serde_json::from_value::<Approval>(json)
        .unwrap()
        .worker_account_setup
        .is_none());
    assert!(host
        .approve_worker_accounts_using("project", &revision, &setup, |_| panic!("must not launch"))
        .is_err());
    assert!(!host.root.join("worker-account-setup").exists());
    assert!(host
        .prepare_authorized_worker_accounts("project", &setup)
        .is_err());
}

#[test]
fn durable_approval_precedes_preparation_and_duplicate_reopen_never_launches() {
    let temp = tempfile::tempdir().unwrap();
    let (mut host, revision, setup) = fixture(temp.path());
    let mut calls = 0;
    host.approve_worker_accounts_using("project", &revision, &setup, |h| {
        calls += 1;
        assert!(h.projects["project"]
            .approval()
            .unwrap()
            .worker_account_setup
            .is_some());
        // The real plan/pinned files are prepared, but no UAC or account mutation.
        let prepared = h.prepare_authorized_worker_accounts("project", &setup)?;
        let plan: serde_json::Value =
            serde_json::from_slice(&fs::read(prepared.plan_path_for_test()).unwrap()).unwrap();
        assert_eq!(plan["projectId"], "project");
        assert_eq!(plan["slots"].as_array().unwrap().len(), 1);
        assert!(plan.get("command").is_none());
        fs::write(
            prepared.plan_path_for_test().with_extension("launch.json"),
            br#"{"success":false,"error":"fixture UAC cancelled"}"#,
        )
        .unwrap();
        Ok(())
    })
    .unwrap();
    assert_eq!(calls, 1);
    let history = host.projects["project"].history().len();
    host.approve_worker_accounts_using("project", &revision, &setup, |_| panic!("duplicate UAC"))
        .unwrap();
    assert_eq!(host.projects["project"].history().len(), history);
    drop(host);
    let mut host = RuntimeHost::open(&temp.path().join("data")).unwrap();
    assert_eq!(
        crate::windows_worker_account::views(&host.root)[0].state,
        "failed"
    );
    host.approve_worker_accounts_using("project", &revision, &setup, |_| panic!("reopen UAC"))
        .unwrap();
    assert_eq!(host.projects["project"].history().len(), history);
    assert_eq!(
        fs::read_dir(host.root.join("worker-account-setup"))
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn launch_failure_and_crash_before_preparation_do_not_retry_approval() {
    let temp = tempfile::tempdir().unwrap();
    let (mut host, revision, setup) = fixture(temp.path());
    let error = host
        .approve_worker_accounts_using("project", &revision, &setup, |_| {
            Err("fixture failed before launch".into())
        })
        .unwrap_err();
    assert!(error.contains("授权已保存"));
    assert!(host.projects["project"]
        .approval()
        .unwrap()
        .worker_account_setup
        .is_some());
    drop(host);
    let mut host = RuntimeHost::open(&temp.path().join("data")).unwrap();
    host.approve_worker_accounts_using("project", &revision, &setup, |_| panic!("crash retry"))
        .unwrap();
    assert!(!host.root.join("worker-account-setup").exists());
    // A separate, explicit setup-button action may prepare a new single attempt.
    let _prepared = host
        .prepare_authorized_worker_accounts("project", &setup)
        .unwrap();
    assert!(host
        .prepare_authorized_worker_accounts("project", &setup)
        .err()
        .unwrap()
        .contains("设置仍在进行"));
}

#[test]
fn altered_digests_and_directory_scope_cannot_inherit_admin_authority() {
    let temp = tempfile::tempdir().unwrap();
    let (mut host, revision, setup) = fixture(temp.path());
    for (r, s) in [
        ("a".repeat(64), setup.clone()),
        (revision.clone(), "b".repeat(64)),
    ] {
        assert!(host
            .approve_worker_accounts_using("project", &r, &s, |_| panic!("stale request"))
            .is_err());
    }
    assert!(host.projects["project"].approval().is_none());
    host.approve_worker_accounts_using("project", &revision, &setup, |_| Ok(()))
        .unwrap();
    let original = host.root.clone();
    // Binding covers the core data location, not only a reusable setup digest.
    let scope = host.worker_account_scope("project", &setup).unwrap();
    host.root = temp.path().join("different-core");
    assert!(host
        .prepare_authorized_worker_accounts("project", &setup)
        .is_err());
    host.root = original;
    host.project_setups
        .get_mut("project")
        .unwrap()
        .setup
        .chief_directory = Some(temp.path().join("different-chief").to_string_lossy().into());
    assert!(host.worker_account_scope("project", &setup).is_err());
    assert_eq!(
        host.projects["project"]
            .approval()
            .unwrap()
            .worker_account_setup
            .as_ref(),
        Some(&scope)
    );
}

#[test]
fn explicit_preparation_never_mutates_active_run_permissions() {
    let temp = tempfile::tempdir().unwrap();
    let (mut host, revision, setup) = fixture(temp.path());
    host.approve_worker_accounts_using("project", &revision, &setup, |_| Ok(()))
        .unwrap();
    host.worker_account_idle("project").unwrap();
    assert!(host.planning_idle("project").is_err()); // Ordinary configuration stays locked.
    host.runs
        .save(
            ProjectRun {
                id: "run".into(),
                project_id: "project".into(),
                revision_digest: revision,
                setup_digest: setup.clone(),
                state: RunState::Active,
                message: "fixture".into(),
                acknowledged_decisions: 0,
            },
            "user",
        )
        .unwrap();
    assert!(host.worker_account_idle("project").is_err());
    assert!(host
        .prepare_authorized_worker_accounts("project", &setup)
        .is_err());
    assert!(!host.root.join("worker-account-setup").exists());
}

#[test]
fn scoped_approval_enables_reader20_before_launch_and_missing_guard_fails_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let (mut host, revision, setup) = fixture(temp.path());
    host.approve_worker_accounts_using("project", &revision, &setup, |h| {
        assert_eq!(
            crate::data_format::status(&h.root)?.minimum_reader_generation,
            20
        );
        Ok(())
    })
    .unwrap();
    let marker = host
        .root
        .join("events")
        .join(crate::worker_account_guard::FILE_NAME);
    drop(host);
    let host = RuntimeHost::open(&temp.path().join("data")).unwrap();
    drop(host);
    fs::remove_file(marker).unwrap();
    assert!(RuntimeHost::open(&temp.path().join("data")).is_err());
}

#[test]
fn review_and_snapshot_only_expose_authorization_without_launching() {
    let temp = tempfile::tempdir().unwrap();
    let (mut host, revision, setup) = fixture(temp.path());
    assert_eq!(
        host.worker_account_review_digest("project"),
        Some(setup.clone())
    );
    host.approve_worker_accounts_using("project", &revision, &setup, |_| Ok(()))
        .unwrap();
    assert_eq!(host.worker_account_review_digest("project"), Some(setup));
    assert!(!host.root.join("worker-account-setup").exists());
    let encoded = serde_json::to_value(host.projects["project"].approval().unwrap()).unwrap();
    assert!(encoded["workerAccountSetup"]["scopeDigest"]
        .as_str()
        .is_some());
    let mut altered = encoded;
    altered["workerAccountSetup"]["administratorCommands"] = serde_json::json!(["anything"]);
    assert!(serde_json::from_value::<Approval>(altered).is_err());
}
