//! Windows local-user service transport. A connection has the OS user's role;
//! it cannot supply a principal or issue worker/core workflow transitions.
use crate::diagnostic_service::ServiceHost;
use crate::host::RuntimeHost;
#[path = "resident_process.rs"]
mod resident_process;
use cerulean_domain::{
    ApiError, Command, Request, Response, Snapshot, PRODUCT_NAME, PROTOCOL_VERSION,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    ffi::c_void, io, os::windows::io::AsRawHandle, path::Path, ptr, sync::Arc, time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::windows::named_pipe::{ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions},
    sync::{watch, Mutex, Semaphore},
    task::JoinSet,
    time::timeout,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, LocalFree, HANDLE},
    Security::{
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        },
        GetTokenInformation, TokenUser, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    },
    System::{
        Pipes::{GetNamedPipeClientProcessId, GetNamedPipeServerProcessId},
        Threading::{
            GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
        },
    },
};

mod document_stream;
const REQUEST_LIMIT: usize = 64 * 1024;
const RESPONSE_LIMIT: usize = 4 * 1024 * 1024;
const DEADLINE: Duration = Duration::from_secs(15);

/// Failure-only facts obtained by the existing connection checks. No token probes.
#[derive(Default, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionFailure {
    stage: &'static str,
    win32_error: Option<i32>,
    message: String,
    caller_sid: Option<String>,
    canonical_directory: Option<String>,
    endpoint: Option<String>,
    server_pid: Option<u32>,
    server_sid: Option<String>,
    greeting: Option<Greeting>,
}
impl ConnectionFailure {
    fn io(&mut self, error: io::Error) -> String {
        self.win32_error = error.raw_os_error();
        self.reject(error.to_string())
    }
    fn reject(&mut self, message: impl Into<String>) -> String {
        self.message = message.into();
        serde_json::to_string(self).expect("connection facts are JSON serializable")
    }
}

async fn connection_greeting<R: AsyncRead + Unpin>(
    stream: &mut R,
    facts: &mut ConnectionFailure,
    deadline: Duration,
) -> Result<Greeting, String> {
    facts.stage = "handshakeRead";
    let greeting: Greeting = match timeout(deadline, read_frame_io(stream, REQUEST_LIMIT)).await {
        Err(_) => {
            facts.stage = "handshakeTimeout";
            return Err(facts.reject("handshake timed out"));
        }
        Ok(result) => result.map_err(|e| facts.io(e))?,
    };
    facts.stage = "handshakeIdentity";
    if greeting.product != PRODUCT_NAME
        || greeting.protocol_version != PROTOCOL_VERSION
        || Some(greeting.server_pid) != facts.server_pid
        || Some(&greeting.owner_sid) != facts.caller_sid.as_ref()
    {
        facts.greeting = Some(greeting);
        return Err(facts.reject("service handshake mismatch"));
    }
    Ok(greeting)
}

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
struct LocalAllocation(*mut c_void);
impl Drop for LocalAllocation {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}
fn check(result: i32) -> io::Result<()> {
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
pub(crate) fn sid_for_process(process: HANDLE) -> io::Result<String> {
    unsafe {
        let mut token = ptr::null_mut();
        check(OpenProcessToken(process, TOKEN_QUERY, &mut token))?;
        let token = Handle(token);
        let mut needed = 0;
        GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut needed);
        if needed == 0 || needed > 65536 {
            return Err(io::Error::other("invalid token size"));
        }
        // usize storage supplies alignment required by TOKEN_USER.
        let mut buffer = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
        check(GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        ))?;
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut sid = ptr::null_mut();
        check(ConvertSidToStringSidW(user.User.Sid, &mut sid))?;
        let _allocation = LocalAllocation(sid.cast());
        let mut length = 0;
        while *sid.add(length) != 0 {
            length += 1;
        }
        String::from_utf16(std::slice::from_raw_parts(sid, length)).map_err(io::Error::other)
    }
}
pub fn current_sid() -> io::Result<String> {
    sid_for_process(unsafe { GetCurrentProcess() })
}
pub(crate) fn process_sid(pid: u32) -> io::Result<String> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(io::Error::last_os_error());
    }
    let handle = Handle(process);
    sid_for_process(handle.0)
}
pub fn endpoint(directory: &Path) -> io::Result<String> {
    endpoint_details(directory, &mut ConnectionFailure::default())
}
fn endpoint_details(directory: &Path, facts: &mut ConnectionFailure) -> io::Result<String> {
    facts.stage = "endpointRecovery";
    crate::backup_recovery::ensure_not_recovering(directory).map_err(io::Error::other)?;
    facts.stage = "endpointDirectory";
    std::fs::create_dir_all(directory)?;
    facts.stage = "endpointCanonicalize";
    let root = std::fs::canonicalize(directory)?;
    facts.canonical_directory = Some(root.to_string_lossy().into_owned());
    facts.stage = "endpointCallerSid";
    let sid = current_sid()?;
    facts.caller_sid = Some(sid.clone());
    let identity = format!("{}\n{}", sid, root.to_string_lossy().to_lowercase());
    let name = format!(
        r"\\.\pipe\CeruleanLonging-v1-{:x}",
        Sha256::digest(identity.as_bytes())
    );
    facts.endpoint = Some(name.clone());
    Ok(name)
}
fn listener(name: &str, sid: &str, first: bool) -> io::Result<NamedPipeServer> {
    restricted_listener(name, &[sid], first)
}
pub(crate) fn restricted_listener(
    name: &str,
    sids: &[&str],
    first: bool,
) -> io::Result<NamedPipeServer> {
    if sids.is_empty()
        || sids.len() > 2
        || sids.iter().any(|sid| {
            !sid.starts_with("S-1-")
                || !sid
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b == b'S' || b == b'-')
        })
    {
        return Err(io::Error::other("invalid listener identity"));
    }
    let grants = sids
        .iter()
        .map(|sid| format!("(A;;GA;;;{sid})"))
        .collect::<String>();
    let sddl: Vec<u16> = format!("D:P{grants}\0").encode_utf16().collect();
    unsafe {
        let mut descriptor = ptr::null_mut();
        check(ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            1,
            &mut descriptor,
            ptr::null_mut(),
        ))?;
        let _allocation = LocalAllocation(descriptor);
        let mut attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(
                name,
                (&mut attributes as *mut SECURITY_ATTRIBUTES).cast(),
            )
    }
}
#[cfg(test)]
pub(crate) fn private_listener_for_test(name: &str) -> io::Result<NamedPipeServer> {
    listener(name, &current_sid()?, true)
}
pub async fn read_frame<R: AsyncRead + Unpin, T: serde::de::DeserializeOwned>(
    stream: &mut R,
    limit: usize,
) -> Result<T, String> {
    read_frame_io(stream, limit)
        .await
        .map_err(|e| e.to_string())
}
async fn read_frame_io<R: AsyncRead + Unpin, T: serde::de::DeserializeOwned>(
    stream: &mut R,
    limit: usize,
) -> io::Result<T> {
    let size = stream.read_u32_le().await? as usize;
    if size == 0 || size > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid frame length",
        ));
    }
    let mut bytes = vec![0; size];
    stream.read_exact(&mut bytes).await?;
    serde_json::from_slice(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}
