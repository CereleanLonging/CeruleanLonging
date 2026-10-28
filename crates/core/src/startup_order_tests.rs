//! Exercise the production start entry before any worker can own a process.
use super::*;
use crate::management_execution::{DecisionIntent, DecisionState};
use cerulean_adapters::openai::DecisionObservation;
use cerulean_domain::management::{DecisionEnvelope, ManagementDecision, Purpose};

#[test]
fn startup_intent_survives_job_io_failure_and_blocks_duplicate_production_starts() {
    for managed in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let (mut host, draft) = super::project_planning::tests::fixture(dir.path());
        host.configure_project(draft).unwrap();
        super::project_planning::tests::complete(
            &mut host,
            super::project_planning::tests::proposal("tests"),
        );
        let revision = host.projects["project"]
            .revision()
            .unwrap()
            .approval_digest()
            .unwrap();
        host.approve("project", &revision).unwrap();
        if managed {
            let context = host
                .management_context("project", "Implement a sum function")
                .unwrap();
            let state_digest = digest(&context).unwrap();
            host.management
                .begin(DecisionIntent {
                    operation_id: "start-fixture".into(),
                    project_id: "project".into(),
                    purpose: Purpose::Assign,
                    model: "fixture-model".into(),
                    state_digest: state_digest.clone(),
                    context,
                    created_at: chrono::Utc::now().to_rfc3339(),
                })
                .unwrap();
            host.management
                .finish(
                    "start-fixture",
                    Ok(DecisionObservation {
                        response_id: "resp_start_fixture".into(),
                        requested_model: "fixture-model".into(),
                        actual_model: "fixture-model".into(),
                        input_tokens: 0,
                        output_tokens: 0,
                        envelope: DecisionEnvelope {
                            state_digest,
                            decision: ManagementDecision::Assign {
                                task_id: "task".into(),
                                worker_id: "worker-codex".into(),
                                rationale: "controlled fixture".into(),
                            },
                        },
                    }),
                )
                .unwrap();
        }
        // An owned fixture file prevents job directory creation after StartupIntent commits.
        let obstruction = host.root.join("codex-jobs");
        fs::write(&obstruction, b"owned job IO failure fixture").unwrap();
        let result = if managed {
            host.start_managed_codex("start-fixture", "startup-io")
        } else {
            host.start_codex("project", "task", "startup-io", &revision)
        };
        assert!(result.is_err());
        assert!(
            host.startups.get("project", "startup-io").is_some(),
            "intent must precede mutable job IO"
        );
        assert!(host.codex_jobs.is_empty() && host.codex_active.is_empty());
        assert_eq!(host.projects["project"].history().len(), 2);
        assert!(
            host.start_codex("project", "task", "different-id", &revision)
                .is_err(),
            "orphan intent must be reviewed first"
        );
        fs::remove_file(obstruction).unwrap();
        drop(host);
        let mut host = RuntimeHost::open(&dir.path().join("data")).unwrap();
        let detail = host.startup_detail("project", "startup-io").unwrap();
        host.abandon_codex_startup(
            "project",
            "startup-io",
            detail.evidence_digest.as_deref().unwrap(),
            "Close after verified job write failure",
        )
        .unwrap();
        assert!(
            host.codex_jobs.is_empty(),
            "closing missing job never manufactures it"
        );
        assert_eq!(host.projects["project"].history().len(), 2);
        assert!(host
            .start_codex("project", "task", "startup-io", &revision)
            .unwrap_err()
            .contains("永久保留"));
        if managed {
            assert_eq!(
                host.management
                    .statuses()
                    .into_iter()
                    .find(|s| s.operation_id == "start-fixture")
                    .unwrap()
                    .state,
                DecisionState::StartupClosed
            );
            assert!(host
                .start_managed_codex("start-fixture", "fresh-id")
                .is_err());
        }
        drop(host);
        let host = RuntimeHost::open(&dir.path().join("data")).unwrap();
        assert_eq!(
            host.startup_detail("project", "startup-io")
                .unwrap()
                .summary
                .state,
            "closed"
        );
        assert_eq!(host.projects["project"].history().len(), 2);
    }
}
