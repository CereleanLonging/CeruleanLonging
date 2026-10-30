//! Actual Git worktrees, native Codex fixture and independent verifier.
//! Provider replies are controlled; this is not real-model acceptance evidence.
use super::*;
use crate::management_execution::DecisionState;
use cerulean_domain::management_ingress::ExternalChiefDecision;

#[test]
#[ignore = "native offline mixed DAG, failed terminal and frozen external Accept; explicit fixture paths"]
fn preserved_shutdown_mixed_failed_codex_keeps_verified_api_and_external_accept() {
    use ProjectWorkerAdapter::{Codex, Deepseek};
    // Keep this directory even on assertion failure for diagnosis.
    let root = PathBuf::from(
        std::env::var_os("CERULEAN_PRESERVE_MIXED_ROOT").expect("new evidence directory"),
    );
    assert!(!root.exists());
    fs::create_dir(&root).unwrap();
    let runtime = root.join("unused-omp-parent");
    fs::create_dir(&runtime).unwrap();
    let mut host = mixed::mixed_host_external(&root, &runtime, [Deepseek, Codex, Deepseek]);
    fs::write(root.join("bin/gate-codex"), "controlled fixture").unwrap();
    fs::write(root.join("bin/fail-b"), "controlled failed terminal").unwrap();
    host.api_fixture = Some(parallel::gated);
    let (entered, release) = parallel::gate("a");
    let setup = host.project_setups["project"].digest.clone();
    host.request_chief_plan("project", &setup).unwrap();
    let plan_id = host.management_decisions()[0].operation_id.clone();
    let request = host.read_external_chief_request(&plan_id).unwrap();
    let intent = host.management.intent(&plan_id).unwrap().clone();
    host.submit_external_chief_decision(
        &plan_id,
        "current-session",
        ExternalChiefDecision {
            request_digest: digest(&request).unwrap(),
            session_id: "current-session".into(),
            envelope: mixed::mixed_manager(intent).unwrap().envelope,
        },
    )
    .unwrap();
    let revision = host.projects["project"]
        .revision()
        .unwrap()
        .approval_digest()
        .unwrap();
    host.approve("project", &revision).unwrap();
    host.start_project_run("project", &revision, &setup)
        .unwrap();
    spin(&mut host, |h| {
        assert_eq!(
            h.runs.all()[0].state,
            RunState::Active,
            "API startup: {:?}",
            h.snapshot().api_jobs
        );
        entered.try_recv().is_ok()
    });
    spin(&mut host, |h| {
        assert_eq!(
            h.runs.all()[0].state,
            RunState::Active,
            "mixed startup: {:?}; {:?}",
            h.snapshot().api_jobs,
            h.snapshot().codex_jobs
        );
        codex_parallel::all_running(h, &root, &[Deepseek, Codex, Deepseek])
    });
    assert!(
        host.preserved_shutdown_evidence().is_err(),
        "live workers must block review"
    );
    release.send(()).unwrap();
    let accept = external_accept::waiting(&mut host);
    assert_eq!(accept.purpose, Purpose::Accept);
    assert!(
        host.preserved_shutdown_evidence().is_err(),
        "Codex still owns the ledger"
    );
    fs::write(
        root.join("bin/release-b"),
        "release the one original terminal",
    )
    .unwrap();
    spin(&mut host, |h| {
        h.codex_active.is_empty()
            && h.api_active.is_empty()
            && h.api_writers.is_empty()
            && h.active.is_empty()
            && h.runs.all()[0].state == RunState::Attention
    });
    let store = &host.projects["project"];
    assert_eq!(store.state().tasks["a"].state, TaskState::Submitted);
    assert!(store.state().tasks["a"].verified);
    assert_eq!(store.state().tasks["b"].state, TaskState::Unknown);
    assert_eq!(store.state().tasks["integration"].state, TaskState::Pending);
    assert!(store.state().tasks["integration"].attempt.is_none());
    let history = store.history().to_vec();
    for task in ["a", "b", "integration"] {
        assert_eq!(
            history
                .iter()
                .filter(|e| matches!(&e.body.event.command,
            WorkflowCommand::DispatchIntent {task: t, ..} if t == task))
                .count(),
            usize::from(task != "integration")
        );
    }
    assert!(host.management.statuses().iter().any(
        |s| s.operation_id == accept.operation_id && s.state == DecisionState::AwaitingExternal
    ));
    assert!(
        host.read_external_chief_request(&accept.operation_id)
            .is_err(),
        "Unknown must keep Accept frozen"
    );
    assert!(!host.can_shutdown());
    let (snapshot, proof) = host.preserved_shutdown_evidence().unwrap();
    let state_digest = digest(&snapshot).unwrap();
    let management_count = host.management_operation_total();
    let requests = crate::api_execution::prior_requests(&host.projects["project"]).unwrap();
    assert_eq!(
        requests, 4,
        "three API rounds plus one Codex dispatch in the shared request budget"
    );
    let a_path = dependency_host_fixture::task_path(&host, "a");
    let b_path = dependency_host_fixture::task_path(&host, "b");
    assert_eq!(fs::read_to_string(a_path.join("a.txt")).unwrap(), "A");
    assert_eq!(fs::read_to_string(b_path.join("b.txt")).unwrap(), "B");
    crate::derived_index::wait_for_test_projections(&root.join("data"));
    drop(host);
    let cold = crate::reader_generation::tests::snapshot(&root.join("data"));
    let pool = crate::reader_generation::tests::snapshot(&root.join("pool"));
    let peer = crate::reader_generation::tests::snapshot(&root.join("bin"));
    for round in 0..2 {
        let mut reopened = RuntimeHost::open(&root.join("data")).unwrap();
        // No fixture configured: an unexpected provider send fails the state/byte assertions.
        for _ in 0..5 {
            reopened.poll_verifications();
        }
        assert_eq!(reopened.projects["project"].history(), history);
        assert_eq!(reopened.management_operation_total(), management_count);
        assert_eq!(
            crate::api_execution::prior_requests(&reopened.projects["project"]).unwrap(),
            requests
        );
        let (again, digest_again) = reopened.preserved_shutdown_evidence().unwrap();
        fs::write(
            root.join(format!("snapshot-{round}-before.json")),
            serde_json::to_vec_pretty(&snapshot).unwrap(),
        )
        .unwrap();
        fs::write(
            root.join(format!("snapshot-{round}-after.json")),
            serde_json::to_vec_pretty(&again).unwrap(),
        )
        .unwrap();
        assert_eq!(digest_again, proof);
        assert_eq!(digest(&again).unwrap(), state_digest);
        assert!(!reopened.can_shutdown());
        assert!(reopened
            .read_external_chief_request(&accept.operation_id)
            .is_err());
        drop(reopened);
        assert_eq!(
            crate::reader_generation::tests::snapshot(&root.join("data")),
            cold
        );
        assert_eq!(
            crate::reader_generation::tests::snapshot(&root.join("pool")),
            pool
        );
        assert_eq!(
            crate::reader_generation::tests::snapshot(&root.join("bin")),
            peer
        );
    }
    println!("mixed preservation: API Submitted+verified, Codex failed Unknown with edit, integration Pending, external Accept frozen, 2 cold reopens, 0 redispatch, 0 real model calls");
}
