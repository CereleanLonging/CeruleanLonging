//! Opt-in one-task product runner. Never used by ordinary test discovery.
use super::*;

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Reference {
    observation: DesktopObservation,
    baseline: Vec<String>,
    session_id: String,
    cwd: String,
    model_id: String,
    workspace_id: String,
    installed: Vec<PinnedFile>,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PinnedFile {
    path: PathBuf,
    sha256: String,
}
fn reference() -> Reference {
    let path = PathBuf::from(std::env::var_os("CERULEAN_QODER_PRODUCT_REFERENCE").unwrap())
        .canonicalize()
        .unwrap();
    assert!(path.starts_with(repo().join("reports").canonicalize().unwrap()));
    ordinary(&path, false).unwrap();
    for parent in path.parent().unwrap().ancestors() {
        ordinary(parent, true).unwrap();
    }
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
fn scope(reference: &Reference, project: &Path) -> Result<(), String> {
    check_profile_view(
        ProductProfile::QoderQwen,
        &reference.observation,
        &reference.baseline,
        &reference.observation,
        &reference.baseline,
    )?;
    require(
        reference.observation.window.version == "0.4.3.0"
            && reference.observation.session_id.as_deref() == Some(reference.session_id.as_str())
            && reference.session_id == "0fa16906-4f98-4f59-8b4b-9ea6b556982a"
            && reference.model_id == "byok:80fc8950-525d-4afa-a5ab-11e23fdbae44"
            && reference.workspace_id == "9d2371a2-6459-4e9b-91f7-1cafee0fe3f3",
        "Original isolated Qwen session/configuration differs",
    )?;
    require(
        Path::new(&reference.cwd)
            .canonicalize()
            .map_err(|e| e.to_string())?
            == project.canonicalize().map_err(|e| e.to_string())?,
        "Qoder cwd is not the approved isolated project",
    )
}
fn installed(reference: &Reference) -> Result<(), String> {
    require(
        reference.installed.len() == 2,
        "Pin executable and app.asar",
    )?;
    let exe = Path::new(&reference.observation.window.exe)
        .canonicalize()
        .map_err(|e| e.to_string())?;
    let expected = [
        exe.clone(),
        exe.parent().unwrap().join("resources/app.asar"),
    ];
    for (entry, expected) in reference.installed.iter().zip(expected) {
        require(
            entry.path.canonicalize().map_err(|e| e.to_string())? == expected
                && hash(&entry.path) == entry.sha256,
            "Qoder installed executable or package changed",
        )?;
    }
    Ok(())
}
fn database_scope(reference: &Reference, project: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    scope(reference, project)?;
    // Reuse the already pinned Node runtime's built-in SQLite. No new runtime
    // dependency and no credential/configuration table is queried.
    let output = std::process::Command::new("D:/qqagent/node/node.exe")
        .arg(repo().join("tests/fixtures/qoder-product-session.mjs"))
        .arg("D:/Qoder/desktop-data/main.sqlite")
        .arg(&reference.session_id)
        .creation_flags(0x08000000)
        .output()
        .map_err(|e| e.to_string())?;
    require(
        output.status.success() && output.stdout.len() <= 16384,
        "Read-only Qoder session metadata unavailable",
    )?;
    let metadata: SessionMetadata = serde_json::from_slice(&output.stdout)
        .map_err(|_| "Invalid session metadata".to_owned())?;
    require(
        metadata.model == reference.model_id
            && metadata.workspace_id == reference.workspace_id
            && metadata.execution_kind == "local"
            && Path::new(&metadata.cwd)
                .canonicalize()
                .map_err(|e| e.to_string())?
                == project.canonicalize().map_err(|e| e.to_string())?,
        "Current Qoder session metadata changed",
    )
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionMetadata {
    cwd: String,
    model: String,
    workspace_id: String,
    execution_kind: String,
}
fn preflight(host: &RuntimeHost, reference: &Reference, project: &Path) -> ReadReply {
    assert!(host.desktop_fixture.is_none());
    installed(reference).unwrap();
    database_scope(reference, project).unwrap();
    let read = observe(
        &host.root,
        &ReadRequest::TaskPrepare {
            version: 1,
            window: reference.observation.window.clone(),
            previous: None,
        },
    )
    .unwrap();
    // Preserve the actual read before checking so a stopped attempt can be
    // diagnosed from its own observation without another window interaction.
    let diagnostic = host.root.parent().unwrap().join("preflight-observations");
    fs::create_dir_all(&diagnostic).unwrap();
    ordinary(&diagnostic, true).unwrap();
    save(
        &diagnostic.join(format!(
            "{}.json",
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        )),
        &json!({"observation":read.observation,"baseline":read.baseline,"inputOrSend":false}),
    );
    check_profile_view(
        ProductProfile::QoderQwen,
        &reference.observation,
        &reference.baseline,
        read.observation.as_ref().unwrap(),
        &read.baseline,
    )
    .unwrap();
    read
}
fn material(reference: &Reference) -> Value {
    json!({
        "referenceDigest":digest(reference).unwrap(),
        "testExecutableSha256":hash(&std::env::current_exe().unwrap()),
        "bridgeSha256":fingerprint(),
        "runnerSha256":hash(&repo().join("scripts/test-qoder-product-live.ps1")),
        "ownedRunnerSha256":hash(&repo().join("scripts/test-codex-cancel-live.ps1")),
        "sessionReaderSha256":hash(&repo().join("tests/fixtures/qoder-product-session.mjs")),
        "nodeSha256":hash(Path::new("D:/qqagent/node/node.exe")),
        "verifierSha256":hash(&repo().join("tests/fixtures/workbuddy-product-verifier.mjs")),
        "codingVerifierSha256":hash(&repo().join("tests/fixtures/workbuddy-coding-verifier.mjs"))
    })
}
fn selected_project(root: &Path) -> PathBuf {
    let project = std::env::var_os("CERULEAN_QODER_PRODUCT_PROJECT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("project"))
        .canonicalize()
        .unwrap();
    assert!(project.starts_with(repo().join("reports").canonicalize().unwrap()));
    for parent in project.ancestors() {
        ordinary(parent, true).unwrap();
    }
    project
}
fn fresh_root(root: &Path, project: &Path) -> Result<(), String> {
    let entries = fs::read_dir(root)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if project == root.join("project") {
        require(
            entries.len() == 1 && entries[0].file_name() == "project",
            "Preparation must contain only the existing project directory",
        )?;
    } else {
        require(
            entries.is_empty() && !project.starts_with(root) && !root.starts_with(project),
            "New state directory must be empty and separate from the existing project",
        )?;
    }
    empty_project(project)
}
fn empty_project(project: &Path) -> Result<(), String> {
    ordinary(project, true)?;
    require(
        fs::read_dir(project)
            .map_err(|e| e.to_string())?
            .next()
            .is_none(),
        "Project is no longer empty",
    )
}

#[test]
#[ignore = "read-only Qoder native preparation; explicit owned runner and original reference required"]
fn prepare_qoder_product() {
    let root = owned_root("CERULEAN_QODER_PRODUCT");
    let project = selected_project(&root);
    fresh_root(&root, &project).unwrap();
    let reference = reference();
    database_scope(&reference, &project).unwrap();
    installed(&reference).unwrap();
    let (mut host, draft) = project_planning::tests::fixture_at_existing_project(&root, &project);
    let read = preflight(&host, &reference, &project);
    save(
        &root.join("preflight.json"),
        &json!({
            "observation":read.observation,"baseline":read.baseline,"messages":0
        }),
    );
    host.discover_desktop().unwrap();
    spin(&mut host);
    host.inspect_desktop(reference.observation.window.clone())
        .unwrap();
    spin(&mut host);
    assert!(crate::desktop_execution::same_approved_qoder_context(
        &reference.observation,
        host.desktop_access.observation.as_ref().unwrap()
    ));
    let observed = host.desktop_access.observation_digest.clone().unwrap();
    host.bind_desktop("project", &observed, "工作台 Qwen 验收 228 / Token Plan")
        .unwrap();
    spin(&mut host);
    assert!(host.desktop_access.error.is_none() && host.desktop_access.bindings.len() == 1);
    configure_profile(ProductProfile::QoderQwen, &mut host, draft);
    let revision = host.projects["project"].revision().unwrap();
    let review = json!({
        "schemaVersion":1,"scope":"real Qoder desktop and RuntimeHost; synthetic chief/deputy",
        "materials":material(&reference),"reference":reference,
        "revisionDigest":revision.approval_digest().unwrap(),
        "setupDigest":host.project_setups["project"].digest,"plan":revision,
        "maxMessages":1,"maxAdviceMessages":0,"maxStopActions":0,
        "taskMaxSeconds":90,"nativeReplyWaitSeconds":50,"outerTimeoutSeconds":210,
        "realManagementCalls":0,"automaticRetry":false,"preparedWithoutApproval":true,
        "goal":GOAL,"releaseAccepted":false,
        "workspaceScope":"Qoder cwd checked read-only; external tool permissions are not a per-file OS sandbox."
    });
    assert!(
        host.projects["project"].approval().is_none()
            && host.runs.all().is_empty()
            && host.desktop_access.tasks.is_empty()
    );
    save(&root.join("review.json"), &review);
    save(
        &root.join("review-digest.json"),
        &json!({"digest":digest(&review).unwrap()}),
    );
}

fn validate(
    root: &Path,
    host: &RuntimeHost,
    review: &Value,
    reference: &Reference,
    approved: &str,
) -> Result<(), String> {
    let project = selected_project(root);
    empty_project(&project)?;
    require(
        digest(review)? == approved && review["materials"] == material(reference),
        "Approved scope or frozen executables changed",
    )?;
    require(
        review["reference"] == serde_json::to_value(reference).map_err(|e| e.to_string())?,
        "Original reference changed",
    )?;
    let revision = host.projects["project"].revision().ok_or("Missing plan")?;
    require(
        Path::new(&revision.project.root)
            .canonicalize()
            .map_err(|e| e.to_string())?
            == project,
        "Prepared project directory changed",
    )?;
    require(
        review["revisionDigest"] == revision.approval_digest()?
            && review["setupDigest"] == host.project_setups["project"].digest,
        "Prepared plan changed",
    )?;
    require(
        host.projects["project"].approval().is_none()
            && host.runs.all().is_empty()
            && host.desktop_access.tasks.is_empty()
            && !root.join("live-intent.json").exists(),
        "Already started or project is no longer empty",
    )
}
#[test]
#[ignore = "REAL Qoder programming message; explicit frozen digest/owned runner; never retries"]
fn qoder_product_once() {
    let root = owned_root("CERULEAN_QODER_PRODUCT");
    let reference = reference();
    let review: Value =
        serde_json::from_slice(&fs::read(root.join("review.json")).unwrap()).unwrap();
    let mut host = RuntimeHost::open(&root.join("data")).unwrap();
    validate(
        &root,
        &host,
        &review,
        &reference,
        &std::env::var("CERULEAN_QODER_PRODUCT_APPROVED_DIGEST").unwrap(),
    )
    .unwrap();
    preflight(&host, &reference, &selected_project(&root));
    save(
        &root.join("live-intent.json"),
        &json!({
            "at":chrono::Utc::now().to_rfc3339(),"reviewDigest":digest(&review).unwrap(),
            "maxMessages":1,"automaticRetry":false
        }),
    );
    let outcome = execute_profile(
        ProductProfile::QoderQwen,
        &mut host,
        &root,
        &review,
        &reference.observation,
        &reference.baseline,
    );
    host.set_openai_key(None).unwrap();
    save(&root.join("after-live.json"), &host.snapshot());
    save(
        &root.join("execution-result.json"),
        &json!({
            "success":outcome.is_ok(),"error":outcome.as_ref().err(),
            "realManagementCalls":0,"releaseAccepted":false
        }),
    );
    outcome.unwrap();
    let history = host.projects["project"].history().to_vec();
    let decisions = host.management.statuses().len();
    drop(host);
    for i in 0..2 {
        let mut reopened = RuntimeHost::open(&root.join("data")).unwrap();
        reopened.management_fixture = Some(manager);
        for _ in 0..5 {
            reopened.poll_verifications();
            reopened.poll_project_runs();
        }
        assert_eq!(reopened.projects["project"].history(), history);
        assert_eq!(reopened.management.statuses().len(), decisions);
        assert_eq!(reopened.desktop_access.tasks.len(), 1);
        assert!(reopened.desktop_active.is_none() && reopened.desktop_probe.is_none());
        save(&root.join(format!("reopen-{i}.json")), &reopened.snapshot());
    }
    save(
        &root.join("result.json"),
        &json!({
            "status":"passed","realDesktopMessages":1,"realManagementCalls":0,
            "reopensWithoutResend":2,"realThreeLevelAcceptance":false,"releaseAccepted":false
        }),
    );
}

#[test]
fn existing_project_guard_preserves_files_and_rejects_reuse() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("project")).unwrap();
    fresh_root(root.path(), &root.path().join("project")).unwrap();
    fs::write(root.path().join("project/user-file"), b"keep").unwrap();
    assert!(fresh_root(root.path(), &root.path().join("project")).is_err());
    assert_eq!(
        fs::read(root.path().join("project/user-file")).unwrap(),
        b"keep"
    );
    fs::remove_file(root.path().join("project/user-file")).unwrap();
    fs::write(root.path().join("review.json"), b"original").unwrap();
    assert!(fresh_root(root.path(), &root.path().join("project")).is_err());
    assert_eq!(
        fs::read(root.path().join("review.json")).unwrap(),
        b"original"
    );
}

#[test]
fn new_state_directory_preserves_existing_workspace_and_original_evidence() {
    let original = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fs::create_dir(original.path().join("project")).unwrap();
    fs::write(
        original.path().join("review.json"),
        b"original frozen review",
    )
    .unwrap();
    let project = original.path().join("project");
    fresh_root(state.path(), &project).unwrap();
    let (host, _) = project_planning::tests::fixture_at_existing_project(state.path(), &project);
    assert_eq!(
        Path::new(
            &host.projects["project"]
                .configuration()
                .execution_scope
                .as_ref()
                .unwrap()
                .project
                .root
        )
        .canonicalize()
        .unwrap(),
        project.canonicalize().unwrap()
    );
    assert_eq!(
        fs::read(original.path().join("review.json")).unwrap(),
        b"original frozen review"
    );
    assert!(fs::read_dir(&project).unwrap().next().is_none());
    assert!(!state.path().join("project").exists());
    assert!(fresh_root(state.path(), &project).is_err());
}

fn fixture_reference(project: &Path) -> Reference {
    let mut o = observation(qoder_window());
    o.session_id = Some("0fa16906-4f98-4f59-8b4b-9ea6b556982a".into());
    o.model_label = Some(ProductProfile::QoderQwen.model().into());
    Reference {
        observation: o,
        baseline: vec!["a".repeat(64), "b".repeat(64)],
        session_id: "0fa16906-4f98-4f59-8b4b-9ea6b556982a".into(),
        cwd: project.to_string_lossy().into(),
        model_id: "byok:80fc8950-525d-4afa-a5ab-11e23fdbae44".into(),
        workspace_id: "9d2371a2-6459-4e9b-91f7-1cafee0fe3f3".into(),
        installed: vec![],
    }
}

#[test]
fn scope_rejects_other_workspace_session_model_and_cwd() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    fs::create_dir(&project).unwrap();
    let original = fixture_reference(&project);
    scope(&original, &project).unwrap();
    for case in 0..6 {
        let mut changed = original.clone();
        match case {
            0 => changed.workspace_id = "different".into(),
            1 => changed.model_id = "different".into(),
            2 => {
                changed.session_id = "76edb3bc-078f-4ce4-bc9b-b6525fde9c51".into();
                changed.observation.session_id = Some(changed.session_id.clone());
            }
            3 => changed.cwd = root.path().to_string_lossy().into(),
            4 => changed.observation.window.version = "0.4.4.0".into(),
            _ => changed.baseline.clear(),
        }
        assert!(scope(&changed, &project).is_err(), "case {case}");
    }
}

#[test]
fn installed_pins_reject_changed_package_and_incomplete_materials() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("resources")).unwrap();
    let exe = root.path().join("Qoder.exe");
    let package = root.path().join("resources/app.asar");
    fs::write(&exe, b"non-executable fixture").unwrap();
    fs::write(&package, b"package one").unwrap();
    let mut reference = fixture_reference(root.path());
    reference.observation.window.exe = exe.to_string_lossy().into();
    reference.installed = [&exe, &package]
        .into_iter()
        .map(|path| PinnedFile {
            path: path.clone(),
            sha256: hash(path),
        })
        .collect();
    installed(&reference).unwrap();
    fs::write(&package, b"package two").unwrap();
    assert!(installed(&reference).is_err());
    fs::write(&package, b"package one").unwrap();
    reference.installed.swap(0, 1);
    assert!(installed(&reference).is_err());
    reference.installed.pop();
    assert!(installed(&reference).is_err());
}

