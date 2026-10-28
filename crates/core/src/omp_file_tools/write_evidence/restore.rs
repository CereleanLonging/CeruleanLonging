//! Trusted user-channel rollback for an unknown OMP attempt. No model/tool entry.
//! Keeps original task and journals unknown; does not resend or accept the task.
use super::*;
use crate::{omp_journal::OmpJournal, scheduler::DurableWorkflow};
use cerulean_domain::workflow::{Role, TaskState, WorkflowCommand};
use windows_sys::Win32::Storage::FileSystem::{
    FileDispositionInfo, SetFileInformationByHandle, FILE_DISPOSITION_INFO,
};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RestoreReview {
    pub intent_digest: String,
    pub workflow_head: String,
    pub prompt_head: String,
    pub tool_head: String,
    pub write: WriteInspection,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RestoreReceipt {
    pub version: u32,
    pub review_digest: String,
    pub user_id: String,
    pub restored_sha256: Option<String>,
    pub at: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Decision {
    version: u32,
    review: RestoreReview,
    user_id: String,
    at: String,
}
struct Bound {
    intent: DispatchIntent,
    plan: PlanRevision,
    prompt: OmpJournal,
    tools: OmpToolJournal,
    _pins: Vec<File>,
    head: String,
}
fn bind(
    store: &DurableWorkflow,
    service_root: &Path,
    task: &str,
    attempt: &str,
) -> Result<Bound, String> {
    let (plan, _, intent, _) = crate::omp_execution::scope(store, task, attempt)?;
    require(
        store.state().tasks[task].state == TaskState::Unknown
            && store.state().tasks[task].attempt.as_deref() == Some(attempt),
        "File restore requires original unknown attempt",
    )?;
    require(store.history().iter().any(|e|matches!(&e.body.event.command,WorkflowCommand::DispatchIntent{task:t,attempt:a} if t==task && a==attempt)),"File restore lacks workflow dispatch")?;
    let path = store.omp_attempt_path(attempt)?;
    require(path.is_dir(), "Original OMP prompt evidence missing")?;
    let prompt = OmpJournal::open(&path, &intent)?;
    require(
        prompt.observation().dispatched
            && !prompt.observation().not_started
            && prompt.observation().terminal.as_ref().is_none_or(|r| {
                r.status != cerulean_adapters::omp_protocol::PromptStatus::Completed
            }),
        "File restore refuses unsent or completed original prompt",
    )?;
    let pins = pin_absolute(service_root)?;
    let tools = OmpToolJournal::open(&service_root.join("events"), &intent)?;
    let head = store
        .history()
        .last()
        .ok_or("Workflow history missing")?
        .hash
        .clone();
    Ok(Bound {
        intent,
        plan,
        prompt,
        tools,
        _pins: pins,
        head,
    })
}
fn make_review(bound: &Bound, service_root: &Path, call_id: &str) -> Result<RestoreReview, String> {
    Ok(RestoreReview {
        intent_digest: digest(&bound.intent)?,
        workflow_head: bound.head.clone(),
        prompt_head: bound.prompt.head().into(),
        tool_head: bound.tools.head().into(),
        write: inspect_locked(service_root, &bound.intent, call_id, &bound.tools)?,
    })
}
pub fn review(
    store: &DurableWorkflow,
    service_root: &Path,
    task: &str,
    attempt: &str,
    call_id: &str,
) -> Result<RestoreReview, String> {
    let bound = bind(store, service_root, task, attempt)?;
    make_review(&bound, service_root, call_id)
}
/// `user` comes from the authenticated local user command channel. The caller
/// must display `reviewed` and invoke only after the user chooses Restore original.
/// This API is deliberately absent from HostService and model tool definitions.
pub fn restore_original(
    store: &DurableWorkflow,
    service_root: &Path,
    task: &str,
    attempt: &str,
    user: &str,
    reviewed: &RestoreReview,
) -> Result<RestoreReceipt, String> {
    restore_observed(store, service_root, task, attempt, user, reviewed, |_| {
        Ok(())
    })
}
#[allow(clippy::too_many_arguments)]
fn restore_observed(
    store: &DurableWorkflow,
    service_root: &Path,
    task: &str,
    attempt: &str,
    user: &str,
    reviewed: &RestoreReview,
    mut observe: impl FnMut(&str) -> Result<(), String>,
) -> Result<RestoreReceipt, String> {
    require(
        store
            .configuration()
            .principals
            .iter()
            .any(|p| p.id == user && p.role == Role::User),
        "Only authenticated user can restore files",
    )?;
    let bound = bind(store, service_root, task, attempt)?;
    require(
        bound
            .plan
            .policy
            .tools
            .contains(&ToolPermission::WriteFiles),
        "Restore needs approved file write permission",
    )?;
    let fresh = make_review(&bound, service_root, &reviewed.write.call_id)?;
    require(
        fresh == *reviewed,
        "Restore review is stale; inspect current file again",
    )?;
    let image_root = service_root
        .join("writes")
        .join(hash(reviewed.write.call_id.as_bytes()));
    let _image_pins = pin_absolute(&image_root)?;
    let original = if let Some(expected) = &reviewed.write.before_sha256 {
        let bytes = load(&image_root, "before.bin", MAX_FILE)?;
        require(hash(&bytes) == *expected, "Original restore image changed")?;
        Some(bytes)
    } else {
        None
    };
    let project_pins = pin_absolute(Path::new(&bound.intent.binding.workspace))?;
    let project = Root {
        write_paths: None,
        path: root_path(&bound.intent.binding.workspace)?,
        _pins: project_pins,
        protected: BTreeSet::new(),
        read: true,
        write: true,
        writes: service_root.join("writes"),
        intent_digest: digest(&bound.intent)?,
    };
    let (path, _parents) = checked_parent(&project, &reviewed.write.path, false)
        .map_err(|_| "Restore target parent unavailable")?;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .access_mode(0x80000000 | 0x40000000 | 0x00010000)
        .share_mode(0)
        .custom_flags(FLAGS);
    let (mut target, current) = match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (None, None),
        Err(_) => return Err("Restore target metadata unavailable".into()),
        Ok(_) => {
            let mut file = options
                .open(&path)
                .map_err(|_| "Restore target cannot be exclusively locked")?;
            let data = bytes(&mut file).map_err(|_| "Restore target invalid")?;
            (Some(file), Some(data))
        }
    };
    require(
        current.as_deref().map(hash) == reviewed.write.current_sha256,
        "Restore target changed after review",
    )?;
    let repairs = service_root.join("repairs");
    if fs::symlink_metadata(&repairs).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
        fs::create_dir(&repairs).map_err(|_| "Restore evidence directory unavailable")?;
    }
    let mut pins = pin_absolute(&repairs)?;
    let repair = repairs.join(hash(reviewed.write.call_id.as_bytes()));
    fs::create_dir(&repair).map_err(|_| "This file restore already has an intent; do not retry")?;
    pins.push(pin_directory(&repair)?);
    if let Some(data) = &current {
        publish(&repair, "current.bin", data)?;
    }
    if let Some(data) = &original {
        publish(&repair, "original.bin", data)?;
    }
    let decision = Decision {
        version: 1,
        review: reviewed.clone(),
        user_id: user.into(),
        at: chrono::Utc::now().to_rfc3339(),
    };
    publish(
        &repair,
        "decision.json",
        &serde_json::to_vec(&decision).map_err(|_| "Restore decision encoding failed")?,
    )?;
    observe("prepared")?;
    if let Some(data) = &original {
        if target.is_none() {
            target = Some(
                options
                    .create_new(true)
                    .open(&path)
                    .map_err(|_| "Restore target changed or cannot be created")?,
            );
        }
        let file = target.as_mut().unwrap();
        validate_file(file).map_err(|_| "Restore target invalid")?;
        file.seek(SeekFrom::Start(0))
            .and_then(|_| file.write_all(&data[..data.len() / 2]))
            .map_err(|_| "Restore write outcome unknown")?;
        observe("partial")?;
        file.write_all(&data[data.len() / 2..])
            .and_then(|_| file.set_len(data.len() as u64))
            .and_then(|_| file.sync_all())
            .map_err(|_| "Restore sync outcome unknown")?;
    } else if let Some(file) = &target {
        let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
        if unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle(),
                FileDispositionInfo,
                (&disposition as *const FILE_DISPOSITION_INFO).cast(),
                std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
            )
        } == 0
        {
            return Err("Restore deletion outcome unknown".into());
        }
        observe("partial")?;
    }
    drop(target);
    observe("mutated")?;
    let restored = if original.is_some() {
        let mut file = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .custom_flags(FLAGS)
            .open(&path)
            .map_err(|_| "Restored file observation unavailable")?;
        Some(hash(
            &bytes(&mut file).map_err(|_| "Restored file observation invalid")?,
        ))
    } else {
        require(
            fs::symlink_metadata(&path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
            "Restored absence not confirmed",
        )?;
        None
    };
    require(
        restored == reviewed.write.before_sha256,
        "Restored file differs from original",
    )?;
    let receipt = RestoreReceipt {
        version: 1,
        review_digest: digest(reviewed)?,
        user_id: user.into(),
        restored_sha256: restored,
        at: chrono::Utc::now().to_rfc3339(),
    };
    publish(
        &repair,
        "result.json",
        &serde_json::to_vec(&receipt).map_err(|_| "Restore result encoding failed")?,
    )?;
    observe("recorded")?;
    Ok(receipt)
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RestoreInspection {
    pub review_digest: String,
    pub result_recorded: bool,
    pub current_matches_original: bool,
    pub current_sha256: Option<String>,
}
/// Observes an original restore decision without applying it again.
pub fn inspect_restore(
    store: &DurableWorkflow,
    service_root: &Path,
    task: &str,
    attempt: &str,
    call_id: &str,
) -> Result<RestoreInspection, String> {
    let bound = bind(store, service_root, task, attempt)?;
    let current = make_review(&bound, service_root, call_id)?;
    let root = service_root.join("repairs").join(hash(call_id.as_bytes()));
    let _pins = pin_absolute(&root)?;
    let entries = fs::read_dir(&root)
        .map_err(|_| "Restore records unavailable")?
        .take(6)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Restore records unavailable")?;
    require(entries.len() <= 4, "Unexpected restore record")?;
    for e in entries {
        crate::host::ordinary(&e.path(), false)?;
        require(
            [
                "current.bin",
                "original.bin",
                "decision.json",
                "result.json",
            ]
            .iter()
            .any(|s| e.file_name() == *s),
            "Unexpected restore record",
        )?;
    }
    let decision: Decision = serde_json::from_slice(&load(&root, "decision.json", MAX_RECORD)?)
        .map_err(|_| "Restore decision invalid or incomplete")?;
    let r = &decision.review;
    require(
        decision.version == 1
            && r.intent_digest == current.intent_digest
            && r.workflow_head == current.workflow_head
            && r.prompt_head == current.prompt_head
            && r.tool_head == current.tool_head
            && r.write.call_id == current.write.call_id
            && r.write.path == current.write.path
            && r.write.write_digest == current.write.write_digest
            && r.write.before_sha256 == current.write.before_sha256
            && r.write.after_sha256 == current.write.after_sha256
            && store
                .configuration()
                .principals
                .iter()
                .any(|p| p.id == decision.user_id && p.role == Role::User),
        "Restore decision scope changed",
    )?;
    timestamp(&decision.at)?;
    for (name, expected) in [
        ("current.bin", &r.write.current_sha256),
        ("original.bin", &r.write.before_sha256),
    ] {
        if let Some(expected) = expected {
            require(
                hash(&load(&root, name, MAX_FILE)?) == *expected,
                "Restore image changed",
            )?;
        } else {
            require(
                fs::symlink_metadata(root.join(name))
                    .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
                "Unexpected restore image",
            )?;
        }
    }
    let review_digest = digest(r)?;
    let result_recorded = if root.join("result.json").exists() {
        let result: RestoreReceipt =
            serde_json::from_slice(&load(&root, "result.json", MAX_RECORD)?)
                .map_err(|_| "Restore result invalid")?;
        require(
            result.version == 1
                && result.review_digest == review_digest
                && result.user_id == decision.user_id
                && result.restored_sha256 == r.write.before_sha256,
            "Restore result differs from decision",
        )?;
        timestamp(&result.at)?;
        true
    } else {
        false
    };
    Ok(RestoreInspection {
        review_digest,
        result_recorded,
        current_matches_original: current.write.current_sha256 == r.write.before_sha256,
        current_sha256: current.write.current_sha256,
    })
}
#[cfg(test)]
mod tests;
