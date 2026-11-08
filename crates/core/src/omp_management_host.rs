//! Deputy OMP uses a fresh no-tool process for each durable recommendation.
use super::*;
use crate::management_execution::{
    omp::{OmpRequest, OmpResponse},
    DecisionIntent,
};
use cerulean_domain::{management::Purpose, project_setup::DeputySource};

impl RuntimeHost {
    pub(super) fn omp_deputy_selected(&self, project: &str, purpose: Purpose) -> bool {
        matches!(purpose, Purpose::Assign | Purpose::Advise)
            && self
                .project_setups
                .get(project)
                .and_then(|s| s.setup.management_sources.as_ref())
                .is_some_and(|s| matches!(s.deputy, DeputySource::Omp { .. }))
    }
    #[cfg(windows)]
    pub(super) fn start_omp_management(&mut self, intent: DecisionIntent) -> Result<(), String> {
        intent.validate()?;
        require(
            self.omp_deputy_selected(&intent.project_id, intent.purpose),
            "OMP 副指挥来源与请求角色不符",
        )?;
        self.require_currency_execution_available(&intent.project_id)?;
        self.require_management_dispatch_available(&intent.project_id, intent.purpose)?;
        self.ensure_run_dispatch_authorized(&intent.operation_id)?;
        self.require_management_connection(&intent.project_id, intent.purpose)?;
        require(
            self.management_work_count() < 4,
            "management concurrency limit reached",
        )?;
        let saved = self
            .project_setups
            .get(&intent.project_id)
            .ok_or("副指挥设置缺失")?;
        require(
            intent.model == saved.setup.deputy_model
                && self.management_operation_ids(&intent.project_id).len()
                    < saved.setup.management_max_requests as usize,
            "副指挥模型不同或请求次数已达上限",
        )?;
        let worker = saved
            .setup
            .management_sources
            .as_ref()
            .ok_or("副指挥来源缺失")?
            .deputy_omp_worker(&intent.model)?;
        if intent.context.get("supervision").is_some() {
            require(
                intent.purpose == Purpose::Advise,
                "监督材料只用于副指挥建议",
            )?;
            self.revalidate_counted_supervision(&intent)?;
        } else {
            require(
                !self.has_api_activity(&intent.project_id) || intent.purpose == Purpose::Assign,
                "执行期间的建议需要原监督记录",
            )?;
            let requirement = intent.context["requirement"]
                .as_str()
                .ok_or("管理需求缺失")?;
            let current = self.rebuild_management_context(
                &intent,
                self.management_context(&intent.project_id, requirement)?,
            )?;
            require(
                digest(&current)? == intent.state_digest,
                "OMP 派发前项目状态已变化",
            )?;
        }
        let request = OmpRequest::new(&intent, worker)?;
        let key = crate::credential_vault::load_for(
            &self.root,
            cerulean_domain::credentials::CredentialProvider::DeepSeek,
        )?;
        let root = self.root.clone();
        let id = intent.operation_id.clone();
        let state = intent.state_digest.clone();
        self.management.begin_omp(intent.clone(), request.clone())?;
        let failure_request = request.clone();
        let (send, receive) = std::sync::mpsc::channel();
        let dispatch_id = id.clone();
        let spawned = std::thread::Builder::new()
            .name("omp-deputy".into())
            .spawn(move || {
                let result = (|| {
                    let runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|_| "OMP 管理运行时不可用".to_string())?;
                    runtime.block_on(async {
                        let (text, events) = prompt_once(
                            &root,
                            &dispatch_id,
                            &state,
                            &request.worker,
                            &request.prompt,
                            &key,
                            None,
                        )
                        .await?;
                        OmpResponse::from_reply(&intent, &request, events, text)
                    })
                })();
                let outcome = result.or_else(|error| OmpResponse::failed(&request, error));
                let _ = send.send(
                    outcome.map(|r| management_scoped_host::ManagementCompletion::Omp(Box::new(r))),
                );
            });
        if spawned.is_err() {
            self.management.finish_omp(
                &id,
                OmpResponse::failed(&failure_request, "OMP 管理线程未启动，未重试".into())?,
            )?;
            return Err("OMP 管理线程未启动".into());
        }
        self.management_active.insert(id, receive);
        Ok(())
    }
}

