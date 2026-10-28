//! Optional derived prefix, never an authorization or replay input.
use super::*;

pub(super) fn event(
    entry: &Entry,
    state: &WorkflowState,
    sequence: u64,
) -> Result<Option<HandoffEvent>, String> {
    let command = &entry.body.event.command;
    let task = match command {
        WorkflowCommand::Propose { .. }
        | WorkflowCommand::AuthorizePlanningBudget { .. }
        | WorkflowCommand::ConfirmHistoricalCharge { .. }
        | WorkflowCommand::ConfirmManagementCountResolution { .. }
        | WorkflowCommand::ConfirmApiGenerationResolution { .. }
        | WorkflowCommand::ConfirmApiCountResolution { .. }
        | WorkflowCommand::Approve { .. }
        | WorkflowCommand::RequestRevision { .. } => return Ok(None),
        WorkflowCommand::RetireSubmittedPlan { .. } => return Ok(None),
        WorkflowCommand::Assign { task, .. }
        | WorkflowCommand::AbandonUnsent { task, .. }
        | WorkflowCommand::RetireUnknownApi { task, .. }
        | WorkflowCommand::RetireUnknownDesktop { task, .. }
        | WorkflowCommand::DispatchIntent { task, .. }
        | WorkflowCommand::Started { task, .. }
        | WorkflowCommand::CancelIntent { task, .. }
        | WorkflowCommand::RecoveryUnknown { task, .. }
        | WorkflowCommand::Reconcile { task, .. }
        | WorkflowCommand::Submit { task, .. }
        | WorkflowCommand::Verify { task, .. }
        | WorkflowCommand::Accept { task, .. }
        | WorkflowCommand::Advise { task, .. } => task,
    };
    let run = state
        .tasks
        .get(task)
        .ok_or("handoff task missing from verified state")?;
    let status = match (command, run.state) {
        (WorkflowCommand::AbandonUnsent { .. }, _) => HandoffStatus::Blocked,
        (_, TaskState::Accepted) => HandoffStatus::Accepted,
        (_, TaskState::Cancelled) => HandoffStatus::Cancelled,
        (_, TaskState::Submitted) => HandoffStatus::Submitted,
        (_, TaskState::Unknown) => HandoffStatus::Blocked,
        (_, TaskState::Retired) => HandoffStatus::Blocked,
        _ => HandoffStatus::Running,
    };
    let summary = match command {
        WorkflowCommand::RetireUnknownDesktop { reason, .. } => {
            format!("用户结束本地桌面跟踪，外部任务结果与费用仍未知，已有修改保留：{reason}")
        }
        WorkflowCommand::RetireUnknownApi { reason, .. } => {
            format!("用户结束本地API尝试，远端结果与费用仍未知，已有修改保留：{reason}")
        }
        WorkflowCommand::AbandonUnsent { reason, .. } => {
            format!("用户放弃尚未发送的本地尝试：{reason}")
        }
        WorkflowCommand::Submit { summary, .. } => summary.clone(),
        WorkflowCommand::Advise { suggestion, .. } => suggestion.clone(),
        _ => serde_json::to_value(command).map_err(|e| e.to_string())?["kind"]
            .as_str()
            .ok_or("missing command kind")?
            .into(),
    };
    let time = chrono::DateTime::parse_from_rfc3339(&entry.body.received_at)
        .map_err(|_| "invalid ledger time")?
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    Ok(Some(HandoffEvent {
        sequence,
        // Stable ledger linkage; caller operation IDs may be longer than display IDs.
        message_id: format!("event-{}", entry.body.event.sequence),
        task_id: task.clone(),
        attempt_id: match command {
            WorkflowCommand::AbandonUnsent { attempt, .. } => attempt.clone(),
            _ => run.attempt.clone().unwrap_or("unassigned".into()),
        },
        actor: entry.body.event.principal.clone(),
        received_at: time.clone(),
        occurred_at: time,
        next_action: match (command, &status) {
            (
                WorkflowCommand::RetireUnknownApi { .. }
                | WorkflowCommand::RetireUnknownDesktop { .. },
                _,
            ) => "须申请修订并重新批准方案；原请求不得重发",
            (WorkflowCommand::AbandonUnsent { .. }, _) => "原尝试已关闭；后续须使用新尝试重新分配",
            (_, HandoffStatus::Blocked) => "对账后继续，禁止自动重发",
            (_, HandoffStatus::Submitted) => "独立验证后验收",
            _ => "按计划推进",
        }
        .into(),
        status,
        summary,
        evidence: run.evidence.clone(),
    }))
}

pub(super) struct Cached {
    pub head: crate::derived_index::Head,
    pub events: Vec<HandoffEvent>,
    text_bytes: usize,
    memory: crate::derived_index::RetainedCapacity,
}
fn text_bytes(e: &HandoffEvent) -> usize {
    [
        &e.message_id,
        &e.task_id,
        &e.attempt_id,
        &e.actor,
        &e.received_at,
        &e.occurred_at,
        &e.summary,
        &e.next_action,
    ]
    .into_iter()
    .map(String::capacity)
    .sum::<usize>()
        + e.evidence.capacity() * std::mem::size_of::<String>()
        + e.evidence.iter().map(String::capacity).sum::<usize>()
}
fn retained_bytes(head: &crate::derived_index::Head, capacity: usize, text: usize) -> usize {
    std::mem::size_of::<Cached>()
        + head.ledger.capacity()
        + head.hash.capacity()
        + head.bindings.capacity()
        + capacity * std::mem::size_of::<HandoffEvent>()
        + text
}
impl Cached {
    pub fn new(
        head: crate::derived_index::Head,
        events: Vec<HandoffEvent>,
        index: &crate::derived_index::Cache,
    ) -> Result<Self, String> {
        let text_bytes = events.iter().map(text_bytes).sum();
        let memory = index.retain_handoffs(retained_bytes(&head, events.capacity(), text_bytes))?;
        Ok(Self {
            head,
            events,
            text_bytes,
            memory,
        })
    }
    pub fn advance(
        mut self,
        head: crate::derived_index::Head,
        event: Option<HandoffEvent>,
    ) -> Result<Self, String> {
        let wanted_len = self.events.len() + usize::from(event.is_some());
        let capacity = if wanted_len > self.events.capacity() {
            wanted_len.div_ceil(64) * 64
        } else {
            self.events.capacity()
        };
        let text = self.text_bytes + event.as_ref().map(text_bytes).unwrap_or(0);
        self.memory.resize(retained_bytes(&head, capacity, text))?;
        if capacity > self.events.capacity() {
            self.events
                .try_reserve_exact(capacity - self.events.len())
                .map_err(|e| e.to_string())?;
        }
        // Account actual allocation capacity if the allocator reserved more.
        self.memory
            .resize(retained_bytes(&head, self.events.capacity(), text))?;
        self.text_bytes = text;
        if let Some(event) = event {
            self.events.push(event);
        }
        self.head = head;
        Ok(self)
    }
}
