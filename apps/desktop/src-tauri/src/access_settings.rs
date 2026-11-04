//! Durable UI configuration, independent of disposable WebView profiles and core data.
use serde::{Deserialize, Serialize};
#[cfg(windows)]
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use tauri::Manager;

const LIMIT: usize = 2 * 1024 * 1024;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    schema: u32,
    entries: BTreeMap<String, String>,
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn normalized(path: &Path) -> String {
    path.to_string_lossy()
        .trim_start_matches(r"\\?\")
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}
fn validate(directory: &Path, key: &str, value: &str) -> Result<(), String> {
    if key.len() > 16384 || value.len() > 1024 * 1024 {
        return Err("接入配置过大".into());
    }
    let parts: Vec<serde_json::Value> =
        serde_json::from_str(key).map_err(|_| "无效接入配置标识")?;
    if parts.len() != 4
        || parts[1] != 1
        || parts[2]
            .as_str()
            .is_none_or(|p| normalized(Path::new(p)) != normalized(directory))
        || parts[3].as_str().is_none_or(str::is_empty)
    {
        return Err("接入配置不属于当前工作台".into());
    }
    let item: serde_json::Value = serde_json::from_str(value).map_err(|_| "无效接入配置内容")?;
    match parts[0].as_str() {
        Some("cerulean-access-preference") if item.is_string() => {
            let name = parts[3].as_str().unwrap_or_default();
            let known = matches!(
                name,
                "planning-project"
                    | "worker-cli"
                    | "chief-method"
                    | "codex-executable"
                    | "omp-test-executable"
                    | "omp-test-model"
                    | "omp-test-runtime"
            ) || ["chief", "deputy", "worker"].iter().any(|role| {
                name == format!("model-default:{role}")
                    || ["api", "key"]
                        .iter()
                        .any(|view| name == format!("provider:{role}:{view}"))
            }) || ["deepseek", "anthropic", "qwen", "kimi", "a6api", "grok"]
                .iter()
                .any(|p| name == format!("endpoint:{p}") || name == format!("workspace:{p}"));
            if !known {
                return Err("未知接入选项".into());
            }
        }
        Some("cerulean-access-draft")
            if item["schema"] == 1
                && item["projectId"] == parts[3]
                && item["selection"].is_object() => {}
        _ => return Err("不支持的接入配置格式".into()),
    }
    fn credential_field(v: &serde_json::Value) -> bool {
        match v {
            serde_json::Value::Object(o) => o.iter().any(|(k, v)| {
                matches!(
                    k.to_ascii_lowercase().as_str(),
                    "key"
                        | "apikey"
                        | "api_key"
                        | "token"
                        | "password"
                        | "secret"
                        | "authorization"
                ) || credential_field(v)
            }),
            serde_json::Value::Array(a) => a.iter().any(credential_field),
            _ => false,
        }
    }
    if credential_field(&item) {
        return Err("凭证必须使用加密保存入口".into());
    }
    Ok(())
}

#[cfg(windows)]
fn prepare(root: &Path, directory: &Path) -> Result<(PathBuf, Vec<File>), String> {
    if !root.is_absolute()
        || root.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        return Err("接入配置目录必须是绝对路径".into());
    }
    let path = root.join(format!(
        "{:x}",
        Sha256::digest(normalized(directory).as_bytes())
    ));
    let mut pins = vec![];
    for parent in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        match fs::symlink_metadata(parent) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if let Err(e) = fs::create_dir(parent) {
                    if e.kind() != std::io::ErrorKind::AlreadyExists {
                        return Err(err(e));
                    }
                }
            }
            Err(e) => return Err(err(e)),
        }
        pins.push(cerulean_core::backup::open(parent, true, false).map_err(err)?);
    }
    Ok((path, pins))
}
#[cfg(not(windows))]
fn prepare(_root: &Path, _directory: &Path) -> Result<(PathBuf, Vec<File>), String> {
    Err("接入配置持久保存仅支持 Windows 桌面".into())
}

