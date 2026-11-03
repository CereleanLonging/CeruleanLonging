//! Per-model local Windows identities. Credentials are DPAPI-bound to the host
//! user; an absent/mismatched account never falls back to the host identity.
use crate::{artifacts::root_path, host::ordinary};
use cerulean_domain::execution::*;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    ptr,
};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::{Authorization::*, Cryptography::*, *},
};
use zeroize::{Zeroize, Zeroizing};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountRecord {
    pub schema_version: u32,
    pub owner_sid: String,
    pub project_id: String,
    pub identity: String,
    pub shared_root: String,
    pub chief_root: String,
    pub data_root: String,
    pub workspace: String,
    pub username: String,
    pub sid: String,
}
pub(crate) struct Account {
    pub record: AccountRecord,
    pub password: Zeroizing<Vec<u16>>,
    // Names cannot be removed or replaced during the process lifetime.
    _pins: Vec<fs::File>,
}
/// Validate the actual token, including deny-only administrator membership.
/// A matching username alone must never authorize an elevated worker.
pub(crate) fn verify_process(
    process: windows_sys::Win32::Foundation::HANDLE,
    expected: &str,
) -> Result<(), String> {
    use windows_sys::Win32::{Foundation::CloseHandle, System::Threading::OpenProcessToken};
    require(
        crate::ipc::sid_for_process(process).map_err(|e| e.to_string())? == expected,
        "执行进程账户不符",
    )?;
    let mut token = ptr::null_mut();
    require(
        unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } != 0,
        "无法核对执行账户组",
    )?;
    let result = (|| {
        let mut needed = 0;
        unsafe {
            GetTokenInformation(token, TokenGroups, ptr::null_mut(), 0, &mut needed);
        }
        require(
            (std::mem::size_of::<TOKEN_GROUPS>() as u32..=65536).contains(&needed),
            "执行账户组信息无效",
        )?;
        let mut buffer = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
        require(
            unsafe {
                GetTokenInformation(
                    token,
                    TokenGroups,
                    buffer.as_mut_ptr().cast(),
                    needed,
                    &mut needed,
                )
            } != 0,
            "无法读取执行账户组",
        )?;
        let groups = unsafe { &*buffer.as_ptr().cast::<TOKEN_GROUPS>() };
        let offset = std::mem::offset_of!(TOKEN_GROUPS, Groups);
        require(
            offset + groups.GroupCount as usize * std::mem::size_of::<SID_AND_ATTRIBUTES>()
                <= needed as usize,
            "执行账户组长度无效",
        )?;
        for group in unsafe {
            std::slice::from_raw_parts(groups.Groups.as_ptr(), groups.GroupCount as usize)
        } {
            require(
                sid_text(group.Sid)? != "S-1-5-32-544",
                "执行账户已加入管理员组，拒绝启动",
            )?;
        }
        Ok(())
    })();
    unsafe {
        CloseHandle(token);
    }
    result
}

