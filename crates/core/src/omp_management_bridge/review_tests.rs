use super::*;
use crate::management_execution::{DecisionState, UserResolution};
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
fn broken_stage(root: &Path, stage: u8, empty: bool) -> (PathBuf, String, Vec<u8>) {
    let mut host = fixture(root, None);
    if stage == 3 {
        super::publication_tests::assigned(&mut host);
    } else {
        decision(&mut host);
    }
    if stage == 2 {
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
        drop(reply);
        drain(&mut host, thread);
        assert_eq!(completed.load(Ordering::SeqCst), 0);
    }
    if stage == 4 {
        let head = host
            .management
            .evidence_digest("deputy-omp")
            .unwrap()
            .to_owned();
        host.resolve_management(
            "deputy-omp",
            &head,
            UserResolution::Discard {
                reason: "initial fixture user decision".into(),
            },
        )
        .unwrap();
    }
    let data = host.root.clone();
    drop(host);
    if stage == 0 {
        fs::remove_file(data.join("management-decisions/deputy-omp.1.json")).unwrap();
    }
    let target = data.join(format!("management-decisions/deputy-omp.{stage}.json"));
    let original = fs::read(&target).unwrap();
    let partial = if empty {
        vec![]
    } else {
        original[..original.len() / 2].to_vec()
    };
    let relative = format!("management-decisions/deputy-omp.{stage}.pending");
    fs::rename(target, data.join(&relative)).unwrap();
    fs::write(data.join(&relative), &partial).unwrap();
    (data, relative, partial)
}
#[test]
fn reviewed_management_stages_reopen_without_resend_and_require_real_followup() {
    for stage in 0..=4 {
        for empty in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let (data, relative, raw) = broken_stage(root.path(), stage, empty);
            let ledger = events(&data);
            let mut service = crate::diagnostic_service::ServiceHost::open(&data).unwrap();
            assert!(service.is_diagnostic());
            let evidence = crate::management_review::inspect(&data, &relative)
                .unwrap()
                .0;
            assert!(service
                .handle(Request {
                    id: "review".into(),
                    protocol_version: PROTOCOL_VERSION,
                    command: Command::ReviewManagementRecord {
                        relative_path: relative.clone(),
                        evidence_digest: evidence,
                        reason: "explicit fixture user review".into()
                    }
                })
                .error
                .is_none());
            assert!(service.is_diagnostic());
            drop(service);
            let mut host = RuntimeHost::open(&data).unwrap();
            host.poll_verifications();
            host.poll_project_runs();
            assert!(
                host.omp_active.is_empty()
                    && host.codex_active.is_empty()
                    && host.management_active.is_empty()
            );
            assert_eq!(host.snapshot().management_record_reviews.len(), 1);
            assert_eq!(
                host.projects["project"].state().tasks["task"].state,
                if stage == 3 {
                    TaskState::Assigned
                } else {
                    TaskState::Pending
                }
            );
            assert_eq!(events(&data), ledger);
            assert!(host
                .start_managed_omp("deputy-omp", "fresh-attempt")
                .is_err());
            match stage {
                0 => assert!(host.management.statuses().is_empty()),
                1 | 4 => {
                    assert_eq!(
                        host.management.statuses()[0].state,
                        if stage == 1 {
                            DecisionState::Unknown
                        } else {
                            DecisionState::Ready
                        }
                    );
                    let head = host
                        .management
                        .evidence_digest("deputy-omp")
                        .unwrap()
                        .to_owned();
                    host.resolve_management(
                        "deputy-omp",
                        &head,
                        UserResolution::Discard {
                            reason: "separate user confirmation".into(),
                        },
                    )
                    .unwrap();
                }
                2 => {
                    assert_eq!(host.management.statuses()[0].state, DecisionState::Ready);
                    let review = host.snapshot().omp_jobs[0].closure_digest.clone().unwrap();
                    host.abandon_omp_startup(
                        "project",
                        "attempt",
                        &review,
                        "separate user startup closure",
                    )
                    .unwrap();
                }
                3 => assert_eq!(host.management.statuses()[0].state, DecisionState::Applied),
                _ => unreachable!(),
            }
            drop(host);
            let mut host = RuntimeHost::open(&data).unwrap();
            host.poll_verifications();
            host.poll_project_runs();
            assert_eq!(fs::read(data.join(&relative)).unwrap(), raw);
            assert_eq!(events(&data), ledger);
            assert!(host.omp_active.is_empty() && host.management_active.is_empty());
        }
    }
}

#[test]
fn reviewing_partial_preparation_cannot_hide_an_existing_assignment() {
    let root = tempfile::tempdir().unwrap();
    let mut host = fixture(root.path(), None);
    super::publication_tests::assigned(&mut host);
    let data = host.root.clone();
    drop(host);
    // Preserve a real Assign, but truncate its application evidence. The
    // locally valid earlier prefix must not authorize dropping the assignment.
    fs::remove_file(data.join("management-decisions/deputy-omp.3.json")).unwrap();
    let target = data.join("management-decisions/deputy-omp.2.json");
    let raw = fs::read(&target).unwrap();
    let relative = "management-decisions/deputy-omp.2.pending";
    fs::rename(target, data.join(relative)).unwrap();
    fs::write(data.join(relative), &raw[..raw.len() / 2]).unwrap();
    let ledger = events(&data);
    let evidence = crate::management_review::inspect(&data, relative)
        .unwrap()
        .0;
    crate::management_review::acknowledge(
        &data,
        relative,
        &evidence,
        "review cannot erase Assign",
        |_| Ok(()),
    )
    .unwrap();
    assert!(crate::management_execution::DecisionJournal::open(&data).is_ok());
    assert!(RuntimeHost::open(&data).is_err());
    assert_eq!(events(&data), ledger);
    assert_eq!(fs::read(data.join(relative)).unwrap(), raw[..raw.len() / 2]);
}

#[test]
#[ignore = "isolated native management review fixture; no provider"]
fn native_management_review_fixture() {
    let root = PathBuf::from(std::env::var_os("CERULEAN_MANAGEMENT_REVIEW_DIR").unwrap())
        .canonicalize()
        .unwrap();
    assert!(!root.join("data").exists());
    let stage: u8 = std::env::var("CERULEAN_MANAGEMENT_REVIEW_STAGE")
        .unwrap()
        .parse()
        .unwrap();
    assert!(stage <= 4);
    let (data, relative, _) = broken_stage(&root, stage, false);
    fs::write(root.join("fixture.json"),serde_json::to_vec_pretty(&serde_json::json!({"data":data,"relativePath":relative,"stage":stage,"operation":"deputy-omp","realModelCalls":0})).unwrap()).unwrap();
}
