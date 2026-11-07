//! A bounded data-only handoff from the standard-integrity file picker.
use crate::file_import_process::{self, Process};
use cerulean_core::backup::open;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

const SCRIPT: &str = include_str!("../../../../scripts/file-import-window.ps1");
const FILES: &[&str] = &[
    "window.ps1",
    "plan.json",
    "input.part",
    "selected.bin",
    "receipt.part",
    "receipt.json",
    "cancel.signal",
    "finish.signal",
];
const CHUNK_BYTES: usize = 64 * 1024;

#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "camelCase", deny_unknown_fields)]
pub enum Request {
    Start,
    Poll { session: String },
    Read { session: String, offset: u64 },
    Finish { session: String },
    Cancel { session: String },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipt {
    session: String,
    name: String,
    bytes: u64,
    sha256: String,
}

struct Transfer {
    receipt: Receipt,
    file: File,
    digest: Sha256,
    offset: u64,
    verified: bool,
}

impl Transfer {
    fn open(directory: &Path, id: &str) -> Result<Self, String> {
        let receipt_file = open(&directory.join("receipt.json"), false, false)
            .map_err(|e| format!("无法核对导入回执：{e}"))?;
        let mut bytes = Vec::new();
        receipt_file
            .take(4097)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 4096 {
            return Err("导入回执过大".into());
        }
        let receipt: Receipt = serde_json::from_slice(&bytes).map_err(|_| "导入回执格式无效")?;
        let extension = Path::new(&receipt.name)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if receipt.session != id
            || receipt.name.is_empty()
            || receipt.name.len() > 1024
            || receipt
                .name
                .chars()
                .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
            || !["md", "json"].contains(&extension.to_ascii_lowercase().as_str())
            || receipt.sha256.len() != 64
            || !receipt.sha256.bytes().all(|c| c.is_ascii_hexdigit())
            || receipt.bytes > 9_007_199_254_740_991
        {
            return Err("导入会话、文件名或摘要不符".into());
        }
        if extension.eq_ignore_ascii_case("json") && receipt.bytes > 65536 {
            return Err("工作流模板不得超过64 KiB。".into());
        }
        // The receipt never supplies a filesystem path to this elevated reader.
        let file = open(&directory.join("selected.bin"), false, false)
            .map_err(|e| format!("导入交接文件不可用：{e}"))?;
        if file.metadata().map_err(|e| e.to_string())?.len() != receipt.bytes {
            return Err("导入文件长度与回执不符".into());
        }
        Ok(Self {
            receipt,
            file,
            digest: Sha256::new(),
            offset: 0,
            verified: false,
        })
    }

    fn read(&mut self, offset: u64) -> Result<serde_json::Value, String> {
        if offset != self.offset || self.verified {
            return Err("导入分块顺序不符，请重新选择文件".into());
        }
        let remaining = self.receipt.bytes - self.offset;
        let mut bytes = vec![0u8; remaining.min(CHUNK_BYTES as u64) as usize];
        self.file
            .read_exact(&mut bytes)
            .map_err(|e| format!("导入文件读取失败：{e}"))?;
        self.digest.update(&bytes);
        self.offset += bytes.len() as u64;
        let done = self.offset == self.receipt.bytes;
        if done {
            if format!("{:x}", self.digest.clone().finalize())
                != self.receipt.sha256.to_ascii_lowercase()
            {
                return Err("导入文件摘要不符，内容已拒绝".into());
            }
            self.verified = true;
        }
        Ok(serde_json::json!({"bytes":bytes,"offset":self.offset,"done":done}))
    }
}

struct Session {
    id: String,
    directory: PathBuf,
    process: Option<Process>,
    transfer: Option<Transfer>,
    pins: Vec<File>,
    directory_pin: Option<File>,
    _ancestors: Vec<File>,
}

fn signal(directory: &Path, name: &str) -> Result<(), String> {
    // create_new refuses pre-existing symlinks and files, without following them.
    match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(directory.join(name))
    {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(format!("无法通知导入窗口关闭：{e}")),
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if self.process.is_some() {
            let _ = signal(&self.directory, "cancel.signal");
        }
    }
}

fn pin_ancestors(root: &Path) -> Result<Vec<File>, String> {
    if !root.is_absolute() {
        return Err("导入根目录必须为绝对路径".into());
    }
    let mut pins = Vec::new();
    for path in root.ancestors().collect::<Vec<_>>().into_iter().rev() {
        if !path.exists() {
            fs::create_dir(path).map_err(|e| format!("创建导入目录失败：{e}"))?;
        }
        pins.push(open(path, true, false).map_err(|e| format!("导入目录包含无效链接或类型：{e}"))?);
    }
    Ok(pins)
}

impl Session {
    fn start(root: &Path) -> Result<Self, String> {
        let ancestors = pin_ancestors(root)?;
        let owner = file_import_process::current_owner()?;
        let id = file_import_process::session_id()?;
        let directory = root.join(&id);
        file_import_process::create_private_directory(&directory, &owner)?;
        let mut session = Self {
            id,
            directory,
            process: None,
            transfer: None,
            pins: Vec::new(),
            directory_pin: None,
            _ancestors: ancestors,
        };
        let result: Result<(), String> = (|| {
            session.directory_pin =
                Some(open(&session.directory, true, false).map_err(|e| e.to_string())?);
            // Windows PowerShell 5.1 needs a BOM to interpret the embedded Chinese text.
            session.write_pinned(
                "window.ps1",
                format!("\u{feff}{}", SCRIPT.trim_start_matches('\u{feff}')).as_bytes(),
            )?;
            let plan = serde_json::to_vec(&serde_json::json!({
                "session":session.id, "parentPid":std::process::id()
            }))
            .map_err(|e| e.to_string())?;
            session.write_pinned("plan.json", &plan)?;
            session.process = Some(file_import_process::launch(
                &session.directory.join("window.ps1"),
                &session.directory.join("plan.json"),
                &owner,
            )?);
            Ok(())
        })();
        if let Err(error) = result {
            let cleanup = session.cleanup();
            return Err(match cleanup {
                Ok(()) => error,
                Err(extra) => format!("{error}；{extra}"),
            });
        }
        Ok(session)
    }

