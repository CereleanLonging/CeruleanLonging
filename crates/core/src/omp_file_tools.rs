//! Approved local file tools for OMP. Calls run bounded synchronous file I/O in
//! their polled future: no detached work can outlive a dropped transport future.
use crate::{
    artifacts::{contains_path, root_path},
    omp_tool_journal::OmpToolJournal,
};
use cerulean_adapters::omp_transport::worker::{
    host_calls::{HostEvent, HostFuture, HostOutput, HostRequest, HostService},
    host_tools::HostToolDefinition,
    DispatchIntent,
};
use cerulean_domain::execution::*;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::{Component, Path, PathBuf},
    sync::Arc,
};
use tokio::sync::watch;
use windows_sys::Win32::Storage::FileSystem::{
    GetFileInformationByHandle, BY_HANDLE_FILE_INFORMATION,
};
pub mod write_evidence;
const FLAGS: u32 = 0x02000000 | 0x00200000; // BACKUP_SEMANTICS | OPEN_REPARSE_POINT
const MAX_FILE: usize = 1024 * 1024;
struct Root {
    path: PathBuf,
    _pins: Vec<File>,
    protected: BTreeSet<String>,
    read: bool,
    write: bool,
    write_paths: Option<BTreeSet<String>>,
    writes: PathBuf,
    intent_digest: String,
}
struct PendingGrant {
    tool_call_id: String,
    name: String,
    arguments_sha256: String,
    dispatch_digest: String,
}
pub struct OmpFileTools {
    root: Arc<Root>,
    journal: OmpToolJournal,
    pending: BTreeMap<String, PendingGrant>,
    observer: Option<crate::omp_tool_checkpoint::Observer>,
    advice_boundary: Option<cerulean_adapters::omp_transport::worker::advice::AdviceBoundary>,
    errors: Arc<std::sync::Mutex<crate::omp_tool_checkpoint::ErrorTexts>>,
}
impl OmpFileTools {
    /// Called by the trusted host with channel-authenticated user/policy identity.
    /// The workflow still owns dispatch freshness, budgets and scheduling.
    pub fn create(
        plan: &PlanRevision,
        approval: &Approval,
        user: &str,
        policy_version: u32,
        intent: &DispatchIntent,
        journal_root: &Path,
    ) -> Result<Self, String> {
        approval.validate(plan, user, policy_version)?;
        intent.validate_metadata()?;
        let spec = plan.task(&intent.task_id)?;
        let worker = spec.worker.as_ref().ok_or("OMP worker selection missing")?;
        require(
            intent.revision_digest == plan.approval_digest()?
                && intent.binding.workspace
                    == plan.execution_root(&intent.task_id, &intent.attempt_id)?
                && spec.adapter_id == "omp"
                && intent.provider == worker.provider
                && intent.model == worker.model,
            "OMP tool scope differs from approval",
        )?;
        let root = file_root(
            plan,
            None,
            &intent.binding.workspace,
            journal_root,
            digest(intent)?,
        )?;
        let journal = OmpToolJournal::create(&journal_root.join("events"), intent)?;
        Ok(Self {
            root,
            journal,
            pending: BTreeMap::new(),
            observer: None,
            advice_boundary: None,
            errors: Default::default(),
        })
    }
    pub fn definitions(&self) -> Vec<HostToolDefinition> {
        file_definitions(&self.root)
    }
    pub fn observation(&self) -> &crate::omp_tool_journal::ToolObservation {
        self.journal.observation()
    }
    pub(crate) fn observe_completed_tools(
        &mut self,
        observer: crate::omp_tool_checkpoint::Observer,
    ) {
        self.observer = Some(observer);
    }
}
pub(crate) mod api;
fn file_root(
    plan: &PlanRevision,
    write_paths: Option<&[String]>,
    workspace: &str,
    journal_root: &Path,
    intent_digest: String,
) -> Result<Arc<Root>, String> {
    let read = plan.policy.tools.contains(&ToolPermission::ReadFiles);
    let write = plan.policy.tools.contains(&ToolPermission::WriteFiles);
    if let Some(paths) = write_paths {
        require(
            write && !paths.is_empty() && paths.len() <= 64,
            "invalid approved file write scope",
        )?;
        let mut seen = BTreeSet::new();
        for path in paths {
            artifact_path(path)?;
            require(
                !path.contains('~') && seen.insert(path.to_lowercase()),
                "invalid or duplicated approved write path",
            )?;
        }
    }
    require(read || write, "Approved file permissions absent")?;
    let pins = pin_absolute(Path::new(workspace))?;
    let path = root_path(workspace)?;
    let parent = root_path(
        journal_root
            .parent()
            .and_then(Path::to_str)
            .ok_or("Approved tool journal parent invalid")?,
    )?;
    require(
        !contains_path(&path, &parent),
        "Approved tool evidence must be outside project",
    )?;
    let mut protected = BTreeSet::new();
    for task in &plan.execution {
        if let Some(worker) = &task.worker {
            protected.insert(
                fs::canonicalize(&worker.executable.path)
                    .map_err(|_| "Approved worker path unavailable")?
                    .to_string_lossy()
                    .to_lowercase(),
            );
        }
        for check in &task.checks {
            if let Some(tool) = &check.tool {
                for input in std::iter::once(&tool.executable).chain(&tool.inputs) {
                    protected.insert(
                        fs::canonicalize(&input.path)
                            .map_err(|_| "Approved verifier path unavailable")?
                            .to_string_lossy()
                            .to_lowercase(),
                    );
                }
            }
        }
    }
    fs::create_dir(journal_root)
        .map_err(|_| "Approved file service requires a new evidence directory")?;
    let writes = journal_root.join("writes");
    fs::create_dir(&writes).map_err(|_| "Approved write evidence directory unavailable")?;
    Ok(Arc::new(Root {
        write_paths: write_paths.map(|paths| paths.iter().cloned().collect()),
        path,
        _pins: pins,
        protected,
        read,
        write,
        writes,
        intent_digest,
    }))
}
fn file_definitions(root: &Root) -> Vec<HostToolDefinition> {
    let mut result = Vec::new();
    if root.read {
        result.push(HostToolDefinition{name:"cerulean_read_file".into(),description:"Read a UTF-8 project file in chunks. Returns content, next byte offset, total bytes and full-file SHA-256. Offset must be a UTF-8 boundary.".into(),parameters:json!({"type":"object","properties":{"path":{"type":"string"},"offset":{"type":"integer","minimum":0}},"required":["path","offset"],"additionalProperties":false})});
    }
    if root.write {
        result.push(HostToolDefinition{name:"cerulean_write_file".into(),description:"Write UTF-8 project content (up to 48 KiB). expectedSha256 must match the current file, or be 'absent' for a new file. A conflict never overwrites the file. Missing parent directories may be created.".into(),parameters:json!({"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"},"expectedSha256":{"type":"string"}},"required":["path","content","expectedSha256"],"additionalProperties":false})});
        if let Some(paths) = &root.write_paths {
            let definition = result.last_mut().unwrap();
            definition.parameters["properties"]["path"]["enum"] = json!(paths);
            definition.description.push_str(" Only the exact declared task artifact paths are writable; other project files may be read but cannot be changed.");
        }
    }
    result
}

