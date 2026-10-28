//! A pinned Windows OMP worker owner. No ambient credentials, shell, project
//! configuration, extensions or native tools are inherited. Tool proxy follows.
use crate::{artifacts::root_path, verifier::process::interactive::InteractiveChild};
use cerulean_adapters::{omp_protocol::SessionIdentity, omp_transport::worker::WorkerTransport};
use cerulean_domain::{
    execution::{require, WorkerSelection},
    secret::SecretInput,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::Read,
    os::windows::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};
use tokio::{
    fs::File as AsyncFile,
    io::{AsyncReadExt, BufReader},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

pub type OwnedTransport = WorkerTransport<BufReader<AsyncFile>, AsyncFile>;
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerExit {
    pub pid: u32,
    pub exit_code: u32,
    pub forced: bool,
    pub remaining_processes: u32,
    pub stderr_bytes: u64,
}
pub struct WorkerProcess {
    child: InteractiveChild,
    _executable: File,
    rpc: OwnedTransport,
    expected: WorkerSelection,
    diagnostics: tokio::task::JoinHandle<u64>,
    connected: bool,
    profile: PathBuf,
}
impl Drop for WorkerProcess {
    fn drop(&mut self) {
        let _ = self.child.terminate();
        self.diagnostics.abort();
    }
}
fn ordinary(path: &Path, directory: bool) -> Result<(), String> {
    use std::os::windows::fs::MetadataExt;
    let m = fs::symlink_metadata(path).map_err(|_| "OMP path inaccessible")?;
    require(
        !m.file_type().is_symlink()
            && m.file_attributes() & 0x400 == 0
            && if directory { m.is_dir() } else { m.is_file() },
        "OMP linked or incorrect path type",
    )
}
fn lock_executable(worker: &WorkerSelection) -> Result<(PathBuf, File), String> {
    worker.validate()?;
    let path = Path::new(&worker.executable.path);
    ordinary(path, false)?;
    root_path(
        path.parent()
            .ok_or("OMP executable parent missing")?
            .to_str()
            .ok_or("OMP executable parent invalid")?,
    )?;
    let path = fs::canonicalize(path).map_err(|_| "OMP executable path unavailable")?;
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .open(&path)
        .map_err(|_| "OMP approved executable cannot be locked")?;
    let mut hash = Sha256::new();
    let mut buf = [0; 65536];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|_| "OMP executable hash failed")?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    require(
        format!("{:x}", hash.finalize()) == worker.executable.sha256,
        "OMP executable changed since approval",
    )?;
    // Return normal DOS spelling for Bun's compiled native module loader.
    Ok((normal_path(&path)?, file))
}
fn normal_path(path: &Path) -> Result<PathBuf, String> {
    let canonical = fs::canonicalize(path).map_err(|_| "OMP canonical path unavailable")?;
    let text = canonical.to_str().ok_or("OMP path must be Unicode")?;
    let normal = if let Some(tail) = text.strip_prefix("\\\\?\\UNC\\") {
        PathBuf::from(format!("\\\\{tail}"))
    } else if let Some(tail) = text.strip_prefix("\\\\?\\") {
        PathBuf::from(tail)
    } else {
        canonical.clone()
    };
    require(
        fs::canonicalize(&normal).map_err(|_| "OMP normal path unavailable")? == canonical,
        "OMP path representation mismatch",
    )?;
    Ok(normal)
}
pub(crate) fn check_profile_parent(parent: &Path) -> Result<PathBuf, String> {
    let parent = root_path(parent.to_str().ok_or("OMP profile parent invalid")?)?;
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
            // OMP discovers standalone context independently of --no-rules.
            "AGENTS.md",
            "CLAUDE.md",
        ] {
            match fs::symlink_metadata(ancestor.join(name)) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                _ => {
                    return Err(format!(
                        "OMP profile ancestor contains {name} or it is inaccessible; use a configuration-free runtime parent"
                    ))
                }
            }
        }
    }
    Ok(parent)
}
fn profile(root: &Path) -> Result<PathBuf, String> {
    require(root.is_absolute(), "OMP profile must be absolute")?;
    check_profile_parent(root.parent().ok_or("OMP profile parent missing")?)?;
    fs::create_dir(root).map_err(|_| "OMP profile must be newly created")?;
    let root = normal_path(root)?;
    for name in [
        "home",
        "tmp",
        "workspace",
        "agent",
        "sessions",
        "appdata",
        "localappdata",
    ] {
        fs::create_dir(root.join(name)).map_err(|_| "OMP isolated directory creation failed")?;
    }
    fs::write(
        root.join("config.yml"),
        // Defense in depth: rules and standalone project context have separate
        // discovery paths in the pinned OMP release. Keep the ancestor guard;
        // this setting is not a filesystem sandbox or a discovery allowlist.
        "disabledProviders:\n  - agents-md\n  - claude-md\nretry:\n  enabled: false\n  maxRetries: 0\n  modelFallback: false\n",
    )
    .map_err(|_| "OMP isolated configuration write failed")?;
    Ok(root)
}

