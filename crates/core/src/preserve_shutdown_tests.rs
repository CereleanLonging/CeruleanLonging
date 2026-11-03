//! Synthetic durable failure fixtures; no model or worker process is launched.
use super::*;
use crate::host::project_planning::tests::{complete, fixture as empty, proposal};
use cerulean_domain::worker_observation::WorkerObservationTrace;

pub(crate) fn fixture(root: &Path) -> RuntimeHost {
    let (mut host, draft) = empty(root);
    host.configure_project(draft).unwrap();
    let mut plan = proposal("tests");
    if let management::ManagementDecision::Plan { tasks, .. } = &mut plan {
        let mut next = tasks[0].clone();
        next.id = "dependent".into();
        next.dependencies = vec!["task".into()];
        tasks.push(next);
    }
    complete(&mut host, plan);
    let revision = host.projects["project"].revision().unwrap().clone();
    let revision_digest = revision.approval_digest().unwrap();
    host.approve("project", &revision_digest).unwrap();
    crate::host::project_planning::tests::retain_legacy_run(&mut host);
    let run = host.runs.all()[0].clone();
    // No scheduler tick occurs in this fixture: pause before any dispatch.
    host.pause_project_run(&run.id).unwrap();
    let attempt = Attempt {
        workspace_proof: None,
        id: "failed-attempt".into(),
        task_id: "task".into(),
        worker_id: "worker-codex".into(),
        revision_digest: revision_digest.clone(),
        fence: 1,
        binding: SessionBinding {
            adapter_id: "codex".into(),
            adapter_version: "synthetic".into(),
            session_id: "failed-thread".into(),
            identity_strength: IdentityStrength::ProviderId,
            workspace: revision.project.root.clone(),
        },
    };
    let store = host.projects.get_mut("project").unwrap();
    store
        .command(
            "assign-failed",
            "deputy",
            WorkflowCommand::Assign {
                task: "task".into(),
                worker: "worker-codex".into(),
                attempt: attempt.id.clone(),
            },
            CommandContext {
                execution: Some(ExecutionEvidence::Attempt {
                    attempt: attempt.clone(),
                }),
                ..Default::default()
            },
            true,
        )
        .unwrap();
    store
        .command(
            "dispatch-failed",
            "core",
            WorkflowCommand::DispatchIntent {
                task: "task".into(),
                attempt: attempt.id.clone(),
            },
            CommandContext {
                thread_id: Some(attempt.binding.session_id.clone()),
                cwd: Some(attempt.binding.workspace.clone()),
                ..Default::default()
            },
            true,
        )
        .unwrap();
    let receipt = ExecutionReceipt {
        schema_version: 1,
        task_id: "task".into(),
        attempt_id: attempt.id.clone(),
        fence: 1,
        binding: attempt.binding.clone(),
        turn_id: Some("failed-turn".into()),
        observed_at: chrono::Utc::now().to_rfc3339(),
        status: ReceiptStatus::Running,
        evidence_sha256: "a".repeat(64),
    };
    store
        .command(
            "started-failed",
            "core",
            WorkflowCommand::Started {
                task: "task".into(),
                attempt: attempt.id.clone(),
            },
            CommandContext {
                thread_id: Some(attempt.binding.session_id.clone()),
                turn_id: receipt.turn_id.clone(),
                cwd: Some(attempt.binding.workspace.clone()),
                execution: Some(ExecutionEvidence::Receipt {
                    receipt,
                    manifest: None,
                }),
                ..Default::default()
            },
            true,
        )
        .unwrap();
    store
        .command(
            "unknown-failed",
            "core",
            WorkflowCommand::RecoveryUnknown {
                task: "task".into(),
                attempt: attempt.id.clone(),
            },
            CommandContext::default(),
            true,
        )
        .unwrap();
    let now = chrono::Utc::now().to_rfc3339();
    let job = CodexJob {
        management_operation: None,
        project_id: "project".into(),
        task_id: "task".into(),
        attempt_id: attempt.id.clone(),
        revision_digest,
        worker: revision.task("task").unwrap().worker.clone().unwrap(),
        state: CodexJobState::Unknown,
        started_at: now.clone(),
        updated_at: now,
        message: Some("Synthetic failed terminal; no model was called".into()),
    };
    crate::async_codex::save(&host.root, &job).unwrap();
    let observed = crate::worker_observation::view(
        &job,
        WorkerObservationTrace {
            thread_id: Some(attempt.binding.session_id),
            turn_id: Some("failed-turn".into()),
            finished: true,
            ..Default::default()
        },
        None,
        None,
    )
    .unwrap();
    let folder = host.root.join("worker-observations/project");
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("failed-attempt.json"),
        serde_json::to_vec(&serde_json::json!({
            "schemaVersion": 1, "view": observed,
        }))
        .unwrap(),
    )
    .unwrap();
    host.codex_jobs.insert(("project".into(), attempt.id), job);
    // Preserve an actual pre-failure edit, without claiming it was submitted.
    fs::write(root.join("project/sum.mjs"), "export const partial = 1;\n").unwrap();
    host
}

