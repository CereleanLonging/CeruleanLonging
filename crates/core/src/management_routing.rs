//! Route by saved role selection, never by a model name or another provider key.
use super::*;
use cerulean_domain::{
    management::Purpose,
    project_setup::{ChiefSource, DeputySource},
};

impl RuntimeHost {
    pub(super) fn selected_management_source(
        &self,
        project: &str,
        purpose: Purpose,
        model: &str,
    ) -> Option<cerulean_domain::management_ingress::ManagementSource> {
        use cerulean_domain::management_ingress::{ManagementRole, ManagementSource};
        let sources = self
            .project_setups
            .get(project)?
            .setup
            .management_sources
            .as_ref()?;
        let role = ManagementRole::for_purpose(purpose);
        let endpoint = match role {
            ManagementRole::Chief => match &sources.chief {
                ChiefSource::Provider { endpoint } => endpoint.clone(),
                _ => return None,
            },
            ManagementRole::Deputy => match &sources.deputy {
                DeputySource::DeepSeek {} => {
                    return Some(ManagementSource::DeepSeek {
                        model: model.into(),
                    })
                }
                DeputySource::Provider { endpoint } => endpoint.clone(),
                _ => return None,
            },
        };
        Some(ManagementSource::Provider {
            endpoint,
            model: model.into(),
            role,
        })
    }
    pub(super) fn selected_management_provider(
        &self,
        project: &str,
        purpose: Purpose,
    ) -> Option<credentials::CredentialProvider> {
        self.selected_management_source(project, purpose, "selection")
            .and_then(|s| {
                s.api_selection()
                    .ok()
                    .map(|(endpoint, _)| endpoint.provider())
            })
    }
    pub(super) fn external_chief_selected(&self, project: &str, purpose: Purpose) -> bool {
        matches!(purpose, Purpose::Plan | Purpose::Accept)
            && self
                .project_setups
                .get(project)
                .and_then(|s| s.setup.management_sources.as_ref())
                .is_some_and(|s| matches!(s.chief, ChiefSource::CodexSession { .. }))
    }
    pub(super) fn management_probe_in_progress(&self, project: &str, purpose: Purpose) -> bool {
        if self.external_chief_selected(project, purpose) {
            return false;
        }
        if let Some(provider) = self.selected_management_provider(project, purpose) {
            self.provider_probes.contains_key(&provider)
        } else {
            self.openai_probe.is_some()
        }
    }
    pub(super) fn require_management_connection(
        &self,
        project: &str,
        purpose: Purpose,
    ) -> Result<(), String> {
        require(
            !self.management_probe_in_progress(project, purpose),
            "selected management provider connection check in progress",
        )?;
        if self.external_chief_selected(project, purpose) {
            return Ok(());
        }
        if let Some(provider) = self.selected_management_provider(project, purpose) {
            require(
                self.provider_connections
                    .get(&provider)
                    .is_some_and(|s| s.key_configured),
                "所选指挥需要该供应商独立配置的本机凭证",
            )
        } else {
            require(
                self.openai_connection.key_configured,
                "OpenAI API Key required",
            )
        }
    }
    #[cfg(windows)]
    pub(super) fn load_management_credentials(&self, project: &str) -> Result<(), String> {
        let sources = self
            .project_setups
            .get(project)
            .and_then(|s| s.setup.management_sources.as_ref());
        let openai = sources.is_none_or(|s| {
            matches!(s.chief, ChiefSource::OpenAi {}) || matches!(s.deputy, DeputySource::OpenAi {})
        });
        if openai {
            crate::credential_vault::load(&self.root)
                .map_err(|_| "请配置所选指挥的本机 OpenAI 凭证")?;
        }
        for purpose in [Purpose::Plan, Purpose::Assign] {
            if let Some(provider) = self.selected_management_provider(project, purpose) {
                crate::credential_vault::load_for(&self.root, provider)
                    .map_err(|_| "请配置所选指挥供应商的本机凭证")?;
            }
        }
        Ok(())
    }
    #[cfg(windows)]
    pub(super) fn launch_routed_management(
        &mut self,
        intent: crate::management_execution::DecisionIntent,
    ) -> Result<(), String> {
        self.require_review_dispatch_idle()?;
        if self
            .selected_management_provider(&intent.project_id, intent.purpose)
            .is_some()
        {
            self.start_provider_management_intent(intent)
        } else {
            self.launch_authenticated_management(intent)
        }
    }
}
