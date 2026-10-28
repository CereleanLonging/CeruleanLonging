//! Portable sealed evidence sets, compatible with the offline Python v1 tool.
//! Never opens a Host, credentials, original payload paths, or an executor.
use crate::backup;
use cerulean_domain::execution::require;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path},
};

const KIND: &str = "cerulean-portable-evidence-v1";
const MAX_INDEX: u64 = 4 * 1024 * 1024;
const MAX_FILE: u64 = 64 * 1024 * 1024;
const MAX_TOTAL: u64 = 512 * 1024 * 1024;
const README: &[u8] = b"CeruleanLonging portable evidence v1\nVerify this folder using portable-evidence.py verify --archive <folder>\n--manifest-sha256 <digest saved separately at creation>.\nAll payload paths are relative to payload/. The source workspace is unnecessary.\nVerification reads bytes only. It does not execute bundled programs, resume tasks,\nrestore a workbench, attest provider reality, or approve a release.\nThe selected set may contain source code, logs, original paths and user content.\nKnown credential filenames are refused, but this is not content redaction.\nReview the selected content before sharing; creation never uploads anything.\n";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: String,
    bytes: u64,
    sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    schema: u32,
    kind: String,
    source_index_sha256: String,
    files: Vec<Entry>,
    readme_sha256: String,
    total_bytes: u64,
    execution_allowed: bool,
    restorable_workbench: bool,
    reality_attested: bool,
    release_accepted: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Verified {
    kind: String,
    manifest_sha256: String,
    source_index_sha256: String,
    files_verified: usize,
    bytes_verified: u64,
    source_workspace_required: bool,
    credentials_content_redacted: bool,
    execution_allowed: bool,
    restorable_workbench: bool,
    reality_attested: bool,
    release_accepted: bool,
}
fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn digest(value: &str) -> Result<(), String> {
    require(
        value.len() == 64
            && value
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c)),
        "需要另行保存的小写SHA256预期摘要",
    )
}
fn safe_name(name: &str) -> Result<Vec<&str>, String> {
    require(
        !name.is_empty()
            && name.len() <= 4096
            && !name
                .chars()
                .any(|c| c.is_control() || "\\:\"<>|?*".contains(c)),
        "归档相对路径无效",
    )?;
    let parts: Vec<_> = name.split('/').collect();
    require(parts.len() <= 64, "归档路径层数超限")?;
    for part in &parts {
        require(
            !part.is_empty() && !matches!(*part, "." | "..") && !part.ends_with([' ', '.']),
            "归档路径越界或别名",
        )?;
        let lower = part.to_lowercase();
        let base = lower.split('.').next().unwrap();
        let device = (base.starts_with("com") || base.starts_with("lpt"))
            && base.chars().count() == 4
            && "123456789¹²³".contains(base.chars().last().unwrap());
        require(
            !matches!(base, "con" | "prn" | "aux" | "nul" | "clock$") && !device,
            "归档不能包含Windows设备路径",
        )?;
        require(
            !matches!(
                lower.as_str(),
                "credentials"
                    | ".aws"
                    | ".ssh"
                    | ".gnupg"
                    | "auth.json"
                    | "credentials.json"
                    | "id_rsa"
                    | "id_ed25519"
            ) && !lower.starts_with(".env")
                && ![".pem", ".pfx", ".p12", ".key", ".dpapi"]
                    .iter()
                    .any(|s| lower.ends_with(s)),
            "拒绝已知凭证文件路径",
        )?;
    }
    Ok(parts)
}
fn inventory(files: &[Entry]) -> Result<u64, String> {
    require(
        !files.is_empty() && files.len() <= 10000,
        "归档文件数量无效",
    )?;
    let mut seen = BTreeSet::new();
    let mut spellings = BTreeMap::new();
    let mut total = 0;
    for file in files {
        let parts = safe_name(&file.path)?;
        digest(&file.sha256)?;
        require(
            seen.insert(file.path.to_lowercase()),
            "归档路径重复或大小写别名",
        )?;
        for end in 1..=parts.len() {
            let spelling = parts[..end].join("/");
            if let Some(previous) = spellings.insert(spelling.to_lowercase(), spelling.clone()) {
                require(previous == spelling, "归档目录大小写别名")?;
            }
        }
        require(file.bytes <= MAX_FILE, "归档单文件超过64MiB")?;
        total += file.bytes;
    }
    require(total <= MAX_TOTAL, "归档总量超过512MiB")?;
    for file in files {
        let parts = safe_name(&file.path)?;
        for end in 1..parts.len() {
            require(
                !seen.contains(&parts[..end].join("/").to_lowercase()),
                "归档文件与目录冲突",
            )?;
        }
    }
    Ok(total)
}
fn pin_directory(path: &Path) -> Result<Vec<File>, String> {
    require(
        path.is_absolute()
            && !path
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir)),
        "归档路径须为无相对段的绝对路径",
    )?;
    path.ancestors()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|p| backup::open(p, true, false).map_err(error))
        .collect()
}
fn read(path: &Path, maximum: u64) -> Result<Vec<u8>, String> {
    let _parents = pin_directory(path.parent().ok_or("证据文件缺少父目录")?)?;
    let mut file = backup::open(path, false, false).map_err(error)?;
    require(
        file.metadata().map_err(error)?.len() <= maximum,
        "证据文件超过读取上限",
    )?;
    let mut bytes = vec![];
    (&mut file)
        .take(maximum + 1)
        .read_to_end(&mut bytes)
        .map_err(error)?;
    require(bytes.len() as u64 <= maximum, "证据文件超过读取上限")?;
    Ok(bytes)
}
fn parse<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    // Do not echo unexpected JSON values; logs can contain user content.
    serde_json::from_slice(bytes).map_err(|_| "归档JSON字段、类型或版本不符".into())
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(error)?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(error)
}
pub fn create(root: &Path, index: &Path, expected: &str, to: &Path) -> Result<Verified, String> {
    digest(expected)?;
    let _source = pin_directory(root)?;
    require(
        to.is_absolute()
            && !to
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir)),
        "归档目标须为无相对段的绝对路径",
    )?;
    let parent = to.parent().ok_or("归档缺少父目录")?;
    let _parent = pin_directory(parent)?;
    let root = fs::canonicalize(root).map_err(error)?;
    let to = fs::canonicalize(parent)
        .map_err(error)?
        .join(to.file_name().ok_or("归档缺少目录名称")?);
    require(
        !to.starts_with(&root) && !root.starts_with(&to),
        "归档须位于源证据根目录之外",
    )?;
    require(
        matches!(fs::symlink_metadata(&to),Err(e) if e.kind()==std::io::ErrorKind::NotFound),
        "归档目标已存在或不可核查；保留原目录，不覆盖/重试",
    )?;
    let raw = read(index, MAX_INDEX)?;
    require(hash(&raw) == expected, "原冻结索引摘要变化")?;
    let files: Vec<Entry> = parse(&raw)?;
    let total = inventory(&files)?;
    let manifest = Manifest {
        schema: 1,
        kind: KIND.into(),
        source_index_sha256: expected.into(),
        files,
        readme_sha256: hash(README),
        total_bytes: total,
        execution_allowed: false,
        restorable_workbench: false,
        reality_attested: false,
        release_accepted: false,
    };
    let mut bytes = serde_json::to_vec_pretty(&manifest).map_err(error)?;
    bytes.push(b'\n');
    require(bytes.len() as u64 <= MAX_INDEX, "归档清单过大")?;
    fs::create_dir(&to).map_err(error)?;
    let _destination = backup::open(&to, true, false).map_err(error)?;
    write_new(&to.join("manifest.pending"), &bytes)?;
    let payload = to.join("payload");
    fs::create_dir(&payload).map_err(error)?;
    write_new(&to.join("README.txt"), README)?;
    for item in &manifest.files {
        let content = read(&root.join(&item.path), MAX_FILE)?;
        require(
            content.len() as u64 == item.bytes && hash(&content) == item.sha256,
            "所选证据内容已变化；保留不完整归档",
        )?;
        let target = payload.join(&item.path);
        let parent = target.parent().unwrap();
        // Pin each new directory before descending; never follow a concurrently supplied link.
        let mut handles = vec![backup::open(&payload, true, false).map_err(error)?];
        let mut directory = payload.clone();
        let parts = safe_name(&item.path)?;
        for part in &parts[..parts.len() - 1] {
            directory.push(part);
            match fs::create_dir(&directory) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
                Err(e) => return Err(error(e)),
            }
            handles.push(backup::open(&directory, true, false).map_err(error)?);
        }
        require(parent == directory, "归档路径不一致")?;
        write_new(&target, &content)?;
    }
    backup::publish(&to.join("manifest.pending"), &to.join("manifest.json")).map_err(error)?;
    verify(&to, &hash(&bytes))
}
pub fn verify(archive: &Path, expected: &str) -> Result<Verified, String> {
    digest(expected)?;
    let _root = pin_directory(archive)?;
    let names = fs::read_dir(archive)
        .map_err(error)?
        .take(4)
        .map(|e| e.map(|v| v.file_name()).map_err(error))
        .collect::<Result<BTreeSet<_>, _>>()?;
    require(
        names
            == ["manifest.json", "README.txt", "payload"]
                .into_iter()
                .map(std::ffi::OsString::from)
                .collect(),
        "归档未完成或含额外根条目",
    )?;
    let raw = read(&archive.join("manifest.json"), MAX_INDEX)?;
    require(hash(&raw) == expected, "清单与另存的预期摘要不符")?;
    let m: Manifest = parse(&raw)?;
    require(
        m.schema == 1
            && m.kind == KIND
            && !m.execution_allowed
            && !m.restorable_workbench
            && !m.reality_attested
            && !m.release_accepted,
        "归档版本无效或冒充执行/验收授权",
    )?;
    digest(&m.source_index_sha256)?;
    let total = inventory(&m.files)?;
    require(total == m.total_bytes, "归档总量不符")?;
    require(
        hash(&read(&archive.join("README.txt"), 8192)?) == m.readme_sha256
            && m.readme_sha256 == hash(README),
        "归档使用说明变化",
    )?;
    let payload = archive.join("payload");
    let _payload = pin_directory(&payload)?;
    let expected_files = m
        .files
        .iter()
        .map(|f| f.path.clone())
        .collect::<BTreeSet<_>>();
    let mut expected_dirs = BTreeSet::new();
    for file in &m.files {
        let parts = safe_name(&file.path)?;
        for end in 1..parts.len() {
            expected_dirs.insert(parts[..end].join("/"));
        }
    }
    let mut files = BTreeSet::new();
    let mut dirs = BTreeSet::new();
    walk(
        &payload,
        "",
        &expected_files,
        &expected_dirs,
        &mut files,
        &mut dirs,
    )?;
    require(
        files == expected_files && dirs == expected_dirs,
        "归档文件/目录清单不符",
    )?;
    for item in &m.files {
        let bytes = read(&payload.join(&item.path), MAX_FILE)?;
        require(
            bytes.len() as u64 == item.bytes && hash(&bytes) == item.sha256,
            "归档载荷大小或摘要不符",
        )?;
    }
    Ok(Verified {
        kind: KIND.into(),
        manifest_sha256: expected.into(),
        source_index_sha256: m.source_index_sha256,
        files_verified: m.files.len(),
        bytes_verified: total,
        source_workspace_required: false,
        credentials_content_redacted: false,
        execution_allowed: false,
        restorable_workbench: false,
        reality_attested: false,
        release_accepted: false,
    })
}
fn walk(
    root: &Path,
    relative: &str,
    expected_files: &BTreeSet<String>,
    expected_dirs: &BTreeSet<String>,
    files: &mut BTreeSet<String>,
    dirs: &mut BTreeSet<String>,
) -> Result<(), String> {
    let _pin = backup::open(root, true, false).map_err(error)?;
    for entry in fs::read_dir(root).map_err(error)? {
        let entry = entry.map_err(error)?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "归档文件名不是UTF-8")?;
        let relative = if relative.is_empty() {
            name
        } else {
            format!("{relative}/{name}")
        };
        safe_name(&relative)?;
        if entry.file_type().map_err(error)?.is_dir() {
            require(expected_dirs.contains(&relative), "归档含额外目录")?;
            dirs.insert(relative.clone());
            walk(
                &entry.path(),
                &relative,
                expected_files,
                expected_dirs,
                files,
                dirs,
            )?;
        } else {
            require(expected_files.contains(&relative), "归档含额外文件")?;
            let _file = backup::open(&entry.path(), false, false).map_err(error)?;
            files.insert(relative);
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests;
