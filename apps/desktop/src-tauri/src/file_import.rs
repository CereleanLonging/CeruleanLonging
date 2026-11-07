#[tauri::command]
pub async fn file_import_window(
    request: serde_json::Value,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, super::State>,
) -> Result<serde_json::Value, String> {
    let _access = state.lifecycle.enter(window.label()).await?;
    state.workbenches.directory(window.label())?;
    #[cfg(windows)]
    {
        let request = serde_json::from_value(request).map_err(|_| "文件导入请求无效")?;
        let owner = window.label().to_owned();
        let imports = state.file_imports.clone();
        tauri::async_runtime::spawn_blocking(move || imports.run(&owner, request))
            .await
            .map_err(|_| "文件导入任务未完成".to_string())?
    }
    #[cfg(not(windows))]
    {
        let _ = request;
        Err("独立文件导入窗口需要 Windows".into())
    }
}
