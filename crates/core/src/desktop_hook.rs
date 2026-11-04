//! Qoder's supported command-hook transport, scoped to one approved desktop attempt.
//! Hook replies contain additional context only. Permission decisions are never emitted.
use cerulean_domain::execution::{digest, require, sha256};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    os::windows::io::AsRawHandle,
    path::Path,
    sync::mpsc,
    time::Duration,
};
use tokio::net::windows::named_pipe::ClientOptions;
use tokio::sync::{oneshot, watch};
use windows_sys::Win32::System::Pipes::{GetNamedPipeClientProcessId, GetNamedPipeServerProcessId};

const LIMIT: usize = 256 * 1024;
const DEADLINE: Duration = Duration::from_secs(45);

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Configuration {
    pub version: u32,
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    pub endpoint: String,
    pub owner_sid: String,
    pub worker_sid: String,
    pub review_digest: String,
    pub token: String,
    pub workspace: String,
    pub session_id: String,
}
fn enabled_by_default() -> bool {
    true
}
impl Configuration {
    fn validate(&self) -> Result<(), String> {
        require(
            self.version == 1
                && self.endpoint == format!(r"\\.\pipe\CeruleanLonging-hook-{}", self.token),
            "Hook端点无效",
        )?;
        sha256(&self.token)?;
        sha256(&self.review_digest)?;
        require(
            Path::new(&self.workspace).is_absolute()
                && !self.session_id.is_empty()
                && self.session_id.len() <= 200,
            "Hook会话范围无效",
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Event {
    pub name: String,
    pub session_id: String,
    pub agent_id: Option<String>,
    pub workspace: String,
    pub tool_id: Option<String>,
    pub tool_name: Option<String>,
    pub action_digest: Option<String>,
    pub payload_digest: String,
    pub prompt_digest: Option<String>,
    pub interrupted: bool,
}
impl Event {
    pub fn from_input(value: Value) -> Result<Self, String> {
        let field = |name: &str, limit: usize| {
            value[name]
                .as_str()
                .filter(|s| !s.is_empty() && s.len() <= limit && !s.chars().any(char::is_control))
                .map(str::to_owned)
                .ok_or_else(|| format!("Hook字段{name}无效"))
        };
        let name = field("hook_event_name", 64)?;
        require(
            [
                "UserPromptSubmit",
                "PreToolUse",
                "PostToolUse",
                "PostToolUseFailure",
                "Stop",
                "StopFailure",
                "SessionEnd",
            ]
            .contains(&name.as_str()),
            "Hook事件不在采集范围",
        )?;
        let agent_id = match value.get("agent_id").and_then(Value::as_str) {
            Some("") | None => None,
            Some(_) => Some(field("agent_id", 200)?),
        };
        let tool = matches!(
            name.as_str(),
            "PreToolUse" | "PostToolUse" | "PostToolUseFailure"
        );
        let tool_name = if tool {
            Some(field("tool_name", 128)?)
        } else {
            None
        };
        let tool_id = if tool {
            Some(field("tool_use_id", 200)?)
        } else {
            None
        };
        let action_digest = if tool {
            require(value["tool_input"].is_object(), "Hook工具输入缺失")?;
            Some(digest(&(&tool_name, &value["tool_input"]))?)
        } else {
            None
        };
        let prompt_digest = if name == "UserPromptSubmit" {
            let prompt = value["prompt"]
                .as_str()
                .filter(|s| s.len() <= 64 * 1024)
                .ok_or("Hook任务消息无效")?;
            use sha2::Digest;
            Some(format!("{:x}", sha2::Sha256::digest(prompt.as_bytes())))
        } else {
            None
        };
        Ok(Self {
            name,
            session_id: field("session_id", 200)?,
            agent_id,
            workspace: field("cwd", 4096)?,
            tool_id,
            tool_name,
            action_digest,
            payload_digest: digest(&value)?,
            prompt_digest,
            interrupted: value["is_interrupt"] == true,
        })
    }
    pub fn validate(&self, config: &Configuration) -> Result<(), String> {
        let normalize = |path: &str| {
            path.trim_start_matches(r"\\?\")
                .replace('/', "\\")
                .trim_end_matches('\\')
                .to_lowercase()
        };
        require(
            self.session_id == config.session_id
                && normalize(&self.workspace) == normalize(&config.workspace),
            "Hook事件不属于当前任务会话或目录",
        )?;
        sha256(&self.payload_digest)?;
        if let Some(hash) = &self.action_digest {
            sha256(hash)?;
        }
        if let Some(hash) = &self.prompt_digest {
            sha256(hash)?;
        }
        require(
            self.agent_id.as_ref().is_none_or(|id| {
                !id.is_empty() && id.len() <= 200 && !id.chars().any(char::is_control)
            }),
            "Hook执行身份无效",
        )?;
        if matches!(
            self.name.as_str(),
            "PreToolUse" | "PostToolUse" | "PostToolUseFailure"
        ) {
            require(
                self.tool_id.as_ref().is_some_and(|id| {
                    !id.is_empty() && id.len() <= 200 && !id.chars().any(char::is_control)
                }) && self.tool_name.as_ref().is_some_and(|name| {
                    !name.is_empty() && name.len() <= 128 && !name.chars().any(char::is_control)
                }) && self.action_digest.is_some(),
                "Hook工具身份无效",
            )?;
        }
        require(
            [
                "UserPromptSubmit",
                "PreToolUse",
                "PostToolUse",
                "PostToolUseFailure",
                "Stop",
                "StopFailure",
                "SessionEnd",
            ]
            .contains(&self.name.as_str()),
            "Hook事件无效",
        )
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request {
    token: String,
    review_digest: String,
    event: Event,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Advice {
    pub operation_id: String,
    pub text: String,
}
pub struct Incoming {
    pub event: Event,
    pub reply: oneshot::Sender<Option<Advice>>,
}
pub enum Notice {
    Event(Box<Incoming>),
    OutputReleased { operation_id: String },
    OutputUnknown { operation_id: String },
}

pub struct Session {
    pub config: Configuration,
    pub receiver: mpsc::Receiver<Notice>,
    stop: watch::Sender<bool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
impl Session {
    pub fn start(config: Configuration) -> Result<Self, String> {
        config.validate()?;
        let (sender, receiver) = mpsc::sync_channel(64);
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let (stop, mut cancelled) = watch::channel(false);
        let bound = config.clone();
        let thread=std::thread::Builder::new().name("desktop-hook".into()).spawn(move||{
            let runtime=match tokio::runtime::Builder::new_current_thread().enable_all().build(){Ok(runtime)=>runtime,Err(error)=>{let _=ready_sender.send(Err(error.to_string()));return;}};
            runtime.block_on(async move{
                let mut pipe=match crate::ipc::restricted_listener(&bound.endpoint,&[&bound.owner_sid,&bound.worker_sid],true){Ok(pipe)=>pipe,Err(error)=>{let _=ready_sender.send(Err(error.to_string()));return;}};
                let _=ready_sender.send(Ok(()));
                loop {
                    tokio::select! {
                        _=cancelled.changed()=>break,
                        accepted=pipe.connect()=>if accepted.is_err(){break;},
                    }
                    let result=tokio::select! {
                        _=cancelled.changed()=>break,
                        result=tokio::time::timeout(DEADLINE,async{
                            let mut pid=0;
                            require(unsafe{GetNamedPipeClientProcessId(pipe.as_raw_handle(),&mut pid)}!=0,"Hook调用者无法核对")?;
                            require(crate::ipc::process_sid(pid).map_err(|e|e.to_string())?==bound.worker_sid,"Hook执行账户不符")?;
                            crate::ipc::write_frame(&mut pipe,&bound.review_digest,LIMIT).await?;
                            let request:Request=crate::ipc::read_frame(&mut pipe,LIMIT).await?;
                            require(request.token==bound.token && request.review_digest==bound.review_digest,"Hook任务凭据不符")?;
                            request.event.validate(&bound)?;
                            let (reply,answer)=oneshot::channel();
                            sender.try_send(Notice::Event(Box::new(Incoming{event:request.event,reply}))).map_err(|_|"Hook事件队列已满")?;
                            let advice=answer.await.map_err(|_|"Hook任务已结束")?;
                            if let Some(advice)=&advice {
                                let outcome=async{
                                    crate::ipc::write_frame(&mut pipe,&Some(advice),LIMIT).await?;
                                    let ack:String=crate::ipc::read_frame(&mut pipe,1024).await?;
                                    require(ack==advice.operation_id,"Hook输出回执不符")
                                }.await;
                                let notice=if outcome.is_ok(){Notice::OutputReleased{operation_id:advice.operation_id.clone()}}else{Notice::OutputUnknown{operation_id:advice.operation_id.clone()}};
                                let _=sender.try_send(notice);
                                outcome?;
                            }else{crate::ipc::write_frame(&mut pipe,&Option::<Advice>::None,LIMIT).await?;}
                            Ok::<(),String>(())
                        })=>result,
                    };
                    let _=result;
                    let _=pipe.disconnect();
                }
            });
        }).map_err(|e|e.to_string())?;
        match ready_receiver.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Self {
                config,
                receiver,
                stop,
                thread: Some(thread),
            }),
            result => {
                let _ = stop.send(true);
                let _ = thread.join();
                Err(format!("Hook监听启动失败 {result:?}"))
            }
        }
    }
}

/// Runs inside the ordinary Qoder worker identity. It has no core command route.
pub fn command(path: &Path) -> Result<(), String> {
    let mut file = crate::backup::open(path, false, false).map_err(|e| e.to_string())?;
    let mut bytes = vec![];
    (&mut file)
        .take(16 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    require(bytes.len() <= 16 * 1024, "Hook配置过大")?;
    let config: Configuration = serde_json::from_slice(&bytes).map_err(|_| "Hook配置无效")?;
    config.validate()?;
    if !config.enabled {
        println!("{{}}");
        return Ok(());
    }
    let mut bytes = vec![];
    std::io::stdin()
        .take(LIMIT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    require(bytes.len() <= LIMIT, "Hook输入过大")?;
    let event = Event::from_input(serde_json::from_slice(&bytes).map_err(|_| "Hook输入无效")?)?;
    event.validate(&config)?;
    let name = event.name.clone();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    runtime.block_on(async{
        tokio::time::timeout(DEADLINE,async{
            let mut pipe=ClientOptions::new().open(&config.endpoint).map_err(|e|e.to_string())?;
            let mut pid=0;require(unsafe{GetNamedPipeServerProcessId(pipe.as_raw_handle(),&mut pid)}!=0,"Hook服务身份无法核对")?;
            require(crate::ipc::process_sid(pid).map_err(|e|e.to_string())?==config.owner_sid,"Hook服务账户不符")?;
            let review:String=crate::ipc::read_frame(&mut pipe,1024).await?;require(review==config.review_digest,"Hook服务任务不符")?;
            crate::ipc::write_frame(&mut pipe,&Request{token:config.token.clone(),review_digest:config.review_digest.clone(),event},LIMIT).await?;
            let advice:Option<Advice>=crate::ipc::read_frame(&mut pipe,LIMIT).await?;
            let output=advice.as_ref().map_or_else(||json!({}),|advice|json!({"hookSpecificOutput":{"hookEventName":name,"additionalContext":advice.text}}));
            {
                let mut stdout=std::io::stdout().lock();serde_json::to_writer(&mut stdout,&output).map_err(|e|e.to_string())?;
                stdout.write_all(b"\n").and_then(|_|stdout.flush()).map_err(|e|e.to_string())?;
            }
            if let Some(advice)=advice {crate::ipc::write_frame(&mut pipe,&advice.operation_id,1024).await?;}
            Ok::<(),String>(())
        }).await.map_err(|_|"Hook响应超时".to_owned())?
    })
}

fn protected_directory(path: &Path) -> Result<Vec<std::fs::File>, String> {
    let mut pins = vec![];
    for parent in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        if !parent.exists() {
            std::fs::create_dir(parent).map_err(|e| e.to_string())?;
        }
        pins.push(crate::backup::open(parent, true, false).map_err(|e| e.to_string())?);
    }
    Ok(pins)
}

fn read_configuration(path: &Path) -> Result<Option<Vec<u8>>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let file = crate::backup::open(path, false, false).map_err(|e| e.to_string())?;
    let mut bytes = vec![];
    file.take(65537)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    require(bytes.len() <= 65536, "已有Hook配置过大，已保留原配置")?;
    Ok(Some(bytes))
}

fn replace_configuration(
    path: &Path,
    previous: Option<&[u8]>,
    value: &impl Serialize,
    suffix: &str,
) -> Result<(), String> {
    use std::fs::OpenOptions;
    let pending = path.with_extension(format!("{suffix}.pending"));
    let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    require(bytes.len() <= 65536, "Hook配置超出容量")?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&pending)
        .map_err(|e| e.to_string())?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())?;
    drop(file);
    require(
        read_configuration(path)?.as_deref() == previous,
        "Hook配置已被其他程序修改，请重新核对",
    )?;
    std::fs::rename(pending, path).map_err(|e| e.to_string())
}

/// Installs only a project-local command entry in the approved worker directory.
pub fn install(config: &Configuration, executable: &Path) -> Result<(), String> {
    config.validate()?;
    require(config.enabled, "不能安装已停用的Hook连接")?;
    let workspace = Path::new(&config.workspace);
    let runtime = workspace.join(".cerulean-runtime/desktop-supervision");
    let qoder = workspace.join(".qoder");
    let _runtime_pins = protected_directory(&runtime)?;
    let _qoder_pins = protected_directory(&qoder)?;
    let _executable = crate::backup::open(executable, false, false).map_err(|e| e.to_string())?;
    let lock_path = runtime.join("configuration.lock");
    if !lock_path.exists() {
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
            .map_err(|e| e.to_string())?;
    }
    let lock = crate::backup::open(&lock_path, false, false).map_err(|e| e.to_string())?;
    fs2::FileExt::try_lock_exclusive(&lock).map_err(|_| "Hook配置正在由其他任务更新")?;
    let config_path = runtime.join("connection.json");
    let previous_connection = read_configuration(&config_path)?;
    let settings_path = qoder.join("settings.local.json");
    let previous = read_configuration(&settings_path)?;
    let mut settings: Value = match &previous {
        Some(bytes) => {
            serde_json::from_slice(bytes).map_err(|_| "已有Qoder设置无效，已保留原文件")?
        }
        None => json!({}),
    };
    let settings_map = settings.as_object_mut().ok_or("Qoder设置必须是对象")?;
    let hooks = settings_map
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("已有Hooks配置无效")?;
    let quote = |path: &Path| format!("'{}'", path.to_string_lossy().replace('\'', "''"));
    let arguments = format!("--desktop-hook {}", quote(&config_path));
    let command = format!("& {} {arguments}", quote(executable));
    for event in [
        "UserPromptSubmit",
        "PreToolUse",
        "PostToolUse",
        "PostToolUseFailure",
        "Stop",
        "StopFailure",
        "SessionEnd",
    ] {
        let groups = hooks
            .entry(event)
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .ok_or("已有Hook事件配置无效")?;
        for group in groups.iter_mut() {
            let entries = group
                .get_mut("hooks")
                .and_then(Value::as_array_mut)
                .ok_or("已有Hook处理器配置无效")?;
            entries.retain(|entry| {
                !(entry["statusMessage"] == "CeruleanLonging task supervision"
                    && entry["command"]
                        .as_str()
                        .is_some_and(|text| text.ends_with(&arguments)))
            });
        }
        groups.retain(|group| {
            group["hooks"]
                .as_array()
                .is_some_and(|entries| !entries.is_empty())
        });
        groups.push(json!({"hooks":[{"type":"command","command":command,"shell":"powershell","timeout":50,"statusMessage":"CeruleanLonging task supervision"}]}));
    }
    if let Some(bytes) = &previous {
        let backup = runtime.join(format!("settings-before-{}.json", digest(bytes)?));
        if !backup.exists() {
            let mut file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(backup)
                .map_err(|e| e.to_string())?;
            file.write_all(bytes)
                .and_then(|_| file.sync_all())
                .map_err(|e| e.to_string())?;
        }
    }
    replace_configuration(
        &config_path,
        previous_connection.as_deref(),
        config,
        &config.token,
    )?;
    replace_configuration(
        &settings_path,
        previous.as_deref(),
        &settings,
        &config.token,
    )
}

/// Disable the matching connection first, then remove only this workbench's
/// command entries. Never overwrite a newer task's connection or user settings.
pub fn deactivate(config: &Configuration) -> Result<(), String> {
    config.validate()?;
    let workspace = Path::new(&config.workspace);
    let runtime = workspace.join(".cerulean-runtime/desktop-supervision");
    let qoder = workspace.join(".qoder");
    let _runtime_pins = protected_directory(&runtime)?;
    let _qoder_pins = protected_directory(&qoder)?;
    let lock = crate::backup::open(&runtime.join("configuration.lock"), false, false)
        .map_err(|e| e.to_string())?;
    fs2::FileExt::try_lock_exclusive(&lock).map_err(|_| "Hook配置正在由其他任务更新")?;
    let path = runtime.join("connection.json");
    let previous = read_configuration(&path)?.ok_or("原Hook连接配置缺失")?;
    let current: Configuration =
        serde_json::from_slice(&previous).map_err(|_| "原Hook连接配置无效")?;
    current.validate()?;
    if current.token != config.token || current.review_digest != config.review_digest {
        return Ok(()); // A later approved task owns the shared project hook now.
    }
    if current.enabled {
        let mut disabled = current;
        disabled.enabled = false;
        replace_configuration(
            &path,
            Some(&previous),
            &disabled,
            &format!("{}-disabled", config.token),
        )?;
    }
    let settings_path = qoder.join("settings.local.json");
    let Some(previous) = read_configuration(&settings_path)? else {
        return Ok(());
    };
    let mut settings: Value =
        serde_json::from_slice(&previous).map_err(|_| "已有Qoder设置无效，已保留原文件")?;
    let suffix = format!(
        "--desktop-hook '{}'",
        path.to_string_lossy().replace('\'', "''")
    );
    if let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) {
        for groups in hooks.values_mut().filter_map(Value::as_array_mut) {
            for group in groups.iter_mut() {
                if let Some(entries) = group.get_mut("hooks").and_then(Value::as_array_mut) {
                    entries.retain(|entry| {
                        !(entry["statusMessage"] == "CeruleanLonging task supervision"
                            && entry["command"]
                                .as_str()
                                .is_some_and(|command| command.ends_with(&suffix)))
                    });
                }
            }
            groups.retain(|group| !group["hooks"].as_array().is_some_and(Vec::is_empty));
        }
    }
    if serde_json::from_slice::<Value>(&previous).map_err(|e| e.to_string())? != settings {
        replace_configuration(
            &settings_path,
            Some(&previous),
            &settings,
            &format!("{}-removed", config.token),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture_configuration(root: &Path) -> Configuration {
        let token = digest(&(root.to_string_lossy(), std::process::id())).unwrap();
        Configuration {
            enabled: true,
            version: 1,
            endpoint: format!(r"\\.\pipe\CeruleanLonging-hook-{token}"),
            owner_sid: crate::ipc::current_sid().unwrap(),
            worker_sid: crate::ipc::current_sid().unwrap(),
            review_digest: "a".repeat(64),
            token,
            workspace: root.to_string_lossy().into_owned(),
            session_id: "synthetic-session".into(),
        }
    }
    fn input(config: &Configuration) -> Value {
        json!({"hook_event_name":"PostToolUseFailure","session_id":config.session_id,"cwd":config.workspace,"tool_name":"Bash","tool_use_id":"call-one",
            "tool_input":{"command":"synthetic-sensitive-command"},"error":"synthetic failure","is_interrupt":false})
    }
    #[test]
    fn hook_event_keeps_only_bounded_diagnostic_identity() {
        let temp = tempfile::tempdir().unwrap();
        let config = fixture_configuration(temp.path());
        let event = Event::from_input(input(&config)).unwrap();
        event.validate(&config).unwrap();
        let text = serde_json::to_string(&event).unwrap();
        assert!(!text.contains("synthetic-sensitive-command"));
        assert!(!text.contains("synthetic failure"));
        let mut changed = event.clone();
        changed.session_id = "other".into();
        assert!(changed.validate(&config).is_err());
        let mut changed = event;
        changed.workspace = temp.path().join("other").to_string_lossy().into_owned();
        assert!(changed.validate(&config).is_err());
        let mut denied = input(&config);
        denied["hook_event_name"] = "PermissionRequest".into();
        assert!(Event::from_input(denied).is_err());
        let mut missing = input(&config);
        missing.as_object_mut().unwrap().remove("tool_use_id");
        assert!(Event::from_input(missing).is_err());
        let mut oversized = input(&config);
        oversized["tool_name"] = "x".repeat(129).into();
        assert!(Event::from_input(oversized).is_err());
    }
    #[test]
    fn project_hook_install_preserves_existing_settings_and_is_idempotent() {
        let temp = tempfile::tempdir().unwrap();
        let config = fixture_configuration(temp.path());
        std::fs::create_dir(temp.path().join(".qoder")).unwrap();
        let file = temp.path().join(".qoder/settings.local.json");
        let before = json!({"language":"original","hooks":{"PermissionRequest":[{"hooks":[{"type":"command","command":"original-permission-handler"}]}],
            "PostToolUse":[{"hooks":[{"type":"command","command":"original-post-handler"}]}]}});
        std::fs::write(&file, serde_json::to_vec(&before).unwrap()).unwrap();
        install(&config, &std::env::current_exe().unwrap()).unwrap();
        let first: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(first["language"], before["language"]);
        assert_eq!(
            first["hooks"]["PermissionRequest"],
            before["hooks"]["PermissionRequest"]
        );
        assert_eq!(
            first["hooks"]["PostToolUse"][0],
            before["hooks"]["PostToolUse"][0]
        );
        install(&config, &std::env::current_exe().unwrap()).unwrap();
        let second: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(first, second);
        assert_eq!(
            second["hooks"]["PostToolUseFailure"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        std::fs::write(&file, b"invalid existing user settings").unwrap();
        assert!(install(&config, &std::env::current_exe().unwrap()).is_err());
        assert_eq!(
            std::fs::read(file).unwrap(),
            b"invalid existing user settings"
        );
    }
    fn client(config: Configuration, ack: bool) -> Result<Option<Advice>, String> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let mut pipe = ClientOptions::new()
                    .open(&config.endpoint)
                    .map_err(|e| e.to_string())?;
                let greeting: String = crate::ipc::read_frame(&mut pipe, 1024).await?;
                require(greeting == config.review_digest, "greeting mismatch")?;
                let event = Event::from_input(input(&config))?;
                crate::ipc::write_frame(
                    &mut pipe,
                    &Request {
                        token: config.token,
                        review_digest: config.review_digest,
                        event,
                    },
                    LIMIT,
                )
                .await?;
                let reply: Option<Advice> = crate::ipc::read_frame(&mut pipe, LIMIT).await?;
                if ack {
                    if let Some(advice) = &reply {
                        crate::ipc::write_frame(&mut pipe, &advice.operation_id, 1024).await?;
                    }
                }
                Ok(reply)
            })
    }
    #[test]
    fn deactivation_preserves_user_hooks_and_does_not_disable_a_later_task() {
        let temp = tempfile::tempdir().unwrap();
        let config = fixture_configuration(temp.path());
        let exe = std::env::current_exe().unwrap();
        install(&config, &exe).unwrap();
        let settings_path = temp.path().join(".qoder/settings.local.json");
        let config_path = temp
            .path()
            .join(".cerulean-runtime/desktop-supervision/connection.json");
        let mut settings: Value =
            serde_json::from_slice(&std::fs::read(&settings_path).unwrap()).unwrap();
        settings["language"] = json!("user-changed");
        settings["hooks"]["PostToolUse"]
            .as_array_mut()
            .unwrap()
            .push(json!({"hooks":[{"command":"user-handler"}]}));
        std::fs::write(&settings_path, serde_json::to_vec(&settings).unwrap()).unwrap();
        deactivate(&config).unwrap();
        deactivate(&config).unwrap();
        let after: Value = serde_json::from_slice(&std::fs::read(&settings_path).unwrap()).unwrap();
        assert_eq!(after["language"], "user-changed");
        assert_eq!(
            after["hooks"]["PostToolUse"][0]["hooks"][0]["command"],
            "user-handler"
        );
        assert!(!after.to_string().contains("--desktop-hook"));
        let disabled: Configuration =
            serde_json::from_slice(&std::fs::read(&config_path).unwrap()).unwrap();
        assert!(!disabled.enabled);
        let mut later = config.clone();
        later.token = "b".repeat(64);
        later.endpoint = format!(r"\\.\pipe\CeruleanLonging-hook-{}", later.token);
        later.review_digest = "c".repeat(64);
        install(&later, &exe).unwrap();
        let before = std::fs::read(&config_path).unwrap();
        deactivate(&config).unwrap();
        assert_eq!(std::fs::read(&config_path).unwrap(), before);
        deactivate(&later).unwrap();
    }
    #[test]
    fn native_hook_pipe_releases_once_and_records_missing_output_ack() {
        for acknowledge in [true, false] {
            let temp = tempfile::tempdir().unwrap();
            let config = fixture_configuration(temp.path());
            let session = Session::start(config.clone()).unwrap();
            let caller = std::thread::spawn(move || client(config, acknowledge));
            let Notice::Event(incoming) = session
                .receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
            else {
                panic!("expected event");
            };
            incoming
                .reply
                .send(Some(Advice {
                    operation_id: "advice-fixture".into(),
                    text: "Keep the approved task scope".into(),
                }))
                .unwrap();
            assert_eq!(
                caller.join().unwrap().unwrap().unwrap().operation_id,
                "advice-fixture"
            );
            let notice = session
                .receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            assert!(matches!(
                (&notice, acknowledge),
                (Notice::OutputReleased { .. }, true) | (Notice::OutputUnknown { .. }, false)
            ));
        }
    }
    #[test]
    fn native_hook_pipe_rejects_other_identity_and_cancel_closes_pending_request() {
        let temp = tempfile::tempdir().unwrap();
        let mut config = fixture_configuration(temp.path());
        config.worker_sid = "S-1-5-18".into();
        let session = Session::start(config.clone()).unwrap();
        assert!(client(config, false).is_err());
        assert!(session.receiver.try_recv().is_err());
        drop(session);
        let temp = tempfile::tempdir().unwrap();
        let config = fixture_configuration(temp.path());
        let session = Session::start(config.clone()).unwrap();
        let caller = std::thread::spawn(move || client(config, false));
        let Notice::Event(incoming) = session
            .receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
        else {
            panic!("expected event");
        };
        drop(session);
        assert!(caller.join().unwrap().is_err());
        assert!(incoming.reply.is_closed());
    }
}
