use super::*;

fn flow(graph: &[(&str, &[&str])]) -> Flow {
    let mut f = Flow::with_plan(|plan| {
        plan.worktree.as_mut().unwrap().input =
            GitWorktreeInput::CommittedRawBlobsWithAcceptedDependencies;
        let template = plan.workflow.tasks[0].clone();
        let execution = plan.execution[0].clone();
        plan.workflow.tasks = graph
            .iter()
            .map(|(id, deps)| {
                let mut t = template.clone();
                t.id = (*id).into();
                t.dependencies = deps.iter().map(|s| (*s).into()).collect();
                t
            })
            .collect();
        plan.execution = graph
            .iter()
            .map(|(id, _)| {
                let mut e = execution.clone();
                e.task_id = (*id).into();
                e
            })
            .collect();
        plan.policy.max_requests = 16;
        plan.snapshot_policy.as_mut().unwrap().max_files = 4096;
    });
    f.approve();
    f
}

#[test]
#[ignore = "real Git preparation cancellation with partially copied sealed dependency inputs"]
fn cancel_dependency_copy_preserves_partial_intent_without_refilling() {
    let mut f = flow(&[("a", &[]), ("b", &["a"])]);
    complete(&mut f, "a", |p| {
        for n in 0..128 {
            fs::write(p.join(format!("z-{n:03}.txt")), format!("sealed {n}\n")).unwrap();
        }
    });
    let cancel = Arc::new(AtomicBool::new(false));
    let key = workspace_key("project", "b", "cancel-copy").unwrap();
    let pool = Path::new(&f.plan.worktree.as_ref().unwrap().pool_root);
    let output = pool.join("trees").join(&key);
    let (seen, result) = std::thread::scope(|scope| {
        let thread = scope.spawn(|| {
            f.store
                .prepare_task_workspace_cancellable("b", "cancel-copy", cancel.clone())
        });
        let deadline = Instant::now() + Duration::from_secs(60);
        while !output.join("b.txt").exists() && !thread.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        let seen = output.join("b.txt").exists() && !output.join("z-127.txt").exists();
        cancel.store(true, Ordering::Release);
        (seen, thread.join().unwrap())
    });
    assert!(
        seen,
        "actual partial copy must have been observed: {result:?}"
    );
    assert!(result.is_err());
    let intent = pool.join("records").join(format!("{key}.intent.json"));
    let ready = pool.join("records").join(format!("{key}.ready.json"));
    let original = fs::read(&intent).unwrap();
    assert!(!ready.exists());
    assert!(f.store.prepare_task_workspace("b", "cancel-copy").is_err());
    assert_eq!(fs::read(&intent).unwrap(), original);
    assert!(!ready.exists());
    assert!(!output.join("z-127.txt").exists());
}
fn prepare(f: &Flow, task: &str) -> Attempt {
    let id = format!("attempt-{task}");
    let (workspace, proof) = f.store.prepare_task_workspace(task, &id).unwrap();
    Attempt {
        workspace_proof: Some(proof),
        id,
        task_id: task.into(),
        worker_id: "worker".into(),
        revision_digest: f.plan.approval_digest().unwrap(),
        fence: 1,
        binding: SessionBinding {
            adapter_id: "codex".into(),
            adapter_version: "fixture".into(),
            session_id: "session".into(),
            identity_strength: IdentityStrength::ProviderId,
            workspace,
        },
    }
}
fn complete(f: &mut Flow, task: &str, edit: impl FnOnce(&Path)) -> Attempt {
    let a = prepare(f, task);
    f.assign(&a).unwrap();
    f.dispatch(&a).unwrap();
    apply(
        &mut f.store,
        "core",
        WorkflowCommand::Started {
            task: task.into(),
            attempt: a.id.clone(),
        },
        context(receipt(&a, ReceiptStatus::Running), None),
    )
    .unwrap();
    let path = Path::new(&a.binding.workspace);
    fs::write(path.join("b.txt"), b"worker result\n").unwrap();
    edit(path);
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
            task: task.into(),
            attempt: a.id.clone(),
            summary: "overlay fixture".into(),
            evidence: None,
        },
        context(r, Some(manifest)),
    )
    .unwrap();
    let pins = f
        .plan
        .task(task)
        .unwrap()
        .checks
        .iter()
        .map(PinnedCheck::from_approved)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    f.store
        .run_verification(
            &format!("check-{task}"),
            task,
            &a.id,
            &pins,
            &AtomicBool::new(false),
        )
        .unwrap();
    apply(
        &mut f.store,
        "chief",
        WorkflowCommand::Accept {
            task: task.into(),
            attempt: a.id.clone(),
        },
        CommandContext::default(),
    )
    .unwrap();
    a
}

