//! Durable desktop dispatch, separate from fixed-message connectivity trials.
//! Only a trusted controller may call this module with a locked workflow.
use crate::{
    host::ordinary,
    scheduler::{CommandContext, DurableWorkflow},
};
use cerulean_domain::{desktop_sessions::DesktopObservation, execution::*, workflow::*};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
};

#[path = "desktop_completion.rs"]
mod completion;
pub use completion::{submit_sealed, DesktopCompletion};
#[path = "desktop_continuity.rs"]
mod continuity;
#[cfg(test)]
#[path = "desktop_local_proof_tests.rs"]
mod local_proof_tests;
pub use continuity::{predecessor, prepare_continuing};
#[path = "desktop_cancellation.rs"]
pub(crate) mod cancellation;

pub use cerulean_domain::desktop_sessions::{
    DesktopTaskContinuation as TaskContinuation, DesktopTaskProof as TaskProof,
    DesktopTaskReview as TaskReview,
};

// A one-shot, in-memory capability. It cannot be reconstructed on recovery.
pub(crate) struct ReservedSend(TaskReview);
fn review_hash(r: &TaskReview) -> Result<String, String> {
    let mut copy = r.clone();
    copy.id.clear();
    digest(&copy)
}
fn text_hash(s: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(s.as_bytes()))
}
fn prompt(plan: &PlanRevision, attempt: &Attempt, marker: &str) -> Result<String, String> {
    let task = plan
        .workflow
        .tasks
        .iter()
        .find(|t| t.id == attempt.task_id)
        .ok_or("桌面任务不存在")?;
    let workspace = plan.execution_root(&task.id, &attempt.id)?;
    // JSON escapes line breaks in task text; typing never injects Enter shortcuts.
    Ok(format!("这是用户已批准的编程任务。只执行这里的任务，工作目录与模型须与当前应用配置一致；不一致请停止。任务数据：{}。工具权限沿用当前应用，遇到需用户批准的操作请等待。完成后说明修改与检查结果，并在回复末尾单独输出标记 {}。标记仅用于关联本次回复，不代表独立验收通过，不要代替主指挥宣布项目验收。",serde_json::to_string(&serde_json::json!({"projectDirectory":workspace,"taskId":task.id,"attemptId":attempt.id,"goal":task.goal,"requiredArtifacts":plan.task(&task.id)?.artifact_paths,"revisionDigest":attempt.revision_digest})).map_err(|_|"任务编码失败")?,marker))
}
fn fresh(o: &DesktopObservation) -> Result<(), String> {
    let at =
        chrono::DateTime::parse_from_rfc3339(&o.captured_at).map_err(|_| "任务观察时间无效")?;
    require(
        (0..=120).contains(&(chrono::Utc::now() - at.with_timezone(&chrono::Utc)).num_seconds()),
        "任务观察已过期",
    )
}
fn workbuddy_read_version(version: &str) -> bool {
    matches!(version, "5.7.7.0" | "5.8.3.0")
}

// A Qoder binding can outlive window layout changes while waiting for user
// approval. Keep route/document/process/model/content immutable. The fresh
// task review still pins the complete current layout, and the native sender
// compares that exact review both before focus and before typing.
pub(crate) fn same_approved_qoder_context(
    original: &DesktopObservation,
    current: &DesktopObservation,
) -> bool {
    use cerulean_domain::desktop_sessions::DesktopApp;
    if original.window.app != DesktopApp::Qoder
        || current.window.app != DesktopApp::Qoder
        || original.session_id.is_none()
        || !current.draft_empty
        || current.busy
        || !(1..=1000).contains(&current.node_count)
        || sha256(&current.layout_digest).is_err()
    {
        return false;
    }
    let mut expected = original.clone();
    expected.captured_at = current.captured_at.clone();
    expected.layout_digest = current.layout_digest.clone();
    expected.node_count = current.node_count;
    expected == *current
}

