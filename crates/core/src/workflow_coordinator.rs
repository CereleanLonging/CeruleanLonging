//! One owned journal writer. API workers hold scoped clients, never the file lock.
//! Network requests and workspace preparation execute outside this thread.
use crate::{
    scheduler::{AcceptedDependencyInput, CommandContext, DurableWorkflow, Entry, SchedulerConfig},
    workflow_access::WorkflowAccess,
};
use cerulean_domain::{
    execution::*,
    workflow::{WorkflowCommand, WorkflowState},
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc,
    },
    thread,
    time::Duration,
};
#[path = "coordinated_verification.rs"]
mod verification;
pub(crate) use verification::VerificationClient;
#[path = "coordinated_acceptance.rs"]
mod acceptance;
#[path = "coordinated_codex.rs"]
mod codex;
#[path = "coordinated_omp.rs"]
mod omp;

#[derive(Clone)]
pub(crate) struct Scope {
    pub project: String,
    pub task: String,
    pub attempt: String,
    pub revision: String,
}
impl Scope {
    fn validate(&self, store: &DurableWorkflow) -> Result<(), String> {
        identifier(&self.attempt)?;
        let plan = store.revision().ok_or("协调器缺少原批准方案")?;
        require(
            plan.project.id == self.project && plan.approval_digest()? == self.revision,
            "协调器项目或修订身份变化",
        )?;
        require(
            store
                .approval()
                .is_some_and(|a| a.revision_digest == self.revision),
            "协调器原方案尚未批准",
        )?;
        plan.task(&self.task)?;
        Ok(())
    }
    fn command(&self, command: &WorkflowCommand) -> Result<(), String> {
        require(
            command.task() == Some(self.task.as_str())
                && command.attempt() == Some(self.attempt.as_str()),
            "协调器拒绝跨任务或跨尝试命令",
        )?;
        require(
            matches!(
                command,
                WorkflowCommand::Assign { .. }
                    | WorkflowCommand::DispatchIntent { .. }
                    | WorkflowCommand::Started { .. }
                    | WorkflowCommand::Submit { .. }
                    | WorkflowCommand::RecoveryUnknown { .. }
                    | WorkflowCommand::Reconcile { .. }
            ),
            "API工作线程不能批准、验收或修订方案",
        )
    }
}
#[derive(Clone)]
struct Cursor {
    count: usize,
    hash: Option<String>,
}
impl Cursor {
    fn validate(&self, store: &DurableWorkflow) -> Result<(), String> {
        require(
            self.count <= store.history().len()
                && self.hash.as_deref()
                    == self
                        .count
                        .checked_sub(1)
                        .and_then(|i| store.history().get(i))
                        .map(|e| e.hash.as_str()),
            "协调器历史前缀不匹配",
        )
    }
}
struct Packet {
    snapshot: ExecutionSnapshot,
    tail: Vec<Entry>,
    entry: Option<Entry>,
    request_limit: Option<u32>,
}
fn packet(store: &DurableWorkflow, cursor: &Cursor, entry: Option<Entry>) -> Packet {
    Packet {
        snapshot: crate::async_verification::snapshot(store),
        tail: store.history()[cursor.count..].to_vec(),
        entry,
        request_limit: None,
    }
}
enum Message {
    TimingPrefix(mpsc::Sender<Result<(usize, Option<String>), String>>),
    ApiRequestLimit {
        scope: Scope,
        reply: mpsc::Sender<Result<u32, String>>,
    },
    OmpPaths {
        scope: Scope,
        reply: mpsc::Sender<Result<(PathBuf, PathBuf), String>>,
    },
    ApplyCodex {
        scope: Scope,
        cursor: Cursor,
        operation: String,
        principal: String,
        require_new: bool,
        command: WorkflowCommand,
        context: Box<CommandContext>,
        reply: mpsc::Sender<Result<Packet, String>>,
    },
    CodexCancellationAnchors {
        scope: Scope,
        reply: mpsc::Sender<Result<StartedCancellationAnchors, String>>,
    },
    ApplyOmp {
        scope: Scope,
        cursor: Cursor,
        operation: Option<String>,
        principal: String,
        require_new: bool,
        command: WorkflowCommand,
        context: Box<CommandContext>,
        reply: mpsc::Sender<Result<Packet, String>>,
    },
    ManagementPublication {
        request: Box<acceptance::ManagementPublication>,
        reply: mpsc::Sender<Result<Entry, String>>,
    },
    BeginVerification {
        scope: Scope,
        job: VerificationJob,
        reply: mpsc::Sender<
            Result<
                (
                    crate::scheduler::PreparedVerification,
                    crate::verification_detail::Basis,
                ),
                String,
            >,
        >,
    },
    FinishVerification {
        scope: Scope,
        cursor: Cursor,
        prepared: Box<crate::scheduler::PreparedVerification>,
        report: Box<VerificationReport>,
        reply: mpsc::Sender<Result<Packet, String>>,
    },
    Read {
        scope: Scope,
        cursor: Cursor,
        reply: mpsc::Sender<Result<Packet, String>>,
    },
    Apply {
        scoped_dispatch: Option<Box<crate::api_currency::DispatchPermit>>,
        scope: Scope,
        cursor: Cursor,
        operation: Option<String>,
        principal: String,
        require_new: bool,
        command: WorkflowCommand,
        context: Box<CommandContext>,
        reply: mpsc::Sender<Result<Packet, String>>,
    },
    Prepare {
        scope: Scope,
        reply: mpsc::Sender<Result<(PlanRevision, Vec<AcceptedDependencyInput>), String>>,
    },
    Inspect(mpsc::Sender<Result<ExecutionSnapshot, String>>),
    History(
        cerulean_domain::workspace::WorkspaceQuery,
        mpsc::Sender<Result<cerulean_domain::workspace::WorkspaceResult, String>>,
    ),
    WorkspaceObservationBasis {
        job: cerulean_domain::CodexJob,
        trace: cerulean_domain::worker_observation::WorkerObservationTrace,
        reply: mpsc::Sender<Result<crate::host::workspace_observation::OriginalBasis, String>>,
    },
    WorkspaceVerificationBasis {
        job: cerulean_domain::execution::VerificationJob,
        reply: mpsc::Sender<Result<crate::host::workspace_verification::OriginalBasis, String>>,
    },
    WorkspaceManagementBasis {
        project_id: String,
        reply: mpsc::Sender<
            Result<crate::host::workspace_host::management_decisions::OriginalBasis, String>,
        >,
    },
    WorkspaceProjectRunsBasis {
        project_id: String,
        reply:
            mpsc::Sender<Result<crate::host::workspace_host::project_runs::OriginalBasis, String>>,
    },
    WorkspaceRevisionReviewsBasis {
        project_id: String,
        reply: mpsc::Sender<
            Result<crate::host::workspace_host::revision_reviews::OriginalBasis, String>,
        >,
    },
    WorkspaceApiPublicationReviewsBasis {
        project_id: String,
        reply: mpsc::Sender<Result<String, String>>,
    },
    WorkspaceUnsentAssignmentsBasis {
        project_id: String,
        reply: mpsc::Sender<
            Result<crate::host::workspace_host::unsent_assignments::OriginalBasis, String>,
        >,
    },
    WorkspaceCodexCancellationBasis {
        project_id: String,
        jobs: Vec<cerulean_domain::CodexJob>,
        reply: mpsc::Sender<
            Result<crate::host::workspace_host::codex_cancellation::OriginalBasis, String>,
        >,
    },
    WorkspaceStartupBasis {
        records: Vec<crate::startup_journal::StartupRecord>,
        jobs: Vec<cerulean_domain::CodexJob>,
        management: crate::host::startup_management_proof::StartupManagementProof,
        reply: mpsc::Sender<Result<crate::host::startup_recovery::OriginalBasis, String>>,
    },
    Finish(mpsc::Sender<(DurableWorkflow, bool)>),
    Stop,
    #[cfg(test)]
    InterruptNextPublication,
    #[cfg(test)]
    LoseNextAcknowledgement,
}
pub(crate) struct WorkflowCoordinator {
    pub(crate) derived_diagnostics: crate::derived_index::Diagnostics,
    sender: mpsc::Sender<Message>,
    thread: Option<thread::JoinHandle<()>>,
    clients: Arc<AtomicUsize>,
    config: SchedulerConfig,
    path: PathBuf,
}
pub(crate) struct WorkflowClient {
    kind: ClientKind,
    sender: mpsc::Sender<Message>,
    clients: Arc<AtomicUsize>,
    scope: Scope,
    config: SchedulerConfig,
    path: PathBuf,
    snapshot: ExecutionSnapshot,
    history: Vec<Entry>,
    failed: bool,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum ClientKind {
    Api,
    Omp,
    Codex,
}
fn receive<T>(receiver: mpsc::Receiver<T>) -> Result<T, String> {
    receiver
        .recv_timeout(Duration::from_secs(30))
        .map_err(|_| "协调器回执丢失或超时；不得自动重发".into())
}
impl WorkflowCoordinator {
    pub(crate) fn configuration(&self) -> &SchedulerConfig {
        &self.config
    }

