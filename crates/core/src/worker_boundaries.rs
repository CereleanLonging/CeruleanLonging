//! Live dispatch policy. Historical records keep their original validation rules.
//! A review packet never grants filesystem access to another execution directory.
use cerulean_domain::{execution::*, management::Purpose, project_setup::SavedProjectSetup};
use serde_json::{json, Value};

pub(crate) fn validate(plan: &PlanRevision, require_bound: bool) -> Result<(), String> {
    validate_tasks(&plan.execution, require_bound)?;
    if plan.execution.iter().any(|t| t.worker_directory.is_some()) {
        require(
            plan.worktree.is_none(),
            "固定执行目录不能被 Git 全库副本替代",
        )?;
        for task in &plan.execution {
            task.worker_directory
                .as_ref()
                .ok_or("执行目录未绑定")?
                .root(&plan.project.root)?;
        }
    }
    Ok(())
}

fn validate_tasks(tasks: &[TaskExecution], require_bound: bool) -> Result<(), String> {
    let isolated = tasks.iter().any(|t| t.worker_directory.is_some());
    if !require_bound && !isolated {
        return Ok(());
    }
    require(
        !tasks.is_empty() && tasks.iter().all(|t| t.worker_directory.is_some()),
        "正常执行必须为每个模型绑定独立工作目录；请配置角色隔离并重新规划、批准，旧方案不能派发",
    )?;
    let mut owned: Vec<(String, &str, &str)> = Vec::new();
    for task in tasks {
        let directory = task.worker_directory.as_ref().ok_or("执行目录未绑定")?;
        directory.validate(task)?;
        require(
            !task.artifact_paths.is_empty(),
            "任务必须明确声明负责的源码或交付文件",
        )?;
        for path in &task.artifact_paths {
            artifact_path(path)?;
            // Conservative Windows file ownership key: uppercase also folds
            // forms such as Greek final sigma that lowercase alone misses.
            let key = path.replace('\\', "/").to_uppercase();
            for (previous, identity, previous_task) in &owned {
                let overlaps = key == *previous
                    || key.starts_with(&format!("{previous}/"))
                    || previous.starts_with(&format!("{key}/"));
                require(!overlaps || *identity == directory.identity,
                    &format!("源码分工冲突：任务 {previous_task} 与 {} 将 {path} 分配给不同执行端；请由主指挥修订为单一归属", task.task_id))?;
            }
            owned.push((key, &directory.identity, &task.task_id));
        }
    }
    Ok(())
}

fn rows(tasks: &[TaskExecution], project_root: &str) -> Result<Vec<Value>, String> {
    tasks.iter().map(|task| {
        let root = task.worker_directory.as_ref().map(|d| d.root(project_root)).transpose()?;
        Ok(json!({"taskId":task.task_id,"profileId":task.profile_id,"adapterId":task.adapter_id,
            "modelIdentity":task.model_identity()?,"workingDirectory":root,
            "ownedSourcePaths":task.artifact_paths,"normalAccess":"assigned-directory-only",
            "reviewAccess":"approved-read-only-fragments-only","directoryBound":root.is_some()}))
    }).collect()
}

/// Added only to new requests, and rebuilt only when the original carried it.
/// This packet informs both managers; the live guard remains the authority.
pub(crate) fn annotate(mut context: Value, purpose: Purpose) -> Result<Value, String> {
    if !matches!(purpose, Purpose::Plan | Purpose::Assign) {
        return Ok(context);
    }
    let snapshot: ExecutionSnapshot = serde_json::from_value(context["workflow"].clone())
        .map_err(|_| "执行边界缺少工作流快照")?;
    let tasks = if purpose == Purpose::Plan && context["executionSetup"].is_object() {
        let saved: SavedProjectSetup = serde_json::from_value(context["executionSetup"].clone())
            .map_err(|_| "执行边界配置无效")?;
        let mut tasks = vec![saved.setup.execution_for("default", None)?];
        for p in &saved.setup.profiles {
            tasks.push(saved.setup.execution_for(&p.id, Some(&p.id))?);
        }
        // Profiles may be alternatives; ownership conflicts are checked against
        // the actual selected plan, not every registered unused alternative.
        tasks
    } else if let Some(plan) = snapshot.revision {
        validate(&plan, false)?;
        plan.execution
    } else {
        return Ok(context);
    };
    context["executionBoundaries"] = json!({"schemaVersion":1,"tasks":rows(&tasks,&snapshot.project.root)?,
        "rule":"Normal execution can read and write only its bound model directory. Never assign another model's source path, a parent/shared directory, or a repository-wide scan. Different executor identities must have disjoint source-file ownership (case-insensitive, including parent/child path overlap). Same-model tasks remain serialized by the host. A missing directory blocks dispatch. Review uses only approved read-only source fragments delivered by the independent verifier through the core; review never expands directory ACLs or exposes file tools. If boundaries conflict, ask the chief to revise the plan and obtain user approval before dispatch."});
    require(
        serde_json::to_vec(&context)
            .map_err(|e| e.to_string())?
            .len()
            <= 64 * 1024,
        "指挥请求的任务边界超过容量，请拆分方案；不会截断边界后分配",
    )?;
    Ok(context)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn task(id: &str, model: &str, path: &str) -> TaskExecution {
        let mut task = TaskExecution {
            task_id: id.into(),
            adapter_id: "codex".into(),
            profile_id: None,
            worker: Some(WorkerSelection {
                provider: "openai".into(),
                model: model.into(),
                executable: ValidationFile {
                    path: "C:\\fixture\\codex.exe".into(),
                    sha256: "a".repeat(64),
                },
                omp_runtime_parent: None,
            }),
            api_worker: None,
            desktop_worker: None,
            artifact_paths: vec![path.into()],
            checks: vec![],
            supervision_override: None,
            worker_directory: None,
        };
        task.worker_directory = Some(WorkerDirectory::for_task(&task).unwrap());
        task
    }
    #[test]
    fn worker_boundaries_reject_cross_model_source_overlap_and_aliases() {
        let first = task("one", "model-a", "src/main.rs");
        for path in ["src/main.rs", "SRC/Main.rs", "src", "src/main.rs/child"] {
            let error =
                validate_tasks(&[first.clone(), task("two", "model-b", path)], true).unwrap_err();
            assert!(error.contains("源码分工冲突"), "{error}");
        }
        assert!(validate_tasks(
            &[first.clone(), task("two", "model-b", "src/main.rs2")],
            true
        )
        .is_ok());
        assert!(validate_tasks(&[first, task("two", "model-a", "src/main.rs")], true).is_ok());
        assert!(validate_tasks(
            &[
                task("one", "model-a", "src/σ.rs"),
                task("two", "model-b", "src/ς.rs")
            ],
            true
        )
        .is_err());
    }
    #[test]
    fn worker_boundaries_reject_missing_mixed_or_forged_bindings_without_rewriting_legacy() {
        let bound = task("one", "model-a", "src/a.rs");
        let mut old = task("two", "model-b", "src/b.rs");
        old.worker_directory = None;
        let before = serde_json::to_vec(&old).unwrap();
        assert!(validate_tasks(&[old.clone()], false).is_ok());
        assert!(validate_tasks(&[old.clone()], true).is_err());
        assert_eq!(serde_json::to_vec(&old).unwrap(), before);
        assert!(validate_tasks(&[bound.clone(), old], false).is_err());
        let mut forged = bound;
        forged.worker_directory.as_mut().unwrap().relative_path = "executors/shared".into();
        assert!(validate_tasks(&[forged], true).is_err());
    }
}
