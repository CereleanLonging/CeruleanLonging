//! Optional A6API header observations. No execution, resend, or monetary authority.
//! Original journals keep their exact wire format; diagnostics sit outside their inventories.
use crate::host::ordinary;
use cerulean_adapters::api_generation::{gateway::GatewayHeaders, ApiFailure};
use cerulean_domain::execution::{digest, identifier, require, sha256, timestamp};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const DIRECTORY: &str = "gateway-response-evidence";
const LIMIT: u64 = 4096;
const ENDPOINT: &str = "https://api.a6api.com/v1/chat/completions";
const VIP_ENDPOINT: &str = "https://api.a6api.vip/v1/chat/completions";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Binding {
    pub lane: Lane,
    pub project_id: String,
    pub local_id: String,
    pub request_digest: String,
    pub intent_hash: String,
    /// Omitted for historical .com records to preserve their exact digest.
    /// Only the independently selected .vip endpoint may be explicit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Lane {
    Worker,
    Management,
}
impl Binding {
    pub(crate) fn endpoint_override(
        target: &cerulean_domain::api_worker::ApiTarget,
    ) -> Result<Option<String>, String> {
        match target.url()?.as_str() {
            ENDPOINT => Ok(None),
            VIP_ENDPOINT => Ok(Some(VIP_ENDPOINT.into())),
            _ => Err("gateway target is not an approved relay endpoint".into()),
        }
    }
    fn endpoint(&self) -> &str {
        self.endpoint.as_deref().unwrap_or(ENDPOINT)
    }
    fn validate(&self) -> Result<(), String> {
        require(
            self.endpoint.as_deref().is_none_or(|v| v == VIP_ENDPOINT),
            "gateway endpoint override is not the fixed vip origin",
        )?;
        identifier(&self.project_id)?;
        identifier(&self.local_id)?;
        sha256(&self.request_digest)?;
        sha256(&self.intent_hash)
    }
    fn stem(&self) -> Result<String, String> {
        digest(self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HeaderRecord {
    schema: u32,
    provider: String,
    endpoint: String,
    binding: Binding,
    observed_at: String,
    observation: GatewayHeaders,
}
impl HeaderRecord {
    fn validate(&self, binding: &Binding) -> Result<(), String> {
        require(
            self.schema == 1
                && self.provider == "a6api"
                && self.endpoint == binding.endpoint()
                && &self.binding == binding,
            "gateway header binding differs",
        )?;
        binding.validate()?;
        timestamp(&self.observed_at)?;
        self.observation.validate()
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum HeaderLink {
    NotObserved {},
    Recorded { digest: String },
    Unavailable {},
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub(crate) enum Outcome {
    Reply {},
    ManagementReceived {},
    NotSent {},
    LocalCancellation {},
    Transport {},
    Http { status: u16 },
    InvalidResponse {},
    Oversized {},
}
impl Outcome {
    pub(crate) fn failure(failure: &ApiFailure) -> Self {
        match failure {
            ApiFailure::NotSent {} => Self::NotSent {},
            ApiFailure::LocalCancellation {} => Self::LocalCancellation {},
            ApiFailure::Transport {} => Self::Transport {},
            ApiFailure::Http { status } => Self::Http { status: *status },
            ApiFailure::InvalidResponse {} => Self::InvalidResponse {},
            ApiFailure::ResponseRejected { .. } => Self::InvalidResponse {},
            ApiFailure::Oversized {} => Self::Oversized {},
        }
    }
}
/// Constructed only from the original journal, not from a sidecar or remote ID.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CompletionSource {
    pub record_hash: String,
    pub outcome_digest: String,
    pub outcome: Outcome,
    pub body_response_id: Option<String>,
}
impl CompletionSource {
    fn validate(&self) -> Result<(), String> {
        sha256(&self.record_hash)?;
        sha256(&self.outcome_digest)?;
        if let Outcome::Http { status } = self.outcome {
            require(
                (100..=599).contains(&status) && !(200..=299).contains(&status),
                "gateway completion HTTP status invalid",
            )?;
        }
        if let Some(id) = &self.body_response_id {
            require(
                !id.is_empty()
                    && id.len() <= 200
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-_.:/".contains(&b)),
                "gateway completion body ID invalid",
            )?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CompletionRecord {
    schema: u32,
    binding: Binding,
    completed_at: String,
    headers: HeaderLink,
    source: CompletionSource,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record<T> {
    body: T,
    hash: String,
}

/// Owned by one original attempt. IO failures are observational only.
pub(crate) struct GatewayAttempt {
    base: PathBuf,
    binding: Binding,
    headers: HeaderLink,
    observed: bool,
}
impl GatewayAttempt {
    pub(crate) fn new(base: &Path, binding: Binding) -> Self {
        Self {
            base: base.into(),
            binding,
            headers: HeaderLink::NotObserved {},
            observed: false,
        }
    }
    pub(crate) fn binding_matches(&self, binding: &Binding) -> bool {
        &self.binding == binding
    }
    /// Called synchronously after safe header extraction and before reading the body.
    /// IO errors do not replace the original outcome or authorize a retry.
    /// Synchronous durable IO can delay body/cancellation processing and use HTTP time.
    pub(crate) fn headers(&mut self, observation: &GatewayHeaders) {
        if self.observed {
            self.headers = HeaderLink::Unavailable {};
            return;
        }
        self.observed = true;
        self.headers = HeaderLink::Unavailable {};
        let record = HeaderRecord {
            schema: 1,
            provider: "a6api".into(),
            endpoint: self.binding.endpoint().into(),
            binding: self.binding.clone(),
            observed_at: chrono::Utc::now().to_rfc3339(),
            observation: observation.clone(),
        };
        let result = record
            .validate(&self.binding)
            .and_then(|_| publish(&self.base, &self.binding, "headers", &record));
        if let Ok(hash) = result {
            self.headers = HeaderLink::Recorded { digest: hash };
        }
    }
    pub(crate) fn finish(self, source: CompletionSource) -> Result<(), String> {
        self.binding.validate()?;
        source.validate()?;
        let record = CompletionRecord {
            schema: 1,
            binding: self.binding.clone(),
            completed_at: chrono::Utc::now().to_rfc3339(),
            headers: self.headers,
            source,
        };
        publish(&self.base, &self.binding, "completion", &record).map(|_| ())
    }
}

/// Read-only correlation, never a fee receipt or a recovery capability.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayReview {
    pub provider: String,
    pub local_id: String,
    pub body_response_id: Option<String>,
    pub gateway_request_id: Option<String>,
    pub status: String,
    pub headers: Option<GatewayHeaders>,
}
pub(crate) fn read(
    base: &Path,
    binding: &Binding,
    original: Option<&CompletionSource>,
) -> Result<GatewayReview, String> {
    binding.validate()?;
    if let Some(source) = original {
        source.validate()?;
    }
    let header = read_record::<HeaderRecord>(base, binding, "headers")?;
    if let Some(record) = &header {
        record.body.validate(binding)?;
    }
    let complete = read_record::<CompletionRecord>(base, binding, "completion")?;
    let status = if let Some(record) = &complete {
        let c = &record.body;
        timestamp(&c.completed_at)?;
        require(
            c.schema == 1 && &c.binding == binding && Some(&c.source) == original,
            "gateway completion differs from original journal",
        )?;
        match (&c.headers, &header) {
            (HeaderLink::Recorded { digest: expected }, Some(h)) => {
                sha256(expected)?;
                require(&h.hash == expected, "gateway header link changed")?;
                "linked"
            }
            (HeaderLink::NotObserved {}, None) => "noHeadersObserved",
            (HeaderLink::Unavailable {}, _) => "unavailable",
            _ => return Err("gateway header evidence missing or conflicting".into()),
        }
    } else if header.is_some() {
        "headerOnly"
    } else {
        "missing"
    };
    let headers = if status == "unavailable" {
        None
    } else {
        header.map(|r| r.body.observation)
    };
    // A conflicting/incomplete publication is not exposed as a verified ID.
    let gateway_request_id = if status == "unavailable" {
        None
    } else {
        headers.as_ref().and_then(|h| match &h.request_id {
            cerulean_adapters::api_generation::gateway::GatewayRequestId::Accepted { value } => {
                Some(value.clone())
            }
            _ => None,
        })
    };
    Ok(GatewayReview {
        provider: "a6api".into(),
        local_id: binding.local_id.clone(),
        body_response_id: original.and_then(|s| s.body_response_id.clone()),
        gateway_request_id,
        status: status.into(),
        headers,
    })
}

fn check_base(base: &Path) -> Result<(), String> {
    require(
        base.is_absolute()
            && !base
                .components()
                .any(|p| matches!(p, std::path::Component::ParentDir)),
        "gateway evidence path invalid",
    )?;
    for p in base.ancestors() {
        ordinary(p, true).map_err(|_| "gateway evidence path unavailable")?;
    }
    Ok(())
}
fn pin_base(base: &Path) -> Result<Vec<File>, String> {
    check_base(base)?;
    #[cfg(windows)]
    {
        let mut files = Vec::new();
        for path in base.ancestors() {
            files.push(
                crate::backup::open(path, true, false)
                    .map_err(|_| "gateway evidence path unavailable")?,
            );
        }
        // The held handles deny deletion/replacement on Windows. Recheck after
        // acquiring every ancestor, before creating any child at the path.
        check_base(base)?;
        Ok(files)
    }
    #[cfg(not(windows))]
    {
        Ok(Vec::new())
    }
}
fn pin_directory(directory: &Path) -> Result<Option<File>, String> {
    ordinary(directory, true).map_err(|_| "gateway directory invalid")?;
    #[cfg(windows)]
    {
        crate::backup::open(directory, true, false)
            .map(Some)
            .map_err(|_| "gateway directory unavailable".into())
    }
    #[cfg(not(windows))]
    {
        Ok(None)
    }
}
fn path(base: &Path, binding: &Binding, suffix: &str) -> Result<PathBuf, String> {
    require(
        ["headers", "completion"].contains(&suffix),
        "gateway evidence suffix invalid",
    )?;
    Ok(base
        .join(DIRECTORY)
        .join(format!("{}.{}.json", binding.stem()?, suffix)))
}
fn absent(path: &Path) -> Result<(), String> {
    require(
        fs::symlink_metadata(path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
        "gateway evidence already exists or inaccessible",
    )
}
fn publish<T: Serialize>(
    base: &Path,
    binding: &Binding,
    suffix: &str,
    body: &T,
) -> Result<String, String> {
    let _base_pins = pin_base(base)?;
    binding.validate()?;
    let hash = digest(body)?;
    let bytes = serde_json::to_vec(&Record {
        body,
        hash: hash.clone(),
    })
    .map_err(|_| "gateway encoding failed")?;
    require(bytes.len() <= LIMIT as usize, "gateway evidence too large")?;
    let directory = base.join(DIRECTORY);
    match fs::create_dir(&directory) {
        Ok(()) => (),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(_) => return Err("gateway directory unavailable".into()),
    }
    let _directory_pin = pin_directory(&directory)?;
    let final_path = path(base, binding, suffix)?;
    let pending = final_path.with_extension("pending");
    absent(&final_path)?;
    absent(&pending)?;
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&pending)
        .map_err(|_| "gateway publication unavailable")?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "gateway publication incomplete")?;
    drop(file);
    fs::hard_link(&pending, &final_path).map_err(|_| "gateway publication incomplete")?;
    fs::remove_file(pending).map_err(|_| "gateway pending retained")?;
    Ok(hash)
}
fn read_record<T: Serialize + DeserializeOwned>(
    base: &Path,
    binding: &Binding,
    suffix: &str,
) -> Result<Option<Record<T>>, String> {
    let _base_pins = pin_base(base)?;
    let directory = base.join(DIRECTORY);
    match fs::symlink_metadata(&directory) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("gateway directory unreadable".into()),
        Ok(_) => (),
    }
    let _directory_pin = pin_directory(&directory)?;
    let file_path = path(base, binding, suffix)?;
    absent(&file_path.with_extension("pending"))?;
    match fs::symlink_metadata(&file_path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("gateway evidence unreadable".into()),
        Ok(_) => (),
    }
    ordinary(&file_path, false).map_err(|_| "gateway evidence invalid")?;
    #[cfg(windows)]
    let file =
        crate::backup::open(&file_path, false, false).map_err(|_| "gateway evidence unreadable")?;
    #[cfg(not(windows))]
    let file = File::open(&file_path).map_err(|_| "gateway evidence unreadable")?;
    let mut bytes = Vec::new();
    file.take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "gateway evidence unreadable")?;
    require(bytes.len() <= LIMIT as usize, "gateway evidence too large")?;
    let record: Record<T> =
        serde_json::from_slice(&bytes).map_err(|_| "gateway evidence invalid")?;
    require(
        digest(&record.body)? == record.hash,
        "gateway evidence digest changed",
    )?;
    Ok(Some(record))
}

#[cfg(test)]
mod tests;
