//! Read-only local executable discovery. Never launches candidates or reads credentials.
use serde::Serialize;
use std::{
    collections::BTreeSet,
    fs,
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub executable: String,
    pub directory: String,
    pub source: String,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanResult {
    pub candidates: Vec<Candidate>,
    pub warnings: Vec<String>,
    pub limited: bool,
}

fn unlinked(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return false;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    true
}

/// Retains the existing no-link/no-reparse executable boundary, with actionable errors.
pub fn validate_executable(path: &Path) -> Result<(), String> {
    if !path.is_absolute() || path.as_os_str().len() > 4096 {
        return Err("请输入 Codex 程序的完整绝对路径（以 codex.exe 结尾）".into());
    }
    let metadata = fs::symlink_metadata(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => {
            "找不到该 Codex 程序，请检查路径或扫描本机安装位置".to_string()
        }
        std::io::ErrorKind::PermissionDenied => {
            "无法读取该 Codex 路径，请选择当前 Windows 账户可访问的程序".to_string()
        }
        _ => format!("无法读取 Codex 程序路径：{e}"),
    })?;
    if !unlinked(&metadata) {
        return Err("该路径是链接、应用执行别名或重解析点，请选择实际的 codex.exe 文件".into());
    }
    if metadata.is_dir() {
        return Err(
            "当前填写的是文件夹，请选择其中的 codex.exe 文件，或点击“扫描本地 Codex”".into(),
        );
    }
    if !metadata.is_file()
        || !path
            .file_name()
            .is_some_and(|n| n.eq_ignore_ascii_case("codex.exe"))
    {
        return Err("请选择 Codex 的 codex.exe 程序文件".into());
    }
    // The Electron desktop launcher has the same basename on Windows. Its CLI is in resources.
    if path
        .parent()
        .is_some_and(|p| p.join("resources/app.asar").is_file())
    {
        return Err(
            "这是 Codex 桌面启动程序，请选择 resources 目录内的 codex.exe，或使用本机扫描".into(),
        );
    }
    Ok(())
}

fn local_absolute(path: &Path) -> bool {
    if !path.is_absolute()
        || path.as_os_str().len() > 4096
        || path.components().any(|p| p == Component::ParentDir)
    {
        return false;
    }
    #[cfg(windows)]
    {
        // Exclude UNC/device paths and mapped network drives; this is a local scan only.
        use std::path::Prefix;
        let Some(Component::Prefix(prefix)) = path.components().next() else {
            return false;
        };
        let Prefix::Disk(drive) = prefix.kind() else {
            return false;
        };
        let root = [drive as u16, b':' as u16, b'\\' as u16, 0];
        if unsafe { windows_sys::Win32::Storage::FileSystem::GetDriveTypeW(root.as_ptr()) } != 3 {
            return false;
        }
    }
    true
}

