//! One resident-owned workflow writer; no model call before two durable assignment ACKs.
use crate::{
    api_startup::ApiStartup,
    scheduler::DurableWorkflow,
    workflow_coordinator::{WorkflowClient, WorkflowCoordinator},
};
use cerulean_adapters::api_generation::ApiClient;
use cerulean_domain::{api_jobs::ApiJob, execution::ExecutionSnapshot};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
};
use tokio::sync::oneshot;

pub(crate) enum Update {
    WorkspaceReady {
        startup: Box<ApiStartup>,
        reply: oneshot::Sender<Result<(), String>>,
    },
    Assignment(crate::managed_assignment::Update),
    Currency(Box<crate::api_currency::Request>),
    ToolCheckpoint(Box<crate::api_tool_checkpoint::Request>),
    Progress(Box<ExecutionSnapshot>),
    Done {
        interrupted: bool,
        outcome: Result<(), String>,
    },
}
pub(crate) struct ActiveApi {
    pub snapshot: ExecutionSnapshot,
    pub job: ApiJob,
    pub receiver: mpsc::Receiver<Update>,
    pub cancel: Option<oneshot::Sender<()>>,
    pub thread: Option<std::thread::JoinHandle<()>>,
    pub disconnected: bool,
    pub preparation_cancel: Arc<AtomicBool>,
    pub tool_checkpoint: Option<crate::api_tool_checkpoint::Checkpoint>,
    pub advice_waiter: Option<crate::api_tool_checkpoint::Reply>,
}
/// Exactly one owner and authoritative projection for each active API project.
pub(crate) struct ApiWorkflow {
    pub owner: WorkflowCoordinator,
    pub snapshot: ExecutionSnapshot,
    pub interrupted: bool,
}
impl ApiWorkflow {
    pub(crate) fn start(store: DurableWorkflow, path: std::path::PathBuf) -> Result<Self, String> {
        let snapshot = crate::async_verification::snapshot(&store);
        Ok(Self {
            owner: WorkflowCoordinator::start(store, path)?,
            snapshot,
            interrupted: false,
        })
    }
    pub(crate) fn refresh(&mut self) -> Result<(), String> {
        let snapshot = self.owner.snapshot()?;
        cerulean_domain::execution::require(
            snapshot.project == self.snapshot.project
                && snapshot.revision_digest == self.snapshot.revision_digest
                && snapshot.approval == self.snapshot.approval
                && snapshot.sequence >= self.snapshot.sequence,
            "API项目协调器快照身份改变或序号倒退",
        )?;
        if snapshot.sequence == self.snapshot.sequence {
            cerulean_domain::execution::require(
                cerulean_domain::execution::digest(&snapshot)?
                    == cerulean_domain::execution::digest(&self.snapshot)?,
                "API项目同序号快照不一致",
            )?;
        }
        self.snapshot = snapshot;
        Ok(())
    }
}
#[cfg(test)]
pub(crate) struct PreparationGate {
    pub entered: mpsc::Sender<()>,
    pub release: mpsc::Receiver<()>,
}
#[cfg(test)]
pub(crate) type Fixture = fn(
    &cerulean_adapters::api_generation::PreparedRequest,
    u32,
) -> Result<
    cerulean_adapters::api_generation::ApiReply,
    cerulean_adapters::api_generation::ApiFailure,
>;

#[cfg(test)]
pub(crate) type GatewayFixture = fn(
    &cerulean_adapters::api_generation::PreparedRequest,
    u32,
    &mut cerulean_adapters::api_generation::GatewayObserver<'_>,
) -> Result<
    cerulean_adapters::api_generation::ApiReply,
    cerulean_adapters::api_generation::ApiFailure,
>;

