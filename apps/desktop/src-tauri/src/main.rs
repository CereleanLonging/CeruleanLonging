#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use cerulean_domain::{Command, Request, Snapshot, PROTOCOL_VERSION};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use tauri::Manager;
mod experiment_workbench;
mod maintenance;
mod portable_archive;
mod webview_profile;

#[tauri::command]
async fn scan_codex_installations(
    hint: Option<String>,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, State>,
) -> Result<cerulean_core::codex_discovery::ScanResult, String> {
    state.workbenches.directory(window.label())?;
    tauri::async_runtime::spawn_blocking(move || {
        cerulean_core::codex_discovery::scan(hint.as_deref())
    })
    .await
    .map_err(|_| "本机 Codex 扫描未完成，请稍后重试".to_string())?
}

#[tauri::command]
fn review_workflow_template(
    input: String,
) -> Result<cerulean_domain::workflow_template::WorkflowTemplateReview, String> {
    cerulean_domain::workflow_template::review(&input)
}

struct State {
    workbenches: experiment_workbench::Windows,
    experiment_root: PathBuf,
    workbench_launch: tokio::sync::Mutex<()>,
    executable: PathBuf,
    sequence: AtomicU64,
    maintenance_cache: PathBuf,
    maintenance_launch: tokio::sync::Mutex<()>,
    archive: portable_archive::Service,
}
#[tauri::command]
async fn portable_evidence(
    request: portable_archive::Request,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, State>,
) -> Result<serde_json::Value, String> {
    state.workbenches.directory(window.label())?;
    state.archive.run(request).await
}
#[tauri::command]
async fn workspace_read(
    query: cerulean_domain::workspace::WorkspaceQuery,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, State>,
) -> Result<cerulean_domain::workspace::WorkspaceResult, String> {
    let directory = state.workbenches.directory(window.label())?;
    #[cfg(windows)]
    {
        use cerulean_core::ipc::{Client, UserOperation};
        let reply = Client::connect_or_start(&directory, &state.executable)
            .await?
            .call(UserOperation::Workspace {
                id: format!(
                    "{}-{}",
                    std::process::id(),
                    state.sequence.fetch_add(1, Ordering::Relaxed)
                ),
                protocol_version: PROTOCOL_VERSION,
                query,
            })
            .await?;
        if let Some(error) = reply.response.error {
            return Err(error.message);
        }
        reply.workspace.ok_or("核心没有返回工作区查询结果".into())
    }
    #[cfg(not(windows))]
    {
        let _ = (query, state);
        Err("工作区查询需要Windows核心".into())
    }
}
#[tauri::command]
async fn core_submit(
    command: Command,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, State>,
) -> Result<cerulean_domain::command_result::CommandResult, String> {
    let directory = state.workbenches.directory(window.label())?;
    #[cfg(windows)]
    {
        use cerulean_core::ipc::{Client, UserOperation};
        use cerulean_domain::command_result::CommandResult;
        let client = Client::connect_or_start(&directory, &state.executable).await?;
        if matches!(
            command,
            Command::SendDesktopTrial { .. } | Command::SendDesktopTask { .. }
        ) {
            // Same authenticated foreground handoff as the original entry.
            // The bridge still enforces its action-time foreground guards.
            let _ = client.allow_desktop_foreground();
        }
        let receipt = command.supports_receipt();
        let request = Request {
            id: format!(
                "{}-{}",
                std::process::id(),
                state.sequence.fetch_add(1, Ordering::Relaxed)
            ),
            protocol_version: PROTOCOL_VERSION,
            command,
        };
        let operation = if receipt {
            UserOperation::Command { request }
        } else {
            UserOperation::Request { request }
        };
        // Select the result route before the only send. Never retry a command
        // because its acknowledgement or a later read failed.
        let reply = client
            .call(operation)
            .await
            .map_err(|e| format!("操作结果尚未确认，未重复发送；请读取原记录核对：{e}"))?;
        if let Some(error) = reply.response.error {
            return Err(error.message);
        }
        if receipt {
            reply
                .command_receipt
                .map(CommandResult::Receipt)
                .ok_or("核心未返回操作确认；未重发".into())
        } else {
            reply
                .response
                .result
                .map(|s| CommandResult::Snapshot(Box::new(s)))
                .ok_or("核心未返回查询结果".into())
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (command, state);
        Err("操作提交需要Windows核心".into())
    }
}
#[tauri::command]
async fn historical_request(
    command: Command,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, State>,
) -> Result<cerulean_domain::historical_money::HistoricalResult, String> {
    let directory = state.workbenches.directory(window.label())?;
    if !command.is_historical() {
        return Err("历史接口不接受其他工作台操作".into());
    }
    #[cfg(windows)]
    {
        use cerulean_core::ipc::{Client, UserOperation};
        let client = Client::connect_or_start(&directory, &state.executable).await?;
        let reply = client
            .call(UserOperation::Historical {
                request: Request {
                    id: format!(
                        "{}-{}",
                        std::process::id(),
                        state.sequence.fetch_add(1, Ordering::Relaxed)
                    ),
                    protocol_version: PROTOCOL_VERSION,
                    command,
                },
            })
            .await
            .map_err(|e| format!("核心通信中断，未重复发送；请读取原确认核对：{e}"))?;
        if let Some(e) = reply.response.error {
            return Err(e.message);
        }
        reply
            .historical
            .ok_or("核心未返回独立历史结果；未重发".into())
    }
    #[cfg(not(windows))]
    {
        let _ = state;
        Err("历史查询需要Windows核心".into())
    }
}
#[tauri::command]
async fn count_resolution_request(
    command: Command,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, State>,
) -> Result<cerulean_domain::count_resolution::CountResolutionResult, String> {
    let directory = state.workbenches.directory(window.label())?;
    if !command.is_count_resolution() {
        return Err("计数核对接口不接受其他工作台操作".into());
    }
    #[cfg(windows)]
    {
        use cerulean_core::ipc::{Client, UserOperation};
        let client = Client::connect_or_start(&directory, &state.executable).await?;
        let reply = client
            .call(UserOperation::CountResolution {
                request: Request {
                    id: format!(
                        "{}-{}",
                        std::process::id(),
                        state.sequence.fetch_add(1, Ordering::Relaxed)
                    ),
                    protocol_version: PROTOCOL_VERSION,
                    command,
                },
            })
            .await
            .map_err(|e| format!("核心通信中断，未重复发送；请读取原确认核对：{e}"))?;
        if let Some(e) = reply.response.error {
            return Err(e.message);
        }
        reply
            .count_resolution
            .ok_or("核心未返回独立计数核对结果；未重发".into())
    }
    #[cfg(not(windows))]
    {
        let _ = state;
        Err("计数核对查询需要Windows核心".into())
    }
}
#[tauri::command]
fn maintenance_info(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, State>,
) -> Result<serde_json::Value, String> {
    let directory = state.workbenches.directory(window.label())?;
    Ok(maintenance::information(&directory, &state.executable))
}
#[tauri::command]
async fn open_maintenance_window(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, State>,
    import_retention: Option<bool>,
) -> Result<serde_json::Value, String> {
    let directory = state.workbenches.directory(window.label())?;
    let _guard = state
        .maintenance_launch
        .try_lock()
        .map_err(|_| "维护窗口正在准备，请等待原操作结果")?;
    #[cfg(windows)]
    {
        let data = directory.clone();
        let executable = state.executable.clone();
        let cache = state.maintenance_cache.clone();
        tokio::task::spawn_blocking(move || {
            maintenance::stage(&data, &executable, &cache).and_then(|session| {
                if import_retention.unwrap_or(false) {
                    maintenance::launch_import_retention(session)
                } else {
                    maintenance::launch(session)
                }
            })
        })
        .await
        .map_err(|e| format!("维护窗口准备被中断，未重试：{e}"))?
    }
    #[cfg(not(windows))]
    {
        Err("维护窗口仅支持Windows".into())
    }
}
#[tauri::command]
async fn restart_data_diagnostics(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, State>,
) -> Result<Snapshot, String> {
    let directory = state.workbenches.directory(window.label())?;
    #[cfg(windows)]
    {
        cerulean_core::ipc::Client::restart_data_diagnostics(&directory, &state.executable).await
    }
    #[cfg(not(windows))]
    {
        let _ = state;
        Err("恢复需要Windows".into())
    }
}
#[tauri::command]
async fn restart_reviewed_stops(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, State>,
) -> Result<Snapshot, String> {
    let directory = state.workbenches.directory(window.label())?;
    #[cfg(windows)]
    {
        cerulean_core::ipc::Client::restart_reviewed_stops(&directory, &state.executable).await
    }
    #[cfg(not(windows))]
    {
        let _ = state;
        Err("恢复需要Windows".into())
    }
}
#[tauri::command]
async fn core_request(
    command: Command,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, State>,
) -> Result<Snapshot, String> {
    let directory = state.workbenches.directory(window.label())?;
    #[cfg(windows)]
    {
        use cerulean_core::ipc::{Client, UserOperation};
        // Startup/reconnect is permitted only before sending this user's command.
        // Any error after call begins returns to UI; mutations are never replayed.
        let client = Client::connect_or_start(&directory, &state.executable).await?;
        if matches!(
            command,
            Command::SendDesktopTrial { .. } | Command::SendDesktopTask { .. }
        ) {
            // Forward Windows foreground permission only to our authenticated
            // core; the core then forwards it to its single owned desktop bridge.
            // Failure is still enforced by the bridge's action-time guards.
            let _ = client.allow_desktop_foreground();
        }
        let reply = client
            .call(UserOperation::Request {
                request: Request {
                    id: format!(
                        "{}-{}",
                        std::process::id(),
                        state.sequence.fetch_add(1, Ordering::Relaxed)
                    ),
                    protocol_version: PROTOCOL_VERSION,
                    command,
                },
            })
            .await
            .map_err(|e| format!("核心通信中断，未重复发送；请读取当前记录核对：{e}"))?;
        if let Some(error) = reply.response.error {
            return Err(error.message);
        }
        reply.response.result.ok_or("核心没有返回结果".into())
    }
    #[cfg(not(windows))]
    {
        let _ = (command, state);
        Err("首版常驻核心仅支持 Windows".into())
    }
}
#[tauri::command]
async fn experiment_workbenches(
    window: tauri::WebviewWindow,
    state: tauri::State<'_, State>,
) -> Result<serde_json::Value, String> {
    let current = state.workbenches.directory(window.label())?;
    #[cfg(windows)]
    {
        let root = state.experiment_root.clone();
        let label = window.label().to_owned();
        tokio::task::spawn_blocking(move || {
            let entries = experiment_workbench::catalog::list(&root)?;
            Ok(serde_json::json!({"currentLabel":label,"currentDirectory":current,"catalogRoot":root,"entries":entries}))
        }).await.map_err(|e| format!("读取工作台登记中断：{e}"))?
    }
    #[cfg(not(windows))]
    {
        let _ = current;
        Err("实验工作台需要Windows核心".into())
    }
}
#[tauri::command]
async fn create_experiment_workbench(
    id: String,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, State>,
) -> Result<serde_json::Value, String> {
    state.workbenches.directory(window.label())?;
    let _guard = state
        .workbench_launch
        .try_lock()
        .map_err(|_| "工作台操作尚未返回，请核对原结果")?;
    #[cfg(windows)]
    {
        let root = state.experiment_root.clone();
        tokio::task::spawn_blocking(move || {
            experiment_workbench::catalog::create(&root, &id)
                .and_then(|entry| serde_json::to_value(entry).map_err(|e| e.to_string()))
        })
        .await
        .map_err(|e| format!("创建结果未确认；请读取原登记，不自动重试：{e}"))?
    }
    #[cfg(not(windows))]
    {
        let _ = id;
        Err("实验工作台需要Windows核心".into())
    }
}
#[tauri::command]
async fn open_experiment_workbench(
    id: String,
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, State>,
) -> Result<serde_json::Value, String> {
    state.workbenches.directory(window.label())?;
    let _guard = state
        .workbench_launch
        .try_lock()
        .map_err(|_| "工作台操作尚未返回，请核对原结果")?;
    #[cfg(windows)]
    {
        let label = if id == "main" {
            "main".into()
        } else {
            let root = state.experiment_root.clone();
            let selected = id.clone();
            let (_, binding) = tokio::task::spawn_blocking(move || {
                experiment_workbench::catalog::open(&root, &selected)
            })
            .await
            .map_err(|e| format!("工作台核对中断，未启动：{e}"))??;
            let label = format!("experiment-{id}");
            state.workbenches.bind(&label, binding)?;
            label
        };
        if let Some(existing) = app.get_webview_window(&label) {
            existing
                .unminimize()
                .and_then(|_| existing.show())
                .and_then(|_| existing.set_focus())
                .map_err(|e| e.to_string())?;
        } else {
            // A new WebView has independent React state and immutable IPC routing.
            // Reuse the process's WebView2 environment; no Host is stopped/switched.
            let mut config = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == "main")
                .ok_or("缺少窗口配置")?
                .clone();
            config.label = label.clone();
            config.title = if id == "main" {
                "CeruleanLonging".into()
            } else {
                format!("CeruleanLonging · {id}")
            };
            let profile = app.state::<webview_profile::Profile>().directory.clone();
            tauri::WebviewWindowBuilder::from_config(&app, &config)
                .map_err(|e| e.to_string())?
                .data_directory(profile)
                .build()
                .map_err(|e| format!("目录已保留，窗口未确认打开：{e}"))?;
        }
        Ok(serde_json::json!({"label":label,"directory":state.workbenches.directory(&label)?}))
    }
    #[cfg(not(windows))]
    {
        let _ = (id, app);
        Err("实验工作台需要Windows核心".into())
    }
}
fn main() {
    tauri::Builder::default()
        .setup(|app| {
            // Browser switches (including debug ports) cannot share WebView2's
            // environment. Only disposable browser data is separated per process.
            let cache_root = app
                .path()
                .app_local_data_dir()?
                .join("webview-instances-v1");
            #[cfg(debug_assertions)]
            let cache_root = std::env::var_os("CERULEAN_TEST_WEBVIEW_ROOT")
                .map(PathBuf::from)
                .unwrap_or(cache_root);
            let profile = webview_profile::Profile::acquire(&cache_root)?;
            let profile_directory = profile.directory.clone();
            app.manage(profile);
            let directory = app.path().app_data_dir()?.join("workspace");
            #[cfg(debug_assertions)]
            let directory = std::env::var_os("CERULEAN_TEST_DATA_DIR")
                .map(PathBuf::from)
                .unwrap_or(directory);
            let current = std::env::current_exe()?;
            let executable = current
                .parent()
                .ok_or("无法定位核心程序")?
                .join(if cfg!(windows) {
                    "cerulean-runtime.exe"
                } else {
                    "cerulean-runtime"
                });
            let maintenance_cache = app
                .path()
                .app_local_data_dir()?
                .join("maintenance-sessions-v1");
            #[cfg(debug_assertions)]
            let maintenance_cache = std::env::var_os("CERULEAN_TEST_MAINTENANCE_ROOT")
                .map(PathBuf::from)
                .unwrap_or(maintenance_cache);
            let experiment_root = directory
                .parent()
                .ok_or("工作台缺少父目录")?
                .join("experiment-workbenches-v1");
            let workbenches = experiment_workbench::Windows::default();
            workbenches.bind(
                "main",
                experiment_workbench::Binding {
                    directory,
                    _pins: vec![],
                },
            )?;
            app.manage(State {
                workbenches,
                experiment_root,
                workbench_launch: tokio::sync::Mutex::new(()),
                executable,
                sequence: AtomicU64::new(1),
                maintenance_cache,
                maintenance_launch: tokio::sync::Mutex::new(()),
                archive: portable_archive::Service::default(),
            });
            let config = app
                .config()
                .app
                .windows
                .iter()
                .find(|window| window.label == "main")
                .ok_or("缺少主窗口配置")?;
            tauri::WebviewWindowBuilder::from_config(app, config)?
                .data_directory(profile_directory)
                .build()?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            scan_codex_installations,
            review_workflow_template,
            portable_evidence,
            historical_request,
            count_resolution_request,
            workspace_read,
            core_submit,
            core_request,
            restart_reviewed_stops,
            restart_data_diagnostics,
            maintenance_info,
            open_maintenance_window,
            experiment_workbenches,
            create_experiment_workbench,
            open_experiment_workbench
        ])
        .run(tauri::generate_context!())
        .expect("CeruleanLonging startup failed");
}
