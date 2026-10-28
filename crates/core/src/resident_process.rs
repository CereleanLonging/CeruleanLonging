//! A resident outlives its caller, so it must not inherit caller pipes or locks.
use std::{
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::Path,
    ptr,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, WAIT_OBJECT_0},
    System::Threading::{
        CreateProcessW, TerminateProcess, WaitForSingleObject, CREATE_NO_WINDOW,
        PROCESS_INFORMATION, STARTUPINFOW,
    },
};
pub(super) struct Child {
    process: OwnedHandle,
    pid: u32,
}
// Native CRT argument quoting; no cmd.exe, PowerShell or shell interpolation.
fn quote(arg: &str) -> String {
    let mut result = String::from("\"");
    let mut slashes = 0;
    for c in arg.chars() {
        if c == '\\' {
            slashes += 1;
            continue;
        }
        result.extend(std::iter::repeat_n(
            '\\',
            if c == '"' { slashes * 2 + 1 } else { slashes },
        ));
        result.push(c);
        slashes = 0;
    }
    result.extend(std::iter::repeat_n('\\', slashes * 2));
    result.push('"');
    result
}
impl Child {
    pub(super) fn spawn(executable: &Path, directory: &Path) -> Result<Self, String> {
        let executable = executable.canonicalize().map_err(|e| e.to_string())?;
        let directory = directory.canonicalize().map_err(|e| e.to_string())?;
        let exe = executable
            .to_str()
            .ok_or("resident executable must be Unicode")?;
        let data = directory
            .to_str()
            .ok_or("resident directory must be Unicode")?;
        if exe.contains('\0') || data.contains('\0') {
            return Err("invalid resident path".into());
        }
        let application = exe.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
        let mut command = [exe, "--serve", "--data-dir", data]
            .into_iter()
            .map(quote)
            .collect::<Vec<_>>()
            .join(" ")
            .encode_utf16()
            .chain(Some(0))
            .collect::<Vec<_>>();
        if command.len() > 32767 {
            return Err("resident command exceeds Windows limit".into());
        }
        let startup = STARTUPINFOW {
            cb: std::mem::size_of::<STARTUPINFOW>() as u32,
            ..unsafe { std::mem::zeroed() }
        };
        let mut info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        // No standard handles are supplied and bInheritHandles is FALSE. The
        // child neither keeps CLI output alive nor inherits unrelated locks.
        if unsafe {
            CreateProcessW(
                application.as_ptr(),
                command.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                0,
                CREATE_NO_WINDOW,
                ptr::null(),
                ptr::null(),
                &startup,
                &mut info,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        unsafe {
            CloseHandle(info.hThread);
        }
        Ok(Self {
            process: unsafe { OwnedHandle::from_raw_handle(info.hProcess) },
            pid: info.dwProcessId,
        })
    }
    pub(super) fn id(&self) -> u32 {
        self.pid
    }
    pub(super) fn kill(&mut self) -> Result<(), String> {
        if unsafe { TerminateProcess(self.process.as_raw_handle(), 1) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(())
    }
    pub(super) fn wait(&mut self) -> Result<(), String> {
        if unsafe { WaitForSingleObject(self.process.as_raw_handle(), 5000) } != WAIT_OBJECT_0 {
            return Err("resident starter exit not confirmed".into());
        }
        Ok(())
    }
}
