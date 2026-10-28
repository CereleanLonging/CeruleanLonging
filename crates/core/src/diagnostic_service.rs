//! Service fallback when the base store is healthy but the full execution host
//! cannot load. Only snapshots and explicit evidence reviews are allowed; no
//! partially trusted scheduler is created.
use crate::{
    host::{ordinary, RuntimeHost},
    Store,
};
use cerulean_domain::{
    ApiError, Command, CoreDiagnostic, DiagnosticFile, Request, Response, Snapshot,
    PROTOCOL_VERSION,
};
use sha2::{Digest, Sha256};
use std::{fs, io::Read, path::Path};

pub(crate) enum ServiceHost {
    Ready(Box<RuntimeHost>),
    Diagnostic(Box<DiagnosticHost>),
    Importing(Box<crate::team_import_service::Importing>),
}
pub(crate) struct DiagnosticHost {
    // Store owns the same exclusive writer.lock as the execution host. A
    // competing/healthy core cannot launch while diagnostics are being read.
    base: Store,
    message: String,
    detected_at: String,
}
impl ServiceHost {
    pub(crate) fn workspace_query(
        &self,
        query: &cerulean_domain::workspace::WorkspaceQuery,
    ) -> Result<cerulean_domain::workspace::WorkspaceResult, String> {
        use cerulean_domain::workspace::*;
        match self {
            Self::Ready(host) => host.workspace_query(query),
            Self::Importing(job) => {
                if matches!(query, WorkspaceQuery::Overview) {
                    Ok(job.overview.clone())
                } else {
                    Err("团队材料正在导入，原核心继续工作；稍后读取同一记录，不重复提交".into())
                }
            }
            Self::Diagnostic(host) => match query {
                WorkspaceQuery::Overview => crate::workspace_read::bounded(
                    WorkspaceResult::Overview(host.base.workspace_overview(
                        WorkspaceMode::Diagnostic,
                        Some(host.message.clone()),
                        None,
                    )),
                ),
                WorkspaceQuery::Plans { .. } | WorkspaceQuery::Plan { .. } => {
                    host.base.workspace_plans(query)
                }
                WorkspaceQuery::Projects { .. }
                | WorkspaceQuery::History { .. }
                | WorkspaceQuery::HistoryEntry { .. }
                | WorkspaceQuery::Configurations { .. }
                | WorkspaceQuery::Configuration { .. }
                | WorkspaceQuery::Recovery { .. }
                | WorkspaceQuery::RecoveryDetail { .. }
                | WorkspaceQuery::Jobs { .. }
                | WorkspaceQuery::Job { .. }
                | WorkspaceQuery::CodexStartups { .. }
                | WorkspaceQuery::CodexStartup { .. }
                | WorkspaceQuery::ProjectRuns { .. }
                | WorkspaceQuery::ProjectRun { .. }
                | WorkspaceQuery::RevisionReviews { .. }
                | WorkspaceQuery::RevisionReview { .. }
                | WorkspaceQuery::UnsentAssignments { .. }
                | WorkspaceQuery::UnsentAssignment { .. }
                | WorkspaceQuery::CodexCancellations { .. }
                | WorkspaceQuery::CodexCancellation { .. }
                | WorkspaceQuery::ManagementRecordReviews { .. }
                | WorkspaceQuery::ManagementRecordReview { .. }
                | WorkspaceQuery::ApiPublicationReviews { .. }
                | WorkspaceQuery::ApiPublicationReview { .. }
                | WorkspaceQuery::ManagementDecisions { .. }
                | WorkspaceQuery::ManagementDecision { .. }
                | WorkspaceQuery::ManagementGatewayEvidence { .. }
                | WorkspaceQuery::VerificationResult { .. }
                | WorkspaceQuery::VerificationResultArtifacts { .. }
                | WorkspaceQuery::VerificationResultStages { .. }
                | WorkspaceQuery::WorkerObservation { .. }
                | WorkspaceQuery::WorkerObservationEvents { .. }
                | WorkspaceQuery::WorkerObservationIncidents { .. }
                | WorkspaceQuery::Tasks { .. }
                | WorkspaceQuery::Task { .. }
                | WorkspaceQuery::TaskEvidence { .. }
                | WorkspaceQuery::ExecutionReview { .. } => {
                    Err("项目记录处于诊断状态，清单暂不可用；不代表没有项目".into())
                }
            },
        }
    }
    pub(crate) fn open(root: &Path) -> Result<Self, String> {
        match RuntimeHost::open(root) {
            Ok(host) => Ok(Self::Ready(Box::new(host))),
            Err(message) => {
                // Failure of the authoritative base store/lock still fails
                // closed. Do not misreport a competing writer as damaged data.
                let base = Store::open(root)?;
                ordinary(&base.root, true)?;
                Ok(Self::Diagnostic(Box::new(DiagnosticHost {
                    base,
                    message: message.chars().take(1000).collect(),
                    detected_at: chrono::Utc::now().to_rfc3339(),
                })))
            }
        }
    }
    pub(crate) fn directory(&self) -> &Path {
        match self {
            Self::Ready(h) => h.directory(),
            Self::Diagnostic(h) => &h.base.root,
            Self::Importing(job) => &job.root,
        }
    }
    pub(crate) fn is_diagnostic(&self) -> bool {
        matches!(self, Self::Diagnostic(_))
    }
    pub(crate) fn can_restart_data_diagnostics(&self) -> bool {
        self.is_diagnostic() || matches!(self,Self::Ready(host) if host.can_restart_api_fault())
    }
    pub(crate) fn snapshot(&self) -> Snapshot {
        match self {
            Self::Ready(h) => h.snapshot(),
            Self::Importing(job) => job.snapshot.clone(),
            Self::Diagnostic(h) => {
                let mut snapshot = h.base.snapshot();
                snapshot.team_import = crate::team_import_service::view(&h.base.root);
                let (startup_files, inventory_limited) = inventory(&h.base.root);
                let (management_files, management_limited) = management_inventory(&h.base.root);
                let (api_files, api_limited) = api_inventory(&h.base.root);
                let (desktop_files, desktop_limited) = desktop_inventory(&h.base.root);
                snapshot.core_diagnostic = Some(CoreDiagnostic {
                    desktop_files,
                    api_files,
                    message: h.message.clone(),
                    detected_at: h.detected_at.clone(),
                    startup_files,
                    management_files,
                    inventory_limited: inventory_limited
                        || management_limited
                        || api_limited
                        || desktop_limited,
                });
                snapshot
            }
        }
    }
    pub(crate) fn poll_verifications(&mut self) {
        self.poll_import();
        if let Self::Ready(h) = self {
            h.poll_verifications();
        }
    }
    pub(crate) fn poll_import(&mut self) {
        if let Self::Importing(job) = self {
            if let Some(host) = job.take_finished() {
                *self = Self::Ready(host);
            }
        }
    }
    pub(crate) fn import_fenced(&self) -> bool {
        matches!(self, Self::Importing(_))
            || crate::team_import_service::unresolved(self.directory())
    }
    pub(crate) fn can_shutdown(&mut self) -> bool {
        match self {
            Self::Ready(h) => h.can_shutdown(),
            Self::Diagnostic(_) => true,
            Self::Importing(_) => false,
        }
    }
    pub(crate) fn can_restart_reviewed_stops(&self) -> bool {
        match self {
            Self::Ready(h) => h.can_restart_reviewed_stops(),
            Self::Diagnostic(_) => false,
            Self::Importing(_) => false,
        }
    }
    pub(crate) fn handle(&mut self, request: Request) -> Response {
        self.handle_with_snapshot(request, true)
    }
    pub(crate) fn handle_command(&mut self, request: Request) -> Response {
        if !request.command.supports_receipt() {
            return Response {
                id: request.id,
                result: None,
                error: Some(ApiError {
                    code: "receiptUnsupported".into(),
                    message: "此操作需要原查询/详情接口，未执行".into(),
                }),
            };
        }
        self.handle_with_snapshot(request, false)
    }
    fn handle_with_snapshot(&mut self, request: Request, include_snapshot: bool) -> Response {
        self.poll_import();
        if matches!(self, Self::Importing(_)) {
            let error = if request.protocol_version != PROTOCOL_VERSION {
                Some(ApiError {
                    code: "protocolMismatch".into(),
                    message: "核心协议版本不匹配".into(),
                })
            } else if !matches!(request.command, Command::Snapshot) {
                Some(ApiError {
                    code: "teamImportRunning".into(),
                    message: "原团队材料正在导入；仅允许读取，不重复提交或派发".into(),
                })
            } else {
                None
            };
            return Response {
                id: request.id,
                result: (error.is_none() && include_snapshot).then(|| self.snapshot()),
                error,
            };
        }
        if request.protocol_version == PROTOCOL_VERSION {
            if let Command::TeamTiming {
                action: cerulean_domain::team_timing::TeamTimingAction::Import { path, sha256 },
            } = &request.command
            {
                if let Self::Ready(host) = self {
                    let job =
                        match crate::team_import_service::begin(host, &request.id, path, sha256) {
                            Ok(job) => job,
                            Err(message) => {
                                return Response {
                                    id: request.id,
                                    result: None,
                                    error: Some(ApiError {
                                        code: "teamImportRejected".into(),
                                        message,
                                    }),
                                }
                            }
                        };
                    let original = std::mem::replace(self, Self::Importing(Box::new(job)));
                    if let (Self::Ready(host), Self::Importing(job)) = (original, &mut *self) {
                        job.launch(host);
                    }
                    return Response {
                        id: request.id,
                        result: include_snapshot.then(|| self.snapshot()),
                        error: None,
                    };
                }
            }
        }
        if let Self::Ready(h) = self {
            return if include_snapshot {
                h.handle(request)
            } else {
                h.handle_command(request)
            };
        }
        let error = if request.protocol_version != PROTOCOL_VERSION {
            Some(ApiError {
                code: "protocolMismatch".into(),
                message: "核心协议版本不匹配".into(),
            })
        } else if let Command::ReviewDesktopStopRecord {
            relative_path,
            evidence_digest,
            reason,
        } = &request.command
        {
            crate::desktop_execution::cancellation::review::acknowledge(
                self.directory(),
                relative_path,
                evidence_digest,
                reason,
                |_| Ok(()),
            )
            .err()
            .map(|message| ApiError {
                code: "desktopStopReviewRejected".into(),
                message,
            })
        } else if let Command::ReviewApiPublication {
            relative_path,
            evidence_digest,
            reason,
        } = &request.command
        {
            crate::api_publication::acknowledge(
                self.directory(),
                relative_path,
                evidence_digest,
                reason,
                |_| Ok(()),
            )
            .err()
            .map(|message| ApiError {
                code: "apiPublicationReviewRejected".into(),
                message,
            })
        } else if let Command::ReviewManagementRecord {
            relative_path,
            evidence_digest,
            reason,
        } = &request.command
        {
            crate::management_review::acknowledge(
                self.directory(),
                relative_path,
                evidence_digest,
                reason,
                |_| Ok(()),
            )
            .err()
            .map(|message| ApiError {
                code: "managementReviewRejected".into(),
                message,
            })
        } else if let Command::ReviewStartupClosure {
            relative_path,
            evidence_digest,
            reason,
        } = &request.command
        {
            crate::startup_closure_review::acknowledge(
                self.directory(),
                relative_path,
                evidence_digest,
                reason,
                |_| Ok(()),
            )
            .err()
            .map(|message| ApiError {
                code: "startupClosureReviewRejected".into(),
                message,
            })
        } else if let Command::ReviewStartupIntent {
            relative_path,
            evidence_digest,
            reason,
        } = &request.command
        {
            crate::startup_review::acknowledge(
                self.directory(),
                relative_path,
                evidence_digest,
                reason,
                |_| Ok(()),
            )
            .err()
            .map(|message| ApiError {
                code: "startupReviewRejected".into(),
                message,
            })
        } else if !matches!(request.command, Command::Snapshot) {
            Some(ApiError {
                code: "diagnosticReadOnly".into(),
                message:
                    "执行数据尚未通过校验；诊断模式仅允许读取和专门的启动复核，禁止派发和模型调用"
                        .into(),
            })
        } else {
            None
        };
        Response {
            id: request.id,
            result: (error.is_none() && include_snapshot).then(|| self.snapshot()),
            error,
        }
    }
}
fn entry(root: &Path, path: &Path, limit: usize) -> DiagnosticFile {
    let mut item = DiagnosticFile {
        review_kind: None,
        review_digest: None,
        reviewed: false,
        relative_path: path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/"),
        state: "unreadable".into(),
        bytes: None,
        sha256: None,
    };
    if ordinary(path, false).is_err() {
        item.state = "not-ordinary-file".into();
        return item;
    }
    if fs::metadata(path).is_ok_and(|m| m.len() > limit as u64) {
        item.state = "oversized".into();
        return item;
    }
    let read = || -> Result<Vec<u8>, std::io::Error> {
        let mut bytes = vec![];
        fs::File::open(path)?
            .take((limit + 1) as u64)
            .read_to_end(&mut bytes)?;
        Ok(bytes)
    };
    if let Ok(bytes) = read() {
        if bytes.len() > limit {
            item.state = "oversized".into();
            return item;
        }
        item.bytes = Some(bytes.len() as u32);
        item.sha256 = Some(format!("{:x}", Sha256::digest(&bytes)));
        item.state = if bytes.is_empty() {
            "empty"
        } else {
            match serde_json::from_slice::<serde_json::Value>(&bytes) {
                Ok(_) => "complete-json-unverified",
                Err(e) if e.is_eof() => "truncated-json",
                Err(_) => "invalid-json",
            }
        }
        .into();
    }
    if let Ok((digest, reviewed)) = crate::startup_review::inspect(root, &item.relative_path) {
        item.review_kind = Some("intent".into());
        item.review_digest = Some(digest);
        item.reviewed = reviewed;
    } else if let Ok((digest, reviewed)) =
        crate::startup_closure_review::inspect(root, &item.relative_path)
    {
        item.review_kind = Some("closure".into());
        item.review_digest = Some(digest);
        item.reviewed = reviewed;
    } else if let Ok((digest, reviewed)) =
        crate::management_review::inspect(root, &item.relative_path)
    {
        item.review_kind = Some("management".into());
        item.review_digest = Some(digest);
        item.reviewed = reviewed;
    } else if let Ok((digest, reviewed)) =
        crate::desktop_execution::cancellation::review::inspect(root, &item.relative_path)
    {
        item.review_kind = Some("desktop-stop".into());
        item.review_digest = Some(digest);
        item.reviewed = reviewed;
    } else if let Ok((digest, reviewed)) =
        crate::api_publication::inspect(root, &item.relative_path)
    {
        item.review_kind = Some("api-publication".into());
        item.review_digest = Some(digest);
        item.reviewed = reviewed;
    }
    item
}
// Separate limits keep desktop faults visible even if other ledgers are full.
// Directory hashes group evidence; unvalidated JSON never supplies trusted IDs.
fn desktop_inventory(root: &Path) -> (Vec<DiagnosticFile>, bool) {
    let dir = root.join("desktop-task-dispatch-v1");
    if ordinary(&dir, true).is_err() {
        return (
            vec![],
            !matches!(fs::symlink_metadata(&dir),Err(e) if e.kind()==std::io::ErrorKind::NotFound),
        );
    }
    let Ok(attempts) = fs::read_dir(&dir) else {
        return (vec![], true);
    };
    let mut limited = false;
    let mut paths = Vec::new();
    let mut remaining = 8192;
    for (n, attempt) in attempts.take(513).enumerate() {
        if n == 512 || remaining == 0 {
            limited = true;
            break;
        }
        let Ok(attempt) = attempt else {
            limited = true;
            continue;
        };
        let path = attempt.path();
        if ordinary(&path, true).is_err() {
            limited = true;
            paths.push(path);
            continue;
        }
        let Ok(files) = fs::read_dir(&path) else {
            limited = true;
            continue;
        };
        let mut found = false;
        for file in files {
            if remaining == 0 {
                limited = true;
                break;
            }
            remaining -= 1;
            found = true;
            match file {
                Ok(f) => paths.push(f.path()),
                Err(_) => limited = true,
            }
        }
        if !found {
            limited = true;
            paths.push(path);
        }
    }
    // Scan names before reading bodies; pending reservations take precedence.
    paths.sort_by_key(|p| (!p.extension().is_some_and(|x| x == "pending"), p.clone()));
    limited |= paths.len() > 64;
    (
        paths
            .into_iter()
            .take(64)
            .map(|p| entry(root, &p, 131072))
            .collect(),
        limited,
    )
}
fn inventory(root: &Path) -> (Vec<DiagnosticFile>, bool) {
    let mut entries = vec![];
    let mut remaining = 4096usize;
    let mut limited = false;
    for name in ["codex-startups", "omp-startups"] {
        let folder = root.join(name);
        if ordinary(&folder, true).is_err() {
            if !matches!(fs::symlink_metadata(&folder), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
            {
                limited = true;
            }
            continue;
        }
        let Ok(projects) = fs::read_dir(&folder) else {
            limited = true;
            continue;
        };
        for project in projects {
            if remaining == 0 || entries.len() >= 64 {
                limited = true;
                break;
            }
            remaining -= 1;
            let Ok(project) = project else {
                limited = true;
                continue;
            };
            if ordinary(&project.path(), true).is_err() {
                limited = true;
                continue;
            }
            let Ok(files) = fs::read_dir(project.path()) else {
                limited = true;
                continue;
            };
            for file in files {
                if remaining == 0 || entries.len() >= 64 {
                    limited = true;
                    break;
                }
                remaining -= 1;
                let Ok(file) = file else {
                    limited = true;
                    continue;
                };
                if file.path().extension().is_some_and(|ext| ext == "pending") {
                    entries.push(entry(root, &file.path(), 65536));
                }
            }
        }
    }
    entries.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    (entries, limited)
}

fn api_inventory(root: &Path) -> (Vec<DiagnosticFile>, bool) {
    let mut result = vec![];
    let mut limited = false;
    for dir in [
        "api-startups",
        "api-cancellations",
        "api-workspace-bindings",
    ] {
        let folder = root.join(dir);
        if ordinary(&folder, true).is_err() {
            limited |= !matches!(fs::symlink_metadata(&folder),Err(e) if e.kind()==std::io::ErrorKind::NotFound);
            continue;
        }
        let Ok(files) = fs::read_dir(&folder) else {
            limited = true;
            continue;
        };
        let mut paths = vec![];
        for (count, file) in files.take(2001).enumerate() {
            if count == 2000 {
                limited = true;
                break;
            }
            match file {
                Ok(file) => paths.push(file.path()),
                Err(_) => limited = true,
            }
        }
        paths.sort_by_key(|p| (!p.extension().is_some_and(|e| e == "pending"), p.clone()));
        limited |= paths.len() > 64;
        result.extend(paths.into_iter().take(64).map(|p| entry(root, &p, 65536)));
    }
    (result, limited)
}

// Independent bounds prevent a large startup directory from hiding the ledger
// that failed earlier in RuntimeHost::open. Raw record bodies never enter IPC.
fn management_inventory(root: &Path) -> (Vec<DiagnosticFile>, bool) {
    let dir = root.join("management-decisions");
    if ordinary(&dir, true).is_err() {
        return (
            vec![],
            !matches!(fs::symlink_metadata(&dir), Err(e) if e.kind() == std::io::ErrorKind::NotFound),
        );
    }
    let Ok(iter) = fs::read_dir(&dir) else {
        return (vec![], true);
    };
    let mut limited = false;
    let mut files = Vec::new();
    for (index, file) in iter.take(4097).enumerate() {
        if index == 4096 {
            limited = true;
            break;
        }
        match file {
            Ok(file) => files.push(file),
            Err(_) => limited = true,
        }
    }
    // Pending first; valid JSON syntax alone is not a validity judgement.
    files.sort_by_key(|f| {
        (
            !f.path().extension().is_some_and(|x| x == "pending"),
            f.file_name(),
        )
    });
    limited |= files.len() > 64;
    let entries = files
        .into_iter()
        .take(64)
        .map(|f| entry(root, &f.path(), 256 * 1024))
        .collect();
    (entries, limited)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    #[test]
    fn desktop_diagnostics_keep_seven_record_kinds_private_and_readonly_across_reopens() {
        let root = tempfile::tempdir().unwrap();
        drop(RuntimeHost::open(root.path()).unwrap());
        let dir = root
            .path()
            .join("desktop-task-dispatch-v1")
            .join("a".repeat(64));
        fs::create_dir_all(&dir).unwrap();
        let raw = br#"{"private-desktop-marker":"never expose","unfinished":"#;
        for name in [
            "intent",
            "proof",
            "seal-intent",
            "completion",
            "stop-request",
            "stop-intent",
            "stop-proof",
        ] {
            fs::write(dir.join(format!("{name}.pending")), raw).unwrap();
        }
        for _ in 0..2 {
            let mut host = ServiceHost::open(root.path()).unwrap();
            assert!(host.is_diagnostic());
            let snapshot = host.snapshot();
            let d = snapshot.core_diagnostic.as_ref().unwrap();
            assert_eq!(d.desktop_files.len(), 7);
            assert!(!d.inventory_limited);
            assert!(!serde_json::to_string(&snapshot)
                .unwrap()
                .contains("private-desktop-marker"));
            for f in &d.desktop_files {
                assert_eq!(f.state, "truncated-json");
                assert_eq!(f.bytes, Some(raw.len() as u32));
                assert!(f.sha256.is_some());
                assert!(f.review_kind.is_none() && f.review_digest.is_none());
                assert_eq!(fs::read(root.path().join(&f.relative_path)).unwrap(), raw);
            }
            for command in [
                Command::CancelDesktopTask {
                    project_id: "project".into(),
                    attempt_id: "attempt".into(),
                    review_digest: "f".repeat(64),
                    reason: "blocked".into(),
                },
                Command::ReviewStartupIntent {
                    relative_path: d.desktop_files[0].relative_path.clone(),
                    evidence_digest: "f".repeat(64),
                    reason: "wrong route".into(),
                },
            ] {
                assert!(host
                    .handle(Request {
                        id: "blocked".into(),
                        protocol_version: PROTOCOL_VERSION,
                        command
                    })
                    .error
                    .is_some());
            }
            assert_eq!(fs::read_dir(&dir).unwrap().count(), 7);
        }
        let old:CoreDiagnostic=serde_json::from_value(serde_json::json!({"message":"old","detectedAt":"old","startupFiles":[],"inventoryLimited":false})).unwrap();
        assert!(old.desktop_files.is_empty());
    }
    #[test]
    fn desktop_inventory_has_independent_limits_pending_priority_and_no_recursive_following() {
        let root = tempfile::tempdir().unwrap();
        broken(root.path());
        let dir = root
            .path()
            .join("desktop-task-dispatch-v1")
            .join("b".repeat(64));
        fs::create_dir_all(&dir).unwrap();
        for n in 0..70 {
            fs::write(dir.join(format!("record-{n}.json")), b"{}").unwrap();
        }
        fs::write(dir.join("stop-request.pending"), vec![b' '; 100_000]).unwrap();
        fs::write(dir.join("stop-proof.pending"), vec![b' '; 131073]).unwrap();
        fs::create_dir(dir.join("stop-intent.pending")).unwrap();
        fs::write(dir.join("stop-intent.pending/private.json"), b"{}").unwrap();
        let d = ServiceHost::open(root.path())
            .unwrap()
            .snapshot()
            .core_diagnostic
            .unwrap();
        assert_eq!(d.desktop_files.len(), 64);
        assert_eq!(d.startup_files.len(), 1);
        assert!(d.inventory_limited);
        let find = |name: &str| {
            d.desktop_files
                .iter()
                .find(|f| f.relative_path.ends_with(name))
                .unwrap()
        };
        assert_eq!(find("stop-request.pending").state, "truncated-json");
        assert_eq!(find("stop-request.pending").bytes, Some(100_000));
        assert_eq!(find("stop-proof.pending").state, "oversized");
        assert_eq!(find("stop-intent.pending").state, "not-ordinary-file");
        assert!(!d
            .desktop_files
            .iter()
            .any(|f| f.relative_path.ends_with("private.json")));
    }
    #[test]
    fn desktop_inventory_reports_missing_empty_and_invalid_roots_without_mutation() {
        let root = tempfile::tempdir().unwrap();
        assert!(!desktop_inventory(root.path()).1);
        let dir = root.path().join("desktop-task-dispatch-v1");
        fs::write(&dir, b"original").unwrap();
        assert!(desktop_inventory(root.path()).1);
        assert_eq!(fs::read(&dir).unwrap(), b"original");
        fs::remove_file(&dir).unwrap();
        fs::create_dir(&dir).unwrap();
        fs::create_dir(dir.join("empty")).unwrap();
        let (files, limited) = desktop_inventory(root.path());
        assert!(limited);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].state, "not-ordinary-file");
        assert!(dir.join("empty").is_dir());
    }
    #[test]
    fn management_diagnostics_survive_reopen_without_body_exposure_or_mutation() {
        let root = tempfile::tempdir().unwrap();
        drop(RuntimeHost::open(root.path()).unwrap());
        let raw = br#"{"private-marker-do-not-expose":"secret","unfinished":"#;
        for sequence in 0..=4 {
            fs::write(
                root.path().join(format!(
                    "management-decisions/request-{sequence}.{sequence}.pending"
                )),
                raw,
            )
            .unwrap();
        }
        for _ in 0..2 {
            let mut service = ServiceHost::open(root.path()).unwrap();
            assert!(service.is_diagnostic());
            let snapshot = service.snapshot();
            let diagnostic = snapshot.core_diagnostic.as_ref().unwrap();
            assert_eq!(diagnostic.management_files.len(), 5);
            assert!(diagnostic.startup_files.is_empty());
            assert!(!diagnostic.inventory_limited);
            assert!(!serde_json::to_string(&snapshot)
                .unwrap()
                .contains("private-marker-do-not-expose"));
            for file in &diagnostic.management_files {
                assert_eq!(file.state, "truncated-json");
                assert_eq!(file.bytes, Some(raw.len() as u32));
                assert!(file.sha256.is_some());
                let can_review = file.relative_path.ends_with("request-0.0.pending");
                assert_eq!(file.review_digest.is_some(), can_review);
                assert_eq!(
                    file.review_kind.as_deref(),
                    can_review.then_some("management")
                );
                assert_eq!(
                    fs::read(root.path().join(&file.relative_path)).unwrap(),
                    raw
                );
            }
            let error = service
                .handle(Request {
                    id: "blocked".into(),
                    protocol_version: PROTOCOL_VERSION,
                    command: Command::CheckOpenAi,
                })
                .error
                .unwrap();
            assert_eq!(error.code, "diagnosticReadOnly");
            let error = service
                .handle(Request {
                    id: "wrong-review".into(),
                    protocol_version: PROTOCOL_VERSION,
                    command: Command::ReviewStartupIntent {
                        relative_path: "management-decisions/request-0.0.pending".into(),
                        evidence_digest: "0".repeat(64),
                        reason: "must not bypass management validation".into(),
                    },
                })
                .error
                .unwrap();
            assert_eq!(error.code, "startupReviewRejected");
            service.poll_verifications();
            assert_eq!(
                fs::read_dir(root.path().join("management-decisions"))
                    .unwrap()
                    .count(),
                5
            );
        }
        let old: cerulean_domain::CoreDiagnostic = serde_json::from_value(serde_json::json!({
            "message":"old","detectedAt":"2026-10-10T00:00:00Z","startupFiles":[],"inventoryLimited":false
        })).unwrap();
        assert!(old.management_files.is_empty());
    }

    #[test]
    fn management_inventory_has_independent_capacity_and_correct_record_size_limit() {
        let root = tempfile::tempdir().unwrap();
        broken(root.path());
        let dir = root.path().join("management-decisions");
        for n in 0..65 {
            fs::write(dir.join(format!("record-{n}.0.json")), b"{}").unwrap();
        }
        // A valid-sized management file can exceed the startup limit.
        let mut raw = br#"{"unfinished":"#.to_vec();
        raw.resize(100_000, b' ');
        fs::write(dir.join("pending.1.pending"), &raw).unwrap();
        fs::write(dir.join("too-big.0.pending"), vec![b' '; 256 * 1024 + 1]).unwrap();
        let service = ServiceHost::open(root.path()).unwrap();
        // The reader preflight rejects a directory masquerading as a record
        // before the base store can open. Add it after diagnostic startup to
        // exercise bounded observation of a later external filesystem change.
        fs::create_dir(dir.join("directory.2.pending")).unwrap();
        let diagnostic = service.snapshot().core_diagnostic.unwrap();
        assert_eq!(diagnostic.management_files.len(), 64);
        assert_eq!(diagnostic.startup_files.len(), 1);
        assert!(diagnostic.inventory_limited);
        let find = |name: &str| {
            diagnostic
                .management_files
                .iter()
                .find(|f| f.relative_path.ends_with(name))
                .unwrap()
        };
        assert_eq!(find("pending.1.pending").state, "truncated-json");
        assert_eq!(find("pending.1.pending").bytes, Some(100_000));
        assert_eq!(find("too-big.0.pending").state, "oversized");
        assert_eq!(find("directory.2.pending").state, "not-ordinary-file");
        assert!(diagnostic
            .management_files
            .iter()
            .skip(3)
            .all(|f| f.state == "complete-json-unverified"));
        drop(service);
        assert!(ServiceHost::open(root.path()).is_err());
        assert!(dir.join("directory.2.pending").is_dir());
        assert_eq!(fs::read(dir.join("pending.1.pending")).unwrap(), raw);
    }

    #[test]
    fn management_root_failure_is_reported_as_incomplete_without_replacing_it() {
        let root = tempfile::tempdir().unwrap();
        assert!(!management_inventory(root.path()).1);
        let path = root.path().join("management-decisions");
        fs::write(&path, b"original").unwrap();
        let (entries, limited) = management_inventory(root.path());
        assert!(entries.is_empty() && limited);
        assert_eq!(fs::read(path).unwrap(), b"original");
    }
    fn broken(root: &Path) -> PathBuf {
        let h = RuntimeHost::open(root).unwrap();
        drop(h);
        let path = root.join("omp-startups/project/attempt.0.pending");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"{\"schemaVersion\":").unwrap();
        path
    }
    #[test]
    fn failed_execution_load_serves_readonly_diagnostics_under_exclusive_lock() {
        let root = tempfile::tempdir().unwrap();
        let path = broken(root.path());
        let before = fs::read(&path).unwrap();
        let mut service = ServiceHost::open(root.path()).unwrap();
        assert!(service.is_diagnostic());
        assert!(Store::open(root.path()).is_err());
        assert!(ServiceHost::open(root.path()).is_err());
        let snapshot = service.snapshot();
        let diagnostic = snapshot.core_diagnostic.unwrap();
        assert_eq!(diagnostic.startup_files.len(), 1);
        assert_eq!(diagnostic.startup_files[0].state, "truncated-json");
        assert!(diagnostic.startup_files[0].sha256.is_some());
        for command in [
            Command::CreatePlan {
                title: "denied".into(),
                tasks: vec!["denied".into()],
            },
            Command::CheckOpenAi,
            Command::RecoverOmp {
                project_id: "project".into(),
                attempt_id: "attempt".into(),
            },
        ] {
            assert_eq!(
                service
                    .handle(Request {
                        id: "deny".into(),
                        protocol_version: PROTOCOL_VERSION,
                        command
                    })
                    .error
                    .unwrap()
                    .code,
                "diagnosticReadOnly"
            );
        }
        service.poll_verifications();
        assert!(service.can_shutdown());
        assert!(!service.can_restart_reviewed_stops());
        assert_eq!(fs::read(&path).unwrap(), before);
        let response = service.handle(Request {
            id: "stale".into(),
            protocol_version: 999,
            command: Command::Snapshot,
        });
        assert_eq!(response.error.unwrap().code, "protocolMismatch");
    }
    #[test]
    fn diagnostic_inventory_is_bounded_and_never_exposes_raw_contents() {
        let root = tempfile::tempdir().unwrap();
        broken(root.path());
        for n in 0..70 {
            fs::write(
                root.path()
                    .join(format!("omp-startups/project/item-{n}.pending")),
                b"do-not-expose-private-content",
            )
            .unwrap();
        }
        let service = ServiceHost::open(root.path()).unwrap();
        let snapshot = service.snapshot();
        let d = snapshot.core_diagnostic.as_ref().unwrap();
        assert!(d.inventory_limited);
        assert_eq!(d.startup_files.len(), 64);
        assert!(!serde_json::to_string(&snapshot)
            .unwrap()
            .contains("do-not-expose-private-content"));
    }
    #[test]
    fn damaged_base_store_never_falls_back_to_a_fresh_empty_scheduler() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("events")).unwrap();
        fs::write(root.path().join("events/00000001.json"), b"{").unwrap();
        assert!(ServiceHost::open(root.path()).is_err());
        assert_eq!(
            fs::read(root.path().join("events/00000001.json")).unwrap(),
            b"{"
        );
    }
    #[test]
    fn inventory_reports_non_directory_roots_as_incomplete() {
        let root = tempfile::tempdir().unwrap();
        let (entries, limited) = inventory(root.path());
        assert!(entries.is_empty());
        assert!(!limited);
        fs::write(root.path().join("omp-startups"), b"original").unwrap();
        let (entries, limited) = inventory(root.path());
        assert!(entries.is_empty());
        assert!(limited);
        assert_eq!(
            fs::read(root.path().join("omp-startups")).unwrap(),
            b"original"
        );
    }
}