pub(crate) fn validate_read_task_window(
    window: &cerulean_domain::desktop_sessions::DesktopWindow,
) -> Result<(), String> {
    use cerulean_domain::desktop_sessions::DesktopApp;
    require(
        match window.app {
            DesktopApp::Qoder => cerulean_domain::desktop_upgrade::qoder_candidate(&window.version),
            DesktopApp::Workbuddy => workbuddy_read_version(&window.version),
        },
        "当前桌面版本尚未接入原任务读取",
    )
}

// Dispatch has an explicit allowlist independent of read support. Adding an
// observed version alone must not silently grant its action capability.
pub(crate) fn validate_task_window(
    window: &cerulean_domain::desktop_sessions::DesktopWindow,
) -> Result<(), String> {
    use cerulean_domain::desktop_sessions::DesktopApp;
    require(
        match window.app {
            DesktopApp::Qoder => cerulean_domain::desktop_upgrade::qoder_candidate(&window.version),
            DesktopApp::Workbuddy => workbuddy_read_version(&window.version),
        },
        "当前桌面应用或版本尚未接入编程调度；需要先完成适配",
    )
}
fn validate(store: &DurableWorkflow, r: &TaskReview) -> Result<(), String> {
    timestamp(&r.observation.captured_at)?;
    for h in [
        &r.id,
        &r.binding_id,
        &r.prompt_sha256,
        &r.bridge_sha256,
        &r.workflow_hash,
    ] {
        sha256(h)?;
    }
    require(
        r.id == review_hash(r)? && r.prompt_sha256 == text_hash(&r.prompt),
        "任务发送内容摘要不符",
    )?;
    let prefix = store
        .history()
        .get(..r.workflow_sequence as usize)
        .ok_or("任务派发锚点越界")?;
    require(
        prefix.last().is_some_and(|e| e.hash == r.workflow_hash),
        "任务派发锚点不符",
    )?;
    let plan = prefix
        .iter()
        .rev()
        .find_map(|e| {
            if let Some(ExecutionEvidence::Revision { revision }) = &e.body.context.execution {
                Some(revision)
            } else {
                None
            }
        })
        .ok_or("原任务方案缺失")?;
    let approval = prefix
        .iter()
        .rev()
        .find_map(|e| {
            if let Some(ExecutionEvidence::Approval { approval }) = &e.body.context.execution {
                Some(approval)
            } else {
                None
            }
        })
        .ok_or("原用户批准缺失")?;
    approval.validate(
        plan,
        "user",
        store
            .configuration()
            .execution_scope
            .as_ref()
            .ok_or("执行范围缺失")?
            .policy_version,
    )?;
    let replay = Workflow::replay(
        store.configuration().principals.clone(),
        &prefix
            .iter()
            .map(|e| e.body.event.clone())
            .collect::<Vec<_>>(),
    )?;
    let task = replay
        .state()
        .tasks
        .get(&r.attempt.task_id)
        .ok_or("派发任务缺失")?;
    require(
        task.state == TaskState::Assigned
            && task.attempt.as_deref() == Some(&r.attempt.id)
            && task.fence == r.attempt.fence
            && replay.state().revision_requested.is_none(),
        "派发锚点不是已分配任务",
    )?;
    require(
        store.attempt_record(&r.attempt.task_id, &r.attempt.id) == Some(&r.attempt)
            && plan.project.id == r.project_id
            && plan.approval_digest()? == r.attempt.revision_digest,
        "派发尝试不符",
    )?;
    let selected = plan
        .task(&r.attempt.task_id)?
        .desktop_worker
        .as_ref()
        .ok_or("原方案没有桌面执行器")?;
    selected.validate_attempt(&r.attempt.binding)?;
    let mut original = selected.binding.observation.clone();
    original.captured_at = r.observation.captured_at.clone();
    require(
        (if r.continuation.is_some() {
            original.window == r.observation.window
                && original.document_runtime_id == r.observation.document_runtime_id
                && original.session_id == r.observation.session_id
                && original.model_label == r.observation.model_label
                && r.observation.draft_empty
                && !r.observation.busy
                && (1..=1000).contains(&r.observation.node_count)
        } else if original.window.app == cerulean_domain::desktop_sessions::DesktopApp::Qoder {
            same_approved_qoder_context(&original, &r.observation)
        } else {
            original == r.observation
        }) && r.binding_id == selected.binding.id
            && r.bridge_sha256 == selected.binding.bridge_sha256,
        "派发观察或桥接与批准关联不符",
    )?;
    validate_task_window(&r.observation.window)?;
    require(
        (2..=128).contains(&r.baseline.len()),
        "派发需要至少两轮可核对历史",
    )?;
    sha256(&r.observation.content_digest)?;
    sha256(&r.observation.layout_digest)?;
    if let Some(c) = &r.continuation {
        identifier(&c.previous_attempt_id)?;
        sha256(&c.previous_review_digest)?;
    }
    for h in &r.baseline {
        sha256(h)?;
    }
    let marker = format!(
        "CL_TASK_{}",
        &digest(&(&r.attempt, &r.workflow_hash, &r.observation.captured_at))?[..32]
    );
    require(
        r.marker == marker
            && r.prompt
                == crate::task_rework::append_prompt(
                    prompt(plan, &r.attempt, &marker)?,
                    store.history(),
                    &r.attempt.task_id,
                    &r.attempt.id,
                )?
            && r.max_wait_seconds == plan.policy.max_seconds.min(50)
            && r.prompt.len() <= 16000
            && !r.prompt.chars().any(char::is_control),
        "派发提示词与批准任务不符",
    )
}
pub fn prepare(
    store: &DurableWorkflow,
    task: &str,
    id: &str,
    observation: DesktopObservation,
    baseline: Vec<String>,
) -> Result<TaskReview, String> {
    prepare_inner(store, task, id, observation, baseline, None)
}
fn prepare_inner(
    store: &DurableWorkflow,
    task: &str,
    id: &str,
    observation: DesktopObservation,
    baseline: Vec<String>,
    continuation: Option<TaskContinuation>,
) -> Result<TaskReview, String> {
    observation.window.app.require_available()?;
    let attempt = store
        .attempt_record(task, id)
        .ok_or("桌面尝试尚未分配")?
        .clone();
    let plan = store.revision().ok_or("桌面方案缺失")?;
    let selection = plan
        .task(task)?
        .desktop_worker
        .as_ref()
        .ok_or("桌面配置缺失")?;
    let head = store.history().last().ok_or("工作流为空")?;
    let marker = format!(
        "CL_TASK_{}",
        &digest(&(&attempt, &head.hash, &observation.captured_at))?[..32]
    );
    let mut r = TaskReview {
        id: String::new(),
        binding_id: selection.binding.id.clone(),
        marker: marker.clone(),
        prompt: crate::task_rework::append_prompt(
            prompt(plan, &attempt, &marker)?,
            store.history(),
            task,
            id,
        )?,
        prompt_sha256: String::new(),
        bridge_sha256: selection.binding.bridge_sha256.clone(),
        observation,
        baseline,
        attempt,
        project_id: plan.project.id.clone(),
        workflow_sequence: head.body.event.sequence,
        workflow_hash: head.hash.clone(),
        max_wait_seconds: plan.policy.max_seconds.min(50),
        continuation,
    };
    r.prompt_sha256 = text_hash(&r.prompt);
    r.id = review_hash(&r)?;
    validate(store, &r)?;
    fresh(&r.observation)?;
    Ok(r)
}
fn directory(root: &Path, r: &TaskReview) -> Result<std::path::PathBuf, String> {
    ordinary(root, true)?;
    let dir = root.join("desktop-task-dispatch-v1");
    if !dir.exists() {
        fs::create_dir(&dir).map_err(|_| "创建任务派发目录失败")?;
    }
    ordinary(&dir, true)?;
    let path = dir.join(digest(&(&r.project_id, &r.attempt.id))?);
    if !path.exists() {
        require(
            fs::read_dir(&dir)
                .map_err(|_| "任务派发目录不可读")?
                .take(512)
                .count()
                < 512,
            "任务派发记录超限",
        )?;
        fs::create_dir(&path).map_err(|_| "创建任务尝试目录失败")?;
    }
    ordinary(&path, true)?;
    Ok(path)
}
pub(crate) fn publish<T: Serialize>(dir: &Path, name: &str, value: &T) -> Result<(), String> {
    let pending = dir.join(format!("{name}.pending"));
    let target = dir.join(format!("{name}.json"));
    require(
        !target.exists() && !pending.exists(),
        "原派发记录已存在，禁止重复发送或覆盖",
    )?;
    let bytes = serde_json::to_vec(value).map_err(|_| "派发记录编码失败")?;
    require(bytes.len() <= 131072, "派发记录超限")?;
    let mut f = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&pending)
        .map_err(|_| "派发记录已存在或不可写，禁止发送")?;
    f.write_all(&bytes)
        .and_then(|_| f.sync_all())
        .map_err(|_| "派发记录同步失败，保留待核对")?;
    drop(f);
    fs::hard_link(&pending, &target).map_err(|_| "派发发布失败，禁止发送")?;
    fs::remove_file(&pending).map_err(|_| "派发临时记录待核对".into())
}
pub(crate) fn load<T: for<'a> Deserialize<'a>>(dir: &Path, name: &str) -> Result<T, String> {
    ordinary(dir, true)?;
    let mut saved = None;
    for extension in ["json", "pending"] {
        let path = dir.join(format!("{name}.{extension}"));
        if !path.exists() {
            continue;
        }
        ordinary(&path, false)?;
        let mut bytes = Vec::new();
        fs::File::open(path)
            .map_err(|_| "派发记录不可读")?
            .take(131073)
            .read_to_end(&mut bytes)
            .map_err(|_| "派发记录读取失败")?;
        require(bytes.len() <= 131072, "派发记录过大")?;
        if let Some(prior) = &saved {
            require(prior == &bytes, "派发正式与临时记录冲突")?;
        }
        saved = Some(bytes);
    }
    serde_json::from_slice(&saved.ok_or("派发记录缺失")?)
        .map_err(|_| "派发记录不完整，禁止重发".into())
}
fn validate_proof(r: &TaskReview, p: &TaskProof) -> Result<(), String> {
    use cerulean_domain::desktop_sessions::DesktopApp;
    sha256(&p.reply_sha256)?;
    if let Some(reader) = &p.reader_sha256 {
        sha256(reader)?;
    }
    if let Some(round) = &p.round_sha256 {
        sha256(round)?;
    }
    timestamp(&p.observed_at)?;
    match r.observation.window.app {
        DesktopApp::Qoder => require(
            r.observation.session_id.is_some()
                && p.session_id == r.observation.session_id
                && p.local_binding.is_none(),
            "Qoder回复会话身份不符",
        )?,
        DesktopApp::Workbuddy => {
            let local = p
                .local_binding
                .as_ref()
                .ok_or("WorkBuddy回复缺少本地关联证据")?;
            let o = &local.observation;
            sha256(&local.binding_id)?;
            sha256(&local.task_review_digest)?;
            sha256(&o.content_digest)?;
            sha256(&o.layout_digest)?;
            require(
                workbuddy_read_version(&r.observation.window.version)
                    && r.observation.session_id.is_none()
                    && p.session_id.is_none()
                    && o.session_id.is_none()
                    && local.binding_id == r.binding_id
                    && local.task_review_digest == r.id
                    && o.window == r.observation.window
                    && o.document_runtime_id == r.observation.document_runtime_id
                    && !o.document_runtime_id.trim().is_empty()
                    && o.model_label == r.observation.model_label
                    && o.model_label.as_ref().is_some_and(|m| !m.trim().is_empty())
                    && o.captured_at == p.observed_at
                    && o.draft_empty
                    && !o.busy
                    && (1..=1000).contains(&o.node_count)
                    && p.round_sha256.is_some(),
                "WorkBuddy回复与原本地关联不符",
            )?;
        }
    }
    require(
        p.completed
            && p.marker == r.marker
            && p.prompt_sha256 == r.prompt_sha256
            && p.observed_baseline.len() >= 2
            && r.baseline.ends_with(&p.observed_baseline)
            && chrono::DateTime::parse_from_rfc3339(&p.observed_at).unwrap()
                >= chrono::DateTime::parse_from_rfc3339(&r.observation.captured_at)
                    .map_err(|_| "观察时间无效")?,
        "任务回复与原派发不符",
    )
}
/// Persist intent and workflow DispatchIntent before the only transport invocation.
/// Completion is observed evidence only; independent artifact verification remains mandatory.
pub fn dispatch(
    root: &Path,
    store: &mut DurableWorkflow,
    r: &TaskReview,
    send: impl FnOnce(&TaskReview) -> Result<TaskProof, String>,
) -> Result<TaskProof, String> {
    dispatch_and_seal_observed(root, store, r, send, |_| Ok(()))
}
pub(crate) fn dispatch_observed(
    root: &Path,
    store: &mut DurableWorkflow,
    r: &TaskReview,
    send: impl FnOnce(&TaskReview) -> Result<TaskProof, String>,
    mut checkpoint: impl FnMut(&str) -> Result<(), String>,
) -> Result<TaskProof, String> {
    let permit = reserve_observed(root, store, r, &mut checkpoint)?;
    send_reserved(root, &permit.0, send, &mut checkpoint)
}
pub(crate) fn reserve(
    root: &Path,
    store: &mut DurableWorkflow,
    r: &TaskReview,
) -> Result<ReservedSend, String> {
    reserve_observed(root, store, r, &mut |_| Ok(()))
}
fn reserve_observed(
    root: &Path,
    store: &mut DurableWorkflow,
    r: &TaskReview,
    checkpoint: &mut impl FnMut(&str) -> Result<(), String>,
) -> Result<ReservedSend, String> {
    r.observation.window.app.require_available()?;
    validate(store, r)?;
    if !cerulean_domain::desktop_upgrade::qoder_established(&r.observation.window.version) {
        let binding = &store
            .revision()
            .ok_or("方案缺失")?
            .task(&r.attempt.task_id)?
            .desktop_worker
            .as_ref()
            .ok_or("桌面配置缺失")?
            .binding;
        cerulean_domain::desktop_upgrade::require_upgrade_trial(
            binding,
            &crate::host::desktop_sessions::load_trials(root)?,
            &crate::host::desktop_sessions::fingerprint(),
        )?;
    }
    continuity::validate_chain(root, store, r)?;
    fresh(&r.observation)?;
    require(
        store
            .history()
            .last()
            .is_some_and(|e| e.hash == r.workflow_hash),
        "工作流已变化，需要重新核对",
    )?;
    let used = store
        .history()
        .iter()
        .filter(|e| match &e.body.event.command {
            WorkflowCommand::DispatchIntent { task, attempt } => store
                .attempt_record(task, attempt)
                .is_some_and(|a| a.revision_digest == r.attempt.revision_digest),
            _ => false,
        })
        .count();
    require(
        used < store.revision().ok_or("当前方案缺失")?.policy.max_requests as usize,
        "当前批准方案的派发次数已用尽",
    )?;
    let dir = directory(root, r)?;
    publish(&dir, "intent", r)?;
    checkpoint("intent-published")?;
    store.command(
        &format!("desktop-dispatch-{}", &r.id[..32]),
        &store.configuration().core_id.clone(),
        WorkflowCommand::DispatchIntent {
            task: r.attempt.task_id.clone(),
            attempt: r.attempt.id.clone(),
        },
        CommandContext {
            thread_id: Some(r.attempt.binding.session_id.clone()),
            cwd: Some(r.attempt.binding.workspace.clone()),
            ..Default::default()
        },
        false,
    )?;
    checkpoint("workflow-dispatched")?;
    Ok(ReservedSend(r.clone()))
}
fn send_reserved(
    root: &Path,
    r: &TaskReview,
    send: impl FnOnce(&TaskReview) -> Result<TaskProof, String>,
    checkpoint: &mut impl FnMut(&str) -> Result<(), String>,
) -> Result<TaskProof, String> {
    let dir = directory(root, r)?;
    let proof = send(r)?;
    validate_proof(r, &proof)?;
    checkpoint("reply-observed")?;
    publish(&dir, "proof", &proof)?;
    checkpoint("proof-published")?;
    Ok(proof)
}
/// Read persisted evidence only. This function never invokes a transport or rewrites files.
pub fn inspect(
    root: &Path,
    store: &DurableWorkflow,
    project: &str,
    attempt: &str,
) -> Result<(TaskReview, Option<TaskProof>), String> {
    let result = inspect_single(root, store, project, attempt)?;
    continuity::validate_chain(root, store, &result.0)?;
    Ok(result)
}
fn inspect_single(
    root: &Path,
    store: &DurableWorkflow,
    project: &str,
    attempt: &str,
) -> Result<(TaskReview, Option<TaskProof>), String> {
    let (review, proof) = inspect_without_cancellation(root, store, project, attempt)?;
    cancellation::inspect(root, store, &review)?;
    Ok((review, proof))
}
fn inspect_without_cancellation(
    root: &Path,
    store: &DurableWorkflow,
    project: &str,
    attempt: &str,
) -> Result<(TaskReview, Option<TaskProof>), String> {
    identifier(project)?;
    identifier(attempt)?;
    let dir = root.join("desktop-task-dispatch-v1");
    ordinary(root, true)?;
    ordinary(&dir, true)?;
    let dir = dir.join(digest(&(project, attempt))?);
    let review: TaskReview = load(&dir, "intent")?;
    validate(store, &review)?;
    require(
        review.project_id == project && review.attempt.id == attempt,
        "派发记录归属不符",
    )?;
    let proof = if dir.join("proof.json").exists() || dir.join("proof.pending").exists() {
        require(
            store
                .history()
                .get(review.workflow_sequence as usize)
                .is_some_and(|e| {
                    e.body.operation_id == format!("desktop-dispatch-{}", &review.id[..32])
                        && e.body.event.principal == store.configuration().core_id
                        && e.body.event.command
                            == WorkflowCommand::DispatchIntent {
                                task: review.attempt.task_id.clone(),
                                attempt: review.attempt.id.clone(),
                            }
                        && e.body.context.thread_id.as_ref()
                            == Some(&review.attempt.binding.session_id)
                        && e.body.context.cwd.as_ref() == Some(&review.attempt.binding.workspace)
                }),
            "回复记录缺少对应工作流派发证据",
        )?;
        let p = load(&dir, "proof")?;
        validate_proof(&review, &p)?;
        Some(p)
    } else {
        None
    };
    completion::inspect_completion(root, store, &review, proof.as_ref())?;
    Ok((review, proof))
}

