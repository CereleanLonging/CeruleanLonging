//! Add explicit execution identities only to new provider planning/assignment requests.
//! Old contexts and wire instructions remain byte compatible.
use super::*;
use crate::management_execution::DecisionIntent;
use serde_json::{json, Value};

#[cfg(all(test, windows))]
#[path = "management_assignments/planning_tests.rs"]
mod planning_tests;

pub(super) fn add_bindings(mut context: Value) -> Result<Value, String> {
    let snapshot: ExecutionSnapshot = serde_json::from_value(context["workflow"].clone())
        .map_err(|_| "assignment workflow context invalid")?;
    let revision = snapshot
        .revision
        .as_ref()
        .ok_or("assignment revision missing")?;
    let tasks = revision.execution.iter().map(|task| {
        let adapter: project_setup::ProjectWorkerAdapter =
            serde_json::from_value(Value::String(task.adapter_id.clone()))
                .map_err(|_| "assignment adapter invalid")?;
        Ok(json!({"taskId":task.task_id,"workerId":adapter.worker_id(),"adapterId":task.adapter_id}))
    }).collect::<Result<Vec<_>, String>>()?;
    context["assignmentBindings"] = json!({"schemaVersion":1,
        "rule":"For Assign, copy the exact taskId and workerId pair below. adapterId identifies an adapter, not a worker. These bindings do not grant execution permission.",
        "tasks":tasks});
    Ok(context)
}

fn add_planning_bindings(mut context: Value) -> Result<Value, String> {
    let saved: project_setup::SavedProjectSetup =
        serde_json::from_value(context["executionSetup"].clone())
            .map_err(|_| "planning execution setup invalid")?;
    let setup = &saved.setup;
    let profiles = setup
        .profiles
        .iter()
        .map(|profile| {
            let validator = profile
                .acceptance
                .stages
                .first()
                .ok_or("planning profile validator missing")?;
            Ok(
                json!({"profileId":profile.id,"adapterId":profile.worker_adapter.id(),
            "workerId":profile.worker_adapter.worker_id(),"validatorId":validator.validator_id}),
            )
        })
        .collect::<Result<Vec<_>, String>>()?;
    context["planningBindings"] = json!({
        "schemaVersion":1,
        "default":{"adapterId":setup.worker_adapter.id(),"workerId":setup.worker_adapter.worker_id(),
            "validatorId":setup.validator_id},
        "profiles":profiles,
        "integration":setup.integration().map(|i| json!({"taskId":i.task_id,"validatorId":i.validator_id})),
        "rule":"For Plan, copy workerId and validatorId from the selected binding. adapterId is not workerId. Omit profileId for the default binding; otherwise copy the selected profileId. The integration task uses its integration validatorId with the selected worker binding. If the requirement specifies exact tasks, preserve their complete goals, IDs, dependencies and profile selections verbatim; do not summarize or substitute them. These bindings grant no execution or approval permission."
    });
    Ok(context)
}

pub(super) fn rebuild(intent: &DecisionIntent, mut context: Value) -> Result<Value, String> {
    if let (Some(original), Some(current)) = (intent.context.get("workers"), context.get("workers"))
    {
        if original != current {
            let legacy =
                serde_json::to_value(cerulean_adapters::catalog()).map_err(|e| e.to_string())?;
            let extended = serde_json::to_value(cerulean_adapters::current_catalog())
                .map_err(|e| e.to_string())?;
            require(
                original == &legacy && current == &extended,
                "management worker catalog changed beyond the known reader16 extension",
            )?;
            context["workers"] = legacy;
        }
    }
    if let Some(original) = intent.context.get("assignmentBindings") {
        require(
            intent.purpose == management::Purpose::Assign,
            "assignment bindings require Assign purpose",
        )?;
        let current = add_bindings(context)?;
        require(
            original == &current["assignmentBindings"],
            "approved assignment bindings changed",
        )?;
        context = current;
    }
    if let Some(original) = intent.context.get("planningBindings") {
        require(
            intent.purpose == management::Purpose::Plan,
            "planning bindings require Plan purpose",
        )?;
        let current = add_planning_bindings(context)?;
        require(
            original == &current["planningBindings"],
            "approved planning bindings changed",
        )?;
        Ok(current)
    } else {
        // Absence is the original context format, including already-pending requests.
        Ok(context)
    }
}