    fn write_pinned(&mut self, name: &str, bytes: &[u8]) -> Result<(), String> {
        let path = self.directory.join(name);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        file.write_all(bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        let mut pin = open(&path, false, false).map_err(|e| e.to_string())?;
        let mut actual = Vec::new();
        (&mut pin)
            .take(bytes.len() as u64 + 1)
            .read_to_end(&mut actual)
            .map_err(|e| e.to_string())?;
        if actual != bytes {
            return Err("导入程序输入在启动前发生变化".into());
        }
        self.pins.push(pin);
        Ok(())
    }

    fn poll(&mut self) -> Result<serde_json::Value, String> {
        if self
            .directory
            .join("cancel.signal")
            .try_exists()
            .map_err(|e| e.to_string())?
        {
            return Ok(serde_json::json!({"status":"cancelled"}));
        }
        if self.transfer.is_none()
            && self
                .directory
                .join("receipt.json")
                .try_exists()
                .map_err(|e| e.to_string())?
        {
            self.transfer = Some(Transfer::open(&self.directory, &self.id)?);
        }
        if let Some(transfer) = &self.transfer {
            return Ok(
                serde_json::json!({"status":"ready","name":transfer.receipt.name,"bytes":transfer.receipt.bytes}),
            );
        }
        if self.process.as_ref().ok_or("导入进程缺失")?.finished()? {
            let code = self.process.as_ref().ok_or("导入进程缺失")?.exit_code()?;
            if code != 0 {
                return Err(format!("普通权限导入窗口运行失败，退出码 {code}"));
            }
            return Ok(serde_json::json!({"status":"cancelled"}));
        }
        Ok(serde_json::json!({"status":"waiting"}))
    }

    fn cleanup(&mut self) -> Result<(), String> {
        if let Some(process) = &self.process {
            if !process.wait(4000)? {
                return Err("导入窗口正在关闭，请稍后重试".into());
            }
        }
        self.process = None;
        self.transfer = None;
        self.pins.clear();
        // Only direct, fixed children of the pinned session directory are removed.
        // Unexpected files are retained, and recursive deletion is never used.
        for name in FILES {
            match fs::remove_file(self.directory.join(name)) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(format!("导入临时文件清理失败：{e}")),
            }
        }
        self.directory_pin = None;
        fs::remove_dir(&self.directory).map_err(|e| format!("导入临时目录清理失败：{e}"))
    }
}

pub struct Service {
    root: PathBuf,
    sessions: Mutex<HashMap<String, Session>>,
}

impl Service {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub fn run(&self, owner: &str, request: Request) -> Result<serde_json::Value, String> {
        let mut sessions = self.sessions.lock().map_err(|_| "导入窗口状态不可用")?;
        if matches!(request, Request::Start) {
            if sessions.contains_key(owner) {
                return Err("请先关闭已有的文件导入窗口".into());
            }
            let session = Session::start(&self.root)?;
            let result = serde_json::json!({"session":session.id,"pid":session.process.as_ref().map(|p| p.pid)});
            sessions.insert(owner.into(), session);
            return Ok(result);
        }
        let id = match &request {
            Request::Poll { session }
            | Request::Read { session, .. }
            | Request::Finish { session }
            | Request::Cancel { session } => session,
            Request::Start => unreachable!(),
        };
        let session = sessions
            .get_mut(owner)
            .filter(|s| &s.id == id)
            .ok_or("导入会话已结束或不属于此窗口")?;
        match request {
            Request::Poll { .. } => session.poll(),
            Request::Read { offset, .. } => {
                if session
                    .directory
                    .join("cancel.signal")
                    .try_exists()
                    .map_err(|e| e.to_string())?
                {
                    return Err("文件导入已取消".into());
                }
                session
                    .transfer
                    .as_mut()
                    .ok_or("导入文件尚未就绪")?
                    .read(offset)
            }
            Request::Finish { .. } | Request::Cancel { .. } => {
                let finish = matches!(request, Request::Finish { .. });
                if finish && !session.transfer.as_ref().is_some_and(|t| t.verified) {
                    return Err("文件完整性校验尚未完成".into());
                }
                if finish
                    && session
                        .directory
                        .join("cancel.signal")
                        .try_exists()
                        .map_err(|e| e.to_string())?
                {
                    return Err("文件导入已取消".into());
                }
                signal(
                    &session.directory,
                    if finish {
                        "finish.signal"
                    } else {
                        "cancel.signal"
                    },
                )?;
                session.cleanup()?;
                sessions.remove(owner);
                Ok(serde_json::json!({"status":"closed"}))
            }
            Request::Start => unreachable!(),
        }
    }

