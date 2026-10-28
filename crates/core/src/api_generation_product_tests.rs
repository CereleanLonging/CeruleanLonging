use super::*;
use cerulean_domain::{
    api_generation_resolution::ApiGenerationResolutionInput, count_resolution::CountResolutionState,
};
fn ready(root: &Path) -> RuntimeHost {
    super::ready(root, true)
}
fn input(request: &str) -> ApiGenerationResolutionInput {
    ApiGenerationResolutionInput {
        project_id: "project".into(),
        request_id: request.into(),
        amount_usd_nanos: 0,
        billing_reference: "explicit zero generation invoice".into(),
        explanation: "isolated user confirmation".into(),
    }
}
#[test]
fn api_generation_product_separate_identity_original_readback_and_changed_page() {
    let t = tempfile::tempdir().unwrap();
    let data = t.path().join("data");
    let mut h = ready(t.path());
    let before = snapshot(&data);
    let result = h
        .count_resolution_result(&Command::ReadApiGenerations {
            offset: 0,
            revision: None,
        })
        .unwrap();
    assert!(result.counts.is_none());
    let page = result.api_generations.unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(page.entries[0].task_id, "task");
    assert_eq!(page.entries[0].attempt_id, "attempt");
    assert_eq!(page.entries[0].round, 1);
    assert_eq!(snapshot(&data), before);
    let request = &page.entries[0].request_id;
    assert!(h
        .count_resolution_result(&Command::ReadApiGenerations {
            offset: 1,
            revision: None
        })
        .is_err());
    let r = h
        .count_resolution_result(&Command::ReviewApiGenerationResolution {
            input: input(request),
        })
        .unwrap()
        .api_generation_review
        .unwrap();
    assert_eq!(r.task_id, "task");
    assert_eq!(r.round, 1);
    assert!(r.response_id.is_some());
    assert!(r.pricing_issue.is_some());
    assert_eq!(r.original_outcome, "unpriced");
    assert_eq!(snapshot(&data), before);
    let command = Command::ConfirmApiGenerationResolution {
        input: input(request),
        review_digest: r.review_digest.clone(),
    };
    assert!(!command.supports_receipt());
    let response = h.handle(Request {
        id: "wrong-route".into(),
        protocol_version: cerulean_domain::PROTOCOL_VERSION,
        command: command.clone(),
    });
    assert!(response.error.is_some());
    assert_eq!(snapshot(&data), before);
    let record = h
        .count_resolution_result(&command)
        .unwrap()
        .api_generation_confirmation
        .unwrap()
        .record
        .unwrap();
    assert_eq!(record.review, r);
    assert_eq!(record.state, CountResolutionState::Recorded);
    let after = snapshot(&data);
    h.count_resolution_result(&command).unwrap();
    assert_eq!(snapshot(&data), after);
    assert!(h
        .count_resolution_result(&Command::ReadApiGenerations {
            offset: 1,
            revision: Some(page.revision)
        })
        .is_err());
    let lookup = Command::ReadApiGenerationConfirmation {
        project_id: "project".into(),
        request_id: request.clone(),
        review_digest: r.review_digest,
    };
    assert_eq!(
        h.count_resolution_result(&lookup)
            .unwrap()
            .api_generation_confirmation
            .unwrap()
            .record,
        Some(record)
    );
    assert_eq!(snapshot(&data), after);
    assert!(h.count_resolution_result(&Command::Snapshot).is_err());
}
#[test]
fn api_generation_product_bounds_reply_before_confirming_user_event() {
    let t = tempfile::tempdir().unwrap();
    let data = t.path().join("data");
    let mut h = ready(t.path());
    let request = h.currency_accounts["project"]
        .state()
        .bookings
        .keys()
        .next()
        .unwrap()
        .clone();
    let mut i = input(&request);
    i.billing_reference = "\u{1f}".repeat(8000);
    i.explanation = "\u{1f}".repeat(8000);
    i.validate().unwrap();
    let r = h.review_api_generation_resolution(&i).unwrap();
    let before = snapshot(&data);
    assert!(h
        .count_resolution_result(&Command::ReviewApiGenerationResolution { input: i.clone() })
        .unwrap_err()
        .contains("64KiB"));
    assert!(h
        .count_resolution_result(&Command::ConfirmApiGenerationResolution {
            input: i,
            review_digest: r.digest().unwrap()
        })
        .unwrap_err()
        .contains("64KiB"));
    assert_eq!(snapshot(&data), before);
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "Owned Windows authenticated IPC, synthetic provider only; normal host permissions"]
async fn api_generation_product_native_confirmation_and_reopen_readback() {
    use crate::ipc::{self, Client, UserOperation};
    struct Service(tokio::task::JoinHandle<Result<(), String>>);
    impl Drop for Service {
        fn drop(&mut self) {
            self.0.abort();
        }
    }
    async fn client(data: &Path) -> Client {
        for _ in 0..100 {
            if let Ok(c) = Client::connect(data).await {
                return c;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("owned service not ready");
    }
    async fn query(
        data: &Path,
        command: Command,
    ) -> cerulean_domain::count_resolution::CountResolutionResult {
        let r = client(data)
            .await
            .call(UserOperation::CountResolution {
                request: Request {
                    id: "api-fee-owned".into(),
                    protocol_version: cerulean_domain::PROTOCOL_VERSION,
                    command,
                },
            })
            .await
            .unwrap();
        assert!(r.response.error.is_none(), "{:?}", r.response.error);
        assert!(r.response.result.is_none());
        r.count_resolution.unwrap()
    }
    async fn stop(data: &Path, mut service: Service) {
        let r = client(data)
            .await
            .call(UserOperation::Shutdown {
                id: "api-fee-stop".into(),
                protocol_version: cerulean_domain::PROTOCOL_VERSION,
            })
            .await
            .unwrap();
        // Unknown execution still requires its original disposition. Fee confirmation
        // must not make this task eligible for ordinary shutdown.
        assert!(!r.stopping);
        assert_eq!(r.response.error.unwrap().code, "unresolvedTasks");
        // End only this test-owned idle service, then await cancellation before
        // opening the same data again. This models interruption, not acceptance.
        service.0.abort();
        let stopped = tokio::time::timeout(Duration::from_secs(20), &mut service.0)
            .await
            .unwrap();
        assert!(stopped.unwrap_err().is_cancelled());
    }
    let t = tempfile::tempdir().unwrap();
    let data = t.path().join("data");
    let h = ready(t.path());
    assert!(h.api_active.is_empty());
    assert_eq!(
        h.projects["project"].state().tasks["task"].state,
        TaskState::Unknown
    );
    let d = data.clone();
    let service = Service(tokio::spawn(async move { ipc::serve_host(&d, h).await }));
    let page = query(
        &data,
        Command::ReadApiGenerations {
            offset: 0,
            revision: None,
        },
    )
    .await
    .api_generations
    .unwrap();
    let i = input(&page.entries[0].request_id);
    let r = query(
        &data,
        Command::ReviewApiGenerationResolution { input: i.clone() },
    )
    .await
    .api_generation_review
    .unwrap();
    let record = query(
        &data,
        Command::ConfirmApiGenerationResolution {
            input: i,
            review_digest: r.review_digest.clone(),
        },
    )
    .await
    .api_generation_confirmation
    .unwrap()
    .record
    .unwrap();
    assert_eq!(record.review, r);
    let before = snapshot(&data);
    let lookup = Command::ReadApiGenerationConfirmation {
        project_id: "project".into(),
        request_id: page.entries[0].request_id.clone(),
        review_digest: r.review_digest,
    };
    assert_eq!(
        query(&data, lookup.clone())
            .await
            .api_generation_confirmation
            .unwrap()
            .record,
        Some(record.clone())
    );
    assert_eq!(snapshot(&data), before);
    stop(&data, service).await;
    let h = RuntimeHost::open(&data).unwrap();
    let d = data.clone();
    let service = Service(tokio::spawn(async move { ipc::serve_host(&d, h).await }));
    assert_eq!(
        query(&data, lookup)
            .await
            .api_generation_confirmation
            .unwrap()
            .record,
        Some(record)
    );
    stop(&data, service).await;
    assert_eq!(snapshot(&data), before);
}
