//! Explicit observation binding and separately confirmed, fixed connectivity trials.
use super::*;
use cerulean_domain::desktop_sessions::*;
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    sync::mpsc,
    time::Duration,
};

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub(super) enum ReadRequest {
    ReviewSend {
        version: u32,
        #[serde(rename = "taskReview")]
        review: ReviewMessage,
    },
    ReviewRead {
        version: u32,
        #[serde(rename = "taskReview")]
        review: ReviewMessage,
    },
    Discover {
        version: u32,
    },
    Inspect {
        version: u32,
        window: DesktopWindow,
    },
    TrialPrepare {
        version: u32,
        window: DesktopWindow,
    },
    TrialSend {
        version: u32,
        review: DesktopTrialReview,
    },
    TaskPrepare {
        version: u32,
        window: DesktopWindow,
        #[serde(rename = "taskReview")]
        previous: Option<crate::desktop_execution::TaskReview>,
    },
    TaskSend {
        version: u32,
        #[serde(rename = "taskReview")]
        review: crate::desktop_execution::TaskReview,
    },
    TaskStopPrepare {
        version: u32,
        #[serde(rename = "taskReview")]
        review: crate::desktop_execution::TaskReview,
    },
    TaskStop {
        version: u32,
        #[serde(rename = "taskReview")]
        review: crate::desktop_execution::TaskReview,
        #[serde(rename = "stopObservation")]
        observation: serde_json::Value,
    },
    TaskStopRead {
        version: u32,
        #[serde(rename = "taskReview")]
        review: crate::desktop_execution::TaskReview,
        #[serde(rename = "stopObservation")]
        observation: serde_json::Value,
    },
    TaskRead {
        version: u32,
        #[serde(rename = "taskReview")]
        review: crate::desktop_execution::TaskReview,
    },
    TaskProgress {
        version: u32,
        #[serde(rename = "taskReview")]
        review: crate::desktop_execution::TaskReview,
    },
    TrialRead {
        version: u32,
        review: DesktopTrialReview,
    },
}
impl ReadRequest {
    fn require_available(&self) -> Result<(), String> {
        let window = match self {
            Self::ReviewSend { review, .. } | Self::ReviewRead { review, .. } => {
                &review.observation.window
            }
            Self::Discover { .. } => return Ok(()),
            Self::Inspect { window, .. }
            | Self::TrialPrepare { window, .. }
            | Self::TaskPrepare { window, .. } => window,
            Self::TrialSend { review, .. } | Self::TrialRead { review, .. } => {
                &review.observation.window
            }
            Self::TaskSend { review, .. }
            | Self::TaskRead { review, .. }
            | Self::TaskProgress { review, .. }
            | Self::TaskStopPrepare { review, .. }
            | Self::TaskStop { review, .. }
            | Self::TaskStopRead { review, .. } => &review.observation.window,
        };
        window.app.require_available()
    }
}
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReadReply {
    #[serde(rename = "reviewText")]
    pub review_text: Option<String>,
    #[serde(rename = "taskProgress")]
    pub task_progress: Option<DesktopTaskProgress>,
    #[serde(rename = "stopObservation")]
    pub stop_observation: Option<serde_json::Value>,
    #[serde(rename = "stopProof")]
    pub stop_proof: Option<serde_json::Value>,
    #[serde(default)]
    pub windows: Vec<DesktopWindow>,
    pub observation: Option<DesktopObservation>,
    #[serde(default)]
    pub baseline: Vec<String>,
    #[serde(rename = "trialProof")]
    pub trial_proof: Option<DesktopTrialProof>,
    #[serde(rename = "taskProof")]
    pub task_proof: Option<crate::desktop_execution::TaskProof>,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ReviewMessage {
    pub id: String,
    pub binding_id: String,
    pub marker: String,
    pub prompt: String,
    pub prompt_sha256: String,
    pub bridge_sha256: String,
    pub observation: DesktopObservation,
    pub baseline: Vec<String>,
    pub max_wait_seconds: u32,
}
#[cfg(test)]
pub(super) type Fixture = fn(&ReadRequest) -> Result<ReadReply, String>;
enum Action {
    Discover,
    BindSession {
        project: String,
        session_id: String,
    },
    Inspect(DesktopWindow),
    Bind {
        project: String,
        label: String,
        previous: DesktopObservation,
    },
    PrepareTrial(DesktopBinding),
    PrepareTask {
        project: String,
        task: String,
        attempt: String,
    },
    Trial(String),
}
pub(super) struct Probe {
    action: Action,
    receiver: mpsc::Receiver<Result<ReadReply, String>>,
}
const BRIDGE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/CeruleanDesktopObserver.exe"));
pub(super) fn fingerprint() -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(BRIDGE))
}
fn validate_window(window: &DesktopWindow) -> Result<(), String> {
    require(
        window.hwnd.parse::<i64>().is_ok_and(|n| n > 0)
            && window.pid > 0
            && window.start_ticks.parse::<u64>().is_ok_and(|n| n > 0),
        "窗口进程身份无效",
    )?;
    let name = match window.app {
        DesktopApp::Qoder => "qoder.exe",
        DesktopApp::Workbuddy => "workbuddy.exe",
    };
    require(
        Path::new(&window.exe).is_absolute()
            && Path::new(&window.exe)
                .file_name()
                .is_some_and(|v| v.to_string_lossy().eq_ignore_ascii_case(name))
            && window.exe.len() < 4096
            && window.title.chars().count() <= 512
            && !window.version.is_empty()
            && window.version.len() <= 100,
        "窗口程序身份无效",
    )
}
fn validate_qoder_session_id(id: &str) -> Result<(), String> {
    require(
        id.len() == 36
            && id.bytes().enumerate().all(|(i, b)| {
                if [8, 13, 18, 23].contains(&i) {
                    b == b'-'
                } else {
                    b.is_ascii_hexdigit()
                }
            }),
        "请填写完整的 Qoder 会话 ID（UUID）",
    )
}
fn validate_observation(observation: &DesktopObservation) -> Result<(), String> {
    if let Some(model) = &observation.model_label {
        require(
            !model.trim().is_empty() && model.len() <= 200 && !model.chars().any(char::is_control),
            "模型显示名无效",
        )?;
    }
    validate_window(&observation.window)?;
    timestamp(&observation.captured_at)?;
    require(
        !observation.document_runtime_id.is_empty()
            && observation.document_runtime_id.len() <= 512
            && observation.node_count > 0
            && observation.node_count <= 1000
            && observation.draft_empty
            && !observation.busy,
        "会话观察不完整或存在草稿/忙碌状态",
    )?;
    for d in [&observation.content_digest, &observation.layout_digest] {
        require(
            d.len() == 64
                && d.bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()),
            "会话指纹无效",
        )?;
    }
    match observation.window.app {
        DesktopApp::Qoder => {
            validate_qoder_session_id(observation.session_id.as_deref().ok_or("Qoder会话ID缺失")?)?
        }
        DesktopApp::Workbuddy => require(
            observation.session_id.is_none(),
            "WorkBuddy不可伪造供应商会话ID",
        )?,
    }
    Ok(())
}
fn same_view(a: &DesktopObservation, b: &DesktopObservation) -> bool {
    let mut a = a.clone();
    a.captured_at = b.captured_at.clone();
    a == *b
}
fn observe_session(
    session_id: &str,
    mut read: impl FnMut(&ReadRequest) -> Result<ReadReply, String>,
) -> Result<ReadReply, String> {
    validate_qoder_session_id(session_id)?;
    let discovery = read(&ReadRequest::Discover { version: 1 })?;
    require(
        discovery.observation.is_none() && discovery.windows.len() <= 32,
        "窗口列表格式错误",
    )?;
    let windows: Vec<_> = discovery
        .windows
        .into_iter()
        .filter(|w| w.app == DesktopApp::Qoder)
        .collect();
    require(!windows.is_empty(), "请打开 Qoder 并进入所填 ID 对应的聊天")?;
    require(
        windows.len() <= 8,
        "Qoder 窗口超过8个，请缩小待核对范围后重试",
    )?;
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    let mut matched = None;
    let mut read_error = None;
    for window in windows {
        require(
            std::time::Instant::now() < deadline,
            "会话查找超时，请保持目标聊天空闲后重试",
        )?;
        validate_window(&window)?;
        let result = read(&ReadRequest::Inspect {
            version: 1,
            window: window.clone(),
        });
        let reply = match result {
            Ok(reply) => reply,
            Err(error) => {
                read_error = Some(error);
                continue;
            }
        };
        let observed = reply
            .observation
            .ok_or("窗口未返回聊天信息，请打开目标聊天后重试")?;
        validate_observation(&observed)?;
        require(
            observed.window == window,
            "查找过程中窗口身份已变化，请重新绑定",
        )?;
        if observed
            .session_id
            .as_deref()
            .is_some_and(|id| id.eq_ignore_ascii_case(session_id))
        {
            require(
                matched.is_none(),
                "多个 Qoder 窗口显示同一会话，请只保留一个目标窗口后重试",
            )?;
            require(
                observed.model_label.is_some(),
                "未读取到当前模型，请在 Qoder 中选择模型后重试",
            )?;
            matched = Some(observed);
        }
    }
    let previous = matched.ok_or_else(|| match read_error {
        Some(error) => format!("未找到可读取的目标会话，请打开对应聊天并保持空闲。{error}"),
        None => "未找到所填会话 ID，请在 Qoder 中打开对应聊天并核对 ID".into(),
    })?;
    require(
        std::time::Instant::now() < deadline,
        "会话查找超时，请重新绑定",
    )?;
    let confirmed = read(&ReadRequest::Inspect {
        version: 1,
        window: previous.window.clone(),
    })?;
    let current = confirmed.observation.as_ref().ok_or("确认观察缺失")?;
    validate_observation(current)?;
    require(
        same_view(&previous, current),
        "会话内容、模型或窗口已变化，请保持目标聊天空闲后重新绑定",
    )?;
    Ok(confirmed)
}
fn binding_digest(b: &DesktopBinding) -> Result<String, String> {
    digest(&(
        &b.project_id,
        &b.label,
        &b.observation,
        &b.confirmed_at,
        &b.bridge_sha256,
    ))
}
fn validate_binding(b: &DesktopBinding) -> Result<(), String> {
    identifier(&b.project_id)?;
    validate_observation(&b.observation)?;
    timestamp(&b.confirmed_at)?;
    require(
        !b.label.trim().is_empty()
            && b.label.chars().count() <= 120
            && !b.label.chars().any(char::is_control),
        "关联名称无效",
    )?;
    require(
        b.bridge_sha256.len() == 64
            && b.bridge_sha256.bytes().all(|v| v.is_ascii_hexdigit())
            && binding_digest(b)? == b.id,
        "桌面关联记录摘要不匹配",
    )
}
pub(super) fn load(root: &Path) -> Result<Vec<DesktopBinding>, String> {
    let directory = root.join("desktop-bindings-v1");
    if !directory.exists() {
        return Ok(vec![]);
    }
    ordinary(&directory, true)?;
    let mut records = BTreeMap::<String, Vec<u8>>::new();
    for (index, entry) in fs::read_dir(&directory)
        .map_err(|_| "桌面关联目录不可读")?
        .enumerate()
    {
        require(index < 512, "桌面关联记录超限")?;
        let path = entry.map_err(|_| "桌面关联记录不可读")?.path();
        ordinary(&path, false)?;
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("桌面关联文件名无效")?
            .to_string();
        require(
            stem.len() == 64
                && stem.bytes().all(|b| b.is_ascii_hexdigit())
                && matches!(
                    path.extension().and_then(|s| s.to_str()),
                    Some("json" | "pending")
                ),
            "桌面关联存在未知文件",
        )?;
        let mut bytes = vec![];
        fs::File::open(&path)
            .map_err(|_| "桌面关联不可读")?
            .take(32769)
            .read_to_end(&mut bytes)
            .map_err(|_| "桌面关联读取失败")?;
        require(bytes.len() <= 32768, "桌面关联文件超限")?;
        if let Some(previous) = records.insert(stem.clone(), bytes.clone()) {
            require(previous == bytes, "桌面关联发布冲突")?;
        }
        let binding: DesktopBinding =
            serde_json::from_slice(&bytes).map_err(|_| "桌面关联记录不完整或格式无效")?;
        validate_binding(&binding)?;
        require(binding.id == stem, "桌面关联文件身份不符")?;
    }
    let mut bindings = records
        .into_values()
        .map(|bytes| {
            serde_json::from_slice::<DesktopBinding>(&bytes).map_err(|_| "桌面关联解析失败".into())
        })
        .collect::<Result<Vec<_>, String>>()?;
    bindings.sort_by(|a, b| {
        a.confirmed_at
            .cmp(&b.confirmed_at)
            .then_with(|| a.id.cmp(&b.id))
    });
    Ok(bindings)
}
fn persist(root: &Path, binding: &DesktopBinding) -> Result<(), String> {
    validate_binding(binding)?;
    let existing = load(root)?;
    require(existing.len() < 128, "桌面关联已达128条，请先归档")?;
    let dir = root.join("desktop-bindings-v1");
    if !dir.exists() {
        fs::create_dir(&dir).map_err(|_| "无法创建桌面关联目录")?;
    }
    ordinary(&dir, true)?;
    let pending = dir.join(format!("{}.pending", binding.id));
    let target = dir.join(format!("{}.json", binding.id));
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&pending)
        .map_err(|_| "桌面关联写入被拒绝")?;
    file.write_all(&serde_json::to_vec(binding).map_err(|_| "桌面关联编码失败")?)
        .and_then(|_| file.sync_all())
        .map_err(|_| "桌面关联同步失败，保留原记录待核对")?;
    drop(file);
    fs::hard_link(&pending, target).map_err(|_| "桌面关联发布失败，保留原记录待核对")?;
    fs::remove_file(pending).map_err(|_| "桌面关联临时记录保留，需重新读取".into())
}
pub(super) fn observe(root: &Path, request: &ReadRequest) -> Result<ReadReply, String> {
    observe_cancellable(root, request, &std::sync::atomic::AtomicBool::new(false))
}
pub(crate) fn test_connection(
    root: &Path,
    expected: &DesktopObservation,
) -> Result<String, String> {
    validate_observation(expected)?;
    let current = observe(
        root,
        &ReadRequest::Inspect {
            version: 1,
            window: expected.window.clone(),
        },
    )?
    .observation
    .ok_or("未读取到桌面会话，请打开目标聊天后重试")?;
    validate_observation(&current)?;
    require(
        current.window.app == expected.window.app
            && current.window.hwnd == expected.window.hwnd
            && current.window.pid == expected.window.pid
            && current.window.start_ticks == expected.window.start_ticks
            && current.window.exe == expected.window.exe
            && current.window.version == expected.window.version
            && current.session_id == expected.session_id
            && current.model_label == expected.model_label,
        "桌面会话、模型或应用版本已变化，请重新读取并关联目标聊天",
    )?;
    Ok("Qoder 窗口、会话和模型核对通过，当前聊天可读取且处于空闲状态。".into())
}
#[cfg(test)]
pub(crate) fn discover_for_account_probe(root: &Path) -> Result<Vec<DesktopWindow>, String> {
    Ok(observe(root, &ReadRequest::Discover { version: 1 })?.windows)
}
fn observe_cancellable(
    root: &Path,
    request: &ReadRequest,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<ReadReply, String> {
    request.require_available()?;
    use std::os::windows::fs::OpenOptionsExt;
    ordinary(root, true)?;
    let directory = root.join("desktop-observer-v1");
    if !directory.exists() {
        fs::create_dir(&directory).map_err(|_| "无法准备桌面观察器")?;
    }
    ordinary(&directory, true)?;
    let path = directory.join(format!("{}.exe", fingerprint()));
    if !path.exists() {
        let mut f = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
            .map_err(|_| "无法写入桌面观察器")?;
        f.write_all(BRIDGE)
            .and_then(|_| f.sync_all())
            .map_err(|_| "桌面观察器保存失败")?;
    }
    ordinary(&path, false)?;
    let mut locked = fs::OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)
        .map_err(|_| "桌面观察器不可固定")?;
    let mut bytes = vec![];
    (&mut locked)
        .take(BRIDGE.len() as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "桌面观察器核验失败")?;
    require(bytes == BRIDGE, "桌面观察器内容已变化")?;
    let text = serde_json::to_string(request).map_err(|_| "桌面观察请求编码失败")?;
    require(text.len() <= 65536, "桌面桥接请求过大")?;
    // .NET Framework rejects the extended spelling produced by the core's
    // canonical data directory. Keep the locked identity, convert only spelling.
    let executable = normal_path(&path)?;
    let working = normal_path(&directory)?;
    let desktop_action = matches!(
        request,
        ReadRequest::TrialSend { .. } | ReadRequest::TaskSend { .. } | ReadRequest::TaskStop { .. }
    );
    let runner = if desktop_action {
        crate::verifier::process::run_desktop
    } else {
        crate::verifier::process::run
    };
    let outcome = runner(
        &executable,
        &[text],
        &working,
        &working,
        Duration::from_secs(
            if matches!(
                request,
                ReadRequest::TrialSend { .. } | ReadRequest::TaskSend { .. }
            ) {
                90
            } else {
                20
            },
        ),
        cancel,
        &BTreeMap::new(),
    )?;
    require(
        !outcome.timed_out && outcome.failure.is_none(),
        "桌面桥接超时或异常；如已发起发送，请仅回读核对，勿重发",
    )?;
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Envelope {
        version: u32,
        ok: bool,
        result: Option<ReadReply>,
        error: Option<String>,
    }
    let result: Envelope =
        serde_json::from_slice(&outcome.stdout).map_err(|_| "桌面观察器响应无效")?;
    require(result.version == 1, "桌面观察器协议不符")?;
    let message = match result.error.as_deref() {
        Some("stop_identity_changed" | "stop_history_changed" | "stop_turn_changed") => {
            "停止目标的聊天、原任务或历史已变化"
        }
        Some("stop_control_ambiguous" | "stop_control_unavailable") => "无法唯一确认停止按钮",
        Some("stop_not_running") => "原任务已结束或没有可确认的停止控件",
        Some("stop_review_invalid" | "stop_review_expired") => "停止审阅无效或过期，未点击",
        Some("stop_terminal_unverified") => "停止后的取消终态尚未核实，禁止重复点击",
        Some("trial_app_not_enabled") => "当前桌面应用或版本尚未支持此操作；请核对该版本的接入状态",
        Some("trial_review_invalid") => "发送审阅内容无效，未执行输入",
        Some("trial_expired") => "验证确认已过期，请重新准备",
        Some("trial_view_changed" | "trial_history_changed") => {
            "会话、历史或布局已变化；请核对原窗口和发送记录，勿重发"
        }
        Some("trial_history_unavailable") => "无法完整识别聊天轮次；请核对布局和历史，不会自动重发",
        Some("trial_focus_changed") => {
            "目标聊天未保持前台焦点，验证已停止；请核对原记录，不会自动重发"
        }
        Some("trial_draft_changed") => {
            "输入框未按预期更新或草稿变化，验证已停止；请检查草稿，不会自动重发"
        }
        Some("trial_send_unavailable") => "发送按钮不可用，验证已停止；请检查草稿，不会自动重发",
        Some("trial_keyboard_unavailable") => {
            "文字输入未完整完成，验证已停止；请保留并检查草稿，不会重输或发送"
        }
        Some("trial_lease_busy") => "另一个工作台实例正在操作桌面；本次结果待核对，不会自动重发",
        Some("trial_reply_unverified") => "尚未核实本次完整回复；可稍后只读核对，不会自动重发",
        Some("window_not_visible") => {
            "目标窗口已最小化或不可见，请恢复窗口并打开要关联的聊天后重试"
        }
        Some("window_changed") => "窗口或应用身份已变化，请重新发现窗口",
        Some("draft_or_composer_unavailable") => {
            "存在草稿或无法唯一识别输入框；请保留自己的草稿，准备好聊天后重新读取"
        }
        Some("session_busy_or_approval_pending") => {
            "当前聊天正在运行或等待工具批准，请处理完成后重新读取"
        }
        Some("session_id_unavailable") => "没有读到Qoder聊天ID，请打开要关联的已有聊天",
        Some("document_not_unique") => "无法唯一识别聊天页面，请核对窗口和布局",
        Some("tree_limit" | "text_limit" | "value_limit" | "content_unavailable_or_limit") => {
            "聊天页面超出当前读取范围或缺少可核对内容，本次未建立关联"
        }
        Some("sensitive_control_present") => "当前页面含密码控件，请返回聊天页面后重新读取",
        _ => "桌面观察暂不可用，请核对应用窗口后重试",
    };
    require(result.ok && outcome.exit_code == 0, message)?;
    result.result.ok_or("桌面观察结果缺失".into())
}
fn normal_path(path: &Path) -> Result<PathBuf, String> {
    let canonical = fs::canonicalize(path).map_err(|_| "桌面观察器路径不可读")?;
    let text = canonical.to_str().ok_or("桌面观察器路径编码无效")?;
    let normal = if let Some(tail) = text.strip_prefix("\\\\?\\UNC\\") {
        PathBuf::from(format!("\\\\{tail}"))
    } else if let Some(tail) = text.strip_prefix("\\\\?\\") {
        PathBuf::from(tail)
    } else {
        canonical.clone()
    };
    require(
        fs::canonicalize(&normal).map_err(|_| "桌面观察器普通路径不可读")? == canonical,
        "桌面观察器路径身份不符",
    )?;
    Ok(normal)
}
impl RuntimeHost {
    pub(super) fn validate_desktop_selection(
        &self,
        project: &str,
        selection: &DesktopWorkerSelection,
    ) -> Result<(), String> {
        selection.validate()?;
        let binding = self
            .desktop_access
            .bindings
            .iter()
            .find(|b| b.id == selection.binding.id)
            .ok_or("桌面关联未在本工作台保存")?;
        require(
            binding == &selection.binding && binding.project_id == project,
            "桌面选择与原保存关联或项目不符",
        )?;
        let scope = self
            .projects
            .get(project)
            .and_then(|s| s.configuration().execution_scope.as_ref())
            .ok_or("桌面项目范围缺失")?;
        require(
            selection.project_root == scope.project.root
                || selection.project_root
                    == cerulean_domain::execution::WorkerDirectory::for_desktop(selection)?
                        .root(&scope.project.root)?,
            "桌面配置目录与登记项目不一致",
        )
    }
    fn launch_desktop(&mut self, request: ReadRequest, action: Action) -> Result<(), String> {
        request.require_available()?;
        require(
            self.desktop_active.is_none() && self.desktop_probe.is_none(),
            "桌面观察仍在进行",
        )?;
        let root = self.root.clone();
        let (tx, rx) = mpsc::channel();
        #[cfg(test)]
        let fixture = self.desktop_fixture;
        let session_id = match &action {
            Action::BindSession { session_id, .. } => Some(session_id.clone()),
            _ => None,
        };
        std::thread::Builder::new()
            .name("desktop-observer".into())
            .spawn(move || {
                let read = |request: &ReadRequest| {
                    #[cfg(test)]
                    if let Some(fixture) = fixture {
                        return fixture(request);
                    }
                    observe(&root, request)
                };
                let result = match session_id {
                    Some(id) => observe_session(&id, read),
                    None => read(&request),
                };
                let _ = tx.send(result);
            })
            .map_err(|_| "桌面观察后台无法启动")?;
        self.desktop_probe = Some(Probe {
            action,
            receiver: rx,
        });
        self.desktop_access.checking = true;
        self.desktop_access.error = None;
        Ok(())
    }
    pub(super) fn discover_desktop(&mut self) -> Result<(), String> {
        require(
            self.desktop_active.is_none() && self.desktop_probe.is_none(),
            "桌面观察仍在进行",
        )?;
        self.desktop_access.windows.clear();
        self.desktop_access.observation = None;
        self.desktop_access.observation_digest = None;
        self.launch_desktop(ReadRequest::Discover { version: 1 }, Action::Discover)
    }
    pub(super) fn bind_desktop_session(
        &mut self,
        project: &str,
        session_id: &str,
    ) -> Result<(), String> {
        require(self.projects.contains_key(project), "关联项目不存在")?;
        let session_id = session_id.trim().to_ascii_lowercase();
        validate_qoder_session_id(&session_id)?;
        self.launch_desktop(
            ReadRequest::Discover { version: 1 },
            Action::BindSession {
                project: project.into(),
                session_id,
            },
        )
    }
    fn save_desktop_binding(
        &mut self,
        project: String,
        label: String,
        observation: DesktopObservation,
    ) -> Result<(), String> {
        require(self.projects.contains_key(&project), "关联项目不存在")?;
        let mut binding = DesktopBinding {
            id: String::new(),
            project_id: project,
            label,
            observation,
            confirmed_at: chrono::Utc::now().to_rfc3339(),
            bridge_sha256: fingerprint(),
        };
        binding.id = binding_digest(&binding)?;
        persist(&self.root, &binding)?;
        self.desktop_access.bindings = load(&self.root)?;
        self.desktop_access.observation = None;
        self.desktop_access.observation_digest = None;
        Ok(())
    }
    pub(super) fn inspect_desktop(&mut self, window: DesktopWindow) -> Result<(), String> {
        window.app.require_available()?;
        require(
            self.desktop_active.is_none() && self.desktop_probe.is_none(),
            "桌面观察仍在进行",
        )?;
        validate_window(&window)?;
        require(
            self.desktop_access.windows.contains(&window),
            "请先刷新窗口列表并选择窗口",
        )?;
        self.desktop_access.observation = None;
        self.desktop_access.observation_digest = None;
        self.launch_desktop(
            ReadRequest::Inspect {
                version: 1,
                window: window.clone(),
            },
            Action::Inspect(window),
        )
    }
    pub(super) fn bind_desktop(
        &mut self,
        project: &str,
        expected: &str,
        label: &str,
    ) -> Result<(), String> {
        require(self.projects.contains_key(project), "关联项目不存在")?;
        require(
            self.desktop_access.observation_digest.as_deref() == Some(expected),
            "会话观察已变化，请重新读取",
        )?;
        let previous = self
            .desktop_access
            .observation
            .clone()
            .ok_or("请先读取当前会话")?;
        let at = chrono::DateTime::parse_from_rfc3339(&previous.captured_at)
            .map_err(|_| "观察时间无效")?;
        require(
            (0..=120)
                .contains(&(chrono::Utc::now() - at.with_timezone(&chrono::Utc)).num_seconds()),
            "会话观察已过期，请重新读取",
        )?;
        require(
            !label.trim().is_empty()
                && label.chars().count() <= 120
                && !label.chars().any(char::is_control),
            "请输入有效关联名称",
        )?;
        self.launch_desktop(
            ReadRequest::Inspect {
                version: 1,
                window: previous.window.clone(),
            },
            Action::Bind {
                project: project.into(),
                label: label.trim().into(),
                previous,
            },
        )
    }
    pub(super) fn poll_desktop(&mut self) {
        self.poll_desktop_tasks();
        let Some(probe) = &self.desktop_probe else {
            return;
        };
        let result = match probe.receiver.try_recv() {
            Ok(r) => r,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(_) => Err("桌面桥接中断；若已确认发送，请仅回读核对，勿重发".into()),
        };
        let action = self.desktop_probe.take().unwrap().action;
        self.desktop_access.checking = false;
        let applied = (|| {
            let reply = result?;
            match action {
                Action::BindSession {
                    project,
                    session_id,
                } => {
                    let observation = reply.observation.ok_or("会话绑定结果缺失")?;
                    validate_observation(&observation)?;
                    require(
                        observation.window.app == DesktopApp::Qoder
                            && observation
                                .session_id
                                .as_deref()
                                .is_some_and(|id| id.eq_ignore_ascii_case(&session_id)),
                        "绑定结果与所填会话 ID 不一致",
                    )?;
                    let bridge = fingerprint();
                    let existing = self
                        .desktop_access
                        .bindings
                        .iter()
                        .rev()
                        .find(|binding| {
                            binding.project_id == project
                                && binding
                                    .observation
                                    .session_id
                                    .as_deref()
                                    .is_some_and(|id| id.eq_ignore_ascii_case(&session_id))
                        })
                        .is_some_and(|binding| {
                            binding.bridge_sha256 == bridge
                                && same_view(&binding.observation, &observation)
                        });
                    if !existing {
                        self.save_desktop_binding(
                            project,
                            format!("Qoder {session_id}"),
                            observation,
                        )?;
                    }
                    self.desktop_access.observation = None;
                    self.desktop_access.observation_digest = None;
                }
                Action::PrepareTask {
                    project,
                    task,
                    attempt,
                } => {
                    self.require_available(&project)?;
                    let store = self.projects.get(&project).ok_or("项目不存在")?;
                    self.desktop_access.task_review =
                        Some(crate::desktop_execution::prepare_continuing(
                            &self.root,
                            store,
                            &task,
                            &attempt,
                            reply.observation.ok_or("任务观察缺失")?,
                            reply.baseline,
                        )?);
                }
                Action::PrepareTrial(binding) => desktop_trials::prepared(self, binding, reply)?,
                Action::Trial(id) => desktop_trials::completed(self, &id, reply)?,
                Action::Discover => {
                    require(
                        reply.observation.is_none() && reply.windows.len() <= 32,
                        "窗口列表格式错误",
                    )?;
                    for w in &reply.windows {
                        validate_window(w)?;
                    }
                    self.desktop_access.windows = reply
                        .windows
                        .into_iter()
                        .filter(|w| w.app.require_available().is_ok())
                        .collect();
                }
                Action::Inspect(expected) => {
                    let observation = reply.observation.ok_or("会话观察缺失")?;
                    validate_observation(&observation)?;
                    require(observation.window == expected, "窗口身份已变化")?;
                    self.desktop_access.observation_digest = Some(digest(&observation)?);
                    self.desktop_access.observation = Some(observation);
                }
                Action::Bind {
                    project,
                    label,
                    previous,
                } => {
                    let observation = reply.observation.ok_or("确认观察缺失")?;
                    validate_observation(&observation)?;
                    require(
                        same_view(&previous, &observation),
                        "会话内容、窗口或布局已变化；未保存关联，请重新读取",
                    )?;
                    self.save_desktop_binding(project, label, observation)?;
                }
            }
            Ok::<_, String>(())
        })();
        if let Err(error) = applied {
            self.desktop_access.error = Some(error);
            self.desktop_access.observation = None;
            self.desktop_access.observation_digest = None;
        }
    }
}

