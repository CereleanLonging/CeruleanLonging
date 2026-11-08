//! Persistent five-slot registry. New identities create directories; switching
//! back reuses the original identity. No worker controls a registry write.
use crate::{artifacts::root_path, host::ordinary};
use cerulean_domain::execution::*;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
};

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Registration {
    schema_version: u32,
    project_id: String,
    shared_root: String,
    binding: WorkerDirectory,
}
pub(crate) fn verify_saved(
    data: &Path,
    project: &Project,
    saved: &BTreeMap<String, WorkerDirectory>,
) -> Result<(), String> {
    identifier(&project.id)?;
    require(
        !saved.is_empty() && saved.len() <= 5,
        "执行目录登记数量无效",
    )?;
    let records = data.join("worker-directories").join(&project.id);
    ordinary(&records, true)?;
    require(
        fs::read_dir(&records).map_err(|e| e.to_string())?.count() == saved.len(),
        "执行目录登记存在未完成或额外事务",
    )?;
    let parent = root_path(&project.root)?.join("executors");
    ordinary(&parent, true)?;
    require(
        fs::read_dir(&parent).map_err(|e| e.to_string())?.count() == saved.len(),
        "执行目录与登记数量不符，请核对",
    )?;
    for (key, binding) in saved {
        sha256(key)?;
        let file = records.join(format!("{key}.json"));
        ordinary(&file, false)?;
        let mut bytes = vec![];
        fs::File::open(file)
            .map_err(|e| e.to_string())?
            .take(8193)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        require(bytes.len() <= 8192, "执行目录登记过大")?;
        let record: Registration =
            serde_json::from_slice(&bytes).map_err(|_| "执行目录登记损坏")?;
        require(
            record.schema_version == 1
                && record.project_id == project.id
                && record.shared_root == project.root
                && record.binding == *binding,
            "已保存配置与持久目录登记不符",
        )?;
        root_path(&binding.root(&project.root)?)?;
    }
    Ok(())
}

