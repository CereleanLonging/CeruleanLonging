use super::*;
use cerulean_domain::execution::VerificationReport;

pub(crate) struct FunctionalOutcome {
    pub model: String,
    pub authorization: FunctionalDecision,
    pub report: Option<VerificationReport>,
    pub decision: FunctionalDecision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct FunctionalRun {
    pub status: FunctionalTestStatus,
    pub pending: Option<Pending>,
    pub operation: Option<String>,
    pub deadline: Option<i64>,
    pub model: Option<String>,
    pub authorization: Option<FunctionalDecision>,
    pub report: Option<VerificationReport>,
    pub decision: Option<FunctionalDecision>,
    pub message: Option<String>,
}
impl FunctionalRun {
    pub fn locked() -> Self {
        Self {
            status: FunctionalTestStatus::Locked,
            pending: None,
            operation: None,
            deadline: None,
            model: None,
            authorization: None,
            report: None,
            decision: None,
            message: None,
        }
    }
    pub fn view(&self) -> FunctionalTestView {
        FunctionalTestView {
            status: self.status.clone(),
            operation_id: self.operation.clone(),
            model: self.model.clone(),
            checks: self
                .report
                .as_ref()
                .map(|r| r.checks.clone())
                .unwrap_or_default(),
            report_digest: self.report.as_ref().and_then(|r| digest(r).ok()),
            decision: self.decision.clone(),
            message: self.message.clone(),
        }
    }
    pub fn validate(&self, round: &Round) -> Result<(), String> {
        if matches!(
            self.status,
            FunctionalTestStatus::Locked | FunctionalTestStatus::Ready
        ) {
            require(
                self.operation.is_none()
                    && self.deadline.is_none()
                    && self.pending.is_none()
                    && self.model.is_none()
                    && self.authorization.is_none()
                    && self.report.is_none()
                    && self.decision.is_none(),
                "尚未开放的功能测试不能包含执行记录",
            )?;
        } else {
            require(
                self.operation.as_deref() == Some(format!("functional-{}", round.id).as_str())
                    && self.deadline.is_some(),
                "功能测试操作不属于原轮次",
            )?;
        }
        if let Some(p) = &self.pending {
            require(
                self.status == FunctionalTestStatus::Running && p.seat == "chief" && !p.readiness,
                "功能测试仅允许本轮主指挥",
            )?;
            cerulean_domain::execution::sha256(&p.epoch)?;
            identifier(&p.request)?;
        }
        require(
            (self.status == FunctionalTestStatus::Running) == self.pending.is_some(),
            "功能测试请求状态不一致",
        )?;
        if let Some(operation) = &self.operation {
            identifier(operation)?;
        }
        if let Some(model) = &self.model {
            require(
                round
                    .seats
                    .iter()
                    .any(|s| s.role == "chief" && s.allowed_models.contains(model)),
                "功能测试返回模型不是本轮主指挥",
            )?;
        }
        for decision in [&self.authorization, &self.decision].into_iter().flatten() {
            decision.validate()?;
        }
        if let Some(report) = &self.report {
            require(
                self.authorization
                    .as_ref()
                    .is_some_and(|a| a.vote == Vote::Pass)
                    && report
                        .source_snapshot
                        .as_ref()
                        .is_some_and(|s| Some(&s.operation_id) == self.operation.as_ref()),
                "功能检查缺少主指挥授权或封存不符",
            )?;
        }
        if self.status == FunctionalTestStatus::Passed {
            require(
                self.model.is_some()
                    && self.decision.as_ref().is_some_and(|d| d.vote == Vote::Pass)
                    && self.report.as_ref().is_some_and(|r| {
                        !r.checks.is_empty()
                            && r.checks.iter().all(|c| {
                                c.exit_code == Some(0)
                                    && !c.timed_out
                                    && c.assertion_error.is_none()
                                    && c.assertions.iter().all(|a| a.passed)
                            })
                    }),
                "功能测试通过证据不足",
            )?;
        }
        require(
            match round.phase {
                ReviewPhase::Prepared | ReviewPhase::Collecting | ReviewPhase::Rejected => {
                    self.status == FunctionalTestStatus::Locked
                }
                ReviewPhase::AwaitingFunctionalTest => self.status == FunctionalTestStatus::Ready,
                ReviewPhase::FunctionalTesting => self.status == FunctionalTestStatus::Running,
                ReviewPhase::FunctionalTestFailed => matches!(
                    self.status,
                    FunctionalTestStatus::Failed | FunctionalTestStatus::Interrupted
                ),
                ReviewPhase::AwaitingUser | ReviewPhase::Accepted => {
                    self.status == FunctionalTestStatus::Passed
                }
                _ => self.status != FunctionalTestStatus::Running,
            },
            "功能测试与交付阶段不符",
        )
    }
}

impl Round {
    pub fn confirmation_digest(&self) -> Option<String> {
        if self.tool_audit_required != Some(true) {
            return None;
        }
        let run = self.functional.as_ref()?;
        (run.status == FunctionalTestStatus::Passed)
            .then(|| digest(&(&self.digest, run)).ok())
            .flatten()
    }
}

impl Ledger {
    pub fn start_functional(
        &mut self,
        id: &str,
        key: &str,
        current: &ReviewBinding,
        now: i64,
        seconds: u32,
    ) -> Result<(Pending, ReviewSeat), String> {
        let mut r = self.checked(&current.project_id, id)?;
        require(
            r.phase == ReviewPhase::AwaitingFunctionalTest
                && r.tool_audit_required == Some(true)
                && r.digest == key
                && r.binding == *current,
            "只有当前盲评全票通过的交付才能解锁功能测试",
        )?;
        require((1..=3600).contains(&seconds), "功能检查期限无效")?;
        let chief = r
            .seats
            .iter()
            .find(|s| s.role == "chief" && s.id == "chief")
            .ok_or("本轮主指挥缺失")?
            .clone();
        let pending = Pending {
            epoch: self.epoch.clone(),
            request: format!("functional-{:08}-{}", self.sequence + 1, &self.epoch[..16]),
            seat: chief.id.clone(),
            readiness: false,
        };
        let run = r.functional.as_mut().ok_or("功能测试许可缺失")?;
        require(
            run.status == FunctionalTestStatus::Ready && run.operation.is_none(),
            "原功能测试已经执行，不会重试",
        )?;
        run.status = FunctionalTestStatus::Running;
        run.operation = Some(format!("functional-{id}"));
        run.pending = Some(pending.clone());
        run.deadline = Some(now + i64::from(seconds) + 210);
        r.phase = ReviewPhase::FunctionalTesting;
        self.commit(r)?;
        Ok((pending, chief))
    }