/// Recover using one read-only transport call; retain the exact original intent.
pub fn recover(
    root: &Path,
    store: &DurableWorkflow,
    project: &str,
    attempt: &str,
    read: impl FnOnce(&TaskReview) -> Result<TaskProof, String>,
) -> Result<TaskProof, String> {
    require(
        !is_retired(store, attempt),
        "本地跟踪已结束，禁止恢复原尝试",
    )?;
    let (review, proof) = inspect(root, store, project, attempt)?;
    cancellation::review::require_unrestricted(root, store, &review)?;
    let dir = directory(root, &review)?;
    require(
        !dir.join("stop-proof.json").exists() && !dir.join("stop-proof.pending").exists(),
        "取消证明已保留，不能补采完成结果",
    )?;
    if let Some(proof) = proof {
        return Ok(proof);
    }
    require(
        store
            .history()
            .get(review.workflow_sequence as usize)
            .is_some_and(|e| {
                e.body.operation_id == format!("desktop-dispatch-{}", &review.id[..32])
                    && e.body.event.command
                        == WorkflowCommand::DispatchIntent {
                            task: review.attempt.task_id.clone(),
                            attempt: review.attempt.id.clone(),
                        }
            }),
        "没有实际工作流派发，只保留发送意图",
    )?;
    let proof = read(&review)?;
    validate_proof(&review, &proof)?;
    let dir = directory(root, &review)?;
    publish(&dir, "proof", &proof)?;
    Ok(proof)
}