pub async fn write_frame<W: AsyncWrite + Unpin, T: Serialize>(
    stream: &mut W,
    value: &T,
    limit: usize,
) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    write_frame_bytes(stream, &bytes, limit).await
}
async fn write_frame_bytes<W: AsyncWrite + Unpin>(
    stream: &mut W,
    bytes: &[u8],
    limit: usize,
) -> Result<(), String> {
    if bytes.is_empty() || bytes.len() > limit {
        return Err("frame exceeds limit".into());
    }
    stream
        .write_u32_le(bytes.len() as u32)
        .await
        .map_err(|e| e.to_string())?;
    stream.write_all(bytes).await.map_err(|e| e.to_string())?;
    stream.flush().await.map_err(|e| e.to_string())
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Greeting {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub document_stream: bool,
    pub product: String,
    pub protocol_version: u32,
    pub server_pid: u32,
    pub owner_sid: String,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum UserOperation {
    ExternalChief {
        id: String,
        protocol_version: u32,
        operation: cerulean_domain::management_ingress::ExternalChiefOperation,
    },
    ReviewPreservedShutdown {
        id: String,
        protocol_version: u32,
    },
    PreserveShutdown {
        id: String,
        protocol_version: u32,
        review_digest: String,
    },
    ReviewUpgrade {
        id: String,
        protocol_version: u32,
    },
    PrepareUpgrade {
        id: String,
        protocol_version: u32,
        review_digest: String,
    },
    RestartDataDiagnostics {
        id: String,
        protocol_version: u32,
    },
    RestartReviewedStops {
        id: String,
        protocol_version: u32,
    },
    Request {
        request: Request,
    },
    Command {
        request: Request,
    },
    CountResolution {
        request: Request,
    },
    Historical {
        request: Request,
    },
    Workspace {
        id: String,
        protocol_version: u32,
        query: cerulean_domain::workspace::WorkspaceQuery,
    },
    Shutdown {
        id: String,
        protocol_version: u32,
    },
}
fn response_deadline(operation: &UserOperation) -> Duration {
    // Local evidence preparation can rehash several large pinned tools twice.
    // This never extends a provider request, handshake, or ordinary query.
    match operation {
        UserOperation::Workspace {
            query:
                cerulean_domain::workspace::WorkspaceQuery::VerificationRecheckReview { .. }
                | cerulean_domain::workspace::WorkspaceQuery::VerificationRecheckDispositionReview {
                    ..
                },
            ..
        } => Duration::from_secs(60),
        UserOperation::Request { request } | UserOperation::Command { request }
            if matches!(
                request.command,
                Command::ReviewVerificationRecheckDisposition { .. }
                    | Command::RetireVerificationRecheck { .. }
                    | Command::ReviewVerificationRecheck { .. }
                    | Command::ApproveVerificationRecheck { .. }
                    | Command::StartVerificationRecheck { .. }
                    | Command::RecoverVerificationRecheck { .. }
            ) =>
        {
            Duration::from_secs(60)
        }
        _ => DEADLINE,
    }
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ServiceReply {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_chief: Option<cerulean_domain::management_ingress::ExternalChiefResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_receipt: Option<cerulean_domain::command_result::CommandReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<cerulean_domain::workspace::WorkspaceResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub historical: Option<cerulean_domain::historical_money::HistoricalResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count_resolution: Option<cerulean_domain::count_resolution::CountResolutionResult>,
    pub response: Response,
    pub stopping: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upgrade_review: Option<crate::upgrade::UpgradeReview>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preservation_review: Option<crate::preserve_shutdown::PreservationReview>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upgrade_hold: Option<crate::upgrade::HoldStatus>,
}
struct ServiceState {
    host: ServiceHost,
    stopping: bool,
    preservation_review: Option<crate::preserve_shutdown::PreservationReview>,
}
impl ServiceState {
    fn reply(&self, response: Response) -> ServiceReply {
        ServiceReply {
            external_chief: None,
            command_receipt: None,
            workspace: None,
            historical: None,
            count_resolution: None,
            response,
            stopping: self.stopping,
            upgrade_review: None,
            preservation_review: None,
            upgrade_hold: None,
        }
    }
    fn error(&self, id: String, code: &str, message: &str) -> ServiceReply {
        self.reply(Response {
            id,
            result: None,
            error: Some(ApiError {
                code: code.into(),
                message: message.into(),
            }),
        })
    }
    fn apply(&mut self, operation: UserOperation) -> ServiceReply {
        let id = match &operation {
            UserOperation::Request { request }
            | UserOperation::Command { request }
            | UserOperation::Historical { request }
            | UserOperation::CountResolution { request } => request.id.clone(),
            UserOperation::ExternalChief { id, .. }
            | UserOperation::Workspace { id, .. }
            | UserOperation::Shutdown { id, .. }
            | UserOperation::RestartDataDiagnostics { id, .. }
            | UserOperation::RestartReviewedStops { id, .. }
            | UserOperation::ReviewPreservedShutdown { id, .. }
            | UserOperation::PreserveShutdown { id, .. }
            | UserOperation::ReviewUpgrade { id, .. }
            | UserOperation::PrepareUpgrade { id, .. } => id.clone(),
        };
        if self.stopping {
            return self.error(
                id,
                "coreStopping",
                "核心已进入退出屏障；请求未执行，请等待原进程退出",
            );
        }
        self.host.poll_import();
        if self.host.import_fenced()
            && !matches!(
                &operation,
                UserOperation::Request {
                    request: Request {
                        command: Command::Snapshot,
                        ..
                    }
                } | UserOperation::Workspace { .. }
                    | UserOperation::ReviewUpgrade { .. }
                    | UserOperation::PrepareUpgrade { .. }
                    | UserOperation::Shutdown { .. }
                    | UserOperation::RestartDataDiagnostics { .. }
                    | UserOperation::RestartReviewedStops { .. }
            )
        {
            return self.error(
                id,
                "teamImportUnresolved",
                "原团队材料导入运行中或需核查；仅读取原记录，不重复提交或派发",
            );
        }
        match operation {
            UserOperation::ExternalChief {
                protocol_version,
                operation,
                ..
            } => {
                if protocol_version != PROTOCOL_VERSION {
                    return self.error(id, "protocolMismatch", "核心协议版本不匹配");
                }
                let ServiceHost::Ready(host) = &mut self.host else {
                    return self.error(
                        id,
                        "diagnosticReadOnly",
                        "原数据需诊断；外部主指挥请求未执行",
                    );
                };
                match host.external_chief_operation(operation) {
                    Ok(result) => {
                        let mut reply = self.reply(Response {
                            id,
                            result: None,
                            error: None,
                        });
                        reply.external_chief = Some(result);
                        reply
                    }
                    Err(message) => self.error(id, "externalChiefRejected", &message),
                }
            }
            UserOperation::Command { request } => {
                if request.protocol_version != PROTOCOL_VERSION {
                    return self.error(id, "protocolMismatch", "核心协议版本不匹配");
                }
                let response = self.host.handle_command(request);
                let accepted = response.error.is_none();
                let mut reply = self.reply(response);
                if accepted {
                    reply.command_receipt =
                        Some(cerulean_domain::command_result::CommandReceipt { request_id: id });
                }
                reply
            }
            UserOperation::Workspace {
                protocol_version,
                query,
                ..
            } => {
                if protocol_version != PROTOCOL_VERSION {
                    return self.error(id, "protocolMismatch", "核心协议版本不匹配");
                }
                match self.host.workspace_query(&query) {
                    Ok(result) => {
                        let mut reply = self.reply(Response {
                            id,
                            result: None,
                            error: None,
                        });
                        reply.workspace = Some(result);
                        reply
                    }
                    Err(message) => self.error(id, "workspaceReadRejected", &message),
                }
            }
            UserOperation::CountResolution { request } => {
                if request.protocol_version != PROTOCOL_VERSION {
                    return self.error(id, "protocolMismatch", "核心协议版本不匹配");
                }
                let ServiceHost::Ready(host) = &mut self.host else {
                    return self.error(
                        id,
                        "diagnosticReadOnly",
                        "原核心记录需诊断，请保留数据，历史查询和确认未执行",
                    );
                };
                match host.count_resolution_result(&request.command) {
                    Ok(result) => {
                        let mut reply = self.reply(Response {
                            id,
                            result: None,
                            error: None,
                        });
                        reply.count_resolution = Some(result);
                        reply
                    }
                    Err(message) => self.error(id, "countResolutionRejected", &message),
                }
            }
            UserOperation::Historical { request } => {
                if request.protocol_version != PROTOCOL_VERSION {
                    return self.error(id, "protocolMismatch", "核心协议版本不匹配");
                }
                let ServiceHost::Ready(host) = &mut self.host else {
                    return self.error(
                        id,
                        "diagnosticReadOnly",
                        "原核心记录需诊断，请保留数据，历史查询和确认未执行",
                    );
                };
                match host.historical_result(&request.command) {
                    Ok(result) => {
                        let mut reply = self.reply(Response {
                            id,
                            result: None,
                            error: None,
                        });
                        reply.historical = Some(result);
                        reply
                    }
                    Err(message) => self.error(id, "historicalRejected", &message),
                }
            }
            UserOperation::Request { request } => {
                let response = self.host.handle(request);
                self.reply(response)
            }
            UserOperation::ReviewPreservedShutdown {
                protocol_version, ..
            } => self.preserve_shutdown(id, protocol_version, None),
            UserOperation::PreserveShutdown {
                protocol_version,
                review_digest,
                ..
            } => self.preserve_shutdown(id, protocol_version, Some(review_digest)),
            UserOperation::ReviewUpgrade {
                protocol_version, ..
            } => self.upgrade(id, protocol_version, None),
            UserOperation::PrepareUpgrade {
                protocol_version,
                review_digest,
                ..
            } => self.upgrade(id, protocol_version, Some(review_digest)),
            operation => {
                let protocol_version = match &operation {
                    UserOperation::Shutdown {
                        protocol_version, ..
                    }
                    | UserOperation::RestartDataDiagnostics {
                        protocol_version, ..
                    }
                    | UserOperation::RestartReviewedStops {
                        protocol_version, ..
                    } => *protocol_version,
                    _ => unreachable!(),
                };
                if protocol_version != PROTOCOL_VERSION {
                    return self.error(id, "protocolMismatch", "核心协议版本不匹配");
                }
                let (allowed, code) = match operation {
                    UserOperation::Shutdown { .. } => (self.host.can_shutdown(), "unresolvedTasks"),
                    UserOperation::RestartDataDiagnostics { .. } => {
                        (self.host.can_restart_data_diagnostics(), "restartRejected")
                    }
                    UserOperation::RestartReviewedStops { .. } => {
                        (self.host.can_restart_reviewed_stops(), "restartRejected")
                    }
                    _ => unreachable!(),
                };
                if !allowed {
                    return self.error(id, code, "仍有未决任务或诊断条件未满足，未退出核心");
                }
                self.stopping = true;
                self.reply(Response {
                    id,
                    result: Some(self.host.snapshot()),
                    error: None,
                })
            }
        }
    }
    fn preserve_shutdown(
        &mut self,
        id: String,
        protocol: u32,
        expected: Option<String>,
    ) -> ServiceReply {
        if protocol != PROTOCOL_VERSION {
            return self.error(id, "protocolMismatch", "核心协议版本不匹配");
        }
        let reviewed_at = if let Some(expected) = &expected {
            let Some(prior) = self.preservation_review.take() else {
                return self.error(
                    id,
                    "preservationReviewRequired",
                    "请先在当前核心审阅未决记录",
                );
            };
            if &prior.digest != expected {
                return self.error(
                    id,
                    "preservationReviewChanged",
                    "审阅已失效，请重新核对原记录",
                );
            }
            prior.reviewed_at
        } else {
            self.preservation_review = None;
            chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Nanos, true)
        };
        let review = match crate::preserve_shutdown::review(&self.host, reviewed_at) {
            Ok(review) => review,
            Err(message) => return self.error(id, "preservedShutdownRejected", &message),
        };
        if let Some(expected) = expected {
            if review.digest != expected {
                return self.error(
                    id,
                    "preservationReviewChanged",
                    "记录或活动已变化，请重新审阅",
                );
            }
            // Same serialized service lock as all mutations and scheduler ticks.
            // No journal write, model send, acceptance or outcome conversion.
            self.stopping = true;
        } else {
            self.preservation_review = Some(review.clone());
        }
        let mut reply = self.reply(Response {
            id,
            result: None,
            error: None,
        });
        reply.preservation_review = Some(review);
        reply
    }
    fn upgrade(&mut self, id: String, protocol: u32, expected: Option<String>) -> ServiceReply {
        if protocol != PROTOCOL_VERSION {
            return self.error(id, "protocolMismatch", "核心协议版本不匹配");
        }
        let (review, snapshot) = match crate::upgrade::review(&mut self.host) {
            Ok(r) => r,
            Err(e) => return self.error(id, "upgradeReviewFailed", &e),
        };
        let mut hold = None;
        if let Some(expected) = expected {
            if expected != review.digest || !review.ready {
                return self.error(
                    id,
                    "upgradeNotReady",
                    "状态已变化或仍有未决记录，未准备升级；请重新审阅",
                );
            }
            let prepared = crate::upgrade::prepare(self.host.directory(), review.clone());
            // A partially written hold still fences requests and startup.
            self.stopping = crate::upgrade::ensure_not_held(self.host.directory()).is_err();
            match prepared {
                Ok(r) => hold = Some(r),
                Err(e) => return self.error(id, "upgradeHoldFailed", &e),
            }
        }
        ServiceReply {
            command_receipt: None,
            external_chief: None,
            workspace: None,
            historical: None,
            count_resolution: None,
            response: Response {
                id,
                result: Some(snapshot),
                error: None,
            },
            stopping: self.stopping,
            upgrade_review: Some(review),
            preservation_review: None,
            upgrade_hold: hold,
        }
    }
}

async fn handle(
    mut pipe: NamedPipeServer,
    sid: &str,
    store: Arc<Mutex<ServiceState>>,
    stop: watch::Sender<bool>,
) -> Result<(), String> {
    let mut client_pid = 0;
    unsafe {
        check(GetNamedPipeClientProcessId(
            pipe.as_raw_handle(),
            &mut client_pid,
        ))
    }
    .map_err(|e| e.to_string())?;
    if process_sid(client_pid).map_err(|e| e.to_string())? != sid {
        return Err("client user mismatch".into());
    }
    let (operation, documents, size): (UserOperation, bool, usize) = timeout(DEADLINE, async {
        write_frame(
            &mut pipe,
            &Greeting {
                document_stream: true,
                product: PRODUCT_NAME.into(),
                protocol_version: PROTOCOL_VERSION,
                server_pid: std::process::id(),
                owner_sid: sid.into(),
            },
            REQUEST_LIMIT,
        )
        .await?;
        document_stream::read(&mut pipe, REQUEST_LIMIT).await
    })
    .await
    .map_err(|_| "service input timed out")??;
    if documents && !document_stream::document_operation(&operation) {
        return Err("document transport is restricted to user documents".into());
    }
    if !documents && size > REQUEST_LIMIT {
        return Err("request exceeds limit".into());
    }
    timeout(response_deadline(&operation), async {
        let reply = store.lock().await.apply(operation);
        // Commit the stop barrier before awaiting delivery: a lost ACK cannot
        // leave an upgrade-prepared process accepting requests indefinitely.
        if documents
            && (reply.workspace.as_ref().is_some_and(|v| {
                matches!(
                    v,
                    cerulean_domain::workspace::WorkspaceResult::Plan(_)
                        | cerulean_domain::workspace::WorkspaceResult::Configuration(_)
                )
            }) || reply.response.result.is_some())
        {
            document_stream::write(&mut pipe, &reply).await
        } else {
            send_reply(&mut pipe, &reply, &stop).await
        }
    })
    .await
    .map_err(|_| "service response timed out")?
}
async fn send_reply<W: AsyncWrite + Unpin>(
    writer: &mut W,
    reply: &ServiceReply,
    stop: &watch::Sender<bool>,
) -> Result<(), String> {
    if reply.stopping {
        let _ = stop.send(true);
    }
    let bytes = bounded_reply_bytes(reply)?;
    write_frame_bytes(writer, &bytes, RESPONSE_LIMIT).await
}

fn bounded_reply_bytes(reply: &ServiceReply) -> Result<Vec<u8>, String> {
    let bytes = serde_json::to_vec(reply).map_err(|e| e.to_string())?;
    if bytes.len() <= RESPONSE_LIMIT {
        return Ok(bytes);
    }
    // The operation has already run. Never rerun, truncate a success snapshot,
    // or describe an oversized response as a rejected/unexecuted operation.
    // A small correlated error lets clients use the original readback path.
    serde_json::to_vec(&ServiceReply {
        external_chief: None,
        command_receipt: None,
            workspace: None,
            historical: None,
            count_resolution: None,
        response: Response {
            id: reply.response.id.clone(),
            result: None,
            error: Some(ApiError {
                code: "responseTooLarge".into(),
                message: "结果超过本机通信大小限制，请求可能已经生效。请勿重复提交，请读取原记录核对；若原记录仍无法读取，请保留数据。".into(),
            }),
        },
        stopping: reply.stopping,
        upgrade_review: None,
            preservation_review: None,
        upgrade_hold: None,
    }).map_err(|e| e.to_string())
}

pub async fn serve(directory: &Path) -> Result<(), String> {
    serve_mode(directory, ServiceHost::open(directory)?).await
}
/// Trusted embedding entry; role callbacks remain Rust methods, not wire commands.
pub async fn serve_host(directory: &Path, host: RuntimeHost) -> Result<(), String> {
    serve_mode(directory, ServiceHost::Ready(Box::new(host))).await
}
async fn serve_mode(directory: &Path, host: ServiceHost) -> Result<(), String> {
    if std::fs::canonicalize(directory).map_err(|e| e.to_string())? != host.directory() {
        return Err("service host directory mismatch".into());
    }
    let store = Arc::new(Mutex::new(ServiceState {
        host,
        stopping: false,
        preservation_review: None,
    }));
    let name = endpoint(directory).map_err(|e| e.to_string())?;
    let sid = current_sid().map_err(|e| e.to_string())?;
    let mut waiting = listener(&name, &sid, true).map_err(|e| e.to_string())?;
    let slots = Arc::new(Semaphore::new(16));
    let (stop_tx, mut stop_rx) = watch::channel(false);
    let mut clients = JoinSet::new();
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    loop {
        tokio::select! {
            _ = tick.tick() => { let mut state = store.lock().await; if !state.stopping { state.host.poll_verifications(); } },
            _ = stop_rx.changed() => break,
            connected = waiting.connect() => {
                connected.map_err(|e| e.to_string())?;
                let next=listener(&name,&sid,false).map_err(|e| e.to_string())?;
                let pipe=std::mem::replace(&mut waiting,next);
                let Ok(permit)=slots.clone().try_acquire_owned() else { drop(pipe); continue; };
                let owner=sid.clone();let store=store.clone();let stop=stop_tx.clone();
                clients.spawn(async move {
                    let _permit=permit;
                    let _ = handle(pipe,&owner,store,stop).await;
                });
            }
            Some(_) = clients.join_next(), if !clients.is_empty() => {}
        }
    }
    drop(waiting);
    // Finish already accepted requests; shutdown never kills a user application.
    let _ = timeout(DEADLINE, async {
        while clients.join_next().await.is_some() {}
    })
    .await;
    Ok(())
}
pub struct Client {
    documents: bool,
    pipe: NamedPipeClient,
    pub server_pid: u32,
}

impl Client {
    /// Desktop close never starts a service. Absence must also agree with the
    /// authoritative writer lock; busy/denied/invalid endpoints are not absence.
    pub async fn stop_for_window(directory: &Path, id: String) -> Result<Option<u32>, String> {
        let mut facts = ConnectionFailure::default();
        let name = endpoint_details(directory, &mut facts).map_err(|e| facts.io(e))?;
        facts.stage = "windowClosePipeOpen";
        let pipe = match ClientOptions::new().open(&name) {
            Ok(pipe) => pipe,
            Err(e) if e.raw_os_error() == Some(2) => {
                let path = directory.join("writer.lock");
                match std::fs::symlink_metadata(&path) {
                    Ok(_) => {
                        let _lock = crate::backup::open(&path, false, true)
                            .map_err(|e| format!("核心未连接，但数据仍被占用；未确认退出：{e}"))?;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                    Err(e) => return Err(facts.io(e)),
                }
                return Ok(None);
            }
            Err(e) => return Err(facts.io(e)),
        };
        let client = Self::from_pipe(pipe, facts).await?;
        let pid = client.server_pid;
        let reply = client
            .call_and_wait_stop(UserOperation::Shutdown {
                id,
                protocol_version: PROTOCOL_VERSION,
            })
            .await?;
        if let Some(error) = reply.response.error {
            return Err(error.message);
        }
        if !reply.stopping {
            return Err("核心未确认停止；窗口保留，未重发退出请求".into());
        }
        Ok(Some(pid))
    }

    /// Wait on the authenticated original process, without resending the operation.
    pub async fn call_and_wait_stop(
        self,
        operation: UserOperation,
    ) -> Result<ServiceReply, String> {
        use std::os::windows::io::{FromRawHandle, OwnedHandle};
        use windows_sys::Win32::{
            Foundation::WAIT_OBJECT_0,
            System::Threading::{WaitForSingleObject, PROCESS_SYNCHRONIZE},
        };
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, self.server_pid) };
        if handle.is_null() {
            return Err("无法保留原核心退出句柄；请求未发送".into());
        }
        let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
        let reply = self.call(operation).await?;
        if reply.stopping {
            let result = tokio::task::spawn_blocking(move || unsafe {
                WaitForSingleObject(handle.as_raw_handle(), 30000)
            })
            .await
            .map_err(|_| "等待核心退出失败")?;
            if result != WAIT_OBJECT_0 {
                return Err("原核心退出尚未确认；不重发准备请求，请读取upgrade status".into());
            }
        }
        Ok(reply)
    }

    /// Explicit diagnostic restart. Wait on the authenticated server's retained
    /// process handle; a timeout never causes a replacement process to launch.
    pub async fn restart_reviewed_stops(
        directory: &Path,
        executable: &Path,
    ) -> Result<Snapshot, String> {
        Self::restart_mode(
            directory,
            executable,
            UserOperation::RestartReviewedStops {
                id: "restart-reviewed-stops".into(),
                protocol_version: PROTOCOL_VERSION,
            },
        )
        .await
    }
    pub async fn restart_data_diagnostics(
        directory: &Path,
        executable: &Path,
    ) -> Result<Snapshot, String> {
        Self::restart_mode(
            directory,
            executable,
            UserOperation::RestartDataDiagnostics {
                id: "restart-data-diagnostics".into(),
                protocol_version: PROTOCOL_VERSION,
            },
        )
        .await
    }
    async fn restart_mode(
        directory: &Path,
        executable: &Path,
        operation: UserOperation,
    ) -> Result<Snapshot, String> {
        use std::os::windows::io::{FromRawHandle, OwnedHandle};
        use windows_sys::Win32::{
            Foundation::WAIT_OBJECT_0,
            System::Threading::{WaitForSingleObject, PROCESS_SYNCHRONIZE},
        };
        let client = Self::connect(directory).await?;
        let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, client.server_pid) };
        if handle.is_null() {
            return Err("无法确认本地核心退出身份".into());
        }
        let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
        let reply = client.call(operation).await?;
        if let Some(e) = reply.response.error {
            return Err(e.message);
        }
        if !reply.stopping {
            return Err("核心未确认重启，未启动替代进程".into());
        }
        let stopped = tokio::task::spawn_blocking(move || unsafe {
            WaitForSingleObject(handle.as_raw_handle(), 30000)
        })
        .await
        .map_err(|_| "核心退出等待失败")?;
        if stopped != WAIT_OBJECT_0 {
            return Err("尚未确认核心退出，未启动替代进程；请重新读取状态".into());
        }
        let reply = Self::connect_or_start(directory, executable)
            .await?
            .call(UserOperation::Request {
                request: Request {
                    id: "reopened-stop-review".into(),
                    protocol_version: PROTOCOL_VERSION,
                    command: Command::Snapshot,
                },
            })
            .await?;
        if let Some(e) = reply.response.error {
            return Err(e.message);
        }
        reply.response.result.ok_or("重新打开核心未返回记录".into())
    }
    pub async fn connect(directory: &Path) -> Result<Self, String> {
        let mut facts = ConnectionFailure::default();
        let name = endpoint_details(directory, &mut facts).map_err(|e| facts.io(e))?;
        facts.stage = "pipeOpen";
        let pipe = ClientOptions::new().open(&name).map_err(|e| facts.io(e))?;
        Self::from_pipe(pipe, facts).await
    }
    pub async fn connect_or_start(directory: &Path, executable: &Path) -> Result<Self, String> {
        let mut facts = ConnectionFailure {
            stage: "startupRecovery",
            ..ConnectionFailure::default()
        };
        crate::backup_recovery::ensure_not_recovering(directory).map_err(|e| facts.reject(e))?;
        facts.stage = "startupUpgradeHold";
        crate::upgrade::ensure_not_held(directory).map_err(|e| facts.reject(e))?;
        facts.stage = "startupDataFormat";
        crate::data_format::status(directory).map_err(|e| facts.reject(e))?;
        // Retain ownership until the handshake identifies the selected service.
        // A losing concurrent starter must not remain detached and start later,
        // after the selected service has already shut down.
        struct Starter(Option<resident_process::Child>);
        impl Drop for Starter {
            fn drop(&mut self) {
                if let Some(child) = &mut self.0 {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            }
        }
        let mut starter = Starter(None);
        let name = endpoint_details(directory, &mut facts).map_err(|e| facts.io(e))?;
        facts.stage = "pipeOpen";
        match ClientOptions::new().open(&name) {
            Ok(pipe) => return Self::from_pipe(pipe, facts).await,
            Err(e) if e.raw_os_error() == Some(2) => {
                facts.stage = "startupSpawn";
                let child = resident_process::Child::spawn(executable, directory)
                    .map_err(|e| facts.reject(e))?;
                starter.0 = Some(child);
            }
            Err(e) if e.raw_os_error() == Some(231) => {}
            Err(e) => return Err(facts.io(e)),
        }
        let until = tokio::time::Instant::now() + Duration::from_secs(8);
        facts.stage = "pipeOpenAfterStart";
        loop {
            match ClientOptions::new().open(&name) {
                Ok(pipe) => {
                    let client = Self::from_pipe(pipe, facts).await?;
                    if starter
                        .0
                        .as_ref()
                        .is_some_and(|child| child.id() == client.server_pid)
                    {
                        // Only the confirmed selected resident owns its own lifetime.
                        starter.0.take();
                    }
                    return Ok(client);
                }
                Err(e)
                    if matches!(e.raw_os_error(), Some(2 | 231))
                        && tokio::time::Instant::now() < until =>
                {
                    tokio::time::sleep(Duration::from_millis(50)).await
                }
                Err(e) => {
                    facts.stage = "pipeOpenDeadline";
                    return Err(facts.io(e));
                }
            }
        }
    }
    async fn from_pipe(
        mut pipe: NamedPipeClient,
        mut facts: ConnectionFailure,
    ) -> Result<Self, String> {
        facts.stage = "serverPid";
        let mut server_pid = 0;
        unsafe {
            check(GetNamedPipeServerProcessId(
                pipe.as_raw_handle(),
                &mut server_pid,
            ))
        }
        .map_err(|e| facts.io(e))?;
        facts.server_pid = Some(server_pid);
        facts.stage = "callerSid";
        let sid = current_sid().map_err(|e| facts.io(e))?;
        facts.caller_sid = Some(sid.clone());
        facts.stage = "serverSid";
        let server_sid = process_sid(server_pid).map_err(|e| facts.io(e))?;
        facts.server_sid = Some(server_sid.clone());
        if server_sid != sid {
            facts.stage = "serverIdentity";
            return Err(facts.reject("server user mismatch"));
        }
        let greeting = connection_greeting(&mut pipe, &mut facts, DEADLINE).await?;
        Ok(Self {
            documents: greeting.document_stream,
            pipe,
            server_pid,
        })
    }
    pub async fn call(mut self, operation: UserOperation) -> Result<ServiceReply, String> {
        let deadline = response_deadline(&operation);
        let expect_receipt = matches!(&operation, UserOperation::Command { .. });
        let expect_external = matches!(&operation, UserOperation::ExternalChief { .. });
        let expect_preservation = matches!(
            &operation,
            UserOperation::ReviewPreservedShutdown { .. } | UserOperation::PreserveShutdown { .. }
        );
        let preserving_stop = matches!(&operation, UserOperation::PreserveShutdown { .. });
        let id = match &operation {
            UserOperation::Request { request }
            | UserOperation::Command { request }
            | UserOperation::Historical { request }
            | UserOperation::CountResolution { request } => request.id.clone(),
            UserOperation::ExternalChief { id, .. }
            | UserOperation::Workspace { id, .. }
            | UserOperation::Shutdown { id, .. }
            | UserOperation::RestartReviewedStops { id, .. }
            | UserOperation::ReviewPreservedShutdown { id, .. }
            | UserOperation::PreserveShutdown { id, .. }
            | UserOperation::ReviewUpgrade { id, .. }
            | UserOperation::PrepareUpgrade { id, .. }
            | UserOperation::RestartDataDiagnostics { id, .. } => id.clone(),
        };
        timeout(deadline, async {
            let documents = self.documents && document_stream::document_operation(&operation);
            if documents {
                document_stream::write(&mut self.pipe, &operation).await?;
            } else {
                write_frame(&mut self.pipe, &operation, REQUEST_LIMIT).await?;
            }
            let reply: ServiceReply = if documents {
                document_stream::read(&mut self.pipe, RESPONSE_LIMIT)
                    .await?
                    .0
            } else {
                read_frame(&mut self.pipe, RESPONSE_LIMIT).await?
            };
            if reply.response.id != id {
                return Err("response correlation mismatch".into());
            }
            if expect_receipt
                && reply.response.error.is_none()
                && (reply
                    .command_receipt
                    .as_ref()
                    .is_none_or(|r| r.request_id != id)
                    || reply.response.result.is_some()
                    || reply.workspace.is_some()
                    || reply.historical.is_some()
                    || reply.count_resolution.is_some()
                    || reply.preservation_review.is_some())
            {
                return Err("command receipt missing or inconsistent; not resent".into());
            }
            if reply.response.error.is_none()
                && (expect_external != reply.external_chief.is_some()
                    || (expect_external
                        && (reply.response.result.is_some()
                            || reply.command_receipt.is_some()
                            || reply.workspace.is_some()
                            || reply.historical.is_some()
                            || reply.count_resolution.is_some()
                            || reply.upgrade_review.is_some()
                            || reply.upgrade_hold.is_some()
                            || reply.preservation_review.is_some())))
            {
                return Err("external chief response missing or inconsistent; not resent".into());
            }
            if reply.response.error.is_none()
                && (expect_preservation != reply.preservation_review.is_some()
                    || (expect_preservation
                        && (reply.stopping != preserving_stop
                            || reply
                                .preservation_review
                                .as_ref()
                                .is_none_or(|r| r.server_pid != self.server_pid)
                            || reply.response.result.is_some()
                            || reply.command_receipt.is_some()
                            || reply.workspace.is_some()
                            || reply.historical.is_some()
                            || reply.count_resolution.is_some()
                            || reply.external_chief.is_some()
                            || reply.upgrade_review.is_some()
                            || reply.upgrade_hold.is_some())))
            {
                return Err(
                    "preserved shutdown response missing or inconsistent; not resent".into(),
                );
            }
            Ok(reply)
        })
        .await
        .map_err(|_| "service request timed out; not resent")?
    }
    /// Called only by a user-confirmed desktop action in the foreground UI.
    /// The PID is obtained from and checked against the authenticated pipe.
    pub fn allow_desktop_foreground(&self) -> bool {
        crate::verifier::process::allow_foreground(self.server_pid)
    }
    pub async fn snapshot(directory: &Path) -> Result<Snapshot, String> {
        let reply = Self::connect(directory)
            .await?
            .call(UserOperation::Request {
                request: Request {
                    id: "snapshot".into(),
                    protocol_version: PROTOCOL_VERSION,
                    command: Command::Snapshot,
                },
            })
            .await?;
        reply.response.result.ok_or_else(|| {
            reply
                .response
                .error
                .map(|e| e.message)
                .unwrap_or("missing snapshot".into())
        })
    }
}