#[cfg(windows)]
pub(super) async fn prompt_once(
    root: &Path,
    request: &str,
    state: &str,
    worker: &cerulean_domain::execution::WorkerSelection,
    prompt: &str,
    key: &cerulean_domain::secret::SecretInput,
    control: Option<&local_review::Control>,
) -> Result<
    (
        String,
        Vec<cerulean_adapters::omp_transport::worker::WorkerEvent>,
    ),
    String,
> {
    use cerulean_adapters::omp_transport::worker::DispatchIntent;
    use cerulean_domain::execution::{ControllerRequestIdentity, IdentityStrength, SessionBinding};
    use sha2::Digest;
    use std::io::Write;
    if let Some(control) = control {
        control.check()?;
    }
    identifier(request)?;
    let parent = worker
        .omp_runtime_parent
        .as_deref()
        .map(Path::new)
        .unwrap_or(root);
    let parent = crate::omp_process::check_profile_parent(parent)?;
    let folder = parent.join(format!("omp-deputy-{request}"));
    require(!folder.exists(), "OMP 原请求目录已存在，未重发")?;
    crate::windows_worker_account::private_directory(
        &folder,
        &crate::ipc::current_sid().map_err(|e| e.to_string())?,
    )?;
    let mut process =
        crate::omp_process::WorkerProcess::launch(worker, &folder.join("profile"), key)?;
    let operation = async {
        let identity = process.connect().await?;
        if let Some(control) = control {
            control.check()?;
        }
        let intent = DispatchIntent {
            version: 1,
            task_id: "deputy".into(),
            attempt_id: request.into(),
            revision_digest: state.into(),
            fence: 1,
            binding: SessionBinding {
                adapter_id: "omp".into(),
                adapter_version: "18.4.4".into(),
                session_id: identity.session_id,
                identity_strength: IdentityStrength::ControllerRequest(ControllerRequestIdentity {
                    connection_id: request.into(),
                    request_id: request.into(),
                }),
                workspace: process.profile().join("workspace").to_string_lossy().into(),
            },
            provider: worker.provider.clone(),
            model: worker.model.clone(),
            prompt_sha256: format!("{:x}", sha2::Sha256::digest(prompt.as_bytes())),
        };
        let (_cancel, mut receiver) = tokio::sync::oneshot::channel();
        let mut events = Vec::new();
        let (_, text) = process
            .channel()?
            .execute_review(
                intent,
                prompt,
                std::time::Duration::from_secs(90),
                &mut receiver,
                |event| {
                    let mut file = fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(folder.join(format!("event-{}.json", events.len())))
                        .map_err(|e| e.to_string())?;
                    file.write_all(&serde_json::to_vec(event).map_err(|e| e.to_string())?)
                        .and_then(|_| file.sync_all())
                        .map_err(|e| e.to_string())?;
                    events.push(event.clone());
                    Ok(())
                },
            )
            .await?;
        let mut response = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(folder.join("response.txt"))
            .map_err(|e| e.to_string())?;
        response
            .write_all(text.as_bytes())
            .and_then(|_| response.sync_all())
            .map_err(|e| e.to_string())?;
        Ok::<_, String>((text, events))
    };
    let result = if let Some(control) = control {
        tokio::select! {
            result = operation => result,
            error = async {
                loop {
                    if let Err(error) = control.check() { break error; }
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                }
            } => Err(error),
        }
    } else {
        operation.await
    };
    let closed = process.close().await;
    let result = result?;
    let exit = closed?;
    require(
        exit.remaining_processes == 0 && (exit.forced || exit.exit_code == 0),
        "OMP 管理进程收尾失败",
    )?;
    Ok(result)
}