#[path = "desktop_background.rs"]
pub(super) mod desktop_background;

#[path = "desktop_progress.rs"]
pub(super) mod desktop_progress;

#[path = "desktop_trials.rs"]
mod desktop_trials;
pub(super) use desktop_trials::load as load_trials;

#[cfg(test)]
#[path = "desktop_sessions_tests.rs"]
pub(crate) mod tests;

impl RuntimeHost {
    pub(super) fn validate_desktop_dispatch_records(&self) -> Result<(), String> {
        crate::desktop_execution::cancellation::review::validate_all(&self.root, &self.projects)?;
        for (project, attempt) in crate::desktop_execution::recorded_attempts(&self.root)? {
            let store = self.projects.get(&project).ok_or("桌面派发项目缺失")?;
            crate::desktop_execution::inspect(&self.root, store, &project, &attempt)?;
        }
        Ok(())
    }
    pub(super) fn require_desktop_tasks_resolved(&self) -> Result<(), String> {
        for (project, attempt) in crate::desktop_execution::recorded_attempts(&self.root)? {
            let store = self.projects.get(&project).ok_or("桌面派发项目缺失")?;
            let (review, proof) =
                crate::desktop_execution::inspect(&self.root, store, &project, &attempt)?;
            let cancelled =
                crate::desktop_execution::cancellation::inspect(&self.root, store, &review)?
                    .is_some_and(|s| s.confirmed);
            require(
                cancelled || crate::desktop_execution::is_retired(store, &attempt) || (proof.is_some()
                    && store.history().iter().any(|e|matches!(&e.body.event.command,WorkflowCommand::Submit{attempt:id,..}|WorkflowCommand::Reconcile{attempt:id,outcome:ReconcileOutcome::Submitted,..} if id==&attempt))),
                "存在未核实桌面任务发送或未完成交付提交；请核对原记录，禁止新发送",
            )?;
        }
        Ok(())
    }
    /// Synchronous trusted-controller API; not exposed to user IPC or the continuous runner yet.
    /// Caller must arrange foreground permission and own the host for this bounded operation.
    pub fn dispatch_desktop_attempt(
        &mut self,
        project: &str,
        task: &str,
        attempt: &str,
    ) -> Result<crate::desktop_execution::TaskProof, String> {
        require(
            self.desktop_active.is_none() && self.desktop_probe.is_none(),
            "桌面操作仍在进行",
        )?;
        require(
            load_trials(&self.root)?.iter().all(|t| t.proof.is_some()),
            "仍有未核实固定验证，不能派发任务",
        )?;
        self.require_available(project)?;
        self.require_desktop_tasks_resolved()?;
        let store = self.projects.get(project).ok_or("项目不存在")?;
        let selection = store
            .revision()
            .ok_or("方案缺失")?
            .task(task)?
            .desktop_worker
            .as_ref()
            .ok_or("桌面配置缺失")?
            .clone();
        self.validate_desktop_selection(project, &selection)?;
        require(
            selection.binding.bridge_sha256 == fingerprint(),
            "桌面桥接已变化，需重新观察、配置和批准",
        )?;
        let prepared = observe(
            &self.root,
            &ReadRequest::TaskPrepare {
                version: 1,
                window: selection.binding.observation.window.clone(),
                previous: crate::desktop_execution::predecessor(
                    &self.root,
                    store,
                    &selection.binding.id,
                )?,
            },
        )?;
        let review = crate::desktop_execution::prepare_continuing(
            &self.root,
            store,
            task,
            attempt,
            prepared.observation.ok_or("缺少任务观察")?,
            prepared.baseline,
        )?;
        let root = self.root.clone();
        crate::desktop_execution::dispatch(
            &root,
            self.projects.get_mut(project).unwrap(),
            &review,
            |review| {
                observe(
                    &root,
                    &ReadRequest::TaskSend {
                        version: 1,
                        review: review.clone(),
                    },
                )?
                .task_proof
                .ok_or("任务回复尚未核实，禁止重发".into())
            },
        )
    }
    /// Read the original attempt only; never focus, type, invoke, or retry a send.
    pub fn observe_desktop_attempt(
        &mut self,
        project: &str,
        attempt: &str,
    ) -> Result<crate::desktop_execution::TaskProof, String> {
        require(
            self.desktop_active.is_none() && self.desktop_probe.is_none(),
            "桌面操作仍在进行",
        )?;
        let store = self.projects.get_mut(project).ok_or("项目不存在")?;
        crate::desktop_execution::recover_and_submit(
            &self.root,
            store,
            project,
            attempt,
            |review| {
                require(
                    review.bridge_sha256 == fingerprint(),
                    "任务回读须使用原批准桥接版本",
                )?;
                observe(
                    &self.root,
                    &ReadRequest::TaskRead {
                        version: 1,
                        review: review.clone(),
                    },
                )?
                .task_proof
                .ok_or("任务回复尚未核实".into())
            },
        )
    }
}