#[cfg(test)]
#[path = "ipc_external_chief_tests.rs"]
mod external_chief_tests;
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_recheck_review_workspace_query_extends_response_deadline() {
        use cerulean_domain::workspace::WorkspaceQuery;
        let op = |query| UserOperation::Workspace {
            id: "deadline".into(),
            protocol_version: PROTOCOL_VERSION,
            query,
        };
        assert_eq!(
            response_deadline(&op(WorkspaceQuery::VerificationRecheckReview {
                input: Box::new(cerulean_domain::verification_recheck::RecheckRequest {
                    project_id: "p".into(),
                    original_operation_id: "original".into(),
                    replacement_checks: vec![],
                    reason: "selector only".into(),
                }),
            })),
            Duration::from_secs(60)
        );
        assert_eq!(
            response_deadline(&op(WorkspaceQuery::VerificationRecheckDispositionReview {
                project_id: "p".into(),
                operation_id: "old".into(),
                reason: "retire".into(),
            })),
            Duration::from_secs(60)
        );
        let request = UserOperation::Request {
            request: Request {
                id: "review".into(),
                protocol_version: PROTOCOL_VERSION,
                command: Command::ReviewVerificationRecheckDisposition {
                    project_id: "p".into(),
                    operation_id: "old".into(),
                    reason: "retire".into(),
                },
            },
        };
        assert_eq!(response_deadline(&request), Duration::from_secs(60));
        for query in [
            WorkspaceQuery::Overview,
            WorkspaceQuery::CurrentVerificationRecheck {
                project_id: "p".into(),
            },
            WorkspaceQuery::VerificationRecheckInputs {
                project_id: "p".into(),
                original_operation_id: "original".into(),
            },
            WorkspaceQuery::RegisteredValidator {
                id: "tool".into(),
                digest: "a".repeat(64),
            },
            WorkspaceQuery::VerificationRechecks {
                project_id: "p".into(),
                offset: 0,
                revision: None,
            },
            WorkspaceQuery::VerificationRecheck {
                project_id: "p".into(),
                operation_id: "original".into(),
            },
        ] {
            assert_eq!(response_deadline(&op(query)), Duration::from_secs(15));
        }
    }
    #[test]
    fn connection_failure_preserves_absent_busy_and_identity_facts() {
        for code in [2, 231, 5] {
            let mut facts = ConnectionFailure {
                stage: "pipeOpen",
                caller_sid: Some("S-1-fixture".into()),
                ..Default::default()
            };
            let value: serde_json::Value =
                serde_json::from_str(&facts.io(io::Error::from_raw_os_error(code))).unwrap();
            assert_eq!(value["stage"], "pipeOpen");
            assert_eq!(value["win32Error"], code);
            assert_eq!(value["callerSid"], "S-1-fixture");
            assert!(value["serverPid"].is_null());
        }
    }
    #[tokio::test]
    async fn connection_failure_preserves_rejected_greeting_and_timeout() {
        let mut facts = ConnectionFailure {
            server_pid: Some(42),
            caller_sid: Some("S-1-fixture".into()),
            server_sid: Some("S-1-fixture".into()),
            ..Default::default()
        };
        let greeting = Greeting {
            document_stream: true,
            product: PRODUCT_NAME.into(),
            protocol_version: PROTOCOL_VERSION,
            server_pid: 99,
            owner_sid: "S-1-other".into(),
        };
        let (mut writer, mut reader) = tokio::io::duplex(4096);
        write_frame(&mut writer, &greeting, REQUEST_LIMIT)
            .await
            .unwrap();
        let error = connection_greeting(&mut reader, &mut facts, DEADLINE)
            .await
            .unwrap_err();
        let value: serde_json::Value = serde_json::from_str(&error).unwrap();
        assert_eq!(value["stage"], "handshakeIdentity");
        assert_eq!(value["serverPid"], 42);
        assert_eq!(value["greeting"]["serverPid"], 99);
        assert_eq!(value["greeting"]["ownerSid"], "S-1-other");
        assert!(value["win32Error"].is_null());
        let (_writer, mut reader) = tokio::io::duplex(64);
        let error = connection_greeting(&mut reader, &mut facts, Duration::ZERO)
            .await
            .unwrap_err();
        let value: serde_json::Value = serde_json::from_str(&error).unwrap();
        assert_eq!(value["stage"], "handshakeTimeout");
        assert_eq!(value["callerSid"], "S-1-fixture");
    }
    #[tokio::test]
    async fn connection_failure_preserves_handshake_io_code() {
        struct Broken;
        impl AsyncRead for Broken {
            fn poll_read(
                self: std::pin::Pin<&mut Self>,
                _: &mut std::task::Context<'_>,
                _: &mut tokio::io::ReadBuf<'_>,
            ) -> std::task::Poll<io::Result<()>> {
                std::task::Poll::Ready(Err(io::Error::from_raw_os_error(109)))
            }
        }
        let error = connection_greeting(&mut Broken, &mut ConnectionFailure::default(), DEADLINE)
            .await
            .unwrap_err();
        let value: serde_json::Value = serde_json::from_str(&error).unwrap();
        assert_eq!(value["stage"], "handshakeRead");
        assert_eq!(value["win32Error"], 109);
    }
    #[test]
    fn command_receipts_preserve_guards_and_never_return_a_snapshot() {
        let temp = tempfile::tempdir().unwrap();
        let mut state = upgrade_state(temp.path());
        let op = |version, command| UserOperation::Command {
            request: Request {
                id: "receipt-guard".into(),
                protocol_version: version,
                command,
            },
        };
        let create = || Command::CreatePlan {
            title: "one original".into(),
            tasks: vec!["local only".into()],
        };
        let before = crate::reader_generation::tests::snapshot(temp.path());
        for (version, command, code) in [
            (999, create(), "protocolMismatch"),
            (PROTOCOL_VERSION, Command::Snapshot, "receiptUnsupported"),
            (
                PROTOCOL_VERSION,
                Command::ReadHistoricalSources {
                    offset: 0,
                    revision: None,
                },
                "receiptUnsupported",
            ),
            (
                PROTOCOL_VERSION,
                Command::CreatePlan {
                    title: String::new(),
                    tasks: vec![],
                },
                "commandRejected",
            ),
        ] {
            let reply = state.apply(op(version, command));
            assert_eq!(reply.response.error.unwrap().code, code);
            assert!(reply.command_receipt.is_none() && reply.response.result.is_none());
        }
        assert!(before == crate::reader_generation::tests::snapshot(temp.path()));
        let reply = state.apply(op(PROTOCOL_VERSION, create()));
        assert!(reply.response.error.is_none() && reply.response.result.is_none());
        assert!(reply.workspace.is_none() && reply.historical.is_none());
        assert_eq!(reply.command_receipt.unwrap().request_id, "receipt-guard");
        assert_eq!(state.host.snapshot().plans.len(), 1);
        let after = crate::reader_generation::tests::snapshot(temp.path());
        state.stopping = true;
        let reply = state.apply(op(PROTOCOL_VERSION, create()));
        assert_eq!(reply.response.error.unwrap().code, "coreStopping");
        assert!(reply.command_receipt.is_none());
        assert!(after == crate::reader_generation::tests::snapshot(temp.path()));
        drop(state);
        let damaged = temp.path().join("omp-startups/project");
        std::fs::create_dir_all(&damaged).unwrap();
        std::fs::write(damaged.join("attempt.0.pending"), b"{partial").unwrap();
        let mut state = upgrade_state(temp.path());
        assert!(state.host.is_diagnostic());
        let before = crate::reader_generation::tests::snapshot(temp.path());
        let reply = state.apply(op(PROTOCOL_VERSION, create()));
        assert_eq!(reply.response.error.unwrap().code, "diagnosticReadOnly");
        assert!(reply.command_receipt.is_none());
        assert!(before == crate::reader_generation::tests::snapshot(temp.path()));
    }
    #[test]
    fn workspace_read_guards_and_diagnostic_unavailable_are_explicit() {
        use cerulean_domain::workspace::*;
        let temp = tempfile::tempdir().unwrap();
        let mut state = upgrade_state(temp.path());
        let op = |version, query| UserOperation::Workspace {
            id: "workspace-guard".into(),
            protocol_version: version,
            query,
        };
        let reply = state.apply(op(999, WorkspaceQuery::Overview));
        assert_eq!(reply.response.error.unwrap().code, "protocolMismatch");
        assert!(reply.workspace.is_none());
        let reply = state.apply(op(PROTOCOL_VERSION, WorkspaceQuery::Overview));
        assert!(reply.response.result.is_none() && reply.response.error.is_none());
        let WorkspaceResult::Overview(view) = reply.workspace.unwrap() else {
            panic!("overview")
        };
        assert_eq!(view.mode, WorkspaceMode::Ready);
        assert_eq!(view.legacy_plan_count, Some(0));
        assert!(serde_json::from_value::<UserOperation>(serde_json::json!({"type":"workspace","id":"bad","protocolVersion":1,"principal":"chief","query":{"type":"overview"}})).is_err());
        state.stopping = true;
        assert_eq!(
            state
                .apply(op(PROTOCOL_VERSION, WorkspaceQuery::Overview))
                .response
                .error
                .unwrap()
                .code,
            "coreStopping"
        );
        drop(state);
        let damaged = temp.path().join("omp-startups/project");
        std::fs::create_dir_all(&damaged).unwrap();
        std::fs::write(damaged.join("attempt.0.pending"), b"{partial").unwrap();
        let before = crate::reader_generation::tests::snapshot(temp.path());
        let mut state = upgrade_state(temp.path());
        assert!(state.host.is_diagnostic());
        let reply = state.apply(op(PROTOCOL_VERSION, WorkspaceQuery::Overview));
        let WorkspaceResult::Overview(view) = reply.workspace.unwrap() else {
            panic!("overview")
        };
        assert_eq!(view.mode, WorkspaceMode::Diagnostic);
        assert_eq!(view.project_count, None);
        for query in [
            WorkspaceQuery::CurrentVerificationRecheck {
                project_id: "project".into(),
            },
            WorkspaceQuery::VerificationRecheckInputs {
                project_id: "project".into(),
                original_operation_id: "original".into(),
            },
            WorkspaceQuery::RegisteredValidator {
                id: "tool".into(),
                digest: "a".repeat(64),
            },
            WorkspaceQuery::VerificationRechecks {
                project_id: "project".into(),
                offset: 0,
                revision: None,
            },
            WorkspaceQuery::VerificationRecheck {
                project_id: "project".into(),
                operation_id: "original".into(),
            },
            WorkspaceQuery::VerificationRecheckReview {
                input: Box::new(cerulean_domain::verification_recheck::RecheckRequest {
                    project_id: "project".into(),
                    original_operation_id: "original".into(),
                    replacement_checks: vec![],
                    reason: "diagnostic guard".into(),
                }),
            },
            WorkspaceQuery::VerificationRecheckDispositionReview {
                project_id: "project".into(),
                operation_id: "original".into(),
                reason: "retain".into(),
            },
            WorkspaceQuery::Recovery {
                project_id: "project".into(),
                offset: 0,
                revision: None,
            },
            WorkspaceQuery::RecoveryDetail {
                project_id: "project".into(),
                kind: cerulean_domain::workspace::WorkspaceRecoveryKind::OmpStop,
                id: "original.pending".into(),
                revision: "a".repeat(64),
            },
            WorkspaceQuery::Jobs {
                project_id: "project".into(),
                offset: 0,
                revision: None,
            },
            WorkspaceQuery::Job {
                project_id: "project".into(),
                kind: cerulean_domain::workspace::WorkspaceJobKind::Omp,
                id: "attempt".into(),
                revision: "a".repeat(64),
            },
            WorkspaceQuery::ExecutionReview {
                project_id: "project".into(),
                digest: None,
            },
            WorkspaceQuery::Tasks {
                project_id: "project".into(),
                offset: 0,
                revision: None,
            },
            WorkspaceQuery::Task {
                project_id: "project".into(),
                task_id: "task".into(),
                revision: "a".repeat(64),
            },
            WorkspaceQuery::TaskEvidence {
                project_id: "project".into(),
                task_id: "task".into(),
                revision: "a".repeat(64),
                offset: 0,
            },
        ] {
            let reply = state.apply(op(PROTOCOL_VERSION, query));
            assert!(reply.workspace.is_none() && reply.response.result.is_none());
            assert_eq!(reply.response.error.unwrap().code, "workspaceReadRejected");
        }
        assert!(state
            .apply(op(
                PROTOCOL_VERSION,
                WorkspaceQuery::Projects {
                    offset: 0,
                    revision: None
                }
            ))
            .response
            .error
            .is_some());
        assert!(state
            .apply(op(
                PROTOCOL_VERSION,
                WorkspaceQuery::Plans {
                    offset: 0,
                    revision: None
                }
            ))
            .workspace
            .is_some());
        assert!(before == crate::reader_generation::tests::snapshot(temp.path()));
    }
    #[test]
    fn historical_result_protocol_command_and_diagnostic_guards_preserve_records() {
        let temp = tempfile::tempdir().unwrap();
        let mut state = upgrade_state(temp.path());
        for (version, command, code) in [
            (
                999,
                Command::ReadHistoricalSources {
                    offset: 0,
                    revision: None,
                },
                "protocolMismatch",
            ),
            (
                PROTOCOL_VERSION,
                Command::CreatePlan {
                    title: "forbidden route".into(),
                    tasks: vec!["never run".into()],
                },
                "historicalRejected",
            ),
        ] {
            let reply = state.apply(UserOperation::Historical {
                request: Request {
                    id: "guard".into(),
                    protocol_version: version,
                    command,
                },
            });
            assert_eq!(reply.response.error.unwrap().code, code);
            assert!(reply.historical.is_none());
        }
        assert!(state.host.snapshot().plans.is_empty());
        let reply = state.apply(UserOperation::Historical {
            request: Request {
                id: "bounded".into(),
                protocol_version: PROTOCOL_VERSION,
                command: Command::ReadHistoricalSources {
                    offset: 0,
                    revision: None,
                },
            },
        });
        assert!(reply.response.error.is_none() && reply.response.result.is_none());
        assert_eq!(
            reply.historical.unwrap().historical_sources.unwrap().total,
            0
        );
        assert!(serde_json::from_value::<UserOperation>(serde_json::json!({"type":"historical","principal":"chief","request":{"id":"forge","protocolVersion":1,"command":{"type":"readHistoricalSources","offset":0,"revision":null}}})).is_err());
        drop(state);
        let bad = temp.path().join("omp-startups/project");
        std::fs::create_dir_all(&bad).unwrap();
        std::fs::write(bad.join("attempt.0.pending"), b"{partial").unwrap();
        let mut state = upgrade_state(temp.path());
        assert!(state.host.is_diagnostic());
        let reply = state.apply(UserOperation::Historical {
            request: Request {
                id: "diagnostic".into(),
                protocol_version: PROTOCOL_VERSION,
                command: Command::ReadHistoricalSources {
                    offset: 0,
                    revision: None,
                },
            },
        });
        assert_eq!(reply.response.error.unwrap().code, "diagnosticReadOnly");
        assert!(reply.historical.is_none());
    }
    #[test]
    fn count_resolution_protocol_command_and_diagnostic_guards_preserve_records() {
        let temp = tempfile::tempdir().unwrap();
        let mut state = upgrade_state(temp.path());
        for (version, command, code) in [
            (
                999,
                Command::ReadManagementCounts {
                    offset: 0,
                    revision: None,
                },
                "protocolMismatch",
            ),
            (
                PROTOCOL_VERSION,
                Command::CreatePlan {
                    title: "forbidden route".into(),
                    tasks: vec!["never run".into()],
                },
                "countResolutionRejected",
            ),
        ] {
            let reply = state.apply(UserOperation::CountResolution {
                request: Request {
                    id: "guard".into(),
                    protocol_version: version,
                    command,
                },
            });
            assert_eq!(reply.response.error.unwrap().code, code);
            assert!(reply.count_resolution.is_none());
        }
        assert!(state.host.snapshot().plans.is_empty());
        let reply = state.apply(UserOperation::CountResolution {
            request: Request {
                id: "bounded".into(),
                protocol_version: PROTOCOL_VERSION,
                command: Command::ReadManagementCounts {
                    offset: 0,
                    revision: None,
                },
            },
        });
        assert!(reply.response.error.is_none() && reply.response.result.is_none());
        assert_eq!(reply.count_resolution.unwrap().counts.unwrap().total, 0);
        assert!(serde_json::from_value::<UserOperation>(serde_json::json!({"type":"countResolution","principal":"chief","request":{"id":"forge","protocolVersion":1,"command":{"type":"readManagementCounts","offset":0,"revision":null}}})).is_err());
        drop(state);
        let bad = temp.path().join("omp-startups/project");
        std::fs::create_dir_all(&bad).unwrap();
        std::fs::write(bad.join("attempt.0.pending"), b"{partial").unwrap();
        let mut state = upgrade_state(temp.path());
        assert!(state.host.is_diagnostic());
        let reply = state.apply(UserOperation::CountResolution {
            request: Request {
                id: "diagnostic".into(),
                protocol_version: PROTOCOL_VERSION,
                command: Command::ReadManagementCounts {
                    offset: 0,
                    revision: None,
                },
            },
        });
        assert_eq!(reply.response.error.unwrap().code, "diagnosticReadOnly");
        assert!(reply.count_resolution.is_none());
    }
    #[test]
    fn historical_result_maximum_escaped_review_fits_whole_service_reply() {
        use cerulean_domain::historical_money::*;
        let review = crate::historical_confirmation::HistoricalReview {
            schema: 1,
            project_id: "project".into(),
            source: HistoricalSource {
                scope: HistoricalScope::Management {
                    operation_id: "old".into(),
                    original_project_id: Some("project".into()),
                },
                digest: "a".repeat(64),
                references: vec!["\u{1}".repeat(2048); 256],
                uncertain_outcomes: u32::MAX,
            },
            policy_digest: "b".repeat(64),
            previous_ledger_head: "c".repeat(64),
            amount_usd_nanos: 17,
            billing_reference: "\u{1}".repeat(8000),
            explanation: "\u{1}".repeat(8000),
        };
        review
            .validate("project", &review.digest().unwrap())
            .unwrap();
        for confirm in [false, true] {
            let mut result = HistoricalResult::default();
            if confirm {
                result.historical_confirmation = Some(HistoricalConfirmationLookup {
                    project_id: "project".into(),
                    scope: review.source.scope.clone(),
                    review_digest: review.digest().unwrap(),
                    record: Some(HistoricalAccounting {
                        review: review.view().unwrap(),
                        user_event_digest: "d".repeat(64),
                        state: HistoricalAccountingState::Accounted,
                    }),
                });
            } else {
                result.historical_charge_review = Some(review.view().unwrap());
            }
            let reply = ServiceReply {
                external_chief: None,
                response: Response {
                    id: "i".repeat(64 * 1024),
                    result: None,
                    error: None,
                },
                command_receipt: None,
                workspace: None,
                historical: Some(result),
                count_resolution: None,
                stopping: false,
                upgrade_review: None,
                preservation_review: None,
                upgrade_hold: None,
            };
            let bytes = serde_json::to_vec(&reply).unwrap();
            assert!(bytes.len() + 2 * 64 * 1024 + 16 * 1024 < RESPONSE_LIMIT);
            assert_eq!(bounded_reply_bytes(&reply).unwrap(), bytes);
        }
    }
    #[tokio::test]
    async fn response_budget_exact_limit_and_overflow_have_correlated_complete_frames() {
        let root = tempfile::tempdir().unwrap();
        let mut state = upgrade_state(root.path());
        let mut reply = state.apply(UserOperation::Request {
            request: Request {
                id: "boundary-original-request".into(),
                protocol_version: PROTOCOL_VERSION,
                command: Command::Snapshot,
            },
        });
        let initial = serde_json::to_vec(&reply).unwrap().len();
        reply
            .response
            .result
            .as_mut()
            .unwrap()
            .events
            .push(cerulean_domain::EventSummary {
                sequence: 1,
                kind: "controlled-capacity".into(),
                at: "2026-10-14T00:00:00Z".into(),
                summary: String::new(),
            });
        let overhead = serde_json::to_vec(&reply).unwrap().len();
        assert!(overhead > initial);
        reply.response.result.as_mut().unwrap().events[0].summary =
            "x".repeat(RESPONSE_LIMIT - overhead);
        let exact = bounded_reply_bytes(&reply).unwrap();
        assert_eq!(exact.len(), RESPONSE_LIMIT);
        assert!(serde_json::from_slice::<ServiceReply>(&exact)
            .unwrap()
            .response
            .error
            .is_none());
        reply.response.result.as_mut().unwrap().events[0]
            .summary
            .push('x');
        let before = serde_json::to_vec(&reply).unwrap();
        let (mut writer, mut reader) = tokio::io::duplex(1024);
        let (stop, stopped) = watch::channel(false);
        let (sent, received) = tokio::join!(
            send_reply(&mut writer, &reply, &stop),
            read_frame::<_, ServiceReply>(&mut reader, RESPONSE_LIMIT)
        );
        sent.unwrap();
        let received = received.unwrap();
        assert_eq!(received.response.id, "boundary-original-request");
        assert_eq!(received.response.error.unwrap().code, "responseTooLarge");
        assert!(received.response.result.is_none() && !received.stopping && !*stopped.borrow());
        assert_eq!(serde_json::to_vec(&reply).unwrap(), before);
    }
    #[tokio::test]
    async fn response_budget_oversized_shutdown_keeps_stop_barrier_even_if_delivery_fails() {
        let root = tempfile::tempdir().unwrap();
        let mut state = upgrade_state(root.path());
        let mut reply = state.apply(UserOperation::Shutdown {
            id: "oversized-stop".into(),
            protocol_version: PROTOCOL_VERSION,
        });
        assert!(reply.stopping);
        reply.response.result.as_mut().unwrap().data_directory = "x".repeat(RESPONSE_LIMIT);
        let fallback: ServiceReply =
            serde_json::from_slice(&bounded_reply_bytes(&reply).unwrap()).unwrap();
        assert!(fallback.stopping);
        assert_eq!(fallback.response.error.unwrap().code, "responseTooLarge");
        let (mut writer, reader) = tokio::io::duplex(64);
        drop(reader);
        let (stop, stopped) = watch::channel(false);
        assert!(send_reply(&mut writer, &reply, &stop).await.is_err());
        assert!(*stopped.borrow());
        assert!(state.stopping);
    }
    #[test]
    fn preserved_shutdown_ipc_requires_fresh_review_and_fences_all_requests() {
        let root = tempfile::tempdir().unwrap();
        let host = crate::host::preserve_shutdown_host::tests::fixture(root.path());
        let mut state = ServiceState {
            host: ServiceHost::Ready(Box::new(host)),
            stopping: false,
            preservation_review: None,
        };
        let review = |id: &str| UserOperation::ReviewPreservedShutdown {
            id: id.into(),
            protocol_version: PROTOCOL_VERSION,
        };
        let stop = |digest: String| UserOperation::PreserveShutdown {
            id: "preserve-stop".into(),
            protocol_version: PROTOCOL_VERSION,
            review_digest: digest,
        };
        assert_eq!(
            state
                .apply(stop("a".repeat(64)))
                .response
                .error
                .unwrap()
                .code,
            "preservationReviewRequired"
        );
        let original = state.apply(review("original")).preservation_review.unwrap();
        assert_eq!(original.unknown_tasks, 1);
        // A changed authoritative base ledger makes the review stale.
        let changed = state.apply(UserOperation::Request {
            request: Request {
                id: "change".into(),
                protocol_version: PROTOCOL_VERSION,
                command: Command::CreatePlan {
                    title: "local metadata changed".into(),
                    tasks: vec!["no execution".into()],
                },
            },
        });
        assert!(changed.response.error.is_none());
        assert_eq!(
            state
                .apply(stop(original.digest))
                .response
                .error
                .unwrap()
                .code,
            "preservationReviewChanged"
        );
        assert!(!state.stopping);
        let fresh = state.apply(review("fresh")).preservation_review.unwrap();
        let before = serde_json::to_value(state.host.snapshot()).unwrap();
        // Read-only review cannot grant ordinary stop or upgrade authorization.
        assert!(!state.host.can_shutdown());
        assert!(!crate::upgrade::review(&mut state.host).unwrap().0.ready);
        let stopped = state.apply(stop(fresh.digest));
        assert!(stopped.response.error.is_none() && stopped.stopping);
        assert_eq!(serde_json::to_value(state.host.snapshot()).unwrap(), before);
        assert_eq!(
            state
                .apply(review("after-stop"))
                .response
                .error
                .unwrap()
                .code,
            "coreStopping"
        );
        drop(state);
        let mut reopened = ServiceState {
            host: ServiceHost::open(&root.path().join("data")).unwrap(),
            stopping: false,
            preservation_review: None,
        };
        assert_eq!(
            reopened
                .apply(stop(stopped.preservation_review.unwrap().digest))
                .response
                .error
                .unwrap()
                .code,
            "preservationReviewRequired"
        );
    }
    #[tokio::test]
    #[ignore = "owned synthetic service child; requires its parent-provided directory"]
    async fn preserved_shutdown_owned_service() {
        let root = std::path::PathBuf::from(
            std::env::var_os("CERULEAN_PRESERVE_NATIVE_ROOT").expect("owned fixture directory"),
        );
        let data = root.join("data");
        // The parent creates the fixture before connecting. Directory existence
        // is not a readiness marker: Client::connect also creates this directory.
        let host = crate::host::RuntimeHost::open(&data).unwrap();
        crate::derived_index::wait_for_test_projections(&data);
        // A failed test cannot leave a resident fixture indefinitely. Timeout is
        // a test failure, never a claimed normal product shutdown.
        tokio::time::timeout(Duration::from_secs(45), serve_host(&data, host))
            .await
            .expect("owned fixture did not receive reviewed shutdown")
            .unwrap();
    }

    #[tokio::test]
    #[ignore = "real named pipe, child handles and repeated cold recovery; zero model calls"]
    async fn preserved_shutdown_native_process_exit_and_cold_reopen() {
        let root = std::path::PathBuf::from(
            std::env::var_os("CERULEAN_PRESERVE_NATIVE_ROOT")
                .expect("new retained evidence directory"),
        );
        assert!(!root.exists());
        std::fs::create_dir(&root).unwrap();
        drop(crate::host::preserve_shutdown_host::tests::fixture(&root));
        let executable = std::env::current_exe().unwrap();
        let mut original_data = None;
        for round in 0..2 {
            let mut command = std::process::Command::new(&executable);
            command
                .args([
                    "--exact",
                    "ipc::tests::preserved_shutdown_owned_service",
                    "--ignored",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env("CERULEAN_PRESERVE_NATIVE_ROOT", &root)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
            let child = command.spawn().unwrap();
            let pid = child.id();
            let data = root.join("data");
            let deadline = tokio::time::Instant::now() + Duration::from_secs(15);
            let connection = loop {
                match Client::connect(&data).await {
                    Ok(c) => break c,
                    Err(_) => {
                        assert!(
                            tokio::time::Instant::now() < deadline,
                            "fixture startup not observed"
                        );
                        tokio::time::sleep(Duration::from_millis(25)).await;
                    }
                }
            };
            assert_eq!(connection.server_pid, pid);
            let rejected = connection
                .call(UserOperation::Shutdown {
                    id: "ordinary-stop".into(),
                    protocol_version: PROTOCOL_VERSION,
                })
                .await
                .unwrap();
            assert_eq!(
                rejected.response.error.as_ref().map(|e| e.code.as_str()),
                Some("unresolvedTasks")
            );
            assert!(!rejected.stopping);
            let reviewed = Client::connect(&data)
                .await
                .unwrap()
                .call(UserOperation::ReviewPreservedShutdown {
                    id: "review-original".into(),
                    protocol_version: PROTOCOL_VERSION,
                })
                .await
                .unwrap();
            assert!(
                reviewed.response.error.is_none(),
                "{:?}",
                reviewed.response.error
            );
            let proof = reviewed.preservation_review.unwrap();
            assert_eq!(proof.unknown_tasks, 1);
            let stopped = Client::connect(&data)
                .await
                .unwrap()
                .call_and_wait_stop(UserOperation::PreserveShutdown {
                    id: "stop-original".into(),
                    protocol_version: PROTOCOL_VERSION,
                    review_digest: proof.digest,
                })
                .await
                .unwrap();
            assert!(stopped.stopping && stopped.response.error.is_none());
            let output = child.wait_with_output().unwrap();
            std::fs::write(root.join(format!("child-{round}.stdout")), &output.stdout).unwrap();
            std::fs::write(root.join(format!("child-{round}.stderr")), &output.stderr).unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
            let cold = crate::reader_generation::tests::snapshot(&data);
            if let Some(before) = &original_data {
                assert_eq!(before, &cold);
            } else {
                original_data = Some(cold);
            }
            assert_eq!(
                std::fs::read_to_string(root.join("project/sum.mjs")).unwrap(),
                "export const partial = 1;\n"
            );
            println!("preserved native round={round}: original PID observed, normal exit0, pipes EOF, original Unknown and files retained, no redispatch");
        }
    }
    fn upgrade_state(root: &Path) -> ServiceState {
        ServiceState {
            host: ServiceHost::open(root).unwrap(),
            stopping: false,
            preservation_review: None,
        }
    }
    #[test]
    fn upgrade_partial_hold_rejects_direct_start_before_creating_any_store_files() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join(crate::upgrade::HOLD), b"{partial").unwrap();
        assert!(ServiceHost::open(root.path()).is_err());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
        assert!(!root.path().join("events").exists());
        assert!(!root.path().join("writer.lock").exists());
    }
    fn upgrade_review(state: &mut ServiceState) -> crate::upgrade::UpgradeReview {
        let reply = state.apply(UserOperation::ReviewUpgrade {
            id: "review".into(),
            protocol_version: PROTOCOL_VERSION,
        });
        assert!(reply.response.error.is_none());
        reply.upgrade_review.unwrap()
    }
    fn prepare_upgrade(state: &mut ServiceState, digest: &str) -> ServiceReply {
        state.apply(UserOperation::PrepareUpgrade {
            id: "prepare".into(),
            protocol_version: PROTOCOL_VERSION,
            review_digest: digest.into(),
        })
    }
    #[test]
    fn upgrade_hold_blocks_restart_and_late_requests_until_reviewed_release() {
        let root = tempfile::tempdir().unwrap();
        let mut state = upgrade_state(root.path());
        let review = upgrade_review(&mut state);
        assert!(review.ready);
        let reply = prepare_upgrade(&mut state, &review.digest);
        assert!(reply.response.error.is_none() && reply.stopping);
        let hold = reply.upgrade_hold.unwrap();
        let response = state.apply(UserOperation::Request {
            request: Request {
                id: "late".into(),
                protocol_version: PROTOCOL_VERSION,
                command: Command::CreatePlan {
                    title: "must not execute".into(),
                    tasks: vec!["late".into()],
                },
            },
        });
        assert_eq!(response.response.error.unwrap().code, "coreStopping");
        assert!(state.host.snapshot().plans.is_empty());
        assert!(crate::upgrade::release(root.path(), &hold.hold_digest).is_err());
        drop(state);
        assert!(ServiceHost::open(root.path()).is_err());
        assert!(crate::upgrade::release(root.path(), &"0".repeat(64)).is_err());
        crate::upgrade::release(root.path(), &hold.hold_digest).unwrap();
        let state = upgrade_state(root.path());
        assert!(state.host.snapshot().plans.is_empty());
        assert_eq!(
            std::fs::read_dir(root.path().join("upgrade-holds"))
                .unwrap()
                .count(),
            1
        );
    }
    #[test]
    fn upgrade_requires_current_review_and_valid_protocol() {
        let root = tempfile::tempdir().unwrap();
        let mut state = upgrade_state(root.path());
        let review = upgrade_review(&mut state);
        let project = root.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let reply = state.apply(UserOperation::Request {
            request: Request {
                id: "change".into(),
                protocol_version: PROTOCOL_VERSION,
                command: Command::RegisterProject {
                    project: cerulean_domain::execution::Project {
                        id: "new".into(),
                        root: project.to_string_lossy().into(),
                        timezone: "Asia/Shanghai".into(),
                    },
                },
            },
        });
        assert!(reply.response.error.is_none());
        assert_eq!(
            prepare_upgrade(&mut state, &review.digest)
                .response
                .error
                .unwrap()
                .code,
            "upgradeNotReady"
        );
        let invalid = state.apply(UserOperation::PrepareUpgrade {
            id: "bad".into(),
            protocol_version: 99,
            review_digest: upgrade_review(&mut upgrade_state(&root.path().join("other"))).digest,
        });
        assert_eq!(invalid.response.error.unwrap().code, "protocolMismatch");
        assert!(!root.path().join(crate::upgrade::HOLD).exists());
        assert!(!state.stopping);
    }
    #[test]
    fn upgrade_diagnostic_shutdown_permission_is_not_upgrade_permission() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("omp-startups/project")).unwrap();
        std::fs::write(
            root.path().join("omp-startups/project/attempt.0.pending"),
            b"{broken",
        )
        .unwrap();
        let mut state = upgrade_state(root.path());
        assert!(state.host.can_shutdown());
        let review = upgrade_review(&mut state);
        assert!(!review.ready);
        assert!(review
            .blockers
            .iter()
            .any(|b| b == "diagnostic-or-stop-review"));
        assert!(prepare_upgrade(&mut state, &review.digest)
            .response
            .error
            .is_some());
        assert!(!root.path().join(crate::upgrade::HOLD).exists());
    }
    #[test]
    fn upgrade_backup_remains_held_after_restore_and_partial_hold_can_be_reviewed() {
        let parent = tempfile::tempdir().unwrap();
        let data = parent.path().join("data");
        let mut state = upgrade_state(&data);
        let review = upgrade_review(&mut state);
        let hold = prepare_upgrade(&mut state, &review.digest)
            .upgrade_hold
            .unwrap();
        drop(state);
        let archive = parent.path().join("backup");
        assert!(crate::backup::create_held(&data, &archive, &"0".repeat(64)).is_err());
        assert!(!archive.exists());
        let backup = crate::backup::create_held(&data, &archive, &hold.hold_digest).unwrap();
        std::fs::rename(&data, parent.path().join("preserved")).unwrap();
        crate::backup::restore(&archive, &data, &backup.manifest_digest).unwrap();
        assert!(ServiceHost::open(&data).is_err());
        assert_eq!(
            crate::upgrade::status(&data).unwrap().hold_digest,
            hold.hold_digest
        );
        crate::upgrade::release(&data, &hold.hold_digest).unwrap();
        drop(upgrade_state(&data));
        std::fs::write(data.join(crate::upgrade::HOLD), b"{partial").unwrap();
        let partial = crate::upgrade::status(&data).unwrap();
        assert!(!partial.valid_record);
        assert!(ServiceHost::open(&data).is_err());
        assert!(crate::backup::create_held(
            &data,
            &parent.path().join("bad-backup"),
            &partial.hold_digest
        )
        .is_err());
        crate::upgrade::release(&data, &partial.hold_digest).unwrap();
        drop(upgrade_state(&data));
    }
    #[tokio::test]
    async fn upgrade_stop_signal_survives_lost_ack() {
        let root = tempfile::tempdir().unwrap();
        let mut state = upgrade_state(root.path());
        let review = upgrade_review(&mut state);
        let reply = prepare_upgrade(&mut state, &review.digest);
        let (mut writer, reader) = tokio::io::duplex(64);
        drop(reader);
        let (stop, stopped) = watch::channel(false);
        assert!(send_reply(&mut writer, &reply, &stop).await.is_err());
        assert!(*stopped.borrow());
        assert!(state.stopping);
        assert!(crate::upgrade::status(root.path()).unwrap().valid_record);
    }
    #[tokio::test]
    async fn actual_pipe_dacl_contains_only_current_user() {
        use windows_sys::Win32::Security::{
            Authorization::{GetSecurityInfo, SE_KERNEL_OBJECT},
            GetAce, ACCESS_ALLOWED_ACE, DACL_SECURITY_INFORMATION,
        };
        let root = tempfile::tempdir().unwrap();
        let sid = current_sid().unwrap();
        let name = endpoint(root.path()).unwrap();
        let pipe = listener(&name, &sid, true).unwrap();
        assert!(listener(&name, &sid, true).is_err());
        unsafe {
            let mut dacl = ptr::null_mut();
            let mut descriptor = ptr::null_mut();
            assert_eq!(
                GetSecurityInfo(
                    pipe.as_raw_handle(),
                    SE_KERNEL_OBJECT,
                    DACL_SECURITY_INFORMATION,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    &mut dacl,
                    ptr::null_mut(),
                    &mut descriptor
                ),
                0
            );
            let _memory = LocalAllocation(descriptor);
            assert!(!dacl.is_null());
            assert_eq!((*dacl).AceCount, 1);
            let mut ace = ptr::null_mut();
            check(GetAce(dacl, 0, &mut ace)).unwrap();
            let ace = &*ace.cast::<ACCESS_ALLOWED_ACE>();
            assert_eq!(ace.Header.AceType, 0);
            let mut text = ptr::null_mut();
            check(ConvertSidToStringSidW(
                (&ace.SidStart as *const u32).cast_mut().cast(),
                &mut text,
            ))
            .unwrap();
            let _text = LocalAllocation(text.cast());
            let mut length = 0;
            while *text.add(length) != 0 {
                length += 1;
            }
            assert_eq!(
                String::from_utf16(std::slice::from_raw_parts(text, length)).unwrap(),
                sid
            );
        }
    }
}
