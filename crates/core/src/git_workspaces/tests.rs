//! Real Git, owned process tree, isolated local repositories. No model/network.
use super::*;
use tempfile::TempDir;
mod approved;

#[test]
#[ignore = "real Git cancellation during file materialization; preserves incomplete intent"]
fn cancellation_during_materialization_preserves_partial_workspace() {
    let mut f = Fixture::new();
    for index in 0..32 {
        fs::write(
            f.repo.join(format!("z-{index:02}.txt")),
            format!("fixture {index}\n"),
        )
        .unwrap();
    }
    f.commit();
    let cancel = Arc::new(AtomicBool::new(false));
    f.store.cancel = cancel.clone();
    let request = f.request("cancel-task");
    let key = GitWorkspaces::key(&request).unwrap();
    let output = f.store.root.join("trees").join(key).join("b.txt");
    let intent = f.record_path(&request, "intent");
    let ready = f.record_path(&request, "ready");
    let (started, result) = std::thread::scope(|scope| {
        let worker = scope.spawn(|| f.store.prepare(&request));
        let deadline = Instant::now() + Duration::from_secs(20);
        while !output.exists() && !worker.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        let started = output.exists();
        cancel.store(true, Ordering::Release);
        (started, worker.join().unwrap())
    });
    assert!(
        started,
        "preparation must have started actual file materialization: {result:?}"
    );
    assert!(result.is_err());
    assert!(!ready.exists());
    let original = fs::read(&intent).unwrap();
    assert_eq!(fs::read(&output).unwrap(), b"base\n");
    cancel.store(false, Ordering::Release);
    // Reconciliation may inspect, but must not repeat add or fill missing files.
    assert!(f.store.prepare(&request).is_err());
    assert_eq!(fs::read(&intent).unwrap(), original);
    assert!(!ready.exists());
    assert_eq!(fs::read(output).unwrap(), b"base\n");
}

