//! User disposition of no-tool management calls. Never cancels or replays remote work.
use super::*;
use crate::management_execution::{DecisionState, UserResolution};

impl RuntimeHost {
    fn management_review_idle(&self, operation: &str) -> Result<(), String> {
        let intent = self.management.intent(operation)?;
        let project = &intent.project_id;
        self.require_available(project)?;
        require(
            !self
                .runs
                .all()
                .iter()
                .any(|r| r.project_id == *project && r.state == project_setup::RunState::Active),
            "请先暂停项目调度，再处理指挥请求",
        )?;
        require(
            !self.management_active.keys().any(|id| {
                self.management
                    .intent(id)
                    .is_ok_and(|i| i.project_id == *project)
            }),
            "项目指挥请求仍在运行，请等待其结束",
        )?;
        require(
            !self.management.statuses().iter().any(|s| {
                s.project_id == *project && s.operation_id != operation && !s.state.resolved()
            }),
            "项目存在另一条待处理指挥请求",
        )?;
        Ok(())
    }
    fn management_discardable(&self, operation: &str) -> Result<(), String> {
        self.management_review_idle(operation)?;
        let intent = self.management.intent(operation)?;
        require(
            self.management.application(operation).is_none(),
            "请求已经进入应用事务，请先对账",
        )?;
        require(
            !self
                .codex_jobs
                .values()
                .any(|j| j.management_operation.as_deref() == Some(operation)),
            "请求关联编程尝试，必须先处理该尝试",
        )?;
        let store = self
            .projects
            .get(&intent.project_id)
            .ok_or("项目账本不可用")?;
        require(
            !store
                .history()
                .iter()
                .any(|e| e.body.operation_id == format!("mgmt-{operation}")),
            "请求已有关联工作流事件，不能按未应用请求放弃",
        )?;
        let status = self
            .management
            .statuses()
            .into_iter()
            .find(|s| s.operation_id == operation)
            .ok_or("请求不存在")?;
        require(
            matches!(
                status.state,
                DecisionState::Unknown | DecisionState::Ready | DecisionState::AwaitingExternal
            ),
            "该请求不能放弃",
        )
    }
    fn management_replyable(&self, operation: &str) -> Result<(), String> {
        self.management_review_idle(operation)?;
        require(
            self.management.unanswered_wait(operation),
            "该请求没有等待用户回答的问题",
        )
    }
    pub(super) fn resolve_management(
        &mut self,
        operation: &str,
        expected: &str,
        resolution: UserResolution,
    ) -> Result<(), String> {
        match &resolution {
            UserResolution::AbandonApplication { .. } | UserResolution::AbandonStartup { .. } => {
                return Err("应用事务处置必须经过专门的跨账本审阅".into())
            }
            UserResolution::Discard { .. } => self.management_discardable(operation)?,
            UserResolution::Reply { answer } => {
                self.management_replyable(operation)?;
                let intent = self.management.intent(operation)?;
                let requirement = intent.context["requirement"]
                    .as_str()
                    .ok_or("request requirement missing")?;
                let mut base = self.rebuild_management_context(
                    intent,
                    self.management_context(&intent.project_id, requirement)?,
                )?;
                let object = base.as_object_mut().ok_or("context must be an object")?;
                object.remove("userResponses");
                object.remove("userResponsesOmitted");
                let (_, question, _, _) = self.management.review(operation)?;
                let candidate = serde_json::json!({"operationId":operation,"question":question,"answer":answer});
                require(
                    serde_json::to_vec(&base).map_err(|e| e.to_string())?.len()
                        + serde_json::to_vec(&candidate)
                            .map_err(|e| e.to_string())?
                            .len()
                        + 128
                        <= 64 * 1024,
                    "当前方案与本条答复超出指挥上下文容量，请缩短答复",
                )?;
            }
        }
        self.management.resolve(operation, expected, resolution)?;
        self.planning_errors.remove(operation);
        Ok(())
    }
    pub(super) fn management_view(
        &self,
        s: crate::management_execution::DecisionStatus,
    ) -> project_setup::ManagementView {
        let (evidence_digest, wait_reason, user_reply, discard_reason) = self
            .management
            .review(&s.operation_id)
            .expect("known management request");
        project_setup::ManagementView {
            refinement: self
                .management
                .original_envelope(&s.operation_id)
                .ok()
                .and_then(|e| {
                    if let cerulean_domain::management::ManagementDecision::Refine { document } =
                        &e.decision
                    {
                        Some(document.clone())
                    } else {
                        None
                    }
                }),
            supervision_advice: None,
            purpose: serde_json::to_value(s.purpose)
                .unwrap()
                .as_str()
                .unwrap()
                .into(),
            state: serde_json::to_value(s.state)
                .unwrap()
                .as_str()
                .unwrap()
                .into(),
            message: self
                .planning_errors
                .get(&s.operation_id)
                .cloned()
                .or_else(|| {
                    wait_reason
                        .as_ref()
                        .map(|reason| format!("指挥请求补充信息：{reason}"))
                }),
            can_discard: self.management_discardable(&s.operation_id).is_ok(),
            can_reply: self.management_replyable(&s.operation_id).is_ok(),
            operation_id: s.operation_id,
            project_id: s.project_id,
            evidence_digest,
            wait_reason,
            user_reply,
            discard_reason,
            application_recovery: None,
            advice_tracking_review: None,
            advice_tracking_closure: None,
        }
    }
    pub(super) fn management_detail(
        &self,
        operation: &str,
    ) -> Result<project_setup::ManagementView, String> {
        let status = self
            .management_decisions()
            .into_iter()
            .find(|s| s.operation_id == operation)
            .ok_or("指挥请求不存在")?;
        let mut view = self.management_view(status);
        if let Some(diagnostic) = self.management.decode_diagnostic_message(operation) {
            view.message = Some(match view.message.take() {
                Some(message) => format!("{message}\n{diagnostic}"),
                None => diagnostic,
            });
        }
        #[cfg(windows)]
        {
            view.supervision_advice = self.supervision_advice_view(operation)?;
            view.advice_tracking_review = self.advice_tracking_review(operation).ok();
        }
        view.application_recovery = self.application_recovery_review(operation).ok();
        view.advice_tracking_closure =
            self.advice_closures
                .get(operation)
                .map(|c| project_setup::AdviceTrackingClosure {
                    reason: c.reason.clone(),
                    at: c.at.clone(),
                });
        Ok(view)
    }
}
#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::management_execution::{DecisionIntent, UserResolution};
    use cerulean_domain::management::{ManagementDecision, Purpose};
    fn setup(root: &Path) -> RuntimeHost {
        let (mut host, mut draft) = super::super::project_planning::tests::fixture(root);
        draft.management_max_requests = 6;
        host.configure_project(draft).unwrap();
        host
    }
    fn intent(host: &RuntimeHost, id: &str) -> DecisionIntent {
        let context = host
            .management_context("project", "Implement a sum function")
            .unwrap();
        DecisionIntent {
            operation_id: id.into(),
            project_id: "project".into(),
            purpose: Purpose::Plan,
            model: "fixture-model".into(),
            state_digest: digest(&context).unwrap(),
            context,
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }
    #[test]
    fn active_request_cannot_be_discarded_and_finished_unknown_needs_fresh_review() {
        let root = tempfile::tempdir().unwrap();
        let mut host = setup(root.path());
        let i = intent(&host, "unknown-1");
        let (release, wait) = std::sync::mpsc::channel();
        host.launch_management(i, move |_| {
            wait.recv().unwrap();
            Err("fixture failure".into())
        })
        .unwrap();
        let view = host.snapshot().management_decisions[0].clone();
        assert!(!view.can_discard);
        assert!(host
            .resolve_management(
                "unknown-1",
                &view.evidence_digest,
                UserResolution::Discard {
                    reason: "user closes request".into()
                }
            )
            .is_err());
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !host.management_active.is_empty() && std::time::Instant::now() < deadline {
            host.poll_verifications();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(host.management_active.is_empty());
        assert!(host
            .resolve_management(
                "unknown-1",
                &view.evidence_digest,
                UserResolution::Discard {
                    reason: "stale review".into()
                }
            )
            .is_err());
        let current = host.snapshot().management_decisions[0].clone();
        assert!(current.can_discard);
        let result = host.handle(Request {
            id: "user-discard".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::DiscardManagementDecision {
                operation_id: "unknown-1".into(),
                evidence_digest: current.evidence_digest,
                reason: "Close local request; preserve unknown billing".into(),
            },
        });
        assert!(result.result.is_some());
        assert_eq!(host.management.statuses().len(), 1);
        assert!(host.snapshot().workflows[0].revision.is_none());
        drop(host);
        let mut host = RuntimeHost::open(&root.path().join("data")).unwrap();
        host.poll_verifications();
        let view = &host
            .management_detail(&host.snapshot().management_decisions[0].operation_id)
            .unwrap();
        assert_eq!(view.state, "discarded");
        assert!(!view.can_discard);
        assert!(view
            .discard_reason
            .as_ref()
            .unwrap()
            .contains("unknown billing"));
        assert_eq!(host.management.statuses().len(), 1);
    }
    #[test]
    fn wait_answer_is_persistent_context_but_never_approval_or_automatic_request() {
        let root = tempfile::tempdir().unwrap();
        let mut host = setup(root.path());
        super::super::project_planning::tests::complete(
            &mut host,
            ManagementDecision::Wait {
                reason: "请确认只处理数字输入".into(),
            },
        );
        let view = host.snapshot().management_decisions[0].clone();
        assert!(view.can_reply);
        assert!(!view.can_discard);
        let before = host
            .management_context("project", "Implement a sum function")
            .unwrap();
        assert!(before.get("userResponses").is_none());
        let result = host.handle(Request {
            id: "user-answer".into(),
            protocol_version: PROTOCOL_VERSION,
            command: Command::AnswerManagementWait {
                operation_id: view.operation_id,
                evidence_digest: view.evidence_digest,
                answer: "确认，仅数字输入，沿用原验收范围".into(),
            },
        });
        assert!(result.result.is_some());
        host.poll_verifications();
        assert_eq!(host.management.statuses().len(), 1);
        assert!(host.snapshot().workflows[0].approval.is_none());
        assert!(host.runs.all().is_empty());
        assert!(host.codex_jobs.is_empty());
        drop(host);
        let host = RuntimeHost::open(&root.path().join("data")).unwrap();
        let view = &host
            .management_detail(&host.snapshot().management_decisions[0].operation_id)
            .unwrap();
        assert!(!view.can_reply);
        assert_eq!(view.wait_reason.as_deref(), Some("请确认只处理数字输入"));
        assert_eq!(
            view.user_reply.as_deref(),
            Some("确认，仅数字输入，沿用原验收范围")
        );
        let after = host
            .management_context("project", "Implement a sum function")
            .unwrap();
        assert_eq!(
            after["userResponses"][0]["answer"],
            "确认，仅数字输入，沿用原验收范围"
        );
        assert_ne!(digest(&before).unwrap(), digest(&after).unwrap());
    }
    #[test]
    #[ignore = "isolated native management review fixture, no provider calls"]
    fn seed_native_management_review_fixture() {
        let root = PathBuf::from(std::env::var_os("CERULEAN_MANAGEMENT_NATIVE_DIR").unwrap())
            .canonicalize()
            .unwrap();
        let reports = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../reports")
            .canonicalize()
            .unwrap();
        assert!(root.starts_with(reports) && !root.join("data").exists());
        let mut host = setup(&root);
        super::super::project_planning::tests::complete(
            &mut host,
            ManagementDecision::Wait {
                reason: "请确认只处理数字输入".into(),
            },
        );
        let i = intent(&host, "unknown-native");
        host.management.begin(i).unwrap();
        // Unresolved second request intentionally disables answering until it is discarded.
        assert!(
            !host
                .snapshot()
                .management_decisions
                .iter()
                .find(|d| d.has_wait)
                .unwrap()
                .can_reply
        );
    }
}
