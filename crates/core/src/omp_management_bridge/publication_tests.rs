use super::*;
use std::collections::BTreeMap;

fn events(data: &Path) -> BTreeMap<String, Vec<u8>> {
    fs::read_dir(data.join("workflows/project/events"))
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            (
                e.file_name().into_string().unwrap(),
                fs::read(e.path()).unwrap(),
            )
        })
        .collect()
}

pub(super) fn assigned(host: &mut RuntimeHost) {
    decision(host);
    let (thread, completed) = simulated(host);
    let Assignment::Prepare {
        operation,
        attempt,
        snapshot,
        reply,
    } = next(host)
    else {
        panic!("prepare")
    };
    host.prepare_managed_assignment("project", &operation, *attempt, *snapshot, "omp")
        .unwrap();
    reply.send(Ok(())).unwrap();
    let Assignment::Commit {
        operation,
        entry,
        snapshot,
        reply,
    } = next(host)
    else {
        panic!("commit")
    };
    host.commit_managed_assignment("project", &operation, *entry, *snapshot, "omp")
        .unwrap();
    reply.send(Ok(())).unwrap();
    drain(host, thread);
    assert_eq!(completed.load(Ordering::SeqCst), 1);
}

#[test]
fn management_complete_pending_recovers_after_full_host_validation_without_dispatch() {
    for sequence in 0..=4 {
        for duplicate in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let mut host = fixture(root.path(), None);
            if sequence == 4 {
                decision(&mut host);
                reserve(&mut host);
                let review = host.snapshot().omp_jobs[0].closure_digest.clone().unwrap();
                host.abandon_omp_startup("project", "attempt", &review, "fixture user close")
                    .unwrap();
            } else {
                assigned(&mut host);
            }
            let data = host.root.clone();
            let target = data.join(format!("management-decisions/deputy-omp.{sequence}.json"));
            let pending = target.with_extension("pending");
            let original = fs::read(&target).unwrap();
            let ledger = events(&data);
            drop(host);
            if duplicate {
                fs::copy(&target, &pending).unwrap();
            } else {
                fs::rename(&target, &pending).unwrap();
            }
            // Journal loading alone must not publish anything.
            assert!(crate::management_execution::DecisionJournal::open(&data).is_ok());
            assert!(pending.exists());
            assert_eq!(target.exists(), duplicate);
            for _ in 0..2 {
                let mut host = RuntimeHost::open(&data).unwrap();
                host.poll_verifications();
                host.poll_project_runs();
                assert!(host.omp_active.is_empty() && host.management_active.is_empty());
                assert_eq!(
                    host.projects["project"].state().tasks["task"].state,
                    if sequence == 4 {
                        TaskState::Pending
                    } else {
                        TaskState::Assigned
                    }
                );
                assert!(!host.projects["project"].history().iter().any(|e| matches!(
                    e.body.event.command,
                    WorkflowCommand::DispatchIntent { .. }
                )));
                assert!(host.start_managed_omp("deputy-omp", "new-attempt").is_err());
                assert_eq!(events(&data), ledger);
                assert_eq!(fs::read(&target).unwrap(), original);
                assert!(!pending.exists());
            }
        }
    }
}

#[test]
fn a_locally_valid_pending_receipt_cannot_bypass_the_real_workflow_event() {
    let root = tempfile::tempdir().unwrap();
    let mut host = fixture(root.path(), None);
    decision(&mut host);
    let (thread, completed) = simulated(&mut host);
    let Assignment::Prepare {
        operation,
        attempt,
        snapshot,
        reply,
    } = next(&host)
    else {
        panic!("prepare")
    };
    host.prepare_managed_assignment("project", &operation, *attempt, *snapshot, "omp")
        .unwrap();
    reply.send(Ok(())).unwrap();
    let Assignment::Commit { reply, .. } = next(&host) else {
        panic!("commit")
    };
    // The journal can validate a hash's format, but only the host can establish
    // that it identifies the actual immutable assignment event.
    host.management
        .record_applied("deputy-omp", Some("f".repeat(64)))
        .unwrap();
    drop(reply);
    drain(&mut host, thread);
    assert_eq!(completed.load(Ordering::SeqCst), 0);
    let data = host.root.clone();
    let target = data.join("management-decisions/deputy-omp.3.json");
    let pending = target.with_extension("pending");
    let ledger = events(&data);
    let raw = fs::read(&target).unwrap();
    drop(host);
    fs::rename(&target, &pending).unwrap();
    assert!(crate::management_execution::DecisionJournal::open(&data).is_ok());
    assert!(RuntimeHost::open(&data)
        .err()
        .unwrap()
        .contains("receipt differs from workflow evidence"));
    assert_eq!(fs::read(&pending).unwrap(), raw);
    assert!(!target.exists());
    assert_eq!(events(&data), ledger);
}
