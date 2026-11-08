//! Bound local review channels. Only approved text and acknowledgements cross this boundary.
use super::*;
use cerulean_domain::blind_review::*;
use std::time::Duration;

fn review_scratch(root: &Path) -> Result<PathBuf, String> {
    let path = root.join("review-session-launch");
    crate::windows_worker_account::private_directory(
        &path,
        &crate::ipc::current_sid().map_err(|e| e.to_string())?,
    )?;
    crate::windows_worker_account::dos_path(&path)
}
fn save_review_record(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec(value).map_err(|e| e.to_string())?;
    require(bytes.len() <= 64 * 1024, "会话恢复记录超限")?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| e.to_string())
}

pub(super) struct Control {
    pub cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub deadline: i64,
    pub progress: std::sync::Arc<std::sync::Mutex<Option<ReviewSessionProgress>>>,
    pub audit: std::sync::Arc<std::sync::Mutex<Option<ReviewToolAudit>>>,
}
impl Control {
    fn record_audit(&self, mut audit: ReviewToolAudit, request: &str) -> Result<bool, String> {
        audit.request = request.into();
        let clean = audit.status == ReviewToolStatus::Clean;
        let mut previous = self.audit.lock().map_err(|_| "评分核验记录锁损坏")?;
        if let Some(old) = previous.as_ref() {
            require(
                old.status == ReviewToolStatus::Clean,
                "原评分核验已经未通过",
            )?;
            audit.evidence_digest = digest(&(&old.evidence_digest, &audit.evidence_digest))?;
        }
        *previous = Some(audit);
        Ok(clean)
    }
    fn message(
        &self,
        seat: &ReviewSeat,
        request: &str,
        session: &str,
        index: usize,
        packets: usize,
        text: &str,
    ) -> Result<(), String> {
        self.check()?;
        *self.progress.lock().map_err(|_| "评审进度锁损坏")? = Some(ReviewSessionProgress {
            seat: seat.id.clone(),
            model: seat.model.clone(),
            session_id: Some(session.into()),
            request: request.into(),
            packet: index as u32 + 1,
            packets: packets as u32,
            material_message: Some(text.into()),
            acknowledged: false,
        });
        Ok(())
    }
    fn acknowledge(&self) -> Result<(), String> {
        self.check()?;
        if let Some(p) = self.progress.lock().map_err(|_| "评审进度锁损坏")?.as_mut() {
            p.acknowledged = true;
        }
        Ok(())
    }
    pub(super) fn check(&self) -> Result<(), String> {
        require(
            !self.cancel.load(std::sync::atomic::Ordering::Acquire)
                && chrono::Utc::now().timestamp() < self.deadline,
            "评审许可已取消或到期，停止后续分片",
        )
    }
}
pub(super) struct LocalReview {
    root: PathBuf,
    plan: PlanRevision,
    task: String,
    transport: ReviewTransport,
    key: Option<cerulean_domain::secret::SecretInput>,
    chief_executable: Option<cerulean_domain::execution::ValidationFile>,
}
impl RuntimeHost {
    pub(super) fn chief_review_executable(
        &self,
    ) -> Result<cerulean_domain::execution::ValidationFile, String> {
        let path = crate::codex_discovery::resolve_session_executable(
            self.codex_connection
                .as_ref()
                .map(|connection| connection.executable.as_str()),
        )?;
        crate::codex_discovery::validate_executable(Path::new(&path))?;
        crate::async_codex::describe_executable(Path::new(&path))
    }
    pub(super) fn inspect_chief_review_model(
        &self,
        session: &str,
    ) -> Result<(String, String), String> {
        let executable = self.chief_review_executable()?;
        let scratch = review_scratch(&self.root)?;
        let session = session.to_owned();
        std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| "Codex会话核对运行时不可用")?
                .block_on(async {
                    let mut client = cerulean_adapters::codex::CodexClient::launch_readonly(
                        Path::new(&executable.path),
                        &scratch,
                    )?;
                    let result: Result<(String, String), String> = async {
                        client.connect_review().await?;
                        client.review_session_identity(&session).await
                    }
                    .await;
                    let closed = client.close().await;
                    let value = result?;
                    closed?;
                    Ok::<_, String>(value)
                })
        })
        .join()
        .map_err(|_| "Codex会话身份核对中断".to_string())?
    }
    pub(super) fn prepare_local_review(
        &self,
        project: &str,
        seat: &ReviewSeat,
    ) -> Result<LocalReview, String> {
        let transport = seat.transport.clone().ok_or("本地评审通道缺失")?;
        if let ReviewTransport::OmpDeputy { worker } = &transport {
            seat.validate_source()?;
            let setup = &self
                .project_setups
                .get(project)
                .ok_or("副指挥设置缺失")?
                .setup;
            require(
                setup
                    .management_sources
                    .as_ref()
                    .ok_or("副指挥来源缺失")?
                    .deputy_omp_worker(&setup.deputy_model)?
                    == *worker,
                "副指挥评审配置已经变化",
            )?;
            return Ok(LocalReview {
                root: self.root.clone(),
                plan: self
                    .projects
                    .get(project)
                    .and_then(|s| s.revision())
                    .ok_or("副指挥原方案缺失")?
                    .clone(),
                task: String::new(),
                transport,
                key: Some(crate::credential_vault::load_for(
                    &self.root,
                    cerulean_domain::credentials::CredentialProvider::DeepSeek,
                )?),
                chief_executable: None,
            });
        }
        if let ReviewTransport::CodexSession {
            session_id,
            provider,
        } = &transport
        {
            require(
                seat.role == "chief" && seat.id == "chief",
                "绑定的Codex桌面会话只属于主指挥",
            )?;
            require(
                self.inspect_chief_review_model(session_id)?
                    == (seat.model.clone(), provider.clone()),
                "主指挥原会话模型或供应商已变化",
            )?;
            return Ok(LocalReview {
                root: self.root.clone(),
                plan: self
                    .projects
                    .get(project)
                    .and_then(|s| s.revision())
                    .ok_or("主指挥原方案缺失")?
                    .clone(),
                task: String::new(),
                transport,
                key: None,
                chief_executable: Some(self.chief_review_executable()?),
            });
        }
        let task = match &transport {
            ReviewTransport::Cli { task_id, .. } | ReviewTransport::Desktop { task_id, .. } => {
                task_id
            }
            ReviewTransport::CodexSession { .. } | ReviewTransport::OmpDeputy { .. } => {
                unreachable!()
            }
        };
        let plan = self
            .projects
            .get(project)
            .and_then(|s| s.revision())
            .ok_or("评审原方案缺失")?
            .clone();
        let selected = plan.task(task)?;
        let _account = crate::windows_worker_account::load(&self.root, &plan, task)?;
        let key = match &transport {
            ReviewTransport::Cli { adapter, .. } => {
                let worker = selected.worker.as_ref().ok_or("CLI评审程序缺失")?;
                require(
                    &selected.adapter_id == adapter && worker.model == seat.model,
                    "CLI评审模型与原方案不同",
                )?;
                if adapter == "omp" {
                    Some(crate::credential_vault::load_for(
                        &self.root,
                        cerulean_domain::credentials::CredentialProvider::DeepSeek,
                    )?)
                } else {
                    None
                }
            }
            ReviewTransport::Desktop { session_id, .. } => {
                let desktop = selected.desktop_worker.as_ref().ok_or("桌面评审配置缺失")?;
                self.validate_active_desktop_selection(project, desktop)?;
                require(
                    desktop.binding.observation.session_id.as_ref() == Some(session_id)
                        && desktop.model_label == seat.model,
                    "桌面评审会话或模型与原方案不同",
                )?;
                None
            }
            ReviewTransport::CodexSession { .. } | ReviewTransport::OmpDeputy { .. } => {
                unreachable!()
            }
        };
        Ok(LocalReview {
            root: self.root.clone(),
            plan,
            task: task.clone(),
            transport,
            key,
            chief_executable: None,
        })
    }
}
impl LocalReview {
    pub(super) async fn analysis(
        &self,
        seat: &ReviewSeat,
        request: &str,
        material: &ReviewMaterial,
        ballots: &[RevealedBallot],
        weights: Option<&cerulean_domain::review_weights::WeightedResult>,
        control: &Control,
    ) -> Result<cerulean_adapters::blind_review::AnalysisObservation, String> {
        let ReviewTransport::OmpDeputy { worker } = &self.transport else {
            return Err("当前会话接入不支持副指挥分析".into());
        };
        let (instruction, input) = cerulean_adapters::blind_review::analysis_prompt(
            seat, request, material, ballots, weights,
        )?;
        let prompt =
            serde_json::to_string(&serde_json::json!({"instruction":instruction,"input":input}))
                .map_err(|e| e.to_string())?;
        control.message(seat, request, "fresh-omp-deputy", 0, 1, &prompt)?;
        let (text, _) = super::omp_management_host::prompt_once(
            &self.root,
            request,
            &self.plan.approval_digest()?,
            worker,
            &prompt,
            self.key.as_ref().ok_or("OMP 凭证缺失")?,
            Some(control),
        )
        .await?;
        control.acknowledge()?;
        cerulean_adapters::blind_review::parse_analysis_text(&text, seat, request)
    }
    pub(super) async fn functional(
        &self,
        seat: &ReviewSeat,
        request: &str,
        material: &ReviewMaterial,
        checks: &serde_json::Value,
        results: Option<&serde_json::Value>,
        control: &Control,
    ) -> Result<cerulean_adapters::blind_review::FunctionalObservation, String> {
        let packets = cerulean_adapters::blind_review::functional_packets(
            seat, request, material, checks, results,
        )?;
        let text = self.chief(seat, request, &packets, control).await?;
        require(
            control
                .audit
                .lock()
                .map_err(|_| "功能测试会话核验记录不可用")?
                .as_ref()
                .is_some_and(|a| a.status == ReviewToolStatus::Clean),
            "主指挥功能测试会话的工具核验未通过",
        )?;
        cerulean_adapters::blind_review::parse_functional_text(&text, seat, request)
    }
    async fn chief(
        &self,
        seat: &ReviewSeat,
        request: &str,
        packets: &[(String, Option<String>)],
        control: &Control,
    ) -> Result<String, String> {
        let ReviewTransport::CodexSession {
            session_id,
            provider,
        } = &self.transport
        else {
            return Err("主指挥会话接入不符".into());
        };
        let executable = self
            .chief_executable
            .as_ref()
            .ok_or("Codex主指挥程序缺失")?;
        let _pin = crate::backup::open(Path::new(&executable.path), false, false)
            .map_err(|_| "主指挥程序无法固定")?;
        require(
            crate::async_codex::describe_executable(Path::new(&executable.path))?.sha256
                == executable.sha256,
            "主指挥程序与评审准备不同",
        )?;
        let scratch = review_scratch(&self.root)?;
        let base = self.root.join("review-session-restores");
        let owner = crate::ipc::current_sid().map_err(|e| e.to_string())?;
        crate::windows_worker_account::private_directory(&base, &owner)?;
        cerulean_domain::execution::identifier(request)?;
        let folder = base.join(request);
        require(!folder.exists(), "本次会话操作已有记录，禁止重新发送")?;
        crate::windows_worker_account::private_directory(&folder, &owner)?;
        let mut client = cerulean_adapters::codex::CodexClient::launch_reviewer(
            Path::new(&executable.path),
            &scratch,
        )
        .await?;
        let result: Result<String,String> = async {
            client.connect_review().await?;
            control.check()?;
            let original = client.prepare_bound_review(session_id,&seat.model,provider).await?;
            save_review_record(&folder.join("intent.json"),&serde_json::json!({"request":request,"projectId":self.plan.project.id,"sessionId":session_id,"originalSettings":original}))?;
            let binding = client.bind_review_session(&seat.model,Some(session_id)).await?;
            let mut reply = String::new();
            for (index,(prompt,ack)) in packets.iter().enumerate() {
                control.message(seat,request,session_id,index,packets.len(),prompt)?;
                reply = client.review_message(&binding,&format!("{request}-{}",index+1),prompt,Duration::from_secs(90)).await?;
                let audit = client.take_review_audit().ok_or("主指挥评分调用记录缺失")?;
                if !control.record_audit(audit,request)? { return Ok(reply); }
                if let Some(ack) = ack { require(reply.trim() == ack,"主指挥分片接收回执不符，停止后续发送")?; }
                control.acknowledge()?;
            }
            Ok(reply)
        }.await;
        let restored = client.restore_bound_review().await;
        let closed = client.close().await;
        save_review_record(
            &folder.join("result.json"),
            &serde_json::json!({"request":request,"restored":restored.is_ok(),"closed":closed.is_ok(),"restoreError":restored.as_ref().err(),"closeError":closed.as_ref().err(),"scoringCompleted":result.is_ok()}),
        )?;
        if restored.is_err() {
            if let Err(error) = &result {
                if error.starts_with("review_tool_observed:") {
                    return Err(format!("{error};restore_pending"));
                }
            }
            return Err("Codex评分会话的原设置恢复待核对，已保留恢复记录".into());
        }
        closed?;
        result
    }
    pub(super) async fn run(
        self,
        seat: &ReviewSeat,
        pending: &crate::blind_review::Pending,
        material: &ReviewMaterial,
        control: &Control,
    ) -> Result<cerulean_adapters::blind_review::Observation, String> {
        control.check()?;
        let request = &pending.request;
        let packets = cerulean_adapters::blind_review::local_packets(
            request,
            pending.readiness,
            material,
            Some(&self.transport),
        )?;
        let (reply, audit) = match &self.transport {
            ReviewTransport::OmpDeputy { worker } => {
                control.check()?;
                let prompt = serde_json::to_string(&serde_json::json!({"materialPackets":packets.iter().map(|(p,_)|p).collect::<Vec<_>>(),"instruction":"All packets are delivered together. Do not emit intermediate ACKs. Return the final packet's strict JSON only. Never call tools."})).map_err(|e|e.to_string())?;
                control.message(seat, request, "fresh-omp-deputy", 0, 1, &prompt)?;
                let (reply, events) = super::omp_management_host::prompt_once(
                    &self.root,
                    request,
                    &self.plan.approval_digest()?,
                    worker,
                    &prompt,
                    self.key.as_ref().ok_or("OMP 凭证缺失")?,
                    Some(control),
                )
                .await?;
                control.record_audit(
                    ReviewToolAudit {
                        status: ReviewToolStatus::Clean,
                        source: "omp-events".into(),
                        request: request.into(),
                        evidence_digest: digest(&(&events, &reply))?,
                        tools: vec![],
                        reason: "独立 OMP 无工具协议已核验实际模型与原请求终态".into(),
                    },
                    request,
                )?;
                control.acknowledge()?;
                (reply, None)
            }
            ReviewTransport::Cli { adapter, .. } if adapter == "codex" => {
                (self.codex(seat, request, &packets, control).await?, None)
            }
            ReviewTransport::Cli { adapter, .. } if adapter == "omp" => {
                (self.omp(seat, request, &packets, control).await?, None)
            }
            ReviewTransport::Desktop { .. } => {
                let (text, audit) = self.desktop(seat, request, &packets, control)?;
                (text, Some(audit))
            }
            ReviewTransport::CodexSession { .. } => {
                (self.chief(seat, request, &packets, control).await?, None)
            }
            _ => return Err("本地评审通道不可用".into()),
        };
        control.check()?;
        let audit = match audit {
            Some(value) => Some(value),
            None => control
                .audit
                .lock()
                .map_err(|_| "评分过程核验记录不可读取")?
                .clone(),
        };
        let parsed = cerulean_adapters::blind_review::parse_local(
            &reply,
            &seat.model,
            seat,
            request,
            material,
        );
        match (parsed, audit) {
            (Ok(mut observed), Some(audit)) => {
                observed.audit = audit;
                Ok(observed)
            }
            (Err(_), Some(audit)) if audit.status != ReviewToolStatus::Clean => {
                Ok(cerulean_adapters::blind_review::Observation {
                    model: seat.model.clone(),
                    ballot: Ballot {
                        assessments: vec![],
                        vote: Vote::Fail,
                        citations: vec![],
                        reason: "评分过程未通过工具核验".into(),
                    },
                    audit,
                })
            }
            (result, _) => result,
        }
    }
    async fn codex(
        &self,
        seat: &ReviewSeat,
        request: &str,
        packets: &[(String, Option<String>)],
        control: &Control,
    ) -> Result<String, String> {
        let task = self.plan.task(&self.task)?;
        let worker = task.worker.as_ref().ok_or("Codex配置缺失")?;
        let _pin = crate::backup::open(Path::new(&worker.executable.path), false, false)
            .map_err(|_| "Codex评审程序无法固定")?;
        require(
            crate::async_codex::describe_executable(Path::new(&worker.executable.path))?.sha256
                == worker.executable.sha256,
            "Codex评审程序与批准摘要不同",
        )?;
        let root = task
            .worker_directory
            .as_ref()
            .ok_or("评审目录未隔离")?
            .root(&self.plan.project.root)?;
        let mut client = crate::windows_worker_account::codex(
            &self.root,
            &self.plan,
            &self.task,
            Path::new(&worker.executable.path),
            Path::new(&root),
            false,
        )?;
        let result: Result<String, String> = async {
            let connection = client.connect().await?;
            client.configure_worker_effort(&connection, &seat.model, seat.reasoning_effort)?;
            let binding = client.bind_review_session(&seat.model, None).await?;
            let mut result = String::new();
            for (index, (prompt, ack)) in packets.iter().enumerate() {
                control.check()?;
                control.message(
                    seat,
                    request,
                    &binding.thread_id,
                    index,
                    packets.len(),
                    prompt,
                )?;
                result = client
                    .review_message(
                        &binding,
                        &format!("{request}-{}", index + 1),
                        prompt,
                        Duration::from_secs(90),
                    )
                    .await?;
                let audit = client.take_review_audit().ok_or("CLI评分调用记录缺失")?;
                if !control.record_audit(audit, request)? {
                    return Ok(result);
                }
                if let Some(ack) = ack {
                    require(result.trim() == ack, "Codex分片接收回执不符，未继续发送")?;
                }
                control.acknowledge()?;
            }
            Ok(result)
        }
        .await;
        let closed = client.close().await;
        let result = result?;
        closed?;
        Ok(result)
    }
    async fn omp(
        &self,
        seat: &ReviewSeat,
        request: &str,
        packets: &[(String, Option<String>)],
        control: &Control,
    ) -> Result<String, String> {
        use cerulean_adapters::omp_transport::worker::DispatchIntent;
        use cerulean_domain::execution::{
            ControllerRequestIdentity, IdentityStrength, SessionBinding,
        };
        use sha2::Digest;
        let task = self.plan.task(&self.task)?;
        let account = crate::windows_worker_account::load(&self.root, &self.plan, &self.task)?;
        let root = PathBuf::from(&account.record.workspace);
        let profiles = root.join(".cerulean-omp");
        fs::create_dir_all(&profiles).map_err(|e| e.to_string())?;
        let worker = task.worker.as_ref().ok_or("OMP配置缺失")?;
        let mut process = crate::omp_process::WorkerProcess::launch_with_account(
            worker,
            &profiles.join(request),
            self.key.as_ref().ok_or("OMP凭证缺失")?,
            Some(account),
        )?;
        let result: Result<String,String> = async {
            control.check()?;
            let identity = process.connect().await?;
            // OMP's owned no-tool transport accepts one bounded prompt with the original fragments.
            let prompt = serde_json::to_string(&serde_json::json!({"materialPackets":packets.iter().map(|(p,_)|p).collect::<Vec<_>>(),"instruction":"All material packets are delivered together. Do not emit intermediate ACKs. Follow the final packet and return its strict JSON only."})).map_err(|e|e.to_string())?;
            control.message(seat,request,&identity.session_id,0,1,&prompt)?;
            let intent = DispatchIntent { version:1, task_id:self.task.clone(), attempt_id:request.into(), revision_digest:self.plan.approval_digest()?, fence:1,
                binding:SessionBinding { adapter_id:"omp".into(), adapter_version:"18.4.4".into(), session_id:identity.session_id, identity_strength:IdentityStrength::ControllerRequest(ControllerRequestIdentity { connection_id:request.into(), request_id:request.into() }), workspace:root.to_string_lossy().into() },
                provider:worker.provider.clone(), model:seat.model.clone(), prompt_sha256:format!("{:x}",sha2::Sha256::digest(prompt.as_bytes())) };
            let (_cancel,mut receiver) = tokio::sync::oneshot::channel();
            let mut events = Vec::new();
            let (receipt,text) = process.channel()?.execute_review(intent,&prompt,Duration::from_secs(90),&mut receiver, |event| {
                events.push(event.clone());
                Ok(())
            }).await?;
            control.record_audit(ReviewToolAudit {
                status: ReviewToolStatus::Clean,
                source: "omp-events".into(),
                request: request.into(),
                evidence_digest: digest(&(events, receipt, &text))?,
                tools: vec![],
                reason: "OMP封闭协议已核对原请求、实际模型、无工具回复及会话结束状态".into(),
            }, request)?;
            control.acknowledge()?;
            Ok::<_,String>(text)
        }.await;
        let closed = process.close().await;
        let result = result?;
        closed?;
        Ok(result)
    }
    fn desktop(
        &self,
        seat: &ReviewSeat,
        request: &str,
        packets: &[(String, Option<String>)],
        control: &Control,
    ) -> Result<(String, ReviewToolAudit), String> {
        use super::desktop_sessions::{observe, ReadRequest, ReviewMessage};
        use sha2::Digest;
        let selected = self
            .plan
            .task(&self.task)?
            .desktop_worker
            .as_ref()
            .ok_or("桌面配置缺失")?;
        let _account = crate::windows_worker_account::desktop(&self.root, &self.plan, &self.task)?
            .ok_or("桌面评审账户未隔离")?;
        let window = selected.binding.observation.window.clone();
        let mut result = String::new();
        let mut evidence = vec![];
        for (index, (packet, ack)) in packets.iter().enumerate() {
            control.check()?;
            let current = observe(
                &self.root,
                &ReadRequest::ReviewPrepare {
                    version: 1,
                    window: window.clone(),
                },
            )?;
            let observation = current.observation.ok_or("桌面评审会话不可读取")?;
            require(
                observation.session_id == selected.binding.observation.session_id
                    && observation.model_label.as_deref() == Some(&seat.model),
                "桌面评审会话或模型变化",
            )?;
            let marker = format!("CL_TASK_{}", &digest(&(request, index))?[..32]);
            let prompt = serde_json::to_string(&serde_json::json!({"packet":packet,"replyInstruction":format!("先输出本包要求的回复，最后追加标记 {marker}。") })).map_err(|e|e.to_string())?;
            control.message(
                seat,
                request,
                observation.session_id.as_deref().ok_or("桌面会话ID缺失")?,
                index,
                packets.len(),
                &prompt,
            )?;
            let message = ReviewMessage {
                id: digest(&(request, index, &observation))?,
                binding_id: selected.binding.id.clone(),
                marker,
                prompt_sha256: format!("{:x}", sha2::Sha256::digest(prompt.as_bytes())),
                prompt,
                bridge_sha256: super::desktop_sessions::fingerprint(),
                observation,
                baseline: current.baseline,
                max_wait_seconds: 50,
            };
            let audit = crate::qoder_review_audit::Audit::begin(
                Path::new(&selected.project_root),
                message
                    .observation
                    .session_id
                    .as_deref()
                    .ok_or("桌面会话ID缺失")?,
            )?;
            result = observe(
                &self.root,
                &ReadRequest::ReviewSend {
                    version: 1,
                    review: message.clone(),
                },
            )?
            .review_text
            .ok_or("桌面评审回复缺失，未重发")?;
            // A separate read revalidates the same session and original prompt after send.
            let reread = observe(
                &self.root,
                &ReadRequest::ReviewRead {
                    version: 1,
                    review: message.clone(),
                },
            )?
            .review_text
            .ok_or("桌面评审回读缺失")?;
            require(result == reread, "桌面评审回读与原回复不同")?;
            let (original, audit) =
                audit.finish(request, &message.prompt, &result, &message.marker)?;
            result = original;
            if audit.status != ReviewToolStatus::Clean {
                return Ok((result, audit));
            }
            evidence.push(audit.evidence_digest);
            if let Some(ack) = ack {
                require(result.trim() == ack, "桌面分片接收回执不符，未继续发送")?;
            }
            control.acknowledge()?;
        }
        Ok((
            result,
            ReviewToolAudit {
                status: ReviewToolStatus::Clean,
                source: "desktop-history".into(),
                request: request.into(),
                evidence_digest: digest(&evidence)?,
                tools: vec![],
                reason: "各分片及最终评分均已核对原会话完整工具事件清单".into(),
            },
        ))
    }
}
