//! Continue only from the latest accepted, sealed round on the original binding.
use super::*;

fn records(root: &Path, binding: &str) -> Result<Vec<TaskReview>, String> {
    let mut reviews = vec![];
    for (project, attempt) in recorded_attempts(root)? {
        let dir = root
            .join("desktop-task-dispatch-v1")
            .join(digest(&(&project, &attempt))?);
        let r: TaskReview = load(&dir, "intent")?;
        if r.binding_id == binding {
            reviews.push(r);
        }
    }
    Ok(reviews)
}
fn previous<'a>(
    reviews: &'a [TaskReview],
    project: &str,
    before: u32,
) -> Result<Option<&'a TaskReview>, String> {
    require(
        reviews.iter().all(|r| r.project_id == project),
        "桌面关联已由其他项目使用",
    )?;
    Ok(reviews
        .iter()
        .filter(|r| r.workflow_sequence < before)
        .max_by_key(|r| r.workflow_sequence))
}
fn accepted(
    root: &Path,
    store: &DurableWorkflow,
    prior: &TaskReview,
    before: u32,
) -> Result<TaskProof, String> {
    let (_, proof) = inspect_single(root, store, &prior.project_id, &prior.attempt.id)?;
    let proof = proof.ok_or("上一桌面任务缺少回复凭据")?;
    require(
        proof.round_sha256.is_some(),
        "上一桌面任务缺少整轮消息摘要，不能自动接续",
    )?;
    require(
        completion::inspect_completion(root, store, prior, Some(&proof))?.is_some(),
        "上一桌面任务缺少完整封存",
    )?;
    let prefix = store
        .history()
        .get(..before as usize)
        .ok_or("连续历史锚点越界")?;
    let replay = Workflow::replay(
        store.configuration().principals.clone(),
        &prefix
            .iter()
            .map(|e| e.body.event.clone())
            .collect::<Vec<_>>(),
    )?;
    require(
        replay
            .state()
            .tasks
            .get(&prior.attempt.task_id)
            .is_some_and(|t| {
                t.state == TaskState::Accepted
                    && t.verified
                    && t.attempt.as_deref() == Some(&prior.attempt.id)
            }),
        "上一桌面任务尚未独立验收并收口",
    )?;
    Ok(proof)
}
pub(super) fn validate_chain(
    root: &Path,
    store: &DurableWorkflow,
    review: &TaskReview,
) -> Result<(), String> {
    let reviews = records(root, &review.binding_id)?;
    let mut current = review;
    for _ in 0..=512 {
        let prior = previous(&reviews, &current.project_id, current.workflow_sequence)?;
        match (&current.continuation, prior) {
            (None, None) => return Ok(()),
            (Some(c), Some(p)) => {
                require(
                    c.previous_attempt_id == p.attempt.id
                        && c.previous_review_digest == p.id
                        && p.attempt.id != current.attempt.id,
                    "连续历史没有引用最近一次原派发",
                )?;
                require(
                    p.attempt.revision_digest == current.attempt.revision_digest
                        && p.bridge_sha256 == current.bridge_sha256
                        && p.observation.window == current.observation.window
                        && p.observation.document_runtime_id
                            == current.observation.document_runtime_id
                        && p.observation.session_id == current.observation.session_id
                        && p.observation.model_label == current.observation.model_label,
                    "连续历史身份或批准方案发生变化",
                )?;
                let proof = accepted(root, store, p, current.workflow_sequence)?;
                let prefix = current
                    .baseline
                    .get(..current.baseline.len().saturating_sub(1))
                    .ok_or("连续历史缺失")?;
                require(
                    prefix.len() >= 2
                        && p.baseline.ends_with(prefix)
                        && current.baseline.last() == proof.round_sha256.as_ref(),
                    "连续历史或上一整轮消息摘要不符",
                )?;
                let observed =
                    chrono::DateTime::parse_from_rfc3339(&current.observation.captured_at)
                        .map_err(|_| "连续观察时间无效")?;
                let completed = chrono::DateTime::parse_from_rfc3339(&proof.observed_at)
                    .map_err(|_| "上一回复时间无效")?;
                require(observed >= completed, "连续观察早于上一回复")?;
                current = p;
            }
            _ => return Err("连续历史凭据缺失或没有对应原派发".into()),
        }
    }
    Err("连续历史超过上限".into())
}
/// Read and validate the latest predecessor before asking the bridge for a new view.
pub fn predecessor(
    root: &Path,
    store: &DurableWorkflow,
    binding: &str,
) -> Result<Option<TaskReview>, String> {
    let plan = store.revision().ok_or("方案缺失")?;
    let before = store
        .history()
        .last()
        .ok_or("工作流为空")?
        .body
        .event
        .sequence;
    let reviews = records(root, binding)?;
    let Some(p) = previous(&reviews, &plan.project.id, before)? else {
        return Ok(None);
    };
    require(
        p.attempt.revision_digest == plan.approval_digest()?,
        "连续历史属于不同批准方案",
    )?;
    validate_chain(root, store, p)?;
    accepted(root, store, p, before)?;
    Ok(Some(p.clone()))
}
pub fn prepare_continuing(
    root: &Path,
    store: &DurableWorkflow,
    task: &str,
    attempt: &str,
    observation: DesktopObservation,
    baseline: Vec<String>,
) -> Result<TaskReview, String> {
    let binding = &store
        .revision()
        .ok_or("方案缺失")?
        .task(task)?
        .desktop_worker
        .as_ref()
        .ok_or("桌面配置缺失")?
        .binding
        .id;
    let continuation = predecessor(root, store, binding)?.map(|p| TaskContinuation {
        previous_attempt_id: p.attempt.id,
        previous_review_digest: p.id,
    });
    let review = prepare_inner(store, task, attempt, observation, baseline, continuation)?;
    validate_chain(root, store, &review)?;
    Ok(review)
}
