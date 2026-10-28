//! Immutable selected management evidence passed to the original workflow owner.
use super::*;
use crate::management_execution::{
    ApplicationTarget, DecisionIntent, DecisionStatus, StoredDecision,
};
use crate::startup_journal::StartupRecord;
use cerulean_adapters::openai::DecisionObservation;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct StartupManagementClosure {
    pub reason: String,
    pub attempt_id: String,
    pub startup_hash: String,
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct StartupManagementOperation {
    pub status: DecisionStatus,
    pub intent: DecisionIntent,
    pub evidence_digest: String,
    pub response_head: Option<String>,
    pub observation: Option<DecisionObservation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_decision: Option<StoredDecision>,
    pub application: Option<(ApplicationTarget, bool, Option<String>)>,
    pub closure: Option<StartupManagementClosure>,
    pub ready: bool,
}
impl StartupManagementOperation {
    pub(crate) fn envelope(&self) -> Option<&cerulean_domain::management::DecisionEnvelope> {
        self.source_decision
            .as_ref()
            .and_then(StoredDecision::envelope)
            .or_else(|| self.observation.as_ref().map(|o| &o.envelope))
    }
}
#[derive(Clone, Debug, Serialize)]
pub(crate) struct StartupManagementProof {
    pub project_id: String,
    pub operations: Vec<StartupManagementOperation>,
}
impl RuntimeHost {
    pub(crate) fn startup_management_proof(
        &self,
        project: &str,
        records: &[StartupRecord],
    ) -> Result<StartupManagementProof, String> {
        require(self.management.healthy(), "管理账本需要重开恢复")?;
        identifier(project)?;
        let mut operations = Vec::new();
        for status in self
            .management
            .statuses()
            .into_iter()
            .filter(|s| s.project_id == project)
        {
            let id = &status.operation_id;
            let intent = self.management.intent(id)?;
            require(
                intent.project_id == project && intent.operation_id == *id,
                "管理启动证据项目身份不一致",
            )?;
            let application = self
                .management
                .application(id)
                .map(|(target, applied, hash)| (target.clone(), applied, hash.map(str::to_owned)));
            if let Some((
                ApplicationTarget::Workflow {
                    command: WorkflowCommand::Assign { attempt, .. },
                    ..
                },
                _,
                _,
            )) = &application
            {
                for record in records.iter().filter(|r| r.intent.attempt_id == *attempt) {
                    require(
                        record.intent.management_operation.as_deref() == Some(id.as_str()),
                        "启动预留与另一管理应用冲突",
                    )?;
                }
            }
            let closure = self
                .management
                .startup_closure(id)
                .map(|c| StartupManagementClosure {
                    reason: c.reason,
                    attempt_id: c.attempt_id,
                    startup_hash: c.startup_hash,
                });
            let selected = records
                .iter()
                .any(|r| r.intent.management_operation.as_deref() == Some(id.as_str()));
            let response_head = if selected {
                Some(self.management.response_head(id)?.to_owned())
            } else {
                self.management.response_head(id).ok().map(str::to_owned)
            };
            let stored = self.management.stored_decision(id)?;
            let envelope = stored.envelope();
            let ready = self
                .management
                .ready_envelope(id, &intent.state_digest)
                .is_ok();
            if selected {
                require(
                    intent.purpose == cerulean_domain::management::Purpose::Assign,
                    "启动管理请求用途不符",
                )?;
                for record in records
                    .iter()
                    .filter(|r| r.intent.management_operation.as_deref() == Some(id.as_str()))
                {
                    require(
                        self.codex_jobs
                            .values()
                            .filter(|job| job.management_operation.as_deref() == Some(id.as_str()))
                            .all(|job| {
                                job.project_id == record.intent.project_id
                                    && job.task_id == record.intent.task_id
                                    && job.attempt_id == record.intent.attempt_id
                            }),
                        "管理请求关联多个或错误启动记录",
                    )?;
                    require(
                        response_head.as_deref() == record.intent.management_hash.as_deref(),
                        "启动意图与原管理响应链不符",
                    )?;
                    require(
                        matches!(envelope.map(|o| &o.decision), Some(cerulean_domain::management::ManagementDecision::Assign { task_id, worker_id, .. }) if task_id == &record.intent.task_id && worker_id == "worker-codex"),
                        "启动意图与原管理分配决定不符",
                    )?;
                    if application.is_none() && closure.is_none() {
                        require(
                            self.management.evidence_digest(id)?
                                == record
                                    .intent
                                    .management_hash
                                    .as_deref()
                                    .ok_or("启动管理摘要缺失")?
                                && ready,
                            "原管理链已变化或被其他方式处置",
                        )?;
                    }
                }
            }
            let evidence_digest = self.management.evidence_digest(id)?.to_owned();
            let (observation, source_decision) = match stored {
                StoredDecision::OpenAi { observation } => (observation.map(|o| *o), None),
                source => (None, Some(source)),
            };
            operations.push(StartupManagementOperation {
                status,
                intent: intent.clone(),
                evidence_digest,
                response_head,
                observation,
                source_decision,
                application,
                closure,
                ready,
            });
        }
        for record in records {
            require(record.intent.project_id == project, "启动管理证据跨项目")?;
            if let Some(id) = &record.intent.management_operation {
                require(
                    operations.iter().any(|op| op.intent.operation_id == *id),
                    "启动意图缺少原管理请求",
                )?;
            }
        }
        operations.sort_by(|a, b| a.intent.operation_id.cmp(&b.intent.operation_id));
        Ok(StartupManagementProof {
            project_id: project.into(),
            operations,
        })
    }
}