#[test]
fn preserved_shutdown_unknown_keeps_attempt_files_and_pending_dependency() {
    let root = tempfile::tempdir().unwrap();
    let mut host = fixture(root.path());
    assert!(!host.can_shutdown());
    let (before, digest) = host.preserved_shutdown_evidence().unwrap();
    assert_eq!(
        before.workflows[0].state.tasks["task"].state,
        TaskState::Unknown
    );
    assert_eq!(
        before.workflows[0].state.tasks["dependent"].state,
        TaskState::Pending
    );
    assert!(before.workflows[0].state.tasks["dependent"]
        .attempt
        .is_none());
    let events = host.projects["project"].history().to_vec();
    assert_eq!(host.preserved_shutdown_evidence().unwrap().1, digest);
    assert_eq!(host.projects["project"].history(), events.as_slice());
    let artifact = fs::read(root.path().join("project/sum.mjs")).unwrap();
    drop(host);
    let cold = crate::reader_generation::tests::snapshot(&root.path().join("data"));
    let mut reopened = RuntimeHost::open(&root.path().join("data")).unwrap();
    reopened.poll_verifications();
    assert!(!reopened.can_shutdown());
    assert_eq!(reopened.preserved_shutdown_evidence().unwrap().1, digest);
    assert_eq!(reopened.projects["project"].history(), events.as_slice());
    assert!(reopened.codex_active.is_empty() && reopened.api_active.is_empty());
    drop(reopened);
    assert_eq!(
        crate::reader_generation::tests::snapshot(&root.path().join("data")),
        cold
    );
    assert_eq!(
        fs::read(root.path().join("project/sum.mjs")).unwrap(),
        artifact
    );
}

#[test]
fn preserved_shutdown_rejects_incomplete_observation_activity_and_storage_faults() {
    let root = tempfile::tempdir().unwrap();
    let mut host = fixture(root.path());
    let path = host
        .root
        .join("worker-observations/project/failed-attempt.json");
    let original = fs::read(&path).unwrap();
    let mut v: serde_json::Value = serde_json::from_slice(&original).unwrap();
    v["view"]["trace"]["finished"] = false.into();
    fs::write(&path, serde_json::to_vec(&v).unwrap()).unwrap();
    assert!(host.preserved_shutdown_evidence().is_err());
    fs::write(&path, original).unwrap();
    for kind in 0..4 {
        match kind {
            0 => host.base.poisoned = true,
            1 => host.api_fault = Some("fixture persistence failure".into()),
            2 => host.runner_error = Some("fixture runner failure".into()),
            _ => host.orchestrating = true,
        }
        assert!(host.preserved_shutdown_evidence().is_err());
        host.base.poisoned = false;
        host.api_fault = None;
        host.runner_error = None;
        host.orchestrating = false;
    }
    let (sender, receiver) = std::sync::mpsc::channel();
    host.codex_probe = Some(receiver);
    assert!(host.preserved_shutdown_evidence().is_err());
    host.codex_probe = None;
    drop(sender);
    let job = host.codex_jobs.values().next().unwrap().clone();
    let (_updates, receiver) = std::sync::mpsc::channel();
    host.codex_active.insert(
        (job.project_id.clone(), job.attempt_id.clone()),
        crate::async_codex::ActiveCodex {
            preparation_cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            advice_wake: tokio::sync::watch::channel(()).0,
            advice_gate: std::sync::Arc::new(std::sync::atomic::AtomicU8::new(0)),
            observation: None,
            observation_saved: None,
            observation_error: None,
            snapshot: crate::async_verification::snapshot(&host.projects["project"]),
            job,
            receiver,
            disconnected: true,
            cancel: None,
        },
    );
    assert!(
        host.preserved_shutdown_evidence().is_err(),
        "unconfirmed cleanup cannot be ignored"
    );
    host.codex_active.clear();
    assert!(host.preserved_shutdown_evidence().is_ok());
}
