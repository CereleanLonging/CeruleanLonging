//! Immutable user configuration revisions, separate from execution approval.
use crate::host::ordinary;
use cerulean_domain::{execution::*, project_setup::*};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
};
pub(crate) fn validate(record: &SavedProjectSetup) -> Result<(), String> {
    let s = &record.setup;
    require(
        s.worker_adapter.is_api() == s.api_worker.is_some()
            && s.worker_adapter.is_desktop() == s.desktop_worker.is_some()
            && (s.worker_adapter.is_api() || s.worker_adapter.is_desktop()) == s.worker.is_none(),
        "project setup worker selection mismatch",
    )?;
    ProjectSetupDraft {
        management_sources: s.management_sources.clone(),
        resource_keys: s.resource_keys.clone(),
        profiles: s.profiles.iter().map(WorkerProfile::draft).collect(),
        worktree: s.worktree.as_ref().map(|w| WorktreeSetupDraft {
            integration: w.integration.as_deref().map(ProjectIntegration::draft),
            inherit_dependencies: w.input
                == GitWorktreeInput::CommittedRawBlobsWithAcceptedDependencies,
            base_commit: w.base_commit.clone(),
            pool_root: w.pool_root.clone(),
            git_executable: w.git.path.clone(),
        }),
        desktop_worker: s.desktop_worker.clone(),
        api_worker: s.api_worker.clone(),
        omp_runtime_parent: s.worker.as_ref().and_then(|w| w.omp_runtime_parent.clone()),
        worker_adapter: s.worker_adapter,
        supervision: s.policy.supervision.clone(),
        project_id: s.project_id.clone(),
        requirement: s.requirement.clone(),
        codex_executable: s
            .worker
            .as_ref()
            .map_or_else(String::new, |w| w.executable.path.clone()),
        worker_model: s.worker.as_ref().map_or_else(
            || {
                s.desktop_worker
                    .as_ref()
                    .map(|w| w.model_label.clone())
                    .unwrap_or_else(|| {
                        s.api_worker
                            .as_ref()
                            .map_or_else(String::new, |w| w.model.clone())
                    })
            },
            |w| w.model.clone(),
        ),
        chief_model: s.chief_model.clone(),
        deputy_model: s.deputy_model.clone(),
        validator_id: s.validator_id.clone(),
        check_args: s.check.args.clone(),
        acceptance: s.acceptance.as_ref().map(|a| AcceptanceDraft {
            schema_version: a.schema_version,
            stages: a
                .stages
                .iter()
                .map(|stage| AcceptanceStageDraft {
                    id: stage.check.id.clone(),
                    name: stage.check.name.clone().unwrap_or_default(),
                    validator_id: stage.validator_id.clone(),
                    args: stage.check.args.clone(),
                    timeout_seconds: stage.check.timeout_seconds,
                })
                .collect(),
        }),
        artifact_paths: s.artifact_paths.clone(),
        currency_budget: s.policy.currency_budget.clone(),
        worker_max_requests: s.policy.max_requests,
        management_max_requests: s.management_max_requests,
        max_seconds: s.policy.max_seconds,
    }
    .validate()?;
    require(
        s.version > 0
            && s.version <= 32
            && s.worker
                .as_ref()
                .is_none_or(|w| w.provider == s.worker_adapter.provider())
            && s.policy.version == 1
            && s.policy.tools
                == vec![
                    ToolPermission::ReadFiles,
                    ToolPermission::WriteFiles,
                    ToolPermission::RunCommands,
                ]
                .into_iter()
                .chain(s.has_desktop().then_some(ToolPermission::DesktopSend))
                .collect::<Vec<_>>()
            && s.policy.budget_usd_cents.is_none(),
        "unsupported project setup",
    )?;
    if let Some(worker) = &s.worker {
        worker.validate()?;
    }
    s.snapshot_policy.validate()?;
    for profile in &s.profiles {
        let mut selected = s.clone();
        selected.profiles.clear();
        selected.resource_keys = profile.resource_keys.clone();
        selected.worker_adapter = profile.worker_adapter;
        selected.policy.supervision = s.supervision_for(Some(&profile.id))?.cloned();
        selected.worker = profile.worker.clone();
        selected.api_worker = profile.api_worker.clone();
        selected.desktop_worker = profile.desktop_worker.clone();
        selected.artifact_paths = profile.artifact_paths.clone();
        let first = profile
            .acceptance
            .stages
            .first()
            .ok_or("执行配置验收缺失")?;
        selected.validator_id = first.validator_id.clone();
        selected.check = first.check.clone();
        selected.acceptance = Some(profile.acceptance.clone());
        selected.policy.tools = vec![
            ToolPermission::ReadFiles,
            ToolPermission::WriteFiles,
            ToolPermission::RunCommands,
        ]
        .into_iter()
        .chain(
            profile
                .worker_adapter
                .is_desktop()
                .then_some(ToolPermission::DesktopSend),
        )
        .collect();
        validate(&SavedProjectSetup {
            digest: digest(&selected)?,
            setup: selected,
        })?;
    }
    if let Some(worktree) = &s.worktree {
        require(
            worktree.schema_version == 1 && s.snapshot_policy.excludes(".git"),
            "invalid configured worktree",
        )?;
        ValidationTool {
            executable: worktree.git.clone(),
            inputs: vec![],
            environment: Default::default(),
        }
        .validate()?;
    }
    for check in s.checks() {
        let tool = check
            .tool
            .as_ref()
            .ok_or("configured validator binding missing")?;
        tool.validate()?;
        require(
            check.program == tool.executable.path,
            "configured validator program mismatch",
        )?;
    }
    if let Some(integration) = s.integration() {
        let check = &integration.check;
        let tool = check.tool.as_ref().ok_or("整体检查工具绑定缺失")?;
        tool.validate()?;
        require(
            check.id == "project-integration-check"
                && check.name.as_deref() == Some("项目整体检查")
                && check.program == tool.executable.path
                && integration
                    .artifact_paths
                    .iter()
                    .all(|p| !s.snapshot_policy.excludes(p)),
            "整体检查配置或最终交付范围无效",
        )?;
    }
    if let Some(acceptance) = &s.acceptance {
        require(
            acceptance.stages[0].check == s.check,
            "first acceptance stage differs from compatibility check",
        )?;
    }
    require(
        (s.acceptance.is_some() || (s.check.id == "project-check" && s.check.name.is_none()))
            && s.check.timeout_seconds > 0
            && s.check.timeout_seconds <= s.policy.max_seconds
            && s.artifact_paths
                .iter()
                .all(|p| !s.snapshot_policy.excludes(p)),
        "invalid configured acceptance scope",
    )?;
    require(digest(s)? == record.digest, "project setup digest mismatch")
}
pub(crate) fn load(root: &Path) -> Result<BTreeMap<String, SavedProjectSetup>, String> {
    Ok(history(root)?
        .into_iter()
        .map(|s| (s.setup.project_id.clone(), s))
        .collect())
}
pub(crate) fn history(root: &Path) -> Result<Vec<SavedProjectSetup>, String> {
    let dir = root.join("project-setups");
    if !dir.exists() {
        fs::create_dir(&dir).map_err(|e| e.to_string())?;
    }
    ordinary(&dir, true)?;
    let mut files = fs::read_dir(&dir)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    require(files.len() <= 256, "project setup history full")?;
    files.sort_by_key(|f| f.file_name());
    let mut result: BTreeMap<String, SavedProjectSetup> = BTreeMap::new();
    let mut history = Vec::new();
    for f in files {
        ordinary(&f.path(), false)?;
        require(
            f.path().extension().is_some_and(|s| s == "json"),
            "incomplete project setup publication; recovery required",
        )?;
        let mut bytes = Vec::new();
        fs::File::open(f.path())
            .map_err(|e| e.to_string())?
            .take(65537)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        require(bytes.len() <= 65536, "project setup too large")?;
        let saved: SavedProjectSetup =
            serde_json::from_slice(&bytes).map_err(|_| "invalid project setup record")?;
        crate::management_source_guard::require_protected(
            root,
            saved.setup.management_sources.is_some(),
        )?;
        validate(&saved)?;
        require(
            f.file_name().to_string_lossy()
                == format!("{}.{:08}.json", saved.setup.project_id, saved.setup.version),
            "project setup filename mismatch",
        )?;
        let previous = result
            .get(&saved.setup.project_id)
            .map_or(0, |p| p.setup.version);
        require(
            saved.setup.version == previous + 1,
            "project setup revision gap",
        )?;
        history.push(saved.clone());
        result.insert(saved.setup.project_id.clone(), saved);
    }
    Ok(history)
}
pub(crate) fn save(root: &Path, saved: &SavedProjectSetup) -> Result<(), String> {
    crate::management_source_guard::require_protected(
        root,
        saved.setup.management_sources.is_some(),
    )?;
    crate::currency_scoped_guard::require_protected(
        root,
        saved
            .setup
            .policy
            .currency_budget
            .as_ref()
            .is_some_and(|s| s.scoped_quotes.is_some()),
    )?;
    crate::currency_count_guard::require_settings(
        root,
        saved.setup.policy.currency_budget.as_ref(),
    )?;
    crate::currency_guard::require_protected(root, saved.setup.policy.currency_budget.is_some())?;
    crate::reader_generation::require_override(
        root,
        saved
            .setup
            .profiles
            .iter()
            .any(|p| p.supervision_override.is_some()),
    )?;
    validate(saved)?;
    let dir = root.join("project-setups");
    ordinary(&dir, true)?;
    require(
        fs::read_dir(&dir).map_err(|e| e.to_string())?.count() < 256,
        "project setup history full",
    )?;
    let file = dir.join(format!(
        "{}.{:08}.json",
        saved.setup.project_id, saved.setup.version
    ));
    let temp = file.with_extension("pending");
    require(
        !file.exists() && !temp.exists(),
        "project setup revision exists; reopen before retry",
    )?;
    let bytes = serde_json::to_vec(saved).map_err(|e| e.to_string())?;
    require(bytes.len() <= 65536, "project setup too large")?;
    let mut f = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    f.write_all(&bytes)
        .and_then(|_| f.sync_all())
        .map_err(|e| e.to_string())?;
    drop(f);
    fs::rename(temp, file).map_err(|e| e.to_string())
}
