//! Durable user intent and a non-reconstructible capability for one native stop.
use super::*;
use cerulean_domain::desktop_sessions::DesktopCancellationStatus;
#[path = "desktop_stop_review.rs"]
pub(crate) mod review;
#[cfg(test)]
#[path = "desktop_workbuddy_cancel_tests.rs"]
mod workbuddy_tests;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StopObservation {
    pub task_review_digest: String,
    pub observation: DesktopObservation,
    pub baseline: Vec<String>,
    pub turn_runtime_id: String,
    pub prompt_runtime_id: String,
    pub stop_runtime_id: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StopProof {
    pub task_review_digest: String,
    pub observation: DesktopObservation,
    pub first_observed_at: String,
    pub turn_runtime_id: String,
    pub prompt_runtime_id: String,
    pub observed_baseline: Vec<String>,
    pub status_text: String,
    pub cancelled: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StopRequest {
    review_digest: String,
    actor: String,
    reason: String,
    requested_at: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StopIntent {
    request_digest: String,
    observation: StopObservation,
    reserved_at: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record<T> {
    body: T,
    hash: String,
}
// Only the caller that successfully publishes the request receives this value.
// It cannot be cloned, deserialized, or recovered from stored JSON.
pub(crate) struct ReservedRequest(StopRequest);
pub(crate) trait Transport {
    fn prepare(&mut self, r: &TaskReview) -> Result<StopObservation, String>;
    fn stop(&mut self, r: &TaskReview, o: &StopObservation) -> Result<StopProof, String>;
    fn read(&mut self, r: &TaskReview, o: &StopObservation) -> Result<StopProof, String>;
    fn completed(&mut self, r: &TaskReview) -> Result<TaskProof, String>;
}
fn exists(dir: &Path, name: &str) -> bool {
    dir.join(format!("{name}.json")).exists() || dir.join(format!("{name}.pending")).exists()
}
fn save<T: Serialize>(
    dir: &Path,
    name: &str,
    value: T,
    observer: &mut impl FnMut(&str, &str) -> Result<(), String>,
) -> Result<(), String> {
    ordinary(dir, true)?;
    let pending = dir.join(format!("{name}.pending"));
    let target = dir.join(format!("{name}.json"));
    require(
        !pending.exists() && !target.exists(),
        "原停止记录已存在，禁止重复操作或覆盖",
    )?;
    let bytes = serde_json::to_vec(&Record {
        hash: digest(&value)?,
        body: value,
    })
    .map_err(|_| "停止记录编码失败")?;
    require(bytes.len() <= 131072, "停止记录超限")?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)
        .map_err(|_| "停止记录预留失败")?;
    observer(name, "temp-created")?;
    let split = bytes.len() / 2;
    file.write_all(&bytes[..split])
        .map_err(|_| "停止记录写入失败")?;
    observer(name, "partial-written")?;
    file.write_all(&bytes[split..])
        .and_then(|_| file.sync_all())
        .map_err(|_| "停止记录同步失败")?;
    drop(file);
    observer(name, "temp-synced")?;
    fs::hard_link(&pending, &target).map_err(|_| "停止记录发布失败")?;
    observer(name, "published")?;
    fs::remove_file(pending).map_err(|_| "停止临时记录收尾失败")?;
    observer(name, "pending-removed")
}
fn record<T: Serialize + for<'a> Deserialize<'a>>(dir: &Path, name: &str) -> Result<T, String> {
    let r: Record<T> = load(dir, name)?;
    require(digest(&r.body)? == r.hash, "桌面停止记录摘要不符")?;
    Ok(r.body)
}
fn at(value: &str) -> Result<chrono::DateTime<chrono::FixedOffset>, String> {
    chrono::DateTime::parse_from_rfc3339(value).map_err(|_| "桌面停止时间无效".into())
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
fn validate_request(r: &TaskReview, q: &StopRequest) -> Result<(), String> {
    require(
        q.review_digest == r.id && q.actor == "user",
        "桌面停止请求归属不符",
    )?;
    validate_reason(&q.reason)?;
    require(
        at(&q.requested_at)? >= at(&r.observation.captured_at)?,
        "停止请求早于原任务",
    )
}
pub(crate) fn validate_reason(reason: &str) -> Result<(), String> {
    require(
        !reason.trim().is_empty() && reason.len() <= 8000 && !reason.contains('\0'),
        "请输入有效停止原因",
    )
}
/// The host may call this while the workflow writer is away. It only writes a
/// separate immutable request; it never changes workflow state or clicks UI.
pub(crate) fn request(
    root: &Path,
    r: &TaskReview,
    reason: &str,
) -> Result<ReservedRequest, String> {
    request_observed(root, r, reason, |_, _| Ok(()))
}
pub(crate) fn request_observed(
    root: &Path,
    r: &TaskReview,
    reason: &str,
    mut observer: impl FnMut(&str, &str) -> Result<(), String>,
) -> Result<ReservedRequest, String> {
    let dir = path(root, r)?;
    let original: TaskReview = load(&dir, "intent")?;
    require(
        original == *r && review_hash(r)? == r.id,
        "停止目标与原派发不符",
    )?;
    let q = StopRequest {
        review_digest: r.id.clone(),
        actor: "user".into(),
        reason: reason.into(),
        requested_at: chrono::Utc::now().to_rfc3339(),
    };
    validate_request(r, &q)?;
    save(&dir, "stop-request", q.clone(), &mut observer)?;
    Ok(ReservedRequest(q))
}
fn same_identity(a: &DesktopObservation, b: &DesktopObservation) -> bool {
    a.window == b.window
        && a.document_runtime_id == b.document_runtime_id
        && a.session_id == b.session_id
        && a.model_label == b.model_label
}
fn observation(o: &DesktopObservation) -> Result<(), String> {
    sha256(&o.content_digest)?;
    sha256(&o.layout_digest)?;
    timestamp(&o.captured_at)?;
    require((1..=1000).contains(&o.node_count), "停止观察节点数量无效")
}
fn validate_local_review(r: &TaskReview) -> Result<(), String> {
    if r.observation.window.app == cerulean_domain::desktop_sessions::DesktopApp::Workbuddy {
        // The review digest already binds the approved local binding, attempt,
        // original observation and history. It is not a provider session ID.
        sha256(&r.binding_id)?;
        require(
            workbuddy_read_version(&r.observation.window.version)
                && r.observation.session_id.is_none()
                && !r.observation.document_runtime_id.trim().is_empty()
                && r.observation
                    .model_label
                    .as_ref()
                    .is_some_and(|m| !m.trim().is_empty())
                && r.id == review_hash(r)?,
            "WorkBuddy停止与原本地任务审阅不符",
        )?;
    }
    Ok(())
}
fn cancelled_status(r: &TaskReview, text: &str) -> bool {
    use cerulean_domain::desktop_sessions::DesktopApp;
    match r.observation.window.app {
        DesktopApp::Qoder => matches!(
            text.split('，').next(),
            Some(
                "状态：已取消"
                    | "状态：取消"
                    | "状态：已停止"
                    | "状态：停止"
                    | "状态：已中断"
                    | "状态：中断"
            )
        ),
        DesktopApp::Workbuddy => matches!(
            text,
            "已取消" | "已停止" | "已中断" | "任务已停止" | "任务已取消"
        ),
    }
}
fn validate_observation(
    r: &TaskReview,
    q: &StopRequest,
    o: &StopObservation,
) -> Result<(), String> {
    validate_local_review(r)?;
    observation(&o.observation)?;
    require(
        o.task_review_digest == r.id
            && same_identity(&o.observation, &r.observation)
            && o.observation.draft_empty
            && o.observation.busy
            && o.baseline.len() >= 2
            && r.baseline.ends_with(&o.baseline)
            && [&o.turn_runtime_id, &o.prompt_runtime_id, &o.stop_runtime_id]
                .iter()
                .all(|s| !s.trim().is_empty() && s.len() <= 512)
            && at(&o.observation.captured_at)? >= at(&q.requested_at)?,
        "停止观察与原任务不符",
    )
}
fn validate_proof(r: &TaskReview, i: &StopIntent, p: &StopProof) -> Result<(), String> {
    validate_local_review(r)?;
    observation(&p.observation)?;
    let first = at(&p.first_observed_at)?;
    let second = at(&p.observation.captured_at)?;
    require(
        p.cancelled
            && p.task_review_digest == r.id
            && same_identity(&p.observation, &i.observation.observation)
            && p.observation.draft_empty
            && !p.observation.busy
            && p.turn_runtime_id == i.observation.turn_runtime_id
            && p.prompt_runtime_id == i.observation.prompt_runtime_id
            && p.observed_baseline.len() >= 2
            && i.observation.baseline.ends_with(&p.observed_baseline)
            && first >= at(&i.reserved_at)?
            && (second - first).num_milliseconds() >= 250
            && p.status_text.len() <= 1536
            && cancelled_status(r, &p.status_text),
        "桌面取消终态与原停止意图不符",
    )
}
fn cancel_context(r: &TaskReview, q: &StopRequest) -> Result<CommandContext, String> {
    Ok(CommandContext {
        thread_id: Some(r.attempt.binding.session_id.clone()),
        cancellation_source: Some(format!("desktop-user:{}", digest(q)?)),
        ..Default::default()
    })
}
fn cancel_recorded(
    store: &DurableWorkflow,
    r: &TaskReview,
    q: &StopRequest,
) -> Result<bool, String> {
    let rows: Vec<_> = store.history().iter().filter(|e| matches!(&e.body.event.command, WorkflowCommand::CancelIntent { task, attempt, .. } if task == &r.attempt.task_id && attempt == &r.attempt.id)).collect();
    require(rows.len() <= 1, "桌面停止请求重复入账")?;
    if let Some(e) = rows.first() {
        require(
            e.body.operation_id == format!("desktop-cancel-{}", &digest(q)?[..32])
                && e.body.event.principal == store.configuration().core_id
                && e.body.event.command
                    == (WorkflowCommand::CancelIntent {
                        task: r.attempt.task_id.clone(),
                        attempt: r.attempt.id.clone(),
                        reason: q.reason.clone(),
                    })
                && e.body.context == cancel_context(r, q)?,
            "工作流停止意图与原用户请求不符",
        )?;
    }
    Ok(!rows.is_empty())
}
fn receipt(
    r: &TaskReview,
    q: &StopRequest,
    i: &StopIntent,
    p: &StopProof,
) -> Result<ExecutionReceipt, String> {
    Ok(ExecutionReceipt {
        schema_version: 1,
        task_id: r.attempt.task_id.clone(),
        attempt_id: r.attempt.id.clone(),
        fence: r.attempt.fence,
        binding: r.attempt.binding.clone(),
        turn_id: None,
        observed_at: p.observation.captured_at.clone(),
        status: ReceiptStatus::Cancelled,
        evidence_sha256: digest(&(q, i, p))?,
    })
}
fn context(receipt: ExecutionReceipt) -> CommandContext {
    CommandContext {
        thread_id: Some(receipt.binding.session_id.clone()),
        execution: Some(ExecutionEvidence::Receipt {
            receipt,
            manifest: None,
        }),
        ..Default::default()
    }
}
struct Records {
    request: StopRequest,
    intent: Option<StopIntent>,
    proof: Option<StopProof>,
    recorded: bool,
    reconciled: bool,
}
fn records(
    root: &Path,
    store: &DurableWorkflow,
    r: &TaskReview,
) -> Result<Option<Records>, String> {
    let dir = path(root, r)?;
    let reviewed = review::reviewed(root, store, r)?;
    let reconciliations: Vec<_> = store.history().iter().filter(|e|matches!(&e.body.event.command, WorkflowCommand::Reconcile { task, attempt, outcome: ReconcileOutcome::Cancelled, .. } if task == &r.attempt.task_id && attempt == &r.attempt.id)).collect();
    if reviewed.as_deref() == Some("stop-request") {
        return Ok(None);
    }
    if !exists(&dir, "stop-request") {
        require(
            !exists(&dir, "stop-intent")
                && !exists(&dir, "stop-proof")
                && reconciliations.is_empty()
                && !store.history().iter().any(|e| matches!(&e.body.event.command, WorkflowCommand::CancelIntent { task, attempt, .. } if task == &r.attempt.task_id && attempt == &r.attempt.id)),
            "桌面停止缺少原用户请求",
        )?;
        return Ok(None);
    }
    let q: StopRequest = record(&dir, "stop-request")?;
    validate_request(r, &q)?;
    let recorded = cancel_recorded(store, r, &q)?;
    let intent = if exists(&dir, "stop-intent") && reviewed.as_deref() != Some("stop-intent") {
        let i: StopIntent = record(&dir, "stop-intent")?;
        validate_observation(r, &q, &i.observation)?;
        require(
            recorded
                && i.request_digest == digest(&q)?
                && at(&i.reserved_at)? >= at(&i.observation.observation.captured_at)?,
            "停止动作缺少原工作流取消意图",
        )?;
        Some(i)
    } else {
        None
    };
    let proof = if exists(&dir, "stop-proof") && reviewed.as_deref() != Some("stop-proof") {
        let p: StopProof = record(&dir, "stop-proof")?;
        validate_proof(r, intent.as_ref().ok_or("取消证明缺少原停止动作意图")?, &p)?;
        require(
            !exists(&dir, "proof") && !exists(&dir, "seal-intent") && !exists(&dir, "completion"),
            "桌面取消与完成记录冲突",
        )?;
        Some(p)
    } else {
        None
    };
    require(reconciliations.len() <= 1, "桌面取消重复对账")?;
    if let Some(e) = reconciliations.first() {
        let p = proof.as_ref().ok_or("桌面已取消但原终态证明缺失")?;
        let receipt = receipt(r, &q, intent.as_ref().unwrap(), p)?;
        require(
            e.body.event.principal == store.configuration().core_id
                && e.body.context == context(receipt.clone())
                && e.body.event.command
                    == (WorkflowCommand::Reconcile {
                        task: r.attempt.task_id.clone(),
                        attempt: r.attempt.id.clone(),
                        outcome: ReconcileOutcome::Cancelled,
                        evidence: vec![receipt.evidence_sha256],
                    }),
            "桌面取消对账与原证明不符",
        )?;
    }
    Ok(Some(Records {
        request: q,
        intent,
        proof,
        recorded,
        reconciled: !reconciliations.is_empty(),
    }))
}
pub(crate) fn inspect(
    root: &Path,
    store: &DurableWorkflow,
    r: &TaskReview,
) -> Result<Option<DesktopCancellationStatus>, String> {
    Ok(records(root, store, r)?.map(|s| DesktopCancellationStatus {
        requested_at: s.request.requested_at,
        reason: s.request.reason,
        control_reserved: s.intent.is_some(),
        confirmed: s.reconciled,
    }))
}
pub(super) fn allows_completion(
    root: &Path,
    store: &DurableWorkflow,
    r: &TaskReview,
) -> Result<bool, String> {
    Ok(records(root, store, r)?.is_some_and(|s| s.recorded && s.proof.is_none() && !s.reconciled))
}
fn unknown(store: &mut DurableWorkflow, r: &TaskReview, q: &StopRequest) -> Result<(), String> {
    if store.state().tasks[&r.attempt.task_id].state == TaskState::Cancelling {
        store.command(
            &format!("desktop-cancel-unknown-{}", &digest(q)?[..32]),
            &store.configuration().core_id.clone(),
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
fn reconcile(root: &Path, store: &mut DurableWorkflow, r: &TaskReview) -> Result<(), String> {
    let s = records(root, store, r)?.ok_or("原停止请求缺失")?;
    if s.reconciled {
        return Ok(());
    }
    let receipt = receipt(
        r,
        &s.request,
        s.intent.as_ref().ok_or("停止动作意图缺失")?,
        s.proof.as_ref().ok_or("取消证明缺失")?,
    )?;
    unknown(store, r, &s.request)?;
    store.command(
        &format!("desktop-cancelled-{}", &digest(&s.request)?[..32]),
        &store.configuration().core_id.clone(),
        WorkflowCommand::Reconcile {
            task: r.attempt.task_id.clone(),
            attempt: r.attempt.id.clone(),
            outcome: ReconcileOutcome::Cancelled,
            evidence: vec![receipt.evidence_sha256.clone()],
        },
        context(receipt),
        false,
    )?;
    Ok(())
}
/// A recovered call never receives a permit and can only read the original turn.
pub(crate) fn run(
    root: &Path,
    store: &mut DurableWorkflow,
    r: &TaskReview,
    permit: Option<ReservedRequest>,
    transport: &mut impl Transport,
) -> Result<(), String> {
    run_observed(root, store, r, permit, transport, |_, _| Ok(()))
}
pub(crate) fn run_observed(
    root: &Path,
    store: &mut DurableWorkflow,
    r: &TaskReview,
    permit: Option<ReservedRequest>,
    transport: &mut impl Transport,
    mut observer: impl FnMut(&str, &str) -> Result<(), String>,
) -> Result<(), String> {
    review::require_unrestricted(root, store, r)?;
    let (original, original_proof) = super::inspect(root, store, &r.project_id, &r.attempt.id)?;
    require(
        original == *r && !is_retired(store, &r.attempt.id),
        "原桌面任务不可停止或恢复",
    )?;
    let mut s = records(root, store, r)?.ok_or("原用户停止请求缺失")?;
    if let Some(p) = &permit {
        require(p.0 == s.request, "停止动作资格与原请求不符")?;
    }
    let task = store
        .state()
        .tasks
        .get(&r.attempt.task_id)
        .ok_or("任务缺失")?;
    require(
        task.attempt.as_ref() == Some(&r.attempt.id) && task.fence == r.attempt.fence,
        "当前停止尝试已变化",
    )?;
    if matches!(task.state, TaskState::Submitted | TaskState::Accepted) {
        completion::inspect_completion(
            root,
            store,
            r,
            super::inspect(root, store, &r.project_id, &r.attempt.id)?
                .1
                .as_ref(),
        )?
        .ok_or("已完成任务缺少原封存")?;
        return Ok(());
    }
    if original_proof.is_some() {
        // A reply already observed as completed must never be replaced with a
        // cancellation. Preserve the original seal, including incomplete ones.
        return super::submit_sealed(root, store, &r.project_id, &r.attempt.id).map(|_| ());
    }
    if s.proof.is_some() {
        reconcile(root, store, r)?;
        return observer("workflow", "reconciled");
    }
    require(
        store.revision().is_some_and(|p| {
            p.approval_digest()
                .is_ok_and(|h| h == r.attempt.revision_digest)
        }) && store.state().revision_requested.is_none(),
        "停止任务批准版本已变化",
    )?;
    completion::mark_unknown(store, r)?;
    if !s.recorded {
        store.command(
            &format!("desktop-cancel-{}", &digest(&s.request)?[..32]),
            &store.configuration().core_id.clone(),
            WorkflowCommand::CancelIntent {
                task: r.attempt.task_id.clone(),
                attempt: r.attempt.id.clone(),
                reason: s.request.reason.clone(),
            },
            cancel_context(r, &s.request)?,
            false,
        )?;
    }
    observer("workflow", "cancel-recorded")?;
    unknown(store, r, &s.request)?;
    observer("workflow", "unknown-recorded")?;
    let observed = if permit.is_some() {
        require(s.intent.is_none(), "停止动作已预留，禁止重复点击")?;
        match transport.prepare(r) {
            Ok(o) => {
                validate_observation(r, &s.request, &o)?;
                let now = chrono::Utc::now();
                let age = now.signed_duration_since(at(&o.observation.captured_at)?);
                require(
                    age >= chrono::Duration::zero() && age <= chrono::Duration::seconds(30),
                    "停止观察已过期",
                )?;
                let intent = StopIntent {
                    request_digest: digest(&s.request)?,
                    observation: o,
                    reserved_at: now.to_rfc3339(),
                };
                save(
                    &path(root, r)?,
                    "stop-intent",
                    intent.clone(),
                    &mut observer,
                )?;
                let result = transport.stop(r, &intent.observation);
                observer("transport", "stop-returned")?;
                s.intent = Some(intent);
                result
            }
            Err(error) => Err(error),
        }
    } else if let Some(i) = &s.intent {
        transport.read(r, &i.observation)
    } else {
        Err("停止动作未取得原观察；仅可核对完成结果，禁止补发停止".into())
    };
    match observed {
        Ok(proof) => {
            validate_proof(r, s.intent.as_ref().ok_or("停止意图缺失")?, &proof)?;
            observer("transport", "proof-validated")?;
            save(&path(root, r)?, "stop-proof", proof, &mut observer)?;
            reconcile(root, store, r)?;
            observer("workflow", "reconciled")
        }
        Err(stop_error) => {
            super::recover_and_submit(root, store, &r.project_id, &r.attempt.id, |r| {
                transport.completed(r)
            })
            .map(|_| ())
            .map_err(|completion_error| {
                format!("停止结果未确认：{stop_error}；完成回读：{completion_error}")
            })
        }
    }
}
