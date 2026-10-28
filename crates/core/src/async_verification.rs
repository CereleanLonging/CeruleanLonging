use crate::{host::ordinary, scheduler::DurableWorkflow, verifier::PinnedCheck};
use cerulean_domain::{execution::*, workflow::*};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
};

pub(crate) struct Completion {
    pub store: Option<Result<DurableWorkflow, String>>,
    pub outcome: Result<bool, String>,
    pub uncertain: bool,
}
fn unfinished(root: &Path, project: &str, operation: &str) -> bool {
    let run = root
        .join("workflows")
        .join(project)
        .join("verification")
        .join(operation);
    if !run.exists() {
        return false;
    }
    let path = run.join("result.json");
    if ordinary(&path, false).is_err() {
        return true;
    }
    let Ok(file) = std::fs::File::open(path) else {
        return true;
    };
    use std::io::Read;
    let mut bytes = vec![];
    if file.take(1024 * 1024 + 1).read_to_end(&mut bytes).is_err() || bytes.len() > 1024 * 1024 {
        return true;
    }
    Some(bytes)
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .is_none_or(|record| {
            record["schemaVersion"] != 1
                || (!record["report"].is_object() && !record["error"].is_string())
        })
}
pub(crate) struct ActiveVerification {
    pub derived_diagnostics: Option<crate::derived_index::Diagnostics>,
    pub shared: bool,
    pub snapshot: ExecutionSnapshot,
    pub detail_basis: crate::verification_detail::Basis,
    pub job: VerificationJob,
    pub cancel: Arc<AtomicBool>,
    pub receiver: mpsc::Receiver<Completion>,
}
impl Drop for ActiveVerification {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}
pub(crate) fn snapshot(
    store: &(impl crate::workflow_access::WorkflowAccess + ?Sized),
) -> ExecutionSnapshot {
    ExecutionSnapshot {
        project: store
            .configuration()
            .execution_scope
            .as_ref()
            .expect("strict scope")
            .project
            .clone(),
        revision: store.revision().cloned(),
        revision_digest: store
            .revision()
            .map(|r| r.approval_digest().expect("validated revision")),
        approval: store.approval().cloned(),
        state: store.state().clone(),
        sequence: store.history().len() as u32,
    }
}
pub(crate) fn folder(root: &Path, project: &str) -> PathBuf {
    root.join("verification-jobs").join(project)
}
pub(crate) fn save(root: &Path, job: &VerificationJob) -> Result<(), String> {
    identifier(&job.project_id)?;
    identifier(&job.operation_id)?;
    let base = root.join("verification-jobs");
    fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    ordinary(&base, true)?;
    let dir = folder(root, &job.project_id);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    ordinary(&dir, true)?;
    let dest = dir.join(format!("{}.json", job.operation_id));
    if dest.exists() {
        ordinary(&dest, false)?;
    }
    let temp = dir.join(format!(
        "{}.{}.tmp",
        job.operation_id,
        chrono::Utc::now()
            .timestamp_nanos_opt()
            .ok_or("clock out of range")?
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec(job).map_err(|e| e.to_string())?;
    require(bytes.len() <= 64 * 1024, "verification status size limit")?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    drop(file);
    fs::rename(temp, dest).map_err(|e| e.to_string())
}
pub(crate) fn load(
    root: &Path,
    project: &str,
    store: &DurableWorkflow,
) -> Result<Vec<VerificationJob>, String> {
    let dir = folder(root, project);
    if !dir.exists() {
        return Ok(vec![]);
    }
    ordinary(&root.join("verification-jobs"), true)?;
    ordinary(&dir, true)?;
    let mut jobs = vec![];
    for entry in fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        if path.extension().is_some_and(|s| s == "tmp") {
            continue;
        }
        ordinary(&path, false)?;
        require(
            path.extension().is_some_and(|s| s == "json"),
            "unexpected job file",
        )?;
        require(
            fs::metadata(&path).map_err(|e| e.to_string())?.len() <= 64 * 1024,
            "job file too large",
        )?;
        let mut job: VerificationJob =
            serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        for id in [&job.operation_id, &job.task_id, &job.attempt_id] {
            identifier(id)?;
        }
        timestamp(&job.started_at)?;
        timestamp(&job.updated_at)?;
        require(
            job.project_id == project
                && path
                    .file_stem()
                    .is_some_and(|s| s == job.operation_id.as_str()),
            "job scope mismatch",
        )?;
        let proof = store
            .history()
            .iter()
            .find(|e| e.body.operation_id == job.operation_id);
        if let Some(entry) = proof {
            match &entry.body.event.command {
                WorkflowCommand::Verify {
                    task,
                    attempt,
                    passed,
                    ..
                } if task == &job.task_id && attempt == &job.attempt_id => {
                    job.state = if *passed {
                        VerificationState::Succeeded
                    } else {
                        VerificationState::Failed
                    };
                    job.message = None;
                }
                _ => return Err("job operation conflicts with journal".into()),
            }
        } else {
            match crate::verification_reconciliation::applies(root, &job) {
                Ok(true) => {
                    job.state = VerificationState::Abandoned;
                    job.message = Some("用户已放弃本次验收；保留证据，任务未通过。".into());
                }
                Err(error) => {
                    job.state = VerificationState::Unknown;
                    job.message = Some(error);
                }
                Ok(false)
                    if job.state != VerificationState::Failed
                        || unfinished(root, project, &job.operation_id) =>
                {
                    job.state = VerificationState::Unknown;
                    job.message = Some("验收未形成可确认的账本结果，禁止自动重跑，等待对账".into());
                }
                Ok(false) => {}
            }
        }
        job.reconciliation_digest = None;
        // Even a saved 'succeeded' badge requires a matching Verify journal event.
        save(root, &job)?;
        jobs.push(job);
        require(jobs.len() <= 1000, "verification job history limit")?;
    }
    Ok(jobs)
}
pub(crate) fn launch(
    root: &Path,
    mut store: DurableWorkflow,
    job: VerificationJob,
    pins: Vec<PinnedCheck>,
) -> Result<ActiveVerification, String> {
    let snap = snapshot(&store);
    let derived_diagnostics = Some(store.derived_diagnostics());
    let detail_basis = crate::verification_detail::basis(&store, &job)?;
    let config = store.configuration().clone();
    let path = root.join("workflows").join(&job.project_id);
    let host_root = root.to_path_buf();
    let project = job.project_id.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    let token = cancel.clone();
    let (sender, receiver) = mpsc::channel();
    let operation = job.operation_id.clone();
    let task = job.task_id.clone();
    let attempt = job.attempt_id.clone();
    thread::Builder::new().name(format!("verify-{}",job.operation_id)).spawn(move || {
        let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(||store.run_verification(&operation,&task,&attempt,&pins,&token)));
        let completion=match result {
            Ok(outcome)=>{
                let uncertain=store.requires_reopen() || (outcome.is_err() && unfinished(&host_root,&project,&operation));
                let store=if store.requires_reopen(){drop(store);DurableWorkflow::open(&path,config)}else{Ok(store)};
                Completion{store:Some(store),uncertain,outcome:outcome.map(|e|matches!(e.body.event.command,WorkflowCommand::Verify{passed:true,..}))}
            },
            Err(_)=>{drop(store);Completion{store:Some(DurableWorkflow::open(&path,config)),uncertain:true,outcome:Err("verification worker panicked; reopened journal, reconciliation required".into())}},
        };
        let _=sender.send(completion);
    }).map_err(|e|format!("verification thread launch failed: {e}"))?;
    Ok(ActiveVerification {
        derived_diagnostics,
        shared: false,
        snapshot: snap,
        detail_basis,
        job,
        cancel,
        receiver,
    })
}