/// Enumerate only bounded, immutable task records. Empty/partial reservations
/// are unresolved faults, never evidence that input did not occur.
pub fn recorded_attempts(root: &Path) -> Result<Vec<(String, String)>, String> {
    ordinary(root, true)?;
    let dir = root.join("desktop-task-dispatch-v1");
    if !dir.exists() {
        return Ok(vec![]);
    }
    ordinary(&dir, true)?;
    let mut result = Vec::new();
    for (index, entry) in fs::read_dir(&dir)
        .map_err(|_| "任务派发目录不可读")?
        .enumerate()
    {
        require(index < 512, "任务派发记录超限")?;
        let path = entry.map_err(|_| "任务派发目录项不可读")?.path();
        ordinary(&path, true)?;
        let review: TaskReview = load(&path, "intent")?;
        require(
            path.file_name().and_then(|s| s.to_str())
                == Some(digest(&(&review.project_id, &review.attempt.id))?.as_str()),
            "任务派发目录身份不符",
        )?;
        for (n, file) in fs::read_dir(&path)
            .map_err(|_| "任务派发文件不可读")?
            .enumerate()
        {
            require(n < 14, "未知任务派发文件")?;
            let file = file.map_err(|_| "任务派发文件不可读")?.path();
            ordinary(&file, false)?;
            require(
                matches!(
                    file.file_name().and_then(|s| s.to_str()),
                    Some(
                        "intent.json"
                            | "intent.pending"
                            | "proof.json"
                            | "proof.pending"
                            | "seal-intent.json"
                            | "seal-intent.pending"
                            | "completion.json"
                            | "completion.pending"
                            | "stop-request.json"
                            | "stop-request.pending"
                            | "stop-intent.json"
                            | "stop-intent.pending"
                            | "stop-proof.json"
                            | "stop-proof.pending"
                    )
                ),
                "未知任务派发文件",
            )?;
        }
        result.push((review.project_id, review.attempt.id));
    }
    Ok(result)
}

