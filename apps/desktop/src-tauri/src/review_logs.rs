//! Read-only review window. It is deliberately absent from the workbench
//! bindings, so ordinary core/mutation commands cannot route from this window.
use cerulean_domain::workspace::{WorkspaceQuery, WorkspaceResult};
use tauri::Manager;

const PREFIX: &str = "review-logs-";
pub fn label(owner: &str) -> String {
    format!("{PREFIX}{owner}")
}
pub fn owner(label: &str) -> Option<&str> {
    label.strip_prefix(PREFIX).filter(|owner| !owner.is_empty())
}
fn allow_query(query: &WorkspaceQuery) -> Result<(), String> {
    match query {
        WorkspaceQuery::ApiPublicationReviews { .. }
        | WorkspaceQuery::ApiPublicationReview { .. }
        | WorkspaceQuery::ManagementRecordReviews { .. }
        | WorkspaceQuery::ManagementRecordReview { .. } => Ok(()),
        _ => Err("此窗口仅可读取 API 中断与管理复核日志".into()),
    }
}

#[tauri::command]
pub async fn open_review_logs_window(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, super::State>,
) -> Result<(), String> {
    let _access = state.lifecycle.enter(window.label()).await?;
    state.workbenches.directory(window.label())?;
    let _launch = state
        .workbench_launch
        .try_lock()
        .map_err(|_| "窗口正在打开，请等待原结果")?;
    let label = label(window.label());
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
    config.label = label;
    config.title = format!("CeruleanLonging · 只读日志 · {}", window.label());
    config.url = tauri::WebviewUrl::App("index.html#review-logs".into());
    config.width = 1060.;
    config.height = 760.;
    config.min_width = Some(760.);
    config.min_height = Some(520.);
    let profile = app
        .state::<super::webview_profile::Profile>()
        .directory
        .clone();
    tauri::WebviewWindowBuilder::from_config(&app, &config)
        .map_err(|e| e.to_string())?
        .data_directory(profile)
        .build()
        .map_err(|e| format!("日志窗口未确认打开：{e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn review_logs_read(
    query: WorkspaceQuery,
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, super::State>,
) -> Result<WorkspaceResult, String> {
    allow_query(&query)?;
    let owner = owner(window.label()).ok_or("请从只读日志窗口读取")?;
    // Route from the native window label, never a caller-supplied directory or id.
    let directory = state.workbenches.directory(owner)?;
    let _access = state.lifecycle.enter(owner).await?;
    if app.get_webview_window(owner).is_none() {
        return Err("所属工作台已经关闭".into());
    }
    #[cfg(windows)]
    {
        use cerulean_core::ipc::{Client, UserOperation};
        use std::sync::atomic::Ordering;
        // A log refresh must not restart a stopped core or initialize a data root.
        let client = Client::connect(&directory)
            .await
            .map_err(|e| format!("日志读取失败，请先在所属工作台核对核心连接：{e}"))?;
        let reply = client
            .call(UserOperation::Workspace {
                id: format!(
                    "logs-{}-{}",
                    std::process::id(),
                    state.sequence.fetch_add(1, Ordering::Relaxed)
                ),
                protocol_version: super::PROTOCOL_VERSION,
                query,
            })
            .await?;
        if let Some(error) = reply.response.error {
            return Err(error.message);
        }
        reply.workspace.ok_or("核心没有返回复核日志".into())
    }
    #[cfg(not(windows))]
    {
        let _ = (query, directory);
        Err("日志读取需要 Windows 核心".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_windows_cannot_use_workbench_mutation_routes() {
        let windows = super::super::experiment_workbench::Windows::default();
        for name in ["main", "experiment-exp-synthetic"] {
            let directory = std::path::PathBuf::from(name);
            windows
                .bind(
                    name,
                    super::super::experiment_workbench::Binding {
                        directory: directory.clone(),
                        _pins: vec![],
                    },
                )
                .unwrap();
            let log = label(name);
            assert_eq!(owner(&log), Some(name));
            assert_eq!(windows.directory(owner(&log).unwrap()).unwrap(), directory);
            assert!(windows.directory(&log).is_err());
        }
        assert!(owner("main").is_none());
        assert!(owner(PREFIX).is_none());
        assert!(windows
            .directory(owner("review-logs-missing").unwrap())
            .is_err());
    }

    #[test]
    fn only_the_four_review_log_queries_are_allowed() {
        for query in [
            WorkspaceQuery::ApiPublicationReviews {
                offset: 0,
                revision: None,
            },
            WorkspaceQuery::ManagementRecordReviews {
                offset: 20,
                revision: Some("v1".into()),
            },
            WorkspaceQuery::ApiPublicationReview {
                relative_path: "api/original".into(),
                revision: "v1".into(),
            },
            WorkspaceQuery::ManagementRecordReview {
                relative_path: "management/original".into(),
                revision: "v1".into(),
            },
        ] {
            assert!(allow_query(&query).is_ok());
        }
        assert!(allow_query(&WorkspaceQuery::Overview).is_err());
        assert!(allow_query(&WorkspaceQuery::Projects {
            offset: 0,
            revision: None
        })
        .is_err());
    }

    #[tokio::test]
    async fn owner_exit_fence_blocks_log_reads_until_explicit_reopen() {
        let lifecycle = super::super::workbench_exit::Lifecycle::default();
        let gate = lifecycle.gate("main").unwrap();
        let read = lifecycle
            .enter(owner("review-logs-main").unwrap())
            .await
            .unwrap();
        let directory = tempfile::tempdir().unwrap();
        {
            let stop = gate.stop(directory.path(), "log-window-test".into());
            tokio::pin!(stop);
            // Poll the exit into its drain while the log's read permit is still held.
            assert!(
                tokio::time::timeout(std::time::Duration::from_millis(20), &mut stop)
                    .await
                    .is_err()
            );
            assert!(lifecycle.enter("main").await.is_err());
        }
        drop(read);
        // The cancelled exit still fences readers. Only an explicit reopen lifts it.
        assert!(lifecycle.enter("main").await.is_err());
        gate.reopen().unwrap();
        assert!(lifecycle.enter("main").await.is_ok());
    }
}
