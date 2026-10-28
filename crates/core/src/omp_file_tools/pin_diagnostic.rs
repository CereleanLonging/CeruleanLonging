//! Best-effort current-process metadata snapshot after the first failed pin.
//! Test-only, Windows x64. No content reads, retries of the pin, or foreign PID.
use std::{
    ffi::c_void,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};

type Handle = *mut c_void;
#[repr(C)]
#[derive(Clone, Copy)]
struct Entry {
    handle: usize,
    _handle_count: usize,
    _pointer_count: usize,
    granted_access: u32,
    _object_type: u32,
    attributes: u32,
    _reserved: u32,
}
const _: () = assert!(std::mem::size_of::<usize>() == 8 && std::mem::size_of::<Entry>() == 40);
#[link(name = "ntdll")]
extern "system" {
    fn NtQueryInformationProcess(
        process: Handle,
        class: u32,
        data: *mut c_void,
        length: u32,
        returned: *mut u32,
    ) -> i32;
}
#[link(name = "kernel32")]
extern "system" {
    fn GetCurrentProcess() -> Handle;
    fn DuplicateHandle(
        source_process: Handle,
        source: Handle,
        target_process: Handle,
        target: *mut Handle,
        access: u32,
        inherit: i32,
        options: u32,
    ) -> i32;
    fn CloseHandle(handle: Handle) -> i32;
    fn GetFileType(handle: Handle) -> u32;
    fn GetFinalPathNameByHandleW(handle: Handle, path: *mut u16, length: u32, flags: u32) -> u32;
}
struct Owned(Handle);
impl Drop for Owned {
    fn drop(&mut self) {
        // Only successfully duplicated handles reach this owner. Never close a source.
        unsafe {
            CloseHandle(self.0);
        }
    }
}
fn path_key(path: &str) -> String {
    let path = path.replace('/', "\\");
    path.strip_prefix("\\\\?\\")
        .unwrap_or(&path)
        .trim_end_matches('\\')
        .to_lowercase()
}

pub(super) fn report(path: &Path) {
    static REPORTED: AtomicBool = AtomicBool::new(false);
    if REPORTED.swap(true, Ordering::Relaxed) {
        return;
    }
    // Aligned storage, at most three snapshot queries and at most 2 MiB.
    // ProcessHandleInformation (51) queries only our process, not a system table.
    const LIMIT: usize = 2 * 1024 * 1024;
    let process = unsafe { GetCurrentProcess() };
    let mut words = vec![0usize; 64 * 1024 / 8];
    let mut valid_bytes = 0usize;
    for attempt in 0..3 {
        let mut returned = 0u32;
        let bytes = words.len() * 8;
        let status = unsafe {
            NtQueryInformationProcess(
                process,
                51,
                words.as_mut_ptr().cast(),
                bytes as u32,
                &mut returned,
            )
        };
        if status == 0 {
            valid_bytes = if returned == 0 {
                bytes
            } else {
                returned as usize
            };
            break;
        }
        if !matches!(status as u32, 0xc0000004 | 0xc0000023) || attempt == 2 {
            eprintln!(
                "omp pin 328 handles unavailable: ntstatus={:#010x}",
                status as u32
            );
            return;
        }
        let next = (returned as usize).max(bytes * 2);
        if next > LIMIT {
            eprintln!("omp pin 328 handles unavailable: snapshot exceeds 2 MiB");
            return;
        }
        words.resize(next.div_ceil(8), 0);
    }
    let bytes = words.len() * 8;
    if valid_bytes < 16 || valid_bytes > bytes {
        eprintln!("omp pin 328 handles unavailable: invalid returned size");
        return;
    }
    let count = words[0];
    let Some(required) = count.checked_mul(40).and_then(|n| n.checked_add(16)) else {
        return;
    };
    if required > valid_bytes {
        eprintln!("omp pin 328 handles unavailable: entry count outside snapshot");
        return;
    }
    let wanted = path_key(&path.to_string_lossy());
    let mut name = vec![0u16; 32768];
    let (mut duplicated, mut disk, mut matches) = (0usize, 0usize, 0usize);
    let started = std::time::Instant::now();
    let mut visited = 0usize;
    for index in 0..count.min(2048) {
        // Admission bound only: a single Win32 metadata call has no hard timeout.
        if started.elapsed() > std::time::Duration::from_millis(500) {
            break;
        }
        visited += 1;
        // Header is two pointer-sized fields. x64 entry size/alignment is fixed above.
        let entry = unsafe {
            std::ptr::read_unaligned(
                words
                    .as_ptr()
                    .cast::<u8>()
                    .add(16 + index * 40)
                    .cast::<Entry>(),
            )
        };
        let mut target = std::ptr::null_mut();
        // DUPLICATE_SAME_ACCESS only; never DUPLICATE_CLOSE_SOURCE, never inherit.
        if unsafe {
            DuplicateHandle(
                process,
                entry.handle as Handle,
                process,
                &mut target,
                0,
                0,
                2,
            )
        } == 0
        {
            continue;
        }
        let owned = Owned(target);
        duplicated += 1;
        if unsafe { GetFileType(owned.0) } != 1 {
            // FILE_TYPE_DISK
            continue;
        }
        disk += 1;
        let length =
            unsafe { GetFinalPathNameByHandleW(owned.0, name.as_mut_ptr(), name.len() as u32, 0) }
                as usize;
        if length == 0 || length >= name.len() {
            continue;
        }
        if path_key(&String::from_utf16_lossy(&name[..length])) == wanted {
            matches += 1;
            // A source handle may have closed/reused before DuplicateHandle. The
            // snapshot access is auxiliary evidence, never a causal attribution.
            eprintln!("omp pin 328 matching directory handle: snapshotHandle={:#x} snapshotGrantedAccess={:#010x} snapshotAttributes={:#010x} identityMayHaveRaced=true", entry.handle, entry.granted_access, entry.attributes);
        }
        // No unmatched path, file content, PID table or kernel pointer is printed.
    }
    eprintln!("omp pin 328 current-process handles: entries={count} visited={visited} duplicated={duplicated} disk={disk} matches={matches} truncated={} bestEffort=true noMatchDoesNotProveExternal=true", visited < count);
}
