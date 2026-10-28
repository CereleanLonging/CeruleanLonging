use super::*;

fn accepted() -> (Flow, Attempt) {
    let mut f = Flow::with_dependency(true);
    assert!(f.store.accepted_dependency_inputs("downstream").is_err());
    f.approve();
    assert!(f.store.accepted_dependency_inputs("downstream").is_err());
    assert!(f
        .store
        .accepted_dependency_inputs("task")
        .unwrap()
        .is_empty());
    let a = f.prepare();
    f.assign(&a).unwrap();
    f.dispatch(&a).unwrap();
    apply(
        &mut f.store,
        "core",
        WorkflowCommand::Started {
            task: "task".into(),
            attempt: a.id.clone(),
        },
        context(receipt(&a, ReceiptStatus::Running), None),
    )
    .unwrap();
    fs::write(
        Path::new(&a.binding.workspace).join("b.txt"),
        b"worker result\n",
    )
    .unwrap();
    let r = receipt(&a, ReceiptStatus::Completed);
    let manifest = ArtifactObserver::new(
        &f.plan,
        f.store.approval().unwrap(),
        "user",
        1,
        &a,
        &r,
        Some("turn"),
    )
    .unwrap()
    .capture()
    .unwrap();
    apply(
        &mut f.store,
        "worker",
        WorkflowCommand::Submit {
            task: "task".into(),
            attempt: a.id.clone(),
            summary: "sealed input fixture".into(),
            evidence: None,
        },
        context(r, Some(manifest)),
    )
    .unwrap();
    assert!(f.store.accepted_dependency_inputs("downstream").is_err());
    let pins = f.plan.execution[0]
        .checks
        .iter()
        .map(PinnedCheck::from_approved)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    f.store
        .run_verification("input-check", "task", &a.id, &pins, &AtomicBool::new(false))
        .unwrap();
    assert!(f.store.accepted_dependency_inputs("downstream").is_err());
    apply(
        &mut f.store,
        "chief",
        WorkflowCommand::Accept {
            task: "task".into(),
            attempt: a.id.clone(),
        },
        CommandContext::default(),
    )
    .unwrap();
    (f, a)
}

#[test]
#[ignore = "real Git, independent verifier, sealed accepted dependency input locks"]
fn accepted_inputs_bind_history_and_ignore_mutable_worker_and_check_directories() {
    let (f, a) = accepted();
    let mut inputs = f.store.accepted_dependency_inputs("downstream").unwrap();
    assert_eq!(inputs.len(), 1);
    let binding = inputs[0].binding().clone();
    assert_eq!(binding.attempt_id, a.id);
    assert_eq!(binding.revision_digest, f.plan.approval_digest().unwrap());
    assert_eq!(inputs[0].read_file("b.txt").unwrap(), b"worker result\n");
    assert_eq!(
        inputs[0].read_file("src/a.mjs").unwrap(),
        b"export const a = 1;\n"
    );
    assert_eq!(inputs[0].read_file("b.txt").unwrap(), b"worker result\n");
    assert!(inputs[0].read_file("../b.txt").is_err());
    assert!(inputs[0].read_file(".git").is_err());
    let sealed = f.journal.join("verification/input-check/inputs/b.txt");
    assert!(fs::write(&sealed, b"tamper").is_err());
    assert!(fs::remove_file(&sealed).is_err());
    assert!(fs::write(
        f.journal.join("verification/input-check/snapshot.json"),
        b"{}"
    )
    .is_err());
    fs::write(
        Path::new(&a.binding.workspace).join("b.txt"),
        b"later unaccepted edit",
    )
    .unwrap();
    fs::write(
        f.journal.join("verification/input-check/work/b.txt"),
        b"later check scratch edit",
    )
    .unwrap();
    assert_eq!(inputs[0].read_file("b.txt").unwrap(), b"worker result\n");
    drop(inputs);
    let count = f.store.history().len();
    drop(f.store);
    let reopened = DurableWorkflow::open(&f.journal, f.config).unwrap();
    let mut inputs = reopened.accepted_dependency_inputs("downstream").unwrap();
    assert_eq!(inputs[0].binding(), &binding);
    assert_eq!(inputs[0].read_file("b.txt").unwrap(), b"worker result\n");
    assert_eq!(reopened.history().len(), count);
    let (_, manifest) = reopened.completed_artifacts("task", &a.id).unwrap();
    assert!(crate::source_snapshot::require_sealed(
        &f.plan,
        manifest,
        &binding.snapshot,
        &f.journal.join("verification")
    )
    .is_err());
    assert!(reopened
        .prepare_task_workspace("downstream", "next-attempt")
        .is_err());
}

#[test]
#[ignore = "real Git and verifier; sealed payload/manifest tamper and missing inputs rejected"]
fn accepted_inputs_refuse_corruption_missing_extra_and_foreign_snapshot() {
    let (f, _) = accepted();
    let input = f.journal.join("verification/input-check/inputs/b.txt");
    let original = fs::read(&input).unwrap();
    fs::write(&input, b"changed bytes").unwrap();
    assert!(f.store.accepted_dependency_inputs("downstream").is_err());
    fs::write(&input, &original).unwrap();
    let extra = input.parent().unwrap().join("unexpected.txt");
    fs::write(&extra, b"not sealed").unwrap();
    assert!(f.store.accepted_dependency_inputs("downstream").is_err());
    fs::remove_file(&extra).unwrap();
    fs::remove_file(&input).unwrap();
    assert!(f.store.accepted_dependency_inputs("downstream").is_err());
    assert!(!input.exists());
    fs::write(&input, &original).unwrap();
    let path = f.journal.join("verification/input-check/snapshot.json");
    let bytes = fs::read(&path).unwrap();
    let mut manifest: crate::source_snapshot::SnapshotManifest =
        serde_json::from_slice(&bytes).unwrap();
    manifest.attempt_id = "foreign-attempt".into();
    fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    assert!(f.store.accepted_dependency_inputs("downstream").is_err());
    fs::write(&path, &bytes).unwrap();
    assert!(f.store.accepted_dependency_inputs("downstream").is_ok());
    let held = f.store.accepted_dependency_inputs("downstream").unwrap();
    let binding = held[0].binding().clone();
    drop(held);
    let (_, artifacts) = f
        .store
        .completed_artifacts("task", &binding.attempt_id)
        .unwrap();
    for (mut files, mut size) in [(0, u64::MAX), (usize::MAX, 0)] {
        assert!(crate::source_snapshot::open_frozen(
            &f.plan,
            artifacts,
            &binding.snapshot,
            &f.journal.join("verification"),
            &mut files,
            &mut size
        )
        .is_err());
    }
    let saved_path = path.with_extension("saved");
    fs::rename(&path, &saved_path).unwrap();
    assert!(f.store.accepted_dependency_inputs("downstream").is_err());
    assert!(!path.exists());
    fs::rename(&saved_path, &path).unwrap();
    assert!(f.store.accepted_dependency_inputs("unknown").is_err());
    assert!(f.store.accepted_dependency_inputs("task").is_err());
}