fn verify_desktop_process(account: &Account, pid: u32) -> Result<(), String> {
    with_identity(account, || {
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
        };
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        require(!process.is_null(), "无法读取独立窗口进程")?;
        let result = verify_process(process, &account.record.sid);
        unsafe {
            CloseHandle(process);
        }
        result
    })
}
pub(crate) fn with_identity<T: Send>(
    account: &Account,
    action: impl FnOnce() -> Result<T, String> + Send,
) -> Result<T, String> {
    // Query on a dedicated short-lived thread under the worker identity. The
    // host user cannot ordinarily reopen another user's process token. Never
    // impersonate on the runtime or UI thread, or relax the process DACL.
    std::thread::scope(|scope| {
        scope
            .spawn(move || {
                use windows_sys::Win32::Foundation::CloseHandle;
                let user = account
                    .record
                    .username
                    .encode_utf16()
                    .chain(Some(0))
                    .collect::<Vec<_>>();
                let domain = [b'.' as u16, 0];
                let mut token = ptr::null_mut();
                require(
                    unsafe {
                        LogonUserW(
                            user.as_ptr(),
                            domain.as_ptr(),
                            account.password.as_ptr(),
                            LOGON32_LOGON_INTERACTIVE,
                            LOGON32_PROVIDER_DEFAULT,
                            &mut token,
                        )
                    } != 0,
                    "无法核对独立账户进程",
                )?;
                struct Token(windows_sys::Win32::Foundation::HANDLE);
                impl Drop for Token {
                    fn drop(&mut self) {
                        unsafe {
                            CloseHandle(self.0);
                        }
                    }
                }
                let token = Token(token);
                require(
                    unsafe { ImpersonateLoggedOnUser(token.0) } != 0,
                    "无法读取独立账户进程身份",
                )?;
                struct Impersonation;
                impl Drop for Impersonation {
                    fn drop(&mut self) {
                        unsafe {
                            RevertToSelf();
                        }
                    }
                }
                let _impersonation = Impersonation;
                action()
            })
            .join()
            .map_err(|_| "独立进程身份检查异常".to_string())?
    })
}
pub(crate) fn dos_path(path: &Path) -> Result<PathBuf, String> {
    let text = path.to_str().ok_or("隔离路径编码无效")?;
    let text = text.strip_prefix(r"\\?\").unwrap_or(text);
    require(
        text.len() > 3
            && text.as_bytes()[0].is_ascii_alphabetic()
            && text.as_bytes()[1] == b':'
            && matches!(text.as_bytes()[2], b'\\' | b'/'),
        "Windows隔离账户需要本地磁盘路径",
    )?;
    Ok(PathBuf::from(text))
}
fn bounded(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    ordinary(path, false)?;
    let mut bytes = vec![];
    crate::backup::open(path, false, false)
        .map_err(|e| e.to_string())?
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    require(bytes.len() <= limit, "账户记录超出上限")?;
    Ok(bytes)
}
fn sid_text(sid: PSID) -> Result<String, String> {
    let mut value = ptr::null_mut();
    require(
        unsafe { ConvertSidToStringSidW(sid, &mut value) } != 0,
        "无法读取账户SID",
    )?;
    let mut len = 0;
    unsafe {
        while *value.add(len) != 0 {
            len += 1;
        }
    }
    let result = String::from_utf16(unsafe { std::slice::from_raw_parts(value, len) })
        .map_err(|_| "SID编码无效".to_string());
    unsafe {
        LocalFree(value.cast());
    }
    result
}
/// Reject broad/unknown ACEs, null ACLs and inheritance from public parents.
/// The root allows only host/SYSTEM/Admin, plus the bound model when supplied.
pub(crate) fn check_acl(
    path: &Path,
    owner: &str,
    worker: Option<&str>,
    traverse: bool,
) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let mut sd = ptr::null_mut();
    let mut dacl = ptr::null_mut();
    let mut actual_owner = ptr::null_mut();
    let status = unsafe {
        GetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | OWNER_SECURITY_INFORMATION,
            &mut actual_owner,
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut sd,
        )
    };
    require(status == 0, "无法核对执行隔离ACL")?;
    struct Sd(*mut std::ffi::c_void);
    impl Drop for Sd {
        fn drop(&mut self) {
            unsafe {
                LocalFree(self.0);
            }
        }
    }
    let _sd = Sd(sd);
    require(
        !dacl.is_null() && sid_text(actual_owner)? == owner,
        "隔离目录所有者或ACL不符",
    )?;
    let mut control = 0u16;
    let mut revision = 0;
    require(
        unsafe { GetSecurityDescriptorControl(sd, &mut control, &mut revision) } != 0
            && control & SE_DACL_PROTECTED != 0,
        "隔离目录仍继承外部权限",
    )?;
    let mut host = false;
    let mut system = false;
    let mut admin = false;
    let mut selected = worker.is_none();
    for i in 0..unsafe { (*dacl).AceCount } as u32 {
        let mut ptr_ace = ptr::null_mut();
        require(
            unsafe { GetAce(dacl, i, &mut ptr_ace) } != 0,
            "无法核对ACL条目",
        )?;
        let ace = unsafe { &*ptr_ace.cast::<ACCESS_ALLOWED_ACE>() };
        require(
            ace.Header.AceType == 0 && ace.Header.AceFlags & INHERIT_ONLY_ACE as u8 == 0,
            "隔离ACL包含未经批准的条目",
        )?;
        let sid = sid_text((&ace.SidStart as *const u32).cast_mut().cast())?;
        match sid.as_str() {
            "S-1-5-18" => {
                system = true;
                require(ace.Mask == 0x1f01ff, "SYSTEM权限不完整")?;
            }
            "S-1-5-32-544" => {
                admin = true;
                require(ace.Mask == 0x1f01ff, "管理员权限不完整")?;
            }
            s if s == owner => {
                host = true;
                require(ace.Mask == 0x1f01ff, "管理账户权限不完整")?;
            }
            s if Some(s) == worker => {
                selected = true;
                require(ace.Mask == 0x1301bf, "执行账户权限不符")?;
            }
            s if traverse && s.starts_with("S-1-5-21-") => {
                require(
                    matches!(ace.Mask, 0x20 | 0x100020) && ace.Header.AceFlags == 0,
                    "共享父目录仅允许穿越",
                )?;
            }
            _ => return Err("隔离目录存在其他账户访问权限".into()),
        }
    }
    require(host && system && admin && selected, "隔离ACL缺少必需权限")
}
fn decrypt(bytes: &[u8]) -> Result<Zeroizing<Vec<u16>>, String> {
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: ptr::null_mut(),
    };
    require(
        unsafe {
            CryptUnprotectData(
                &input,
                ptr::null_mut(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } != 0,
        "执行账户凭证无法由当前Windows用户解密",
    )?;
    let slice = unsafe { std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize) };
    let result = if slice.len() <= 1024 && slice.len() % 2 == 0 {
        let mut chars = Zeroizing::new(
            slice
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes(*b))
                .collect::<Vec<_>>(),
        );
        if chars.is_empty() || chars.contains(&0) {
            Err("执行账户凭证无效".into())
        } else {
            chars.push(0);
            Ok(chars)
        }
    } else {
        Err("执行账户凭证无效".into())
    };
    slice.zeroize();
    unsafe {
        LocalFree(output.pbData.cast());
    }
    result
}
pub(crate) fn directory(data: &Path, project: &str, identity: &str) -> Result<PathBuf, String> {
    identifier(project)?;
    sha256(identity)?;
    Ok(data.join("worker-accounts").join(project).join(identity))
}
pub(crate) fn load(data: &Path, plan: &PlanRevision, task: &str) -> Result<Account, String> {
    let binding = plan
        .task(task)?
        .worker_directory
        .as_ref()
        .ok_or("执行目录未绑定")?;
    binding.validate(plan.task(task)?)?;
    load_binding(data, &plan.project, binding)
}
pub(crate) fn desktop(
    data: &Path,
    plan: &PlanRevision,
    task: &str,
) -> Result<Option<Account>, String> {
    let selected = plan.task(task)?;
    crate::blind_review_guard::require_isolated_execution(
        data,
        selected.worker_directory.is_some(),
    )?;
    if selected.worker_directory.is_none() {
        return Ok(None);
    }
    let desktop = selected.desktop_worker.as_ref().ok_or("桌面选择缺失")?;
    let account = load(data, plan, task)?;
    require(
        desktop.project_root == account.record.workspace,
        "桌面工作目录与执行模型不符",
    )?;
    verify_desktop_process(&account, desktop.binding.observation.window.pid)?;
    Ok(Some(account))
}
pub(crate) fn load_binding(
    data: &Path,
    project: &Project,
    binding: &WorkerDirectory,
) -> Result<Account, String> {
    let folder = directory(data, &project.id, &binding.identity)?;
    require(
        !folder.join("check-quarantine.json").exists(),
        "该执行账户存在未完成的检查权限回收，请先核对原检查；不会继续执行",
    )?;
    require(
        !folder.join("check-active-lease.json").exists(),
        "该账户的原检查权限尚未确认回收，请核对原操作；不会启动新进程",
    )?;
    root_path(folder.to_str().ok_or("账户路径无效")?)
        .map_err(|_| "该模型的Windows隔离账户尚未就绪，请先设置账户；不会使用当前账户执行")?;
    let owner = crate::ipc::current_sid().map_err(|e| e.to_string())?;
    check_acl(&folder, &owner, None, false)?;
    let record: AccountRecord =
        serde_json::from_slice(&bounded(&folder.join("account.json"), 8192)?)
            .map_err(|_| "执行账户登记无效")?;
    require(
        record.schema_version == 1
            && record.owner_sid == owner
            && record.project_id == project.id
            && record.identity == binding.identity
            && record.shared_root == project.root
            && record.workspace == binding.root(&project.root)?
            && root_path(&record.data_root)? == root_path(data.to_str().ok_or("数据路径无效")?)?
            && record.username
                == format!(
                    "CLW{}",
                    &digest(&(
                        record.data_root.as_str(),
                        project.id.as_str(),
                        binding.identity.as_str()
                    ))?[..16]
                )
            && record.sid.starts_with("S-1-5-21-")
            && record.sid != owner,
        "执行账户与当前项目/模型目录不符",
    )?;
    let mut pins = vec![];
    for (p, worker, traverse) in [
        (PathBuf::from(&record.chief_root), None, false),
        (data.to_owned(), None, false),
        (PathBuf::from(&record.shared_root), None, true),
        (Path::new(&record.shared_root).join("executors"), None, true),
        (
            PathBuf::from(&record.workspace),
            Some(record.sid.as_str()),
            false,
        ),
    ] {
        root_path(p.to_str().ok_or("隔离目录编码无效")?)?;
        check_acl(&p, &owner, worker, traverse)?;
        pins.push(crate::backup::open(&p, true, false).map_err(|e| e.to_string())?);
    }
    let password = decrypt(&bounded(&folder.join("password.dpapi"), 16384)?)?;
    Ok(Account {
        record,
        password,
        _pins: pins,
    })
}