struct Scanner {
    result: ScanResult,
    seen: BTreeSet<String>,
    roots: BTreeSet<String>,
    remaining: usize,
    deadline: Instant,
}
impl Scanner {
    fn new() -> Self {
        Self {
            result: ScanResult::default(),
            seen: BTreeSet::new(),
            roots: BTreeSet::new(),
            remaining: 8192,
            deadline: Instant::now() + Duration::from_secs(4),
        }
    }
    fn step(&mut self) -> bool {
        if self.remaining == 0
            || Instant::now() >= self.deadline
            || self.result.candidates.len() >= 32
        {
            self.result.limited = true;
            return false;
        }
        self.remaining -= 1;
        true
    }
    fn warning(&mut self, text: &str) {
        if self.result.warnings.len() < 8 && !self.result.warnings.iter().any(|s| s == text) {
            self.result.warnings.push(text.into());
        }
    }
    fn add(&mut self, path: PathBuf, source: &str) {
        if !self.step() || !local_absolute(&path) || validate_executable(&path).is_err() {
            return;
        }
        // Canonicalization is only for deduplication. Keep the validated spelling for execution.
        let Ok(identity) = fs::canonicalize(&path) else {
            return;
        };
        if !self.seen.insert(identity.to_string_lossy().to_lowercase()) {
            return;
        }
        self.result.candidates.push(Candidate {
            directory: path.parent().unwrap().to_string_lossy().into_owned(),
            executable: path.to_string_lossy().into_owned(),
            source: source.into(),
        });
    }
    fn children(&mut self, root: &Path) -> Vec<PathBuf> {
        if !self.step() || !local_absolute(root) {
            return vec![];
        }
        // Do not traverse directory links discovered by enumeration.
        if !fs::symlink_metadata(root).is_ok_and(|m| m.is_dir() && unlinked(&m)) {
            return vec![];
        }
        let entries = match fs::read_dir(root) {
            Ok(v) => v,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    self.warning("部分安装目录无读取权限，已跳过；可先打开 Codex 后重新扫描。");
                }
                return vec![];
            }
        };
        let mut paths = vec![];
        for entry in entries {
            if !self.step() {
                break;
            }
            if let Ok(entry) = entry {
                if fs::symlink_metadata(entry.path()).is_ok_and(|m| m.is_dir() && unlinked(&m)) {
                    paths.push(entry.path());
                }
            }
        }
        paths.sort();
        paths.reverse();
        paths
    }
    fn layout(&mut self, root: &Path, source: &str) {
        if !self.step() || !local_absolute(root) {
            return;
        }
        if !self.roots.insert(root.to_string_lossy().to_lowercase()) {
            return;
        }
        // Known layouts only: no recursive disk walk, shell scripts, or credential files.
        for suffix in [
            "resources/codex.exe",
            "app/resources/codex.exe",
            "codex.exe",
            "bin/codex.exe",
        ] {
            self.add(root.join(suffix), source);
        }
        for base in [
            root.to_path_buf(),
            root.join("node_modules/@openai/codex"),
            root.join("node_modules/@openai/codex/node_modules/@openai/codex-win32-x64"),
        ] {
            self.add(
                base.join("vendor/x86_64-pc-windows-msvc/codex/codex.exe"),
                source,
            );
            self.add(
                base.join("vendor/aarch64-pc-windows-msvc/codex/codex.exe"),
                source,
            );
        }
    }
    fn installation(&mut self, root: &Path, source: &str) {
        self.layout(root, source);
        // Desktop updates store the CLI below bin/<build-id>.
        for child in self.children(&root.join("bin")) {
            self.layout(&child, source);
        }
        // A user may provide WindowsApps or an installed package directory.
        for child in self.children(root) {
            let name = child.file_name().unwrap().to_string_lossy().to_lowercase();
            if name.starts_with("openai.codex_") || name.starts_with("app-") {
                self.layout(&child, source);
            }
        }
    }
}

/// Resolve a session transport after a desktop update moved its bundled CLI.
/// An existing explicit path remains authoritative. Missing paths may be
/// replaced only by a unique discovered installation or running CLI.
pub fn resolve_session_executable(preferred: Option<&str>) -> Result<String, String> {
    let preferred = preferred.map(str::trim).filter(|p| !p.is_empty());
    if let Some(path) = preferred {
        let path = Path::new(path);
        match fs::symlink_metadata(path) {
            Ok(_) => {
                validate_executable(path)?;
                return Ok(path.to_string_lossy().into_owned());
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if !path.is_absolute() {
                    return Err("Codex 程序路径需要是完整绝对路径".into());
                }
            }
            Err(_) => return Err("Codex 程序无法读取，请核对权限后重新选择".into()),
        }
    }
    select_session_candidate(scan(preferred)?)
}

fn select_session_candidate(result: ScanResult) -> Result<String, String> {
    let running: Vec<_> = result
        .candidates
        .iter()
        .filter(|candidate| candidate.source == "正在运行的 Codex")
        .collect();
    if !result.limited && running.len() == 1 {
        return Ok(running[0].executable.clone());
    }
    if !result.limited && result.candidates.len() == 1 {
        return Ok(result.candidates[0].executable.clone());
    }
    Err("Codex 升级后需要重新定位程序。请扫描本地 Codex，明确选择安装位置并检查登录，再核对原会话。".into())
}

