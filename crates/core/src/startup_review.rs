//! User review of an unpublished, truncated startup intent. Retain its bytes
//! and permanently reserve its ID; never manufacture a valid launch intent.
use crate::host::ordinary;
use cerulean_domain::{
    execution::{artifact_path, digest, identifier, require, timestamp},
    StartupReservation,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
const LIMIT: usize = 65536;
const FOLDER: &str = "startup-intent-reviews";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Receipt {
    version: u32,
    relative_path: String,
    evidence_digest: String,
    reason: String,
    at: String,
    hash: String,
}

fn parts(relative: &str) -> Result<(&str, &str, &str), String> {
    let p: Vec<_> = relative.split('/').collect();
    require(
        p.len() == 3 && matches!(p[0], "codex-startups" | "omp-startups"),
        "不支持的启动复核路径",
    )?;
    let attempt = p[2]
        .strip_suffix(".0.pending")
        .ok_or("仅支持未发布启动意图，关闭记录需另行恢复")?;
    for id in [p[1], attempt] {
        identifier(id)?;
        artifact_path(id)?;
    }
    Ok((p[0], p[1], attempt))
}
pub(crate) fn bytes(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
        Ok(_) => ordinary(path, false)?,
    }
    let mut data = vec![];
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    require(data.len() <= LIMIT, "启动复核文件过大")?;
    Ok(Some(data))
}
pub(crate) fn reason(text: &str) -> Result<(), String> {
    require(
        !text.trim().is_empty() && text.len() <= 8000 && !text.contains('\0'),
        "请提供有效的启动复核说明",
    )
}
fn path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    parts(relative)?;
    let folder = root.join(FOLDER);
    match fs::symlink_metadata(&folder) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
        Ok(_) => ordinary(&folder, true)?,
    }
    Ok(folder.join(format!("{}.json", digest(&relative)?)))
}
fn fingerprint(root: &Path, relative: &str) -> Result<String, String> {
    let (executor, project, attempt) = parts(relative)?;
    ordinary(root, true)?;
    let folder = root.join(executor);
    ordinary(&folder, true)?;
    let dir = folder.join(project);
    ordinary(&dir, true)?;
    // Reject Windows case aliases before reading evidence under that name.
    require(
        fs::read_dir(&folder)
            .map_err(|e| e.to_string())?
            .any(|e| e.is_ok_and(|e| e.file_name() == project)),
        "启动项目目录大小写不符",
    )?;
    for suffix in ["0.json", "1.json", "1.pending"] {
        let other = dir.join(format!("{attempt}.{suffix}"));
        require(
            bytes(&other)?.is_none(),
            "存在正式启动或关闭证据，不能隔离为半写意图",
        )?;
    }
    let name = format!("{attempt}.0.pending");
    require(
        fs::read_dir(&dir)
            .map_err(|e| e.to_string())?
            .any(|e| e.is_ok_and(|e| e.file_name() == name.as_str())),
        "启动文件大小写不符",
    )?;
    let raw = bytes(&dir.join(name))?.ok_or("半写启动意图已缺失")?;
    require(
        serde_json::from_slice::<serde_json::Value>(&raw).is_err_and(|e| e.is_eof()),
        "只能复核空或未写完整的启动意图",
    )?;
    digest(&(
        1,
        relative,
        format!("{:x}", Sha256::digest(&raw)),
        raw.len(),
        "intent-and-closure-unpublished",
    ))
}
fn verify(root: &Path, r: &Receipt) -> Result<StartupReservation, String> {
    let (executor, project, attempt) = parts(&r.relative_path)?;
    reason(&r.reason)?;
    timestamp(&r.at)?;
    require(
        r.version == 1
            && r.evidence_digest == fingerprint(root, &r.relative_path)?
            && r.hash
                == digest(&(
                    r.version,
                    &r.relative_path,
                    &r.evidence_digest,
                    &r.reason,
                    &r.at,
                ))?,
        "启动复核凭据与原文不符",
    )?;
    Ok(StartupReservation {
        executor: executor.strip_suffix("-startups").unwrap().into(),
        project_id: project.into(),
        attempt_id: attempt.into(),
        reason: r.reason.clone(),
        at: r.at.clone(),
    })
}
pub(crate) fn inspect(root: &Path, relative: &str) -> Result<(String, bool), String> {
    let evidence = fingerprint(root, relative)?;
    let reviewed = if let Some(raw) = bytes(&path(root, relative)?)? {
        let r: Receipt = serde_json::from_slice(&raw).map_err(|_| "启动复核凭据损坏")?;
        require(r.relative_path == relative, "启动复核路径不符")?;
        verify(root, &r)?;
        true
    } else {
        false
    };
    Ok((evidence, reviewed))
}
pub(crate) fn load(root: &Path) -> Result<Vec<StartupReservation>, String> {
    let dir = root.join(FOLDER);
    match fs::symlink_metadata(&dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.to_string()),
        Ok(_) => ordinary(&dir, true)?,
    }
    let files = fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .take(10001)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    require(files.len() <= 10000, "启动复核目录已满")?;
    let mut result = vec![];
    for file in files {
        ordinary(&file.path(), false)?;
        if file.path().extension().is_some_and(|x| x == "pending") {
            continue;
        }
        require(
            file.path().extension().is_some_and(|x| x == "json"),
            "未知启动复核文件",
        )?;
        let r: Receipt = serde_json::from_slice(&bytes(&file.path())?.ok_or("启动复核消失")?)
            .map_err(|_| "启动复核凭据损坏")?;
        require(
            file.path() == path(root, &r.relative_path)?,
            "启动复核凭据名称不符",
        )?;
        result.push(verify(root, &r)?);
    }
    result.sort_by(|a, b| {
        (&a.executor, &a.project_id, &a.attempt_id).cmp(&(
            &b.executor,
            &b.project_id,
            &b.attempt_id,
        ))
    });
    Ok(result)
}
pub(crate) fn acknowledge(
    root: &Path,
    relative: &str,
    expected: &str,
    text: &str,
    mut observe: impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    reason(text)?;
    let (current, reviewed) = inspect(root, relative)?;
    require(current == expected, "启动证据已变化，请刷新后重新复核")?;
    if reviewed {
        return Ok(());
    }
    let mut r = Receipt {
        version: 1,
        relative_path: relative.into(),
        evidence_digest: expected.into(),
        reason: text.into(),
        at: chrono::Utc::now().to_rfc3339(),
        hash: String::new(),
    };
    r.hash = digest(&(
        r.version,
        &r.relative_path,
        &r.evidence_digest,
        &r.reason,
        &r.at,
    ))?;
    let raw = serde_json::to_vec(&r).map_err(|e| e.to_string())?;
    require(raw.len() <= LIMIT, "启动复核记录过大")?;
    let final_path = path(root, relative)?;
    let dir = final_path.parent().unwrap();
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    ordinary(dir, true)?;
    require(
        fs::read_dir(dir)
            .map_err(|e| e.to_string())?
            .take(10000)
            .count()
            < 10000,
        "启动复核目录已满",
    )?;
    let pending = final_path.with_extension(format!("{}.pending", r.hash));
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&pending)
        .map_err(|e| e.to_string())?;
    observe("created")?;
    f.write_all(&raw[..raw.len() / 2])
        .map_err(|e| e.to_string())?;
    observe("partial")?;
    f.write_all(&raw[raw.len() / 2..])
        .and_then(|_| f.sync_all())
        .map_err(|e| e.to_string())?;
    drop(f);
    observe("synced")?;
    require(
        fingerprint(root, relative)? == expected,
        "发布前启动原文已变化",
    )?;
    fs::hard_link(&pending, &final_path).map_err(|e| e.to_string())?;
    observe("published")?;
    fs::remove_file(pending).map_err(|e| e.to_string())?;
    observe("finished")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(root: &Path) -> String {
        let relative = "omp-startups/project/attempt.0.pending";
        fs::create_dir_all(root.join("omp-startups/project")).unwrap();
        fs::write(root.join(relative), b"{\"body\":").unwrap();
        relative.into()
    }
    #[test]
    fn review_preserves_evidence_and_detects_changed_missing_or_foreign_receipts() {
        let root = tempfile::tempdir().unwrap();
        let relative = fixture(root.path());
        let original = fs::read(root.path().join(&relative)).unwrap();
        let evidence = inspect(root.path(), &relative).unwrap().0;
        assert!(acknowledge(root.path(), &relative, &"0".repeat(64), "stale", |_| Ok(())).is_err());
        acknowledge(root.path(), &relative, &evidence, "reviewed", |_| Ok(())).unwrap();
        acknowledge(
            root.path(),
            &relative,
            &evidence,
            "do not replace reason",
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(load(root.path()).unwrap()[0].reason, "reviewed");
        assert_eq!(fs::read(root.path().join(&relative)).unwrap(), original);
        fs::write(root.path().join(&relative), b"{").unwrap();
        assert!(load(root.path()).is_err());
        fs::remove_file(root.path().join(&relative)).unwrap();
        assert!(load(root.path()).is_err());
        assert!(inspect(root.path(), "../project/attempt.0.pending").is_err());
        assert!(inspect(root.path(), "omp-startups/project/attempt.1.pending").is_err());
    }
    #[test]
    fn review_never_accepts_complete_or_conflicting_intent_and_publication_requires_confirmation() {
        for point in ["created", "partial", "synced", "published", "finished"] {
            let root = tempfile::tempdir().unwrap();
            let relative = fixture(root.path());
            let evidence = inspect(root.path(), &relative).unwrap().0;
            assert!(
                acknowledge(root.path(), &relative, &evidence, "first", |p| {
                    if p == point {
                        Err("interrupted".into())
                    } else {
                        Ok(())
                    }
                })
                .is_err()
            );
            assert_eq!(
                load(root.path()).unwrap().len(),
                usize::from(matches!(point, "published" | "finished"))
            );
            acknowledge(root.path(), &relative, &evidence, "explicit retry", |_| {
                Ok(())
            })
            .unwrap();
            assert_eq!(load(root.path()).unwrap().len(), 1);
        }
        let root = tempfile::tempdir().unwrap();
        let relative = fixture(root.path());
        fs::write(root.path().join(&relative), b"{}").unwrap();
        assert!(inspect(root.path(), &relative).is_err());
        fs::write(root.path().join(&relative), b"{").unwrap();
        fs::write(
            root.path().join("omp-startups/project/attempt.0.json"),
            b"{",
        )
        .unwrap();
        assert!(inspect(root.path(), &relative).is_err());
    }
}
