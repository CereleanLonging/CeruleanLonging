//! Only replace rebuildable projections. Never retry an authoritative event,
//! command, model request or tool operation after a lost acknowledgement.
use super::{require, safe_metadata, temp_path};
use crate::handoff::DailyVolume;
use cerulean_domain::workflow::WorkflowState;
use std::{
    fs,
    io::Write,
    path::Path,
    time::{Duration, Instant},
};

/// Data derived only from already committed, verified events. Never replayed.
#[derive(serde::Deserialize)]
pub(crate) struct Snapshot {
    pub sequence: usize,
    pub last_hash: String,
    pub state: WorkflowState,
    pub daily: Vec<DailyVolume>,
}
#[derive(serde::Serialize)]
pub(crate) struct SnapshotRef<'a> {
    pub sequence: usize,
    pub last_hash: &'a str,
    pub state: &'a WorkflowState,
    pub daily: &'a [DailyVolume],
}
impl Snapshot {
    pub fn publish(self, root: &Path) -> Result<(), String> {
        let volumes = &self.daily;
        // Validate each directory component before traversal; never follow a junction.
        let daily = root.join("daily");
        if !daily.exists() {
            fs::create_dir(&daily).map_err(|e| e.to_string())?;
        }
        require(safe_metadata(&daily)?.is_dir(), "invalid daily directory")?;
        for volume in volumes {
            let day = daily.join(&volume.day);
            if !day.exists() {
                fs::create_dir(&day).map_err(|e| e.to_string())?;
            }
            require(safe_metadata(&day)?.is_dir(), "invalid daily day directory")?;
            let destination = root.join(&volume.path);
            if destination.exists() {
                require(
                    safe_metadata(&destination)?.is_file(),
                    "invalid daily projection",
                )?;
                // Sealed volumes stay byte-for-byte unchanged during normal append.
                if fs::metadata(&destination).map_err(|e| e.to_string())?.len()
                    == volume.markdown.len() as u64
                    && fs::read(&destination).map_err(|e| e.to_string())?
                        == volume.markdown.as_bytes()
                {
                    continue;
                }
            }
            publish_bytes(&destination, volume.markdown.as_bytes())?;
        }
        let destination = root.join("state.json");
        if destination.exists() {
            require(
                safe_metadata(&destination)?.is_file(),
                "invalid projection path",
            )?;
        }
        let daily_index: Vec<_> = volumes.iter().map(|v| serde_json::json!({"path":v.path,"day":v.day,"number":v.number,"sealed":v.sealed,"eventIds":v.event_ids})).collect();
        let value = serde_json::json!({"schemaVersion":1,"sequence":self.sequence,"lastHash":self.last_hash,"state":self.state,"daily":daily_index});
        publish_bytes(
            &destination,
            &serde_json::to_vec(&value).map_err(|e| e.to_string())?,
        )
    }
}

fn publish_bytes(destination: &Path, bytes: &[u8]) -> Result<(), String> {
    let temp = temp_path(destination);
    // Only a successfully created, owned temporary file may be cleaned up.
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    let written = file
        .write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string());
    drop(file);
    let result = written.and_then(|_| publish(&temp, destination));
    if let Err(error) = result {
        if let Err(cleanup) = fs::remove_file(&temp) {
            return Err(format!(
                "{error}; owned projection temp cleanup failed: {cleanup}"
            ));
        }
        return Err(error);
    }
    Ok(())
}

pub(crate) fn publish(temp: &Path, destination: &Path) -> Result<(), String> {
    replace(
        temp,
        destination,
        Instant::now() + Duration::from_secs(1),
        || {},
    )
}

fn replace(
    temp: &Path,
    destination: &Path,
    deadline: Instant,
    mut blocked: impl FnMut(),
) -> Result<(), String> {
    loop {
        // Recheck the existing destination on every attempt. Keep the same
        // synced source and the old projection until atomic replacement succeeds.
        match fs::symlink_metadata(destination) {
            Ok(_) => require(
                safe_metadata(destination)?.is_file(),
                "invalid projection path",
            )?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("projection inspection failed: {e}")),
        }
        match fs::rename(temp, destination) {
            Ok(()) => return Ok(()),
            Err(e)
                if cfg!(windows) && e.raw_os_error() == Some(32) && Instant::now() < deadline =>
            {
                blocked();
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => {
                return Err(format!(
                    "projection replacement failed ({}): {e}",
                    destination.display()
                ))
            }
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt};

    #[test]
    fn real_sharing_conflict_preserves_old_projection_until_release_or_timeout() {
        let root = tempfile::tempdir().unwrap();
        let destination = root.path().join("state.json");
        let temp = root.path().join("state.tmp");
        fs::write(&destination, b"old").unwrap();
        fs::write(&temp, b"next").unwrap();
        let mut reader = Some(
            OpenOptions::new()
                .read(true)
                .share_mode(3)
                .open(&temp)
                .unwrap(),
        );
        let mut conflicts = 0;
        replace(
            &temp,
            &destination,
            Instant::now() + Duration::from_secs(1),
            || {
                conflicts += 1;
                assert_eq!(fs::read(&destination).unwrap(), b"old");
                assert_eq!(fs::read(&temp).unwrap(), b"next");
                drop(reader.take());
            },
        )
        .unwrap();
        assert!(conflicts > 0);
        assert_eq!(fs::read(&destination).unwrap(), b"next");
        assert!(!temp.exists());

        fs::write(&temp, b"later").unwrap();
        let _held = OpenOptions::new()
            .read(true)
            .share_mode(3)
            .open(&temp)
            .unwrap();
        let error = replace(&temp, &destination, Instant::now(), || {
            panic!("deadline passed")
        })
        .unwrap_err();
        assert!(error.contains("os error 32"), "{error}");
        assert_eq!(fs::read(&destination).unwrap(), b"next");
        assert_eq!(fs::read(&temp).unwrap(), b"later");
        // Ordinary invalid paths are rejected, with no contention callback.
        let directory = root.path().join("invalid");
        fs::create_dir(&directory).unwrap();
        assert!(replace(
            &temp,
            &directory,
            Instant::now() + Duration::from_secs(1),
            || panic!("not contention")
        )
        .is_err());
    }
}