pub(crate) struct PreparedAccounts {
    plan: PathBuf,
    _pins: Vec<fs::File>,
}
#[cfg(test)]
impl PreparedAccounts {
    pub(crate) fn plan_path_for_test(&self) -> &Path {
        &self.plan
    }
}
pub(crate) fn private_directory(path: &Path, owner: &str) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    if path.exists() {
        ordinary(path, true)?;
        return check_acl(path, owner, None, false);
    }
    let sddl = format!("O:{owner}D:P(A;OICI;FA;;;{owner})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)\0")
        .encode_utf16()
        .collect::<Vec<_>>();
    let mut descriptor = ptr::null_mut();
    require(
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } != 0,
        "无法创建私有账户设置目录",
    )?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        windows_sys::Win32::Storage::FileSystem::CreateDirectoryW(wide.as_ptr(), &attributes)
    };
    let error = std::io::Error::last_os_error();
    unsafe {
        LocalFree(descriptor);
    }
    require(result != 0, &format!("账户设置目录无法创建：{error}"))?;
    check_acl(path, owner, None, false)
}
/// Grant only a newly-created check workspace to its isolated account. This is
/// never applied to the core root, sealed inputs, credentials or evidence files.
pub(crate) fn grant_check_workspace(path: &Path, account: &Account) -> Result<(), String> {
    check_workspace_acl(path, account, true)
}
pub(crate) fn revoke_check_workspace(path: &Path, account: &Account) -> Result<(), String> {
    check_workspace_acl(path, account, false)
}
fn check_workspace_acl(path: &Path, account: &Account, allow: bool) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    let extra = if allow {
        format!("(A;OICI;0x1301bf;;;{})", account.record.sid)
    } else {
        String::new()
    };
    // OWNER RIGHTS suppresses the implicit WRITE_DAC of files created by the
    // checker, so it cannot remove host access or grant itself access after exit.
    let sddl = format!(
        "D:P(A;OICI;FA;;;{})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;RC;;;OW){}\0",
        account.record.owner_sid, extra
    )
    .encode_utf16()
    .collect::<Vec<_>>();
    let mut descriptor = ptr::null_mut();
    require(
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                ptr::null_mut(),
            )
        } != 0,
        "检查目录ACL无法构造",
    )?;
    let result = (|| {
        let mut present = 0;
        let mut defaulted = 0;
        let mut acl = ptr::null_mut();
        require(
            unsafe {
                GetSecurityDescriptorDacl(descriptor, &mut present, &mut acl, &mut defaulted)
            } != 0
                && present != 0
                && !acl.is_null(),
            "检查目录ACL无效",
        )?;
        let mut pending = vec![path.to_owned()];
        let mut count = 0;
        while let Some(p) = pending.pop() {
            count += 1;
            require(count <= 20000, "检查目录条目超限")?;
            let meta = fs::symlink_metadata(&p).map_err(|e| e.to_string())?;
            ordinary(&p, meta.is_dir())?;
            let wide = p
                .as_os_str()
                .encode_wide()
                .chain(Some(0))
                .collect::<Vec<_>>();
            require(
                unsafe {
                    SetNamedSecurityInfoW(
                        wide.as_ptr(),
                        SE_FILE_OBJECT,
                        DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
                        ptr::null_mut(),
                        ptr::null_mut(),
                        acl,
                        ptr::null_mut(),
                    )
                } == 0,
                "检查目录ACL设置失败",
            )?;
            if meta.is_dir() {
                for e in fs::read_dir(&p).map_err(|e| e.to_string())? {
                    pending.push(e.map_err(|e| e.to_string())?.path());
                }
            }
        }
        Ok(())
    })();
    unsafe {
        LocalFree(descriptor);
    }
    result
}
pub(crate) fn with_check_workspaces<T>(
    paths: &[&Path],
    account: &Account,
    action: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    use std::io::Write;
    let folder = directory(
        Path::new(&account.record.data_root),
        &account.record.project_id,
        &account.record.identity,
    )?;
    let lease = folder.join("check-active-lease.json");
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&lease)
        .map_err(|_| "已有检查权限租约或无法保存；未开放检查目录")?;
    file.write_all(
        &serde_json::to_vec(
            &serde_json::json!({"schema":1,"paths":paths,"sid":account.record.sid}),
        )
        .map_err(|e| e.to_string())?,
    )
    .and_then(|_| file.sync_all())
    .map_err(|e| e.to_string())?;
    drop(file);
    struct Grant<'a> {
        paths: &'a [&'a Path],
        account: &'a Account,
        closed: bool,
    }
    impl Drop for Grant<'_> {
        fn drop(&mut self) {
            if !self.closed {
                for p in self.paths {
                    let _ = revoke_check_workspace(p, self.account);
                }
            }
        }
    }
    let mut guard = Grant {
        paths,
        account,
        closed: false,
    };
    let result = (|| {
        for p in paths {
            grant_check_workspace(p, account)?;
        }
        action()
    })();
    let cleanup = paths
        .iter()
        .filter_map(|p| revoke_check_workspace(p, account).err())
        .collect::<Vec<_>>();
    guard.closed = true;
    if !cleanup.is_empty() {
        let error = cleanup.join("；");
        let marker = directory(
            Path::new(&account.record.data_root),
            &account.record.project_id,
            &account.record.identity,
        )?
        .join("check-quarantine.json");
        if !marker.exists() {
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&marker)
                .map_err(|e| format!("检查权限回收及账户隔离标记均失败：{e}"))?;
            file.write_all(b"{\"reason\":\"check-workspace-permission-cleanup-failed\"}")
                .and_then(|_| file.sync_all())
                .map_err(|e| e.to_string())?;
        }
        return Err(format!(
            "检查目录权限回收失败，该账户已阻止再次执行，原检查不计通过：{error}"
        ));
    }
    fs::remove_file(lease).map_err(|e| format!("检查权限已回收，但租约尚未关闭：{e}"))?;
    result
}
// Explicit user action only. The marker is durable before UAC; cancellation or
// interruption never schedules another elevation automatically.
pub(crate) fn prepare(
    data: &Path,
    project: &Project,
    chief: &str,
    bindings: &std::collections::BTreeMap<String, WorkerDirectory>,
) -> Result<PreparedAccounts, String> {
    identifier(&project.id)?;
    require(
        (1..=5).contains(&bindings.len()),
        "账户设置需要一至五个已登记目录",
    )?;
    let owner = crate::ipc::current_sid().map_err(|e| e.to_string())?;
    let mut pins = vec![crate::backup::open(data, true, false).map_err(|e| e.to_string())?];
    let folder = data.join("worker-account-setup");
    private_directory(&folder, &owner)?;
    pins.push(crate::backup::open(&folder, true, false).map_err(|e| e.to_string())?);
    let slots=bindings.values().map(|b|Ok(serde_json::json!({"identity":b.identity,"workspace":b.root(&project.root)?,
        "username":format!("CLW{}",&digest(&(data.to_str().ok_or("数据路径无效")?,project.id.as_str(),b.identity.as_str()))?[..16])}))).collect::<Result<Vec<_>,String>>()?;
    let value = serde_json::json!({"schemaVersion":1,"ownerSid":owner,"projectId":project.id,"dataRoot":data,"sharedRoot":project.root,"chiefRoot":chief,"slots":slots});
    let id = format!(
        "{}-{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%S%.9f"),
        digest(&value)?
    );
    let folder = folder.join(id);
    private_directory(&folder, &owner)?;
    pins.push(crate::backup::open(&folder, true, false).map_err(|e| e.to_string())?);
    let plan = folder.join("plan.json");
    let script = folder.join("provision.ps1");
    let script_bytes = include_bytes!("../../../scripts/provision-worker-accounts.ps1");
    if script.exists() {
        require(
            bounded(&script, 128 * 1024)? == script_bytes,
            "账户设置脚本已变化，请核对",
        )?;
    } else {
        fs::write(&script, script_bytes).map_err(|e| e.to_string())?;
    }
    if !plan.exists() {
        fs::write(
            &plan,
            serde_json::to_vec(&value).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
    }
    for path in [&script, &plan] {
        pins.push(crate::backup::open(path, false, false).map_err(|e| e.to_string())?);
    }
    Ok(PreparedAccounts { plan, _pins: pins })
}

