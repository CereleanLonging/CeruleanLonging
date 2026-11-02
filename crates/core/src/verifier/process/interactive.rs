//! Interactive stdio with the same atomic Windows Job ownership as validators.
use super::*;

pub(crate) struct InteractiveChild {
    // Job is dropped first: pipes owned by this process tree reach EOF on drop.
    job: OwnedHandle,
    process: OwnedHandle,
    pub pid: u32,
}
pub(crate) struct Spawned {
    pub child: InteractiveChild,
    pub stdin: File,
    pub stdout: File,
    pub stderr: File,
}
impl InteractiveChild {
    pub fn spawn(
        exe: &Path,
        args: &[String],
        cwd: &Path,
        environment: &[u16],
    ) -> Result<Spawned, String> {
        Self::spawn_inner(exe, args, cwd, environment, None, false)
    }
    pub(crate) fn spawn_as(
        exe: &Path,
        args: &[String],
        cwd: &Path,
        environment: &[u16],
        account: &crate::windows_worker_account::Account,
    ) -> Result<Spawned, String> {
        Self::spawn_inner(exe, args, cwd, environment, Some(account), false)
    }
    pub(crate) fn spawn_visible_as(
        exe: &Path,
        args: &[String],
        cwd: &Path,
        environment: &[u16],
        account: &crate::windows_worker_account::Account,
    ) -> Result<Spawned, String> {
        Self::spawn_inner(exe, args, cwd, environment, Some(account), true)
    }
    fn spawn_inner(
        exe: &Path,
        args: &[String],
        cwd: &Path,
        environment: &[u16],
        account: Option<&crate::windows_worker_account::Account>,
        visible: bool,
    ) -> Result<Spawned, String> {
        let plain = if account.is_some() {
            Some((
                crate::windows_worker_account::dos_path(exe)?,
                crate::windows_worker_account::dos_path(cwd)?,
            ))
        } else {
            None
        };
        let (exe, cwd) = plain
            .as_ref()
            .map(|(e, c)| (e.as_path(), c.as_path()))
            .unwrap_or((exe, cwd));
        if !exe.is_absolute() || !cwd.is_absolute() || !environment.ends_with(&[0, 0]) {
            return Err("invalid owned process launch configuration".into());
        }
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
        let (stdin_read, stdin) = pipe()?;
        unsafe {
            if SetHandleInformation(
                stdin_read.as_raw_handle(),
                HANDLE_FLAG_INHERIT,
                HANDLE_FLAG_INHERIT,
            ) == 0
                || SetHandleInformation(stdin.as_raw_handle(), HANDLE_FLAG_INHERIT, 0) == 0
            {
                return Err(os_error());
            }
        }
        let inherited = [
            stdin_read.as_raw_handle(),
            stdout_write.as_raw_handle(),
            stderr_write.as_raw_handle(),
        ];
        let jobs = [job.as_raw_handle()];
        let mut attrs = Attributes::new()?;
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
        if application[..application.len() - 1].contains(&0)
            || directory[..directory.len() - 1].contains(&0)
            || args.iter().any(|a| a.contains('\0'))
        {
            return Err("invalid owned process path or argument".into());
        }
        let text = std::iter::once(exe.to_str().ok_or("non-Unicode executable")?)
            .chain(args.iter().map(String::as_str))
            .map(quote)
            .collect::<Vec<_>>()
            .join(" ");
        let mut command = wide(OsStr::new(&text));
        if command.len() > if account.is_some() { 1024 } else { 32767 } {
            return Err("owned process command exceeds limit".into());
        }
        let mut info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        let launched = if let Some(account) = account {
            let user = wide(OsStr::new(&account.record.username));
            let domain = wide(OsStr::new("."));
            // Secondary Logon cannot use STARTUPINFOEX job attributes. Create
            // suspended, verify the actual token, assign the job, then resume.
            startup.StartupInfo.cb = size_of::<STARTUPINFOW>() as u32;
            if visible {
                startup.StartupInfo.dwFlags = 0;
            }
            unsafe {
                CreateProcessWithLogonW(
                    user.as_ptr(),
                    domain.as_ptr(),
                    account.password.as_ptr(),
                    LOGON_WITH_PROFILE,
                    application.as_ptr(),
                    command.as_mut_ptr(),
                    (if visible {
                        CREATE_NEW_CONSOLE
                    } else {
                        CREATE_NO_WINDOW
                    }) | CREATE_UNICODE_ENVIRONMENT
                        | CREATE_SUSPENDED,
                    environment.as_ptr().cast(),
                    directory.as_ptr(),
                    &startup.StartupInfo,
                    &mut info,
                )
            }
        } else {
            unsafe {
                CreateProcessW(
                    application.as_ptr(),
                    command.as_mut_ptr(),
                    null(),
                    null(),
                    1,
                    CREATE_NO_WINDOW | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT,
                    environment.as_ptr().cast(),
                    directory.as_ptr(),
                    &startup.StartupInfo,
                    &mut info,
                )
            }
        };
        if launched == 0 {
            return Err(format!("owned process creation: {}", os_error()));
        }
        let process = handle(info.hProcess)?;
        if let Some(account) = account {
            let verified = crate::windows_worker_account::verify_process(
                process.as_raw_handle(),
                &account.record.sid,
            );
            if verified.is_err()
                || unsafe { AssignProcessToJobObject(job.as_raw_handle(), process.as_raw_handle()) }
                    == 0
            {
                let error = verified
                    .err()
                    .unwrap_or_else(|| format!("isolated process job assignment: {}", os_error()));
                unsafe {
                    TerminateProcess(process.as_raw_handle(), 1);
                    CloseHandle(info.hThread);
                }
                return Err(error);
            }
            if unsafe { ResumeThread(info.hThread) } == u32::MAX {
                unsafe {
                    TerminateJobObject(job.as_raw_handle(), 1);
                    CloseHandle(info.hThread);
                }
                return Err(os_error());
            }
        }
        drop(handle(info.hThread)?);
        drop(stdin_read);
        drop(stdout_write);
        drop(stderr_write);
        Ok(Spawned {
            child: Self {
                job,
                process,
                pid: info.dwProcessId,
            },
            stdin: File::from(stdin),
            stdout,
            stderr,
        })
    }
    pub fn active(&self) -> Result<u32, String> {
        active(&self.job)
    }
    pub fn exit_code(&self) -> Result<Option<u32>, String> {
        match unsafe { WaitForSingleObject(self.process.as_raw_handle(), 0) } {
            WAIT_TIMEOUT => Ok(None),
            WAIT_OBJECT_0 => {
                let mut code = 0;
                if unsafe { GetExitCodeProcess(self.process.as_raw_handle(), &mut code) } == 0 {
                    return Err(os_error());
                }
                Ok(Some(code))
            }
            _ => Err(os_error()),
        }
    }
    pub fn terminate(&self) -> Result<(), String> {
        if unsafe { TerminateJobObject(self.job.as_raw_handle(), 1) } == 0 {
            Err(os_error())
        } else {
            Ok(())
        }
    }
}
