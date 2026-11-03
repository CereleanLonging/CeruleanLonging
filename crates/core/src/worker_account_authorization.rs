//! Local-user approval for two fixed management operations. This authority is
//! never sent to a worker and never changes the worker's token or tool policy.
use super::*;

impl RuntimeHost {
    pub(super) fn worker_account_review_digest(&self, project: &str) -> Option<String> {
        #[cfg(windows)]
        {
            let saved = self.project_setups.get(project)?;
            self.worker_account_scope(project, &saved.digest).ok()?;
            Some(saved.digest.clone())
        }
        #[cfg(not(windows))]
        {
            let _ = project;
            None
        }
    }

    #[cfg(windows)]
    fn worker_account_scope(
        &self,
        project: &str,
        setup: &str,
    ) -> Result<WorkerAccountAuthorization, String> {
        let saved = self.project_setups.get(project).ok_or("请先保存项目配置")?;
        require(
            saved.digest == setup && digest(&saved.setup)? == setup,
            "项目配置已变化，请重新审阅",
        )?;
        saved.setup.require_available_adapters()?;
        let chief = saved
            .setup
            .chief_directory
            .as_deref()
            .ok_or("项目尚未启用目录隔离")?;
        let store = self.projects.get(project).ok_or("项目尚未登记")?;
        let revision = store.revision().ok_or("方案尚未生成")?;
        require(
            revision.management_setup_digest.as_deref() == Some(setup),
            "方案未绑定当前隔离配置，请重新规划和审阅",
        )?;
        crate::worker_boundaries::validate(revision, true)?;
        require(
            store
                .configuration()
                .execution_scope
                .as_ref()
                .is_some_and(|s| s.project == revision.project),
            "方案与登记项目不符",
        )?;
        crate::worker_directories::verify_saved(
            &self.root,
            &revision.project,
            &saved.setup.worker_directories,
        )?;
        // Includes the saved chief requirements and all registered model slots.
        crate::role_workspace::read_requirements(saved, &revision.project.root)?;
        for task in &revision.execution {
            let directory = task.worker_directory.as_ref().ok_or("方案缺少隔离目录")?;
            require(
                saved.setup.worker_directories.get(&directory.identity) == Some(directory),
                "方案目录与当前配置不符",
            )?;
        }
        Ok(WorkerAccountAuthorization {
            setup_digest: setup.into(),
            scope_digest: digest(&(
                "ordinary-worker-accounts-and-project-acls-v1",
                &self.root,
                &revision.project,
                chief,
                &saved.setup.worker_directories,
                crate::windows_worker_account::provision_script_digest(),
            ))?,
        })
    }

    pub(super) fn approve_with_worker_accounts(
        &mut self,
        project: &str,
        revision: &str,
        setup: &str,
    ) -> Result<(), String> {
        #[cfg(windows)]
        {
            self.approve_worker_accounts_using(project, revision, setup, |host| {
                let prepared = host.prepare_authorized_worker_accounts(project, setup)?;
                crate::windows_worker_account::start(prepared)
            })
        }
        #[cfg(not(windows))]
        {
            let _ = (project, revision, setup);
            Err("执行账户隔离需要 Windows".into())
        }
    }

    #[cfg(windows)]
    fn approve_worker_accounts_using(
        &mut self,
        project: &str,
        revision: &str,
        setup: &str,
        launch: impl FnOnce(&mut Self) -> Result<(), String>,
    ) -> Result<(), String> {
        let authority = self.worker_account_scope(project, setup)?;
        let store = self.projects.get(project).ok_or("项目尚未登记")?;
        require(
            store.revision().ok_or("方案缺失")?.approval_digest()? == revision,
            "方案已变化，请重新审阅",
        )?;
        if let Some(prior) = store.approval() {
            require(
                prior.worker_account_setup.as_ref() == Some(&authority),
                "原批准不包含这次管理授权；请审阅新方案，不能追加原授权",
            )?;
            // Also covers a crash between durable approval and launch: only an
            // explicit setup-button action may start a new preparation attempt.
            return Ok(());
        }
        self.planning_idle(project)?;
        self.approve_with_timing_scoped(project, revision, Some(authority))?;
        launch(self).map_err(|e| {
            format!("方案及管理授权已保存，但账户准备未完成：{e}。请核对原记录；不会自动重弹 UAC")
        })
    }

    #[cfg(windows)]
    pub(super) fn prepare_authorized_worker_accounts(
        &self,
        project: &str,
        setup: &str,
    ) -> Result<crate::windows_worker_account::PreparedAccounts, String> {
        self.worker_account_idle(project)?;
        let authority = self.worker_account_scope(project, setup)?;
        let store = self.projects.get(project).ok_or("项目尚未登记")?;
        let approval = store.approval().ok_or("缺少本方案管理授权")?;
        require(
            approval.worker_account_setup.as_ref() == Some(&authority),
            "方案管理授权与当前目录或程序不符，请重新审阅",
        )?;
        let revision = store.revision().ok_or("方案缺失")?;
        approval.validate(revision, "user", revision.policy.version)?;
        let saved = &self.project_setups[project];
        crate::windows_worker_account::prepare(
            &self.root,
            &revision.project,
            saved
                .setup
                .chief_directory
                .as_deref()
                .ok_or("主指挥目录缺失")?,
            &saved.setup.worker_directories,
        )
    }
}

#[cfg(all(test, windows))]
#[path = "worker_account_authorization_tests.rs"]
mod tests;