    pub fn finish_functional(
        &mut self,
        id: &str,
        pending: &Pending,
        outcome: FunctionalOutcome,
        now: i64,
    ) -> Result<(), String> {
        let mut r = self.rounds.get(id).ok_or("原功能测试轮次缺失")?.clone();
        let run = r.functional.as_mut().ok_or("功能测试许可缺失")?;
        require(
            r.phase == ReviewPhase::FunctionalTesting
                && run.pending.as_ref() == Some(pending)
                && pending.epoch == self.epoch
                && run.deadline.is_some_and(|d| now < d),
            "功能测试回包已取消、超时或不是原请求",
        )?;
        run.model = Some(outcome.model);
        run.authorization = Some(outcome.authorization);
        run.report = outcome.report;
        run.pending = None;
        run.status = if outcome.decision.vote == Vote::Pass {
            FunctionalTestStatus::Passed
        } else {
            FunctionalTestStatus::Failed
        };
        run.decision = Some(outcome.decision);
        r.phase = if run.status == FunctionalTestStatus::Passed {
            ReviewPhase::AwaitingUser
        } else {
            ReviewPhase::FunctionalTestFailed
        };
        self.commit(r)
    }

    pub fn interrupt_functional(&mut self, id: &str, message: &str) -> Result<(), String> {
        let mut r = self.rounds.get(id).ok_or("原功能测试轮次缺失")?.clone();
        require(
            r.phase == ReviewPhase::FunctionalTesting,
            "原功能测试已经结束",
        )?;
        require(
            !message.is_empty() && message.len() <= 4096,
            "功能测试说明无效",
        )?;
        let run = r.functional.as_mut().ok_or("功能测试许可缺失")?;
        run.status = FunctionalTestStatus::Interrupted;
        run.pending = None;
        run.message = Some(message.into());
        r.phase = ReviewPhase::FunctionalTestFailed;
        self.commit(r)
    }
}

pub(super) fn transition(old: &Round, new: &Round) -> Result<(), String> {
    let allowed = match (&old.functional, &new.functional) {
        (None, None) => true,
        (None, Some(b)) => {
            b.status == FunctionalTestStatus::Ready && old.phase == ReviewPhase::AwaitingUser
                || b.status == FunctionalTestStatus::Locked && old.phase == ReviewPhase::Prepared
        }
        (Some(a), Some(b)) if a == b => true,
        (Some(a), Some(b)) => match (&a.status, &b.status) {
            (FunctionalTestStatus::Locked, FunctionalTestStatus::Ready) => {
                old.phase == ReviewPhase::Collecting
                    && new.phase == ReviewPhase::AwaitingFunctionalTest
            }
            (FunctionalTestStatus::Ready, FunctionalTestStatus::Running) => {
                old.phase == ReviewPhase::AwaitingFunctionalTest
                    && new.phase == ReviewPhase::FunctionalTesting
            }
            (
                FunctionalTestStatus::Running,
                FunctionalTestStatus::Passed
                | FunctionalTestStatus::Failed
                | FunctionalTestStatus::Interrupted
                | FunctionalTestStatus::Cancelled,
            ) => a.operation == b.operation && a.deadline == b.deadline && b.pending.is_none(),
            _ => false,
        },
        _ => false,
    };
    require(allowed, "功能测试许可和结果不可重发或回滚")
}