    pub(crate) fn start(mut store: DurableWorkflow, path: PathBuf) -> Result<Self, String> {
        let config = store.configuration().clone();
        let derived_diagnostics = store.derived_diagnostics();
        let (sender, receiver) = mpsc::channel();
        let thread=thread::Builder::new().name("workflow-writer".into()).spawn(move || {
            let mut fault=false;
              let mut api_request_limits = std::collections::BTreeMap::<String, u32>::new();
            #[cfg(test)] let mut interrupt=false;
            #[cfg(test)] let mut lose_ack=false;
            while let Ok(message)=receiver.recv() {
                match message {
                      Message::ApiRequestLimit { scope, reply } => {
                          let result = (|| {
                              require(!fault && !store.requires_reopen(), "coordinator requires recovery")?;
                              scope.validate(&store)?;
                              api_request_limits.get(&scope.attempt).copied().ok_or("original live API request grant missing".into())
                          })();
                          let _ = reply.send(result);
                      }
                    Message::OmpPaths {scope,reply} => {
                        let result=(|| {
                            require(!fault && !store.requires_reopen(),"协调器账本需要重开核对")?;
                            omp::validate(&scope,&store)?;
                            Ok((store.omp_attempt_path(&scope.attempt)?,store.omp_file_service_path(&scope.attempt)?))
                        })();
                        let _=reply.send(result);
                    }
                    Message::CodexCancellationAnchors {scope,reply} => {
                        let result=(|| {
                            require(!fault && !store.requires_reopen(),"协调器账本需要重开核对")?;
                            codex::validate(&scope,&store)?;
                            store.started_cancellation_anchors(&scope.task,&scope.attempt)
                        })();
                        let _=reply.send(result);
                    }
                    Message::ApplyCodex {scope,cursor,operation,principal,require_new,command,context,reply} => {
                        let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            require(!fault && !store.requires_reopen(),"协调器账本需要重开核对")?;
                            codex::validate(&scope,&store)?;cursor.validate(&store)?;
                            codex::command(&scope,&store,&operation,&command,&context)?;
                            #[cfg(test)] let entry=store.command_observed(&operation,&principal,command,*context,require_new,|point| {
                                if interrupt && matches!(point,crate::scheduler::CommitPoint::EventPublished) {
                                    interrupt=false;panic!("controlled Codex coordinator interruption after publication");
                                }
                            })?;
                            #[cfg(not(test))] let entry=store.command(&operation,&principal,command,*context,require_new)?;
                            Ok(packet(&store,&cursor,Some(entry)))
                        }));
                        let result=match result {Ok(result)=>result,Err(_)=>{fault=true;Err("Codex协调器写入中断，须核对原事件".into())}};
                        #[cfg(test)] if lose_ack {lose_ack=false;drop(reply);continue;}
                        let _=reply.send(result);
                    }
                    Message::ApplyOmp {scope,cursor,operation,principal,require_new,command,context,reply} => {
                        let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            require(!fault && !store.requires_reopen(),"协调器账本需要重开核对")?;
                            omp::validate(&scope,&store)?;cursor.validate(&store)?;
                            omp::command(&scope,&store,&command,&context)?;
                            let operation=operation.unwrap_or_else(||format!("omp-event-{}",store.history().len()+1));
                            #[cfg(test)] let entry=store.command_observed(&operation,&principal,command,*context,require_new,|point| {
                                if interrupt && matches!(point,crate::scheduler::CommitPoint::EventPublished) {
                                    interrupt=false;panic!("controlled OMP coordinator interruption after publication");
                                }
                            })?;
                            #[cfg(not(test))] let entry=store.command(&operation,&principal,command,*context,require_new)?;
                            Ok(packet(&store,&cursor,Some(entry)))
                        }));
                        let result=match result {Ok(result)=>result,Err(_)=>{fault=true;Err("OMP协调器写入中断，须核对原事件".into())}};
                        #[cfg(test)] if lose_ack {lose_ack=false;drop(reply);continue;}
                        let _=reply.send(result);
                    }
                    Message::ManagementPublication {request,reply} => {
                        let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            require(!fault && !store.requires_reopen(), "协调器账本需要重开核对")?;
                            request.publish(&mut store)
                        }));
                        let result=match result {Ok(result)=>result,Err(_)=>{fault=true;Err("管理发布入账中断，须核对原应用事务".into())}};
                        #[cfg(test)] if lose_ack {lose_ack=false;drop(reply);continue;}
                        let _=reply.send(result);
                    }
                    Message::BeginVerification {scope,job,reply} => {
                        let result=(|| {
                            require(!fault && !store.requires_reopen(), "协调器账本需要重开核对")?;
                            scope.validate(&store)?;
                            require(job.project_id==scope.project && job.task_id==scope.task && job.attempt_id==scope.attempt, "verification job scope mismatch")?;
                            let prepared=store.prepare_verification(&job.operation_id,&scope.task,&scope.attempt)?;
                            Ok((prepared,crate::verification_detail::basis(&store,&job)?))
                        })();
                        let _=reply.send(result);
                    }
                    Message::FinishVerification {scope,cursor,prepared,report,reply} => {
                        let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            require(!fault && !store.requires_reopen(), "协调器账本需要重开核对")?;
                            scope.validate(&store)?;cursor.validate(&store)?;
                            prepared.require_scope(&scope.project,&scope.task,&scope.attempt,&scope.revision)?;
                            let entry=store.finish_prepared_verification(&prepared,*report)?;
                            Ok(packet(&store,&cursor,Some(entry)))
                        }));
                        let result=match result {Ok(result)=>result,Err(_)=>{fault=true;Err("独立验收入账中断，须核对原证据".into())}};
                        #[cfg(test)] if lose_ack {lose_ack=false;drop(reply);continue;}
                        let _=reply.send(result);
                    }
                    Message::Finish(reply) => {let _=reply.send((store,fault));return;}
                    Message::Stop => return,
                    Message::Inspect(reply) => {let _=reply.send(if fault || store.requires_reopen() {Err("协调器账本需要重开核对".into())} else {Ok(crate::async_verification::snapshot(&store))});}
                    Message::TimingPrefix(reply) => {let _=reply.send(if fault || store.requires_reopen() {Err("协调器账本需要重开核对".into())} else {Ok((store.history().len(),store.history().last().map(|e|e.hash.clone())))});}
                    Message::History(query,reply) => {
                        let result = (|| { require(!fault && !store.requires_reopen(), "协调器账本需要重开核对")?; crate::workspace_history::query(&store, &query) })();
                        let _ = reply.send(result);
                    }
                    Message::WorkspaceObservationBasis {job,trace,reply} => {
                        let result = (|| {
                            require(!fault && !store.requires_reopen(), "协调器账本需要重开核对")?;
                            crate::host::workspace_observation::original_basis(&store, &job, &trace)
                        })();
                        let _ = reply.send(result);
                    }
                    Message::WorkspaceVerificationBasis {job,reply} => {
                        let result = (|| {
                            require(!fault && !store.requires_reopen(), "协调器账本需要重开核对")?;
                            crate::host::workspace_verification::original_basis(&store, &job)
                        })();
                        let _ = reply.send(result);
                    }
                    Message::WorkspaceManagementBasis {project_id,reply} => {
                        let result = (|| {
                            require(!fault && !store.requires_reopen(), "协调器账本需要重开核对")?;
                            crate::host::workspace_host::management_decisions::original_basis(&store, &project_id)
                        })();
                        let _ = reply.send(result);
                    }
                    Message::WorkspaceProjectRunsBasis { project_id, reply } => {
                        let result = (|| {
                            require(!fault && !store.requires_reopen(), "协调器账本需要重开核对")?;
                            crate::host::workspace_host::project_runs::original_basis(
                                &store,
                                &project_id,
                            )
                        })();
                        let _ = reply.send(result);
                    }
                    Message::WorkspaceRevisionReviewsBasis { project_id, reply } => {
                        let result = (|| {
                            require(!fault && !store.requires_reopen(), "协调器账本需要重开核对")?;
                            crate::host::workspace_host::revision_reviews::original_basis(&store, &project_id)
                        })();
                        let _ = reply.send(result);
                    }
                    Message::WorkspaceApiPublicationReviewsBasis { project_id, reply } => {
                        let result = (|| {
                            require(!fault && !store.requires_reopen(), "协调器账本需要重开核对")?;
                            crate::host::workspace_host::api_publication_reviews::original_basis(&store, &project_id)
                        })();
                        let _ = reply.send(result);
                    }
                    Message::WorkspaceUnsentAssignmentsBasis { project_id, reply } => {
                        let result = (|| {
                            require(!fault && !store.requires_reopen(), "协调器账本需要重开核对")?;
                            crate::host::workspace_host::unsent_assignments::original_basis(&store, &project_id)
                        })();
                        let _ = reply.send(result);
                    }
                    Message::WorkspaceCodexCancellationBasis { project_id, jobs, reply } => {
                        let result = (|| {
                            require(!fault && !store.requires_reopen(), "协调器账本需要重开核对")?;
                            crate::host::workspace_host::codex_cancellation::original_basis(&store, &project_id, &jobs)
                        })();
                        let _ = reply.send(result);
                    }
                    Message::WorkspaceStartupBasis { records, jobs, management, reply } => {
                        let result = (|| {
                            require(!fault && !store.requires_reopen(), "协调器账本需要重开核对")?;
                            crate::host::startup_recovery::inspect_workflow_startup(
                                &store,
                                &records,
                                &jobs,
                                &management,
                            )
                        })();
                        let _ = reply.send(result);
                    }
                    Message::Read {scope,cursor,reply} => {
                        let result=(|| {require(!fault && !store.requires_reopen(),"协调器账本需要重开核对")?;scope.validate(&store)?;cursor.validate(&store)?;Ok(packet(&store,&cursor,None))})();
                        let _=reply.send(result);
                    }
                    Message::Prepare {scope,reply} => {
                        let result=(|| {require(!fault && !store.requires_reopen(),"协调器账本需要重开核对")?;scope.validate(&store)?;store.workspace_preparation(&scope.task,&scope.attempt)})();
                        let _=reply.send(result);
                    }
                    Message::Apply {scope,cursor,operation,principal,require_new,command,context,reply,scoped_dispatch} => {
                        let result=std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            require(!fault && !store.requires_reopen(),"协调器账本需要重开核对")?;
                            scope.validate(&store)?;scope.command(&command)?;cursor.validate(&store)?;
                            // Every DispatchIntent uses the authoritative writer history;
                            // generic command submission cannot bypass the budget gate.
                            let request_limit = match &command {
                                WorkflowCommand::DispatchIntent { task, attempt } => Some(match scoped_dispatch {
                                    Some(permit) => crate::api_execution::dispatch_scoped_reservation(&store, task, attempt, *permit)?,
                                    None => crate::api_execution::dispatch_reservation(&store, task, attempt)?,
                                }),
                                _ => { require(scoped_dispatch.is_none(), "scoped dispatch permit cannot authorize another command")?; None },
                            };
                            let operation=operation.unwrap_or_else(||format!("api-event-{}",store.history().len()+1));
                            #[cfg(test)] let entry=store.command_observed(&operation,&principal,command,*context,require_new,|point| {
                                if interrupt && matches!(point,crate::scheduler::CommitPoint::EventPublished) {
                                    interrupt=false;panic!("controlled coordinator interruption after event publication");
                                }
                            })?;
                            #[cfg(not(test))] let entry=store.command(&operation,&principal,command,*context,require_new)?;
                            let mut result = packet(&store,&cursor,Some(entry));
                            if let Some(limit) = request_limit {
                                api_request_limits.insert(scope.attempt.clone(), limit);
                            }
                            result.request_limit = request_limit;
                            Ok(result)
                        }));
                        let result=match result {Ok(result)=>result,Err(_)=>{fault=true;Err("协调器写入中断；保留原事件并重开核对".into())}};
                        #[cfg(test)] if lose_ack {lose_ack=false;drop(reply);continue;}
                        let _=reply.send(result);
                    }
                    #[cfg(test)] Message::InterruptNextPublication => interrupt=true,
                    #[cfg(test)] Message::LoseNextAcknowledgement => lose_ack=true,
                }
            }
        }).map_err(|_|"无法启动账本协调器")?;
        Ok(Self {
            derived_diagnostics,
            sender,
            thread: Some(thread),
            clients: Arc::new(AtomicUsize::new(0)),
            config,
            path,
        })
    }
    pub(crate) fn client(&self, scope: Scope) -> Result<WorkflowClient, String> {
        let (reply, response) = mpsc::channel();
        self.sender
            .send(Message::Read {
                scope: scope.clone(),
                cursor: Cursor {
                    count: 0,
                    hash: None,
                },
                reply,
            })
            .map_err(|_| "协调器已停止")?;
        let initial = receive(response)??;
        self.clients.fetch_add(1, Ordering::AcqRel);
        Ok(WorkflowClient {
            kind: ClientKind::Api,
            sender: self.sender.clone(),
            clients: self.clients.clone(),
            scope,
            config: self.config.clone(),
            path: self.path.clone(),
            snapshot: initial.snapshot,
            history: initial.tail,
            failed: false,
        })
    }
    pub(crate) fn snapshot(&self) -> Result<ExecutionSnapshot, String> {
        let (reply, response) = mpsc::channel();
        self.sender
            .send(Message::Inspect(reply))
            .map_err(|_| "协调器已停止")?;
        receive(response)?
    }
    /// One atomic read from the existing writer; never a second journal reader/writer.
    pub(crate) fn timing_prefix(&self) -> Result<(usize, Option<String>), String> {
        let (reply, response) = mpsc::channel();
        self.sender
            .send(Message::TimingPrefix(reply))
            .map_err(|_| "原协调器已停止")?;
        receive(response)?
    }
    pub(crate) fn api_request_limit(&self, scope: Scope) -> Result<u32, String> {
        let (reply, response) = mpsc::channel();
        self.sender
            .send(Message::ApiRequestLimit { scope, reply })
            .map_err(|_| "original API coordinator unavailable")?;
        receive(response)?
    }
    pub(crate) fn workspace_history(
        &self,
        query: cerulean_domain::workspace::WorkspaceQuery,
    ) -> Result<cerulean_domain::workspace::WorkspaceResult, String> {
        let (reply, response) = mpsc::channel();
        self.sender
            .send(Message::History(query, reply))
            .map_err(|_| "协调器已停止")?;
        receive(response)?
    }
    pub(crate) fn workspace_observation_basis(
        &self,
        job: &cerulean_domain::CodexJob,
        trace: cerulean_domain::worker_observation::WorkerObservationTrace,
    ) -> Result<crate::host::workspace_observation::OriginalBasis, String> {
        let (reply, response) = mpsc::channel();
        self.sender
            .send(Message::WorkspaceObservationBasis {
                job: job.clone(),
                trace,
                reply,
            })
            .map_err(|_| "协调器已停止")?;
        receive(response)?
    }
    pub(crate) fn workspace_verification_basis(
        &self,
        job: &cerulean_domain::execution::VerificationJob,
    ) -> Result<crate::host::workspace_verification::OriginalBasis, String> {
        let (reply, response) = mpsc::channel();
        self.sender
            .send(Message::WorkspaceVerificationBasis {
                job: job.clone(),
                reply,
            })
            .map_err(|_| "协调器已停止")?;
        receive(response)?
    }
    pub(crate) fn workspace_management_basis(
        &self,
        project_id: &str,
    ) -> Result<crate::host::workspace_host::management_decisions::OriginalBasis, String> {
        let (reply, response) = mpsc::channel();
        self.sender
            .send(Message::WorkspaceManagementBasis {
                project_id: project_id.into(),
                reply,
            })
            .map_err(|_| "协调器已停止")?;
        receive(response)?
    }
    pub(crate) fn workspace_project_runs_basis(
        &self,
        project_id: &str,
    ) -> Result<crate::host::workspace_host::project_runs::OriginalBasis, String> {
        let (reply, response) = mpsc::channel();
        self.sender
            .send(Message::WorkspaceProjectRunsBasis {
                project_id: project_id.into(),
                reply,
            })
            .map_err(|_| "协调器已停止")?;
        receive(response)?
    }
    pub(crate) fn workspace_revision_reviews_basis(
        &self,
        project_id: &str,
    ) -> Result<crate::host::workspace_host::revision_reviews::OriginalBasis, String> {
        let (reply, response) = mpsc::channel();
        self.sender
            .send(Message::WorkspaceRevisionReviewsBasis {
                project_id: project_id.into(),
                reply,
            })
            .map_err(|_| "协调器已停止")?;
        receive(response)?
    }
    pub(crate) fn workspace_api_publication_reviews_basis(
        &self,
        project_id: &str,
    ) -> Result<String, String> {
        let (reply, response) = mpsc::channel();
        self.sender
            .send(Message::WorkspaceApiPublicationReviewsBasis {
                project_id: project_id.into(),
                reply,
            })
            .map_err(|_| "协调器已停止")?;
        receive(response)?
    }
    pub(crate) fn workspace_unsent_assignments_basis(
        &self,
        project_id: &str,
    ) -> Result<crate::host::workspace_host::unsent_assignments::OriginalBasis, String> {
        let (reply, response) = mpsc::channel();
        self.sender
            .send(Message::WorkspaceUnsentAssignmentsBasis {
                project_id: project_id.into(),
                reply,
            })
            .map_err(|_| "协调器已停止")?;
        receive(response)?
    }
    pub(crate) fn workspace_codex_cancellation_basis(
        &self,
        project_id: &str,
        jobs: Vec<cerulean_domain::CodexJob>,
    ) -> Result<crate::host::workspace_host::codex_cancellation::OriginalBasis, String> {
        let (reply, response) = mpsc::channel();
        self.sender
            .send(Message::WorkspaceCodexCancellationBasis {
                project_id: project_id.into(),
                jobs,
                reply,
            })
            .map_err(|_| "协调器已停止")?;
        receive(response)?
    }
    pub(crate) fn workspace_startup_basis(
        &self,
        records: Vec<crate::startup_journal::StartupRecord>,
        jobs: Vec<cerulean_domain::CodexJob>,
        management: crate::host::startup_management_proof::StartupManagementProof,
    ) -> Result<crate::host::startup_recovery::OriginalBasis, String> {
        let (reply, response) = mpsc::channel();
        self.sender
            .send(Message::WorkspaceStartupBasis {
                records,
                jobs,
                management,
                reply,
            })
            .map_err(|_| "协调器已停止")?;
        receive(response)?
    }
    #[cfg(test)]
    pub(crate) fn interrupt_next_publication(&self) {
        self.sender.send(Message::InterruptNextPublication).unwrap();
    }
    #[cfg(test)]
    pub(crate) fn lose_next_acknowledgement(&self) {
        self.sender.send(Message::LoseNextAcknowledgement).unwrap();
    }
    pub(crate) fn finish(&mut self, reopen: bool) -> Result<DurableWorkflow, String> {
        require(
            self.clients.load(Ordering::Acquire) == 0,
            "仍有尝试持有协调器，不能转移账本",
        )?;
        let (reply, response) = mpsc::channel();
        self.sender
            .send(Message::Finish(reply))
            .map_err(|_| "协调器已停止，须重开原账本")?;
        let (store, interrupted) = receive(response)?;
        if let Some(thread) = self.thread.take() {
            thread.join().map_err(|_| "协调器线程异常")?;
        }
        if reopen || interrupted || store.requires_reopen() {
            drop(store);
            DurableWorkflow::open(&self.path, self.config.clone())
        } else {
            Ok(store)
        }
    }
}
impl Drop for WorkflowCoordinator {
    fn drop(&mut self) {
        let _ = self.sender.send(Message::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
impl Drop for WorkflowClient {
    fn drop(&mut self) {
        self.clients.fetch_sub(1, Ordering::AcqRel);
    }
}
impl WorkflowClient {
    fn cursor(&self) -> Cursor {
        Cursor {
            count: self.history.len(),
            hash: self.history.last().map(|e| e.hash.clone()),
        }
    }
    fn absorb(
        &mut self,
        response: mpsc::Receiver<Result<Packet, String>>,
    ) -> Result<(Option<Entry>, Option<u32>), String> {
        let result = receive(response).and_then(|r| r);
        match result {
            Ok(packet) => {
                self.history.extend(packet.tail);
                self.snapshot = packet.snapshot;
                Ok((packet.entry, packet.request_limit))
            }
            Err(error) => {
                self.failed = true;
                Err(error)
            }
        }
    }
    fn apply(
        &mut self,
        operation: Option<String>,
        principal: &str,
        require_new: bool,
        command: WorkflowCommand,
        context: CommandContext,
    ) -> Result<(Entry, Option<u32>), String> {
        require(!self.failed, "协调器客户端已失去确认，禁止继续命令")?;
        let (reply, response) = mpsc::channel();
        let message = if self.kind == ClientKind::Codex {
            Message::ApplyCodex {
                scope: self.scope.clone(),
                cursor: self.cursor(),
                operation: operation.ok_or("Codex命令必须保留原操作ID")?,
                principal: principal.into(),
                require_new,
                command,
                context: Box::new(context),
                reply,
            }
        } else if self.kind == ClientKind::Omp {
            Message::ApplyOmp {
                scope: self.scope.clone(),
                cursor: self.cursor(),
                operation,
                principal: principal.into(),
                require_new,
                command,
                context: Box::new(context),
                reply,
            }
        } else {
            Message::Apply {
                scoped_dispatch: None,
                scope: self.scope.clone(),
                cursor: self.cursor(),
                operation,
                principal: principal.into(),
                require_new,
                command,
                context: Box::new(context),
                reply,
            }
        };
        self.sender.send(message).map_err(|_| "协调器不可用")?;
        let (entry, limit) = self.absorb(response)?;
        Ok((entry.ok_or("协调器缺少已发布事件")?, limit))
    }
    pub(crate) fn prepare_task_workspace_cancellable(
        &mut self,
        task: &str,
        attempt: &str,
        cancel: Arc<AtomicBool>,
    ) -> Result<(String, WorkspaceProof), String> {
        require(
            !self.failed && task == self.scope.task && attempt == self.scope.attempt,
            "工作目录准备超出原尝试",
        )?;
        let (reply, response) = mpsc::channel();
        self.sender
            .send(Message::Prepare {
                scope: self.scope.clone(),
                reply,
            })
            .map_err(|_| "协调器不可用")?;
        let result = receive(response).and_then(|r| r);
        let (plan, mut inputs) = match result {
            Ok(value) => value,
            Err(error) => {
                self.failed = true;
                return Err(error);
            }
        };
        crate::git_workspaces::prepare_approved_cancellable(
            &plan,
            task,
            attempt,
            cancel,
            &mut inputs,
        )
    }
}
impl WorkflowAccess for WorkflowClient {
    fn configuration(&self) -> &SchedulerConfig {
        &self.config
    }
    fn state(&self) -> &WorkflowState {
        &self.snapshot.state
    }
    fn history(&self) -> &[Entry] {
        &self.history
    }
    fn revision(&self) -> Option<&PlanRevision> {
        self.snapshot.revision.as_ref()
    }
    fn approval(&self) -> Option<&Approval> {
        self.snapshot.approval.as_ref()
    }
    fn api_attempt_path(&self, attempt: &str) -> Result<PathBuf, String> {
        identifier(attempt)?;
        Ok(self.path.join(format!("api-attempt-{}", digest(&attempt)?)))
    }
    fn refresh(&mut self) -> Result<(), String> {
        require(!self.failed, "协调器客户端已失去确认，禁止继续命令")?;
        let (reply, response) = mpsc::channel();
        self.sender
            .send(Message::Read {
                scope: self.scope.clone(),
                cursor: self.cursor(),
                reply,
            })
            .map_err(|_| "协调器不可用")?;
        self.absorb(response).map(|_| ())
    }
    fn command(
        &mut self,
        operation: &str,
        principal: &str,
        command: WorkflowCommand,
        context: CommandContext,
        require_new: bool,
    ) -> Result<Entry, String> {
        self.apply(
            Some(operation.into()),
            principal,
            require_new,
            command,
            context,
        )
        .map(|(entry, _)| entry)
    }
    fn begin_api_dispatch(&mut self, task: &str, attempt: &str) -> Result<u32, String> {
        require(self.kind == ClientKind::Api, "OMP客户端不能使用API派发能力")?;
        let (principal, command, context) =
            crate::api_execution::dispatch_command(self, task, attempt)?;
        let (_, limit) = self.apply(None, &principal, true, command, context)?;
        limit.ok_or("协调器未确认API请求额度".into())
    }
    fn begin_scoped_api_dispatch(
        &mut self,
        task: &str,
        attempt: &str,
        permit: crate::api_currency::DispatchPermit,
    ) -> Result<u32, String> {
        require(
            !self.failed && self.kind == ClientKind::Api,
            "scoped dispatch requires healthy API client",
        )?;
        let (principal, command, context) =
            crate::api_execution::dispatch_command(self, task, attempt)?;
        let (reply, response) = mpsc::channel();
        self.sender
            .send(Message::Apply {
                scoped_dispatch: Some(Box::new(permit)),
                scope: self.scope.clone(),
                cursor: self.cursor(),
                operation: None,
                principal,
                require_new: true,
                command,
                context: Box::new(context),
                reply,
            })
            .map_err(|_| "coordinator unavailable")?;
        let (_, limit) = self.absorb(response)?;
        limit.ok_or("coordinator did not acknowledge scoped request reservation".into())
    }
    fn record_api(
        &mut self,
        principal: &str,
        command: WorkflowCommand,
        context: CommandContext,
    ) -> Result<(), String> {
        self.apply(None, principal, true, command, context)
            .map(|_| ())
    }
}