#[test]
#[ignore = "real Git and independent verifier; transitive dependency additions/deletions, ready recovery and stale source isolation"]
fn transitive_overlay_uses_sealed_bytes_and_binds_initial_tree() {
    let mut f = flow(&[("a", &[]), ("b", &["a"]), ("c", &["b"])]);
    let a = complete(&mut f, "a", |p| {
        fs::write(p.join("added.txt"), b"from a").unwrap();
    });
    fs::write(
        Path::new(&a.binding.workspace).join("added.txt"),
        b"unaccepted later edit",
    )
    .unwrap();
    let b = complete(&mut f, "b", |p| {
        assert_eq!(fs::read(p.join("added.txt")).unwrap(), b"from a");
        fs::remove_file(p.join("added.txt")).unwrap();
        fs::write(p.join("result.txt"), b"from b").unwrap();
    });
    let c = prepare(&f, "c");
    // A lower-level base-only worktree cannot bypass the approved dependency
    // preparation gate even if a caller supplies its actual record digest.
    let raw = {
        let policy = f.plan.worktree.as_ref().unwrap();
        let store = GitWorkspaces::open(Path::new(&policy.pool_root), &policy.git).unwrap();
        store
            .prepare(&approved_request(&f.plan, "c", "bypass").unwrap())
            .unwrap()
    };
    let mut bypass = c.clone();
    bypass.id = "bypass".into();
    bypass.binding.workspace = raw.path.clone();
    bypass.workspace_proof = Some(WorkspaceProof {
        record_digest: digest(&raw).unwrap(),
    });
    assert!(f.assign(&bypass).is_err());
    let path = Path::new(&c.binding.workspace);
    assert!(!path.join("added.txt").exists());
    assert_eq!(fs::read(path.join("result.txt")).unwrap(), b"from b");
    let policy = f.plan.worktree.as_ref().unwrap();
    let records = Path::new(&policy.pool_root).join("records");
    let key = workspace_key("project", "c", &c.id).unwrap();
    let ready = records.join(format!("{key}.ready.json"));
    let original = fs::read(&ready).unwrap();
    let record: WorkspaceRecord = serde_json::from_slice(&original).unwrap();
    assert_eq!(record.overlay.as_ref().unwrap().inputs.len(), 2);
    assert!(record
        .overlay
        .as_ref()
        .unwrap()
        .inputs
        .iter()
        .any(|i| i.attempt_id == b.id));
    // All expected bytes are present; an interrupted final ready publication is reconcilable.
    fs::remove_file(&ready).unwrap();
    assert!(f.store.prepare_task_workspace("c", &c.id).is_ok());
    assert_eq!(fs::read(&ready).unwrap(), original);
    fs::write(path.join("result.txt"), b"worker edit").unwrap();
    assert!(f.store.prepare_task_workspace("c", &c.id).is_ok());
    assert_eq!(fs::read(path.join("result.txt")).unwrap(), b"worker edit");
    fs::remove_file(&ready).unwrap();
    assert!(f.store.prepare_task_workspace("c", &c.id).is_err());
    assert!(!ready.exists());
    assert_eq!(fs::read(path.join("result.txt")).unwrap(), b"worker edit");
    fs::write(&ready, &original).unwrap();
    f.assign(&c).unwrap();
    f.dispatch(&c).unwrap();
    drop(f.store);
    let reopened = DurableWorkflow::open(&f.journal, f.config).unwrap();
    assert_eq!(reopened.state().tasks["c"].state, TaskState::Unknown);
    assert_eq!(
        reopened.attempt_record("c", &c.id).unwrap().workspace_proof,
        c.workspace_proof
    );
}

#[test]
#[ignore = "real Git and independent verifier; diamond merge plus order independent conflict before intent"]
fn diamond_composes_disjoint_changes_and_conflict_creates_no_worktree() {
    let mut f = flow(&[
        ("a", &[]),
        ("b", &["a"]),
        ("c", &["a"]),
        ("end", &["b", "c"]),
    ]);
    complete(&mut f, "a", |p| {
        fs::write(p.join("shared.txt"), b"ancestor").unwrap();
    });
    complete(&mut f, "b", |p| {
        fs::remove_file(p.join("shared.txt")).unwrap();
        fs::write(p.join("left.txt"), b"left").unwrap();
    });
    complete(&mut f, "c", |p| {
        fs::write(p.join("right.txt"), b"right").unwrap();
    });
    let end = prepare(&f, "end");
    let path = Path::new(&end.binding.workspace);
    assert!(!path.join("shared.txt").exists());
    assert_eq!(fs::read(path.join("left.txt")).unwrap(), b"left");
    assert_eq!(fs::read(path.join("right.txt")).unwrap(), b"right");
    drop(f);
    for parents in [["b", "c"], ["c", "b"]] {
        let mut f = flow(&[("a", &[]), ("b", &[]), ("c", &["a"]), ("end", &parents)]);
        complete(&mut f, "a", |p| {
            fs::write(p.join("shared.txt"), b"same").unwrap();
        });
        complete(&mut f, "b", |p| {
            fs::write(p.join("shared.txt"), b"same").unwrap();
        });
        complete(&mut f, "c", |p| {
            fs::write(p.join("shared.txt"), b"different").unwrap();
        });
        assert!(f
            .store
            .prepare_task_workspace("end", "conflict")
            .unwrap_err()
            .contains("conflict at shared.txt"));
        let key = workspace_key("project", "end", "conflict").unwrap();
        let pool = Path::new(&f.plan.worktree.as_ref().unwrap().pool_root);
        assert!(!pool
            .join("records")
            .join(format!("{key}.intent.json"))
            .exists());
        assert!(!pool.join("trees").join(key).exists());
    }
}
