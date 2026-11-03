//! Bounded read-only projections. No scheduler tick, repair, or full Snapshot.
use crate::Store;
use cerulean_domain::{
    execution::{digest, require, sha256},
    workspace::*,
    PRODUCT_NAME, PROTOCOL_VERSION,
};

const PAGE_ENTRIES: usize = 20;
const PAGE_BYTES: usize = 64 * 1024;
pub(crate) const RESULT_BYTES: usize = 512 * 1024;

pub(crate) fn page<T: serde::Serialize>(
    items: impl IntoIterator<Item = T>,
    total: usize,
    offset: u32,
    expected: Option<&str>,
    revision: &str,
) -> Result<(Vec<T>, Option<u32>), String> {
    page_window(
        items.into_iter().skip(offset as usize),
        total,
        offset,
        expected,
        revision,
    )
}
fn page_window<T: serde::Serialize>(
    items: impl IntoIterator<Item = T>,
    total: usize,
    offset: u32,
    expected: Option<&str>,
    revision: &str,
) -> Result<(Vec<T>, Option<u32>), String> {
    require(
        offset == 0 || expected.is_some(),
        "工作区续页需要原版本，请刷新首页",
    )?;
    if let Some(expected) = expected {
        sha256(expected)?;
        require(expected == revision, "工作区记录已变化，请刷新首页")?;
    }
    require(
        (offset as usize) <= total && total <= u32::MAX as usize,
        "工作区页偏移或总数超出范围",
    )?;
    let mut bytes = 1024;
    let mut entries = Vec::new();
    for item in items.into_iter().take(PAGE_ENTRIES) {
        let size = serde_json::to_vec(&item).map_err(|e| e.to_string())?.len() + 1;
        if bytes + size > PAGE_BYTES {
            require(!entries.is_empty(), "单条工作区摘要过大，请保留原记录")?;
            break;
        }
        bytes += size;
        entries.push(item);
    }
    let end = offset as usize + entries.len();
    require(
        end > offset as usize || end == total,
        "工作区分页未取得原记录，不能重复同一游标",
    )?;
    Ok((entries, (end < total).then_some(end as u32)))
}
pub(crate) fn bounded(result: WorkspaceResult) -> Result<WorkspaceResult, String> {
    // User-authored documents use negotiated chunk transport; evidence stays bounded.
    if matches!(
        result,
        WorkspaceResult::Plan(_) | WorkspaceResult::Configuration(_)
    ) {
        return Ok(result);
    }
    require(
        serde_json::to_vec(&result)
            .map_err(|e| e.to_string())?
            .len()
            <= RESULT_BYTES,
        "单条工作区详情超过返回上限；原记录已保留",
    )?;
    Ok(result)
}
impl Store {
    pub(crate) fn workspace_history(
        &self,
        query: &WorkspaceQuery,
    ) -> Result<WorkspaceResult, String> {
        require(!self.poisoned, "原历史写入尚未核对，请保留记录并重开")?;
        let revision = crate::workspace_history::version(&(
            "workspace-history-v1",
            &self.last_hash,
            &self.events,
        ))?;
        let result = match query {
            WorkspaceQuery::History {
                project_id,
                offset,
                revision: expected,
            } => {
                require(project_id.is_none(), "项目执行历史需要项目专用账本")?;
                let selected = self
                    .index
                    .summaries(
                        &self.root,
                        &self.index_head(),
                        *offset as usize,
                        PAGE_ENTRIES,
                    )
                    .unwrap_or_else(|_| {
                        self.events
                            .iter()
                            .skip(*offset as usize)
                            .take(PAGE_ENTRIES)
                            .cloned()
                            .collect()
                    });
                let entries = selected.iter().map(|event| WorkspaceHistorySummary {
                    sequence: event.sequence,
                    at: event.at.clone(),
                    kind: event.kind.clone(),
                    principal: None,
                });
                let (entries, next_offset) = page_window(
                    entries,
                    self.events.len(),
                    *offset,
                    expected.as_deref(),
                    &revision,
                )?;
                WorkspaceResult::History(WorkspaceHistoryPage {
                    project_id: None,
                    revision,
                    offset: *offset,
                    total: self.events.len() as u32,
                    next_offset,
                    entries,
                })
            }
            WorkspaceQuery::HistoryEntry {
                project_id,
                sequence,
                revision: expected,
            } => {
                require(project_id.is_none(), "项目执行历史需要项目专用账本")?;
                sha256(expected)?;
                require(expected == &revision, "历史记录已变化，请刷新后读取原详情")?;
                let event = self
                    .events
                    .iter()
                    .find(|event| event.sequence == *sequence)
                    .ok_or("原历史记录不存在")?;
                let original_json = serde_json::to_string(event).map_err(|e| e.to_string())?;
                WorkspaceResult::HistoryEntry(Box::new(WorkspaceHistoryDetail {
                    project_id: None,
                    revision,
                    sequence: *sequence,
                    original_json,
                }))
            }
            _ => return Err("历史查询类型不符".into()),
        };
        bounded(result)
    }
}
impl Store {
    pub(crate) fn workspace_overview(
        &self,
        mode: WorkspaceMode,
        message: Option<String>,
        project_count: Option<u32>,
    ) -> WorkspaceOverview {
        WorkspaceOverview {
            product: PRODUCT_NAME.into(),
            protocol_version: PROTOCOL_VERSION,
            data_directory: self.root.display().to_string(),
            mode,
            message: message.or_else(|| self.index.error().map(|_| {
                "查询索引暂不可用，当前从已核验账本读取；任务记录已保留，重开后会重新构建索引。".into()
            })),
            legacy_plan_count: (!self.poisoned).then_some(self.plans.len() as u32),
            pending_legacy_plan_count: (!self.poisoned)
                .then(|| self.plans.iter().filter(|p| !p.approved).count() as u32),
            project_count,
        }
    }
    pub(crate) fn workspace_plans(
        &self,
        query: &WorkspaceQuery,
    ) -> Result<WorkspaceResult, String> {
        require(!self.poisoned, "原计划写入尚未核对，请保留记录并重开")?;
        let revision = digest(&("workspace-plans-v1", &self.last_hash))?;
        let result = match query {
            WorkspaceQuery::Plans {
                offset,
                revision: expected,
            } => {
                let selected = self
                    .index
                    .plans(
                        &self.root,
                        &self.index_head(),
                        *offset as usize,
                        PAGE_ENTRIES,
                    )
                    .unwrap_or_else(|_| {
                        self.plans
                            .iter()
                            .skip(*offset as usize)
                            .take(PAGE_ENTRIES)
                            .cloned()
                            .collect()
                    });
                let summaries = selected.iter().map(|p| PlanSummary {
                    id: p.id.clone(),
                    revision: p.revision,
                    title: p.title.clone(),
                    digest: p.digest.clone(),
                    created_at: p.created_at.clone(),
                    approved: p.approved,
                    task_count: p.tasks.len() as u32,
                });
                let (entries, next_offset) = page_window(
                    summaries,
                    self.plans.len(),
                    *offset,
                    expected.as_deref(),
                    &revision,
                )?;
                WorkspaceResult::Plans(PlanPage {
                    revision,
                    offset: *offset,
                    total: self.plans.len() as u32,
                    next_offset,
                    entries,
                })
            }
            WorkspaceQuery::Plan {
                id,
                revision: expected,
            } => {
                sha256(expected)?;
                require(expected == &revision, "计划列表已变化，请刷新后读取原详情")?;
                let plan = self
                    .plans
                    .iter()
                    .find(|p| &p.id == id)
                    .ok_or("原计划不存在")?;
                WorkspaceResult::Plan(plan.clone())
            }
            _ => return Err("计划查询类型不符".into()),
        };
        bounded(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader_generation::tests::snapshot;
    use cerulean_domain::Command;
    #[test]
    fn workspace_history_pages_details_versions_and_readonly() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::open(temp.path()).unwrap();
        for n in 0..45 {
            store
                .execute(Command::CreatePlan {
                    title: format!("history {n}"),
                    tasks: vec!["task".into()],
                })
                .unwrap();
        }
        store
            .wait_for_index(std::time::Duration::from_secs(20))
            .unwrap();
        let before = snapshot(temp.path());
        let first_query = WorkspaceQuery::History {
            project_id: None,
            offset: 0,
            revision: None,
        };
        let WorkspaceResult::History(first) = store.workspace_history(&first_query).unwrap() else {
            panic!("history")
        };
        assert_eq!(first.next_offset, Some(20));
        let mut seen = first.entries.iter().map(|e| e.sequence).collect::<Vec<_>>();
        let mut next = first.next_offset;
        while let Some(offset) = next {
            let result = store
                .workspace_history(&WorkspaceQuery::History {
                    project_id: None,
                    offset,
                    revision: Some(first.revision.clone()),
                })
                .unwrap();
            assert!(serde_json::to_vec(&result).unwrap().len() <= PAGE_BYTES);
            let WorkspaceResult::History(page) = result else {
                panic!("page")
            };
            seen.extend(page.entries.iter().map(|e| e.sequence));
            next = page.next_offset;
        }
        assert_eq!(
            seen,
            store.events.iter().map(|e| e.sequence).collect::<Vec<_>>()
        );
        let detail = WorkspaceQuery::HistoryEntry {
            project_id: None,
            sequence: seen[21],
            revision: first.revision.clone(),
        };
        let WorkspaceResult::HistoryEntry(found) = store.workspace_history(&detail).unwrap() else {
            panic!("detail")
        };
        assert_eq!(
            found.original_json,
            serde_json::to_string(&store.events[21]).unwrap()
        );
        assert!(store
            .workspace_history(&WorkspaceQuery::History {
                project_id: None,
                offset: 20,
                revision: None
            })
            .is_err());
        assert!(store
            .workspace_history(&WorkspaceQuery::History {
                project_id: None,
                offset: 46,
                revision: Some(first.revision.clone())
            })
            .is_err());
        assert_eq!(before, snapshot(temp.path()));
        drop(store);
        let mut store = Store::open(temp.path()).unwrap();
        store
            .wait_for_index(std::time::Duration::from_secs(20))
            .unwrap();
        assert!(store.workspace_history(&detail).is_ok());
        store.events[21].summary.push_str("changed same sequence");
        assert!(store.workspace_history(&detail).is_err());
        store.poisoned = true;
        assert!(store.workspace_history(&first_query).is_err());
        assert_eq!(before, snapshot(temp.path()));
    }
    #[test]
    fn workspace_history_boundary_encoded_detail_and_summary_refusal() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::open(temp.path()).unwrap();
        store
            .execute(Command::CreatePlan {
                title: "boundary".into(),
                tasks: vec!["task".into()],
            })
            .unwrap();
        // Controlled in-memory oversize probe; not a valid persisted event.
        store.events[0].summary = "\u{0001}".repeat(256 * 1024);
        // Publish only the test cache, then bracket the read-only queries.
        // The real event remains unchanged and a normal reopen rejects this cache.
        let event_path = temp.path().join("events/00000001.json");
        let event_before = std::fs::read(&event_path).unwrap();
        store.refresh_index();
        store
            .wait_for_index(std::time::Duration::from_secs(20))
            .unwrap();
        let before = snapshot(temp.path());
        let query = WorkspaceQuery::History {
            project_id: None,
            offset: 0,
            revision: None,
        };
        let WorkspaceResult::History(page) = store.workspace_history(&query).unwrap() else {
            panic!("page")
        };
        assert!(serde_json::to_vec(&page).unwrap().len() < PAGE_BYTES);
        assert!(store
            .workspace_history(&WorkspaceQuery::HistoryEntry {
                project_id: None,
                sequence: page.entries[0].sequence,
                revision: page.revision
            })
            .is_err());
        assert_eq!(before, snapshot(temp.path()));
        store.events[0].kind = "x".repeat(PAGE_BYTES);
        store.refresh_index();
        store
            .wait_for_index(std::time::Duration::from_secs(20))
            .unwrap();
        let before = snapshot(temp.path());
        assert!(store.workspace_history(&query).is_err());
        assert_eq!(before, snapshot(temp.path()));
        assert_eq!(std::fs::read(event_path).unwrap(), event_before);
    }

