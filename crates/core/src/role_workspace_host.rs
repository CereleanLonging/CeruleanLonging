//! Bounded chief/deputy reads and a chief acceptance packet made from sealed bytes.
use super::*;
use cerulean_domain::management::Purpose;
use serde_json::{json, Value};

impl RuntimeHost {
    pub(super) fn prepare_chief_directory(
        &self,
        draft: &project_setup::ProjectSetupDraft,
    ) -> Result<Option<String>, String> {
        let Some(path) = &draft.chief_directory else {
            return Ok(None);
        };
        let project = &self
            .projects
            .get(&draft.project_id)
            .ok_or("项目不存在")?
            .configuration()
            .execution_scope
            .as_ref()
            .ok_or("项目范围缺失")?
            .project;
        let canonical = crate::role_workspace::prepare(
            path,
            &project.root,
            draft.worktree.as_ref().map(|w| w.pool_root.as_str()),
            &self.root,
        )?;
        for other in self
            .projects
            .values()
            .filter_map(|s| s.configuration().execution_scope.as_ref())
        {
            require(
                !crate::artifacts::contains_path(
                    Path::new(&other.project.root),
                    Path::new(&canonical),
                ) && !crate::artifacts::contains_path(
                    Path::new(&canonical),
                    Path::new(&other.project.root),
                ),
                "主指挥目录不能与已登记项目目录重叠",
            )?;
        }
        for (id, other) in &self.project_setups {
            if id == &draft.project_id {
                continue;
            }
            if let Some(other) = &other.setup.chief_directory {
                require(
                    !crate::artifacts::contains_path(Path::new(other), Path::new(&canonical))
                        && !crate::artifacts::contains_path(
                            Path::new(&canonical),
                            Path::new(other),
                        ),
                    "主指挥目录不能与其他项目的主指挥目录重叠",
                )?;
            }
        }
        Ok(Some(canonical))
    }

    pub(super) fn role_context(
        &self,
        mut context: Value,
        purpose: Purpose,
        publish: bool,
    ) -> Result<Value, String> {
        let snapshot: ExecutionSnapshot = serde_json::from_value(context["workflow"].clone())
            .map_err(|_| "角色目录工作流无效")?;
        let Some(saved) = self
            .project_setups
            .get(&snapshot.project.id)
            .filter(|s| s.setup.chief_directory.is_some())
        else {
            return Ok(context);
        };
        let chief = saved.setup.chief_directory.as_deref().unwrap();
        crate::worker_directories::verify_saved(
            &self.root,
            &snapshot.project,
            &saved.setup.worker_directories,
        )?;
        let mut role = crate::role_workspace::read_requirements(saved, &snapshot.project.root)?;
        if let Some(plan) = &snapshot.revision {
            let key = plan.approval_digest()?;
            let material = crate::role_workspace::read(chief, "plans", &key)?;
            require(
                material == serde_json::to_value(plan).map_err(|e| e.to_string())?,
                "主指挥方案资料与当前方案不同",
            )?;
            role["planDigest"] = json!(key);
            role["plan"] = material;
        }
        context["roleWorkspace"] = role;
        if purpose == Purpose::Accept {
            #[cfg(windows)]
            {
                let packet = self.chief_review_packet(&snapshot)?;
                let key = digest(&packet)?;
                if publish {
                    crate::role_workspace::publish(chief, "reviews", &key, &packet)?;
                } else {
                    require(
                        crate::role_workspace::read(chief, "reviews", &key)? == packet,
                        "原主指挥审阅材料已变化",
                    )?;
                }
                context["chiefReview"] = json!({"digest":key,"packet":packet});
            }
            #[cfg(not(windows))]
            return Err("主指挥实字节审阅需要Windows".into());
        }
        let bytes = serde_json::to_vec(&context)
            .map_err(|e| e.to_string())?
            .len();
        require(
            bytes <= purpose.context_limit_bytes(),
            &format!(
                "主指挥材料共 {bytes} 字节，超过本次 {} KiB 容量，请拆分需求后保存配置",
                purpose.context_limit_bytes() / 1024
            ),
        )?;
        Ok(context)
    }

    /// Old pending requests retain their original context. New requests must
    /// re-observe their current materials instead of copying the saved packet.
    pub(super) fn rebuild_role_context(
        &self,
        original: &Value,
        context: Value,
        purpose: Purpose,
    ) -> Result<Value, String> {
        if original.get("roleWorkspace").is_some() {
            self.role_context(context, purpose, false)
        } else {
            Ok(context)
        }
    }