/// A shared owner stays resident while either API or verification clients run.
pub(crate) fn launch_shared(
    root: &Path,
    mut client: crate::workflow_coordinator::VerificationClient,
    job: VerificationJob,
    pins: Vec<PinnedCheck>,
) -> Result<ActiveVerification, String> {
    let snap = client.snapshot();
    let detail_basis = client.basis();
    let host_root = root.to_path_buf();
    let project = job.project_id.clone();
    let operation = job.operation_id.clone();
    let cancel = Arc::new(AtomicBool::new(false));
    let token = cancel.clone();
    let (sender, receiver) = mpsc::channel();
    thread::Builder::new()
        .name(format!("verify-{}", job.operation_id))
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                client.run(&pins, &token)
            }));
            let (outcome, uncertain) = match result {
                Ok(outcome) => {
                    let uncertain = client.uncertain()
                        || (outcome.is_err() && unfinished(&host_root, &project, &operation));
                    (
                        outcome.map(|e| {
                            matches!(
                                e.body.event.command,
                                WorkflowCommand::Verify { passed: true, .. }
                            )
                        }),
                        uncertain,
                    )
                }
                Err(_) => (
                    Err("shared verification worker panicked; reconcile original evidence".into()),
                    true,
                ),
            };
            drop(client); // Release ownership before the host can reclaim the writer.
            let _ = sender.send(Completion {
                store: None,
                outcome,
                uncertain,
            });
        })
        .map_err(|e| format!("verification thread launch failed: {e}"))?;
    Ok(ActiveVerification {
        derived_diagnostics: None, // The shared coordinator remains in api_writers.
        shared: true,
        snapshot: snap,
        detail_basis,
        job,
        cancel,
        receiver,
    })
}
