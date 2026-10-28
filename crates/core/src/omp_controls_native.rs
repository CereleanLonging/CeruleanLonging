//! Isolated native control fixture. Synthetic management and offline worker only.
use super::*;
use crate::management_execution::ApplicationTarget;
use cerulean_domain::project_setup::ProjectWorkerAdapter;
use std::time::{Duration, Instant};
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "explicit isolated native OMP controls fixture; no real model calls"]
async fn native_omp_controls_fixture() {
    let outer = PathBuf::from(
        std::env::var_os("CERULEAN_OMP_CONTROL_DIR").expect("fresh fixture directory"),
    )
    .canonicalize()
    .unwrap();
    let parent = PathBuf::from(
        std::env::var_os("CERULEAN_OMP_TEST_PARENT").expect("configuration-free parent"),
    )
    .canonicalize()
    .unwrap();
    assert!(outer.starts_with(&parent) && outer != parent && !outer.join("data").exists());
    let mode = std::env::var("CERULEAN_OMP_CONTROL_MODE").unwrap();
    assert!(matches!(
        mode.as_str(),
        "cancel" | "startup" | "prepared" | "unsent" | "recover" | "stop-review"
    ));
    let mut host = if mode == "cancel" {
        let (mut host, mut draft) = super::project_planning::tests::fixture(&outer);
        let exe = PathBuf::from(std::env::var_os("CERULEAN_OMP_FIXTURE_EXE").unwrap());
        assert_eq!(exe.file_name().unwrap(), "omp_offline_worker_fixture.exe");
        draft.worker_adapter = ProjectWorkerAdapter::Omp;
        draft.codex_executable = exe.to_string_lossy().into();
        draft.worker_model = "fixture-file-wait".into();
        draft.artifact_paths = vec!["result.txt".into()];
        draft.management_max_requests = 5;
        draft.max_seconds = 180;
        host.configure_project(draft).unwrap();
        crate::credential_vault::save_for(
            &host.root,
            credentials::CredentialProvider::DeepSeek,
            &secret::SecretInput::new("cerulean-offline-placeholder-not-a-real-key".into())
                .unwrap(),
        )
        .unwrap();
        host.deepseek_key_configured = true;
        super::omp_project_tests::enable(&mut host);
        let setup = host.project_setups["project"].digest.clone();
        host.request_chief_plan("project", &setup).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while host.projects["project"].revision().is_none() && Instant::now() < deadline {
            host.poll_verifications();
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let revision = host.projects["project"]
            .revision()
            .unwrap()
            .approval_digest()
            .unwrap();
        host.approve("project", &revision).unwrap();
        host.start_project_run("project", &revision, &setup)
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !outer.join("project/result.txt").exists() && Instant::now() < deadline {
            host.poll_verifications();
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert!(outer.join("project/result.txt").exists());
        assert_eq!(host.omp_active.len(), 1);
        host
    } else {
        let mut host = super::omp_host::tests::fixture(&outer, None);
        super::omp_management_bridge::tests::decision(&mut host);
        super::omp_management_bridge::tests::reserve(&mut host);
        if mode != "startup" && mode != "stop-review" {
            let plan = host.projects["project"].revision().unwrap().clone();
            let attempt = Attempt {
                workspace_proof: None,
                id: "attempt".into(),
                task_id: "task".into(),
                worker_id: "worker-omp".into(),
                revision_digest: plan.approval_digest().unwrap(),
                fence: 1,
                binding: SessionBinding {
                    adapter_id: "omp".into(),
                    adapter_version: "native-fixture".into(),
                    session_id: "fixture-session".into(),
                    identity_strength: IdentityStrength::ControllerRequest(
                        ControllerRequestIdentity {
                            connection_id: "fixture-connection".into(),
                            request_id: "fixture-prompt".into(),
                        },
                    ),
                    workspace: plan.project.root.clone(),
                },
            };
            let command = WorkflowCommand::Assign {
                task: "task".into(),
                worker: "worker-omp".into(),
                attempt: "attempt".into(),
            };
            let context = CommandContext {
                execution: Some(ExecutionEvidence::Attempt {
                    attempt: attempt.clone(),
                }),
                ..Default::default()
            };
            let current = host
                .management
                .intent("deputy-omp")
                .unwrap()
                .state_digest
                .clone();
            host.management
                .prepare_application(
                    "deputy-omp",
                    ApplicationTarget::Workflow {
                        principal: "deputy".into(),
                        command: command.clone(),
                        context: Box::new(context.clone()),
                    },
                    &current,
                )
                .unwrap();
            if mode != "prepared" {
                let store = host.projects.get_mut("project").unwrap();
                store
                    .command("mgmt-deputy-omp", "deputy", command, context, true)
                    .unwrap();
                if mode == "recover" {
                    let (_, _, intent, _) =
                        crate::omp_execution::scope(store, "task", "attempt").unwrap();
                    store
                        .command(
                            "dispatch",
                            "core",
                            WorkflowCommand::DispatchIntent {
                                task: "task".into(),
                                attempt: "attempt".into(),
                            },
                            CommandContext {
                                thread_id: Some(attempt.binding.session_id),
                                cwd: Some(plan.project.root),
                                ..Default::default()
                            },
                            true,
                        )
                        .unwrap();
                    let mut journal = crate::omp_journal::OmpJournal::open(
                        &store.omp_attempt_path("attempt").unwrap(),
                        &intent,
                    )
                    .unwrap();
                    journal.record(&cerulean_adapters::omp_transport::worker::WorkerEvent::DispatchIntent{intent}).unwrap();
                    journal
                        .record(
                            &cerulean_adapters::omp_transport::worker::WorkerEvent::NotStarted {
                                request_id: "fixture-prompt".into(),
                            },
                        )
                        .unwrap();
                }
            }
        }
        if mode == "stop-review" {
            let record = host.omp_startups.get("project", "attempt").unwrap();
            let path = crate::async_omp::cancel_path(&host.root, &record.intent)
                .unwrap()
                .with_extension("pending");
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, b"{\"version\":1,").unwrap();
        }
        drop(host);
        RuntimeHost::open(&outer.join("data")).unwrap()
    };
    let snapshot = host.snapshot();
    let job = snapshot.omp_jobs[0].clone();
    fs::write(outer.join("fixture.json"),serde_json::to_vec_pretty(&serde_json::json!({"mode":mode,"projectId":job.project_id,"taskId":job.task_id,"attemptId":job.attempt_id,"realModelCalls":0,"operation":job.management_operation,"ownerPid":std::process::id()})).unwrap()).unwrap();
    if mode == "cancel" {
        let data = host.root.clone();
        crate::ipc::serve_host(&data, host).await.unwrap();
    } else {
        host.poll_verifications();
        if mode == "startup"
            && std::env::var("CERULEAN_OMP_CONTROL_CLOSURE_PENDING").as_deref() == Ok("1")
        {
            let r = host.omp_startups.get("project", "attempt").unwrap().clone();
            assert!(host
                .omp_startups
                .close_observed(
                    "project",
                    "attempt",
                    &r.head,
                    crate::startup_journal::StartupClosure {
                        reason: "interrupted native close".into(),
                        workflow_sequence: r.intent.workflow_sequence,
                        workflow_hash: r.intent.workflow_hash
                    },
                    |p| if p == crate::startup_journal::PublishPoint::PartialWritten {
                        Err("fixture partial close".into())
                    } else {
                        Ok(())
                    }
                )
                .is_err());
        }
        if mode == "startup"
            && std::env::var("CERULEAN_OMP_CONTROL_STARTUP_PENDING").as_deref() == Ok("1")
        {
            let path = host.root.join("omp-startups/project/attempt.0.json");
            fs::hard_link(&path, path.with_extension("pending")).unwrap();
            fs::remove_file(path).unwrap();
        }
    }
}