struct Fixture {
    temp: TempDir,
    store: GitWorkspaces,
    repo: PathBuf,
    base: String,
    pin: ValidationFile,
}
impl Fixture {
    fn new() -> Self {
        let workspace =
            fs::canonicalize(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reports"))
                .unwrap();
        let temp = tempfile::Builder::new()
            .prefix("git-workspaces-")
            .tempdir_in(&workspace)
            .unwrap();
        assert!(fs::canonicalize(temp.path())
            .unwrap()
            .starts_with(fs::canonicalize(&workspace).unwrap()));
        let repo = temp.path().join("source");
        let pool = temp.path().join("pool");
        fs::create_dir(&repo).unwrap();
        fs::create_dir(&pool).unwrap();
        let executable =
            std::env::var("CERULEAN_TEST_GIT").expect("explicit local Git executable required");
        let pin = PinnedCheck::describe(Path::new(&executable), &[], Default::default())
            .unwrap()
            .executable;
        let store = GitWorkspaces::open(&pool, &pin).unwrap();
        let mut fixture = Self {
            temp,
            store,
            repo,
            base: String::new(),
            pin,
        };
        fixture.command(&["init", "--initial-branch=main"]);
        fixture.command(&["config", "user.name", "Workspace fixture"]);
        fixture.command(&["config", "user.email", "fixture@localhost"]);
        fs::create_dir(fixture.repo.join("src")).unwrap();
        fs::write(fixture.repo.join("src/a.mjs"), b"export const a = 1;\n").unwrap();
        fs::write(fixture.repo.join("b.txt"), b"base\n").unwrap();
        fixture.commit();
        fixture
    }
    fn command(&self, args: &[&str]) -> String {
        self.store
            .line(&self.repo, args, Instant::now() + Duration::from_secs(30))
            .unwrap()
    }
    fn commit(&mut self) {
        self.command(&["add", "--all"]);
        self.command(&["commit", "--no-gpg-sign", "-m", "fixture"]);
        self.base = self.command(&["rev-parse", "HEAD"]);
    }
    fn request(&self, id: &str) -> WorkspaceRequest {
        WorkspaceRequest {
            project_id: "project".into(),
            revision_digest: "a".repeat(64),
            task_id: id.into(),
            attempt_id: "attempt-1".into(),
            repository: text(&self.repo).unwrap(),
            base_commit: self.base.clone(),
        }
    }
    fn record_path(&self, request: &WorkspaceRequest, suffix: &str) -> PathBuf {
        self.store.root.join("records").join(format!(
            "{}.{}.json",
            GitWorkspaces::key(request).unwrap(),
            suffix
        ))
    }
}

#[test]
#[ignore = "explicit real Git fixture: set CERULEAN_TEST_GIT"]
fn separate_worktrees_preserve_worker_edits_and_reopen_identity() {
    let f = Fixture::new();
    let a = f.store.prepare(&f.request("task-a")).unwrap();
    let b = f.store.prepare(&f.request("task-b")).unwrap();
    assert_ne!(a.path, b.path);
    assert_eq!(a.common_dir, b.common_dir);
    fs::write(Path::new(&a.path).join("src/a.mjs"), b"worker A\n").unwrap();
    assert_eq!(
        fs::read(Path::new(&b.path).join("src/a.mjs")).unwrap(),
        b"export const a = 1;\n"
    );
    assert_eq!(
        fs::read(f.repo.join("src/a.mjs")).unwrap(),
        b"export const a = 1;\n"
    );
    assert!(GitWorkspaces::open(&f.store.root, &f.pin).is_err());
    let before = f.command(&["worktree", "list", "--porcelain"]);
    let root = f.store.root.clone();
    let request = f.request("task-a");
    let pin = f.pin.clone();
    drop(f.store);
    let reopened = GitWorkspaces::open(&root, &pin).unwrap();
    assert_eq!(reopened.prepare(&request).unwrap(), a);
    assert_eq!(
        fs::read(Path::new(&a.path).join("src/a.mjs")).unwrap(),
        b"worker A\n"
    );
    assert_eq!(
        reopened
            .line(
                &f.repo,
                &["worktree", "list", "--porcelain"],
                Instant::now() + Duration::from_secs(30)
            )
            .unwrap(),
        before
    );
    drop(reopened);
    drop(f.temp);
}

#[test]
#[ignore = "explicit real Git fixture: set CERULEAN_TEST_GIT"]
fn creation_recovery_adopts_complete_tree_but_never_recreates_partial_tree() {
    let f = Fixture::new();
    let request = f.request("complete");
    let record = f.store.prepare(&request).unwrap();
    fs::remove_file(f.record_path(&request, "ready")).unwrap(); // simulated crash before ready publication
    assert_eq!(f.store.prepare(&request).unwrap(), record);
    fs::remove_file(f.record_path(&request, "ready")).unwrap();
    fs::remove_file(Path::new(&record.path).join("b.txt")).unwrap();
    assert!(f.store.prepare(&request).is_err());
    assert!(!Path::new(&record.path).join("b.txt").exists());
    assert!(!f.record_path(&request, "ready").exists());
    let missing = f.request("intent-only");
    let mut intent = record.clone();
    intent.request = missing.clone();
    intent.path = text(
        &f.store
            .root
            .join("trees")
            .join(GitWorkspaces::key(&missing).unwrap()),
    )
    .unwrap();
    write_new(
        &f.record_path(&missing, "intent"),
        &serde_json::to_vec(&intent).unwrap(),
    )
    .unwrap();
    assert!(f.store.prepare(&missing).is_err());
    assert!(!Path::new(&intent.path).exists());
}

#[test]
#[ignore = "explicit real Git fixture: set CERULEAN_TEST_GIT"]
fn attempt_rebinding_and_changed_head_are_rejected() {
    let mut f = Fixture::new();
    let request = f.request("task");
    let record = f.store.prepare(&request).unwrap();
    let mut rebound = request.clone();
    rebound.revision_digest = "b".repeat(64);
    assert!(f.store.prepare(&rebound).is_err());
    fs::write(f.repo.join("b.txt"), "next commit\n").unwrap();
    f.commit();
    f.store
        .git(
            Path::new(&record.path),
            &["update-ref", "HEAD", &f.base],
            Instant::now() + Duration::from_secs(30),
        )
        .unwrap();
    assert!(f.store.prepare(&request).is_err());
    assert_eq!(f.command(&["rev-parse", "HEAD"]), f.base);
    let mut named = f.request("named-ref");
    named.base_commit = "HEAD".into();
    assert!(f.store.prepare(&named).is_err());
}

#[test]
#[ignore = "explicit real Git fixture: set CERULEAN_TEST_GIT"]
fn shared_branch_and_replaced_ready_record_are_rejected() {
    let f = Fixture::new();
    let request = f.request("task");
    let record = f.store.prepare(&request).unwrap();
    f.store
        .git(
            Path::new(&record.path),
            &["symbolic-ref", "HEAD", "refs/heads/main"],
            Instant::now() + Duration::from_secs(30),
        )
        .unwrap();
    assert!(f.store.prepare(&request).unwrap_err().contains("detached"));
    let second = f.request("other");
    let mut changed = f.store.prepare(&second).unwrap();
    changed.request.revision_digest = "b".repeat(64);
    fs::write(
        f.record_path(&second, "ready"),
        serde_json::to_vec(&changed).unwrap(),
    )
    .unwrap();
    assert!(f
        .store
        .prepare(&second)
        .unwrap_err()
        .contains("ready workspace record changed"));
}

#[test]
#[ignore = "explicit real Git fixture: set CERULEAN_TEST_GIT"]
fn unfinished_extra_files_and_index_changes_are_not_silently_adopted() {
    let f = Fixture::new();
    let req = f.request("task");
    let record = f.store.prepare(&req).unwrap();
    fs::remove_file(f.record_path(&req, "ready")).unwrap();
    let extra = Path::new(&record.path).join("unexpected.txt");
    fs::write(&extra, "worker edit").unwrap();
    assert!(f.store.prepare(&req).is_err());
    assert!(extra.exists());
    fs::remove_file(extra).unwrap();
    f.store
        .git(
            Path::new(&record.path),
            &["update-index", "--force-remove", "b.txt"],
            Instant::now() + Duration::from_secs(30),
        )
        .unwrap();
    assert!(f.store.prepare(&req).is_err());
    assert!(!f.record_path(&req, "ready").exists());
}

#[test]
#[ignore = "explicit real Git fixture: set CERULEAN_TEST_GIT"]
fn checkout_does_not_execute_repository_filters_or_hooks() {
    let mut f = Fixture::new();
    fs::write(f.repo.join(".gitattributes"), "*.txt filter=fixture\n").unwrap();
    f.commit();
    f.command(&["config", "filter.fixture.smudge", "cmd.exe /c exit 91"]);
    f.command(&["config", "filter.fixture.process", "cmd.exe /c exit 92"]);
    f.command(&["config", "filter.fixture.required", "true"]);
    let hook = f.repo.join(".git/hooks/post-checkout");
    fs::write(hook, "#!/bin/sh\nexit 93\n").unwrap();
    let record = f.store.prepare(&f.request("task")).unwrap();
    assert_eq!(
        fs::read(Path::new(&record.path).join("b.txt")).unwrap(),
        b"base\n"
    );
    assert_eq!(f.command(&["rev-parse", "HEAD"]), f.base);
}

#[test]
#[ignore = "explicit real Git fixture: set CERULEAN_TEST_GIT"]
fn unsupported_symlink_tree_rejected_before_intent_or_git_mutation() {
    let mut f = Fixture::new();
    let object = f.command(&["rev-parse", "HEAD:b.txt"]);
    f.command(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("120000,{object},link"),
    ]);
    f.command(&["commit", "--no-gpg-sign", "-m", "symlink tree"]);
    f.base = f.command(&["rev-parse", "HEAD"]);
    let request = f.request("task");
    assert!(f
        .store
        .prepare(&request)
        .unwrap_err()
        .contains("links/submodules"));
    assert!(!f.record_path(&request, "intent").exists());
    assert_eq!(fs::read_dir(f.store.root.join("trees")).unwrap().count(), 0);
}

