//! A separately confirmed fixed-message probe, never a workflow task or acceptance.
use super::*;

const DIRECTORY: &str = "desktop-trials-v1";
pub(super) fn prompt(marker: &str) -> String {
    format!("这是一次独立的工作台接入验证。请仅回复本条新标记：{marker}。不添加解释，不读取或修改文件，不调用工具，不运行命令。")
}
fn review_digest(review: &DesktopTrialReview) -> Result<String, String> {
    let mut copy = review.clone();
    copy.id.clear();
    digest(&copy)
}
fn fresh(observation: &DesktopObservation) -> Result<(), String> {
    let at = chrono::DateTime::parse_from_rfc3339(&observation.captured_at)
        .map_err(|_| "验证观察时间无效")?;
    require(
        (0..=120).contains(&(chrono::Utc::now() - at.with_timezone(&chrono::Utc)).num_seconds()),
        "验证观察已过期，请重新准备",
    )
}
fn validate_review(r: &DesktopTrialReview) -> Result<(), String> {
    validate_observation(&r.observation)?;
    for d in [&r.binding_id, &r.bridge_sha256] {
        sha256(d)?;
    }
    require(
        r.observation.window.app == DesktopApp::Qoder && r.observation.window.version == "0.4.3.0",
        "当前仅支持Qoder 0.4.3.0固定消息验证；WorkBuddy及其他版本尚未开放发送",
    )?;
    require(
        r.marker.strip_prefix("CL_DESKTOP_").is_some_and(|s| {
            s.len() == 32
                && s.bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        }) && r.prompt == prompt(&r.marker)
            && (2..=128).contains(&r.baseline.len())
            && review_digest(r)? == r.id,
        "桌面验证确认内容无效",
    )?;
    for d in &r.baseline {
        sha256(d)?;
    }
    Ok(())
}
fn validate_trial(t: &DesktopTrial) -> Result<(), String> {
    validate_review(&t.review)?;
    timestamp(&t.started_at)?;
    if let Some(proof) = &t.proof {
        timestamp(&proof.observed_at)?;
        if let Some(reader) = &proof.reader_sha256 {
            sha256(reader)?;
        }
        if let Some(observed) = &proof.observed_baseline {
            require(
                proof.reader_sha256.is_some()
                    && observed.len() >= 2
                    && observed.len() <= t.review.baseline.len()
                    && t.review.baseline.ends_with(observed),
                "可见历史不足两轮或与原历史后缀不符",
            )?;
        }
        require(
            proof.marker == t.review.marker
                && Some(&proof.session_id) == t.review.observation.session_id.as_ref()
                && proof.baseline == t.review.baseline
                && proof.prompt_seen
                && proof.completed
                && proof.reply_matches
                && chrono::DateTime::parse_from_rfc3339(&proof.observed_at).unwrap()
                    >= chrono::DateTime::parse_from_rfc3339(&t.started_at).unwrap(),
            "桌面验证回复与原始发送意图不符",
        )?;
    }
    Ok(())
}
/// Complete pending publications are read logically; partial or conflicting bytes
/// stop further sends. No record is overwritten or automatically replayed.
pub(in crate::host) fn load(root: &Path) -> Result<Vec<DesktopTrial>, String> {
    let dir = root.join(DIRECTORY);
    if !dir.exists() {
        return Ok(vec![]);
    }
    ordinary(&dir, true)?;
    let mut records = BTreeMap::<(String, String), Vec<u8>>::new();
    for (index, entry) in fs::read_dir(&dir)
        .map_err(|_| "验证记录目录不可读")?
        .enumerate()
    {
        require(index < 1024, "桌面验证记录超限")?;
        let path = entry.map_err(|_| "验证记录不可读")?.path();
        ordinary(&path, false)?;
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("验证记录文件名无效")?;
        let parts: Vec<_> = name.split('.').collect();
        require(
            parts.len() == 3
                && matches!(parts[1], "intent" | "proof")
                && matches!(parts[2], "json" | "pending"),
            "未知桌面验证文件",
        )?;
        sha256(parts[0])?;
        let mut bytes = vec![];
        fs::File::open(&path)
            .map_err(|_| "验证记录不可读")?
            .take(65537)
            .read_to_end(&mut bytes)
            .map_err(|_| "验证记录读取失败")?;
        require(bytes.len() <= 65536, "验证记录超限")?;
        if let Some(previous) = records.insert((parts[0].into(), parts[1].into()), bytes.clone()) {
            require(previous == bytes, "桌面验证发布冲突")?;
        }
        let trial: DesktopTrial = serde_json::from_slice(&bytes)
            .map_err(|_| "桌面验证记录不完整，停止发送并保留原记录")?;
        validate_trial(&trial)?;
        require(
            trial.review.id == parts[0] && trial.proof.is_some() == (parts[1] == "proof"),
            "桌面验证记录身份不符",
        )?;
    }
    let mut results = BTreeMap::new();
    for ((id, kind), bytes) in &records {
        if kind == "intent" {
            let trial: DesktopTrial =
                serde_json::from_slice(bytes).map_err(|_| "验证意图解析失败")?;
            results.insert(id.clone(), trial);
        }
    }
    require(results.len() <= 256, "桌面验证最多256条")?;
    for ((id, kind), bytes) in &records {
        if kind == "proof" {
            let proof: DesktopTrial =
                serde_json::from_slice(bytes).map_err(|_| "验证结果解析失败")?;
            let intent = results.get_mut(id).ok_or("验证回复缺少原始发送意图")?;
            require(
                proof.review == intent.review && proof.started_at == intent.started_at,
                "验证回复关联冲突",
            )?;
            *intent = proof;
        }
    }
    Ok(results.into_values().collect())
}
fn publish(root: &Path, trial: &DesktopTrial) -> Result<(), String> {
    validate_trial(trial)?;
    let history = load(root)?;
    require(
        trial.proof.is_some() || history.len() < 256,
        "桌面验证记录已达上限",
    )?;
    let dir = root.join(DIRECTORY);
    if !dir.exists() {
        fs::create_dir(&dir).map_err(|_| "验证记录目录创建失败")?;
    }
    ordinary(&dir, true)?;
    let kind = if trial.proof.is_some() {
        "proof"
    } else {
        "intent"
    };
    let stem = format!("{}.{}", trial.review.id, kind);
    let pending = dir.join(format!("{stem}.pending"));
    let target = dir.join(format!("{stem}.json"));
    let bytes = serde_json::to_vec(trial).map_err(|_| "验证记录编码失败")?;
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&pending)
        .map_err(|_| "验证记录创建失败；禁止重发")?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "验证记录同步失败；保留原记录，禁止重发")?;
    drop(file);
    fs::hard_link(&pending, &target).map_err(|_| "验证记录发布失败；禁止重发")?;
    fs::remove_file(&pending).map_err(|_| "验证记录临时发布待核对".into())
}
pub(super) fn prepared(
    host: &mut RuntimeHost,
    binding: DesktopBinding,
    reply: ReadReply,
) -> Result<(), String> {
    let observation = reply.observation.ok_or("验证准备缺少观察")?;
    validate_observation(&observation)?;
    fresh(&observation)?;
    require(
        same_view(&binding.observation, &observation),
        "历史关联已变化，请重新观察并关联当前聊天",
    )?;
    let seed = digest(&(
        binding.id.clone(),
        &observation.captured_at,
        chrono::Utc::now().to_rfc3339(),
        &host.root,
    ))?;
    let marker = format!("CL_DESKTOP_{}", &seed[..32]);
    let mut review = DesktopTrialReview {
        id: String::new(),
        binding_id: binding.id,
        observation,
        marker: marker.clone(),
        prompt: prompt(&marker),
        baseline: reply.baseline,
        bridge_sha256: fingerprint(),
    };
    review.id = review_digest(&review)?;
    validate_review(&review)?;
    host.desktop_access.trial_review = Some(review);
    Ok(())
}
pub(super) fn completed(host: &mut RuntimeHost, id: &str, reply: ReadReply) -> Result<(), String> {
    let mut trial = load(&host.root)?
        .into_iter()
        .find(|t| t.review.id == id)
        .ok_or("原始验证意图缺失")?;
    require(trial.proof.is_none(), "此验证已经完成，禁止重复落账")?;
    trial.proof = Some(
        reply
            .trial_proof
            .ok_or("尚无完整回复证据，保留待核对状态")?,
    );
    // Provenance is stamped by the host running the pinned reader, not supplied
    // by the UI or inferred from the original (possibly older) sending bridge.
    trial.proof.as_mut().unwrap().reader_sha256 = Some(fingerprint());
    validate_trial(&trial)?;
    publish(&host.root, &trial)?;
    host.desktop_access.trials = load(&host.root)?;
    Ok(())
}
impl RuntimeHost {
    pub(crate) fn prepare_desktop_trial(&mut self, id: &str) -> Result<(), String> {
        self.require_desktop_tasks_resolved()?;
        require(
            self.desktop_active.is_none() && self.desktop_probe.is_none(),
            "桌面操作仍在进行",
        )?;
        self.desktop_access.trial_review = None;
        let trials = load(&self.root)?;
        require(
            trials.iter().all(|t| t.proof.is_some()),
            "存在未核实的桌面发送，请先只读核对原记录，禁止另发验证",
        )?;
        let binding = self
            .desktop_access
            .bindings
            .iter()
            .find(|b| b.id == id)
            .ok_or("关联不存在")?
            .clone();
        require(
            binding.observation.window.app == DesktopApp::Qoder
                && binding.observation.window.version == "0.4.3.0",
            "当前仅支持Qoder 0.4.3.0固定消息验证；WorkBuddy及其他版本尚未开放发送",
        )?;
        self.launch_desktop(
            ReadRequest::TrialPrepare {
                version: 1,
                window: binding.observation.window.clone(),
            },
            Action::PrepareTrial(binding),
        )
    }
    pub(crate) fn send_desktop_trial(&mut self, id: &str) -> Result<(), String> {
        self.require_desktop_tasks_resolved()?;
        require(
            self.desktop_active.is_none() && self.desktop_probe.is_none(),
            "桌面操作仍在进行",
        )?;
        let review = self
            .desktop_access
            .trial_review
            .as_ref()
            .ok_or("请先准备并审阅验证消息")?
            .clone();
        require(review.id == id, "验证确认摘要已变化")?;
        validate_review(&review)?;
        fresh(&review.observation)?;
        require(
            review.bridge_sha256 == fingerprint(),
            "桌面桥接版本已变化，请重新准备",
        )?;
        require(
            load(&self.root)?
                .iter()
                .all(|t| t.proof.is_some() && t.review.id != id),
            "存在未核实发送或重复请求；禁止重发",
        )?;
        let binding = self
            .desktop_access
            .bindings
            .iter()
            .find(|b| b.id == review.binding_id)
            .ok_or("验证关联缺失")?;
        require(
            same_view(&binding.observation, &review.observation),
            "验证关联观察不一致",
        )?;
        self.desktop_access.trial_review = None;
        let trial = DesktopTrial {
            review: review.clone(),
            started_at: chrono::Utc::now().to_rfc3339(),
            proof: None,
        };
        // Intent is durable before starting even the background thread. Every
        // error after this point leaves an unreplayed record for read-only review.
        publish(&self.root, &trial)?;
        self.desktop_access.trials = load(&self.root)?;
        self.launch_desktop(
            ReadRequest::TrialSend { version: 1, review },
            Action::Trial(id.into()),
        )
    }
    pub(crate) fn read_desktop_trial(&mut self, id: &str, reader: &str) -> Result<(), String> {
        require(
            self.desktop_active.is_none() && self.desktop_probe.is_none(),
            "桌面操作仍在进行",
        )?;
        let trial = load(&self.root)?
            .into_iter()
            .find(|t| t.review.id == id)
            .ok_or("验证记录不存在")?;
        require(trial.proof.is_none(), "该验证已有完整回复证据")?;
        require(
            reader == fingerprint(),
            "只读适配器已更新，请刷新并明确使用当前版本核对；原发送意图不变",
        )?;
        self.launch_desktop(
            ReadRequest::TrialRead {
                version: 1,
                review: trial.review,
            },
            Action::Trial(id.into()),
        )
    }
}