    pub fn close_owner(&self, owner: &str) -> Result<(), String> {
        let mut sessions = self.sessions.lock().map_err(|_| "导入窗口状态不可用")?;
        if let Some(session) = sessions.get_mut(owner) {
            signal(&session.directory, "cancel.signal")?;
            session.cleanup()?;
            sessions.remove(owner);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str, data: &[u8]) -> (tempfile::TempDir, Receipt) {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("selected.bin"), data).unwrap();
        let receipt = Receipt {
            session: "fixture".into(),
            name: name.into(),
            bytes: data.len() as u64,
            sha256: format!("{:x}", Sha256::digest(data)),
        };
        fs::write(
            dir.path().join("receipt.json"),
            serde_json::to_vec(&receipt).unwrap(),
        )
        .unwrap();
        (dir, receipt)
    }

    #[test]
    fn chunks_verify_large_markdown_and_reject_out_of_order() {
        let data = vec![b'x'; 150_001];
        let (dir, _) = fixture("需求.MD", &data);
        let mut transfer = Transfer::open(dir.path(), "fixture").unwrap();
        assert!(transfer.read(1).is_err());
        assert_eq!(transfer.read(0).unwrap()["done"], false);
        assert_eq!(transfer.read(65536).unwrap()["done"], false);
        assert_eq!(transfer.read(131072).unwrap()["done"], true);
        assert!(transfer.verified);
        assert!(transfer.read(150001).is_err());
    }

    #[test]
    fn receipt_bounds_and_identity_are_enforced() {
        for name in [
            "../outside.md",
            "C:\\outside.md",
            "name.md:stream",
            "bad.txt",
        ] {
            let (dir, _) = fixture(name, b"hello");
            assert!(Transfer::open(dir.path(), "fixture").is_err());
        }
        let (dir, _) = fixture("flow.json", &vec![0; 65537]);
        assert!(Transfer::open(dir.path(), "fixture").is_err());
        let (dir, _) = fixture("ok.md", b"hello");
        assert!(Transfer::open(dir.path(), "different").is_err());
        fs::write(dir.path().join("receipt.json"), vec![b' '; 4097]).unwrap();
        assert!(Transfer::open(dir.path(), "fixture").is_err());
    }

    #[test]
    fn tampered_length_digest_and_hardlink_are_rejected() {
        let (dir, _) = fixture("ok.md", b"hello");
        fs::write(dir.path().join("selected.bin"), b"other").unwrap();
        let mut transfer = Transfer::open(dir.path(), "fixture").unwrap();
        assert!(transfer.read(0).is_err());
        drop(transfer);
        fs::write(dir.path().join("selected.bin"), b"shorter?").unwrap();
        assert!(Transfer::open(dir.path(), "fixture").is_err());
        fs::hard_link(
            dir.path().join("selected.bin"),
            dir.path().join("linked.md"),
        )
        .unwrap();
        assert!(Transfer::open(dir.path(), "fixture").is_err());
    }

    #[test]
    fn empty_markdown_can_be_transferred_for_frontend_validation() {
        let (dir, _) = fixture("empty.md", b"");
        let mut transfer = Transfer::open(dir.path(), "fixture").unwrap();
        assert_eq!(transfer.read(0).unwrap()["done"], true);
    }
}
