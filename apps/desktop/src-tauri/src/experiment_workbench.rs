//! New, owned experiment containers. Existing/partial directories are never adopted.
//! A window's binding is immutable: late IPC replies cannot cross workbenches.
use std::{collections::BTreeMap, path::PathBuf, sync::Mutex};

pub struct Binding {
    pub directory: PathBuf,
    pub _pins: Vec<std::fs::File>,
}
#[derive(Default)]
pub struct Windows(Mutex<BTreeMap<String, Binding>>);
impl Windows {
    pub fn bind(&self, label: &str, binding: Binding) -> Result<(), String> {
        let mut windows = self.0.lock().map_err(|_| "窗口目录登记不可用")?;
        if let Some(existing) = windows.get(label) {
            if existing.directory != binding.directory {
                return Err("窗口已经绑定另一工作台，拒绝切换目录".into());
            }
        } else {
            windows.insert(label.into(), binding);
        }
        Ok(())
    }
    pub fn directory(&self, label: &str) -> Result<PathBuf, String> {
        self.0
            .lock()
            .map_err(|_| "窗口目录登记不可用")?
            .get(label)
            .map(|b| b.directory.clone())
            .ok_or("窗口未绑定工作台".into())
    }
}

#[cfg(windows)]
pub mod catalog {
    use super::Binding;
    use cerulean_core::{backup, data_format, experiment_guard, rejected_usage_guard, Store};
    use serde::{Deserialize, Serialize};
    use sha2::{Digest, Sha256};
    use std::{
        fs::{self, File, OpenOptions},
        io::{Read, Write},
        path::{Component, Path, PathBuf},
    };

