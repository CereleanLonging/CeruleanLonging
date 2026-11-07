//! Original journal reads, without scheduler actions or full snapshots.
use crate::scheduler::DurableWorkflow;
use cerulean_domain::{execution::require, workspace::*};
use sha2::Digest;
struct Hash(sha2::Sha256);
impl std::io::Write for Hash {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
pub(crate) fn version(value: &impl serde::Serialize) -> Result<String, String> {
    let mut hash = Hash(sha2::Sha256::new());
    serde_json::to_writer(&mut hash, value).map_err(|e| e.to_string())?;
    Ok(format!("{:x}", hash.0.finalize()))
}
pub(crate) fn query(
    store: &DurableWorkflow,
    query: &WorkspaceQuery,
) -> Result<WorkspaceResult, String> {
    require(
        !store.requires_reopen(),
        "原历史写入尚未核对，请保留记录并重开",
    )?;
    let project = &store
        .configuration()
        .execution_scope
        .as_ref()
        .ok_or("原项目范围不存在")?
        .project;
    if let WorkspaceQuery::ProjectActivity {
        project_id,
        after,
        before,
    } = query
    {
        return crate::project_activity::query(store, project_id, after.as_ref(), before.as_ref());
    }
    let entries = store.history();
    let revision = version(&(
        "workspace-project-history-v1",
        project,
        store.configuration(),
        store.state(),
        store.revision(),
        store.approval(),
        entries,
    ))?;
    let id = match query {
        WorkspaceQuery::History { project_id, .. }
        | WorkspaceQuery::HistoryEntry { project_id, .. } => {
            project_id.as_deref().ok_or("项目历史缺少原项目身份")?
        }
        _ => return Err("历史查询类型不符".into()),
    };
    require(id == project.id, "原项目历史身份不一致")?;
    let result = match query {
        WorkspaceQuery::History {
            offset,
            revision: expected,
            ..
        } => {
            let summaries = entries.iter().map(|entry| WorkspaceHistorySummary {
                sequence: entry.body.event.sequence,
                at: entry.body.received_at.clone(),
                kind: "workflow".into(),
                principal: Some(entry.body.event.principal.clone()),
            });
            let (summaries, next_offset) = crate::workspace_read::page(
                summaries,
                entries.len(),
                *offset,
                expected.as_deref(),
                &revision,
            )?;
            WorkspaceResult::History(WorkspaceHistoryPage {
                project_id: Some(id.into()),
                revision,
                offset: *offset,
                total: entries.len() as u32,
                next_offset,
                entries: summaries,
            })
        }
        WorkspaceQuery::HistoryEntry {
            sequence,
            revision: expected,
            ..
        } => {
            cerulean_domain::execution::sha256(expected)?;
            require(expected == &revision, "历史记录已变化，请刷新后读取原详情")?;
            let entry = entries
                .iter()
                .find(|entry| entry.body.event.sequence == *sequence)
                .ok_or("原历史记录不存在")?;
            let original_json = serde_json::to_string(entry).map_err(|e| e.to_string())?;
            WorkspaceResult::HistoryEntry(Box::new(WorkspaceHistoryDetail {
                project_id: Some(id.into()),
                revision,
                sequence: *sequence,
                original_json,
            }))
        }
        _ => return Err("历史查询类型不符".into()),
    };
    crate::workspace_read::bounded(result)
}
