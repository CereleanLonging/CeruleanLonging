//! Disposable compiler caches for a verified workbench source checkout.
use chrono::{FixedOffset, Utc};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);
type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    pub id: String,
    pub at: String,
    pub kind: String,
    pub completed: bool,
    pub bytes_before: u64,
    pub bytes_removed: u64,
    pub files_removed: u64,
    pub skipped_active: u64,
    pub errors: Vec<String>,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub path: PathBuf,
    pub kind: String,
    pub bytes: u64,
    pub files: u64,
    pub active: bool,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub project_root: PathBuf,
    pub temporary_root: PathBuf,
    pub log_directory: PathBuf,
    pub entries: Vec<Entry>,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub cumulative_cleaned_bytes: u64,
    pub scan_count: u64,
    pub pending_operations: usize,
    pub recent: Vec<Operation>,
    pub warnings: Vec<String>,
}

pub struct Project {
    root: PathBuf,
    _pins: Vec<File>,
}
pub struct Session {
    pub directory: PathBuf,
    _lease: File,
}
struct Candidate {
    path: PathBuf,
    kind: &'static str,
    lease: Option<PathBuf>,
    cargo_profiles: Vec<PathBuf>,
}

struct CandidateGuard {
    _handles: Vec<File>,
    protected: Vec<PathBuf>,
    active: bool,
}

impl Candidate {
    fn acquire(&self) -> Result<CandidateGuard> {
        let mut guard = CandidateGuard {
            _handles: vec![],
            protected: vec![],
            active: false,
        };
        let mut paths = vec![];
        if let Some(lease) = &self.lease {
            paths.push(lease.clone());
        }
        for profile in &self.cargo_profiles {
            guard._handles.extend(pin_chain(profile, true)?);
            let path = profile.join(".cargo-lock");
            guard.protected.push(path.clone());
            paths.push(path);
        }
        for path in paths {
            match lock(&path)? {
                Some(file) => guard._handles.push(file),
                None => guard.active = true,
            }
        }
        Ok(guard)
    }
}

fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn report_issue(operation: &mut Operation, issue: impl std::fmt::Display) {
    if operation.errors.len() < 30 {
        operation
            .errors
            .push(issue.to_string().chars().take(1024).collect());
    } else if operation.errors.len() == 30 {
        operation
            .errors
            .push("其余问题已省略，剩余缓存可通过再次扫描核对".into());
    }
}

fn id() -> String {
    format!(
        "{}-{}-{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}

fn linked(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn pin_directory(path: &Path) -> Result<File> {
    let metadata = fs::symlink_metadata(path).map_err(error)?;
    if linked(&metadata) || !metadata.is_dir() {
        return Err(format!("缓存目录包含链接或类型不符 {}", path.display()));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Hold directory identities against replacement while walking/deleting children.
        options
            .access_mode(0x80)
            .custom_flags(0x02200000)
            .share_mode(3);
    }
    let file = options
        .open(path)
        .map_err(|e| format!("缓存目录访问失败 {} ({e})", path.display()))?;
    let metadata = file.metadata().map_err(error)?;
    if linked(&metadata) || !metadata.is_dir() {
        return Err("缓存目录身份发生变化".into());
    }
    Ok(file)
}

fn pin_chain(path: &Path, create: bool) -> Result<Vec<File>> {
    if !path.is_absolute()
        || path
            .components()
            .any(|p| matches!(p, std::path::Component::ParentDir))
    {
        return Err("缓存路径必须为明确的绝对路径".into());
    }
    let mut current = PathBuf::new();
    let mut pins = Vec::new();
    for part in path.components() {
        current.push(part);
        if matches!(part, std::path::Component::Prefix(_)) || !current.has_root() {
            continue;
        }
        if create && !current.exists() {
            fs::create_dir(&current).map_err(error)?;
        }
        pins.push(pin_directory(&current)?);
    }
    Ok(pins)
}

fn open_lock(path: &Path) -> Result<File> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if linked(&metadata) || !metadata.is_file() {
            return Err("缓存锁文件类型不符".into());
        }
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(3).custom_flags(0x00200000);
    }
    let file = options.open(path).map_err(error)?;
    let metadata = file.metadata().map_err(error)?;
    if linked(&metadata) || !metadata.is_file() {
        return Err("缓存锁文件身份发生变化".into());
    }
    Ok(file)
}

fn lock(path: &Path) -> Result<Option<File>> {
    let file = open_lock(path)?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => Err(error(e)),
    }
}

