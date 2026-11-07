//! Launch only the fixed import script, using this user's non-elevated token.
use std::{
    ffi::c_void,
    io, mem,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    ptr,
};
use windows_sys::Win32::{
    Foundation::{LocalFree, WAIT_OBJECT_0, WAIT_TIMEOUT},
    Security::{
        Authorization::ConvertSidToStringSidW,
        Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG},
        *,
    },
    System::{
        Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock},
        SystemInformation::GetSystemDirectoryW,
        Threading::*,
    },
    UI::WindowsAndMessaging::{GetShellWindow, GetWindowThreadProcessId},
};

fn error(message: &str) -> String {
    format!("{message}（{}）", io::Error::last_os_error())
}
fn wide(value: &std::ffi::OsStr) -> Vec<u16> {
    value.encode_wide().chain(Some(0)).collect()
}

pub fn session_id() -> Result<String, String> {
    let mut bytes = [0u8; 24];
    let status = unsafe {
        BCryptGenRandom(
            ptr::null_mut(),
            bytes.as_mut_ptr(),
            bytes.len() as u32,
            BCRYPT_USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status < 0 {
        return Err("无法生成导入会话标识".into());
    }
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn token_information<T: Default>(
    token: *mut c_void,
    class: TOKEN_INFORMATION_CLASS,
) -> Result<T, String> {
    let mut value = T::default();
    let mut length = 0;
    if unsafe {
        GetTokenInformation(
            token,
            class,
            (&mut value as *mut T).cast(),
            mem::size_of::<T>() as u32,
            &mut length,
        )
    } == 0
    {
        return Err(error("无法核对进程权限"));
    }
    if length as usize != mem::size_of::<T>() {
        return Err("进程权限数据长度不符".into());
    }
    Ok(value)
}

fn token_sid(token: *mut c_void) -> Result<String, String> {
    let mut length = 0;
    unsafe {
        GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut length);
    }
    if length == 0 || length > 65536 {
        return Err("进程账户数据长度无效".into());
    }
    let mut storage = vec![0usize; (length as usize).div_ceil(mem::size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            storage.as_mut_ptr().cast(),
            length,
            &mut length,
        )
    } == 0
    {
        return Err(error("无法核对进程账户"));
    }
    let user = unsafe { &*storage.as_ptr().cast::<TOKEN_USER>() };
    let mut text = ptr::null_mut();
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut text) } == 0 {
        return Err(error("无法读取账户标识"));
    }
    let result = unsafe {
        let mut count = 0;
        while count < 256 && *text.add(count) != 0 {
            count += 1;
        }
        let result = if count < 256 {
            String::from_utf16(std::slice::from_raw_parts(text, count))
                .map_err(|_| "账户标识编码无效".to_string())
        } else {
            Err("账户标识超长".into())
        };
        LocalFree(text.cast());
        result
    };
    result
}

fn open_token(process: *mut c_void, rights: u32) -> Result<OwnedHandle, String> {
    let mut token = ptr::null_mut();
    if unsafe { OpenProcessToken(process, rights, &mut token) } == 0 {
        return Err(error("无法打开进程权限"));
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(token) })
}

fn verify_standard(token: *mut c_void, owner: &str) -> Result<(), String> {
    if token_information::<TOKEN_ELEVATION>(token, TokenElevation)?.TokenIsElevated != 0
        || token_information::<u32>(token, TokenUIAccess)? != 0
        || token_sid(token)? != owner
    {
        return Err("导入进程必须使用当前账户的普通权限".into());
    }
    let mut length = 0;
    unsafe {
        GetTokenInformation(token, TokenIntegrityLevel, ptr::null_mut(), 0, &mut length);
    }
    if length == 0 || length > 65536 {
        return Err("进程完整性数据长度无效".into());
    }
    let mut storage = vec![0usize; (length as usize).div_ceil(mem::size_of::<usize>())];
    if unsafe {
        GetTokenInformation(
            token,
            TokenIntegrityLevel,
            storage.as_mut_ptr().cast(),
            length,
            &mut length,
        )
    } == 0
    {
        return Err(error("无法核对导入进程完整性级别"));
    }
    let label = unsafe { &*storage.as_ptr().cast::<TOKEN_MANDATORY_LABEL>() };
    let integrity = unsafe {
        let count = *GetSidSubAuthorityCount(label.Label.Sid);
        if count == 0 {
            return Err("进程完整性标识无效".into());
        }
        *GetSidSubAuthority(label.Label.Sid, u32::from(count - 1))
    };
    if integrity != 0x2000 {
        // SECURITY_MANDATORY_MEDIUM_RID
        return Err("导入窗口需要普通权限的中完整性令牌".into());
    }
    Ok(())
}

