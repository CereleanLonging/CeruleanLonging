//! Bridge the 1024-character logon limit without interpreting approved arguments
//! as shell code. Both files remain write/delete-locked until the owned Job exits.
use super::{launch_path, quote, wide};
use std::{
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::windows::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

const SCRIPT: &str = r#"$ErrorActionPreference='Stop'
try {
  $record=ConvertFrom-Json ([IO.File]::ReadAllText((Join-Path $PSScriptRoot 'approved-command.json'),[Text.Encoding]::UTF8))
  $start=[Diagnostics.ProcessStartInfo]::new()
  $start.FileName=$record.executable
  $start.Arguments=$record.arguments
  $start.WorkingDirectory=$record.directory
  $start.UseShellExecute=$false
  $start.CreateNoWindow=$true
  $start.RedirectStandardInput=$true
  $start.RedirectStandardOutput=$true
  $start.RedirectStandardError=$true
  $process=[Diagnostics.Process]::Start($start)
  $process.StandardInput.Close()
  $stdout=$process.StandardOutput.BaseStream.CopyToAsync([Console]::OpenStandardOutput())
  $stderr=$process.StandardError.BaseStream.CopyToAsync([Console]::OpenStandardError())
  $process.WaitForExit()
  $null=$stdout.GetAwaiter().GetResult()
  $null=$stderr.GetAwaiter().GetResult()
  $code=$process.ExitCode
  $process.Dispose()
  exit $code
} catch {
  [Console]::Error.WriteLine('Approved isolated command could not start: '+$_.Exception.Message)
  exit 2
}
"#;

pub(super) struct Launch {
    pub executable: PathBuf,
    pub args: Vec<String>,
    files: Vec<(PathBuf, Option<File>)>,
}

impl Launch {
    fn write_locked(&mut self, path: PathBuf, bytes: &[u8]) -> Result<(), String> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .share_mode(1)
            .open(&path)
            .map_err(|e| format!("隔离启动材料无法创建：{e}"))?;
        let result = file.write_all(bytes).and_then(|_| file.sync_all());
        self.files.push((path.clone(), Some(file)));
        result.map_err(|e| e.to_string())?;
        // Readers such as ReadAllText themselves allow only shared reads.
        // Reopen read-only, then compare every byte while holding the final
        // write/delete lock. Any change during the reopen gap aborts launch.
        let slot = &mut self.files.last_mut().expect("owned launch file").1;
        drop(slot.take());
        let mut file = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .map_err(|e| e.to_string())?;
        let mut observed = Vec::new();
        Read::by_ref(&mut file)
            .take(bytes.len() as u64 + 1)
            .read_to_end(&mut observed)
            .map_err(|e| e.to_string())?;
        *slot = Some(file);
        if observed != bytes {
            return Err("隔离启动材料已变化，未启动检查".into());
        }
        Ok(())
    }
}

impl Drop for Launch {
    fn drop(&mut self) {
        for (path, file) in self.files.drain(..) {
            drop(file);
            let _ = fs::remove_file(path);
        }
    }
}

pub(super) fn prepare(
    exe: &Path,
    args: &[String],
    cwd: &Path,
    temp: &Path,
) -> Result<Option<Launch>, String> {
    let executable = launch_path(exe)?;
    let name = executable.to_str().ok_or("隔离程序路径编码无效")?;
    let text = std::iter::once(name)
        .chain(args.iter().map(String::as_str))
        .map(quote)
        .collect::<Vec<_>>()
        .join(" ");
    let length = wide(OsStr::new(&text)).len();
    if length > 32767 || args.iter().any(|arg| arg.contains('\0')) {
        return Err("invalid native command line".into());
    }
    if length <= 1024 {
        return Ok(None);
    }
    let temp = launch_path(temp)?;
    let script = temp.join("approved-command.ps1");
    let system = std::env::var_os("SystemRoot").ok_or("SystemRoot missing")?;
    let mut launch = Launch {
        executable: PathBuf::from(system).join("System32/WindowsPowerShell/v1.0/powershell.exe"),
        args: vec![
            "-NoLogo".into(),
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-ExecutionPolicy".into(),
            "Bypass".into(),
            "-File".into(),
            script.to_str().ok_or("隔离启动路径编码无效")?.into(),
        ],
        files: vec![],
    };
    let record = serde_json::to_vec(&serde_json::json!({
        "executable": executable,
        "arguments": args.iter().map(|arg| quote(arg)).collect::<Vec<_>>().join(" "),
        "directory": launch_path(cwd)?,
    }))
    .map_err(|e| e.to_string())?;
    launch.write_locked(temp.join("approved-command.json"), &record)?;
    launch.write_locked(script, SCRIPT.as_bytes())?;
    Ok(Some(launch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_and_invalid_commands_do_not_create_launch_files() {
        let root = tempfile::tempdir().unwrap();
        let exe = std::env::current_exe().unwrap();
        assert!(prepare(&exe, &["short".into()], root.path(), root.path())
            .unwrap()
            .is_none());
        for args in [vec!["bad\0argument".into()], vec!["x".repeat(32768)]] {
            assert!(prepare(&exe, &args, root.path(), root.path()).is_err());
        }
        assert_eq!(root.path().read_dir().unwrap().count(), 0);
    }

    #[test]
    fn existing_launch_material_is_preserved() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("approved-command.json");
        fs::write(&path, b"original").unwrap();
        assert!(prepare(
            &std::env::current_exe().unwrap(),
            &["x".repeat(2000)],
            root.path(),
            root.path()
        )
        .is_err());
        assert_eq!(fs::read(path).unwrap(), b"original");
        assert!(!root.path().join("approved-command.ps1").exists());
    }

    #[test]
    #[ignore = "Explicit Node path required; launches one offline argument echo process"]
    fn long_unicode_arguments_remain_literal_and_material_stays_locked() {
        use std::os::windows::process::CommandExt;
        let root = tempfile::tempdir().unwrap();
        let exe = PathBuf::from(std::env::var_os("CERULEAN_ISOLATED_COMMAND_TEST_NODE").unwrap());
        let arguments = vec![
            "中文 arguments ".repeat(180),
            "\"quoted\" \\backslash\\ ".into(),
            "$(throw 'must remain literal'); `escape`; & | > <".into(),
            "line one\nline two".into(),
            String::new(),
        ];
        let mut args = vec![
            "-e".into(),
            "process.stdout.write(JSON.stringify(process.argv.slice(1)))".into(),
        ];
        args.extend(arguments.clone());
        let launch = prepare(&exe, &args, root.path(), root.path())
            .unwrap()
            .unwrap();
        for name in ["approved-command.json", "approved-command.ps1"] {
            let path = root.path().join(name);
            assert!(fs::write(&path, b"changed").is_err());
            assert!(fs::remove_file(&path).is_err());
        }
        let result = std::process::Command::new(&launch.executable)
            .args(&launch.args)
            .current_dir(root.path())
            .env_clear()
            .env("SystemRoot", std::env::var_os("SystemRoot").unwrap())
            .env("TEMP", root.path())
            .env("TMP", root.path())
            .creation_flags(0x08000000)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            serde_json::from_slice::<Vec<String>>(&result.stdout).unwrap(),
            arguments
        );
        drop(launch);
        assert_eq!(root.path().read_dir().unwrap().count(), 0);
    }
}
