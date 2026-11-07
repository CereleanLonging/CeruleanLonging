//! A model-free Windows identity for the existing objective checker. Its private
//! workspace lives beside the data directory and never consumes an executor slot.
use crate::{artifacts::root_path, windows_worker_account as accounts};
use cerulean_domain::execution::{digest, identifier, require, Project, WorkerDirectory};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

struct Scope {
    project: Project,
    chief: PathBuf,
    binding: WorkerDirectory,
    parent: PathBuf,
}

fn scope(data: &Path, project: &str) -> Result<Scope, String> {
    identifier(project)?;
    let data = root_path(data.to_str().ok_or("检查账户数据目录编码无效")?)?;
    let identity = digest(&("objective-checker-account-v1", &data, project))?;
    let parent = data
        .parent()
        .ok_or("检查账户父目录缺失")?
        .join("objective-checker-workspaces")
        .join(&identity);
    let workspace = parent.join("workspace");
    Ok(Scope {
        project: Project {
            id: format!("delivery-check-{}", &identity[..32]),
            root: workspace.to_string_lossy().into_owned(),
            timezone: "UTC".into(),
        },
        chief: parent.join("control"),
        binding: WorkerDirectory {
            relative_path: format!("executors/{identity}"),
            identity,
        },
        parent,
    })
}

pub(crate) fn load(data: &Path, project: &str) -> Result<accounts::Account, String> {
    let scope = scope(data, project)?;
    accounts::load_binding(data, &scope.project, &scope.binding)
}

pub(crate) fn provision(data: &Path, project: &str) -> Result<(), String> {
    let mut scope = scope(data, project)?;
    if load(data, project).is_ok() {
        return Ok(());
    }
    require(
        !accounts::views(data)
            .iter()
            .any(|view| view.project_id == scope.project.id && view.state == "pending"),
        "独立检查账户设置尚在进行，请先核对原操作",
    )?;
    let owner = crate::ipc::current_sid().map_err(|e| e.to_string())?;
    let container = scope.parent.parent().ok_or("检查账户容器缺失")?;
    for directory in [
        container,
        &scope.parent,
        Path::new(&scope.project.root),
        &scope.chief,
    ] {
        accounts::private_directory(directory, &owner)?;
    }
    scope.project.root = root_path(&scope.project.root)?
        .to_string_lossy()
        .into_owned();
    scope.chief = root_path(scope.chief.to_str().ok_or("检查控制目录编码无效")?)?;
    let executors = Path::new(&scope.project.root).join("executors");
    accounts::private_directory(&executors, &owner)?;
    accounts::private_directory(Path::new(&scope.binding.root(&scope.project.root)?), &owner)?;
    let prepared = accounts::prepare(
        data,
        &scope.project,
        scope.chief.to_str().ok_or("检查控制目录编码无效")?,
        &BTreeMap::from([(scope.binding.identity.clone(), scope.binding)]),
    )?;
    accounts::start(prepared)
}
