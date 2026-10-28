//! Coordinate pin acquisition with our background projection renames on Windows.
//! A rename briefly opens its parent for writing, which conflicts with a strict
//! FILE_SHARE_READ directory open. Serialize only those syscalls, not the pin's
//! lifetime: retained pins must still prevent replacement, and an external
//! holder must still fail closed. This never retries a command or file action.
use std::{io, path::Path};

#[cfg(windows)]
mod windows {
    use super::*;
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex, OnceLock, Weak},
    };

    type Gate = Mutex<()>;
    static GATES: OnceLock<Mutex<BTreeMap<String, Weak<Gate>>>> = OnceLock::new();

    fn gate(directory: &Path) -> io::Result<Arc<Gate>> {
        // This key is for coordination only, never authorization. The caller
        // still opens with OPEN_REPARSE_POINT and checks the actual handle.
        let key = std::fs::canonicalize(directory)?
            .to_string_lossy()
            .to_lowercase();
        let mut gates = GATES
            .get_or_init(Default::default)
            .lock()
            .map_err(|_| io::Error::other("directory publication registry poisoned"))?;
        gates.retain(|_, gate| gate.strong_count() != 0);
        if let Some(gate) = gates.get(&key).and_then(Weak::upgrade) {
            return Ok(gate);
        }
        let gate = Arc::new(Mutex::new(()));
        gates.insert(key, Arc::downgrade(&gate));
        Ok(gate)
    }

    pub(super) fn serialize<T>(
        directory: &Path,
        action: impl FnOnce() -> io::Result<T>,
    ) -> io::Result<T> {
        let gate = gate(directory)?;
        let _guard = gate
            .lock()
            .map_err(|_| io::Error::other("directory publication gate poisoned"))?;
        action()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn directory_publication_keys_aliases_and_releases_unused_entries() {
            let root = tempfile::tempdir().unwrap();
            let other = root.path().join("other");
            std::fs::create_dir(&other).unwrap();
            let first = gate(root.path()).unwrap();
            let alias = gate(&std::fs::canonicalize(root.path()).unwrap()).unwrap();
            assert!(Arc::ptr_eq(&first, &alias));
            assert!(!Arc::ptr_eq(&first, &gate(&other).unwrap()));
            let weak = Arc::downgrade(&first);
            drop(first);
            drop(alias);
            assert!(weak.upgrade().is_none());
        }
    }
}

#[cfg(windows)]
pub(crate) fn pin<T>(directory: &Path, open: impl FnOnce() -> io::Result<T>) -> io::Result<T> {
    windows::serialize(directory, open)
}

/// Only for derived files staged in the same directory as their destination.
/// Existing validation and any bounded projection retry remain with the caller.
pub(crate) fn replace(temp: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(windows)]
    {
        let parent = destination
            .parent()
            .ok_or_else(|| io::Error::other("projection parent missing"))?;
        if temp.parent() != Some(parent) {
            return Err(io::Error::other(
                "projection temp must share destination parent",
            ));
        }
        windows::serialize(parent, || std::fs::rename(temp, destination))
    }
    #[cfg(not(windows))]
    std::fs::rename(temp, destination)
}