impl RuntimeHost {
    pub(super) fn match_original_catalog(
        &self,
        original: &Value,
        mut context: Value,
    ) -> Result<Value, String> {
        if original.get("workers") == context.get("workers") {
            return Ok(context);
        }
        let legacy =
            serde_json::to_value(cerulean_adapters::catalog()).map_err(|e| e.to_string())?;
        let extended = serde_json::to_value(cerulean_adapters::current_catalog())
            .map_err(|e| e.to_string())?;
        require(
            original.get("workers") == Some(&legacy) && context.get("workers") == Some(&extended),
            "original management catalog is not a known preserved version",
        )?;
        context["workers"] = legacy;
        // Assignment/supervision fields are added after the base reply budget.
        // Measure only the exact base fields, without copying old mutable state.
        let map = context
            .as_object_mut()
            .ok_or("management context invalid")?;
        map.remove("userResponses");
        map.remove("userResponsesOmitted");
        let project = context["workflow"]["project"]["id"]
            .as_str()
            .ok_or("management project missing")?
            .to_owned();
        let base = context
            .as_object()
            .unwrap()
            .iter()
            .filter(|(k, _)| {
                [
                    "requirement",
                    "workflow",
                    "validators",
                    "workers",
                    "executionSetup",
                ]
                .contains(&k.as_str())
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<serde_json::Map<_, _>>();
        let bytes = serde_json::to_vec(&base).map_err(|e| e.to_string())?.len();
        let replies = self
            .management
            .user_replies(&project, (64 * 1024usize).saturating_sub(bytes + 128));
        let count = self.management.reply_count(&project);
        if count > 0 {
            context["userResponsesOmitted"] = (count - replies.len()).into();
            context["userResponses"] = Value::Array(replies);
        }
        Ok(context)
    }
    pub(super) fn rebuild_management_context(
        &self,
        intent: &DecisionIntent,
        context: Value,
    ) -> Result<Value, String> {
        rebuild(
            intent,
            self.match_original_catalog(&intent.context, context)?,
        )
    }
    pub(super) fn management_dispatch_context(
        &self,
        project: &str,
        purpose: management::Purpose,
        requirement: &str,
    ) -> Result<Value, String> {
        let context = self.management_context(project, requirement)?;
        if purpose == management::Purpose::Assign
            && self
                .selected_management_provider(project, purpose)
                .is_some()
            && context["workflow"]["revision"].is_object()
        {
            add_bindings(context)
        } else if purpose == management::Purpose::Plan
            && self
                .selected_management_provider(project, purpose)
                .is_some()
            && context["executionSetup"].is_object()
        {
            add_planning_bindings(context)
        } else {
            Ok(context)
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn catalog_285_original_plan_response_is_readable_after_reopen_without_ledger_rewrite() {
        use cerulean_adapters::openai::DecisionObservation;
        use cerulean_domain::management::{DecisionEnvelope, ManagementDecision};
        let root = tempfile::tempdir().unwrap();
        let (mut host, draft) = super::super::project_planning::tests::fixture(root.path());
        host.configure_project(draft).unwrap();
        let mut context = host
            .management_context("project", "historical wait")
            .unwrap();
        context["workers"] = serde_json::to_value(cerulean_adapters::catalog()).unwrap();
        let state_digest = digest(&context).unwrap();
        let i = DecisionIntent {
            operation_id: "old-plan285".into(),
            project_id: "project".into(),
            purpose: management::Purpose::Plan,
            model: "fixture-chief".into(),
            state_digest: state_digest.clone(),
            context,
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        host.management.begin(i).unwrap();
        host.management
            .finish(
                "old-plan285",
                Ok(DecisionObservation {
                    response_id: "resp_old285".into(),
                    requested_model: "fixture-chief".into(),
                    actual_model: "fixture-chief".into(),
                    input_tokens: 1,
                    output_tokens: 1,
                    envelope: DecisionEnvelope {
                        state_digest,
                        decision: ManagementDecision::Wait {
                            reason: "pending user decision".into(),
                        },
                    },
                }),
            )
            .unwrap();
        let path = root
            .path()
            .join("data/management-decisions/old-plan285.1.json");
        let original = std::fs::read(&path).unwrap();
        assert!(host.management_decision("old-plan285").is_ok());
        drop(host);
        let host = RuntimeHost::open(&root.path().join("data")).unwrap();
        assert!(host.management_decision("old-plan285").is_ok());
        assert_eq!(std::fs::read(path).unwrap(), original);
    }
    #[test]
    fn catalog_285_rebuild_preserves_legacy_digest_but_rejects_arbitrary_registration_changes() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, draft) = super::super::project_planning::tests::fixture(root.path());
        host.configure_project(draft).unwrap();
        let current = host
            .management_context("project", "legacy plan awaiting decision")
            .unwrap();
        let mut historic = current.clone();
        historic["workers"] = serde_json::to_value(cerulean_adapters::catalog()).unwrap();
        let i = DecisionIntent {
            operation_id: "legacy285".into(),
            project_id: "project".into(),
            purpose: management::Purpose::Plan,
            model: "fixture-model".into(),
            state_digest: digest(&historic).unwrap(),
            context: historic.clone(),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        assert_eq!(rebuild(&i, current.clone()).unwrap(), historic);
        let mut changed = current.clone();
        changed["workers"][0]["note"] = json!("arbitrary changed registration");
        assert!(rebuild(&i, changed).is_err());
        let mut changed = current.clone();
        changed["requirement"] = json!("changed task");
        assert_ne!(
            digest(&rebuild(&i, changed).unwrap()).unwrap(),
            i.state_digest
        );
        let mut extended = i;
        extended.context = current.clone();
        extended.state_digest = digest(&current).unwrap();
        assert_eq!(rebuild(&extended, current.clone()).unwrap(), current);
        assert!(
            rebuild(&extended, historic).is_err(),
            "must not downgrade an existing extended request"
        );
    }
    #[test]
    fn assignment_bindings_preserve_legacy_context_and_reject_changed_pairs() {
        let root = tempfile::tempdir().unwrap();
        let (mut host, draft) = super::super::project_planning::tests::fixture(root.path());
        host.configure_project(draft).unwrap();
        super::super::project_planning::tests::complete(
            &mut host,
            super::super::project_planning::tests::proposal("tests"),
        );
        let original = host.management_context("project", "bounded task").unwrap();
        let mut intent = DecisionIntent {
            operation_id: "assignment-test".into(),
            project_id: "project".into(),
            purpose: management::Purpose::Assign,
            model: "deepseek-flash".into(),
            state_digest: digest(&original).unwrap(),
            context: original.clone(),
            created_at: chrono::Utc::now().to_rfc3339(),
        };
        assert_eq!(rebuild(&intent, original.clone()).unwrap(), original);
        let updated = add_bindings(original.clone()).unwrap();
        assert_eq!(
            updated["assignmentBindings"]["tasks"][0]["workerId"],
            "worker-codex"
        );
        assert_eq!(
            updated["assignmentBindings"]["tasks"][0]["adapterId"],
            "codex"
        );
        intent.context = updated.clone();
        intent.state_digest = digest(&updated).unwrap();
        assert_eq!(rebuild(&intent, original.clone()).unwrap(), updated);
        intent.context["assignmentBindings"]["tasks"][0]["workerId"] = json!("codex");
        intent.state_digest = digest(&intent.context).unwrap();
        assert!(rebuild(&intent, original.clone()).is_err());
        intent.context = updated;
        intent.purpose = management::Purpose::Accept;
        assert!(rebuild(&intent, original).is_err());
    }
}
