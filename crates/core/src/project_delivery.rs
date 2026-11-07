//! Assemble only authenticated, checked snapshots. No executor receives access
//! to another executor's live directory or to the core's delivery archive.
use crate::source_snapshot::FrozenSnapshot;
use cerulean_domain::{execution::*, project_delivery::*};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub(crate) fn assemble(
    plan: &PlanRevision,
    setup: &str,
    mut snapshots: Vec<(String, String, String, FrozenSnapshot)>,
    inputs: Vec<DeliveryInput>,
) -> Result<(DeliveryManifest, Vec<(String, Vec<u8>)>), String> {
    require(plan.integration().is_none(), "显式集成方案应沿用原集成任务")?;
    let manifest = DeliveryManifest::assemble(
        plan.project.id.clone(),
        plan.approval_digest()?,
        setup.into(),
        inputs,
    )?;
    let mut latest = BTreeMap::new();
    for input in &manifest.inputs {
        let previous = latest.entry(&input.identity).or_insert(input);
        if input.acceptance.sequence > previous.acceptance.sequence {
            *previous = input;
        }
    }
    let policy = plan.snapshot_policy.as_ref().ok_or("源码封存范围缺失")?;
    for input in latest.values() {
        let (_, root, _, frozen) = snapshots
            .iter()
            .find(|(task, ..)| task == &input.manifest.task_id)
            .ok_or("汇总来源封存缺失")?;
        require(
            crate::source_snapshot::observe(Path::new(root), policy)? == frozen.manifest().tree,
            "执行端成果在检查后发生变化，请重新检查",
        )?;
    }
    let mut files = Vec::new();
    for file in &manifest.files {
        let (_, _, _, frozen) = snapshots
            .iter_mut()
            .find(|(task, ..)| task == &file.source_task)
            .ok_or("汇总文件来源缺失")?;
        files.push((
            file.artifact.path.clone(),
            frozen.read_file(&file.artifact.path)?,
        ));
    }
    Ok((manifest, files))
}

pub(crate) fn source_path(root: &Path, manifest: &DeliveryManifest) -> Result<PathBuf, String> {
    manifest.validate()?;
    Ok(root
        .join("project-deliveries-v1")
        .join(digest(manifest)?)
        .join("source"))
}

pub(crate) fn require_source(root: &Path, manifest: &DeliveryManifest) -> Result<PathBuf, String> {
    let source = source_path(root, manifest)?;
    let source = crate::artifacts::root_path(source.to_str().ok_or("汇总路径编码无效")?)?;
    let policy = SnapshotPolicy {
        exclude: vec![],
        max_files: 128,
        max_bytes: 96 * 1024,
    };
    let expected = crate::source_snapshot::Tree {
        directories: manifest.directories.clone(),
        files: manifest.files.iter().map(|f| f.artifact.clone()).collect(),
    };
    require(
        crate::source_snapshot::observe(&source, &policy)? == expected,
        "汇总只读交付副本已变化",
    )?;
    let folder = source.parent().ok_or("汇总目录缺失")?;
    let mut bytes = Vec::new();
    crate::backup::open(&folder.join("manifest.json"), false, false)
        .map_err(|e| e.to_string())?
        .take(512 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    require(
        bytes.len() <= 512 * 1024
            && bytes == serde_json::to_vec(manifest).map_err(|e| e.to_string())?,
        "汇总交付清单缺失或变化",
    )?;
    Ok(source)
}

pub(crate) fn persist(
    root: &Path,
    manifest: &DeliveryManifest,
    files: &[(String, Vec<u8>)],
) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    manifest.validate()?;
    require(files.len() == manifest.files.len(), "汇总源码数量不符")?;
    let source = source_path(root, manifest)?;
    let folder = source.parent().ok_or("汇总目录缺失")?;
    if folder.join("manifest.json").exists() {
        require_source(root, manifest)?;
        return Ok(());
    }
    let container = folder.parent().ok_or("汇总容器缺失")?;
    let owner = crate::ipc::current_sid().map_err(|e| e.to_string())?;
    for path in [container, folder, source.as_path()] {
        crate::windows_worker_account::private_directory(path, &owner)?;
    }
    let source = crate::artifacts::root_path(source.to_str().ok_or("汇总路径编码无效")?)?;
    for directory in &manifest.directories {
        fs::create_dir_all(source.join(directory)).map_err(|e| e.to_string())?;
        crate::artifacts::root_path(source.join(directory).to_str().ok_or("汇总路径编码无效")?)?;
    }
    for ((name, bytes), file) in files.iter().zip(&manifest.files) {
        require(
            name == &file.artifact.path
                && bytes.len() == file.artifact.bytes as usize
                && format!("{:x}", Sha256::digest(bytes)) == file.artifact.sha256,
            "汇总源码与检查封存不符",
        )?;
        let path = source.join(name);
        if path.exists() {
            require(
                crate::artifacts::read_artifact(&source, name)? == file.artifact,
                "汇总交付文件冲突",
            )?;
            continue;
        }
        // Partial publication remains incomplete and cannot enter a review.
        let mut output = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        output
            .write_all(bytes)
            .and_then(|_| output.sync_all())
            .map_err(|e| e.to_string())?;
        drop(output);
        let mut permissions = fs::metadata(&path)
            .map_err(|e| e.to_string())?
            .permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&path, permissions).map_err(|e| e.to_string())?;
    }
    crate::independent_checks::persist(folder, "manifest", manifest)?;
    require_source(root, manifest)?;
    Ok(())
}
