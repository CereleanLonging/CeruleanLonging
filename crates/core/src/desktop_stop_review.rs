//! Review preserves a partial stop record and retires its action capability.
//! It restores local management only; it never reconstructs cancellation proof.
use super::*;
use std::{collections::BTreeMap, path::PathBuf};
const FOLDER: &str = "desktop-stop-reviews-v1";
const STAGES: [&str; 3] = ["stop-request", "stop-intent", "stop-proof"];

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipt {
    version: u32,
    relative_path: String,
    evidence_digest: String,
    actor: String,
    reason: String,
    at: String,
    hash: String,
}
impl Receipt {
    fn hash(&self) -> Result<String, String> {
        digest(&(
            self.version,
            &self.relative_path,
            &self.evidence_digest,
            &self.actor,
            &self.reason,
            &self.at,
        ))
    }
}
fn raw(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("桌面复核记录不可读取".into()),
        Ok(_) => ordinary(path, false)?,
    }
    let mut bytes = vec![];
    fs::File::open(path)
        .map_err(|_| "桌面复核记录不可读取")?
        .take(131073)
        .read_to_end(&mut bytes)
        .map_err(|_| "桌面复核记录读取失败")?;
    require(bytes.len() <= 131072, "桌面复核记录超限")?;
    Ok(Some(bytes))
}
fn parts(relative: &str) -> Result<(&str, &str), String> {
    let value = relative
        .strip_prefix("desktop-task-dispatch-v1/")
        .ok_or("桌面复核路径无效")?;
    let (key, file) = value.split_once('/').ok_or("桌面复核路径无效")?;
    sha256(key)?;
    let stage = file
        .strip_suffix(".pending")
        .ok_or("仅可复核未发布停止记录")?;
    require(STAGES.contains(&stage), "此桌面阶段尚未开放复核")?;
    Ok((key, stage))
}
fn relative(r: &TaskReview, stage: &str) -> Result<String, String> {
    Ok(format!(
        "desktop-task-dispatch-v1/{}/{stage}.pending",
        digest(&(&r.project_id, &r.attempt.id))?
    ))
}
fn receipt_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    parts(relative)?;
    let folder = root.join(FOLDER);
    match fs::symlink_metadata(&folder) {
        Ok(_) => ordinary(&folder, true)?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err("桌面复核目录不可读取".into()),
    }
    Ok(folder.join(format!("{}.json", digest(&relative)?)))
}
fn entries(dir: &Path, limit: usize) -> Result<Vec<fs::DirEntry>, String> {
    ordinary(dir, true)?;
    let list = fs::read_dir(dir)
        .map_err(|_| "桌面目录不可读取")?
        .take(limit + 1)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "桌面目录项不可读取")?;
    require(list.len() <= limit, "桌面复核清单超限")?;
    Ok(list)
}
fn fingerprint(
    root: &Path,
    store: &DurableWorkflow,
    r: &TaskReview,
    relative_path: &str,
) -> Result<String, String> {
    let (key, stage) = parts(relative_path)?;
    require(
        key == digest(&(&r.project_id, &r.attempt.id))?,
        "桌面复核身份不符",
    )?;
    let (original, proof) =
        super::super::inspect_without_cancellation(root, store, &r.project_id, &r.attempt.id)?;
    require(
        original == *r && proof.is_none(),
        "原派发已变化或已有完成证据，不能复核停止残片",
    )?;
    super::super::continuity::validate_chain(root, store, r)?;
    let scope = store
        .configuration()
        .execution_scope
        .as_ref()
        .ok_or("原执行范围缺失")?;
    require(
        crate::host::matches_config(store.configuration(), &scope.project),
        "桌面复核角色绑定不符",
    )?;
    let dir = path(root, r)?;
    let mut files = BTreeMap::new();
    for e in entries(&dir, 14)? {
        let name = e
            .file_name()
            .into_string()
            .map_err(|_| "桌面原文件名无效")?;
        let (stem, extension) = name.rsplit_once('.').ok_or("桌面文件名无效")?;
        require(
            matches!(extension, "json" | "pending")
                && matches!(
                    stem,
                    "intent"
                        | "proof"
                        | "seal-intent"
                        | "completion"
                        | "stop-request"
                        | "stop-intent"
                        | "stop-proof"
                ),
            "未知桌面原文件",
        )?;
        let bytes = raw(&e.path())?.ok_or("桌面原文件缺失")?;
        files.insert(name, (text_hash_bytes(&bytes), bytes.len()));
    }
    require(
        files.contains_key(&format!("{stage}.pending"))
            && !files.contains_key(&format!("{stage}.json")),
        "仅可复核独立半写文件，正式记录或大小写不符",
    )?;
    let bytes = raw(&root.join(relative_path))?.ok_or("原停止残片缺失")?;
    require(
        serde_json::from_slice::<serde_json::Value>(&bytes).is_err_and(|e| e.is_eof()),
        "仅可复核空或未写完整的停止记录",
    )?;
    require(
        !exists(&dir, "proof") && !exists(&dir, "seal-intent") && !exists(&dir, "completion"),
        "完成与停止证据冲突，请继续核查",
    )?;
    let index = STAGES.iter().position(|s| *s == stage).unwrap();
    for later in &STAGES[index + 1..] {
        require(!exists(&dir, later), "存在后续停止证据，不能越过原记录")?;
    }
    let dispatch = store
        .history()
        .get(r.workflow_sequence as usize)
        .ok_or("原工作流派发缺失")?;
    require(
        dispatch.body.operation_id == format!("desktop-dispatch-{}", &r.id[..32])
            && dispatch.body.event.principal == store.configuration().core_id
            && dispatch.body.event.command
                == (WorkflowCommand::DispatchIntent {
                    task: r.attempt.task_id.clone(),
                    attempt: r.attempt.id.clone(),
                })
            && dispatch.body.context.thread_id.as_ref() == Some(&r.attempt.binding.session_id)
            && dispatch.body.context.cwd.as_ref() == Some(&r.attempt.binding.workspace),
        "原派发事件不符",
    )?;
    let cancels: Vec<_> = store.history().iter().filter(|e| matches!(&e.body.event.command, WorkflowCommand::CancelIntent { task, attempt, .. } if task == &r.attempt.task_id && attempt == &r.attempt.id)).collect();
    if index == 0 {
        require(cancels.is_empty(), "已入账取消但请求原文缺失，不能复核绕过")?;
    } else {
        let q: StopRequest = record(&dir, "stop-request")?;
        validate_request(r, &q)?;
        require(cancel_recorded(store, r, &q)?, "停止动作缺少原工作流请求")?;
        if index == 2 {
            let i: StopIntent = record(&dir, "stop-intent")?;
            validate_observation(r, &q, &i.observation)?;
            require(
                i.request_digest == digest(&q)?
                    && at(&i.reserved_at)? >= at(&i.observation.observation.captured_at)?,
                "原停止动作身份不符",
            )?;
        }
    }
    require(!store.history().iter().any(|e| matches!(&e.body.event.command,
        WorkflowCommand::Submit { attempt, .. } | WorkflowCommand::Verify { attempt, .. } | WorkflowCommand::Accept { attempt, .. } if attempt == &r.attempt.id)
        || matches!(&e.body.event.command, WorkflowCommand::Reconcile { attempt, outcome: ReconcileOutcome::Cancelled | ReconcileOutcome::Submitted, .. } if attempt == &r.attempt.id)), "已有完成或取消终态，不能降级复核")?;
    digest(&(
        "desktop-stop-partial-review-v1",
        relative_path,
        &r.id,
        &dispatch.hash,
        cancels.iter().map(|e| &e.hash).collect::<Vec<_>>(),
        files,
    ))
}
fn text_hash_bytes(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}
fn verify(
    root: &Path,
    store: &DurableWorkflow,
    r: &TaskReview,
    receipt: &Receipt,
) -> Result<(), String> {
    validate_reason(&receipt.reason)?;
    timestamp(&receipt.at)?;
    require(
        receipt.version == 1
            && receipt.actor == "user"
            && receipt.hash == receipt.hash()?
            && receipt.evidence_digest == fingerprint(root, store, r, &receipt.relative_path)?,
        "桌面复核与原文或批准链不符",
    )
}
fn with_store<T>(
    root: &Path,
    relative_path: &str,
    inspect: impl FnOnce(&DurableWorkflow, &TaskReview) -> Result<T, String>,
) -> Result<T, String> {
    let (key, _) = parts(relative_path)?;
    ordinary(root, true)?;
    ordinary(&root.join("desktop-task-dispatch-v1"), true)?;
    let r: TaskReview = load(&root.join("desktop-task-dispatch-v1").join(key), "intent")?;
    identifier(&r.project_id)?;
    ordinary(&root.join("workflows"), true)?;
    DurableWorkflow::inspect_existing(&root.join("workflows").join(&r.project_id), |store| {
        inspect(store, &r)
    })
}
pub(crate) fn inspect(root: &Path, relative_path: &str) -> Result<(String, bool), String> {
    with_store(root, relative_path, |store, r| {
        let hash = fingerprint(root, store, r, relative_path)?;
        let reviewed = if let Some(bytes) = raw(&receipt_path(root, relative_path)?)? {
            let receipt: Receipt =
                serde_json::from_slice(&bytes).map_err(|_| "桌面复核凭据损坏")?;
            require(receipt.relative_path == relative_path, "桌面复核目标不符")?;
            verify(root, store, r, &receipt)?;
            true
        } else {
            false
        };
        Ok((hash, reviewed))
    })
}
pub(crate) fn reviewed(
    root: &Path,
    store: &DurableWorkflow,
    r: &TaskReview,
) -> Result<Option<String>, String> {
    let mut found = None;
    for stage in STAGES {
        let rel = relative(r, stage)?;
        if let Some(bytes) = raw(&receipt_path(root, &rel)?)? {
            let receipt: Receipt =
                serde_json::from_slice(&bytes).map_err(|_| "桌面复核凭据损坏")?;
            require(
                receipt.relative_path == rel && found.is_none(),
                "桌面复核目标冲突",
            )?;
            verify(root, store, r, &receipt)?;
            found = Some(stage.into());
        }
    }
    Ok(found)
}
pub(crate) fn require_unrestricted(
    root: &Path,
    store: &DurableWorkflow,
    r: &TaskReview,
) -> Result<(), String> {
    require(
        reviewed(root, store, r)?.is_none(),
        "原停止残片已复核，仅恢复本地管理；不能继续原尝试，请核对后结束本地跟踪",
    )
}
pub(crate) fn validate_all(
    root: &Path,
    projects: &BTreeMap<String, DurableWorkflow>,
) -> Result<(), String> {
    let folder = root.join(FOLDER);
    if matches!(fs::symlink_metadata(&folder), Err(e) if e.kind() == std::io::ErrorKind::NotFound) {
        return Ok(());
    }
    for e in entries(&folder, 10000)? {
        ordinary(&e.path(), false)?;
        let name = e
            .file_name()
            .into_string()
            .map_err(|_| "桌面复核名称无效")?;
        let components: Vec<_> = name.split('.').collect();
        require(matches!(components.len(), 2 | 3), "桌面复核名称无效")?;
        sha256(components[0])?;
        if components.len() == 3 {
            sha256(components[1])?;
            require(components[2] == "pending", "桌面复核临时文件名无效")?;
            continue; // Unpublished receipt never grants recovery, and can be retried.
        }
        require(components[1] == "json", "桌面复核扩展名无效")?;
        let receipt: Receipt = serde_json::from_slice(&raw(&e.path())?.ok_or("桌面复核缺失")?)
            .map_err(|_| "桌面复核损坏")?;
        require(
            e.path() == receipt_path(root, &receipt.relative_path)?,
            "桌面复核文件名不符",
        )?;
        let (key, _) = parts(&receipt.relative_path)?;
        let r: TaskReview = load(&root.join("desktop-task-dispatch-v1").join(key), "intent")?;
        verify(
            root,
            projects.get(&r.project_id).ok_or("桌面复核原项目缺失")?,
            &r,
            &receipt,
        )?;
    }
    Ok(())
}
pub(crate) fn acknowledge(
    root: &Path,
    relative_path: &str,
    expected: &str,
    reason: &str,
    mut observe: impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    validate_reason(reason)?;
    sha256(expected)?;
    let (hash, done) = inspect(root, relative_path)?;
    require(hash == expected, "桌面原记录已变化，请刷新后复核")?;
    if done {
        return Ok(());
    }
    let mut receipt = Receipt {
        version: 1,
        relative_path: relative_path.into(),
        evidence_digest: expected.into(),
        actor: "user".into(),
        reason: reason.into(),
        at: chrono::Utc::now().to_rfc3339(),
        hash: String::new(),
    };
    receipt.hash = receipt.hash()?;
    let target = receipt_path(root, relative_path)?;
    let folder = target.parent().unwrap();
    if !folder.exists() {
        fs::create_dir(folder).map_err(|_| "桌面复核目录创建失败")?;
    }
    require(entries(folder, 10000)?.len() < 10000, "桌面复核记录已满")?;
    let pending = target.with_extension(format!("{}.pending", receipt.hash));
    let bytes = serde_json::to_vec(&receipt).map_err(|_| "桌面复核编码失败")?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&pending)
        .map_err(|_| "桌面复核预留失败")?;
    observe("created")?;
    file.write_all(&bytes[..bytes.len() / 2])
        .map_err(|_| "桌面复核写入失败")?;
    observe("partial")?;
    file.write_all(&bytes[bytes.len() / 2..])
        .and_then(|_| file.sync_all())
        .map_err(|_| "桌面复核同步失败")?;
    drop(file);
    observe("synced")?;
    require(
        inspect(root, relative_path)? == (expected.into(), false),
        "发布前桌面复核证据已变化",
    )?;
    fs::hard_link(&pending, target).map_err(|_| "桌面复核发布失败")?;
    observe("published")?;
    fs::remove_file(pending).map_err(|_| "桌面复核收尾失败")?;
    observe("finished")
}