    const LIMIT: usize = 64;
    const RECORD_LIMIT: u64 = 65536;
    const PURPOSE: &str = "CeruleanLonging experiment workbench v1";
    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Intent {
        purpose: String,
        id: String,
        data_root: PathBuf,
        minimum_reader: u32,
        credentials_copied: bool,
    }
    #[derive(Serialize, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Ready {
        purpose: String,
        id: String,
        data_root: PathBuf,
        intent_digest: String,
        review_digest: String,
        backup_digest: String,
        guard_digest: String,
    }
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Entry {
        pub id: String,
        pub directory: PathBuf,
        pub ready: bool,
        pub error: Option<String>,
    }
    fn err(e: impl std::fmt::Display) -> String {
        e.to_string()
    }
    fn require(ok: bool, message: &str) -> Result<(), String> {
        if ok {
            Ok(())
        } else {
            Err(message.into())
        }
    }
    pub fn valid_id(id: &str) -> bool {
        (5..=48).contains(&id.len())
            && id.starts_with("exp-")
            && id
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
            && !id.ends_with('-')
    }
    // Pin all ancestors, rejecting links and lexical aliases before any write.
    fn pins(path: &Path, create: bool) -> Result<Vec<File>, String> {
        require(
            path.is_absolute()
                && !path
                    .components()
                    .any(|c| matches!(c, Component::ParentDir | Component::CurDir)),
            "工作台路径必须是无相对段的绝对目录",
        )?;
        let mut handles = vec![];
        for p in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
            if create {
                // Windows volume roots can return AccessDenied, rather than
                // AlreadyExists, for CreateDirectory. Inspect first, as the
                // existing WebView profile path preparation does.
                match fs::symlink_metadata(p) {
                    Ok(_) => (),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => match fs::create_dir(p) {
                        Ok(()) => (),
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
                        Err(e) => return Err(format!("创建工作台父目录 {}：{e}", p.display())),
                    },
                    Err(e) => return Err(format!("读取工作台父目录 {}：{e}", p.display())),
                }
            }
            handles.push(
                backup::open(p, true, false)
                    .map_err(|e| format!("固定工作台父目录 {}：{e}", p.display()))?,
            );
        }
        Ok(handles)
    }
    fn read(path: &Path) -> Result<Vec<u8>, String> {
        read_pinned(path, &mut vec![])
    }
    fn read_pinned(path: &Path, handles: &mut Vec<File>) -> Result<Vec<u8>, String> {
        let mut file = backup::open(path, false, false).map_err(err)?;
        let mut bytes = vec![];
        (&mut file)
            .take(RECORD_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(err)?;
        require(bytes.len() <= RECORD_LIMIT as usize, "工作台登记文件过大")?;
        handles.push(file);
        Ok(bytes)
    }
    fn hash(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }
    fn write_new(path: &Path, value: &impl Serialize) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(value).map_err(err)?;
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .map_err(err)?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(err)
    }
    fn count(root: &Path) -> Result<usize, String> {
        let mut count = 0;
        for item in fs::read_dir(root).map_err(err)? {
            item.map_err(err)?;
            count += 1;
            require(count <= LIMIT, "实验工作台登记超过64项，请先核查原目录")?;
        }
        Ok(count)
    }
    pub fn create(root: &Path, id: &str) -> Result<Entry, String> {
        create_for_generation(root, id, rejected_usage_guard::GENERATION)
    }
    #[cfg(test)]
    pub(super) fn create_legacy(root: &Path, id: &str) -> Result<Entry, String> {
        create_for_generation(root, id, experiment_guard::GENERATION)
    }
    fn create_for_generation(root: &Path, id: &str, generation: u32) -> Result<Entry, String> {
        require(matches!(generation, 15 | 17), "未知新工作台格式")?;
        require(
            valid_id(id),
            "实验编号须以exp-开头，总长5至48位，仅含小写字母、数字或连字符",
        )?;
        let _root_pins = pins(root, true)?;
        require(count(root)? < LIMIT, "实验工作台登记已达64项")?;
        let folder = root.join(id);
        // This is also the cross-process reservation. Never reuse a failed name.
        fs::create_dir(&folder).map_err(|e| format!("未重复创建；请读取原登记：{e}"))?;
        let _folder_pin = backup::open(&folder, true, false).map_err(err)?;
        let folder = fs::canonicalize(folder).map_err(err)?;
        let data = folder.join("data");
        write_new(
            &folder.join("intent.json"),
            &Intent {
                purpose: PURPOSE.into(),
                id: id.into(),
                data_root: data.clone(),
                minimum_reader: generation,
                credentials_copied: false,
            },
        )?;
        #[cfg(test)]
        super::creation_tests::fault(&folder, "after-intent");
        fs::create_dir(&data).map_err(err)?;
        // No runtime Host, provider, scheduler or existing data is involved.
        drop(Store::open(&data)?);
        let archive = folder.join("initial-cold-backup");
        let backup_digest = backup::create(&data, &archive)?.manifest_digest;
        let (status, guard_file) = if generation == rejected_usage_guard::GENERATION {
            let review = rejected_usage_guard::review(&data, &archive, &backup_digest)?;
            write_new(&folder.join("protection-review.json"), &review)?;
            (
                rejected_usage_guard::promote(&data, &archive, &backup_digest, &review.digest)?,
                rejected_usage_guard::MARKER,
            )
        } else {
            let review = experiment_guard::review(&data, &archive, &backup_digest)?;
            write_new(&folder.join("protection-review.json"), &review)?;
            (
                experiment_guard::promote(&data, &archive, &backup_digest, &review.digest)?,
                experiment_guard::MARKER,
            )
        };
        require(
            status.minimum_reader_generation == generation,
            "新工作台实验保护未完成",
        )?;
        let ready = Ready {
            purpose: PURPOSE.into(),
            id: id.into(),
            data_root: data,
            intent_digest: hash(&read(&folder.join("intent.json"))?),
            review_digest: hash(&read(&folder.join("protection-review.json"))?),
            backup_digest,
            guard_digest: hash(&read(&folder.join("data").join(guard_file))?),
        };
        // A crash during publication leaves ready.pending and is diagnostic-only.
        write_new(&folder.join("ready.pending"), &ready)?;
        #[cfg(test)]
        super::creation_tests::fault(&folder, "before-publication");
        fs::rename(folder.join("ready.pending"), folder.join("ready.json")).map_err(err)?;
        let (entry, _) = open(root, id)?;
        Ok(entry)
    }
    pub fn open(root: &Path, id: &str) -> Result<(Entry, Binding), String> {
        require(valid_id(id), "实验编号无效")?;
        let mut handles = pins(&root.join(id), false)?;
        let folder = fs::canonicalize(root.join(id)).map_err(err)?;
        require(
            !folder.join("ready.pending").try_exists().map_err(err)?,
            "创建发布未完成，保留原目录；不自动修复或重试",
        )?;
        require(
            folder.join("ready.json").try_exists().map_err(err)?,
            "创建尚未确认完成：缺少最终就绪登记。原目录保留用于核查；不能打开或重复创建同一编号，继续其他实验请使用新编号",
        )?;
        let ready: Ready =
            serde_json::from_slice(&read_pinned(&folder.join("ready.json"), &mut handles)?)
                .map_err(err)?;
        let intent_bytes = read_pinned(&folder.join("intent.json"), &mut handles)?;
        let intent: Intent = serde_json::from_slice(&intent_bytes).map_err(err)?;
        let data = folder.join("data");
        require(
            ready.purpose == PURPOSE
                && ready.id == id
                && ready.data_root == data
                && intent.purpose == PURPOSE
                && intent.id == id
                && intent.data_root == data
                && matches!(intent.minimum_reader, 14 | 15 | 17)
                && !intent.credentials_copied
                && ready.intent_digest == hash(&intent_bytes)
                && ready.review_digest
                    == hash(&read_pinned(
                        &folder.join("protection-review.json"),
                        &mut handles,
                    )?),
            "工作台登记与原创建意图不符",
        )?;
        handles.push(backup::open(&data, true, false).map_err(err)?);
        handles.push(backup::open(&data.join("events"), true, false).map_err(err)?);
        let guard_file = match intent.minimum_reader {
            14 => "events/00000000.reader14.json",
            15 => experiment_guard::MARKER,
            17 => rejected_usage_guard::MARKER,
            _ => return Err("未知原工作台保护格式".into()),
        };
        let guard = read_pinned(&data.join(guard_file), &mut handles)?;
        if intent.minimum_reader < experiment_guard::GENERATION
            && data
                .join(experiment_guard::MARKER)
                .try_exists()
                .map_err(err)?
        {
            let _current_guard = read_pinned(&data.join(experiment_guard::MARKER), &mut handles)?;
        }
        if data
            .join(cerulean_core::relay_guard::MARKER)
            .try_exists()
            .map_err(err)?
        {
            let _relay_guard =
                read_pinned(&data.join(cerulean_core::relay_guard::MARKER), &mut handles)?;
        }
        if data
            .join(cerulean_core::rejected_usage_guard::MARKER)
            .try_exists()
            .map_err(err)?
        {
            let _usage_guard = read_pinned(
                &data.join(cerulean_core::rejected_usage_guard::MARKER),
                &mut handles,
            )?;
        }
        let guard_value: serde_json::Value = serde_json::from_slice(&guard).map_err(err)?;
        let archive = folder.join("initial-cold-backup");
        let backup_review = backup::verify(&archive)?;
        let original = serde_json::to_value(&backup_review).map_err(err)?;
        require(
            backup_review.manifest_digest == ready.backup_digest
                && original["sourceRoot"] == serde_json::json!(data)
                && guard_value["sourceBackupDigest"] == ready.backup_digest
                && ready.guard_digest == hash(&guard),
            "工作台保护或初始备份不匹配",
        )?;
        require(
            (intent.minimum_reader..=cerulean_core::rejected_usage_guard::GENERATION)
                .contains(&data_format::status(&data)?.minimum_reader_generation),
            "实验保护状态不匹配",
        )?;
        // Keep the identity records read above pinned for this window's lifetime.
        Ok((
            Entry {
                id: id.into(),
                directory: data.clone(),
                ready: true,
                error: None,
            },
            Binding {
                directory: data,
                _pins: handles,
            },
        ))
    }
    pub fn list(root: &Path) -> Result<Vec<Entry>, String> {
        if !root.try_exists().map_err(err)? {
            return Ok(vec![]);
        }
        let _pins = pins(root, false)?;
        count(root)?;
        let mut result = vec![];
        for item in fs::read_dir(root).map_err(err)? {
            let item = item.map_err(err)?;
            let id = item.file_name().to_string_lossy().into_owned();
            result.push(match open(root, &id) {
                Ok((entry, _)) => entry,
                Err(error) => Entry {
                    id,
                    directory: item.path().join("data"),
                    ready: false,
                    error: Some(error),
                },
            });
        }
        result.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(result)
    }
}

#[cfg(all(test, windows))]
mod creation_tests;
#[cfg(all(test, windows))]
mod tests;
