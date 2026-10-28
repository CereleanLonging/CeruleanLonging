//! Pure control-plane migration. Principal bindings must come from the trusted
//! host, never from worker-authored messages. Persist returned events BEFORE effects.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Chief,
    Deputy,
    Worker,
    Core,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Principal {
    pub id: String,
    pub role: Role,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct WorkflowTask {
    pub id: String,
    pub dependencies: Vec<String>,
    pub workspace_key: String,
    pub goal: String,
    // Keep order and omitted-vs-empty distinction to preserve prototype approval hashes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource_keys: Option<Vec<String>>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WorkflowPlan {
    pub id: String,
    pub version: u32,
    pub tasks: Vec<WorkflowTask>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum TaskState {
    Pending,
    Assigned,
    Dispatching,
    Running,
    Cancelling,
    Unknown,
    Submitted,
    Accepted,
    Cancelled,
    Retired,
}
impl TaskState {
    pub fn holds_resources(self) -> bool {
        matches!(
            self,
            Self::Assigned
                | Self::Dispatching
                | Self::Running
                | Self::Cancelling
                | Self::Unknown
                | Self::Submitted
        )
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct TaskRun {
    pub state: TaskState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub worker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attempt: Option<String>,
    pub fence: u32,
    pub evidence: Vec<String>,
    pub verified: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cancellation_requested: Option<bool>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Advice {
    pub task: String,
    pub suggestion: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
pub struct WorkflowState {
    pub plan: Option<WorkflowPlan>,
    pub digest: Option<String>,
    pub approved: bool,
    pub tasks: BTreeMap<String, TaskRun>,
    pub advice: Vec<Advice>,
    #[serde(
        default,
        rename = "revisionRequested",
        skip_serializing_if = "Option::is_none"
    )]
    pub revision_requested: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ReconcileOutcome {
    NotStarted,
    Running,
    Submitted,
    Cancelled,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum WorkflowCommand {
    ConfirmApiGenerationResolution {
        review_digest: String,
    },
    ConfirmApiCountResolution {
        review_digest: String,
    },
    ConfirmManagementCountResolution {
        review_digest: String,
    },
    ConfirmHistoricalCharge {
        review_digest: String,
    },
    AuthorizePlanningBudget {
        setup_digest: String,
    },
    RetireSubmittedPlan {
        digest: String,
        reason: String,
    },
    RequestRevision {
        digest: String,
        reason: String,
    },
    Propose {
        plan: WorkflowPlan,
    },
    Approve {
        digest: String,
    },
    Assign {
        task: String,
        worker: String,
        attempt: String,
    },
    AbandonUnsent {
        task: String,
        attempt: String,
        reason: String,
    },
    RetireUnknownDesktop {
        task: String,
        attempt: String,
        evidence_digest: String,
        reason: String,
    },
    RetireUnknownApi {
        task: String,
        attempt: String,
        reason: String,
    },
    DispatchIntent {
        task: String,
        attempt: String,
    },
    Started {
        task: String,
        attempt: String,
    },
    CancelIntent {
        task: String,
        attempt: String,
        reason: String,
    },
    RecoveryUnknown {
        task: String,
        attempt: String,
    },
    Reconcile {
        task: String,
        attempt: String,
        outcome: ReconcileOutcome,
        evidence: Vec<String>,
    },
    Submit {
        task: String,
        attempt: String,
        summary: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        evidence: Option<Vec<String>>,
    },
    Verify {
        task: String,
        attempt: String,
        passed: bool,
        evidence: Vec<String>,
    },
    Accept {
        task: String,
        attempt: String,
    },
    Advise {
        task: String,
        suggestion: String,
    },
}
impl WorkflowCommand {
    pub fn task(&self) -> Option<&str> {
        match self {
            Self::Propose { .. }
            | Self::AuthorizePlanningBudget { .. }
            | Self::ConfirmHistoricalCharge { .. }
            | Self::ConfirmManagementCountResolution { .. }
            | Self::ConfirmApiGenerationResolution { .. }
            | Self::ConfirmApiCountResolution { .. }
            | Self::Approve { .. }
            | Self::RequestRevision { .. }
            | Self::RetireSubmittedPlan { .. } => None,
            Self::Assign { task, .. }
            | Self::AbandonUnsent { task, .. }
            | Self::RetireUnknownApi { task, .. }
            | Self::RetireUnknownDesktop { task, .. }
            | Self::DispatchIntent { task, .. }
            | Self::Started { task, .. }
            | Self::CancelIntent { task, .. }
            | Self::RecoveryUnknown { task, .. }
            | Self::Reconcile { task, .. }
            | Self::Submit { task, .. }
            | Self::Verify { task, .. }
            | Self::Accept { task, .. }
            | Self::Advise { task, .. } => Some(task),
        }
    }
    pub fn attempt(&self) -> Option<&str> {
        match self {
            Self::Propose { .. }
            | Self::AuthorizePlanningBudget { .. }
            | Self::ConfirmHistoricalCharge { .. }
            | Self::ConfirmManagementCountResolution { .. }
            | Self::ConfirmApiGenerationResolution { .. }
            | Self::ConfirmApiCountResolution { .. }
            | Self::Approve { .. }
            | Self::Advise { .. }
            | Self::RequestRevision { .. }
            | Self::RetireSubmittedPlan { .. } => None,
            Self::Assign { attempt, .. }
            | Self::AbandonUnsent { attempt, .. }
            | Self::RetireUnknownApi { attempt, .. }
            | Self::RetireUnknownDesktop { attempt, .. }
            | Self::DispatchIntent { attempt, .. }
            | Self::Started { attempt, .. }
            | Self::CancelIntent { attempt, .. }
            | Self::RecoveryUnknown { attempt, .. }
            | Self::Reconcile { attempt, .. }
            | Self::Submit { attempt, .. }
            | Self::Verify { attempt, .. }
            | Self::Accept { attempt, .. } => Some(attempt),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct WorkflowEvent {
    pub sequence: u32,
    pub principal: String,
    pub command: WorkflowCommand,
}

#[derive(Debug, Clone)]
pub struct Workflow {
    principals: BTreeMap<String, Principal>,
    state: WorkflowState,
    journal: Vec<WorkflowEvent>,
    single_executor: bool,
}
fn require(condition: bool, message: &str) -> Result<(), String> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}
fn id(value: &str) -> Result<(), String> {
    require(
        !value.is_empty()
            && value.len() <= 80
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
        "invalid identifier",
    )
}
fn text(value: &str) -> Result<(), String> {
    require(
        !value.trim().is_empty() && value.encode_utf16().count() <= 8192,
        "invalid text",
    )
}
fn evidence(values: &[String]) -> Result<(), String> {
    require(!values.is_empty(), "evidence required")?;
    values.iter().try_for_each(|s| text(s))
}
fn role(principal: &Principal, expected: Role) -> Result<(), String> {
    require(principal.role == expected, "role not authorized")
}
pub fn workflow_digest(plan: &WorkflowPlan) -> Result<String, String> {
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(plan).map_err(|e| e.to_string())?)
    ))
}
fn validate_plan(plan: &WorkflowPlan) -> Result<(), String> {
    id(&plan.id)?;
    require(plan.version > 0 && !plan.tasks.is_empty(), "invalid plan")?;
    let ids = plan
        .tasks
        .iter()
        .map(|t| t.id.as_str())
        .collect::<BTreeSet<_>>();
    require(ids.len() == plan.tasks.len(), "invalid task graph")?;
    for t in &plan.tasks {
        id(&t.id)?;
        id(&t.workspace_key)?;
        text(&t.goal)?;
        for d in &t.dependencies {
            id(d)?;
            require(ids.contains(d.as_str()), "invalid task graph")?;
        }
        if let Some(keys) = &t.resource_keys {
            require(keys.len() <= 32, "invalid resource keys")?;
            for key in keys {
                id(key)?;
            }
            require(
                keys.iter().collect::<BTreeSet<_>>().len() == keys.len(),
                "duplicate resource keys",
            )?;
        }
    }
    // Iterative topological check avoids recursion through an untrusted task graph.
    let mut visited = BTreeSet::new();
    while visited.len() < plan.tasks.len() {
        let before = visited.len();
        for t in &plan.tasks {
            if t.dependencies.iter().all(|d| visited.contains(d)) {
                visited.insert(t.id.clone());
            }
        }
        require(visited.len() > before, "dependency cycle")?;
    }
    Ok(())
}
impl Workflow {
    pub fn new(principals: Vec<Principal>) -> Result<Self, String> {
        let mut bindings = BTreeMap::new();
        for principal in principals {
            id(&principal.id)?;
            if matches!(principal.role, Role::Chief | Role::Deputy) {
                require(
                    principal.provider.as_deref() == Some("openai"),
                    "commander requires OpenAI binding",
                )?;
            }
            require(!bindings.contains_key(&principal.id), "duplicate principal")?;
            bindings.insert(principal.id.clone(), principal);
        }
        Ok(Self {
            principals: bindings,
            state: WorkflowState::default(),
            journal: vec![],
            single_executor: false,
        })
    }
    /// Explicit experiment control mode. No chief/deputy or model-origin
    /// decisions exist: a user proposes/approves one task, the core assigns and
    /// accepts it through the same state machine and independent verification.
    /// The ordinary constructor and its OpenAI commander rule are unchanged.
    pub fn new_single_executor(principals: Vec<Principal>) -> Result<Self, String> {
        require(
            principals.len() == 3,
            "single executor requires three local bindings",
        )?;
        for expected in [Role::User, Role::Core, Role::Worker] {
            require(
                principals.iter().filter(|p| p.role == expected).count() == 1,
                "single executor requires one user, core and worker",
            )?;
        }
        require(
            principals.iter().all(|p| p.provider.is_none()),
            "single executor identities are local; worker provider belongs to approved selection",
        )?;
        let mut workflow = Self::new(principals)?;
        workflow.single_executor = true;
        Ok(workflow)
    }
    pub fn state(&self) -> &WorkflowState {
        &self.state
    }
    pub fn journal(&self) -> &[WorkflowEvent] {
        &self.journal
    }
    /// Trusted durable host only. The caller MUST authenticate the checkpoint,
    /// bind it to this executable/configuration, verify every original event byte,
    /// and recheck current permissions first. Never expose this through RPC.
    /// Principals and execution mode remain those of the validated constructor.
    pub fn resume_authenticated_checkpoint(
        &mut self,
        state: WorkflowState,
        journal: Vec<WorkflowEvent>,
    ) -> Result<(), String> {
        require(
            self.journal.is_empty(),
            "checkpoint requires an empty workflow",
        )?;
        for (i, event) in journal.iter().enumerate() {
            require(
                event.sequence as usize == i + 1 && self.principals.contains_key(&event.principal),
                "checkpoint journal mismatch",
            )?;
        }
        self.state = state;
        self.journal = journal;
        Ok(())
    }
    pub fn replay(principals: Vec<Principal>, journal: &[WorkflowEvent]) -> Result<Self, String> {
        let mut core = Self::new(principals)?;
        for event in journal {
            require(
                event.sequence as usize == core.journal.len() + 1,
                "workflow sequence gap",
            )?;
            core.apply(&event.principal, event.command.clone())?;
        }
        Ok(core)
    }
    /// Returns a restored pure state, including new recovery events. Durable host
    /// MUST commit those events before permitting any external operation.
    pub fn restore(
        principals: Vec<Principal>,
        journal: &[WorkflowEvent],
        core_principal: &str,
    ) -> Result<Self, String> {
        Self::replay(principals, journal)?.restore_current(core_principal)
    }
    /// Retains the mode of the already validated instance during recovery.
    pub fn restore_current(mut self, core_principal: &str) -> Result<Self, String> {
        let core = &mut self;
        // JS Object.entries uses array-index IDs first, then insertion order.
        // Preserve that prototype recovery-event order, including numeric task IDs.
        let mut tasks = core
            .state
            .plan
            .as_ref()
            .map(|p| p.tasks.clone())
            .unwrap_or_default();
        tasks.sort_by_key(|task| {
            let index = task
                .id
                .parse::<u32>()
                .ok()
                .filter(|n| *n != u32::MAX && n.to_string() == task.id);
            match index {
                Some(n) => (0, n),
                None => (1, 0),
            }
        });
        for task in tasks {
            let run = &core.state.tasks[&task.id];
            if matches!(
                run.state,
                TaskState::Dispatching | TaskState::Running | TaskState::Cancelling
            ) {
                let attempt = run.attempt.clone().ok_or("missing attempt")?;
                core.apply(
                    core_principal,
                    WorkflowCommand::RecoveryUnknown {
                        task: task.id,
                        attempt,
                    },
                )?;
            }
        }
        Ok(self)
    }
    pub fn apply(
        &mut self,
        principal_id: &str,
        command: WorkflowCommand,
    ) -> Result<WorkflowEvent, String> {
        let principal = self
            .principals
            .get(principal_id)
            .ok_or("unknown principal")?;
        let mut next = self.state.clone();
        self.transition(&mut next, principal, &command)?;
        let sequence =
            u32::try_from(self.journal.len() + 1).map_err(|_| "workflow sequence overflow")?;
        let event = WorkflowEvent {
            sequence,
            principal: principal_id.into(),
            command,
        };
        self.state = next;
        self.journal.push(event.clone());
        Ok(event)
    }
    fn transition(
        &self,
        state: &mut WorkflowState,
        principal: &Principal,
        cmd: &WorkflowCommand,
    ) -> Result<(), String> {
        if self.single_executor {
            require(
                !matches!(
                    cmd,
                    WorkflowCommand::Advise { .. }
                        | WorkflowCommand::RequestRevision { .. }
                        | WorkflowCommand::RetireSubmittedPlan { .. }
                        | WorkflowCommand::AuthorizePlanningBudget { .. }
                ),
                "single executor does not provide management, revisions or retries",
            )?;
        }
        if let WorkflowCommand::ConfirmHistoricalCharge { review_digest }
        | WorkflowCommand::ConfirmManagementCountResolution { review_digest }
        | WorkflowCommand::ConfirmApiGenerationResolution { review_digest }
        | WorkflowCommand::ConfirmApiCountResolution { review_digest } = cmd
        {
            role(principal, Role::User)?;
            crate::execution::sha256(review_digest)?;
            return Ok(());
        }
        if let WorkflowCommand::AuthorizePlanningBudget { setup_digest } = cmd {
            role(principal, Role::User)?;
            crate::execution::sha256(setup_digest)?;
            require(
                !state.approved || state.revision_requested.is_some(),
                "active execution approval must be revised before new planning permission",
            )?;
            return Ok(());
        }
        if let WorkflowCommand::RequestRevision { digest, reason }
        | WorkflowCommand::RetireSubmittedPlan { digest, reason } = cmd
        {
            let retire_submitted = matches!(cmd, WorkflowCommand::RetireSubmittedPlan { .. });
            role(principal, Role::User)?;
            text(reason)?;
            require(
                !reason.contains('\0') && reason.len() <= 8000,
                "invalid revision reason",
            )?;
            require(
                state.approved
                    && state.digest.as_ref() == Some(digest)
                    && state.revision_requested.is_none()
                    && !state.tasks.is_empty()
                    && state.tasks.values().all(|t| {
                        (retire_submitted && t.state == TaskState::Submitted)
                            || matches!(
                                t.state,
                                TaskState::Pending
                                    | TaskState::Accepted
                                    | TaskState::Cancelled
                                    | TaskState::Retired
                            )
                    }),
                "quiescent approved plan and fresh user revision request required",
            )?;
            require(
                !retire_submitted
                    || state
                        .tasks
                        .values()
                        .any(|t| t.state == TaskState::Submitted),
                "submitted plan retirement requires a submitted task",
            )?;
            state.revision_requested = Some(digest.clone());
            return Ok(());
        }
        if let WorkflowCommand::Propose { plan } = cmd {
            role(
                principal,
                if self.single_executor {
                    Role::User
                } else {
                    Role::Chief
                },
            )?;
            if self.single_executor {
                require(
                    state.plan.is_none()
                        && plan.version == 1
                        && plan.tasks.len() == 1
                        && plan.tasks[0].dependencies.is_empty(),
                    "single executor requires one initial dependency-free task",
                )?;
            }
            let terminal_revision = state.revision_requested.as_ref() == state.digest.as_ref()
                && state.revision_requested.is_some()
                && state.tasks.values().all(|t| {
                    matches!(
                        t.state,
                        TaskState::Pending
                            | TaskState::Accepted
                            | TaskState::Cancelled
                            | TaskState::Retired
                            | TaskState::Submitted
                    )
                });
            require(
                state.tasks.values().all(|t| t.state == TaskState::Pending) || terminal_revision,
                "cannot replace plan after assignment",
            )?;
            validate_plan(plan)?;
            if let Some(old) = &state.plan {
                require(
                    plan.id == old.id && plan.version > old.version,
                    "plan version must advance",
                )?;
            }
            state.plan = Some(plan.clone());
            state.digest = Some(workflow_digest(plan)?);
            state.approved = false;
            state.revision_requested = None;
            if terminal_revision {
                state.advice.clear();
            }
            state.tasks = plan
                .tasks
                .iter()
                .map(|t| {
                    (
                        t.id.clone(),
                        TaskRun {
                            state: TaskState::Pending,
                            worker: None,
                            attempt: None,
                            fence: if self.journal.iter().any(|event| {
                                matches!(event.command, WorkflowCommand::RequestRevision { .. } | WorkflowCommand::RetireSubmittedPlan { .. })
                            }) {
                                self.journal.iter().filter(|event| {
                                    matches!(&event.command, WorkflowCommand::Assign { task, .. } if task == &t.id)
                                }).count() as u32
                            } else {
                                0
                            },
                            evidence: vec![],
                            verified: false,
                            cancellation_requested: None,
                        },
                    )
                })
                .collect();
            return Ok(());
        }
        require(
            state.revision_requested.is_none(),
            "old plan is awaiting revision",
        )?;
        if let WorkflowCommand::Approve { digest } = cmd {
            role(principal, Role::User)?;
            require(
                state.plan.is_some() && state.digest.as_deref() == Some(digest),
                "approval does not match current plan",
            )?;
            state.approved = true;
            return Ok(());
        }
        require(state.approved && state.plan.is_some(), "plan not approved")?;
        let task_id = cmd.task().ok_or("unknown task")?;
        let run = state.tasks.get(task_id).ok_or("unknown task")?;
        let plan = state.plan.as_ref().ok_or("plan missing")?;
        let definition = plan
            .tasks
            .iter()
            .find(|t| t.id == task_id)
            .ok_or("unknown task")?;
        if let WorkflowCommand::Advise { suggestion, .. } = cmd {
            role(principal, Role::Deputy)?;
            text(suggestion)?;
            state.advice.push(Advice {
                task: task_id.into(),
                suggestion: suggestion.clone(),
            });
            return Ok(());
        }
        if let WorkflowCommand::Assign {
            worker, attempt, ..
        } = cmd
        {
            role(
                principal,
                if self.single_executor {
                    Role::Core
                } else {
                    Role::Deputy
                },
            )?;
            if self.single_executor {
                require(
                    !self
                        .journal
                        .iter()
                        .any(|e| matches!(e.command, WorkflowCommand::Assign { .. })),
                    "single executor attempt already reserved; do not retry",
                )?;
            }
            id(attempt)?;
            require(
                !state.tasks.values().any(|t| t.state == TaskState::Retired),
                "retired unknown attempt requires a revised and approved plan",
            )?;
            require(
                run.state == TaskState::Pending
                    && self.principals.get(worker).map(|p| p.role) == Some(Role::Worker),
                "invalid assignment",
            )?;
            require(
                !self.journal.iter().any(|event| {
                    matches!(&event.command, WorkflowCommand::Assign { attempt: used, .. } if used == attempt)
                }),
                "attempt already used",
            )?;
            require(
                definition
                    .dependencies
                    .iter()
                    .all(|d| state.tasks[d].state == TaskState::Accepted),
                "dependencies not accepted",
            )?;
            for other in &plan.tasks {
                if other.id == task_id || !state.tasks[&other.id].state.holds_resources() {
                    continue;
                }
                require(
                    other.workspace_key != definition.workspace_key,
                    "workspace is held",
                )?;
                let required = definition.resource_keys.as_deref().unwrap_or_default();
                require(
                    !other
                        .resource_keys
                        .as_deref()
                        .unwrap_or_default()
                        .iter()
                        .any(|k| required.contains(k)),
                    "shared resource is held",
                )?;
            }
            let run = state.tasks.get_mut(task_id).ok_or("unknown task")?;
            run.state = TaskState::Assigned;
            run.worker = Some(worker.clone());
            run.attempt = Some(attempt.clone());
            run.fence = run.fence.checked_add(1).ok_or("fence overflow")?;
            return Ok(());
        }
        require(cmd.attempt() == run.attempt.as_deref(), "stale attempt")?;
        let run = state.tasks.get_mut(task_id).ok_or("unknown task")?;
        match cmd {
            WorkflowCommand::RetireUnknownApi {
                attempt, reason, ..
            }
            | WorkflowCommand::RetireUnknownDesktop {
                attempt, reason, ..
            } => {
                if let WorkflowCommand::RetireUnknownDesktop {
                    evidence_digest, ..
                } = cmd
                {
                    crate::execution::sha256(evidence_digest)?;
                }
                role(principal, Role::User)?;
                text(reason)?;
                require(
                    !reason.contains('\0') && reason.len() <= 8000,
                    "invalid retirement reason",
                )?;
                require(
                    run.state == TaskState::Unknown,
                    "only an unknown attempt can be retired",
                )?;
                require(
                    self.journal.iter().any(|event| {
                        matches!(&event.command,
                    WorkflowCommand::DispatchIntent { attempt: sent, .. } if sent == attempt)
                    }),
                    "retirement requires original dispatch history",
                )?;
                // Retain identity, fence, files and uncertainty; this is no remote receipt.
                run.state = TaskState::Retired;
                run.verified = false;
            }
            WorkflowCommand::AbandonUnsent {
                attempt, reason, ..
            } => {
                role(principal, Role::User)?;
                text(reason)?;
                require(!reason.contains('\0'), "invalid abandonment reason")?;
                require(
                    run.state == TaskState::Assigned,
                    "only an assigned unsent attempt can be abandoned",
                )?;
                require(!self.journal.iter().any(|event| {
                    matches!(&event.command, WorkflowCommand::DispatchIntent { attempt: sent, .. } if sent == attempt)
                }), "attempt has dispatch history; reconciliation is required")?;
                run.state = TaskState::Pending;
                run.worker = None;
                run.attempt = None;
                run.evidence.clear();
                run.verified = false;
                run.cancellation_requested = None;
                // Retain the fence and immutable Assign event; any later worker
                // requires a fresh attempt ID and advances the existing fence.
            }
            WorkflowCommand::DispatchIntent { .. } => {
                role(principal, Role::Core)?;
                require(
                    run.state == TaskState::Assigned,
                    "cannot redispatch unresolved attempt",
                )?;
                run.state = TaskState::Dispatching;
            }
            WorkflowCommand::Started { .. } => {
                role(principal, Role::Core)?;
                require(run.state == TaskState::Dispatching, "invalid start")?;
                run.state = TaskState::Running;
            }
            WorkflowCommand::CancelIntent { reason, .. } => {
                role(principal, Role::Core)?;
                text(reason)?;
                require(
                    matches!(run.state, TaskState::Running | TaskState::Unknown)
                        && run.cancellation_requested != Some(true),
                    "cancellation unresolved or not running",
                )?;
                run.cancellation_requested = Some(true);
                run.state = TaskState::Cancelling;
            }
            WorkflowCommand::RecoveryUnknown { .. } => {
                role(principal, Role::Core)?;
                require(
                    matches!(
                        run.state,
                        TaskState::Dispatching | TaskState::Running | TaskState::Cancelling
                    ),
                    "invalid recovery",
                )?;
                run.state = TaskState::Unknown;
            }
            WorkflowCommand::Reconcile {
                outcome,
                evidence: proof,
                ..
            } => {
                role(principal, Role::Core)?;
                require(run.state == TaskState::Unknown, "invalid reconciliation")?;
                require(
                    !(run.cancellation_requested == Some(true)
                        && *outcome == ReconcileOutcome::NotStarted),
                    "cancellation requires terminal evidence; no redispatch",
                )?;
                evidence(proof)?;
                run.evidence = proof.clone();
                run.state = match outcome {
                    ReconcileOutcome::NotStarted => TaskState::Assigned,
                    ReconcileOutcome::Running if run.cancellation_requested == Some(true) => {
                        TaskState::Cancelling
                    }
                    ReconcileOutcome::Running => TaskState::Running,
                    ReconcileOutcome::Submitted => TaskState::Submitted,
                    ReconcileOutcome::Cancelled => TaskState::Cancelled,
                };
            }
            WorkflowCommand::Submit {
                summary,
                evidence: proof,
                ..
            } => {
                role(principal, Role::Worker)?;
                require(
                    run.worker.as_deref() == Some(&principal.id) && run.state == TaskState::Running,
                    "worker not bound to active attempt",
                )?;
                text(summary)?;
                if let Some(p) = proof {
                    evidence(p)?;
                }
                run.evidence = proof.clone().unwrap_or_default();
                run.state = TaskState::Submitted;
            }
            WorkflowCommand::Verify {
                passed,
                evidence: proof,
                ..
            } => {
                role(principal, Role::Core)?;
                require(run.state == TaskState::Submitted, "invalid verification")?;
                evidence(proof)?;
                run.verified = *passed;
                run.evidence = proof.clone();
            }
            WorkflowCommand::Accept { .. } => {
                role(
                    principal,
                    if self.single_executor {
                        Role::Core
                    } else {
                        Role::Chief
                    },
                )?;
                require(
                    run.state == TaskState::Submitted && run.verified,
                    "verification required",
                )?;
                run.state = TaskState::Accepted;
            }
            _ => return Err("unknown command".into()),
        }
        Ok(())
    }
}
