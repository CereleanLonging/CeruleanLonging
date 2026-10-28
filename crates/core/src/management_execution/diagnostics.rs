//! Optional observations, outside the authoritative journal. Missing or invalid
//! diagnostics do not alter decisions, recovery, billing, or resend permissions.
//! Old readers ignore this directory and retain the exact original JSON schema.
use super::*;
use cerulean_adapters::deputy_management::{DecodeStage, DeputyOutcome, DeputyResult};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DecodeRecord {
    version: u32,
    operation_id: String,
    request_digest: String,
    result_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    response_sha256: Option<String>,
    stage: DecodeStage,
}
const DIRECTORY: &str = "management-response-diagnostics";
const LIMIT: u64 = 4096;

impl DecisionJournal {
    pub(super) fn save_decode_diagnostic(&self, id: &str, r: &DeputyResult) -> Result<(), String> {
        let Some(stage) = r.decode_stage else {
            return Ok(());
        };
        identifier(id)?;
        stage.validate_result(r)?;
        require(
            id == r.request.handoff.operation_id,
            "diagnostic operation mismatch",
        )?;
        let response_sha256 = match &r.outcome {
            DeputyOutcome::Received {
                response_sha256, ..
            } => Some(response_sha256.clone()),
            DeputyOutcome::Failed { .. } => None,
        };
        let record = DecodeRecord {
            version: if response_sha256.is_some() { 1 } else { 2 },
            operation_id: id.into(),
            request_digest: digest(&r.request)?,
            result_digest: digest(r)?,
            response_sha256,
            stage,
        };
        let root = self.root.parent().ok_or("management root missing")?;
        ordinary(root, true)?;
        let directory = root.join(DIRECTORY);
        match fs::create_dir(&directory) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
            Err(_) => return Err("diagnostic directory unavailable".into()),
        }
        ordinary(&directory, true)?;
        #[cfg(windows)]
        let _pins = [root, directory.as_path()]
            .iter()
            .map(|p| crate::backup::open(p, true, false))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "diagnostic directory unavailable")?;
        let bytes = serde_json::to_vec(&record).map_err(|_| "diagnostic serialization failed")?;
        require(bytes.len() <= LIMIT as usize, "diagnostic too large")?;
        let pending = directory.join(format!("{id}.pending"));
        let target = directory.join(format!("{id}.json"));
        absent(&target)?;
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&pending)
            .map_err(|_| "diagnostic publication unavailable")?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| "diagnostic publication incomplete")?;
        drop(file);
        fs::hard_link(&pending, &target).map_err(|_| "diagnostic publication incomplete")?;
        fs::remove_file(pending).map_err(|_| "diagnostic pending retained")?;
        Ok(())
    }

    pub(super) fn read_decode_diagnostic(&self, id: &str) -> Result<Option<DecodeStage>, String> {
        identifier(id)?;
        let op = self
            .operations
            .get(id)
            .ok_or("original diagnostic operation missing")?;
        let r = op
            .deputy_response
            .as_ref()
            .ok_or("original provider response missing")?;
        let directory = self
            .root
            .parent()
            .ok_or("management root missing")?
            .join(DIRECTORY);
        if let Err(e) = fs::symlink_metadata(&directory) {
            return if e.kind() == std::io::ErrorKind::NotFound {
                Ok(None)
            } else {
                Err("diagnostic directory unreadable".into())
            };
        }
        ordinary(&directory, true)?;
        let path = directory.join(format!("{id}.json"));
        if let Err(e) = fs::symlink_metadata(&path) {
            return if e.kind() == std::io::ErrorKind::NotFound {
                Ok(None)
            } else {
                Err("diagnostic unreadable".into())
            };
        }
        ordinary(&path, false)?;
        #[cfg(windows)]
        let _pin = crate::backup::open(&directory, true, false)
            .map_err(|_| "diagnostic directory unavailable")?;
        #[cfg(windows)]
        let file = crate::backup::open(&path, false, false).map_err(|_| "diagnostic unreadable")?;
        #[cfg(not(windows))]
        let file = fs::File::open(path).map_err(|_| "diagnostic unreadable")?;
        let mut bytes = Vec::new();
        file.take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "diagnostic unreadable")?;
        require(bytes.len() <= LIMIT as usize, "diagnostic too large")?;
        let record: DecodeRecord =
            serde_json::from_slice(&bytes).map_err(|_| "invalid diagnostic")?;
        let response_sha256 = match &r.outcome {
            DeputyOutcome::Received {
                response_sha256, ..
            } => Some(response_sha256.clone()),
            DeputyOutcome::Failed { .. } => None,
        };
        require(
            record.version == if response_sha256.is_some() { 1 } else { 2 }
                && record.operation_id == id
                && record.request_digest == digest(&r.request)?
                && record.result_digest == digest(r)?
                && record.response_sha256 == response_sha256,
            "diagnostic differs from original response",
        )?;
        record.stage.validate_result(r)?;
        Ok(Some(record.stage))
    }

    pub(crate) fn decode_diagnostic_message(&self, id: &str) -> Option<String> {
        let response = self.operations.get(id)?.deputy_response.as_ref()?;
        let transport = matches!(
            response.outcome,
            DeputyOutcome::Failed {
                failure: cerulean_adapters::api_generation::ApiFailure::Transport {}
            }
        );
        if !transport
            && !matches!(
                response.outcome,
                DeputyOutcome::Received { envelope: None, .. }
            )
        {
            return None;
        }
        Some(match self.read_decode_diagnostic(id) {
            Ok(Some(stage)) if transport => {
                format!("管理请求传输阶段：{stage:?}。远端执行与计费仍未知，未自动重发。")
            }
            Ok(None) if transport => {
                "管理请求发生传输失败，但未记录具体阶段；远端执行与计费仍未知，未自动重发。".into()
            }
            Ok(Some(stage)) => {
                format!("管理响应拒绝阶段：{stage:?}。原用量与响应摘要保留，未自动重发。")
            }
            Ok(None) => {
                "管理响应缺少可用决定，未记录拒绝阶段，不能追溯具体原因；原用量与摘要保留。".into()
            }
            Err(_) => {
                "管理响应诊断不可用或与原记录不符；原决定、用量及恢复状态保持，不根据诊断重新调用。"
                    .into()
            }
        })
    }
}
