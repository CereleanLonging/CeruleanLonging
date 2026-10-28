//! One-time artifact sealing for observed desktop task replies.
use super::*;
use crate::artifacts::ArtifactObserver;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SealIntent {
    review_digest: String,
    proof_digest: String,
    created_at: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DesktopCompletion {
    pub version: u32,
    pub review_digest: String,
    pub proof_digest: String,
    pub seal_digest: String,
    pub receipt: ExecutionReceipt,
    pub manifest: ArtifactManifest,
    pub sealed_at: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SealedRecord {
    body: DesktopCompletion,
    hash: String,
}
fn original_plan<'a>(
    store: &'a DurableWorkflow,
    r: &TaskReview,
) -> Result<&'a PlanRevision, String> {
    store
        .history()
        .get(..r.workflow_sequence as usize)
        .ok_or("派发锚点越界")?
        .iter()
        .rev()
        .find_map(|e| match &e.body.context.execution {
            Some(ExecutionEvidence::Revision { revision }) => Some(revision),
            _ => None,
        })
        .ok_or("原批准方案缺失".into())
}
fn path(root: &Path, r: &TaskReview) -> Result<std::path::PathBuf, String> {
    let dir = root
        .join("desktop-task-dispatch-v1")
        .join(digest(&(&r.project_id, &r.attempt.id))?);
    ordinary(root, true)?;
    ordinary(&root.join("desktop-task-dispatch-v1"), true)?;
    ordinary(&dir, true)?;
    Ok(dir)
}
fn exists(dir: &Path, name: &str) -> bool {
    dir.join(format!("{name}.json")).exists() || dir.join(format!("{name}.pending")).exists()
}
fn check_current(root: &Path, store: &DurableWorkflow, r: &TaskReview) -> Result<(), String> {
    require(
        store.revision().is_some_and(|p| {
            p.approval_digest()
                .is_ok_and(|h| h == r.attempt.revision_digest)
        }) && store.state().revision_requested.is_none(),
        "当前方案已变化，不能提交原桌面任务",
    )?;
    let task = store
        .state()
        .tasks
        .get(&r.attempt.task_id)
        .ok_or("桌面任务缺失")?;
    require(
        task.attempt.as_ref() == Some(&r.attempt.id)
            && task.fence == r.attempt.fence
            && (!task.cancellation_requested.unwrap_or(false)
                || (task.state == TaskState::Unknown
                    && cancellation::allows_completion(root, store, r)?))
            && matches!(
                task.state,
                TaskState::Dispatching | TaskState::Running | TaskState::Unknown
            ),
        "桌面提交需要当前未取消的执行尝试",
    )
}
fn context(receipt: ExecutionReceipt, manifest: Option<ArtifactManifest>) -> CommandContext {
    CommandContext {
        thread_id: Some(receipt.binding.session_id.clone()),
        terminal_status: (receipt.status == ReceiptStatus::Completed).then(|| "completed".into()),
        execution: Some(ExecutionEvidence::Receipt { receipt, manifest }),
        ..Default::default()
    }
}
fn receipt(r: &TaskReview, p: &TaskProof) -> Result<ExecutionReceipt, String> {
    Ok(ExecutionReceipt {
        schema_version: 1,
        task_id: r.attempt.task_id.clone(),
        attempt_id: r.attempt.id.clone(),
        fence: r.attempt.fence,
        binding: r.attempt.binding.clone(),
        turn_id: None,
        observed_at: p.observed_at.clone(),
        status: ReceiptStatus::Completed,
        evidence_sha256: digest(&(r.id.as_str(), p))?,
    })
}
fn validate_seal(r: &TaskReview, p: &TaskProof, s: &SealIntent) -> Result<(), String> {
    timestamp(&s.created_at)?;
    require(
        s.review_digest == r.id
            && s.proof_digest == digest(p)?
            && chrono::DateTime::parse_from_rfc3339(&s.created_at).unwrap()
                >= chrono::DateTime::parse_from_rfc3339(&p.observed_at)
                    .map_err(|_| "回复时间无效")?,
        "文件封存意图与原回复不符",
    )
}
/// Structural validation uses historical evidence only. Reopening must not read
/// mutable working files or mark an old completion valid for a new plan.
pub(super) fn inspect_completion(
    root: &Path,
    store: &DurableWorkflow,
    r: &TaskReview,
    p: Option<&TaskProof>,
) -> Result<Option<DesktopCompletion>, String> {
    let dir = path(root, r)?;
    let submitted=store.history().iter().any(|e|matches!(&e.body.event.command,WorkflowCommand::Submit{task,attempt,..}|WorkflowCommand::Reconcile{task,attempt,outcome:ReconcileOutcome::Submitted,..} if task==&r.attempt.task_id&&attempt==&r.attempt.id));
    if !exists(&dir, "seal-intent") && !exists(&dir, "completion") {
        require(!submitted, "桌面提交缺少原文件封存")?;
        return Ok(None);
    }
    let p = p.ok_or("文件封存缺少原桌面回复")?;
    let seal: SealIntent = load(&dir, "seal-intent")?;
    validate_seal(r, p, &seal)?;
    if !exists(&dir, "completion") {
        require(!submitted, "桌面提交缺少原文件封存")?;
        return Ok(None);
    }
    let record: SealedRecord = load(&dir, "completion")?;
    let c = record.body;
    let plan = original_plan(store, r)?;
    require(
        record.hash == digest(&c)?
            && c.version == 1
            && c.review_digest == r.id
            && c.proof_digest == digest(p)?
            && c.seal_digest == digest(&seal)?
            && c.receipt == receipt(r, p)?,
        "桌面封存摘要或回执不符",
    )?;
    c.receipt.validate(plan, &r.attempt, None)?;
    timestamp(&c.sealed_at)?;
    require(
        chrono::DateTime::parse_from_rfc3339(&c.sealed_at).unwrap()
            >= chrono::DateTime::parse_from_rfc3339(&seal.created_at).unwrap(),
        "封存时间顺序无效",
    )?;
    let mut paths = plan.task(&r.attempt.task_id)?.artifact_paths.clone();
    paths.sort();
    require(
        c.manifest.schema_version == 1
            && c.manifest.revision_digest == r.attempt.revision_digest
            && c.manifest.task_id == r.attempt.task_id
            && c.manifest.attempt_id == r.attempt.id
            && c.manifest.receipt_digest == digest(&c.receipt)?
            && c.manifest
                .artifacts
                .iter()
                .map(|a| a.path.clone())
                .collect::<Vec<_>>()
                == paths,
        "封存交付集合不符",
    )?;
    for a in &c.manifest.artifacts {
        sha256(&a.sha256)?;
    }
    for event in store.history().iter().filter(|e|matches!(&e.body.event.command,WorkflowCommand::Submit{task,attempt,..}|WorkflowCommand::Reconcile{task,attempt,outcome:ReconcileOutcome::Submitted,..} if task==&r.attempt.task_id&&attempt==&r.attempt.id)){
        require(event.body.context==context(c.receipt.clone(),Some(c.manifest.clone())),"工作流提交与桌面原封存不符")?;
    }
    Ok(Some(c))
}
/// Called once, in the same live call that first persisted the observed reply.
/// This is not a recovery entry point: a reserved seal is never recaptured.
pub(super) fn seal_fresh(
    root: &Path,
    store: &DurableWorkflow,
    r: &TaskReview,
    p: &TaskProof,
    checkpoint: &mut impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    check_current(root, store, r)?;
    validate(store, r)?;
    validate_proof(r, p)?;
    let dir = path(root, r)?;
    let original: TaskProof = load(&dir, "proof")?;
    require(original == *p, "原回复已变化")?;
    let seal = SealIntent {
        review_digest: r.id.clone(),
        proof_digest: digest(p)?,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    validate_seal(r, p, &seal)?;
    publish(&dir, "seal-intent", &seal)?;
    checkpoint("seal-reserved")?;
    let plan = original_plan(store, r)?;
    let completed = receipt(r, p)?;
    let observer = ArtifactObserver::new(
        plan,
        store.approval().ok_or("批准缺失")?,
        "user",
        plan.policy.version,
        &r.attempt,
        &completed,
        None,
    )?;
    let manifest = observer.capture()?;
    checkpoint("artifacts-captured")?;
    observer.require_unchanged(&manifest)?;
    let c = DesktopCompletion {
        version: 1,
        review_digest: r.id.clone(),
        proof_digest: digest(p)?,
        seal_digest: digest(&seal)?,
        receipt: completed,
        manifest,
        sealed_at: chrono::Utc::now().to_rfc3339(),
    };
    publish(
        &dir,
        "completion",
        &SealedRecord {
            hash: digest(&c)?,
            body: c,
        },
    )?;
    checkpoint("completion-published")?;
    Ok(())
}
/// Reuses the immutable completion; never captures new artifact hashes.
pub fn submit_sealed(
    root: &Path,
    store: &mut DurableWorkflow,
    project: &str,
    attempt: &str,
) -> Result<DesktopCompletion, String> {
    let (review, proof) = inspect(root, store, project, attempt)?;
    let c = inspect_completion(root, store, &review, proof.as_ref())?
        .ok_or("缺少原文件封存，禁止恢复时重新取样")?;
    if store.history().iter().any(|e|matches!(&e.body.event.command,WorkflowCommand::Submit{task,attempt,..}|WorkflowCommand::Reconcile{task,attempt,outcome:ReconcileOutcome::Submitted,..} if task==&review.attempt.task_id&&attempt==&review.attempt.id)) {return Ok(c);}
    check_current(root, store, &review)?;
    let plan = original_plan(store, &review)?.clone();
    ArtifactObserver::new(
        &plan,
        store.approval().ok_or("批准缺失")?,
        "user",
        plan.policy.version,
        &review.attempt,
        &c.receipt,
        None,
    )?
    .require_unchanged(&c.manifest)?;
    let core = store.configuration().core_id.clone();
    let task = review.attempt.task_id.clone();
    let attempt = review.attempt.id.clone();
    if store.state().tasks[&task].state == TaskState::Dispatching {
        let mut running = c.receipt.clone();
        running.status = ReceiptStatus::Running;
        store.command(
            &format!("desktop-start-{}", &review.id[..32]),
            &core,
            WorkflowCommand::Started {
                task: task.clone(),
                attempt: attempt.clone(),
            },
            context(running, None),
            false,
        )?;
    }
    let (principal, command) = if store.state().tasks[&task].state == TaskState::Unknown {
        (
            core,
            WorkflowCommand::Reconcile {
                task,
                attempt,
                outcome: ReconcileOutcome::Submitted,
                evidence: vec![digest(&c.manifest)?],
            },
        )
    } else {
        (
            review.attempt.worker_id.clone(),
            WorkflowCommand::Submit {
                task,
                attempt,
                summary: "桌面回复已观察，原交付文件已封存；等待独立验收".into(),
                evidence: Some(vec![digest(&c.manifest)?]),
            },
        )
    };
    store.command(
        &format!("desktop-submit-{}", &review.id[..32]),
        &principal,
        command,
        context(c.receipt.clone(), Some(c.manifest.clone())),
        false,
    )?;
    Ok(c)
}
pub(super) fn mark_unknown(store: &mut DurableWorkflow, r: &TaskReview) -> Result<(), String> {
    if store
        .state()
        .tasks
        .get(&r.attempt.task_id)
        .is_some_and(|t| {
            t.attempt.as_ref() == Some(&r.attempt.id)
                && matches!(t.state, TaskState::Dispatching | TaskState::Running)
        })
    {
        let core = store.configuration().core_id.clone();
        store.command(
            &format!("desktop-unknown-{}", &r.id[..32]),
            &core,
            WorkflowCommand::RecoveryUnknown {
                task: r.attempt.task_id.clone(),
                attempt: r.attempt.id.clone(),
            },
            CommandContext::default(),
            false,
        )?;
    }
    Ok(())
}
