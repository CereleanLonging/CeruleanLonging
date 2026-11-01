//! Bounded raw evidence identity, including absence/corruption. No repair,
//! deletion, validation execution, or reinterpretation as a successful result.
use crate::host::ordinary;
use cerulean_domain::{execution::*, verification_recheck::RecheckApproval};
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path};

fn file(path: &Path, limit: u64) -> Result<Option<String>, String> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
        Ok(_) => ordinary(path, false)?,
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    require(bytes.len() as u64 <= limit, "复验处置证据文件过大")?;
    Ok(Some(format!("{:x}", Sha256::digest(bytes))))
}
pub(crate) fn fingerprint(root: &Path, a: &RecheckApproval) -> Result<String, String> {
    let parent = root
        .join("workflows")
        .join(&a.review.project_id)
        .join("verification");
    crate::artifacts::root_path(parent.to_str().ok_or("invalid recheck evidence root")?)?;
    let dir = parent.join(&a.operation_id);
    let present = match fs::symlink_metadata(&dir) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(e.to_string()),
        Ok(_) => {
            ordinary(&dir, true)?;
            true
        }
    };
    let mut names = vec![
        ("intent.json".to_string(), 1024 * 1024),
        ("result.json".into(), 1024 * 1024),
        ("snapshot.json".into(), 8 * 1024 * 1024),
        ("abandonment.json".into(), 65536),
    ];
    for stage in &a.review.replacement_checks {
        identifier(&stage.id)?;
        for suffix in ["json", "process-exit.json", "stdout", "stderr"] {
            names.push((format!("{}.{suffix}", stage.id), 1024 * 1024));
        }
    }
    let mut hashes = Vec::new();
    for (name, limit) in names {
        hashes.push((
            name.clone(),
            if present {
                file(&dir.join(name), limit)?
            } else {
                None
            },
        ));
    }
    let claim =
        crate::async_verification_recheck::directory(root, &a.review.project_id, &a.operation_id);
    for path in [
        root.join("verification-recheck-jobs"),
        root.join("verification-recheck-jobs")
            .join(&a.review.project_id),
        claim.clone(),
    ] {
        match fs::symlink_metadata(&path) {
            Ok(_) => ordinary(&path, true)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.to_string()),
        }
    }
    // Only immutable launch authority; job.json is a derived projection and is
    // allowed to change when the durable disposition is applied or reopened.
    let launch = file(&claim.join("intent.json"), 65536)?;
    digest(&("recheck-disposition-evidence-v1", present, launch, hashes))
}