    #[cfg(windows)]
    fn chief_review_packet(&self, snapshot: &ExecutionSnapshot) -> Result<Value, String> {
        let mut submissions = vec![];
        let mut text_bytes = 0usize;
        for (task, state) in &snapshot.state.tasks {
            if state.state != TaskState::Submitted || !state.verified {
                continue;
            }
            let attempt = state.attempt.as_deref().ok_or("验收尝试缺失")?;
            let job = self
                .jobs
                .values()
                .find(|j| {
                    j.project_id == snapshot.project.id
                        && j.task_id == *task
                        && j.attempt_id == attempt
                        && j.state == VerificationState::Succeeded
                })
                .ok_or("主指挥缺少已成功的原验收作业")?;
            let basis = if let Some(store) = self.projects.get(&snapshot.project.id) {
                crate::verification_detail::basis(store, job)?
            } else {
                self.api_writers
                    .get(&snapshot.project.id)
                    .ok_or("项目写入者缺失")?
                    .owner
                    .workspace_verification_basis(job)?
                    .detail_basis
            };
            let (detail, report) = crate::verification_detail::inspect(&self.root, job, &basis)?;
            require(detail.ledger_recorded, "独立检查尚未入账，不能提交主指挥")?;
            let report = report.ok_or("主指挥验收报告缺失")?;
            let seal = report
                .source_snapshot
                .as_ref()
                .ok_or("主指挥审阅需要已封存来源")?;
            let evidence = self
                .root
                .join("workflows")
                .join(&snapshot.project.id)
                .join("verification");
            let mut files = 10000usize;
            let mut bytes = 512 * 1024 * 1024u64;
            let mut frozen = crate::source_snapshot::open_frozen(
                &basis.plan,
                &basis.manifest,
                seal,
                &evidence,
                &mut files,
                &mut bytes,
            )?;
            let mut artifacts = vec![];
            for artifact in &basis.manifest.artifacts {
                text_bytes = text_bytes
                    .checked_add(artifact.bytes as usize)
                    .ok_or("审阅容量溢出")?;
                require(
                    text_bytes <= 24 * 1024,
                    "提交正文超过主指挥审阅容量，请拆分任务；不会仅凭摘要收口",
                )?;
                let text = String::from_utf8(frozen.read_file(&artifact.path)?)
                    .map_err(|_| "非UTF-8产物需要专门的审阅接入；不会只看摘要自动收口")?;
                artifacts.push(json!({"path":artifact.path,"sha256":artifact.sha256,"bytes":artifact.bytes,"content":text}));
            }
            let stages=detail.stages.iter().map(|s|json!({"id":s.id,"name":s.name,"state":s.state,"exitCode":s.exit_code,"timedOut":s.timed_out,"stdout":s.stdout,"stderr":s.stderr})).collect::<Vec<_>>();
            submissions.push(json!({"taskId":task,"attemptId":attempt,"artifacts":artifacts,"verification":report,"stages":stages,"evidenceDigest":detail.evidence_digest}));
        }
        require(!submissions.is_empty(), "没有可供主指挥审阅的已检查提交")?;
        Ok(
            json!({"schemaVersion":1,"revisionDigest":snapshot.revision_digest,"submissions":submissions,
            "reviewInstruction":"Review the full submitted artifact contents against the original requirement and every task goal. Verification does not replace semantic review. Missing coverage, wrong behavior or insufficient evidence requires Wait, never Accept based only on verified=true."}),
        )
    }

    pub(super) fn validate_chief_acceptance_material(
        &self,
        operation: &str,
        task: &str,
        attempt: &str,
    ) -> Result<(), String> {
        let intent = self.management.intent(operation)?;
        if intent.context.get("roleWorkspace").is_none() {
            return Ok(());
        }
        let original: ExecutionSnapshot =
            serde_json::from_value(intent.context["workflow"].clone())
                .map_err(|_| "主指挥原快照无效")?;
        let current = self.role_context(
            self.context_for_snapshot(
                intent.context["requirement"].as_str().ok_or("需求缺失")?,
                &original,
            ),
            Purpose::Accept,
            false,
        )?;
        require(
            current["roleWorkspace"] == intent.context["roleWorkspace"]
                && current["chiefReview"] == intent.context["chiefReview"],
            "主指挥审阅来源已变化",
        )?;
        require(
            current["chiefReview"]["packet"]["submissions"]
                .as_array()
                .is_some_and(|items| {
                    items
                        .iter()
                        .any(|s| s["taskId"] == task && s["attemptId"] == attempt)
                }),
            "主指挥不能接受未提供完整正文和检查证据的提交",
        )
    }
}