pub fn current_owner() -> Result<String, String> {
    let token = open_token(unsafe { GetCurrentProcess() }, TOKEN_QUERY)?;
    token_sid(token.as_raw_handle())
}

fn desktop_token(owner: &str, current: *mut c_void) -> Result<OwnedHandle, String> {
    // TokenLinkedToken may be identification-only on UAC systems. Obtain a
    // primary token from this interactive desktop and verify its identity.
    let shell = unsafe { GetShellWindow() };
    let mut pid = 0;
    if shell.is_null() || unsafe { GetWindowThreadProcessId(shell, &mut pid) } == 0 {
        return Err("请先登录普通权限的 Windows 桌面，再打开导入窗口".into());
    }
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(error("无法核对当前桌面进程"));
    }
    let process = unsafe { OwnedHandle::from_raw_handle(process) };
    let token = open_token(process.as_raw_handle(), TOKEN_QUERY | TOKEN_DUPLICATE)?;
    verify_standard(token.as_raw_handle(), owner)?;
    if token_information::<u32>(token.as_raw_handle(), TokenSessionId)?
        != token_information::<u32>(current, TokenSessionId)?
    {
        return Err("导入窗口与主窗口必须属于同一个 Windows 登录会话".into());
    }
    let mut primary = ptr::null_mut();
    if unsafe {
        DuplicateTokenEx(
            token.as_raw_handle(),
            TOKEN_QUERY
                | TOKEN_DUPLICATE
                | TOKEN_ASSIGN_PRIMARY
                | TOKEN_ADJUST_DEFAULT
                | TOKEN_ADJUST_SESSIONID,
            ptr::null(),
            SecurityImpersonation,
            TokenPrimary,
            &mut primary,
        )
    } == 0
    {
        return Err(error("无法准备普通权限主令牌"));
    }
    let primary = unsafe { OwnedHandle::from_raw_handle(primary) };
    verify_standard(primary.as_raw_handle(), owner)?;
    if token_information::<TOKEN_TYPE>(primary.as_raw_handle(), TokenType)? != TokenPrimary {
        return Err("导入进程需要普通权限主令牌".into());
    }
    Ok(primary)
}

pub fn create_private_directory(path: &Path, owner: &str) -> Result<(), String> {
    // Explicit medium label allows the verified standard child to write here.
    let descriptor = wide(std::ffi::OsStr::new(&format!(
        "D:P(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;FA;;;{owner})S:(ML;OICI;NW;;;ME)"
    )));
    let mut security = ptr::null_mut();
    if unsafe {
        windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW(
        descriptor.as_ptr(), 1, &mut security, ptr::null_mut())
    } == 0
    {
        return Err(error("无法准备导入目录权限"));
    }
    let attributes = SECURITY_ATTRIBUTES {
        nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: security,
        bInheritHandle: 0,
    };
    let name = wide(path.as_os_str());
    let result = unsafe {
        windows_sys::Win32::Storage::FileSystem::CreateDirectoryW(name.as_ptr(), &attributes)
    };
    let failure = if result == 0 {
        Some(error("无法创建私有导入目录"))
    } else {
        None
    };
    unsafe {
        LocalFree(security);
    }
    failure.map_or(Ok(()), Err)
}

pub struct Process {
    handle: OwnedHandle,
    pub pid: u32,
}
impl Process {
    pub fn finished(&self) -> Result<bool, String> {
        match unsafe { WaitForSingleObject(self.handle.as_raw_handle(), 0) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(error("无法核对导入窗口状态")),
        }
    }
    pub fn wait(&self, milliseconds: u32) -> Result<bool, String> {
        match unsafe { WaitForSingleObject(self.handle.as_raw_handle(), milliseconds) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(error("无法等待导入窗口关闭")),
        }
    }