#[test]
fn contended_writer_wait_cancels_or_acquires_only_after_original_owner_releases() {
    for cancel_wait in [true, false] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("writer.lock");
        let owner = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        owner.try_lock_exclusive().unwrap();
        let contender = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        assert!(
            acquire_writer(&contender, Some(&cancel), Instant::now(), || panic!(
                "expired wait cannot block"
            ))
            .unwrap_err()
            .contains("deadline")
        );
        let (entered, waiting) = std::sync::mpsc::channel();
        let (release, resume) = std::sync::mpsc::channel();
        let worker_cancel = cancel.clone();
        let thread = std::thread::spawn(move || {
            acquire_writer(
                &contender,
                Some(&worker_cancel),
                Instant::now() + Duration::from_secs(5),
                || {
                    entered.send(()).unwrap();
                    resume.recv_timeout(Duration::from_secs(5)).unwrap();
                },
            )
            .map(|()| contender)
        });
        waiting.recv_timeout(Duration::from_secs(5)).unwrap();
        if cancel_wait {
            cancel.store(true, Ordering::Release);
        } else {
            drop(owner);
        }
        release.send(()).unwrap();
        let result = thread.join().unwrap();
        if cancel_wait {
            assert!(result.unwrap_err().contains("cancelled"));
        } else {
            let new_owner = result.unwrap();
            let observer = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .unwrap();
            assert!(observer.try_lock_exclusive().is_err());
            drop(new_owner);
            observer.try_lock_exclusive().unwrap();
        }
    }
}

#[test]
#[ignore = "explicit real Git; immutable ready inspection while original pool writer remains locked"]
fn sealed_inspection_does_not_take_writer_lock_or_gain_mutation_authority() {
    let f = Fixture::new();
    let request = f.request("reader-a");
    let record = f.store.prepare(&request).unwrap();
    let ready = fs::read(f.record_path(&request, "ready")).unwrap();
    assert!(GitWorkspaces::open(&f.store.root, &f.pin).is_err());
    assert_eq!(
        GitWorkspaces::inspect_ready(&f.store.root, &f.pin, &request).unwrap(),
        record
    );
    let read = GitWorkspaces::open_inner(&f.store.root, &f.pin, false, false, None).unwrap();
    assert!(read
        .prepare(&f.request("reader-b"))
        .unwrap_err()
        .contains("read-only"));
    assert!(read
        .recover(&record, Instant::now() + Duration::from_secs(5))
        .unwrap_err()
        .contains("read-only"));
    assert!(!f.record_path(&f.request("reader-b"), "intent").exists());
    assert_eq!(fs::read(f.record_path(&request, "ready")).unwrap(), ready);
    let mut changed = record.clone();
    changed.request.task_id = "changed".into();
    fs::write(
        f.record_path(&request, "ready"),
        serde_json::to_vec(&changed).unwrap(),
    )
    .unwrap();
    assert!(GitWorkspaces::inspect_ready(&f.store.root, &f.pin, &request).is_err());
    println!("sealed reader: writer remains exclusive, ready inspection succeeds, mutations/changed seal rejected");
}
