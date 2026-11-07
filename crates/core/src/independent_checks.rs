//! One-way host-owned archive for independently checked evidence. The bound
//! offline child validates records; the core persists and rereads its receipt.
use cerulean_domain::{
    execution::{digest, require, AcceptanceCheck, ArtifactManifest, VerificationReport},
    independent_checks::{CheckArchive, CheckRecord},
    independent_verifier::{Operation, Output},
};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};

pub(crate) fn inspect(
    project: &str,
    operation: &str,
    manifest: &ArtifactManifest,
    checks: &[AcceptanceCheck],
    report: &VerificationReport,
) -> Result<CheckRecord, String> {
    match crate::objective_checker::inspect(Operation::InspectChecks {
        project: project.into(),
        operation: operation.into(),
        manifest: manifest.clone(),
        approved: checks.to_vec(),
        report: report.clone(),
    })? {
        Output::ChecksInspected { record } => {
            require(
                record
                    == cerulean_domain::independent_checks::inspect(
                        project, operation, manifest, checks, report,
                    )?,
                "独立检查程序返回了不一致的事实",
            )?;
            Ok(record)
        }
        _ => Err("独立检查响应类型无效".into()),
    }
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
    let file = crate::backup::open(path, false, false).map_err(|e| e.to_string())?;
    let mut bytes = vec![];
    file.take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    require(bytes.len() <= 1024 * 1024, "检查归档过大")?;
    Ok(bytes)
}

// A crash between flush and publication can finish the same exact immutable
// write. A conflicting pending record is never replaced and tests never rerun.
pub(crate) fn persist(
    folder: &Path,
    name: &str,
    value: &impl serde::Serialize,
) -> Result<(), String> {
    crate::host::ordinary(folder, true)?;
    cerulean_domain::execution::identifier(name)?;
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    require(bytes.len() <= 1024 * 1024, "检查归档过大")?;
    let target = folder.join(format!("{name}.json"));
    let pending = folder.join(format!("{name}.pending"));
    if pending.exists() {
        require(read(&pending)? == bytes, "检查归档待写入记录冲突")?;
    }
    if target.exists() {
        return require(read(&target)? == bytes, "检查归档不可覆盖");
    }
    if !pending.exists() {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&pending)
            .map_err(|e| e.to_string())?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
    }
    // Publish without replacement or a second hard link. The protected reader
    // rejects multiply linked files, including our own temporary aliases.
    crate::backup::publish(&pending, &target).map_err(|e| e.to_string())?;
    require(read(&target)? == bytes, "检查归档回读不一致")
}

pub(crate) fn archive(root: &Path, archive: &CheckArchive) -> Result<String, String> {
    let expected = digest(archive)?;
    match crate::independent_verifier::call(Operation::ArchiveChecks {
        archive: archive.clone(),
    })? {
        Output::CheckArchiveValidated {
            digest,
            basis_points,
        } => {
            require(
                digest == expected && basis_points == archive.basis_points()?,
                "验收工具归档核对失败",
            )?;
        }
        _ => return Err("验收工具归档响应无效".into()),
    }
    let folder = root.join("independent-check-archive");
    crate::windows_worker_account::private_directory(
        &folder,
        &crate::ipc::current_sid().map_err(|e| e.to_string())?,
    )?;
    persist(&folder, &expected, archive)?;
    require_archived(root, archive)?;
    Ok(expected)
}
pub(crate) fn require_archived(root: &Path, archive: &CheckArchive) -> Result<(), String> {
    archive.validate()?;
    let path = root
        .join("independent-check-archive")
        .join(format!("{}.json", digest(archive)?));
    let saved: CheckArchive = serde_json::from_slice(&read(&path)?).map_err(|_| "检查归档损坏")?;
    require(saved == *archive, "检查归档与本轮验收不一致")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_commit_recovers_exact_pending_and_rejects_conflicts() {
        let temp = tempfile::tempdir().unwrap();
        let value = serde_json::json!({"report":"approved"});
        fs::write(
            temp.path().join("record.pending"),
            serde_json::to_vec(&value).unwrap(),
        )
        .unwrap();
        persist(temp.path(), "record", &value).unwrap();
        assert!(!temp.path().join("record.pending").exists());
        assert_eq!(
            read(&temp.path().join("record.json")).unwrap(),
            serde_json::to_vec(&value).unwrap()
        );
        persist(temp.path(), "record", &value).unwrap();
        assert!(persist(
            temp.path(),
            "record",
            &serde_json::json!({"report":"changed"})
        )
        .is_err());
        fs::write(temp.path().join("record.pending"), b"different").unwrap();
        assert!(persist(temp.path(), "record", &value).is_err());
    }
}