    pub fn exit_code(&self) -> Result<u32, String> {
        let mut code = 0;
        if unsafe { GetExitCodeProcess(self.handle.as_raw_handle(), &mut code) } == 0 {
            return Err(error("无法读取导入窗口退出状态"));
        }
        Ok(code)
    }
}

pub fn launch(script: &Path, plan: &Path, owner: &str) -> Result<Process, String> {
    let mut system = [0u16; 32768];
    let length = unsafe { GetSystemDirectoryW(system.as_mut_ptr(), system.len() as u32) } as usize;
    if length == 0 || length >= system.len() {
        return Err(error("无法定位系统 PowerShell"));
    }
    let shell = std::path::PathBuf::from(
        String::from_utf16(&system[..length]).map_err(|_| "系统目录编码无效")?,
    )
    .join("WindowsPowerShell/v1.0/powershell.exe");
    let parent = unsafe { GetCurrentProcess() };
    let current = open_token(parent, TOKEN_QUERY | TOKEN_DUPLICATE | TOKEN_ASSIGN_PRIMARY)?;
    let elevated = token_information::<TOKEN_ELEVATION>(current.as_raw_handle(), TokenElevation)?
        .TokenIsElevated
        != 0;
    let standard = if elevated {
        Some(desktop_token(owner, current.as_raw_handle())?)
    } else {
        None
    };
    let token = standard.as_ref().unwrap_or(&current).as_raw_handle();
    verify_standard(token, owner)?;
    let mut environment = ptr::null_mut();
    if unsafe { CreateEnvironmentBlock(&mut environment, token, 0) } == 0 {
        return Err(error("无法准备普通权限环境"));
    }
    // Paths are generated by the host. Windows filenames cannot contain quotes.
    let line = format!("\"{}\" -NoProfile -NonInteractive -STA -ExecutionPolicy RemoteSigned -File \"{}\" -PlanPath \"{}\"", shell.display(), script.display(), plan.display());
    let mut command = wide(std::ffi::OsStr::new(&line));
    let program = wide(shell.as_os_str());
    let directory = wide(script.parent().ok_or("导入目录缺失")?.as_os_str());
    let mut startup: STARTUPINFOW = unsafe { mem::zeroed() };
    startup.cb = mem::size_of::<STARTUPINFOW>() as u32;
    let mut information: PROCESS_INFORMATION = unsafe { mem::zeroed() };
    let flags = CREATE_SUSPENDED | CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT;
    let created = unsafe {
        let result = if elevated {
            CreateProcessWithTokenW(
                token,
                0,
                program.as_ptr(),
                command.as_mut_ptr(),
                flags,
                environment,
                directory.as_ptr(),
                &startup,
                &mut information,
            )
        } else {
            CreateProcessW(
                program.as_ptr(),
                command.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                0,
                flags,
                environment,
                directory.as_ptr(),
                &startup,
                &mut information,
            )
        };
        let error = if result == 0 {
            Some(error("普通权限导入窗口启动失败"))
        } else {
            None
        };
        DestroyEnvironmentBlock(environment);
        error
    };
    if let Some(error) = created {
        return Err(error);
    }
    let process = unsafe { OwnedHandle::from_raw_handle(information.hProcess) };
    let thread = unsafe { OwnedHandle::from_raw_handle(information.hThread) };
    let checked = open_token(process.as_raw_handle(), TOKEN_QUERY)
        .and_then(|token| verify_standard(token.as_raw_handle(), owner));
    if let Err(error) = checked {
        // The owned child has never executed. Reject it before the first instruction.
        unsafe {
            TerminateProcess(process.as_raw_handle(), 1);
        }
        return Err(error);
    }
    if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
        let failure = error("无法启动已核验的导入窗口");
        unsafe {
            TerminateProcess(process.as_raw_handle(), 1);
        }
        return Err(failure);
    }
    Ok(Process {
        handle: process,
        pid: information.dwProcessId,
    })
}
