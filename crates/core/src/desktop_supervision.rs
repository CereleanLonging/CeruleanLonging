//! Desktop task supervision through Qoder's scoped, supported lifecycle hooks.
use super::*;
use crate::{
    desktop_execution::TaskReview,
    desktop_hook::{Advice, Configuration, Event, Incoming, Notice, Session},
    management_execution::{ApplicationTarget, DecisionIntent, DecisionState},
};
use cerulean_domain::{
    desktop_sessions::DesktopSupervisionView,
    management::{ManagementDecision, Purpose},
    project_setup::{ProjectRun, RunState},
    supervision::{SupervisionAdviceView, SupervisionIncident, SupervisionPolicy},
};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Base {
    review: TaskReview,
    policy: SupervisionPolicy,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Evidence {
    review_digest: String,
    events: Vec<Event>,
}

pub(in crate::host) fn recovered_view(
    root: &Path,
    review: &TaskReview,
) -> Result<Option<DesktopSupervisionView>, String> {
    let path = directory(root, &review.id)?;
    if !path.exists() {
        return Ok(None);
    }
    let base: Base = crate::desktop_execution::load(&path, "scope")?;
    require(
        digest(&base.review)? == digest(review)?,
        "桌面监督历史任务不符",
    )?;
    base.policy.validate()?;
    let mut evidence = Evidence {
        review_digest: review.id.clone(),
        events: vec![],
    };
    let mut gap = false;
    for index in 1..=128 {
        let stem = format!("event-{index:03}");
        let present = path.join(format!("{stem}.json")).exists()
            || path.join(format!("{stem}.pending")).exists();
        if !present {
            gap = true;
            continue;
        }
        require(!gap, "桌面监督历史事件序列缺失")?;
        evidence
            .events
            .push(crate::desktop_execution::load::<Event>(&path, &stem)?);
    }
    let ended = path.join("ended.json").exists();
    let error = if ended {
        let receipt: serde_json::Value = crate::desktop_execution::load(&path, "ended")?;
        require(
            receipt["reviewDigest"] == review.id
                && receipt["events"] == evidence.events.len()
                && receipt["evidenceDigest"] == digest(&evidence)?,
            "桌面监督结束回执不符",
        )?;
        receipt["error"].as_str().map(str::to_owned)
    } else {
        Some("监督通道已中断，保留原事件与投送回执；不会自动重连或重发".into())
    };
    Ok(Some(DesktopSupervisionView {
        connected: false,
        events: evidence.events.len() as u32,
        failures: evidence
            .events
            .iter()
            .filter(|e| e.name == "PostToolUseFailure")
            .count() as u32,
        incidents: incidents(&evidence, &base.policy)?.len() as u32,
        state: if ended { "stopped" } else { "interrupted" }.into(),
        error,
    }))
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Binding {
    version: u32,
    kind: String,
    run_id: String,
    run_digest: String,
    pub(super) task_id: String,
    attempt_id: String,
    review_digest: String,
    incident_id: String,
    observation_digest: String,
}

fn directory(root: &Path, review: &str) -> Result<std::path::PathBuf, String> {
    sha256(review)?;
    Ok(root.join("desktop-supervision-v1").join(review))
}
pub(super) fn binding(intent: &DecisionIntent) -> Result<Option<Binding>, String> {
    let Some(raw) = intent.context.get("supervision") else {
        return Ok(None);
    };
    if raw["version"] != 5 {
        return Ok(None);
    }
    let bound: Binding = serde_json::from_value(raw.clone()).map_err(|_| "桌面监督绑定无效")?;
    require(
        bound.version == 5 && bound.kind == "desktop" && intent.purpose == Purpose::Advise,
        "桌面监督目的不符",
    )?;
    for id in [
        &bound.run_id,
        &bound.task_id,
        &bound.attempt_id,
        &bound.incident_id,
    ] {
        identifier(id)?;
    }
    for hash in [
        &bound.run_digest,
        &bound.review_digest,
        &bound.observation_digest,
    ] {
        sha256(hash)?;
    }
    Ok(Some(bound))
}
fn incidents(
    evidence: &Evidence,
    policy: &SupervisionPolicy,
) -> Result<Vec<SupervisionIncident>, String> {
    policy.validate()?;
    let mut result = vec![];
    let mut current = None;
    let mut first = 0usize;
    let mut count = 0u32;
    for (index, event) in evidence.events.iter().enumerate() {
        let action = event.action_digest.as_ref();
        if event.interrupted || event.name != "PostToolUseFailure" {
            if !event.interrupted && event.name == "PreToolUse" && current == action {
                continue;
            }
            current = None;
            count = 0;
            continue;
        }
        if action.is_none() {
            return Err("桌面异常操作摘要缺失".into());
        }
        if current != action {
            current = action;
            first = index;
            count = 0;
        }
        count += 1;
        if count == policy.repeated_failure_threshold {
            let hash = digest(&(&evidence.review_digest, policy, &evidence.events[..=index]))?;
            result.push(SupervisionIncident {
                id: format!("incident-{}", &hash[..48]),
                first_sequence: first as u32 + 1,
                trigger_sequence: index as u32 + 1,
                action_digest: action.cloned().ok_or("桌面异常操作摘要缺失")?,
                evidence_digest: hash,
            });
        }
    }
    Ok(result)
}

// An immutable failure prefix proves the incident happened, not that advice is
// still useful. A successful call, changed operation, or conversation boundary
// retires it even if the old prefix is still present in the event history.
fn incident_is_current(evidence: &Evidence, incident: &SupervisionIncident) -> bool {
    let trigger = incident.trigger_sequence.saturating_sub(1) as usize;
    let Some(event) = evidence.events.get(trigger) else {
        return false;
    };
    event.name == "PostToolUseFailure"
        && !event.interrupted
        && event.action_digest.as_ref() == Some(&incident.action_digest)
        && evidence.events[trigger + 1..].iter().all(|next| {
            !next.interrupted
                && matches!(next.name.as_str(), "PreToolUse" | "PostToolUseFailure")
                && next.action_digest.as_ref() == Some(&incident.action_digest)
                && next.agent_id == event.agent_id
        })
}

pub(in crate::host) struct Live {
    root: std::path::PathBuf,
    session: Session,
    base: Base,
    evidence: Evidence,
    connected: bool,
    finished: bool,
    error: Option<String>,
    pending: Option<oneshot::Sender<Option<Advice>>>,
    deactivated: bool,
    pub(in crate::host) view: DesktopSupervisionView,
}
impl Live {
    pub(in crate::host) fn start(
        root: &Path,
        review: &TaskReview,
        policy: SupervisionPolicy,
        worker_sid: &str,
    ) -> Result<Self, String> {
        policy.validate()?;
        crate::review_checks_guard::enable(root)?;
        let mut random = [0u8; 32];
        require(
            unsafe {
                windows_sys::Win32::Security::Cryptography::BCryptGenRandom(
                    std::ptr::null_mut(),
                    random.as_mut_ptr(),
                    32,
                    windows_sys::Win32::Security::Cryptography::BCRYPT_USE_SYSTEM_PREFERRED_RNG,
                )
            } == 0,
            "Hook随机凭据生成失败",
        )?;
        let token = digest(&random)?;
        let config = Configuration {
            enabled: true,
            version: 1,
            endpoint: format!(r"\\.\pipe\CeruleanLonging-hook-{token}"),
            owner_sid: crate::ipc::current_sid().map_err(|e| e.to_string())?,
            worker_sid: worker_sid.into(),
            review_digest: review.id.clone(),
            token,
            workspace: review.attempt.binding.workspace.clone(),
            session_id: review
                .observation
                .session_id
                .clone()
                .ok_or("桌面监督需要明确的会话ID")?,
        };
        let path = directory(root, &review.id)?;
        ordinary(root, true)?;
        let parent = path.parent().ok_or("桌面监督目录缺失")?;
        if !parent.exists() {
            std::fs::create_dir(parent).map_err(|e| e.to_string())?;
        }
        ordinary(parent, true)?;
        std::fs::create_dir(&path).map_err(|_| "桌面监督尝试已登记，禁止重建旧投送通道")?;
        ordinary(&path, true)?;
        let base = Base {
            review: review.clone(),
            policy,
        };
        crate::desktop_execution::publish(&path, "scope", &base)?;
        let session = Session::start(config)?;
        if let Err(error) = crate::desktop_hook::install(
            &session.config,
            &std::env::current_exe().map_err(|e| e.to_string())?,
        ) {
            let cleanup = crate::desktop_hook::deactivate(&session.config);
            return Err(match cleanup {
                Ok(()) => error,
                Err(cleanup) => format!("{error}；Hook收尾待核对：{cleanup}"),
            });
        }
        Ok(Self {
            root: root.to_path_buf(),
            session,
            base,
            evidence: Evidence {
                review_digest: review.id.clone(),
                events: vec![],
            },
            connected: false,
            finished: false,
            error: None,
            pending: None,
            deactivated: false,
            view: DesktopSupervisionView {
                state: "waitingForHooks".into(),
                ..Default::default()
            },
        })
    }
    fn observe(&mut self, root: &Path, incoming: Incoming) -> Result<(), String> {
        let Incoming { event, reply } = incoming;
        event.validate(&self.session.config)?;
        if event.name == "UserPromptSubmit" {
            if event.prompt_digest.as_deref() != Some(&self.base.review.prompt_sha256) {
                self.finished = true;
                self.error = Some("原会话收到另一条用户消息，监督投送已结束".into());
            } else if self.connected {
                self.finished = true;
                self.error = Some("同一会话开始了另一轮任务，原监督已结束".into());
            } else if !self.finished {
                crate::desktop_execution::publish(
                    &directory(root, &self.base.review.id)?,
                    "event-001",
                    &event,
                )?;
                self.evidence.events.push(event);
                self.connected = true;
            }
            if self.finished {
                if let Some(pending) = self.pending.take() {
                    let _ = pending.send(None);
                }
            }
            let _ = reply.send(None);
            return Ok(());
        }
        if !self.connected || self.finished || self.error.is_some() {
            let _ = reply.send(None);
            return Ok(());
        }
        if self
            .evidence
            .events
            .first()
            .is_none_or(|first| first.agent_id != event.agent_id)
        {
            let _ = reply.send(None);
            return Ok(());
        }
        if matches!(event.name.as_str(), "Stop" | "StopFailure" | "SessionEnd") {
            self.finished = true;
            if let Some(pending) = self.pending.take() {
                let _ = pending.send(None);
            }
            let _ = reply.send(None);
            return Ok(());
        }
        if let Some(previous) = self
            .evidence
            .events
            .iter()
            .find(|old| old.name == event.name && old.tool_id == event.tool_id)
        {
            require(previous == &event, "同一Hook事件出现不同内容，监督需要核对")?;
            let _ = reply.send(None);
            return Ok(());
        }
        require(self.evidence.events.len() < 128, "桌面监督事件达到容量上限")?;
        let sequence = self.evidence.events.len() + 1;
        crate::desktop_execution::publish(
            &directory(root, &self.base.review.id)?,
            &format!("event-{sequence:03}"),
            &event,
        )?;
        self.evidence.events.push(event.clone());
        let needs_advice = self.base.policy.delivery_enabled
            && self.base.policy.max_advice_per_attempt > 0
            && incidents(&self.evidence, &self.base.policy)?
                .iter()
                .any(|incident| incident_is_current(&self.evidence, incident));
        if !needs_advice {
            if let Some(previous) = self.pending.take() {
                let _ = previous.send(None);
            }
        }
        if needs_advice && matches!(event.name.as_str(), "PreToolUse" | "PostToolUseFailure") {
            if let Some(previous) = self.pending.replace(reply) {
                let _ = previous.send(None);
            }
        } else {
            let _ = reply.send(None);
        }
        Ok(())
    }
    pub(in crate::host) fn poll(&mut self, root: &Path) {
        while let Ok(notice) = self.session.receiver.try_recv() {
            let result = match notice {
                Notice::Event(incoming) => self.observe(root, *incoming),
                Notice::OutputReleased { operation_id } => {
                    self.record_output(root, &operation_id, "outputReleased")
                }
                Notice::OutputUnknown { operation_id } => {
                    self.record_output(root, &operation_id, "unknown")
                }
            };
            if let Err(error) = result {
                self.error = Some(error);
                if let Some(reply) = self.pending.take() {
                    let _ = reply.send(None);
                }
            }
        }
        if self.finished || self.error.is_some() {
            if let Err(error) = self.finish(root) {
                self.error = Some(error);
            }
        }
        self.view.connected = self.connected;
        self.view.events = self.evidence.events.len() as u32;
        self.view.failures = self
            .evidence
            .events
            .iter()
            .filter(|event| event.name == "PostToolUseFailure")
            .count() as u32;
        self.view.incidents =
            incidents(&self.evidence, &self.base.policy).map_or(0, |values| values.len() as u32);
        self.view.error = self.error.clone();
        if self.error.is_some() {
            self.view.state = "failed".into();
        } else if self.finished {
            self.view.state = "stopped".into();
        } else if self.connected && self.view.state == "waitingForHooks" {
            self.view.state = "collecting".into();
        }
    }
    pub(in crate::host) fn finish(&mut self, root: &Path) -> Result<(), String> {
        self.finished = true;
        if let Some(reply) = self.pending.take() {
            let _ = reply.send(None);
        }
        if !self.deactivated {
            crate::desktop_hook::deactivate(&self.session.config)?;
            let path = directory(root, &self.base.review.id)?;
            if !path.join("ended.json").exists() {
                crate::desktop_execution::publish(
                    &path,
                    "ended",
                    &serde_json::json!({
                        "reviewDigest": self.base.review.id,
                        "events": self.evidence.events.len(),
                        "evidenceDigest": digest(&self.evidence)?,
                        "error": self.error,
                        "state": "stopped"
                    }),
                )?;
            }
            self.deactivated = true;
        }
        self.view.state = "stopped".into();
        Ok(())
    }
    fn record_output(&mut self, root: &Path, operation: &str, state: &str) -> Result<(), String> {
        identifier(operation)?;
        let path = directory(root, &self.base.review.id)?;
        let reserved: serde_json::Value =
            crate::desktop_execution::load(&path, &format!("delivery-{operation}"))?;
        require(
            reserved["reviewDigest"] == self.base.review.id,
            "Hook输出回执不属于原任务",
        )?;
        crate::desktop_execution::publish(
            &path,
            &format!("output-{operation}"),
            &serde_json::json!({"operationId":operation,"state":state,"reservationDigest":digest(&reserved)?}),
        )?;
        self.view.state = state.into();
        Ok(())
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        if !self.deactivated {
            // Normal host teardown may drop an active attempt before its UI
            // polling callback. Preserve evidence and disable only its hook.
            let root = self.root.clone();
            let _ = self.finish(&root);
        }
    }
}

impl RuntimeHost {
    pub(super) fn poll_desktop_supervision(&mut self, run: &ProjectRun) -> Result<(), String> {
        let Some(active) = self
            .desktop_active
            .as_mut()
            .filter(|active| active.snapshot.project.id == run.project_id)
        else {
            return Ok(());
        };
        let Some(live) = active.supervision.as_mut() else {
            return Ok(());
        };
        live.poll(&self.root);
        active
            .status
            .progress
            .get_or_insert_with(Default::default)
            .supervision = Some(live.view.clone());
        if live.finished
            || !live.connected
            || live.error.is_some()
            || active.status.stopping
            || active.status.reading
        {
            return Ok(());
        }
        let evidence = live.evidence.clone();
        let policy = live.base.policy.clone();
        let originals = self.management_original_intents(&run.project_id);
        if let Some(operation) = originals
            .values()
            .find(|intent| {
                binding(intent).is_ok_and(|bound| {
                    bound.is_some_and(|bound| bound.review_digest == evidence.review_digest)
                }) && self.management.application(&intent.operation_id).is_none()
            })
            .map(|intent| intent.operation_id.clone())
        {
            return self.deliver_desktop_supervision(&operation);
        }
        if policy.max_advice_per_attempt == 0 {
            return Ok(());
        }
        if originals
            .values()
            .filter(|intent| {
                binding(intent).is_ok_and(|bound| {
                    bound.is_some_and(|bound| bound.review_digest == evidence.review_digest)
                })
            })
            .count()
            >= policy.max_advice_per_attempt as usize
        {
            return Ok(());
        }
        if self
            .management
            .statuses()
            .iter()
            .any(|status| status.project_id == run.project_id && !status.state.resolved())
            || self.counting_project(&run.project_id)
            || self.management_work_count() >= 4
        {
            return Ok(());
        }
        let known = self.management_operation_ids(&run.project_id);
        let Some(incident) = incidents(&evidence, &policy)?.into_iter().find(|incident| {
            incident_is_current(&evidence, incident)
                && !known.contains(&format!("advice-{}", &incident.evidence_digest[..48]))
        }) else {
            return Ok(());
        };
        let active = self.desktop_active.as_ref().ok_or("桌面监督执行者缺失")?;
        let live = active.supervision.as_ref().ok_or("桌面监督通道缺失")?;
        if policy.delivery_enabled && live.pending.as_ref().is_none_or(oneshot::Sender::is_closed) {
            return Ok(());
        }
        let setup = self
            .project_setups
            .get(&run.project_id)
            .ok_or("桌面监督配置缺失")?;
        require(
            run.state == RunState::Active
                && setup.digest == run.setup_digest
                && originals.len() < setup.setup.management_max_requests as usize,
            "桌面监督运行授权或请求额度无效",
        )?;
        let mut context = self.context_for_snapshot(&setup.setup.requirement, &active.snapshot);
        let bound = Binding {
            version: 5,
            kind: "desktop".into(),
            run_id: run.id.clone(),
            run_digest: digest(run)?,
            task_id: active.status.review.attempt.task_id.clone(),
            attempt_id: active.status.review.attempt.id.clone(),
            review_digest: evidence.review_digest.clone(),
            incident_id: incident.id,
            observation_digest: digest(&evidence)?,
        };
        context["supervision"] = serde_json::to_value(bound).map_err(|e| e.to_string())?;
        context["supervisionEvidence"] =
            serde_json::to_value(evidence).map_err(|e| e.to_string())?;
        context["supervisionFailureContext"] = serde_json::json!({"source":"qoder-supported-hooks","rule":"Only advise about the observed repeated tool failure within this task. Keep original task, file, tool and permission boundaries. Hook input hashes are diagnostic observations, not task completion evidence."});
        self.require_management_connection(&run.project_id, Purpose::Advise)?;
        let intent = DecisionIntent {
            operation_id: format!("advice-{}", &incident.evidence_digest[..48]),
            project_id: run.project_id.clone(),
            purpose: Purpose::Advise,
            model: self.project_setups[&run.project_id]
                .setup
                .deputy_model
                .clone(),
            state_digest: digest(&context)?,
            context,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        self.launch_routed_management(intent)?;
        if let Some(live) = self
            .desktop_active
            .as_mut()
            .and_then(|active| active.supervision.as_mut())
        {
            live.view.state = "advicePending".into();
        }
        Ok(())
    }

    pub(super) fn revalidate_desktop_supervision(
        &self,
        intent: &DecisionIntent,
    ) -> Result<(), String> {
        intent.validate()?;
        let bound = binding(intent)?.ok_or("桌面监督绑定缺失")?;
        let active = self
            .desktop_active
            .as_ref()
            .filter(|active| {
                active.snapshot.project.id == intent.project_id
                    && active.status.review.id == bound.review_digest
            })
            .ok_or("原桌面任务已结束")?;
        let live = active.supervision.as_ref().ok_or("原桌面监督通道已关闭")?;
        require(
            live.connected
                && !live.finished
                && live.error.is_none()
                && !active.status.stopping
                && !active.status.reading
                && !active.has_pending_stop(),
            "原桌面监督已停止或需要核对",
        )?;
        let evidence: Evidence =
            serde_json::from_value(intent.context["supervisionEvidence"].clone())
                .map_err(|_| "桌面监督证据缺失")?;
        require(
            digest(&evidence)? == bound.observation_digest
                && live.evidence.events.starts_with(&evidence.events)
                && evidence.review_digest == bound.review_digest,
            "桌面监督原事件已变化",
        )?;
        let run = self
            .runs
            .all()
            .into_iter()
            .find(|run| run.id == bound.run_id && run.state == RunState::Active)
            .ok_or("桌面监督运行授权已结束")?;
        require(digest(&run)? == bound.run_digest, "桌面监督运行授权已变化")?;
        let policy = super::supervision_requests::request_policy(intent)?;
        require(
            policy == live.base.policy && policy.max_advice_per_attempt > 0,
            "桌面监督策略与批准不符",
        )?;
        require(
            incidents(&evidence, &policy)?.iter().any(|incident| {
                incident.id == bound.incident_id
                    && intent.operation_id == format!("advice-{}", &incident.evidence_digest[..48])
                    && incident_is_current(&live.evidence, incident)
            }),
            "桌面监督异常不符",
        )?;
        let setup = self
            .project_setups
            .get(&intent.project_id)
            .ok_or("桌面监督配置缺失")?;
        require(
            setup.digest == run.setup_digest && setup.setup.deputy_model == intent.model,
            "桌面副指挥配置已变化",
        )?;
        let mut context = self.context_for_snapshot(&setup.setup.requirement, &active.snapshot);
        for field in [
            "supervision",
            "supervisionEvidence",
            "supervisionFailureContext",
        ] {
            context[field] = intent.context[field].clone();
        }
        require(
            digest(&self.match_original_catalog(&intent.context, context)?)? == intent.state_digest,
            "桌面监督上下文已变化",
        )
    }

    fn deliver_desktop_supervision(&mut self, operation: &str) -> Result<(), String> {
        let Some(status) = self
            .management
            .statuses()
            .into_iter()
            .find(|status| status.operation_id == operation)
        else {
            return Ok(());
        };
        if status.state != DecisionState::Ready {
            return Ok(());
        }
        let intent = self.management.intent(operation)?.clone();
        let bound = binding(&intent)?.ok_or("桌面监督绑定缺失")?;
        if let Some(live) = self
            .desktop_active
            .as_ref()
            .and_then(|a| a.supervision.as_ref())
        {
            let original: Evidence =
                serde_json::from_value(intent.context["supervisionEvidence"].clone())
                    .map_err(|_| "桌面监督证据缺失")?;
            require(
                digest(&original)? == bound.observation_digest,
                "桌面监督原证据摘要不符",
            )?;
            if incidents(&original, &live.base.policy)?
                .iter()
                .any(|incident| {
                    incident.id == bound.incident_id
                        && !incident_is_current(&live.evidence, incident)
                })
            {
                // Keep the original management/fee evidence for user disposition.
                // An obsolete suggestion does not stop a worker that recovered.
                return Ok(());
            }
        }
        self.revalidate_desktop_supervision(&intent)?;
        let policy = super::supervision_requests::request_policy(&intent)?;
        let envelope = self.management.original_envelope(operation)?.clone();
        let ManagementDecision::Advise {
            task_id,
            suggestion,
            ..
        } = envelope.decision
        else {
            return Ok(());
        };
        require(
            task_id == bound.task_id
                && !suggestion.trim().is_empty()
                && suggestion.len() <= 16 * 1024,
            "桌面监督建议目标或长度不符",
        )?;
        let live = self
            .desktop_active
            .as_mut()
            .and_then(|active| active.supervision.as_mut())
            .ok_or("桌面监督通道缺失")?;
        if policy.delivery_enabled && live.pending.as_ref().is_none_or(oneshot::Sender::is_closed) {
            return Ok(());
        }
        let target = ApplicationTarget::DesktopHook {
            task_id: task_id.clone(),
            attempt_id: bound.attempt_id.clone(),
            review_digest: bound.review_digest.clone(),
            suggestion_digest: digest(&suggestion)?,
        };
        let fee = self.management_fee_ack(operation)?;
        self.management.prepare_application_with_fee(
            operation,
            target,
            &intent.state_digest,
            fee,
        )?;
        let reservation = serde_json::json!({"operationId":operation,"taskId":task_id,"attemptId":bound.attempt_id,"reviewDigest":bound.review_digest,"suggestionDigest":digest(&suggestion)?,"deliveryEnabled":policy.delivery_enabled});
        let path = directory(&self.root, &bound.review_digest)?;
        crate::desktop_execution::publish(&path, &format!("delivery-{operation}"), &reservation)?;
        self.management
            .record_applied(operation, Some(digest(&reservation)?))?;
        let live = self
            .desktop_active
            .as_mut()
            .and_then(|active| active.supervision.as_mut())
            .ok_or("桌面监督通道缺失")?;
        if policy.delivery_enabled {
            let sender = live.pending.take().ok_or("原Hook投送窗口缺失")?;
            if sender
                .send(Some(Advice {
                    operation_id: operation.into(),
                    text: suggestion,
                }))
                .is_err()
            {
                live.record_output(&self.root, operation, "unknown")?;
            }
        } else {
            live.record_output(&self.root, operation, "recordOnly")?;
        }
        Ok(())
    }

    pub(super) fn desktop_supervision_advice_view(
        &self,
        intent: &DecisionIntent,
    ) -> Result<SupervisionAdviceView, String> {
        let bound = binding(intent)?.ok_or("桌面监督绑定缺失")?;
        let (suggestion, rationale) = match self
            .management
            .original_envelope(&intent.operation_id)
            .ok()
            .map(|envelope| &envelope.decision)
        {
            Some(ManagementDecision::Advise {
                suggestion,
                rationale,
                ..
            }) => (Some(suggestion.clone()), Some(rationale.clone())),
            _ => (None, None),
        };
        let path = directory(&self.root, &bound.review_digest)?;
        let output = format!("output-{}", intent.operation_id);
        let current = self.revalidate_desktop_supervision(intent).is_ok();
        let state = if path.join(format!("{output}.json")).exists() {
            let value: serde_json::Value = crate::desktop_execution::load(&path, &output)?;
            value["state"].as_str().unwrap_or("unknown").to_owned()
        } else if self.management.application(&intent.operation_id).is_some() {
            "unknown".into()
        } else if !current {
            "expired".into()
        } else {
            "awaitingHook".into()
        };
        Ok(SupervisionAdviceView {
            omp: None,
            api: None,
            incident_id: bound.incident_id,
            task_id: bound.task_id,
            attempt_id: bound.attempt_id,
            thread_id: String::new(),
            turn_id: bound.review_digest,
            observation_digest: bound.observation_digest,
            suggestion,
            rationale,
            current,
            delivered: false,
            delivery_state: Some(format!("desktopHook:{state}")),
        })
    }

    pub(super) fn validate_desktop_advice_application(
        &self,
        operation: &str,
    ) -> Result<Option<String>, String> {
        let intent = self.management.intent(operation)?;
        let bound = binding(intent)?.ok_or("桌面监督绑定缺失")?;
        let evidence: Evidence =
            serde_json::from_value(intent.context["supervisionEvidence"].clone())
                .map_err(|_| "桌面监督证据无效")?;
        let path = directory(&self.root, &bound.review_digest)?;
        let base: Base = crate::desktop_execution::load(&path, "scope")?;
        require(
            base.review.id == bound.review_digest
                && base.review.attempt.id == bound.attempt_id
                && base.review.project_id == intent.project_id
                && digest(&evidence)? == bound.observation_digest,
            "桌面监督历史范围不符",
        )?;
        let original: ExecutionSnapshot =
            serde_json::from_value(intent.context["workflow"].clone())
                .map_err(|_| "桌面监督原工作流缺失")?;
        if let Some(store) = self.projects.get(&intent.project_id) {
            let (review, _) = crate::desktop_execution::inspect(
                &self.root,
                store,
                &intent.project_id,
                &bound.attempt_id,
            )?;
            require(
                review == base.review
                    && digest(&crate::workflow_prefix::prefix(store, original.sequence)?)?
                        == digest(&original)?,
                "桌面监督派发身份与原批准记录不符",
            )?;
        } else {
            let active = self.desktop_active.as_ref().ok_or("桌面监督原执行者缺失")?;
            require(
                active.status.review == base.review
                    && digest(&active.snapshot)? == digest(&original)?,
                "桌面监督活动执行身份不符",
            )?;
        }
        require(
            self.runs
                .has_authorization(&bound.run_id, &bound.run_digest),
            "桌面监督原运行授权缺失",
        )?;
        require(
            evidence.events.len() <= 128
                && evidence.events.first().is_some_and(|event| {
                    event.name == "UserPromptSubmit"
                        && event.prompt_digest.as_deref()
                            == Some(base.review.prompt_sha256.as_str())
                }),
            "桌面监督缺少原任务消息回执",
        )?;
        for (index, event) in evidence.events.iter().enumerate() {
            let saved: Event =
                crate::desktop_execution::load(&path, &format!("event-{:03}", index + 1))?;
            require(saved == *event, "桌面监督历史事件不符")?;
        }
        let policy = super::supervision_requests::request_policy(intent)?;
        require(
            policy == base.policy
                && incidents(&evidence, &policy)?
                    .iter()
                    .any(|incident| incident.id == bound.incident_id),
            "桌面监督历史策略或异常不符",
        )?;
        let name = format!("delivery-{operation}");
        if !path.join(format!("{name}.json")).exists()
            && !path.join(format!("{name}.pending")).exists()
        {
            return Ok(None);
        }
        let reservation: serde_json::Value = crate::desktop_execution::load(&path, &name)?;
        let envelope = self.management.original_envelope(operation)?;
        let ManagementDecision::Advise {
            task_id,
            suggestion,
            ..
        } = &envelope.decision
        else {
            return Err("原桌面监督决定无效".into());
        };
        require(
            reservation["operationId"] == operation
                && reservation["reviewDigest"] == bound.review_digest
                && reservation["attemptId"] == bound.attempt_id
                && reservation["taskId"] == *task_id
                && reservation["suggestionDigest"] == digest(suggestion)?
                && reservation["deliveryEnabled"] == policy.delivery_enabled,
            "桌面监督投送保留记录不符",
        )?;
        Ok(Some(digest(&reservation)?))
    }

    pub(super) fn validate_desktop_supervision_requests(&self) -> Result<(), String> {
        for status in self.management.statuses() {
            if binding(self.management.intent(&status.operation_id)?)?.is_some() {
                self.validate_desktop_advice_application(&status.operation_id)?;
            }
        }
        Ok(())
    }

    pub(super) fn require_resolved_desktop_advice(&self, project: &str) -> Result<(), String> {
        for status in
            self.management.statuses().iter().filter(|status| {
                status.project_id == project && status.state == DecisionState::Applied
            })
        {
            let intent = self.management.intent(&status.operation_id)?;
            let Some(bound) = binding(intent)? else {
                continue;
            };
            if self
                .desktop_active
                .as_ref()
                .is_some_and(|active| active.status.review.id == bound.review_digest)
            {
                continue;
            }
            if self.advice_closures.get(&status.operation_id).is_some() {
                continue;
            }
            require(
                self.desktop_supervision_advice_view(intent)?
                    .delivery_state
                    .as_deref()
                    != Some("desktopHook:unknown"),
                "桌面Hook建议输出结果待核对，请在指挥记录中核对或结束本地跟踪",
            )?;
        }
        Ok(())
    }

    pub(super) fn desktop_advice_closure_proof(
        &self,
        operation: &str,
        sequence: u32,
        hash: &str,
    ) -> Result<crate::advice_closure::ClosureProof, String> {
        let intent = self.management.intent(operation)?;
        let bound = binding(intent)?.ok_or("桌面监督绑定缺失")?;
        let receipt = self
            .validate_desktop_advice_application(operation)?
            .ok_or("桌面建议应用记录缺失")?;
        let (_, applied, stored) = self
            .management
            .application(operation)
            .ok_or("桌面建议应用事务缺失")?;
        require(
            applied && stored == Some(receipt.as_str()),
            "桌面建议应用回执不符",
        )?;
        require(
            self.desktop_supervision_advice_view(intent)?
                .delivery_state
                .as_deref()
                == Some("desktopHook:unknown"),
            "桌面建议输出状态已明确",
        )?;
        let store = self
            .projects
            .get(&intent.project_id)
            .ok_or("请等待原桌面任务结束")?;
        require(
            sequence > 0
                && store
                    .history()
                    .get(sequence as usize - 1)
                    .is_some_and(|entry| entry.hash == hash),
            "桌面建议处置锚点不符",
        )?;
        let path = directory(&self.root, &bound.review_digest)?;
        let mut files = vec![];
        for stem in [
            format!("delivery-{operation}"),
            format!("output-{operation}"),
        ] {
            for extension in ["json", "pending"] {
                let file = path.join(format!("{stem}.{extension}"));
                if file.exists() {
                    ordinary(&file, false)?;
                    require(
                        std::fs::metadata(&file).map_err(|e| e.to_string())?.len() <= 131072,
                        "桌面建议处置文件过大",
                    )?;
                    files.push(Some(digest(
                        &std::fs::read(file).map_err(|e| e.to_string())?,
                    )?));
                } else {
                    files.push(None);
                }
            }
        }
        Ok(crate::advice_closure::ClosureProof {
            operation: operation.into(),
            project: intent.project_id.clone(),
            attempt: bound.attempt_id,
            management_digest: self.management.evidence_digest(operation)?.into(),
            delivery_digest: digest(&files)?,
            advice_event_hash: receipt,
            workflow_sequence: sequence,
            workflow_hash: hash.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event(id: &str, name: &str, action: &str) -> Event {
        Event {
            name: name.into(),
            session_id: "fixture".into(),
            agent_id: None,
            workspace: "D:/synthetic".into(),
            tool_id: Some(id.into()),
            tool_name: Some("Bash".into()),
            action_digest: Some(digest(&action).unwrap()),
            payload_digest: digest(&(id, name, action)).unwrap(),
            prompt_digest: None,
            interrupted: false,
        }
    }
    #[test]
    fn desktop_incidents_count_only_consecutive_matching_failed_operations() {
        let policy = SupervisionPolicy {
            schema_version: 1,
            repeated_failure_threshold: 2,
            max_advice_per_attempt: 1,
            delivery_enabled: true,
        };
        let mut evidence = Evidence {
            review_digest: "a".repeat(64),
            events: vec![
                event("one", "PostToolUseFailure", "same"),
                event("two", "PreToolUse", "same"),
                event("two", "PostToolUseFailure", "same"),
            ],
        };
        assert_eq!(incidents(&evidence, &policy).unwrap().len(), 1);
        let first = incidents(&evidence, &policy).unwrap()[0]
            .evidence_digest
            .clone();
        evidence
            .events
            .push(event("three", "PostToolUseFailure", "same"));
        assert_eq!(incidents(&evidence, &policy).unwrap().len(), 1);
        assert_eq!(
            incidents(&evidence, &policy).unwrap()[0].evidence_digest,
            first
        );
        evidence
            .events
            .insert(1, event("success", "PostToolUse", "same"));
        assert_eq!(incidents(&evidence, &policy).unwrap().len(), 1);
        evidence.events[2].interrupted = true; // A pre-tool observation does not count as a result.
        evidence.events[3].interrupted = true;
        assert!(incidents(&evidence, &policy).unwrap().is_empty());
        evidence.events = vec![
            event("one", "PostToolUseFailure", "first"),
            event("two", "PostToolUseFailure", "other"),
        ];
        assert!(incidents(&evidence, &policy).unwrap().is_empty());
    }

    #[test]
    fn advice_expires_after_success_changed_operation_or_conversation_boundary() {
        let policy = SupervisionPolicy {
            schema_version: 1,
            repeated_failure_threshold: 2,
            max_advice_per_attempt: 1,
            delivery_enabled: true,
        };
        let original = Evidence {
            review_digest: "a".repeat(64),
            events: vec![
                event("one", "PostToolUseFailure", "same"),
                event("two", "PostToolUseFailure", "same"),
            ],
        };
        let incident = incidents(&original, &policy).unwrap().remove(0);
        assert!(incident_is_current(&original, &incident));
        for (kind, action, expected) in [
            ("PreToolUse", "same", true),
            ("PostToolUseFailure", "same", true),
            ("PreToolUse", "other", false),
            ("PostToolUse", "same", false),
            ("PostToolUseFailure", "other", false),
            ("UserPromptSubmit", "same", false),
            ("Stop", "same", false),
            ("SessionEnd", "same", false),
        ] {
            let mut current = original.clone();
            current.events.push(event("next", kind, action));
            assert_eq!(
                incident_is_current(&current, &incident),
                expected,
                "{kind}/{action}"
            );
            current.events.last_mut().unwrap().interrupted = true;
            assert!(!incident_is_current(&current, &incident));
        }
    }
}