/// The optional hint is the input field's directory or executable; selection is separate.
pub fn scan(hint: Option<&str>) -> Result<ScanResult, String> {
    let mut scanner = Scanner::new();
    if let Some(hint) = hint.map(str::trim).filter(|s| !s.is_empty()) {
        let path = Path::new(hint);
        if !local_absolute(path) {
            return Err("扫描位置需要是本机磁盘的完整路径，不支持网络路径或相对路径".into());
        }
        if path.is_file() {
            scanner.layout(path.parent().unwrap(), "填写的程序位置");
        } else if path.is_dir() {
            scanner.installation(path, "填写的目录");
        } else {
            scanner.warning("填写的位置不存在或无法读取，已继续扫描常见安装位置。");
        }
    }
    #[cfg(windows)]
    match running_paths() {
        Ok(paths) => {
            for path in paths {
                if let Some(parent) = path.parent() {
                    scanner.layout(parent, "正在运行的 Codex");
                }
            }
        }
        Err(_) => scanner.warning("未能读取正在运行的 Codex 位置，已继续扫描常见安装目录。"),
    }
    if let Some(path) = std::env::var_os("PATH") {
        for root in std::env::split_paths(&path).take(256) {
            scanner.layout(&root, "PATH 环境变量");
        }
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let root = PathBuf::from(local);
        for suffix in [
            "OpenAI/Codex",
            "OpenAI-Codex",
            "Programs/Codex",
            "Programs/OpenAI Codex",
        ] {
            scanner.installation(&root.join(suffix), "当前账户安装目录");
        }
    }
    if let Some(roaming) = std::env::var_os("APPDATA") {
        scanner.layout(&PathBuf::from(roaming).join("npm"), "npm 安装目录");
    }
    #[cfg(windows)]
    for drive in b'A'..=b'Z' {
        let root = PathBuf::from(format!("{}:\\", drive as char));
        if local_absolute(&root) {
            scanner.installation(&root.join("WindowsApps"), "Windows 应用安装目录");
            scanner.installation(
                &root.join("Program Files/WindowsApps"),
                "Windows 应用安装目录",
            );
        }
    }
    if scanner.result.limited {
        scanner.warning(
            "已达到本次扫描范围或时间限制，结果可能不完整；可填写具体安装目录后再次扫描。",
        );
    }
    Ok(scanner.result)
}

