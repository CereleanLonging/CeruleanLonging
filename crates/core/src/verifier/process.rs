//! Windows process-tree ownership. The job is assigned atomically at creation;
//! no child instruction can run before it belongs to our kill-on-close job.
use std::{
    ffi::OsStr,
    fs::File,
    io::Read,
    mem::size_of,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    ptr::{null, null_mut},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::*,
    Security::SECURITY_ATTRIBUTES,
    System::{JobObjects::*, Pipes::CreatePipe, Threading::*},
};

pub const OUTPUT_LIMIT: usize = 1024 * 1024;
pub(crate) mod interactive;
pub struct Outcome {
    pub exit_code: i32,
    pub timed_out: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub failure: Option<String>,
}
fn os_error() -> String {
    std::io::Error::last_os_error().to_string()
}
fn wide(value: &OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}
fn handle(value: HANDLE) -> Result<OwnedHandle, String> {
    if value.is_null() || value == INVALID_HANDLE_VALUE {
        Err(os_error())
    } else {
        Ok(unsafe { OwnedHandle::from_raw_handle(value) })
    }
}
fn pipe() -> Result<(File, OwnedHandle), String> {
    let mut read = null_mut();
    let mut write = null_mut();
    let security = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: null_mut(),
        bInheritHandle: 1,
    };
    unsafe {
        if CreatePipe(&mut read, &mut write, &security, 0) == 0 {
            return Err(os_error());
        }
        let read = handle(read)?;
        let write = handle(write)?;
        if SetHandleInformation(read.as_raw_handle(), HANDLE_FLAG_INHERIT, 0) == 0 {
            return Err(os_error());
        }
        Ok((File::from(read), write))
    }
}
fn read_output(
    mut file: File,
    overflow: Arc<AtomicBool>,
) -> thread::JoinHandle<Result<Vec<u8>, String>> {
    thread::spawn(move || {
        let mut result = Vec::new();
        let mut buffer = [0u8; 8192];
        loop {
            let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                return Ok(result);
            }
            let keep = n.min(OUTPUT_LIMIT - result.len());
            result.extend_from_slice(&buffer[..keep]);
            if keep != n {
                overflow.store(true, Ordering::Release);
            }
        }
    })
}
// CRT-compatible quoting for native executables. Batch files/shell expansion are
// not supported; executable path is supplied separately to CreateProcessW.
fn quote(arg: &str) -> String {
    let mut out = String::from("\"");
    let mut slashes = 0;
    for c in arg.chars() {
        if c == '\\' {
            slashes += 1;
            continue;
        }
        out.extend(std::iter::repeat_n(
            '\\',
            if c == '"' { slashes * 2 + 1 } else { slashes },
        ));
        out.push(c);
        slashes = 0;
    }
    out.extend(std::iter::repeat_n('\\', slashes * 2));
    out.push('"');
    out
}
struct Attributes {
    storage: Vec<usize>,
    initialized: bool,
}
impl Attributes {
    fn new() -> Result<Self, String> {
        let mut size = 0;
        unsafe {
            InitializeProcThreadAttributeList(null_mut(), 2, 0, &mut size);
        }
        if size == 0 {
            return Err(os_error());
        }
        let mut value = Self {
            storage: vec![0; size.div_ceil(size_of::<usize>())],
            initialized: false,
        };
        if unsafe { InitializeProcThreadAttributeList(value.ptr(), 2, 0, &mut size) } == 0 {
            return Err(os_error());
        }
        value.initialized = true;
        Ok(value)
    }
    fn ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        self.storage.as_mut_ptr().cast()
    }
    fn set(&mut self, key: u32, handles: &[HANDLE]) -> Result<(), String> {
        if unsafe {
            UpdateProcThreadAttribute(
                self.ptr(),
                0,
                key as usize,
                handles.as_ptr().cast(),
                std::mem::size_of_val(handles),
                null_mut(),
                null(),
            )
        } == 0
        {
            return Err(os_error());
        }
        Ok(())
    }
}
impl Drop for Attributes {
    fn drop(&mut self) {
        if self.initialized {
            unsafe {
                DeleteProcThreadAttributeList(self.ptr());
            }
        }
    }
}
fn active(job: &OwnedHandle) -> Result<u32, String> {
    let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe {
        QueryInformationJobObject(
            job.as_raw_handle(),
            JobObjectBasicAccountingInformation,
            (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
            size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
            null_mut(),
        )
    } == 0
    {
        return Err(os_error());
    }
    Ok(info.ActiveProcesses)
}
pub fn run(
    exe: &Path,
    args: &[String],
    cwd: &Path,
    temp: &Path,
    timeout: Duration,
    cancel: &AtomicBool,
    allowed_environment: &std::collections::BTreeMap<String, String>,
) -> Result<Outcome, String> {
    run_inner(
        exe,
        args,
        cwd,
        temp,
        timeout,
        cancel,
        allowed_environment,
        false,
    )
}
#[link(name = "user32")]
extern "system" {
    fn AllowSetForegroundWindow(process_id: u32) -> i32;
}
pub(crate) fn allow_foreground(process_id: u32) -> bool {
    // A specific, already authenticated/owned PID; never ASFW_ANY.
    unsafe { AllowSetForegroundWindow(process_id) != 0 }
}
pub(crate) fn run_desktop(
    exe: &Path,
    args: &[String],
    cwd: &Path,
    temp: &Path,
    timeout: Duration,
    cancel: &AtomicBool,
    allowed_environment: &std::collections::BTreeMap<String, String>,
) -> Result<Outcome, String> {
    run_inner(
        exe,
        args,
        cwd,
        temp,
        timeout,
        cancel,
        allowed_environment,
        true,
    )
}
#[allow(clippy::too_many_arguments)]
fn run_inner(
    exe: &Path,
    args: &[String],
    cwd: &Path,
    temp: &Path,
    timeout: Duration,
    cancel: &AtomicBool,
    allowed_environment: &std::collections::BTreeMap<String, String>,
    desktop_foreground: bool,
) -> Result<Outcome, String> {
    let job = handle(unsafe { CreateJobObjectW(null(), null()) })?;
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if unsafe {
        SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    } == 0
    {
        return Err(os_error());
    }
    let (stdout, stdout_write) = pipe()?;
    let (stderr, stderr_write) = pipe()?;
    // stdin is a closed pipe (EOF), never the user's terminal.
    let (stdin, stdin_write) = pipe()?;
    drop(stdin_write);
    use std::os::windows::io::AsRawHandle;
    if unsafe {
        SetHandleInformation(
            stdin.as_raw_handle(),
            HANDLE_FLAG_INHERIT,
            HANDLE_FLAG_INHERIT,
        )
    } == 0
    {
        return Err(os_error());
    }
    let mut attrs = Attributes::new()?;
    let inherited = [
        stdin.as_raw_handle(),
        stdout_write.as_raw_handle(),
        stderr_write.as_raw_handle(),
    ];
    let jobs = [job.as_raw_handle()];
    attrs.set(PROC_THREAD_ATTRIBUTE_HANDLE_LIST, &inherited)?;
    attrs.set(PROC_THREAD_ATTRIBUTE_JOB_LIST, &jobs)?;
    let mut startup: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
    startup.StartupInfo.cb = size_of::<STARTUPINFOEXW>() as u32;
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = inherited[0];
    startup.StartupInfo.hStdOutput = inherited[1];
    startup.StartupInfo.hStdError = inherited[2];
    startup.lpAttributeList = attrs.ptr();
    let application = wide(exe.as_os_str());
    let directory = wide(cwd.as_os_str());
    let text = std::iter::once(exe.to_str().ok_or("non-Unicode executable")?)
        .chain(args.iter().map(String::as_str))
        .map(quote)
        .collect::<Vec<_>>()
        .join(" ");
    let mut command = wide(OsStr::new(&text));
    if command.len() > 32767 || args.iter().any(|a| a.contains('\0')) {
        return Err("invalid native command line".into());
    }
    // No inherited API keys, runtime injection variables, PATH or shell profile.
    let system = std::env::var_os("SystemRoot").ok_or("SystemRoot missing")?;
    let mut values: std::collections::BTreeMap<String, std::ffi::OsString> = allowed_environment
        .iter()
        .map(|(k, v)| (k.clone(), v.into()))
        .collect();
    values.insert("SYSTEMROOT".into(), system.clone());
    values.insert("TEMP".into(), temp.as_os_str().to_owned());
    values.insert("TMP".into(), temp.as_os_str().to_owned());
    values.insert("WINDIR".into(), system);
    let mut environment = Vec::new();
    for (key, value) in values {
        environment.extend(format!("{key}=").encode_utf16());
        environment.extend(value.encode_wide());
        environment.push(0);
    }
    environment.push(0);
    if cancel.load(Ordering::Acquire) {
        return Err("verification cancelled before launch".into());
    }
    let mut info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
    if unsafe {
        CreateProcessW(
            application.as_ptr(),
            command.as_mut_ptr(),
            null(),
            null(),
            1,
            CREATE_NO_WINDOW
                | CREATE_UNICODE_ENVIRONMENT
                | EXTENDED_STARTUPINFO_PRESENT
                | if desktop_foreground {
                    CREATE_SUSPENDED
                } else {
                    0
                },
            environment.as_ptr().cast(),
            directory.as_ptr(),
            &startup.StartupInfo,
            &mut info,
        )
    } == 0
    {
        return Err(os_error());
    }
    let process = handle(info.hProcess)?;
    let initial_thread = handle(info.hThread)?;
    if desktop_foreground {
        // The foreground product UI grants the authenticated service permission;
        // pass it only to this owned bridge before it can execute. Windows can
        // revoke permission on user input, so the bridge still checks foreground
        // immediately before every edit/invoke and fails without retrying.
        let _ = allow_foreground(info.dwProcessId);
        if unsafe { ResumeThread(initial_thread.as_raw_handle()) } == u32::MAX {
            return Err(os_error());
        }
    }
    drop(initial_thread);
    drop(stdin);
    drop(stdout_write);
    drop(stderr_write);
    let overflow = Arc::new(AtomicBool::new(false));
    let out = read_output(stdout, overflow.clone());
    let err = read_output(stderr, overflow.clone());
    let started = Instant::now();
    let mut timed_out = false;
    let mut failure = None;
    loop {
        if cancel.load(Ordering::Acquire) {
            failure = Some("cancelled".into());
            break;
        }
        if overflow.load(Ordering::Acquire) {
            failure = Some("output-limit".into());
            break;
        }
        if started.elapsed() >= timeout {
            timed_out = true;
            break;
        }
        match unsafe { WaitForSingleObject(process.as_raw_handle(), 10) } {
            WAIT_OBJECT_0 => break,
            WAIT_TIMEOUT => {}
            _ => {
                failure = Some(os_error());
                break;
            }
        }
    }
    let mut exit_code = 0;
    if unsafe { GetExitCodeProcess(process.as_raw_handle(), &mut exit_code) } == 0 {
        failure = Some(os_error());
    }
    if !timed_out && failure.is_none() {
        // A signalled parent handle can precede Job accounting removal. Give
        // ordinary teardown a bounded drain interval before classifying orphans.
        let drain = Instant::now();
        while active(&job)? > 0 && drain.elapsed() < Duration::from_millis(100) {
            thread::sleep(Duration::from_millis(5));
        }
        if active(&job)? > 0 {
            failure = Some("descendants-left-running".into());
        }
    }
    if unsafe { TerminateJobObject(job.as_raw_handle(), 1) } == 0 {
        return Err(os_error());
    }
    // Confirm tree exit before trusting outputs or inspecting mutable artifacts.
    let cleanup = Instant::now();
    while active(&job)? != 0 {
        if cleanup.elapsed() > Duration::from_secs(5) {
            return Err("process tree cleanup timeout".into());
        }
        thread::sleep(Duration::from_millis(10));
    }
    if timed_out {
        unsafe {
            GetExitCodeProcess(process.as_raw_handle(), &mut exit_code);
        }
    }
    let stdout = out.join().map_err(|_| "stdout reader panicked")??;
    let stderr = err.join().map_err(|_| "stderr reader panicked")??;
    if overflow.load(Ordering::Acquire) {
        failure = Some("output-limit".into());
    }
    Ok(Outcome {
        exit_code: exit_code as i32,
        timed_out,
        stdout,
        stderr,
        failure,
    })
}
