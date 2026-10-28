use super::*;
use cerulean_adapters::api_generation::{ApiTarget, ApiUsage};
use std::sync::atomic::{AtomicUsize, Ordering};
pub(super) fn identity() -> CallIdentity {
    CallIdentity {
        project_id: "project".into(),
        task_id: "task".into(),
        attempt_id: "attempt".into(),
        revision_digest: "a".repeat(64),
        request_id: "request".into(),
        round: 1,
        fingerprint: RequestFingerprint {
            target: ApiTarget::OpenAi {},
            requested_model: "fixture".into(),
            body_sha256: "b".repeat(64),
            tool_catalog_sha256: "c".repeat(64),
            max_output_tokens: 1000,
            timeout_seconds: 10,
        },
    }
}
pub(super) fn observation() -> ApiObservation {
    ApiObservation {
        response_id: "response-1".into(),
        actual_model: "fixture".into(),
        text: "Done.".into(),
        tool_calls: vec![],
        usage: ApiUsage {
            input_tokens: 2,
            output_tokens: 3,
            cache_read_tokens: None,
            cache_write_tokens: None,
        },
    }
}
#[tokio::test]
async fn diagnostic_rejection_returns_reason_but_reopens_legacy_failure_without_resend() {
    use cerulean_adapters::api_generation::ResponseRejection;
    let dir = tempfile::tempdir().unwrap();
    let id = identity();
    let mut journal = ApiCallJournal::open(dir.path(), &id).unwrap();
    let failure = ApiFailure::ResponseRejected {
        reason: ResponseRejection::OutputLimit,
    };
    let returned = journal
        .execute_with(&id.fingerprint, || async {
            Err::<((), ApiObservation), _>(failure.clone())
        })
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(returned, failure);
    assert!(
        format!("API request stopped: {returned:?}; no automatic retry").contains("OutputLimit")
    );
    let head = journal.head().to_owned();
    drop(journal);
    let bytes = fs::read(dir.path().join("01.json")).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        value["body"]["event"]["outcome"]["failure"],
        serde_json::json!({"kind":"invalidResponse"})
    );
    let mut recovered = ApiCallJournal::read_existing(dir.path()).unwrap();
    assert_eq!(recovered.head(), head);
    assert!(recovered.state().remote_outcome_unknown());
    assert!(recovered
        .execute_with(&id.fingerprint, || async {
            panic!("must never replay a rejected response");
            #[allow(unreachable_code)]
            Ok(((), observation()))
        })
        .await
        .is_err());
    assert_eq!(fs::read(dir.path().join("01.json")).unwrap(), bytes);
}
#[test]
fn call_recovery_never_creates_a_missing_directory_lock_or_intent() {
    let temp = tempfile::tempdir().unwrap();
    let id = identity();
    let missing = temp.path().join("missing");
    assert!(ApiCallJournal::open_existing(&missing, &id).is_err());
    assert!(ApiCallJournal::read_existing(&missing).is_err());
    assert!(!missing.exists());
    let empty = temp.path().join("empty");
    fs::create_dir(&empty).unwrap();
    assert!(ApiCallJournal::open_existing(&empty, &id).is_err());
    assert!(ApiCallJournal::read_existing(&empty).is_err());
    assert_eq!(fs::read_dir(&empty).unwrap().count(), 0);
    fs::write(empty.join("writer.lock"), []).unwrap();
    assert!(ApiCallJournal::open_existing(&empty, &id).is_err());
    assert!(ApiCallJournal::read_existing(&empty).is_err());
    assert_eq!(fs::read_dir(&empty).unwrap().count(), 1);
}
#[tokio::test]
async fn call_recovery_discovers_original_identity_under_lock_and_cannot_send() {
    let temp = tempfile::tempdir().unwrap();
    let id = identity();
    let mut journal = ApiCallJournal::open(temp.path(), &id).unwrap();
    assert!(ApiCallJournal::read_existing(temp.path()).is_err());
    journal
        .execute_with(&id.fingerprint, || async {
            Err::<((), ApiObservation), _>(ApiFailure::Transport {})
        })
        .await
        .unwrap()
        .unwrap_err();
    let expected = journal.head().to_owned();
    drop(journal);
    let before: Vec<_> = fs::read_dir(temp.path())
        .unwrap()
        .map(|f| {
            let p = f.unwrap().path();
            (p.clone(), fs::read(p).unwrap())
        })
        .collect();
    let mut recovered = ApiCallJournal::read_existing(temp.path()).unwrap();
    assert_eq!(recovered.identity(), &id);
    assert_eq!(recovered.head(), expected);
    assert!(recovered.state().remote_outcome_unknown());
    assert!(recovered
        .execute_with(&id.fingerprint, || async {
            panic!("read-only recovery must never send");
            #[allow(unreachable_code)]
            Ok(((), observation()))
        })
        .await
        .is_err());
    drop(recovered);
    let mut wrong = id;
    wrong.request_id = "other".into();
    assert!(ApiCallJournal::open_existing(temp.path(), &wrong).is_err());
    for (p, bytes) in before {
        assert_eq!(fs::read(p).unwrap(), bytes);
    }
}
#[test]
fn call_recovery_rejects_missing_intent_pending_and_nonempty_lock_without_repair() {
    for mode in ["intent", "pending", "lock"] {
        let temp = tempfile::tempdir().unwrap();
        let id = identity();
        let mut journal = ApiCallJournal::open(temp.path(), &id).unwrap();
        journal
            .publish(
                &Event::Intent {
                    identity: id.clone(),
                },
                |_| Ok(()),
            )
            .unwrap();
        drop(journal);
        match mode {
            "intent" => fs::remove_file(temp.path().join("00.json")).unwrap(),
            "pending" => fs::write(temp.path().join("01.pending"), b"partial").unwrap(),
            _ => fs::write(temp.path().join("writer.lock"), b"foreign").unwrap(),
        }
        let before: Vec<_> = fs::read_dir(temp.path())
            .unwrap()
            .map(|f| {
                let p = f.unwrap().path();
                (p.clone(), fs::read(p).unwrap())
            })
            .collect();
        assert!(ApiCallJournal::open_existing(temp.path(), &id).is_err());
        assert!(ApiCallJournal::read_existing(temp.path()).is_err());
        for (p, bytes) in before {
            assert_eq!(fs::read(p).unwrap(), bytes);
        }
    }
}
#[tokio::test]
async fn dropping_inflight_future_leaves_unknown_and_never_retries() {
    let dir = tempfile::tempdir().unwrap();
    let id = identity();
    let mut journal = ApiCallJournal::open(dir.path(), &id).unwrap();
    let operation = journal.execute_with(&id.fingerprint, || async {
        std::future::pending::<Result<((), ApiObservation), ApiFailure>>().await
    });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), operation)
            .await
            .is_err()
    );
    assert!(journal.state().remote_outcome_unknown());
    drop(journal);
    let mut reopened = ApiCallJournal::open(dir.path(), &id).unwrap();
    assert!(reopened.state().remote_outcome_unknown());
    assert!(reopened
        .execute_with(&id.fingerprint, || async { Ok(((), observation())) })
        .await
        .is_err());
}
#[tokio::test]
async fn production_entry_records_pre_dispatch_cancellation_without_http() {
    use cerulean_adapters::api_generation::{ApiMessage, ApiRequest};
    use cerulean_domain::secret::SecretInput;
    let client =
        ApiClient::new(SecretInput::new("sk-synthetic-journal-test".into()).unwrap()).unwrap();
    let request = client
        .prepare(ApiRequest {
            target: ApiTarget::OpenAi {},
            model: "fixture".into(),
            instructions: "approved scope".into(),
            messages: vec![ApiMessage::User("test".into())],
            tools: vec![],
            max_output_tokens: 1000,
            timeout_seconds: 10,
        })
        .unwrap();
    let mut id = identity();
    id.fingerprint = request.fingerprint().clone();
    let dir = tempfile::tempdir().unwrap();
    let mut journal = ApiCallJournal::open(dir.path(), &id).unwrap();
    let (tx, mut rx) = tokio::sync::oneshot::channel();
    tx.send(()).unwrap();
    assert_eq!(
        journal
            .execute_once(&client, request, &mut rx)
            .await
            .unwrap()
            .unwrap_err(),
        ApiFailure::NotSent {}
    );
    drop(journal);
    let reopened = ApiCallJournal::open(dir.path(), &id).unwrap();
    assert!(matches!(
        reopened.state().outcome,
        Some(CallOutcome::Failure {
            failure: ApiFailure::NotSent {}
        })
    ));
    assert!(!reopened.state().remote_outcome_unknown());
}
#[tokio::test]
async fn durable_intent_precedes_send_and_reopening_never_resends() {
    for success in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let id = identity();
        let sends = AtomicUsize::new(0);
        let mut journal = ApiCallJournal::open(dir.path(), &id).unwrap();
        assert!(ApiCallJournal::open(dir.path(), &id).is_err());
        let result = journal
            .execute_with(&id.fingerprint, || async {
                assert!(dir.path().join("00.json").is_file());
                assert!(!dir.path().join("00.pending").exists());
                sends.fetch_add(1, Ordering::SeqCst);
                if success {
                    Ok(("reply", observation()))
                } else {
                    Err(ApiFailure::Transport {})
                }
            })
            .await
            .unwrap();
        assert_eq!(result.is_ok(), success);
        assert_eq!(journal.state().remote_outcome_unknown(), !success);
        assert!(dir.path().join("01.json").is_file());
        drop(journal);
        let mut reopened = ApiCallJournal::open(dir.path(), &id).unwrap();
        assert!(reopened
            .execute_with(&id.fingerprint, || async {
                sends.fetch_add(1, Ordering::SeqCst);
                Ok(((), observation()))
            })
            .await
            .is_err());
        assert_eq!(sends.load(Ordering::SeqCst), 1);
        drop(reopened);
        let mut wrong = id;
        wrong.attempt_id = "other".into();
        assert!(ApiCallJournal::open(dir.path(), &wrong).is_err());
    }
}
#[tokio::test]
async fn mismatch_and_publication_failure_prevent_network() {
    let dir = tempfile::tempdir().unwrap();
    let id = identity();
    let mut journal = ApiCallJournal::open(dir.path(), &id).unwrap();
    let mut wrong = id.fingerprint.clone();
    wrong.body_sha256 = "d".repeat(64);
    assert!(journal
        .execute_with(&wrong, || async {
            panic!("must not send");
            #[allow(unreachable_code)]
            Ok(((), observation()))
        })
        .await
        .is_err());
    assert!(!journal.state().intent_recorded);
    fs::write(dir.path().join("00.pending"), b"existing").unwrap();
    assert!(journal
        .execute_with(&id.fingerprint, || async {
            panic!("must not send");
            #[allow(unreachable_code)]
            Ok(((), observation()))
        })
        .await
        .is_err());
    assert!(!journal.healthy());
    assert_eq!(
        fs::read(dir.path().join("00.pending")).unwrap(),
        b"existing"
    );
}
#[tokio::test]
async fn result_publication_failure_withholds_reply_and_no_second_send() {
    let dir = tempfile::tempdir().unwrap();
    let id = identity();
    let mut journal = ApiCallJournal::open(dir.path(), &id).unwrap();
    let result = journal
        .execute_with(&id.fingerprint, || async {
            fs::write(dir.path().join("01.pending"), b"interrupted").unwrap();
            Ok(("must-not-escape", observation()))
        })
        .await;
    assert!(result.is_err());
    assert!(!journal.healthy());
    assert!(journal.state().remote_outcome_unknown());
    drop(journal);
    assert!(ApiCallJournal::open(dir.path(), &id).is_err());
}
#[tokio::test]
async fn interrupted_publication_is_conservative_at_all_eight_boundaries() {
    for sequence in 0..2 {
        for stage in ["created", "synced", "published", "committed"] {
            let dir = tempfile::tempdir().unwrap();
            let id = identity();
            let mut journal = ApiCallJournal::open(dir.path(), &id).unwrap();
            if sequence == 1 {
                journal
                    .publish(
                        &Event::Intent {
                            identity: id.clone(),
                        },
                        |_| Ok(()),
                    )
                    .unwrap();
            }
            let event = if sequence == 0 {
                Event::Intent {
                    identity: id.clone(),
                }
            } else {
                Event::Outcome {
                    rejected_usage: None,
                    outcome: CallOutcome::Reply {
                        observation: observation(),
                    },
                }
            };
            assert!(journal
                .publish(&event, |at| if at == stage {
                    Err("injected publication fault".into())
                } else {
                    Ok(())
                })
                .is_err());
            assert!(!journal.healthy());
            drop(journal);
            match ApiCallJournal::open(dir.path(), &id) {
                Ok(mut reopened) => {
                    assert_eq!(stage, "committed");
                    assert_eq!(reopened.state().remote_outcome_unknown(), sequence == 0);
                    assert!(reopened
                        .execute_with(&id.fingerprint, || async {
                            panic!("must not resend");
                            #[allow(unreachable_code)]
                            Ok(((), observation()))
                        })
                        .await
                        .is_err());
                }
                Err(_) => assert_ne!(stage, "committed"),
            }
        }
    }
}
#[test]
fn corrupt_missing_extra_and_reordered_evidence_is_rejected() {
    for case in ["corrupt", "missing", "extra", "order"] {
        let dir = tempfile::tempdir().unwrap();
        let id = identity();
        let mut journal = ApiCallJournal::open(dir.path(), &id).unwrap();
        journal
            .publish(
                &Event::Intent {
                    identity: id.clone(),
                },
                |_| Ok(()),
            )
            .unwrap();
        journal
            .publish(
                &Event::Outcome {
                    rejected_usage: None,
                    outcome: CallOutcome::Failure {
                        failure: ApiFailure::NotSent {},
                    },
                },
                |_| Ok(()),
            )
            .unwrap();
        assert!(!journal.state().remote_outcome_unknown());
        drop(journal);
        match case {
            "corrupt" => fs::write(dir.path().join("01.json"), b"{}").unwrap(),
            "missing" => fs::remove_file(dir.path().join("00.json")).unwrap(),
            "extra" => fs::write(dir.path().join("02.json"), b"{}").unwrap(),
            "order" => {
                let mut v: serde_json::Value =
                    serde_json::from_slice(&fs::read(dir.path().join("01.json")).unwrap()).unwrap();
                v["body"]["sequence"] = serde_json::json!(0);
                fs::write(dir.path().join("01.json"), v.to_string()).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(ApiCallJournal::open(dir.path(), &id).is_err());
    }
}