pub(crate) fn dispatch_and_seal_observed(
    root: &Path,
    store: &mut DurableWorkflow,
    r: &TaskReview,
    send: impl FnOnce(&TaskReview) -> Result<TaskProof, String>,
    mut checkpoint: impl FnMut(&str) -> Result<(), String>,
) -> Result<TaskProof, String> {
    let fresh_attempt = store
        .state()
        .tasks
        .get(&r.attempt.task_id)
        .is_some_and(|t| {
            t.state == TaskState::Assigned && t.attempt.as_ref() == Some(&r.attempt.id)
        });
    let result = (|| {
        let p = dispatch_observed(root, store, r, send, &mut checkpoint)?;
        completion::seal_fresh(root, store, r, &p, &mut checkpoint)?;
        submit_sealed(root, store, &r.project_id, &r.attempt.id)?;
        checkpoint("workflow-submitted")?;
        Ok(p)
    })();
    if result.is_err() && fresh_attempt {
        completion::mark_unknown(store, r)?;
    }
    result
}
/// Only a newly observed reply can be sampled. An old proof without a complete
/// seal remains unresolved; recovery never silently adopts newer file contents.
pub fn recover_and_submit(
    root: &Path,
    store: &mut DurableWorkflow,
    project: &str,
    attempt: &str,
    read: impl FnOnce(&TaskReview) -> Result<TaskProof, String>,
) -> Result<TaskProof, String> {
    let (review, prior) = inspect(root, store, project, attempt)?;
    let newly_observed = prior.is_none();
    let proof = recover(root, store, project, attempt, read)?;
    if newly_observed {
        completion::seal_fresh(root, store, &review, &proof, &mut |_| Ok(()))?;
    }
    submit_sealed(root, store, project, attempt)?;
    Ok(proof)
}

