//! Explicit local disposition of applied advice. No remote call or workflow mutation.
#[cfg(windows)]
use super::supervision_requests::binding;
use super::*;
use crate::advice_closure::{Closure, ClosureProof};
#[cfg(windows)]
#[path = "omp_advice_recovery.rs"]
mod omp;

impl RuntimeHost {
    #[cfg(windows)]
    fn advice_closure_proof(
        &self,
        operation: &str,
        sequence: u32,
        hash: &str,
    ) -> Result<ClosureProof, String> {
        use crate::management_execution::ApplicationTarget;
        let intent = self.management.intent(operation)?;
        if super::omp_supervision_requests::binding(intent)?.is_some() {
            return self.omp_advice_closure_proof(operation, sequence, hash);
        }
        let b = binding(intent)?.ok_or("缺少原监督请求")?;
        require(
            super::supervision_requests::request_policy(intent)?.delivery_enabled,
            "原方案未批准建议发送",
        )?;
        let (target, applied, receipt) = self
            .management
            .application(operation)
            .ok_or("建议没有应用事务")?;
        require(applied, "建议尚未完成应用对账")?;
        let ApplicationTarget::Workflow {
            principal,
            command,
            context,
        } = target
        else {
            return Err("不是工作流建议".into());
        };
        require(
            matches!(command, WorkflowCommand::Advise { task, .. } if *task == b.task_id),
            "不是原任务建议",
        )?;
        let store = self
            .projects
            .get(&intent.project_id)
            .ok_or("项目仍在执行或账本不可用")?;
        require(!store.requires_reopen(), "项目账本需要重开恢复")?;
        let history = store.history();
        let end = sequence as usize;
        require(
            end > 0 && end <= history.len() && history[end - 1].hash == hash,
            "建议处置锚点与工作流不符",
        )?;
        let entry = history[..end]
            .iter()
            .find(|e| e.body.operation_id == format!("mgmt-{operation}"))
            .ok_or("建议处置缺少原应用事件")?;
        require(
            entry.body.event.principal == *principal
                && entry.body.event.command == *command
                && entry.body.context == **context
                && receipt == Some(entry.hash.as_str()),
            "建议应用事件与回执不符",
        )?;
        let job = self
            .codex_jobs
            .get(&(intent.project_id.clone(), b.attempt_id.clone()))
            .ok_or("原执行记录缺失")?;
        require(job.task_id == b.task_id, "建议任务与执行不符")?;
        let delivery = self.delivery_for(operation)?;
        if let Some(record) = &delivery {
            self.validate_delivery_record(record)?;
            require(!record.acknowledged(), "建议已确认接收，无需结束未知跟踪")?;
        }
        // Bind the actual immutable files including timestamps; absence is evidence too.
        let folder =
            crate::supervised_worker::folder(&self.root, &intent.project_id, &b.attempt_id);
        let id = crate::advice_delivery::DeliveryIntent::id_for(operation)?;
        let mut files = Vec::new();
        for sequence in 0..=1 {
            let path = folder.join(format!("{id}.{sequence}.json"));
            if path.exists() {
                ordinary(&path, false)?;
                require(
                    fs::metadata(&path).map_err(|e| e.to_string())?.len() <= 16384,
                    "送达证据过大",
                )?;
                files.push(Some(digest(&fs::read(path).map_err(|e| e.to_string())?)?));
            } else {
                files.push(None);
            }
        }
        Ok(ClosureProof {
            operation: operation.into(),
            project: intent.project_id.clone(),
            attempt: b.attempt_id,
            management_digest: self.management.evidence_digest(operation)?.into(),
            delivery_digest: digest(&files)?,
            advice_event_hash: entry.hash.clone(),
            workflow_sequence: sequence,
            workflow_hash: hash.into(),
        })
    }
    pub(super) fn validate_advice_closures(&self) -> Result<(), String> {
        require(self.advice_closures.healthy(), "建议处置账本需要重开恢复")?;
        for closure in self.advice_closures.all() {
            // The sole worker temporarily owns the verified workflow; startup has no active workers.
            #[cfg(windows)]
            if self.has_codex_activity(&closure.proof.project)
                || self.has_api_activity(&closure.proof.project)
                || self.has_omp_activity(&closure.proof.project)
                || self.active.contains_key(&closure.proof.project)
            {
                continue;
            }
            #[cfg(windows)]
            require(
                self.advice_closure_proof(
                    &closure.proof.operation,
                    closure.proof.workflow_sequence,
                    &closure.proof.workflow_hash,
                )? == closure.proof,
                "建议处置与原始证据不符",
            )?;
            #[cfg(not(windows))]
            {
                let _ = closure;
                return Err("建议处置验证需要 Windows 执行宿主".into());
            }
        }
        Ok(())
    }
    #[cfg(windows)]
    pub(super) fn advice_tracking_review(
        &self,
        operation: &str,
    ) -> Result<project_setup::AdviceTrackingReview, String> {
        require(
            self.runner_error.is_none()
                && self.management.healthy()
                && self.runs.healthy()
                && self.startups.healthy()
                && self.advice_closures.healthy(),
            "核心账本需要恢复",
        )?;
        require(
            self.advice_closures.get(operation).is_none(),
            "建议跟踪已经结束",
        )?;
        let project = &self.management.intent(operation)?.project_id;
        let is_omp =
            super::omp_supervision_requests::binding(self.management.intent(operation)?)?.is_some();
        if is_omp {
            require(
                self.omp_startups.healthy() && self.api_fault.is_none(),
                "OMP启动或共享执行账本需要恢复",
            )?;
        }
        require(
            !self.has_codex_activity(project)
                && !self.has_api_activity(project)
                && !self.has_omp_activity(project)
                && !self.active.contains_key(project),
            "请等待当前执行和验收结束",
        )?;
        require(
            !self.counting_project(project)
                && !self.management_active.keys().any(|op| {
                    self.management
                        .intent(op)
                        .is_ok_and(|i| i.project_id == *project)
                }),
            "请等待指挥请求结束",
        )?;
        let runs: Vec<_> = self
            .runs
            .all()
            .into_iter()
            .filter(|r| r.project_id == *project)
            .collect();
        require(
            !runs
                .iter()
                .any(|r| r.state == project_setup::RunState::Active),
            "请先暂停项目调度",
        )?;
        let store = self.projects.get(project).ok_or("项目账本不可用")?;
        let last = store.history().last().ok_or("项目没有工作流记录")?;
        let proof = self.advice_closure_proof(operation, last.body.event.sequence, &last.hash)?;
        let jobs: Vec<_> = self
            .codex_jobs
            .values()
            .filter(|j| j.project_id == *project)
            .collect();
        let verifications: Vec<_> = self
            .jobs
            .values()
            .filter(|j| j.project_id == *project)
            .collect();
        let heads = self
            .management
            .statuses()
            .into_iter()
            .filter(|s| s.project_id == *project)
            .map(|s| {
                Ok((
                    s.operation_id.clone(),
                    self.management.evidence_digest(&s.operation_id)?.to_owned(),
                ))
            })
            .collect::<Result<Vec<_>, String>>()?;
        let evidence_digest = if is_omp {
            let omp_jobs: Vec<_> = self
                .omp_jobs
                .values()
                .filter(|j| j.project_id == *project)
                .collect();
            digest(&(
                "omp-advice-tracking-review-v1",
                self.runs.evidence_digest(),
                &proof,
                runs,
                jobs,
                omp_jobs,
                verifications,
                heads,
            ))?
        } else {
            // Keep the original Codex review digest and stored closure schema.
            digest(&(
                "advice-tracking-review-v1",
                self.runs.evidence_digest(),
                &proof,
                runs,
                jobs,
                verifications,
                heads,
            ))?
        };
        Ok(project_setup::AdviceTrackingReview {
            evidence_digest,
            workflow_sequence: proof.workflow_sequence,
        })
    }
    pub(super) fn close_advice_tracking(
        &mut self,
        operation: &str,
        expected: &str,
        reason: &str,
    ) -> Result<(), String> {
        #[cfg(not(windows))]
        {
            let _ = (operation, expected, reason);
            Err("建议处置需要 Windows 执行宿主".into())
        }
        #[cfg(windows)]
        {
            sha256(expected)?;
            let review = self.advice_tracking_review(operation)?;
            require(
                review.evidence_digest == expected,
                "建议或项目证据已变化，请刷新后重新审阅",
            )?;
            let project = &self.management.intent(operation)?.project_id;
            let hash = &self.projects[project]
                .history()
                .last()
                .ok_or("工作流缺失")?
                .hash;
            let proof = self.advice_closure_proof(operation, review.workflow_sequence, hash)?;
            self.advice_closures.save(Closure {
                version: 1,
                actor: "user".into(),
                at: chrono::Utc::now().to_rfc3339(),
                reason: reason.into(),
                reviewed_digest: expected.into(),
                proof,
            })
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use cerulean_domain::{management::Purpose, project_setup::RunState};
    use std::time::{Duration, Instant};
    fn copy_tree(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let kind = entry.file_type().unwrap();
            assert!(!kind.is_symlink());
            if kind.is_dir() {
                copy_tree(&entry.path(), &to.join(entry.file_name()));
            } else {
                fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
            }
        }
    }
    #[test]
    #[ignore = "requires an offline closure evidence directory; copies records, no models"]
    fn closure_rejects_rehashed_cross_ledger_proofs() {
        let source = PathBuf::from(std::env::var_os("CERULEAN_CLOSURE_SOURCE").unwrap())
            .canonicalize()
            .unwrap();
        let reports = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../reports")
            .canonicalize()
            .unwrap();
        assert!(source.starts_with(&reports) && source != reports);
        for field in [
            "managementDigest",
            "deliveryDigest",
            "adviceEventHash",
            "workflowHash",
            "workflowSequence",
            "attempt",
        ] {
            let temp = tempfile::tempdir().unwrap();
            copy_tree(&source.join("data"), temp.path());
            let file = fs::read_dir(temp.path().join("advice-closures"))
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path();
            let mut record: serde_json::Value =
                serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
            record["body"]["proof"][field] = if field == "workflowSequence" {
                serde_json::json!(1)
            } else if field == "attempt" {
                serde_json::json!("foreign-attempt")
            } else {
                serde_json::json!("0".repeat(64))
            };
            let body: Closure = serde_json::from_value(record["body"].clone()).unwrap();
            record["hash"] = serde_json::json!(digest(&body).unwrap());
            fs::write(file, serde_json::to_vec(&record).unwrap()).unwrap();
            assert!(
                RuntimeHost::open(temp.path()).is_err(),
                "forged closure accepted: {field}"
            );
        }
    }
    #[test]
    #[ignore = "requires a fresh owned offline delivery seed; never invokes real providers"]
    fn user_closure_preserves_unknown_and_requires_explicit_resume() {
        let source = PathBuf::from(std::env::var_os("CERULEAN_CLOSURE_SOURCE").unwrap())
            .canonicalize()
            .unwrap();
        let reports = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../reports")
            .canonicalize()
            .unwrap();
        assert!(source.starts_with(&reports) && source != reports);
        let original: serde_json::Value =
            serde_json::from_slice(&fs::read(source.join("delivery-result.json")).unwrap())
                .unwrap();
        let mode = original["mode"].as_str().unwrap();
        assert!(["ACK", "PAUSE_COMMIT", "LOST_ACK"].contains(&mode));
        let data = source.join("data");
        let mut host = RuntimeHost::open(&data).unwrap();
        super::super::project_dag_tests::enable_offline_management(&mut host);
        let operation = host
            .management
            .statuses()
            .into_iter()
            .find(|s| s.purpose == Purpose::Advise)
            .unwrap()
            .operation_id;
        if mode == "ACK" {
            assert!(host.advice_tracking_review(&operation).is_err());
            assert!(host
                .close_advice_tracking(&operation, &"0".repeat(64), "already received")
                .is_err());
            return;
        }
        let before = host.snapshot();
        let management = host
            .management
            .evidence_digest(&operation)
            .unwrap()
            .to_owned();
        let review = host.advice_tracking_review(&operation).unwrap();
        assert!(host
            .close_advice_tracking(&operation, &"0".repeat(64), "stale review")
            .is_err());
        assert!(host
            .close_advice_tracking(&operation, &review.evidence_digest, " ")
            .is_err());
        let mut run = host.runs.all()[0].clone();
        let saved = run.clone();
        run.state = RunState::Active;
        host.runs.save(run.clone(), "user").unwrap();
        assert!(host.advice_tracking_review(&operation).is_err());
        host.pause_project_run(&run.id).unwrap();
        let fresh = host.advice_tracking_review(&operation).unwrap();
        assert_ne!(fresh.evidence_digest, review.evidence_digest);
        assert!(host
            .close_advice_tracking(&operation, &review.evidence_digest, "old review")
            .is_err());
        let response = host.handle(Request {
            id: "close-advice".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::CloseAdviceTracking {
                operation_id: operation.clone(),
                evidence_digest: fresh.evidence_digest,
                reason: "已检查原建议，结束跟踪；执行结果另行核对".into(),
            },
        });
        assert!(response.error.is_none(), "{:?}", response.error);
        assert!(host.advice_tracking_review(&operation).is_err());
        assert_eq!(
            host.management.evidence_digest(&operation).unwrap(),
            management
        );
        assert_eq!(
            digest(&host.snapshot().workflows).unwrap(),
            digest(&before.workflows).unwrap()
        );
        assert_eq!(
            digest(&host.snapshot().codex_jobs).unwrap(),
            digest(&before.codex_jobs).unwrap()
        );
        assert!(!host.advice_delivered(&operation));
        assert_eq!(host.advice_delivery_state(&operation), "unknown");
        assert!(host.require_resolved_advice_deliveries("project").is_ok());
        assert_eq!(host.runs.all()[0].state, RunState::Paused);
        let seq = host.projects["project"].history().len();
        let requests = host.management.statuses().len();
        drop(host);
        host = RuntimeHost::open(&data).unwrap();
        super::super::project_dag_tests::enable_offline_management(&mut host);
        for _ in 0..5 {
            host.poll_verifications();
        }
        assert_eq!(host.projects["project"].history().len(), seq);
        assert_eq!(host.management.statuses().len(), requests);
        assert!(host
            .management_detail(&operation)
            .unwrap()
            .advice_tracking_closure
            .is_some());
        assert!(!host.advice_delivered(&operation));
        if mode == "PAUSE_COMMIT" {
            fs::write(source.join("bin/await-steer"), b"1").unwrap();
        }
        host.start_project_run("project", &saved.revision_digest, &saved.setup_digest)
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(50);
        loop {
            host.poll_verifications();
            let state = host.runs.all()[0].state.clone();
            if state == RunState::Completed || state == RunState::Attention {
                break;
            }
            assert!(Instant::now() < deadline, "resumed fixture did not finish");
            std::thread::sleep(Duration::from_millis(20));
        }
        let completed = mode == "PAUSE_COMMIT";
        assert_eq!(
            host.runs.all()[0].state,
            if completed {
                RunState::Completed
            } else {
                RunState::Attention
            }
        );
        if completed {
            assert!(host.projects["project"]
                .state()
                .tasks
                .values()
                .all(|t| t.state == TaskState::Accepted));
            assert_eq!(host.jobs.len(), 2);
        } else {
            assert_eq!(host.projects["project"].history().len(), seq);
            assert_eq!(host.jobs.len(), 0);
            assert!(host
                .codex_jobs
                .values()
                .any(|j| j.state == CodexJobState::Unknown));
        }
        let detail = host.management_detail(&operation).unwrap();
        let final_snapshot = host.snapshot();
        drop(host);
        let reopened = RuntimeHost::open(&data).unwrap();
        assert!(!reopened.advice_delivered(&operation));
        assert_eq!(
            digest(&reopened.snapshot().workflows).unwrap(),
            digest(&final_snapshot.workflows).unwrap()
        );
        fs::write(source.join("closure-result.json"), serde_json::to_vec_pretty(&serde_json::json!({"status":"passed", "mode":mode,"realModelCalls":0,"explicitResumeCompleted":completed,"detail":detail,"snapshot":final_snapshot})).unwrap()).unwrap();
    }
}