fn read(path: &Path, directory: &Path) -> Result<Settings, String> {
    let file = path.join("settings.json");
    match fs::symlink_metadata(&file) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Settings {
                schema: 1,
                ..Settings::default()
            })
        }
        Err(e) => return Err(err(e)),
        Ok(_) => {}
    }
    #[cfg(windows)]
    let mut input = cerulean_core::backup::open(&file, false, false).map_err(err)?;
    #[cfg(not(windows))]
    let mut input = File::open(&file).map_err(err)?;
    let mut bytes = vec![];
    (&mut input)
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(err)?;
    if bytes.len() > LIMIT {
        return Err("接入配置文件过大，原文件已保留".into());
    }
    let data: Settings =
        serde_json::from_slice(&bytes).map_err(|_| "接入配置文件无法读取，原文件已保留")?;
    if data.schema != 1 || data.entries.len() > 1024 {
        return Err("不支持的接入配置文件".into());
    }
    for (key, value) in &data.entries {
        validate(directory, key, value)?;
    }
    Ok(data)
}
fn load(root: &Path, directory: &Path) -> Result<BTreeMap<String, String>, String> {
    let (path, _pins) = prepare(root, directory)?;
    Ok(read(&path, directory)?.entries)
}
fn put(root: &Path, directory: &Path, key: String, value: String) -> Result<(), String> {
    validate(directory, &key, &value)?;
    let (path, _pins) = prepare(root, directory)?;
    // A separate lock serializes read-modify-write across GUI processes.
    let lock_path = path.join(".write-lock");
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
    {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(err(e)),
    };
    #[cfg(windows)]
    let _lock = cerulean_core::backup::open(&lock_path, false, true).map_err(err)?;
    #[cfg(not(windows))]
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(lock_path)
        .map_err(err)?;
    #[cfg(not(windows))]
    lock.try_lock()
        .map_err(|_| "接入配置正在保存，请稍后重试")?;
    let mut data = read(&path, directory)?;
    data.entries.insert(key, value);
    let bytes = serde_json::to_vec(&data).map_err(err)?;
    if bytes.len() > LIMIT || data.entries.len() > 1024 {
        return Err("接入配置超过保存上限".into());
    }
    let pending = path.join(format!(
        ".pending-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)
        .map_err(err)?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(err)?;
    drop(file);
    fs::rename(&pending, path.join("settings.json")).map_err(err)
}
fn root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    #[cfg(debug_assertions)]
    if let Some(root) = std::env::var_os("CERULEAN_TEST_ACCESS_ROOT") {
        return Ok(PathBuf::from(root));
    }
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(err)?
        .join("access-settings-v1"))
}
#[tauri::command]
pub async fn access_settings_load(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, super::State>,
) -> Result<BTreeMap<String, String>, String> {
    let directory = state.workbenches.directory(window.label())?;
    let _permit = state.lifecycle.enter(window.label()).await?;
    let root = root(&app)?;
    tauri::async_runtime::spawn_blocking(move || load(&root, &directory))
        .await
        .map_err(err)?
}
#[tauri::command]
pub async fn access_setting_put(
    key: String,
    value: String,
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, super::State>,
) -> Result<(), String> {
    let directory = state.workbenches.directory(window.label())?;
    let _permit = state.lifecycle.enter(window.label()).await?;
    let root = root(&app)?;
    tauri::async_runtime::spawn_blocking(move || put(&root, &directory, key, value))
        .await
        .map_err(err)?
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    fn key(dir: &Path, name: &str) -> String {
        serde_json::to_string(&("cerulean-access-preference", 1, dir.to_string_lossy(), name))
            .unwrap()
    }
    #[test]
    fn reload_and_merge_preserve_separate_settings() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("workbench");
        let root = temp.path().join("settings");
        put(&root, &dir, key(&dir, "chief-method"), "\"desktop\"".into()).unwrap();
        put(
            &root,
            &dir,
            key(&dir, "codex-executable"),
            "\"D:/fixture/codex.exe\"".into(),
        )
        .unwrap();
        let restored = load(&root, &dir).unwrap();
        assert_eq!(restored.len(), 2);
        assert_eq!(restored[&key(&dir, "chief-method")], "\"desktop\"");
        assert!(load(&root, &temp.path().join("other")).unwrap().is_empty());
    }
    #[test]
    fn namespaces_scope_and_credentials_are_rejected() {
        let dir = Path::new("D:/fixture/workspace");
        assert!(validate(
            dir,
            &key(Path::new("D:/other"), "chief-method"),
            "\"desktop\""
        )
        .is_err());
        assert!(validate(dir, &key(dir, "password"), "\"secret\"").is_err());
        for role in ["chief", "deputy", "worker"] {
            let model = serde_json::to_string(
                &serde_json::json!({
                    "target": {"kind": "openAi"}, "model": "fixture-model"
                })
                .to_string(),
            )
            .unwrap();
            assert!(validate(dir, &key(dir, &format!("model-default:{role}")), &model).is_ok());
        }
        assert!(validate(dir, &key(dir, "model-default:unknown"), "\"fixture-model\"").is_err());
        let draft = serde_json::json!(["cerulean-access-draft", 1, dir, "project"]).to_string();
        assert!(validate(
            dir,
            &draft,
            &serde_json::json!({"schema":1,"projectId":"project","selection":{"apiKey":"secret"}})
                .to_string()
        )
        .is_err());
    }
    #[test]
    fn corrupt_and_oversized_files_are_not_overwritten() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("workbench");
        let root = temp.path().join("settings");
        let (path, _pins) = prepare(&root, &dir).unwrap();
        let file = path.join("settings.json");
        fs::write(&file, b"broken original").unwrap();
        assert!(put(&root, &dir, key(&dir, "chief-method"), "\"api\"".into()).is_err());
        assert_eq!(fs::read(&file).unwrap(), b"broken original");
        fs::write(&file, vec![b'x'; LIMIT + 1]).unwrap();
        assert!(load(&root, &dir).is_err());
    }
    #[test]
    fn linked_destination_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("workbench");
        let root = temp.path().join("settings");
        let (path, _pins) = prepare(&root, &dir).unwrap();
        let outside = temp.path().join("unrelated");
        fs::write(&outside, b"original").unwrap();
        fs::hard_link(&outside, path.join("settings.json")).unwrap();
        assert!(put(&root, &dir, key(&dir, "chief-method"), "\"api\"".into()).is_err());
        assert_eq!(fs::read(outside).unwrap(), b"original");
    }
}
