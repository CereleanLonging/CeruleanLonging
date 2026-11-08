//! Explicit, bounded connection checks used by the desktop access settings.
use cerulean_adapters::codex::CodexClient;
use cerulean_domain::{
    credentials::ProviderEndpoint,
    desktop_sessions::DesktopObservation,
    execution::{identifier, require, WorkerSelection},
    secret::SecretInput,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    sync::Mutex,
};
use tokio::time::{timeout, Duration};

#[derive(Debug, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum LocalConnectionRequest {
    CodexSession {
        session_id: String,
        executable: Option<String>,
    },
    Omp {
        executable: String,
        model: String,
        #[serde(default)]
        reasoning_effort: Option<cerulean_domain::reasoning::ReasoningEffort>,
        runtime_parent: Option<String>,
    },
    Qoder {
        observation: Box<DesktopObservation>,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionResult {
    pub message: String,
    pub checked_at: String,
}

static TEST_LOCK: Mutex<()> = Mutex::new(());
static NEXT_TEST: AtomicU64 = AtomicU64::new(0);

pub fn test_local_connection(
    root: &Path,
    request: LocalConnectionRequest,
) -> Result<ConnectionResult, String> {
    let _lock = TEST_LOCK
        .try_lock()
        .map_err(|_| "本机接入测试正在进行，请等待完成后重试")?;
    let root = crate::artifacts::root_path(root.to_str().ok_or("工作台目录无效")?)?;
    if let LocalConnectionRequest::Qoder { observation } = &request {
        return Ok(ConnectionResult {
            message: crate::host::desktop_sessions::test_connection(&root, observation)?,
            checked_at: chrono::Utc::now().to_rfc3339(),
        });
    }
    let parent = match &request {
        LocalConnectionRequest::Omp {
            runtime_parent: Some(parent),
            ..
        } => crate::omp_process::check_profile_parent(Path::new(parent))?,
        _ => root.clone(),
    };
    let scratch = parent.join(format!(
        "connection-test-{}-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_millis(),
        NEXT_TEST.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&scratch).map_err(|_| "无法创建连通测试临时目录")?;
    let result: Result<String, String> = (|| {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "无法启动连通测试")?;
        runtime.block_on(async {
            match request {
                LocalConnectionRequest::CodexSession {
                    session_id,
                    executable,
                } => {
                    identifier(&session_id)?;
                    require(session_id.len() <= 80, "Codex 会话标识过长")?;
                    let executable =
                        crate::codex_discovery::resolve_session_executable(executable.as_deref())?;
                    crate::codex_discovery::validate_executable(Path::new(&executable))?;
                    let mut client =
                        CodexClient::launch_readonly(Path::new(&executable), &scratch)?;
                    let checked = timeout(Duration::from_secs(60), async {
                        let connection = client.connect().await?;
                        require(connection.account_type.is_some(), "请先在 Codex 中登录")?;
                        client.check_session(&session_id).await?;
                        Ok::<_, String>(connection.server_version)
                    })
                    .await
                    .map_err(|_| "Codex 连通测试超时，请稍后重试".to_string());
                    let closed = client.close().await;
                    let version = checked??;
                    closed?;
                    Ok(format!(
                        "Codex 登录、协议 {version} 和所填会话标识核验通过。当前程序 {executable}"
                    ))
                }
                LocalConnectionRequest::Omp {
                    executable,
                    model,
                    reasoning_effort,
                    ..
                } => {
                    let worker = WorkerSelection {
                        reasoning_effort,
                        executable: crate::verifier::PinnedCheck::describe(
                            Path::new(&executable),
                            &[],
                            Default::default(),
                        )?
                        .executable,
                        provider: "deepseek".into(),
                        model,
                        omp_runtime_parent: None,
                    };
                    worker.validate()?;
                    let placeholder = SecretInput::new(
                        "cerulean-connectivity-placeholder-not-a-real-key".into(),
                    )?;
                    let mut process = crate::omp_process::WorkerProcess::launch(
                        &worker,
                        &scratch.join("profile"),
                        &placeholder,
                    )?;
                    let checked = process.connect().await;
                    let closed = process.close().await;
                    checked?;
                    let closed = closed?;
                    require(
                        closed.remaining_processes == 0 && (closed.forced || closed.exit_code == 0),
                        "OMP 测试进程退出异常",
                    )?;
                    let target = ProviderEndpoint::DeepSeek {};
                    let key = crate::credential_vault::load_for(&root, target.provider())?;
                    let models = cerulean_adapters::provider_models::models(&target, key).await?;
                    require(
                        models.models.contains(&worker.model),
                        "OMP 协议通过，但 DeepSeek 未返回所选模型，请核对模型 ID 和账户权限",
                    )?;
                    Ok("OMP 协议、所选模型及 DeepSeek API 认证检查通过。".into())
                }
                LocalConnectionRequest::Qoder { .. } => unreachable!(),
            }
        })
    })();
    let cleanup = fs::remove_dir_all(&scratch)
        .map_err(|_| "连通测试临时目录清理失败，请检查目录占用".to_string());
    let message = result?;
    cleanup?;
    Ok(ConnectionResult {
        message,
        checked_at: chrono::Utc::now().to_rfc3339(),
    })
}

/// Catalog metadata only. The temporary read-only client never starts a turn.
pub async fn cli_model_catalog(
    root: &Path,
    executable: &str,
) -> Result<Vec<cerulean_adapters::codex::Model>, String> {
    let executable = Path::new(executable);
    crate::codex_discovery::validate_executable(executable)?;
    let root = crate::artifacts::root_path(root.to_str().ok_or("工作台目录无效")?)?;
    let scratch = root.join(format!(
        "cli-catalog-{}-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_millis(),
        NEXT_TEST.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&scratch).map_err(|_| "无法创建 CLI 目录检查环境")?;
    let result: Result<Vec<cerulean_adapters::codex::Model>, String> = async {
        let mut client = CodexClient::launch_readonly(executable, &scratch)?;
        let connection = timeout(Duration::from_secs(60), client.connect()).await;
        let closed = client.close().await;
        let connection = connection.map_err(|_| "CLI 模型目录读取超时")??;
        closed?;
        require(
            connection.account_type.is_some(),
            "请先登录 Codex CLI，或切换 Key 接入配置相应凭证",
        )?;
        Ok(connection.models)
    }
    .await;
    let cleaned: Result<(), String> = (|| {
        let resolved = crate::artifacts::root_path(scratch.to_str().ok_or("CLI 检查目录无效")?)?;
        require(
            resolved.parent() == Some(root.as_path()),
            "CLI 检查目录边界发生变化",
        )?;
        fs::remove_dir_all(&resolved).map_err(|_| "CLI 目录检查环境待清理".into())
    })();
    let models = result?;
    cleaned?;
    Ok(models)
}
