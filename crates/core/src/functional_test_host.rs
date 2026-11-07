//! Whole-delivery tests have their own one-shot authority after unanimous review.
use super::*;
use crate::blind_review::FunctionalOutcome as Completed;
use cerulean_domain::blind_review::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};

pub(super) struct ActiveFunctionalTest {
    pub id: String,
    pub cancel: Arc<AtomicBool>,
    pub progress: Arc<std::sync::Mutex<Option<ReviewSessionProgress>>>,
    pending: crate::blind_review::Pending,
    receiver: mpsc::Receiver<Result<Completed, String>>,
}
impl Drop for ActiveFunctionalTest {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}
enum PreparedFunctional {
    Task(crate::scheduler::PreparedVerification),
    Delivery(crate::delivery_functional::Prepared),
}
impl PreparedFunctional {
    fn checks(&self) -> Result<&[AcceptanceCheck], String> {
        match self {
            Self::Task(p) => p.checks(),
            Self::Delivery(p) => Ok(p.checks()),
        }
    }
    fn run(
        &self,
        pins: &[crate::verifier::PinnedCheck],
        cancel: &AtomicBool,
        source: &str,
    ) -> Result<VerificationReport, String> {
        match self {
            Self::Task(p) => p.functional_run(pins, cancel, source),
            Self::Delivery(p) => p.run(pins, cancel),
        }
    }
    fn require_pass(&self, report: &VerificationReport) -> Result<(), String> {
        match self {
            Self::Task(p) => p.require_functional_pass(report),
            Self::Delivery(p) => p.require_pass(report),
        }
    }
}
impl RuntimeHost {
    pub(super) fn start_functional_test(
        &mut self,
        project: &str,
        id: &str,
        key: &str,
    ) -> Result<(), String> {
        require(
            self.blind_active.is_none() && self.functional_active.is_none(),
            "原评审或功能测试尚未结束",
        )?;
        self.require_review_environment()?;
        let round = self.blind_reviews.checked(project, id)?;
        let binding = self.current_blind_binding(id)?;
        require(
            round.phase == ReviewPhase::AwaitingFunctionalTest
                && round.binding == binding
                && round.digest == key,
            "尚未通过当前交付盲评，功能测试权限未开放",
        )?;
        let gathered = self.gather_blind_review(project)?;
        let store = self.projects.get(project).ok_or("项目不可用")?;
        let plan = store.revision().ok_or("批准方案缺失")?;
        // Use a deterministic unique operation, and persist authority before a request or process.
        let operation = format!("functional-{id}");
        let prepared = if let Some(delivery) = &gathered.check_archive.assembled {
            PreparedFunctional::Delivery(crate::delivery_functional::Prepared::new(
                &self.root,
                delivery.clone(),
                &operation,
                plan.policy.max_seconds,
            )?)
        } else {
            let attempt = store
                .state()
                .tasks
                .get(&gathered.final_task)
                .and_then(|t| t.attempt.as_deref())
                .ok_or("交付尝试缺失")?;
            PreparedFunctional::Task(store.prepare_functional_verification(
                &round,
                &operation,
                &gathered.final_task,
                attempt,
                gathered.functional_checks.clone(),
            )?)
        };
        let checks = prepared.checks()?;
        let pins = checks
            .iter()
            .map(crate::verifier::PinnedCheck::from_approved)
            .collect::<Result<Vec<_>, _>>()?;
        let descriptions = serde_json::json!(checks.iter().enumerate().map(|(index, c)|serde_json::json!({
            "check":index+1,"approvedDigest":digest(c).ok(),"expectedAssertions":c.expected_assertions.len(),"timeoutSeconds":c.timeout_seconds
        })).collect::<Vec<_>>());
        let seconds = plan.policy.max_seconds;
        let chief = round
            .seats
            .iter()
            .find(|s| s.role == "chief")
            .ok_or("本轮主指挥缺失")?;
        let (local, credential) = if chief.transport.is_some() {
            cerulean_adapters::blind_review::functional_packets(
                chief,
                "functional-preflight",
                &round.material,
                &descriptions,
                None,
            )?;
            (Some(self.prepare_local_review(project, chief)?), None)
        } else {
            cerulean_adapters::blind_review::functional_body(
                chief,
                "functional-preflight",
                &round.material,
                &descriptions,
                None,
            )?;
            (
                None,
                Some(
                    crate::credential_vault::load_for(&self.root, chief.api_target()?.provider())
                        .map_err(|_| "主指挥凭证不可用，功能测试未启动")?,
                ),
            )
        };
        let (pending, chief) = self.blind_reviews.start_functional(
            id,
            key,
            &binding,
            chrono::Utc::now().timestamp(),
            seconds,
        )?;
        // The verifier's operation is the durable round ID, shared with result lookup.
        let cancel = Arc::new(AtomicBool::new(false));
        let token = cancel.clone();
        let request = pending.request.clone();
        let progress = Arc::new(std::sync::Mutex::new(None));
        let control = super::local_review::Control {
            cancel: token.clone(),
            deadline: self.blind_reviews.rounds[id]
                .functional
                .as_ref()
                .and_then(|f| f.deadline)
                .ok_or("功能测试期限缺失")?,
            progress: progress.clone(),
            audit: std::sync::Arc::new(std::sync::Mutex::new(None)),
        };
        let (sender, receiver) = mpsc::channel();
        let launched = std::thread::Builder::new()
            .name(format!("functional-{id}"))
            .spawn(move || {
                let result = (|| {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|_| "功能测试运行时不可用")?;
                    let call_chief = |request_id: &str, results: Option<&serde_json::Value>| {
                        rt.block_on(async {
                            if let Some(local) = &local {
                                local
                                    .functional(
                                        &chief,
                                        request_id,
                                        &round.material,
                                        &descriptions,
                                        results,
                                        &control,
                                    )
                                    .await
                            } else {
                                cerulean_adapters::blind_review::run_functional(
                                    credential.as_ref().ok_or("主指挥凭证缺失")?,
                                    &chief,
                                    request_id,
                                    &round.material,
                                    &descriptions,
                                    results,
                                )
                                .await
                            }
                        })
                    };
                    control.check()?;
                    let authorization = call_chief(&format!("{request}-authorize"), None)?;
                    control.check()?;
                    if authorization.decision.vote == Vote::Fail {
                        return Ok(Completed {
                            model: authorization.model,
                            authorization: authorization.decision.clone(),
                            report: None,
                            decision: authorization.decision,
                        });
                    }
                    let report = prepared.run(&pins, &token, &gathered.source_digest)?;
                    control.check()?;
                    if prepared.require_pass(&report).is_err() {
                        return Ok(Completed {
                            model: authorization.model,
                            authorization: authorization.decision,
                            report: Some(report),
                            decision: FunctionalDecision {
                                vote: Vote::Fail,
                                reason: "整体功能检查未全部通过，请核对原检查报告并修订方案".into(),
                            },
                        });
                    }
                    let results = serde_json::json!({
                        "reportDigest":digest(&report)?,
                        "checks":report.checks.iter().enumerate().map(|(index,c)|serde_json::json!({
                            "check":index+1,"passed":true,"assertions":c.assertions.len(),
                            "stdoutDigest":c.stdout_sha256,"stderrDigest":c.stderr_sha256
                        })).collect::<Vec<_>>()
                    });
                    let decision = call_chief(&format!("{request}-result"), Some(&results))?;
                    require(
                        decision.model == authorization.model,
                        "功能测试期间主指挥模型变化",
                    )?;
                    control.check()?;
                    Ok(Completed {
                        model: decision.model,
                        authorization: authorization.decision,
                        report: Some(report),
                        decision: decision.decision,
                    })
                })();
                let _ = sender.send(result);
            });
        if launched.is_err() {
            self.blind_reviews
                .interrupt_functional(id, "功能测试线程启动失败，原许可撤销")?;
            return Err("功能测试线程启动失败".into());
        }
        self.functional_active = Some(ActiveFunctionalTest {
            id: id.into(),
            cancel,
            progress,
            pending,
            receiver,
        });
        Ok(())
    }
    pub(super) fn poll_functional_test(&mut self) {
        let Some(active) = &self.functional_active else {
            return;
        };
        if self
            .blind_reviews
            .rounds
            .get(&active.id)
            .and_then(|r| r.functional.as_ref())
            .and_then(|f| f.deadline)
            .is_some_and(|d| chrono::Utc::now().timestamp() >= d)
        {
            active.cancel.store(true, Ordering::Release);
        }
        let result = match active.receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(_) => Err("功能测试进程结束但没有回执，请核对原操作".into()),
        };
        let active = self.functional_active.take().unwrap();
        if !self
            .blind_reviews
            .rounds
            .get(&active.id)
            .is_some_and(|r| r.phase == ReviewPhase::FunctionalTesting)
        {
            return;
        }
        let completed = (|| {
            let binding = self.current_blind_binding(&active.id)?;
            require(
                self.blind_reviews.rounds[&active.id].binding == binding,
                "功能测试期间方案或交付变化，结果未用于验收",
            )?;
            let done = result?;
            self.blind_reviews.finish_functional(
                &active.id,
                &active.pending,
                done,
                chrono::Utc::now().timestamp(),
            )
        })();
        if let Err(error) = completed {
            let _ = self.blind_reviews.interrupt_functional(&active.id, &error);
        }
    }
    pub(super) fn require_functional_evidence(&self, id: &str) -> Result<(), String> {
        let round = self.blind_reviews.rounds.get(id).ok_or("原轮次缺失")?;
        require(
            round.confirmation_digest().is_some(),
            "主指挥整体功能测试尚未通过",
        )?;
        let run = round.functional.as_ref().ok_or("功能测试记录缺失")?;
        let report = run.report.as_ref().ok_or("功能测试报告缺失")?;
        let path = self
            .root
            .join("workflows")
            .join(&round.binding.project_id)
            .join("functional-verification")
            .join(format!("functional-{id}"))
            .join("result.json");
        ordinary(&path, false)?;
        use std::io::Read;
        let mut bytes = vec![];
        fs::File::open(&path)
            .map_err(|e| e.to_string())?
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        require(bytes.len() <= 1024 * 1024, "功能测试证据超限")?;
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| "功能测试报告损坏")?;
        let persisted: VerificationReport =
            serde_json::from_value(value["report"].clone()).map_err(|_| "功能测试报告缺失")?;
        require(
            value["schemaVersion"] == 1
                && value["operationId"] == format!("functional-{id}")
                && persisted == *report,
            "功能测试证据与原记录不符",
        )
    }
}
