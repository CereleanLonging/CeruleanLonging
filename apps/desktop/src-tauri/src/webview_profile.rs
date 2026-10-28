//! Disposable WebView state is per process; the Rust core owns durable business data.
use std::{
    fs::{self, File, Metadata, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const MARKER: &str = "CeruleanLonging WebView cache v1";
const LEASE: &str = ".cerulean-owner";
const MAX_ENTRIES: usize = 10_000;
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub struct Profile {
    pub directory: PathBuf,
    // Holding the file keeps the OS lock until this desktop process exits.
    _lease: File,
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn is_link(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

fn directory_without_links(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || is_link(&metadata) {
        return Err(invalid("WebView cache path is not a plain directory"));
    }
    Ok(())
}

fn prepare_root(path: &Path) -> io::Result<PathBuf> {
    if !path.is_absolute() {
        return Err(invalid("WebView cache root must be absolute"));
    }
    for ancestor in path.ancestors().collect::<Vec<_>>().into_iter().rev() {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => (),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match fs::create_dir(ancestor) {
                    Ok(()) => (),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => (),
                    Err(error) => return Err(error),
                }
            }
            Err(error) => return Err(error),
        }
        directory_without_links(ancestor)?;
    }
    path.canonicalize()
}

fn options() -> OpenOptions {
    let mut result = OpenOptions::new();
    result.read(true).write(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Never follow a link substituted for the lease file.
        result.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT
    }
    result
}

fn validate_name(name: &str) -> bool {
    let parts: Vec<_> = name.split('-').collect();
    parts.len() == 4
        && parts[0] == "instance"
        && parts[1].len() == 8
        && parts[2].len() == 32
        && parts[3].len() == 16
        && parts[1..]
            .iter()
            .all(|part| part.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn marker(name: &str) -> String {
    format!("{MARKER}\n{name}\n")
}

// The complete preflight runs before deletion. A WebView child that still has an
// open cache file prevents collection even if its desktop parent has exited.
fn inspect_tree(
    path: &Path,
    depth: usize,
    count: &mut usize,
    files: &mut Vec<File>,
    deadline: Instant,
) -> io::Result<()> {
    if depth > 32 {
        return Err(invalid("WebView cache tree exceeds collection depth"));
    }
    directory_without_links(path)?;
    for entry in fs::read_dir(path)? {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "WebView collection preflight budget exhausted",
            ));
        }
        *count += 1;
        if *count > MAX_ENTRIES {
            return Err(invalid("WebView cache exceeds collection entry limit"));
        }
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if is_link(&metadata) {
            return Err(invalid("WebView cache contains a reparse point"));
        }
        if metadata.is_dir() {
            inspect_tree(&entry.path(), depth + 1, count, files, deadline)?;
        } else if metadata.is_file() {
            if depth == 0 && entry.file_name() == LEASE {
                continue;
            }
            let mut opening = OpenOptions::new();
            opening.read(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                // Deny other readers/writers while allowing the checked removal.
                opening.share_mode(4).custom_flags(0x0020_0000);
            }
            let file = opening.open(entry.path())?;
            if is_link(&file.metadata()?) {
                return Err(invalid("WebView cache file became a reparse point"));
            }
            files.push(file);
        } else {
            return Err(invalid("WebView cache contains a special file"));
        }
    }
    Ok(())
}

fn collect_one(root: &Path, name: &str, deadline: Instant) -> io::Result<()> {
    if !validate_name(name) {
        return Err(invalid("Unrecognized WebView cache directory"));
    }
    let candidate = root.join(name);
    directory_without_links(root)?;
    directory_without_links(&candidate)?;
    if candidate.canonicalize()?.parent() != Some(root) {
        return Err(invalid("WebView cache escaped its owned root"));
    }
    let mut lease = options().open(candidate.join(LEASE))?;
    let metadata = lease.metadata()?;
    if !metadata.is_file() || is_link(&metadata) || metadata.len() > 256 {
        return Err(invalid("Invalid WebView cache owner marker"));
    }
    lease.try_lock().map_err(io::Error::other)?;
    let mut content = String::new();
    (&mut lease).take(257).read_to_string(&mut content)?;
    if content != marker(name) {
        return Err(invalid("WebView cache owner marker does not match"));
    }
    let mut files = Vec::new();
    inspect_tree(&candidate, 0, &mut 0, &mut files, deadline)?;
    // Keep the owner marker until every cache entry is removed. A read-only or
    // inaccessible entry must not leave an unmarked directory behind for good.
    for entry in fs::read_dir(&candidate)? {
        let entry = entry?;
        if entry.file_name() == LEASE {
            continue;
        }
        let metadata = fs::symlink_metadata(entry.path())?;
        if is_link(&metadata) {
            return Err(invalid("WebView cache entry became a reparse point"));
        }
        // Only entries below this validated, locked, direct child are removed.
        // Windows std additionally protects against symlink replacement races.
        if metadata.is_dir() {
            fs::remove_dir_all(entry.path())?;
        } else {
            fs::remove_file(entry.path())?;
        }
    }
    drop(files);
    fs::remove_file(candidate.join(LEASE))?;
    fs::remove_dir(&candidate)
}

impl Profile {
    pub fn acquire(root: &Path) -> io::Result<Self> {
        let root = prepare_root(root)?;
        let gate = options()
            .create(true)
            .truncate(false)
            .open(root.join(".collection-lock"))?;
        if is_link(&gate.metadata()?) || !gate.metadata()?.is_file() {
            return Err(invalid("WebView collection lock is not a plain file"));
        }
        gate.lock()?;
        // Bounded best-effort collection. Unrecognized, active, linked or locked
        // caches are left untouched and never prevent creation of a new profile.
        let deadline = Instant::now() + Duration::from_millis(250);
        let mut collected = 0;
        for entry in fs::read_dir(&root)?.take(128).flatten() {
            if collected >= 2 || Instant::now() >= deadline {
                break;
            }
            if let Some(name) = entry
                .file_name()
                .to_str()
                .filter(|name| validate_name(name))
            {
                if collect_one(&root, name, deadline).is_ok() {
                    collected += 1;
                }
            }
        }
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let name = format!(
            "instance-{:08x}-{time:032x}-{:016x}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        let directory = root.join(&name);
        fs::create_dir(&directory)?;
        let mut lease = options().create_new(true).open(directory.join(LEASE))?;
        lease.lock()?;
        lease.write_all(marker(&name).as_bytes())?;
        lease.sync_all()?;
        Ok(Self {
            directory,
            _lease: lease,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        let value = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("cerulean-webview-{}-{value}", std::process::id()));
        fs::create_dir(&root).unwrap();
        root.canonicalize().unwrap()
    }

    #[test]
    fn live_profiles_are_distinct_and_only_retired_owned_profiles_are_collected() {
        let root = root();
        let first = Profile::acquire(&root).unwrap();
        fs::write(first.directory.join("cache"), b"disposable").unwrap();
        let second = Profile::acquire(&root).unwrap();
        assert_ne!(first.directory, second.directory);
        assert!(first.directory.join("cache").is_file());
        let retired = first.directory.clone();
        drop(first);
        let third = Profile::acquire(&root).unwrap();
        assert!(!retired.exists());
        assert!(second.directory.is_dir());
        assert!(third.directory.is_dir());
    }

    #[test]
    fn foreign_and_mismatched_markers_are_never_removed() {
        let root = root();
        let first = Profile::acquire(&root).unwrap();
        let directory = first.directory.clone();
        drop(first);
        fs::write(directory.join(LEASE), b"other application").unwrap();
        fs::create_dir(root.join("foreign-cache")).unwrap();
        let _next = Profile::acquire(&root).unwrap();
        assert!(directory.is_dir());
        assert!(root.join("foreign-cache").is_dir());
        assert!(collect_one(&root, "../outside", Instant::now() + Duration::from_secs(1)).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn lingering_webview_file_handle_prevents_any_partial_collection() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = root();
        let first = Profile::acquire(&root).unwrap();
        fs::write(first.directory.join("cache"), b"in use").unwrap();
        let busy = OpenOptions::new()
            .read(true)
            .share_mode(7)
            .open(first.directory.join("cache"))
            .unwrap();
        let directory = first.directory.clone();
        drop(first);
        let _second = Profile::acquire(&root).unwrap();
        assert_eq!(fs::read(directory.join("cache")).unwrap(), b"in use");
        assert!(directory.join(LEASE).is_file());
        drop(busy);
        let _third = Profile::acquire(&root).unwrap();
        assert!(!directory.exists());
    }
}
