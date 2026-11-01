//! Recheck-only process identities. A name is evidence only when derived from a
//! validated v2 launch claim and queried while the original ledger is exclusive.
//! In particular, absence alone is never permission to release a task.
use super::*;
use cerulean_domain::execution::{digest, identifier, sha256, EventAnchor};
use windows_sys::Win32::Security::Authorization::ConvertStringSecurityDescriptorToSecurityDescriptorW;

// winnt.h JOB_OBJECT_QUERY; SystemServices is otherwise unused by this crate.
const JOB_OBJECT_QUERY: u32 = 0x0004;

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ExitReceipt {
    schema_version: u32,
    job_name: String,
    observed_at: String,
    active_processes: u32,
    stdout_sha256: String,
    stderr_sha256: String,
}
impl ExitReceipt {
    /// Only called with an Outcome returned after run_inner confirmed zero
    /// active processes and joined both pipe readers. It precedes result/log
    /// publication so a later I/O failure does not discard the exit evidence.
    pub(crate) fn new(name: &str, outcome: &Outcome) -> Result<Self, String> {
        use sha2::{Digest, Sha256};
        checked_name(name)?;
        Ok(Self {
            schema_version: 1,
            job_name: name.into(),
            observed_at: chrono::Utc::now().to_rfc3339(),
            active_processes: 0,
            stdout_sha256: format!("{:x}", Sha256::digest(&outcome.stdout)),
            stderr_sha256: format!("{:x}", Sha256::digest(&outcome.stderr)),
        })
    }
    pub(crate) fn validate(&self, name: &str, started_at: &str) -> Result<(), String> {
        use cerulean_domain::execution::require;
        checked_name(name)?;
        sha256(&self.stdout_sha256)?;
        sha256(&self.stderr_sha256)?;
        let at =
            chrono::DateTime::parse_from_rfc3339(&self.observed_at).map_err(|e| e.to_string())?;
        let start = chrono::DateTime::parse_from_rfc3339(started_at).map_err(|e| e.to_string())?;
        require(
            self.schema_version == 1
                && self.job_name == name
                && self.active_processes == 0
                && at >= start
                && at <= chrono::Utc::now(),
            "复验进程退出回执身份或时间不匹配",
        )
    }
}

pub(crate) fn scope(
    evidence_root: &Path,
    approval: &EventAnchor,
    operation: &str,
) -> Result<String, String> {
    identifier(operation)?;
    sha256(&approval.hash)?;
    let root = crate::artifacts::root_path(
        evidence_root
            .to_str()
            .ok_or("non-Unicode recheck evidence root")?,
    )?;
    digest(&(
        "recheck-process-scope-v1",
        root.to_str().ok_or("non-Unicode canonical evidence root")?,
        crate::ipc::current_sid().map_err(|e| e.to_string())?,
        approval,
        operation,
    ))
}

pub(crate) fn name(scope: &str, stage: &str) -> Result<String, String> {
    sha256(scope)?;
    identifier(stage)?;
    // Global is intentional: a later login must not query an empty Local
    // namespace and mistake a process in the previous session for stopped.
    Ok(format!(
        "Global\\CeruleanLonging-recheck-{}",
        digest(&("recheck-stage-job-v1", scope, stage))?
    ))
}

fn checked_name(name: &str) -> Result<(), String> {
    let suffix = name
        .strip_prefix("Global\\CeruleanLonging-recheck-")
        .ok_or("invalid recheck job namespace")?;
    sha256(suffix)
}

pub(super) fn create(name: &str) -> Result<OwnedHandle, String> {
    checked_name(name)?;
    let sid = crate::ipc::current_sid().map_err(|e| e.to_string())?;
    let sddl = wide(OsStr::new(&format!("D:P(A;;GA;;;{sid})")));
    let name = wide(OsStr::new(name));
    unsafe {
        let mut descriptor = null_mut();
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            null_mut(),
        ) == 0
        {
            return Err(os_error());
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        SetLastError(ERROR_SUCCESS);
        let raw = CreateJobObjectW(&attributes, name.as_ptr());
        let error = GetLastError();
        LocalFree(descriptor);
        if raw.is_null() {
            return Err(std::io::Error::from_raw_os_error(error as i32).to_string());
        }
        let job = handle(raw)?;
        if error == ERROR_ALREADY_EXISTS {
            // Never configure, join, or terminate an existing object.
            return Err("recheck process job already exists; preserve original operation".into());
        }
        Ok(job)
    }
}

/// Query only. No terminate/set rights, waits, repair, or process launch.
/// None means the name is absent; it is not a standalone completion proof.
pub(crate) fn observe(name: &str) -> Result<Option<u32>, String> {
    checked_name(name)?;
    let name = wide(OsStr::new(name));
    let raw = unsafe { OpenJobObjectW(JOB_OBJECT_QUERY, 0, name.as_ptr()) };
    if raw.is_null() {
        let error = unsafe { GetLastError() };
        return if error == ERROR_FILE_NOT_FOUND {
            Ok(None)
        } else {
            Err(std::io::Error::from_raw_os_error(error as i32).to_string())
        };
    }
    active(&handle(raw)?).map(Some)
}

#[cfg(test)]
mod tests;
