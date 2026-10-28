//! One exclusively owned Host imports in a background thread. IPC stays readable.
use crate::{
    api_execution::{load, publish},
    host::RuntimeHost,
};
use cerulean_domain::{
    execution::{digest, require, sha256},
    team_timing::TeamImportView,
    Snapshot,
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    thread::JoinHandle,
};

const OPERATION: &str = "team-import-operation.json";
const OUTCOME: &str = "team-import-outcome.json";
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Operation {
    schema: u32,
    root: String,
    request_id: String,
    source_path: String,
    source_sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Outcome {
    operation_digest: String,
    completed: bool,
    binding: Option<Binding>,
    message: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Binding {
    project_id: String,
    digest: String,
}
fn original_binding(root: &Path, binding: &Binding) -> Result<(), String> {
    cerulean_domain::execution::identifier(&binding.project_id)?;
    let b: crate::team_materials::TeamBlueprint = load(
        &root
            .join("team-material-bindings")
            .join(&binding.project_id),
        "binding.json",
    )?;
    require(
        b.plan.project.id == binding.project_id
            && std::fs::canonicalize(&b.request.material.data_root).map_err(|e| e.to_string())?
                == root
            && digest(&b)? == binding.digest,
        "原导入结果与材料绑定不一致",
    )
}
fn completed_binding(host: &RuntimeHost) -> Result<Binding, String> {
    let snapshot = host.snapshot();
    require(
        snapshot.team_timelines.len() == 1 && snapshot.team_timelines[0].phase == "prepared",
        "导入没有产生唯一的原材料绑定",
    )?;
    let project_id = snapshot.team_timelines[0].project_id.clone();
    let b: crate::team_materials::TeamBlueprint = load(
        &host
            .directory()
            .join("team-material-bindings")
            .join(&project_id),
        "binding.json",
    )?;
    let binding = Binding {
        project_id,
        digest: digest(&b)?,
    };
    original_binding(host.directory(), &binding)?;
    Ok(binding)
}
pub(crate) struct Importing {
    pub root: PathBuf,
    pub snapshot: Snapshot,
    pub overview: cerulean_domain::workspace::WorkspaceResult,
    pub thread: Option<JoinHandle<Box<RuntimeHost>>>,
    // Retains the writer if thread creation fails; never drops a live Host as
    // a side effect of failing to start the observer or importer.
    slot: Arc<Mutex<Option<Box<RuntimeHost>>>>,
    operation: Operation,
}
fn present(root: &Path, name: &str) -> bool {
    !matches!(std::fs::symlink_metadata(root.join(name)),Err(e) if e.kind()==std::io::ErrorKind::NotFound)
}
fn read_operation(root: &Path) -> Result<Option<Operation>, String> {
    require(
        !present(root, &format!("{OPERATION}.pending")),
        "原导入意图存在未完成写入",
    )?;
    if !present(root, OPERATION) {
        require(
            !present(root, OUTCOME) && !present(root, &format!("{OUTCOME}.pending")),
            "导入结果缺少原操作意图",
        )?;
        return Ok(None);
    }
    let op: Operation = load(root, OPERATION)?;
    require(
        op.schema == 1
            && Path::new(&op.root) == std::fs::canonicalize(root).map_err(|e| e.to_string())?
            && !op.request_id.is_empty()
            && op.request_id.len() <= 256
            && Path::new(&op.source_path).is_absolute(),
        "导入操作身份不一致",
    )?;
    sha256(&op.source_sha256)?;
    Ok(Some(op))
}
pub(crate) fn view(root: &Path) -> Option<TeamImportView> {
    let op = match read_operation(root) {
        Ok(None) => return legacy_unfinished(root),
        Ok(Some(v)) => v,
        Err(e) => {
            return Some(TeamImportView {
                request_id: String::new(),
                source_path: String::new(),
                source_sha256: String::new(),
                state: "interrupted".into(),
                message: Some(e),
            })
        }
    };
    let mut v = TeamImportView {
        request_id: op.request_id.clone(),
        source_path: op.source_path.clone(),
        source_sha256: op.source_sha256.clone(),
        state: "interrupted".into(),
        message: Some("原导入没有完整结果；保留现有材料，不自动重复导入".into()),
    };
    if present(root, &format!("{OUTCOME}.pending")) {
        v.message = Some("原导入结果存在未完成写入；保留原记录核查".into());
        return Some(v);
    }
    if present(root, OUTCOME) {
        match load::<Outcome>(root, OUTCOME) {
            Ok(result)
                if digest(&op).is_ok_and(|d| d == result.operation_digest)
                    && (!result.completed
                        || result
                            .binding
                            .as_ref()
                            .is_some_and(|b| original_binding(root, b).is_ok())) =>
            {
                v.state = if result.completed {
                    "completed"
                } else {
                    "failed"
                }
                .into();
                v.message = result.message;
            }
            _ => v.message = Some("导入结果不完整或与原操作不一致；保留原记录核查".into()),
        }
    }
    Some(v)
}
fn legacy_unfinished(root: &Path) -> Option<TeamImportView> {
    const LEGACY: &str = "team-material-import.json";
    if !present(root, LEGACY) && !present(root, &format!("{LEGACY}.pending")) {
        return None;
    }
    let intent = load::<serde_json::Value>(root, LEGACY).ok();
    let bindings = root.join("team-material-bindings");
    let complete = (|| -> Result<bool, String> {
        crate::host::ordinary(&bindings, true)?;
        let entries = std::fs::read_dir(&bindings)
            .map_err(|e| e.to_string())?
            .take(2)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        require(entries.len() == 1, "材料绑定不是唯一完整导入")?;
        let b: crate::team_materials::TeamBlueprint = load(&entries[0].path(), "binding.json")?;
        Ok(intent
            .as_ref()
            .is_some_and(|v| v["bindingDigest"] == digest(&b).unwrap_or_default())
            && std::fs::canonicalize(&b.request.material.data_root).map_err(|e| e.to_string())?
                == root)
    })()
    .unwrap_or(false);
    if complete {
        return None;
    }
    Some(TeamImportView {
        request_id: "legacy-import".into(),
        source_path: intent
            .as_ref()
            .and_then(|v| v["requestPath"].as_str())
            .unwrap_or_default()
            .into(),
        source_sha256: intent
            .as_ref()
            .and_then(|v| v["sha256"].as_str())
            .unwrap_or_default()
            .into(),
        state: "interrupted".into(),
        message: Some(
            "原版本的团队材料导入未形成完整绑定；保留半导入记录核查，不自动重新导入".into(),
        ),
    })
}
pub(crate) fn unresolved(root: &Path) -> bool {
    view(root).is_some_and(|v| v.state != "completed")
}
pub(crate) fn begin(
    host: &mut RuntimeHost,
    id: &str,
    path: &str,
    sha: &str,
) -> Result<Importing, String> {
    require(crate::data_format::status(host.directory())?.minimum_reader_generation>=15,"后台团队导入需要新的实验保护；请创建新实验工作台，或停止核心、冷备份后审阅提升至reader15。原任务不要重派")?;
    require(
        view(host.directory()).is_none(),
        "已有原团队导入操作；请读取原记录，不重复导入",
    )?;
    require(!id.is_empty() && id.len() <= 256, "导入请求编号无效")?;
    require(
        host.can_shutdown(),
        "当前核心仍有运行或未决任务，不能导入材料",
    )?;
    host.prepare_team_import(path, sha)?;
    let root = host.directory().to_owned();
    let operation = Operation {
        schema: 1,
        root: root.to_string_lossy().into(),
        request_id: id.into(),
        source_path: path.into(),
        source_sha256: sha.to_ascii_lowercase(),
    };
    let overview = host.workspace_query(&cerulean_domain::workspace::WorkspaceQuery::Overview)?;
    publish(&root, OPERATION, &operation)?;
    let mut snapshot = host.snapshot();
    snapshot.team_import = Some(TeamImportView {
        request_id: id.into(),
        source_path: path.into(),
        source_sha256: sha.to_ascii_lowercase(),
        state: "running".into(),
        message: Some("正在核对并导入原材料；可关闭窗口，核心继续完成，不重复提交".into()),
    });
    Ok(Importing {
        root,
        snapshot,
        overview,
        thread: None,
        slot: Arc::new(Mutex::new(None)),
        operation,
    })
}
impl Importing {
    pub(crate) fn launch(&mut self, host: Box<RuntimeHost>) {
        *self.slot.lock().unwrap() = Some(host);
        let slot = self.slot.clone();
        let root = self.root.clone();
        let operation = self.operation.clone();
        let attempt = std::thread::Builder::new()
            .name("cerulean-team-import".into())
            .spawn(move || {
                let mut host = slot
                    .lock()
                    .unwrap()
                    .take()
                    .expect("one original import owner");
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    host.import_timed_team(&operation.source_path, &operation.source_sha256)?;
                    completed_binding(&host)
                }))
                .unwrap_or_else(|_| Err("后台导入异常，原记录已保留，不自动重试".into()));
                let outcome = Outcome {
                    operation_digest: digest(&operation).unwrap(),
                    completed: result.is_ok(),
                    message: result
                        .as_ref()
                        .err()
                        .map(|e| e.chars().take(1000).collect()),
                    binding: result.ok(),
                };
                // A missing/partial outcome remains interrupted and fences writes.
                let _ = publish(&root, OUTCOME, &outcome);
                host
            });
        match attempt {
            Ok(thread) => self.thread = Some(thread),
            Err(_) => {
                if let Ok(operation) = load::<Operation>(&self.root, OPERATION) {
                    let _ = publish(
                        &self.root,
                        OUTCOME,
                        &Outcome {
                            operation_digest: digest(&operation).unwrap(),
                            completed: false,
                            binding: None,
                            message: Some("导入后台未启动；原意图保留，未重复执行".into()),
                        },
                    );
                }
            }
        }
    }
    pub(crate) fn take_finished(&mut self) -> Option<Box<RuntimeHost>> {
        if self.thread.as_ref().is_some_and(|t| !t.is_finished()) {
            return None;
        }
        if let Some(thread) = self.thread.take() {
            return Some(
                thread
                    .join()
                    .expect("import catches operation panic while preserving owned Host"),
            );
        }
        self.slot.lock().unwrap().take()
    }
}

#[cfg(all(test, windows))]
mod fault_tests;
#[cfg(all(test, windows))]
pub(crate) mod partial_fault_tests;
#[cfg(all(test, windows))]
mod publication_fault_tests;
#[cfg(test)]
mod tests;