#[cfg_attr(test, allow(clippy::too_many_arguments))]
pub(crate) fn launch(
    root: &Path,
    mut store: WorkflowClient,
    mut startup: ApiStartup,
    job: ApiJob,
    client: ApiClient,
    #[cfg(test)] scoped_fixture: Option<crate::api_currency::Fixture>,
    #[cfg(test)] fixture: Option<Fixture>,
    #[cfg(test)] gateway_fixture: Option<GatewayFixture>,
    #[cfg(test)] gate: Option<PreparationGate>,
) -> Result<ActiveApi, String> {
    let snapshot = crate::async_verification::snapshot(&store);
    let host_root = root.to_path_buf();
    let preparation_cancel = Arc::new(AtomicBool::new(false));
    let preparing = preparation_cancel.clone();
    let (sender, receiver) = mpsc::channel();
    let (cancel, mut cancellation) = oneshot::channel();
    let thread = std::thread::Builder::new()
        .name(format!("api-{}", job.attempt_id))
        .spawn(move || {
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<(), String> {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|_| "API后台运行时不可用")?;
                    runtime.block_on(async {
                        if startup.prepare_workspace {
                            #[cfg(test)]
                            if let Some(gate) = gate {
                                gate.entered
                                    .send(())
                                    .map_err(|_| "preparation fixture unavailable")?;
                                loop {
                                    if preparing.load(Ordering::Acquire) {
                                        return Err("workspace preparation cancelled".into());
                                    }
                                    match gate
                                        .release
                                        .recv_timeout(std::time::Duration::from_millis(20))
                                    {
                                        Ok(()) => break,
                                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                                        Err(_) => return Err("preparation fixture closed".into()),
                                    }
                                }
                            }
                            let (workspace, proof) = store.prepare_task_workspace_cancellable(
                                &startup.attempt.task_id,
                                &startup.attempt.id,
                                preparing.clone(),
                            )?;
                            cerulean_domain::execution::require(
                                workspace == startup.attempt.binding.workspace,
                                "prepared API workspace differs from reservation",
                            )?;
                            if preparing.load(Ordering::Acquire) {
                                return Err(
                                    "workspace preparation cancelled; preserve directory".into()
                                );
                            }
                            startup.attempt.workspace_proof = Some(proof);
                            crate::api_startup::publish_prepared(&host_root, &startup)?;
                            let (reply, response) = oneshot::channel();
                            sender
                                .send(Update::WorkspaceReady {
                                    startup: Box::new(startup.clone()),
                                    reply,
                                })
                                .map_err(|_| "API workspace acknowledgement unavailable")?;
                            tokio::select! {
                                ack = tokio::time::timeout(std::time::Duration::from_secs(30), response) => {
                                    ack.map_err(|_| "API workspace acknowledgement timed out")?
                                        .map_err(|_| "API workspace acknowledgement lost")??;
                                }
                                _ = &mut cancellation => return Err("workspace preparation cancelled before acknowledgement".into()),
                            }
                            if preparing.load(Ordering::Acquire) {
                                return Err(
                                    "workspace preparation cancelled before assignment".into()
                                );
                            }
                        }
                        crate::managed_assignment::assign(
                            &mut store,
                            &startup.management_operation,
                            startup.attempt.clone(),
                            |update| {
                                sender
                                    .send(Update::Assignment(update))
                                    .map_err(|_| "API分配通道中断".into())
                            },
                        )
                        .await?;
                        let _ = sender.send(Update::Progress(Box::new(
                            crate::async_verification::snapshot(&store),
                        )));
                        if crate::workflow_access::WorkflowAccess::revision(&store)
                            .and_then(|p| p.policy.currency_budget.as_ref())
                            .is_some_and(|p| p.schema == 2) {
                            let inner = crate::api_currency::Driver {
                                client: &client, sender: &sender, last: None,
                                #[cfg(test)] fixture: scoped_fixture,
                            };
                            let mut driver = crate::api_tool_checkpoint::OwnedDriver { inner, sender: &sender };
                            return crate::api_execution::dispatch_protocol(
                                &mut store, &startup.attempt.task_id, &startup.attempt.id,
                                &mut cancellation, &mut driver, &mut |_| Ok(()),
                                &mut |store| { let _ = sender.send(Update::Progress(Box::new(crate::async_verification::snapshot(store)))); },
                            ).await;
                        }
                        #[cfg(test)]
                        let mut round = 0;
                        let mut send = async |prepared, cancel: &mut oneshot::Receiver<()>, gateway: &mut cerulean_adapters::api_generation::GatewayObserver<'_>| {
                                #[cfg(test)]
                                if let Some(fixture) = gateway_fixture {
                                    round += 1;
                                    return fixture(&prepared, round, gateway).into();
                                }
                                #[cfg(test)]
                                if let Some(fixture) = fixture {
                                    round += 1;
                                    return fixture(&prepared, round).into();
                                }
                                client.send_measured_with_gateway(prepared, cancel, gateway).await
                            };
                        let inner = crate::api_execution::LegacyDriver { client: &client, send: &mut send };
                        let mut driver = crate::api_tool_checkpoint::OwnedDriver { inner, sender: &sender };
                        crate::api_execution::dispatch_protocol(
                            &mut store,
                            &startup.attempt.task_id,
                            &startup.attempt.id,
                            &mut cancellation,
                            &mut driver,
                            &mut |_| Ok(()),
                            &mut |store| {
                                let _ = sender.send(Update::Progress(Box::new(
                                    crate::async_verification::snapshot(store),
                                )));
                            },
                        )
                        .await
                    })
                }));
            let panicked = result.is_err();
            let outcome = result.unwrap_or_else(|_| Err("API后台异常；请核对原尝试".into()));
            let _ = sender.send(Update::Done {
                interrupted: panicked,
                outcome,
            });
        })
        .map_err(|_| "API后台线程未启动；保留原启动记录")?;
    Ok(ActiveApi {
        snapshot,
        job,
        receiver,
        cancel: Some(cancel),
        thread: Some(thread),
        disconnected: false,
        preparation_cancel,
        tool_checkpoint: None,
        advice_waiter: None,
    })
}