fn read_small(path: &Path) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path).map_err(error)?;
    if linked(&metadata) || !metadata.is_file() || metadata.len() > 1024 * 1024 {
        return Err(format!("缓存记录类型或大小不符 {}", path.display()));
    }
    let mut data = Vec::new();
    File::open(path)
        .map_err(error)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut data)
        .map_err(error)?;
    if data.len() > 1024 * 1024 {
        return Err("缓存记录超过读取范围".into());
    }
    Ok(data)
}

fn inventory(
    path: &Path,
    remove: bool,
    operation: &mut Operation,
    protected: &[PathBuf],
) -> Result<(u64, u64)> {
    if !path.exists() {
        return Ok((0, 0));
    }
    let _pins = pin_chain(path, false)?;
    let mut bytes = 0u64;
    let mut files = 0u64;
    for item in fs::read_dir(path).map_err(error)? {
        let item = item.map_err(error)?;
        if protected.contains(&item.path()) {
            continue;
        }
        let metadata = fs::symlink_metadata(item.path()).map_err(error)?;
        if linked(&metadata) {
            report_issue(operation, format!("已跳过链接 {}", item.path().display()));
        } else if metadata.is_dir() {
            let (child_bytes, child_files) = inventory(&item.path(), remove, operation, protected)?;
            bytes = bytes.saturating_add(child_bytes);
            files = files.saturating_add(child_files);
        } else if metadata.is_file() {
            bytes = bytes.saturating_add(metadata.len());
            files += 1;
            if remove {
                match fs::remove_file(item.path()) {
                    Ok(()) => {
                        operation.bytes_removed =
                            operation.bytes_removed.saturating_add(metadata.len());
                        operation.files_removed += 1;
                    }
                    Err(e) => report_issue(
                        operation,
                        format!("保留占用或不可清理文件 {} ({e})", item.path().display()),
                    ),
                }
            }
        }
    }
    // Directories intentionally remain, including empty nested directories.
    Ok((bytes, files))
}

#[cfg(windows)]
fn compiler_active() -> bool {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::Diagnostics::ToolHelp::*,
    };
    unsafe {
        let handle = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if handle == INVALID_HANDLE_VALUE {
            return true;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = Process32FirstW(handle, &mut entry) != 0;
        let mut active = !found;
        while found {
            let end = entry
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..end]).to_ascii_lowercase();
            if matches!(
                name.as_str(),
                "cargo.exe" | "rustc.exe" | "rustdoc.exe" | "clippy-driver.exe" | "link.exe"
            ) {
                active = true;
                break;
            }
            found = Process32NextW(handle, &mut entry) != 0;
        }
        CloseHandle(handle);
        active
    }
}
#[cfg(not(windows))]
fn compiler_active() -> bool {
    false
}