pub(crate) fn elevate(plan: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    let plan = dos_path(plan)?;
    let script = plan.parent().ok_or("设置目录缺失")?.join("provision.ps1");
    // Quoted literals only, never concatenate untrusted text as PowerShell code.
    let quote = |p: &Path| format!("'{}'", p.to_string_lossy().replace('\'', "''"));
    // An already elevated host can use its token. Otherwise Windows owns the
    // one consent prompt; no auto-clicking, UAC policy change or persistent broker.
    let text = elevation_script(host_is_elevated()?, &quote(&script), &quote(&plan));
    let shell = PathBuf::from(std::env::var_os("SystemRoot").ok_or("SystemRoot缺失")?)
        .join("System32/WindowsPowerShell/v1.0/powershell.exe");
    let status = std::process::Command::new(shell)
        .args(["-NoProfile", "-NonInteractive", "-Command", &text])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x08000000)
        .status()
        .map_err(|_| "无法发起账户设置")?;
    require(status.success(), "账户设置取消或失败；不会自动重弹UAC")
}
pub(crate) fn provision_script_digest() -> String {
    use sha2::{Digest, Sha256};
    format!(
        "{:x}",
        Sha256::digest(include_bytes!(
            "../../../scripts/provision-worker-accounts.ps1"
        ))
    )
}
fn elevation_script(elevated: bool, script: &str, plan: &str) -> String {
    let verb = if elevated { "" } else { "-Verb RunAs " };
    format!("$ErrorActionPreference='Stop'; $workerSetupProcess=Start-Process -FilePath (Join-Path $env:SystemRoot 'System32/WindowsPowerShell/v1.0/powershell.exe') {verb}-WindowStyle Hidden -ArgumentList @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',('\"'+{script}+'\"'),'-PlanPath',('\"'+{plan}+'\"')) -Wait -PassThru; exit $workerSetupProcess.ExitCode")
}
fn host_is_elevated() -> Result<bool, String> {
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };
    let mut token = ptr::null_mut();
    require(
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } != 0,
        "无法核对工作台当前权限",
    )?;
    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut size = 0;
    let ok = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut size,
        )
    };
    unsafe {
        CloseHandle(token);
    }
    require(
        ok != 0 && size as usize == std::mem::size_of::<TOKEN_ELEVATION>(),
        "无法读取工作台当前权限",
    )?;
    Ok(elevation.TokenIsElevated != 0)
}
pub(crate) fn start(prepared: PreparedAccounts) -> Result<(), String> {
    std::thread::Builder::new()
        .name("worker-account-setup".into())
        .spawn(move || {
            let plan = prepared.plan.clone();
            let outcome = elevate(&plan);
            let receipt = plan.with_extension("launch.json");
            let value = serde_json::json!({"success":outcome.is_ok(),"error":outcome.err()});
            if let Ok(mut f) = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(receipt)
            {
                use std::io::Write;
                if let Ok(bytes) = serde_json::to_vec(&value) {
                    let _ = f.write_all(&bytes).and_then(|_| f.sync_all());
                }
            }
            drop(prepared);
        })
        .map_err(|_| "无法启动账户设置后台")?;
    Ok(())
}
#[cfg(debug_assertions)]
pub fn probe_setup(plan_file: &Path) -> Result<serde_json::Value, String> {
    let value: serde_json::Value =
        serde_json::from_slice(&bounded(plan_file, 64 * 1024)?).map_err(|_| "探测设置无效")?;
    let field = |name: &str| {
        value[name]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| "探测目录缺失".to_string())
    };
    let data = PathBuf::from(field("dataRoot")?);
    let project = Project {
        id: field("projectId")?,
        root: field("sharedRoot")?,
        timezone: "UTC".into(),
    };
    let slots = value["slots"].as_array().ok_or("探测模型缺失")?;
    let mut bindings = std::collections::BTreeMap::new();
    for slot in slots {
        let id = slot["identity"].as_str().ok_or("探测身份缺失")?;
        sha256(id)?;
        bindings.insert(
            id.into(),
            WorkerDirectory {
                identity: id.into(),
                relative_path: format!("executors/{id}"),
            },
        );
    }
    let prepared = prepare(&data, &project, &field("chiefRoot")?, &bindings)?;
    elevate(&prepared.plan)?;
    let receipt = prepared.plan.with_extension("result.json");
    let result: serde_json::Value =
        serde_json::from_slice(&bounded(&receipt, 64 * 1024)?).map_err(|_| "账户设置回执无效")?;
    require(result["success"] == true, "账户设置未成功")?;
    Ok(serde_json::json!({"passed":true,"receipt":receipt,"result":result}))
}
pub(crate) fn views(data: &Path) -> Vec<cerulean_domain::project_setup::WorkerAccountSetupView> {
    let mut latest = std::collections::BTreeMap::new();
    let mut entries = match fs::read_dir(data.join("worker-account-setup")) {
        Ok(e) => e
            .filter_map(Result::ok)
            .map(|e| e.path())
            .collect::<Vec<_>>(),
        Err(_) => return vec![],
    };
    entries.sort();
    for folder in entries.iter().rev().take(256) {
        if ordinary(folder, true).is_err() {
            continue;
        }
        let path = folder.join("plan.json");
        let plan = bounded(&path, 64 * 1024)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
        let Some(project) = plan.as_ref().and_then(|p| p["projectId"].as_str()) else {
            continue;
        };
        if latest.contains_key(project) {
            continue;
        }
        let receipt = path.with_extension("result.json");
        let result = bounded(&receipt, 64 * 1024)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
        let launch = bounded(&path.with_extension("launch.json"), 8192)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
        let (state, message) = if let Some(r) = result {
            if r["success"] == true {
                ("ready", None)
            } else {
                ("failed", r["error"].as_str().map(str::to_owned))
            }
        } else if launch.is_some() {
            (
                "failed",
                Some("账户设置未完成或UAC已取消；不会自动重弹".into()),
            )
        } else {
            (
                "pending",
                Some("等待本次Windows账户设置；中断后须核对原事务".into()),
            )
        };
        latest.insert(
            project.to_owned(),
            cerulean_domain::project_setup::WorkerAccountSetupView {
                project_id: project.into(),
                state: state.into(),
                message,
                receipt_path: receipt.to_string_lossy().into(),
            },
        );
    }
    latest.into_values().collect()
}

