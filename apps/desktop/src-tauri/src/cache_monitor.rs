use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    Status,
    Scan,
    Clean,
}

fn discover(executable: &Path) -> Option<PathBuf> {
    executable
        .ancestors()
        .chain(Path::new(env!("CARGO_MANIFEST_DIR")).ancestors())
        .find(|path| path.join("Cargo.toml").is_file() && path.join("package.json").is_file())
        .map(Path::to_path_buf)
}

#[tauri::command]
pub async fn build_cache_monitor(
    action: Action,
    project_root: Option<PathBuf>,
    window: tauri::WebviewWindow,
    state: tauri::State<'_, crate::State>,
) -> Result<cerulean_cache::Snapshot, String> {
    let _access = state.lifecycle.enter(window.label()).await?;
    state.workbenches.directory(window.label())?;
    let root = project_root
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| discover(&state.executable))
        .ok_or("请填写工作台源码工程目录后扫描缓存")?;
    tauri::async_runtime::spawn_blocking(move || {
        let project = cerulean_cache::Project::open(&root)?;
        match action {
            Action::Status => project.snapshot(false),
            Action::Scan => project.snapshot(true),
            Action::Clean => project.clean(),
        }
    })
    .await
    .map_err(|e| format!("缓存操作未确认，请重新扫描核对 ({e})"))?
}