impl Project {
    pub fn open(root: &Path) -> Result<Self> {
        let pins = pin_chain(root, false)?;
        let root = fs::canonicalize(root).map_err(error)?;
        let package: serde_json::Value =
            serde_json::from_slice(&read_small(&root.join("package.json"))?).map_err(error)?;
        let cargo = String::from_utf8(read_small(&root.join("Cargo.toml"))?).map_err(error)?;
        if package["name"] != "cerulean-longing" || !cargo.contains("[workspace]") {
            return Err("请选择 CeruleanLonging 工作台源码工程目录".into());
        }
        let mut project = Self { root, _pins: pins };
        project._pins.extend(pin_chain(&project.control(), true)?);
        project._pins.extend(pin_chain(&project.logs(), true)?);
        Ok(project)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn temporary(&self) -> PathBuf {
        self.root.join(".temp/build-cache")
    }
    fn control(&self) -> PathBuf {
        self.root.join(".temp/build-cache-control")
    }
    fn logs(&self) -> PathBuf {
        self.root.join("docs/build-cache/cleanup-logs")
    }
    fn guard(&self) -> Result<File> {
        lock(&self.control().join("operations.lock"))?
            .ok_or("缓存扫描或清理正在进行，请等待当前操作".into())
    }

    fn wait_guard(&self) -> Result<File> {
        let file = open_lock(&self.control().join("operations.lock"))?;
        file.lock().map_err(error)?;
        Ok(file)
    }

    pub fn begin(&self) -> Result<Session> {
        let _guard = self.wait_guard()?;
        let name = id();
        let directory = self.temporary().join("jobs").join(&name);
        pin_chain(&directory, true)?;
        let lease =
            lock(&self.control().join(format!("job-{name}.lock")))?.ok_or("缓存作业已被占用")?;
        Ok(Session {
            directory,
            _lease: lease,
        })
    }

    fn operation(&self, kind: &str) -> Operation {
        Operation {
            id: id(),
            at: Utc::now()
                .with_timezone(&FixedOffset::east_opt(8 * 3600).unwrap())
                .to_rfc3339(),
            kind: kind.into(),
            completed: false,
            bytes_before: 0,
            bytes_removed: 0,
            files_removed: 0,
            skipped_active: 0,
            errors: vec![],
        }
    }

    fn record(&self, operation: &Operation) -> Result<()> {
        let _pins = pin_chain(&self.logs(), true)?;
        let phase = if operation.completed {
            "completed"
        } else {
            "started"
        };
        let path = self.logs().join(format!("{}-{phase}.md", operation.id));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(error)?;
        let summary = format!("# 缓存操作提炼记录\n\n时间为 {}。操作为 {}，阶段为 {}。\n\n- 扫描缓存 {} 字节。\n- 实际清理 {} 字节、{} 个文件。\n- 跳过 {} 个使用中的缓存区域。\n- 保留全部空目录。\n- 记录 {} 项待处理问题。\n\n{}\n<!-- cerulean-cache-v1 {} -->\n",
            operation.at, operation.kind, phase, operation.bytes_before, operation.bytes_removed,
            operation.files_removed, operation.skipped_active, operation.errors.len(),
            operation.errors.iter().take(30).map(|e| format!("- {}", e.replace(['\r', '\n'], " "))).collect::<Vec<_>>().join("\n"),
            serde_json::to_string(operation).map_err(error)?);
        file.write_all(summary.as_bytes()).map_err(error)?;
        file.sync_all().map_err(error)
    }

    fn candidates(&self) -> Result<Vec<Candidate>> {
        let mut result = vec![];
        let jobs = self.temporary().join("jobs");
        if jobs.exists() {
            let _pins = pin_chain(&jobs, false)?;
            for job in fs::read_dir(&jobs).map_err(error)? {
                let job = job.map_err(error)?;
                let name = job.file_name().to_string_lossy().into_owned();
                if name.is_empty() || !name.chars().all(|c| c.is_ascii_digit() || c == '-') {
                    continue;
                }
                result.push(Candidate {
                    path: job.path(),
                    kind: "任务临时缓存",
                    lease: Some(self.control().join(format!("job-{name}.lock"))),
                    cargo_profiles: vec![],
                });
            }
        }
        for name in ["manual", "bootstrap"] {
            result.push(Candidate {
                path: self.temporary().join(name),
                kind: "工具临时缓存",
                lease: None,
                cargo_profiles: ["cargo", "cargo/x86_64-pc-windows-msvc"]
                    .into_iter()
                    .flat_map(|target| {
                        ["debug", "release"].into_iter().map(move |profile| {
                            self.temporary().join(name).join(target).join(profile)
                        })
                    })
                    .collect(),
            });
        }
        for base in ["target", "target/x86_64-pc-windows-msvc"] {
            for profile in ["debug", "release"] {
                for name in ["deps", "build", ".fingerprint", "incremental", "examples"] {
                    result.push(Candidate {
                        path: self.root.join(base).join(profile).join(name),
                        kind: "旧编译缓存",
                        lease: None,
                        cargo_profiles: vec![self.root.join(base).join(profile)],
                    });
                }
            }
        }
        Ok(result)
    }

    fn snapshot_inner(&self) -> Result<Snapshot> {
        let mut result = Snapshot {
            project_root: self.root.clone(),
            temporary_root: self.temporary(),
            log_directory: self.logs(),
            entries: vec![],
            total_bytes: 0,
            available_bytes: 0,
            cumulative_cleaned_bytes: 0,
            scan_count: 0,
            pending_operations: 0,
            recent: vec![],
            warnings: vec![],
        };
        let mut observation = self.operation("读取");
        let compiler = compiler_active();
        for candidate in self.candidates()? {
            if !candidate.path.exists() {
                continue;
            }
            let guard = candidate.acquire()?;
            let active = guard.active || (candidate.lease.is_none() && compiler);
            match inventory(&candidate.path, false, &mut observation, &guard.protected) {
                Ok((bytes, files)) => {
                    result.total_bytes = result.total_bytes.saturating_add(bytes);
                    if !active {
                        result.available_bytes = result.available_bytes.saturating_add(bytes);
                    }
                    if bytes > 0 {
                        result.entries.push(Entry {
                            path: candidate.path,
                            kind: candidate.kind.into(),
                            bytes,
                            files,
                            active,
                        });
                    }
                }
                Err(e) => result.warnings.push(e),
            }
        }
        result.warnings.extend(observation.errors);
        let _pins = pin_chain(&self.logs(), false)?;
        let mut started = std::collections::BTreeSet::new();
        let mut completed = std::collections::BTreeSet::new();
        for item in fs::read_dir(self.logs()).map_err(error)? {
            let path = item.map_err(error)?.path();
            if path.extension().is_none_or(|x| x != "md") {
                continue;
            }
            let parsed = read_small(&path).and_then(|raw| {
                let text = String::from_utf8(raw).map_err(error)?;
                let json = text
                    .lines()
                    .find_map(|line| {
                        line.strip_prefix("<!-- cerulean-cache-v1 ")
                            .and_then(|s| s.strip_suffix(" -->"))
                    })
                    .ok_or("清理记录缺少可核对摘要")?;
                serde_json::from_str::<Operation>(json).map_err(error)
            });
            match parsed {
                Ok(record) if record.completed => {
                    if completed.insert(record.id.clone()) {
                        result.cumulative_cleaned_bytes = result
                            .cumulative_cleaned_bytes
                            .saturating_add(record.bytes_removed);
                        if record.kind == "扫描" {
                            result.scan_count += 1;
                        }
                        result.recent.push(record);
                    }
                }
                Ok(record) => {
                    started.insert(record.id);
                }
                Err(e) => result
                    .warnings
                    .push(format!("历史缓存记录需核对 {} ({e})", path.display())),
            }
        }
        result.pending_operations = started.difference(&completed).count();
        result.recent.sort_by(|a, b| b.at.cmp(&a.at));
        result.recent.truncate(12);
        Ok(result)
    }

    pub fn snapshot(&self, record_scan: bool) -> Result<Snapshot> {
        let _guard = self.guard()?;
        let mut snapshot = self.snapshot_inner()?;
        if record_scan {
            let mut operation = self.operation("扫描");
            operation.bytes_before = snapshot.total_bytes;
            for warning in &snapshot.warnings {
                report_issue(&mut operation, warning);
            }
            operation.completed = true;
            self.record(&operation)?;
            snapshot = self.snapshot_inner()?;
        }
        Ok(snapshot)
    }

    pub fn clean(&self) -> Result<Snapshot> {
        self.clean_as("手动清理")
    }

    pub fn collect_after_task(&self) -> Result<Snapshot> {
        self.clean_as("任务收尾缓存清理")
    }

    fn clean_as(&self, kind: &str) -> Result<Snapshot> {
        let _guard = self.guard()?;
        let mut operation = self.operation(kind);
        operation.bytes_before = self.snapshot_inner()?.total_bytes;
        self.record(&operation)?;
        let compiler = compiler_active();
        for candidate in self.candidates()? {
            if !candidate.path.exists() {
                continue;
            }
            let guard = candidate.acquire()?;
            if guard.active || (candidate.lease.is_none() && compiler) {
                operation.skipped_active += 1;
                continue;
            }
            match inventory(&candidate.path, true, &mut operation, &guard.protected) {
                Ok(_) => {}
                Err(e) => report_issue(&mut operation, e),
            }
        }
        operation.completed = true;
        self.record(&operation)?;
        self.snapshot_inner()
    }

    pub fn finish(&self, session: Session, succeeded: bool) -> Result<()> {
        if session.directory.parent() != Some(self.temporary().join("jobs").as_path()) {
            return Err("缓存作业不属于当前源码工程".into());
        }
        let _guard = self.wait_guard()?;
        let mut operation = self.operation(if succeeded {
            "任务完成自动清理"
        } else {
            "任务失败自动清理"
        });
        operation.bytes_before = inventory(&session.directory, false, &mut operation, &[])?.0;
        self.record(&operation)?;
        match inventory(&session.directory, true, &mut operation, &[]) {
            Ok(_) => {}
            Err(e) => report_issue(&mut operation, e),
        }
        operation.completed = true;
        self.record(&operation)?;
        if operation.errors.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "任务已结束，{} 项缓存清理问题已记入提炼日志",
                operation.errors.len()
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_entries_track_occupied_bytes_and_hide_retained_empty_directories() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            root.path().join("package.json"),
            r#"{"name":"cerulean-longing"}"#,
        )
        .unwrap();
        fs::write(root.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        let project = Project::open(root.path()).unwrap();
        let session = project.begin().unwrap();
        let directory = session.directory.clone();
        let nested = directory.join("nested");
        fs::create_dir(&nested).unwrap();
        fs::write(nested.join("empty.tmp"), b"").unwrap();
        fs::create_dir_all(root.path().join("target/release/deps")).unwrap();
        assert!(project.snapshot(true).unwrap().entries.is_empty());

        fs::write(nested.join("compiled.tmp"), b"abc").unwrap();
        let occupied = project.snapshot(false).unwrap();
        assert_eq!(occupied.entries.len(), 1);
        assert_eq!(occupied.entries[0].bytes, 3);
        assert!(occupied.entries[0].active);
        assert_eq!(occupied.total_bytes, 3);
        assert_eq!(occupied.available_bytes, 0);
        project.finish(session, true).unwrap();

        let empty = project.snapshot(false).unwrap();
        assert!(empty.entries.is_empty());
        assert_eq!(empty.total_bytes, 0);
        assert_eq!(empty.cumulative_cleaned_bytes, 3);
        assert_eq!(empty.pending_operations, 0);
        assert!(nested.is_dir());
        fs::write(nested.join("new.tmp"), b"x").unwrap();
        let reused = project.snapshot(true).unwrap();
        assert_eq!(reused.entries.len(), 1);
        assert_eq!(reused.entries[0].path, directory);
        assert_eq!(reused.entries[0].bytes, 1);
        assert!(!reused.entries[0].active);
        assert_eq!(reused.available_bytes, 1);
    }

    #[test]
    fn cargo_profile_mutex_protects_direct_builds() {
        let root = tempfile::tempdir().unwrap();
        let profile = root.path().join("release");
        let candidate = Candidate {
            path: profile.join("deps"),
            kind: "旧编译缓存",
            lease: None,
            cargo_profiles: vec![profile.clone()],
        };
        let first = candidate.acquire().unwrap();
        assert!(!first.active);
        assert_eq!(first.protected, vec![profile.join(".cargo-lock")]);
        let second = candidate.acquire().unwrap();
        assert!(second.active);
        drop(first);
        assert!(!candidate.acquire().unwrap().active);
    }

    #[test]
    fn diagnostic_details_remain_bounded() {
        let mut operation = Operation {
            id: "test".into(),
            at: "test".into(),
            kind: "test".into(),
            completed: false,
            bytes_before: 0,
            bytes_removed: 0,
            files_removed: 0,
            skipped_active: 0,
            errors: vec![],
        };
        for _ in 0..100 {
            report_issue(&mut operation, "路径".repeat(3000));
        }
        assert_eq!(operation.errors.len(), 31);
        assert!(serde_json::to_vec(&operation).unwrap().len() < 128 * 1024);
    }
}
