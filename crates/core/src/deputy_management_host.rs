//! Trusted RuntimeHost entry; request count and provider credentials stay local.
//! Source selection is explicit in the product runner, never inferred
//! from a model name. No direct user IPC starts arbitrary management requests.
use super::*;
use crate::management_execution::DecisionIntent;
use cerulean_adapters::{
    api_generation::{ApiClient, ApiFailure},
    deputy_management::{self, DeputyOutcome, DeputyRequest, DeputyResult, PreparedDeputy},
};
#[cfg(test)]
use cerulean_domain::credentials::CredentialProvider;
use cerulean_domain::{
    management::Purpose,
    management_ingress::{ManagementHandoff, ManagementSource},
};

impl RuntimeHost {
    #[cfg(windows)]
    pub fn start_deepseek_management(
        &mut self,
        project: &str,
        operation: &str,
        purpose: Purpose,
        model: &str,
        requirement: &str,
    ) -> Result<(), String> {
        require(
            matches!(purpose, Purpose::Assign | Purpose::Advise),
            "DeepSeek deputy may only assign or advise",
        )?;
        require(
            self.selected_management_provider(project, purpose)
                .is_none_or(|p| p == cerulean_domain::credentials::CredentialProvider::DeepSeek),
            "DeepSeek entry cannot dispatch a different configured provider",
        )?;
        self.poll_verifications();
        let context = self.management_dispatch_context(project, purpose, requirement)?;
        self.start_provider_management_intent(DecisionIntent {
            operation_id: operation.into(),
            project_id: project.into(),
            purpose,
            model: model.into(),
            state_digest: digest(&context)?,
            context,
            created_at: chrono::Utc::now().to_rfc3339(),
        })
    }
    #[cfg(windows)]
    pub(super) fn start_provider_management_intent(
        &mut self,
        intent: DecisionIntent,
    ) -> Result<(), String> {
        intent.validate()?;
        let project = intent.project_id.as_str();
        let purpose = intent.purpose;
        let model = intent.model.as_str();
        let source = self
            .selected_management_source(project, purpose, model)
            .unwrap_or_else(|| ManagementSource::DeepSeek {
                model: model.into(),
            });
        let (endpoint, _) = source.api_selection()?;
        let provider = endpoint.provider();
        self.require_currency_execution_available(project)?;
        self.require_management_dispatch_available(project, purpose)?;
        self.ensure_run_dispatch_authorized(&intent.operation_id)?;
        require(
            self.management_work_count() < 4,
            "management concurrency limit reached",
        )?;
        if let Some(setup) = self.project_setups.get(project) {
            if setup.setup.management_sources.is_some() {
                let expected_model = match purpose {
                    Purpose::Plan | Purpose::Accept => &setup.setup.chief_model,
                    _ => &setup.setup.deputy_model,
                };
                require(
                    self.selected_management_source(project, purpose, model)
                        .is_some()
                        && model == expected_model,
                    "管理请求与已保存来源或模型不符",
                )?;
            }
            require(
                self.management_operation_ids(project).len()
                    < setup.setup.management_max_requests as usize,
                "management request limit reached",
            )?;
        }
        crate::management_source_guard::require_protected(&self.root, true)?;
        crate::relay_guard::require_protected(
            &self.root,
            matches!(source, ManagementSource::Provider { .. }),
        )?;
        require(
            !self.provider_probes.contains_key(&provider),
            "selected management provider connection check in progress",
        )?;
        if intent.context.get("supervision").is_some() {
            require(
                purpose == Purpose::Advise,
                "supervision context needs advice purpose",
            )?;
            self.revalidate_counted_supervision(&intent)?;
        } else {
            require(
                !self.has_api_activity(project)
                    || matches!(purpose, Purpose::Assign | Purpose::Accept),
                "active advice requires original scoped supervisor evidence",
            )?;
            let requirement = intent.context["requirement"]
                .as_str()
                .ok_or("management requirement missing")?;
            require(
                digest(&self.rebuild_management_context(
                    &intent,
                    self.management_context(project, requirement)?,
                )?)? == intent.state_digest,
                "management context changed before dispatch",
            )?;
        }
        let handoff = ManagementHandoff {
            operation_id: intent.operation_id.clone(),
            project_id: intent.project_id.clone(),
            purpose,
            source,
            state_digest: intent.state_digest.clone(),
            context: intent.context.clone(),
        };
        let client = ApiClient::new(crate::credential_vault::load_for(&self.root, provider)?)?;
        let prepared = deputy_management::prepare(&client, handoff)?;
        #[cfg(test)]
        if let Some(send) = self.deputy_management_fixture {
            return self.launch_deepseek_management(intent, prepared, move |p, _| send(&p));
        }

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "deputy runtime unavailable")?;
        self.launch_deepseek_management(intent, prepared, move |prepared, gateway| {
            let (_keep, mut cancel) = tokio::sync::oneshot::channel();
            runtime.block_on(deputy_management::send_evidenced_with_gateway(
                &client,
                prepared,
                &mut cancel,
                gateway,
            ))
        })
    }
    pub fn read_deepseek_management(
        &self,
        operation: &str,
    ) -> Result<(DeputyRequest, Option<DeputyResult>, String), String> {
        self.management.deputy_evidence(operation)
    }
    #[cfg(windows)]
    pub(super) fn launch_deepseek_management(
        &mut self,
        intent: DecisionIntent,
        prepared: PreparedDeputy,
        send_once: impl FnOnce(
                PreparedDeputy,
                &mut cerulean_adapters::api_generation::GatewayObserver<'_>,
            ) -> DeputyResult
            + Send
            + 'static,
    ) -> Result<(), String> {
        let id = intent.operation_id.clone();
        let request = prepared.request();
        self.management.begin_deputy(intent, &prepared)?;
        let mut gateway = self.management.gateway_capture(&id);
        let (send, receive) = std::sync::mpsc::channel();
        let spawn = std::thread::Builder::new()
            .name("provider-management".into())
            .spawn(move || {
                let reply = send_once(prepared, &mut |headers| {
                    if let Some(capture) = &mut gateway {
                        capture.headers(headers);
                    }
                });
                let _ = send.send(Ok(management_scoped_host::ManagementCompletion::Deputy {
                    response: Box::new(reply),
                    gateway,
                }));
            });
        match spawn {
            Ok(_) => {
                self.management_active.insert(id, receive);
                Ok(())
            }
            Err(_) => {
                self.management.finish_deputy(
                    &id,
                    DeputyResult {
                        request,
                        decode_stage: None,
                        outcome: DeputyOutcome::Failed {
                            failure: ApiFailure::NotSent {},
                        },
                    },
                )?;
                Err("deputy thread did not start; original intent retained, no retry".into())
            }
        }
    }
}

#[cfg(all(test, windows))]
#[path = "deputy_management_host/tests.rs"]
mod tests;