    #[test]
    fn workspace_pages_preserve_all_plans_details_and_restart_revision() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::open(temp.path()).unwrap();
        for n in 0..45 {
            store
                .execute(Command::CreatePlan {
                    title: format!("plan {n}"),
                    tasks: vec!["x".repeat(2000); 30],
                })
                .unwrap();
        }
        store
            .wait_for_index(std::time::Duration::from_secs(20))
            .unwrap();
        let original = snapshot(temp.path());
        let mut offset = 0;
        let mut revision = None;
        let mut found = Vec::new();
        loop {
            let result = store
                .workspace_plans(&WorkspaceQuery::Plans {
                    offset,
                    revision: revision.clone(),
                })
                .unwrap();
            assert!(serde_json::to_vec(&result).unwrap().len() <= PAGE_BYTES);
            let WorkspaceResult::Plans(page) = result else {
                panic!("page")
            };
            assert_eq!(page.total, 45);
            revision = Some(page.revision.clone());
            found.extend(page.entries);
            if let Some(next) = page.next_offset {
                offset = next;
            } else {
                break;
            }
        }
        assert_eq!(found.len(), 45);
        assert_eq!(
            found
                .iter()
                .map(|s| &s.id)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            45
        );
        let query = WorkspaceQuery::Plan {
            id: found[21].id.clone(),
            revision: revision.clone().unwrap(),
        };
        let WorkspaceResult::Plan(detail) = store.workspace_plans(&query).unwrap() else {
            panic!("detail")
        };
        assert_eq!(&detail, &store.plans[21]);
        assert!(snapshot(temp.path()) == original);
        drop(store);
        let mut store = Store::open(temp.path()).unwrap();
        store
            .wait_for_index(std::time::Duration::from_secs(20))
            .unwrap();
        assert!(store.workspace_plans(&query).is_ok());
        assert!(snapshot(temp.path()) == original);
        store
            .execute(Command::ApprovePlan {
                id: detail.id,
                digest: detail.digest,
            })
            .unwrap();
        assert!(store.workspace_plans(&query).is_err());
        assert!(store
            .workspace_plans(&WorkspaceQuery::Plans {
                offset: 20,
                revision
            })
            .is_err());
        let overview = store.workspace_overview(WorkspaceMode::Ready, None, Some(0));
        assert_eq!(overview.pending_legacy_plan_count, Some(44));
    }
    #[test]
    fn workspace_pages_bound_escaped_bytes_and_reject_invalid_cursors() {
        let entries = vec!["\u{1}".repeat(2000); 20];
        let revision = "a".repeat(64);
        let (first, next) = page(entries.clone(), 20, 0, None, &revision).unwrap();
        assert!(first.len() < 20 && next.is_some());
        assert!(serde_json::to_vec(&first).unwrap().len() < PAGE_BYTES);
        for (offset, expected) in [(1, None), (21, Some(revision.as_str())), (0, Some("wrong"))] {
            assert!(page(entries.clone(), 20, offset, expected, &revision).is_err());
        }
        assert!(page(vec!["x".repeat(PAGE_BYTES)], 1, 0, None, &revision).is_err());
    }
    #[test]
    fn workspace_pages_unknown_write_and_wrong_query_are_not_empty_success() {
        let temp = tempfile::tempdir().unwrap();
        let mut store = Store::open(temp.path()).unwrap();
        assert!(store.workspace_plans(&WorkspaceQuery::Overview).is_err());
        store.poisoned = true;
        let overview = store.workspace_overview(WorkspaceMode::ReviewRequired, None, None);
        assert_eq!(overview.legacy_plan_count, None);
        assert_eq!(overview.pending_legacy_plan_count, None);
        assert!(store
            .workspace_plans(&WorkspaceQuery::Plans {
                offset: 0,
                revision: None
            })
            .is_err());
        assert!(serde_json::from_value::<WorkspaceQuery>(
            serde_json::json!({"type":"createPlan","title":"not a read","tasks":["no"]})
        )
        .is_err());
    }
}