fn environment(root: &Path, key: &SecretInput) -> Result<Zeroizing<Vec<u16>>, String> {
    key.validate()?;
    let mut values = BTreeMap::new();
    for name in ["SystemRoot", "WINDIR"] {
        values.insert(
            name.to_ascii_uppercase(),
            std::env::var(name).map_err(|_| "Windows loader environment missing")?,
        );
    }
    for (name, dir) in [
        ("HOME", "home"),
        ("USERPROFILE", "home"),
        ("TEMP", "tmp"),
        ("TMP", "tmp"),
        ("APPDATA", "appdata"),
        ("LOCALAPPDATA", "localappdata"),
        ("PI_CODING_AGENT_DIR", "agent"),
    ] {
        values.insert(
            name.into(),
            root.join(dir)
                .to_str()
                .ok_or("OMP profile path invalid")?
                .into(),
        );
    }
    // Build and wipe one native environment block. Secret is not copied into
    // command arguments, config, persisted metadata, error messages or Debug.
    let mut block = Zeroizing::new(Vec::new());
    let mut entries = values
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect::<Vec<_>>();
    entries.push(("DEEPSEEK_API_KEY", key.expose()));
    entries.sort_by_key(|(k, _)| *k);
    for (k, v) in entries {
        require(!v.contains('\0'), "OMP environment contains NUL")?;
        block.extend(k.encode_utf16());
        block.push(b'=' as u16);
        block.extend(v.encode_utf16());
        block.push(0);
    }
    block.push(0);
    Ok(block)
}
impl WorkerProcess {
    /// Caller must validate approval before launch. The only initial provider is
    /// explicit DeepSeek; other providers need their own credential mapping.
    pub fn launch(
        worker: &WorkerSelection,
        fresh_profile: &Path,
        key: &SecretInput,
    ) -> Result<Self, String> {
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| "OMP worker launch requires an active runtime")?;
        require(
            worker.provider == "deepseek"
                && worker
                    .model
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c)),
            "OMP worker provider/model not supported",
        )?;
        key.validate()?;
        let (exe, pin) = lock_executable(worker)?;
        let profile = profile(fresh_profile)?;
        let mut args = [
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
        ]
        .map(String::from)
        .to_vec();
        args.push(format!("{}/{}", worker.provider, worker.model));
        args.extend([
            "--config".into(),
            profile.join("config.yml").to_string_lossy().into(),
            "--session-dir".into(),
            profile.join("sessions").to_string_lossy().into(),
        ]);
        let env = environment(&profile, key)?;
        let spawned = InteractiveChild::spawn(&exe, &args, &profile.join("workspace"), &env)
            .map_err(|_| "OMP owned worker launch failed")?;
        let mut stderr = AsyncFile::from_std(spawned.stderr);
        let diagnostics = runtime.spawn(async move {
            let mut count = 0u64;
            let mut buffer = Zeroizing::new([0u8; 8192]);
            while let Ok(n) = stderr.read(&mut buffer[..]).await {
                if n == 0 {
                    break;
                }
                count = count.saturating_add(n as u64);
            }
            count
        });
        Ok(Self {
            child: spawned.child,
            _executable: pin,
            rpc: WorkerTransport::new(
                BufReader::new(AsyncFile::from_std(spawned.stdout)),
                AsyncFile::from_std(spawned.stdin),
            ),
            expected: worker.clone(),
            diagnostics,
            connected: false,
            profile,
        })
    }
    pub fn pid(&self) -> u32 {
        self.child.pid
    }
    pub fn profile(&self) -> &Path {
        &self.profile
    }
    pub fn command_attempts(&self) -> &BTreeMap<String, u64> {
        self.rpc.command_attempts()
    }
    pub async fn connect(&mut self) -> Result<SessionIdentity, String> {
        require(!self.connected, "OMP owned worker already connected")?;
        let v = self.rpc.connect(Duration::from_secs(40)).await?;
        let session = v["sessionId"]
            .as_str()
            .ok_or("OMP initial session missing")?;
        require(
            !session.is_empty() && session.len() <= 512 && !session.chars().any(char::is_control),
            "OMP initial session invalid",
        )?;
        require(
            v["model"]["provider"] == self.expected.provider
                && v["model"]["id"] == self.expected.model
                && v["isStreaming"] == false
                && v["isCompacting"] == false
                && v["hasPendingAsyncWork"] == false
                && v["isSettled"] == true
                && v["queuedMessageCount"] == 0
                && v["messageCount"] == 0
                && v["dumpTools"].as_array().is_some_and(Vec::is_empty),
            "OMP worker initial state differs from approved clean configuration",
        )?;
        self.connected = true;
        Ok(SessionIdentity {
            session_id: session.into(),
            provider: self.expected.provider.clone(),
            model: self.expected.model.clone(),
        })
    }
    pub fn channel(&mut self) -> Result<&mut OwnedTransport, String> {
        require(self.connected, "OMP worker handshake required")?;
        Ok(&mut self.rpc)
    }
    pub async fn close(mut self) -> Result<WorkerExit, String> {
        let _ = self.rpc.close_input().await;
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut forced = false;
        while self.child.active()? > 0 {
            if Instant::now() >= deadline {
                forced = true;
                self.child.terminate()?;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let cleanup = Instant::now() + Duration::from_secs(5);
        while self.child.active()? > 0 {
            require(
                Instant::now() < cleanup,
                "OMP worker tree exit not confirmed",
            )?;
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let code = self
            .child
            .exit_code()?
            .ok_or("OMP parent exit not confirmed")?;
        let count = tokio::time::timeout(Duration::from_secs(2), &mut self.diagnostics)
            .await
            .map_err(|_| "OMP diagnostic stream remained open")?
            .map_err(|_| "OMP diagnostic reader failed")?;
        Ok(WorkerExit {
            pid: self.pid(),
            exit_code: code,
            forced,
            remaining_processes: self.child.active()?,
            stderr_bytes: count,
        })
    }
}

#[cfg(test)]
mod profile_tests {
    use super::*;

    #[test]
    fn standalone_ancestor_context_is_rejected_without_reading_or_creating_profile() {
        for name in ["AGENTS.md", "CLAUDE.md"] {
            let temp = tempfile::tempdir().unwrap();
            let parent = temp.path().join("nested");
            fs::create_dir(&parent).unwrap();
            let marker = temp.path().join(name);
            // Invalid UTF-8 is intentional: the preflight only needs metadata.
            let contents = b"\xffdo-not-include-file-contents-in-errors";
            fs::write(&marker, contents).unwrap();
            let fresh = parent.join("fresh");
            let error = profile(&fresh).unwrap_err();
            assert!(error.contains(name), "{error}");
            assert!(!error.contains("do-not-include"));
            assert!(!fresh.exists());
            assert_eq!(fs::read(marker).unwrap(), contents);
        }
    }

    #[test]
    fn context_named_directory_and_existing_config_still_block_profile() {
        for name in ["AGENTS.md", "CLAUDE.md", ".codex", ".omp", ".git", ".env"] {
            let temp = tempfile::tempdir().unwrap();
            fs::create_dir(temp.path().join(name)).unwrap();
            let fresh = temp.path().join("fresh");
            let error = profile(&fresh).unwrap_err();
            assert!(error.contains(name), "{error}");
            assert!(!fresh.exists());
        }
    }
}
