//! Immutable before/after bytes for a file action. Inspecting never replays or
//! restores a write. The trusted host must obtain a fresh decision before repair.
use super::*;
use serde::{Deserialize, Serialize};
pub mod restore;
const MAX_RECORD: usize = 16384;
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Body {
    version: u32,
    intent_digest: String,
    dispatch_digest: String,
    call_id: String,
    path: String,
    before_sha256: Option<String>,
    before_bytes: usize,
    after_sha256: String,
    after_bytes: usize,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    body: Body,
    hash: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Applied {
    version: u32,
    write_digest: String,
}
pub(super) struct PreparedWrite {
    root: PathBuf,
    hash: String,
    _pins: Vec<File>,
}
fn publish(root: &Path, name: &str, data: &[u8]) -> Result<(), String> {
    // Every file is create-new and synced before any project write. A crash may
    // leave an incomplete image/record; inspection rejects it by size/hash/schema.
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .custom_flags(FLAGS)
        .open(root.join(name))
        .map_err(|_| "Write evidence creation failed")?;
    file.write_all(data)
        .and_then(|_| file.sync_all())
        .map_err(|_| "Write evidence sync failed")?;
    Ok(())
}
fn load(root: &Path, name: &str, max: usize) -> Result<Vec<u8>, String> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(1)
        .custom_flags(FLAGS)
        .open(root.join(name))
        .map_err(|_| "Write evidence unavailable")?;
    validate_file(&file).map_err(|_| "Write evidence file invalid")?;
    let mut bytes = Vec::new();
    file.take((max + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "Write evidence read failed")?;
    require(bytes.len() <= max, "Write evidence size exceeded")?;
    Ok(bytes)
}
impl PreparedWrite {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn prepare(
        writes: &Path,
        intent_digest: &str,
        dispatch_digest: &str,
        id: &str,
        path: &str,
        before: Option<&[u8]>,
        after: &[u8],
    ) -> Result<Self, String> {
        sha256(intent_digest)?;
        sha256(dispatch_digest)?;
        artifact_path(path)?;
        require(
            before.is_none_or(|b| b.len() <= MAX_FILE) && after.len() <= 48 * 1024,
            "Write image size exceeded",
        )?;
        let mut pins = pin_absolute(writes)?;
        let root = writes.join(hash(id.as_bytes()));
        fs::create_dir(&root).map_err(|_| "Write evidence already exists or is unavailable")?;
        pins.push(pin_directory(&root)?);
        if let Some(before) = before {
            publish(&root, "before.bin", before)?;
        }
        publish(&root, "after.bin", after)?;
        let body = Body {
            version: 1,
            intent_digest: intent_digest.into(),
            dispatch_digest: dispatch_digest.into(),
            call_id: id.into(),
            path: path.into(),
            before_sha256: before.map(hash),
            before_bytes: before.map_or(0, |b| b.len()),
            after_sha256: hash(after),
            after_bytes: after.len(),
        };
        let digest = digest(&body)?;
        publish(
            &root,
            "prepared.json",
            &serde_json::to_vec(&Record {
                body,
                hash: digest.clone(),
            })
            .map_err(|_| "Write evidence encoding failed")?,
        )?;
        Ok(Self {
            root,
            hash: digest,
            _pins: pins,
        })
    }
    pub(super) fn applied(&self) -> Result<(), String> {
        publish(
            &self.root,
            "applied.json",
            &serde_json::to_vec(&Applied {
                version: 1,
                write_digest: self.hash.clone(),
            })
            .map_err(|_| "Applied write encoding failed")?,
        )
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FileState {
    Original,
    Intended,
    Other,
    Missing,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriteInspection {
    pub call_id: String,
    pub path: String,
    pub write_digest: String,
    pub before_sha256: Option<String>,
    pub after_sha256: String,
    pub current_sha256: Option<String>,
    pub state: FileState,
    pub applied_recorded: bool,
    pub tool_result_recorded: bool,
}
/// Reads original evidence and current bytes under the original scope. No file
/// mutation or automatic repair. Refuses while the original service owns journal.
pub fn inspect(
    service_root: &Path,
    intent: &DispatchIntent,
    id: &str,
) -> Result<WriteInspection, String> {
    let _pins = pin_absolute(service_root)?;
    let journal = OmpToolJournal::open(&service_root.join("events"), intent)?;
    inspect_locked(service_root, intent, id, &journal)
}
fn inspect_locked(
    service_root: &Path,
    intent: &DispatchIntent,
    id: &str,
    journal: &OmpToolJournal,
) -> Result<WriteInspection, String> {
    let call = journal
        .observation()
        .calls
        .get(id)
        .ok_or("Write lacks original tool dispatch")?;
    let root = service_root.join("writes").join(hash(id.as_bytes()));
    let _image_pins = pin_absolute(&root)?;
    let entries = fs::read_dir(&root)
        .map_err(|_| "Write evidence listing failed")?
        .take(6)
        .map(|e| e.map(|e| e.file_name()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Write evidence listing failed")?;
    require(
        entries.len() <= 4
            && entries.iter().all(|n| {
                ["before.bin", "after.bin", "prepared.json", "applied.json"]
                    .iter()
                    .any(|allowed| n == allowed)
            }),
        "Write evidence incomplete or unexpected",
    )?;
    for name in &entries {
        crate::host::ordinary(&root.join(name), false)?;
    }
    require(
        call.tool_name == "cerulean_write_file",
        "Write evidence belongs to another tool",
    )?;
    let record: Record = serde_json::from_slice(&load(&root, "prepared.json", MAX_RECORD)?)
        .map_err(|_| "Write evidence record invalid")?;
    let body = record.body;
    require(
        body.version == 1
            && body.intent_digest == digest(intent)?
            && body.dispatch_digest == call.dispatch_digest
            && body.call_id == id
            && digest(&body)? == record.hash,
        "Write evidence scope or digest mismatch",
    )?;
    artifact_path(&body.path)?;
    if let Some(expected) = &body.before_sha256 {
        let bytes = load(&root, "before.bin", MAX_FILE)?;
        require(
            bytes.len() == body.before_bytes && hash(&bytes) == *expected,
            "Write before image changed",
        )?;
    } else {
        require(
            body.before_bytes == 0 && !root.join("before.bin").exists(),
            "Unexpected write before image",
        )?;
    }
    let after = load(&root, "after.bin", 48 * 1024)?;
    require(
        after.len() == body.after_bytes && hash(&after) == body.after_sha256,
        "Write after image changed",
    )?;
    let content = std::str::from_utf8(&after).map_err(|_| "Write after image is not UTF-8")?;
    let original_arguments = json!({"path":body.path,"content":content,"expectedSha256":body.before_sha256.as_deref().unwrap_or("absent")});
    require(
        digest(&original_arguments)? == call.arguments_sha256,
        "Write images differ from original tool arguments",
    )?;
    let applied_recorded = if root.join("applied.json").exists() {
        let applied: Applied = serde_json::from_slice(&load(&root, "applied.json", MAX_RECORD)?)
            .map_err(|_| "Applied record invalid")?;
        require(
            applied.version == 1 && applied.write_digest == record.hash,
            "Applied record differs",
        )?;
        true
    } else {
        false
    };
    let project_pins = pin_absolute(Path::new(&intent.binding.workspace))?;
    let project = Root {
        write_paths: None,
        path: root_path(&intent.binding.workspace)?,
        _pins: project_pins,
        protected: BTreeSet::new(),
        read: true,
        write: false,
        writes: service_root.join("writes"),
        intent_digest: digest(intent)?,
    };
    let (path, _parents) = checked_parent(&project, &body.path, false)
        .map_err(|_| "Write target parent unavailable or protected")?;
    let current_sha256 = match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err("Write target metadata unavailable".into()),
        Ok(_) => {
            let mut file = OpenOptions::new()
                .read(true)
                .share_mode(1)
                .custom_flags(FLAGS)
                .open(&path)
                .map_err(|_| "Write target cannot be locked")?;
            Some(hash(
                &bytes(&mut file).map_err(|_| "Write target changed or unavailable")?,
            ))
        }
    };
    let state = if current_sha256 == body.before_sha256 {
        FileState::Original
    } else if current_sha256.as_ref() == Some(&body.after_sha256) {
        FileState::Intended
    } else if current_sha256.is_none() {
        FileState::Missing
    } else {
        FileState::Other
    };
    Ok(WriteInspection {
        call_id: id.into(),
        path: body.path,
        write_digest: record.hash,
        before_sha256: body.before_sha256,
        after_sha256: body.after_sha256,
        current_sha256,
        state,
        applied_recorded,
        tool_result_recorded: call.result_sha256.is_some(),
    })
}
#[cfg(test)]
mod tests;

/// Read bounded previews only if they still match the reviewed hashes.
pub fn preview_content(
    service_root: &Path,
    intent: &DispatchIntent,
    review: &WriteInspection,
) -> Result<
    (
        cerulean_domain::omp_restore::FilePreview,
        cerulean_domain::omp_restore::FilePreview,
    ),
    String,
> {
    use cerulean_domain::omp_restore::FilePreview;
    fn preview(data: Option<Vec<u8>>) -> FilePreview {
        let Some(data) = data else {
            return FilePreview {
                text: None,
                bytes: None,
                truncated: false,
                binary: false,
            };
        };
        let size = data.len() as u32;
        match std::str::from_utf8(&data) {
            Ok(text) if !text.contains('\0') => {
                let mut end = text.len().min(16384);
                while !text.is_char_boundary(end) {
                    end -= 1;
                }
                FilePreview {
                    text: Some(text[..end].into()),
                    bytes: Some(size),
                    truncated: end < text.len(),
                    binary: false,
                }
            }
            _ => FilePreview {
                text: None,
                bytes: Some(size),
                truncated: false,
                binary: true,
            },
        }
    }
    let images = service_root
        .join("writes")
        .join(hash(review.call_id.as_bytes()));
    let _pins = pin_absolute(&images)?;
    let before = if let Some(expected) = &review.before_sha256 {
        let data = load(&images, "before.bin", MAX_FILE)?;
        require(hash(&data) == *expected, "Original preview changed")?;
        Some(data)
    } else {
        None
    };
    let pins = pin_absolute(Path::new(&intent.binding.workspace))?;
    let project = Root {
        write_paths: None,
        path: root_path(&intent.binding.workspace)?,
        _pins: pins,
        protected: BTreeSet::new(),
        read: true,
        write: false,
        writes: service_root.join("writes"),
        intent_digest: digest(intent)?,
    };
    let (path, _parents) =
        checked_parent(&project, &review.path, false).map_err(|_| "Preview path unavailable")?;
    let current = if let Some(expected) = &review.current_sha256 {
        let mut file = OpenOptions::new()
            .read(true)
            .share_mode(1)
            .custom_flags(FLAGS)
            .open(path)
            .map_err(|_| "Preview file unavailable")?;
        let data = bytes(&mut file).map_err(|_| "Preview file invalid")?;
        require(hash(&data) == *expected, "Current preview changed")?;
        Some(data)
    } else {
        require(
            fs::symlink_metadata(path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
            "Preview absence changed",
        )?;
        None
    };
    Ok((preview(before), preview(current)))
}