/// Offline integration probe; only caller-created fixture roots should be used.
/// Tests actual logon and file access using the same launcher as CLI execution.
#[cfg(debug_assertions)]
pub fn probe(plan_file: &Path) -> Result<serde_json::Value, String> {
    use std::io::Read;
    let value: serde_json::Value =
        serde_json::from_slice(&bounded(plan_file, 64 * 1024)?).map_err(|_| "探测设置无效")?;
    let field = |name: &str| {
        value[name]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| "探测目录缺失".to_string())
    };
    let data = PathBuf::from(field("dataRoot")?);
    let project = Project {
        id: field("projectId")?,
        root: field("sharedRoot")?,
        timezone: "UTC".into(),
    };
    let slots = value["slots"].as_array().ok_or("探测模型缺失")?;
    require(slots.len() <= 5 && !slots.is_empty(), "探测目录数量无效")?;
    let mut results = vec![];
    for slot in slots {
        let identity = slot["identity"].as_str().ok_or("探测身份缺失")?;
        let binding = WorkerDirectory {
            identity: identity.into(),
            relative_path: format!("executors/{identity}"),
        };
        let account = load_binding(&data, &project, &binding)?;
        let own = PathBuf::from(&account.record.workspace);
        let denied = std::iter::once(account.record.chief_root.clone())
            .chain(std::iter::once(account.record.data_root.clone()))
            .chain(std::iter::once(project.root.clone()))
            .chain(
                slots
                    .iter()
                    .filter(|s| s["identity"] != identity)
                    .map(|s| s["workspace"].as_str().unwrap_or("").to_owned()),
            )
            .collect::<Vec<_>>();
        for directory in &denied {
            let marker = Path::new(directory).join("managed-read-probe-764.txt");
            if marker.exists() {
                require(
                    bounded(&marker, 64)? == b"private-fixture-marker",
                    "探测标记与原文件冲突",
                )?;
            } else {
                use std::io::Write;
                let mut f = fs::OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(marker)
                    .map_err(|e| e.to_string())?;
                f.write_all(b"private-fixture-marker")
                    .map_err(|e| e.to_string())?;
            }
        }
        let quote = |s: &str| format!("'{}'", s.replace('\'', "''"));
        let script = format!(
            r#"$ErrorActionPreference='Stop';$own={};$denied=@({});$results=@();$file=[IO.Path]::Combine($own,'account-probe.txt');[IO.File]::WriteAllText($file,'own-directory-ok');$ownOK=([IO.File]::ReadAllText($file)-eq'own-directory-ok');foreach($p in $denied){{$readDenied=$false;$writeDenied=$false;$fileReadDenied=$false;try{{$null=[IO.File]::ReadAllText([IO.Path]::Combine($p,'managed-read-probe-764.txt'))}}catch [UnauthorizedAccessException]{{$fileReadDenied=$true}};try{{$null=[IO.Directory]::GetFileSystemEntries($p)}}catch [UnauthorizedAccessException]{{$readDenied=$true}};try{{[IO.File]::WriteAllText([IO.Path]::Combine($p,'must-not-write-account-probe.txt'),'FAIL')}}catch [UnauthorizedAccessException]{{$writeDenied=$true}};$results+=@{{path=$p;readDenied=$readDenied;writeDenied=$writeDenied;fileReadDenied=$fileReadDenied}}}};$id=[Security.Principal.WindowsIdentity]::GetCurrent();$pr=[Security.Principal.WindowsPrincipal]::new($id);@{{sid=$id.User.Value;administrator=$pr.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator);ownReadWrite=$ownOK;denied=$results}}|ConvertTo-Json -Compress -Depth 4"#,
            quote(&account.record.workspace),
            denied
                .iter()
                .map(|s| quote(s))
                .collect::<Vec<_>>()
                .join(",")
        );
        let system = std::env::var("SystemRoot").map_err(|_| "SystemRoot missing")?;
        let environment = format!(
            "SYSTEMROOT={system}\0TEMP={}\0TMP={}\0WINDIR={system}\0\0",
            own.display(),
            own.display()
        )
        .encode_utf16()
        .collect::<Vec<_>>();
        let exe = PathBuf::from(&system).join("System32/WindowsPowerShell/v1.0/powershell.exe");
        let script_path = own.join(format!("account-probe-{}.ps1", &digest(&script)?[..12]));
        let script_bytes = [&[0xef, 0xbb, 0xbf][..], script.as_bytes()].concat();
        if script_path.exists() {
            require(
                bounded(&script_path, 65536)? == script_bytes,
                "原探测脚本已变化",
            )?;
        } else {
            use std::io::Write;
            let mut f = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&script_path)
                .map_err(|e| e.to_string())?;
            f.write_all(&script_bytes)
                .and_then(|_| f.sync_all())
                .map_err(|e| e.to_string())?;
        }
        let _script_pin =
            crate::backup::open(&script_path, false, false).map_err(|e| e.to_string())?;
        let args = vec![
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-ExecutionPolicy".into(),
            "Bypass".into(),
            "-File".into(),
            script_path.to_string_lossy().into(),
        ];
        let spawned = crate::verifier::process::interactive::InteractiveChild::spawn_as(
            &exe,
            &args,
            &own,
            &environment,
            &account,
        )?;
        verify_desktop_process(&account, spawned.child.pid)?;
        require(
            verify_desktop_process(&account, std::process::id()).is_err(),
            "错误账户进程未被拒绝",
        )?;
        drop(spawned.stdin);
        let out = std::thread::spawn(move || {
            let mut bytes = vec![];
            spawned
                .stdout
                .take(65537)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            Ok::<_, String>(bytes)
        });
        let err = std::thread::spawn(move || {
            let mut bytes = vec![];
            spawned
                .stderr
                .take(8193)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            Ok::<_, String>(bytes)
        });
        let until = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while spawned.child.active()? != 0 && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        if spawned.child.active()? != 0 {
            spawned.child.terminate()?;
            return Err("账户隔离探测超时，已关闭测试进程树".into());
        }
        let stdout = out.join().map_err(|_| "探测输出读取失败")??;
        let stderr = err.join().map_err(|_| "探测错误读取失败")??;
        require(
            spawned.child.exit_code()? == Some(0) && stderr.is_empty() && stdout.len() <= 65536,
            "账户隔离探测进程失败",
        )?;
        let result: serde_json::Value =
            serde_json::from_slice(&stdout).map_err(|_| "账户隔离探测回执无效")?;
        require(
            result["sid"] == account.record.sid
                && result["administrator"] == false
                && result["ownReadWrite"] == true
                && result["denied"].as_array().is_some_and(|items| {
                    items.len() == denied.len()
                        && items.iter().all(|r| {
                            r["readDenied"] == true
                                && r["writeDenied"] == true
                                && r["fileReadDenied"] == true
                        })
                }),
            "跨账户目录访问未被拒绝",
        )?;
        results.push(result);
    }
    Ok(serde_json::json!({"passed":true,"modelRequests":0,"accounts":results}))
}

