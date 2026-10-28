//! Ship exact, versioned maintenance tools outside the installation being replaced.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, io::Write, path::Path};

const TOOLS: &[(&str, &str)] = &[
    (
        "manage-windows-update.ps1",
        include_str!("../../../../scripts/manage-windows-update.ps1"),
    ),
    (
        "test-windows-package.ps1",
        include_str!("../../../../scripts/test-windows-package.ps1"),
    ),
    (
        "maintenance-window.ps1",
        include_str!("../../../../scripts/maintenance-window.ps1"),
    ),
];
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn write_new(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}
fn script_bytes(script: &str) -> Vec<u8> {
    // Windows PowerShell 5 requires a BOM for UTF-8 Chinese labels.
    // include_str! retains a source BOM. A second BOM hides the leading param
    // declaration from PowerShell, leaving its required arguments unbound.
    let mut bytes = vec![0xef, 0xbb, 0xbf];
    bytes.extend_from_slice(script.trim_start_matches('\u{feff}').as_bytes());
    bytes
}
#[cfg(windows)]
fn shell_path(path: &Path) -> std::path::PathBuf {
    // PowerShell 5 RemoteSigned can classify verbatim DOS paths as remote.
    let text = path.to_string_lossy();
    match text.strip_prefix("\\\\?\\") {
        Some(rest) => std::path::PathBuf::from(
            rest.strip_prefix("UNC\\")
                .map(|p| format!("\\\\{p}"))
                .unwrap_or_else(|| rest.to_owned()),
        ),
        None => path.to_path_buf(),
    }
}
pub fn information(data: &Path, executable: &Path) -> Value {
    json!({"version":env!("CARGO_PKG_VERSION"),"bundleSchema":1,
        "dataDirectory":data,"installationDirectory":executable.parent(),
        "holdPresent":fs::symlink_metadata(data.join("upgrade-hold.json")).is_ok(),
        "separateWindow":true,"automaticCoreStart":false,
        "tools":TOOLS.iter().map(|(name,body)|json!({"name":name,"sha256":hash(&script_bytes(body))})).collect::<Vec<_>>()})
}
#[cfg(windows)]
pub fn stage(data: &Path, executable: &Path, cache: &Path) -> Result<Value, String> {
    use std::{
        io::Read,
        time::{SystemTime, UNIX_EPOCH},
    };
    let run = || -> Result<Value, Box<dyn std::error::Error>> {
        let install = fs::canonicalize(
            executable
                .parent()
                .ok_or("missing installation directory")?,
        )?;
        let data = if data.exists() {
            fs::canonicalize(data)?
        } else {
            fs::canonicalize(data.parent().ok_or("missing data parent")?)?
                .join(data.file_name().ok_or("missing data name")?)
        };
        if !data.is_absolute() || !cache.is_absolute() {
            return Err("Maintenance paths must be absolute".into());
        }
        let cache = fs::canonicalize(cache.parent().ok_or("missing maintenance parent")?)?
            .join(cache.file_name().ok_or("missing maintenance name")?);
        if cache.starts_with(&install)
            || install.starts_with(&cache)
            || cache.starts_with(&data)
            || data.starts_with(&cache)
        {
            return Err("Maintenance cache overlaps installation or core data".into());
        }
        fs::create_dir_all(&cache)?;
        let _cache_pin = cerulean_core::backup::open(&cache, true, false)?;
        let _install_pin = cerulean_core::backup::open(&install, true, false)?;
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let directory = cache.join(format!("session-{}-{stamp}", std::process::id()));
        fs::create_dir(&directory)?;
        let _directory_pin = cerulean_core::backup::open(&directory, true, false)?;
        let mut artifacts = Vec::new();
        for (name, body) in TOOLS {
            let bytes = script_bytes(body);
            write_new(&directory.join(name), &bytes)?;
            artifacts.push(json!({"path":name,"sha256":hash(&bytes),"bytes":bytes.len()}));
        }
        let mut runtime = cerulean_core::backup::open(executable, false, false)?;
        let mut bytes = Vec::new();
        (&mut runtime)
            .take(256 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 256 * 1024 * 1024 {
            return Err("runtime exceeds maintenance staging limit".into());
        }
        write_new(&directory.join("cerulean-runtime.exe"), &bytes)?;
        artifacts
            .push(json!({"path":"cerulean-runtime.exe","sha256":hash(&bytes),"bytes":bytes.len()}));
        let config = json!({"schema":1,"ownerSid":cerulean_core::ipc::current_sid()?,
            "version":env!("CARGO_PKG_VERSION"),"directory":shell_path(&directory),
            "installationDirectory":shell_path(&install),"dataDirectory":shell_path(&data),"artifacts":artifacts});
        let bytes = serde_json::to_vec_pretty(&config)?;
        write_new(&directory.join("session.json"), &bytes)?;
        Ok(
            json!({"directory":shell_path(&directory),"sessionDigest":hash(&bytes),"version":env!("CARGO_PKG_VERSION"),"started":false}),
        )
    };
    run().map_err(|e| format!("维护副本准备未完成，保留已有文件供核对：{e}"))
}
#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn script_encoding_has_one_bom_and_powershell_binds_parameters() {
        let body = "param([Parameter(Mandatory=$true)][string]$Value)\n[Console]::Write($Value)";
        let expected = script_bytes(body);
        for prefix in ["", "\u{feff}", "\u{feff}\u{feff}"] {
            assert_eq!(script_bytes(&format!("{prefix}{body}")), expected);
        }
        for (_, tool) in TOOLS {
            let bytes = script_bytes(tool);
            assert!(bytes.starts_with(&[0xef, 0xbb, 0xbf]));
            assert!(!bytes[3..].starts_with(&[0xef, 0xbb, 0xbf]));
        }
        #[cfg(windows)]
        {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("parameter-check.ps1");
            fs::write(&path, script_bytes(&format!("\u{feff}{body}"))).unwrap();
            let shell = std::path::PathBuf::from(std::env::var_os("SystemRoot").unwrap())
                .join("System32/WindowsPowerShell/v1.0/powershell.exe");
            let output = std::process::Command::new(shell)
                // Same process-local policy as the shipped maintenance launcher;
                // no user/machine execution policy or trust store is changed.
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-ExecutionPolicy",
                    "RemoteSigned",
                    "-File",
                ])
                .arg(&path)
                .args(["-Value", "bound-259"])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(output.stdout, b"bound-259");
        }
    }
    fn fixture() -> std::path::PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../reports")
            .join(format!(
                "maintenance-stage-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
        fs::create_dir_all(&dir).unwrap();
        fs::canonicalize(dir).unwrap()
    }
    #[test]
    fn maintenance_bundle_stages_exact_embedded_tools_and_pinned_runtime() {
        let root = fixture();
        let install = root.join("install");
        let data = root.join("data");
        fs::create_dir(&install).unwrap();
        fs::create_dir(&data).unwrap();
        let executable = install.join("cerulean-runtime.exe");
        fs::write(&executable, b"controlled non-executed test runtime").unwrap();
        let info = stage(&data, &executable, &root.join("cache")).unwrap();
        let dir = Path::new(info["directory"].as_str().unwrap());
        let raw = fs::read(dir.join("session.json")).unwrap();
        assert_eq!(hash(&raw), info["sessionDigest"].as_str().unwrap());
        let config: Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(config["artifacts"].as_array().unwrap().len(), 4);
        for entry in config["artifacts"].as_array().unwrap() {
            let bytes = fs::read(dir.join(entry["path"].as_str().unwrap())).unwrap();
            assert_eq!(hash(&bytes), entry["sha256"]);
        }
        assert!(fs::read(dir.join("maintenance-window.ps1"))
            .unwrap()
            .starts_with(&[0xef, 0xbb, 0xbf]));
        assert_eq!(fs::read_dir(&data).unwrap().count(), 0);
        write_new(
            &root.join("stage-result.json"),
            &serde_json::to_vec_pretty(&info).unwrap(),
        )
        .unwrap();
    }
    #[test]
    fn maintenance_bundle_rejects_overlapping_cache_before_creation_and_hardlinked_runtime() {
        let root = fixture();
        let install = root.join("install");
        let data = root.join("data");
        fs::create_dir(&install).unwrap();
        fs::create_dir(&data).unwrap();
        let executable = install.join("cerulean-runtime.exe");
        fs::write(&executable, b"test").unwrap();
        let overlap = install.join("cache");
        assert!(stage(&data, &executable, &overlap)
            .unwrap_err()
            .contains("overlap"));
        assert!(!overlap.exists());
        fs::hard_link(&executable, root.join("runtime-link")).unwrap();
        assert!(stage(&data, &executable, &root.join("cache")).is_err());
        assert_eq!(fs::read(&executable).unwrap(), b"test");
        assert_eq!(fs::read_dir(&data).unwrap().count(), 0);
    }
}
#[cfg(windows)]
pub fn launch(session: Value) -> Result<Value, String> {
    launch_mode(session, false)
}
#[cfg(windows)]
pub fn launch_import_retention(session: Value) -> Result<Value, String> {
    launch_mode(session, true)
}
#[cfg(windows)]
fn launch_mode(session: Value, import_retention: bool) -> Result<Value, String> {
    use std::process::{Command, Stdio};
    let directory = std::path::PathBuf::from(
        session["directory"]
            .as_str()
            .ok_or("missing maintenance directory")?,
    );
    let digest = session["sessionDigest"]
        .as_str()
        .ok_or("missing maintenance digest")?;
    let shell = std::env::var_os("SystemRoot")
        .map(std::path::PathBuf::from)
        .ok_or("SystemRoot unavailable")?
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let modules = shell
        .parent()
        .ok_or("PowerShell module directory unavailable")?
        .join("Modules");
    let mut command = Command::new(shell);
    command
        // A GUI launched from PowerShell 7 can inherit incompatible module paths.
        // Use Windows PowerShell's own modules, without changing the user's environment.
        .env("PSModulePath", modules)
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "RemoteSigned",
            "-WindowStyle",
            "Hidden",
            "-File",
        ])
        .arg(directory.join("maintenance-window.ps1"))
        .arg("-Session")
        .arg(directory.join("session.json"))
        .arg("-SessionDigest")
        .arg(digest)
        .current_dir(&directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if import_retention {
        command.arg("-ImportRetention");
    }
    let child = command
        .spawn()
        .map_err(|e| format!("维护窗口未启动；未重新尝试：{e}"))?;
    let mut result = session;
    result["started"] = json!(true);
    result["pid"] = json!(child.id());
    result["importRetention"] = json!(import_retention);
    Ok(result)
}
