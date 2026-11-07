//! Read-only, append-cursor projection of committed workflow records.
use crate::scheduler::DurableWorkflow;
use cerulean_domain::{
    execution::EventAnchor,
    execution::{require, ExecutionEvidence},
    workflow::WorkflowCommand,
    workspace::*,
};

pub(crate) fn query(
    store: &DurableWorkflow,
    project_id: &str,
    after: Option<&EventAnchor>,
    before: Option<&EventAnchor>,
) -> Result<WorkspaceResult, String> {
    require(
        !store.requires_reopen(),
        "项目记录写入尚未核对，请保留已有内容",
    )?;
    require(
        store
            .configuration()
            .execution_scope
            .as_ref()
            .is_some_and(|s| s.project.id == project_id),
        "项目记录身份不符",
    )?;
    require(
        after.is_none() || before.is_none(),
        "不能同时向前和向后读取",
    )?;
    let entries = store.history();
    let locate = |cursor: &EventAnchor| -> Result<usize, String> {
        entries
            .iter()
            .position(|e| e.body.event.sequence == cursor.sequence && e.hash == cursor.hash)
            .ok_or("记录游标与原账本不符，请重新读取".into())
    };
    let (start, end) = if let Some(cursor) = after {
        let start = locate(cursor)? + 1;
        (start, (start + 40).min(entries.len()))
    } else {
        let end = before.map(locate).transpose()?.unwrap_or(entries.len());
        (end.saturating_sub(40), end)
    };
    let mut projected = Vec::new();
    for (index, entry) in entries.iter().enumerate().take(end).skip(start) {
        let command = &entry.body.event.command;
        let revision =
            entries[..=index]
                .iter()
                .rev()
                .find_map(|e| match &e.body.context.execution {
                    Some(ExecutionEvidence::Revision { revision }) => Some(revision),
                    _ => None,
                });
        let task = command
            .task()
            .and_then(|id| revision.and_then(|r| r.execution.iter().find(|t| t.task_id == id)));
        let model = task
            .filter(|_| entry.body.event.principal.starts_with("worker-"))
            .and_then(|t| {
                t.worker
                    .as_ref()
                    .map(|w| w.model.clone())
                    .or_else(|| t.api_worker.as_ref().map(|w| w.model.clone()))
                    .or_else(|| t.desktop_worker.as_ref().map(|w| w.model_label.clone()))
            });
        let (title, detail): (&str, &str) = match command {
            WorkflowCommand::Propose { .. } => ("主指挥提出方案", "任务和交付文件已进入待批准方案"),
            WorkflowCommand::Approve { .. } => ("用户批准方案", "按原批准范围执行"),
            WorkflowCommand::Assign { .. } => ("副指挥分配任务", "已绑定执行模型及其工作目录"),
            WorkflowCommand::DispatchIntent { .. } => {
                ("准备派发", "已记录派发意图，尚不代表模型完成")
            }
            WorkflowCommand::Started { .. } => ("执行模型开始工作", "已关联本次尝试"),
            WorkflowCommand::Submit { summary, .. } => ("执行模型提交结果", summary),
            WorkflowCommand::Verify { passed, .. } => {
                if *passed {
                    ("独立检查通过", "客观检查结果已记录")
                } else {
                    ("独立检查未通过", "保留原提交和检查结果，等待处理")
                }
            }
            WorkflowCommand::Accept { .. } => ("主指挥接受任务结果", "任务收口记录已保存"),
            WorkflowCommand::Advise { suggestion, .. } => ("副指挥监督建议", suggestion),
            WorkflowCommand::ReturnForRework { suggestion, .. } => {
                ("副指挥根据独立检查安排返工", suggestion)
            }
            WorkflowCommand::RequestRevision { reason, .. } => ("请求修订方案", reason),
            WorkflowCommand::CancelIntent { reason, .. } => ("请求停止任务", reason),
            WorkflowCommand::RecoveryUnknown { .. } => {
                ("执行结果待核对", "尚未确认原请求结果，不自动重发")
            }
            WorkflowCommand::Reconcile { .. } => ("核对原执行记录", "已保存核对结果"),
            WorkflowCommand::ApproveVerificationRecheck { .. } => {
                ("复验方案已批准", "保留原失败检查")
            }
            WorkflowCommand::RetireVerificationRecheck { .. } => ("复验已收尾", "原证据保留"),
            WorkflowCommand::AbandonUnsent { reason, .. }
            | WorkflowCommand::RetireUnknownApi { reason, .. }
            | WorkflowCommand::RetireUnknownDesktop { reason, .. }
            | WorkflowCommand::RetireSubmittedPlan { reason, .. } => ("任务处置记录", reason),
            _ => ("项目授权记录", "核心已保存该次授权或核对记录"),
        };
        let files: Vec<String> = match command {
            WorkflowCommand::Propose { .. } => revision
                .into_iter()
                .flat_map(|r| &r.execution)
                .flat_map(|t| &t.artifact_paths)
                .take(8)
                .map(|p| p.chars().take(240).collect())
                .collect(),
            WorkflowCommand::Submit { .. } => task
                .into_iter()
                .flat_map(|t| &t.artifact_paths)
                .take(8)
                .map(|p| p.chars().take(240).collect())
                .collect(),
            _ => vec![],
        };
        projected.push(WorkspaceActivityEntry {
            anchor: EventAnchor {
                sequence: entry.body.event.sequence,
                hash: entry.hash.clone(),
            },
            at: entry.body.received_at.clone(),
            principal: entry.body.event.principal.clone(),
            model,
            task_id: command.task().map(str::to_owned),
            attempt_id: command.attempt().map(str::to_owned),
            title: title.into(),
            detail: detail.chars().take(3000).collect(),
            files,
        });
    }
    crate::workspace_read::bounded(WorkspaceResult::ProjectActivity(WorkspaceActivityPage {
        project_id: project_id.into(),
        entries: projected,
        head: entries.last().map(|e| EventAnchor {
            sequence: e.body.event.sequence,
            hash: e.hash.clone(),
        }),
        has_older: start > 0,
        has_newer: end < entries.len(),
        total: entries.len() as u32,
    }))
}