impl HostService for OmpFileTools {
    fn take_advice_boundary(
        &mut self,
        _: &DispatchIntent,
    ) -> Result<Option<cerulean_adapters::omp_transport::worker::advice::AdviceBoundary>, String>
    {
        Ok(self.advice_boundary.take())
    }
    fn record(&mut self, intent: &DispatchIntent, event: &HostEvent) -> Result<(), String> {
        self.journal.record(intent, event)?;
        if let HostEvent::Dispatch {
            id,
            tool_call_id,
            tool_name,
            arguments_sha256,
        } = event
        {
            self.pending.insert(
                id.clone(),
                PendingGrant {
                    tool_call_id: tool_call_id.clone(),
                    name: tool_name.clone(),
                    arguments_sha256: arguments_sha256.clone(),
                    dispatch_digest: digest(event)?,
                },
            );
        }
        if let HostEvent::Cancel { id } | HostEvent::Result { id, .. } = event {
            self.pending.remove(id);
        }
        if matches!(event, HostEvent::Result { .. }) && self.journal.observation().unresolved() == 0
        {
            if let Some(observer) = self.observer.as_mut() {
                require(
                    self.advice_boundary.is_none(),
                    "OMP advice boundary was not consumed",
                )?;
                self.advice_boundary =
                    observer(crate::omp_tool_checkpoint::OmpToolCheckpoint::new(
                        intent,
                        self.journal.checkpoint()?,
                        self.errors
                            .lock()
                            .map_err(|_| "OMP error context unavailable")?
                            .clone(),
                    )?)?;
            }
        }
        Ok(())
    }
    fn start(&mut self, request: HostRequest, cancel: watch::Receiver<bool>) -> HostFuture {
        let grant = match &request {
            HostRequest::HostToolCall { id, .. } => self.pending.remove(id),
            _ => None,
        };
        let dispatch_digest = grant.as_ref().map(|g| g.dispatch_digest.clone());
        let authorized = match (&request, grant) {
            (
                HostRequest::HostToolCall {
                    tool_call_id,
                    tool_name,
                    arguments,
                    ..
                },
                Some(g),
            ) => {
                g.tool_call_id == *tool_call_id
                    && g.name == *tool_name
                    && digest(arguments).is_ok_and(|h| h == g.arguments_sha256)
            }
            _ => false,
        };
        let root = self.root.clone();
        let errors = self.errors.clone();
        Box::pin(async move {
            require(
                authorized,
                "OMP file action lacks durable matching dispatch",
            )?;
            if *cancel.borrow() || cancel.has_changed().is_err() {
                return Ok(error("File action cancelled before execution"));
            }
            let HostRequest::HostToolCall {
                id,
                tool_name,
                arguments,
                ..
            } = request
            else {
                unreachable!()
            };
            let result = match tool_name.as_str() {
                "cerulean_read_file" if root.read => read_file(
                    &root,
                    serde_json::from_value(Value::Object(arguments))
                        .map_err(|_| "Invalid read arguments"),
                ),
                "cerulean_write_file" if root.write => write_file(
                    &root,
                    &id,
                    dispatch_digest.as_deref().unwrap(),
                    serde_json::from_value(Value::Object(arguments))
                        .map_err(|_| "Invalid write arguments"),
                ),
                _ => Err("File permission denied".into()),
            };
            // Validation failures have no content side effect. I/O failures may
            // follow partial writes, so propagate them as unknown, not a tool success.
            match result {
                Ok(text) => Ok(HostOutput {
                    text,
                    is_error: false,
                }),
                Err(FileError::Rejected(message)) => {
                    if message.len() <= crate::omp_tool_checkpoint::MAX_ERROR_BYTES {
                        errors
                            .lock()
                            .map_err(|_| "OMP error context unavailable")?
                            .insert(id, message.clone());
                    }
                    Ok(error(&message))
                }
                Err(FileError::Unknown) => {
                    Err("OMP file I/O outcome unknown; inspect original file and journal".into())
                }
            }
        })
    }
}
fn error(text: &str) -> HostOutput {
    HostOutput {
        text: text.into(),
        is_error: true,
    }
}
enum FileError {
    Rejected(String),
    Unknown,
}
impl From<&str> for FileError {
    fn from(value: &str) -> Self {
        Self::Rejected(value.into())
    }
}
impl From<String> for FileError {
    fn from(value: String) -> Self {
        Self::Rejected(value)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadArgs {
    path: String,
    offset: usize,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WriteArgs {
    path: String,
    content: String,
    expected_sha256: String,
}
#[cfg(all(test, windows, target_pointer_width = "64"))]
mod pin_diagnostic;
#[cfg_attr(test, track_caller)]
fn pin_directory(path: &Path) -> Result<File, String> {
    #[cfg(test)]
    let caller = std::panic::Location::caller();
    let file = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .custom_flags(FLAGS)
        .open(path)
        .map_err(|_error| {
            #[cfg(test)]
            eprintln!(
                "omp restore 328 pin-open failure: path={path:?} raw_os_error={:?} kind={:?} caller={caller} pid={} thread={:?} access=GENERIC_READ share=FILE_SHARE_READ flags={FLAGS:#010x}",
                _error.raw_os_error(),
                _error.kind(),
                std::process::id(),
                std::thread::current().name(),
            );
            #[cfg(all(test, windows, target_pointer_width = "64"))]
            let _ = std::panic::catch_unwind(|| pin_diagnostic::report(path));
            "OMP directory pin unavailable"
        })?;
    let meta = file
        .metadata()
        .map_err(|_| "OMP directory metadata unavailable")?;
    require(
        meta.is_dir() && meta.file_attributes() & 0x400 == 0,
        "OMP linked or invalid directory",
    )?;
    Ok(file)
}
#[cfg_attr(test, track_caller)]
fn pin_absolute(path: &Path) -> Result<Vec<File>, String> {
    require(
        path.is_absolute()
            && path.components().all(|c| {
                matches!(
                    c,
                    Component::Prefix(_) | Component::RootDir | Component::Normal(_)
                )
            }),
        "OMP project root must be absolute and normalized",
    )?;
    let mut pins = Vec::new();
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component.as_os_str());
        if matches!(component, Component::Normal(_)) {
            pins.push(pin_directory(&current)?);
        }
    }
    Ok(pins)
}
fn checked_parent(
    root: &Root,
    relative: &str,
    create: bool,
) -> Result<(PathBuf, Vec<File>), FileError> {
    artifact_path(relative)?;
    if create {
        require(
            root.write_paths
                .as_ref()
                .is_none_or(|paths| paths.contains(relative)),
            "File outside approved task artifact write scope; use the exact declared path",
        )?;
    }
    require(
        !relative.contains('~'),
        "OMP short-name aliases are not supported",
    )?;
    require(relative.split('/').count() <= 64, "OMP path depth exceeded")?;
    for part in relative.split('/') {
        let part = part.to_lowercase();
        require(
            !matches!(
                part.as_str(),
                ".git"
                    | ".ssh"
                    | ".aws"
                    | ".codex"
                    | ".omp"
                    | ".pi"
                    | ".claude"
                    | ".gemini"
                    | "credentials"
                    | "secrets"
                    | "auth.json"
                    | "credentials.json"
            ) && !part.starts_with(".env")
                && !part.ends_with(".dpapi"),
            "OMP protected file path",
        )?;
    }
    let mut path = root.path.clone();
    let mut pins = Vec::new();
    let parts = relative.split('/').collect::<Vec<_>>();
    for part in &parts[..parts.len() - 1] {
        path.push(part);
        if create
            && fs::symlink_metadata(&path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
        {
            fs::create_dir(&path).map_err(|_| FileError::Unknown)?;
        }
        pins.push(pin_directory(&path)?);
    }
    path.push(parts.last().unwrap());
    if create {
        require(
            !root
                .protected
                .contains(&path.to_string_lossy().to_lowercase()),
            "OMP approved worker/verifier is protected",
        )?;
    }
    Ok((path, pins))
}
fn validate_file(file: &File) -> Result<(), FileError> {
    let meta = file.metadata().map_err(|_| FileError::Unknown)?;
    require(
        meta.is_file() && meta.file_attributes() & 0x400 == 0 && meta.len() <= MAX_FILE as u64,
        "OMP linked, oversized or invalid file",
    )?;
    let mut info: BY_HANDLE_FILE_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut info) } == 0 {
        return Err(FileError::Unknown);
    }
    require(info.nNumberOfLinks == 1, "OMP hard-linked file refused")?;
    Ok(())
}
fn bytes(file: &mut File) -> Result<Vec<u8>, FileError> {
    validate_file(file)?;
    let mut data = Vec::new();
    file.take((MAX_FILE + 1) as u64)
        .read_to_end(&mut data)
        .map_err(|_| FileError::Unknown)?;
    require(data.len() <= MAX_FILE, "OMP file exceeds limit")?;
    Ok(data)
}
fn hash(data: &[u8]) -> String {
    format!("{:x}", Sha256::digest(data))
}
fn read_file(root: &Root, args: Result<ReadArgs, &str>) -> Result<String, FileError> {
    let args = args?;
    let (path, _pins) = checked_parent(root, &args.path, false)?;
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .custom_flags(FLAGS)
        .open(path)
        .map_err(|_| "File unavailable or concurrently modified")?;
    let data = bytes(&mut file)?;
    let content = std::str::from_utf8(&data).map_err(|_| "File is not UTF-8")?;
    require(
        args.offset <= data.len() && content.is_char_boundary(args.offset),
        "Invalid UTF-8 read offset",
    )?;
    let mut end = (args.offset + 8192).min(data.len());
    while !content.is_char_boundary(end) {
        end -= 1;
    }
    Ok(json!({"path":args.path,"content":&content[args.offset..end],"nextOffset":end,"bytes":data.len(),"sha256":hash(&data)}).to_string())
}
fn write_file(
    root: &Root,
    id: &str,
    dispatch_digest: &str,
    args: Result<WriteArgs, &str>,
) -> Result<String, FileError> {
    write_file_observed(root, id, dispatch_digest, args, |_| Ok(()))
}
fn write_file_observed(
    root: &Root,
    id: &str,
    dispatch_digest: &str,
    args: Result<WriteArgs, &str>,
    mut observe: impl FnMut(&str) -> Result<(), FileError>,
) -> Result<String, FileError> {
    let args = args?;
    require(
        args.content.len() <= 48 * 1024 && !args.content.contains('\0'),
        "Write content exceeds limit or contains NUL",
    )?;
    if args.expected_sha256 != "absent" {
        sha256(&args.expected_sha256)?;
    }
    let (path, _pins) = checked_parent(root, &args.path, true)?;
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .share_mode(0)
        .custom_flags(FLAGS);
    let (existing, before) = if args.expected_sha256 == "absent" {
        require(
            fs::symlink_metadata(&path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
            "File already exists or is inaccessible",
        )?;
        (None, None)
    } else {
        let mut file = options
            .open(&path)
            .map_err(|_| "File unavailable or concurrently modified")?;
        let data = bytes(&mut file)?;
        require(
            hash(&data) == args.expected_sha256,
            "File version conflict; read again before writing",
        )?;
        (Some(file), Some(data))
    };
    let evidence = write_evidence::PreparedWrite::prepare(
        &root.writes,
        &root.intent_digest,
        dispatch_digest,
        id,
        &args.path,
        before.as_deref(),
        args.content.as_bytes(),
    )
    .map_err(|_| FileError::Unknown)?;
    observe("prepared")?;
    let mut file = if let Some(file) = existing {
        file
    } else {
        options
            .create_new(true)
            .open(&path)
            .map_err(|_| FileError::Unknown)?
    };
    validate_file(&file)?;
    file.seek(SeekFrom::Start(0))
        .map_err(|_| FileError::Unknown)?;
    let split = args.content.len() / 2;
    file.write_all(&args.content.as_bytes()[..split])
        .map_err(|_| FileError::Unknown)?;
    observe("partial")?;
    file.write_all(&args.content.as_bytes()[split..])
        .and_then(|_| file.set_len(args.content.len() as u64))
        .and_then(|_| file.sync_all())
        .map_err(|_| FileError::Unknown)?;
    observe("synced")?;
    evidence.applied().map_err(|_| FileError::Unknown)?;
    observe("recorded")?;
    Ok(
        json!({"path":args.path,"bytes":args.content.len(),"sha256":hash(args.content.as_bytes())})
            .to_string(),
    )
}
#[cfg(test)]
pub(crate) mod tests;
