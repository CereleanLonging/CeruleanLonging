//! Explicitly offline app-server peer. Never reads provider credentials or invokes a model.
use cerulean_adapters::codex::{CodexClient, ControlOutcome, SteerRequest};
use cerulean_core::advice_delivery::{DeliveryIntent, DeliveryJournal, DeliveryOutcome};
use cerulean_domain::execution::digest;
use serde_json::{json, Value};
use std::{fs, path::PathBuf, time::Duration};

#[tokio::test]
#[ignore = "requires explicitly supplied built protocol peer; no real model"]
async fn owned_process_steer_binds_original_turn_and_rejects_wrong_ack_without_replay() {
    let root =
        PathBuf::from(std::env::var_os("CERULEAN_STEER_DIR").expect("explicit proof directory"));
    let exe =
        PathBuf::from(std::env::var_os("CERULEAN_STEER_EXE").expect("explicit fixture executable"));
    fs::create_dir_all(&root).unwrap();
    let mut reports = vec![];
    for mode in ["accepted", "wrong-ack", "lost-local-ack"] {
        let wrong = mode == "wrong-ack";
        let base = root.join(mode);
        fs::create_dir(&base).unwrap();
        let workspace = base.join("workspace");
        fs::create_dir(&workspace).unwrap();
        let executable = base.join("offline-peer.exe");
        fs::copy(&exe, &executable).unwrap();
        fs::write(
            base.join("workspace.json"),
            serde_json::to_vec(&workspace).unwrap(),
        )
        .unwrap();
        fs::write(base.join("await-steer"), "").unwrap();
        if wrong {
            fs::write(base.join("wrong-steer-ack"), "").unwrap();
        }
        let mut client = CodexClient::launch(&executable, &workspace).unwrap();
        assert_eq!(
            client.connect().await.unwrap().account_type.as_deref(),
            Some("fixture")
        );
        let binding = client.create_worker("fixture-worker").await.unwrap();
        let original = "DAG_STEP_A: write alpha to result.txt";
        let turn = client
            .start_turn(&binding, "original-attempt", original)
            .await
            .unwrap();
        let (_stop, mut cancel) = tokio::sync::oneshot::channel();
        let (wake, mut control) = tokio::sync::watch::channel(());
        wake.send(()).unwrap();
        assert_eq!(
            client
                .wait_turn_or_control(
                    &binding.thread_id,
                    &turn,
                    Duration::from_secs(3),
                    &mut cancel,
                    &mut control
                )
                .await
                .unwrap(),
            ControlOutcome::SupervisionReady
        );
        let request = SteerRequest {
            delivery_id: DeliveryIntent::id_for("advice-fixture").unwrap(),
            thread_id: binding.thread_id.clone(),
            turn_id: turn.clone(),
            text: "Inspect the failing check without changing scope.".into(),
        };
        // Hash anchors are fixture data here; cross-ledger host authorization is separate.
        let intent = DeliveryIntent {
            delivery_id: request.delivery_id.clone(),
            management_operation: "advice-fixture".into(),
            project_id: "project".into(),
            task_id: "task".into(),
            attempt_id: "original-attempt".into(),
            thread_id: binding.thread_id.clone(),
            turn_id: turn.clone(),
            incident_id: "incident".into(),
            revision_digest: digest(&"fixture-revision").unwrap(),
            run_digest: digest(&"fixture-run").unwrap(),
            observation_digest: digest(&"fixture-observation").unwrap(),
            management_response_digest: digest(&"fixture-response").unwrap(),
            advice_event_hash: digest(&"fixture-local-advice").unwrap(),
            text_digest: digest(&request.text).unwrap(),
        };
        let journal_root = base.join("delivery");
        let mut journal = DeliveryJournal::open(&journal_root).unwrap();
        journal.begin(intent.clone()).unwrap();
        assert!(journal_root
            .join(format!("{}.0.json", request.delivery_id))
            .exists());
        let outcome = client.steer_turn(&request).await;
        if wrong {
            assert!(outcome
                .unwrap_err()
                .contains("acknowledgement turn mismatch"));
            journal
                .finish(&request.delivery_id, DeliveryOutcome::Unknown)
                .unwrap();
            assert!(client.steer_turn(&request).await.is_err());
            assert!(client
                .read_attempt(
                    &binding.thread_id,
                    "original-attempt",
                    original,
                    Some(&turn)
                )
                .await
                .is_err());
        } else {
            let ack = outcome.unwrap();
            assert_eq!(ack.turn_id, turn);
            if mode != "lost-local-ack" {
                journal
                    .finish(&request.delivery_id, DeliveryOutcome::Acknowledged { ack })
                    .unwrap();
            }
            assert!(client.steer_turn(&request).await.is_err());
            assert_eq!(
                client
                    .wait_turn(&binding.thread_id, &turn, Duration::from_secs(3))
                    .await
                    .unwrap()
                    .status,
                "completed"
            );
            assert_eq!(
                client
                    .read_attempt(
                        &binding.thread_id,
                        "original-attempt",
                        original,
                        Some(&turn)
                    )
                    .await
                    .unwrap()
                    .turn_id,
                turn
            );
            // Completion clears active ownership: another delivery cannot target the old turn.
            let after = SteerRequest {
                delivery_id: "delivery-after-completion".into(),
                ..request.clone()
            };
            assert!(client.steer_turn(&after).await.is_err());
        }
        assert_eq!(client.request_counts().get("turn/steer"), Some(&1));
        assert_eq!(client.request_counts().get("turn/start"), Some(&1));
        client.close().await.unwrap();
        drop(journal);
        let mut journal = DeliveryJournal::open(&journal_root).unwrap();
        assert_eq!(
            journal.record(&request.delivery_id).unwrap().acknowledged(),
            mode == "accepted"
        );
        assert!(journal.begin(intent).is_err());
        let rows: Vec<Value> = fs::read_to_string(base.join("protocol.jsonl"))
            .unwrap()
            .lines()
            .map(|s| serde_json::from_str(s).unwrap())
            .collect();
        for method in ["turn/steer", "turn/start", "thread/start"] {
            assert_eq!(rows.iter().filter(|r| r["method"] == method).count(), 1);
        }
        let wire: Value =
            serde_json::from_slice(&fs::read(base.join("steer-request.json")).unwrap()).unwrap();
        assert_eq!(wire["expectedTurnId"], turn);
        assert_eq!(wire["clientUserMessageId"], request.delivery_id);
        reports.push(json!({"mode":mode,"oneThread":true,"oneTurnStart":true,"oneSteer":true,"boundTurn":turn,"readback":!wrong,"journalReopened":true,"noNewSendAuthorization":true,"acknowledged":mode=="accepted"}));
    }
    fs::write(root.join("result.json"),serde_json::to_vec_pretty(&json!({"at":chrono::Utc::now().to_rfc3339(),"status":"passed","realModelCalls":0,"scope":"actual CodexClient and owned offline protocol peer; no host delivery transaction","cases":reports})).unwrap()).unwrap();
}
