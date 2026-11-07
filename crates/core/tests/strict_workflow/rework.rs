use super::*;

#[test]
fn rework_binds_failed_check_and_preserves_history_on_reopen() {
    for missing in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (mut store, plan) = proposed(root.path());
        approve(&mut store, &plan);
        let attempt = assign(&mut store, &plan);
        dispatch(&mut store, &plan);
        apply(
            &mut store,
            "core",
            WorkflowCommand::Started {
                task: "task".into(),
                attempt: attempt.id.clone(),
            },
            ctx(receipt(&attempt, ReceiptStatus::Running), None),
        )
        .unwrap();
        if missing {
            fs::remove_file(root.path().join("project/result.txt")).unwrap();
        }
        let manifest = submit(&mut store, &plan, &attempt);
        assert_eq!(manifest.artifacts.is_empty(), missing);
        let mut result = report(&plan, &manifest);
        if !missing {
            result.checks[0].exit_code = Some(2);
        }
        let report_digest = digest(&result).unwrap();
        let returned = |operation: &str, hash: &str| WorkflowCommand::ReturnForRework {
            task: "task".into(),
            attempt: attempt.id.clone(),
            verification_operation: operation.into(),
            report_digest: hash.into(),
            suggestion: "修正原任务输出，并保留批准的检查标准".into(),
        };
        assert!(apply(
            &mut store,
            "deputy",
            returned("missing", &report_digest),
            CommandContext::default()
        )
        .is_err());
        let checked = apply(
            &mut store,
            "core",
            WorkflowCommand::Verify {
                task: "task".into(),
                attempt: attempt.id.clone(),
                passed: false,
                evidence: vec![report_digest.clone()],
            },
            CommandContext {
                execution: Some(ExecutionEvidence::Verification { report: result }),
                ..Default::default()
            },
        )
        .unwrap();
        let operation = &checked.body.operation_id;
        assert!(apply(
            &mut store,
            "worker-codex",
            returned(operation, &report_digest),
            CommandContext::default()
        )
        .is_err());
        assert!(apply(
            &mut store,
            "deputy",
            returned("wrong-check", &report_digest),
            CommandContext::default()
        )
        .is_err());
        assert!(apply(
            &mut store,
            "deputy",
            returned(operation, &"0".repeat(64)),
            CommandContext::default()
        )
        .is_err());
        apply(
            &mut store,
            "deputy",
            returned(operation, &report_digest),
            CommandContext::default(),
        )
        .unwrap();
        assert_eq!(store.state().tasks["task"].state, TaskState::Pending);
        assert_eq!(store.state().tasks["task"].fence, 1);
        assert!(store.attempt_record("task", &attempt.id).is_some());
        assert!(apply(
            &mut store,
            "deputy",
            returned(operation, &report_digest),
            CommandContext::default()
        )
        .is_err());
        let history = serde_json::to_vec(store.history()).unwrap();
        drop(store);
        let mut reopened =
            DurableWorkflow::open(&root.path().join("ledger"), config(&plan)).unwrap();
        assert_eq!(serde_json::to_vec(reopened.history()).unwrap(), history);
        let mut next = attempt.clone();
        next.id = "attempt-rework".into();
        next.fence = 2;
        apply(
            &mut reopened,
            "deputy",
            WorkflowCommand::Assign {
                task: "task".into(),
                worker: next.worker_id.clone(),
                attempt: next.id.clone(),
            },
            CommandContext {
                execution: Some(ExecutionEvidence::Attempt { attempt: next }),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(reopened.state().tasks["task"].fence, 2);
        assert!(!reopened.state().tasks["task"].verified);
    }
}
