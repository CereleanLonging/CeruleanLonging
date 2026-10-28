//! Explicit recovery of a previously owned real attempt; never creates a model task.
use super::*;
use cerulean_adapters::codex::CodexClient;
use serde_json::json;
use sha2::{Digest, Sha256};

fn bytes_hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn files(root: &Path, path: &Path, out: &mut BTreeMap<String, String>) {
    ordinary(path, true).unwrap();
    for entry in fs::read_dir(path).unwrap() {
        let path = entry.unwrap().path();
        let metadata = fs::symlink_metadata(&path).unwrap();
        ordinary(&path, metadata.is_dir()).unwrap();
        if metadata.is_dir() {
            files(root, &path, out);
        } else {
            out.insert(
                path.strip_prefix(root).unwrap().to_string_lossy().into(),
                bytes_hash(&fs::read(path).unwrap()),
            );
        }
    }
}
fn save(root: &Path, name: &str, value: &impl serde::Serialize) {
    use std::io::Write;
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(name))
        .unwrap();
    f.write_all(&serde_json::to_vec_pretty(value).unwrap())
        .unwrap();
    f.sync_all().unwrap();
}
#[test]
#[ignore = "explicit read-only recovery of owned original report, no model turn or interrupt"]
fn recover_original_empty_interruption_readonly() {
    assert_eq!(
        std::env::var("CERULEAN_CANCEL_LIVE_OWNED_JOB").as_deref(),
        Ok("1")
    );
    let mut in_job = 0;
    assert_ne!(
        unsafe {
            windows_sys::Win32::System::JobObjects::IsProcessInJob(
                windows_sys::Win32::System::Threading::GetCurrentProcess(),
                std::ptr::null_mut(),
                &mut in_job,
            )
        },
        0
    );
    assert_ne!(in_job, 0);
    let root = PathBuf::from(std::env::var_os("CERULEAN_CANCEL_LIVE_DIR").unwrap())
        .canonicalize()
        .unwrap();
    let source = PathBuf::from(std::env::var_os("CERULEAN_CANCEL_RECOVERY_SOURCE").unwrap())
        .canonicalize()
        .unwrap();
    let reports = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../reports")
        .canonicalize()
        .unwrap();
    assert!(root.starts_with(&reports) && source.starts_with(&reports) && root != source);
    assert!(!root.starts_with(&source) && !source.starts_with(&root));
    ordinary(&root, true).unwrap();
    assert!(fs::read_dir(&root).unwrap().next().is_none());
    let mut original = BTreeMap::new();
    files(&source, &source, &mut original);
    let data = root.join("data");
    fs::create_dir(&data).unwrap();
    for name in original.keys() {
        let destination = data.join(name);
        fs::create_dir_all(destination.parent().unwrap()).unwrap();
        fs::copy(source.join(name), destination).unwrap();
    }
    save(&root, "original-data-hashes.json", &original);
    let mut host = RuntimeHost::open(&data).unwrap();
    let job = host
        .codex_jobs
        .get(&("project".into(), "cancel-live-once".into()))
        .unwrap()
        .clone();
    assert_eq!(job.state, CodexJobState::Unknown);
    assert!(host
        .runs
        .all()
        .iter()
        .all(|run| run.state == cerulean_domain::project_setup::RunState::Paused));
    let management = host.management.statuses().len();
    save(&root, "before.json", &host.snapshot());
    let store = host.projects.get_mut("project").unwrap();
    let before = store.history().to_vec();
    let plan = store.revision().unwrap().clone();
    assert!(store
        .started_cancellation_anchors(&job.task_id, &job.attempt_id)
        .is_ok());
    let executable = Path::new(&job.worker.executable.path);
    assert_eq!(
        bytes_hash(&fs::read(executable).unwrap()),
        job.worker.executable.sha256
    );
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let mut client = {
        let _entered = runtime.enter();
        CodexClient::launch_readonly(executable, Path::new(&plan.project.root)).unwrap()
    };
    let outcome = runtime.block_on(async {
        client.connect().await?;
        crate::codex_execution::reconcile(store, &mut client, &job.task_id, &job.attempt_id).await
    });
    let counts = client.request_counts().clone();
    let cleanup = runtime.block_on(client.close());
    let history = store.history().to_vec();
    let state = store.state().tasks[&job.task_id].state;
    save(&root, "recovered-history.json", &history);
    drop(host);
    let reopened = RuntimeHost::open(&data).unwrap();
    save(&root, "reopened.json", &reopened.snapshot());
    let mut after_original = BTreeMap::new();
    files(&source, &source, &mut after_original);
    let prefix_unchanged = history.get(..before.len()) == Some(before.as_slice());
    let proof = history
        .last()
        .and_then(|e| match &e.body.context.execution {
            Some(ExecutionEvidence::StartedTurnCancellation { proof, .. }) => Some(proof.clone()),
            _ => None,
        });
    let counts_safe = counts.keys().all(|m| {
        matches!(
            m.as_str(),
            "initialize" | "account/read" | "model/list" | "thread/read"
        )
    });
    let passed = outcome.is_ok()
        && cleanup.is_ok()
        && state == TaskState::Cancelled
        && proof.is_some()
        && history.len() == before.len() + 1
        && prefix_unchanged
        && original == after_original
        && counts_safe
        && counts.get("thread/read") == Some(&1)
        && reopened.projects["project"].history() == history
        && reopened.codex_jobs[&("project".into(), "cancel-live-once".into())].state
            == CodexJobState::Cancelled
        && reopened.management.statuses().len() == management
        && reopened
            .runs
            .all()
            .iter()
            .all(|run| run.state == cerulean_domain::project_setup::RunState::Paused);
    let result = json!({"status":if passed {"passed"} else {"blocked"},"at":chrono::Utc::now().to_rfc3339(),
        "scope":"read-only recovery of same real original attempt; synthetic management and approval retained",
        "methods":counts,"newTaskStarts":0,"newInterrupts":0,"realManagementCalls":0,
        "sourceUnchanged":original==after_original,"originalHistoryPrefixUnchanged":prefix_unchanged,
        "newWorkflowEvents":history.len()-before.len(),"state":state,"proof":proof,
        "reopenHistoryUnchanged":reopened.projects["project"].history()==history,
        "outcomeError":outcome.err(),"clientCleanupError":cleanup.err(),"fullP2Passed":false});
    save(&root, "result.json", &result);
    println!("{result}");
    assert!(passed);
}
