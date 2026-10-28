//! OMP disposition reuses the user closure journal. It grants no send permit,
//! receipt, execution result, refund, or automatic scheduling permission.
use super::*;
use crate::omp_advice_journal::{self, Reference};
use std::io::Read;

fn delivery_digest(root: &Path, reference: &Reference) -> Result<String, String> {
    let path = reference.folder(root)?;
    let present = path.try_exists().map_err(|e| e.to_string())?;
    if present {
        ordinary(&path, true)?;
    }
    let mut names = vec![
        "reference.json".to_owned(),
        "reference.json.pending".to_owned(),
    ];
    for sequence in 1..=5 {
        names.push(format!("{sequence:02}.json"));
        names.push(format!("{sequence:02}.json.pending"));
    }
    let mut files = Vec::new();
    for name in names {
        let file = path.join(&name);
        let observed = if file.try_exists().map_err(|e| e.to_string())? {
            ordinary(&file, false)?;
            let mut bytes = Vec::new();
            fs::File::open(&file)
                .map_err(|e| e.to_string())?
                .take(1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            require(bytes.len() <= 1024 * 1024, "OMP建议处置证据过大")?;
            if name == "reference.json" {
                let saved: Reference =
                    serde_json::from_slice(&bytes).map_err(|_| "OMP原建议引用损坏")?;
                require(saved == *reference, "OMP原建议引用与处置不符")?;
            }
            Some(digest(&bytes)?)
        } else {
            None
        };
        files.push((name, observed));
    }
    // Names, missing files, partial publication bytes, and the absence of the
    // entire folder are evidence. Reading never creates or finishes a record.
    digest(&("omp-advice-disposition-v1", reference, present, files))
}

impl RuntimeHost {
    pub(in crate::host) fn omp_advice_closure_proof(
        &self,
        operation: &str,
        sequence: u32,
        hash: &str,
    ) -> Result<ClosureProof, String> {
        let intent = self.management.intent(operation)?;
        require(
            super::super::omp_supervision_requests::binding(intent)?.is_some(),
            "不是原OMP建议",
        )?;
        require(
            super::super::supervision_requests::request_policy(intent)?.delivery_enabled,
            "原OMP方案未批准建议发送",
        )?;
        require(
            !self.has_api_activity(&intent.project_id)
                && !self.has_omp_activity(&intent.project_id)
                && !self.has_codex_activity(&intent.project_id)
                && !self.active.contains_key(&intent.project_id),
            "OMP原执行仍在运行，不能结束跟踪",
        )?;
        // Reuse original approval/run/tool-prefix verification. No historical
        // observation is converted into a live waiter or steering authorization.
        self.validate_omp_supervision_requests()?;
        let (root, reference) = self.omp_advice_reference(intent)?;
        let store = self
            .projects
            .get(&intent.project_id)
            .ok_or("OMP项目账本不可用")?;
        require(!store.requires_reopen(), "OMP项目账本需要重开恢复")?;
        let end = sequence as usize;
        let history = store.history();
        require(
            end > 0 && end <= history.len() && history[end - 1].hash == hash,
            "OMP建议处置锚点与工作流不符",
        )?;
        require(
            history[..end].iter().any(|entry| {
                entry.hash == reference.workflow_hash
                    && entry.body.operation_id == format!("mgmt-{operation}")
            }),
            "OMP建议处置缺少原应用事件",
        )?;
        let job = self
            .omp_jobs
            .get(&(
                intent.project_id.clone(),
                reference.dispatch.attempt_id.clone(),
            ))
            .ok_or("OMP原执行记录缺失")?;
        require(
            job.task_id == reference.dispatch.task_id
                && job.revision_digest == reference.dispatch.revision_digest,
            "OMP建议与原执行范围不符",
        )?;
        let state = omp_advice_journal::inspect(&root, &reference)?;
        require(
            state.is_none_or(|s| !s.confirmed() && s.label() != "notSent"),
            "OMP建议已有原消息观察或发送前撤回证明，无需结束未知跟踪",
        )?;
        Ok(ClosureProof {
            operation: operation.into(),
            project: intent.project_id.clone(),
            attempt: reference.dispatch.attempt_id.clone(),
            management_digest: self.management.evidence_digest(operation)?.into(),
            delivery_digest: delivery_digest(&root, &reference)?,
            advice_event_hash: reference.workflow_hash,
            workflow_sequence: sequence,
            workflow_hash: hash.into(),
        })
    }

    pub(in crate::host) fn validate_omp_advice_closure(
        &self,
        operation: &str,
    ) -> Result<bool, String> {
        require(self.advice_closures.healthy(), "建议处置账本需要重开恢复")?;
        let Some(closure) = self.advice_closures.get(operation) else {
            return Ok(false);
        };
        require(
            self.omp_advice_closure_proof(
                operation,
                closure.proof.workflow_sequence,
                &closure.proof.workflow_hash,
            )? == closure.proof,
            "OMP建议处置与原始证据不符",
        )?;
        Ok(true)
    }
}
