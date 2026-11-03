//! Document-only chief recommendations. Reuses management credentials, budgets and evidence.
use super::*;
use crate::management_execution::{ApplicationTarget, DecisionIntent, DecisionState};
use cerulean_domain::management::{
    DocumentRefinementRequest, ManagementDecision, Purpose, RefinedDocument,
};

pub(crate) fn validate_quotes(
    intent: &DecisionIntent,
    document: &RefinedDocument,
) -> Result<(), String> {
    let original = intent.context["document"]["text"]
        .as_str()
        .ok_or("文档原文缺失")?;
    require(
        document
            .points
            .iter()
            .all(|point| !point.quote.trim().is_empty() && original.contains(&point.quote)),
        "提炼结果引用无法在原文中定位，请保留原记录核对",
    )
}

impl RuntimeHost {
    pub(super) fn document_refinement_context(
        &self,
        project: &str,
        input: DocumentRefinementRequest,
    ) -> Result<serde_json::Value, String> {
        identifier(&input.operation_id)?;
        cerulean_domain::execution::sha256(&input.document_digest)?;
        let saved = self
            .project_setups
            .get(project)
            .ok_or("请先保存项目主指挥配置")?;
        require(
            input.project_id == project && input.setup_digest == saved.digest,
            "主指挥配置已变化，请重新选择文档",
        )?;
        require(
            input.total > 0
                && input.part < input.total
                && !input.text.trim().is_empty()
                && input.text.len() <= 6000
                && !input.text.contains(char::from(0)),
            "文档分段无效；长文档须完整分段，不会截断",
        )?;
        require(!matches!(saved.setup.management_sources.as_ref().map(|s| &s.chief), Some(project_setup::ChiefSource::CodexSession { .. })), "当前主指挥绑定外部 Codex 聊天，无法后台自动提炼；请在项目配置中选择 API 主指挥后导入，不会改用其他模型")?;
        Ok(
            serde_json::json!({"requirement":serde_json::to_string(&input).map_err(|e|e.to_string())?,"document":input,"chiefModel":saved.setup.chief_model,"chiefSource":saved.setup.management_sources.as_ref().map(|s| &s.chief)}),
        )
    }
    pub(super) fn rebuild_refinement_context(
        &self,
        intent: &DecisionIntent,
    ) -> Result<serde_json::Value, String> {
        let input: DocumentRefinementRequest =
            serde_json::from_value(intent.context["document"].clone())
                .map_err(|_| "原提炼分段不可读")?;
        require(
            input.operation_id == intent.operation_id,
            "提炼请求身份不符",
        )?;
        let context = self.document_refinement_context(&intent.project_id, input)?;
        require(
            context == intent.context,
            "文档提炼依据已变化，不能应用旧结果",
        )?;
        Ok(context)
    }
    pub(super) fn request_document_refinement(
        &mut self,
        input: DocumentRefinementRequest,
    ) -> Result<(), String> {
        self.require_management_planning_available(&input.project_id)?;
        self.planning_idle(&input.project_id)?;
        self.document_refinement_context(&input.project_id, input.clone())?;
        let model = self.project_setups[&input.project_id]
            .setup
            .chief_model
            .clone();
        #[cfg(windows)]
        {
            self.start_management_decision(
                &input.project_id,
                &input.operation_id,
                Purpose::Refine,
                &model,
                &serde_json::to_string(&input).map_err(|e| e.to_string())?,
            )
        }
        #[cfg(not(windows))]
        {
            let _ = (input, model);
            Err("模型提炼需要 Windows 本机凭证".into())
        }
    }
    pub(super) fn poll_document_refinements(&mut self) {
        let ready: Vec<_> = self
            .management
            .statuses()
            .into_iter()
            .filter(|s| {
                s.purpose == Purpose::Refine
                    && s.state == DecisionState::Ready
                    && !self.planning_errors.contains_key(&s.operation_id)
            })
            .map(|s| s.operation_id)
            .collect();
        for operation in ready {
            let result = (|| {
                if let ManagementDecision::Refine { document } =
                    &self.management_envelope(&operation)?.decision
                {
                    validate_quotes(self.management.intent(&operation)?, document)?;
                }
                // This is a no-workflow journal receipt. It never creates or approves a plan.
                self.apply_management_decision_inner(&operation, ApplicationTarget::Wait)
            })();
            if let Err(error) = result {
                self.planning_errors.insert(operation, error);
            }
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use cerulean_adapters::openai::DecisionObservation;
    use cerulean_domain::management::{DecisionEnvelope, RefinedKind, RefinedPoint};
    fn reply(intent: DecisionIntent) -> Result<DecisionObservation, String> {
        let input: DocumentRefinementRequest =
            serde_json::from_value(intent.context["document"].clone()).unwrap();
        Ok(DecisionObservation {
            requested_model: intent.model.clone(),
            actual_model: intent.model.clone(),
            response_id: "resp_fixture_refinement".into(),
            envelope: DecisionEnvelope {
                state_digest: intent.state_digest,
                decision: ManagementDecision::Refine {
                    document: RefinedDocument {
                        title: "CSV 项目".into(),
                        points: vec![RefinedPoint {
                            kind: RefinedKind::Acceptance,
                            text: "校验全部 CSV 数据".into(),
                            quote: input.text,
                        }],
                    },
                },
            },
            input_tokens: 10,
            output_tokens: 20,
        })
    }
    fn input(host: &RuntimeHost, id: &str) -> DocumentRefinementRequest {
        DocumentRefinementRequest {
            project_id: "project".into(),
            setup_digest: host.project_setups["project"].digest.clone(),
            operation_id: id.into(),
            document_digest: "a".repeat(64),
            part: 0,
            total: 1,
            text: "必须校验全部 CSV 数据".into(),
        }
    }
    #[test]
    fn document_refinement_is_recorded_once_without_plan_or_approval_and_reopens() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, mut draft) = project_planning::tests::fixture(root.path());
        draft.management_max_requests = 1;
        host.configure_project(draft).unwrap();
        host.management_fixture = Some(reply);
        host.openai_connection.key_configured = true;
        let original = serde_json::to_value((
            host.projects["project"].state(),
            host.projects["project"].history(),
            host.projects["project"].approval(),
        ))
        .unwrap();
        let request = input(&host, "md-fixture-0");
        host.request_document_refinement(request.clone()).unwrap();
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            host.poll_management();
            host.poll_document_refinements();
            if host
                .management
                .application("md-fixture-0")
                .is_some_and(|(_, applied, _)| applied)
            {
                break;
            }
            assert!(
                std::time::Instant::now() < end,
                "refinement did not finish: {:?} {:?}",
                host.planning_errors,
                host.management_decisions()
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(
            serde_json::to_value((
                host.projects["project"].state(),
                host.projects["project"].history(),
                host.projects["project"].approval()
            ))
            .unwrap(),
            original
        );
        assert!(host
            .management_detail("md-fixture-0")
            .unwrap()
            .refinement
            .is_some());
        assert!(host.request_document_refinement(request).is_err());
        assert!(host
            .request_document_refinement(input(&host, "md-fixture-1"))
            .unwrap_err()
            .contains("上限"));
        drop(host);
        let host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert!(host
            .management_detail("md-fixture-0")
            .unwrap()
            .refinement
            .is_some());
    }
    #[test]
    fn document_refinement_rejects_stale_binding_forged_quote_and_non_refinement_use() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, draft) = project_planning::tests::fixture(root.path());
        host.configure_project(draft).unwrap();
        let mut request = input(&host, "md-guard");
        request.setup_digest = "b".repeat(64);
        assert!(host.request_document_refinement(request).is_err());
        let context = host
            .document_refinement_context("project", input(&host, "md-guard"))
            .unwrap();
        let intent = DecisionIntent {
            operation_id: "md-guard".into(),
            project_id: "project".into(),
            purpose: Purpose::Refine,
            model: "fixture-chief".into(),
            state_digest: digest(&context).unwrap(),
            context,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        let mut observed = reply(intent.clone()).unwrap();
        if let ManagementDecision::Refine { document } = &mut observed.envelope.decision {
            document.points[0].quote = "invented quote".into();
            assert!(validate_quotes(&intent, document).is_err());
        }
        assert!(observed
            .envelope
            .validate(Purpose::Plan, &intent.state_digest)
            .is_err());
        assert!(host.management_operation_ids("project").is_empty());
    }
    #[test]
    fn large_document_requirements_survive_save_reload_and_chief_material_read() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, mut draft) = project_planning::tests::fixture(root.path());
        let text = "文档需求与验收标准\n".repeat(60_000);
        draft.requirement = text.clone();
        host.configure_project(draft).unwrap();
        drop(host);
        let host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert_eq!(host.project_setups["project"].setup.requirement, text);
        let chief = root.path().join("chief");
        fs::create_dir(&chief).unwrap();
        let value = serde_json::json!({"requirement":text});
        let key = digest(&value).unwrap();
        crate::role_workspace::publish(chief.to_str().unwrap(), "requirements", &key, &value)
            .unwrap();
        assert_eq!(
            crate::role_workspace::read(chief.to_str().unwrap(), "requirements", &key).unwrap(),
            value
        );
    }
}