#[cfg(windows)]
fn running_paths() -> Result<Vec<PathBuf>, ()> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
                TH32CS_SNAPPROCESS,
            },
            Threading::{
                OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
            },
        },
    };
    let mut paths = vec![];
    // Read process image paths only; never command lines, environment, or memory.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(());
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snapshot, &mut entry);
        for _ in 0..4096 {
            if ok == 0 {
                break;
            }
            let length = entry
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.szExeFile.len());
            if String::from_utf16_lossy(&entry.szExeFile[..length])
                .eq_ignore_ascii_case("codex.exe")
            {
                let process =
                    OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, entry.th32ProcessID);
                if !process.is_null() {
                    let mut buffer = vec![0u16; 32768];
                    let mut len = buffer.len() as u32;
                    if QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut len) != 0 {
                        paths.push(PathBuf::from(std::ffi::OsString::from_wide(
                            &buffer[..len as usize],
                        )));
                    }
                    CloseHandle(process);
                }
            }
            ok = Process32NextW(snapshot, &mut entry);
        }
        CloseHandle(snapshot);
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_upgrade_discovery_requires_unique_complete_result() {
        fn candidate(path: &str, source: &str) -> Candidate {
            Candidate {
                executable: path.into(),
                directory: "D:/fixture".into(),
                source: source.into(),
            }
        }
        assert!(select_session_candidate(ScanResult::default()).is_err());
        let one = ScanResult {
            candidates: vec![candidate("new.exe", "安装目录")],
            ..Default::default()
        };
        assert_eq!(select_session_candidate(one).unwrap(), "new.exe");
        let two = ScanResult {
            candidates: vec![
                candidate("old.exe", "安装目录"),
                candidate("new.exe", "安装目录"),
            ],
            ..Default::default()
        };
        assert!(select_session_candidate(two).is_err());
        let running = ScanResult {
            candidates: vec![
                candidate("old.exe", "安装目录"),
                candidate("new.exe", "正在运行的 Codex"),
            ],
            ..Default::default()
        };
        assert_eq!(select_session_candidate(running).unwrap(), "new.exe");
        let partial = ScanResult {
            candidates: vec![candidate("new.exe", "正在运行的 Codex")],
            limited: true,
            warnings: vec![],
        };
        assert!(select_session_candidate(partial).is_err());
    }

    #[test]
    fn directories_and_missing_files_have_actionable_messages() {
        let temp = tempfile::tempdir().unwrap();
        assert!(validate_executable(temp.path())
            .unwrap_err()
            .contains("文件夹"));
        assert!(validate_executable(&temp.path().join("codex.exe"))
            .unwrap_err()
            .contains("找不到"));
    }
    #[test]
    fn discovers_store_cli_and_excludes_desktop_launcher() {
        let temp = tempfile::tempdir().unwrap();
        let app = temp.path().join("OpenAI.Codex_1_x64/app");
        fs::create_dir_all(app.join("resources")).unwrap();
        fs::write(app.join("codex.exe"), b"desktop fixture").unwrap();
        fs::write(app.join("resources/app.asar"), b"fixture").unwrap();
        fs::write(app.join("resources/codex.exe"), b"never execute fixture").unwrap();
        let mut scanner = Scanner::new();
        scanner.installation(temp.path(), "fixture");
        scanner.installation(&app, "duplicate");
        assert_eq!(scanner.result.candidates.len(), 1);
        assert!(
            scanner.result.candidates[0]
                .executable
                .ends_with("resources\\codex.exe")
                || scanner.result.candidates[0]
                    .executable
                    .ends_with("resources/codex.exe")
        );
        assert!(validate_executable(&app.join("codex.exe"))
            .unwrap_err()
            .contains("桌面启动程序"));
    }
    #[test]
    fn discovers_versioned_cli_without_walking_unrelated_directories() {
        let temp = tempfile::tempdir().unwrap();
        for sub in ["bin/build123", "unrelated/private"] {
            fs::create_dir_all(temp.path().join(sub)).unwrap();
            fs::write(temp.path().join(sub).join("codex.exe"), b"fixture").unwrap();
        }
        let mut scanner = Scanner::new();
        scanner.installation(temp.path(), "fixture");
        assert_eq!(scanner.result.candidates.len(), 1);
        assert!(scanner.result.candidates[0].executable.contains("build123"));
    }
    #[test]
    fn scan_limits_and_invalid_roots_are_explicit() {
        let mut scanner = Scanner::new();
        scanner.remaining = 0;
        scanner.layout(Path::new("C:\\fixture"), "fixture");
        assert!(scanner.result.limited);
        assert!(scan(Some("relative/path")).is_err());
        #[cfg(windows)]
        assert!(scan(Some("\\\\server\\share")).is_err());
    }
    #[cfg(windows)]
    #[test]
    fn rejects_directory_junctions_without_traversing_them() {
        use std::os::windows::process::CommandExt;
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        let link = temp.path().join("bin");
        fs::create_dir(&target).unwrap();
        fs::create_dir(target.join("build")).unwrap();
        fs::write(target.join("build/codex.exe"), b"fixture").unwrap();
        let result = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .creation_flags(0x08000000)
            .output()
            .unwrap();
        assert!(result.status.success(), "junction fixture creation failed");
        let mut scanner = Scanner::new();
        assert!(!local_absolute(Path::new("\\\\?\\C:\\fixture")));
        scanner.installation(temp.path(), "fixture");
        assert!(scanner.result.candidates.is_empty());
        assert!(validate_executable(&link).unwrap_err().contains("重解析点"));
        // Remove only this fixture link, never recurse into its target.
        fs::remove_dir(&link).unwrap();
        assert_eq!(
            fs::read(target.join("build/codex.exe")).unwrap(),
            b"fixture"
        );
    }
    #[test]
    #[ignore = "explicit read-only inspection of the current machine; no executable launch"]
    fn inspect_local_installations() {
        println!("{}", serde_json::to_string(&scan(None).unwrap()).unwrap());
    }
}
