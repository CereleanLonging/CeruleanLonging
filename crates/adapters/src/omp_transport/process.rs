use super::ReadOnlyTransport;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path, process::Stdio};
use tokio::{
    io::{AsyncRead, AsyncReadExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    time::{timeout, Duration},
};

/// Whitelisted local state; never serializes provider credentials or raw output.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeState {
    pub session_id: String,
    pub provider: String,
    pub model: String,
    pub protocol_version: u8,
    pub is_streaming: bool,
    pub is_compacting: bool,
    pub has_pending_async_work: bool,
    pub is_settled: bool,
    pub queued_message_count: u64,
    pub message_count: u64,
    pub advertised_tool_count: usize,
}
/// An isolated no-model diagnostic, not a worker and not a credential check.
/// Only negotiation and get_state are exposed. No reconnect or retry is performed.
pub struct ProbeClient {
    child: Child,
    rpc: ReadOnlyTransport<BufReader<ChildStdout>, ChildStdin>,
    diagnostic: Option<tokio::task::JoinHandle<FailureHints>>,
}
#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FailureHints {
    native_addon: bool,
    permission_denied: bool,
    unknown_option: bool,
    no_models: bool,
    invalid_argument: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloseEvidence {
    pub forced_termination: bool,
    pub exit_code: Option<i32>,
    pub failure_hints: FailureHints,
}
async fn drain_diagnostics(mut stderr: impl AsyncRead + Unpin) -> FailureHints {
    let mut hints = FailureHints::default();
    let mut tail = Vec::new();
    let mut chunk = [0u8; 8192];
    while let Ok(n) = stderr.read(&mut chunk).await {
        if n == 0 {
            break;
        }
        tail.extend_from_slice(&chunk[..n]);
        let text = String::from_utf8_lossy(&tail).to_ascii_lowercase();
        hints.native_addon |= text.contains("failed to load pi_natives");
        hints.permission_denied |= ["eperm", "eacces", "access is denied"]
            .iter()
            .any(|v| text.contains(v));
        hints.unknown_option |= ["unknown option", "unknown argument"]
            .iter()
            .any(|v| text.contains(v));
        hints.no_models |= text.contains("no models available");
        hints.invalid_argument |= text.contains("invalid argument");
        if tail.len() > 4096 {
            tail.drain(..tail.len() - 4096);
        }
    }
    hints
}
impl Drop for ProbeClient {
    fn drop(&mut self) {
        if let Some(task) = &self.diagnostic {
            task.abort();
        }
    }
}
impl ProbeClient {
    pub fn launch(
        executable: &Path,
        expected_sha256: &str,
        fresh_root: &Path,
    ) -> Result<Self, String> {
        if !executable.is_absolute()
            || !executable.is_file()
            || !fresh_root.is_absolute()
            || expected_sha256.len() != 64
            || !expected_sha256.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err("OMP probe requires absolute paths and an approved binary digest".into());
        }
        // Avoid loading ancestor project config or .env. Fresh root must have an
        // existing parent; this probe deliberately does not run inside a project.
        let parent = fresh_root.parent().ok_or("OMP probe parent missing")?;
        let parent = fs::canonicalize(parent).map_err(|_| "OMP probe parent unavailable")?;
        for ancestor in parent.ancestors() {
            for name in [
                ".env",
                ".env.local",
                ".env.development",
                ".env.development.local",
                ".omp",
                ".pi",
                ".claude",
                ".codex",
                ".gemini",
                ".git",
            ] {
                if fs::symlink_metadata(ancestor.join(name)).is_ok() {
                    return Err(
                        "OMP probe ancestor contains project configuration; use an isolated parent"
                            .into(),
                    );
                }
            }
        }
        let mut file = fs::File::open(executable).map_err(|_| "OMP executable unreadable")?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let n = file
                .read(&mut buffer)
                .map_err(|_| "OMP executable hash failed")?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
        if format!("{:x}", hasher.finalize()) != expected_sha256.to_ascii_lowercase() {
            return Err("OMP executable digest differs from approval".into());
        }
        // create_dir (not create_dir_all) rejects any preexisting profile.
        fs::create_dir(fresh_root).map_err(|_| "OMP probe root must be newly created")?;
        // Bun's compiled native-module resolver rejects verbatim Windows paths
        // (\\?\D:\...) even when the extracted file exists. Keep canonical
        // resolution, but use the normal DOS spelling where it is equivalent.
        let root = dunce::canonicalize(fresh_root).map_err(|_| "OMP probe root unavailable")?;
        for name in [
            "home",
            "tmp",
            "workspace",
            "agent",
            "sessions",
            "appdata",
            "localappdata",
        ] {
            fs::create_dir(root.join(name)).map_err(|_| "OMP probe directory creation failed")?;
        }
        fs::write(
            root.join("config.yml"),
            "retry:\n  enabled: false\n  maxRetries: 0\n  modelFallback: false\n",
        )
        .map_err(|_| "OMP probe config creation failed")?;
        let mut command = Command::new(executable);
        command
            .args([
                "--mode",
                "rpc",
                "--no-session",
                "--no-tools",
                "--no-extensions",
                "--no-skills",
                "--no-rules",
                "--no-lsp",
                "--no-title",
                "--no-ui",
                "--no-pty",
                "--no-prewalk",
                "--thinking",
                "off",
                "--model",
                "deepseek/deepseek-flash",
            ])
            .arg("--config")
            .arg(root.join("config.yml"))
            .arg("--session-dir")
            .arg(root.join("sessions"))
            .current_dir(root.join("workspace"))
            .env_clear();
        // Inherit only OS loader settings. API keys, proxy credentials, Bun/Node
        // preload variables and user configuration variables are never inherited.
        for key in ["SystemRoot", "WINDIR"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        command
            .env("USERPROFILE", root.join("home"))
            .env("HOME", root.join("home"))
            .env("APPDATA", root.join("appdata"))
            .env("LOCALAPPDATA", root.join("localappdata"))
            .env("TEMP", root.join("tmp"))
            .env("TMP", root.join("tmp"))
            .env("PI_CODING_AGENT_DIR", root.join("agent"))
            .env(
                "DEEPSEEK_API_KEY",
                "cerulean-offline-placeholder-not-a-real-key",
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let mut child = command
            .spawn()
            .map_err(|_| "OMP probe process launch failed")?;
        let reader = child.stdout.take().ok_or("OMP stdout unavailable")?;
        let writer = child.stdin.take().ok_or("OMP stdin unavailable")?;
        let stderr = child.stderr.take().ok_or("OMP stderr unavailable")?;
        Ok(Self {
            child,
            rpc: ReadOnlyTransport::new(BufReader::new(reader), writer),
            diagnostic: Some(tokio::spawn(drain_diagnostics(stderr))),
        })
    }
    pub fn process_id(&self) -> Option<u32> {
        self.child.id()
    }
    pub fn command_attempts(&self) -> &std::collections::BTreeMap<String, u64> {
        self.rpc.command_attempts()
    }
    pub async fn inspect(&mut self) -> Result<ProbeState, String> {
        self.rpc.connect(Duration::from_secs(40)).await?;
        let value = self.rpc.get_state(Duration::from_secs(10)).await?;
        let text = |field: &serde_json::Value| -> Result<String, String> {
            let text = field.as_str().ok_or("OMP state identity missing")?;
            if text.is_empty() || text.len() > 512 || text.chars().any(char::is_control) {
                return Err("OMP state identity invalid".into());
            }
            Ok(text.into())
        };
        let boolean = |key: &str| {
            value[key]
                .as_bool()
                .ok_or_else(|| "OMP state activity evidence missing".to_string())
        };
        let number = |key: &str| {
            value[key]
                .as_u64()
                .ok_or_else(|| "OMP state count evidence missing".to_string())
        };
        let state = ProbeState {
            session_id: text(&value["sessionId"])?,
            provider: text(&value["model"]["provider"])?,
            model: text(&value["model"]["id"])?,
            protocol_version: 2,
            is_streaming: boolean("isStreaming")?,
            is_compacting: boolean("isCompacting")?,
            has_pending_async_work: boolean("hasPendingAsyncWork")?,
            is_settled: boolean("isSettled")?,
            queued_message_count: number("queuedMessageCount")?,
            message_count: number("messageCount")?,
            advertised_tool_count: value["dumpTools"]
                .as_array()
                .ok_or("OMP tool evidence missing")?
                .len(),
        };
        if state.provider != "deepseek"
            || state.model != "deepseek-flash"
            || state.is_streaming
            || state.is_compacting
            || state.has_pending_async_work
            || !state.is_settled
            || state.queued_message_count != 0
            || state.message_count != 0
            || state.advertised_tool_count != 0
        {
            return Err(
                "OMP isolated probe state differs from requested idle no-tool configuration".into(),
            );
        }
        Ok(state)
    }
    /// Closing stdin is preferred; force termination and wait apply only to this
    /// owned process. Drop also kills it if a caller abandons the whole operation.
    pub async fn close(mut self) -> Result<CloseEvidence, String> {
        let _ = self.rpc.close_input().await;
        let (forced_termination, status) =
            match timeout(Duration::from_secs(5), self.child.wait()).await {
                Ok(Ok(status)) => (false, status),
                Ok(Err(_)) => return Err("OMP process wait failed".into()),
                Err(_) => {
                    self.child
                        .start_kill()
                        .map_err(|_| "OMP owned process termination failed")?;
                    let status = timeout(Duration::from_secs(5), self.child.wait())
                        .await
                        .map_err(|_| "OMP process termination not confirmed")?
                        .map_err(|_| "OMP process wait failed")?;
                    (true, status)
                }
            };
        let task = self
            .diagnostic
            .as_mut()
            .ok_or("OMP diagnostic reader missing")?;
        let failure_hints = timeout(Duration::from_secs(2), task)
            .await
            .map_err(|_| "OMP diagnostic stream did not close")?
            .map_err(|_| "OMP diagnostic reader failed")?;
        Ok(CloseEvidence {
            forced_termination,
            exit_code: status.code(),
            failure_hints,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn diagnostic_hints_discard_raw_error_bodies() {
        let hints=drain_diagnostics(std::io::Cursor::new(b"private-key text Failed to load pi_natives EPERM unknown option No models available invalid argument")).await;
        let value = serde_json::to_string(&hints).unwrap();
        assert!(!value.contains("private-key"));
        assert!(
            hints.native_addon
                && hints.permission_denied
                && hints.unknown_option
                && hints.no_models
                && hints.invalid_argument
        );
    }
}
