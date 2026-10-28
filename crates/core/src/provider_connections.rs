use super::*;
use cerulean_domain::credentials::{CredentialProvider, ProviderConnection, ProviderEndpoint};
pub(super) fn load_statuses(
    root: &Path,
) -> Result<BTreeMap<CredentialProvider, ProviderConnection>, String> {
    let mut statuses = BTreeMap::new();
    for provider in CredentialProvider::WORKERS {
        #[cfg(windows)]
        let configured = crate::credential_vault::exists_for(root, provider)?;
        #[cfg(not(windows))]
        let configured = {
            let _ = root;
            false
        };
        statuses.insert(
            provider,
            ProviderConnection::unchecked(provider, configured),
        );
    }
    Ok(statuses)
}
impl RuntimeHost {
    fn require_provider_management_idle(&self, provider: CredentialProvider) -> Result<(), String> {
        use crate::management_execution::StoredDecision;
        for operation in self.management_active.keys() {
            let busy = match self.management.stored_decision(operation)? {
                StoredDecision::DeepSeek { request, .. }
                | StoredDecision::Provider { request, .. } => {
                    request.wire.target.provider() == provider
                }
                _ => false,
            };
            require(
                !busy,
                "该供应商指挥请求进行中，请等待结束再变更凭证或检查连接",
            )?;
        }
        Ok(())
    }
    pub(super) fn require_provider_idle(&self, provider: CredentialProvider) -> Result<(), String> {
        self.require_provider_management_idle(provider)?;
        #[cfg(windows)]
        require(
            !self
                .api_active
                .values()
                .any(|a| a.job.worker.target.provider() == provider),
            "API任务仍在运行，不能变更对应供应商凭证",
        )?;
        require(
            !self.provider_probes.contains_key(&provider),
            "该供应商检查正在进行，请等待结束后变更凭证",
        )?;
        #[cfg(windows)]
        if provider == CredentialProvider::DeepSeek {
            require(
                self.omp_active.is_empty(),
                "OMP 任务仍在运行，不能变更 DeepSeek 凭证",
            )?;
        }
        Ok(())
    }
    pub(super) fn check_provider_models(&mut self, target: ProviderEndpoint) -> Result<(), String> {
        let endpoint = cerulean_adapters::provider_models::endpoint(&target)?;
        let provider = target.provider();
        self.require_provider_management_idle(provider)?;
        require(
            !self.provider_probes.contains_key(&provider),
            "该供应商检查正在进行",
        )?;
        let status = self
            .provider_connections
            .get(&provider)
            .ok_or("供应商未登记")?;
        require(status.key_configured, "请先在本机配置对应供应商 Key")?;
        #[cfg(windows)]
        {
            let root = self.root.clone();
            let checked_target = target.clone();
            let (sender, receiver) = std::sync::mpsc::channel();
            std::thread::Builder::new()
                .name(format!("{}-models-check", provider.id()))
                .spawn(move || {
                    let result = (|| {
                        let key = crate::credential_vault::load_for(&root, provider)?;
                        let rt = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .map_err(|_| "供应商检查运行环境不可用")?;
                        rt.block_on(cerulean_adapters::provider_models::models(
                            &checked_target,
                            key,
                        ))
                    })();
                    let _ = sender.send(result);
                })
                .map_err(|_| "无法启动供应商检查")?;
            self.provider_probes.insert(provider, receiver);
            let status = self.provider_connections.get_mut(&provider).unwrap();
            *status = ProviderConnection::unchecked(provider, true);
            status.state = "checking".into();
            status.target = Some(target);
            status.endpoint = Some(endpoint);
            Ok(())
        }
        #[cfg(not(windows))]
        {
            let _ = (endpoint, target);
            Err("本机凭证存储当前仅支持Windows".into())
        }
    }
    pub(super) fn poll_provider_models(&mut self) {
        let mut done = vec![];
        for (&provider, receiver) in &self.provider_probes {
            let outcome = match receiver.try_recv() {
                Ok(r) => Some(r),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Some(Err("供应商检查中断；未重试".into()))
                }
            };
            if let Some(outcome) = outcome {
                done.push((provider, outcome));
            }
        }
        for (provider, outcome) in done {
            self.provider_probes.remove(&provider);
            let status = self
                .provider_connections
                .get_mut(&provider)
                .expect("registered probe");
            status.checked_at = Some(chrono::Utc::now().to_rfc3339());
            status.models.clear();
            status.list_limited = false;
            match outcome {
                Ok(list) => {
                    status.state = "listed".into();
                    status.models = list.models;
                    status.list_limited = list.limited;
                    status.error = None;
                }
                Err(error) => {
                    status.state = "failed".into();
                    status.error = Some(error);
                }
            }
        }
    }
}

#[cfg(all(test, windows))]
#[path = "provider_connections/tests.rs"]
mod tests;