#[test]
#[ignore = "Read-only recovery of consumed 231 task; no desktop sends, synthetic management only"]
fn recover_qoder_231_original() {
    let evidence = owned_root("CERULEAN_QODER_PRODUCT");
    assert!(fs::read_dir(&evidence).unwrap().next().is_none());
    let original = repo().join("reports/qoder-context-231/prepared");
    let review: crate::desktop_execution::TaskReview =
        serde_json::from_slice(&fs::read(original.join("task-review.json")).unwrap()).unwrap();
    assert_eq!(
        review.id,
        "4c0f7be47a7ee2e89bf45e6c5ac8f752daa065143de0422f777bc91583038928"
    );
    let project = repo().join("reports/qoder-product-228/project");
    let reference = reference();
    installed(&reference).unwrap();
    database_scope(&reference, &project).unwrap();
    ordinary(&project.join("sum.mjs"), false).unwrap();
    assert_eq!(fs::read_dir(&project).unwrap().count(), 1);
    let data = original.join("data");
    let mut host = RuntimeHost::open(&data).unwrap();
    assert!(host.desktop_fixture.is_none());
    host.management_fixture = Some(manager);
    assert_eq!(host.desktop_access.tasks.len(), 1);
    assert_eq!(host.desktop_access.tasks[0].review, review);
    assert!(
        !host.desktop_access.tasks[0].submitted && host.desktop_access.tasks[0].proof.is_none()
    );
    assert_eq!(
        host.projects["project"].state().tasks["task"].state,
        TaskState::Unknown
    );
    assert_eq!(host.runs.all()[0].state, RunState::Attention);
    let intent_path = data
        .join("desktop-task-dispatch-v1")
        .join(digest(&(&review.project_id, &review.attempt.id)).unwrap())
        .join("intent.json");
    let intent = fs::read(&intent_path).unwrap();
    let decisions_before = host.management.statuses().len();
    save(&evidence.join("before.json"), &host.snapshot());
    save(
        &evidence.join("recovery-intent.json"),
        &json!({"reviewDigest":review.id,"readerSha256":fingerprint(),"messages":0,"automaticRetry":false,"at":chrono::Utc::now().to_rfc3339()}),
    );
    let outcome = (|| -> Result<(), String> {
        host.read_desktop_task_with_reader("project", &review.attempt.id, Some(&fingerprint()))?;
        wait_for(&mut host, 65, |h| h.desktop_active.is_none())?;
        save(&evidence.join("after-read.json"), &host.snapshot());
        require(
            host.desktop_access.tasks[0].submitted,
            "Original read failed; preserve evidence and do not resend",
        )?;
        require(
            host.desktop_access.tasks[0]
                .proof
                .as_ref()
                .is_some_and(|p| p.reader_sha256.as_deref() == Some(fingerprint().as_str())),
            "Current host reader was not recorded",
        )?;
        require(
            fs::read(&intent_path).map_err(|e| e.to_string())? == intent,
            "Original intent changed",
        )?;
        require(
            host.management.statuses().len() == decisions_before,
            "Read-only recovery unexpectedly invoked management",
        )?;
        require(
            !crate::credential_vault::exists_for(
                &data,
                cerulean_domain::credentials::CredentialProvider::OpenAi,
            )?,
            "Isolated data must contain no provider credential",
        )?;
        let _cleanup = RemoveSyntheticCredential(data.clone());
        host.set_openai_key(Some(secret::SecretInput::new(
            "sk-synthetic-qoder-product-not-a-provider-key".into(),
        )?))?;
        let revision = host.projects["project"]
            .revision()
            .unwrap()
            .approval_digest()?;
        let setup = host.project_setups["project"].digest.clone();
        host.start_project_run("project", &revision, &setup)?;
        wait_for(&mut host, 100, |h| {
            h.desktop_active.is_none()
                && h.active.is_empty()
                && h.management_active.is_empty()
                && h.runs.all()[0].state != RunState::Active
        })?;
        let task = &host.projects["project"].state().tasks["task"];
        require(
            task.state == TaskState::Accepted
                && task.verified
                && host.runs.all()[0].state == RunState::Completed,
            "Original recovered task did not reach independent Accept",
        )?;
        require(
            host.desktop_access.tasks.len() == 1
                && host.jobs.len() == 1
                && host.management.statuses().len() == decisions_before + 1,
            "Unexpected task or management count",
        )?;
        host.set_openai_key(None)?;
        Ok(())
    })();
    save(&evidence.join("after-recovery.json"), &host.snapshot());
    save(
        &evidence.join("recovery-outcome.json"),
        &json!({"success":outcome.is_ok(),"error":outcome.as_ref().err(),"realDesktopMessages":0,"realManagementCalls":0,"releaseAccepted":false}),
    );
    outcome.unwrap();
    assert!(!crate::credential_vault::exists_for(
        &data,
        cerulean_domain::credentials::CredentialProvider::OpenAi
    )
    .unwrap());
    let history = host.projects["project"].history().to_vec();
    let decisions = host.management.statuses().len();
    drop(host);
    for i in 0..2 {
        let mut reopened = RuntimeHost::open(&data).unwrap();
        reopened.management_fixture = Some(manager);
        for _ in 0..5 {
            reopened.poll_verifications();
            reopened.poll_project_runs();
        }
        assert_eq!(reopened.projects["project"].history(), history);
        assert_eq!(reopened.management.statuses().len(), decisions);
        assert_eq!(reopened.desktop_access.tasks.len(), 1);
        assert!(reopened.desktop_active.is_none() && reopened.desktop_probe.is_none());
        assert_eq!(fs::read(&intent_path).unwrap(), intent);
        save(
            &evidence.join(format!("reopen-{i}.json")),
            &reopened.snapshot(),
        );
    }
    save(
        &evidence.join("result.json"),
        &json!({"status":"passed","originalTaskRecovered":true,"originalReviewId":review.id,"additionalDesktopMessages":0,"realManagementCalls":0,"reopensWithoutResend":2,"realThreeLevelAcceptance":false,"releaseAccepted":false}),
    );
}
