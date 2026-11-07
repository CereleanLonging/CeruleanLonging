//! Explicit recovery for checks proven to have failed before process launch.
//! One durable authorization permits one new launch of the original submission.
use super::*;

impl RuntimeHost {
    #[cfg(windows)]
    pub(super) fn verification_start_retry_available(
        &self,
        project: &str,
        operation: &str,
    ) -> Result<String, String> {
        identifier(project)?;
        identifier(operation)?;
        self.require_available(project)?;
        self.require_review_dispatch_idle()?;
        let job = self
            .jobs
            .get(&(project.into(), operation.into()))
            .ok_or("原检查不存在")?;
        require(
            job.state == VerificationState::Failed,
            "仅启动前失败的检查可重新启动",
        )?;
        require(
            !self.is_verification_recheck(project, operation),
            "修订检查请使用原复验入口",
        )?;
        require(
            self.management
                .statuses()
                .iter()
                .all(|s| s.project_id != project || s.state.resolved()),
            "请先核对原指挥请求",
        )?;
        let store = self.projects.get(project).ok_or("原项目不可读")?;
        let task = store.state().tasks.get(&job.task_id).ok_or("原任务缺失")?;
        require(
            !store.requires_reopen()
                && store.state().approved
                && store.state().revision_requested.is_none()
                && task.state == TaskState::Submitted
                && !task.verified
                && task.attempt.as_deref() == Some(job.attempt_id.as_str()),
            "只可检查当前批准方案的原提交",
        )?;
        let basis = crate::verification_detail::basis(store, job)?;
        require(
            store.revision().ok_or("原方案缺失")?.approval_digest()?
                == basis.manifest.revision_digest,
            "原提交与当前方案不符",
        )?;
        let directory = self
            .root
            .join("workflows")
            .join(project)
            .join("verification")
            .join(operation);
        if require_prelaunch_absence(&directory).is_err() {
            crate::verification_detail::require_logon_prelaunch_failure(&self.root, job, &basis)?;
        }
        let detail = crate::verification_detail::read(&self.root, job, &basis)?;
        require(!detail.ledger_recorded, "已有真实检查结果，请核对原结果")?;
        let authorization = retry_authorization_path(&self.root, project, operation);
        require_prelaunch_absence(&authorization)?;
        Ok(detail.evidence_digest)
    }

    pub(super) fn retry_verification_start(
        &mut self,
        project: &str,
        original: &str,
        evidence: &str,
        reason: &str,
    ) -> Result<(), String> {
        #[cfg(windows)]
        {
            use std::io::Write;
            identifier(project)?;
            identifier(original)?;
            sha256(evidence)?;
            require(
                !reason.trim().is_empty() && reason.len() <= 2000 && !reason.contains('\0'),
                "请填写已修复的检查启动问题",
            )?;
            let path = retry_authorization_path(&self.root, project, original);
            // An ACK lost after the durable intent must never cause another launch.
            if fs::symlink_metadata(&path).is_ok() {
                return Err("原检查已记录一次重新启动意图，请核对对应记录，禁止重复发送".into());
            }
            let observed = self.verification_start_retry_available(project, original)?;
            require(observed == evidence, "原检查证据已变化，请重新读取")?;
            for run in self
                .runs
                .all()
                .into_iter()
                .filter(|r| r.project_id == project && !r.state.terminal())
            {
                self.pause_project_run(&run.id)?;
            }
            require(
                self.verification_start_retry_available(project, original)? == evidence,
                "原检查证据已变化",
            )?;
            let job = self.jobs[&(project.into(), original.into())].clone();
            let operation = format!(
                "verify-retry-{}",
                &digest(&(project, original, evidence))?[..32]
            );
            require(
                !self.jobs.contains_key(&(project.into(), operation.clone())),
                "重新启动操作已有记录",
            )?;
            let plan = self.projects[project].revision().ok_or("批准方案缺失")?;
            let pins = plan
                .task(&job.task_id)?
                .checks
                .iter()
                .map(crate::verifier::PinnedCheck::from_approved)
                .collect::<Result<Vec<_>, _>>()?;
            let authority = serde_json::json!({
                "schemaVersion": 1, "principal": "user", "projectId": project,
                "originalOperationId": original, "operationId": operation,
                "evidenceDigest": evidence, "reason": reason,
                "taskId": job.task_id, "attemptId": job.attempt_id,
                "revisionDigest": plan.approval_digest()?,
                "ledgerHead": self.projects[project].history().last().map(|e| &e.hash),
                "at": chrono::Utc::now().to_rfc3339(),
            });
            let base = self.root.join("verification-start-retries");
            fs::create_dir_all(&base).map_err(|e| e.to_string())?;
            ordinary(&base, true)?;
            let folder = base.join(project);
            fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
            ordinary(&folder, true)?;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|e| format!("重新启动意图未写入，保留原记录：{e}"))?;
            file.write_all(&serde_json::to_vec(&authority).map_err(|e| e.to_string())?)
                .and_then(|_| file.sync_all())
                .map_err(|e| e.to_string())?;
            drop(file);
            self.start_verification(project, &operation, &job.task_id, &job.attempt_id, pins)
        }
        #[cfg(not(windows))]
        {
            let _ = (project, original, evidence, reason);
            Err("重新启动检查需要Windows运行时".into())
        }
    }
}

#[cfg(windows)]
fn retry_authorization_path(root: &Path, project: &str, operation: &str) -> PathBuf {
    root.join("verification-start-retries")
        .join(project)
        .join(format!("{operation}.json"))
}

#[cfg(any(windows, test))]
fn require_prelaunch_absence(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
        Ok(_) => Err("检查已有运行或重新启动记录，请保留现场并核对".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_runner_or_authority_always_prevents_preflight_retry() {
        let root = tempfile::tempdir().unwrap();
        let runner = root.path().join("original");
        assert!(require_prelaunch_absence(&runner).is_ok());
        fs::create_dir(&runner).unwrap();
        assert!(require_prelaunch_absence(&runner).is_err());
        let authority = root.path().join("authority.json");
        fs::write(&authority, b"incomplete").unwrap();
        assert!(require_prelaunch_absence(&authority).is_err());
    }
}
