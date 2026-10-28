//! Resource identity derived from approved execution, never a model-supplied lock.
use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ResourceKey {
    Named(String),
    Directory {
        path: String,
        write: bool,
    },
    DesktopWindow {
        pid: u32,
        start_ticks: u64,
        hwnd: u64,
    },
    DesktopSession {
        adapter: String,
        session: String,
    },
}
impl ResourceKey {
    pub(super) fn conflicts(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Directory { path: a, write: aw }, Self::Directory { path: b, write: bw }) => {
                (*aw || *bw)
                    && (crate::artifacts::contains_path(Path::new(a), Path::new(b))
                        || crate::artifacts::contains_path(Path::new(b), Path::new(a)))
            }
            _ => self == other,
        }
    }
    pub(super) fn label(&self) -> String {
        match self {
            Self::Named(name) => name.clone(),
            Self::Directory { path, write } => {
                format!("{}目录 {}", if *write { "写入" } else { "读取" }, path)
            }
            Self::DesktopWindow { pid, hwnd, .. } => format!("桌面窗口 {pid}/{hwnd}"),
            Self::DesktopSession { adapter, session } => format!("桌面会话 {adapter}/{session}"),
        }
    }
}

pub(super) fn task_resources(
    plan: &PlanRevision,
    task: &str,
    attempt: &str,
) -> Result<Vec<ResourceKey>, String> {
    let definition = plan
        .workflow
        .tasks
        .iter()
        .find(|t| t.id == task)
        .ok_or("资源占用缺少原任务")?;
    let spec = plan.task(task)?;
    let mut keys: Vec<_> = definition
        .resource_keys
        .as_deref()
        .unwrap_or_default()
        .iter()
        .cloned()
        .map(ResourceKey::Named)
        .collect();
    keys.push(ResourceKey::Directory {
        path: plan.execution_root(task, attempt)?,
        write: true,
    });
    if plan.worktree.is_some() {
        // Multiple worktrees can share the approved input repository. A worker
        // writing that source directory cannot overlap them. Git's own metadata
        // transaction lock remains the preparation authority, not a lifetime lock.
        keys.push(ResourceKey::Directory {
            path: plan.project.root.clone(),
            write: false,
        });
    }
    if let Some(desktop) = &spec.desktop_worker {
        let window = &desktop.binding.observation.window;
        keys.push(ResourceKey::DesktopWindow {
            pid: window.pid,
            start_ticks: window.start_ticks.parse().map_err(|_| "桌面进程时间无效")?,
            hwnd: window.hwnd.parse().map_err(|_| "桌面窗口句柄无效")?,
        });
        if let Some(session) = &desktop.binding.observation.session_id {
            keys.push(ResourceKey::DesktopSession {
                adapter: desktop.adapter_id().into(),
                session: session.to_ascii_lowercase(),
            });
        }
    }
    Ok(keys)
}