struct CodexOwner {
    child: crate::verifier::process::interactive::InteractiveChild,
    _account: Account,
    diagnostics: tokio::task::JoinHandle<()>,
}
impl Drop for CodexOwner {
    fn drop(&mut self) {
        self.diagnostics.abort();
    }
}
impl cerulean_adapters::codex::OwnedCodexProcess for CodexOwner {
    fn try_wait(&mut self) -> Result<Option<bool>, String> {
        if self.child.active()? != 0 {
            return Ok(None);
        }
        Ok(self.child.exit_code()?.map(|c| c == 0))
    }
    fn terminate(&mut self) -> Result<(), String> {
        self.child.terminate()
    }
}
pub(crate) fn codex(
    data: &Path,
    plan: &PlanRevision,
    task: &str,
    executable: &Path,
    workspace: &Path,
    readonly: bool,
) -> Result<cerulean_adapters::codex::CodexClient, String> {
    let account = load(data, plan, task)?;
    codex_with_account(account, executable, workspace, readonly)
}
fn codex_with_account(
    account: Account,
    executable: &Path,
    workspace: &Path,
    readonly: bool,
) -> Result<cerulean_adapters::codex::CodexClient, String> {
    use tokio::io::AsyncReadExt;
    require(
        root_path(workspace.to_str().ok_or("工作目录无效")?)?
            == root_path(&account.record.workspace)?,
        "启动目录与账户绑定不符",
    )?;
    let (environment, _) = runtime_environment(workspace)?;
    let args = [
        "app-server",
        "--listen",
        "stdio://",
        "-c",
        "features.hooks=false",
        "-c",
        "features.apps=false",
        "-c",
        "features.multi_agent=false",
        "-c",
        "web_search=\"disabled\"",
    ]
    .map(String::from);
    let spawned = crate::verifier::process::interactive::InteractiveChild::spawn_as(
        executable,
        &args,
        workspace,
        &environment,
        &account,
    )?;
    let mut stderr = tokio::fs::File::from_std(spawned.stderr);
    let diagnostics = tokio::spawn(async move {
        let mut buf = zeroize::Zeroizing::new([0u8; 8192]);
        while let Ok(n) = stderr.read(&mut buf[..]).await {
            if n == 0 {
                break;
            }
        }
    });
    let owner = Box::new(CodexOwner {
        child: spawned.child,
        _account: account,
        diagnostics,
    });
    cerulean_adapters::codex::CodexClient::from_owned_process(
        owner,
        Box::new(tokio::fs::File::from_std(spawned.stdout)),
        Box::new(tokio::fs::File::from_std(spawned.stdin)),
        workspace,
        readonly,
    )
}
fn runtime_environment(workspace: &Path) -> Result<(Vec<u16>, PathBuf), String> {
    let runtime = workspace.join(".cerulean-runtime");
    if !runtime.exists() {
        fs::create_dir(&runtime).map_err(|e| e.to_string())?;
    }
    ordinary(&runtime, true)?;
    for name in ["home", "appdata", "localappdata", "tmp", "codex"] {
        let p = runtime.join(name);
        if !p.exists() {
            fs::create_dir(&p).map_err(|e| e.to_string())?;
        }
        ordinary(&p, true)?;
    }
    let system = std::env::var("SystemRoot").map_err(|_| "SystemRoot缺失")?;
    // Windows shell components expand %SystemDrive% even with a custom user
    // profile. Without it they can create a literal relative cache directory.
    // Derive the drive from SystemRoot; do not inherit the host environment.
    let system_drive = match Path::new(&system).components().next() {
        Some(std::path::Component::Prefix(prefix)) => match prefix.kind() {
            std::path::Prefix::Disk(drive) | std::path::Prefix::VerbatimDisk(drive) => {
                format!("{}:", char::from(drive))
            }
            _ => return Err("SystemRoot必须位于本地系统盘".into()),
        },
        _ => return Err("SystemRoot必须位于本地系统盘".into()),
    };
    let values = std::collections::BTreeMap::from([
        ("SYSTEMDRIVE", system_drive),
        ("SYSTEMROOT", system.clone()),
        ("WINDIR", system.clone()),
        ("PATH", format!("{system}\\System32;{system}")),
        ("USERPROFILE", runtime.join("home").to_string_lossy().into()),
        ("APPDATA", runtime.join("appdata").to_string_lossy().into()),
        (
            "LOCALAPPDATA",
            runtime.join("localappdata").to_string_lossy().into(),
        ),
        ("TEMP", runtime.join("tmp").to_string_lossy().into()),
        ("TMP", runtime.join("tmp").to_string_lossy().into()),
        ("CODEX_HOME", runtime.join("codex").to_string_lossy().into()),
    ]);
    let mut environment = vec![];
    for (k, v) in values {
        require(!v.contains('\0'), "启动环境无效")?;
        environment.extend(format!("{k}={v}").encode_utf16());
        environment.push(0);
    }
    environment.push(0);
    Ok((environment, runtime))
}
pub(crate) fn open_login(
    data: &Path,
    project: &Project,
    task: &TaskExecution,
) -> Result<(), String> {
    if let Some(desktop) = &task.desktop_worker {
        desktop.binding.observation.window.app.require_available()?;
    }
    use fs2::FileExt;
    require(
        task.adapter_id == "codex" || task.desktop_worker.is_some(),
        "此接入不需要独立交互登录",
    )?;
    let binding = task
        .worker_directory
        .as_ref()
        .ok_or("请先启用角色目录隔离")?;
    binding.validate(task)?;
    let account = load_binding(data, project, binding)?;
    let folder = directory(data, &project.id, &binding.identity)?;
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(folder.join("interactive.lock"))
        .map_err(|e| e.to_string())?;
    lock.try_lock_exclusive()
        .map_err(|_| "该模型的独立窗口仍在运行，请使用原窗口")?;
    let workspace = PathBuf::from(&account.record.workspace);
    let (environment, runtime) = runtime_environment(&workspace)?;
    let path = task
        .worker
        .as_ref()
        .map(|w| w.executable.path.as_str())
        .or_else(|| {
            task.desktop_worker
                .as_ref()
                .map(|w| w.binding.observation.window.exe.as_str())
        })
        .ok_or("程序路径缺失")?;
    let pin = crate::backup::open(Path::new(path), false, false)
        .map_err(|_| "程序不可用；请为独立账户安装可读取的程序")?;
    if let Some(worker) = &task.worker {
        use sha2::{Digest, Sha256};
        let mut reader = &pin;
        let mut hash = Sha256::new();
        let mut buf = [0u8; 65536];
        loop {
            let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            hash.update(&buf[..n]);
        }
        require(
            format!("{:x}", hash.finalize()) == worker.executable.sha256,
            "登录程序已变化，请重新保存配置",
        )?;
    }
    let (exe, args) = if task.adapter_id == "codex" {
        let system = std::env::var_os("SystemRoot").ok_or("SystemRoot缺失")?;
        (
            PathBuf::from(system).join("System32/WindowsPowerShell/v1.0/powershell.exe"),
            vec![
                "-NoProfile".into(),
                "-NoExit".into(),
                "-Command".into(),
                format!("& '{}' login", path.replace('\'', "''")),
            ],
        )
    } else {
        (
            PathBuf::from(path),
            vec![format!(
                "--user-data-dir={}",
                runtime.join("desktop").display()
            )],
        )
    };
    let spawned = crate::verifier::process::interactive::InteractiveChild::spawn_visible_as(
        &exe,
        &args,
        &workspace,
        &environment,
        &account,
    )?;
    std::thread::Builder::new()
        .name("isolated-worker-window".into())
        .spawn(move || {
            let _guards = (account, pin, lock);
            drop(spawned.stdin);
            drop(spawned.stdout);
            drop(spawned.stderr);
            while spawned.child.active().is_ok_and(|n| n > 0) {
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        })
        .map_err(|_| "无法保留独立窗口")?;
    Ok(())
}

#[cfg(test)]
#[path = "windows_worker_native_tests.rs"]
mod native_tests;

#[cfg(test)]
mod tests {
    #[test]
    fn account_elevation_uses_existing_token_or_one_windows_prompt() {
        let plain = super::elevation_script(false, "'fixed.ps1'", "'fixed.json'");
        let elevated = super::elevation_script(true, "'fixed.ps1'", "'fixed.json'");
        assert_eq!(plain.matches("-Verb RunAs").count(), 1);
        assert!(!elevated.contains("-Verb"));
        for script in [&plain, &elevated] {
            assert!(script.contains("-WindowStyle Hidden"));
            assert!(script.contains("-Wait -PassThru"));
            assert!(!script.contains("while"));
            assert!(!script.contains("SendKeys"));
        }
        super::host_is_elevated().unwrap(); // Real read-only Windows token query.
    }
    use super::*;
    #[test]
    fn isolated_environment_expands_windows_system_drive() {
        use std::os::windows::process::CommandExt;
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().canonicalize().unwrap();
        let (block, runtime) = runtime_environment(&workspace).unwrap();
        assert!(block.ends_with(&[0, 0]));
        let values: std::collections::BTreeMap<_, _> = block
            .split(|c| *c == 0)
            .filter(|entry| !entry.is_empty())
            .map(|entry| {
                let text = String::from_utf16(entry).unwrap();
                let (key, value) = text.split_once('=').unwrap();
                (key.to_owned(), value.to_owned())
            })
            .collect();
        assert_eq!(
            values.keys().map(String::as_str).collect::<Vec<_>>(),
            [
                "APPDATA",
                "CODEX_HOME",
                "LOCALAPPDATA",
                "PATH",
                "SYSTEMDRIVE",
                "SYSTEMROOT",
                "TEMP",
                "TMP",
                "USERPROFILE",
                "WINDIR"
            ]
        );
        assert_eq!(
            values["USERPROFILE"],
            runtime.join("home").to_string_lossy()
        );
        assert_eq!(
            values["CODEX_HOME"],
            runtime.join("codex").to_string_lossy()
        );
        let result = std::process::Command::new(
            Path::new(&values["SYSTEMROOT"])
                .join("System32")
                .join("cmd.exe"),
        )
        .env_clear()
        .envs(&values)
        .current_dir(dos_path(&runtime).unwrap())
        .args(["/D", "/C"])
        .raw_arg("echo %SystemDrive%\\ProgramData")
        .output()
        .unwrap();
        assert!(
            result.status.success(),
            "status={:?}, stdout={}, stderr={}",
            result.status.code(),
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            String::from_utf8(result.stdout).unwrap().trim(),
            format!("{}\\ProgramData", values["SYSTEMDRIVE"])
        );
        assert!(!runtime.join("%SystemDrive%").exists());
    }
    #[tokio::test]
    #[ignore = "requires explicitly provisioned disposable Windows account and credential-free Codex executable"]
    async fn native_codex_account_handshake_without_model_turn() {
        let plan_path = std::env::var("WORKER_ACCOUNT_TEST_PLAN").expect("fixture plan required");
        let value: serde_json::Value =
            serde_json::from_slice(&bounded(Path::new(&plan_path), 65536).unwrap()).unwrap();
        let data = Path::new(value["dataRoot"].as_str().unwrap());
        let project = Project {
            id: value["projectId"].as_str().unwrap().into(),
            root: value["sharedRoot"].as_str().unwrap().into(),
            timezone: "UTC".into(),
        };
        let id = value["slots"][0]["identity"].as_str().unwrap();
        let binding = WorkerDirectory {
            identity: id.into(),
            relative_path: format!("executors/{id}"),
        };
        let account = load_binding(data, &project, &binding).unwrap();
        let workspace = PathBuf::from(&account.record.workspace);
        assert!(!workspace.join(".cerulean-runtime/codex/auth.json").exists());
        let executable = PathBuf::from(std::env::var("WORKER_CODEX_TEST_EXE").unwrap());
        let mut client = codex_with_account(account, &executable, &workspace, true).unwrap();
        let connection = client.connect().await.unwrap();
        assert_eq!(
            connection.account_type, None,
            "host login must not be inherited"
        );
        assert_eq!(connection.model_calls, 0);
        assert!(!connection.models.is_empty());
        assert!(client.request_counts().keys().all(|method| matches!(
            method.as_str(),
            "initialize" | "account/read" | "model/list"
        )));
        println!(
            "native Codex handshake: version={}, unauthenticated=true, modelTurns=0, models={}",
            connection.server_version,
            connection.models.len()
        );
        client.close().await.unwrap();
        assert!(!workspace.join(".cerulean-runtime/codex/auth.json").exists());
    }
    #[test]
    fn elevated_setup_is_private_and_pinned_before_any_uac() {
        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("data");
        let shared = root.path().join("shared");
        let chief = root.path().join("chief");
        for path in [&data, &shared, &chief] {
            fs::create_dir(path).unwrap();
        }
        let project = Project {
            id: "private-setup".into(),
            root: shared.to_string_lossy().into(),
            timezone: "UTC".into(),
        };
        let id = "a".repeat(64);
        let bindings = std::collections::BTreeMap::from([(
            id.clone(),
            WorkerDirectory {
                identity: id.clone(),
                relative_path: format!("executors/{id}"),
            },
        )]);
        let prepared = prepare(&data, &project, chief.to_str().unwrap(), &bindings).unwrap();
        let owner = crate::ipc::current_sid().unwrap();
        check_acl(prepared.plan.parent().unwrap(), &owner, None, false).unwrap();
        assert!(fs::write(&prepared.plan, b"replace").is_err());
        assert!(fs::rename(prepared.plan.parent().unwrap(), root.path().join("moved")).is_err());
        assert_eq!(views(&data)[0].state, "pending");
        assert!(!prepared.plan.with_extension("intent").exists());
        let value: serde_json::Value =
            serde_json::from_slice(&bounded(&prepared.plan, 65536).unwrap()).unwrap();
        assert_eq!(value["slots"].as_array().unwrap().len(), 1);
        assert!(value.get("password").is_none());
        drop(prepared);
        // A core restart must not spontaneously elevate an old pending plan.
        assert_eq!(views(&data)[0].state, "pending");
    }
}