/// Only the process that persisted the dispatch can consume this capability.
pub(crate) fn execute_reserved(
    root: &Path,
    store: &mut DurableWorkflow,
    permit: ReservedSend,
    send: impl FnOnce(&TaskReview) -> Result<TaskProof, String>,
) -> Result<TaskProof, String> {
    let r = &permit.0;
    let result = (|| {
        let p = send_reserved(root, r, send, &mut |_| Ok(()))?;
        completion::seal_fresh(root, store, r, &p, &mut |_| Ok(()))?;
        submit_sealed(root, store, &r.project_id, &r.attempt.id)?;
        Ok(p)
    })();
    if result.is_err() {
        completion::mark_unknown(store, r)?;
    }
    result
}

/// A local user disposition never claims a remote cancellation or successful result.
pub fn is_retired(store: &DurableWorkflow, attempt: &str) -> bool {
    store.history().iter().any(|e| matches!(&e.body.event.command, WorkflowCommand::RetireUnknownDesktop { attempt: a, .. } if a == attempt))
}
pub fn retirement_identity(
    root: &Path,
    store: &DurableWorkflow,
    project: &str,
    attempt: &str,
) -> Result<(String, String), String> {
    let (review, proof) = inspect(root, store, project, attempt)?;
    let state = store
        .state()
        .tasks
        .get(&review.attempt.task_id)
        .ok_or("原任务缺失")?;
    require(
        state.state == TaskState::Unknown && state.attempt.as_deref() == Some(attempt),
        "只能结束当前未知桌面尝试",
    )?;
    require(
        store.revision().ok_or("方案缺失")?.approval_digest()? == review.attempt.revision_digest,
        "原批准方案已变化",
    )?;
    require(store.history().iter().any(|e| matches!(&e.body.event.command, WorkflowCommand::DispatchIntent { task, attempt: a } if task == &review.attempt.task_id && a == attempt)), "缺少原派发记录")?;
    let completion = completion::inspect_completion(root, store, &review, proof.as_ref())?;
    let dir = root
        .join("desktop-task-dispatch-v1")
        .join(digest(&(project, attempt))?);
    let mut files = std::collections::BTreeMap::new();
    for item in fs::read_dir(&dir).map_err(|_| "桌面派发记录不可读")? {
        let path = item.map_err(|_| "桌面派发记录不可读")?.path();
        ordinary(&path, false)?;
        let mut bytes = Vec::new();
        fs::File::open(&path)
            .map_err(|_| "桌面派发记录不可读")?
            .take(131073)
            .read_to_end(&mut bytes)
            .map_err(|_| "桌面派发记录不可读")?;
        require(bytes.len() <= 131072, "桌面派发记录超限")?;
        files.insert(
            path.file_name()
                .ok_or("记录名称缺失")?
                .to_string_lossy()
                .into_owned(),
            digest(&bytes)?,
        );
    }
    Ok((
        review.attempt.task_id.clone(),
        digest(&(review, proof, completion, files))?,
    ))
}