pub(crate) fn register(
    data: &Path,
    project: &Project,
    tasks: &[TaskExecution],
) -> Result<BTreeMap<String, WorkerDirectory>, String> {
    identifier(&project.id)?;
    let base = data.join("worker-directories");
    if !base.exists() {
        fs::create_dir(&base).map_err(|e| e.to_string())?;
    }
    ordinary(&base, true)?;
    let records = base.join(&project.id);
    if !records.exists() {
        fs::create_dir(&records).map_err(|e| e.to_string())?;
    }
    ordinary(&records, true)?;
    let shared = root_path(&project.root)?;
    let mut saved = BTreeMap::new();
    for item in fs::read_dir(&records).map_err(|e| e.to_string())? {
        let path = item.map_err(|e| e.to_string())?.path();
        ordinary(&path, false)?;
        let mut bytes = vec![];
        fs::File::open(&path)
            .map_err(|e| e.to_string())?
            .take(8193)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        require(bytes.len() <= 8192, "worker registry record too large")?;
        let record: Registration = serde_json::from_slice(&bytes)
            .map_err(|_| "执行目录登记不完整，请核对；不会另建目录")?;
        sha256(&record.binding.identity)?;
        require(
            record.schema_version == 1
                && record.project_id == project.id
                && record.shared_root == project.root
                && path.file_name().and_then(|s| s.to_str())
                    == Some(format!("{}.json", record.binding.identity).as_str())
                && record.binding.relative_path == format!("executors/{}", record.binding.identity),
            "执行目录登记身份不一致",
        )?;
        root_path(&record.binding.root(&project.root)?)?;
        require(
            saved
                .insert(record.binding.identity.clone(), record.binding)
                .is_none(),
            "duplicate worker registration",
        )?;
    }
    let wanted = tasks
        .iter()
        .map(WorkerDirectory::for_task)
        .collect::<Result<Vec<_>, _>>()?;
    let all = saved
        .keys()
        .cloned()
        .chain(wanted.iter().map(|d| d.identity.clone()))
        .collect::<std::collections::BTreeSet<_>>();
    require(
        all.len() <= 5,
        "执行模型工作目录已达五个上限；切换已有模型可复用，不会创建第六个目录",
    )?;
    let parent = shared.join("executors");
    if !parent.exists() {
        fs::create_dir(&parent).map_err(|e| e.to_string())?;
    }
    ordinary(&parent, true)?;
    // A failed earlier transaction or an externally added directory must not
    // disappear from the slot count. Never silently adopt or delete it.
    for entry in fs::read_dir(&parent).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path();
        ordinary(&path, true)?;
        require(
            path.file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|id| saved.contains_key(id)),
            "发现未登记的执行目录，请核对原创建事务；不会继续分配目录",
        )?;
    }
    for binding in wanted {
        if saved.contains_key(&binding.identity) {
            continue;
        }
        let dir = parent.join(&binding.identity);
        // An unregistered pre-existing directory is never adopted silently.
        require(!dir.exists(), "存在未登记的执行目录，请先核对原创建事务")?;
        let record = Registration {
            schema_version: 1,
            project_id: project.id.clone(),
            shared_root: project.root.clone(),
            binding: binding.clone(),
        };
        let file = records.join(format!("{}.json", binding.identity));
        let pending = file.with_extension("pending");
        let bytes = serde_json::to_vec(&record).map_err(|e| e.to_string())?;
        let mut writer = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&pending)
            .map_err(|e| e.to_string())?;
        writer
            .write_all(&bytes)
            .and_then(|_| writer.sync_all())
            .map_err(|e| e.to_string())?;
        drop(writer);
        // Durable intent precedes allocation. An interrupted operation remains
        // visible as .pending and requires reconciliation, never auto-retry.
        fs::create_dir(&dir).map_err(|e| e.to_string())?;
        ordinary(&dir, true)?;
        fs::rename(pending, file).map_err(|e| e.to_string())?;
        saved.insert(binding.identity.clone(), binding);
    }
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn task(model: &str) -> TaskExecution {
        TaskExecution {
            task_id: "task".into(),
            adapter_id: "deepseek".into(),
            worker: None,
            api_worker: Some(cerulean_domain::api_worker::ApiWorkerSelection {
                reasoning_effort: None,
                target: cerulean_domain::api_worker::ApiTarget::Provider {
                    endpoint: cerulean_domain::credentials::ProviderEndpoint::DeepSeek {},
                },
                model: model.into(),
                max_output_tokens: 256,
                request_timeout_seconds: 30,
                file_tools: vec![ToolPermission::ReadFiles, ToolPermission::WriteFiles],
                write_scope: None,
                max_rounds: 1,
                max_tool_calls: 1,
                max_total_tokens: 1024,
            }),
            desktop_worker: None,
            artifact_paths: vec![],
            checks: vec![],
            profile_id: None,
            supervision_override: None,
            worker_directory: None,
        }
    }
    fn fixture() -> (tempfile::TempDir, std::path::PathBuf, Project) {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let shared = temp.path().join("shared");
        fs::create_dir(&data).unwrap();
        fs::create_dir(&shared).unwrap();
        let project = Project {
            id: "project".into(),
            root: shared.to_string_lossy().into(),
            timezone: "Asia/Shanghai".into(),
        };
        (temp, data, project)
    }
    #[test]
    fn five_models_reuse_persistent_directories_and_sixth_allocates_nothing() {
        let (_temp, data, p) = fixture();
        let one = register(&data, &p, &[task("a"), task("a")]).unwrap();
        assert_eq!(one.len(), 1);
        let binding = one.values().next().unwrap();
        let artifact = Path::new(&binding.root(&p.root).unwrap()).join("result.txt");
        fs::write(&artifact, "keep").unwrap();
        let five = register(&data, &p, &[task("b"), task("c"), task("d"), task("e")]).unwrap();
        assert_eq!(five.len(), 5);
        assert_eq!(register(&data, &p, &[task("a")]).unwrap(), five);
        assert_eq!(fs::read_to_string(artifact).unwrap(), "keep");
        assert!(register(&data, &p, &[task("sixth")]).is_err());
        assert_eq!(
            fs::read_dir(Path::new(&p.root).join("executors"))
                .unwrap()
                .count(),
            5
        );
    }
    #[test]
    fn orphan_or_tampered_record_blocks_new_allocation() {
        let (_temp, data, p) = fixture();
        let first = register(&data, &p, &[task("a")]).unwrap();
        let parent = Path::new(&p.root).join("executors");
        fs::create_dir(parent.join("orphan")).unwrap();
        assert!(register(&data, &p, &[task("b")]).is_err());
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 2);
        fs::remove_dir(parent.join("orphan")).unwrap();
        let record = data
            .join("worker-directories/project")
            .join(format!("{}.json", first.keys().next().unwrap()));
        fs::write(record, "{}").unwrap();
        assert!(register(&data, &p, &[task("b")]).is_err());
        assert_eq!(fs::read_dir(&parent).unwrap().count(), 1);
    }
}
