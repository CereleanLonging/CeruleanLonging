//! Shared real Git inputs for the existing offline resident DAG probes.
use super::*;
use cerulean_domain::project_setup::WorktreeSetupDraft;
use std::process::{Command, Stdio};

pub(super) fn configure(root: &Path) -> WorktreeSetupDraft {
    let executable = std::env::var("CERULEAN_TEST_GIT").expect("explicit real Git required");
    let project = fs::canonicalize(root.join("project")).unwrap();
    let git = |args: &[&str]| {
        let mut command = Command::new(&executable);
        command
            .env_clear()
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "NUL")
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null());
        for key in ["SystemRoot", "WINDIR", "TEMP", "TMP"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        let output = command
            .current_dir(&project)
            .args(["-c", "core.hooksPath=NUL", "-c", "core.fsmonitor=false"])
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "Git fixture: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    };
    fs::write(project.join("base.txt"), "committed dependency base\n").unwrap();
    git(&["init", "--initial-branch=main"]);
    git(&["add", "--all"]);
    git(&[
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@localhost",
        "commit",
        "--no-gpg-sign",
        "-m",
        "approved dependency baseline",
    ]);
    let base_commit = git(&["rev-parse", "HEAD"]);
    fs::write(project.join("base.txt"), "uncommitted source change\n").unwrap();
    fs::create_dir(root.join("pool")).unwrap();
    WorktreeSetupDraft {
        integration: None,
        inherit_dependencies: true,
        base_commit,
        pool_root: fs::canonicalize(root.join("pool"))
            .unwrap()
            .to_string_lossy()
            .into(),
        git_executable: executable,
    }
}

pub(super) fn task_path(host: &RuntimeHost, task: &str) -> PathBuf {
    let store = &host.projects["project"];
    let attempt = store.state().tasks[task].attempt.as_deref().unwrap();
    PathBuf::from(
        store
            .revision()
            .unwrap()
            .execution_root(task, attempt)
            .unwrap(),
    )
}
pub(super) fn assert_delivery(host: &RuntimeHost, first: &str, second: &str) {
    let store = &host.projects["project"];
    let plan = store.revision().unwrap();
    let a = task_path(host, "a");
    let b = task_path(host, "b");
    assert_ne!(a, b);
    assert_ne!(a, PathBuf::from(&plan.project.root));
    assert_ne!(b, PathBuf::from(&plan.project.root));
    assert!(!Path::new(&plan.project.root).join("result.txt").exists());
    assert_eq!(
        fs::read_to_string(Path::new(&plan.project.root).join("base.txt")).unwrap(),
        "uncommitted source change\n"
    );
    for (task, path, result) in [("a", &a, first), ("b", &b, second)] {
        assert_eq!(store.state().tasks[task].state, TaskState::Accepted);
        assert_eq!(fs::read_to_string(path.join("result.txt")).unwrap(), result);
        assert_eq!(
            fs::read_to_string(path.join("base.txt")).unwrap(),
            "committed dependency base\n"
        );
        let id = store.state().tasks[task].attempt.as_deref().unwrap();
        assert!(store
            .attempt_record(task, id)
            .unwrap()
            .workspace_proof
            .is_some());
    }
    let id = store.state().tasks["b"].attempt.as_deref().unwrap();
    let key = workspace_key("project", "b", id).unwrap();
    let record: crate::git_workspaces::WorkspaceRecord = serde_json::from_slice(
        &fs::read(
            Path::new(&plan.worktree.as_ref().unwrap().pool_root)
                .join("records")
                .join(format!("{key}.ready.json")),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(record.overlay.as_ref().unwrap().inputs.len(), 1);
    assert_eq!(
        record.overlay.as_ref().unwrap().inputs[0].attempt_id,
        store.state().tasks["a"].attempt.as_deref().unwrap()
    );
    assert_eq!(
        digest(&record).unwrap(),
        store
            .attempt_record("b", id)
            .unwrap()
            .workspace_proof
            .as_ref()
            .unwrap()
            .record_digest
    );
}
