//! Real authenticated user pipe, using only synthetic local checker fixtures.
use super::*;
use cerulean_core::ipc::{Client, ServiceReply, UserOperation};
use std::time::{Duration, Instant};

async fn cli(data: &Path, args: Vec<String>) -> serde_json::Value {
    let label = args[1].clone();
    let started = Instant::now();
    eprintln!("CLI recheck {label} begin");
    let data = data.to_path_buf();
    let output = tokio::task::spawn_blocking(move || {
        use std::os::windows::process::CommandExt;
        let binary = std::env::var_os("CERULEAN_RECHECK_TEST_CORE")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_cerulean-core").into());
        std::process::Command::new(binary)
            .args(["--workbench", "--data-dir"])
            .arg(data)
            .args(args)
            .creation_flags(0x08000000)
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    assert!(
        output.status.success(),
        "CLI {label} after {:?}: {}",
        started.elapsed(),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    eprintln!("CLI recheck {label} done in {:?}", started.elapsed());
    assert_eq!(value["ok"], true);
    value
}

pub(super) async fn call(data: &Path, command: Command) -> ServiceReply {
    let receipt = command.supports_receipt();
    let request = Request {
        id: "recheck-user-call".into(),
        protocol_version: PROTOCOL_VERSION,
        command,
    };
    let operation = if receipt {
        UserOperation::Command { request }
    } else {
        UserOperation::Request { request }
    };
    let reply = Client::connect(data)
        .await
        .unwrap()
        .call(operation)
        .await
        .unwrap();
    if reply.response.error.is_none() {
        assert_eq!(reply.command_receipt.is_some(), receipt);
        assert_eq!(reply.response.result.is_none(), receipt);
    }
    reply
}
pub(super) async fn connect(data: &Path) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Client::connect(data).await.is_err() {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
async fn status(data: &Path, operation: &str) -> RecheckStatus {
    let reply = call(
        data,
        Command::ReadVerificationRecheck {
            project_id: "project".into(),
            operation_id: operation.into(),
        },
    )
    .await;
    assert!(reply.response.error.is_none(), "{:?}", reply.response.error);
    reply
        .response
        .result
        .unwrap()
        .verification_recheck_status
        .unwrap()
}
pub(super) async fn terminal(data: &Path, operation: &str) -> RecheckStatus {
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let result = status(data, operation).await;
        if !matches!(
            result.job.as_ref().unwrap().state,
            VerificationState::Running | VerificationState::Cancelling
        ) {
            return result;
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
pub(super) async fn stop(data: &Path, preserve: bool) {
    let operation = if preserve {
        let reply = Client::connect(data)
            .await
            .unwrap()
            .call(UserOperation::ReviewPreservedShutdown {
                id: "review-stop".into(),
                protocol_version: PROTOCOL_VERSION,
            })
            .await
            .unwrap();
        assert!(reply.response.error.is_none(), "{:?}", reply.response.error);
        UserOperation::PreserveShutdown {
            id: "preserve-stop".into(),
            protocol_version: PROTOCOL_VERSION,
            review_digest: reply.preservation_review.unwrap().digest,
        }
    } else {
        UserOperation::Shutdown {
            id: "stop".into(),
            protocol_version: PROTOCOL_VERSION,
        }
    };
    let reply = Client::connect(data)
        .await
        .unwrap()
        .call(operation)
        .await
        .unwrap();
    assert!(reply.stopping, "{:?}", reply.response.error);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "explicit ordinary Windows accounts; run the native recheck suite"]
async fn recheck_user_pipe_reviews_approves_queries_runs_and_recovers_exact_operation() {
    let (root, store, a) = runner::corrected("corrected_checker");
    drop(store);
    let data = root.path().join("data");
    let host = RuntimeHost::open(&data).unwrap();
    let owned = data.clone();
    let service = tokio::spawn(async move { cerulean_core::ipc::serve_host(&owned, host).await });
    connect(&data).await;
    let request_path = root.path().join("recheck-request.json");
    fs::write(
        &request_path,
        serde_json::to_vec(&RecheckRequest {
            project_id: "project".into(),
            original_operation_id: a.review.original_operation_id.clone(),
            replacement_checks: a.review.replacement_checks.clone(),
            reason: a.review.reason.clone(),
        })
        .unwrap(),
    )
    .unwrap();
    let cli_review = cli(
        &data,
        vec![
            "verification-recheck".into(),
            "review".into(),
            "--file".into(),
            request_path.to_string_lossy().into(),
        ],
    )
    .await;
    let review = call(
        &data,
        Command::ReviewVerificationRecheck {
            input: Box::new(RecheckRequest {
                project_id: "project".into(),
                original_operation_id: a.review.original_operation_id.clone(),
                replacement_checks: a.review.replacement_checks.clone(),
                reason: a.review.reason.clone(),
            }),
        },
    )
    .await;
    assert!(
        review.response.error.is_none(),
        "{:?}",
        review.response.error
    );
    let review = review
        .response
        .result
        .unwrap()
        .verification_recheck_review
        .unwrap();
    assert_eq!(review, a.review);
    assert_eq!(
        serde_json::from_value::<RecheckReview>(cli_review["result"].clone()).unwrap(),
        review
    );
    let review_path = root.path().join("recheck-review.json");
    fs::write(&review_path, serde_json::to_vec(&review).unwrap()).unwrap();
    let evidence = data
        .join("workflows/project/verification")
        .join(&a.operation_id);
    let claims = data
        .join("verification-recheck-jobs/project")
        .join(&a.operation_id);
    assert!(!evidence.exists() && !claims.exists());
    assert!(call(
        &data,
        Command::StartVerificationRecheck {
            project_id: "project".into(),
            operation_id: a.operation_id.clone()
        }
    )
    .await
    .response
    .error
    .is_some());
    let mut stale = review.clone();
    stale.ledger_head = "0".repeat(64);
    assert!(call(
        &data,
        Command::ApproveVerificationRecheck {
            review: Box::new(stale),
            approval_operation_id: "approve-recheck".into(),
            operation_id: a.operation_id.clone()
        }
    )
    .await
    .response
    .error
    .is_some());
    cli(
        &data,
        vec![
            "verification-recheck".into(),
            "approve".into(),
            "--file".into(),
            review_path.to_string_lossy().into(),
            "--approval-operation".into(),
            "approve-recheck".into(),
            "--operation".into(),
            a.operation_id.clone(),
        ],
    )
    .await;
    {
        let approved = call(
            &data,
            Command::ApproveVerificationRecheck {
                review: Box::new(review.clone()),
                approval_operation_id: "approve-recheck".into(),
                operation_id: a.operation_id.clone(),
            },
        )
        .await;
        assert!(
            approved.response.error.is_none(),
            "{:?}",
            approved.response.error
        );
    }
    let approved = status(&data, &a.operation_id).await;
    assert_eq!(approved.review_digest, digest(&review).unwrap());
    assert_eq!(approved.approval.review, review);
    assert!(approved.job.is_none());
    let shown = cli(
        &data,
        vec![
            "verification-recheck".into(),
            "show".into(),
            "--project".into(),
            "project".into(),
            "--operation".into(),
            a.operation_id.clone(),
        ],
    )
    .await;
    assert_eq!(
        serde_json::from_value::<RecheckStatus>(shown["result"].clone()).unwrap(),
        approved
    );
    assert!(
        !evidence.exists() && !claims.exists(),
        "approval must not launch"
    );
    cli(
        &data,
        vec![
            "verification-recheck".into(),
            "start".into(),
            "--project".into(),
            "project".into(),
            "--operation".into(),
            a.operation_id.clone(),
        ],
    )
    .await;
    let running = status(&data, &a.operation_id).await;
    assert_eq!(running.approval_event, approved.approval_event);
    assert!(running.job.is_some());
    assert!(call(
        &data,
        Command::StartVerificationRecheck {
            project_id: "project".into(),
            operation_id: a.operation_id.clone()
        }
    )
    .await
    .response
    .error
    .is_some());
    assert_eq!(
        terminal(&data, &a.operation_id).await.job.unwrap().state,
        VerificationState::Succeeded
    );
    cli(
        &data,
        vec![
            "verification-recheck".into(),
            "recover".into(),
            "--project".into(),
            "project".into(),
            "--operation".into(),
            a.operation_id.clone(),
        ],
    )
    .await;
    assert!(call(
        &data,
        Command::RecoverVerificationRecheck {
            project_id: "project".into(),
            operation_id: a.operation_id.clone()
        }
    )
    .await
    .response
    .error
    .is_none());
    assert_eq!(
        fs::read(evidence.join("check-temp/validator-count")).unwrap(),
        b"one"
    );
    let details = call(
        &data,
        Command::ReadVerificationDetail {
            project_id: "project".into(),
            operation_id: a.operation_id.clone(),
        },
    )
    .await;
    assert!(
        details
            .response
            .result
            .unwrap()
            .verification_detail
            .unwrap()
            .ledger_recorded
    );
    let snapshot = Client::snapshot(&data).await.unwrap();
    assert_eq!(
        snapshot.workflows[0].state.tasks["task"].state,
        TaskState::Submitted
    );
    assert!(snapshot.workflows[0].state.tasks["task"].verified);
    let normal_stop = Client::connect(&data)
        .await
        .unwrap()
        .call(UserOperation::Shutdown {
            id: "normal-stop-before-chief-accept".into(),
            protocol_version: PROTOCOL_VERSION,
        })
        .await
        .unwrap();
    assert!(normal_stop.response.error.is_some());
    assert!(!normal_stop.stopping);
    stop(&data, true).await;
    tokio::time::timeout(Duration::from_secs(15), service)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let mut host = RuntimeHost::open(&data).unwrap();
    let reopened = host.handle(Request {
        id: "reopened".into(),
        protocol_version: PROTOCOL_VERSION,
        command: Command::ReadVerificationRecheck {
            project_id: "project".into(),
            operation_id: a.operation_id.clone(),
        },
    });
    let reopened = reopened
        .result
        .unwrap()
        .verification_recheck_status
        .unwrap();
    assert_eq!(reopened.approval_event, approved.approval_event);
    assert_eq!(reopened.job.unwrap().state, VerificationState::Succeeded);
    assert_eq!(
        fs::read(evidence.join("check-temp/validator-count")).unwrap(),
        b"one"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "explicit ordinary Windows accounts; run the native recheck suite"]
async fn recheck_user_pipe_cancel_preserves_unknown_without_relaunch_or_old_recovery() {
    let (root, store, a) = runner::corrected("slow_checker");
    drop(store);
    let data = root.path().join("data");
    let mut host = RuntimeHost::open(&data).unwrap();
    host.approve_verification_recheck(&a.review, "approve-recheck", &a.operation_id)
        .unwrap();
    let owned = data.clone();
    let service = tokio::spawn(async move { cerulean_core::ipc::serve_host(&owned, host).await });
    connect(&data).await;
    assert!(call(
        &data,
        Command::CancelVerificationRecheck {
            project_id: "project".into(),
            operation_id: a.review.original_operation_id.clone()
        }
    )
    .await
    .response
    .error
    .is_some());
    assert!(call(
        &data,
        Command::StartVerificationRecheck {
            project_id: "project".into(),
            operation_id: a.operation_id.clone()
        }
    )
    .await
    .response
    .error
    .is_none());
    let evidence = data
        .join("workflows/project/verification")
        .join(&a.operation_id);
    let deadline = Instant::now() + Duration::from_secs(45);
    while !evidence.join("check-temp/validator-started").exists() {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(call(
        &data,
        Command::CancelVerificationRecheck {
            project_id: "project".into(),
            operation_id: a.operation_id.clone()
        }
    )
    .await
    .response
    .error
    .is_none());
    assert_eq!(
        terminal(&data, &a.operation_id).await.job.unwrap().state,
        VerificationState::Unknown
    );
    for command in [
        Command::RecoverVerificationRecheck {
            project_id: "project".into(),
            operation_id: a.operation_id.clone(),
        },
        Command::StartVerificationRecheck {
            project_id: "project".into(),
            operation_id: a.operation_id.clone(),
        },
        Command::RecoverVerification {
            project_id: "project".into(),
            operation_id: a.operation_id.clone(),
            evidence_digest: "a".repeat(64),
        },
    ] {
        assert!(call(&data, command).await.response.error.is_some());
    }
    assert!(!evidence.join("check-temp/validator-count").exists());
    let result_path = evidence.join("result.json");
    let original_result = fs::read(&result_path).unwrap();
    let parked = evidence.join("result-parked.json");
    fs::rename(&result_path, &parked).unwrap();
    let read_stop = || UserOperation::ReviewPreservedShutdown {
        id: "review-recheck-stop".into(),
        protocol_version: PROTOCOL_VERSION,
    };
    assert!(Client::connect(&data)
        .await
        .unwrap()
        .call(read_stop())
        .await
        .unwrap()
        .response
        .error
        .is_some());
    fs::rename(&parked, &result_path).unwrap();
    for failure in ["process tree cleanup timeout", "check check: output-limit"] {
        let mut value: serde_json::Value = serde_json::from_slice(&original_result).unwrap();
        value["error"] = failure.into();
        fs::write(&result_path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(Client::connect(&data)
            .await
            .unwrap()
            .call(read_stop())
            .await
            .unwrap()
            .response
            .error
            .is_some());
    }
    fs::write(&result_path, &original_result).unwrap();
    let stdout_path = evidence.join("check.stdout");
    let original_stdout = fs::read(&stdout_path).unwrap();
    fs::write(&stdout_path, b"tampered disposable fixture log").unwrap();
    assert!(Client::connect(&data)
        .await
        .unwrap()
        .call(read_stop())
        .await
        .unwrap()
        .response
        .error
        .is_some());
    fs::write(&stdout_path, &original_stdout).unwrap();
    stop(&data, true).await;
    tokio::time::timeout(Duration::from_secs(15), service)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let mut host = RuntimeHost::open(&data).unwrap();
    let response = host.handle(Request {
        id: "reopened-cancelled".into(),
        protocol_version: PROTOCOL_VERSION,
        command: Command::ReadVerificationRecheck {
            project_id: "project".into(),
            operation_id: a.operation_id.clone(),
        },
    });
    assert_eq!(
        response
            .result
            .unwrap()
            .verification_recheck_status
            .unwrap()
            .job
            .unwrap()
            .state,
        VerificationState::Unknown
    );
    assert!(!evidence.join("check-temp/validator-count").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "explicit ordinary Windows accounts; run the native recheck suite"]
async fn recheck_disposition_pipe_cli_unstarted_receipt_reopen_and_changed_evidence() {
    use cerulean_domain::{
        verification_recheck::RecheckDispositionReview,
        workspace::{WorkspaceQuery, WorkspaceResult},
    };
    let (root, store, a) = runner::corrected("corrected_checker");
    drop(store);
    let data = root.path().join("data");
    let mut host = RuntimeHost::open(&data).unwrap();
    host.approve_verification_recheck(&a.review, "approve-retirement", &a.operation_id)
        .unwrap();
    let before = host.snapshot().workflows[0].state.clone();
    let owned = data.clone();
    let service = tokio::spawn(async move { cerulean_core::ipc::serve_host(&owned, host).await });
    connect(&data).await;
    let args = vec![
        "verification-recheck".into(),
        "review-disposition".into(),
        "--project".into(),
        "project".into(),
        "--operation".into(),
        a.operation_id.clone(),
        "--reason".into(),
        "Keep original failure, withdraw this check".into(),
    ];
    let reviewed = cli(&data, args).await;
    let review: RecheckDispositionReview =
        serde_json::from_value(reviewed["result"].clone()).unwrap();
    assert!(review.launch.is_none());
    let workspace = Client::connect(&data)
        .await
        .unwrap()
        .call(UserOperation::Workspace {
            id: "workspace-review".into(),
            protocol_version: PROTOCOL_VERSION,
            query: WorkspaceQuery::VerificationRecheckDispositionReview {
                project_id: "project".into(),
                operation_id: a.operation_id.clone(),
                reason: review.reason.clone(),
            },
        })
        .await
        .unwrap();
    let WorkspaceResult::VerificationRecheckDispositionReview(r) = workspace.workspace.unwrap()
    else {
        panic!("review");
    };
    assert_eq!(*r, review);
    let review_file = root.path().join("disposition-review.json");
    fs::write(
        &review_file,
        serde_json::to_vec(&reviewed["result"]).unwrap(),
    )
    .unwrap();
    let confirmed = cli(
        &data,
        vec![
            "verification-recheck".into(),
            "retire".into(),
            "--file".into(),
            review_file.to_string_lossy().into(),
            "--event-operation".into(),
            "dispose-original".into(),
        ],
    )
    .await;
    assert!(confirmed["commandReceipt"].is_object());
    let s = status(&data, &a.operation_id).await;
    assert!(s.job.is_none());
    let receipt = s.disposition.unwrap();
    assert_eq!(receipt.review, review);
    assert_eq!(receipt.event_operation_id, "dispose-original");
    assert!(receipt.evidence_unchanged);
    let shown = cli(
        &data,
        vec![
            "verification-recheck".into(),
            "show".into(),
            "--project".into(),
            "project".into(),
            "--operation".into(),
            a.operation_id.clone(),
        ],
    )
    .await;
    assert_eq!(
        shown["result"]["disposition"]["eventOperationId"],
        "dispose-original"
    );
    let repeated = call(
        &data,
        Command::RetireVerificationRecheck {
            review: Box::new(review.clone()),
            event_operation_id: "dispose-original".into(),
        },
    )
    .await;
    assert!(repeated.response.error.is_none());
    assert_eq!(
        status(&data, &a.operation_id).await.disposition.unwrap(),
        receipt
    );
    assert!(call(
        &data,
        Command::RetireVerificationRecheck {
            review: Box::new(review),
            event_operation_id: "other-disposition".into()
        }
    )
    .await
    .response
    .error
    .is_some());
    assert!(call(
        &data,
        Command::StartVerificationRecheck {
            project_id: "project".into(),
            operation_id: a.operation_id.clone()
        }
    )
    .await
    .response
    .error
    .is_some());
    stop(&data, true).await;
    service.await.unwrap().unwrap();
    let mut host = RuntimeHost::open(&data).unwrap();
    assert_eq!(host.snapshot().workflows[0].state, before);
    let retained = host.handle(Request {
        id: "reopen".into(),
        protocol_version: PROTOCOL_VERSION,
        command: Command::ReadVerificationRecheck {
            project_id: "project".into(),
            operation_id: a.operation_id.clone(),
        },
    });
    assert_eq!(
        retained
            .result
            .unwrap()
            .verification_recheck_status
            .unwrap()
            .disposition
            .unwrap(),
        receipt
    );
    let evidence = data
        .join("workflows/project/verification")
        .join(&a.operation_id);
    assert!(!evidence.exists());
    fs::create_dir(&evidence).unwrap();
    fs::write(evidence.join("result.json"), b"changed after disposition").unwrap();
    let changed = host.handle(Request {
        id: "changed".into(),
        protocol_version: PROTOCOL_VERSION,
        command: Command::ReadVerificationRecheck {
            project_id: "project".into(),
            operation_id: a.operation_id.clone(),
        },
    });
    let d = changed
        .result
        .unwrap()
        .verification_recheck_status
        .unwrap()
        .disposition
        .unwrap();
    assert!(!d.evidence_unchanged);
    assert_eq!(d.event, receipt.event);
    assert_eq!(d.review, receipt.review);
    assert!(host
        .start_verification_recheck("project", &a.operation_id)
        .is_err());
    assert_eq!(host.snapshot().workflows[0].state, before);
}
