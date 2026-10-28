use super::*;
#[path = "desktop_stop_review_crash_tests.rs"]
mod crash;
use crate::desktop_execution::{
    self as execution,
    cancellation::{self, review, StopObservation, StopProof, Transport},
};

struct Wire;
impl Transport for Wire {
    fn prepare(&mut self, r: &execution::TaskReview) -> Result<StopObservation, String> {
        serde_json::from_value(cancellation_tests::running_observation(r))
            .map_err(|e| e.to_string())
    }
    fn stop(
        &mut self,
        _: &execution::TaskReview,
        o: &StopObservation,
    ) -> Result<StopProof, String> {
        serde_json::from_value(cancellation_tests::cancelled_proof(
            &serde_json::to_value(o).unwrap(),
        ))
        .map_err(|e| e.to_string())
    }
    fn read(
        &mut self,
        _: &execution::TaskReview,
        _: &StopObservation,
    ) -> Result<StopProof, String> {
        panic!("review must never perform transport")
    }
    fn completed(&mut self, _: &execution::TaskReview) -> Result<execution::TaskProof, String> {
        panic!("review must never read completion")
    }
}
fn fixture(root: &Path, stage: &str, boundary: &str) -> (PathBuf, execution::TaskReview, String) {
    let (mut host, r) = cancellation_tests::unknown(root);
    let data = host.root.clone();
    if stage == "stop-request" {
        assert!(
            cancellation::request_observed(&data, &r, "stop original", |name, point| {
                if name == stage && point == boundary {
                    Err("injected write interruption".into())
                } else {
                    Ok(())
                }
            })
            .is_err()
        );
    } else {
        let permit = cancellation::request(&data, &r, "stop original").unwrap();
        assert!(cancellation::run_observed(
            &data,
            host.projects.get_mut("project").unwrap(),
            &r,
            Some(permit),
            &mut Wire,
            |name, point| {
                if name == stage && point == boundary {
                    Err("injected write interruption".into())
                } else {
                    Ok(())
                }
            }
        )
        .is_err());
    }
    let rel = format!(
        "desktop-task-dispatch-v1/{}/{stage}.pending",
        digest(&(&r.project_id, &r.attempt.id)).unwrap()
    );
    drop(host);
    assert!(RuntimeHost::open(&data).is_err());
    (data, r, rel)
}
fn tree(path: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    for e in fs::read_dir(path).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(tree(&p));
        } else {
            out.insert(p.clone(), fs::read(p).unwrap());
        }
    }
    out
}
fn check_tree(before: &BTreeMap<PathBuf, Vec<u8>>) {
    for (p, b) in before {
        assert_eq!(&fs::read(p).unwrap(), b, "{}", p.display());
    }
}
fn command(relative: &str, evidence: &str) -> Command {
    Command::ReviewDesktopStopRecord {
        relative_path: relative.into(),
        evidence_digest: evidence.into(),
        reason: "核对原停止残片，仅恢复本地管理".into(),
    }
}
#[test]
fn desktop_stop_review_three_stages_preserve_originals_and_only_allow_local_retirement() {
    for stage in ["stop-request", "stop-intent", "stop-proof"] {
        for boundary in ["temp-created", "partial-written"] {
            let tmp = tempfile::tempdir().unwrap();
            let (data, r, rel) = fixture(tmp.path(), stage, boundary);
            let original = tree(&data.join("desktop-task-dispatch-v1"));
            let journals = tree(&data.join("workflows"));
            let mut service = crate::diagnostic_service::ServiceHost::open(&data).unwrap();
            let snapshot = service.snapshot();
            let file = snapshot
                .core_diagnostic
                .unwrap()
                .desktop_files
                .into_iter()
                .find(|f| f.relative_path == rel)
                .unwrap();
            assert_eq!(file.review_kind.as_deref(), Some("desktop-stop"));
            let evidence = file.review_digest.unwrap();
            assert!(service
                .handle(Request {
                    id: "stale".into(),
                    protocol_version: PROTOCOL_VERSION,
                    command: command(&rel, &"0".repeat(64))
                })
                .error
                .is_some());
            assert!(service
                .handle(Request {
                    id: "review".into(),
                    protocol_version: PROTOCOL_VERSION,
                    command: command(&rel, &evidence)
                })
                .error
                .is_none());
            assert!(
                service
                    .snapshot()
                    .core_diagnostic
                    .unwrap()
                    .desktop_files
                    .iter()
                    .find(|f| f.relative_path == rel)
                    .unwrap()
                    .reviewed
            );
            check_tree(&journals); // Review does not recover workflow events or rewrite projections.
            check_tree(&original);
            drop(service);
            for _ in 0..2 {
                let mut host = RuntimeHost::open(&data).unwrap();
                assert_eq!(
                    host.projects["project"].state().tasks["task"].state,
                    TaskState::Unknown
                );
                assert!(!host.projects["project"].state().tasks["task"].verified);
                assert_eq!(
                    host.desktop_access.tasks[0].reviewed_stop_record.as_deref(),
                    Some(stage)
                );
                assert!(host.read_desktop_task("project", &r.attempt.id).is_err());
                assert!(host
                    .read_desktop_task_with_reader("project", &r.attempt.id, Some(&fingerprint()))
                    .is_err());
                assert!(host
                    .cancel_desktop_task("project", &r.attempt.id, &r.id, "retry forbidden")
                    .is_err());
                assert!(host.require_desktop_tasks_resolved().is_err());
                assert!(execution::recover_and_submit(
                    &data,
                    host.projects.get_mut("project").unwrap(),
                    "project",
                    &r.attempt.id,
                    |_| panic!("no transport")
                )
                .is_err());
                assert!(cancellation::run(
                    &data,
                    host.projects.get_mut("project").unwrap(),
                    &r,
                    None,
                    &mut Wire
                )
                .is_err());
                check_tree(&original);
            }
            let mut host = RuntimeHost::open(&data).unwrap();
            let proof = host
                .desktop_retirement_digest("project", &r.attempt.id)
                .unwrap();
            host.retire_unknown_desktop(
                "project",
                &r.attempt.id,
                &proof,
                "外部应用已自行处理，仅结束本地跟踪",
            )
            .unwrap();
            assert_eq!(
                host.projects["project"].state().tasks["task"].state,
                TaskState::Retired
            );
            assert!(host.require_desktop_tasks_resolved().is_ok());
            drop(host);
            let host = RuntimeHost::open(&data).unwrap();
            assert!(host.desktop_access.tasks[0].retired);
            let commands: Vec<_> = host.projects["project"]
                .history()
                .iter()
                .map(|e| &e.body.event.command)
                .collect();
            assert_eq!(
                commands
                    .iter()
                    .filter(|c| matches!(c, WorkflowCommand::DispatchIntent { .. }))
                    .count(),
                1
            );
            assert!(!commands.iter().any(|c| matches!(
                c,
                WorkflowCommand::Submit { .. }
                    | WorkflowCommand::Verify { .. }
                    | WorkflowCommand::Accept { .. }
                    | WorkflowCommand::Reconcile {
                        outcome: ReconcileOutcome::Cancelled,
                        ..
                    }
            )));
            check_tree(&original);
        }
    }
}
#[test]
fn desktop_stop_review_rejects_changed_evidence_conflicts_and_forged_receipts() {
    for scenario in 0..10 {
        let tmp = tempfile::tempdir().unwrap();
        let (data, r, rel) = fixture(tmp.path(), "stop-proof", "partial-written");
        let (evidence, _) = review::inspect(&data, &rel).unwrap();
        let pending = data.join(&rel);
        let dir = pending.parent().unwrap();
        match scenario {
            0 => fs::write(&pending, b"{\"changed\":").unwrap(),
            1 => fs::copy(&pending, dir.join("stop-proof.json"))
                .map(|_| ())
                .unwrap(),
            2 => fs::write(&pending, b"{}").unwrap(),
            3 => fs::write(&pending, b"not json").unwrap(),
            4 => fs::write(dir.join("completion.pending"), b"{").unwrap(),
            5 => {
                fs::remove_file(dir.join("stop-request.json")).unwrap();
            }
            6 => {
                fs::write(dir.join("foreign.pending"), b"{").unwrap();
            }
            7 => {
                fs::write(&pending, vec![b' '; 131073]).unwrap();
            }
            8 | 9 => {
                review::acknowledge(&data, &rel, &evidence, "review", |_| Ok(())).unwrap();
                if scenario == 8 {
                    let file = fs::read_dir(data.join("desktop-stop-reviews-v1"))
                        .unwrap()
                        .next()
                        .unwrap()
                        .unwrap()
                        .path();
                    let mut value: serde_json::Value =
                        serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
                    value["actor"] = "deputy".into();
                    fs::write(file, serde_json::to_vec(&value).unwrap()).unwrap();
                } else {
                    fs::remove_file(&pending).unwrap();
                }
            }
            _ => unreachable!(),
        }
        assert!(
            review::acknowledge(&data, &rel, &evidence, "review", |_| Ok(())).is_err(),
            "scenario {scenario}"
        );
        assert!(RuntimeHost::open(&data).is_err(), "scenario {scenario}");
        assert!(review::inspect(
            &data,
            &rel.replace("stop-proof.pending", "../stop-proof.pending")
        )
        .is_err());
        assert!(review::inspect(
            &data,
            &rel.replace(
                &digest(&(&r.project_id, &r.attempt.id)).unwrap(),
                &"b".repeat(64)
            )
        )
        .is_err());
    }
}
#[test]
fn desktop_stop_review_receipt_publication_retries_preserve_fragments_without_granting_actions() {
    for stage in ["stop-request", "stop-intent", "stop-proof"] {
        for point in ["created", "partial", "synced", "published", "finished"] {
            let tmp = tempfile::tempdir().unwrap();
            let (data, _, rel) = fixture(tmp.path(), stage, "partial-written");
            let originals = tree(&data.join("desktop-task-dispatch-v1"));
            let (evidence, _) = review::inspect(&data, &rel).unwrap();
            assert!(
                review::acknowledge(&data, &rel, &evidence, "review", |p| if p == point {
                    Err("injected receipt failure".into())
                } else {
                    Ok(())
                })
                .is_err()
            );
            let receipts = tree(&data.join("desktop-stop-reviews-v1"));
            if matches!(point, "created" | "partial" | "synced") {
                assert!(RuntimeHost::open(&data).is_err());
            }
            review::acknowledge(&data, &rel, &evidence, "retry review", |_| Ok(())).unwrap();
            check_tree(&receipts);
            check_tree(&originals);
            for _ in 0..2 {
                let host = RuntimeHost::open(&data).unwrap();
                assert_eq!(
                    host.projects["project"].state().tasks["task"].state,
                    TaskState::Unknown
                );
                assert!(host.desktop_access.tasks[0].reviewed_stop_record.is_some());
            }
        }
    }
}

#[test]
#[ignore = "native review fixture preparation; requires a fresh owned report directory"]
fn native_desktop_stop_review_seed() {
    assert_eq!(
        std::env::var("CERULEAN_NATIVE_OWNED_JOB").as_deref(),
        Ok("1")
    );
    let root = PathBuf::from(std::env::var_os("CERULEAN_DESKTOP_STOP_REVIEW_DIR").unwrap());
    assert!(root.is_dir());
    assert!(!root.join("fixtures.json").exists());
    let mut cases = vec![];
    for stage in ["stop-request", "stop-intent", "stop-proof"] {
        let dir = root.join(stage);
        fs::create_dir(&dir).unwrap();
        let (data, r, relative) = fixture(&dir, stage, "partial-written");
        cases.push(serde_json::json!({"stage":stage,"data":data,"relativePath":relative,"reviewDigest":r.id,"attemptId":r.attempt.id}));
    }
    fs::write(
        root.join("fixtures.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}
