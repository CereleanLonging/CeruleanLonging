use std::process::{Command, ExitStatus};

#[cfg(windows)]
pub fn run(command: &mut Command) -> Result<ExitStatus, String> {
    use std::os::windows::{io::AsRawHandle, process::CommandExt};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::{Diagnostics::ToolHelp::*, JobObjects::*, Threading::*},
    };
    struct Job(HANDLE);
    impl Drop for Job {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    unsafe {
        let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if handle.is_null() {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let job = Job(handle);
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const _,
            std::mem::size_of_val(&limits) as u32,
        ) == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        command.env("CERULEAN_NATIVE_OWNED_JOB", "1");
        command.creation_flags(CREATE_SUSPENDED);
        let mut child = command.spawn().map_err(|e| e.to_string())?;
        if AssignProcessToJobObject(job.0, child.as_raw_handle() as HANDLE) == 0 {
            let error = std::io::Error::last_os_error().to_string();
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("构建子进程生命周期绑定失败 {error}"));
        }
        let threads = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        let mut resumed = false;
        if threads != windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            let mut entry: THREADENTRY32 = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
            let mut found = Thread32First(threads, &mut entry) != 0;
            while found {
                if entry.th32OwnerProcessID == child.id() {
                    let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                    if !thread.is_null() {
                        let previous = ResumeThread(thread);
                        resumed |= previous > 0 && previous != u32::MAX;
                        CloseHandle(thread);
                    }
                }
                found = Thread32Next(threads, &mut entry) != 0;
            }
            CloseHandle(threads);
        }
        if !resumed {
            let _ = child.kill();
            let _ = child.wait();
            return Err("构建进程绑定完成，但启动线程未能恢复".into());
        }
        let status = child.wait().map_err(|e| e.to_string())?;
        let mut deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut stopped_descendants = false;
        loop {
            let mut accounting: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = std::mem::zeroed();
            if QueryInformationJobObject(
                job.0,
                JobObjectBasicAccountingInformation,
                &mut accounting as *mut _ as *mut _,
                std::mem::size_of_val(&accounting) as u32,
                std::ptr::null_mut(),
            ) == 0
            {
                return Err(std::io::Error::last_os_error().to_string());
            }
            if accounting.ActiveProcesses == 0 {
                return Ok(status);
            }
            if std::time::Instant::now() >= deadline {
                if stopped_descendants {
                    return Err("附属构建进程停止后仍未确认退出，缓存待再次核对".into());
                }
                // The task entry point has finished and reported its result. Reap
                // only descendants still held by this task's private Job, then
                // confirm their exit before allowing cache deletion.
                if TerminateJobObject(job.0, 1) == 0 {
                    return Err(std::io::Error::last_os_error().to_string());
                }
                eprintln!(
                    "构建主程序已退出，正在收尾本次任务残留的 {} 个附属进程",
                    accounting.ActiveProcesses
                );
                stopped_descendants = true;
                deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }
}

#[cfg(not(windows))]
pub fn run(command: &mut Command) -> Result<ExitStatus, String> {
    command.status().map_err(|e| e.to_string())
}
