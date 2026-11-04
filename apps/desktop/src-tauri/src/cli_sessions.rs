//! Project-scoped progress windows have a read-only native route.
use cerulean_domain::{
    execution::{digest, identifier},
    Command, Request, Snapshot,
};
use std::{collections::BTreeMap, sync::Mutex};
use tauri::Manager;

#[derive(Clone)]
struct Scope {
    owner: String,
    project: String,
}
#[derive(Default)]
pub struct Windows(Mutex<BTreeMap<String, Scope>>);
pub fn is_label(label: &str) -> bool {
    label.starts_with("cli-session-")
}
impl Windows {
    pub fn close_owner(&self, owner: &str, app: &tauri::AppHandle) -> Result<(), String> {
        let mut windows = self.0.lock().map_err(|_| "会话窗口登记不可用")?;
        let labels = windows
            .iter()
            .filter(|(_, s)| s.owner == owner)
            .map(|(l, _)| l.clone())
            .collect::<Vec<_>>();
        for label in labels {
            if let Some(window) = app.get_webview_window(&label) {
                window.destroy().map_err(|e| e.to_string())?;
            }
            windows.remove(&label);
        }
        Ok(())
    }
}
#[cfg(windows)]
async fn snapshot(state: &super::State, owner: &str) -> Result<Snapshot, String> {
    use cerulean_core::ipc::{Client, UserOperation};
    let directory = state.workbenches.directory(owner)?;
    let client = Client::connect(&directory)
        .await
        .map_err(|_| "请先在所属工作台连接核心")?;
    let reply = client
        .call(UserOperation::Request {
            request: Request {
                id: format!(
                    "cli-window-{}",
                    state
                        .sequence
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
                ),
                protocol_version: super::PROTOCOL_VERSION,
                command: Command::Snapshot,
            },
        })
        .await?;
    if let Some(error) = reply.response.error {
        return Err(error.message);
    }
    reply.response.result.ok_or("会话进度缺失".into())
}
#[tauri::command]
pub async fn open_cli_session_window(
    project_id: String,
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, super::State>,
) -> Result<(), String> {
    identifier(&project_id)?;
    let _access = state.lifecycle.enter(window.label()).await?;
    state.workbenches.directory(window.label())?;
    let _launch = state
        .workbench_launch
        .try_lock()
        .map_err(|_| "会话窗口正在打开")?;
    #[cfg(windows)]
    {
        let data = snapshot(&state, window.label()).await?;
        if !data.workflows.iter().any(|w| w.project.id == project_id) {
            return Err("请先登记当前编程项目".into());
        }
    }
    let label = format!(
        "cli-session-{}",
        &digest(&(window.label(), &project_id))?[..32]
    );
    if let Some(existing) = app.get_webview_window(&label) {
        return existing
            .unminimize()
            .and_then(|_| existing.show())
            .and_then(|_| existing.set_focus())
            .map_err(|e| e.to_string());
    }
    let mut config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == "main")
        .ok_or("缺少窗口配置")?
        .clone();
    config.label = label.clone();
    config.title = format!("CeruleanLonging · 项目进度与CLI会话 · {project_id}");
    config.url = tauri::WebviewUrl::App("index.html#cli-session".into());
    config.width = 1040.;
    config.height = 760.;
    config.min_width = Some(640.);
    config.min_height = Some(480.);
    let profile = app
        .state::<super::webview_profile::Profile>()
        .directory
        .clone();
    state
        .cli_sessions
        .0
        .lock()
        .map_err(|_| "会话窗口登记不可用")?
        .insert(
            label.clone(),
            Scope {
                owner: window.label().into(),
                project: project_id,
            },
        );
    let opened = tauri::WebviewWindowBuilder::from_config(&app, &config)
        .map_err(|e| e.to_string())?
        .data_directory(profile)
        .build();
    if let Err(error) = opened {
        state
            .cli_sessions
            .0
            .lock()
            .map_err(|_| "会话窗口登记不可用")?
            .remove(&label);
        return Err(error.to_string());
    }
    Ok(())
}
fn project_view(data: &Snapshot, project: &str) -> Result<serde_json::Value, String> {
    let workflow = data
        .workflows
        .iter()
        .find(|w| w.project.id == project)
        .ok_or("当前项目已不可用")?;
    let mut runs = vec![];
    for job in data.codex_jobs.iter().filter(|j| j.project_id == project) {
        runs.push(serde_json::json!({"adapter":"Codex","task":job.task_id,"attempt":job.attempt_id,"model":job.worker.model,"state":job.state,"message":job.message}));
    }
    for job in data.omp_jobs.iter().filter(|j| j.project_id == project) {
        runs.push(serde_json::json!({"adapter":"OMP","task":job.task_id,"attempt":job.attempt_id,"model":job.worker.model,"state":job.state,"message":job.message}));
    }
    let progress = data
        .blind_reviews
        .iter()
        .rev()
        .find(|r| r.project_id == project)
        .and_then(|r| r.session_progress.clone());
    Ok(
        serde_json::json!({"projectId":project,"root":workflow.project.root,"total":workflow.state.tasks.len(),"completed":workflow.state.tasks.values().filter(|t|t.state == cerulean_domain::workflow::TaskState::Accepted).count(),"runs":runs,"reviewProgress":progress}),
    )
}
#[tauri::command]
pub async fn cli_session_read(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, super::State>,
) -> Result<serde_json::Value, String> {
    let scope = state
        .cli_sessions
        .0
        .lock()
        .map_err(|_| "会话窗口登记不可用")?
        .get(window.label())
        .cloned()
        .ok_or("请从项目会话窗口读取")?;
    let _access = state.lifecycle.enter(&scope.owner).await?;
    if app.get_webview_window(&scope.owner).is_none() {
        return Err("所属工作台已关闭".into());
    }
    #[cfg(windows)]
    {
        project_view(&snapshot(&state, &scope.owner).await?, &scope.project)
    }
    #[cfg(not(windows))]
    {
        Err("会话进度需要Windows核心".into())
    }
}
